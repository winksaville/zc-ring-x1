# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

**feat: descriptor queues over the SPSC ring**

Getting a pool buffer must not imply sending it, and the
ring alone fuses allocation, queue position, and
publication. Descriptors — small `(pool id, buffer offset)`
values sent through the existing SPSC ring — separate them,
so any pool message travels over any queue. This cycle
builds the in-process slice: the descriptor type, pool ids
via a per-process registry, and guard ↔ descriptor
conversion; cross-process setup (mapping exchange, pool-id
coordination) stays design
[details](ring-buffer-design.md#messaging-layer-pools-and-descriptor-queues).

- 0.7.0-0 chore: open descriptor queue cycle (done)
- 0.7.0-1 docs: settle descriptor design questions —
  descriptor shape (offset vs index, width), pool-id
  source, resolve-side validation, the unsafe ownership
  contract for guard ↔ descriptor (done)
- 0.7.0-2 feat: pool registry + descriptor resolve —
  protocol tests folded in (round-trip, cross-thread free,
  hostile descriptors) so the API never lands untested
  (done)
- 0.7.0-3 feat: demo descriptor flow ring + pool (done)
- 0.7.0 feat: descriptor queues over the SPSC ring
  (close-out)

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

1. Endpoint claims word: CAS-claimed producer/consumer roles
   in the ring header so a second attach/split claimant gets
   an error instead of silently violating SPSC; costs a
   layout_version bump (or spends `_pad0`)
   [details](ring-buffer-design.md#resolved-questions).
2. Typed endpoints: `Producer<T>` / `Consumer<T>` validating
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
- `#[global_allocator]` experiment over size-class pools:
  GlobalAlloc is `&self` + any-thread, so it needs
  shared-allocation pools (phase 2 gen-tagged head) or
  per-thread pools with a routing layer; arbitrary `Layout`
  needs size-class selection + an oversize fallback. Frees
  from any thread are already natural (MPSC push). Classic
  mempool → malloc arc; measure the object-pool form in
  iiac-perf first.
- Private per-handle cache in front of the shared free-stack
  (tcache-over-arenas): alloc/free hit a thread-private list
  with plain load/store; refill/flush moves batches to the
  CAS stack, amortizing one CAS over N messages. Motivating
  datum: 2 uncontended CAS = 8.7 ns of the pool's 9.9 ns
  single-thread round trip (vs malloc tcache's zero
  atomics). Hold until iiac-perf shows per-op CAS matters in
  a composed workload; we think the pool's tail latency
  (p99, stddev) already beats malloc — no arena locks, no
  brk/mmap — and that matters more than the mean.
- `Message` trait over the payload cast boilerplate: const
  `MSG_ID` + the zerocopy bounds; receiver-side dispatch
  (read tag, match, cast) without per-call-site ceremony,
  and maybe a transport seam so an embedded
  pointer-descriptor profile slots in behind the same API
  [details](ring-buffer-design.md#descriptor-and-registry-design-070).
- BufSlot auto-free on Drop (RAII, iceoryx2-style): kills the
  silent leak-on-drop footgun at the cost of guard-type
  asymmetry (ring guards' drop = do-nothing) and a
  ManuallyDrop dance in free/send paths. Decide when
  descriptor-queue send lands — explicit free is easier to
  upgrade than to walk back.
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
- feat: message pool allocator [[7]]
- feat: ring + pool demo binary [[8]]
- feat: demo alloc/free perf loops [[9]]
- feat: demo cpu-pinned placement variants [[10]]

# References

[5]: chores/chores-01.md#feat-ring-buffer-user-line--blocking-contract
[6]: chores/chores-01.md#docs-messaging-layer-design-pools--queues
[7]: chores/chores-01.md#feat-message-pool-allocator
[8]: chores/chores-01.md#feat-ring--pool-demo-binary
[9]: chores/chores-01.md#feat-demo-allocfree-perf-loops
[10]: chores/chores-01.md#feat-demo-cpu-pinned-placement-variants
