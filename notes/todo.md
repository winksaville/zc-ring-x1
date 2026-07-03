# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

**feat: message pool allocator**

Todo #1 picked up: decouple "get a message" from "send it"
with a message pool — self-describing attach-validated
region, fixed-size cache-aligned buffers, intrusive LIFO
free-stack (single-popper Treiber, validated pops), one
owning allocator, anyone frees. Strive for simplicity: the
smallest API that lets us hone the shape and gives iiac-perf
a baseline —

- buffer header is the next-link alone for now; provenance
  (pool id) and any length/type tag land with the
  descriptor-queue cycle, via a pool layout_version bump
  (conscious deferral of the design doc's settle-early note).
- API mirrors the ring: `Pool::init` / `unsafe Pool::attach`,
  a single allocator handle, typed zerocopy access to
  buffers.

Ladder:

- 0.6.0-0 chore: open message pool cycle (done)
- 0.6.0-1 feat: message pool layout + init/attach (done)
- 0.6.0-2 feat: message pool alloc/free (done)
- 0.6.0-3 docs: message pool usage model — roles (ring:
  one producer + one consumer; pool: one allocator + any
  freers), buffer lifecycle states (free → allocated →
  in-flight → freed) and who may touch a buffer in each,
  what "send" means before descriptor queues exist
- 0.6.0-4 test: message pool protocol tests
- 0.6.0 feat: message pool allocator (close-out)

## Todo

 Entries are in **strict priority rank** — #1 highest,
 descending. Reprioritize by moving an entry, then
 `vc-x1 fix-todo --no-dry-run notes/todo.md` to renumber.
 The numbers are positional rank, not stable IDs — to refer
 to a Todo, name it by its **title** (a greppable mention;
 a numbered list item has no anchor to link to), not its
 number. Long-tail entries
 live in [todo-backlog.md](todo-backlog.md). Use the
 [Prose Form in AGENTS.md](../AGENTS.md#prose-form); deeper
 detail goes in `notes/chores/chores-NN.md` design
 subsections (link via `[N]` ref).

1. Descriptor queues: carry `(pool id, buffer offset)`
   descriptors over the existing SPSC ring so any message
   travels over any queue; per-process pool registry to
   resolve ids
   [details](ring-buffer-design.md#messaging-layer-pools-and-descriptor-queues).
2. Endpoint claims word: CAS-claimed producer/consumer roles
   in the ring header so a second attach/split claimant gets
   an error instead of silently violating SPSC; costs a
   layout_version bump (or spends `_pad0`)
   [details](ring-buffer-design.md#resolved-questions).
3. Typed endpoints: `Producer<T>` / `Consumer<T>` validating
   `T`'s geometry once at split instead of asserting on every
   reserve_slot [details](ring-buffer-design.md#api).

## Ideas

- Perf benches live in
  [iiac-perf](https://github.com/winksaville/iiac-perf)
  (sibling repo `../iiac-perf`), not here — its calibrated
  harness compares zc-ring against mpsc et al. directly
  (`zcring-1t`/`zcring-2t` mirroring `mpsc_1t`/`mpsc_2t`).
  An in-repo bench only if per-commit regression tracking
  proves necessary.

- Study [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2)
  before implementing message pools — battle-tested loan/send
  decoupling and pool-offset machinery; how it differs from
  this project in
  [Prior art: iceoryx2](ring-buffer-design.md#prior-art-iceoryx2).
- Overflow FIFO: when a queue's ring is Full, append the
  message to a sender-private intrusive pending list (same
  embedded next-link the free-stack uses; zero allocation,
  naturally bounded by pool capacity)
  [details](ring-buffer-design.md#overflow-fifo-future).
- Blocking layer above the crate (futex, eventfd, async
  wakers) built on the header's user line — mechanism and
  contracts in
  [Blocking and user words](ring-buffer-design.md#blocking-and-user-words);
  possibly a companion wrapper crate so independent peers
  share one protocol.
- loom-based exhaustive ordering exploration of the SPSC
  protocol.
- Polish: `Error` implements `Display` + `core::error::Error`;
  `occupancy()` / `is_empty()` accessors.
- Packed-slot variant (drop the cache-line-multiple slot
  constraint) for small-message space efficiency.
- Per-target / configurable `CACHE_LINE` (128 for Apple
  M-series false sharing, tiny for cache-less MCUs) — safe
  since attach validates the header's `cache_line`; decide
  values from iiac-perf measurements.
- Embedded floor: protocol is atomic load/store only (no
  CAS), so thumbv6m works today; keep it that way where
  possible (endpoint claims wants CAS — gate it), and 8/16-bit
  targets would need index-width genericization.
- Shared `Geometry` struct (`slot_size`, `capacity`, `mask`)
  held by Ring and passed whole to the endpoint
  constructors, slimming their signatures and Ring's fields.
- Black-box test split: move the public-API protocol tests
  (roundtrip, abandoned guards, threaded stress) to
  `tests/protocol.rs`; white-box tests (u32 wrap, attach
  header internals) stay in lib.rs. Do it when a trybuild
  compile-fail harness lands there too (pins the
  "second reserve_slot does not compile" guarantee).

## Done

Completed tasks are moved from `## Todo` to here, `## Done`, as they are completed
and older `## Done` sections are moved to [done.md](done.md) to keep this file small.

- feat: ring buffer user line + blocking contract [[5]]
- docs: messaging layer design (pools + queues) [[6]]

# References

[5]: chores/chores-01.md#feat-ring-buffer-user-line--blocking-contract
[6]: chores/chores-01.md#docs-messaging-layer-design-pools--queues
