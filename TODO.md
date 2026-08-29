# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

- Cycle `feat: segmented seam-word SPSC v1` is mid-ladder on bookmark
  `feat-segmented-seam-word-spsc-v1`, trapezoid at close-out. Three rungs pushed: opening, the v1
  ring, the measurement. Working copy should be empty after the last push.
- Next rung is the inserted `perf: try the in-slot seq`, version-of-record `0.15.5-3` at its
  start. Its gate: v1 must beat MPSC v0 cross-core and not lose to v0 at SMT, or the segment rungs
  do not proceed. Test the hypotheses before building on them, and phrase every analysis as a
  hypothesis until a measurement confirms it (user's standing instruction, 2026-08-28).
- Cheap first probe: line-pad the seq array (one seq per cache line) and rerun the demo's
  streaming lines on the 3900X and the 7600X. If the 7600X cross-core stream (v0 7.0 ns, v1 18.9)
  closes, the false-sharing hypothesis stands and the in-slot seq is the fix; if not, look at the
  two-way-written word itself and at the per-send cost hunt (`WriteSlot` deref path vs `send_with`,
  index store before seq store).
- Measuring: `./target/release/tp-matrix` (5 s per cell, 12 cells) and the installed
  `zc-ring-x1-demo-dev`; the 7600X runs the demo binary copied over by scp. Both are timing runs,
  never overlap them. Numbers so far are in `notes/ring-buffer-design.md` under "SPSC v1:
  seam-word ring".
- Not done: `notes/README.md` update for the new `spsc::v1` module and the `spsc1_` demo lines,
  a closing duty.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### feat: segmented seam-word SPSC v1

#### Problem

The SPSC ring loses to the MPSC ring cross-core (~10.0 cache lines per round trip vs ~6.7, a 26 to
40% loss, [[21]]) because each side reads the other's index line, and both rings are fixed-length:
a Full ring fails the send, and the only overflow design on the books is a per-message pending
list, which pays a pointer chase per message. There is no SPSC that is both faster than MPSC v0 on
the non-overflow path and able to carry an arbitrary number of messages.

#### Solution

A `spsc::v1` sibling, in two layers, with the speed in the first:

- Seam-word ring: MPSC v0's per-slot seq protocol with the producer's CAS removed. Each slot's
  seq word publishes the slot; the producer commits with `seq.store(pos + 1, Release)` and the
  consumer releases with `seq.store(pos + M, Release)`, so each side reads only slot lines and its
  own private index and never the other side's index line. Load/store only, so the no-CAS floor
  holds. `M` is the user's choice, any power of two down to 1, so a sweep isolates the segment seam.
- Segments: a queue is a chain of ring segments allocated from a `Pool`, each segment one pool
  buffer holding a v1 ring region. On Full the producer allocates the next segment, inits it, and
  stores its offset in the current segment's link word (Release), then moves. The consumer, on
  Empty, loads the link (Acquire); a set link means the old segment is fully drained (the producer
  moved only after filling it, and Empty means all of it was read), so the consumer moves and frees
  the old segment. One link per segment, amortized over `M` messages; `M = 1` is the per-message
  linked list, measured rather than imagined.
- Pool: we think unchanged, apart from a way to take a buffer as raw bytes for `Ring::init`, if
  `alloc::<T>` cannot express it. Anything more waits for a shown need.

#### Acceptance check

`vc-x1 validate` passes, including v1 ring tests at `M = 1`, `2`, and a larger power of two, and a
segmented-queue test that crosses several segment boundaries and returns every segment to the pool.
`tp-matrix` with a `spsc-v1` flavor shows a faster round trip than `mpsc` cross-core and no slower
at the SMT placement, and the `M` sweep is recorded in `notes/ring-buffer-design.md`.

#### Ladder

- [feat: segmented seam-word SPSC v1 opening][1] (done)
- [feat: add the spsc v1 seam-word ring][2] (done)
- [perf: measure spsc v1 against v0 and mpsc][3] (done)
- [perf: try the in-slot seq][8]
- [feat: allocate ring segments from a pool][4]
- [feat: add the segmented queue over spsc v1][5]
- [perf: sweep the segment size][6]
- [feat: segmented seam-word SPSC v1 closing][7]

#### Deliberation

- v1 is a sibling module, not an edit of v0: the module layout exists for this (`spsc::v0` stays
  pinned by path for the A/B), and the crate root's default re-export moves to v1 only if the
  numbers earn it, decided at the closing.
- The ring rung and its measurement are separate rungs, and the measurement comes before any
  segment work: the whole bet is the ring protocol, and if v1 does not beat MPSC v0 the segment
  layers are not worth building on it. A failed measurement stops the cycle for a decision, per
  Stop and ask.
- Seq placement is a separate seq array first, as MPSC v0 has it, since that keeps the slot
  contract (`T` fits `slot_size`, cache-line aligned) identical to v0 and the A/B honest. An
  in-slot seq word (the payload and its seq in one line) is the obvious next experiment and is
  left as a finding for the measurement rung, not a commitment.
- The header's index lines stay, diagnostic only: `producer_idx` and `consumer_idx` are the
  endpoints' private counters, and the header copies are for occupancy inspection. The layout
  is v1's own (`layout_version` bump), so nothing v0-attached can misread it.
- The link word lives in the segment's ring header `user` line, word 0, not in the pool's
  free-stack word: the pool stays ignorant of rings, and the free-stack overwrites its word on free
  anyway. The value is the pool buffer index, with a sentinel for none.
- The segmented queue's endpoints hold the pool halves the roles need: the producer holds the
  `Pool` (its one allocator), the consumer a `PoolResolver` to free. That is the pool's existing
  contract, no change.
- The measurement rung failed its gate (v1 at MPSC parity cross-core, not ahead, and 2.5x
  behind v0 on the 7600X's streaming lines) and the user chose to insert an in-slot seq rung
  rather than accept MPSC or stop: the seq riding the slot line is the one lever left on the
  line count, and we think the streaming loss is the separate seq line, a hypothesis the rung
  tests rather than assumes. The segment rungs wait on its result.
- Overflow FIFO in `## Todo` is superseded if this lands; its removal is a closing duty.
- MPSC is out of scope: what the measurements teach is expected to carry to an MPSC v1, and that is
  its own cycle.

#### Ladder details

##### feat: segmented seam-word SPSC v1 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
Todo entry into this block, bump the version-of-record, and rename the demo binary to `-dev`.

##### feat: add the spsc v1 seam-word ring

v0's producer and consumer each poll the other side's index line, and that is where the cross-core
loss to MPSC comes from. The rung adds the seam-word protocol as a sibling.

* The protocol needed a shape that keeps v0's endpoint surface and drops the shared index reads.
  - `spsc::v1` is MPSC v0's seq protocol without the claim CAS: a seq array between the header
    and the slots, the producer claiming on `seq == pos` and committing `pos + M + 1`, the
    consumer reading on that and releasing `pos + M`. Equality checks, since there is no lost
    race to tell apart, so a peer-corrupted seq reads as Full or Empty.
  - The header keeps v0's four-line shape with its own magic and layout version, and the index
    lines are each side's private resume state, `Relaxed` both ways, so a re-attach continues
    mid-stream and nothing hot crosses to the other side.
  - `Producer` / `Consumer` / `WriteSlot` / `ReadSlot` carry v0's `reserve_slot_with` surface
    unchanged, so a caller or a bench flips between v0 and v1 by path alone.
* `M = 1` had to be legal, so a sweep can start at the per-message seam.
  - No sacrificial slot and no index-distance check anywhere; a test alternates a one-slot ring
    through five laps, and the threaded stream runs at `M` of 1, 2, 4, and 16.
  - Those tests caught the first cut: Vyukov's committed value `pos + 1` equals the released
    value `pos + M` at `M = 1`, so the producer overwrote an unread slot and the consumer hung.
    Committed is `pos + M + 1`, distinct from claimable and released at every `M`.
* The design note had no v1 section for the module doc to cite.
  - `notes/ring-buffer-design.md` gains "SPSC v1: seam-word ring" between the MPSC sections and
    the messaging layer, with the in-slot seq and the seq-array padding left open for the
    measurement rung.

##### perf: measure spsc v1 against v0 and mpsc

The v1 ring existed with no measurement beside v0 and the MPSC ring, and the cycle's bet is a
number. The rung is the gate: the numbers decide whether the segment rungs go ahead.

* The cell and the demo loops were written against the v0 `Ring` type by name.
  - v0 and v1 share the endpoint surface and differ by path, so the SPSC cell body and the demo's
    two one-message loops each became a macro instantiated for both, and the A/B measures the
    protocol alone. `tp-cell` gains `spsc-v1` and `all` (the new default), `tp-matrix` runs every
    flavor from one `FLAVORS` list, and the demo prints `spsc1_` lines beside the `spsc_` ones.
* The result had to be recorded where the design lives.
  - `notes/ring-buffer-design.md`'s v1 section carries the numbers: fills per round trip v0 10.0,
    v1 6.85, MPSC 6.7, so the seam word removed the index-line traffic as designed; round trips
    v1 ~26% ahead of v0 cross-core and ~7% behind the MPSC ring, and v0's 2 to 3x SMT and
    single-thread win lost, as the MPSC ring loses it. The bar, faster than MPSC v0 on the
    non-overflow path, is not met by the separate-seq-array form, and the demo reproduces the
    ordering.
  - Recorded with it: v1 does strictly less than the MPSC producer and is a few ns slower per
    send at every placement, an open puzzle with two candidates named (code shape of the
    `WriteSlot` path against `send_with`, and the private index store ahead of the seq store).
  - A 7600X run of the demo reversed the streaming picture, v0 2.5x ahead of both seq protocols
    cross-core. Recorded with it, as a hypothesis and not a finding: that the seq line is
    written by both sides every message and false-shared across 16 slots, which a one-in-flight
    cell cannot show. The gate is not met, and the next rung was inserted on the user's choice.

##### perf: try the in-slot seq

We think the seq word in its own array costs a second line per message and, streaming, a line
both sides write. The rung tests that before building on it (line-padding the seq array is the
cheap probe: if false sharing is the cost, padding cuts the streaming loss), then puts the seq
in the slot's own line and measures again: the `tp-matrix` round trip and the demo's streams on
both machines, against v0 and MPSC, with the per-send puzzle chased while in there. The slot
contract changes (`T` shares the slot with the seq), so the shape of that is part of the rung's
finding, and the segment rungs go ahead only if the numbers now clear the bar.

##### feat: allocate ring segments from a pool

Take a pool buffer as the byte region a v1 `Ring::init` wants, sized by a `region_size(slot_size,
M)` the pool caller uses for `buf_size`; whatever the pool needs for that, and nothing else.

##### feat: add the segmented queue over spsc v1

The chain: producer and consumer endpoints, the link word, segment switch on Full and on Empty,
freeing the drained segment, and the boundary-crossing test.

##### perf: sweep the segment size

`M` from 1 to 256 in the cell, the seam cost as the slope, recorded in
`notes/ring-buffer-design.md` with the segment size the crate defaults to.

##### feat: segmented seam-word SPSC v1 closing

Closing out the cycle.

## Closed

The last cycle's finished record, moved here whole by its closing commit and deleted by the next
opening ([Cycle-record](AGENTS.md#cycle-record)). Earlier cycles are in the landmark commit's copy
of this section, and the cycles before the rule in the frozen [notes/chores/](notes/chores) and
[notes/done.md](notes/done.md).

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

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: notes/chores/chores-02.md#findings-the-gap-is-line-transfer-economics
[1]: #feat-segmented-seam-word-spsc-v1-opening
[2]: #feat-add-the-spsc-v1-seam-word-ring
[3]: #perf-measure-spsc-v1-against-v0-and-mpsc
[4]: #feat-allocate-ring-segments-from-a-pool
[5]: #feat-add-the-segmented-queue-over-spsc-v1
[6]: #perf-sweep-the-segment-size
[7]: #feat-segmented-seam-word-spsc-v1-closing
[8]: #perf-try-the-in-slot-seq
