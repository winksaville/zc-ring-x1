# tp_matrix

Measure what a cross-thread message handoff over the
zc-ring-x1 ring queues actually costs — and where the cost
lives — with two installable binaries: `tp-cell` and
`tp-matrix`.

## The measurement, in one paragraph

Both tools run the same experiment, a **cell**: a main thread
sends a counter to a worker over one ring, the worker echoes
it back over a second ring, as fast as the two threads can go
for a fixed duration (one message in flight, so every trip is
a fresh handoff). Every protocol phase is bracketed by two
hardware tick-counter reads and recorded into its own
histogram: the sends (`reserve + fill + commit` — the
producer's cost of placing a message), the recvs (spin wait
for arrival + read + release), and inside each recv the spin
wait itself plus how many polls it took. On Linux the process
also counts its own cross-core cache-line fills via
`perf_event_open` (per-process, worker threads inherited,
user-mode only — no perf(1), root, bash, or scraping), which
is the hardware's answer to "how many cache lines crossed
between the cores per round trip". A cell varies along two
axes: **flavor** (the SPSC ring vs the MPSC sibling at 1p/1c)
and **placement** (which CPUs the two threads sit on — same
L3, different L3, SMT siblings, or unpinned).

## tp-matrix — the whole picture, one command

Runs *every* flavor × placement cell (placements discovered
from `/sys` CPU topology) and prints two markdown tables
ready to paste into notes:

- **Phase costs** — per cell: `m.send`, `w.recv`, `w.send`,
  `m.recv` (each `mean/stdev` of the trimmed min-p99 band),
  round trips completed, and `fills/RT`.
- **Spin decomposition** — the wait inside each recv:
  `spin` (first failed poll → message visible) and `att`
  (polls per waiting reserve), per side, plus `fills/RT`.

```sh
$ tp-matrix -d 10          # 8 cells x 10 s on a typical SMT machine
tp-matrix 0.1.0 - run the full measurement matrix, markdown tables out
...
| placement | flavor |   m.send |     w.recv | ... |  RTs | fills/RT |
|-----------|--------|---------:|-----------:|-----|-----:|---------:|
| 0,1 CCX   | spsc   | 22.3/6.0 | 132.5/13.9 | ... | 3.7M |   10.120 |
| 0,1 CCX   | mpsc   |  9.5/4.2 |  95.5/19.6 | ... | 5.2M |    6.617 |
```

This is the tool that answers "which flavor is faster here,
and why": e.g. on a Zen 2 the SPSC ring moves ~10 cache lines
per round trip to the MPSC ring's ~6.7 and loses cross-core —
but wins on SMT siblings where no lines cross (see
`notes/chores/chores-02.md` for the full analysis).

## tp-cell — one cell, under the microscope

Runs a single placement (your `--pin` choice, or unpinned)
and prints the *full* per-probe percentile band tables that
the matrix summarizes to `mean/stdev` — min/p1/…/p99/max rows
with first/last/range/count/mean columns — plus the raw fill
counters:

```sh
$ tp-cell spsc -d 5 --pin 0,1
tp-cell 0.1.0 - run one phase-probed ring round-trip cell
spsc round trip [duration=5.0s pin=main=0,worker=1]:
  tprobe: spsc main send (reserve+commit) [count=21,078,016]
    ...band rows...
  ...seven more probes, trip order...
  fills: lcl_cache=209,239,903 (9.927/RT)  lcl_l2=249,403  ...
```

Use it when a matrix row looks odd and you want the shape of
the distribution (bimodality, tail weight), or to A/B one
placement while changing something.

## Build / test / install

```sh
cargo build -p tp_matrix
cargo test --workspace
cargo install --path tp_matrix   # installs tp-cell + tp-matrix
```

`-h` for a summary of the flags, `--help` for details; both
print the `name version - tagline` banner first, as does
every run (so saved output identifies the build it came
from).

## Requirements and caveats

- Fill counters need `kernel.perf_event_paranoid ≤ 2` (the
  usual default — self-profiling only). Without them the
  tools still run; `fills` reports unavailable.
- The fill events are AMD Zen 2 raw encodings, A/B-verified
  against `perf stat`; other microarchitectures need their
  own encodings in `tp_runner::perf`.
- Non-Linux builds run unpinned without counters.
- The crates: probes are `tprobe`, generic runner machinery
  (CLI, pinning, drive loop, perf, topology) is `tp_runner`;
  this crate holds only the ring-aware cells and the two
  binaries.
