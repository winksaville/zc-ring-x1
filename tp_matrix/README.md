# tp_matrix

Phase-probed 1p/1c round-trip measurement cells over the
zc-ring-x1 ring primitives, with in-process cache-fill
counters — and the two binaries that drive them:

- **`tp-matrix`** — runs every flavor × placement cell
  (spsc/mpsc × same-L3, cross-L3, SMT, unpinned; discovered
  from `/sys` topology) and emits two markdown tables ready
  to paste: phase costs and the spin decomposition, each row
  with `fills/RT` (cross-core cache-line fills per round
  trip).
- **`tp-cell`** — runs one cell and prints the full per-probe
  band-table reports plus a `fills` line; the tool for
  looking closely at a single placement.

The probes are the sibling `tprobe` crate; the runner (CLI,
pinning, drive loop, perf counters, topology) is `tp_runner`.
Counters use `perf_event_open` directly (per-process,
`inherit`, user-mode only) — no perf(1), bash, or scraping
involved. Non-Linux builds run but report fills as
unavailable.

## Build / test / install

```sh
cargo build -p tp_matrix
cargo test --workspace
cargo install --path tp_matrix   # installs tp-cell + tp-matrix
```

## Run

```sh
tp-matrix -d 10                  # full matrix, 10 s per cell
tp-cell both -d 5 --pin 0,1      # one placement, full reports
tp-cell spsc -d 5 --pin 0,12 --decimals 3
```

Flags: `-d/--duration <secs>` (per cell), `--pin main,worker`
(tp-cell only), `-t/--ticks` (raw ticks instead of ns),
`--decimals <n>` (default 1).

Counter access needs `kernel.perf_event_paranoid ≤ 2` (the
usual default — self-profiling only). The fill events are AMD
Zen 2 raw encodings, A/B-verified against `perf stat`; other
microarchitectures need their own encodings in
`tp_runner::perf`.
