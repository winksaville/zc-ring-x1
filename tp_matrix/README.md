# tp_matrix

Measure what a cross-thread message handoff over the
zc-ring-x1 ring queues actually costs (and where the cost
lives), with four installable binaries: `tp-cell`,
`tp-matrix`, `tp-stream`, and `tp-pool`.

## The measurements

Each tool moves messages between two threads over the rings
and times it: `tp-cell` and `tp-matrix` time each step with
hardware tick counters, and `tp-stream` and `tp-pool` time
the whole run. On Linux each tool also counts xfills,
cache lines one core had to pull from another core's cache,
through `perf_event_open`, with no perf(1) or root needed.
SMT siblings share one core's caches, so there xfills reads
near 0, which is expected.
Runs vary by **flavor**, which ring (`spsc-v0`, `spsc-v1`,
`spsc-v2`, `spsc-v3`, `mpsc-v0`, `mpsc-v1`, `mpsc-v2`, named
after their module paths), and by **placement**, which CPUs
the two threads sit on: same L3, different L3, SMT siblings,
or unpinned. `spsc-v3` and `mpsc-v2` are rings of segments:
`--segments N`, 1 to 32 and default 2, sets how many per ring,
the depth is each segment's, and their rows add how often they
switched segments, `switches/RT` in `tp-matrix` and
`switches/msg` in `tp-stream`, `-` for every other flavor.

- `tp-cell`: one round trip, main sends a counter to a worker
  and the worker sends it back, for one ring and one
  placement. Prints the full timing distribution of every
  step.
- `tp-matrix`: the same round trip for every ring at every
  placement, one table of mean/stdev per step.
- `tp-stream`: one thread streams counters as fast as the ring
  takes them and the other drains, so the ring fills up.
  Reports ns and xfills per message.
- `tp-pool`: the messaging layer's loop, take a buffer from a
  pool, send it, receive it, free it, over two rings and
  cordyceps. Reports ns and xfills per message by pool size.

## The tools side by side

Each tool answers a different question, so their numbers are
not comparable one to one.

| | `tp-cell` | `tp-matrix` | `tp-stream` | `tp-pool` |
|---|---|---|---|---|
| Question | what one cell's timing looks like | which ring is faster here, and why | what a ring costs when it fills | what the pool loop costs per queue |
| Shape | round trip, two rings | round trip, two rings | one way, one ring | one way, pool + queue |
| In flight | 1 | 1 | up to depth | up to pool size |
| Payload | counter in the slot | counter in the slot | counter in the slot | pool buffer, its `Desc` or pointer crosses |
| Depth | seq sharing, slack at 1 | seq sharing, slack at 1 | how far the producer can run ahead | throttles when below the pool size |
| Runs | `-d` per cell | `-d` per cell | `-d` per cell | `-d` per run, median of `--repeat` |
| Reports | full percentile bands per phase | mean/stdev per phase, RTs, xfills/RT | ns/msg, msgs, xfills/msg | ns/msg and xfills/msg per pool size |
| Flavors | the seven rings | the seven rings | the seven rings | spsc-v2, mpsc-v1, cordyceps |

## tp-cell: one cell, under the microscope

Runs a single placement (your `--pin` choice, or unpinned)
and prints the *full* per-probe percentile band tables that
the matrix summarizes to `mean/stdev` (min/p1/.../p99/max
rows with first/last/range/count/mean columns), plus the raw
fill counters:

```sh
$ tp-cell spsc-v0 -d 5 --pin 0,1 -v
tp-cell 0.1.0 - run one phase-probed ring round-trip cell
spsc-v0 round trip [duration=5.0s pin=main=0,worker=1]:
  tprobe: spsc-v0 main send (reserve+commit) [count=21,078,016]
    ...band rows...
  ...seven more probes, trip order...
  fill counters: lcl_cache=209,239,903 (9.927 xfills/RT)  lcl_l2=249,403  ...

- `lcl_cache`: demand fills served from another core's cache
...
```

Use it when a matrix row looks odd and you want the shape of
the distribution (bimodality, tail weight), or to A/B one
placement while changing something.

## tp-matrix: the whole picture, one command

Runs *every* flavor × placement cell (placements discovered
from `/sys` CPU topology) and prints one markdown table ready
to paste into notes, a row per cell with its columns in trip
order: `m.send`, then `w.recv` with the `w.spin` and `w.att`
inside it, `w.send`, then `m.recv` with `m.spin` and `m.att`,
then round trips completed, `xfills/RT`, and `switches/RT`. The phase and
spin cells are `mean/stdev` of the trimmed min-p99 band.

Every tool takes `-v` (`--verbose`), which adds a legend under
its table explaining each column, a markdown list wrapped to
the table's width within 60 to 80 columns, so a pasted table
can carry its own key. On a run without `-v`, the line under
each banner says so.

The three tools that sweep placements, `tp-matrix`,
`tp-stream`, and `tp-pool`, take `--base-cpu N`, the cpu every
placement starts from: CCX is it and a core on its L3, x-CCX
it and a core outside, SMT it and its sibling. The default is
0, and `tp-cell` pins explicitly with `--pin`. `-d` is 1 s a
cell by default in `tp-cell`, `tp-matrix`, and `tp-stream`,
samples enough for the mean and stdev at millions of trips a
second, and a calmer number wants `-d 5`.

```sh
$ tp-matrix -d 10                  # 28 cells x 10 s on a typical SMT machine, depth 8
$ tp-matrix -d 5 --depth 1,2,8,64  # every cell again at each depth
$ tp-matrix -d 1 -v                # with the column legend
tp-matrix 0.1.0 - run the full measurement matrix, markdown tables out
28 cells, 1.0s each, spsc-v3 and mpsc-v2 with 2 segments
...
| placement | flavor  | depth |   m.send |     w.recv |     w.spin | ... |  RTs | xfills/RT |
|-----------|---------|------:|---------:|-----------:|-----------:|-----|-----:|----------:|
| 0,1 CCX   | spsc-v0 |     8 | 22.3/6.0 | 132.5/13.9 |  111.4/8.6 | ... | 3.7M |    10.120 |
| 0,1 CCX   | mpsc-v0 |     8 |  9.5/4.2 |  95.5/19.6 |   65.6/6.9 | ... | 5.2M |     6.617 |

- `placement`: the CPUs the two threads are pinned to and how they share caches:
  CCX two cores on one L3, x-CCX cores on different L3s, SMT one core's two
  hardware threads sharing its L1 and L2, or unpinned
...
- `xfills/RT`: x-core cache-line fills: cache lines pulled into a core from
  another core's cache, near 0 when the threads share a core's caches, as SMT
  siblings do, per round trip
```

`--depth` takes a comma-separated list of ring depths (slots
per ring, powers of two from 1 up), the default `8`, and each
cell repeats per depth. One message is ever in flight, so the
depth changes how many seq words share a line and, at 1,
whether the ring has any slack. The MPSC v0 ring rejects
depth 1, where its protocol wedges (the design note's "MPSC
v1: equality-seq ring"), so its cells there are skipped with a
note naming mpsc-v1, which runs depth 1.

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
counters divided by the messages moved give the xfills per
message while streaming, and `-v` adds the legend.

```sh
$ tp-stream -d 5 --depth 1,2,8,64
tp-stream 0.1.0 - run the streaming matrix, one markdown table out
...
| placement | flavor  | depth | ns/msg |   msgs | xfills/msg |
|-----------|---------|------:|-------:|-------:|-----------:|
| 0,3 x-CCX | spsc-v1 |    64 |   37.4 | 133.8M |      0.491 |
| 0,3 x-CCX | spsc-v2 |    64 |   14.3 | 350.1M |      0.132 |
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
  from 104 ns per message (1.8 xfills) in a plain counted loop
  to about 40 (0.6 xfills). v1 is bistable there: the two sides
  either write its packed seq line in lockstep or the producer
  runs ahead in bursts, and a periodic hiccup on the producer
  tips it into the second. v0 and v2 read the same in both
  loop shapes. The demo's stream lines are the plain loop.

## tp-pool: the pool-message sweep

The two matrices above write the payload into the ring's slot.
The messaging layer's loop is the other shape: take a message
from the pool, fill it, push its reference, receive it,
process it, return it to the pool. `tp-pool` runs that loop
for a duration per cell, `-d` as in the other tools, over the
descriptor rings,
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
$ tp-pool                                  # pools 1,100,1000; depths 1,8,64,1024; 0.1 s a run; median of 3
$ tp-pool --pool 1,10,100 --depth 1,1024 -d 0.5 --repeat 5
tp-pool 0.1.0 - run the pool-message sweep, one table per placement
...
0,3 x-CCX: ns/msg

| flavor    | depth | pool=1 | pool=100 | pool=1000 |
|-----------|-------|-------:|---------:|----------:|
| spsc-v2   | 1024  |  504.2 |    214.9 |     215.0 |
| cordyceps | -     |  615.0 |    213.7 |     217.2 |
```

Each placement gets an `ns/msg` table and an `xfills/msg`
table of the same shape, and under `-v` one legend follows
the last placement. The cordyceps consumer waits on
`Inconsistent`, a producer between its head swap and its link
store, as it waits on `Empty`, so that window's cost is in the
row's ns/msg as a ring consumer's polls are in its. The pool's
free-stack is in every row: the consumer's free pushes the
buffer it just read, and the producer's alloc pops that same
buffer, so a line the consumer wrote crosses back on every
message whatever the queue does, which is why these numbers sit
far above the in-slot tables' at the same placement.

Depth here throttles: a ring shallower than the pool holds the
producer back, unlike `tp-matrix`, where one message in flight
means depth never throttles. The closest match between the two
is `tp-pool` at pool=1 against `tp-matrix` at depth 1 halved,
since a round trip is two handoffs, and `tp-pool` still reads
higher for the pool's free-stack line crossing on every
message.

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
  tools still run, the fill columns read `-`, and `tp-cell`'s
  fill counters line reads unavailable.
- The fill events are AMD Zen 2 raw encodings, A/B-verified
  against `perf stat`. Other microarchitectures need their
  own encodings in `tp_runner::perf`.
- Non-Linux builds run unpinned without counters.
- The crates: probes are `tprobe`, generic runner machinery
  (CLI, pinning, drive loop, perf, topology) is `tp_runner`.
  This crate holds only the ring-aware cells and the four
  binaries.
