//! tp-cell: run one phase-probed round-trip cell and print the
//! probe reports — the single-cell tool (the matrix's sibling,
//! see `tp-matrix`).
//!
//! Successor of the repo's earlier `tp_roundtrip` example, plus
//! in-process fill counters: each flavor's report ends with a
//! `fills` line (cross-core cache-line fills per round trip)
//! where the platform provides counters.

use tp_matrix::{Flavor, run_cell};
use tp_runner::{Cfg, report, usage_exit};
use tprobe::fmt::fmt_commas;

/// The CLI grammar, interpreted by [`Cfg::parse`] plus the
/// flavor positionals.
const USAGE: &str = "tp-cell [spsc|mpsc|both] [-d secs] [--pin main,worker] [-t] [--decimals n]";

/// Entry point: parse args, run the requested flavors, print
/// reports + fills.
fn main() {
    let cfg = Cfg::parse(USAGE);
    let (mut spsc, mut mpsc) = (true, true);
    for p in &cfg.positionals {
        match p.as_str() {
            "spsc" => (spsc, mpsc) = (true, false),
            "mpsc" => (spsc, mpsc) = (false, true),
            "both" => (spsc, mpsc) = (true, true),
            _ => usage_exit(USAGE),
        }
    }
    let mut flavors = Vec::new();
    if spsc {
        flavors.push(Flavor::Spsc);
    }
    if mpsc {
        flavors.push(Flavor::Mpsc);
    }
    for flavor in flavors {
        let res = run_cell(flavor, cfg.duration, cfg.pin);
        report(flavor.as_str(), &cfg, res.probes);
        match &res.fills {
            Some(f) => println!(
                "  fills: lcl_cache={} ({:.3}/RT)  lcl_l2={}  lcl_dram={}  [RTs={}]\n",
                fmt_commas(f.lcl_cache),
                f.lcl_cache as f64 / res.rts.max(1) as f64,
                fmt_commas(f.lcl_l2),
                fmt_commas(f.lcl_dram),
                fmt_commas(res.rts),
            ),
            None => println!("  fills: unavailable\n"),
        }
    }
}
