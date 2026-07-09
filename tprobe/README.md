# tprobe

Hardware tick-counter probes: named single-writer histograms
of tick deltas. A local dev crate of the zc-ring-x1 workspace,
built as its own crate so a future extraction is a directory
move.

- `TProbe` — the caller brackets a phase with
  `ticks::read_ticks()` and records the delta; tick→ns
  conversion is deferred to report time. The fit for
  high-rate hot paths.
- `TProbeSpan` — `start(site_id)` / `end(id)` span API over a
  deferred-processing record buffer; preserves record order
  and supports non-stack interleaving, at the cost of a
  growing per-record buffer.
- `ticks` — fixed-rate monotonic counter (x86_64 `rdtsc`,
  aarch64 `CNTVCT_EL0`) with calibration and an invariance
  check.
- Reports render as a percentile band table (min/p1/…/p99/max
  rows; ns by default, raw ticks on request).

## Use

```rust
use tprobe::{TProbe, ticks};

let mut probe = TProbe::new("phase name");
let s = ticks::read_ticks();
// ... the measured phase ...
probe.record(ticks::read_ticks().wrapping_sub(s));
probe.report(false, 1); // ns (true = raw ticks), 1 decimal
```

## Build / test

From the workspace root (or here):

```sh
cargo build -p tprobe
cargo test -p tprobe
```

## Run

tprobe is a library — nothing to install. The workspace's
`tp_matrix` crate exercises it end to end (see the sibling
`tp_runner` crate for the runner half):

```sh
cargo run --release -p tp_matrix --bin tp-cell -- both -d 5 --pin 0,1
```

## Reproducing the measurement matrix

The workspace's SPSC-vs-MPSC measurement tables (see
`notes/chores/chores-02.md`) are one command — every
flavor × placement cell, in-process cache-fill counters,
markdown tables out:

```sh
cargo install --path tp_matrix   # installs tp-cell + tp-matrix
tp-matrix -d 10
```

See [tp_matrix/README.md](../tp_matrix/README.md) for the
flags, the single-cell `tp-cell` tool, and the counter
requirements. Historical note: the 0.13.0 tables in
`notes/chores/chores-02.md` predate `tp-matrix` and were made
with `perf stat` + hand-scraping; the recipe lives in that
file's cycle record, and the commits on its `Commits:` line
reproduce it exactly.
