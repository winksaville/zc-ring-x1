# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

_None._

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### feat: in-slot seq SPSC v2

#### Problem

The seam-word ring's seq words live in their own line, and v1 is slower per send than the MPSC
producer at every placement, the puzzle [SPSC v1: seam-word
ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) leaves open. Every streaming number on
record is at one depth per tool, 64 in the demo and 8 in `tp-matrix`, so depth and protocol have
never been separated, and the streaming lines carry no fill counts, so the line traffic behind
them is inferred rather than measured.

#### Solution

A `spsc::v2` sibling ring with the seq in the slot's own line, a crate-owned slot header ahead of
the user-owned body, measured beside v0, v1, and MPSC at depths 1, 2, 8, and 64 in both tools, the
demo first and then a `tp-matrix` streaming cell that counts fills per message, with the findings
recorded in the design note:
- starts from the probe rung's baseline: packed seq array, neither side waiting, v0 ahead within an
  L3 and v1 ahead across one
- the prediction on record: it helps the round trip and may hurt streaming, since the slot line
  would then travel both ways every message where today the seq line amortises
- the cheap probe goes first: a fence after v1's commit, one line, to test the store-buffer reading
- the seq's width is measured (u32 against the native width) before the slot header fixes it
- the slot contract changes (the body sits behind the crate-sized header), so its shape is part of
  the finding.

#### Acceptance check

`vc-x1 validate` passes, including v2 ring tests at `M = 1`, `2`, and a larger power of two.
`tp-matrix` and the demo run all four flavors at depths 1, 2, 8, and 64 on the 3900X, the
streaming cell reports fills per message, and `notes/ring-buffer-design.md` carries the tables
with a why paragraph per placement, the fence probe's result, and the seq width chosen.

#### Ladder

- [feat: in-slot seq SPSC v2 opening][1] (done)
- [feat: runtime depth in the demo and tp-matrix][2] (done)
- [feat: add the spsc v2 in-slot seq ring][3]
- [feat: spsc v2 as a fourth flavor][4]
- [perf: probe a fence after the v1 commit][5]
- [perf: measure spsc v2 across depths][6]
- [feat: a streaming cell with fill counts][7]
- [docs: sweep punctuation in the touched files][8]
- [feat: in-slot seq SPSC v2 closing][9]

#### Deliberation

- v2 is a sibling module, not an edit of v1: the module layout exists for the A/B, and the user
  wants every version comparable at once, so v0, v1, v2, and MPSC all stay reachable by path and
  the crate's default re-export moves only if the numbers earn it.
- The seq sits in a 16-byte crate header at the front of the slot's first line, the body behind
  it, rather than a whole header line ahead of the body: the second is padded v1 at a different
  address, and padded v1 already measured worse. Sixteen bytes so the u32 against u64 flip fits
  without a layout change, and the header is the seq alone, the rest reserved, so the cycle
  carries one design change.
- Depth becomes a runtime parameter in both tools, the regions heap-allocated and sized by each
  ring's own `region_size`, since the const stack arrays fix one depth per build. The demo takes
  the sweep first, a table of flavor by depth per placement, and the `tp-matrix` streaming cell
  with fill counts follows, on the user's call (2026-09-07): more information is better until it
  interferes with the measuring, so the simple form goes first.
- The fence probe keeps its own rung ahead of the measurement, as the Todo entry ordered it: its
  answer says whether v2's commit wants a fence too, and it is a one-line flip measured in the
  same matrix.
- Punctuation: the demo, the `tp_matrix` sources, and the v1 sources carry banned characters, so
  touching them owes the conversion, paid in the penultimate rung as the prose rule says.
- Waiver: the user's delegation of 2026-09-07, "you have permission to complete this cycle,
  including commits and pushes, but leave it on the branch", covers every push from the bookmark
  through the closing and the per-rung review stops, and does not cover Land, which waits on the
  user's review.

#### Ladder details

##### feat: in-slot seq SPSC v2 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
Todo entry into this block, bump the version-of-record, and rename the demo binary to `-dev`.

##### feat: runtime depth in the demo and tp-matrix

Both tools fix the ring depth at build time, so no run can compare depths. The rung makes depth a
runtime parameter and adds the demo's depth sweep table.

* The regions were const stack arrays sized by a `DEPTH` const, one per build.
  - The runner gains a line-aligned heap region sized at runtime by each ring's own size function,
    and the demo carries the same helper over zerocopy. Heap against stack changes nothing the
    loops measure, since the region is touched once at init.
  - v0 exports no region size function, its header being a fixed shape, so both tools compute it
    from the header's size. A v0 export is a deferral, not a need.
* Nothing let a run ask for a depth.
  - `--depth` on the shared args takes a comma list of powers of two, default 8, and every
    `tp-matrix` and `tp-cell` cell repeats per depth, the matrix tables gaining a depth column.
  - The demo's lines stay at depth 64 and a sweep follows them: the ring flavors at every placement
    and at depths 1, 2, 8, and 64, one markdown table per placement in ns per message.
* The MPSC ring accepts capacity 1 and its protocol collapses there, found when the sweep's first
  mpsc cell at depth 1 spun forever.
  - Filed in `notes/bugs.md`: committed `pos + 1` and released `pos + M` coincide at `M = 1`, the
    collapse v1's `M = 1` tests caught in its own first cut. Each flavor now names its floor, and
    the tools skip a cell below it with a note, the demo printing `-`.
  - The fix is unplanned work and stays out of this cycle, on the rule that unplanned work is the
    user's to place.

##### feat: add the spsc v2 in-slot seq ring

The seq and the slot it publishes live on different lines in v1. The rung adds the sibling ring
whose slot carries its own seq, with the tests v1 grew.

##### feat: spsc v2 as a fourth flavor

The tools know three flavors. The rung adds v2 to `tp-matrix`, `tp-cell`, and the demo.

##### perf: probe a fence after the v1 commit

The v1 per-send loss has a store-buffer candidate on record. The rung measures v1 with a fence
after its commit store and records the answer.

##### perf: measure spsc v2 across depths

The v2 ring exists with no numbers. The rung runs both tools at every depth on the 3900X, flips the
seq width, and records the tables and their why in the design note.

##### feat: a streaming cell with fill counts

The streaming lines carry no fill counts. The rung adds a `tp-matrix` streaming cell that counts
fills per message, and records what it shows.

##### docs: sweep punctuation in the touched files

The files the cycle touched carry banned characters and prose semicolons. The rung converts them.

##### feat: in-slot seq SPSC v2 closing

Closing out the cycle.

## Waiting

Important work that cannot start yet. Each entry names what it waits on, in a form that can be
checked, and the rank it takes in `## Todo` once unblocked. Every opening checks each condition
and promotes what is met ([Opening](AGENTS.md#opening)).

_None._

## Todo

Entries are in priority order, the first highest, and reprioritizing is moving an entry. Each is a
`###` heading, so a citation is a link to its anchor. Long-tail entries live in
[todo-backlog.md](notes/todo-backlog.md). Use the [Prose form](agent-data/prose.md#prose-form).
Deeper detail goes in a `notes/` design file (link via `[N]` ref).

### Segmented queue over spsc v1

The rings are fixed-length and a Full ring fails the send. A chain of v1 ring segments allocated
from a `Pool`, designed in [SPSC v1: seam-word
ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) and deferred when the ring's numbers
missed the bar:
- pool segments: take a pool buffer as the byte region a v1 `Ring::init` wants, sized by
  `region_size(slot_size, M)`, and nothing else the pool does not already have
- the chain: producer and consumer endpoints, the link word, the segment switch on Full and on
  Empty, freeing the drained segment, and a boundary-crossing test that returns every segment
- the size sweep: `M` from 1 to 256 in the cell, the seam cost as the slope, recorded with the
  default the crate picks
- supersedes Overflow FIFO if it lands
- goes ahead once a v1 form clears the bar, faster than MPSC v0 cross-core.

### Demo pin-pair picker

The demo's "diff cores" pair is the first cpu outside cpu0's L3, cross-L3 on the 3900X and same-L3
on the 7600X, so the two machines' lines with the same label were different experiments. The picker
wants a same-L3 placement and a cross-L3 one, each labelled by what it is, and the same for
`tp-matrix`'s placements.

### Descriptor queue endpoints

Paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
- own ring endpoint + registry access
- the demo's ~20-line send path becomes ~3 lines
- `resolve`'s unsafe is audited once inside the crate (recv safe by construction)
- guard handed back on Full
- design against both ring flavors (SPSC + MPSC)
- the sender is also where each sender's private overflow pending list will live.

### Overflow FIFO

On ring Full, append the message to a sender-private pending list instead of failing
[details](notes/ring-buffer-design.md#overflow-fifo-future):
- intrusive: the same embedded next-link the free-stack uses, so zero allocation
- naturally bounded by pool capacity
- composes per-sender with MPSC, see [Overflow
  readiness](notes/ring-buffer-design.md#overflow-readiness).

### Batch alloc/free demo

Alongside the one-message alloc_free_1t loops, a variant that allocs X messages (5, 10, ...) then
frees them all, pool vs global allocator. We think the pool's rate stays constant (pop/push is O(1)
regardless of live count, LIFO keeps the working set hot) while Box::new/drop slows as the batch
outgrows malloc's thread-cache fast path, and the demo should show it.

### Endpoint claims word

CAS-claimed producer/consumer roles in the ring header so a second attach/split claimant gets an
error instead of silently violating SPSC, at the cost of a layout_version bump (or spends `_pad0`)
[details](notes/ring-buffer-design.md#resolved-questions).

### Typed endpoints

`Producer<T>` / `Consumer<T>` validating `T`'s geometry once at split instead of asserting on every
reserve_slot_with [details](notes/ring-buffer-design.md#api).

## Ideas

Unranked, not yet solid enough for `## Todo`. Triaged at an opening: promoted to `## Todo` or
[todo-backlog.md](notes/todo-backlog.md), folded into a picked-up cycle, or dropped.

- Perf benches live in [iiac-perf](https://github.com/winksaville/iiac-perf) (sibling repo
  `../iiac-perf`), not here. Its calibrated harness compares zc-ring against mpsc et al. directly
  (`zcring-1t`/`zcring-2t` mirroring `mpsc_1t`/`mpsc_2t`). An in-repo bench only if per-commit
  regression tracking proves necessary.

- Fan-in helper: consumer-side composition polling N SPSC rings under a pluggable service policy
  (priority, round-robin, weighted)
  [details](notes/ring-buffer-design.md#fan-in-composition-not-a-mode):
  - buildable today from shipped parts
  - likely offered alongside the MPSC ring eventually, no commitment yet.
- Study [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) before implementing message pools:
  battle-tested loan/send decoupling and pool-offset machinery. How it differs from this project is
  in [Prior art: iceoryx2](notes/ring-buffer-design.md#prior-art-iceoryx2).
- `#[global_allocator]` experiment over size-class pools: GlobalAlloc is `&self` + any-thread, so it
  needs shared-allocation pools (phase 2 gen-tagged head) or per-thread pools with a routing layer,
  and arbitrary `Layout` needs size-class selection + an oversize fallback. Frees from any thread
  are already natural (MPSC push). Classic mempool -> malloc arc. Measure the object-pool form in
  iiac-perf first.
- Private per-handle cache in front of the shared free-stack (tcache-over-arenas): alloc/free hit a
  thread-private list with plain load/store, and refill/flush moves batches to the CAS stack,
  amortizing one CAS over N messages. Motivating datum: 2 uncontended CAS = 8.7 ns of the pool's 9.9
  ns single-thread round trip (vs malloc tcache's zero atomics). Hold until iiac-perf shows per-op
  CAS matters in a composed workload. We think the pool's tail latency (p99, stddev) already beats
  malloc (no arena locks, no brk/mmap), and that matters more than the mean.
- `Message` trait over the payload cast boilerplate: const `MSG_ID` + the zerocopy bounds,
  receiver-side dispatch (read tag, match, cast) without per-call-site ceremony, and maybe a
  transport seam so an embedded pointer-descriptor profile slots in behind the same API
  [details](notes/ring-buffer-design.md#descriptor-and-registry-design-070).
- BufSlot auto-free on Drop (RAII, iceoryx2-style): kills the silent leak-on-drop footgun at the
  cost of guard-type asymmetry (ring guards' drop = do-nothing) and a ManuallyDrop dance in
  free/send paths. Decide when descriptor-queue send lands, since explicit free is easier to upgrade
  than to walk back.
- Blocking layer above the crate (futex, eventfd, async wakers) built on the header's user line,
  mechanism and contracts in [Blocking and user
  words](notes/ring-buffer-design.md#blocking-and-user-words). Possibly a companion wrapper crate so
  independent peers share one protocol.
- loom-based exhaustive ordering exploration of the SPSC protocol.
- Polish: `Error` implements `Display` + `core::error::Error`, and `occupancy()` / `is_empty()`
  accessors.
- Packed-slot variant (drop the cache-line-multiple slot constraint) for small-message space
  efficiency.
- Per-target / configurable `CACHE_LINE` (128 for Apple M-series false sharing, tiny for cache-less
  MCUs), safe since attach validates the header's `cache_line`. Decide values from iiac-perf
  measurements.
- Embedded floor: protocol is atomic load/store only (no CAS), so thumbv6m works today. Keep it that
  way where possible (endpoint claims wants CAS, so gate it), and 8/16-bit targets would need
  index-width genericization.
- Shared `Geometry` struct (`slot_size`, `capacity`, `mask`) held by Ring and passed whole to the
  endpoint constructors, slimming their signatures and Ring's fields.
- Black-box test split: move the public-API protocol tests (roundtrip, abandoned guards, threaded
  stress) to `tests/protocol.rs`, while white-box tests (u32 wrap, attach header internals) stay in
  lib.rs. Do it when a trybuild compile-fail harness lands there too (pins the "second reservation
  does not compile" guarantee).

## Bugs

_See [bugs.md](notes/bugs.md)._

## Closed

The last cycle's finished record, moved here whole by its closing commit and deleted by the next
opening ([Cycle-record](AGENTS.md#cycle-record)). Earlier cycles are in the landmark commit's copy
of this section, and the cycles before the rule in the frozen [notes/chores/](notes/chores) and
[notes/done.md](notes/done.md).

_None._

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #feat-in-slot-seq-spsc-v2-opening
[2]: #feat-runtime-depth-in-the-demo-and-tp-matrix
[3]: #feat-add-the-spsc-v2-in-slot-seq-ring
[4]: #feat-spsc-v2-as-a-fourth-flavor
[5]: #perf-probe-a-fence-after-the-v1-commit
[6]: #perf-measure-spsc-v2-across-depths
[7]: #feat-a-streaming-cell-with-fill-counts
[8]: #docs-sweep-punctuation-in-the-touched-files
[9]: #feat-in-slot-seq-spsc-v2-closing
