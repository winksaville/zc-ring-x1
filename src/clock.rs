//! The `clock` module holds the monotonic clock the deadline sends read, and the conversions that
//! make [`Ticks`].
//!
//! - A tick is the clock's own unit and has no dimension a caller may rely on. It is one nanosecond
//!   today, and a later clock may count something else, such as a CPU's cycle counter, whose rate
//!   is found when the program runs. So a caller makes every [`Ticks`] with a conversion,
//!   [`secs_to_ticks`], [`millis_to_ticks`], [`microsecs_to_ticks`], or [`nanos_to_ticks`], and
//!   the conversions are ordinary functions, not `const`, since a later one may read that rate.
//! - A conversion is one multiply by a constant today, which the caller pays once, so a send or a
//!   receive only adds and compares.
//! - On Linux the clock is `clock_gettime(CLOCK_MONOTONIC)`, called through `libc`: a vDSO call,
//!   with no syscall and no `std`. It is also the clock a [`Futex`](crate::wake::Futex) sleeps
//!   against, so the kernel reads a [`Deadline`](crate::Deadline) as it is.
//! - Elsewhere, with the `std` feature, the clock is `std::time::Instant`, counted from the first
//!   reading in the process.

use crate::Ticks;

/// `secs_to_ticks` converts a number of seconds to [`Ticks`]. A caller converts once, when it sets
/// up its times, not once per send or receive.
///
/// # Parameters
///
/// - `s`: the number of seconds.
///
/// # Returns
///
/// - The same duration in ticks. A duration too long to count saturates to [`Ticks::FOREVER`].
pub fn secs_to_ticks(s: u64) -> Ticks {
    Ticks(s.saturating_mul(1_000_000_000))
}

/// `millis_to_ticks` converts a number of milliseconds to [`Ticks`]. A caller converts once, when
/// it sets up its times, not once per send or receive.
///
/// # Parameters
///
/// - `ms`: the number of milliseconds.
///
/// # Returns
///
/// - The same duration in ticks. A duration too long to count saturates to [`Ticks::FOREVER`].
pub fn millis_to_ticks(ms: u64) -> Ticks {
    Ticks(ms.saturating_mul(1_000_000))
}

/// `microsecs_to_ticks` converts a number of microseconds to [`Ticks`]. A caller converts once,
/// when it sets up its times, not once per send.
///
/// # Parameters
///
/// - `us`: the number of microseconds.
///
/// # Returns
///
/// - The same duration in ticks. A duration too long to count saturates to [`Ticks::FOREVER`].
pub fn microsecs_to_ticks(us: u64) -> Ticks {
    Ticks(us.saturating_mul(1_000))
}

/// `nanos_to_ticks` converts a number of nanoseconds to [`Ticks`]. A caller converts once, when it
/// sets up its times, not once per send.
///
/// # Parameters
///
/// - `ns`: the number of nanoseconds.
///
/// # Returns
///
/// - The same duration in ticks. `u64::MAX` nanoseconds is [`Ticks::FOREVER`].
pub fn nanos_to_ticks(ns: u64) -> Ticks {
    Ticks(ns)
}

/// `now_ticks` reads the monotonic clock, in ticks, which are nanoseconds.
///
/// - The reading costs one multiply by a constant and an add, which fold the clock's seconds and
///   nanoseconds into one `u64`. A `u64` of nanoseconds wraps after about 584 years.
#[cfg(target_os = "linux")]
#[inline]
pub(crate) fn now_ticks() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a live, writable timespec, and CLOCK_MONOTONIC is a clock every Linux has, so
    // the call cannot fail.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// `now_ticks` reads the monotonic clock, in ticks, which are nanoseconds.
///
/// - The reading costs one multiply by a constant and an add, which fold the clock's seconds and
///   nanoseconds into one `u64`. A `u64` of nanoseconds wraps after about 584 years.
#[cfg(all(not(target_os = "linux"), feature = "std"))]
pub(crate) fn now_ticks() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    // A u64 of nanoseconds outlasts any process, so the cast from u128 truncates nothing reachable.
    ORIGIN.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_agree_and_saturate() {
        // Each unit is a thousand of the next, whatever a tick is.
        assert_eq!(secs_to_ticks(1), millis_to_ticks(1_000));
        assert_eq!(millis_to_ticks(1), microsecs_to_ticks(1_000));
        assert_eq!(microsecs_to_ticks(1), nanos_to_ticks(1_000));
        assert!(secs_to_ticks(1) > Ticks::ZERO);
        // A duration too long to count never passes.
        assert_eq!(secs_to_ticks(u64::MAX), Ticks::FOREVER);
        assert_eq!(millis_to_ticks(u64::MAX), Ticks::FOREVER);
    }
}
