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
phase-probed example exercises it end to end (see the sibling
`tp_runner` crate for the runner half):

```sh
cargo run --release --example tp_roundtrip -- both -d 5 --pin 0,1
```

## Reproducing the measurement matrix

The workspace's SPSC-vs-MPSC measurement tables (see
`notes/chores/chores-02.md`) come from running the example
per flavor × placement under `perf stat`, so one run yields
both the phase histograms and the cross-core cache-fill
counters:

```sh
cargo build --release --examples
BIN=target/release/examples/tp_roundtrip
EV=ls_refills_from_sys.ls_mabresp_lcl_cache,\
ls_refills_from_sys.ls_mabresp_lcl_l2,\
ls_refills_from_sys.ls_mabresp_lcl_dram
for flavor in spsc mpsc; do
  for pin in 0,1 0,3 0,12 none; do
    if [ "$pin" = none ]; then
      perf stat -e "$EV" -- "$BIN" "$flavor" -d 10 --decimals 3 \
        > "$flavor-pin-${pin/,/}.txt" 2>&1
    else
      perf stat -e "$EV" -- "$BIN" "$flavor" -d 10 --decimals 3 \
        --pin "$pin" > "$flavor-pin-${pin/,/}.txt" 2>&1
    fi
  done
done
```

Notes on the pieces:

- `ls_refills_from_sys.*` are AMD Zen 2 events;
  `ls_mabresp_lcl_cache` counts demand fills served from
  another core's cache — the cross-core line-transfer
  signal. On another microarchitecture substitute its
  equivalent (check `perf list`).
- Pick placements from your topology:
  `cat /sys/devices/system/cpu/cpu0/cache/index3/shared_cpu_list`
  shows cpu0's L3 (CCX) group, and
  `.../cpu0/topology/thread_siblings_list` its SMT sibling —
  choose a same-L3 pair, a cross-L3 pair, and the sibling
  pair.
- Table cells: each probe's `mean min-p99` / `stdev min-p99`
  report lines are the `mean/stdev` cells; the probe header's
  `count=` is the round trips (`RTs`); `fills/RT` =
  `ls_mabresp_lcl_cache` ÷ round trips.
- The spin/attempts probes cost ~20% throughput and
  slightly inflate the phase means (the waits absorb the
  peer's instrumentation). The chores Measurements table
  was produced by the example *before* the spin probes
  landed (its commit is on the section's `Commits:` line);
  reproducing it exactly means checking out that commit —
  at HEAD every run includes the spin decomposition.
