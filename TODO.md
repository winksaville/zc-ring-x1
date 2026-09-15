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

### feat: segmented queue MPSC v2

#### Problem

Every MPSC ring is one fixed region, so a producer that outruns the consumer finds it Full. SPSC v3
answered that with a ring of segments, and its design note carries a list for MPSC: producers racing
for a segment need a CAS, a segment must be sealed before the switch so no late claim lands in it,
and a slow producer may still hold a segment being given back, so reclamation is the hard part.

#### Solution

`mpsc::v2`, a sibling of v0 and v1 under the same module layout, over v3's segments: up to 32 taken
from the application's pool at `init`, each a ring of its own, no attach, and nothing in v0 or v1
changed so the three bench against each other. One 32-bit claim word, the segment number and the
position, is CAS-claimed by every producer as v1 claims its index, and a stale view fails the CAS,
so the claim word is the seal. A producer at a full segment takes a free one by CAS on a
producer-shared taken word and moves the claim word to it, writing MOVED, the next segment, and the
end position into the old segment's header. The consumer runs v1's loop within a segment and reads
the seal word only when a slot is neither committed nor tombstoned, so its fast path is v1's one
load. A segment is given back only after the consumer passes its end position, by which time every
claim in it has committed, so nothing is reclaimed under a producer.

#### Acceptance check

Tests stream far more messages than one segment holds through several segments at depths 1 to 1024
with one, two, and four producers, and afterwards every segment but the current one is free. A
threaded stress with two or three segments and several producers forces switches, seals, a
tombstone before a seal, and a producer waiting with no free segment. The whole library passes under
Miri. The diff touches nothing under `src/mpsc/v0` and `src/mpsc/v1`. The sweep in the design note
shows v2 against v1 at one producer on the 3900X and the 7600X with switches per message. `vc-x1
validate` passes.

#### Ladder

- [feat: segmented queue MPSC v2 opening][1] (done)
- [docs: mpsc v2 design and prediction][2] (done)
- [feat: mpsc v2 segment chain][3] (done)
- [test: mpsc v2 across segment counts, depths, and producers][4] (done)
- [feat: mpsc v2 in the measurement tools][5]
- [feat: segmented queue MPSC v2 closing][6]

#### Deliberation

- The claim word is the seal, the plan's design, the user's go on 2026-09-15: one packed word of
  segment and position replaces v1's producer index, so a claim can never land in a segment the
  ring has left and the send path pays nothing for sealing.
  - The alternative weighed: a producer index per segment plus a current-segment word, which
    needs a seal bit in every segment's index word and a stale-segment check on every send.
- The switch is two CASes on a rare path: one on the taken word to own a free segment among
  producers, one on the claim word to move the ring. A lost claim CAS flips the taken bit back and
  retries, not a policy call, and Full means no segment is free.
- The seal lives in a header word, never in a slot word: v3 retired the consumer's second look
  because a flag set in the slot word outside the commit raced the release store. In a header word
  it races nothing, and the consumer loads it only on the empty path.
- Reclamation falls out of the seal: a slow producer holds a claimed slot, never a segment, and the
  consumer gives a segment back only after passing its end position.
- Word layout as v3's, 26 seq bits and 32 segments, with v1's tombstone at bit 31, so v2 builds on
  the 32-bit `no_std` targets.
- `MpscRing` stays v1 this cycle, the plan's recommendation: the v3 verdict is that the default ring
  now costs more on every path that never needs a second segment. The Todo entry `MPSC v2 as the
  default` holds the flip, conditioned on v2 matching v1 with no switch.
- A design rung before code, the plan's recommendation: v3 designed in conversation, and this design
  has more moving parts, so the note section is reviewed as text first, with the prediction on
  record before measuring.
- The tools run every MPSC flavor at one producer and one consumer, so v2's rows compare with v1's
  directly. A producer-count flag is the Todo entry `Multi-producer measurement`.
- The segment table is borrowed on every path, never copied, v3's fast-path finding applied from the
  start.
- One in-use word for the free set, not v3's two parity words, the finding of the code rung: two
  producers with views one store apart can agree a segment in use is free. The design note's
  section records the interleaving.
  - The cost accepted: the consumer's give-back is a read-modify-write, on the switch path only.
    Its fast path is still one load.
- The give-back is one poll late, a consequence of the second look being on the empty path only: a
  segment is given back at the reserve after its last release. The alternative, a seal load at
  every release, is a load on the fast path, and the tools rung measures the path as it is.
- 0.17.0, a minor bump, as v3's: a new queue layer.
- No `-dev` rename: the demo's name is unchanged by the cycle, as in the earlier cycles.
- The user's waiver on 2026-09-15, "you have permission to complete the rungs before close-out and
  then we can test and tweak together": it covers the opening push and every rung push before the
  closing, their work and description reviews included. The closing push and Land are outside it.
- `## Waiting` is `_None._`, nothing to promote.

#### Ladder details

##### feat: segmented queue MPSC v2 opening

The cycle's setup commit: publish the bookmark, clear `## Closed`, move the Todo entry into this
block, file the two follow-on Todo entries, and bump the version to 0.17.0-0.

##### docs: mpsc v2 design and prediction

The design note had v3's list of what MPSC needs and no MPSC v2 section, so the protocol was settled
in prose before code.

* The three carried problems, a CAS to take a segment, a seal before the switch, and reclamation
  under a slow producer, had no MPSC answer.
  - The new section
    [MPSC v2: ring of segments](notes/ring-buffer-design.md#mpsc-v2-ring-of-segments) resolves
    them into the packed claim word, with the switch, the consumer's second look, reuse, the API,
    trust, and the alternative weighed.
* v3's second look was retired for a race, and v2 needs one.
  - The section says why the race does not reach v2: the seal is a header word the consumer never
    stores to, read on the empty path only.
* No prediction was on record.
  - Within run noise of v1 from depth 2 up, within twice v1 at depth 1, and the switch's cost
    itemized, for the tools rung to check.

##### feat: mpsc v2 segment chain

v2 existed only as a design, and the crate's MPSC rings could not grow past one region.

* A ring of segments to measure.
  - `mpsc::v2` builds the design: segments taken from the pool at `init` with a three-line header
    each, the packed claim word CAS-claimed as v1's index, the switch at a full segment, the seal
    in the old segment's header, the consumer's second look on the empty path, and reuse through
    the seal's end position. v3's `seq_of`, `validate_geometry`, and `check_body_type` are shared.
  - Tests cover one segment as a plain ring, a full ring of segments, many laps in uneven bursts,
    depth 1 one message behind, a producer waiting with no free segment, the policies, an
    abandoned read guard, a tombstone mid-segment and one before a seal, a stale seal on a reused
    segment, the 26-bit wrap, and two, four, and shared-reference producers across threads. The
    threaded tests passed 30 release runs, and the module passes under Miri.
* The two-producer stress deadlocked, one run in three.
  - v3's free set, a producer-private taken word against the consumer's give-back word, is sound
    for one producer and not for several. A producer that slept with a stale view woke to the
    taken word reading the same bits again and took a segment that was in fact free, and a second
    producer read the fresh taken word beside a give-back word one consumer store stale, the two
    parities agreed, and it took the segment the first held with the ring inside it. The free set
    is one in-use word now: a take is a `fetch_or` that succeeds only where the bit was clear, and
    the consumer's give-back a `fetch_and`, its one read-modify-write, on the switch path only.
  - Found with a trace of every take, move, loss, and follow, dumped when the consumer stalled.
* The seal's clear could wipe a live seal.
  - A first cut cleared the new segment's seal after the claim CAS, and a producer delayed there
    cleared a seal a later producer had already written into that segment. The clear comes before
    the CAS, and a lost CAS restores the resume position it held.
* A segment is given back one poll late.
  - The consumer reads the seal only when a slot is neither committed nor tombstoned, so it gives
    a segment back at the reserve after its last release, not at that release. At depth 1 a
    consumer one message behind needs three segments where v3 needs two. The tests and the note
    say so.

##### test: mpsc v2 across segment counts, depths, and producers

Multiple segments under several producers were shown working only by tests over a few chosen
shapes.

* The tests covered a handful of segment counts, depths, and producer counts.
  - Three tests run every count from 1 to 32 at depths 1, 8, 64, and 1024: filling every segment
    with the consumer idle and draining, bursts of every size up to capacity, and a stream from
    one, two, and four producer threads with per-producer order checked. Both ends count the same
    switches, a filled ring used every segment, and afterwards every segment but the current one
    is free. Under Miri they run a corner of the matrix, and pass.
* Nothing showed a switch happening under contention.
  - `examples/mpsc_v2_segments.rs` runs the same matrix and prints a fill table and a stream table
    per producer count. Filled, a ring of n segments used all n with n - 1 switches at every
    depth. Streaming 100,000 messages at depth 1, one producer switched on nearly every message
    from three segments up, two producers on about nine in ten, and four on six to eight in ten,
    since a producer that finds the slot unread while another is mid-fill waits on the policy
    rather than switching. At depth 8 and up a few hundred switches per run at most.
  - Its nanoseconds are a hundred thousand messages on unpinned threads, a sign of life rather
    than a measurement, which the tools rung makes.

##### feat: mpsc v2 in the measurement tools

The tools measure v0 and v1 and not v2, so nothing can say what v2 costs against v1: add the flavor,
sweep both machines, and write the tables and verdict into the note.

##### feat: segmented queue MPSC v2 closing

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

### SPSC v3 fast path

With the consumer keeping up, `spsc::v3` should cost what v2 costs, and on 2026-09-15 it cost about
three times as much where no segment switch happens, the demo's one-thread loop reading 24 ns per
message against v2's 7.5.

- Found: `WriteSlot::commit` and `ReadSlot::release` begin `let segs = st.segs;`, copying the ring's
  whole segment table, about 280 bytes, on every message. `let segs = &st.segs;` in both took the
  same-CCX stream at depth 64 from 18.4 to 10.7 ns per message, against v2's 4.5 to 5.7, and the SMT
  pair from 21.8 to 14.3, against 8.2.
- Apply that, then find what keeps v3 behind v2 on the fast path, measured against v2 at each step.
- The user's call on 2026-09-15, deferred from the segmented queue cycle so that cycle makes
  multiple segments work first.

### SPSC v3 attach

`spsc::v3::Ring` has no `attach`, so a v3 ring cannot be joined from another process or resumed.
v0 through v2 can, since a ring's whole state is in its region. v3's state spans a pool and a chain
of segments, so attach needs a control block in the region recording both sides' current segment.
Wait for a user that needs it, and until then name `spsc::v2::Ring`.

### MPSC v2 as the default

`MpscRing` is v1 while `Ring` is v3, so the crate's SPSC default is segmented and its MPSC default
is not. Flip the re-export to `mpsc::v2` once v2 matches v1 where no switch happens, measured in the
tools, since the v3 verdict on 2026-09-15 was that a segmented default that does not match costs
every path that never needs a second segment.

### Multi-producer measurement

The tools run every MPSC flavor at one producer and one consumer, so a claim word contended by
several producers is never measured. A `--producers N` flag for `tp-matrix` and `tp-stream` would
run N pinned producers into one consumer, and v2's switch would then be measured under contention.

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

[1]: #feat-segmented-queue-mpsc-v2-opening
[2]: #docs-mpsc-v2-design-and-prediction
[3]: #feat-mpsc-v2-segment-chain
[4]: #test-mpsc-v2-across-segment-counts-depths-and-producers
[5]: #feat-mpsc-v2-in-the-measurement-tools
[6]: #feat-segmented-queue-mpsc-v2-closing
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
