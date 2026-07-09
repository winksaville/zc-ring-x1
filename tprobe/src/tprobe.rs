//! Free-form measurement probe: a named, single-writer histogram
//! of hardware tick-counter deltas.
//!
//! The caller records tick deltas (`ticks::read_ticks() −
//! ticks::read_ticks()`) rather than nanoseconds: skipping the
//! tick→ns conversion at record time trims a mul-shift from the
//! hot path, and conversion to nanoseconds, if desired, is
//! deferred to the report phase using
//! [`crate::ticks::ticks_per_ns`].
//!
//! For a span-based recording API (`start` / `end` with a
//! deferred-processing record buffer) see [`crate::tprobe_span`].
//! The two primitives are kept separate because the span API's
//! buffer-per-sample model trades hot-path throughput for
//! flexibility, so mixing the paths on one type forces awkward
//! trade-offs.

use hdrhistogram::Histogram;

use crate::band_table;
use crate::ticks;

/// A named, single-writer histogram of hardware tick-counter
/// deltas. Not `Sync`; cross-thread *sharing* is out of scope.
/// `Send` so probes can be moved between threads (e.g. returned
/// via a `JoinHandle<TProbe>` on shutdown).
pub struct TProbe {
    name: String,
    hist: Histogram<u64>,
    /// Values are unitless counts, not ticks — reports render
    /// with the `ct` unit and never convert to ns.
    counts: bool,
}

impl TProbe {
    /// Create an empty probe. Histogram upper bound is 1e12
    /// ticks (~250 s at 4 GHz, ~100 s at 10 GHz), 3 significant
    /// figures.
    ///
    /// Exits the process (code 1) if the hardware tick counter
    /// isn't usable — see [`crate::ticks::require_ok`].
    pub fn new(name: &str) -> Self {
        ticks::require_ok();
        // Trigger calibration eagerly so the first report() doesn't
        // pay for it.
        let _ = ticks::ticks_per_ns();
        Self {
            name: name.to_string(),
            hist: Histogram::<u64>::new_with_bounds(1, 1_000_000_000_000, 3).unwrap(), // OK: constant bounds are valid
            counts: false,
        }
    }

    /// Create an empty probe whose recorded values are unitless
    /// counts (e.g. spin attempts) rather than tick deltas;
    /// reports render with the `ct` unit and never convert.
    pub fn new_counts(name: &str) -> Self {
        TProbe {
            counts: true,
            ..TProbe::new(name)
        }
    }

    /// Number of recorded samples.
    pub fn count(&self) -> u64 {
        self.hist.len()
    }

    /// Whether this probe stores unitless counts
    /// ([`new_counts`](TProbe::new_counts)) rather than tick
    /// deltas — callers rendering values decide conversion by
    /// this.
    pub fn is_counts(&self) -> bool {
        self.counts
    }

    /// Mean and stdev of the trimmed min-p99 band, in stored
    /// units (ticks, or raw counts for a `new_counts` probe) —
    /// the report's `mean min-p99` / `stdev min-p99` lines.
    /// `None` when empty.
    pub fn trimmed_stats(&self) -> Option<(f64, f64)> {
        band_table::trimmed_stats(&self.hist)
    }

    /// Record a single sample, in tick-counter deltas. Values
    /// of 0 are clamped to 1 since the histogram's lower bound
    /// is 1; back-to-back tick reads can produce 0 on fast cores.
    pub fn record(&mut self, ticks: u64) {
        self.hist.record(ticks.max(1)).unwrap(); // OK: clamped ≥1, and any real delta is under the 1e12 bound
    }

    /// Render a band-table report for this probe. `as_ticks`
    /// controls the display unit: `false` converts stored tick
    /// deltas to nanoseconds (default for the CLI); `true` shows
    /// raw ticks (`-t`/`--ticks`); a [`new_counts`] probe always
    /// renders unitless counts. `decimals` is the fractional
    /// digits on every value column.
    ///
    /// [`new_counts`]: TProbe::new_counts
    pub fn report(&self, as_ticks: bool, decimals: usize) {
        let unit = if self.counts {
            band_table::Unit::Count
        } else if as_ticks {
            band_table::Unit::Ticks
        } else {
            band_table::Unit::Ns
        };
        band_table::render("tprobe", &self.name, &self.hist, unit, decimals);
    }
}
