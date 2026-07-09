//! Shared band-table renderer for tick-valued histograms.
//!
//! Both `TProbe` (fast path, direct-histogram) and `TProbeSpan`
//! (scope API, records → drain) store hardware tick deltas and
//! want the same band-table output shape — min/p1/…/p99/max
//! rows with first/last/range/count/mean columns, plus summary
//! lines for mean, stdev, mean min-p99, stdev min-p99. This
//! module provides a single implementation both can call into.
//!
//! Display unit is chosen by `as_ticks`: `false` converts stored
//! tick values to nanoseconds via [`crate::ticks::ticks_per_ns`];
//! `true` shows raw ticks.

use hdrhistogram::Histogram;

use crate::fmt::{fmt_commas, fmt_commas_f64};
use crate::ticks;

const BOUNDARY_PCTS: &[f64] = &[
    0.0, 0.01, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 0.99, 1.0,
];
const BOUNDARY_NAMES: &[&str] = &[
    "min", "p1", "p10", "p20", "p30", "p40", "p50", "p60", "p70", "p80", "p90", "p99", "max",
];

/// Display unit for a rendered band table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    /// Convert stored ticks to nanoseconds (`ns`).
    Ns,
    /// Raw stored ticks (`tk`).
    Ticks,
    /// Unitless counts (`ct`) — no conversion.
    Count,
}

/// Render a band-table report for `hist`. `kind` is the header
/// label (`"tprobe"`, `"tprobe-span"`, …) and `name` is the
/// probe's name. `unit` picks the display unit / conversion;
/// `decimals` is the fractional digits on every value column.
pub(crate) fn render(kind: &str, name: &str, hist: &Histogram<u64>, unit: Unit, decimals: usize) {
    let sample_count = hist.len();
    println!("  {kind}: {name} [count={}]", fmt_commas(sample_count));
    if sample_count == 0 {
        println!();
        return;
    }

    let to_ns = unit == Unit::Ns;
    let unit = match unit {
        Unit::Ns => "ns",
        Unit::Ticks => "tk",
        Unit::Count => "ct",
    };
    let tpn = ticks::ticks_per_ns();
    let conv = |v: u64| -> f64 { if to_ns { v as f64 / tpn } else { v as f64 } };
    let conv_f = |v: f64| -> f64 { if to_ns { v / tpn } else { v } };

    let n_bands = BOUNDARY_PCTS.len() - 1;
    let mut band_first = vec![u64::MAX; n_bands];
    let mut band_last = vec![0u64; n_bands];
    let mut band_count = vec![0u64; n_bands];
    let mut band_sum = vec![0u128; n_bands];

    let mut cumulative = 0u64;
    for iv in hist.iter_recorded() {
        let value = iv.value_iterated_to();
        let count = iv.count_at_value();
        let mid_rank = (cumulative as f64 + count as f64 / 2.0) / sample_count as f64;
        let idx = BOUNDARY_PCTS[1..]
            .iter()
            .position(|&b| mid_rank < b)
            .unwrap_or(n_bands - 1); // OK: rank ≥ last boundary → top band
        band_first[idx] = band_first[idx].min(value);
        band_last[idx] = band_last[idx].max(value);
        band_count[idx] += count;
        band_sum[idx] += value as u128 * count as u128;
        cumulative += count;
    }

    struct BandRow {
        label: String,
        first: String,
        last: String,
        range: String,
        count: String,
        mean: String,
    }

    let mut rows: Vec<BandRow> = Vec::new();
    for i in 0..n_bands {
        if band_count[i] == 0 {
            continue;
        }
        let mean_val = band_sum[i] as f64 / band_count[i] as f64;
        let range_raw = band_last[i] - band_first[i] + 1;
        rows.push(BandRow {
            label: format!("{}-{}", BOUNDARY_NAMES[i], BOUNDARY_NAMES[i + 1]),
            first: fmt_commas_f64(conv(band_first[i]), decimals),
            last: fmt_commas_f64(conv(band_last[i]), decimals),
            range: fmt_commas_f64(conv(range_raw), decimals),
            count: fmt_commas(band_count[i]),
            mean: fmt_commas_f64(conv_f(mean_val), decimals),
        });
    }

    let label_w = rows
        .iter()
        .map(|r| r.label.len())
        .max()
        .unwrap_or(0) // OK: obvious
        .max("stdev min-p99".len());
    let first_w = rows.iter().map(|r| r.first.len()).max().unwrap_or(0); // OK: obvious
    let last_w = rows.iter().map(|r| r.last.len()).max().unwrap_or(0); // OK: obvious
    let range_w = rows.iter().map(|r| r.range.len()).max().unwrap_or(0); // OK: obvious
    let count_w = rows.iter().map(|r| r.count.len()).max().unwrap_or(0); // OK: obvious
    let mean_w = rows.iter().map(|r| r.mean.len()).max().unwrap_or(0); // OK: obvious

    const INDENT: &str = "    ";
    const GAP: &str = "    ";

    let first_col = INDENT.len() + label_w + 1 + first_w;
    let unit_len = 1 + unit.len();
    let last_gap = unit_len + GAP.len() + last_w;
    let range_gap = unit_len + GAP.len() + range_w;
    let count_gap = unit_len + GAP.len() + count_w;
    let mean_gap = GAP.len() + mean_w;
    println!(
        "{:>first_col$}{:>last_gap$}{:>range_gap$}{:>count_gap$}{:>mean_gap$}",
        "first", "last", "range", "count", "mean",
    );

    for r in &rows {
        println!(
            "{INDENT}{:<label_w$} {:>first_w$} {unit}{GAP}{:>last_w$} {unit}{GAP}{:>range_w$} {unit}{GAP}{:>count_w$}{GAP}{:>mean_w$} {unit}",
            r.label, r.first, r.last, r.range, r.count, r.mean,
        );
    }

    let hist_mean = hist.mean();
    let skip = first_w
        + unit_len
        + GAP.len()
        + last_w
        + unit_len
        + GAP.len()
        + range_w
        + unit_len
        + GAP.len()
        + count_w;
    println!(
        "{INDENT}{:<label_w$} {:>skip$}{GAP}{:>mean_w$} {unit}",
        "mean",
        "",
        fmt_commas_f64(conv_f(hist_mean), decimals),
    );
    println!(
        "{INDENT}{:<label_w$} {:>skip$}{GAP}{:>mean_w$} {unit}",
        "stdev",
        "",
        fmt_commas_f64(conv_f(hist.stdev()), decimals),
    );

    let trim_count: u64 = band_count[..n_bands - 1].iter().sum();
    if trim_count > 0 {
        let trim_sum: u128 = band_sum[..n_bands - 1].iter().sum();
        let trim_mean = trim_sum as f64 / trim_count as f64;

        let mut trim_var_sum = 0.0f64;
        let mut trim_var_count = 0u64;
        let mut cum = 0u64;
        for iv in hist.iter_recorded() {
            let value = iv.value_iterated_to();
            let count = iv.count_at_value();
            let mid_rank = (cum as f64 + count as f64 / 2.0) / sample_count as f64;
            let idx = BOUNDARY_PCTS[1..]
                .iter()
                .position(|&b| mid_rank < b)
                .unwrap_or(n_bands - 1); // OK: rank ≥ last boundary → top band
            if idx < n_bands - 1 {
                let diff = value as f64 - trim_mean;
                trim_var_sum += diff * diff * count as f64;
                trim_var_count += count;
            }
            cum += count;
        }
        let trim_stdev = if trim_var_count > 1 {
            (trim_var_sum / trim_var_count as f64).sqrt()
        } else {
            0.0
        };

        println!(
            "{INDENT}{:<label_w$} {:>skip$}{GAP}{:>mean_w$} {unit}",
            "mean min-p99",
            "",
            fmt_commas_f64(conv_f(trim_mean), decimals),
        );
        println!(
            "{INDENT}{:<label_w$} {:>skip$}{GAP}{:>mean_w$} {unit}",
            "stdev min-p99",
            "",
            fmt_commas_f64(conv_f(trim_stdev), decimals),
        );
    }
    println!();
}
