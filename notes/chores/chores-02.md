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

Commits:

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

Phase values are the report's trimmed `mean min-p99` in ns;
`fills/RT` is `ls_mabresp_lcl_cache` (demand fills served
from another core's cache) divided by round trips.

| placement  | flavor | m.send | w.recv | w.send | m.recv | RTs/10s | fills/RT |
|------------|--------|-------:|-------:|-------:|-------:|--------:|---------:|
| 0,1 CCX    | spsc   |   21.8 |   98.2 |   37.9 |   94.4 |   52.6M |     10.0 |
| 0,1 CCX    | mpsc   |   11.9 |   65.8 |   12.8 |   71.4 |   72.9M |      6.7 |
| 0,3 x-CCX  | spsc   |  108.8 |  504.0 |  135.1 |  476.9 |   14.5M |      9.8 |
| 0,3 x-CCX  | mpsc   |   58.2 |  348.8 |   52.7 |  425.1 |   17.4M |      6.8 |
| 0,12 SMT   | spsc   |    8.6 |   32.5 |    8.6 |   31.7 |  131.2M |       ~0 |
| 0,12 SMT   | mpsc   |    8.8 |   47.5 |   13.3 |   50.8 |  104.0M |       ~0 |
| unpinned   | spsc   |   22.9 |  102.6 |   40.2 |  100.0 |   53.9M |     10.0 |
| unpinned   | mpsc   |   11.3 |   68.3 |   13.7 |   77.5 |   66.3M |      6.7 |

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

# References

[1]: https://github.com/winksaville/zc-ring-x1/commit/2ea448654c9a "2ea448654c9a4b7f758e017d56161d9d731ab425"
[2]: https://github.com/winksaville/zc-ring-x1/commit/f2b6384a42b6 "f2b6384a42b6a348c28cfb10a8b8900c4f43fbb5"
[3]: https://github.com/winksaville/zc-ring-x1/commit/6daa99e3a0f8 "6daa99e3a0f81df6fca2c4b2e178e9d4ccaa8180"
[4]: https://github.com/winksaville/zc-ring-x1/commit/9e25f853111d "9e25f853111d5757dc16fae8401dbdc0fe10fff6"
[5]: https://github.com/winksaville/zc-ring-x1/commit/423ac89b4abb "423ac89b4abb1c9330c30724cb9d1256cf95b510"
[6]: chores-01.md#outcome-the-2t-surprise
