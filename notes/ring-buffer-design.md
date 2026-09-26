# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../agent-data/prose.md#prose-form). It covers terminology,
requirements, constraints, the memory layout, the API, and
the validation ladder, and is kept in sync with the
implementation in `src/` (as-built): `lib.rs` holds the
shared core (errors, `Full`/`Empty`, geometry helpers).
Each primitive lives in a versioned module dir
(`src/spsc/v0/` (`mod.rs`: Header/Ring, `producer.rs`:
Producer/WriteSlot, `consumer.rs`: Consumer/ReadSlot),
`src/mpsc/v0/`, `src/pool/v0/`) behind a per-module
default-version re-export, and the crate root re-exports
the defaults.

## Goal

A ring buffer for passing typed messages between a producer and
a consumer with no copying at the boundary: the producer writes
its message directly into buffer memory, the consumer reads it
in place. The same layout must work intra-process (threads) and
inter-process (shared memory).

**Blue-sky goal: ISR-to-ISR messaging.** A stretch target: the
same rings carrying messages between *interrupt service
routines*, no thread in the loop: N producing ISRs into one
consuming ISR, up to N:M for fan-out / consolidation (entities
as a graph of ISR nodes,
[Execution contexts](#execution-contexts)). Lock-free rings
suit ISRs (no lock means no priority inversion), but the
gotchas are real:

- No blocking, ever: an ISR bails, never waits, on Full
  or Empty (the `|_| false` policy). Backpressure must be
  drop or an overflow FIFO, never a spin.
- Something must fire the consumer: a consuming ISR runs
  only when triggered, so an all-ISR system needs a doorbell
  (a software interrupt / IPI raised after commit). The ring
  does not provide it.
- The CAS floor splits the styles by target: a shared
  MPSC ring's multi-producer claim needs an atomic RMW on
  `producer_idx`:
  - has-CAS (`target_has_atomic = "32"`, ≥ Cortex-M3):
    hardware CAS provides the claim, nothing extra needed.
  - no-CAS (M0 / thumbv6m): two ways to keep N producers:
    - EI/DI-emulated claim: disable interrupts around the
      RMW to stand in for the CAS. Keeps the shared ring.
      Costs a brief all-interrupt blackout per claim, and is
      single-core only (a second core still needs a
      hardware spinlock).
    - fan-in: each producer gets its own SPSC ring (no
      shared claim, no CAS, no EI/DI), and the consumer
      polls the N rings.
- Latency is bounded, not constant: MPSC CAS retries
  under nested-ISR contention are bounded, but a hard
  real-time budget must account for them.
- panic=abort assumed: the tombstone/unwind path never
  runs in an ISR, so a panicking fill aborts rather than
  degrading, which is fine, but a property to design around.

## Terminology

The vocabulary the rest of the document builds on, defined
once before first use:

- slot: a cache-line-aligned `[u8; N]`, one of the
  ring's M fixed-size buffers. Untyped storage, a message
  is a zerocopy `&T` / `&mut T` view into it (geometry
  details in [Constraints](#constraints)).
- reserve: take exclusive access to a slot through a
  guard:
  - the producer reserves the next free slot
    (`WriteSlot`)
  - the consumer reserves the oldest unread slot
    (`ReadSlot`)
- commit: the producer publishes its slot to the
  consumer, the handoff of ownership away from the writer:
  - SPSC: the `producer_idx` Release store
  - MPSC: the slot's seq Release store
- release: the consumer hands its slot back for
  reuse, the matching handoff in the other direction:
  - SPSC: the `consumer_idx` Release store
  - MPSC: the slot's seq Release store (`pos + M`)
- claim: MPSC only: a producer wins exclusive
  ownership of a slot position by CAS on the shared
  producer index, and writing and committing follow. Claim
  order and commit order can differ, see
  [MPSC ring (sibling primitive)](#mpsc-ring-sibling-primitive).

The placement terms, for the measurement tools and the demo ([Measurement
placements](#measurement-placements-the-base-cpu-and-its-partners)), fixed on 2026-09-16:

- **core**: a physical execution unit. Twelve on the 3900X, six on the 7600X, four on an RP2350
  with two active. A core has an instruction set, Zen 2, Cortex-M33, Hazard3, and on a mixed
  machine a kind, the kernel's `cpu_capacity` number.
- **cpu**: what the kernel presents and pins to, one number per active execution context, the
  `N` of `cpu N` and of every placement label. One per core without SMT, two with it: the 3900X
  presents 24 cpus on 12 cores, the 7600X 12 on 6, an Apple M1 8 on 8. A cpu carries its core's
  instruction set and kind and belongs to a cluster. Never "logical cpu" or "hardware thread",
  and never "thread", which is software here.
- **SMT siblings**: the cpus of one core, the kernel's term for `thread_siblings_list`. A core's
  **primary cpu** is its lowest-numbered sibling, cpu N on both machines, and the other is its
  **secondary cpu**, N+12 on the 3900X and N+6 on the 7600X.
- **cluster**: the cpus sharing a cache layer, which layer depending on the part. A Zen CCX
  shares L3, an Apple or ARM cluster shares L2. The pickers use L3 today, so `CCX` and `x-CCX`
  are the Zen spelling of same-cluster and cross-cluster.
- **cache layers**: L1, L2, L3, "layer" where prose needs the word, never "level".
- **bare metal**: no kernel numbers cpus, so only cores exist and the SDK names them, core 0
  and core 1 on the RP2350.

## Requirements

- no_std: the crate builds with `#![no_std]` and no
  `alloc`: fixed capacity, operating over a caller-provided
  memory region. Suitable for embedded and kernel-adjacent use.
- Zero-copy via zerocopy: messages are structs implementing
  the [zerocopy](https://docs.rs/zerocopy) traits
  (`IntoBytes`, `FromBytes`, `KnownLayout`, `Immutable`). The
  buffer transmutes slots into and out of `&T`/`&mut T` with no
  serialization step and no `unsafe` at the call site.
- IPC-suitable layout: the control block (the
  producer/consumer indices) and the slot array live in one
  contiguous region that
  can be mapped into multiple address spaces:
  - `#[repr(C)]` explicit layout, stable across processes
    compiled from the same source.
  - No pointers inside the region, offsets and indices only.
- SPSC first: single producer, single consumer, lock-free
  via atomic producer/consumer indices. MPMC is out of scope
  for this cycle, and
  the layout should not preclude adding it later.
- In-place access API: `reserve_slot_with` on either
  endpoint, then commit (producer) or release (consumer),
  exposing references into the buffer rather than returning
  values by move.
- Safety: all `unsafe` encapsulated inside the crate with
  documented invariants. The message path (reserve_slot_with,
  then commit / release) is fully safe. The one `unsafe` public
  entry
  is `Ring::attach` (see [API](#api) for why it cannot be
  made safe).

## Constraints

- Slot geometry, M slots × N bytes: the ring holds M
  [slots](#terminology) of N bytes each.
  - M (capacity) is a power of two, so index wrapping is a
    mask, no division. Checked at construction.
  - N (slot size) is a multiple of the cache-line size, and
    each slot is cache-line aligned, so adjacent slots never
    share a line, and any `T` with `align_of::<T>()` up to
    the line size fits without padding logic.
  - This trades space for isolation (a 16-byte message
    occupies a full 64-byte slot), the right default for
    IPC. A packed-slot variant is a possible follow-on.
- Monotonic indices: producer/consumer indices are
  free-running unsigned counters (wrap via masking), so
  full/empty are distinguishable without a separate flag or
  a sacrificial slot.
- Cache-line separation: producer- and consumer-owned
  indices are padded/aligned to separate cache lines to avoid
  false sharing.
- SPSC roles forbid concurrent access, not multiple
  owners: the single-producer and single-consumer contract
  is "at most one accessor in an endpoint at a time," a
  *serialization* requirement via a serializer/mutex-type
  entity, not a fixed thread identity.
  How threads and ISRs each satisfy it is
  [Execution contexts](#execution-contexts).
- Typed access over untyped slots: the slot geometry
  (M, N) is a property of the buffer, independent of any
  message type. A `T` is a zerocopy view into a slot, valid
  when `size_of::<T>() <= N` and `align_of::<T>()` divides
  the slot alignment (checked at construction or compile
  time). Variable-length framed messages are a possible
  follow-on, not in this cycle.
- zerocopy, core-only: depend on zerocopy with default
  features compatible with `no_std`, no other runtime
  dependencies.

### Execution contexts

An *execution context* (a thread or an ISR) drives
endpoints (a process is only the address-space
boundary around one or more of them,
[System model](#system-model-overview)). Each satisfies the
serialization rule above (one accessor in an endpoint at a
time) differently:

- Thread, sole owner of an endpoint: one thread owns the
  endpoint, no sharing, fully lock-free.
- Threads sharing an endpoint: several take turns under a
  mutex:
  - producer and consumer sides are symmetric
  - the mutex allows only one thread at a time to use the
    endpoint
  - cost is blocking, the endpoint is no longer lock-free
- ISR, sole owner of an endpoint: the canonical embedded
  ring: the ISR exclusively owns *one* end (say the producer),
  a thread owns the other.
  - Safe without interrupt disable, because the protocol
    enforces the two preconditions that make preemption safe:
    - a field the ISR writes (its index, its in-flight slot)
      is read-only to the partner, single writer
    - the shared control words are accessed atomically
  - Preemption is then just another interleaving of the
    handoff, no different from running the two ends on
    separate cores.
  - Why *sole* ownership: an ISR can't be serialized by a lock
    (it preempts, can't block on a thread-held one), so it
    must never *share* an endpoint, only own one outright.
- ISR sharing an endpoint with a thread: possible, but the
  "special care" path:
  - the thread guards its access with interrupts disabled
    (irqsave, the only way to be atomic against a same-core
    ISR)
  - the ISR must try-and-bail: test the access flag, and if
    taken, abandon rather than spin
  - it hinges on the thread being able to disable interrupts
- Lock-free concurrency instead: many producers, or a
  thread + ISR without the irq-disable dance:
  - use the MPSC ring, its CAS claim serializes without a
    lock.

## Memory layout

One contiguous `#[repr(C)]` region, mappable into multiple
address spaces: a `Header` struct followed by the slot array.
The `Header` spans four cache lines, and each concurrently-written
index owns its own line, and alignment is expressed on exactly
the fields that need it via a `CacheAligned` wrapper (the
compiler computes the inter-line padding):

```rust
/// Cache-line-aligned wrapper granting its field sole
/// ownership of the line.
#[repr(C, align(64))]
struct CacheAligned<T>(T);

#[repr(C)]
struct Header {
    // line 0: geometry, written by init (magic last), then
    // read-only. Cold: per-op paths use handle snapshots.
    magic: AtomicU32,          // layout marker, the init/attach handshake
    layout_version: AtomicU32, // bumped on any layout change
    slot_size: AtomicU32,      // N in bytes, cache-line multiple
    capacity: AtomicU32,       // M, power of two, so the index mask is M - 1
    cache_line_size: AtomicU32, // CACHE_LINE_SIZE the region was built with
    // line 1: producer-owned
    producer_idx: CacheAligned<AtomicU32>, // written by producer only
    // line 2: consumer-owned
    consumer_idx: CacheAligned<AtomicU32>, // written by consumer only
    // line 3: app-owned scratch, zeroed at init, never
    // touched by the crate again
    user: CacheAligned<[AtomicU32; 16]>,
}
```

```
offset 0                    Header (256 bytes, 4 cache lines)
offset size_of::<Header>()  slots: M × N bytes
```

- One type owns the whole control-block layout:
  `size_of::<Header>()` is the slot-array offset.
- User line last, on purpose: an app-side overrun walking
  forward out of `user` lands in slot 0 (the app's own message
  data), not in an index line.
- `repr(align(N))` takes only an integer literal: it
  cannot name `CACHE_LINE_SIZE`, so the `64` is written out and
  const asserts tie `align_of::<CacheAligned<AtomicU32>>()`
  and `size_of::<Header>()` back to the constant.
- Every field is atomic: the region may be mapped by a
  peer at any time, so even "immutable" geometry must be free
  of data races. A peer scribbling on plain fields would be
  UB, on atomics it is merely garbage values. `AtomicU32` has
  the same layout as `u32`, so layout_version is unaffected.
- Init/attach handshake: init stores the geometry and
  indices first (`Relaxed`), then `magic` **last** with
  `Release`, and attach loads `magic` **first** with `Acquire`. A
  peer that pre-mapped the region and observes MAGIC therefore
  also observes the geometry it validates. (Re-initializing a
  region a peer is already attached to stays outside the
  contract.)
- Slots: `M` slots of `N` bytes, each cache-line
  aligned (N is a line multiple, so alignment follows from
  the array base).
- `CACHE_LINE_SIZE = 64` plays two roles: a layout-ABI
  constant and a false-sharing pad. On CPUs with 128-byte
  effective line pitch (Apple M-series, Intel's adjacent-line
  prefetcher is why crossbeam pads 128 on x86_64 too) the
  padding under-separates, a perf question for the bench
  work, not correctness. Since layout v2 the header records
  the build's `cache_line_size` and attach rejects a mismatch
  (`BadCacheLine`), so the constant can become per-target
  (cfg) or shrink for cache-less embedded tiers without any
  silent cross-build corruption.
- Assume nothing beyond `AtomicU32` load/store: the
  protocol uses no CAS/RMW, so it reaches
  load/store-only targets (e.g. `thumbv6m`/Cortex-M0).
  Features that want CAS (endpoint claims) should degrade or
  gate rather than raise the floor. 8/16-bit targets (AVR,
  MSP430, no `AtomicU32`) would need index-width
  genericization, out of scope, recorded in Ideas.

Index scheme (resolves the atomic-width question):

- Fixed-width `AtomicU32`, free-running:
  - increments are wrapping adds, never masked in storage
  - fixed width keeps the layout identical for 32- and
    64-bit peers sharing the region
- Masked only at slot access:
  - slot position = `idx & (M - 1)`
- Occupancy = `producer_idx.wrapping_sub(consumer_idx)`
  (valid while `M <= 2^31`):
  - empty: `producer_idx == consumer_idx`
  - full: `producer_idx - consumer_idx == M`, implemented as
    `>= M`, so a peer-corrupted consumer_idx fails toward
    `Full` instead of handing out an unowned slot
  - no sacrificial slot
- u32 wrap at `2^32` is harmless because M is a power
  of two:
  - `2^32 % M == 0`, so occupancy math and masked slot
    positions both carry across the wrap unchanged
- Example (M = 4, `p` = producer_idx, `c` = consumer_idx):
  - start: `p = 0, c = 0`, occupancy 0, empty
  - producer commits 4 messages: `p = 4, c = 0`, occupancy
    4 == M, full. Slot positions written were 0, 1, 2, 3
  - consumer releases 1: `p = 4, c = 1`, occupancy 3, one
    slot free. The next reservation writes slot `4 & 3 == 0`

Ordering: the producer loads `consumer_idx` with `Acquire`
and publishes `producer_idx` with `Release`, and the consumer
mirrors (loads `producer_idx` `Acquire`, publishes
`consumer_idx` `Release`).

## API

Construction (resolves the foreign-memory question):

- `Ring::init(region: &'a mut [u8], slot_size: u32, capacity:
  u32) -> Result<Ring<'a>, Error>`: validates geometry
  (power-of-two M, line-multiple N, alignment, region big
  enough, sized in u64, since the N×M product can wrap a
  32-bit usize), then writes the header per the
  [handshake](#memory-layout) (indices zeroed, magic last).
- `unsafe Ring::attach(region: *mut u8, len: usize) ->
  Result<Ring<'a>, Error>`: validates magic (Acquire),
  layout_version, and geometry against the region length,
  over the same shared `header_ptr` checks as init. Unsafe,
  deliberately, on two grounds:
  - zerocopy's shared validated casts require `Immutable`,
    which `Header` correctly is not (atomics), so the cast
    is hand-verified rather than derive-verified.
  - only the caller can vouch that the pointer is a live,
    genuinely shared, writable mapping (e.g. `MAP_SHARED`)
    and that the SPSC role contract holds across processes.
- `ring.split() -> (Producer<'_>, Consumer<'_>)`: consumes
  the `Ring`, so each handle exists at most once per ring per
  process (SPSC discipline, and cross-process there is one
  producer process and one consumer process by contract).
- Geometry snapshot: slot_size / capacity / mask are
  copied into `Ring` and the handles at init/attach, so per-op
  paths never re-read peer-writable header fields. Combined
  with the all-atomic header, a hostile peer can degrade the
  ring to garbage messages or spurious Full/Empty, never UB.

Producer: reserve_slot_with/commit, in place:

- `producer.reserve_slot_with::<T>(on_full) ->
  Result<WriteSlot<'_, T>, Full>`: next free slot as `&mut T`
  (zerocopy `FromBytes + IntoBytes + KnownLayout`), retrying
  under the injected wait policy: `on_full(attempt) -> bool`,
  `|_| false` for a single non-blocking probe. `T` geometry is
  asserted against the slot size on each call (a mismatch is a
  programming error, so it panics. Typed endpoints that check
  once are a follow-on: see [TODO.md](../TODO.md)).
- `WriteSlot::commit(self)`: publishes the slot
  (`producer_idx + 1`, `Release`). Dropping without commit
  abandons the reservation (nothing published).

Consumer: reserve_slot_with/release, in place:

- `consumer.reserve_slot_with::<T>(on_empty) ->
  Result<ReadSlot<'_, T>, Empty>`: oldest unread slot as
  `&T`, retrying under the injected wait policy (`|_| false`
  for a single non-blocking probe).
- `ReadSlot::release(self)`: frees the slot
  (`consumer_idx + 1`, `Release`). Dropping without release
  leaves the slot unread. The next reservation returns it
  again.

Each side reserves at most **one slot at a time**: the guard
holds the endpoint's `&mut` borrow, so a second reservation
while a guard is live is a compile error, not a runtime state.

Both endpoints also expose `user() -> &[AtomicU32; 16]`: the
header's app-owned scratch line, see
[Blocking and user words](#blocking-and-user-words).

Guard internals: both guards hold **raw slot pointers** and
mint `&T` / `&mut T` per access via Deref. A reference *field*
would be argument-protected (noalias) for the entire
`commit`/`release` call, but the store inside that call hands
the slot to the other side, which may access it while the
protector is still live, an aliasing race Miri flags
(found in 0.3.0-6 by the threaded test).

Batch release (resolves the batching question), deferred:
the guard API leaves room for a `release_n(n)` later. The
single-slot guard is the whole surface this cycle.

## Blocking and user words

The ring's fallible core never blocks: `reserve_slot_with`
returning [`Full`]/[`Empty`] under a give-up policy (`|_| false`)
is the primitive. *How* to wait on it, spin, yield, park,
await, is injected policy: the crate ships [`policy::spin`]
for a pure busy-wait, but anything that actually sleeps
belongs to the layer above (an embedded caller
may WFE on an interrupt, an async runtime wants a waker, a
pinned busy-poll thread wants a pure spin, so no single
crate-blessed hybrid fits all three).

What the crate provides is mechanism: the header's `user`
line (16 `AtomicU32` words, zeroed by `Ring::init`, exposed
via `user()` on both endpoints, and never read, written, or
interpreted by the crate afterwards). It is the well-known
shared-memory home where independently written peers can
build a wakeup protocol (waiter flags, futex words, sequence
numbers). `AtomicU32` deliberately: futex-shaped, portable to
targets without 64-bit atomics, race-free under concurrent
peer access.

Contracts and cautions for that layer:

- Lost-wakeup discipline: the sleep side must re-check
  the ring *after* publishing its "wake me" flag and before
  sleeping (`set flag -> reserve_slot_with once more -> sleep`), and
  the wake side checks the flag *after* its commit/release
  (`commit -> check flag -> wake`). Skipping the re-check races
  a commit landing between flag-set and sleep: the waker saw
  no flag, the sleeper saw no message. It sleeps forever.
  futex-style primitives exist precisely to make the final
  check-and-sleep atomic.
- Values, not addresses: a pointer stored in the region
  is meaningless in the peer's address space (and a
  dereference of peer-writable bytes besides). Store data,
  and keep pointers on your side of the wall.
- Timeouts: a peer can die while you sleep, so every wait
  should carry a timeout so a dead producer degrades the
  consumer to periodic polling, not a hang.

## Validation

Per-commit, alongside the cargo cycle:

- `cargo +nightly miri test`: the full suite, threaded test
  included (its message count is reduced under `cfg(miri)`,
  since interpreted spin loops are slow). Miri has caught two real
  Stacked Borrows violations: the init retag (0.3.0-4) and
  the guard argument-protector race (0.3.0-6). The latter
  reproduces only when the threaded test runs, so the full
  suite, not a non-threaded subset, stays in the ladder.
- `indices_survive_u32_wrap` pins the free-running-index wrap
  argument (fill/drain across `u32::MAX` with masked
  positions intact).
- [loom](https://docs.rs/loom) for exhaustive ordering
  exploration is a possible follow-on (Ideas).

## MPSC ring (sibling primitive)

Design for the multi-producer single-consumer ring, a
sibling type next to the SPSC ring, not a mode of it. The
SPSC hot path is one Relaxed load + one Acquire load + one
Release store per side. Any unified type would put at least a
per-slot sequence check into that path. So the primitives
stay separate, existing SPSC users pay nothing, and iiac-perf
measures the two directly against each other. Anticipated as
a one-bullet placeholder in
[Pool topology and phasing](#pool-topology-and-phasing), and this
section is the full design.

### MPSC protocol

Vyukov's bounded MPMC queue restricted to many-producer /
one-consumer: producers CAS-claim `producer_idx`. A per-slot
sequence word publishes each slot independently. The seq
exists because claim order and commit order differ under
concurrency: the consumer must observe commits, not claims.

- Sequence array: `M × AtomicU32`, `seq[i]` initialized
  to `i`. Slot at free-running position `pos` (masked at
  access, as SPSC) is:
  - claimable when `seq == pos`,
  - committed when `seq == pos + 1`,
  - released by the consumer storing `seq = pos + M`.
- Producer claim: load `producer_idx` (Relaxed), load
  that slot's seq (Acquire), branch on the wrapping-signed
  diff `seq.wrapping_sub(pos) as i32`:
  - `== 0`: claimable:
    - `compare_exchange_weak(pos, pos + 1)` on
      `producer_idx`, and success claims the slot exclusively.
    - Write the payload in place.
    - `seq.store(pos + 1, Release)` commits.
  - `< 0`: full:
    - The slot's previous occupant is not yet released.
    - The `on_full` policy decides retry vs give-up.
  - `> 0`: lost the race to another producer:
    - Reload `producer_idx` and retry.
    - Not a policy call: the system made progress, only
      this producer fell behind.
- Consumer: single, as in SPSC, so its side needs no
  CAS:
  - wait for `seq == c + 1` (Acquire),
  - read the slot,
  - store `seq = c + M` (Release),
  - advance the private index.
- **Producers never read `consumer_idx`**:
  - fullness falls out of the slot seq
  - the header's consumer index line is diagnostic
    occupancy only.
- **Free-running u32 + power-of-two M** carry over
  unchanged: `2^32 % M == 0` keeps seq arithmetic and masked
  positions wrap-safe.
- Atomic floor: the claim CAS raises the floor for MPSC
  users only:
  - the module is gated on `target_has_atomic = "32"`
  - the SPSC ring stays load/store-only (thumbv6m keeps
    working).

### MPSC API: closure send

The producer-side guard is replaced by a closure, because in
MPSC an abandoned *claim* wedges the queue, the consumer
waits at that position forever, where SPSC's dropped guard
was free (`producer_idx` never moved). Abandonment is made
unrepresentable rather than handled:

- `MpscRing::init(region, slot_size, capacity)` /
  `unsafe attach`: mirror the SPSC constructors:
  - own magic and layout_version
  - cross-attaching a region of the wrong kind fails
    toward `BadMagic`.
- `split() -> (MpscProducer, MpscConsumer)` with
  `MpscProducer: Clone + Send`:
  - one handle per producing thread
  - cross-process producers attach
  - the endpoint claims word models role slots, not a
    single producer bit, so N producer claims fit,
    already noted in its todo.
- `producer.send_with::<T>(on_full, fill)` ->
  `Result<(), Full>`:
  - `fill: impl FnOnce(&mut T)` writes the payload in
    place: claim, fill, commit on closure return.
  - commit-by-construction: no public abandonment state.
  - `on_full(attempt) -> bool` is the same wait-policy
    seam as SPSC (`|_| false` = single probe).
- `consumer.reserve_slot_with::<T>(on_empty)` ->
  `ReadSlot`: the SPSC consumer guard survives:
  - consumer abandonment is harmless (drop without
    release re-delivers the same slot)
  - so the guard shape stays.
- Unwind path: a panic in `fill` must not wedge the
  queue:
  - the unwind runs an internal guard's Drop, which
    tombstones the claimed slot, a marked commit the
    consumer releases without delivering.
  - sketch: a reserved seq bit masked out of the index
    arithmetic (constrains `M <= 2^30`). Encoding is an
    open question below.
  - only unwinding reaches it, since panic=abort targets never
    do.

### Overflow readiness

The sender-private overflow FIFO
([Overflow FIFO (future)](#overflow-fifo-future)) must
compose with MPSC when it lands. Two properties keep that
true, and both are named requirements so a later protocol tweak
cannot silently break them:

- **`Full` is a zero-footprint failure**:
  - a send returning `Full` leaves no claim, no tombstone,
    no protocol residue
  - the protocol gives this by construction (fullness is
    observed in the slot seq *before* any CAS, and a lost
    CAS is also stateless), but it is a requirement, not
    an accident.
- **`Full` is the overflow seam**:
  - the future MPSC DescSender probes with `|_| false` and
    on `Err(Full)` appends the buffer to its own pending
    list
  - pending lists stay per-sender: no shared list, no
    CAS
  - per-sender draining preserves exactly the per-producer
    FIFO order MPSC promises anyway, inter-producer order
    was never promised.

### MPSC trust

Same posture as the SPSC ring: every shared word is
untrusted, corruption degrades service and never causes UB.

- The seq array joins the untrusted set:
  - positions are always masked, so scribbled seqs yield
    spurious Full/Empty or garbage-but-valid `T`s
    (zerocopy guarantees representation, not sense)
  - never out-of-bounds access.
- A hostile *producer* peer is new surface relative to
  SPSC:
  - it can claim and never commit, wedging the queue, a
    liveness loss
  - or commit garbage
  - denial of service stays possible, as it always was,
    memory safety does not depend on peers.

### Fan-in (composition, not a mode)

The other standard many-to-one shape: one SPSC ring per
producer, the consumer polls N endpoints. Both have a place:

- the shared MPSC ring:
  - arrival-order fairness ("fair FIFO")
  - one place to poll.
- fan-in:
  - stays load/store-only
  - zero producer-to-producer interference
  - the consumer chooses the service policy (priority,
    round-robin, weighted, ...).

Fan-in is buildable today from shipped SPSC parts. Likely
both shapes get offered eventually (a fan-in helper as a
later convenience, no commitment yet). The bench matrix
measures both from the start.

### MPSC measurement plan

iiac-perf (sibling repo) grows an mpsc set alongside `zcr`:

- `zcr-mpsc-1t`: same-thread round-trip, pure protocol
  overhead (CAS + seq vs load/store), against
  `zcr-with-1t`.
- `zcr-mpsc-2t`: 1 producer / 1 consumer cross-core: what
  MPSC costs when you don't need it, against `zcr-with-2t`.
- `zcr-mpsc-3t` / `-5t`: 2 / 4 producers, one consumer:
  claim-contention scaling, the numbers SPSC cannot
  produce.
- Fan-in comparator at the same producer counts (N SPSC
  rings, round-robin poll).
- We think the shared ring:
  - loses to fan-in on raw throughput under producer
    contention (CAS retries vs disjoint cache lines)
  - wins on consumer-side latency and fairness at larger
    N.
- The matrix exists to check that, not assume it.

### MPSC open questions

- Seq array padding: answered by the SPSC v1 probe
  ([SPSC v1: seam-word ring](#spsc-v1-seam-word-ring),
  measured 2026-09-04): packed wins, on both machines and
  in both instruments, so the family keeps Vyukov's shape
  and no padded variant earns a flag.
- **Tombstone encoding**:
  - reserved seq bit, masked arithmetic, `M <= 2^30`
  - or a per-slot side word
  - only the unwind path needs it, so the simplest
    correct encoding wins.
- **Header consumer index**:
  - keep as diagnostic occupancy (Relaxed stores by the
    consumer), or drop the line entirely
  - the `occupancy()` polish todo argues for keeping it.

## SPSC v1: seam-word ring

The second SPSC protocol, `spsc::v1`, a sibling of v0 under
the same module layout ([Findings: the gap is line-transfer
economics](chores/chores-02.md#findings-the-gap-is-line-transfer-economics)
is the motivation). v0 loses to the MPSC ring cross-core,
~10.0 cache lines per round trip against ~6.7, because each
side polls the other side's index line. v1 is the MPSC
protocol with the claim CAS removed: a per-slot seq word
publishes each slot, and each side reads only slot lines and
its own index.

- Region: the v0 four-line `Header` shape (own type,
  own magic `ZCR2`, own layout version), then the seq array
  (`M x AtomicU32`, padded to a cache line), then the slots.
  `spsc::v1::region_size` is public so a pool can size
  segment buffers by it.
- Protocol: `seq[i]` starts at `i`. Slot at free-running
  `pos`:
  - claimable by the producer when `seq == pos`.
  - committed when `seq == pos + M + 1` (the producer's
    `Release` store, after the fill).
  - released by the consumer storing `seq = pos + M`
    (`Release`), which is the next lap's claimable value.
  - Committed is `pos + M + 1`, not Vyukov's `pos + 1`: at
    `M = 1` that would equal the released value `pos + M`,
    so the producer would read an unread slot as claimable
    and overwrite it (found by the `M = 1` tests, which
    failed and hung on the first cut).
  - Equality, not a signed diff: there is no lost race to
    distinguish, so anything but the expected value reads as
    Full or Empty, and a peer-corrupted seq degrades toward
    that, never toward an unowned slot.
- Index lines are private resume state: `producer_idx`
  is loaded and stored by the producer only, `consumer_idx`
  by the consumer only, both `Relaxed`. They exist so a
  re-attach resumes mid-stream and so occupancy can be
  inspected, and no hot path crosses to the other side's
  line.
- Load/store only: no CAS anywhere, so the v0 atomic
  floor holds. The MPSC tombstone has no counterpart: a single
  producer that unwinds mid-fill abandons the reservation,
  as v0 does.
- `M >= 1`: the state is in the seq, not in an index
  distance, so `M = 1` is legal: one word cycles through
  claimable, committed, and released. The user picks `M`, any power of
  two up to `2^30`, so a segment-size sweep can isolate the
  segment seam ([the cycle](../TODO.md#feat-seam-word-spsc-v1)).
- Measured (2026-08-28, 3900X, `tp-matrix` 5 s cells and
  the demo's 1M-message streams): v1 closes most of v0's
  gap and lands beside the MPSC ring, not ahead of it:
  - fills per round trip: v0 10.0, v1 6.85, MPSC 6.7, at every
    cross-core placement. The seam word removed the index-line
    traffic as designed.
  - round trips per 5 s: 0,1 CCX v0 22.7M, v1 28.5M, MPSC
    30.7M. 0,3 x-CCX v0 6.5M, v1 8.2M, MPSC 8.8M. 0,12 SMT
    v0 40.9M, v1 34.2M, MPSC 34.7M. Demo streams, diff cores
    0+3: v0 166 ns, v1 105 ns, MPSC 92 ns per message. Same
    core 0+12: v0 6.9, v1 13.8, MPSC 15.4. Single thread: v0
    2.6, v1 7.8, MPSC 9.8.
  - So v1 is ~26% faster than v0 cross-core, ~7% slower than
    the MPSC ring there, and loses v0's SMT and single-thread
    win by 2 to 3x, as the MPSC ring does. The cycle's bar,
    faster than MPSC v0 on the non-overflow path, is not met by
    the separate-seq-array form.
  - Open puzzle: v1 does strictly less than the MPSC producer
    (a load where MPSC has a CAS) and is slower by a few ns per
    send at every placement (`w.send` 13.0 vs 10.1 ns on the
    CCX). We think it is not the protocol, and the next
    candidates are code shape (the `WriteSlot` deref path
    against `send_with`'s closure fill) and the store ordering
    (the private index store ahead of the seq store, where the
    MPSC CAS drains the store buffer first).
- Measured on a 7600X (Zen 4, one CCD), the demo's
  streams, the picture reverses: single thread v0 2.2 ns,
  v1 6.1, MPSC 7.1. Diff cores 0+1 v0 7.0, v1 18.9, MPSC
  17.7. Same core 0+6 v0 6.0, v1 13.9, MPSC 13.3. Streaming
  with depth 64, the producer running ahead, v0 beats both
  seq protocols by 2.5x cross-core. We think the cause is
  that a seq word is written by both sides every message and
  16 seq words share a line: v0's slot lines move one way
  (producer writes, consumer reads) and only its two index
  lines move both ways, while every v1 message also drags the
  seq line producer to consumer at commit and back at release,
  and neighbouring slots' seqs false-share it. The
  one-in-flight `tp-matrix` cell cannot show this, since there
  the seq line and the slot line move once each per trip, but
  the streaming demo can.
- Measured (2026-09-04, both machines, the rung `perf:
  probe the v1 streaming loss`): the false-sharing
  hypothesis above is refuted, and two things the numbers
  rested on turned out to be assumptions:
  - Line-padding the seq array (one seq per line, a
    `SEQ_STRIDE` flip in `spsc::v1`) cuts fills per round
    trip from 6.4 to 5.3 on the 3900X, below the MPSC ring's
    6.4, and buys no throughput: normalised to MPSC in the
    same runs, v1's round trips fell at every placement on
    both machines. Streaming, padding cost 13% on the 7600X
    cross-core line it was meant to fix (17.6 to 19.9 ns)
    and 12% at the 3900X's SMT pair, and gained 31% at the
    7600X's SMT pair (12.9 to 8.9 ns, the one placement
    where v1 beats MPSC), an effect with no account yet.
    Packed stays. We think padding loses because the seq
    words are a queue, not unrelated neighbours: packed,
    sixteen consecutive commits land in one line and its
    acquisition amortises. Padded, every commit pays its
    own.
  - Neither side waits. `examples/occupancy_probe` runs the
    demo's two-thread stream and counts wait-policy calls:
    the producer waited on under 0.1% of sends and the
    consumer on 0 to 8% of receives, at every placement on
    both machines. The loss is steady-state per-message
    cost, not one side blocking on the other.
  - The two machines never disagreed. The demo's "diff
    cores" pair is the first cpu outside cpu0's L3: cpu 3 on
    the 3900X, a different CCX, and cpu 1 on the 7600X,
    whose six cores share one L3. Measured same-L3 on the
    3900X (0+1), v0 streams at 12.9 ns against v1's 34.7,
    the 7600X's shape exactly, and v1 wins only across L3
    (0+3: v0 171, v1 105). One picture: v0 wins streaming
    within an L3, v1 wins streaming across one.
  - Every fills-per-round-trip figure in this note is from
    the one-in-flight `tp-matrix` cell. The demo's `*_2t`
    lines are the only streaming evidence.
- Open, for the next step: the per-send puzzle stands
  and is sharper: padded v1 moves fewer lines per round trip
  than MPSC and is still slower. Candidates, all untested:
  the store buffer (MPSC's claim CAS is a full fence and v1
  has none, so a fence after v1's commit is a one-line
  probe), the private index store ahead of the seq store,
  and the guard's code shape against `send_with`. The in-slot
  seq stays the layout step, framed as a crate-owned slot
  header ahead of the user-owned body, with a prediction to
  test: it should help the round trip and may hurt
  streaming, since the slot line would then travel both
  ways every message where today it goes one way and the
  seq line amortises. The seq's width is not to be assumed
  and wants measuring (u32 against the native width) before
  the slot header fixes it. The demo's pin-pair picker wants
  a same-L3 placement and honest labels.

- Measured (2026-09-07, 3900X, the rung `perf: probe a
  fence after the v1 commit`): the store-buffer candidate is
  refuted. A `CommitFence` switch in `spsc::v1`'s producer puts
  a `SeqCst` fence after the commit store (`mfence`) or makes
  the store itself `SeqCst` (`xchg`), and `tp-cell` at depth 8
  for 3 s per placement plus the demo's v1 stream lines ran
  all three forms:
  - Round trip, main send / worker send in ns (mean of the
    min-p99 band) and trips per 3 s: 0,1 CCX none 8.7 / 11.1,
    17.5M, mfence 9.5 / 9.6, 18.2M, xchg 8.6 / 10.9, 17.1M. 0,3
    x-CCX none 8.5 / 10.7, 5.67M, mfence 8.4 / 11.2, 5.66M,
    xchg 8.5 / 11.4, 5.62M. 0,12 SMT none 8.9 / 13.0, 21.2M,
    mfence 8.9 / 13.2, 21.2M, xchg 8.9 / 13.1, 20.2M. Fills per
    trip 6.47 to 6.50 in every cell. Nothing moved beyond run
    noise.
  - Streaming, the demo's v1 line: across the CCX 105.9 none,
    107.1 mfence, 106.1 xchg. At the SMT pair 12.2 none, 15.7
    mfence, 21.2 xchg, so a drained store buffer costs the
    sibling-pair stream 30 to 70%.
  - So v1's per-send gap to the MPSC producer is not the store
    buffer, and the remaining candidates are the private index
    store ahead of the seq store and the guard's code shape.
    The switch stays at `None`.
- Landed as-is (2026-09-07, the cycle `feat: seam-word
  SPSC v1`): the ring landed on `main` with its bar unmet,
  so the later experiments compare against a landmark rather
  than a draft. The crate's default `Ring` stays v0 and v1 is
  reached by path. The in-slot seq and the segments are
  `TODO.md` entries, "In-slot seq for spsc v1" and "Segmented
  queue over spsc v1".
- Segments, designed and deferred: the layer the cycle
  meant to build on the ring, held until a v1 form clears the
  bar. The design as decided:
  - A queue is a chain of ring segments allocated from a
    `Pool`, each segment one pool buffer holding a v1 region
    sized by `region_size`.
  - The link word is the segment's header `user` line, word
    0, holding the next segment's pool buffer index with a
    sentinel for none. Not the pool's free-stack word: the
    free overwrites it, and the pool stays ignorant of rings.
  - Producer on Full: allocate the next segment, init it,
    store its index in the link (`Release`), move. Consumer
    on Empty: load the link (`Acquire`), and a set link means
    the old segment is fully drained, since the producer
    moved only after filling it and Empty means all of it was
    read, so the consumer moves and frees the old segment.
  - The endpoints hold the pool halves their roles need: the
    producer the `Pool`, its one allocator, and the consumer a
    `PoolView` to free. The pool's existing contract.
  - One link per segment, amortised over `M` messages, and
    `M = 1` is the per-message linked list, measured by the
    size sweep rather than imagined.
  - The pool is unchanged apart from a way to take a buffer
    as raw bytes for `Ring::init`, if `alloc::<T>` cannot
    express it. Anything more waits for a shown need.

## SPSC v2: in-slot seq ring

The third SPSC protocol, `spsc::v2`, a sibling of v0 and v1
under the same module layout, the in-slot seq experiment the
v1 cycle left open ([SPSC v1: seam-word
ring](#spsc-v1-seam-word-ring)). v1 publishes a slot through a
seq word in a separate array, so a message costs the slot line
and a share of a seq line. v2 moves the seq into the slot it
publishes, so the commit store and the message it publishes
travel on one line.

- Region: the v0 four-line `Header` shape (own type, own
  magic `ZCR3`, own layout version), then the slots, and no
  seq array. `spsc::v2::region_size` is header plus slots.
- Slot header: every slot opens with `SLOT_HEADER_BYTES`
  (16) of crate-owned bytes, the seq word at offset 0 and the
  rest reserved and zeroed. The user's body starts behind it,
  so a slot of N bytes carries `N - 16` bytes of message at an
  alignment of at most 16. Sixteen so the seq can be u32 or
  u64 without moving the body.
- Slot contract: `T` must fit the body and align to at
  most the header's size, checked at every reserve as the
  other rings check theirs, against the body rather than the
  slot. A one-line slot carries a 48-byte message. This is the
  contract change the Todo entry named as part of the finding.
- Protocol: v1's, unchanged, claimable at `seq == pos`,
  committed at `pos + M + 1`, released at `pos + M`, equality
  checks, load/store only, `M` any power of two down to 1, and
  the index lines private resume state. The one difference is
  where the word lives.
- Seq width: one alias, `Seq`, chooses `AtomicU32` or
  `AtomicU64`. The indices stay u32 and wrap there, so the
  word holds the same values at either width and the flip
  changes the store's width alone. The measurement rung runs
  both before the layout version fixes one.
- Prediction, on record before measuring: the round trip
  should gain, since the consumer needs one fill for data plus
  flag instead of two. Streaming may lose, since the
  consumer's release store dirties the slot line and it then
  travels producer to consumer at commit and back at release,
  two transfers per message, where v1's slot line moves one
  way and its packed seq line amortises sixteen commits. At
  depth 1, v2 is a single line ping-ponging between the cores,
  the cheapest handoff the hardware can express.
- Measured (2026-09-07, 3900X, the rung `perf: measure spsc
  v2 across depths`, `tp-matrix` 5 s cells and the demo's 1M
  message streams, both at depths 1, 2, 8, and 64). The
  round trip confirms the prediction and the streaming
  refutes it, and v2 is the first SPSC form to clear the bar
  the v1 cycle set: faster than MPSC v0 at every cross-core
  placement.
  - Round trips per 5 s and fills per trip, one cell per
    flavor and depth (the MPSC v0 ring cannot run at depth 1,
    see [MPSC v1: equality-seq ring](#mpsc-v1-equality-seq-ring)):

    | placement | flavor  | d=1          | d=2          | d=8          | d=64         |
    |-----------|---------|-------------:|-------------:|-------------:|-------------:|
    | 0,1 CCX   | spsc-v0 | 24.6M (11.1) | 25.6M (10.7) | 25.5M (10.0) | 25.7M (9.98) |
    | 0,1 CCX   | spsc-v1 | 30.4M (8.13) | 25.4M (8.49) | 31.4M (6.35) | 31.3M (6.02) |
    | 0,1 CCX   | spsc-v2 | 33.7M (4.00) | 34.5M (4.05) | 28.9M (3.08) | 28.7M (3.72) |
    | 0,1 CCX   | mpsc-v0 | -            | 27.9M (8.30) | 32.7M (6.36) | 32.2M (6.04) |
    | 0,3 x-CCX | spsc-v0 |  6.8M (11.3) |  6.8M (10.8) |  7.0M (10.1) |  6.8M (10.0) |
    | 0,3 x-CCX | spsc-v1 | 10.1M (8.07) |  7.7M (8.39) | 10.2M (6.36) |  9.2M (6.00) |
    | 0,3 x-CCX | spsc-v2 | 11.4M (4.00) | 11.3M (4.04) | 10.3M (3.08) | 10.4M (3.12) |
    | 0,3 x-CCX | mpsc-v0 | -            |  8.2M (8.24) | 10.4M (6.35) |  9.1M (6.05) |
    | 0,12 SMT  | spsc-v0 | 43.4M        | 43.5M        | 43.7M        | 43.5M        |
    | 0,12 SMT  | spsc-v1 | 35.4M        | 35.4M        | 35.1M        | 35.4M        |
    | 0,12 SMT  | spsc-v2 | 37.9M        | 37.9M        | 37.3M        | 37.9M        |
    | 0,12 SMT  | mpsc-v0 | -            | 36.4M        | 36.3M        | 36.1M        |

  - Streaming, the demo's sweep, ns per message:

    | placement          | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |--------------------|---------|------:|------:|------:|------:|
    | 1t core 0          | spsc-v0 |   2.4 |   2.4 |   2.4 |   2.4 |
    | 1t core 0          | spsc-v1 |   6.9 |   6.8 |   6.7 |   6.7 |
    | 1t core 0          | spsc-v2 |   6.5 |   6.4 |   6.4 |   6.4 |
    | 1t core 0          | mpsc-v0 |     - |  11.0 |  11.0 |  11.0 |
    | 2t diff cores 0+3  | spsc-v0 | 326.6 | 203.3 | 154.2 | 208.8 |
    | 2t diff cores 0+3  | spsc-v1 | 378.3 | 213.8 | 120.7 | 102.5 |
    | 2t diff cores 0+3  | spsc-v2 | 198.4 | 103.1 |  35.1 |  12.1 |
    | 2t diff cores 0+3  | mpsc-v0 |     - | 206.5 | 104.6 |  76.8 |
    | 2t same core 0+12  | spsc-v0 |  30.0 |  14.1 |   7.2 |   6.7 |
    | 2t same core 0+12  | spsc-v1 |  50.1 |  26.1 |  15.1 |  11.9 |
    | 2t same core 0+12  | spsc-v2 |  39.5 |  20.5 |   7.1 |   7.2 |
    | 2t same core 0+12  | mpsc-v0 |     - |  23.8 |  15.3 |  15.2 |

    The unpinned rows are omitted: the scheduler's placement
    varies run to run and the numbers with it.
  - Round trip, the why: a v2 trip moves four lines at depth
    1 and 2, the request slot each way and the response slot
    each way, and nothing else, where v1 and the MPSC ring
    move the seq line beside each slot line, eight. At depth
    8 and 64 the seq protocols fall to six, since the seq line
    then holds several slots' words and a lap amortises it,
    and v2 falls to three, the fourth line's share amortised
    the same way now that consecutive trips use consecutive
    slots. The trips per 5 s follow the fills, and the send
    costs are flat at 8.4 to 10 ns for the three seq
    protocols at every cross-core placement, so the trip is
    line transfers and nothing else.
  - Streaming, the why, and the refuted prediction: the slot
    line does travel both ways per message, and it does not
    matter, because it is the only line that travels. We
    think the gain is line independence rather than line
    count: consecutive messages in v2 are consecutive lines
    with no shared word between them, so the producer's
    commits and the consumer's reads and releases of
    different lines overlap in the memory system, where v0
    serialises every message on its two index lines and v1
    on its packed seq line, both written by both sides every
    message. At depth 1 v2 has one line and no overlap, and
    the stream costs a round trip per message, 198 ns, the
    floor the prediction named. Each doubling of depth from
    there buys overlap, to 12 ns at 64.
  - Within an L3 and at the SMT pair v2 matches v0 to the
    nanosecond at depth 8 and 64, 7 ns, and v1 sits at 12 to
    15. The one place v2 trails is the single-thread loop and
    the shallow SMT stream, where v0's index protocol is
    cheaper than any seq word, 2.4 against 6.4 ns.
  - Seq width: `Seq` as `AtomicU64` against `AtomicU32` in
    `tp-cell` at the three pinned placements and depths 1, 8,
    and 64, and in the demo's stream lines, moved nothing
    beyond run noise (trips within 5%, sends within 1 ns,
    streams within 1 ns pinned). u32 stays, the v1 width and
    the smaller word, and the layout version is fixed at it.
  - The 7600X (Zen 4, one CCD, six cores under one L3), the
    demo at the same build, pasted in by the user 2026-09-07.
    The lines at depth 64, ns per message: single thread v0
    2.1, v1 6.1, v2 5.7, MPSC 7.2. Diff cores 0+1 (same L3)
    v0 6.9, v1 19.1, v2 3.3, MPSC 17.5. Same core 0+6 v0 6.3,
    v1 13.7, v2 4.7, MPSC 13.3. The sweep:

    | placement          | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |--------------------|---------|------:|------:|------:|------:|
    | 1t core 0          | spsc-v0 |   2.1 |   2.1 |   2.1 |   2.1 |
    | 1t core 0          | spsc-v1 |   6.4 |   6.2 |   6.0 |   6.0 |
    | 1t core 0          | spsc-v2 |   5.5 |   5.6 |   5.6 |   5.4 |
    | 1t core 0          | mpsc-v0 |     - |   6.4 |   6.7 |   6.8 |
    | 2t diff cores 0+1  | spsc-v0 |  63.5 |  39.0 |  11.6 |   6.3 |
    | 2t diff cores 0+1  | spsc-v1 |  75.9 |  43.6 |  17.8 |  17.0 |
    | 2t diff cores 0+1  | spsc-v2 |  40.8 |  24.1 |   7.0 |   3.4 |
    | 2t diff cores 0+1  | mpsc-v0 |     - |  37.6 |  17.9 |  15.7 |
    | 2t same core 0+6   | spsc-v0 |  27.8 |  14.7 |   5.3 |   5.8 |
    | 2t same core 0+6   | spsc-v1 |  43.6 |  24.2 |  13.3 |  12.3 |
    | 2t same core 0+6   | spsc-v2 |  26.1 |  10.7 |   4.7 |   4.3 |
    | 2t same core 0+6   | mpsc-v0 |     - |  22.6 |  12.7 |  12.0 |

    So the picture that reversed between the machines for v1
    does not reverse for v2: within the 7600X's one L3, where
    v0 beat v1 by 2.5x, v2 beats v0 at every depth from 2 up,
    3.4 against 6.3 ns at 64, and at the SMT pair it matches
    or beats v0 from depth 2 up, the one place the 3900X gave
    v0 a tie. Single-threaded v0 keeps its lead on both, and
    at depth 1 the two are within run noise of each other on
    the 7600X and v2 is ahead on the 3900X.
- Default (2026-09-07, at the cycle's Land): the crate's
  `Ring` re-export is v2, on the user's call at the review,
  since it is the fastest form at every placement but the
  single-thread loop, where v0 keeps a 2.5x lead and stays
  reachable by path. A user of the bare `Ring` now has v2's
  slot contract, `slot_size - 16` bytes of message at an
  alignment of at most 16.
- Measured (2026-09-07, 3900X, the rung `feat: a streaming
  cell with fill counts`, `tp-stream` 5 s cells): the
  streaming cell, a producer thread streaming a counter for the
  duration to a consumer thread over one ring, with the fill
  counters open. ns per message and fills per message:

    | placement | flavor  | d=1          | d=2          | d=8          | d=64         |
    |-----------|---------|-------------:|-------------:|-------------:|-------------:|
    | 0,1 CCX   | spsc-v0 |  79.7 (6.27) |  57.8 (5.54) |  21.6 (2.71) |   9.6 (0.71) |
    | 0,1 CCX   | spsc-v1 |  92.0 (4.56) |  54.1 (3.69) |  25.6 (1.60) |  22.7 (1.03) |
    | 0,1 CCX   | spsc-v2 |  62.9 (2.00) |  31.6 (2.00) |   8.8 (0.87) |   4.9 (0.14) |
    | 0,1 CCX   | mpsc-v0 | -            |  54.3 (3.68) |  28.8 (1.85) |  21.8 (0.87) |
    | 0,3 x-CCX | spsc-v0 | 340.1 (6.31) | 207.3 (6.01) | 166.7 (4.71) | 193.3 (3.98) |
    | 0,3 x-CCX | spsc-v1 | 404.6 (5.08) | 206.0 (3.94) |  85.7 (1.86) |  37.8 (0.51) |
    | 0,3 x-CCX | spsc-v2 | 198.8 (2.00) | 103.6 (2.00) |  37.0 (1.44) |  13.9 (0.13) |
    | 0,3 x-CCX | mpsc-v0 | -            | 210.1 (3.91) | 113.4 (2.16) |  90.5 (1.57) |
    | 0,12 SMT  | spsc-v0 |  29.5        |  16.5        |   7.4        |   6.8        |
    | 0,12 SMT  | spsc-v1 |  37.9        |  26.3        |  15.9        |  15.5        |
    | 0,12 SMT  | spsc-v2 |  34.6        |  21.1        |  12.8        |  17.2        |
    | 0,12 SMT  | mpsc-v0 | -            |  23.3        |  15.2        |  15.0        |

  - The line count while streaming, the number the round-trip
    cell could not give: at depth 1 and 2 v2 moves exactly
    2.00 lines per message, the slot line each way, the
    prediction's figure. At depth 64 across the CCX it moves
    0.13, and v1 0.51, both below one line per message, so
    most slot lines are not demand-fetched at all. We think
    the consumer's prefetcher pulls consecutive slot lines
    ahead of demand, since consecutive slots are consecutive
    lines, and v2 gains most because the slot line is the only
    line it touches, while v1's seq line and v0's index lines
    are written by both sides and cannot be prefetched into a
    useful state.
  - The demo's streams and this cell disagree on v0 and v1,
    and the disagreement is a finding. Across the CCX at depth
    64, v1 streams at 104 ns per message with 1.80 fills in a
    plain counted loop (the demo's shape, and this cell's
    with its clock check removed) and at 38 with 0.51 when the
    producer reads the clock every 4096 sends. The same
    hiccup leaves v0 at 130 to 190 and v2 at 14 either way. We
    think v1 is bistable there: the two sides either write its
    packed seq line in lockstep, one transfer each way per
    message, or the producer runs ahead and the line moves in
    bursts, and a periodic pause on the producer tips it into
    the second regime. Every earlier v1 streaming figure in
    this note is the lockstep regime.
  - The poll's cost matters too: an out-of-line spin policy
    (the runner's, called from another crate) put v2's
    cross-CCX stream at 31 against 14 with the crate's inline
    `policy::spin`, which the cell now uses. Run length, the
    thread shape, the fill counters, the payload width, and a
    fat-LTO build were each tried and moved nothing of that
    size.
  - Within an L3 v2 at depth 64 streams at 4.9 ns per message
    with 0.14 fills, twice v0's rate. At the SMT pair, where
    no line crosses, v0 keeps its 2x over every seq protocol.

## SPSC v3: ring of segments

The fourth SPSC protocol, `spsc::v3`, a sibling of v0 through
v2 and, since the cycle `feat: segmented queue SPSC v3`, the
crate's default `Ring`. A v2 ring is fixed-length, so a
producer that outruns its consumer finds it Full. v3 is a ring
of up to 32 segments, each a ring of its own, and when the
consumer keeps up only one is in use. The others are insurance
for a producer that runs ahead, and switching between them is
meant to be the only cost v3 adds. It is a design built to be
measured, and one that falls short leads to a v4.

- **Segments**: `Ring::init(pool, slot_size, seg_capacity,
  seg_count)` takes every segment from the application's pool
  with `Pool::alloc_bytes` and initializes each: a header line,
  then `seg_capacity` slots opening with v2's in-slot seq word.
  `segment_size` is what the pool's buffers must hold. Nothing
  allocates, frees, or re-initializes while the ring runs, and
  the pool is borrowed only during `init`.
- **The seq word**: 32 bits, so v3 builds wherever v0 through
  v2 do. The low 26 bits hold v2's claimable `pos`, committed
  `pos + M + 1`, and released `pos + M`, and the high bits a
  MOVED flag and the next segment's number, so one load tells
  the consumer empty, a message, or a message and then segment
  `k`. The width caps a segment at `2^24` slots and a ring at
  32 segments.
- **Switching**: the producer decides at its own commit, the
  one moment it alone writes the slot's word. When the next
  slot is not claimable it takes a free segment and commits the
  message with MOVED. A slot seen claimable stays claimable
  until the producer claims it, so that look-ahead replaces the
  next reserve's load. The consumer, after a MOVED message,
  releases it, gives the old segment back, and follows.
- **Free segments without CAS**: the producer flips a
  segment's bit in a private word when it takes it, the
  consumer a bit in a shared word, in segment 0's header, when
  it gives one back. A segment is free where the two agree, and
  `trailing_zeros` finds one.
- **Reuse**: each side keeps a private resume position per
  segment, and both leave a segment at the same slot, so a
  reused segment's seqs are already claimable.
- **Limits**: no `attach`, the ring's state spanning a pool and
  its segments. Code that needs a single region names
  `spsc::v2::Ring`, and a ring of segments to share between
  processes is [SPSC v4](#spsc-v4-attachable-segments).
- **Counters**: `Producer::switches` and `Consumer::switches`
  count switches on the switch path only, and `segment` names
  the current one. `examples/spsc_v3_segments.rs` runs every
  segment count from 1 to 32 at depths 1, 8, 64, and 1024.
- **Carried to MPSC**, for a segmented MPSC as its own
  implementation: producers racing to take a segment need a CAS
  where v3's single producer does not, a segment must be sealed
  before the switch so no late claim lands in it, and a slow
  producer may still hold a segment being given back, so
  reclamation is the hard part.
- **Prediction, on record before measuring**: with the consumer
  keeping up, v3 matches v2 at the same segment depth, both
  loading one seq per message on each side. We thought segment
  depth 1 with the producer ahead would run within twice v2's
  cost per message.
- **Measured (2026-09-15, 3900X and 7600X, the rung `feat: spsc
  v3 in the measurement tools`, `tp-matrix` and `tp-stream` 1 s
  cells at depths 1, 8, 64, and 1024, two segments, each sweep
  run twice)**. The runs agreed within 10% but for the marked
  (`*`) cells, whose means moved more than 15% between runs. The
  7600X has no cross-CCX placement, its six cores sharing one
  L3.
  - Streaming, the producer running ahead: ns per message v2 /
    v3, and v3's switches per message:

    | 3900X     | d=1                  | d=8                  | d=64                 | d=1024               |
    |-----------|---------------------:|---------------------:|---------------------:|---------------------:|
    | 0,1 CCX   |  63.1 / 64.0 (0.637) |  8.1 / 18.4 (0.003)  |  4.9 / 16.1 (0.000)  |  5.2 / 21.6 (0.000)  |
    | 0,3 x-CCX | 193.7 / 237.4 (0.604)| 31.5 / 56.4 (0.000)  | 13.4 / 20.9 (0.000)  |  8.2 / 29.4 (0.001)  |
    | 0,12 SMT  |  34.8 / 36.3 (0.513) |  8.2 / 20.8 (0.002)  |  8.1 / 21.1 (0.003)  |  8.1 / 21.4 (0.001)  |
    | 7600X     |                      |                      |                      |                      |
    | 0,1 CCX   |  40.0 / 52.4 (0.504) |  7.6 / 13.5 (0.001)  |  3.0 / 11.6 (0.004)  |  7.4 / 9.0 (0.001)   |
    | 0,6 SMT   |  22.1 / 35.2 (0.500) |  4.7 / 16.3 (0.074)  |  4.4 / 15.2 (0.013)  |  4.4 / 15.0 (0.001)  |

  - The round trip, one message in flight so the consumer keeps
    up: main's send and the worker's receive, means in ns, v2 /
    v3, and v3's switches per round trip, 3900X:

    | placement | depth | m.send v2 / v3 | w.recv v2 / v3 | xfills/RT v2 / v3 | switches/RT |
    |-----------|------:|---------------:|---------------:|------------------:|------------:|
    | 0,1 CCX   |     1 |    8.5 / 26.8* |   83.3 / 115.7 |     4.004 / 8.085 |       2.000 |
    | 0,1 CCX   |     8 |     9.0 / 12.1 |  112.1 / 113.9 |     3.487 / 3.287 |       0.000 |
    | 0,1 CCX   |    64 |     9.6 / 12.2 |  122.3 / 115.9 |     2.774 / 2.157 |       0.000 |
    | 0,1 CCX   |  1024 |     9.3 / 12.3 |  117.5 / 115.0 |     2.270 / 2.024 |       0.000 |
    | 0,3 x-CCX |     1 |  8.4* / 125.2* |  271.1 / 387.8 |     4.027 / 8.181 |       2.000 |
    | 0,3 x-CCX |    64 |     8.5 / 12.9 |  336.2 / 344.5 |     2.828 / 2.336 |       0.000 |
    | 0,12 SMT  |     1 |     8.6 / 26.3 |   77.3 / 121.6 |   0.0004 / 0.0010 |       2.000 |
    | 0,12 SMT  |    64 |     8.6 / 18.1 |   76.5 / 115.8 |   0.0005 / 0.0007 |       0.000 |

    and 7600X:

    | placement | depth | m.send v2 / v3 | w.recv v2 / v3 | switches/RT |
    |-----------|------:|---------------:|---------------:|------------:|
    | 0,1 CCX   |     1 |     7.1 / 17.8 |   59.2 / 141.4 |       2.000 |
    | 0,1 CCX   |     8 |     6.8 / 10.8 |   60.4 / 115.7 |       0.000 |
    | 0,1 CCX   |    64 |     6.6 / 10.3 |   89.3 / 111.7 |       0.000 |
    | 0,1 CCX   |  1024 |     6.7 / 10.4 |   82.5 / 111.8 |       0.000 |
    | 0,6 SMT   |     1 |     6.8 / 18.5 |   57.9 / 109.5 |       2.000 |
    | 0,6 SMT   |    64 |     6.8 / 14.7 |   56.8 / 89.1  |       0.000 |

- **Readings**:
  - The design does what it says. With the consumer keeping up,
    the round trip at depth 8 and up switches 0.000 times on
    both machines, and at depth 1 exactly twice per round trip,
    once per ring. Streaming at depth 8 and up, the producer
    running ahead, it switches at most 0.074 times per message.
  - The prediction fails on the fast path, not on switching.
    Where no switch happens, v3 streams 2 to 4 times slower than
    v2, 16.1 against 4.9 ns per message on the 3900X's CCX at
    depth 64 and 11.6 against 3.0 on the 7600X, and its sends in
    the round trip run 3 to 4 ns slower, twice that at the SMT
    pair. Its receives match v2's across the 3900X's CCX but run
    25 to 90% slower at the SMT pairs and on the 7600X.
  - A switch is cheap against the ring it replaces. At depth 1,
    where the stream switches about every other message, v3
    streams within 2% of v2 across the 3900X's CCX, 64.0 against
    63.1, and within 35% on the 7600X, inside the prediction's
    twice. In the round trip a switch doubles the lines moved,
    8 against 4 per round trip, and triples the send.
  - Found while building the tools, and deferred to the Todo
    entry `SPSC v3 fast path`: `commit` and `release` copied the
    ring's whole segment table, about 280 bytes, on every
    message. Borrowing it took the 3900X's CCX stream at depth
    64 from 18.4 to 10.7 ns and the SMT pair from 21.8 to 14.3,
    so the copy is more than half the gap, and the rest is not
    yet found.
  - The 7600X streams differently, not its counters: v2 at
    depth 64 reads 2.0 lines per message across its CCX against
    0.13 on the 3900X, yet its round trips read the expected
    line counts, v0 12, v1 8, and v2 4 per round trip, as on the
    3900X. We think the 3900X's prefetcher pulls consecutive
    slot lines ahead of demand and the 7600X's does not at that
    depth. Its fill columns are left out of the stream table
    above only for width.
- **Verdict (2026-09-15)**: the segment design works and its
  switch is cheap, but v3 as built does not match v2 when the
  consumer keeps up, so the default ring costs more than v2 on
  every path that never needs a second segment. The fast-path
  Todo entry is the next step, measured against these tables.

## SPSC v4: attachable segments

The fifth SPSC protocol, `spsc::v4`, a sibling of v3 built in
the cycle `feat: attachable SPSC v4`: v3's ring of segments,
unchanged as a protocol, over a ring that describes itself in
the region. A v3 ring's table of segments, the pointers every
message goes through, exists only in the process that ran
`init`, so no second process could join the ring, and no design
that keeps the table in shared memory could be measured without
changing v3. v4 is that design, and v3 stays as built to measure
against. The user-facing half is the guide's [Joining from
another process](user-guide.md#joining-from-another-process).

- **Offsets, not pointers**: the endpoints' `Segments` table
  holds each segment's byte offset from the pool's buffer array
  and one base pointer, that array in this process, so the table
  is the same numbers in every process and a slot access is one
  add over v3's. The offsets-only rule ([Offsets only,
  everywhere](#offsets-only-everywhere)) applied to the ring's
  own table. The offsets are from the buffer array rather than
  the region's base because the pool's `bufs` raw pointer
  carries the region's provenance, where a pointer derived from
  its header reference would reach the header alone under
  Stacked Borrows.
- **The control block**: every segment's header grows from v3's
  one line to four, so `segment_size` is v3's plus 192 bytes and
  every segment's slots start at one offset. Line 0 names the
  ring, nine `AtomicU32`s: magic (`"ZCR4"`), layout version,
  `slot_size`, `seg_capacity`, `seg_count`, the segment's own
  number, `given`, the consumer's give-back word, and the
  producer's and consumer's resume positions in this segment.
  Line 1 is the claims line, the two role words and the
  endpoints' checkpoints, alone so the CAS that takes a role
  never shares a line with `given`. Lines 2 and 3 are the table, the
  pool buffer index of segment `i` at entry `i`, `u32::MAX` past
  `seg_count`. Every segment writes line 0, and lines 1 to 3 are
  meaningful in segment 0. `init` stores the magic last, with
  Release.
  - The table holds buffer indices, not byte offsets: a byte
    offset needs a `u64` and four lines, and the pool already
    validates an index and turns it into a pointer. The private
    table holds the byte offsets, computed once at load, so the
    hot path stays at one add.
  - The shape is meant for MPSC v2's successor as well, whose
    header is already a three-line struct with a seal, a claim
    word, and an in-use word, so an attachable MPSC puts the
    same block ahead of them.
- **Attach**: `Ring::attach(&pool, first_segment)`, `unsafe`,
  reads the control block through an attached `Pool` from the
  buffer index of segment 0, `Ring::first_segment()` on the
  initializing side. It checks the magic, the layout version,
  the geometry, that segment 0 says it is segment 0, every table
  entry against the pool's count and the entries before it, and
  every segment's own header against the block, and builds the
  table through the loader `init` uses. Every failure is an
  `Err` (`BadMagic`, `BadLayoutVersion`, the geometry errors,
  `TooSmall`, and `BadSegment` for a table or a header that
  disagrees), never an access outside the pool. It is `unsafe`
  for what validation cannot check, as the pools' `to_slot` is:
  a ring's segment and a buffer freed and reused since look the
  same, and the ring writes seq words into every segment it is
  told it has.
- **Roles claimed by a named holder** (layout version 2, the
  cycle `fix: spsc v4 roles survive their holders`): each role
  is one `AtomicU32` in the claims line, `0` free, `u32::MAX`
  released, and anything else the id of its holder, so claim,
  release, and takeover are each one CAS on it and two
  takeovers cannot both win.
  - `claim_producer(id)` and `claim_consumer(id)`, from a `Ring`
    that `init` or `attach` returned, CAS the role word from
    free or released to `id`. A held role is `Err(RoleTaken)`,
    in this process or another. The ids `0` and `u32::MAX` name
    no holder and are `Err(BadHolder)`. Nothing on the message
    path reads the word.
  - Neither endpoint has a `Drop`, by the rule [Destructors never
    touch shared memory](#destructors-never-touch-shared-memory).
    A dropped endpoint leaves its role held.
    `release(self)` CASes the role word from its own id to
    released, so an endpoint whose role was taken over releases
    nothing.
  - The id is the app's, recorded and never interpreted by the
    crate. On Linux the pid is the natural one, never `0` and
    below `pid_max`, at most `2^22`, so never `u32::MAX`, and a
    supervisor may hand out ids from a counter instead. An id
    must name one holder for the life of the ring, since the
    release CAS is what keeps a stale holder from releasing its
    successor's role. Pids are reused, so an app that restarts
    holders packs a generation into the high bits, 22 bits of
    pid and a 10-bit restart count. Whether a holder is alive
    is the app's judgment, never the crate's.
  - The checkpoint sits beside the role words: the producer's
    `cur`, `pos`, and `taken` and the consumer's `cur` and
    `pos`, with each segment's two resume positions in its info
    line. The consumer's `given` is already shared, so it is its
    own checkpoint. `claimable` is not kept: a successor that
    assumes false loads one seq word it could have skipped.
  - Each switch writes its side's checkpoint, all but `pos`,
    and `release` writes `pos`, so a released role's checkpoint
    is exact and a held one's is exact up to the position,
    which the seq words of one segment hold. Nothing on the
    message path writes it.
  - A switch is several stores, and its holder can die between
    any two, where no lock or CAS makes them one. So each side
    keeps an intent word, the flag a robust mutex would leave: a
    switch sets it, naming the segment left, the segment
    entered, and the free-set bit it flips, before its first
    checkpoint store, and clears it after its last shared store,
    the MOVED commit or the consumer's give-back. Clear means
    the checkpoint is whole. Set means the holder died inside
    the switch, and a takeover finishes or undoes it by the one
    slot the switch left: a producer's still claimable means
    the MOVED commit never happened, a consumer's still
    committed that the release never did.
  - The cost is on the switch path only, four stores and the
    clear for the producer, three and the clear for the
    consumer, all Release so a reader that sees one sees the
    intent. At depth 1, where every commit switches, that is
    per message.
  - `split` stays out. The shape the crate serves is
    multi-process: each app creates the ring it reads and joins
    the other's as producer, so no process holds both roles of
    one ring, and in-process callers claim twice.
  - How to, in brief:

    ```rust
    // In-process, as the tools and the demo do: both claims
    // on a fresh ring, so neither can fail.
    let ring = spsc::v4::Ring::init(&mut pool, 64, 8, 4)?;
    let mut prod = ring.claim_producer(1)?;
    let mut cons = ring.claim_consumer(2)?;

    // Across processes: the reader inits the ring, claims its
    // consumer, and hands `ring.first_segment()` to the
    // producer's process, which attaches its own pool handle
    // over the same region and claims.
    // SAFETY: first_segment came from the reader's ring over
    // this pool's region, whose segments are still the ring's.
    let ring = unsafe { spsc::v4::Ring::attach(&pool, first_segment) }?;
    let mut prod = ring.claim_producer(std::process::id())?;

    // Giving the role back: release, since dropping keeps it.
    prod.release();
    ```

- **Resume and takeover**: a claim of a free role starts in
  segment 0 at position 0, and of a released role loads the
  checkpoint `release` wrote, exact, and continues.
  `take_over_producer(id)` and `take_over_consumer(id)` replace
  any holder by one CAS from the word as loaded, one attempt, so
  of two racing takeovers one wins. The caller vouches the
  holder is dead, [Holders and recovery](#holders-and-recovery)
  says why that is the app's call.
  - A set intent is finished or undone by the slot the switch
    left, as the checkpoint bullet says, and the repaired
    checkpoint is written back with the intent cleared.
  - A clear intent leaves the segment and free-set exact, and
    the position is the scan's. Slot `i` last held a position
    `q = i` modulo the depth, and its seq says which and whether
    committed: `q + M` released, `q + M + 1` committed. The `M`
    positions end just before the producer's, and the committed
    ones are the newest, from the consumer's on. The scan knows
    positions modulo `2^SEQ_BITS`, which is all the protocol
    compares, and lifts them after the segment's resume position.
  - A live producer committing while a consumer's replacement
    scans can make one read disagree with the rest, so the scan
    runs again until the words form one window, and gives up as
    `BadCheckpoint` after 1024, which only words the ring never
    wrote cause.
  - What a takeover loses: a consumer nothing, the message its
    holder read and never released still committed, a producer
    at most the one slot reserved and never committed, written
    again.
  - At depth 1 a slot's released and committed values coincide,
    `q + 1` against `(q - 1) + 2`, so the seq words cannot place
    a position, and a takeover of a held role is
    `Err(BadCapacity)`. A released role there resumes, its
    position exact. Placing it would take a per-message store
    at depth 1, where every commit is already on the switch
    path, and is not done.
  - v2's `attach` still joins at position 0 only.
- **Stacked Borrows shaped the tests**: the handle `init`
  returns holds pointers under the `&mut` it took, and the first
  write through an attached handle, which holds the region's
  raw pointer, invalidates them, the hazard the pools' `attach`
  notes. So the attach test drops the initializing handle once
  it has the first segment's index and attaches twice, one
  handle per role, as two processes would. Real processes share
  no borrow stack.
- **Prediction, on record before measuring**: v4 within
  run-to-run noise of v3 where no switch happens, the code delta
  being 16 bytes in the copied table, one add per slot access,
  and three more header lines ahead of the slots.
- **Measured (2026-09-25, 3900X, the rung `perf: spsc v4 in the
  measurement tools`, `tp-stream` and `tp-matrix` at `-d 1
  --depth 1,8,64,1024`, two segments, each run twice, the demo
  once)**. Stream ns per message, v3 / v4, run 1 then run 2:

  | placement | d=1 | d=8 | d=64 | d=1024 |
  |---|---|---|---|---|
  | 11,10 CCX | 74.0 / 75.5, 67.5 / 68.3 | 15.9 / 18.1, 14.3 / 16.4 | 14.3 / 15.1, 13.0 / 13.6 | 13.7 / 15.4, 12.3 / 13.9 |
  | 11,8 x-CCX | 228.7 / 231.5, 227.2 / 231.1 | 49.0 / 48.8, 48.8 / 48.2 | 23.7 / 22.1, 24.2 / 23.2 | 16.1 / 19.3, 16.1 / 16.8 |
  | 11,23 SMT | 31.3 / 33.4, 31.3 / 33.2 | 17.2 / 20.0, 17.1 / 19.9 | 17.2 / 20.0, 17.1 / 19.9 | 17.2 / 20.0, 17.1 / 19.9 |
  | unpinned | 62.0 / 64.4, 60.9 / 64.0 | 15.0 / 16.0, 14.8 / 15.4 | 12.7 / 12.9, 12.3 / 13.5 | 11.7 / 12.9, 11.9 / 13.0 |

  The round trip, main's send and the worker's receive in ns,
  v3 / v4, run 1 then run 2, and the cross-core fills per round
  trip from run 1:

  | placement | depth | m.send | w.recv | xfills/RT |
  |---|---|---|---|---|
  | 11,10 CCX | 1 | 16.7 / 15.9, 17.0 / 15.7 | 132.0 / 101.3, 132.0 / 101.2 | 9.001 / 8.004 |
  | 11,10 CCX | 8 | 12.9 / 13.1, 12.5 / 13.3 | 110.7 / 107.6, 109.3 / 105.1 | 3.191 / 3.214 |
  | 11,10 CCX | 64 | 13.1 / 13.1, 12.7 / 13.3 | 90.1 / 99.2, 89.0 / 98.9 | 2.168 / 2.150 |
  | 11,10 CCX | 1024 | 13.1 / 13.1, 12.7 / 13.3 | 96.8 / 98.0, 96.9 / 98.2 | 2.006 / 2.005 |
  | 11,8 x-CCX | 1 | 21.9 / 29.5, 65.2 / 31.9 | 481.7 / 351.3, 472.8 / 355.0 | 8.969 / 8.035 |
  | 11,8 x-CCX | 64 | 13.1 / 13.4, 12.3 / 13.3 | 276.0 / 266.0, 282.2 / 264.7 | 2.134 / 2.217 |
  | 11,8 x-CCX | 1024 | 13.2 / 13.8, 12.4 / 13.4 | 273.6 / 264.0, 276.2 / 260.8 | 2.021 / 2.022 |
  | 11,23 SMT | 1 | 24.8 / 25.5, 24.8 / 25.1 | 114.3 / 116.7, 114.1 / 114.0 | 0.0004 / 0.0004 |
  | 11,23 SMT | 64 | 17.6 / 18.4, 17.6 / 18.5 | 113.0 / 116.8, 112.6 / 116.5 | 0.0004 / 0.0004 |
  | 11,23 SMT | 1024 | 18.0 / 18.5, 18.0 / 18.5 | 111.6 / 115.3, 111.6 / 115.4 | 0.0005 / 0.0006 |

  The demo, one segment at depth 64, ns per message: the
  single-thread loop v3 20.5 and v4 19.5, and the two-thread loop
  v3 / v4 at 21.6 / 23.5 on the CCX, 28.7 / 32.3 across it, 20.0
  / 21.4 on the SMT pair, and 23.8 / 26.6 unpinned. The depth
  sweep's single-thread rows read v4 under v3 at every depth.
- **Readings**: the prediction failed, and the failure is not
  the add.
  - Streaming with no switch, v4 runs 0.6 to 2.8 ns per message
    slower than v3 on every pinned placement in both runs, 5 to
    16 percent, the most on the SMT pair (17.1 against 19.9 at
    every depth, the runs agreeing to 0.1) and at depth 1024
    across the CCX. The round trip's sends read 0.2 to 0.9 ns
    slower and its receives 4 ns slower on the SMT pair.
  - The single-thread loop, the instruction path alone, reads v4
    a nanosecond under v3, so the added offset add is not the
    cost, and the fills per message read the same or fewer for
    v4. What two threads pay that one does not is not found.
  - v4 wins where the ring switches on every message: the round
    trip at depth 1 moves 8 lines against v3's 9, and its receive
    runs a fifth to a quarter faster, 101 against 132 ns on the
    CCX and 351 against 477 across it. We think the claims line
    is a spacer: v3's `given` word shares its 128-byte pair with
    slot 0, which the adjacent-line prefetcher drags along on
    every give-back, and v4's shares it with the untouched claims
    line.
- **Verdict (2026-09-25)**: v4 does what it is for, a ring a
  second process joins, at a cost of 5 to 16 percent on the
  no-switch stream that the code delta does not explain. Both
  rings still copy their table per message, the copy that is
  more than half of v3's gap to v2, so the `### SPSC v3 fast
  path` Todo measures v4 beside v3 when it runs and looks for
  the two-thread gap there, with these rows as the mark. The
  default `Ring` stays v3.
- **The gap was the copy (2026-09-26)**: the checkpoint rung of
  `fix: spsc v4 roles survive their holders` passes the table
  by reference to an out-of-line call on the switch path, which
  made the copy certain on every message and v4 regress, 14.6
  to 19.1 ns at depth 64 on the CCX pair. Borrowing `&st.segs`
  in `commit` and `release`, as MPSC v2 already does, took v4
  under v3 in the same `tp-stream` run: at depths 8, 64, and
  1024, 12.8, 10.0, and 10.3 ns on the CCX pair against v3's
  15.5, 13.7, and 12.3, and 12.7, 12.5, and 11.9 on the SMT
  pair against 17.1 at each. v3 keeps its copy as built, the
  `### SPSC v3 fast path` Todo's to change.

## MPSC v1: equality-seq ring

The second MPSC protocol, `mpsc::v1`, a sibling of v0 under
the same module layout, so the two measure side by side. v0
is Vyukov's queue as [MPSC protocol](#mpsc-protocol) states
it, and its committed value `pos + 1` equals its released
value `pos + M` at `M = 1`, so a capacity-1 ring wedges both
sides after the first release, found by the demo's depth
sweep on 2026-09-07. v0 now rejects capacity 1 rather than
hang, and v1 takes the seq values spsc v1 chose for the same
reason, with nothing else changed.

- **Region**: v0's, the four-line header, the seq array, then
  the slots, with its own magic `ZCM2` and its own layout
  version, since a v0 region carries v0's seq values and a
  cross-version attach must fail toward `BadMagic`.
- **Seq values**: claimable at `seq == pos`, committed at
  `pos + M + 1`, released at `pos + M`. At `M = 1` the one
  word cycles through 0, 2, 1, and the released 1 is the next
  lap's claimable. `M` is any power of two from 1 to `2^30`,
  with `M` usable slots: the state is in the word, not in an
  index distance, so no sacrificial slot.
- **Equality, not a signed diff**: with committed at
  `pos + M + 1` a full slot's previous-lap value is `pos + 1`,
  which v0's diff reads as a lost race. So a producer compares
  the seq with `pos` for claimable, and anything else is stale
  or full, told apart by re-reading `producer_idx`: moved
  means another producer claimed `pos`, so reload and retry
  with no policy call, and unmoved means the previous occupant
  is not yet released, so the `on_full` policy runs.
  - A consequence: a tombstoned previous lap reaches the
    policy as Full, where v0's diff read it as a lost race and
    spun with no policy call until the consumer skipped it.
- **Tombstone**: committed plus `2^31`, as v0, and the `2^30`
  cap keeps the three values a side can see distinct.
- **Hot path**: the same loads, stores, and lines as v0. Each
  endpoint carries `capacity + 1` precomputed, so the commit
  value and the consumer's check stay one add. The prediction
  on record: within run noise v1 is v0 at every depth from 2
  up, and at depth 1 it runs lockstep at about the round-trip
  cost.
- **Measured (2026-09-10, 3900X, the rung `perf: measure mpsc
  v1 beside v0`, at the flavor rung's build: `tp-matrix` and
  `tp-stream` 5 s cells and the demo's 1M message streams, all
  at depths 1, 2, 8, and 64)**. The prediction holds: from
  depth 2 up the two rings are within run noise at every
  placement in every instrument, and depth 1 is a number.
  - Round trips per 5 s and fills per trip, the MPSC rows:

    | placement | flavor  | d=1          | d=2          | d=8          | d=64         |
    |-----------|---------|-------------:|-------------:|-------------:|-------------:|
    | 0,1 CCX   | mpsc-v0 | -            | 28.6M (8.34) | 31.3M (6.59) | 32.1M (6.00) |
    | 0,1 CCX   | mpsc-v1 | 32.3M (8.16) | 29.3M (8.30) | 31.3M (6.58) | 32.1M (6.04) |
    | 0,3 x-CCX | mpsc-v0 | -            |  8.2M (8.24) |  9.4M (6.59) |  9.2M (6.01) |
    | 0,3 x-CCX | mpsc-v1 |  9.7M (8.11) |  8.1M (8.27) |  8.8M (6.62) |  9.7M (5.96) |
    | 0,12 SMT  | mpsc-v0 | -            | 35.3M        | 35.5M        | 35.6M        |
    | 0,12 SMT  | mpsc-v1 | 36.1M        | 35.7M        | 36.0M        | 36.0M        |

    The send costs match to the tenth of a nanosecond, 8.3 to
    8.9 ns for both at depth 8 and 64 at every placement.
  - Streaming, `tp-stream`, ns per message and fills per
    message:

    | placement | flavor  | d=1          | d=2          | d=8          | d=64         |
    |-----------|---------|-------------:|-------------:|-------------:|-------------:|
    | 0,1 CCX   | mpsc-v0 | -            |  56.3 (3.71) |  28.5 (1.79) |  22.2 (0.88) |
    | 0,1 CCX   | mpsc-v1 |  74.2 (4.20) |  54.2 (3.65) |  29.3 (1.87) |  21.8 (0.86) |
    | 0,3 x-CCX | mpsc-v0 | -            | 213.8 (3.95) | 106.5 (2.17) |  80.6 (1.35) |
    | 0,3 x-CCX | mpsc-v1 | 465.2 (5.25) | 218.1 (3.99) | 104.0 (2.12) |  81.3 (1.40) |
    | 0,12 SMT  | mpsc-v0 | -            |  23.3        |  15.2        |  15.0        |
    | 0,12 SMT  | mpsc-v1 |  39.8        |  22.2        |  15.2        |  15.0        |

  - Streaming, the demo's sweep, ns per message:

    | placement          | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |--------------------|---------|------:|------:|------:|------:|
    | 1t core 0          | mpsc-v0 |     - |  10.2 |  10.1 |  10.1 |
    | 1t core 0          | mpsc-v1 |  10.4 |  10.2 |  10.1 |  10.1 |
    | 2t diff cores 0+3  | mpsc-v0 |     - | 201.5 | 108.2 |  89.3 |
    | 2t diff cores 0+3  | mpsc-v1 | 461.0 | 196.4 | 100.6 |  79.2 |
    | 2t same core 0+12  | mpsc-v0 |     - |  23.6 |  15.8 |  15.1 |
    | 2t same core 0+12  | mpsc-v1 |  41.9 |  22.3 |  15.1 |  15.0 |

  - Depth 1, the cell v0 could not run: the round trip moves
    eight lines, the seq line beside each slot line each way,
    as spsc v1 does at that depth, and completes more trips
    than at depth 2 at every placement, again as spsc v1 does.
    The stream is lockstep, one message in flight and the
    release travelling back before the next send: 74 ns within
    the CCX and 465 across it against round trips of 155 and
    515, so between half a trip and a whole one, on 4.2 and
    5.2 lines per message.
  - The first `tp-stream` run put v0 at the SMT pair at 47.3
    and 29.5 ns at depth 2 and 8, twice its recorded figures,
    with v1 at 22.5 and 15.3 on them. A rerun read v0 at 23.3
    and 15.2, so the table above is the rerun. We think the
    first run caught the same regime flip the v2 cycle found
    in spsc v1's streams, this time on v0, since nothing in
    the run changed but the cell's luck.
- **Default (2026-09-10, at the measurement rung)**: the
  crate's `MpscRing` re-export is v1. v1 is v0 plus a
  capability at the same cost, so the numbers holding is the
  whole case. v0 stays reachable by path and keeps its guard.
- **Measured (2026-09-10, 7600X, Zen 4, one CCD, at Land's
  plain-name build, the same three instruments at the same
  depths, the demo's full run in the README's example run
  beside the 3900X's)**. The same picture: from depth 2 up
  the two rings are within run noise at every placement in
  every instrument, the sends 7.0 to 7.3 ns for both, and
  depth 1 runs. No fills columns, since the tools' fill
  events are Zen 2 encodings.
  - Round trips per 5 s, the MPSC rows:

    | placement | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |-----------|---------|------:|------:|------:|------:|
    | 0,1 CCX   | mpsc-v0 |     - | 43.8M | 44.7M | 44.4M |
    | 0,1 CCX   | mpsc-v1 | 43.8M | 44.2M | 45.7M | 44.4M |
    | 0,6 SMT   | mpsc-v0 |     - | 49.0M | 49.1M | 49.3M |
    | 0,6 SMT   | mpsc-v1 | 48.7M | 48.7M | 48.7M | 48.6M |
    | unpinned  | mpsc-v0 |     - | 51.3M | 48.9M | 42.4M |
    | unpinned  | mpsc-v1 | 44.4M | 45.4M | 44.6M | 42.8M |

  - Streaming, `tp-stream`, ns per message:

    | placement | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |-----------|---------|------:|------:|------:|------:|
    | 0,1 CCX   | mpsc-v0 |     - |  37.3 |  16.7 |  16.6 |
    | 0,1 CCX   | mpsc-v1 |  81.9 |  37.2 |  16.6 |  15.1 |
    | 0,6 SMT   | mpsc-v0 |     - |  22.3 |  12.6 |  12.1 |
    | 0,6 SMT   | mpsc-v1 |  39.7 |  22.2 |  12.2 |  11.6 |
    | unpinned  | mpsc-v0 |     - |  38.6 |  19.7 |  16.7 |
    | unpinned  | mpsc-v1 |  84.4 |  37.4 |  20.7 |  16.7 |

  - Streaming, the demo's sweep, ns per message:

    | placement          | flavor  |   d=1 |   d=2 |   d=8 |  d=64 |
    |--------------------|---------|------:|------:|------:|------:|
    | 1t core 0          | mpsc-v0 |     - |   7.2 |   7.4 |   7.3 |
    | 1t core 0          | mpsc-v1 |   7.3 |   7.2 |   7.0 |   7.2 |
    | 2t diff cores 0+1  | mpsc-v0 |     - |  38.0 |  16.9 |  16.0 |
    | 2t diff cores 0+1  | mpsc-v1 |  79.6 |  38.8 |  17.1 |  16.6 |
    | 2t same core 0+6   | mpsc-v0 |     - |  22.6 |  12.8 |  12.6 |
    | 2t same core 0+6   | mpsc-v1 |  40.1 |  22.3 |  12.5 |  11.8 |

  - Depth 1 completes the same trips as depth 2 here, where
    the 3900X completed more, and the stream is lockstep as
    there: 82 ns under the one L3 against a 114 ns round
    trip, between half a trip and a whole one.

## MPSC v2: ring of segments

The third MPSC protocol, `mpsc::v2`, a sibling of v0 and v1
under the same module layout, so the three measure side by
side, and v0 and v1 are unchanged by it. A v1 ring is one
fixed region, so producers that outrun the consumer find it
Full. v2 is a ring of up to 32 segments over the
application's pool, as [SPSC v3](#spsc-v3-ring-of-segments)
is, and when the consumer keeps up only one is in use. The
three problems v3's "carried to MPSC" list names, a CAS to
take a segment, a seal before the switch, and reclamation
under a slow producer, resolve into one word. It is a design
built to be measured, and the crate's `MpscRing` stays v1
until v2 matches it where no switch happens.

- **Segments**: `MpscRing::init(pool, slot_size,
  seg_capacity, seg_count)` takes every segment from the pool
  with `Pool::alloc_bytes` and initializes each: a header of
  three lines, then `seg_capacity` slots opening with v1's seq
  word. `segment_size` is what the pool's buffers must hold.
  Nothing allocates, frees, or re-initializes while the ring
  runs, the pool is borrowed only during `init`, and there is
  no `attach`, as v3 has none. The header lines: the segment's
  seal, and in segment 0 only, the claim word, and the in-use
  word beside the switch count. The claim word is the
  contended line, so it is alone on its line. There is no user
  line, as in v3.
- **Words**: 32 bits everywhere, so v2 builds wherever v1
  does. A position is 26 bits, as v3's seq values are, and the
  slot's seq word holds v1's values in those bits, claimable
  at `pos`, committed at `pos + M + 1`, released at `pos + M`,
  with v1's tombstone at bit 31. The width caps a segment at
  `2^24` slots and a ring at 32 segments. The claim word packs
  the current segment in its high five bits over the next
  position, and the seal packs a MOVED flag, the next segment,
  and the end position the same way.
- **The claim word is the seal**: every producer claims as v1
  claims, loading the claim word, checking the slot it names
  is claimable, and CASing the word one position on. A stale
  view fails the CAS, so a claim can never land in a segment
  the ring has left, and sealing costs the send path nothing.
  The commit is v1's: fill in place, store the committed seq,
  or the tombstone on unwind.
- **Switching**: a producer whose claim finds the slot
  unreleased and the claim word unmoved is at a full segment,
  where v1 runs its policy. It reads the in-use word, one bit
  per segment. With none clear the policy runs, and Full means
  the ring waits in place as one ring does. With one, it takes
  the lowest by a `fetch_or` of its bit, which serializes
  producers switching at once and succeeds only where the bit
  was clear, clears the segment's seal, and CASes the claim
  word from its view to the new segment at its resume
  position. On success it stores the seal into the old
  segment's header: MOVED, the new segment, and the end
  position, the position its view held. On failure another
  producer moved the ring first, so it restores the seal,
  clears its bit, and retries with the fresh claim word, no
  policy call, as a lost claim race is none. A switch is two
  read-modify-writes and a store on a path taken only at a
  full segment.
- **The consumer**: single, v1's loop within a segment, the
  tombstone skip included, with no read-modify-write on its
  fast path. When the slot at its
  position is neither committed nor tombstoned it takes a
  second look, at the segment's seal: MOVED with the end
  position equal to its own means the segment is done, since
  every claim in it lies before the end position and each has
  committed or tombstoned. It clears the segment's bit in the
  in-use word, its one read-modify-write, and continues in the
  named segment where it left it. Otherwise the slot is not
  yet committed, and the policy runs. The second look is on
  the empty path only, so the fast path is v1's one load, and
  a segment is given back at the reserve after its last
  release rather than at that release, one poll later than v3
  gives its back. v3 retired its own second look for a reason
  that does not reach here: a flag in a slot word set outside
  the commit raced the release store, and the seal is a header
  word the consumer never stores to.
- **Reclamation**: a slow producer holds a claimed slot, never
  a segment. The consumer gives a segment back only after
  passing its end position, and by then every claim in it has
  committed, so nothing is reclaimed under a producer. The
  free set is one in-use word, not v3's taken XOR given: that
  parity is sound for one producer and not for several, found
  by the two-producer stress on 2026-09-15. A producer that
  slept with a stale view woke to the taken word reading the
  same bits again and took a segment that was in fact free at
  that instant, and a second producer then read the fresh
  taken word beside a give-back word one consumer store
  stale, the two parities agreed, and it took the segment the
  first held with the ring inside it. One bit set by a
  `fetch_or` and cleared by a `fetch_and` has no stale view to
  agree with.
- **Reuse**: both sides leave a segment at its end position,
  the consumer in a private resume array as v3's, and the next
  taker reads it from the seal it clears. A consumed segment
  holds released seqs, so at the end position the seqs are
  already claimable.
- **API**: v1's, less `attach` and the user words:
  `split` into a `Clone` `MpscProducer` with `send_with`, the
  closure send with tombstone on unwind, and an `MpscConsumer`
  with `reserve_slot_with` and its guard. Both carry
  `switches` and `segment` as v3's endpoints do, a producer's
  being the ring's, since a switch is the ring's move rather
  than any one handle's. `examples/mpsc_v2_segments.rs` runs
  every segment count from 1 to 32 at depths 1, 8, 64, and
  1024, filled and then streamed from one, two, and four
  producers.
- **Trust**: as v1's, with the header words joining the
  untrusted set. A seal naming a segment the ring does not
  have reads as Empty, a scribbled claim word masks to a valid
  segment and position, and a claim that never commits wedges
  the segment as it wedges a v1 ring.
- **Weighed and not taken**: a producer index per segment
  with a current-segment word beside it, and v3's two parity
  words for the free set, above. A stale current
  segment would let a claim land in a sealed segment, so every
  segment's index word would need a seal bit and every send a
  check of it. The packed word makes the seal implicit.
- **Prediction, on record before measuring**: with the
  consumer keeping up, v2's send is v1's, the same load, CAS,
  seq load, and commit store, the segment number riding in the
  claim word for free, and its receive is v1's one load. So
  from depth 2 up we think v2 is within run noise of v1 at
  every placement in every instrument, and at depth 1, where
  a consumer one message behind makes every send switch,
  within twice v1's cost. A switch costs the producer two
  read-modify-writes and the seal store, and the consumer one
  extra load, the give-back read-modify-write, and a cold
  segment.
- **Measured (2026-09-15, 3900X, the rung `feat: mpsc v2 in
  the measurement tools`, `tp-matrix` and `tp-stream` 1 s
  cells at depths 1, 8, 64, and 1024, two segments, one
  producer, each sweep run twice, and the demo's sweep)**. The
  runs agreed within 10% but for the marked (`*`) cells, whose
  means moved more than 15% between runs. The 7600X follows,
  under its own heading.
  - Streaming, the producer running ahead: ns per message v1 /
    v2, and v2's switches per message:

    | 3900X     | d=1                   | d=8                  | d=64                 | d=1024               |
    |-----------|----------------------:|---------------------:|---------------------:|---------------------:|
    | 0,1 CCX   | 80.1* / 97.0 (0.993)  |  26.8 / 10.1 (0.000) |  21.8 / 14.5 (0.000) |  19.9 / 11.7 (0.000) |
    | 0,3 x-CCX | 457.4 / 357.4 (0.682) | 102.5 / 31.3 (0.001) |  81.0 / 14.0 (0.000) |  90.3 / 17.3 (0.000) |
    | 0,12 SMT  |  41.5 / 32.9 (0.731)  |  15.3 / 11.7 (0.000) |  15.1 / 11.8 (0.000) |  15.0 / 11.6 (0.000) |
    | unpinned  |  91.7 / 98.6 (0.986)  |  27.1 / 10.3 (0.000) |  22.2 / 14.3 (0.000) |  20.4 / 11.4 (0.000) |

  - The round trip, one message in flight so the consumer keeps
    up: main's send and the worker's receive, means in ns, v1 /
    v2, lines pulled per round trip, and v2's switches:

    | placement | depth | m.send v1 / v2 | w.recv v1 / v2 | xfills/RT v1 / v2 | switches/RT |
    |-----------|------:|---------------:|---------------:|------------------:|------------:|
    | 0,1 CCX   |     1 |      9.0 / 9.0 |    82.5 / 89.5 |     8.136 / 4.016 |       0.000 |
    | 0,1 CCX   |     8 |      8.7 / 9.1 |   89.7 / 106.8 |     6.590 / 3.070 |       0.000 |
    | 0,1 CCX   |    64 |     8.7 / 10.1 |   84.7 / 112.0 |     5.970 / 2.787 |       0.000 |
    | 0,1 CCX   |  1024 |     8.7 / 10.0 |   95.3 / 105.2 |     5.934 / 2.028 |       0.000 |
    | 0,3 x-CCX |     1 |      8.6 / 9.3 |  253.1 / 201.5 |     8.144 / 4.005 |       0.000 |
    | 0,3 x-CCX |    64 |      8.6 / 9.1 |  308.0 / 328.2 |     5.989 / 3.214 |       0.000 |
    | 0,12 SMT  |     1 |     8.8 / 12.1 |    79.2 / 85.2 |   0.0004 / 0.0006 |       0.000 |
    | 0,12 SMT  |    64 |     8.8 / 12.0 |    79.3 / 85.0 |   0.0006 / 0.0007 |       0.000 |

  - The demo's one-thread loop, both ends on core 0, ns per
    message at every depth: v1 10.1, v2 13.0.
- **Measured (2026-09-15, 7600X, Zen 4, one CCD, the rung
  `perf: mpsc v2 on the 7600X and a native build`, the same
  instruments and depths, binaries built on the 3900X as
  before, each sweep run twice, the runs agreeing within
  10%)**. The 7600X has no cross-CCX placement, its six cores
  sharing one L3.
  - Streaming: ns per message v1 / v2, and v2's switches per
    message:

    | 7600X     | d=1                  | d=8                 | d=64                | d=1024              |
    |-----------|---------------------:|--------------------:|--------------------:|--------------------:|
    | 0,1 CCX   | 79.3 / 73.3 (0.740)  | 17.7 / 6.2 (0.000)  | 16.6 / 5.9 (0.000)  | 13.0 / 4.7 (0.001)  |
    | 0,6 SMT   | 40.1 / 31.7 (0.685)  | 12.2 / 7.4 (0.000)  | 11.6 / 7.4 (0.000)  | 11.5 / 7.4 (0.000)  |
    | unpinned  | 81.4 / 75.3 (0.752)  | 19.6 / 6.3 (0.000)  | 16.6 / 6.2 (0.000)  | 13.3 / 4.8 (0.001)  |

  - The round trip, means in ns, v1 / v2, and v2's switches:

    | placement | depth | m.send v1 / v2 | w.recv v1 / v2 | switches/RT |
    |-----------|------:|---------------:|---------------:|------------:|
    | 0,1 CCX   |     1 |      7.2 / 6.8 |    77.1 / 70.7 |       0.000 |
    | 0,1 CCX   |     8 |      7.2 / 6.9 |    70.2 / 80.8 |       0.000 |
    | 0,1 CCX   |    64 |      7.1 / 7.0 |    78.8 / 99.0 |       0.000 |
    | 0,1 CCX   |  1024 |      7.2 / 6.9 |    78.3 / 72.0 |       0.000 |
    | 0,6 SMT   |     1 |      7.0 / 7.5 |    65.2 / 70.5 |       0.000 |
    | 0,6 SMT   |    64 |      7.1 / 7.7 |    64.8 / 68.2 |       0.000 |

    The lines per round trip are the 3900X's, 4.0 against 8.0
    at depth 1.
  - The demo's one-thread loop, ns per message: v1 6.7, v2
    8.2.
  - **Three builds of the same source**, a question never
    asked before: the binaries built on the 3900X, a build on
    the 7600X with the default target, and one with
    `-C target-cpu=native`, the last two by its rustc 1.98.1
    against the 3900X's 1.98.0, each swept the same way. Every
    MPSC cell agreed across the three within run noise, so the
    tables above hold for all three. The one line that moved
    was spsc-v3's one-thread loop, 15.4 ns on the first two
    builds and 10.6 to 12.3 on the native one, a fifth off,
    while spsc-v2 and both MPSC rings stayed put: native
    codegen helps a fast path that is still doing too much,
    which is the SPSC v3 fast-path entry's finding again.
- **Readings**:
  - The design does what it says. With the consumer keeping
    up, the round trip switches 0.000 times at every depth,
    depth 1 included, since a released slot is claimable and
    v2 switches only at a full segment where v3 switches at
    its look-ahead. Streaming at depth 8 and up, the producer
    running ahead, it switches at most 0.001 times per
    message, and at depth 1 on 0.68 to 0.99 of them.
  - The first sweep had v2's send at half again v1's and its
    one-thread loop at twice, and the cause was calls across
    codegen units: the segment table's accessors, the seq
    helper, and the word packers are small non-generic
    functions in the parent module called from the producer
    and consumer modules, real calls without `#[inline]`.
    With the hints the tables above are what v2 costs, and
    the one-thread loop's remaining 3 ns over v1 is not yet
    found.
  - The prediction holds for the send and fails for the
    receive, and the stream beats it. Main's send matches
    v1's within a nanosecond everywhere but the SMT pair,
    where it costs 3 ns more. The worker's receive runs 5 to
    30% slower from depth 8 up, 106 to 112 against 85 to 90
    ns across the CCX, and at depth 1 runs faster than v1's.
    We think the seq word in the slot line is the reason for
    both: the consumer spins on the line the producer then
    writes twice, the body and the seq, where v1's producer
    fills the slot line unwatched and stores the seq beside
    it, and at depth 1 the one line is hot on both sides.
  - Half the lines. v2 pulls 4.0 lines per round trip at
    depth 1 against v1's 8.1, and 2.0 against 5.9 at depth
    1024, the seq array's lines gone as spsc v2 lost them
    against spsc v1.
  - Streaming, v2 is the faster ring from depth 8 up at every
    placement: 10 to 15 ns per message against 20 to 27
    across the CCX, 14 to 31 against 81 to 103 across the
    CCXs, and 12 against 15 on the SMT pair, with the fill
    counts under 1 per message against v1's 0.65 to 2.1. At
    depth 1, where nearly every send switches, it costs 20%
    more than v1 across the CCX and less than v1 elsewhere,
    inside the prediction's twice.
  - The 7600X reads the same, closer. v2 streams two and a
    half to three times faster than v1 from depth 8 up and
    faster at depth 1 too, its send matches or beats v1's
    everywhere but the SMT pair, where it costs half a
    nanosecond more, and its receive is slower only around
    depth 64, 99 against 79 ns, faster at depth 1 and 1024.
  - The cost of one switch, the demo's segment stress table on
    the 3900X, measured as a difference at equal capacity, 32
    segments of one slot against one segment of 32, two runs:
    single-threaded on core 0, 6.5 to 7.0 ns for spsc-v3 and
    12.8 to 14.4 for mpsc-v2, the two read-modify-writes and
    the seal, and streaming across cores at 0+3, 137 to 144 and
    267 to 285 ns. On the 7600X, 3.4 and 6.5 single-threaded, and
    15.7 and 57.5 streaming across cores at 0+1. The two
    machines agree once the placement's transfer cost is taken
    out: 0+3 on the 3900X is cross-CCX at over 100 ns a line,
    0+1 on the 7600X shares one L3 at 15 to 20, and a switch
    costs spsc-v3 one extra transfer, its give-back word, and
    mpsc-v2 three to four, the seal and the in-use word twice,
    since both sides read-modify-write it. The new segment's
    slot line cancels out of the difference, the one-segment
    shape walking 32 lines too. At depth 1 across cores the
    switch is most of the message, and from depth 8 up it is
    paid once in hundreds of messages or never. Making it
    cheaper is the Todo entry `Cheaper segment switches`.
- **Verdict (2026-09-15, 3900X and 7600X)**: the segment design
  works, its switch is cheap, and v2 streams two to five times
  faster than v1 from depth 8 up while pulling half the lines,
  on both machines and under three builds. It does not match
  v1 in the round trip: the send matches, the receive runs up
  to 30% slower from depth 8 up on the 3900X and around depth
  64 on the 7600X, and the SMT send costs more. The Todo entry
  `MPSC v2 as the default` waits on the round trip.
- **Real-time use, the user's reading on 2026-09-15**: a
  segmented ring cannot serve where every send must cost the
  same, an ISR or any hard-real-time path, since the message
  that switches costs 15 to 300 ns more than its neighbours by
  the placement. The narrower claim the numbers support: a
  switch is a bounded number of line transfers, one for
  spsc-v3 and three to four for mpsc-v2, the order of one cache
  miss to another core, paid at depth 8 and up once in hundreds
  of messages or never, where a single ring that is Full has no
  move at all. spsc-v3's switch is loads and stores only, so
  for an ISR it is the ring, sized so the consumer keeps up,
  with the switch as the bounded fallback. mpsc-v2's switch is a
  CAS loop, lock-free rather than wait-free, and stays out of an
  ISR until `Cheaper segment switches` brings it nearer v3's.

### Segment lifecycle

What happens to the segments over a ring's life, for SPSC v3 and MPSC v2 alike, stated once
rather than assembled from the protocol bullets above. Written 2026-09-16 for the user guide
([user-guide.md](user-guide.md)).

- **Every segment is taken at `init`** from the application's pool, `seg_count` of them up to
  32, and held for the life of the pool region. Nothing is allocated or freed while the ring
  runs, and a segment given back goes to the ring's own free set, never to the pool. The
  memory cost of a ring of segments is therefore fixed at `init`: `seg_count` buffers of
  `segment_size`, most of them parked while the consumer keeps up.
- **The ring lives in one segment at a time.** Both endpoints start in segment 0. With a consumer
  that keeps up, no switch ever happens and the other segments are never touched.
- **A switch happens only at a full segment.** The producer moves on when the next slot of the
  current segment is still unread by the consumer, and only then, so no segment is ever left
  part-filled: every slot of a segment the ring has left carried a message. The producer takes
  the lowest free segment, and the free set is a bitmask, not a queue, so the give-back order
  never matters and the same low segments are reused first.
- **Full means no free segment.** When the current segment is full and every other segment is in
  use, the ring waits in place as a single-region ring does, under the caller's policy, and
  reports `Full` when the policy gives up. Nothing is dropped and nothing is lost.
- **A segment is given back after the consumer passes its end.** The consumer drains a segment to
  the point where the producer left it, clears the segment's bit, and continues in the segment
  the producer named. Only then may a producer take it again, so nothing is reclaimed under a
  producer, and a slow producer holds a claimed slot, never a segment.
- **The segment the consumer ends in stays in use.** After a full drain the ring holds one segment
  in use, the one where the producer's position is, and every other segment is free: a drained
  ring is a fresh ring in another segment.
- **The counters** count switches on the switch path only, so a run that never switched reads
  `0`, and once the consumer has read everything sent the two sides' counts agree. `segment`
  names the segment each side is in.

The two rings differ in three mechanics, none of which changes the lifecycle above:

| | SPSC v3 | MPSC v2 |
|---|---|---|
| Where the switch is decided | at the producer's commit, when the next slot is not claimable, and the message commits with MOVED in its slot word | at a producer's claim, when the slot is unreleased and the claim word unmoved, and the seal is a header word |
| When the segment is given back | at the release of the MOVED message, one poll earlier | at the reserve after the last release, when the consumer reads the seal |
| The free set | two words, the producer's taken bits and the consumer's given bits, free where they agree, no CAS | one in-use word, `fetch_or` to take and a clear to give back, since the parity trick is unsound for several producers |

## Generic Queue (idea)

SPSC v3 and MPSC v2 share their setup, `init(pool, slot_size, seg_capacity, seg_count)` and
`split()`, and their consumer, `reserve_slot_with` then `release`, and differ only in the producer.
A user still picks one by module path and gets two unrelated type families, so going from one
producer to several rewrites every signature that names an endpoint. The idea is one type over
both, recorded 2026-09-17 and not yet a Todo.

- The shape: `Queue<P>`, with `P` a sealed producer marker, `Single` or `Multi`.
  - `Queue<Single>` wraps the v3 `Ring` and `Queue<Multi>` wraps the v2 `MpscRing`, each a thin
    facade that adds no state and no protocol.
  - `init` and `split` have one signature for both, and `split` gives `(Sender<P>, Receiver<P>)`.
  - Sealed, so the marker set is the crate's to extend and code generic over `P` sees every kind.
- `T` stays per call: the facade does not fix the message type, as neither ring does, so one queue
  still carries several message types. Fixing `T` at split is the separate [Typed
  endpoints](../TODO.md#typed-endpoints) Todo, which could layer over this one.
- The senders differ where the rings do:
  - `Sender<Single>` holds the v3 `Producer`: one owner, sends through `&mut self`, not `Clone`.
  - `Sender<Multi>` holds the v2 `MpscProducer`: `Clone`, sends through `&self`, so a second
    producer is a clone.
- The receiver is one API over either consumer, and `Receiver<Multi>` skips the tombstones a
  panicking `fill` leaves, as the v2 consumer does now.

### Generic Queue open questions

- Is the closure `send_with` the common send, with `reserve_slot_with` a `Single` extra?
  - MPSC v2 has only `send_with`, whose commit is by construction, since a claimed slot must
    always be committed. SPSC v3 has only the guard, `reserve_slot_with` then `commit`.
  - On `Single`, `send_with` is the guard inside a closure: reserve, `fill`, commit. A panic in
    `fill` drops the guard, which abandons the slot, the SPSC counterpart of the tombstone.
  - We think `send_with` is the common send and `reserve_slot_with` stays a `Single` extra, since
    a guard held across arbitrary code is what a claimed multi-producer slot cannot allow.
- Is the ISR kind a guard axis rather than a producer count?
  - `Multi` today means the claim CAS of MPSC v2, and thumbv6m has no CAS, so a thumbv6m target
    has no multi-producer queue. The SPSC protocol is load/store only and runs there now.
  - On one core the claim could instead run in a critical section, interrupts off around a load
    and a store. That is the "ISR sharing an endpoint" path of [Execution
    contexts](#execution-contexts), done inside the producer rather than asked of the caller.
  - So `Multi` may want a parameter naming how producers serialize, `Multi<Cas>` or
    `Multi<CriticalSection>` in one spelling. We think this is the axis, and a producer count
    alone is the wrong one, since the critical-section kind gives thumbv6m a multi-producer queue,
    the gap between Execution contexts and the embedded-floor bullet in [Ideas](../TODO.md#ideas).
- A consumer-kind axis: both rings are single-consumer, and the shape leaves room for a consumer
  marker beside `P`, defaulted, rather than fixing the single consumer into the type for good.

## Messaging layer: pools and descriptor queues

The messaging layer is the layer above the ring: pools that
own message memory, and queues that carry descriptors. This
section is its design. The sections above it are the as-built
record of the ring itself, and the pools are built as well:
the single-stack pool since the 0.6.0 cycle
(`src/pool/v0/mod.rs`: layout, init/attach, alloc/free), and
the multi-stack pool since 0.18.0 (`src/pool/v1/mod.rs`,
[Multi-stack pool](#multi-stack-pool-0180)). Descriptor queues
and provenance are designed here and not yet built.

The ring's reserve (`reserve_slot_with`) fuses three acts that
a messaging system needs separated: it allocates message
memory (the slot),
assigns queue position (the reservation is the ring head), and
opens a publication window (the guard exclusively borrows the
endpoint, blocking all other sends). Getting a message must not
imply sending it now, or ever. The fix is a layer, not a ring
rework: pools own message memory, queues carry small
descriptors, and the existing ring is the unchanged primitive
underneath (reserve-to-commit becomes a momentary act, where
the fusion is harmless).

### System model (overview)

The layer's rules at a glance, each bullet links to the
section that expands it.

- A process may have one or more pools
  [details](#pool-topology-and-phasing).
  - Each pool has one owning allocator: a single thread
    allocates, any holder frees.
- Not all pools in a process need to be in shared memory
  [details](#layer-requirements).
  - A process-private pool serves intra-process messaging.
- A message to be sent to another process must come from a
  pool in shared memory
  [details](#pool-id-resolution).
  - The receiving process must have that pool mapped.
- Queues carry descriptors, not payloads
  [details](#descriptors).
  - A descriptor is a uniform small
    `(pool id, buffer index)` regardless of message size.
  - The payload stays at rest in its pool buffer.
- Any message travels over any queue
  [details](#layer-requirements).
  - Precondition: both endpoints have the message's pool
    registered.
- Entities form an arbitrary directed graph
  [details](#execution-contexts).
  - A ring is an edge. A node is an *execution context* (a
    thread or an ISR) that may hold endpoints for many
    rings at once, in either role, and own or free into
    several pools: consume ring A, produce on B and C,
    allocate from pool P.
  - A *process* is not a node but a boundary: an address
    space grouping one or more execution contexts and owning
    the pool mappings and registries. An edge crossing it
    enters another address space, shared memory plus a
    registered pool.
  - The two many-to-one realizations (shared MPSC ring,
    fan-in of SPSC rings) are two edge-shapes for the same
    consumer in-degree. Neither is privileged.
  - Fan-out (one producer, many edges) is equally
    admissible, nothing in the primitives assumes a node
    has degree 1.
- Each process keeps its own pool registry
  [details](#descriptor-and-registry-design-070).
  - It maps pool ids to that process's view (mapping) of
    each pool.
  - Registries are private and never shared. Only
    descriptors cross process boundaries.
  - Pool ids are the shared vocabulary: every participating
    process must register a pool under the same id (the
    assignment scheme is an open question,
    [details](#pool-id-allocation)).
- Ownership follows the descriptor
  [details](#usage-model-roles-and-buffer-lifecycle).
  - alloc -> write -> send the descriptor over a queue ->
    the receiver owns it -> read, forward over another
    queue, or free.
  - A buffer is always in exactly one state with one
    permitted toucher.
- Any holder, any thread or process, may free
  [details](#free-path-intrusive-lifo-free-stack).
  - The free pushes the buffer directly onto its pool's
    free-stack in shared memory, no round-trip through
    the owning process.
  - The owning allocator finds the buffer on a later
    alloc.
- Descriptors and free-stack links arriving through shared
  memory are untrusted
  [details](#trust).
  - Both are validated before use.
  - Corruption degrades to errors or lost buffers, never
    UB.
- 0.7.0 builds the in-process slice of this model
  [details](#descriptor-and-registry-design-070).
  - As built, everything is single-process: one address
    space, threads as the peers, no shared-memory mapping
    code yet.
  - Cross-process setup (mapping exchange, pool-id
    coordination) stays design
    [details](#open-questions).

### Layer requirements

- Decoupled get/hold/send/free: allocate a message, hold
  it indefinitely, send it over any queue later, forward it,
  or free it unsent.
- **Any quantity of queues and pools** in a system, and any
  message can travel over any queue.
- Inter- and intra-process: processes share queue ends,
  and message payloads zero-copy across process boundaries.
  The embedded single-address-space case is the degenerate
  mapping where all peers share one base.
- Heterogeneous pools: pools differ arbitrarily in buffer
  size and count, and each meets the alignment constraints and
  self-describes its geometry.
- Phasing: queues are SPSC rings now, an MPSC ring is a
  future sibling primitive, and pools have a single allocator
  now, shared allocation eventually.

### Provenance and descriptors

How a message's origin travels and is resolved: one
sub-topic per heading, each directly linkable.

#### Offsets only, everywhere

Different per-process mappings mean no pointers in any
shared structure, and this also serves the single-address-space
case unchanged.

#### Message provenance

Every buffer begins with a small header naming its pool
`(pool id, offset)`. Whoever holds a message last must free
it to the right pool, so provenance travels with the
message. (Deferred as of 0.7.0, provenance travels in the
descriptor instead, see
[Descriptor and registry design](#descriptor-and-registry-design-070).)

#### Descriptors

A queue entry is a uniform small `(pool id, buffer index)`
regardless of message size. The receiver resolves the pool
id and learns geometry from the pool's own header (index
over byte offset settled in
[Descriptor and registry design](#descriptor-and-registry-design-070)).

#### Type-tag

A message's first word, a number saying which type of message the buffer holds, written by the
sender and read by the receiver before it knows the type. Payload-level, so the descriptor stays
type-agnostic, and the pool never reads it. Fixed on 2026-09-25, at the review of `test:
segmented pool messages over every ring`.

- One term: "type-tag" in prose and `type_tag` in code, the field a message starts with. It keeps
  clear of Rust's `TypeId`, a per-build value no other process can share.
- `Kind`: the decoded type-tag, an enum with `TryFrom<u64>`, so a receive `match` is exhaustive
  and an unknown type-tag fails at the decode and nowhere later. The name follows
  `std::io::ErrorKind`, and the pair follows the wider Rust habit, surveyed the same day: the
  number on the wire is a tag (rustc's encoded discriminant, serde's `tag`) and the enum it
  decodes to is a `Kind` (rustc's `*Kind` types, h2's frame `Head { kind: Kind }`, the
  `enum-kinds` crate), never a compound of the two.
- The receive path: take the buffer back as bytes (`to_slot_bytes`), read the type-tag, decode it
  to a `Kind`, and turn the bytes into the type it names (`into_typed::<T>`), which checks the fit
  and hands the bytes back on a misfit. `tests/pool_v1_over_rings.rs` and
  `mixed_messages_dispatch_by_type_tag` in `src/pool/v1/mod.rs` are the two specimens.
- Not a header: the type-tag is the message's own first field, by the sender's convention, and
  whether a pool-level header ever joins it is [Message header shape](#message-header-shape).

#### Pool self-description

Each pool region starts with a header (magic, layout
version, buffer size, count, alignment), attach-validated
exactly like the ring header.

#### Pool-id resolution

Each process keeps a local registry mapping pool id to its
own mapping of that pool's region. "Any message over any
queue" carries the precondition that both endpoints have
the message's pool mapped.

#### Trust

Offsets and indices arriving through shared memory are
untrusted: bounds- and alignment-checked against the
receiver's own view before use, same discipline as the
ring's geometry snapshot.

### Descriptor and registry design (0.7.0)

The in-process slice as designed for the 0.7.0 cycle.
Cross-process setup (mapping exchange, pool-id
coordination) remains in [Open questions](#open-questions).

- `Desc { pool_id: u32, buf_idx: u32 }`: 8 bytes, POD
  (zerocopy derives), so it rides any queue as an ordinary
  message. A buffer *index*, not a byte offset: the
  offsets-only rule bars pointers (per-process mappings),
  and an index is equally position-independent while
  matching what the free-stack and guards already speak, so
  validation is one bounds check where a byte offset would
  also need a buffer-boundary divisibility check.
- `PoolRegistry`: per-process, fixed capacity
  (const-generic array, no_std, zero allocation).
  `register(view) -> PoolId` assigns the next slot
  index. Phase 1 has no unregister, so ids never dangle.
  Sequential assignment produces cross-process id agreement
  only when one process assigns all ids, the in-process
  slice's case. Cross-process will need registration under
  an externally agreed id (e.g. `register_at(id, ...)`),
  pending [Pool-id allocation](#pool-id-allocation).
- `Pool::view() -> PoolView`: a view that cannot pop
  (header ref, buffer base, geometry) derived from the
  existing handle, so no second region borrow and no
  Stacked Borrows retag hazard (`init` takes the region
  pointer exactly once). Send + Sync: `to_slot` mints
  guards from validated indices, and the only
  shared-memory mutation is the guards' free CAS, already
  any-thread. Cross-process later, the same view derives
  from an `attach`ed handle.
- `to_desc(slot, pool_id)`: safe and O(1). It checks the
  guard's header address against the id's registry entry
  (catches id/pool mispairing), consumes the guard, and
  ownership travels on in the descriptor (the usage
  model's "in-flight" state). The error side hands the
  guard back, so a miss cannot leak the buffer.
- `unsafe to_slot::<T>(desc) -> Result<BufSlot<T>, _>`:
  every failure is an `Err`, never a panic: unknown pool
  id, index out of range, `T` geometry mismatch. The
  geometry case differs from `alloc` (which panics)
  because the descriptor selects which pool gets compared,
  so untrusted input must not select a panic. `unsafe`
  covers the one thing validation cannot check,
  ownership: the caller promises the desc came from
  `to_desc`, arrived over a channel establishing
  happens-before (ring commit -> reserve qualifies), and is
  taken back exactly once.
- `Desc` is plain data on purpose: `FromBytes` means a
  receiver mints one from shared bytes anyway, so a
  move-only ownership token would be theater, and the
  discipline lives in `to_slot`'s contract.
- Names: `into_desc`, `resolve`, `PoolResolver`, and
  `resolver()` were renamed `to_desc`, `to_slot`, `PoolView`,
  and `view()` on 2026-09-24, when the multi-stack pool made
  the registry generic over a sealed `DescMap` trait.
- In-buffer provenance deferred: the descriptor is the
  message's travel form (forwarding re-sends it), so no
  flow carries a buffer without its provenance, and adding an
  in-buffer header later is a pool `layout_version` bump.
- Type dispatch is payload-level: the descriptor stays
  type-agnostic. Multiple message types over one queue
  need a receiver-readable [type-tag](#type-tag): a first-word
  id driving a
  match (demo-minimal), `TryFromBytes` tagged enums, or a
  future `Message` trait hiding the cast boilerplate (see
  todo Ideas).
- Single-address-space profile (future): with no MMU
  and one trust domain, a descriptor could carry pointers
  (a flattened guard), zero-lookup resolve, but nothing to
  validate against. A scribbled pointer descriptor is UB
  on use, where id + index degrades to an `Err`. The
  canonical form stays `(pool id, buf idx)`. A pointer
  profile could arrive later behind the same trait seam as
  other usage styles.

### Free path: intrusive LIFO free-stack

Freeing must be cheap for many freers and allocation must be
O(1) with no searching (a per-buffer available-flag makes free
trivial but alloc a scan). The free operation is itself the
marking: push the buffer onto the pool's free-stack.

- Intrusive: a freed buffer's header holds the next-link
  as an offset, so the free-list needs no extra storage.
- LIFO on purpose: we think a stack beats a free-ring on
  cache behavior: FIFO cycles through every buffer (maximum
  working set), LIFO reuses the most-recently-freed few, and
  the small working set also helps the TLB. The full
  same-thread malloc-style hot-reuse win is diluted because
  the freer and allocator are different cores, but the
  working-set effect stands regardless.
- Single-popper Treiber stack: free = CAS the head to
  your buffer's offset (MPSC push side: any thread or process
  frees into any pool), alloc = the one owning allocator pops
  the head. A single popper eliminates classic Treiber ABA:
  no node leaves the list under a competing pop, pushes just
  retry the CAS.
- Validated pops: head and next-links are untrusted, and
  the popper bounds/alignment-checks each offset and caps
  pops per attempt, so a corrupt peer can lose buffers or
  cycle the list but never make the allocator read outside
  the pool.
- CAS note: the free-stack is the first structure to
  need CAS. The ring protocol itself stays load/store-only,
  so the atomic floor rises only for pool users.

### Pool topology and phasing

- One owning allocator per pool (invariant, phase 1): a
  single thread allocates, and any party frees. Per-thread pools
  are the performance default and satisfy this trivially.
- Shared pools already exist in phase 1: shared for
  reading, forwarding, and freeing, only allocation is
  single-owner.
- Shared allocation (phase 2): per-thread pools cost
  memory, and N threads sharing one pool's capacity needs
  multi-popper alloc, which needs an ABA-proof head:
  pack `(offset, generation)` in one `AtomicU64` and CAS
  both together. A contained per-pool variant (opt-in flag),
  not a redesign, since hot paths keep the cheap single-popper
  version. Heterogeneity mitigates meanwhile: a small private
  hot-path pool plus a big shared fallback pool.
  - Decided 2026-09-25: the shared pool becomes the default,
    not an option. A pool is a shared-memory allocator with no
    roles, any process allocates from any pool it maps, and
    `Ring::init` takes `&Pool`, so a process builds its ring
    in whichever pool it likes. The single-popper pools stay
    as the baselines and for the 32-bit targets. The Todo
    entry `Shared allocation: a pool any process can allocate
    from` holds the plan, behind the inter-application test,
    which needs only one allocator, and ahead of any
    multi-process MPSC, whose every producer allocates.
- MPSC ring (future sibling): multi-producer queues need
  a different ring protocol (CAS-claimed producer index,
  per-slot sequence state), and it slots in as a sibling
  primitive under the same descriptor layer. The endpoint
  claims word should model role slots, not a single producer
  bit, so N producer claims fit later without a layout
  rethink. Full design:
  [MPSC ring (sibling primitive)](#mpsc-ring-sibling-primitive).

### Multi-stack pool (0.18.0)

`pool::v1`, as built in the `feat: segmented pool v1` cycle. A pool had one buffer size, so an
application wanting messages of several sizes built and registered several pools by hand. The
multi-stack pool keeps v0's API over one region holding `N` stacks, one per buffer size, and the
alloc family picks the stack by size. v0 stays the single-stack pool and the default `Pool`, and
the rings of segments still take their segments from a v0 pool (the `spsc4 and mpsc3 over either
pool` Todo). "Stack" is the pools' word and "segment" the rings', a ring segment being a pool
buffer.

- Layout: one region, one `PoolHeader<N>` (the geometry words, each stack's size and count, then
  one cache line per stack head), and the stacks after it, smallest first, each a run of buffers
  starting on a cache line. A buffer's index is local to its stack, and `N = 1` is v0's two-line
  header over one stack. The magic differs from v0's, so neither pool attaches the other's region.
- Geometry: `init` takes `[StackGeometry { buf_size, buf_count }; N]` in any order and sorts the
  stacks by size, so the layout and the search are the pool's to change. Two stacks of one size
  are `BadBufSize`, a duplicate being likelier a mistake than a request, and the total count stays
  below the NIL sentinel so every buffer has an index across the stacks. `attach` requires the
  region's stacks in the pool's order, and `BadStackCount` when its `N` differs. There is no
  public stack index: `stacks()` reports the geometry in the pool's order, and `stats()` a
  `StackStats { geometry, misses }` per stack.
- Alloc: `alloc::<T>()`, `alloc_with`, and `alloc_bytes(size)` take the smallest stack that fits,
  by a scan, and fall back to the next larger stack when it is empty, `Exhausted` when none is
  left. A size larger than the largest stack panics, as v0's size check does, a request the pool
  was not built for. At `N = 1` the pick is v0's one size comparison and the fallback loop is
  empty.
- Misses: a miss is counted once per call whose wanted stack was empty, whatever the fallback
  then does, so a user can tell which size wants more buffers. The counters live in the
  allocating handle, plain counts, since allocation has one owner, fresh per handle.
- Free: a `BufSlot` holds its stack's head and its stack-local index, so a free touches its own
  stack alone and never searches, with no header pointer. `buf_size()` reports the size given, at
  least the size asked for, on a typed slot as on a byte slot.
- Registry: `PoolRegistry<'a, N, R = v0::PoolView>` is generic over a sealed `DescMap`, which
  both pools' views implement, so dispatch is static and one registry holds one kind of pool. A
  descriptor's `buf_idx` numbers the buffers across the stacks, smallest first, the stack's first
  index plus the local one. `to_desc` finds the stack by comparing the slot's head with each
  stack's, one comparison per stack, and `to_slot::<T>` finds the stack whose index range holds
  the index and checks `T` against that stack's size, so a `T` too big for its buffer is `BadType`
  even when a larger stack exists.
- Bytes first: `to_slot_bytes` takes a descriptor back as bytes, the counterpart of `alloc_bytes`,
  and `into_typed::<T>` on a byte `BufSlot` checks the fit and turns it typed, handing it back on
  a misfit. A descriptor is taken back once, by `to_slot` or `to_slot_bytes`, never both. This is
  the receive path for messages of several types over one queue, dispatched by
  [type-tag](#type-tag), and every ring carries them unchanged, since a descriptor is plain data
  (`tests/pool_v1_over_rings.rs`).
- Measured (2026-09-24, the demo's `pool_alloc_free_1t` and `pool1_alloc_free_1t` rows, an alloc,
  write, free loop pinned to the base cpu, 20 runs of each, ns per message means, stdevs 0.1 or
  less). The scratch builds were measured and reverted:

  | build | v0 | v1, 1 stack | v1, 4 stacks, 1st | v1, 4 stacks, 4th |
  |---|---|---|---|---|
  | v1 as committed | 9.74 | 9.12 | 11.00 | 11.03 |
  | scratch: unchecked indexing | 9.79 | 9.12 | 11.00 | 11.06 |
  | scratch: cold fallback | 9.85 | 10.01 | 9.96 | 10.08 |

  - The stack choice is free: the 1st and 4th stacks time alike, and with `alloc` inlined four
    stacks cost what one does, in a loop that picks the same stack every time, so the scan's
    branches always predict. A workload mixing sizes may pay for mispredictions, unmeasured.
  - Bounds checks cost nothing: removing them in the pop changed no row.
  - Inlining is what moves the numbers. At four stacks `alloc`, fallback loop included, is too big
    to inline and stays a call, the 1.9 ns, and a `#[cold]` out-of-line fallback lets it inline
    again.
  - v0 is a handicapped baseline: `next_buf_idx` and `buf_ptr` are calls in the demo's loop, not
    `#[inline]` and not generic, so they cannot inline across the crate boundary, while v1 is
    generic and compiles whole in the demo. That is the 0.6 ns by which v1 at one stack beats
    v0, and the `Pool inlining and an iiac-perf comparison` Todo is the fix and the comparison
    worth trusting.
  - Compile-time stack sizes, a possible v2, would only speed a choice that timed as free, so the
    idea is dropped until a measurement says otherwise.
- Tests: scenario tests over a three-stack pool pin the fallback order and the size boundaries, a
  model test drives 20,000 random allocs and frees per seed against a plain model of the rule,
  and a threaded test races frees against pops on every stack's head, each over a geometry the
  seed picks and handed to `init` shuffled. The tests were checked against two deliberate breaks
  of the rule. Everything passes under Miri.
- Out of scope, each a later cycle if wanted: a v1 flavor in `tp-pool`'s sweep, the rings taking a
  v1 pool, and compile-time sizes.

### Usage model: roles and buffer lifecycle

The user-facing contract, who may do what, per object and
per buffer state, lives in the README:
[Usage model](../README.md#usage-model-roles-and-buffer-lifecycle).
The tests exercise exactly what it permits. Summary:

- ring = one producer + one consumer.
- pool = one allocator + any freers.
- a buffer is always in exactly one state
  (free / allocated / in-flight [vacant until descriptor
  queues] / freed) with one permitted toucher.
- "send" this cycle means moving the `BufSlot`.

### Holders and recovery

A pool shared by processes must not lose what a crashed process held: a crashed consumer is
restarted or replaced and takes over its duties, losing its in-progress work and nothing else,
the requirement set on 2026-09-25. The cycle `fix: spsc v4 roles survive their holders` meets it
for the ring's roles, and names what meets it for the pool's buffers.

#### Destructors never touch shared memory

Shared state changes only through protocol operations, never in a `Drop`.

- A process that dies runs no destructor, so a design that cleans up in one is correct only
  when nothing crashes. One that runs leaves the region saying what the destructor wrote, which
  for a role was "free" when the role's state had gone with the endpoint.
- The v4 endpoints were the crate's only destructors writing shared memory, and their failure
  was the bug of `m-7`: a role taken again after a drop started at segment 0, position 0, on a
  ring that had run, and hung. They now have none.
- So teardown and handoff are calls, `release`, and recovery is a call, a takeover, each one
  deliberate. The rule holds for the next attachable ring, an MPSC, from its first design.
- One kind of destructor breaks it, and whether it stays is open: the MPSC rings' producers,
  v0 through v2, arm `TombstoneOnUnwind` while the fill closure runs, a `Drop` that publishes a
  tombstoned commit into the claimed slot's seq word if a panic unwinds through `send_with`, so
  the consumer is not left waiting on a slot no one will commit. It runs only on the unwind
  path, in a process that survives the panic, and finishes a protocol step rather than undoing
  one. A death it cannot see, an abort or a kill, leaves that slot claimed and the ring stuck,
  which is the MPSC's own takeover question. Found on 2026-09-26 writing this rule, left to the
  user at the cycle's close-out.

#### The inbox model

A ring belongs to the process that reads it. That process creates it in a region it owns, lives
as long as its inbox does, and claims the consumer, and every producer is a process that joins
it by `attach` and claims the producer. A ring's address is `(region, first_segment)`, and
naming it is [Naming and transport](#naming-and-transport)'s. No process holds both roles of one
ring, so there is no call that claims both, and in-process callers claim twice.

#### Handoff and takeover

- Handoff is `release` then a claim: the holder writes its exact state and marks the role
  released, and the next claimant, in any process, continues from it. Nothing is lost.
- Takeover is the replacement of a holder that did not release, because it died or hung. The
  crate records, a holder id per role and a checkpoint per switch, and recovers, from the
  checkpoint and one segment's seq words. It never judges liveness: a `no_std` crate has no
  processes to ask about, so the id is the app's, and `take_over_*` is the app vouching the
  holder is gone. A supervisor that restarts consumers is where that judgment lives.
- The checkpoint is written on the switch path, never per message, which is what keeps the
  message path as it was: the position within a segment is left to the seq words, and a
  takeover scans them once.

#### The pool half

A crashed process also leaves buffers it allocated and never sent, or received and never freed.
The ring's recovery does not reach them, since the pool knows no holders. What does is the same
pattern: an owner word in the in-buffer header, beside the length and the count that header
already owes ([Message header shape](#message-header-shape)), and a sweeper that returns the
buffers of a holder the app declares dead. It follows shared allocation, since a pool with one
allocator has one owner to sweep for, and it waits on the header's layout.

### Overflow FIFO (future)

When a queue's ring is Full, the sender appends the message to
a pending FIFO instead of failing. "send" then always
succeeds while pool memory lasts.

- Intrusive, zero-allocation: the FIFO links through the
  same embedded next-link offset the free-stack uses. A
  buffer is on at most one list at a time (free stack,
  pending FIFO, in flight in a ring, held by an owner), so
  one link field serves every state.
- Sender-private: the pending list belongs to the
  producer endpoint: head + tail offsets for O(1) append, no
  shared mutation, no CAS. Order discipline: the producer
  drains the FIFO oldest-first before ring-sending anything
  new, or FIFO order breaks. Draining happens on subsequent
  send attempts and/or an explicit flush.
- Validated traversal: the links live in shared pool
  memory a peer can scribble, and the drainer bounds- and
  alignment-checks each offset as the free-stack popper does.
- Naturally bounded: messages come from pools, so the
  FIFO cannot outgrow total pool capacity, and backpressure
  reappears as allocation failure rather than queue Full.

### Prior art: iceoryx2

[iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) is a
Rust-native zero-copy IPC library implementing most of this
layer: publishers loan uninitialized samples from a pool
allocator, write in place, and send later or never (get/send
decoupled). Receivers get offsets they resolve against their
own mappings. Events, request-response, and multi-producer
patterns exist. Study its loan/send API and pool-offset
machinery before implementing the pools, its lessons are
cheaper stolen than rediscovered.

What keeps this project distinct from it:

- iceoryx2 is service-oriented middleware (discovery, naming,
  configuration) for Linux/macOS/Windows/QNX-class platforms.
  This crate is a small set of composable primitives.
- No `no_std` / load-store-only floor: our ring reaches
  `thumbv6m`, with CAS confined to the pool layer.
- Trust posture: our attach-validated
  hostile-peer-cannot-cause-UB discipline versus cooperating
  participants within a framework.

### Prior art: cordyceps MpscQueue

[cordyceps](https://github.com/hawkw/mycelium/tree/main/cordyceps)
carries `MpscQueue<T>`, Vyukov's intrusive node-based MPSC,
the linked-list shape the pool-message sweep measures beside
the descriptor rings. The queue owns no storage: a node is
the caller's, reached through the `Linked` trait, whose
handle type is the implementer's choice (a pinned box in the
crate's tests, a pool buffer if the tools rung's check holds)
and whose `Links<T>` field, one atomic next pointer, sits
inside the message. So the message carries its own link, and
one shared line per message moves where the ring moves the
payload line and a descriptor slot. It is `no_std`, unbounded,
and allocation-free once the nodes exist, a stub node the
queue holds keeping the tail from ever being null.

The protocol, as `tests/cordyceps_mpsc.rs` pins it:

- Push is a swap on the head and a store of the previous
  node's link, two atomics and wait-free, where the ring's
  claim is a CAS loop that retries under contention.
- Between those two atomics the consumer can find the tail's
  link null while the head has moved, and reports
  `Inconsistent`, a window a preempted producer widens. The
  blocking dequeue spins on it, and the sweep's consumer
  counts it.
- One consumer at a time, a flag CAS that a second caller
  finds `Busy`, and `Empty` when the tail's link is null and
  the head sits at the tail.
- Drop walks what is still enqueued and hands every node back
  through its handle, so nodes are never leaked by the queue.

What keeps this project distinct from it:

- Pointers, not offsets: a node's link is a machine address,
  valid in one address space and trusted as written, where
  the ring and the pool address shared memory by index and
  validate every one at attach and on every pop, so a
  hostile peer cannot cause UB. That is the reason an own
  version of this shape would use offsets.
- Unbounded: no `Full`, so back-pressure is the pool's
  `Exhausted` alone, which is what makes the pool size the
  sweep's axis and gives the queue no depth of its own.
- In-process: the queue has no attach, no header, and no
  layout version, so it composes with the pool inside one
  process and cannot cross the boundary the rings are
  designed for.

### Measured: pool-message sweep

The messaging layer's loop, take a message from the pool,
fill it, push its reference, receive it, process it, return
it, run by `tp-pool` over `spsc-v2` and `mpsc-v1` carrying a
`Desc` and over cordyceps's `MpscQueue` linked through the
same pool's buffers ([Prior art: cordyceps
MpscQueue](#prior-art-cordyceps-mpscqueue)), so the queue is
the only variable between rows. 1p/1c throughout.

- **Measured (2026-09-12, 3900X, the rung `perf: sweep pool
  size over descriptor rings and cordyceps`, `tp-pool` at its
  defaults: pools 1, 100, and 1000, ring depths 1, 8, 64, and
  1024, 1M messages a cell, median of 3)**. A second full run
  agreed within a few percent at every pinned cell.
  - No prediction was written before the run. The tools
    rung's smoke runs had already shown the numbers, so the
    readings below are findings, not a prediction checked.
  - ns per message, pinned placements:

    | placement | flavor    | depth | pool=1 | pool=100 | pool=1000 |
    |-----------|-----------|------:|-------:|---------:|----------:|
    | 0,1 CCX   | spsc-v2   |     1 |  144.6 |    108.9 |     108.8 |
    | 0,1 CCX   | spsc-v2   |     8 |  146.6 |     70.3 |      61.9 |
    | 0,1 CCX   | spsc-v2   |    64 |  135.6 |     63.3 |      64.4 |
    | 0,1 CCX   | spsc-v2   |  1024 |  132.4 |     63.7 |      64.4 |
    | 0,1 CCX   | mpsc-v1   |     1 |  164.4 |    123.4 |     126.9 |
    | 0,1 CCX   | mpsc-v1   |     8 |  164.4 |     95.2 |      94.7 |
    | 0,1 CCX   | mpsc-v1   |    64 |  163.3 |     91.0 |      91.2 |
    | 0,1 CCX   | mpsc-v1   |  1024 |  161.2 |     90.7 |      91.0 |
    | 0,1 CCX   | cordyceps |     - |  166.7 |     75.0 |      74.3 |
    | 0,3 x-CCX | spsc-v2   |     1 |  523.4 |    379.9 |     373.2 |
    | 0,3 x-CCX | spsc-v2   |     8 |  489.2 |    203.1 |     202.9 |
    | 0,3 x-CCX | spsc-v2   |    64 |  498.8 |    211.5 |     209.4 |
    | 0,3 x-CCX | spsc-v2   |  1024 |  500.3 |    216.8 |     216.4 |
    | 0,3 x-CCX | mpsc-v1   |     1 |  571.8 |    457.0 |     453.7 |
    | 0,3 x-CCX | mpsc-v1   |     8 |  588.3 |    362.8 |     367.9 |
    | 0,3 x-CCX | mpsc-v1   |    64 |  582.2 |    346.3 |     341.0 |
    | 0,3 x-CCX | mpsc-v1   |  1024 |  603.0 |    336.7 |     339.0 |
    | 0,3 x-CCX | cordyceps |     - |  614.2 |    221.8 |     216.8 |
    | 0,12 SMT  | spsc-v2   |     1 |   55.3 |     39.6 |      39.6 |
    | 0,12 SMT  | spsc-v2   |     8 |   57.4 |     33.1 |      33.1 |
    | 0,12 SMT  | spsc-v2   |    64 |   56.7 |     33.3 |      33.3 |
    | 0,12 SMT  | spsc-v2   |  1024 |   56.0 |     33.1 |      33.1 |
    | 0,12 SMT  | mpsc-v1   |     1 |   63.7 |     48.0 |      47.9 |
    | 0,12 SMT  | mpsc-v1   |     8 |   63.5 |     42.2 |      42.1 |
    | 0,12 SMT  | mpsc-v1   |    64 |   63.6 |     41.6 |      41.7 |
    | 0,12 SMT  | mpsc-v1   |  1024 |   63.5 |     37.2 |      36.9 |
    | 0,12 SMT  | cordyceps |     - |   74.8 |     33.0 |      33.1 |

  - Cross-core fills per message at the like-for-like rows,
    ring depth 1024, and the cordyceps consumer's
    `Inconsistent` retries in the median run:

    | placement | flavor    | pool=1 | pool=100 | pool=1000 | Inconsistent at 100, 1000 |
    |-----------|-----------|-------:|---------:|----------:|--------------------------:|
    | 0,1 CCX   | spsc-v2   |  5.847 |    4.991 |     4.977 |                           |
    | 0,1 CCX   | mpsc-v1   |  8.605 |    7.368 |     7.401 |                           |
    | 0,1 CCX   | cordyceps |  8.978 |    6.184 |     6.103 |          172658, 165229   |
    | 0,3 x-CCX | spsc-v2   |  5.857 |    4.795 |     4.805 |                           |
    | 0,3 x-CCX | mpsc-v1   |  8.661 |    7.453 |     7.426 |                           |
    | 0,3 x-CCX | cordyceps |  8.873 |    4.688 |     4.575 |            30576, 21729   |

  - The unpinned cells moved by a third between the two runs
    and are left out.
- **Readings**:
  - The linked list is not faster than the descriptor ring.
    Once the pool lets messages queue, cordyceps matches
    `spsc-v2` across the CCX and on the SMT pair, 217 against
    216 and 33 against 33, and trails it by a sixth on the
    same CCX, 75 against 64. It beats `mpsc-v1`, the ring it
    competes with as an MPSC, by a third across the CCX, 217
    against 339, on 4.6 lines per message against 7.4.
  - At pool 1, one message in flight, cordyceps is the slowest
    row everywhere, 614 against 500 across the CCX and 75
    against 56 on SMT. We think it is the stub: a dequeue that
    empties the queue re-enqueues the stub, so the consumer
    writes the head line the producer writes on every
    message, which the fills read as 8.9 lines against
    `spsc-v2`'s 5.9.
  - Pool 100 and pool 1000 read the same in every row, so
    the L2 footprint the cycle expected at 1000 never shows.
    The free-stack is LIFO: the producer's alloc pops the
    buffer the consumer's free just pushed, so the working
    set is the messages actually in flight, not the pool.
  - Ring depth matters well below the pool: `spsc-v2` reaches
    its plateau by depth 8 and `mpsc-v1` by 64 at pool 100,
    so the throttling rows are depth 1 alone for `spsc-v2`,
    where the cost is 1.8 times the plateau across the CCX.
  - The loop is an order of magnitude above the in-slot path:
    `spsc-v2` streams at 14 ns per message across the CCX at
    depth 64 in `tp-stream` and runs this loop at 209 at the
    same depth, the pool's
    free-stack head crossing back on every message whatever
    the queue does.
  - The `Inconsistent` window is common once messages queue,
    about one message in six on the same CCX, one in forty
    across it, one in twenty on SMT, and never at pool 1. The
    tests' two producers never landed in it, since it takes a
    consumer fast enough to reach the tail between one
    producer's two atomics.

### Open questions

One question per heading, each directly linkable, and a
resolved question migrates to
[Resolved questions](#resolved-questions), keeping its
heading (and so its anchor).

#### Pool-id allocation

In-process settled in 0.7.0 (registry slot index, no
unregister). Cross-process remains open:
coordinator-assigned versus derived from the shm object's
identity.

#### Setup plane

How a process discovers and maps a pool it hasn't seen:
pre-arranged at init (embedded-friendly, allocation-free
after startup) versus fd-passing over a Unix socket
(Linux-friendly, dynamic), likely both, as profiles.

#### Message header shape

0.7.0 settled the near half: provenance travels in the
descriptor, a [type-tag](#type-tag) is payload-level, and the in-buffer
header is deferred (see
[Descriptor and registry design](#descriptor-and-registry-design-070)).
Still open: whether a length (or anything else) joins a
future in-buffer header, and its layout against the
embedded next-link, the link is load-bearing for three
states (free-stack, pending FIFO, future lists), so they
must be laid out together when that header lands.

- Two consumers of the header named on 2026-09-25, so its
  fields are now known even though its layout is not: a
  **length**, since a message that crosses a wire must say how
  many bytes it is ([Naming and transport](#naming-and-transport)),
  and a **count**, the readers still holding a buffer that
  several consumers share, freed by the last ([MPMC: shared
  and copied](#mpmc-shared-and-copied)). The type-tag stays
  the payload's first word, by the sender's convention, so the
  header holds what the sender's type cannot: length, count,
  and the next-link.

#### MPMC: shared and copied

Off the table on 2026-09-25, and coming back, so its two
variants are named now, since they are two mechanisms and
only one is a ring's:

- **Shared**: one buffer, N consumers. A descriptor delivered
  to each, the buffer freed by the last reader, which is the
  count in the in-buffer header ([Message header
  shape](#message-header-shape)). The ring changes little,
  and the pool's buffer gains a header.
- **Copied**: N buffers, one per consumer. Something reads
  the message once and writes N copies into N inboxes. That
  is not a ring at all, it is a bridge, the same component a
  network transport needs, so the copied variant arrives with
  the transport work rather than with a ring.

#### Naming and transport

The design is meant to work over a LAN or the Internet as well
as shared memory, off the table on 2026-09-25 and expected to
be the next large change, so the shapes settled now are the
ones a transport would keep. Performance over a wire is a
different order, and finding a ring and taking a role must
work the same.

- **The inbox model is the socket model**: a ring belongs to
  the process that reads it, which creates it in a region it
  owns and lives as long as its inbox does, and every producer
  joins it. Over a wire that is a listener and its connections,
  so the ownership story does not change, only the transport.
- **Claims are the verb on both**: `claim_producer` over
  shared memory is a CAS on a word in the ring's control block,
  and over a wire it is a request the ring's owner grants or
  refuses, with `RoleTaken` the same answer. A claim ends by
  `release` or a takeover, never by a destructor, and over a
  wire a dropped connection is the owner's evidence for the
  takeover it makes.
- **A ring is found by name, not by address.** In shared
  memory a ring's address is `(region, first_segment)`, over a
  wire it is `(host, port, ring id)`. An app asks for a name
  through one `find` and gets an endpoint, so the resolver is
  the only thing a transport replaces. The setup-plane
  question above becomes "a name resolves to a transport and
  an address", and the first inter-application program
  resolves its name in one function even while that function
  is two lines.
- **Zero-copy ends at the wire.** Descriptors never cross
  hosts, a bridge copies payloads, so a message must be
  self-describing: the type-tag it has and the length the
  header gains. `zerocopy` `repr(C)` messages are already
  wire-shaped bytes, and endianness is the one open decision,
  native today and fixed at the bridge when hosts differ.
- **Pools stay local by design**: a pool is a region's
  memory, so it never spans hosts, "any pool from anywhere"
  means anywhere in the shared-memory domain, and a bridge is
  a process with a pool on each side. The network's unit is
  the message copy, never the buffer.
- **Endpoints as a trait**: the ring test over every ring
  drives seven rings through one `DescTx` / `DescRx` pair, and
  the `Message` trait idea carries a transport seam. An app
  written against a producer / consumer trait with the
  wait-policy closure is one a socket-backed endpoint can
  implement, "the policy gave up" meaning the same thing, so
  the inter-application program is where that trait first
  earns its place.

## Measurement placements: the base cpu and its partners

Every pinned measurement here, the demo's single-thread lines and the 2t placements of the demo
and the tools, starts from a base cpu, `--base-cpu` since 0.17.1, and the partners are picked from
the base's topology. This section records why the default base is the last core's primary cpu
and why partners prefer primary cpus and high cpu numbers. The terms are in
[Terminology](#terminology). Decided 2026-09-16, in the
cycle `feat: the demo's base cpu and pin-pair picker`.

### Where the kernel puts work

Linux does not favor cpu 0 for user work. On fork and exec the scheduler runs its idlest-cpu
search: it walks the sched domains from the top, takes the least loaded group, then the idlest
cpu in it, and breaks ties toward the lowest cpu number. On a wakeup it tries the task's previous
cpu or the waker's if idle, then scans the same L3 for an idle cpu in ascending order. Under
light load, short-lived work and wakeups therefore pile onto the low-numbered cpus, and the
high-numbered ones are rarely reached. Cpu 0 is noisier still as the boot cpu: the legacy timer
interrupt, timer migration for unbound timers, RCU callbacks, and kernel threads with the
default mask land there. Device interrupts are not the issue on these machines, since without
`irqbalance` the kernel spreads MSI-X vectors across cpus.

### The counts

Per-cpu housekeeping on the 3900X after 3.5 hours up, `LOC` and `CAL` from `/proc/interrupts`,
`SCHED` and `RCU` from `/proc/softirqs`. Cpu N and N+12 are SMT siblings, and the CCXs are 0 to
2, 3 to 5, 6 to 8, and 9 to 11 with their siblings.

| cpu | timer ticks | sched softirq | RCU softirq | function calls |
|---:|---:|---:|---:|---:|
| 0 | 1,560,256 | 1,021,188 | 216,670 | 1,295,783 |
| 1 | 1,824,569 | 557,011 | 237,459 | 846,549 |
| 2 | 1,110,485 | 432,517 | 108,673 | 743,820 |
| 3 | 976,948 | 170,059 | 99,133 | 473,892 |
| 4 | 516,865 | 112,977 | 71,676 | 359,730 |
| 5 | 393,071 | 99,727 | 61,993 | 414,282 |
| 6 | 209,793 | 60,003 | 45,120 | 272,310 |
| 7 | 295,510 | 81,022 | 58,740 | 196,706 |
| 8 | 268,064 | 61,596 | 54,457 | 200,067 |
| 9 | 173,550 | 46,754 | 41,032 | 170,477 |
| 10 | 122,279 | 35,107 | 31,547 | 126,697 |
| 11 | 170,349 | 41,980 | 38,943 | 138,013 |
| 12 | 1,302,048 | 251,105 | 174,672 | 497,943 |
| 13 | 1,355,588 | 255,237 | 191,024 | 886,138 |
| 14 | 848,691 | 151,062 | 89,014 | 591,527 |
| 15 | 447,478 | 102,807 | 75,757 | 374,719 |
| 16 | 354,980 | 89,251 | 62,858 | 319,907 |
| 17 | 371,568 | 105,108 | 74,555 | 373,342 |
| 18 | 207,637 | 60,397 | 47,987 | 190,014 |
| 19 | 180,450 | 60,798 | 44,023 | 198,925 |
| 20 | 164,924 | 44,033 | 41,010 | 168,244 |
| 21 | 152,323 | 41,483 | 38,355 | 153,120 |
| 22 | 97,997 | 28,561 | 26,492 | 121,589 |
| 23 | 128,069 | 37,137 | 33,722 | 149,374 |

The gradient is the ascending search: the first CCX and its siblings carry ten times the ticks
of the last, within each CCX the first core is busiest, and each sibling roughly tracks its
partner thread. Cpu 1 is not a quiet core, it is nearly cpu 0's equal.

The 7600X after 21 hours up, siblings N and N+6, one L3 over all twelve:

| cpu | timer ticks | sched softirq | RCU softirq | function calls |
|---:|---:|---:|---:|---:|
| 0 | 676,256 | 544,570 | 69,474 | 85,695 |
| 1 | 322,523 | 110,332 | 56,299 | 71,021 |
| 2 | 527,683 | 223,442 | 105,163 | 115,443 |
| 3 | 453,497 | 212,167 | 103,092 | 106,847 |
| 4 | 247,209 | 112,997 | 48,590 | 62,505 |
| 5 | 200,569 | 99,859 | 46,699 | 57,260 |
| 6 | 266,829 | 118,598 | 11,183 | 173,279 |
| 7 | 633,807 | 226,201 | 129,955 | 52,516 |
| 8 | 285,949 | 138,978 | 60,234 | 61,094 |
| 9 | 251,317 | 115,777 | 57,153 | 52,647 |
| 10 | 209,185 | 100,802 | 49,863 | 52,673 |
| 11 | 234,554 | 119,945 | 52,373 | 45,029 |

Flatter with one L3 and no CCX boundary to stop the scan, but the same shape: cpu 0 has five
times the reschedules of cpu 5, and cores 4 and 5 with their siblings are the quiet end. Cpu 7,
cpu 1's sibling, is busier than cpu 1, which we think is something pinned rather than the fill
order.

### The rule

- The default base is the last core's primary cpu: cpu 11 on the 3900X, cpu 5 on the
  7600X. The first cpu of the highest L3 group was considered and rejected, since on a one-L3
  machine it is cpu 0 again.
- Partners prefer a core's primary cpu over its secondary, and among those the highest cpu number.
  The SMT partner is the base's own sibling. Two earlier orders were tried and dropped: the first
  cpu going up from the base paired base 1 with cpu 0 for CCX, the first other core on the L3,
  and cpus above the base first paired every base in the last CCX with cpu 12 for x-CCX, cpu 0's
  sibling, and paired base 11 with cpu 21, cpu 9's secondary cpu, for CCX.
- What the rule gives:

| machine | base | CCX | x-CCX | SMT |
|---|---:|---|---|---|
| 3900X | 11 (default) | 11,10 | 11,8 | 11,23 |
| 3900X | 9 | 9,11 | 9,8 | 9,21 |
| 3900X | 1 | 1,2 | 1,11 | 1,13 |
| 7600X | 5 (default) | 5,4 | none | 5,11 |
| 7600X | 1 | 1,5 | none | 1,7 |

So the default pairs sit on the quiet end of each machine, and the x-CCX partner is the
neighbouring CCX's quietest core. The L3 grouping is the Zen shape: a part whose cluster shares
L2 and exposes no L3 would find no CCX pair and call every other core x-CCX, and the cluster
list the kernel exposes, `cluster_cpus_list`, is the fix when such a machine arrives. The real fix for calibrated numbers is `isolcpus` and
`nohz_full` on a set of cores, a boot-line change and a separate decision, and the numbers here
are eyeball numbers on a quiet base rather than isolated ones.

## Resolved questions

- Cross-process trust: resolved in 0.3.0-4. The
  all-atomic header plus the geometry snapshot mean a hostile
  peer cannot cause UB, only garbage messages (still valid
  `T`s, zerocopy guarantees representation, not sense) or
  spurious Full/Empty. `attach` remains `unsafe` for the
  mapping-liveness contract, not for peer trust.
- `split()` once-guard: the direction chosen is an
  endpoint-claims word in the header (CAS to claim the
  producer / consumer role, second claimant gets an error),
  which also catches in-process double-attach, and costs a
  layout_version bump (or spends `_pad0`). Promoted to a
  `## Todo` entry in [TODO.md](../TODO.md). The documented
  contract stands until then.
