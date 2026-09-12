# tp_matrix

Measure what a cross-thread message handoff over the
zc-ring-x1 ring queues actually costs (and where the cost
lives), with four installable binaries: `tp-cell`,
`tp-matrix`, `tp-stream`, and `tp-pool`.

## The measurement, in one paragraph

Both tools run the same experiment, a **cell**: a main thread
sends a counter to a worker over one ring, the worker echoes
it back over a second ring, as fast as the two threads can go
for a fixed duration (one message in flight, so every trip is
a fresh handoff). Every protocol phase is bracketed by two
hardware tick-counter reads and recorded into its own
histogram: the sends (`reserve + fill + commit`, the
producer's cost of placing a message), the recvs (spin wait
for arrival + read + release), and inside each recv the spin
wait itself plus how many polls it took. On Linux the process
also counts its own cross-core cache-line fills via
`perf_event_open` (per-process, worker threads inherited,
user-mode only, no perf(1), root, bash, or scraping), which
is the hardware's answer to "how many cache lines crossed
between the cores per round trip". A cell varies along two
axes: **flavor** (the SPSC v0 ring, the SPSC v1 seam-word
ring, the SPSC v2 in-slot seq ring, and the MPSC v0 and v1
siblings at 1p/1c, every flavor named `xpsc-vN` after its
module path) and **placement** (which CPUs the two threads sit on, same
L3, different L3, SMT siblings, or unpinned).

## tp-matrix: the whole picture, one command

Runs *every* flavor × placement cell (placements discovered
from `/sys` CPU topology) and prints two markdown tables
ready to paste into notes:

- **Phase costs**, per cell: `m.send`, `w.recv`, `w.send`,
  `m.recv` (each `mean/stdev` of the trimmed min-p99 band),
  round trips completed, and `fills/RT`.
- **Spin decomposition**, the wait inside each recv:
  `spin` (first failed poll -> message visible) and `att`
  (polls per waiting reserve), per side, plus `fills/RT`.

```sh
$ tp-matrix -d 10                  # 20 cells x 10 s on a typical SMT machine, depth 8
$ tp-matrix -d 5 --depth 1,2,8,64  # every cell again at each depth
tp-matrix 0.1.0 - run the full measurement matrix, markdown tables out
...
| placement | flavor  | depth |   m.send |     w.recv | ... |  RTs | fills/RT |
|-----------|---------|------:|---------:|-----------:|-----|-----:|---------:|
| 0,1 CCX   | spsc-v0 |     8 | 22.3/6.0 | 132.5/13.9 | ... | 3.7M |   10.120 |
| 0,1 CCX   | mpsc-v0 |     8 |  9.5/4.2 |  95.5/19.6 | ... | 5.2M |    6.617 |
```

`--depth` takes a comma-separated list of ring depths (slots
per ring, powers of two from 1 up), the default `8`, and each
cell repeats per depth. One message is ever in flight, so the
depth changes how many seq words share a line and, at 1,
whether the ring has any slack. The MPSC v0 ring rejects
depth 1, where its protocol wedges (the design note's "MPSC
v1: equality-seq ring"), so its cells there are skipped with a
note.

This is the tool that answers "which flavor is faster here,
and why": e.g. on a Zen 2 the SPSC ring moves ~10 cache lines
per round trip to the MPSC ring's ~6.7 and loses cross-core,
but wins on SMT siblings where no lines cross (see
`notes/chores/chores-02.md` for the full analysis).

## tp-stream: the streaming matrix

The round-trip cell keeps one message in flight, so it cannot
say what a ring costs per message when the producer runs ahead
and the ring holds many. `tp-stream` runs the other shape over
the same flavors, placements, and depths: a producer thread
streams a counter as fast as the ring admits for the duration,
a consumer thread drains and checks the order, and the fill
counters divided by the messages moved give the cross-core
line fills per message while streaming.

```sh
$ tp-stream -d 5 --depth 1,2,8,64
tp-stream 0.1.0 - run the streaming matrix, one markdown table out
...
| placement | flavor  | depth | ns/msg |   msgs | fills/msg |
|-----------|---------|------:|-------:|-------:|----------:|
| 0,3 x-CCX | spsc-v1 |    64 |   37.4 | 133.8M |     0.491 |
| 0,3 x-CCX | spsc-v2 |    64 |   14.3 | 350.1M |     0.132 |
```

Two things the streaming number is sensitive to, found while
building the cell and worth knowing before comparing runs:

- The wait policy's inlining: the cell uses the ring crate's
  `policy::spin`, which is `#[inline]`, and an out-of-line
  spin from another crate moved the v2 cross-CCX line from 14
  to 31 ns per message. A poll's cost sets how soon a side
  re-reads a line the other side is writing.
- The producer's loop shape: the cell checks the clock every
  4096 sends, and that check alone moves the v1 cross-CCX line
  from 104 ns per message (1.8 fills) in a plain counted loop
  to about 40 (0.6 fills). v1 is bistable there: the two sides
  either write its packed seq line in lockstep or the producer
  runs ahead in bursts, and a periodic hiccup on the producer
  tips it into the second. v0 and v2 read the same in both
  loop shapes. The demo's stream lines are the plain loop.

## tp-pool: the pool-message sweep

The two matrices above write the payload into the ring's slot.
The messaging layer's loop is the other shape: take a message
from the pool, fill it, push its reference, receive it,
process it, return it to the pool. `tp-pool` runs that loop a
fixed count of messages per cell over the descriptor rings,
`spsc-v2` and `mpsc-v1` carrying a `Desc`, and over cordyceps's
`MpscQueue`, Vyukov's intrusive MPSC, linked through the same
pool's buffers, so the queue is the only variable between the
rows. The pool bounds the messages in flight, so its size is
the axis every flavor shares, the columns. A ring depth at or
above the pool size never reports Full, so that row is the
ring at the pool's bound and the rows below it are the ring
throttling first, and the cordyceps row, unbounded, has no
depth.

```sh
$ tp-pool                                  # pools 1,100,1000; depths 1,8,64,1024; 1M messages; median of 3
$ tp-pool --pool 1,10,100 --depth 1,1024 --count 200000 --repeat 5
tp-pool 0.1.0 - run the pool-message sweep, one table per placement
...
0,3 x-CCX: ns/msg

| flavor    | depth | pool=1 | pool=100 | pool=1000 |
|-----------|-------|-------:|---------:|----------:|
| spsc-v2   | 1024  |  504.2 |    214.9 |     215.0 |
| cordyceps | -     |  615.0 |    213.7 |     217.2 |
```

Each placement gets an `ns/msg` table and a `fills/msg` table
of the same shape, and a line with the `Inconsistent` results
the cordyceps consumer retried in the median run, the window
between a producer's head swap and its link store. The pool's
free-stack is in every row: the consumer's free pushes the
buffer it just read, and the producer's alloc pops that same
buffer, so a line the consumer wrote crosses back on every
message whatever the queue does, which is why these numbers sit
far above the in-slot tables' at the same placement.

## tp-cell: one cell, under the microscope

Runs a single placement (your `--pin` choice, or unpinned)
and prints the *full* per-probe percentile band tables that
the matrix summarizes to `mean/stdev` (min/p1/.../p99/max
rows with first/last/range/count/mean columns), plus the raw
fill counters:

```sh
$ tp-cell spsc-v0 -d 5 --pin 0,1
tp-cell 0.1.0 - run one phase-probed ring round-trip cell
spsc-v0 round trip [duration=5.0s pin=main=0,worker=1]:
  tprobe: spsc-v0 main send (reserve+commit) [count=21,078,016]
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
cargo install --path tp_matrix --locked   # installs tp-cell, tp-matrix, tp-stream, tp-pool
```

`--locked` builds from the committed `Cargo.lock`, so a saved
run's banner names a build another machine can reproduce.

`-h` for a summary of the flags, `--help` for details. Both
print the `name version - tagline` banner first, as does
every run (so saved output identifies the build it came
from).

## Requirements and caveats

- Fill counters need `kernel.perf_event_paranoid ≤ 2` (the
  usual default, self-profiling only). Without them the
  tools still run, and `fills` reports unavailable.
- The fill events are AMD Zen 2 raw encodings, A/B-verified
  against `perf stat`. Other microarchitectures need their
  own encodings in `tp_runner::perf`.
- Non-Linux builds run unpinned without counters.
- The crates: probes are `tprobe`, generic runner machinery
  (CLI, pinning, drive loop, perf, topology) is `tp_runner`.
  This crate holds only the ring-aware cells and the four
  binaries.
