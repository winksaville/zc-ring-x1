# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../AGENTS.md#prose-form). It covers requirements,
constraints, the memory layout, and the API sketch; the
prototype crate validates it in the next cycle step.

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
  documented invariants; public API is safe.

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
    // line 0 — immutable after init
    magic: u32,          // layout marker, rejects foreign regions
    layout_version: u32, // bumped on any layout change
    slot_size: u32,      // N in bytes, cache-line multiple
    capacity: u32,       // M, power of two; index mask is M - 1
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
  `size_of::<Header>()` is the slot-array offset, and a
  single zerocopy derive covers validation.
- **Slots** — `M` slots of `N` bytes, each cache-line
  aligned (N is a line multiple, so alignment follows from
  the array base).

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
  - full: `producer_idx - consumer_idx == M`
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

## API sketch

Construction (resolves the foreign-memory question):

- `Ring::init(region: &mut [u8], slot_size: u32, capacity:
  u32) -> Result<&Ring, Error>` — writes the header, zeroes
  indices; validates geometry (power-of-two M, line-multiple
  N, region big enough, alignment).
- `Ring::attach(region: &[u8]) -> Result<&Ring, Error>` —
  validated cast of an already-initialized region (an mmap'd
  segment): checks magic, layout_version, geometry against
  the region length. Built on zerocopy's `KnownLayout`
  validated reference casts — no `unsafe` at the call site.
- `ring.split() -> (Producer<'_>, Consumer<'_>)` — the two
  endpoint handles; each is `Send + !Sync` and exists at
  most once per ring per process (SPSC discipline;
  cross-process there is one producer process and one
  consumer process by contract).

Producer — reserve/commit, in place:

- `producer.reserve::<T>() -> Result<Reserved<'_, T>, Full>`
  — next free slot as `&mut T` (zerocopy `FromBytes +
  IntoBytes + KnownLayout`); `T` geometry checked against
  `slot_size` once.
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

## Open questions

- Cross-process trust: `attach` validates the header, but a
  hostile peer can scribble on the region afterward; the
  consumer-side typed view means a torn/garbage message is
  still *some* valid `T` (zerocopy guarantees no UB, not
  semantic sense). Document as: the ring is safe against UB,
  not against a malicious peer.
- Whether `split()` needs a runtime once-guard in the header
  (endpoint-claimed flags) or stays a documented contract.
