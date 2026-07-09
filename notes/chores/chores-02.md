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
  binary-only, so its probes can't be imported. Copy, as
  close to verbatim as possible, iiac-perf 0.20.0's
  `tprobe.rs`, `tprobe2.rs`, `ticks.rs` + `ticks/`,
  `band_table.rs`, `bands.rs`, and the two `fmt_commas`
  helpers into a local crate (only external dep:
  `hdrhistogram`), provenance noted in module docs.
  - Root `Cargo.toml` gains `[workspace]` membership and a
    path dev-dependency.
  - A crate boundary from day one keeps future extraction
    (or unification with iiac-perf) a file-level move, not
    a rewrite. Verbatim-copy discipline limits drift while
    two copies exist.
  - `TProbe2` is copied for completeness but not used here:
    its record buffer grows unbounded (tens of millions of
    round trips × 24 B per record), and its report drains
    all sites into one histogram, so it can't separate
    phases anyway. `TProbe` is the fit.
- **Phase-probed round trip** (`0.13.0-2`):
  `examples/tp_roundtrip.rs`, the same 1p/1c main → worker
  → main round trip as iiac-perf's `zcr-with-2t` /
  `zcr-mpsc-2t`, both ring flavors, four `TProbe`s — main
  send, main recv, worker recv, worker send (worker probes
  return via `JoinHandle`). Fixed-duration loop, no
  adaptive harness: the comparison is A/B under identical
  framing, so calibrated overhead subtraction isn't needed.
  `--duration` / `--pin` / flavor args.
- **Measurements + findings** (`0.13.0-3`): run unpinned,
  same-CCX (pin 0,1), cross-CCX (pin 0,3); plus `perf stat`
  with Zen 2's `ls_refills_from_sys.ls_mabresp_lcl_cache`
  (demand fills served from another core's cache — the
  cross-core line-transfer counter). Record protocol,
  numbers, and conclusion here; if the data supports it,
  file a seam-word SPSC variant Todo (give SPSC a seq-like
  publish word so neither side reads the other's index
  line).

### Preliminary evidence: cross-core fill counters

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
