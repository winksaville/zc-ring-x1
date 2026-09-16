//! tp-cell: run one phase-probed round-trip cell and print the
//! probe reports, the single-cell tool (the matrix's sibling,
//! see `tp-matrix`).
//!
//! Successor of the repo's earlier `tp_roundtrip` example, plus
//! in-process fill counters: each flavor's report ends with a
//! `fill counters` line (xfills per round trip) where the
//! platform provides counters, and a legend for it closes the
//! run.

use clap::Parser;

use tp_matrix::{FLAVORS, Flavor, XFILLS_MEANING, print_legend, run_cell};
use tp_runner::{CommonArgs, parse_pin, report};
use tprobe::fmt::fmt_commas;

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-cell ",
    env!("CARGO_PKG_VERSION"),
    " (zc-ring-x1 ",
    env!("ZC_RING_X1_VERSION"),
    ")",
    " - run one phase-probed ring round-trip cell"
);

/// Which ring flavor(s) a run measures.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum FlavorArg {
    /// The SPSC v0 ring (`reserve_slot_with` both ends)
    SpscV0,
    /// The SPSC v1 seam-word ring (same surface, per-slot seq)
    SpscV1,
    /// The SPSC v2 in-slot seq ring (same surface, the seq in
    /// its slot)
    SpscV2,
    /// The SPSC v3 ring of segments (same surface, `--segments`
    /// per ring, the depth each segment's)
    SpscV3,
    /// The MPSC v0 ring at 1p/1c (`send_with` producers)
    MpscV0,
    /// The MPSC v1 equality-seq ring at 1p/1c (same surface,
    /// runs at depth 1)
    MpscV1,
    /// The MPSC v2 ring of segments at 1p/1c (same surface,
    /// `--segments` per ring, the depth each segment's)
    MpscV2,
    /// All seven, in that order
    All,
}

/// The tp-cell CLI.
#[derive(Parser, Debug)]
#[command(
    name = "tp-cell",
    version = concat!(env!("CARGO_PKG_VERSION"), " (zc-ring-x1 ", env!("ZC_RING_X1_VERSION"), ")"),
    about = TOP_ABOUT,
    max_term_width = 80
)]
struct Cli {
    /// Ring flavor(s) to run
    ///
    /// One cell is a main -> worker -> main round trip over two
    /// rings of the given flavor: main sends a counter on the
    /// request ring, the worker echoes it on the response ring.
    /// Each protocol phase (send, recv, recv spin, recv
    /// attempts, per side) is measured by its own probe and
    /// reported as a percentile band table.
    #[arg(value_enum, default_value_t = FlavorArg::All)]
    flavor: FlavorArg,

    /// Pin main to MAIN and the worker to WORKER (cpu
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

    /// Print a legend after the run explaining the fill counters
    /// line
    #[arg(short = 'v', long)]
    verbose: bool,
}

/// Entry point: banner, run the requested flavors, print the
/// per-probe reports + fill counters line.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    if !cli.verbose {
        println!("-v for a fill counters legend");
    }
    let cfg = cli.common.to_cfg(cli.pin);
    let flavors: &[Flavor] = match cli.flavor {
        FlavorArg::SpscV0 => &[Flavor::SpscV0],
        FlavorArg::SpscV1 => &[Flavor::SpscV1],
        FlavorArg::SpscV2 => &[Flavor::SpscV2],
        FlavorArg::SpscV3 => &[Flavor::SpscV3],
        FlavorArg::MpscV0 => &[Flavor::MpscV0],
        FlavorArg::MpscV1 => &[Flavor::MpscV1],
        FlavorArg::MpscV2 => &[Flavor::MpscV2],
        FlavorArg::All => &FLAVORS,
    };
    for &flavor in flavors {
        for &depth in &cfg.depths {
            if depth < flavor.min_depth() {
                println!(
                    "{} round trip [depth={depth}]: skipped, {}\n",
                    flavor.as_str(),
                    flavor.floor_note()
                );
                continue;
            }
            let res = run_cell(flavor, cfg.duration, cfg.pin, depth, cfg.segments);
            report(flavor.as_str(), &cfg, depth, res.probes);
            if let Some(n) = res.switches {
                println!(
                    "  segment switches: {} ({:.3}/RT)",
                    fmt_commas(n),
                    n as f64 / res.rts.max(1) as f64
                );
            }
            match &res.fills {
                Some(f) => println!(
                    "  fill counters: lcl_cache={} ({:.3} xfills/RT)  lcl_l2={}  lcl_dram={}  [RTs={}]\n",
                    fmt_commas(f.lcl_cache),
                    f.lcl_cache as f64 / res.rts.max(1) as f64,
                    fmt_commas(f.lcl_l2),
                    fmt_commas(f.lcl_dram),
                    fmt_commas(res.rts),
                ),
                None => println!("  fill counters: unavailable\n"),
            }
        }
    }
    if !cli.verbose {
        return;
    }
    print_legend(
        80,
        &[
            ("lcl_cache", "demand fills served from another core's cache"),
            (
                "xfills/RT",
                &format!("{XFILLS_MEANING}, lcl_cache per round trip"),
            ),
            ("lcl_l2", "demand fills served from the core's own L2"),
            ("lcl_dram", "demand fills served from local DRAM"),
            ("RTs", "round trips completed in the duration"),
            (
                "segment switches",
                "spsc-v3 and mpsc-v2 only: switches across both rings, and per round trip",
            ),
        ],
    );
}
