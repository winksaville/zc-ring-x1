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

use core::sync::atomic::AtomicU32;

/// The platform's sleep and wake, called by a ring's waiting
/// paths.
pub trait Wake {
    /// Whether the waking side must look for sleepers: `false`
    /// compiles every wake check out of the message paths.
    const WAKES: bool;

    /// Sleep while `word` holds `expected`.
    ///
    /// - Returns on a wake, when the word no longer holds
    ///   `expected`, spuriously, or at the implementation's own
    ///   timeout, and the caller looks again whichever it was.
    fn wait(word: &AtomicU32, expected: u32);

    /// Wake every sleeper on `word`.
    fn wake(word: &AtomicU32);
}

/// No sleeping: a wait is one spin-loop hint and a wake does
/// nothing, so a waiting call polls.
pub struct NoWake;

impl Wake for NoWake {
    const WAKES: bool = false;

    fn wait(_word: &AtomicU32, _expected: u32) {
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

    fn wait(word: &AtomicU32, expected: u32) {
        let timeout = libc::timespec {
            tv_sec: (TIMEOUT_MS / 1000) as libc::time_t,
            tv_nsec: ((TIMEOUT_MS % 1000) * 1_000_000) as libc::c_long,
        };
        // SAFETY: word is a live, aligned u32 for the call, which
        // the kernel reads atomically against expected, and the
        // timeout is a valid relative timespec. An error return
        // (EAGAIN when the word moved, ETIMEDOUT, EINTR) is a
        // return, and the caller looks again.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                word.as_ptr(),
                libc::FUTEX_WAIT,
                expected,
                &timeout as *const libc::timespec,
                core::ptr::null::<u32>(),
                0u32,
            );
        }
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
