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

A `spsc::v3` module whose ring is a chain of v2 segments, each a v2 ring in one pool buffer, in the
same shape as v0 through v2 and the crate's default.

- `spsc::v3::Ring::init(region, slot_size, seg_capacity, seg_count)` builds a `Pool` over the
  region, one buffer per segment, takes the first segment, and splits into `Producer` and
  `Consumer`. Capacity is the product, and an exhausted pool is the ring's Full.
- Inside a segment it is v2, and the guards are v2's `WriteSlot` and `ReadSlot`.
- The link to the next segment is word 0 of a segment's user line, the next buffer's index or
  `u32::MAX` for none, written only by the producer.
- Producer on Full: try the segment once, then allocate a buffer, init a v2 ring in it, store its
  index in the old segment's link, and move. The wait policy covers both tries.
- Consumer on Empty: load the link. Unset means the producer is still here, so wait. Set means try
  the old segment once more, and only if it is still Empty move to the new segment and free the old
  one, since every commit to the old segment came before the link store.
- The pool gains `BufSlot::as_mut_bytes` for the init and a crate-private buffer pointer on the
  resolver for the consumer's attach and free.
- No `attach` for v3 this cycle. A user who needs one names `spsc::v2::Ring`.

#### Acceptance check

A test streams across many segment boundaries with a pool smaller than the message count, at
`M = 1` and larger, and afterwards finds every segment but the live one back in the pool. A
threaded stress with a two-buffer pool forces the exhausted wait and the second look. `Ring` at the
crate root is v3. The segment-size sweep on the 3900X is in the design note, with v3 at `M = depth`
matching v2 within run noise. `vc-x1 validate` passes.

#### Ladder

- [feat: segmented queue SPSC v3 opening][1] (done)
- [feat: bytes and a buffer pointer from a pool buffer][2]
- [feat: spsc v3 segment chain][3]
- [feat: spsc v3 in the measurement tools][4]
- [perf: sweep the segment size][5]
- [feat: segmented queue SPSC v3 closing][6]

#### Deliberation

- On v2: v2 cleared the bar on 2026-09-07, and within a segment v3 is v2, so v3's cost is the seams.
- The v0 through v2 shape, the user's call on 2026-09-14: `spsc::v3::Ring` with `init`, `split`,
  and the same endpoint and guard names, not the draft's `Queue`.
- v3 is the crate default from its own rung, the user's call: one rung moves the call sites that
  need v2's geometry to `spsc::v2::Ring`, rather than a late rung touching them again.
- The second look, found while checking the draft: an Empty read before the producer's last commits
  goes stale by the time the consumer sees the link, and freeing then loses those messages. The
  user walked through it and agreed.
- No seal and no CAS at the seam: one producer, so a plain link store is safe. MPSC will be its own
  implementation, and what v3 teaches about the seam goes into the design note for it.
  - Carried to MPSC: producers racing to link CAS it, and losers return their segments to the
    pool; the old segment is sealed before the link, so no late claim lands in it; and a slow
    producer may still hold a freed segment, so reclamation is the hard part.
- No v3 `attach`, the user's call: attach is a ring's ability to join an existing region, not a
  versioning question, and v3's state spans a pool and a chain. A Todo entry holds it.
- 0.16.0, a minor bump, the user's call: a new queue layer and a new default.
- No `-dev` rename: the demo's name is unchanged by the cycle, as in the earlier cycles.
- Prediction, on record from the draft: within a segment v3 is v2. Each seam costs the producer an
  alloc CAS, the init's header and seq lines, and a link store, and the consumer a header line for
  the link, a free CAS on the shared pool line, and a cold prefetcher on the new buffer. At `M = 1`
  roughly three times v2's two lines per message. We think the cost is flat by `M = 8`.
- `## Waiting` is `_None._`, nothing to promote.

#### Ladder details

##### feat: segmented queue SPSC v3 opening

The cycle's setup commit: publish the bookmark, clear `## Closed`, move the Todo entry into this
block, file the v3 attach Todo, and bump the version to 0.16.0-0.

##### feat: bytes and a buffer pointer from a pool buffer

The pool hands out a buffer only as a typed guard. v3 needs the buffer's bytes to init a v2 ring in
it, and the consumer needs a buffer's address from its index. Add both, with tests.

##### feat: spsc v3 segment chain

The `spsc::v3` module: `Ring`, both endpoints, and the seam protocol, with the acceptance tests
plus `M = 1`, u32 wrap, and the threaded stress. The crate default moves to v3, v2-specific tests
name `spsc::v2::Ring`, and the README example shows v3's `init`.

##### feat: spsc v3 in the measurement tools

`spsc-v3` as a flavor in `tp-cell`, `tp-matrix`, `tp-stream`, and the demo's sweep, with a segment
capacity knob. Depth stays the total capacity, so `M = depth` is v2 inside v3.

##### perf: sweep the segment size

`M` from 1 to depth at depths 64 and 256 across the three pinned placements, into a new design-note
section with the "carried to MPSC" list, and the 7600X pasted in by the user.

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
[2]: #feat-bytes-and-a-buffer-pointer-from-a-pool-buffer
[3]: #feat-spsc-v3-segment-chain
[4]: #feat-spsc-v3-in-the-measurement-tools
[5]: #perf-sweep-the-segment-size
[6]: #feat-segmented-queue-spsc-v3-closing
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
