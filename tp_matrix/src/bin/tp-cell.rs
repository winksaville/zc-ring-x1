//! tp-cell: run one phase-probed round-trip cell and print the
//! probe reports — the single-cell tool (the matrix's sibling,
//! see `tp-matrix`).
//!
//! Successor of the repo's earlier `tp_roundtrip` example, plus
//! in-process fill counters: each flavor's report ends with a
//! `fills` line (cross-core cache-line fills per round trip)
//! where the platform provides counters.

use clap::Parser;

use tp_matrix::{Flavor, run_cell};
use tp_runner::{CommonArgs, parse_pin, report};
use tprobe::fmt::fmt_commas;

/// Banner: name, version, and tagline on one line — the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-cell ",
    env!("CARGO_PKG_VERSION"),
    " - run one phase-probed ring round-trip cell"
);

/// Which ring flavor(s) a run measures.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum FlavorArg {
    /// The SPSC ring (`reserve_slot_with` both ends)
    Spsc,
    /// The MPSC ring at 1p/1c (`send_with` producers)
    Mpsc,
    /// Both, SPSC first
    Both,
}

/// The tp-cell CLI.
#[derive(Parser, Debug)]
#[command(name = "tp-cell", version, about = TOP_ABOUT, max_term_width = 80)]
struct Cli {
    /// Ring flavor(s) to run
    ///
    /// One cell is a main -> worker -> main round trip over two
    /// rings of the given flavor: main sends a counter on the
    /// request ring, the worker echoes it on the response ring.
    /// Each protocol phase (send, recv, recv spin, recv
    /// attempts, per side) is measured by its own probe and
    /// reported as a percentile band table.
    #[arg(value_enum, default_value_t = FlavorArg::Both)]
    flavor: FlavorArg,

    /// Pin main to MAIN and the worker to WORKER (logical CPU
    /// numbers, e.g. `--pin 0,1`); omit to leave the scheduler
    /// free
    ///
    /// Placement decides what the handoff costs: two cores
    /// sharing an L3 (e.g. 0,1 on a Zen 2 CCX), cores in
    /// different L3 domains (0,3), or SMT siblings sharing
    /// L1/L2 (0,12 on a 3900X).
    #[arg(long, value_name = "MAIN,WORKER", value_parser = parse_pin)]
    pin: Option<(usize, usize)>,

    #[command(flatten)]
    common: CommonArgs,
}

/// Entry point: banner, run the requested flavors, print the
/// per-probe reports + fills line.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    let cfg = cli.common.to_cfg(cli.pin);
    let flavors: &[Flavor] = match cli.flavor {
        FlavorArg::Spsc => &[Flavor::Spsc],
        FlavorArg::Mpsc => &[Flavor::Mpsc],
        FlavorArg::Both => &[Flavor::Spsc, Flavor::Mpsc],
    };
    for &flavor in flavors {
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
