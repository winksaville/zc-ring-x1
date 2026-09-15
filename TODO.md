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

### feat: segmented queue SPSC v3

#### Problem

Every SPSC ring is fixed-length, so a full ring fails the send. The segment chain designed in [SPSC
v1: seam-word ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) waited for a ring worth
building on, and v2 cleared that bar on 2026-09-07.

#### Solution

A design to try, agreed with the user on 2026-09-14, not a proven one: if it does not work or is not
fast enough, its measurements and lessons carry into a v4.

The goal: a segmented ring holds a few segments, and when the consumer keeps up only one is in use
and v3 costs what v2 costs. The other segments are insurance for the producer outrunning the
consumer, and switching between them is the only cost v3 adds.

- A `spsc::v3` module in the v0 through v2 shape, `Ring` with `init` and `split`, `Producer`,
  `Consumer`, `WriteSlot`, `ReadSlot`, and the crate's default. v2 is unchanged, so v0 through v3
  measure side by side, and v3 borrows from v2 only where convenient.
- Segments are set up once: `init` borrows the application's pool, takes all of a ring's segments
  from it with `Pool::alloc_bytes`, at most 32 of them, and initializes each. Nothing
  allocates or frees while the ring runs, and the ring does not keep the pool's allocator.
- A segment is a ring of its own, each identical in operation: a pool buffer of slots, each slot
  opening with its seq word as in v2, at a fixed segment depth. Segments are named by a small
  number, and indexes are used over pointers, as the pool does.
- The slot's seq word is 32 bits, so one load gets every bit the consumer needs: the seq value, as
  v2's claimable, committed, and released, in the low 26 bits, and a MOVED bit with the next
  segment's number in the high bits.
- Producer at commit: if the next slot is claimable, commit as v2 does. If not, the segment is about
  to be full, so take a free segment and commit this message with MOVED and that segment's number.
  A slot seen claimable stays claimable until the producer claims it, so the look-ahead replaces
  the next reserve's load. With no free segment it commits plainly, and a later reserve waits on
  the current segment as a ring does.
- Consumer: one load of the slot's seq says empty, a message, or a message and then go to segment
  k. After a MOVED message it releases the slot, gives the old segment back, and switches. No link
  load and no second look: the MOVED message is the producer's last commit in that segment.
- Free segments without CAS: the producer keeps a private word P and the consumer a shared word C,
  one bit per segment. The producer flips a segment's bit in P when it takes it, the consumer in C
  when it gives it back, so a segment is free where the bits agree, and `trailing_zeros` of
  `!(P ^ C)` finds one in one instruction. C is touched only on a switch.
- A segment picks up where it was left: each side keeps a private resume position per segment, and
  both sides left a segment at the same slot, so reusing one needs no shared write and no
  re-initialization.
- At segment depth 1 every commit switches, so depth 1 measures the switch alone.
- No `attach` for v3 this cycle. A user who needs one names `spsc::v2::Ring`.

#### Acceptance check

A test streams far more messages than one segment holds through a ring of several segments, at
segment depth 1 and larger, and afterwards every segment but the current one is free, P and C
agreeing on them. A threaded stress with two or three segments forces switches, MOVED hand-offs,
and a producer waiting with no free segment. `Ring` at the crate root is v3. The sweep in the design
note shows v3 matching v2 at the same segment depth while the consumer keeps up, and the switch cost
at segment depth 1 while the producer runs ahead. `vc-x1 validate` passes.

#### Ladder

- [feat: segmented queue SPSC v3 opening][1] (done)
- [feat: pool buffers as bytes][2] (done)
- [feat: spsc v3 segment chain][3] (done)
- [fix: the two tests Miri rejects][7] (done)
- [feat: spsc v3 in the measurement tools][4]
- [perf: sweep the segment size][5]
- [feat: segmented queue SPSC v3 closing][6]

#### Deliberation

- A design, not a promise, the user's framing on 2026-09-14: v3 is built to be measured, and a
  design that falls short leads to a v4 rather than a rework of v3.
- Segments as insurance, the user's goal: with a consumer that keeps up, the ring lives in one
  segment and costs what v2 costs, so every extra cost is kept to the switch.
- Segments set up at `init`, the user's call, replacing the draft's allocate-on-Full: the whole cost
  of creating segments is paid once, and running never allocates, frees, or re-initializes. A
  queue's capacity is fixed at its segments, and memory is reserved while it is idle.
- Each segment a ring of its own, the user's call, replacing the draft's v2 region per segment:
  v2's four-line header is mostly fields a segment never reads, and v3 may borrow v2's slot
  protocol without being v2.
- One load for the consumer, the user's call: an earlier step had the consumer load a link, and
  then a flag beside the seq, on each empty poll.
  - The MOVED bit and next segment ride in the committed seq value, written only by the producer at
    its own commit. A flag set in the word at any other time races the consumer's release store,
    and without CAS a lost flag strands the consumer in a segment the producer has left.
  - This retires the second look, found while checking the draft: that fix answered an empty read
    going stale before the link was seen, and the MOVED commit carries the hand-off in the message
    itself.
- One bit per segment in two single-writer words, the user's call: a bounded segment count makes
  the free set one word, finding a free segment one bit scan, and giving one back one bit flip,
  with no CAS, so v3 stays load and store only like v0 through v2.
- The v0 through v2 shape, the user's call on 2026-09-14: `spsc::v3::Ring` with `init`, `split`,
  and the same endpoint and guard names, not the draft's `Queue`.
- v3 is the crate default from its own rung, the user's call: one rung moves the call sites that
  need v2's geometry to `spsc::v2::Ring`, rather than a late rung touching them again.
- Segments from the application's pool, the user's call: a segment is an ordinary pool allocation,
  taken as bytes since no compile-time type describes it.
- `Header` leaves the crate root and `init` gains `Error::Exhausted` and `Error::BadSegmentCount`,
  the user's calls: v3's segments carry no ring header, and a ring that cannot get all its segments
  or asks for none or too many fails at `init`. The ring borrows the pool only during `init`, so no
  endpoint lends it out.
- A 32-bit seq word and at most 32 segments, the user's call when the 64 bits first designed met
  the crate's 32-bit `no_std` targets, which have no 64-bit atomics: 26 seq bits cap a segment at
  `2^24` slots, and only the consumer's give-back word is shared.
- The Miri fixes as a rung after v3, the user's call on 2026-09-15: running the whole library under
  Miri while checking v3 found two failures that predate it. They are fixed in this cycle rather
  than logged, and after v3, since they are independent of it, so v3 pushes first with nothing set
  aside.
- MPSC will be its own implementation, and what v3 teaches goes into the design note for it.
  - Carried to MPSC: producers racing to take a segment need CAS where v3's single producer does
    not, a segment must be sealed before the switch so no late claim lands in it, and a slow
    producer may still hold a segment being given back, so reclamation is the hard part.
- No v3 `attach`, the user's call: attach is a ring's ability to join an existing region, not a
  versioning question, and v3's state spans a pool and its segments. A Todo entry holds it.
- 0.16.0, a minor bump, the user's call: a new queue layer and a new default.
- No `-dev` rename: the demo's name is unchanged by the cycle, as in the earlier cycles.
- Prediction, on record: with the consumer keeping up, v3 matches v2 at the same segment depth, both
  loading one seq per message on each side. A switch costs the producer a load of C, a bit scan,
  and a store of P, and the consumer a store of C and a cold segment. We think segment depth 1
  with the producer ahead runs within twice v2's cost per message, the new segment's line being the
  larger part.
- `## Waiting` is `_None._`, nothing to promote.

#### Ladder details

##### feat: segmented queue SPSC v3 opening

The cycle's setup commit: publish the bookmark, clear `## Closed`, move the Todo entry into this
block, file the v3 attach Todo, and bump the version to 0.16.0-0.

##### feat: pool buffers as bytes

The pool handed out a buffer only as a guard typed at compile time, so a layout sized at runtime,
such as a v2 ring inside a segment, had no way in.

* Every allocation named a `T` whose size is fixed when the code compiles.
  - `Pool::alloc_bytes` hands out a buffer as a `BufSlot<[u8]>`, which derefs to all of its bytes,
    mutable like any other allocation. Typed views over the bytes are zero-copy casts.
* `BufSlot` and the registry's `into_desc` required a sized `T`.
  - Both take unsized views now, so a bytes guard frees and travels like a typed one.
* A first design added `BufSlot::as_mut_bytes` and a raw buffer pointer on the resolver.
  - Dropped with the user: a buffer is mutable by design, and bytes are one more kind of
    allocation, not a special accessor. The consumer's bytes guard from an index comes with the
    rung that needs it.

##### feat: spsc v3 segment chain

v3 existed only as a design, and the crate's default ring could not grow past one region.

* A ring of segments to measure.
  - `spsc::v3` builds the Solution's design: segments taken from the pool at `init`, the 32-bit
    word with MOVED and the next segment, the producer's look-ahead at commit, the consumer's
    one-load switch, and free segments found through the give-back word.
  - Tests cover one segment as a plain ring, a full ring of segments, many laps in uneven bursts,
    depth 1 switching on every commit, a producer waiting with no free segment, abandoned guards,
    the wait policy, the u32 wrap, and a two-thread stream, which also passed 30 release runs.
  - Under Miri all of v3's tests pass, and 97 of the library's 99. The two that fail do so on the
    commit before this rung too, so they predate v3.
* The crate default was v2.
  - The crate-root `Ring` is v3 and `Header` leaves the root. Code that needs a single region, the
    demo's descriptor rings and the pool, registry, and MPSC tests, names `spsc::v2::Ring`, and the
    README example shows a ring of segments over a pool.
* Laying a v2 ring over each segment would have needed `init` then `attach`, and a switch to
  derive pointers from a guard's `&mut` borrow.
  - Every segment address comes from the pool's own raw pointer, through a crate-private
    `BufSlot::as_mut_ptr`, so both threads' accesses share one pointer origin.

##### fix: the two tests Miri rejects

Two tests failed under Miri with undefined behavior, on the commit before v3 as well as with it, so
the library never passed a whole Miri run.

* `mpsc::v0::tests::mpsc_attach_validates_header` wrote through a ring whose pointers it had
  already invalidated.
  - The fault was the test's: each `as_mut_ptr()` borrows the whole region, and it called it again
    for its third attach and then wrote through the earlier ring's header. It now takes the pointer
    once. v1's copy of the test stops before that write, which is why only v0 failed.
* `mpsc::v1::tests::threaded_mpsc_two_producers_capacity_1` had two producers filling one slot.
  - The fault was the ring's claim: it loaded and CASed `producer_idx` with `Relaxed`, which does
    not promise a producer sees another's claim. Under Miri's weak memory model a stale re-read
    and a CAS on it let two producers win one position, and at `M = 1`, where a committed value is
    the next-but-one claimable value, that is a double fill. The claim's `producer_idx` accesses are
    `SeqCst` now. Only the CAS and the re-read both being strict removed it: either alone still
    raced.
  - We think today's x86 builds never double-claimed, a CAS there being one locked instruction, but
    the claim rested on a guarantee `Relaxed` does not give.
* Result: the whole library, 99 tests, passes under Miri, and again under three more seeds. mpsc
  v0's claim has the same `Relaxed` pattern and Miri does not reject it. We think v0's capacity
  floor of 2 keeps its seq values from coinciding, and v0 is left unchanged as the historical
  sibling.

##### feat: spsc v3 in the measurement tools

`spsc-v3` as a flavor in `tp-cell`, `tp-matrix`, `tp-stream`, and the demo's sweep, with a segment
count knob. Depth stays the segment depth, so v3 and v2 compare at the same depth.

##### perf: sweep the segment size

Segment depth from 1 up across the three pinned placements, the round trip for a consumer that keeps
up and the stream for a producer that runs ahead, into a new design-note section with the "carried
to MPSC" list and the verdict on the design, and the 7600X pasted in by the user.

##### feat: segmented queue SPSC v3 closing

Closing out the cycle, deleting the Overflow FIFO entry v3 supersedes.

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

### Paired columns in the tp_matrix tables

`tp-matrix` prints each phase cell as `mean/stdev` in one column, and `tp-pool` prints two tables per
placement with the same rows and columns, `ns/msg` and `xfills/msg`. Both would read better as one
heading over two columns. Markdown has one header row and no colspan, and the three layouts weighed
on 2026-09-13 each fell short:

- Pair names in the one header row, `m.send` then `±`, `1 ns` then `1 xf`: valid markdown, but the
  pairing is carried by names alone.
- A group row: group names in the header and sub-names as the first body row, which a markdown
  renderer shows as a data row.
- Plain aligned text with a spanning heading: reads best in a terminal, but a paste is no longer a
  markdown table.

### Segmented pools

A pool has one buffer size, so an application wanting messages of several sizes builds and registers
several pools by hand. A segmented pool holds sub-pools of different buffer sizes, and its
`alloc(size)` takes a buffer from the smallest sub-pool that fits, returning the buffer's location
and actual size. Typed access becomes a zero-copy cast on those bytes: the whole buffer as a `T`,
or `T`s at offsets inside it. `alloc::<T>()` stays as `alloc(size_of::<T>())` plus the cast.

- A sub-pool can be a registered pool of its own, so `Desc { pool_id, buf_idx }` already names the
  sub-pool a buffer came from, and `free` already returns it there.
- Buffers start on a cache line, so any `T` aligned to at most a line fits any sub-pool.
- The user's direction on 2026-09-14, raised while settling how v3's segments come from the pool.

### SPSC v3 attach

`spsc::v3::Ring` has no `attach`, so a v3 ring cannot be joined from another process or resumed.
v0 through v2 can, since a ring's whole state is in its region. v3's state spans a pool and a chain
of segments, so attach needs a control block in the region recording both sides' current segment.
Wait for a user that needs it, and until then name `spsc::v2::Ring`.

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

_None._

# References

[1]: #feat-segmented-queue-spsc-v3-opening
[2]: #feat-pool-buffers-as-bytes
[3]: #feat-spsc-v3-segment-chain
[4]: #feat-spsc-v3-in-the-measurement-tools
[5]: #perf-sweep-the-segment-size
[6]: #feat-segmented-queue-spsc-v3-closing
[7]: #fix-the-two-tests-miri-rejects
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
