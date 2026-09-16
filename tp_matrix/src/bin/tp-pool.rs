//! tp-pool: run the pool-message loop over every placement,
//! flavor, ring depth, and pool size, each cell for a duration,
//! and emit one ns-per-message table per placement,
//! the pool sizes as columns and the flavor-by-depth rows, with
//! the fill counters beside it. The descriptor rings sweep
//! their depth, and the cordyceps row, an unbounded intrusive
//! queue, has none.

use std::time::Duration;

use clap::Parser;

use tp_matrix::pool::{POOL_FLAVORS, PoolFlavor, PoolResult, run_pool_cell};
use tp_matrix::{PLACEMENT_MEANING, XFILLS_MEANING, print_legend};
use tp_runner::parse_depth;
use tp_runner::topo::{BaseCpuArg, Placement, discover_placements};

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-pool ",
    env!("CARGO_PKG_VERSION"),
    " (zc-ring-x1 ",
    env!("ZC_RING_X1_VERSION"),
    ")",
    " - run the pool-message sweep, one table per placement"
);

/// The tp-pool CLI.
#[derive(Parser, Debug)]
#[command(
    name = "tp-pool",
    version = concat!(env!("CARGO_PKG_VERSION"), " (zc-ring-x1 ", env!("ZC_RING_X1_VERSION"), ")"),
    about = TOP_ABOUT,
    max_term_width = 80
)]
struct Cli {
    /// Pool sizes (buffers preallocated) to run, comma-separated,
    /// each a column
    ///
    /// The pool bounds the messages in flight, so this is the
    /// axis every flavor shares. Any count from 1 up.
    #[arg(
        long,
        value_name = "LIST",
        value_delimiter = ',',
        default_value = "1,100,1000",
        value_parser = parse_pool
    )]
    pool: Vec<u32>,

    /// Ring depths (slots per ring) for the ring flavors,
    /// comma-separated powers of two, each a row per flavor
    ///
    /// A depth at or above the pool size never reports Full,
    /// so that row is the ring at the pool's bound and the rows
    /// below it are the ring throttling first.
    #[arg(
        long,
        value_name = "LIST",
        value_delimiter = ',',
        default_value = "1,8,64,1024",
        value_parser = parse_depth
    )]
    depth: Vec<u32>,

    /// Wall-clock seconds per cell run
    #[arg(
        short = 'd',
        long = "duration",
        value_name = "SECS",
        default_value_t = 0.1,
        value_parser = parse_duration
    )]
    duration: f64,

    /// Runs per cell, the median reported
    #[arg(long, value_name = "N", default_value_t = 3, value_parser = parse_repeat)]
    repeat: usize,

    #[command(flatten)]
    base: BaseCpuArg,

    /// Print a legend after the last placement explaining every
    /// table and column
    #[arg(short = 'v', long)]
    verbose: bool,
}

/// clap value parser for one `--pool` element: a count from 1
/// up.
fn parse_pool(s: &str) -> Result<u32, String> {
    let n: u32 = s.parse().map_err(|e| format!("{s}: {e}"))?;
    if n == 0 {
        return Err("pool size must be at least 1".to_string());
    }
    Ok(n)
}

/// clap value parser for `--duration`: seconds above zero.
fn parse_duration(s: &str) -> Result<f64, String> {
    let secs: f64 = s.parse().map_err(|e| format!("{s}: {e}"))?;
    if !(secs > 0.0 && secs.is_finite()) {
        return Err("duration must be a number of seconds above 0".to_string());
    }
    Ok(secs)
}

/// clap value parser for `--repeat`: a count from 1 up.
fn parse_repeat(s: &str) -> Result<usize, String> {
    let n: usize = s.parse().map_err(|e| format!("{s}: {e}"))?;
    if n == 0 {
        return Err("repeat must be at least 1".to_string());
    }
    Ok(n)
}

/// One row of the tables: a flavor at a depth (`None` for the
/// depthless cordyceps row).
struct Row {
    flavor: PoolFlavor,
    depth: Option<u32>,
}

/// The rows in table order: each ring flavor at every depth,
/// then the depthless ones.
fn rows(depths: &[u32]) -> Vec<Row> {
    let mut rows = Vec::new();
    for flavor in POOL_FLAVORS {
        if flavor.has_depth() {
            for &depth in depths {
                rows.push(Row {
                    flavor,
                    depth: Some(depth),
                });
            }
        } else {
            rows.push(Row {
                flavor,
                depth: None,
            });
        }
    }
    rows
}

/// Run one cell `repeat` times and keep the median by ns per
/// message, its fill counters with it.
fn median_cell(
    flavor: PoolFlavor,
    pin: Option<(usize, usize)>,
    pool_size: u32,
    depth: u32,
    dur: Duration,
    repeat: usize,
) -> PoolResult {
    let mut runs: Vec<PoolResult> = (0..repeat)
        .map(|_| run_pool_cell(flavor, pin, pool_size, depth, dur))
        .collect();
    runs.sort_by(|a, b| ns_per_msg(a).total_cmp(&ns_per_msg(b)));
    runs.swap_remove(runs.len() / 2)
}

/// Print `rows` as an aligned markdown table under `headers`.
/// The first two columns left-aligned, the rest right-aligned.
/// Returns the table's width in characters.
fn print_table(headers: &[String], rows: &[Vec<String>]) -> usize {
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
    println!("{}", fmt_row(headers));
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

/// A run's elapsed ns over the messages it moved.
fn ns_per_msg(res: &PoolResult) -> f64 {
    res.secs * 1e9 / res.msgs.max(1) as f64
}

/// `xfills/msg` cell: 3 decimals, or 4 when the value is tiny,
/// `-` when counters were unavailable.
fn fills_cell(res: &PoolResult) -> String {
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

/// Entry point: banner, run the sweep, emit the tables, and
/// the legend once after the last placement.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    let placements = discover_placements(cli.base.base_cpu);
    let rows = rows(&cli.depth);
    println!(
        "{} cells, {}s each, median of {} runs{}",
        placements.len() * rows.len() * cli.pool.len(),
        cli.duration,
        cli.repeat,
        if cli.verbose { "" } else { "; -v for a legend" },
    );

    let mut headers = vec!["flavor".to_string(), "depth".to_string()];
    headers.extend(cli.pool.iter().map(|x| format!("pool={x}")));

    let mut width = 0;
    for (i, placement) in placements.iter().enumerate() {
        let Placement { label, pin } = placement;
        // Two blank lines between placements, so one placement's
        // tables stand apart from the next one's progress lines.
        if i > 0 {
            println!();
            println!();
        }
        let mut ns_rows: Vec<Vec<String>> = Vec::new();
        let mut fill_rows: Vec<Vec<String>> = Vec::new();
        for row in &rows {
            let depth_label = row.depth.map_or("-".to_string(), |d| d.to_string());
            let mut ns_row = vec![row.flavor.as_str().to_string(), depth_label.clone()];
            let mut fill_row = ns_row.clone();
            for &pool_size in &cli.pool {
                eprintln!(
                    "pool {label} {} depth {depth_label} pool {pool_size} ...",
                    row.flavor.as_str()
                );
                let res = median_cell(
                    row.flavor,
                    *pin,
                    pool_size,
                    row.depth.unwrap_or(1),
                    Duration::from_secs_f64(cli.duration),
                    cli.repeat,
                );
                ns_row.push(format!("{:.1}", ns_per_msg(&res)));
                fill_row.push(fills_cell(&res));
            }
            ns_rows.push(ns_row);
            fill_rows.push(fill_row);
        }
        println!();
        println!("{label}: ns/msg");
        println!();
        width = width.max(print_table(&headers, &ns_rows));
        println!();
        println!("{label}: xfills/msg");
        println!();
        width = width.max(print_table(&headers, &fill_rows));
    }
    if !cli.verbose {
        return;
    }
    println!();
    print_legend(
        width,
        &[
            (
                "<placement>",
                &format!("the prefix of each table title, {PLACEMENT_MEANING}"),
            ),
            (
                "<placement>: ns/msg",
                "a table whose cells are elapsed ns over messages moved at the row's flavor and depth and the column's pool size, the median of the runs",
            ),
            (
                "<placement>: xfills/msg",
                &format!(
                    "a table of the same cells' {XFILLS_MEANING}, per message, from the same median run"
                ),
            ),
            (
                "flavor",
                "the queue the pool's messages cross: a descriptor ring carrying a Desc, or cordyceps's intrusive queue linked through the pool's own buffers",
            ),
            (
                "depth",
                "slots in the ring, `-` for cordyceps, which is unbounded. A depth at or above the pool size never reports Full",
            ),
            (
                "pool=N",
                "the pool's buffer count, the bound on messages in flight",
            ),
        ],
    );
}
