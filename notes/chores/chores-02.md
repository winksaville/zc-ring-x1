# Chores-02

Chores-XX files use [Prose form](../../AGENTS.md#prose-form). They
contain discussions and notes on various chores in github compatible
markdown. There is also a [todo.md](../todo.md) file that tracks
tasks and in general there should be a chore section for each task
with the why and how this task will be completed.

Continues [chores-01.md](chores-01.md), opened as that file
approached ~1000 lines. Reference numbering restarts at `[1]`
(file-local — see
[Reference numbering](../../AGENTS.md#reference-numbering)).

## docs: execution contexts + ISR blue-sky goal

Commits: [[1]]

Design-doc pass out of conversation (no ladder): the
arbitrary-graph topology bullet, the Execution contexts
subsection (thread/ISR sole-owner and sharing cases), the
blue-sky ISR-to-ISR goal with its gotchas, and the
2t-surprise cache-line-bounce mechanism sharpened in
[chores-01](chores-01.md#outcome-the-2t-surprise).

## refactor: versioned primitive module dirs

Commits: [[2]],[[3]],[[4]],[[5]]

The three primitives sat at uneven depths — `mpsc/` was a
directory, SPSC was spread across `lib.rs` plus top-level
`producer.rs` / `consumer.rs`, and the pool was one
`pool.rs` — and there was nowhere for a second implementation
of a primitive to live for A/B comparison in iiac-perf. Each
primitive now has a `{primitive}/v0/` module dir so all three
are siblings and a future `v1` slots in beside `v0` as a
peer.

Decisions (the versioning discussion, folded in):

- **Structure** — `src/spsc/v0/`, `src/mpsc/v0/`,
  `src/pool/v0/`; version is the inner axis, so each
  primitive versions independently.
- **Names unchanged** — keep the current type names inside
  `v0` (`Ring`, `MpscRing`, `Pool`, …); a default-version
  re-export per module (`spsc::Ring = spsc::v0::Ring`) plus
  the existing crate-root re-exports keep the public API
  identical, so the cycle is a pure move. `spsc::v0::Ring` is
  newly reachable for explicit version pinning.
- **Shared core stays in `lib.rs`** — `Error`,
  `CACHE_LINE_SIZE`, `USER_WORDS`, `slot_ptr`, `check_type`,
  `CacheAligned` are crate-wide; versions reach them via
  `crate::`.
- **`Full` / `Empty` promote to the shared core** — they were
  defined in the SPSC endpoint files but imported by mpsc
  (`crate::{Empty, Full}`); leaving them inside `spsc/v0/`
  would couple sibling primitives (and any future
  `spsc::v1`) to one version's module. Crate-root re-exports
  keep the public API unchanged.
- **`-1` also moves the SPSC core out of `lib.rs`** —
  `Header`, `Ring`, `MAGIC`, `LAYOUT_VERSION`, `header_ptr`,
  `region_size`, `validate_geometry` into `spsc/v0/`
  alongside `producer.rs` / `consumer.rs`.
- **registry and policy stay top-level** — policy is shared
  core (no version axis); registry's placement waits for the
  descriptor-queue endpoints work.
- **Cargo feature-gating deferred** — coexistence needs no
  features; per-version build-size exclusion only pays off
  once a `v1` exists, so features land with the first fork.
- **Wording: "default-version re-export", not "champion
  alias"** — the `-1` docstrings coined "champion alias" (a
  blend of champion–challenger testing and a Rust
  re-export); `-2` swept it for plain searchable terms.

### As-built ladder

- `0.12.0-0` chore: open versioned module dir refactor
- `0.12.0-1` refactor: spsc into a v0 module dir
- `0.12.0-2` refactor: mpsc into a v0 module dir
  (also the champion-alias wording sweep)
- `0.12.0-3` refactor: pool into a v0 module dir
- `0.12.0` refactor: versioned primitive module dirs
  (close-out: bookkeeping + design-doc as-built sync; its
  SHA backfills on the next push)

## perf: explore spsc vs mpsc 2t gap

Commits: [[7]],[[8]],[[9]],[[10]],[[11]],[[12]]

Picks up the Todo left by the mpsc cycle's
[Outcome: the 2t surprise](chores-01.md#outcome-the-2t-surprise)
[[6]]: at 1p/1c cross-thread the MPSC ring measures ~26%
faster than SPSC (73.9 vs 100.1 ns adjusted mean at 300s).
We think SPSC bounces two index cache lines per handoff
(each side polls the line the other writes) while MPSC's
only shared hot word is the slot seq line. This cycle
verifies that mechanism with measurements that live in this
repo, so the result is reproducible here rather than only
in the sibling iiac-perf checkout.

Plan:

- **Local `tprobe/` dev crate** (`0.13.0-1`): iiac-perf is
  binary-only, so its probes can't be imported. `tprobe/`
  is a **new crate** — trivial/simple but adequate —
  leveraging iiac-perf where appropriate; it derives from
  iiac-perf 0.20.0's `tprobe.rs`, `tprobe2.rs`, `ticks.rs`
  + `ticks/`, `band_table.rs`, and the two `fmt_commas`
  `harness.rs` helpers (which became `fmt.rs`). Provenance
  and deltas are recorded here, not in doc comments. Sole
  external dep: `hdrhistogram`. Deltas from the iiac-perf
  sources:
  - `TProbe2` renamed `TProbeSpan` (`TProbe2RecId` →
    `TProbeSpanId`, `tprobe2.rs` → `tprobe_span.rs`, report
    kind label `tprobe-span`): the `2` named invention
    order, not semantics; "span" is the start/end interval
    vocabulary (non-stack interleaving is span-like, not
    scope-like). Intended as the go-forward name if
    iiac-perf unifies onto an extracted crate.
  - `minstant` dependency dropped: the x86_64 calibration
    anchors on `std::time::Instant` (vDSO `clock_gettime`
    overhead is negligible over the 10 ms window), and the
    kernel-clocksource check reads
    `/sys/.../clocksource0/current_clocksource` directly —
    matching what `minstant::is_tsc_available` did.
  - `bands.rs` not taken: it isn't in the probes' closure
    (`band_table.rs` carries its own percentile ladder) and
    would have dragged in `clap`.
  - iiac-perf-specific doc text (its `Probe`, bench names,
    ideas.md pointers) dropped or reworded; `// OK: …`
    annotations on `unwrap*` and missing `///` docs added
    per this repo's conventions.
  - `TProbeSpan` is included for API completeness but not
    used by this cycle's example: its record buffer grows
    unbounded (tens of millions of round trips × 24 B per
    record), and its report drains all sites into one
    histogram, so it can't separate phases anyway. `TProbe`
    is the fit.
  - Root `Cargo.toml` gains `[workspace]` membership and a
    path dev-dependency. A crate boundary from day one
    keeps future extraction a file-level move, not a
    rewrite.
- **Phase-probed round trip** (`0.13.0-2`):
  `examples/tp_roundtrip.rs`, the same 1p/1c main → worker
  → main round trip as iiac-perf's `zcr-with-2t` /
  `zcr-mpsc-2t`, both ring flavors, four `TProbe`s — main
  send, main recv, worker recv, worker send (worker probes
  return via `JoinHandle`). Fixed-duration loop, no
  adaptive harness: the comparison is A/B under identical
  framing, so calibrated overhead subtraction isn't needed.
  `--duration` / `--pin` / flavor args.
- **Generic `tp_runner/` crate + READMEs** (`0.13.0-3`,
  added during `-2` review): the example's generic runner
  machinery — CLI config (`-d`/`--pin`/`-t` + positionals),
  `pin_to_cpu`, the fixed-duration round-trip drive loop,
  probe report ordering — moves to a third workspace crate,
  `tp_runner/`, leaving only the two ring flavors in the
  example. Deliberately *not* a full harness (no `Bench`
  trait, adaptive sizing, or calibration — that is
  iiac-perf's territory; needing it is the cue to unify).
  `tprobe/README.md` + `tp_runner/README.md` cover build /
  test / run.
- **Measurements + findings** (`0.13.0-4`): run unpinned,
  same-CCX (pin 0,1), cross-CCX (pin 0,3); plus `perf stat`
  with Zen 2's `ls_refills_from_sys.ls_mabresp_lcl_cache`
  (demand fills served from another core's cache — the
  cross-core line-transfer counter). Record protocol,
  numbers, and conclusion here; if the data supports it,
  file a seam-word SPSC variant Todo (give SPSC a seq-like
  publish word so neither side reads the other's index
  line).
- **Spin-wait probes** (`0.13.0-5`, added during `-4`
  review): decompose the recv phases — a spin-time probe
  (first failed attempt → reserve success) and an attempts
  histogram per recv side, recorded only when the reserve
  actually waited (the zero-spin fraction falls out of the
  count difference vs the phase probe). `TProbe` grows a
  counts flavor (`new_counts`) and the band table a count
  unit (`ct`, no tick→ns conversion). Matrix re-run at
  `--decimals 3`, with exact fills/RT for the ~0 SMT cells.
- **Close-out** additionally seeds the crate's own notes —
  `tprobe/notes/design.md` (design rationale that outlived
  the doc comments: probe-type trade-offs, ticks
  abstraction, runner split) and
  `tprobe/notes/chores/chores-01.md` (the crate's
  going-forward record; this cycle's story stays here, the
  seed points back).

### Measurements (0.13.0-4)

Machine: AMD Ryzen 9 3900X (Zen 2, 12c/24t; cpus 0–2 share
CCX0's L3, cpu 3 is in CCX1, cpu 12 is cpu 0's SMT sibling
sharing L1/L2). Command, per flavor × placement:

```
perf stat -e ls_refills_from_sys.ls_mabresp_lcl_cache,\
ls_refills_from_sys.ls_mabresp_lcl_l2,\
ls_refills_from_sys.ls_mabresp_lcl_dram -- \
  target/release/examples/tp_roundtrip <flavor> -d 10 [--pin m,w]
```

One round trip (RT) is main send → worker recv → worker
send → main recv: main places the counter in the request
ring, the worker echoes it on the response ring. Columns:

- `m.send` / `w.send` — the producer phase on that side, in
  ns: reserve + fill + commit (`send_with` for mpsc); the
  time to place a message in the ring, never waiting for
  space (one message in flight, 8 slots).
- `w.recv` / `m.recv` — the consumer phase, in ns: spin
  wait for the message to arrive + read + release.
- Phase cells are `mean/stdev`, both from the report's
  trimmed min-p99 band (`mean min-p99` / `stdev min-p99`).
- `RTs/10s` — completed round trips in the 10 s run; the
  wall-clock cost of one RT ≈ 10 s / RTs (e.g. 52.6M →
  ~190 ns/RT).
- `fills/RT` — `ls_mabresp_lcl_cache` (demand fills served
  from another core's cache, i.e. cache lines that crossed
  between the cores) divided by round trips.

The exact commands and table-building arithmetic are in the
tprobe README's
[Reproducing the measurement matrix](../../tprobe/README.md#reproducing-the-measurement-matrix).

| placement  | flavor | m.send    | w.recv     | w.send    | m.recv     | RTs/10s | fills/RT |
|------------|--------|----------:|-----------:|----------:|-----------:|--------:|---------:|
| 0,1 CCX    | spsc   |  21.8/6.8 |   98.2/9.8 |  37.9/9.5 |   94.4/9.4 |   52.6M |   10.042 |
| 0,1 CCX    | mpsc   |  11.9/5.3 |  65.8/21.4 |  12.8/4.6 |  71.4/19.1 |   72.9M |    6.687 |
| 0,3 x-CCX  | spsc   |108.8/46.6 |504.0/107.9 |135.1/55.0 | 476.9/99.4 |   14.5M |    9.848 |
| 0,3 x-CCX  | mpsc   | 58.2/42.9 |348.8/112.7 | 52.7/39.1 |425.1/110.7 |   17.4M |    6.768 |
| 0,12 SMT   | spsc   |   8.6/3.5 |   32.5/9.4 |   8.6/3.4 |   31.7/9.2 |  131.2M |   0.0004 |
| 0,12 SMT   | mpsc   |   8.8/3.3 |   47.5/7.3 |  13.3/4.7 |   50.8/8.6 |  104.0M |   0.0007 |
| unpinned   | spsc   | 22.9/12.0 |102.6/45.6 |  40.2/12.4 | 100.0/43.8 |   53.9M |    9.954 |
| unpinned   | mpsc   |  11.3/6.1 | 68.3/34.4 |   13.7/6.3 |  77.5/43.8 |   66.3M |    6.714 |

### Findings: the gap is line-transfer economics

- **The gap lives in the send path.** Cross-core, SPSC's
  producer phases cost 2–3× MPSC's (0,1: 21.8/37.9 vs
  11.9/12.8 ns) — the SPSC send both *reads* the
  peer-written index line (`consumer_idx`, the non-full
  check) and *writes* a line the peer is actively polling
  (`producer_idx`). The MPSC send touches one shared line
  (the slot seq); its claim index is core-private. Recv
  waits mirror the same story (they absorb the opposite
  send plus the in-flight half trip).
- **Transfer counts match the hypothesis and are placement-
  invariant**: ~10.0 (SPSC) vs ~6.7 (MPSC) cross-core line
  fills per round trip at same-CCX, cross-CCX, and unpinned
  — ~1.6 fewer transferred lines per handoff. Counts exceed
  the naive 2-vs-1 accounting because every spin poll after
  an invalidation refetches.
- **The SMT control clinches the mechanism**: pinned to
  siblings sharing L1/L2 (0,12), fills drop to ~0 for both
  flavors and the sign flips — SPSC wins (32 vs 48–51 ns
  recv phases; 131M vs 104M round trips). MPSC's 2t
  advantage is entirely cache-line-transfer economics;
  absent transfer cost, SPSC's load/store-only protocol is
  cheaper, consistent with its 1t numbers.
- **Unpinned behaves like same-CCX** — the scheduler mostly
  keeps the pair co-resident on one CCX.
- We think the SPSC main-send vs worker-send asymmetry
  (21.8 vs 37.9 ns at 0,1) comes from what the peer is
  doing concurrently: when the worker sends, main is
  already spin-polling that ring's `producer_idx` line
  hard, stealing it mid-send; when main sends, the worker
  is still finishing its recv on the other ring.
- Cross-CCX (0,3) multiplies everything ~4–5× (L3-to-L3
  through the IO die) but preserves the ratio — the
  mechanism is the same, the lines are just more expensive.
- Conclusion: the mechanism is confirmed; a **seam-word
  SPSC variant** (per-slot publish word, indices
  endpoint-private, still load/store-only — no CAS needed
  at 1p) is worth building. Filed as a Todo.

### Spin decomposition (0.13.0-5)

Where the wait goes: the matrix re-run with the spin probes
at `--decimals 3` (same command shape as
[Measurements (0.13.0-4)](#measurements-0130-4)) decomposes
each recv phase into its wait and its work. Cells are
`mean/stdev` from the report's trimmed min-p99 band (`w.`
worker / `m.` main):

- `spin` — the wait inside the recv phase, in ns: first
  failed attempt → reserve success; recorded only when the
  reserve actually waited.
- `att` — polls per waiting reserve, a count: how many
  times the wait-policy closure ran before the message was
  visible.
- `fills/RT` — cross-core cache-line fills per round trip,
  as in the Measurements table (this run's values).

Same reproduction recipe as Measurements —
[Reproducing the measurement matrix](../../tprobe/README.md#reproducing-the-measurement-matrix).

| placement  | flavor | w.spin     | w.att    | m.spin     | m.att    | fills/RT |
|------------|--------|-----------:|---------:|-----------:|---------:|---------:|
| 0,1 CCX    | spsc   | 104.5/10.4 |  6.8/0.8 | 104.0/10.0 |  7.0/0.4 |   10.001 |
| 0,1 CCX    | mpsc   |  74.0/15.4 |  4.2/0.7 |   72.6/9.5 |  4.0/0.4 |    6.591 |
| 0,3 x-CCX  | spsc   | 543.9/90.5 | 28.8/5.1 | 544.9/84.7 | 27.9/4.6 |   10.082 |
| 0,3 x-CCX  | mpsc   |331.5/118.1 | 14.5/4.4 |324.9/107.6 | 13.9/3.6 |    6.737 |
| 0,12 SMT   | spsc   |   40.1/5.9 |  2.0/0.3 |   40.2/6.4 |  2.1/0.3 |   0.0009 |
| 0,12 SMT   | mpsc   |   63.9/8.1 |  2.7/0.6 |  68.3/10.8 |  2.9/0.7 |   0.0010 |
| unpinned   | spsc   |125.3/90.4  |  7.6/4.1 |126.4/93.8  |  8.0/4.5 |    9.834 |
| unpinned   | mpsc   |101.8/79.6  |  5.0/2.9 |103.0/78.4  |  5.1/2.8 |    6.631 |

- **Essentially every recv waits**: the spin/attempts probe
  counts trail the phase counts by < 0.01%, so the zero-spin
  case is negligible at 1 message in flight.
- **recv ≈ spin + a ~17–20 ns tail** (recv mean − spin
  mean, per cell) in 14 of 16 cells — the post-arrival
  read + release is a near-fixed cost; placement and flavor
  differences live in the wait. Exceptions: spsc 0,3 main
  (22.9), mpsc unpinned main (25.6), and one outlier —
  mpsc 0,3 main (86.2). We think the outlier is partly a
  trim artifact: the phase and spin histograms trim their
  min-p99 bands over different populations, so the two
  trimmed means subtract cleanly only when the tail is
  thin.
- **Attempts retell the fills story from software**: at
  same-CCX SPSC's consumer polls ~7 times per message vs
  MPSC's ~4 — each failed poll after an invalidation
  refetches the line (~15–25 ns per attempt, consistent with
  an L3-mediated line hop; ~19–23 ns cross-CCX). Fewer lines
  bouncing → the message-ready signal stabilizes sooner →
  fewer refetches.
- At SMT both flavors poll only ~2–3 times and SPSC's wait
  is shorter (40 vs 64–68 ns) — with the transfer cost gone,
  what remains is the protocol's own store→visible latency,
  where SPSC's plain index store beats MPSC's seq handoff.
- Caveat: the spin instrumentation costs throughput (~20%
  fewer round trips than the `-4` matrix — the waits absorb
  the peer's extra tick reads and histogram records), so
  phase means here run slightly hot. Use the `-4` table for
  phase costs, this one for the decomposition.

### Preliminary evidence: cross-core fill counters

Superseded by [Measurements (0.13.0-4)](#measurements-0130-4)
above — kept as the exploratory record that motivated the
cycle's shape.

Exploratory 10 s runs of the existing iiac-perf 0.20.0
benches under `perf stat` (before this cycle's in-repo
instrumentation), `ls_mabresp_lcl_cache` fills divided by
round trips:

| placement          | zcr-with-2t | zcr-mpsc-2t |
|--------------------|-------------|-------------|
| pin 0,1 (same CCX) | 10.3        | 7.2         |
| pin 0,3 (cross CCX)| 10.2        | 7.0         |
| unpinned           | 10.2        | 7.1         |

SPSC pays ~3 more cross-core line transfers per round trip
(~1.5 per handoff), stable across placements — consistent
with the two-index-lines hypothesis. Absolute counts exceed
the naive 2-vs-1 line accounting because each spin poll
after an invalidation refetches the line; the ratio matches
the ~26% latency gap. The 2t gap itself also reproduces at
10 s unpinned: adjusted means 131.5 ns (SPSC) vs 93.9 ns
(MPSC).

### As-built ladder

- `0.13.0-0` docs: 2t gap exploration plan
- `0.13.0-1` feat: 2t gap tprobe dev crate
- `0.13.0-2` feat: 2t gap probed roundtrip example
- `0.13.0-3` refactor: 2t gap tp_runner crate + READMEs
- `0.13.0-4` docs: 2t gap measurements + findings
- `0.13.0-5` feat: 2t gap spin-wait probes
- `0.13.0` perf: explore spsc vs mpsc 2t gap (close-out;
  seeded `tprobe/notes/design.md` and
  `tprobe/notes/chores/chores-01.md`)

# References

[1]: https://github.com/winksaville/zc-ring-x1/commit/2ea448654c9a "2ea448654c9a4b7f758e017d56161d9d731ab425"
[2]: https://github.com/winksaville/zc-ring-x1/commit/f2b6384a42b6 "f2b6384a42b6a348c28cfb10a8b8900c4f43fbb5"
[3]: https://github.com/winksaville/zc-ring-x1/commit/6daa99e3a0f8 "6daa99e3a0f81df6fca2c4b2e178e9d4ccaa8180"
[4]: https://github.com/winksaville/zc-ring-x1/commit/9e25f853111d "9e25f853111d5757dc16fae8401dbdc0fe10fff6"
[5]: https://github.com/winksaville/zc-ring-x1/commit/423ac89b4abb "423ac89b4abb1c9330c30724cb9d1256cf95b510"
[6]: chores-01.md#outcome-the-2t-surprise
[7]: https://github.com/winksaville/zc-ring-x1/commit/55cc19d3734f "55cc19d3734f705a478d405953157b42182dfd19"
[8]: https://github.com/winksaville/zc-ring-x1/commit/7fe93150a95e "7fe93150a95e304bab99af31dd0c85d37a21b93c"
[9]: https://github.com/winksaville/zc-ring-x1/commit/2fb9578ef7ff "2fb9578ef7ff30ea3218b8d92f44e8962fafa385"
[10]: https://github.com/winksaville/zc-ring-x1/commit/21f1d1d21c93 "21f1d1d21c93186f2fd010431e8bfb75be5d67f8"
[11]: https://github.com/winksaville/zc-ring-x1/commit/10c085ab8be5 "10c085ab8be5f1c9a0357fb608adbac592c154ba"
[12]: https://github.com/winksaville/zc-ring-x1/commit/7c9d7bcd5278 "7c9d7bcd5278c2bdb7b2d5d4a83e335b7a5c9582"
