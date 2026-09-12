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

### feat: cordyceps MpscQueue beside the mpsc rings

#### Problem

The question behind the linked-list entry is whether an intrusive MPSC, preallocated messages
carrying their own links, beats a pool plus a descriptor ring at the loop the messaging layer is
built on: take a message from the pool, fill it, push its reference, receive it, process it, return
it. cordyceps's `MpscQueue` is a ready Vyukov implementation of the first shape and nothing here
exercises it, and the depth sweep measures the in-slot path, which a linked list cannot take, so no
table answers the question.

#### Solution

Take `cordyceps` as a dev-dependency with a test file that characterizes its queue and a prior-art
section beside iceoryx2's. Add `tp-pool` to the tools crate, a fixed-count sweep of the pool-message
loop over `spsc-v2`, `mpsc-v1`, and cordyceps, every row on one pool of X buffers, X at 1, 100, and
1000, the rings also at depths 1, 8, 64, and 1024, 1M messages a cell across the three placements.
Record the 3900X numbers in the design note beside the mpsc v1 tables, with the prediction first.

#### Acceptance check

`cargo test --test cordyceps_mpsc` passes. `tp-pool --pool 1,100,1000 --depth 1,8,64,1024` prints
one table per placement, rows `spsc-v2` and `mpsc-v1` at each depth and `cordyceps` as one row, cells
ns per message over 1M messages, every cell filled. The design note carries those tables from the
3900X with the prediction they are read against.

#### Ladder

- [feat: cordyceps MpscQueue beside the mpsc rings opening][1] (done)
- [test: exercise cordyceps MpscQueue][2]
- [feat: tp-pool, the pool-message sweep][3]
- [perf: sweep pool size over descriptor rings and cordyceps][4]
- [feat: cordyceps MpscQueue beside the mpsc rings closing][5]

#### Deliberation

- Multi-step, not the single-step test cycle drafted on 2026-09-11: the sweep is what answers the
  question, and the test rung characterizes the queue the sweep leans on, so the two run as rungs
  of one cycle rather than a test cycle and a perf cycle apart.
- The pool bounds the sweep, not the ring: with X messages preallocated at most X descriptors are
  ever in the ring, so a depth at or above X never reports Full and measures the same bound as
  depth X. The swept axis is X, which is what cordyceps shares, since an unbounded queue has no
  depth of its own.
- Ring depths 1, 8, 64, and 1024: for every X the rows below it are the ring throttling before the
  pool does, and the first at or above it is the like-for-like row against cordyceps, 1024 covering
  X = 1000.
- Pool sizes 1, 100, and 1000: the pool takes any count, the rings need powers of two, so the X
  axis is decimal and the depth axis binary. X = 1 is one message in flight, the lockstep ping the
  demo's pool cells run today, and X = 1000 is 64 KB of buffers, past L1 and inside L2, so that
  column measures the queue plus payload lines reused from L2, which the note says when it reads
  the numbers.
- One pool for every row: the queue is the only variable when spsc-v2, mpsc-v1, and cordyceps all
  alloc from and free to the same pool, the rings carrying a descriptor and cordyceps the buffer's
  pointer with its link inside the buffer beside the free-stack's word. cordyceps's `Linked` trait
  leaves the handle type to the implementer, so a pool buffer as the handle is the first thing the
  tools rung checks. The fallback is X boxed nodes returned through a second `MpscQueue` as the
  free list, and then the row measures a different allocator too, which the note would say.
- A new binary in the tools crate: `tp_matrix` can take cordyceps as a plain dependency, where the
  demo cannot see a dev-dependency and would need a feature on the library, and `tp-stream` runs
  for a duration where this sweep runs a count.
- Fixed count, 1M messages, the demo's convention: a `--count` flag defaulting to it and a
  `--repeat` reporting the median of N, since a cell at X = 1000 runs in tens of milliseconds and
  a single run is noise-prone.
- 1p/1c throughout, mpsc-v1 included, as the existing tables do, so the mpsc row isolates protocol
  cost. A producer-count dimension comes after segments, and the ISR case is a future addition.
- Segments later: `--pool` and `--depth` are list flags, so a segment count and capacity become two
  more and the table a row per segment shape, nothing moving.
- The `-dev` rename at the opening, as the capacity-1 cycle did, since the demo and the tools are
  installed and a mid-cycle install must not clobber them.
- The continuation note's facts: the cordyceps proposal became this cycle, its characterization
  the test rung's intent, the messaging status was acted on at acquaint, and the linked-list
  question in `tmp/intrusive-rust-link-lists.md` is what this cycle answers, so the section resets
  and the Todo stub is not filed.
- `## Waiting` is `_None._`, nothing to promote.
- Waiver, given at the opening's description review on 2026-09-12: the user's "you have
  permission to complete the cycle but do NOT land on main" covers every push from the opening
  through the closing rung, the work and description reviews included, and does not cover Land,
  which waits on the user's review of the finished ladder.

#### Ladder details

##### feat: cordyceps MpscQueue beside the mpsc rings opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write
this block, bump the version-of-record, and rename the package and demo binary to `-dev`.

##### test: exercise cordyceps MpscQueue

The queue is Vyukov's intrusive MPSC: a wait-free two-atomic push, a single consumer, an
`Inconsistent` window between a producer's head swap and its link store, a stub node, and nodes
the caller owns through the `Linked` trait. `cordyceps = "0.3"` as a dev-dependency and
`tests/cordyceps_mpsc.rs` covering FIFO, `Empty`, two threaded producers with per-producer order,
`Busy` under a held `Consumer`, drop handing back enqueued nodes, and `Inconsistent` counted in the
threaded test, plus a "Prior art: cordyceps MpscQueue" section beside the iceoryx2 one in the
design note, its push and its window against the rings' claim CAS.

##### feat: tp-pool, the pool-message sweep

The pool-message loop has no sweep: the demo runs it at one depth over spsc alone, and the tools
run the in-slot path. `tp-pool` in the tools crate runs the loop over `spsc-v2`, `mpsc-v1`, and
cordyceps on one pool, flags `--pool`, `--depth`, `--count`, and `--repeat`, the placements
discovered as `tp-stream` does, one markdown table per placement. Whether a pool buffer can be the
cordyceps handle is settled here.

##### perf: sweep pool size over descriptor rings and cordyceps

Run the sweep on the 3900X and record the tables in the design note beside the mpsc v1 ones, the
prediction written before the run and read against them after.

##### feat: cordyceps MpscQueue beside the mpsc rings closing

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

- MPSC v2, the in-slot seq lesson: v2's seq at the front of its slot halved the SPSC round trip's
  lines and freed the stream from the shared seq line, and the MPSC ring still carries Vyukov's
  seq array. A sibling `mpsc::v2` with the seq in the slot, the claim CAS on the index as now, is
  the next MPSC experiment, after the segmented queue.
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

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #feat-cordyceps-mpscqueue-beside-the-mpsc-rings-opening
[2]: #test-exercise-cordyceps-mpscqueue
[3]: #feat-tp-pool-the-pool-message-sweep
[4]: #perf-sweep-pool-size-over-descriptor-rings-and-cordyceps
[5]: #feat-cordyceps-mpscqueue-beside-the-mpsc-rings-closing
