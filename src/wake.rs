//! Sleeping and waking: the [`Wake`] trait a ring calls when one
//! side must wait for the other, and its two implementations.
//!
//! - The crate owns the protocol: which word a side sleeps on, the
//!   flag or count that says it sleeps, and the re-check after the
//!   flag, so a wake is never lost. A [`Wake`] implementation owns
//!   only the sleep and the wake, which are the platform's.
//! - [`NoWake`] spins and wakes nothing, so a ring over it compiles
//!   every wake check out, and its waits are polls.
//! - [`Futex`], on Linux, sleeps in the kernel on the word itself,
//!   shared between processes, with a timeout, since a peer can die
//!   while another sleeps.
//! - Every process attached to one ring uses the same
//!   implementation. A mismatch is not detected and costs latency
//!   only: a futex sleeper no one wakes returns at its timeout.
//! - `MpscRing` in `mpsc::v3` is the ring that takes one.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::Micros;

/// A word and the value it was last seen holding, before the
/// caller's last look at what it waits for: a wait sleeps only
/// while the word still holds it.
///
/// - The guard against a lost wake: the waker changes the word
///   before it wakes, so a change between the look and the sleep
///   makes the sleep return at once, the kernel comparing and
///   sleeping as one step.
/// - Taken by [`load`](Seen::load), or by
///   [`written`](Seen::written) when the caller's own
///   read-modify-write put the value there.
pub struct Seen<'a> {
    word: &'a AtomicU32,
    value: u32,
}

impl<'a> Seen<'a> {
    /// The word as a load finds it, `SeqCst`, before the caller's
    /// look.
    pub fn load(word: &'a AtomicU32) -> Self {
        Seen {
            word,
            value: word.load(Ordering::SeqCst),
        }
    }

    /// The word as the caller's own read-modify-write left it, its
    /// `value` the one written.
    pub fn written(word: &'a AtomicU32, value: u32) -> Self {
        Seen { word, value }
    }

    /// The word a wait sleeps on.
    pub fn word(&self) -> &'a AtomicU32 {
        self.word
    }

    /// The value the word was seen holding.
    pub fn value(&self) -> u32 {
        self.value
    }
}

/// The platform's sleep and wake, called by a ring's waiting
/// paths.
pub trait Wake {
    /// Whether the waking side must look for sleepers: `false`
    /// compiles every wake check out of the message paths.
    const WAKES: bool;

    /// Sleep while the word still holds the value `seen` saw.
    ///
    /// - Returns on a wake, when the word no longer holds
    ///   `expected`, spuriously, or at the implementation's own
    ///   timeout, and the caller looks again whichever it was.
    fn wait(seen: Seen<'_>);

    /// Sleep while the word still holds the value `seen` saw, for at
    /// most `timeout`, the caller's bound in place of the
    /// implementation's own timeout.
    ///
    /// - Returns as [`wait`](Wake::wait) does, and at `timeout`, so a
    ///   caller with a deadline sleeps on what is left of it.
    fn wait_for(seen: Seen<'_>, timeout: Micros);

    /// Wake every sleeper on `word`.
    fn wake(word: &AtomicU32);
}

/// No sleeping: a wait is one spin-loop hint and a wake does
/// nothing, so a waiting call polls.
pub struct NoWake;

impl Wake for NoWake {
    const WAKES: bool = false;

    fn wait(_seen: Seen<'_>) {
        core::hint::spin_loop();
    }

    fn wait_for(_seen: Seen<'_>, _timeout: Micros) {
        core::hint::spin_loop();
    }

    fn wake(_word: &AtomicU32) {}
}

/// A Linux futex on the word itself, shared between processes,
/// the sleep bounded by `TIMEOUT_MS` milliseconds.
///
/// - Not `FUTEX_PRIVATE_FLAG`: the word is in memory other
///   processes map, and the kernel keys the sleep by the page.
/// - The timeout is what turns a dead peer into a poll: a sleeper
///   no one wakes looks again every `TIMEOUT_MS`.
#[cfg(target_os = "linux")]
pub struct Futex<const TIMEOUT_MS: u32 = 10>;

#[cfg(target_os = "linux")]
impl<const TIMEOUT_MS: u32> Wake for Futex<TIMEOUT_MS> {
    const WAKES: bool = true;

    fn wait(seen: Seen<'_>) {
        futex_wait(
            seen,
            libc::timespec {
                tv_sec: (TIMEOUT_MS / 1000) as libc::time_t,
                tv_nsec: ((TIMEOUT_MS % 1000) * 1_000_000) as libc::c_long,
            },
        );
    }

    fn wait_for(seen: Seen<'_>, timeout: Micros) {
        let Micros(us) = timeout;
        futex_wait(
            seen,
            libc::timespec {
                tv_sec: (us / 1_000_000) as libc::time_t,
                tv_nsec: ((us % 1_000_000) * 1_000) as libc::c_long,
            },
        );
    }

    fn wake(word: &AtomicU32) {
        // SAFETY: word is a live, aligned u32, and FUTEX_WAKE only
        // wakes sleepers keyed by its address, reading nothing.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                word.as_ptr(),
                libc::FUTEX_WAKE,
                i32::MAX,
                core::ptr::null::<libc::timespec>(),
                core::ptr::null::<u32>(),
                0u32,
            );
        }
    }
}

/// One `FUTEX_WAIT` on the word while it holds the value `seen`
/// saw, bounded by the relative `timeout`.
#[cfg(target_os = "linux")]
fn futex_wait(seen: Seen<'_>, timeout: libc::timespec) {
    // SAFETY: the word is a live, aligned u32 for the call, which the
    // kernel reads atomically against the seen value, and the timeout
    // is a valid relative timespec. An error return (EAGAIN when the word
    // moved, ETIMEDOUT, EINTR) is a return, and the caller looks
    // again.
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            seen.word().as_ptr(),
            libc::FUTEX_WAIT,
            seen.value(),
            &timeout as *const libc::timespec,
            core::ptr::null::<u32>(),
            0u32,
        );
    }
}
