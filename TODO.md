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

### Test an inter-application message

This is one of the primary initial goals of this project and we've
never tested if it works. The minimal test I can think of is an SPSC
between two apps with the producer sending one message to a consumer.
The consumer will be started first and then the producer sends a
message that is a random number and a checksum of that number to
prove the message arrived intact.

### Unwrap lints for the library

The library has no `unwrap` or `expect` outside tests, but only by discipline. The user
prohibits them in real code, so a lint should enforce it
([`// OK` comments](agent-data/code.md#-ok--comments-on-unwrap-calls-rust)).

- `[lints.clippy]` in `Cargo.toml`: `unwrap_used = "warn"` and `expect_used = "warn"`, so
  validation's `-D warnings` fails any new site in library code.
- Tests are exempt, and the demo and the examples opt out with a crate-level `#![allow(...)]`,
  their setup panics being the right response there.
- The `unwrap_or*` family has no lint and stays under the `// OK:` comment convention.
- Raised by the user on 2026-09-24, at `refactor: segmented pool stack geometry`.

### spsc4 and mpsc3 over either pool

`spsc::v3` and `mpsc::v2` take their segments from a `pool::v0::Pool` only, so a multi-stack
pool cannot supply them. New versions take either pool, and v3 and v2 stay as they are, the
baselines to measure against.

- A sealed segment-source trait, "a buffer of at least N bytes, or none", implemented by both
  pools. v1's must not panic on a size larger than its largest stack.
- spsc4 and mpsc3: copies of v3 and v2 whose `init` takes any segment source. The pool is used
  only in `init`, so the endpoints and every hot path are v3's and v2's code.
- Expectation: spsc4 over a single-stack v1 pool times as spsc3 over a v0 pool, and mpsc3 as
  mpsc2, within noise. Rows for each pair: the old ring over v0, the new ring over v0 (the copy
  alone), over a single-stack v1, and over a multi-stack v1 with one stack for segments beside
  the message stacks. The demo first, iiac-perf for the fine comparison.
- Examples: one SPSC and one MPSC program with one v1 pool supplying both the ring's segments
  and messages of several types, dispatched by type-tag on receipt, each MPSC producer with its own
  pool, since a pool has one allocator.
- The user's direction on 2026-09-24, during `feat: segmented pool v1`: new versions rather than
  a generic `init` on v3 and v2, so the original code stays to measure against.

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

### Pool inlining and an iiac-perf comparison

The demo's alloc/free rows measure the compiler's inlining more than the pools, as the cycle
`feat: segmented pool v1` found at its bench rung: v0's hot helpers cannot inline across the
crate boundary, and v1's `alloc` stops inlining at four stacks. The demo's single timed loop per
row is also too crude for differences near a nanosecond.

- v0: `#[inline]` on `next_buf_idx`, `buf_ptr`, and the pop, so the baseline is not handicapped.
- v1: the miss path in a `#[cold]` out-of-line fallback, so `alloc` inlines at any stack count.
- Measure v0 and v1 at one and four stacks in [iiac-perf](https://github.com/winksaville/iiac-perf),
  whose harness calibrates and reports distributions. Variants selected by a type parameter on
  the pool, rather than copies of the module, would let one harness binary compare them.
- Raised by the user on 2026-09-24, at the bench rung of `feat: segmented pool v1`.

### Pool vocabulary: alloc and guard

No pool allocates memory: the region is fixed at `init`, and `alloc` pops a free buffer off a
stack. The `alloc` family's name suggests otherwise.

- Candidates: `take` / `take_with` / `take_bytes`, paired with `free` or a `give_back`.
- Reaches both pools, the registry docs, the demo, `tp_matrix`, the guide, and the README.
- "Guard": the docs call a `BufSlot` and the ring slots guards about 266 times, never defined,
  and to most readers a guard is a lock. Define it where readers start, or retire it for
  "slot", which the type names already use. No new text uses "guard" meanwhile, the user's call
  on 2026-09-25.
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
- `Message` trait over the payload cast boilerplate: const `TYPE_TAG` + the zerocopy bounds,
  receiver-side dispatch (read the [type-tag](notes/ring-buffer-design.md#type-tag), decode to
  a `Kind`, match, cast) without per-call-site ceremony, and maybe a transport seam so an
  embedded pointer-descriptor profile slots in behind the same API
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

### feat: segmented pool v1

#### Problem

A pool has one buffer size, so an application wanting messages of several sizes builds and registers
several pools by hand.

#### Solution

`pool::v1::Pool<'a, const N: usize>`, the multi-stack pool, keeps v0's API over one region
holding N stacks, one per buffer size, which `init` takes as `[StackGeometry; N]` in any order
and sorts smallest first. Recorded in the design note's [Multi-stack
pool](notes/ring-buffer-design.md#multi-stack-pool-0180).

- `alloc::<T>()`, `alloc_with`, and `alloc_bytes(size)` take the smallest stack that fits, fall
  back to the next larger one when it is empty, and count the miss against the stack that was
  wanted, reported per stack by `stats()`, so a user can tell which size wants more buffers.
- At N=1 the pick is the one size check v0 already makes and the fallback loop is empty, so the
  single-stack pool does v0's work, and the demo times it 0.6 ns faster, an inlining artifact.
- `free` returns a buffer to its own stack, and a `Desc` names a buffer by `pool_id` and a
  `buf_idx` numbered across the stacks, through a registry generic over a sealed `DescMap`.
- A receiver of mixed message types takes a descriptor back as bytes with `to_slot_bytes`, reads
  the type-tag, and turns the bytes typed with `into_typed`, and every ring carries such messages.
- Buffers start on a cache line, as v0's, so any `T` aligned to at most a line fits any size.
- The default re-export stays on v0, and the rings keep taking a v0 pool.

#### Acceptance check

- The v1 tests pass: size choice, fallback, miss counts, exhaustion, attach validation, and a
  descriptor round trip.
- The demo's alloc/free bench shows v1 at N=1 within run-to-run noise of v0, and reports the N=4
  rows for the smallest and the largest size.

Result, 2026-09-25: the tests pass, 29 in `pool::v1` under `cargo test`. The demo reports every
row, and the N=1 clause is a finding rather than a pass: one run on the base cpu read v0 at 10.0
ns and v1 at 9.1, and the bench rung's 20-run means 9.74 and 9.12, a gap outside the 0.1 ns
run-to-run noise. The cause is the baseline, not the pool: v0's `next_buf_idx` and `buf_ptr` are
not `#[inline]` and do not inline across the crate boundary into the demo, while the generic v1
compiles whole there. The `### Pool inlining and an iiac-perf comparison` Todo is the fix and the
comparison to trust.

#### Ladder

- [feat: segmented pool v1 opening][1] (done)
- [feat: segmented pool region and alloc][2] (done)
- [feat: segmented pool in the registry][3] (done)
- [perf: segmented pool in the alloc/free bench][4] (done)
- [test: segmented pool allocation order][8] (done)
- [refactor: segmented pool stack geometry][7] (done)
- [feat: segmented pool byte slots from descriptors][9] (done)
- [test: segmented pool messages over every ring][10] (done)
- [docs: segmented pool in the design note][5] (done)
- [feat: segmented pool v1 closing][6] (done)

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
- Out of scope: a v1 flavor in `tp-pool`'s sweep, the rings taking a v1 pool for their segments
  (the `### spsc4 and mpsc3 over either pool` Todo, new versions so v3 and v2 stay the
  baselines), and compile-time sizes, each a later cycle if wanted.

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

- The rows: `pool1_alloc_free_1t` at one stack, and at four stacks (one, two, four, and eight
  lines) allocating a `T` for the 1st and for the 4th stack, the same alloc -> write -> free loop
  as v0's, pinned to the base cpu.
- Measured 2026-09-24 with the demo, 20 runs of each row on the base cpu, ns/msg means, the
  stdevs 0.1 or less. The scratch builds were measured and reverted, and none is in the tree:

  | build | v0 | v1, 1 stack | v1, 4 stacks, 1st | v1, 4 stacks, 4th |
  |---|---|---|---|---|
  | v1 as committed | 9.74 | 9.12 | 11.00 | 11.03 |
  | scratch: unchecked indexing | 9.79 | 9.12 | 11.00 | 11.06 |
  | scratch: cold fallback | 9.85 | 10.01 | 9.96 | 10.08 |

- What the builds and their disassembly showed:
  - The 1st and 4th stacks time alike, and with `alloc` inlined four stacks cost what one does,
    in a loop that picks the same stack every time, so the scan's branches always predict. A
    workload mixing sizes may pay for mispredictions, and which stack each row hits is claimed
    by its label, not checked, until the stack geometry rung asserts it.
  - Bounds checks cost nothing: removing them in the pop changed no row.
  - Inlining moves the numbers. At four stacks `alloc`, fallback loop included, is too big to
    inline and stays a call, with the guard returned through memory, the 1.9 ns. A `#[cold]`
    out-of-line fallback lets it inline.
  - v0 is a handicapped baseline: `next_buf_idx` and `buf_ptr` are calls in the demo's loop,
    since v0 is not generic and they are not `#[inline]`, so they cannot inline across the
    crate boundary, while the generic v1 is compiled in the demo and inlines whole. That is
    the 0.6 ns by which v1 at one stack beats v0.
  - Compile-time stack sizes, a possible v2, would only speed a choice that timed as free
    here, so the idea is dropped until a measurement says otherwise.
- The demo's numbers are indicative only: one timed loop per row, no warmup or calibration, and
  differences near a nanosecond that code layout alone moves, as the cold-fallback build's one
  stack did. The comparison worth trusting is iiac-perf's, a Todo with the inlining fix.

##### test: segmented pool allocation order

The tests cover the simple fallback paths, but not a small buffer free while a big one is asked
for, or a middle and a big both free under a small request. Scenario tests pin those down, with
the size boundaries, and a model-based test drives thousands of random allocs by size and frees
in random order against a plain model of the rule, checking after every step which stack served
each request, where `Exhausted` falls, and the miss counts. Inserted at the user's call on
2026-09-24, at the bench rung, ahead of the stack geometry rung so the tests pin today's
behavior before the API changes.

- Scenarios, over one pool of 64-, 128-, and 256-byte stacks, each starting from every stack
  exhausted by one-byte requests, and written in bytes, what a user asks for:
  - a 64-byte buffer free under a 256-byte and a 128-byte request: `Exhausted` for both, a miss
    on each wanted stack, and the 64-byte buffer still serves a one-byte request
  - only a 256-byte buffer free: a one-byte request falls back to it
  - a 128-byte and a 256-byte buffer free, the 256-byte one freed last so a single LIFO list
    would hand it out first: one-byte requests get the 128-byte, then the 256-byte, then
    `Exhausted`, so the order can only be the stacks'
  - every buffer handed out is checked for what a user may rely on: at least the size asked
    for, and starting on a cache line
- Size boundaries: zero and every exact fit land in their own stack, and one byte more moves to
  the next.
- Seeds: each randomized test runs three fixed seeds and one fresh random seed per run, and
  `ZC_POOL_SEED=<seed>` runs that one seed alone. The seed picks everything random, the stack
  count and geometry included.
  - A guard in each thread prints, on a failure, the test, the seed, the thread's role, and the
    step it reached, with the command that replays it.
- The model test, `allocation_matches_the_model`: 20,000 steps per seed, over a fixed four-stack
  geometry and over one the seed picks (1, 2, 3, 4, or 8 stacks, sizes one to four lines apart,
  one to four buffers each).
  - Allocs by size, each stack's size range about equally likely, and frees of a random held
    buffer.
  - After every step the serving stack, `Exhausted`, and `misses()` must match a plain model of
    the rule, and each held buffer's step tag, in its first and last word, must survive to its
    free.
  - It asserts its own coverage: fallbacks and `Exhausted` each above 2% of the steps, summed
    over the seeds.
  - A seed replays exactly: a failure recurs at the same step.
- The threaded test, `threaded_random_alloc_and_free`: per seed, 20,000 messages over two
  threads (an allocator and a freer) and three (an allocator and two freers), each over a
  geometry the seed picks.
  - The allocator takes random sizes and hands each buffer to a random freer, and each freer
    frees what it holds in random order, so the frees race the pops on every stack's head.
  - The allocator checks each buffer against its request (at least the size, on a cache line,
    never from a smaller stack) and keeps its own miss count, which `misses()` must match
    exactly, since only the allocator counts misses.
  - The freers check each buffer's tags. At the end every buffer is back, and each stack serves
    exactly its count.
  - A seed replays the plan, the sizes and the routing, but not the thread interleaving, so a
    replayed failure recurs, though not always at the same step: a deliberate break failed at
    steps 18, 20, 24, and 2390 under one seed. Replaying an interleaving would take a tool like
    `loom`.
- The tests were checked against two deliberate breaks of the rule, made and reverted before
  the random-geometry and threaded tests joined: a fallback to any stack, smaller included,
  failed 2 tests, and the largest stack tried first failed 6. The first break, repeated after,
  also failed the threaded test.
- The v1 tests pass under Miri, the model test at 300 steps and the threaded test at 100
  messages per run.

##### refactor: segmented pool stack geometry

`init` and `region_size` take each stack as a bare `(buf_size, buf_count)` tuple, which says
nothing at the call site. A `StackGeometry { buf_size, buf_count }` with a `const fn new` names
the fields, the handle's parallel snapshot arrays become one `[StackGeometry; N]`, and `init`'s
docs give each field's meaning and units. Inserted at the user's call on 2026-09-24, at the bench
rung, so v1 lands with the named type and the design note describes it.

- `StackGeometry { buf_size, buf_count }`, public fields and a `const fn new`, is how a caller
  describes a stack. `init` and `region_size` take `[StackGeometry; N]`.
- The pool orders its stacks, the user's call at this rung's planning: `init` takes them in any
  order and sorts them by size, so the layout and the search are the pool's to change, a sorted
  table or something else later.
  - Two stacks of one size are refused as `BadBufSize`, a duplicate being likelier a mistake
    than a request. `attach` still requires the region's stacks in the pool's order, since
    `init` wrote them so.
- No public stack index: a caller's position means nothing once the pool orders the stacks.
  - `buf_size(stack)`, `buf_count(stack)`, and `misses()` gave way to `stacks()`, the geometry
    in the pool's order, and `stats()`, a `StackStats { geometry, misses }` per stack, each
    count labelled by the stack it belongs to.
  - The handle's and the view's parallel size and count arrays are one `[StackGeometry; N]`.
- `BufSlot::buf_size()` reports the size given, at least the size asked for, for a typed guard
  as for a byte guard, whose `len()` already said so.
- The demo's v1 rows check before the clock starts that a `T` is served by the stack the label
  claims, which the bench rung could only assume.
- Tests: new ones for `init` ordering the stacks itself, `stats()` found by size, and
  `buf_size()` on a typed guard. The model and threaded tests hand `init` a shuffled geometry, so
  every seed exercises the sort, and white-box tests read the handle's `misses` in the pool's
  order. All pass, under Miri too.

##### feat: segmented pool byte slots from descriptors

A receiver of mixed message types learns a buffer's type from a tag inside it, but `to_slot::<T>`
needs `T` up front, and a descriptor may be taken back only once. `to_slot_bytes` takes it back
as bytes, the counterpart of `alloc_bytes`, and `BufSlot<[u8]>::into_typed::<T>` checks the fit
and turns the guard typed, handing it back on a misfit, so a receiver reads the tag and matches.
Inserted at the user's call on 2026-09-24, at the stack geometry rung.

- `DescMap` gains `to_slot_bytes`, and `PoolRegistry::to_slot_bytes(desc)` calls it: the index
  validated, no type to check, since every buffer is valid as bytes. A descriptor is taken back
  once, by `to_slot` or `to_slot_bytes`, never both.
- `into_typed` lives on both pools' byte guards and checks size and alignment with the same
  `type_fits` as `to_slot`, a misfit an `Err` that hands the byte guard back, since the type
  follows from bytes that arrived.
- v1's view shares one index lookup, `locate`, and one guard minting, `mint`, between the typed
  and the byte path. v0's `slot_from_idx` lost a trait bound it never used, so it mints a byte
  guard too, and v0's alloc and free paths are untouched.
- Tests: three message types (16, 112, and 400 bytes) from one three-stack pool taken back as
  bytes and dispatched by tag, a misfit handed back and reused, hostile descriptors refused,
  and v0's byte round trip. All pass, under Miri too.

##### test: segmented pool messages over every ring

Every ring carries descriptors, plain data, so each should carry messages of mixed types from a
multi-stack pool unchanged. One test sends them through all seven rings, spsc v0 to v3 and mpsc
v0 to v2, and dispatches them by type-tag on receipt. Inserted with the byte slots rung.

- `tests/pool_v1_over_rings.rs`, an integration test, so it uses the public API alone, as a
  user would: one test per ring, each ring as it ships.
- Three message types (24, 112, and 400 bytes) from pools of 64-, 128-, and 512-byte stacks,
  two buffers each, so producers wait on empty stacks and every buffer recycles many times.
- Every message starts with a type-tag, a number naming its kind, decoded into an `enum Kind` by
  `TryFrom<u64>`, so the receive `match` is exhaustive and an unknown type-tag fails at the
  decode. The consumer takes each descriptor back with `to_slot_bytes`, decodes the type-tag,
  and turns the bytes into the message's type with `into_typed`, checking each message's
  payload and each producer's order.
- Vocabulary, the user's call at this rung's review: "type-tag" in prose and `type_tag` in
  code, one term in both. It keeps clear of Rust's `TypeId`, a per-build value no other process
  can share, and `Kind` names the decoded enum, as `std::io::ErrorKind` does. The design note
  defines it, and the byte slots rung's unit test follows there.
- MPSC rings run two producers, each with its own pool, a pool having one allocator, and both
  pools in one registry, so one consumer takes back descriptors from two pools.
- A small trait pair, `DescTx` and `DescRx`, implemented per ring version by a macro, lets one
  producer and one consumer function drive all seven rings.
- The segmented rings, spsc v3 and mpsc v2, still take their segments from a v0 pool beside the
  v1 message pools, the spsc4 and mpsc3 Todo's to change.
- All seven pass, under Miri too.

##### docs: segmented pool in the design note

The design note had no record of the multi-stack pool, and "type-tag" was a term the ring test
used without a definition. A `### Multi-stack pool (0.18.0)` section records the pool as built,
and a `#### Type-tag` entry beside `#### Descriptors` defines the term.

- The section: the layout, the geometry and its ordering by `init`, the alloc family's pick and
  fallback, the miss counts and `stats()`, the free to its own stack, the registry's `DescMap`
  and the cross-stack `buf_idx`, the byte-first receive path (`to_slot_bytes`, `into_typed`),
  the bench table with its readings, the tests, and what is out of scope. The messaging layer's
  intro names it beside v0.
- Type-tag: a message's first word naming its type, "type-tag" in prose and `type_tag` in code,
  decoded to a `Kind` by `TryFrom<u64>`. The entry records the 2026-09-25 survey's finding: Rust
  code pairs "tag" for the number with `Kind` for the enum (rustc, serde, h2, `enum-kinds`), and
  never compounds them, so "kind-tag" was not taken. The 0.7.0 design bullet and the open
  question link to the entry, and the `## Ideas` `Message` trait's `MSG_ID` became `TYPE_TAG`.
- The unit test `mixed_messages_dispatch_by_tag` became `mixed_messages_dispatch_by_type_tag`,
  its three consts a `Kind` enum decoded by `TryFrom<u64>` and its `tag` fields `type_tag`, the
  same shape as the ring test, so the two specimens the entry names agree.
- The user guide is unchanged: it covers the rings of segments, which take a v0 pool, and the
  crate docs in `src/lib.rs` already name the pool family.

##### feat: segmented pool v1 closing

Closing out the cycle: the acceptance check run and its result recorded above, the solution
statement replaced with what was done, the block moved to `## Closed`, and the continuation
notes reset.

- Close-out shape: trapezoid, the default, since `main` should read the multi-stack pool as one
  change while every rung stays reachable, and the user's choice on 2026-09-25 when asked to do
  the closing.
- Nothing in the block needs a `notes/` home beyond what the docs rung wrote: the design note's
  `### Multi-stack pool (0.18.0)` and `#### Type-tag` hold the design findings, and the bench
  readings are in both.
- No agent-file changed in this cycle, so `notes/agent-files-size.md` gains no row.
- The `notes/README.md` design-doc entry names the multi-stack pool and the type-tag.
- The existing "guard" wording stays until `### Pool vocabulary: alloc and guard` runs, the
  user's call on 2026-09-25 after a survey of the uses: about 70 name the pool's `BufSlot`, about
  100 the rings' `WriteSlot` / `ReadSlot`, and the rest are unwind guards and prior art. No new
  text uses the word meanwhile.

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #feat-segmented-pool-v1-opening
[2]: #feat-segmented-pool-region-and-alloc
[3]: #feat-segmented-pool-in-the-registry
[4]: #perf-segmented-pool-in-the-allocfree-bench
[5]: #docs-segmented-pool-in-the-design-note
[6]: #feat-segmented-pool-v1-closing
[7]: #refactor-segmented-pool-stack-geometry
[8]: #test-segmented-pool-allocation-order
[9]: #feat-segmented-pool-byte-slots-from-descriptors
[10]: #test-segmented-pool-messages-over-every-ring
