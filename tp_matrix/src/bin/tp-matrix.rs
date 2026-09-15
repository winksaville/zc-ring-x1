//! tp-matrix: run every flavor × placement round-trip cell and
//! emit one markdown table, phase costs with each recv's spin
//! decomposition beside it and a column legend, ready to paste, the one-command replacement
//! for the perf(1)-and-scrape recipe.
//!
//! Placements are discovered from the CPU topology
//! ([`tp_runner::topo`]): same cache domain, cross cache
//! domain, SMT siblings, unpinned, whichever the machine has.
//! Cells run sequentially in this process. Each cell re-pins
//! (or unpins) the threads and collects its own fill counters.

use clap::Parser;

use tp_matrix::{
    CellResult, FLAVORS, Flavor, PLACEMENT_MEANING, XFILLS_MEANING, print_legend, run_cell,
};
use tp_runner::topo::{Placement, discover_placements};
use tp_runner::{Cfg, CommonArgs};
use tprobe::{TProbe, ticks};

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-matrix ",
    env!("CARGO_PKG_VERSION"),
    " (zc-ring-x1 ",
    env!("ZC_RING_X1_VERSION"),
    ")",
    " - run the full measurement matrix, markdown tables out"
);

/// The tp-matrix CLI.
#[derive(Parser, Debug)]
#[command(
    name = "tp-matrix",
    version = concat!(env!("CARGO_PKG_VERSION"), " (zc-ring-x1 ", env!("ZC_RING_X1_VERSION"), ")"),
    about = TOP_ABOUT,
    max_term_width = 80
)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    /// Print a legend under the table explaining every column
    #[arg(short = 'v', long)]
    verbose: bool,
}

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
/// band, ns by default, raw ticks under `-t`, raw counts for
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

/// `xfills/RT` cell: 3 decimals, or 4 when the value is tiny
/// (the SMT cells), `-` when counters were unavailable.
/// `switches/RT` cell for the segmented flavor, `-` for the
/// single-region ones.
fn switches_cell(res: &CellResult) -> String {
    match res.switches {
        Some(n) => format!("{:.3}", n as f64 / res.rts.max(1) as f64),
        None => "-".to_string(),
    }
}

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

/// Print `rows` as an aligned markdown table under `headers`.
/// The first two columns left-aligned, the rest right-aligned.
/// The depth column is numeric, so it takes the right side.
/// Returns the table's width in characters.
fn print_table(headers: &[&str], rows: &[Vec<String>]) -> usize {
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
    sep.len()
}

/// Entry point: banner, run the matrix, emit the table and its
/// legend.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    let cfg: Cfg = cli.common.to_cfg(None);
    let placements = discover_placements();
    let unit = if cfg.ticks { "tk" } else { "ns" };
    println!(
        "{} cells, {:.1}s each, spsc-v3 and mpsc-v2 with {} segments{}",
        placements.len() * FLAVORS.len() * cfg.depths.len(),
        cfg.duration.as_secs_f64(),
        cfg.segments,
        if cli.verbose {
            ""
        } else {
            "; -v for a column legend"
        },
    );

    let mut cells: Vec<(&Placement, Flavor, u32, CellResult)> = Vec::new();
    for placement in &placements {
        for flavor in FLAVORS {
            for &depth in &cfg.depths {
                if depth < flavor.min_depth() {
                    eprintln!(
                        "skipping {} {} depth {depth}: {}",
                        placement.label,
                        flavor.as_str(),
                        flavor.floor_note()
                    );
                    continue;
                }
                eprintln!(
                    "running {} {} depth {depth} ...",
                    placement.label,
                    flavor.as_str()
                );
                let res = run_cell(flavor, cfg.duration, placement.pin, depth, cfg.segments);
                cells.push((placement, flavor, depth, res));
            }
        }
    }

    // Trip order, each recv followed by the spin and polls inside
    // it.
    let rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(p, f, d, r)| {
            vec![
                p.label.clone(),
                f.as_str().to_string(),
                d.to_string(),
                stat_cell(&r.probes[M_SEND], &cfg),
                stat_cell(&r.probes[W_RECV], &cfg),
                stat_cell(&r.probes[W_SPIN], &cfg),
                stat_cell(&r.probes[W_ATT], &cfg),
                stat_cell(&r.probes[W_SEND], &cfg),
                stat_cell(&r.probes[M_RECV], &cfg),
                stat_cell(&r.probes[M_SPIN], &cfg),
                stat_cell(&r.probes[M_ATT], &cfg),
                rts_cell(r),
                fills_cell(r),
                switches_cell(r),
            ]
        })
        .collect();
    println!();
    let width = print_table(
        &[
            "placement",
            "flavor",
            "depth",
            "m.send",
            "w.recv",
            "w.spin",
            "w.att",
            "w.send",
            "m.recv",
            "m.spin",
            "m.att",
            "RTs",
            "xfills/RT",
            "switches/RT",
        ],
        &rows,
    );
    if !cli.verbose {
        return;
    }
    let band = "mean/stdev of the trimmed min-p99 band";
    let send = |who: &str| {
        format!(
            "{who}'s send, reserve + fill + commit, the producer's cost of placing a message, {unit} as {band}"
        )
    };
    let recv = |who: &str| {
        format!("{who}'s recv, spin wait for arrival + read + release, {unit} as {band}")
    };
    let spin = |recv: &str| {
        format!(
            "the wait inside {recv}, first failed poll to reserve success, {unit} as {band}, recorded only for reserves that waited"
        )
    };
    let att = |recv: &str| format!("polls per waiting reserve inside {recv}, as {band}");
    println!();
    print_legend(
        width,
        &[
            ("placement", PLACEMENT_MEANING),
            (
                "flavor",
                "the ring both directions of the round trip run over",
            ),
            (
                "depth",
                "slots per ring, per segment for spsc-v3 and mpsc-v2. One message is ever in flight, so depth changes which seq words share a cache line and, at 1, whether the ring has any slack, and at 1 spsc-v3 switches segments on every message",
            ),
            ("m.send", &send("main")),
            ("w.recv", &recv("the worker")),
            ("w.spin", &spin("w.recv")),
            ("w.att", &att("w.recv")),
            ("w.send", &send("the worker")),
            ("m.recv", &recv("main")),
            ("m.spin", &spin("m.recv")),
            ("m.att", &att("m.recv")),
            (
                "RTs",
                "round trips completed in the cell's duration, in millions",
            ),
            ("xfills/RT", &format!("{XFILLS_MEANING}, per round trip")),
            (
                "switches/RT",
                "segment switches across both rings per round trip, spsc-v3 and mpsc-v2 only: 0 while every message fits its segment, 2 when each ring switches on every message",
            ),
        ],
    );
}
