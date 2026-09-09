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

### fix: mpsc handling of capacity 1

#### Problem

The MPSC ring accepts capacity 1 and its protocol collapses there: the producer commits `pos + 1`
and the consumer releases `pos + capacity`, the same value at `M = 1`, so after the first release
each side reads the other's state and both spin forever. Bug 2 in [bugs.md](notes/bugs.md), found
by the demo's depth sweep, and the measurement tools skip the MPSC cell at depth 1 until it is
fixed.

#### Solution

The MPSC ring keeps its layout and takes the spsc v1 seq values: claimable at `pos`, committed at
`pos + M + 1`, released at `pos + M`, tested by equality rather than a signed diff, so `M` is any
power of two down to 1 with `M` usable slots. The layout version bumps, since the seq values a
region carries change. The tools drop the MPSC depth floor, and the ring is measured at depths 1,
2, 8, and 64 beside its recorded numbers to check the prediction.

#### Acceptance check

`vc-x1 validate` passes, including MPSC tests at `M = 1`, `2`, and a larger power of two, two
threaded producers and a tombstone at `M = 1` among them. The demo sweep and `tp-matrix` run the
`mpsc` flavor at depth 1 with no skipped cell. `notes/ring-buffer-design.md` carries the fixed ring
at depths 1, 2, 8, and 64 on the 3900X, matching the recorded numbers within run noise from depth
2 up.

Prediction, on record: within noise the fixed ring is the old one at every depth from 2 up, since
the layout and the line traffic are unchanged and only the constant the seq is compared against
moves. At depth 1 it runs lockstep, at about the round-trip cost.

#### Ladder

- [fix: mpsc handling of capacity 1 opening][1] (done)
- [fix: commit at pos + M + 1 in the mpsc ring][2]
- [perf: measure the fixed mpsc ring across depths][3]
- [fix: mpsc handling of capacity 1 closing][4]

#### Deliberation

- A fix in v0, not a sibling v1, the user's call at the opening review (2026-09-09), reversing the
  first draft: the layout, the line traffic, and the atomic ops are unchanged and only the constants
  the seq is compared against move, so a sibling would be a second copy of the code with nothing to
  A/B.
- Equality instead of the signed diff is forced, not chosen: with committed at `pos + M + 1`, a
  full slot's previous-lap value is `pos + 1`, which the diff reads as a lost race rather than
  Full. Equality with `pos` decides claimable, and a re-read of `producer_idx` separates stale from
  full, as the negative branch already does.
- The layout version bumps though the layout is unchanged: a region written by the old build and
  attached by the new would misread its seq words, so the bump makes that attach fail toward the
  layout error rather than misbehave. Only mixed builds across processes could reach it.
- The flavor label stays `mpsc`: the v0 flavor is the bare name in the tools, `spsc` beside
  `spsc-v1` and `spsc-v2`, so an eventual MPSC v1 is `mpsc-v1` and the recorded tables keep their
  column.
- The tools' depth floor drops in the fix rung rather than its own: `min_depth` to 1 is two lines,
  and the fix is what makes it true.
- The measurement rung stays, fix or not: the prediction is on record, and the design note's
  `mpsc` columns were measured on the old values, so depth 1 needs a number and the rest a
  confirmation.

#### Ladder details

##### fix: mpsc handling of capacity 1 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write this
block from the bugs entry, bump the version-of-record, and rename the package to its dev name. The
continuation notes held the draft of the segmented-queue cycle, so the opening folds it into that
Todo entry, retitled for v2 segments, and resets the notes. The first draft opened as a sibling
`mpsc::v1` and was reshaped to a fix at the review, the bookmark renamed with it, before any commit.

##### fix: commit at pos + M + 1 in the mpsc ring

The ring hangs at capacity 1 and the tools work around it. The seq values become claimable `pos`,
committed `pos + M + 1`, released `pos + M`, the checks equality, the layout version bumps, the
tests cover `M = 1` at every protocol point, the tombstone and the u32 wrap included, the tools'
depth floor drops to 1, and the bugs entry retires.

##### perf: measure the fixed mpsc ring across depths

The prediction is on record and nothing tests it. The sweep at depths 1, 2, 8, and 64 across the
pinned placements, the numbers into the design note beside the recorded `mpsc` columns, the 7600X
pasted in by the user.

##### fix: mpsc handling of capacity 1 closing

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

### Segmented queue SPSC v3

Every ring is fixed-length and Full fails the send. The segment chain was designed for v1 in [SPSC
v1: seam-word ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) and held until a ring
cleared the bar, v2 cleared it on 2026-09-07, so the layer goes on v2. The cycle, drafted on
2026-09-08 and not yet agreed, runs as `feat: segmented queue SPSC v3`:
- Solution: a `spsc::v3` sibling module whose queue is a chain of v2 ring segments, each one pool
  buffer holding a v2 region.
  - Geometry: `Queue::init(region, slot_size, seg_capacity, seg_count)` builds a `Pool` over the
    caller's region with buffer size `v2::region_size(slot_size, seg_capacity)` and `seg_count`
    buffers, takes the first segment, and splits into `Producer` and `Consumer`. Total capacity is
    the product, and the pool bound is the queue bound, so Full means the pool is exhausted.
  - Endpoints: the producer holds the `Pool` plus a v2 producer over its current segment, the
    consumer a `PoolResolver` plus a v2 consumer over its segment. The guards are v2's `WriteSlot`
    and `ReadSlot` re-exported, so the hot path inside a segment is v2's untouched.
  - Link word: word 0 of the segment header's `user` line holds the next segment's pool buffer
    index, sentinel `u32::MAX` for none. Not the free-stack word, which is buffer word 0 and
    doubles as the v2 magic, so a free scribbling it costs nothing.
  - Producer on Full: try the current segment once, on Full allocate a segment, `v2::Ring::init`
    it, store its index in the old segment's link with Release, move. Pool exhausted is the queue's
    Full, and the wait policy runs over both retries, the current segment first.
  - Consumer on Empty: try the current segment once, on Empty load the link with Acquire. A set
    link means the old segment is drained, since every commit in it happened before the link store
    and Empty says the slot at the consumer's position is uncommitted, so the consumer moves and
    frees the old segment. An unset link means the producer is still here, and the wait policy
    runs.
  - Pool change, the one the design note allowed: `alloc::<()>` already hands out a whole buffer,
    so add `BufSlot::as_mut_bytes` for the init and a crate-private buffer pointer on the resolver
    for the consumer's attach and free.
  - Held out: no `attach` this cycle. The endpoints are in-process, since resuming needs a queue
    control block holding both sides' current segment, a later entry if a consumer for it appears.
- Acceptance check: a test streaming across many segment boundaries with a pool smaller than the
  message count, at `M = 1` and larger, that afterwards finds every segment but the live one back
  in the pool, and the size sweep on the 3900X in the design note with v3 at `M = depth` matching
  v2 within run noise.
- Prediction, on record: within a segment v3 is v2. Each seam costs the producer an alloc CAS, the
  init's header and seq lines, and a link store, and the consumer a header line for the link, a
  free CAS on the shared pool line, and a cold prefetcher on the new buffer. At `M = 1` roughly
  three times v2's two lines per message, and the slope from `M = 64` down shows where the seam
  stops mattering. We think it is flat by `M = 8`.
- Ladder, multi-step on one topic bookmark:
  - `feat: segmented queue SPSC v3 opening`
  - `feat: bytes and a buffer pointer from a pool buffer`, the pool accessors with tests
  - `feat: spsc v3 segment chain`, the module, both endpoints, the link protocol, the tests above
    plus `M = 1`, u32 wrap, and a threaded stress with a two-buffer pool that forces the exhausted
    wait and the unset-link retry
  - `feat: spsc v3 in the measurement tools`, a flavor in `tp-cell`, `tp-matrix`, `tp-stream`, and
    the demo's sweep, with a segment-capacity knob. Depth stays the total capacity and the segment
    count is depth over `M`, so `M = depth` is the v2 baseline inside v3
  - `perf: sweep the segment size`, `M` from 1 to depth at depths 64 and 256 across the three
    pinned placements, the numbers into a new design-note section and the crate's default `M`
    chosen from them, the 7600X pasted in by the user
  - `feat: segmented queue SPSC v3 closing`, which deletes the Overflow FIFO entry this supersedes
- Decisions the user has not yet given: `Ring` stays v2 and v3 is reached by path as
  `spsc::v3::Queue`, attach deferred, trapezoid at close-out.
- supersedes Overflow FIFO if it lands.

### Demo pin-pair picker

The demo's "diff cores" pair is the first cpu outside cpu0's L3, cross-L3 on the 3900X and same-L3
on the 7600X, so the two machines' lines with the same label were different experiments. The picker
wants a same-L3 placement and a cross-L3 one, each labelled by what it is, and the same for
`tp-matrix`'s placements.

### Sweep punctuation in the design note

`notes/ring-buffer-design.md` carries some 190 banned characters and its share of prose
semicolons, and the in-slot seq cycle touched it without paying them, since a count that size is a
rewrite rather than a repunctuation and the prose rule makes that its own cycle ([Typeable
punctuation only](agent-data/prose.md#typeable-punctuation-only)). Convert the file whole, re-point
the inbound links of any heading whose anchor moves, and touch nothing else.

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
[1]: #fix-mpsc-handling-of-capacity-1-opening
[2]: #fix-commit-at-pos--m--1-in-the-mpsc-ring
[3]: #perf-measure-the-fixed-mpsc-ring-across-depths
[4]: #fix-mpsc-handling-of-capacity-1-closing
