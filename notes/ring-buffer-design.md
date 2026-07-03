# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../AGENTS.md#prose-form). It covers requirements,
constraints, the memory layout, the API, and the validation
ladder, and is kept in sync with the implementation in
`src/` (as-built): `lib.rs` holds the region-level machinery
(header, init/attach/split, geometry helpers) and re-exports
the endpoint modules `producer.rs` (Producer, WriteSlot,
Full) and `consumer.rs` (Consumer, ReadSlot, Empty).

## Goal

A ring buffer for passing typed messages between a producer and
a consumer with no copying at the boundary: the producer writes
its message directly into buffer memory, the consumer reads it
in place. The same layout must work intra-process (threads) and
inter-process (shared memory).

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
- **In-place access API** — `reserve_slot` on either endpoint,
  then commit (producer) or release (consumer), exposing
  references into the buffer rather than returning values by
  move.
- **Safety** — all `unsafe` encapsulated inside the crate with
  documented invariants. The message path (reserve_slot, then
  commit / release) is fully safe; the one `unsafe` public
  entry
  is `Ring::attach` — see [API](#api) for why it cannot be
  made safe.

## Constraints

- **Slot geometry: M slots × N bytes** — a *slot* is one
  fixed-size buffer in the ring; the ring holds M of them.
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
    cache_line: AtomicU32,     // CACHE_LINE the region was built with
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
  cannot name `CACHE_LINE`, so the `64` is written out and
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
- **`CACHE_LINE = 64` plays two roles** — a layout-ABI
  constant and a false-sharing pad. On CPUs with 128-byte
  effective line pitch (Apple M-series; Intel's adjacent-line
  prefetcher is why crossbeam pads 128 on x86_64 too) the
  padding under-separates — a perf question for the bench
  work, not correctness. Since layout v2 the header records
  the build's `cache_line` and attach rejects a mismatch
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
    slot free; the next reserve_slot writes slot `4 & 3 == 0`

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

Producer — reserve_slot/commit, in place:

- `producer.reserve_slot::<T>() ->
  Result<WriteSlot<'_, T>, Full>` — next free slot as `&mut T`
  (zerocopy `FromBytes + IntoBytes + KnownLayout`); `T`
  geometry is asserted against the slot size on each call (a
  mismatch is a programming error, so it panics; typed
  endpoints that check once are a follow-on — see
  [todo.md](todo.md)).
- `WriteSlot::commit(self)` — publishes the slot
  (`producer_idx + 1`, `Release`). Dropping without commit
  abandons the reservation (nothing published).

Consumer — reserve_slot/release, in place:

- `consumer.reserve_slot::<T>() ->
  Result<ReadSlot<'_, T>, Empty>` — oldest unread slot as
  `&T`.
- `ReadSlot::release(self)` — frees the slot
  (`consumer_idx + 1`, `Release`). Dropping without release
  leaves the slot unread — the next reserve_slot returns it
  again.

Each side reserves at most **one slot at a time**: the guard
holds the endpoint's `&mut` borrow, so a second `reserve_slot`
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

The crate never blocks and never spins. `reserve_slot`
returning [`Full`]/[`Empty`] immediately is the entire API;
what to do about it — spin, yield, park, await — is policy,
and policy belongs to the layer above (an embedded caller may
WFE on an interrupt, an async runtime wants a waker, a pinned
busy-poll thread wants a pure spin; no single crate-blessed
hybrid fits all three).

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
  sleeping (`set flag → reserve_slot once more → sleep`), and
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
