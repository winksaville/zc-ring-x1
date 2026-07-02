# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../AGENTS.md#prose-form). It covers requirements,
constraints, the memory layout, the API, and the validation
ladder, and is kept in sync with the implementation in
`src/lib.rs` (as-built).

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
- **In-place access API** — reserve/commit on the producer side
  and peek/release on the consumer side, exposing references
  into the buffer rather than returning values by move.
- **Safety** — all `unsafe` encapsulated inside the crate with
  documented invariants. The message path (reserve/commit,
  peek/release) is fully safe; the one `unsafe` public entry
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
The `Header` is one struct spanning three cache lines, so each
concurrently-written index owns its own line:

```rust
#[repr(C, align(64))]
struct Header {
    // line 0 — geometry; written by init (magic last), then
    // read-only
    magic: AtomicU32,          // layout marker; init/attach handshake
    layout_version: AtomicU32, // bumped on any layout change
    slot_size: AtomicU32,      // N in bytes, cache-line multiple
    capacity: AtomicU32,       // M, power of two; index mask is M - 1
    _pad0: [u8; 48],
    // line 1 — producer-owned
    producer_idx: AtomicU32, // written by producer only
    _pad1: [u8; 60],
    // line 2 — consumer-owned
    consumer_idx: AtomicU32, // written by consumer only
    _pad2: [u8; 60],
}
```

```
offset 0                    Header (192 bytes, 3 cache lines)
offset size_of::<Header>()  slots: M × N bytes
```

- One type owns the whole control-block layout:
  `size_of::<Header>()` is the slot-array offset.
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
  constant (baked into layout_version 1, frozen) and a
  false-sharing pad. On CPUs with 128-byte effective line
  pitch (e.g. some aarch64) the padding under-separates; a
  future layout_version could widen it without touching the
  ABI role.

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
    slot free; the next reserve writes slot `4 & 3 == 0`

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

Producer — reserve/commit, in place:

- `producer.reserve::<T>() -> Result<Reserved<'_, T>, Full>`
  — next free slot as `&mut T` (zerocopy `FromBytes +
  IntoBytes + KnownLayout`); `T` geometry is asserted against
  the slot size on each call (a mismatch is a programming
  error, so it panics; typed endpoints that check once are a
  follow-on — see [todo.md](todo.md)).
- `Reserved::commit(self)` — publishes the slot
  (`producer_idx + 1`, `Release`). Dropping without commit
  abandons the reservation (nothing published).

Consumer — peek/release, in place:

- `consumer.peek::<T>() -> Result<Peeked<'_, T>, Empty>` —
  oldest unread slot as `&T`.
- `Peeked::release(self)` — frees the slot
  (`consumer_idx + 1`, `Release`). Dropping without release
  leaves the slot unread — the next peek returns it again.

Batch release (resolves the batching question): deferred —
the guard API leaves room for a `release_n(n)` later; the
single-slot guard is the whole surface this cycle.

## Validation

Per-commit, alongside the cargo cycle:

- `cargo +nightly miri test -- --skip threaded` — Miri caught
  a real Stacked Borrows violation in `init` (0.3.0-4); the
  non-threaded suite runs clean and stays in the ladder. The
  threaded stress test is skipped (100k spin-loop iterations
  under an interpreter).
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
