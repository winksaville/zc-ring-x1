//! tp-stream: run the streaming cell over every flavor,
//! placement, and depth, and emit one markdown table: ns per
//! message, messages moved, and cross-core cache-line fills per
//! message. The streaming sibling of `tp-matrix`, whose cell
//! keeps one message in flight and so cannot show what a
//! full ring costs per message.

use clap::Parser;

use tp_matrix::{FLAVORS, Flavor, StreamResult, run_stream};
use tp_runner::topo::{Placement, discover_placements};
use tp_runner::{Cfg, CommonArgs};

/// Banner: name, version, and tagline on one line, the first
/// line of every run and of `-h`/`--help`.
const TOP_ABOUT: &str = concat!(
    "tp-stream ",
    env!("CARGO_PKG_VERSION"),
    " - run the streaming matrix, one markdown table out"
);

/// The tp-stream CLI.
#[derive(Parser, Debug)]
#[command(name = "tp-stream", version, about = TOP_ABOUT, max_term_width = 80)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,
}

/// `fills/msg` cell: 3 decimals, or 4 when the value is tiny
/// (the SMT cells); `-` when counters were unavailable.
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

/// Entry point: banner, run the matrix, emit the table.
fn main() {
    let cli = Cli::parse();
    println!("{TOP_ABOUT}");
    let cfg: Cfg = cli.common.to_cfg(None);
    let placements = discover_placements();
    println!(
        "{} cells, {:.1}s each; ns/msg = elapsed over messages moved; fills/msg = cross-core \
         cache-line fills per message",
        placements.len() * FLAVORS.len() * cfg.depths.len(),
        cfg.duration.as_secs_f64(),
    );
    println!();

    let mut cells: Vec<(&Placement, Flavor, u32, StreamResult)> = Vec::new();
    for placement in &placements {
        for flavor in FLAVORS {
            for &depth in &cfg.depths {
                if depth < flavor.min_depth() {
                    eprintln!(
                        "skipping {} {} depth {depth}: below the flavor's floor",
                        placement.label,
                        flavor.as_str()
                    );
                    continue;
                }
                eprintln!(
                    "streaming {} {} depth {depth} ...",
                    placement.label,
                    flavor.as_str()
                );
                let res = run_stream(flavor, cfg.duration, placement.pin, depth);
                cells.push((placement, flavor, depth, res));
            }
        }
    }

    let rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(p, f, d, r)| {
            vec![
                p.label.clone(),
                f.as_str().to_string(),
                d.to_string(),
                format!("{:.1}", r.secs * 1e9 / r.msgs.max(1) as f64),
                format!("{:.1}M", r.msgs as f64 / 1e6),
                fills_cell(r),
            ]
        })
        .collect();
    println!("Streaming costs (producer streams for the duration, consumer drains):");
    println!();
    print_table(
        &[
            "placement",
            "flavor",
            "depth",
            "ns/msg",
            "msgs",
            "fills/msg",
        ],
        &rows,
    );
}
