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

use crate::Deadline;

/// `struct Seen` is a word and the value the word was last seen holding, read before the caller's
/// last look at what it waits for. A wait sleeps only while the word still holds that value.
///
/// - The value guards against a lost wake: the waker changes the word before it wakes, so a change
///   between the caller's look and its sleep makes the sleep return at once. The kernel compares
///   and sleeps as one step.
/// - A caller makes a `Seen` with [`Seen::load`], or with [`Seen::written`] when its own
///   read-modify-write put the value in the word.
pub struct Seen<'a> {
    word: &'a AtomicU32,
    value: u32,
}

impl<'a> Seen<'a> {
    /// `Seen::load` reads the word with a `SeqCst` load, before the caller's look.
    pub fn load(word: &'a AtomicU32) -> Self {
        Seen {
            word,
            value: word.load(Ordering::SeqCst),
        }
    }

    /// `Seen::written` records `value` as the word's value, when the caller's own read-modify-write
    /// wrote it there.
    pub fn written(word: &'a AtomicU32, value: u32) -> Self {
        Seen { word, value }
    }

    /// `Seen::word` returns the word a wait sleeps on.
    pub fn word(&self) -> &'a AtomicU32 {
        self.word
    }

    /// `Seen::value` returns the value the word was seen holding.
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

    /// `wait` sleeps while the word in `seen` still holds the value `seen` recorded.
    ///
    /// - `wait` returns on a wake, when the word no longer holds that value, spuriously, or at the
    ///   implementation's own timeout, and the caller looks again whichever it was.
    fn wait(seen: Seen<'_>);

    /// `wait_until` sleeps while the word in `seen` still holds the value `seen` recorded, until
    /// `deadline` at the latest. The caller's deadline replaces the implementation's own timeout.
    ///
    /// - `wait_until` returns as [`wait`](Wake::wait) does, and at `deadline`. A sleep that returns
    ///   early sleeps again to the same deadline.
    fn wait_until(seen: Seen<'_>, deadline: Deadline);

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

    fn wait_until(_seen: Seen<'_>, _deadline: Deadline) {
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
        // FUTEX_WAIT's timeout is relative.
        let timeout = libc::timespec {
            tv_sec: (TIMEOUT_MS / 1000) as libc::time_t,
            tv_nsec: ((TIMEOUT_MS % 1000) * 1_000_000) as libc::c_long,
        };
        futex_wait(seen, libc::FUTEX_WAIT, &timeout);
    }

    fn wait_until(seen: Seen<'_>, deadline: Deadline) {
        // FUTEX_WAIT_BITSET's timeout is absolute, on CLOCK_MONOTONIC, the clock a Deadline is read
        // on, so no remaining time is computed. The divisors are constants, so the split into
        // seconds and nanoseconds costs multiplies, paid once before a sleep.
        let ns = deadline.monotonic_nanos();
        let at = libc::timespec {
            tv_sec: (ns / 1_000_000_000) as libc::time_t,
            tv_nsec: (ns % 1_000_000_000) as libc::c_long,
        };
        futex_wait(seen, libc::FUTEX_WAIT_BITSET, &at);
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

/// `futex_wait` sleeps once on the word in `seen` while the word holds the value `seen` recorded.
/// `op` is `FUTEX_WAIT`, with `timeout` relative, or `FUTEX_WAIT_BITSET`, with `timeout` absolute
/// on `CLOCK_MONOTONIC`.
///
/// - The bitset is `FUTEX_BITSET_MATCH_ANY`, so the plain `FUTEX_WAKE` in [`Futex::wake`] wakes
///   either sleep.
#[cfg(target_os = "linux")]
fn futex_wait(seen: Seen<'_>, op: libc::c_int, timeout: &libc::timespec) {
    // SAFETY: the word is a live, aligned u32 for the call, which the kernel reads atomically
    // against the seen value, and the timeout is a valid timespec. FUTEX_WAIT ignores the last
    // argument. An error return (EAGAIN when the word moved, ETIMEDOUT, EINTR) is a return, and the
    // caller looks again.
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            seen.word().as_ptr(),
            op,
            seen.value(),
            timeout as *const libc::timespec,
            core::ptr::null::<u32>(),
            libc::FUTEX_BITSET_MATCH_ANY,
        );
    }
}
