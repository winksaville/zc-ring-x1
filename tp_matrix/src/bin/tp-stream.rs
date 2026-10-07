//! tp-stream: run the streaming cell over every flavor,
//! placement, and depth, and emit one markdown table: ns per
//! message, messages moved, and xfills per message, with a
//! column legend. The streaming sibling of `tp-matrix`, whose cell
//! keeps one message in flight and so cannot show what a
//! full ring costs per message.

use clap::Parser;

use tp_matrix::{
    FLAVORS, Flavor, MAX_PRODUCERS, PLACEMENT_MEANING, StreamPins, StreamResult, XFILLS_MEANING,
    print_legend, run_stream,
};
use tp_runner::topo::{BaseCpuArg, discover_multi_placements, discover_placements};
use tp_runner::{Cfg, CommonArgs};

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-stream ",
    env!("CARGO_PKG_VERSION"),
    " (zc-ring-x1 ",
    env!("ZC_RING_X1_VERSION"),
    ")",
    " - run the streaming matrix, one markdown table out"
);

/// The tp-stream CLI.
#[derive(Parser, Debug)]
#[command(
    name = "tp-stream",
    version = concat!(env!("CARGO_PKG_VERSION"), " (zc-ring-x1 ", env!("ZC_RING_X1_VERSION"), ")"),
    about = TOP_ABOUT,
    max_term_width = 80
)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    base: BaseCpuArg,

    /// Producer threads per MPSC cell, 1 to 64
    ///
    /// Above 1, only the MPSC flavors run, the SPSC ones skipped,
    /// and the producers contend for the claim word. Every thread
    /// has a cpu of its own: the consumer on the base cpu, and the
    /// producers each on a core of their own near the base (own
    /// cores near), outside the base's L3 (own cores x-L3), or two
    /// to a core on both of its cpus (shared cores), or all
    /// unpinned. The cpus are printed above the table, and a
    /// placement the machine cannot give N is skipped with a note.
    #[arg(
        short = 'p',
        long,
        value_name = "N",
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..=MAX_PRODUCERS as i64)
    )]
    producers: u32,

    /// Print a legend under the table explaining every column
    #[arg(short = 'v', long)]
    verbose: bool,
}

/// `xfills/msg` cell: 3 decimals, or 4 when the value is tiny
/// (the SMT cells), `-` when counters were unavailable.
/// `switches/msg` cell for the segmented flavor, `-` for the
/// single-region ones.
fn switches_cell(res: &StreamResult) -> String {
    match res.switches {
        Some(n) => format!("{:.3}", n as f64 / res.msgs.max(1) as f64),
        None => "-".to_string(),
    }
}

/// A share of a side's operations as a percent, 1 decimal, or 3
/// under 0.1 so a rare wait still shows.
fn percent_cell(part: u64, whole: u64) -> String {
    let v = 100.0 * part as f64 / whole.max(1) as f64;
    if v > 0.0 && v < 0.1 {
        format!("{v:.3}")
    } else {
        format!("{v:.1}")
    }
}

fn fills_cell(res: &StreamResult) -> String {
    match &res.fills {
        Some(f) => {
            let v = f.lcl_cache as f64 / res.msgs.max(1) as f64;
            if v < 0.01 {
                format!("{v:.4}")
            } else {
                format!("{v:.3}")
            }
        }
        None => "-".to_string(),
    }
}

/// A cpu for the placement lines, `-` for unpinned.
fn cpu_cell(cpu: Option<usize>) -> String {
    cpu.map_or("-".to_string(), |c| c.to_string())
}

/// Print `rows` as an aligned markdown table under `headers`.
/// The first two columns left-aligned, the rest right-aligned.
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
    // Each placement's label and its threads' cpus: one producer
    // takes the two-thread placements, several the ones that name
    // every thread.
    let placements: Vec<(String, StreamPins)> = if cli.producers == 1 {
        discover_placements(cli.base.base_cpu)
            .into_iter()
            .map(|p| (p.label, StreamPins::pair(p.pin)))
            .collect()
    } else {
        let (placements, skipped) =
            discover_multi_placements(cli.base.base_cpu, cli.producers as usize);
        for note in skipped {
            println!("skipping {note}");
        }
        placements
            .into_iter()
            .map(|p| {
                println!(
                    "{}: consumer {}, producers {}",
                    p.label,
                    cpu_cell(p.consumer),
                    p.producers
                        .iter()
                        .map(|&c| cpu_cell(c))
                        .collect::<Vec<_>>()
                        .join(",")
                );
                (
                    p.label,
                    StreamPins {
                        consumer: p.consumer,
                        producers: p.producers,
                    },
                )
            })
            .collect()
    };
    let flavors: Vec<Flavor> = FLAVORS
        .into_iter()
        .filter(|f| cli.producers == 1 || f.is_mpsc())
        .collect();
    println!(
        "{} cells, {:.1}s each, {}, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 with {} segments{}",
        placements.len() * flavors.len() * cfg.depths.len(),
        cfg.duration.as_secs_f64(),
        if cli.producers == 1 {
            "1 producer".to_string()
        } else {
            format!("{} producers, the MPSC flavors only", cli.producers)
        },
        cfg.segments,
        if cli.verbose {
            ""
        } else {
            "; -v for a column legend"
        },
    );

    let mut cells: Vec<(&str, Flavor, u32, StreamResult)> = Vec::new();
    for (label, pins) in &placements {
        for &flavor in &flavors {
            for &depth in &cfg.depths {
                if depth < flavor.min_depth() {
                    eprintln!(
                        "skipping {label} {} depth {depth}: {}",
                        flavor.as_str(),
                        flavor.floor_note()
                    );
                    continue;
                }
                eprintln!("streaming {label} {} depth {depth} ...", flavor.as_str());
                let res = run_stream(flavor, cfg.duration, pins, depth, cfg.segments);
                cells.push((label, flavor, depth, res));
            }
        }
    }

    let rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(p, f, d, r)| {
            vec![
                p.to_string(),
                f.as_str().to_string(),
                d.to_string(),
                format!("{:.1}", r.secs * 1e9 / r.msgs.max(1) as f64),
                format!("{:.1}M", r.msgs as f64 / 1e6),
                fills_cell(r),
                switches_cell(r),
                percent_cell(r.waits.full, r.waits.sends),
                percent_cell(r.waits.empty, r.waits.reads),
            ]
        })
        .collect();
    println!();
    let width = print_table(
        &[
            "placement",
            "flavor",
            "depth",
            "ns/msg",
            "msgs",
            "xfills/msg",
            "switches/msg",
            "full %",
            "empty %",
        ],
        &rows,
    );
    if !cli.verbose {
        return;
    }
    println!();
    print_legend(
        width,
        &[
            (
                "placement",
                &format!(
                    "{PLACEMENT_MEANING}. With several producers every thread has a cpu of its own, the consumer on the base cpu: own cores near puts each producer on a core of its own, the base's L3 first, own cores x-L3 each outside the base's L3, and shared cores two producers to a core, the cpus printed above the table"
                ),
            ),
            ("flavor", "the ring the producer streams over"),
            (
                "depth",
                "slots in the ring, per segment for spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4, the slack the producer can run ahead of the consumer by before a segmented ring switches segments",
            ),
            (
                "ns/msg",
                "elapsed ns over messages moved, the producers streaming as fast as the ring admits for the duration and the consumer draining and checking each producer's order",
            ),
            ("msgs", "messages moved in the duration, in millions"),
            ("xfills/msg", &format!("{XFILLS_MEANING}, per message")),
            (
                "switches/msg",
                "segment switches per message, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 only: how often the producer, running ahead, found its segment about to be full (spsc-v3 and v4) or full (mpsc-v2, v3, and v4) and moved to another",
            ),
            (
                "full %",
                "the sends that found the ring full at least once, every producer's, as a percent of all sends",
            ),
            (
                "empty %",
                "the reads that found the ring empty at least once, as a percent of all reads",
            ),
        ],
    );
}
