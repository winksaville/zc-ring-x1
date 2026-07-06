# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../AGENTS.md#prose-form). It covers terminology,
requirements, constraints, the memory layout, the API, and
the validation ladder, and is kept in sync with the
implementation in `src/` (as-built): `lib.rs` holds the
region-level machinery (header, init/attach/split, geometry
helpers) and re-exports the endpoint modules `producer.rs`
(Producer, WriteSlot, Full) and `consumer.rs` (Consumer,
ReadSlot, Empty).

## Goal

A ring buffer for passing typed messages between a producer and
a consumer with no copying at the boundary: the producer writes
its message directly into buffer memory, the consumer reads it
in place. The same layout must work intra-process (threads) and
inter-process (shared memory).

## Terminology

The vocabulary the rest of the document builds on, defined
once before first use:

- **slot** — a cache-line-aligned `[u8; N]`: one of the
  ring's M fixed-size buffers. Untyped storage — a message
  is a zerocopy `&T` / `&mut T` view into it (geometry
  details in [Constraints](#constraints)).
- **reserve** — take exclusive access to a slot through a
  guard:
  - the producer reserves the next free slot
    (`WriteSlot`);
  - the consumer reserves the oldest unread slot
    (`ReadSlot`).
- **commit** — the producer publishes its slot to the
  consumer; the handoff of ownership away from the writer:
  - SPSC: the `producer_idx` Release store;
  - MPSC: the slot's seq Release store.
- **release** — the consumer hands its slot back for
  reuse; the matching handoff in the other direction:
  - SPSC: the `consumer_idx` Release store;
  - MPSC: the slot's seq Release store (`pos + M`).
- **claim** — MPSC only: a producer wins exclusive
  ownership of a slot position by CAS on the shared
  producer index; writing and committing follow. Claim
  order and commit order can differ — see
  [MPSC ring (sibling primitive)](#mpsc-ring-sibling-primitive).

## Requirements

- **no_std** — the crate builds with `#![no_std]` and no
  `alloc`: fixed capacity, operating over a caller-provided
  memory region. Suitable for embedded and kernel-adjacent use.
- **Zero-copy via zerocopy** — messages are structs implementing
  the [zerocopy](https://docs.rs/zerocopy) traits
  (`IntoBytes`, `FromBytes`, `KnownLayout`, `Immutable`); the
  buffer transmutes slots into and out of `&T`/`&mut T` with no
  serialization step and no `unsafe` at the call site.
- **IPC-suitable layout** — the control block (the
  producer/consumer indices) and the slot array live in one
  contiguous region that
  can be mapped into multiple address spaces:
  - `#[repr(C)]` explicit layout, stable across processes
    compiled from the same source.
  - No pointers inside the region — offsets and indices only.
- **SPSC first** — single producer, single consumer, lock-free
  via atomic producer/consumer indices. MPMC is out of scope
  for this cycle;
  the layout should not preclude adding it later.
- **In-place access API** — `reserve_slot_with` on either
  endpoint, then commit (producer) or release (consumer),
  exposing references into the buffer rather than returning
  values by move.
- **Safety** — all `unsafe` encapsulated inside the crate with
  documented invariants. The message path (reserve_slot_with,
  then commit / release) is fully safe; the one `unsafe` public
  entry
  is `Ring::attach` — see [API](#api) for why it cannot be
  made safe.

## Constraints

- **Slot geometry: M slots × N bytes** — the ring holds M
  [slots](#terminology) of N bytes each.
  - M (capacity) is a power of two — index wrapping is a
    mask, no division; checked at construction.
  - N (slot size) is a multiple of the cache-line size, and
    each slot is cache-line aligned — adjacent slots never
    share a line, and any `T` with `align_of::<T>()` up to
    the line size fits without padding logic.
  - This trades space for isolation (a 16-byte message
    occupies a full 64-byte slot) — the right default for
    IPC; a packed-slot variant is a possible follow-on.
- **Monotonic indices** — producer/consumer indices are
  free-running unsigned counters (wrap via masking), so
  full/empty are distinguishable without a separate flag or
  a sacrificial slot.
- **Cache-line separation** — producer- and consumer-owned
  indices are padded/aligned to separate cache lines to avoid
  false sharing.
- **Typed access over untyped slots** — the slot geometry
  (M, N) is a property of the buffer, independent of any
  message type; a `T` is a zerocopy view into a slot, valid
  when `size_of::<T>() <= N` and `align_of::<T>()` divides
  the slot alignment (checked at construction or compile
  time). Variable-length framed messages are a possible
  follow-on, not in this cycle.
- **zerocopy, core-only** — depend on zerocopy with default
  features compatible with `no_std`; no other runtime
  dependencies.

## Memory layout

One contiguous `#[repr(C)]` region, mappable into multiple
address spaces: a `Header` struct followed by the slot array.
The `Header` spans four cache lines; each concurrently-written
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
    // line 0 — geometry; written by init (magic last), then
    // read-only. Cold: per-op paths use handle snapshots.
    magic: AtomicU32,          // layout marker; init/attach handshake
    layout_version: AtomicU32, // bumped on any layout change
    slot_size: AtomicU32,      // N in bytes, cache-line multiple
    capacity: AtomicU32,       // M, power of two; index mask is M - 1
    cache_line_size: AtomicU32, // CACHE_LINE_SIZE the region was built with
    // line 1 — producer-owned
    producer_idx: CacheAligned<AtomicU32>, // written by producer only
    // line 2 — consumer-owned
    consumer_idx: CacheAligned<AtomicU32>, // written by consumer only
    // line 3 — app-owned scratch; zeroed at init, never
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
- **User line last, on purpose** — an app-side overrun walking
  forward out of `user` lands in slot 0 (the app's own message
  data), not in an index line.
- **`repr(align(N))` takes only an integer literal** — it
  cannot name `CACHE_LINE_SIZE`, so the `64` is written out and
  const asserts tie `align_of::<CacheAligned<AtomicU32>>()`
  and `size_of::<Header>()` back to the constant.
- **Every field is atomic** — the region may be mapped by a
  peer at any time, so even "immutable" geometry must be free
  of data races; a peer scribbling on plain fields would be
  UB, on atomics it is merely garbage values. `AtomicU32` has
  the same layout as `u32`, so layout_version is unaffected.
- **Init/attach handshake** — init stores the geometry and
  indices first (`Relaxed`), then `magic` **last** with
  `Release`; attach loads `magic` **first** with `Acquire`. A
  peer that pre-mapped the region and observes MAGIC therefore
  also observes the geometry it validates. (Re-initializing a
  region a peer is already attached to stays outside the
  contract.)
- **Slots** — `M` slots of `N` bytes, each cache-line
  aligned (N is a line multiple, so alignment follows from
  the array base).
- **`CACHE_LINE_SIZE = 64` plays two roles** — a layout-ABI
  constant and a false-sharing pad. On CPUs with 128-byte
  effective line pitch (Apple M-series; Intel's adjacent-line
  prefetcher is why crossbeam pads 128 on x86_64 too) the
  padding under-separates — a perf question for the bench
  work, not correctness. Since layout v2 the header records
  the build's `cache_line_size` and attach rejects a mismatch
  (`BadCacheLine`), so the constant can become per-target
  (cfg) or shrink for cache-less embedded tiers without any
  silent cross-build corruption.
- **Assume nothing beyond `AtomicU32` load/store** — the
  protocol uses no CAS/RMW, so it reaches
  load/store-only targets (e.g. `thumbv6m`/Cortex-M0).
  Features that want CAS (endpoint claims) should degrade or
  gate rather than raise the floor. 8/16-bit targets (AVR,
  MSP430 — no `AtomicU32`) would need index-width
  genericization; out of scope, recorded in Ideas.

Index scheme (resolves the atomic-width question):

- **Fixed-width `AtomicU32`, free-running**:
  - increments are wrapping adds, never masked in storage
  - fixed width keeps the layout identical for 32- and
    64-bit peers sharing the region
- **Masked only at slot access**:
  - slot position = `idx & (M - 1)`
- **Occupancy** = `producer_idx.wrapping_sub(consumer_idx)`
  (valid while `M <= 2^31`):
  - empty: `producer_idx == consumer_idx`
  - full: `producer_idx - consumer_idx == M` — implemented as
    `>= M`, so a peer-corrupted consumer_idx fails toward
    `Full` instead of handing out an unowned slot
  - no sacrificial slot
- **u32 wrap at `2^32` is harmless** because M is a power
  of two:
  - `2^32 % M == 0`, so occupancy math and masked slot
    positions both carry across the wrap unchanged
- **Example** (M = 4, `p` = producer_idx, `c` = consumer_idx):
  - start: `p = 0, c = 0` — occupancy 0, empty
  - producer commits 4 messages: `p = 4, c = 0` — occupancy
    4 == M, full; slot positions written were 0, 1, 2, 3
  - consumer releases 1: `p = 4, c = 1` — occupancy 3, one
    slot free; the next reservation writes slot `4 & 3 == 0`

Ordering: the producer loads `consumer_idx` with `Acquire`
and publishes `producer_idx` with `Release`; the consumer
mirrors (loads `producer_idx` `Acquire`, publishes
`consumer_idx` `Release`).

## API

Construction (resolves the foreign-memory question):

- `Ring::init(region: &'a mut [u8], slot_size: u32, capacity:
  u32) -> Result<Ring<'a>, Error>` — validates geometry
  (power-of-two M, line-multiple N, alignment, region big
  enough — sized in u64, since the N×M product can wrap a
  32-bit usize), then writes the header per the
  [handshake](#memory-layout) (indices zeroed, magic last).
- `unsafe Ring::attach(region: *mut u8, len: usize) ->
  Result<Ring<'a>, Error>` — validates magic (Acquire),
  layout_version, and geometry against the region length,
  over the same shared `header_ptr` checks as init. Unsafe,
  deliberately, on two grounds:
  - zerocopy's shared validated casts require `Immutable`,
    which `Header` correctly is not (atomics) — so the cast
    is hand-verified rather than derive-verified;
  - only the caller can vouch that the pointer is a live,
    genuinely shared, writable mapping (e.g. `MAP_SHARED`)
    and that the SPSC role contract holds across processes.
- `ring.split() -> (Producer<'_>, Consumer<'_>)` — consumes
  the `Ring`, so each handle exists at most once per ring per
  process (SPSC discipline; cross-process there is one
  producer process and one consumer process by contract).
- **Geometry snapshot** — slot_size / capacity / mask are
  copied into `Ring` and the handles at init/attach; per-op
  paths never re-read peer-writable header fields. Combined
  with the all-atomic header, a hostile peer can degrade the
  ring to garbage messages or spurious Full/Empty — never UB.

Producer — reserve_slot_with/commit, in place:

- `producer.reserve_slot_with::<T>(on_full) ->
  Result<WriteSlot<'_, T>, Full>` — next free slot as `&mut T`
  (zerocopy `FromBytes + IntoBytes + KnownLayout`), retrying
  under the injected wait policy — `on_full(attempt) -> bool`,
  `|_| false` for a single non-blocking probe. `T` geometry is
  asserted against the slot size on each call (a mismatch is a
  programming error, so it panics; typed endpoints that check
  once are a follow-on — see [todo.md](todo.md)).
- `WriteSlot::commit(self)` — publishes the slot
  (`producer_idx + 1`, `Release`). Dropping without commit
  abandons the reservation (nothing published).

Consumer — reserve_slot_with/release, in place:

- `consumer.reserve_slot_with::<T>(on_empty) ->
  Result<ReadSlot<'_, T>, Empty>` — oldest unread slot as
  `&T`, retrying under the injected wait policy (`|_| false`
  for a single non-blocking probe).
- `ReadSlot::release(self)` — frees the slot
  (`consumer_idx + 1`, `Release`). Dropping without release
  leaves the slot unread — the next reservation returns it
  again.

Each side reserves at most **one slot at a time**: the guard
holds the endpoint's `&mut` borrow, so a second reservation
while a guard is live is a compile error, not a runtime state.

Both endpoints also expose `user() -> &[AtomicU32; 16]` — the
header's app-owned scratch line; see
[Blocking and user words](#blocking-and-user-words).

Guard internals: both guards hold **raw slot pointers** and
mint `&T` / `&mut T` per access via Deref. A reference *field*
would be argument-protected (noalias) for the entire
`commit`/`release` call, but the store inside that call hands
the slot to the other side, which may access it while the
protector is still live — an aliasing race Miri flags
(found in 0.3.0-6 by the threaded test).

Batch release (resolves the batching question): deferred —
the guard API leaves room for a `release_n(n)` later; the
single-slot guard is the whole surface this cycle.

## Blocking and user words

The ring's fallible core never blocks: `reserve_slot_with`
returning [`Full`]/[`Empty`] under a give-up policy (`|_| false`)
is the primitive. *How* to wait on it — spin, yield, park,
await — is injected policy: the crate ships [`policy::spin`]
for a pure busy-wait, but anything that actually sleeps
belongs to the layer above (an embedded caller
may WFE on an interrupt, an async runtime wants a waker, a
pinned busy-poll thread wants a pure spin; no single
crate-blessed hybrid fits all three).

What the crate provides is mechanism: the header's `user`
line — 16 `AtomicU32` words, zeroed by `Ring::init`, exposed
via `user()` on both endpoints, and never read, written, or
interpreted by the crate afterwards. It is the well-known
shared-memory home where independently written peers can
build a wakeup protocol (waiter flags, futex words, sequence
numbers). `AtomicU32` deliberately: futex-shaped, portable to
targets without 64-bit atomics, race-free under concurrent
peer access.

Contracts and cautions for that layer:

- **Lost-wakeup discipline** — the sleep side must re-check
  the ring *after* publishing its "wake me" flag and before
  sleeping (`set flag → reserve_slot_with once more → sleep`), and
  the wake side checks the flag *after* its commit/release
  (`commit → check flag → wake`). Skipping the re-check races
  a commit landing between flag-set and sleep: the waker saw
  no flag, the sleeper saw no message — it sleeps forever.
  futex-style primitives exist precisely to make the final
  check-and-sleep atomic.
- **Values, not addresses** — a pointer stored in the region
  is meaningless in the peer's address space (and a
  dereference of peer-writable bytes besides). Store data;
  keep pointers on your side of the wall.
- **Timeouts** — a peer can die while you sleep; every wait
  should carry a timeout so a dead producer degrades the
  consumer to periodic polling, not a hang.

## Validation

Per-commit, alongside the cargo cycle:

- `cargo +nightly miri test` — the full suite, threaded test
  included (its message count is reduced under `cfg(miri)`;
  interpreted spin loops are slow). Miri has caught two real
  Stacked Borrows violations: the init retag (0.3.0-4) and
  the guard argument-protector race (0.3.0-6). The latter
  reproduces only when the threaded test runs, so the full
  suite — not a non-threaded subset — stays in the ladder.
- `indices_survive_u32_wrap` pins the free-running-index wrap
  argument (fill/drain across `u32::MAX` with masked
  positions intact).
- [loom](https://docs.rs/loom) for exhaustive ordering
  exploration is a possible follow-on (Ideas).

## MPSC ring (sibling primitive)

Design for the multi-producer single-consumer ring — a
sibling type next to the SPSC ring, not a mode of it. The
SPSC hot path is one Relaxed load + one Acquire load + one
Release store per side; any unified type would put at least a
per-slot sequence check into that path. So the primitives
stay separate, existing SPSC users pay nothing, and iiac-perf
measures the two directly against each other. Anticipated as
a one-bullet placeholder in
[Pool topology and phasing](#pool-topology-and-phasing); this
section is the full design.

### MPSC protocol

Vyukov's bounded MPMC queue restricted to many-producer /
one-consumer: producers CAS-claim `producer_idx`; a per-slot
sequence word publishes each slot independently. The seq
exists because claim order and commit order differ under
concurrency — the consumer must observe commits, not claims.

- **Sequence array** — `M × AtomicU32`, `seq[i]` initialized
  to `i`. Slot at free-running position `pos` (masked at
  access, as SPSC) is:
  - claimable when `seq == pos`,
  - committed when `seq == pos + 1`,
  - released by the consumer storing `seq = pos + M`.
- **Producer claim** — load `producer_idx` (Relaxed), load
  that slot's seq (Acquire), branch on the wrapping-signed
  diff `seq.wrapping_sub(pos) as i32`:
  - `== 0` — claimable:
    - `compare_exchange_weak(pos, pos + 1)` on
      `producer_idx`; success claims the slot exclusively.
    - Write the payload in place.
    - `seq.store(pos + 1, Release)` commits.
  - `< 0` — full:
    - The slot's previous occupant is not yet released.
    - The `on_full` policy decides retry vs give-up.
  - `> 0` — lost the race to another producer:
    - Reload `producer_idx` and retry.
    - Not a policy call: the system made progress, only
      this producer fell behind.
- **Consumer** — single, as in SPSC, so its side needs no
  CAS:
  - wait for `seq == c + 1` (Acquire),
  - read the slot,
  - store `seq = c + M` (Release),
  - advance the private index.
- **Producers never read `consumer_idx`**:
  - fullness falls out of the slot seq;
  - the header's consumer index line is diagnostic
    occupancy only.
- **Free-running u32 + power-of-two M** carry over
  unchanged: `2^32 % M == 0` keeps seq arithmetic and masked
  positions wrap-safe.
- **Atomic floor** — the claim CAS raises the floor for MPSC
  users only:
  - the module is gated on `target_has_atomic = "32"`;
  - the SPSC ring stays load/store-only (thumbv6m keeps
    working).

### MPSC API: closure send

The producer-side guard is replaced by a closure, because in
MPSC an abandoned *claim* wedges the queue — the consumer
waits at that position forever — where SPSC's dropped guard
was free (`producer_idx` never moved). Abandonment is made
unrepresentable rather than handled:

- `MpscRing::init(region, slot_size, capacity)` /
  `unsafe attach` — mirror the SPSC constructors:
  - own magic and layout_version;
  - cross-attaching a region of the wrong kind fails
    toward `BadMagic`.
- `split() -> (MpscProducer, MpscConsumer)` with
  `MpscProducer: Clone + Send`:
  - one handle per producing thread;
  - cross-process producers attach;
  - the endpoint claims word models role slots, not a
    single producer bit, so N producer claims fit —
    already noted in its todo.
- `producer.send_with::<T>(on_full, fill)` →
  `Result<(), Full>`:
  - `fill: impl FnOnce(&mut T)` writes the payload in
    place — claim, fill, commit on closure return;
  - commit-by-construction — no public abandonment state;
  - `on_full(attempt) -> bool` is the same wait-policy
    seam as SPSC (`|_| false` = single probe).
- `consumer.reserve_slot_with::<T>(on_empty)` →
  `ReadSlot` — the SPSC consumer guard survives:
  - consumer abandonment is harmless (drop without
    release re-delivers the same slot);
  - so the guard shape stays.
- **Unwind path** — a panic in `fill` must not wedge the
  queue:
  - the unwind runs an internal guard's Drop, which
    tombstones the claimed slot — a marked commit the
    consumer releases without delivering;
  - sketch: a reserved seq bit masked out of the index
    arithmetic (constrains `M <= 2^30`); encoding is an
    open question below;
  - only unwinding reaches it — panic=abort targets never
    do.

### Overflow readiness

The sender-private overflow FIFO
([Overflow FIFO (future)](#overflow-fifo-future)) must
compose with MPSC when it lands. Two properties keep that
true; both are named requirements so a later protocol tweak
cannot silently break them:

- **`Full` is a zero-footprint failure**:
  - a send returning `Full` leaves no claim, no tombstone,
    no protocol residue;
  - the protocol gives this by construction — fullness is
    observed in the slot seq *before* any CAS, and a lost
    CAS is also stateless — but it is a requirement, not
    an accident.
- **`Full` is the overflow seam**:
  - the future MPSC DescSender probes with `|_| false` and
    on `Err(Full)` appends the buffer to its own pending
    list;
  - pending lists stay per-sender — no shared list, no
    CAS;
  - per-sender draining preserves exactly the per-producer
    FIFO order MPSC promises anyway — inter-producer order
    was never promised.

### MPSC trust

Same posture as the SPSC ring: every shared word is
untrusted, corruption degrades service and never causes UB.

- The seq array joins the untrusted set:
  - positions are always masked, so scribbled seqs yield
    spurious Full/Empty or garbage-but-valid `T`s
    (zerocopy guarantees representation, not sense);
  - never out-of-bounds access.
- A hostile *producer* peer is new surface relative to
  SPSC:
  - it can claim and never commit — wedging the queue, a
    liveness loss;
  - or commit garbage;
  - denial of service stays possible, as it always was —
    memory safety does not depend on peers.

### Fan-in (composition, not a mode)

The other standard many-to-one shape: one SPSC ring per
producer, the consumer polls N endpoints. Both have a place:

- the shared MPSC ring:
  - arrival-order fairness ("fair FIFO");
  - one place to poll.
- fan-in:
  - stays load/store-only;
  - zero producer-to-producer interference;
  - the consumer chooses the service policy (priority,
    round-robin, weighted, …).

Fan-in is buildable today from shipped SPSC parts; likely
both shapes get offered eventually (a fan-in helper as a
later convenience — no commitment yet). The bench matrix
measures both from the start.

### MPSC measurement plan

iiac-perf (sibling repo) grows an mpsc set alongside `zcr`:

- `zcr-mpsc-1t` — same-thread round-trip: pure protocol
  overhead (CAS + seq vs load/store), against
  `zcr-with-1t`.
- `zcr-mpsc-2t` — 1 producer / 1 consumer cross-core: what
  MPSC costs when you don't need it, against `zcr-with-2t`.
- `zcr-mpsc-3t` / `-5t` — 2 / 4 producers, one consumer:
  claim-contention scaling, the numbers SPSC cannot
  produce.
- Fan-in comparator at the same producer counts (N SPSC
  rings, round-robin poll).
- We think the shared ring:
  - loses to fan-in on raw throughput under producer
    contention (CAS retries vs disjoint cache lines);
  - wins on consumer-side latency and fairness at larger
    N.
- The matrix exists to check that, not assume it.

### MPSC open questions

- **Seq array padding**:
  - packed `M × 4` bytes — Vyukov's shape; adjacent-slot
    false sharing;
  - line-padded — `M × 64` bytes of overhead;
  - start packed, and the bench matrix decides whether a
    padded variant earns a layout flag.
- **Tombstone encoding**:
  - reserved seq bit — masked arithmetic, `M <= 2^30`;
  - or a per-slot side word;
  - only the unwind path needs it, so the simplest
    correct encoding wins.
- **Header consumer index**:
  - keep as diagnostic occupancy (Relaxed stores by the
    consumer), or drop the line entirely;
  - the `occupancy()` polish todo argues for keeping it.

## Messaging layer: pools and descriptor queues

Design for the layer above the ring. The pool half is
as-built as of the 0.6.0 cycle (`src/pool.rs`: layout,
init/attach, alloc/free); descriptor queues and provenance
remain design. The sections above are the as-built record of
the ring itself.

The ring's reserve (`reserve_slot_with`) fuses three acts that
a messaging system needs separated: it allocates message
memory (the slot),
assigns queue position (the reservation is the ring head), and
opens a publication window (the guard exclusively borrows the
endpoint, blocking all other sends). Getting a message must not
imply sending it now — or ever. The fix is a layer, not a ring
rework: pools own message memory, queues carry small
descriptors, and the existing ring is the unchanged primitive
underneath (reserve-to-commit becomes a momentary act, where
the fusion is harmless).

### System model (overview)

The layer's rules at a glance; each bullet links to the
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
- Each process keeps its own pool registry
  [details](#descriptor-and-registry-design-070).
  - It maps pool ids to that process's view (mapping) of
    each pool.
  - Registries are private and never shared; only
    descriptors cross process boundaries.
  - Pool ids are the shared vocabulary: every participating
    process must register a pool under the same id (the
    assignment scheme is an open question —
    [details](#pool-id-allocation)).
- Ownership follows the descriptor
  [details](#usage-model-roles-and-buffer-lifecycle).
  - alloc → write → send the descriptor over a queue →
    the receiver owns it → read, forward over another
    queue, or free.
  - A buffer is always in exactly one state with one
    permitted toucher.
- Any holder — any thread or process — may free
  [details](#free-path-intrusive-lifo-free-stack).
  - The free pushes the buffer directly onto its pool's
    free-stack in shared memory — no round-trip through
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

- **Decoupled get/hold/send/free** — allocate a message, hold
  it indefinitely, send it over any queue later, forward it,
  or free it unsent.
- **Any quantity of queues and pools** in a system, and any
  message can travel over any queue.
- **Inter- and intra-process** — processes share queue ends,
  and message payloads zero-copy across process boundaries.
  The embedded single-address-space case is the degenerate
  mapping where all peers share one base.
- **Heterogeneous pools** — pools differ arbitrarily in buffer
  size and count; each meets the alignment constraints and
  self-describes its geometry.
- **Phasing** — queues are SPSC rings now, an MPSC ring is a
  future sibling primitive; pools have a single allocator
  now, shared allocation eventually.

### Provenance and descriptors

How a message's origin travels and is resolved — one
sub-topic per heading, each directly linkable.

#### Offsets only, everywhere

Different per-process mappings mean no pointers in any
shared structure; this also serves the single-address-space
case unchanged.

#### Message provenance

Every buffer begins with a small header naming its pool
`(pool id, offset)`; whoever holds a message last must free
it to the right pool, so provenance travels with the
message. (Deferred as of 0.7.0 — provenance travels in the
descriptor instead; see
[Descriptor and registry design](#descriptor-and-registry-design-070).)

#### Descriptors

A queue entry is a uniform small `(pool id, buffer index)`
regardless of message size; the receiver resolves the pool
id and learns geometry from the pool's own header (index
over byte offset settled in
[Descriptor and registry design](#descriptor-and-registry-design-070)).

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

The in-process slice as designed for the 0.7.0 cycle;
cross-process setup (mapping exchange, pool-id
coordination) remains in [Open questions](#open-questions).

- **`Desc { pool_id: u32, buf_idx: u32 }`** — 8 bytes, POD
  (zerocopy derives), so it rides any queue as an ordinary
  message. A buffer *index*, not a byte offset: the
  offsets-only rule bars pointers (per-process mappings),
  and an index is equally position-independent while
  matching what the free-stack and guards already speak;
  validation is one bounds check where a byte offset would
  also need a buffer-boundary divisibility check.
- **`PoolRegistry`** — per-process, fixed capacity
  (const-generic array; no_std, zero allocation).
  `register(resolver) -> PoolId` assigns the next slot
  index; phase 1 has no unregister, so ids never dangle.
  Sequential assignment produces cross-process id agreement
  only when one process assigns all ids — the in-process
  slice's case; cross-process will need registration under
  an externally agreed id (e.g. `register_at(id, ...)`),
  pending [Pool-id allocation](#pool-id-allocation).
- **`Pool::resolver() -> PoolResolver`** — a non-allocating
  view (header ref, buffer base, geometry) derived from the
  existing handle, so no second region borrow and no
  Stacked Borrows retag hazard (`init` takes the region
  pointer exactly once). Send + Sync: resolving mints
  guards from validated indices, and the only
  shared-memory mutation is the guards' free CAS, already
  any-thread. Cross-process later, the same view derives
  from an `attach`ed handle.
- **`into_desc(slot, pool_id)`** — safe, O(1): checks the
  guard's header address against the id's registry entry
  (catches id/pool mispairing); consumes the guard, and
  ownership travels on in the descriptor (the usage
  model's "in-flight" state). The error side hands the
  guard back, so a miss cannot leak the buffer.
- **`unsafe resolve::<T>(desc) -> Result<BufSlot<T>, _>`**
  — every failure is an `Err`, never a panic: unknown pool
  id, index out of range, `T` geometry mismatch. The
  geometry case differs from `alloc` (which panics)
  because the descriptor selects which pool gets compared
  — untrusted input must not select a panic. `unsafe`
  covers the one thing validation cannot check —
  ownership: the caller promises the desc came from
  `into_desc`, arrived over a channel establishing
  happens-before (ring commit → reserve qualifies), and is
  resolved exactly once.
- **`Desc` is plain data on purpose** — `FromBytes` means a
  receiver mints one from shared bytes anyway, so a
  move-only ownership token would be theater; the
  discipline lives in resolve's contract.
- **In-buffer provenance deferred** — the descriptor is the
  message's travel form (forwarding re-sends it), so no
  flow carries a buffer without its provenance; adding an
  in-buffer header later is a pool `layout_version` bump.
- **Type dispatch is payload-level** — the descriptor stays
  type-agnostic. Multiple message types over one queue
  need a receiver-readable tag: a first-word id driving a
  match (demo-minimal), `TryFromBytes` tagged enums, or a
  future `Message` trait hiding the cast boilerplate (see
  todo Ideas).
- **Single-address-space profile (future)** — with no MMU
  and one trust domain, a descriptor could carry pointers
  (a flattened guard): zero-lookup resolve, but nothing to
  validate against — a scribbled pointer descriptor is UB
  on use, where id + index degrades to an `Err`. The
  canonical form stays `(pool id, buf idx)`; a pointer
  profile could arrive later behind the same trait seam as
  other usage styles.

### Free path: intrusive LIFO free-stack

Freeing must be cheap for many freers and allocation must be
O(1) with no searching (a per-buffer available-flag makes free
trivial but alloc a scan). The free operation is itself the
marking: push the buffer onto the pool's free-stack.

- **Intrusive** — a freed buffer's header holds the next-link
  as an offset; the free-list needs no extra storage.
- **LIFO on purpose** — we think a stack beats a free-ring on
  cache behavior: FIFO cycles through every buffer (maximum
  working set), LIFO reuses the most-recently-freed few, and
  the small working set also helps the TLB. The full
  same-thread malloc-style hot-reuse win is diluted because
  the freer and allocator are different cores; the
  working-set effect stands regardless.
- **Single-popper Treiber stack** — free = CAS the head to
  your buffer's offset (MPSC push side: any thread or process
  frees into any pool); alloc = the one owning allocator pops
  the head. A single popper eliminates classic Treiber ABA:
  no node leaves the list under a competing pop, pushes just
  retry the CAS.
- **Validated pops** — head and next-links are untrusted;
  the popper bounds/alignment-checks each offset and caps
  pops per attempt, so a corrupt peer can lose buffers or
  cycle the list but never make the allocator read outside
  the pool.
- **CAS note** — the free-stack is the first structure to
  need CAS; the ring protocol itself stays load/store-only,
  so the atomic floor rises only for pool users.

### Pool topology and phasing

- **One owning allocator per pool** (invariant, phase 1) — a
  single thread allocates; any party frees. Per-thread pools
  are the performance default and satisfy this trivially.
- **Shared pools already exist in phase 1** — shared for
  reading, forwarding, and freeing; only allocation is
  single-owner.
- **Shared allocation (phase 2)** — per-thread pools cost
  memory; N threads sharing one pool's capacity needs
  multi-popper alloc, which needs an ABA-proof head:
  pack `(offset, generation)` in one `AtomicU64` and CAS
  both together. A contained per-pool variant (opt-in flag),
  not a redesign; hot paths keep the cheap single-popper
  version. Heterogeneity mitigates meanwhile: a small private
  hot-path pool plus a big shared fallback pool.
- **MPSC ring (future sibling)** — multi-producer queues need
  a different ring protocol (CAS-claimed producer index,
  per-slot sequence state); it slots in as a sibling
  primitive under the same descriptor layer. The endpoint
  claims word should model role slots, not a single producer
  bit, so N producer claims fit later without a layout
  rethink. Full design:
  [MPSC ring (sibling primitive)](#mpsc-ring-sibling-primitive).

### Usage model: roles and buffer lifecycle

The user-facing contract — who may do what, per object and
per buffer state — lives in the README:
[Usage model](../README.md#usage-model-roles-and-buffer-lifecycle).
The tests exercise exactly what it permits. Summary: ring =
one producer + one consumer; pool = one allocator + any
freers; a buffer is always in exactly one state
(free / allocated / in-flight [vacant until descriptor
queues] / freed) with one permitted toucher; "send" this
cycle means moving the `BufSlot`.

### Overflow FIFO (future)

When a queue's ring is Full, the sender appends the message to
a pending FIFO instead of failing — "send" then always
succeeds while pool memory lasts.

- **Intrusive, zero-allocation** — the FIFO links through the
  same embedded next-link offset the free-stack uses. A
  buffer is on at most one list at a time (free stack,
  pending FIFO, in flight in a ring, held by an owner), so
  one link field serves every state.
- **Sender-private** — the pending list belongs to the
  producer endpoint: head + tail offsets for O(1) append, no
  shared mutation, no CAS. Order discipline: the producer
  drains the FIFO oldest-first before ring-sending anything
  new, or FIFO order breaks. Draining happens on subsequent
  send attempts and/or an explicit flush.
- **Validated traversal** — the links live in shared pool
  memory a peer can scribble; the drainer bounds- and
  alignment-checks each offset as the free-stack popper does.
- **Naturally bounded** — messages come from pools, so the
  FIFO cannot outgrow total pool capacity; backpressure
  reappears as allocation failure rather than queue Full.

### Prior art: iceoryx2

[iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) is a
Rust-native zero-copy IPC library implementing most of this
layer: publishers loan uninitialized samples from a pool
allocator, write in place, and send later or never (get/send
decoupled); receivers get offsets they resolve against their
own mappings; events, request-response, and multi-producer
patterns exist. Study its loan/send API and pool-offset
machinery before implementing the pools — its lessons are
cheaper stolen than rediscovered.

What keeps this project distinct from it:

- iceoryx2 is service-oriented middleware (discovery, naming,
  configuration) for Linux/macOS/Windows/QNX-class platforms;
  this crate is a small set of composable primitives.
- No `no_std` / load-store-only floor — our ring reaches
  `thumbv6m`, with CAS confined to the pool layer.
- Trust posture: our attach-validated
  hostile-peer-cannot-cause-UB discipline versus cooperating
  participants within a framework.

### Open questions

One question per heading, each directly linkable; a
resolved question migrates to
[Resolved questions](#resolved-questions), keeping its
heading (and so its anchor).

#### Pool-id allocation

In-process settled in 0.7.0 (registry slot index, no
unregister); cross-process remains open:
coordinator-assigned versus derived from the shm object's
identity.

#### Setup plane

How a process discovers and maps a pool it hasn't seen:
pre-arranged at init (embedded-friendly, allocation-free
after startup) versus fd-passing over a Unix socket
(Linux-friendly, dynamic); likely both, as profiles.

#### Message header shape

0.7.0 settled the near half: provenance travels in the
descriptor, a type tag is payload-level, and the in-buffer
header is deferred (see
[Descriptor and registry design](#descriptor-and-registry-design-070)).
Still open: whether a length (or anything else) joins a
future in-buffer header, and its layout against the
embedded next-link — the link is load-bearing for three
states (free-stack, pending FIFO, future lists), so they
must be laid out together when that header lands.

## Resolved questions

- **Cross-process trust** — resolved in 0.3.0-4: the
  all-atomic header plus the geometry snapshot mean a hostile
  peer cannot cause UB, only garbage messages (still valid
  `T`s — zerocopy guarantees representation, not sense) or
  spurious Full/Empty. `attach` remains `unsafe` for the
  mapping-liveness contract, not for peer trust.
- **`split()` once-guard** — direction chosen: an
  endpoint-claims word in the header (CAS to claim the
  producer / consumer role, second claimant gets an error),
  which also catches in-process double-attach; costs a
  layout_version bump (or spends `_pad0`). Promoted to a
  `## Todo` entry in [todo.md](todo.md); the documented
  contract stands until then.
