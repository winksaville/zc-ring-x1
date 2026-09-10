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

An `mpsc::v1` sibling module with v0's layout and the spsc v1 seq values: claimable at `pos`,
committed at `pos + M + 1`, released at `pos + M`, tested by equality rather than a signed diff, so
`M` is any power of two down to 1 with `M` usable slots. Own magic, since a v0 region is not a v1
region. The tombstone stays committed plus `2^31` and the cap stays `2^30`. v0 stays as it is for
the comparison and gains an `init` guard so capacity 1 is an error rather than a hang. v1 joins the
demo sweep and the `tp-` tools as a fifth flavor with a depth floor of 1, every flavor renamed to
the `xpsc-vN` form on the way, and is measured beside v0 at depths 1, 2, 8, and 64, the numbers
into the design note, and the crate's default re-export moves to v1 if they hold.

#### Acceptance check

`vc-x1 validate` passes, including v1 tests at `M = 1`, `2`, and a larger power of two, two
threaded producers and a tombstone at `M = 1` among them, and a v0 test that capacity 1 is rejected.
The demo sweep and `tp-matrix` run the `mpsc-v1` flavor at depth 1 with no skipped cell.
`notes/ring-buffer-design.md` carries v0 beside v1 at depths 1, 2, 8, and 64 on the 3900X, with v1
matching v0 within run noise from depth 2 up.

Prediction, on record: within noise v1 is v0 at every depth from 2 up, since the layout and the
line traffic are unchanged and only the constant the seq is compared against moves. At depth 1 it
runs lockstep, at about the round-trip cost.

#### Ladder

- [fix: mpsc handling of capacity 1 opening][1] (done)
- [feat: add the mpsc v1 equality-seq ring][2] (done)
- [fix: reject capacity 1 in mpsc v0][3] (done)
- [feat: mpsc v1 in the tools, flavors named xpsc-vN][4] (done)
- [perf: measure mpsc v1 beside v0][5] (done)
- [fix: mpsc handling of capacity 1 closing][6]

#### Deliberation

- v1 is a sibling module, not an edit of v0, the user's call at the first rung's review
  (2026-09-09), reversing the opening's call for a fix in place, which had reversed the draft's
  sibling: the change is on the hot path, and a sibling keeps the comparison runnable at any later
  time, where a fix in place leaves it to a two-commit build. The cost accepted is a second copy of
  the ring for a constant. The cycle keeps its pushed title, since the handling of capacity 1 is
  still what it fixes.
- Equality instead of the signed diff is forced, not chosen: with committed at `pos + M + 1`, a
  full slot's previous-lap value is `pos + 1`, which the diff reads as a lost race rather than
  Full. Equality with `pos` decides claimable, and a re-read of `producer_idx` separates stale from
  full, as v0's negative branch already does.
- Own magic rather than a layout version bump: the layout is unchanged, but a v0 region carries
  v0's seq values, and a cross-version attach must fail the way a cross-kind one does.
- Every flavor is named `xpsc-vN`, the user's call (2026-09-09): the bare names `spsc` and `mpsc`
  meant v0 by a rule a reader had to know, and the uniform form removes it. The recorded tables'
  columns and the tools' arguments are relabelled in the same rung, the numbers untouched.
- The default re-export decision waits for the measurement rung: v1 is v0 plus a capability, so
  the numbers holding is the whole case. They held, and the re-export is v1.
- The README's example run is taken at Land, after the rename, the user's call (2026-09-10): the
  README's run is from 0.7.0 and wants a current one under the plain name and the bare version,
  which exist together only between the rename and the fast-forward. The design note keeps this
  rung's tables, cited by rung title, since the code is the same and the analysis is done here.
- The v0 guard is its own rung: v0 stays live for comparison and a hang is worse than an error,
  and a separate commit keeps the v0 diff trivially reviewable.

#### Ladder details

##### fix: mpsc handling of capacity 1 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write this
block from the bugs entry, bump the version-of-record, and rename the package to its dev name. The
continuation notes held the draft of the segmented-queue cycle, so the opening folds it into that
Todo entry, retitled for v2 segments, and resets the notes. The first draft opened as a sibling
`mpsc::v1` and was reshaped to a fix at the review, the bookmark renamed with it, before any commit.

##### feat: add the mpsc v1 equality-seq ring

The ring hangs at capacity 1. The module: v0's files under `mpsc::v1` with the seq values and
equality checks above, its own magic, v0's tests plus `M = 1` at every protocol point, the tombstone
and the u32 wrap included, and a design-note section stating the protocol and the prediction.

* The seq values: at `M = 1` the one word cycles through 0, 2, 1, and the next lap's claimable is
  that 1. Both endpoints carry `capacity + 1` precomputed, so the committed value stays one add on
  the hot path, the review's catch.
  - Equality replaced the signed diff on both sides, and the producer's two not-claimable branches
    became one: re-read `producer_idx`, moved means stale, unmoved means Full.
  - A consequence: a tombstoned previous lap now reaches the wait policy as Full, where the diff
    read it as a lost race and spun with no policy call until the consumer skipped it.
* The tests: the seq values at `M = 1` read directly, a thousand lockstep laps, a tombstone in the
  only slot, the u32 wrap at `M = 1`, the two-producer stress at `M = 1` and 4 through one helper,
  and a cross-version attach failing both ways.
* The rung began as a fix in place in v0 and was reviewed as one, then split into the sibling at
  the user's call, so v0 is untouched by it.

##### fix: reject capacity 1 in mpsc v0

v0 hangs at capacity 1 and the tools work around it. `init` and `attach` reject a capacity below 2,
a test covers it, and the bugs entry retires, since v1 is the fix and v0 the guard.

* The floor is a named constant beside the cap, and the geometry check applies both, so `attach`
  refuses a region another build wrote at capacity 1 as well.
  - The shared `BadCapacity` error's doc now names a floor, since it was the cap and the power of
    two alone.
* The bugs entry's citations moved with it: the tools' floor comments and the v2 tables' note now
  point at the design note's v1 section, which records the finding and both outcomes.

##### feat: mpsc v1 in the tools, flavors named xpsc-vN

The demo sweep and the three `tp-` tools know four flavors, two of them under bare names, and a
depth floor per flavor. v1 joins as `mpsc-v1` with a floor of 1, so the sweep's first MPSC cell is
a number, and every flavor takes the `xpsc-vN` form, `spsc-v0` and `mpsc-v0` included, in the
tools' arguments and labels and in the recorded tables' columns.

* The tools' MPSC cell, stream, and demo loops were concrete functions on the crate's default
  re-export, so a second MPSC version had nowhere to plug in.
  - Each became a macro stamped per version by module path, as the SPSC ones already were, so the
    A/B measures the protocol alone. The demo's two-producer line stays on the default re-export,
    being the one line no SPSC ring has.
* The flavor names and enum variants carried the bare form for v0.
  - `Flavor` and the cell tool's argument are `SpscV0` through `MpscV1`, the labels `xpsc-vN`,
    and the recorded tables' `spsc` and `mpsc` columns read `spsc-v0` and `mpsc-v0` at the same
    width. The demo's older per-line labels keep their function names, since the README's recorded
    output carries them.

##### perf: measure mpsc v1 beside v0

The prediction is on record and nothing tests it. The sweep at depths 1, 2, 8, and 64 across the
pinned placements, a design-note section holding the tables, and the default re-export moved if the
numbers hold, the 7600X pasted in by the user.

* Three instruments on the 3900X at the flavor rung's build, `tp-matrix`, `tp-stream`, and the
  demo, at depths 1, 2, 8, and 64: v1 is v0 within run noise from depth 2 up at every placement,
  and depth 1 is eight lines per trip and a lockstep stream, as spsc v1 at that depth.
  - The default `MpscRing` re-export is v1, the numbers being the whole case. The demo's
    two-producer line moves with it, since it uses the default.
  - One `tp-stream` run read v0 at twice its figures at the SMT pair at depth 2 and 8, and a rerun
    read the record. The rerun is the table, the flip noted as the v2 cycle's regime finding.
* The 7600X did not run at this rung, the user's call at the review (2026-09-10): the new data
  is collected after Land's rename, so the runs carry the plain name and the bare version.
  - The demo, `tp-matrix`, and `tp-stream` built here at release after the rename, copied to the
    7600X and run on both machines: both demo outputs into the README's example run, the 7600X
    tables into the design note's v1 section beside this rung's 3900X ones, and the closing
    amended before the trapezoid. The tools README's install line gained `--locked` so a saved
    banner is a build.

##### fix: mpsc handling of capacity 1 closing

Closing out the cycle. Land's first step, the rename, is followed here by the three instruments
on both machines at the renamed build, the README's example run and the design note's 7600X
tables written from them, and the closing amended before the trapezoid, the user's call
(2026-09-10). The bend: Land's squash carries those pastes beside the rename, and the closing's
body says so.

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
[2]: #feat-add-the-mpsc-v1-equality-seq-ring
[3]: #fix-reject-capacity-1-in-mpsc-v0
[4]: #feat-mpsc-v1-in-the-tools-flavors-named-xpsc-vn
[5]: #perf-measure-mpsc-v1-beside-v0
[6]: #fix-mpsc-handling-of-capacity-1-closing
