//! Hardware tick-counter probes: named single-writer histograms
//! of tick deltas ([`TProbe`]) and a span-based sibling with a
//! deferred-processing record buffer ([`TProbeSpan`]).
//!
//! - [`ticks`] — the fixed-rate monotonic counter (`rdtsc` /
//!   `CNTVCT_EL0`) with tick→ns calibration.
//! - [`band_table`] — the percentile band-table report both
//!   probe types render.
//! - [`fmt`] — thousands-separator number formatting for the
//!   report.

pub mod band_table;
pub mod fmt;
pub mod ticks;
pub mod tprobe;
pub mod tprobe_span;

pub use tprobe::TProbe;
pub use tprobe_span::{TProbeSpan, TProbeSpanId};
