//! Span-based measurement probe: a named, single-writer
//! histogram plus a record buffer, populated via `start` /
//! `end` rather than `record(ticks)`.
//!
//! `start(site_id)` reads the hardware tick counter and returns
//! an opaque [`TProbeSpanId`] carrying `(site_id, start_tsc)`;
//! `end(id)` reads the tick counter again and appends a complete
//! `(site_id, start_tsc, end_tsc)` record to the probe's
//! internal buffer. No delta math, histogram ingestion, or
//! tick→ns conversion happens on the hot path — all of that is
//! deferred to [`TProbeSpan::report`], which drains pending records
//! into the histogram before rendering.
//!
//! This primitive preserves record-order information across
//! interleaved spans and sites (non-stack nesting is supported
//! by construction) and gives future evolution space for
//! per-site grouping, bounded buffers, background drain
//! threads, and long-term trace retention.
//!
//! The trade-off vs. [`crate::tprobe::TProbe`]: a growing
//! `Vec<Record>` in the hot path adds cache pressure and
//! reallocation cost in long, high-rate runs. For high-rate
//! single-histogram measurement prefer `TProbe`.

use hdrhistogram::Histogram;

use crate::band_table;
use crate::ticks;

/// Opaque handle returned by [`TProbeSpan::start`], consumed by
/// [`TProbeSpan::end`]. Carries the caller-supplied `site_id` and
/// the start-time tick reading; no probe-internal allocation
/// happens at `start` time.
///
/// `#[must_use]` — dropping the id without passing it to
/// [`TProbeSpan::end`] leaks the span (no record is appended).
#[must_use]
#[derive(Clone, Copy, Debug)]
pub struct TProbeSpanId {
    site_id: u64,
    start_tsc: u64,
}

/// A complete span record: `(site_id, start_tsc, end_tsc)`.
/// Appended at [`TProbeSpan::end`] time; the record buffer only
/// ever holds complete records. Drained into the histogram at
/// [`TProbeSpan::report`] time.
#[derive(Clone, Copy, Debug)]
struct Record {
    #[allow(dead_code)] // read once per-site grouping lands.
    site_id: u64,
    start_tsc: u64,
    end_tsc: u64,
}

/// A named, single-writer histogram of hardware tick-counter
/// deltas plus a span-record buffer. Not `Sync`; cross-thread
/// *sharing* is out of scope. `Send` so probes can be moved
/// between threads (e.g. returned via a `JoinHandle<TProbeSpan>`
/// on shutdown).
pub struct TProbeSpan {
    name: String,
    hist: Histogram<u64>,
    records: Vec<Record>,
}

impl TProbeSpan {
    /// Create an empty probe. Histogram upper bound is 1e12
    /// ticks (~250 s at 4 GHz, ~100 s at 10 GHz), 3 significant
    /// figures.
    ///
    /// Exits the process (code 1) if the hardware tick counter
    /// isn't usable — see [`crate::ticks::require_ok`].
    pub fn new(name: &str) -> Self {
        ticks::require_ok();
        let _ = ticks::ticks_per_ns();
        Self {
            name: name.to_string(),
            hist: Histogram::<u64>::new_with_bounds(1, 1_000_000_000_000, 3).unwrap(), // OK: constant bounds are valid
            records: Vec::new(),
        }
    }

    /// Begin a span. Reads the hardware tick counter and
    /// returns an opaque [`TProbeSpanId`] carrying `(site_id,
    /// start_tsc)`. The id must eventually be passed to
    /// [`TProbeSpan::end`]; a dropped id leaves no record.
    #[inline]
    pub fn start(&mut self, site_id: u64) -> TProbeSpanId {
        TProbeSpanId {
            site_id,
            start_tsc: ticks::read_ticks(),
        }
    }

    /// End the span started by [`TProbeSpan::start`]. Reads the
    /// hardware tick counter and appends a complete record
    /// `(site_id, start_tsc, end_tsc)` to the probe's record
    /// buffer. Delta and histogram ingestion are deferred to
    /// [`TProbeSpan::report`].
    #[inline]
    pub fn end(&mut self, tpri: TProbeSpanId) {
        let end_tsc = ticks::read_ticks();
        self.records.push(Record {
            site_id: tpri.site_id,
            start_tsc: tpri.start_tsc,
            end_tsc,
        });
    }

    /// Render a band-table report for this probe. `as_ticks`
    /// controls the display unit: `false` converts stored tick
    /// deltas to nanoseconds (default for the CLI); `true` shows
    /// raw ticks (`-t`/`--ticks`). `decimals` is the fractional
    /// digits on every value column.
    ///
    /// Drains any pending `start`/`end` records into the histogram
    /// before rendering: `delta = end_tsc − start_tsc`, clamped to
    /// `1` since the histogram lower bound is 1.
    pub fn report(&mut self, as_ticks: bool, decimals: usize) {
        for r in self.records.drain(..) {
            let delta = r.end_tsc.saturating_sub(r.start_tsc);
            self.hist.record(delta.max(1)).unwrap(); // OK: clamped ≥1, and any real delta is under the 1e12 bound
        }
        let unit = if as_ticks {
            band_table::Unit::Ticks
        } else {
            band_table::Unit::Ns
        };
        band_table::render("tprobe-span", &self.name, &self.hist, unit, decimals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_end_appends_one_record() {
        let mut p = TProbeSpan::new("t");
        let id = p.start(42);
        p.end(id);
        assert_eq!(p.records.len(), 1);
        let r = &p.records[0];
        assert_eq!(r.site_id, 42);
        assert!(r.end_tsc >= r.start_tsc);
    }

    #[test]
    fn start_end_preserves_start_tsc() {
        let mut p = TProbeSpan::new("t");
        let id = p.start(7);
        let saved_start = id.start_tsc;
        p.end(id);
        let r = &p.records[0];
        assert_eq!(r.site_id, 7);
        assert_eq!(r.start_tsc, saved_start);
    }

    #[test]
    fn start_end_interleaved_non_stack() {
        let mut p = TProbeSpan::new("t");
        let a = p.start(1);
        let b = p.start(2);
        p.end(a);
        p.end(b);
        assert_eq!(p.records.len(), 2);
        assert_eq!(p.records[0].site_id, 1);
        assert_eq!(p.records[1].site_id, 2);
    }

    #[test]
    fn report_drains_records_into_histogram() {
        let mut p = TProbeSpan::new("t");
        let id1 = p.start(1);
        p.end(id1);
        let id2 = p.start(2);
        p.end(id2);
        assert_eq!(p.hist.len(), 0);
        assert_eq!(p.records.len(), 2);

        p.report(false, 1);
        assert_eq!(p.records.len(), 0);
        assert_eq!(p.hist.len(), 2);

        // Idempotent: a second report drains nothing, hist unchanged.
        p.report(false, 1);
        assert_eq!(p.hist.len(), 2);
    }
}
