# Ring buffer design

Design notes for the zero-copy ring buffer. This file uses
[Prose form](../AGENTS.md#prose-form). This step covers
requirements and constraints; the API and memory-layout design
follows in the next cycle step.

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
- **IPC-suitable layout** — the control block (head/tail
  indices) and the slot array live in one contiguous region that
  can be mapped into multiple address spaces:
  - `#[repr(C)]` explicit layout, stable across processes
    compiled from the same source.
  - No pointers inside the region — offsets and indices only.
- **SPSC first** — single producer, single consumer, lock-free
  via atomic head/tail. MPMC is out of scope for this cycle;
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
- **Monotonic indices** — head/tail are free-running unsigned
  counters (wrap via masking), so full/empty are
  distinguishable without a separate flag or a sacrificial
  slot.
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

## Open questions

- Atomic width for indices: `AtomicU32` vs `AtomicUsize` —
  cross-process mapping between 32/64-bit peers argues for a
  fixed-width `AtomicU32`. Decide in the API/layout step.
- Whether the consumer releases slots one-at-a-time or in
  batches (batch release amortizes the atomic store).
- How construction over foreign memory (an mmap'd region) is
  expressed safely — likely a `from_bytes`-style validated
  cast via zerocopy's `KnownLayout`.
