//! The clock the time-bounded sends read, behind the `std` feature.
//!
//! - One type, so a `no_std` clock replaces `std`'s here and
//!   nowhere else.

use std::time::Instant;

/// Time elapsed since it started, on a monotonic clock.
pub(crate) struct Stopwatch(Instant);

impl Stopwatch {
    /// Start timing now.
    #[inline]
    pub(crate) fn start() -> Self {
        Stopwatch(Instant::now())
    }

    /// Microseconds since [`start`](Stopwatch::start).
    #[inline]
    pub(crate) fn elapsed_us(&self) -> u64 {
        // u64 microseconds outlast any process, so the cast from
        // u128 truncates nothing reachable.
        self.0.elapsed().as_micros() as u64
    }
}
