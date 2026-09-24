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

### feat: segmented pool v1

#### Problem

A pool has one buffer size, so an application wanting messages of several sizes builds and registers
several pools by hand.

#### Solution

`pool::v1::Pool<'a, const N: usize>` keeps v0's API over one region holding N stacks, one per
buffer size, sorted smallest first.

- `alloc::<T>()`, `alloc_with`, and `alloc_bytes(size)` take the smallest size that fits, fall back
  to the next larger stack when that one is empty, and count the miss against the size that was
  wanted, so a user can tell when a size wants more buffers.
- At N=1 the stack choice is the one size check v0 already makes, so the single-stack pool pays
  nothing over v0.
- `free` returns a buffer to its own stack, and a `Desc` still names a buffer by `pool_id` and
  `buf_idx`.
- Buffers start on a cache line, as v0's, so any `T` aligned to at most a line fits any size.
- The default re-export stays on v0, and the rings keep taking a v0 pool.

#### Acceptance check

- The v1 tests pass: size choice, fallback, miss counts, exhaustion, attach validation, and a
  descriptor round trip.
- The demo's alloc/free bench shows v1 at N=1 within run-to-run noise of v0, and reports the N=4
  rows for the smallest and the largest size.

#### Ladder

- [feat: segmented pool v1 opening][1] (done)
- [feat: segmented pool region and alloc][2] (done)
- [feat: segmented pool in the registry][3] (done)
- [perf: segmented pool in the alloc/free bench][4]
- [docs: segmented pool in the design note][5]
- [feat: segmented pool v1 closing][6]

#### Deliberation

- Multi-step: a new pool version, its registry path, a bench, and a design section are not one
  reviewable step.
- A v1 beside v0: the user's direction on 2026-09-24, so the cost of several stacks is measured
  against the pool as it is.
- One API, N stacks: the user's direction on 2026-09-24. The same alloc family selects a stack by
  size, and the degenerate single-stack case, the rings' and most queues', should cost nothing.
  - `const N: usize` with runtime sizes: v0 already asserts `T`'s size on every alloc, and at N=1
    that one comparison becomes the selection, so N=1 adds no work.
  - Sizes fixed at compile time through a trait, with an inline `const` block picking the stack for
    `alloc::<T>()`, would make N>1 free as well, at the cost of a clumsier API. Held until the
    measurement says the scan matters.
- Fallback and miss counts: the user's direction on 2026-09-24. An empty stack falls back to the
  next larger one rather than failing, and every miss is counted against the wanted size, whether
  the fallback then succeeds or ends in `Exhausted`.
  - The counters live in the allocating handle, a plain count, since allocation has one owner.
- One pool id for all the stacks, with `buf_idx` numbering buffers across the sizes: the Todo
  entry's first thought was a registered pool per sub-pool, and one id keeps the registry and the
  descriptor as they are, at the cost of a size-range search in `to_slot` (one comparison at N=1).
- Vocabulary: "segment" is the rings' word and "stack" the pools', so v0 is the single-stack pool
  and v1 the multi-stack pool. The user's call on 2026-09-24, at the review of the region rung.
  - "Segmented pool" gave "segment" a second meaning beside the rings of segments, whose
    segments are pool buffers.
  - The cycle keeps "segmented pool" as its name and title stem, since the opening pushed with
    it, and the code and docs say "multi-stack".
- Out of scope: a v1 flavor in `tp-pool`'s sweep, the rings taking a v1 pool, and compile-time
  sizes, each a later cycle if wanted.

#### Ladder details

##### feat: segmented pool v1 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
`### Segmented pools` Todo entry into this block, and bump the version-of-record. `## Waiting` held
nothing to promote.

##### feat: segmented pool region and alloc

`pool::v1`: the region header with N stack heads, each on its own cache line, `init` and `attach`,
the alloc family with fallback and miss counts, and `BufSlot` freeing to its own stack, with tests.

- Terms: a stack is one buffer size with its own buffers, linked as v0's free-stack is. The
  stacks follow the header in order, smallest first, and a buffer's index is local to its stack.
- The header is `PoolHeader<N>`: the geometry words and the per-stack sizes and counts, then one
  cache line per head. At N=1 it is two lines, as v0's.
- `init` takes `[(buf_size, buf_count); N]`. Sizes must be strictly ascending, so the first stack
  that fits is the smallest, and the total count stays below the sentinel, leaving room for a
  buffer index across the stacks when the registry rung wants one.
- The pick is a scan for the first stack that fits, and the fallback a loop over the larger
  stacks. At N=1 the fallback range is empty.
- The size check that v0's `check_type` makes becomes the pick. A size larger than the largest
  stack panics, for `alloc_bytes(size)` as for `alloc::<T>()`, since both are a request the pool
  was not built for.
- A miss is counted once per call whose wanted stack was empty, whatever the fallback then does,
  so `alloc_with` counts one per failed attempt. The counters are a plain `[u64; N]` in the
  allocating handle, fresh per handle.
- `BufSlot` holds its stack's head and its stack-local index, so a free touches its own stack
  alone. It has no header pointer, which the registry rung found it does not need.
- `Exhausted` is v0's own type, re-exported, and `Error` gains `BadStackCount` for an attach whose
  `N` differs from the region's.
- The crate docs in `src/lib.rs` name the pool family: v0 single-stack and the default, v1
  multi-stack by path.
- The magic differs from v0's, so neither pool attaches the other's region.
- The v1 tests pass under Miri as well. Its first run caught a test taking the region pointer a
  second time under a live handle, a test bug, and not the pool's.

##### feat: segmented pool in the registry

Take a `Desc` back to a guard against a v1 pool, `buf_idx` numbering buffers across the stacks.
How the registry holds v0 and v1 pool views alike, a trait or an enum, is decided here.

- A trait, not an enum: `PoolRegistry<'a, N, R = v0::PoolView>` is generic over a sealed
  `DescMap`, which v0's and v1's `PoolView`s implement.
  - Dispatch is static, so v0's `to_desc` and `to_slot` make the same checks as before, moved
    into v0's impl, and the existing call sites keep their shape, `R` inferred from `register`.
  - The trait's `Slot<T>` is the pool's own guard, so a registry takes and returns the guard
    type its pool mints. An enum would have needed an enum guard.
  - The cost: one registry holds one kind of pool, v0 or v1. A process mixing them keeps two
    registries, and their ids are separate spaces.
  - Sealed because `to_slot` mints owned guards on the implementor's word. `Sealed`, `Send`, and
    `Sync` have no methods, so their impls are empty, and a comment at each says so.
- v1's descriptor index numbers the buffers across the stacks, smallest stack first, so it is
  the stack's first index plus the stack-local one.
  - `to_desc` finds the stack by comparing the guard's head with each stack's head, one
    comparison per stack, so `BufSlot` needed no header pointer after all.
  - `to_slot` finds the stack whose index range holds the index, then checks `T` against that
    stack's size, so a `T` too big for its buffer is `BadType` even when a larger stack exists.
- Names, the user's call on 2026-09-24 at this rung's review: "resolve" said too little.
  - `into_desc` and `resolve` became `to_desc` and `to_slot`, a pair named by what each returns.
    `from_desc` was the first choice, and clippy's `wrong_self_convention` reserves `from_*` for
    constructors, which take no `self`.
  - `PoolResolver` became `PoolView` and `resolver()` became `view()`, a view that cannot pop,
    replacing "non-allocating", since no pool allocates memory, and the trait is `DescMap`.
  - The rename reaches v0, the demo, `tp_matrix`, the README, and the design note, since these
    names predate the cycle. The `alloc` family's name is a Todo of its own.
- The v1 and registry tests pass under Miri.

##### perf: segmented pool in the alloc/free bench

The demo's `pool_alloc_free_1t` gains v1 rows: N=1 beside v0, and N=4 at its smallest and largest
size, the cheapest and the costliest stack choice.

##### docs: segmented pool in the design note

A design-note section on the layout, the fallback, the miss counts, and the measured cost.

##### feat: segmented pool v1 closing

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

### SPSC v3 fast path

With the consumer keeping up, `spsc::v3` should cost what v2 costs, and on 2026-09-15 it cost about
three times as much where no segment switch happens, the demo's one-thread loop reading 24 ns per
message against v2's 7.5.

- Found: `WriteSlot::commit` and `ReadSlot::release` begin `let segs = st.segs;`, copying the ring's
  whole segment table, about 280 bytes, on every message. `let segs = &st.segs;` in both took the
  same-CCX stream at depth 64 from 18.4 to 10.7 ns per message, against v2's 4.5 to 5.7, and the SMT
  pair from 21.8 to 14.3, against 8.2.
- Found on 2026-09-15 in the MPSC v2 cycle, whose v2 shares v3's shape: the segment table's
  accessors, the seq helper, and the word packers are small non-generic functions in the parent
  module, called from the producer and consumer child modules. A release build without LTO puts
  each module in its own codegen unit, so without `#[inline]` those are real calls on every send
  and receive. Inlining v2's took its one-thread demo loop from 20.0 to 13.2 ns per message
  against v1's 10.1. Only `seq_of` was inlined for v3, and v3's own accessors in `Segments` still
  are not. `slot_ptr` and `check_type` in `lib.rs` are the same kind of call for every ring, v0
  and v1 included, and were left alone so the comparisons stay as they were.
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

- Where v2 stands on 2026-09-15, both machines: it streams two to five times faster than v1 from
  depth 8 up and pulls half the lines, its send matches v1's everywhere but the SMT pair, and its
  receive in the round trip runs up to 30% slower from depth 8 up on the 3900X and around depth 64
  on the 7600X. We think the seq word in the slot line is the cause: the consumer spins on the line
  the producer then writes twice, the body and the seq, where v1's producer fills the slot line
  unwatched and stores the seq beside it. The one-thread loop also carries 3 ns over v1 not yet
  found. Both are the fast-path work this entry waits on, and the switch's own cost is the entry
  `Cheaper segment switches`.

### Multi-producer measurement

The tools run every MPSC flavor at one producer and one consumer, so a claim word contended by
several producers is never measured. A `--producers N` flag for `tp-matrix` and `tp-stream` would
run N pinned producers into one consumer, and v2's switch would then be measured under contention.

### Cheaper segment switches

The demo's segment stress table prices one switch at depth 1 across cores at 140 ns for spsc-v3
and 270 to 290 for mpsc-v2 on the 3900X's cross-CCX pair, and 16 and 58 on the 7600X's same-L3
pair, on 2026-09-15, against 3 to 7 and 7 to 13 single-threaded. At depth 1 across cores the switch
is most of the message, eight times the no-switch shape on the 7600X. Where the consumer keeps
up, from depth 8 on, it is paid once in hundreds of messages or never.

- The lines that cross per switch, beyond the new segment's slot line that the no-switch shape
  walks too:
  - For spsc-v3 the consumer's give-back word, one transfer.
  - For mpsc-v2 the old segment's seal and the in-use word twice, since both sides
    read-modify-write it, three to four.
  - The two machines agree on that once the placement's cost per transfer is taken out, over 100
    ns cross-CCX on the 3900X and 15 to 20 within the 7600X's one L3.
- Candidates, each measured on the stress table's switch-cost rows:
  - A consumer-owned give-back word for v2 again, now that the taking side is sound by itself
    with the in-use word, so the consumer's give-back is a store to a line producers only read.
  - The seal riding in the slot word as v3's MOVED does, which v2 cannot do at the commit since
    another producer may hold the last slot.
  - A prefetch of the next segment's first line at the take.

### Comparison queues in the demo: cordyceps, crossbeam, iceoryx2

The demo compares the crate's rings only with `std::sync::mpsc`, and the user asked on 2026-09-15
for cordyceps, crossbeam, and iceoryx2 beside them. Its own cycle, since each is a dependency
decision and a harness shape:

- cordyceps is a dev-dependency today, used by `tp-pool`, and the demo is the installed binary, so
  it would become a dependency of the crate. Crossbeam and iceoryx2 would be new ones, and
  iceoryx2 is a shared-memory framework with its own runtime and setup.
- cordyceps's intrusive MPSC and crossbeam's channels move a pointer or a value, not a message in
  place, so their line is a pool buffer or a boxed message crossing, `tp-pool`'s shape, not the
  ring lines'. iceoryx2 is publish-subscribe over shared memory with no direct depth knob.
- Which lines and placements they join, and whether the demo or a `tp-pool` sweep is the place, is
  the design question the `tp-pool` cycle answered once for cordyceps.

### Descriptor queue endpoints

Paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
- own ring endpoint + registry access
- the demo's ~20-line send path becomes ~3 lines
- `to_slot`'s unsafe is audited once inside the crate (recv safe by construction)
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

### Pool alloc naming

No pool allocates memory: the region is fixed at `init`, and `alloc` pops a free buffer off a
stack. The `alloc` family's name suggests otherwise.

- Candidates: `take` / `take_with` / `take_bytes`, paired with `free` or a `give_back`.
- Reaches both pools, the registry docs, the demo, `tp_matrix`, the guide, and the README.
- Raised by the user on 2026-09-24, at the review of `feat: segmented pool in the registry`.

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

- Generic Queue: `Queue<P>`, a sealed `Single` / `Multi` producer marker over SPSC v3 and MPSC v2
  with `T` per call, open on the common send and on an ISR guard axis that would give thumbv6m
  a multi-producer queue [details](notes/ring-buffer-design.md#generic-queue-idea).
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
[1]: #feat-segmented-pool-v1-opening
[2]: #feat-segmented-pool-region-and-alloc
[3]: #feat-segmented-pool-in-the-registry
[4]: #perf-segmented-pool-in-the-allocfree-bench
[5]: #docs-segmented-pool-in-the-design-note
[6]: #feat-segmented-pool-v1-closing
