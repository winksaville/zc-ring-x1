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
//!   implementation. On an `mpsc::v3` ring a mismatch is not
//!   detected and costs latency only: a futex sleeper no one wakes
//!   returns at its timeout. An `mpsc::v4` ring records its wake's
//!   [`PROTOCOL`](Wake::PROTOCOL) and refuses an attach that
//!   disagrees.
//! - `MpscRing` in `mpsc::v3` and in `mpsc::v4` are the rings that take one.
//! - [`Waits`] is a ring's choice of how its endpoints wait, which an `mpsc::v4` ring takes in
//!   place of a bare [`Wake`]: [`SpinOnly`], [`Sleep`] over a wake such as [`Futex`], or
//!   [`SpinOrSleep`] over one. [`Spins`] and [`Sleeps`] say which timed sends and receives a choice
//!   offers.

use core::marker::PhantomData;
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

    /// `PROTOCOL` names how this wake's sleepers are woken, as a number a ring records so that
    /// every process attached to the ring wakes the same way.
    ///
    /// - [`PROTOCOL_NONE`]: nothing sleeps and nothing is woken. The default for a wake whose
    ///   [`WAKES`](Wake::WAKES) is `false`.
    /// - [`PROTOCOL_FUTEX`]: a Linux futex on the word, [`Futex`]'s, whatever its timeout.
    /// - [`PROTOCOL_OTHER`]: the default for a wake that sleeps and names no protocol of its own. A
    ///   wake written outside the crate overrides it with its own number, [`PROTOCOL_OTHER`] or
    ///   above, so two different wakes are not taken for one.
    const PROTOCOL: u32 = if Self::WAKES {
        PROTOCOL_OTHER
    } else {
        PROTOCOL_NONE
    };

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

/// `PROTOCOL_NONE` is the [`Wake::PROTOCOL`] of a wake where nothing sleeps.
pub const PROTOCOL_NONE: u32 = 0;

/// `PROTOCOL_FUTEX` is the [`Wake::PROTOCOL`] of [`Futex`].
pub const PROTOCOL_FUTEX: u32 = 1;

/// `PROTOCOL_OTHER` is the [`Wake::PROTOCOL`] of a wake that sleeps and names no protocol, and the
/// least number a wake written outside the crate takes for its own.
pub const PROTOCOL_OTHER: u32 = 16;

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
    const PROTOCOL: u32 = PROTOCOL_FUTEX;

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

/// `trait Waits` marks a ring's choice of how its endpoints wait: [`SpinOnly`], [`Sleep`], or
/// [`SpinOrSleep`]. An `mpsc::v4` ring takes one as its `W`.
///
/// - The choice is the ring's, not each endpoint's: an endpoint can sleep only if every other
///   endpoint checks for sleepers, and those checks cost the endpoints that are awake. The
///   `mpsc::v4` module docs say what the checks are and what they have cost.
/// - A [`Wake`] alone, such as [`Futex`], is how a sleeper sleeps and is woken, and is not a
///   `Waits`: [`Sleep`] and [`SpinOrSleep`] take one as what their endpoints sleep on.
pub trait Waits: Wake {}

/// `trait Spins` marks a [`Waits`] whose ring's endpoints may wait by spinning alone, with no sleep
/// to follow: [`SpinOnly`] and [`SpinOrSleep`]. `mpsc::v4` offers `send_spin` and `recv_spin` on
/// such a ring.
pub trait Spins: Waits {}

/// `trait Sleeps` marks a [`Waits`] whose ring's endpoints may sleep: [`Sleep`] and
/// [`SpinOrSleep`]. `mpsc::v4` offers `send_spin_sleep` and `recv_spin_sleep` on such a ring.
pub trait Sleeps: Waits {}

/// `struct SpinOnly` is the choice that every endpoint of the ring waits by spinning. Nothing
/// sleeps, so nothing checks for a sleeper, and the choice costs nothing.
///
/// - A ring over `SpinOnly` offers `send_spin` and `recv_spin`, and a sleep on it does not compile.
/// - `SpinOnly` waits and wakes as [`NoWake`] does: a wait is one spin hint and a wake does nothing.
pub struct SpinOnly;

impl Wake for SpinOnly {
    const WAKES: bool = false;

    fn wait(_seen: Seen<'_>) {
        core::hint::spin_loop();
    }

    fn wait_until(_seen: Seen<'_>, _deadline: Deadline) {
        core::hint::spin_loop();
    }

    fn wake(_word: &AtomicU32) {}
}

impl Waits for SpinOnly {}

impl Spins for SpinOnly {}

/// `struct Sleep` is the choice that an endpoint of the ring spins for some time, which may be
/// none, then may sleep on `S`, such as [`Futex`], for some further time or forever.
///
/// - A ring over `Sleep<S>` offers `send_spin_sleep` and `recv_spin_sleep`. A spin alone on it
///   does not compile: an endpoint that only spins would pay for the checks that wake sleepers and
///   gain nothing, and [`SpinOrSleep`] is the choice that says so on purpose.
/// - Every endpoint checks for sleepers, a producer after each commit and the consumer at each half
///   segment of messages read, whether or not anyone sleeps.
/// - `S` must be a wake that sleeps. A `Sleep` over one that does not, such as [`NoWake`], fails to
///   compile where the ring is used.
pub struct Sleep<S: Wake>(PhantomData<S>);

impl<S: Wake> Wake for Sleep<S> {
    const WAKES: bool = {
        assert!(S::WAKES, "Sleep needs a wake that sleeps, such as Futex");
        true
    };
    const PROTOCOL: u32 = S::PROTOCOL;

    fn wait(seen: Seen<'_>) {
        S::wait(seen);
    }

    fn wait_until(seen: Seen<'_>, deadline: Deadline) {
        S::wait_until(seen, deadline);
    }

    fn wake(word: &AtomicU32) {
        S::wake(word);
    }
}

impl<S: Wake> Waits for Sleep<S> {}

impl<S: Wake> Sleeps for Sleep<S> {}

/// `struct SpinOrSleep` is the choice that each endpoint of the ring either spins without end or
/// sleeps on `S`, a mix made on purpose. `SpinOrSleep<S>` sleeps and wakes exactly as [`Sleep<S>`]
/// does.
///
/// - A ring over `SpinOrSleep<S>` offers every timed send and receive, the spin forms and the spin
///   and sleep forms.
/// - The cost: every endpoint checks for sleepers, as over [`Sleep`]. The endpoint that pays is the
///   one awake, and the one that gains is the one asleep, so an endpoint that only spins pays for
///   the others' sleep.
/// - How much: from nothing we could measure to about 28% more time a message in our streams, and
///   about 7% of a round trip between two threads on another machine. It moves with the cores, the
///   ring's depth, the machine, and the build, so it has to be measured where it matters. The
///   `mpsc::v4` module docs say what the checks are and where the measurements are.
/// - `SpinOrSleep<S>` and `Sleep<S>` are one wake protocol: a process attached with one and a
///   process attached with the other share a ring correctly. The name is a statement in one
///   process's types.
pub struct SpinOrSleep<S: Wake>(PhantomData<S>);

impl<S: Wake> Wake for SpinOrSleep<S> {
    const WAKES: bool = {
        assert!(
            S::WAKES,
            "SpinOrSleep needs a wake that sleeps, such as Futex"
        );
        true
    };
    const PROTOCOL: u32 = S::PROTOCOL;

    fn wait(seen: Seen<'_>) {
        S::wait(seen);
    }

    fn wait_until(seen: Seen<'_>, deadline: Deadline) {
        S::wait_until(seen, deadline);
    }

    fn wake(word: &AtomicU32) {
        S::wake(word);
    }
}

impl<S: Wake> Waits for SpinOrSleep<S> {}

impl<S: Wake> Spins for SpinOrSleep<S> {}

impl<S: Wake> Sleeps for SpinOrSleep<S> {}
