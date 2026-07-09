//! tp-matrix: run every flavor × placement round-trip cell and
//! emit the two markdown tables (phase costs; spin
//! decomposition) ready to paste — the one-command replacement
//! for the perf(1)-and-scrape recipe.
//!
//! Placements are discovered from the CPU topology
//! ([`tp_runner::topo`]): same cache domain, cross cache
//! domain, SMT siblings, unpinned — whichever the machine has.
//! Cells run sequentially in this process; each cell re-pins
//! (or unpins) the threads and collects its own fill counters.

use tp_matrix::{CellResult, Flavor, run_cell};
use tp_runner::topo::{Placement, discover_placements};
use tp_runner::{Cfg, usage_exit};
use tprobe::{TProbe, ticks};

/// The CLI grammar ([`Cfg::parse`]; no positionals).
const USAGE: &str = "tp-matrix [-d secs-per-cell] [-t] [--decimals n]";

/// Probe indices in [`CellResult::probes`] trip order.
const M_SEND: usize = 0;
const W_RECV: usize = 1;
const W_SPIN: usize = 2;
const W_ATT: usize = 3;
const W_SEND: usize = 4;
const M_RECV: usize = 5;
const M_SPIN: usize = 6;
const M_ATT: usize = 7;

/// One table cell: `mean/stdev` of the probe's trimmed min-p99
/// band — ns by default, raw ticks under `-t`, raw counts for
/// an attempts probe.
fn stat_cell(p: &TProbe, cfg: &Cfg) -> String {
    let Some((mean, stdev)) = p.trimmed_stats() else {
        return "-".to_string();
    };
    let conv = if p.is_counts() || cfg.ticks {
        1.0
    } else {
        ticks::ticks_per_ns()
    };
    let d = cfg.decimals;
    format!("{:.d$}/{:.d$}", mean / conv, stdev / conv)
}

/// `fills/RT` cell: 3 decimals, or 4 when the value is tiny
/// (the SMT cells); `-` when counters were unavailable.
fn fills_cell(res: &CellResult) -> String {
    match &res.fills {
        Some(f) => {
            let v = f.lcl_cache as f64 / res.rts.max(1) as f64;
            if v < 0.01 {
                format!("{v:.4}")
            } else {
                format!("{v:.3}")
            }
        }
        None => "-".to_string(),
    }
}

/// Round trips as a compact millions figure, e.g. `52.6M`.
fn rts_cell(res: &CellResult) -> String {
    format!("{:.1}M", res.rts as f64 / 1e6)
}

/// Print `rows` as an aligned markdown table under `headers`;
/// the first two columns left-aligned, the rest right-aligned.
fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut w: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            w[i] = w[i].max(cell.len());
        }
    }
    let fmt_row = |cells: &[String]| {
        let mut line = String::from("|");
        for (i, cell) in cells.iter().enumerate() {
            if i < 2 {
                line.push_str(&format!(" {:<w$} |", cell, w = w[i]));
            } else {
                line.push_str(&format!(" {:>w$} |", cell, w = w[i]));
            }
        }
        line
    };
    let headers: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    println!("{}", fmt_row(&headers));
    let mut sep = String::from("|");
    for (i, width) in w.iter().enumerate() {
        if i < 2 {
            sep.push_str(&format!("{}|", "-".repeat(width + 2)));
        } else {
            sep.push_str(&format!("{}:|", "-".repeat(width + 1)));
        }
    }
    println!("{sep}");
    for row in rows {
        println!("{}", fmt_row(row));
    }
}

/// Entry point: run the matrix, emit the two tables.
fn main() {
    let cfg = Cfg::parse(USAGE);
    if !cfg.positionals.is_empty() {
        usage_exit(USAGE);
    }
    let placements = discover_placements();
    let unit = if cfg.ticks { "tk" } else { "ns" };
    println!(
        "tp-matrix: {} cells, {:.1}s each; phase cells are mean/stdev of the trimmed \
         min-p99 band in {unit}; att in polls/waiting reserve; fills/RT = cross-core \
         cache-line fills per round trip",
        placements.len() * 2,
        cfg.duration.as_secs_f64(),
    );
    println!();

    let mut cells: Vec<(&Placement, Flavor, CellResult)> = Vec::new();
    for placement in &placements {
        for flavor in [Flavor::Spsc, Flavor::Mpsc] {
            eprintln!("running {} {} ...", placement.label, flavor.as_str());
            let res = run_cell(flavor, cfg.duration, placement.pin);
            cells.push((placement, flavor, res));
        }
    }

    let phase_rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(p, f, r)| {
            vec![
                p.label.clone(),
                f.as_str().to_string(),
                stat_cell(&r.probes[M_SEND], &cfg),
                stat_cell(&r.probes[W_RECV], &cfg),
                stat_cell(&r.probes[W_SEND], &cfg),
                stat_cell(&r.probes[M_RECV], &cfg),
                rts_cell(r),
                fills_cell(r),
            ]
        })
        .collect();
    println!("Phase costs (send = reserve+fill+commit; recv = spin wait+read+release):");
    println!();
    print_table(
        &[
            "placement",
            "flavor",
            "m.send",
            "w.recv",
            "w.send",
            "m.recv",
            "RTs",
            "fills/RT",
        ],
        &phase_rows,
    );
    println!();

    let spin_rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(p, f, r)| {
            vec![
                p.label.clone(),
                f.as_str().to_string(),
                stat_cell(&r.probes[W_SPIN], &cfg),
                stat_cell(&r.probes[W_ATT], &cfg),
                stat_cell(&r.probes[M_SPIN], &cfg),
                stat_cell(&r.probes[M_ATT], &cfg),
                fills_cell(r),
            ]
        })
        .collect();
    println!("Spin decomposition (spin = first failed attempt -> reserve success):");
    println!();
    print_table(
        &[
            "placement",
            "flavor",
            "w.spin",
            "w.att",
            "m.spin",
            "m.att",
            "fills/RT",
        ],
        &spin_rows,
    );
}
