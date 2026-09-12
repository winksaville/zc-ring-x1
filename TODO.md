# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

- No cycle is open. `agent-files(adoption): v0.2.4` landed on main at 8d4cdd2babb3 on
  2026-09-12 (UTC), the artifact installed at 0.15.9, and both repos were clean after Land. The
  working copy holds this note, a Todo stub `### Implement a MPSC using linked list` with an
  unfinished sentence, and `intrusive-rust-link-lists.md` at the repo root, an 11-line question
  about linked-list-based MPSC/SPSC FIFOs in Rust with preallocated messages and embedded links,
  a draft of that entry. The user decides tomorrow whether the stub becomes the entry, with the
  file folded in, and where it ranks against the proposal below.
- The proposed next cycle, single-step `test: exercise cordyceps MpscQueue`, dated 2026-09-11 and
  not yet approved: `cordyceps = "0.3"` as a dev-dependency, `tests/cordyceps_mpsc.rs` covering
  FIFO, Empty, two threaded producers with per-producer order, Busy under a held Consumer, drop
  handing back enqueued nodes, and Inconsistent counted in the threaded test, plus a "Prior art:
  cordyceps MpscQueue" section beside the iceoryx2 one in the design note. No `-dev` rename. The
  bookmark push `jj git push --named test-exercise-cordyceps-mpscqueue=@- -R .` waits on the
  user's go.
- The cordyceps queue is Vyukov's intrusive MPSC: wait-free two-atomic push, single consumer,
  an Inconsistent window between a producer's head swap and its link store, a stub node, nodes
  caller-owned as `Pin<Box<T>>` through the `Linked` trait. Pointers are not forbidden in our
  layout, they are unsafe-heavy in Rust, which is the reason an own version would use offsets.
- Messaging, `../vc-x1-messages`, is at README v0.3.2, read. We answered m-3-0 with m-3-2,
  accepted, and m-4-0 with m-4-2, adopted with the sha-link. Both lines sit uncommitted in that
  clone beside iiac-perf's, nothing is pending for us as of 2026-09-12T00:30Z, and vc-x1 closes
  both threads. iiac-perf raised a gap in m-3-1, no title form for a commit closing two threads,
  vc-x1's pick.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

_No cycle currently in progress._

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

### agent-files(adoption): v0.2.4

#### Problem

The family's agreed set is v0.2.4, landed by vc-x1's proposal at `e378ce9ee494` on 2026-09-11,
and ours was v0.2.3, so `vc-x1 agent-files diff ../vc-x1 -c` reported two of eleven differing:
the version file and `custom.md`. The change is one clause. The messaging pointer said a session
reads our inbox at acquaint, and the messages README has had no inbox since v0.3.0, what a session
reads there is what is pending for us.

#### Solution

A single-step adoption, the source's set taken whole: `agent-data/agent-files-v0.2.3` renamed to
`agent-data/agent-files-v0.2.4` and the clause reworded to "reads what is pending for us there",
so the diff against `../vc-x1` reports nothing differing. Done as stated, the two files the whole
of it.

#### Acceptance check

`vc-x1 agent-files diff ../vc-x1 -c` reports 0 of 11 differing, `vc-x1 agent-files version`
prints `v0.2.4`, and `agent-data` holds no `agent-files-v0.2.3`.

Passed (2026-09-11): the diff reports all eleven files the same, the version prints `v0.2.4`, and
`ls agent-data` lists `agent-files-v0.2.4` alone among the version files.

#### Ladder

- agent-files(adoption): v0.2.4 (done)

#### Deliberation

- Single-step: two files and no design, so one commit carrying the bare `0.15.9`, no dev rename,
  as the v0.2.3 adoption did.
- Adoptions copy and bump nothing: the version file is the source's, so `v0.2.4` arrives by rename
  rather than by a bump of our own ([Agent-files
  version](agent-data/versioning.md#agent-files-version)).
- The reference checkout is `../vc-x1`, the family's payload, not `../vc-x1-template`, which still
  holds the unversioned 2026-08-31 set, so a diff against it names most of the set and says
  nothing about this adoption.
- The working copy held more than the adoption at the opening, the continuation note, a
  half-written Todo entry, and a stray note file at the root. They were set aside as a patch in
  `tmp/` so this commit carries the adoption alone, and they return once it has pushed
  ([Unplanned work](AGENTS.md#unplanned-work)).
- The continuation note's facts had homes already, the SPSC v3 draft in `## Todo` and thread m-2
  closed, so the section resets to `_None._`.
- `## Waiting` is `_None._`, nothing to promote.

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
