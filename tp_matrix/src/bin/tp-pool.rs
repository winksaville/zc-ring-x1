//! tp-pool: run the pool-message loop over every placement,
//! flavor, ring depth, and pool size, a fixed count of messages
//! per cell, and emit one ns-per-message table per placement,
//! the pool sizes as columns and the flavor-by-depth rows, with
//! the fill counters beside it. The descriptor rings sweep
//! their depth, and the cordyceps row, an unbounded intrusive
//! queue, has none.

use clap::Parser;

use tp_matrix::pool::{POOL_FLAVORS, PoolFlavor, PoolResult, run_pool_cell};
use tp_runner::parse_depth;
use tp_runner::topo::{Placement, discover_placements};

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-pool ",
    env!("CARGO_PKG_VERSION"),
    " - run the pool-message sweep, one table per placement"
);

/// The tp-pool CLI.
#[derive(Parser, Debug)]
#[command(name = "tp-pool", version, about = TOP_ABOUT, max_term_width = 80)]
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

    /// Messages per cell
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    count: u64,

    /// Runs per cell, the median reported
    #[arg(long, value_name = "N", default_value_t = 3, value_parser = parse_repeat)]
    repeat: usize,
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

/// Run one cell `repeat` times and keep the median by elapsed
/// time, its fill counters with it.
fn median_cell(
    flavor: PoolFlavor,
    pin: Option<(usize, usize)>,
    pool_size: u32,
    depth: u32,
    count: u64,
    repeat: usize,
) -> PoolResult {
    let mut runs: Vec<PoolResult> = (0..repeat)
        .map(|_| run_pool_cell(flavor, pin, pool_size, depth, count))
        .collect();
    runs.sort_by(|a, b| a.secs.total_cmp(&b.secs));
    runs.swap_remove(runs.len() / 2)
}

/// Print `rows` as an aligned markdown table under `headers`.
/// The first two columns left-aligned, the rest right-aligned.
fn print_table(headers: &[String], rows: &[Vec<String>]) {
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
}

/// `fills/msg` cell: 3 decimals, or 4 when the value is tiny,
/// `-` when counters were unavailable.
fn fills_cell(res: &PoolResult, count: u64) -> String {
    match &res.fills {
        Some(f) => {
            let v = f.lcl_cache as f64 / count.max(1) as f64;
            if v < 0.01 {
                format!("{v:.4}")
            } else {
                format!("{v:.3}")
            }
        }
        None => "-".to_string(),
    }
}

/// Entry point: banner, run the sweep, emit the tables.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    let placements = discover_placements();
    let rows = rows(&cli.depth);
    println!(
        "{} cells, {} messages each, median of {} runs; ns/msg = elapsed over messages; \
         fills/msg = cross-core cache-line fills per message",
        placements.len() * rows.len() * cli.pool.len(),
        cli.count,
        cli.repeat,
    );

    let mut headers = vec!["flavor".to_string(), "depth".to_string()];
    headers.extend(cli.pool.iter().map(|x| format!("pool={x}")));

    for placement in &placements {
        let Placement { label, pin } = placement;
        let mut ns_rows: Vec<Vec<String>> = Vec::new();
        let mut fill_rows: Vec<Vec<String>> = Vec::new();
        let mut inconsistent: Vec<String> = Vec::new();
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
                    cli.count,
                    cli.repeat,
                );
                ns_row.push(format!("{:.1}", res.secs * 1e9 / cli.count.max(1) as f64));
                fill_row.push(fills_cell(&res, cli.count));
                if row.flavor == PoolFlavor::Cordyceps {
                    inconsistent.push(format!("pool={pool_size}: {}", res.inconsistent));
                }
            }
            ns_rows.push(ns_row);
            fill_rows.push(fill_row);
        }
        println!();
        println!("{label}: ns/msg");
        println!();
        print_table(&headers, &ns_rows);
        println!();
        println!("{label}: fills/msg");
        println!();
        print_table(&headers, &fill_rows);
        println!();
        println!(
            "{label}: cordyceps Inconsistent retries in the median run, {}",
            inconsistent.join(", ")
        );
    }
}
