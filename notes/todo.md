# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

**feat: mpsc ring sibling primitive**

N senders sharing one queue needs a multi-producer ring, and
the SPSC hot path (load/store-only, ~2 ns 1t) must not pay
for it — so MPSC lands as a separate sibling primitive,
measured A/B against SPSC and a fan-in comparator in
iiac-perf. Design:
[MPSC ring (sibling primitive)](ring-buffer-design.md#mpsc-ring-sibling-primitive).

- 0.11.0-0 docs: mpsc ring design + plan (done)
- 0.11.0-1 feat: mpsc ring layout + init/attach (done)
- 0.11.0-2 feat: mpsc send_with + consumer recv (done)
- 0.11.0-3 feat: demo mpsc flow lines (done)
- 0.11.0-4 docs: mpsc iiac-perf numbers (done)
- 0.11.0 feat: mpsc ring sibling primitive (close-out)

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

1. Descriptor queue endpoints: paired DescSender (loan +
   send) / DescReceiver (recv) [[11]]:
   - own ring endpoint + registry access;
   - the demo's ~20-line send path becomes ~3 lines;
   - `resolve`'s unsafe is audited once inside the crate
     (recv safe by construction);
   - guard handed back on Full;
   - design against both ring flavors (SPSC + MPSC);
   - the sender is also where each sender's private
     overflow pending list will live.
2. Overflow FIFO: on ring Full, append the message to a
   sender-private pending list instead of failing
   [details](ring-buffer-design.md#overflow-fifo-future):
   - intrusive — the same embedded next-link the
     free-stack uses, so zero allocation;
   - naturally bounded by pool capacity;
   - composes per-sender with MPSC — see
     [Overflow readiness](ring-buffer-design.md#overflow-readiness).
3. Batch alloc/free demo: alongside the one-message
   alloc_free_1t loops, a variant that allocs X messages
   (5, 10, …) then frees them all, pool vs global
   allocator. We think the pool's rate stays constant
   (pop/push is O(1) regardless of live count, LIFO keeps
   the working set hot) while Box::new/drop slows as the
   batch outgrows malloc's thread-cache fast path — the
   demo should show it.
4. Endpoint claims word: CAS-claimed producer/consumer roles
   in the ring header so a second attach/split claimant gets
   an error instead of silently violating SPSC; costs a
   layout_version bump (or spends `_pad0`)
   [details](ring-buffer-design.md#resolved-questions).
5. Typed endpoints: `Producer<T>` / `Consumer<T>` validating
   `T`'s geometry once at split instead of asserting on every
   reserve_slot_with [details](ring-buffer-design.md#api).

## Ideas

- Perf benches live in
  [iiac-perf](https://github.com/winksaville/iiac-perf)
  (sibling repo `../iiac-perf`), not here — its calibrated
  harness compares zc-ring against mpsc et al. directly
  (`zcring-1t`/`zcring-2t` mirroring `mpsc_1t`/`mpsc_2t`).
  An in-repo bench only if per-commit regression tracking
  proves necessary.

- Fan-in helper: consumer-side composition polling N SPSC
  rings under a pluggable service policy (priority,
  round-robin, weighted)
  [details](ring-buffer-design.md#fan-in-composition-not-a-mode):
  - buildable today from shipped parts;
  - likely offered alongside the MPSC ring eventually — no
    commitment yet.
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
  "second reservation does not compile" guarantee).

## Done

Completed tasks are moved from `## Todo` to here, `## Done`, as they are completed
and older `## Done` sections are moved to [done.md](done.md) to keep this file small.

- docs: messaging layer design (pools + queues) [[6]]
- feat: message pool allocator [[7]]
- feat: ring + pool demo binary [[8]]
- feat: demo alloc/free perf loops [[9]]
- feat: demo cpu-pinned placement variants [[10]]
- feat: descriptor queues over the SPSC ring [[12]]
- feat: wait-policy hook + spin models [[13]]
- refactor: demo _closure forms + on_full params [[14]]
- feat: demo seam lines on diff cores, SMT last [[15]]
- refactor: drop reserve_slot, keep _with ladder [[16]]
- refactor: drop reserve_slot_spin and alloc_spin [[17]]
- docs: refresh iiac-perf numbers, seam closed [[18]]

# References

[6]: chores/chores-01.md#docs-messaging-layer-design-pools--queues
[7]: chores/chores-01.md#feat-message-pool-allocator
[8]: chores/chores-01.md#feat-ring--pool-demo-binary
[9]: chores/chores-01.md#feat-demo-allocfree-perf-loops
[10]: chores/chores-01.md#feat-demo-cpu-pinned-placement-variants
[11]: chores/chores-01.md#follow-on-endpoints-and-wait-policies
[12]: chores/chores-01.md#feat-descriptor-queues-over-the-spsc-ring
[13]: chores/chores-01.md#feat-wait-policy-hook--spin-models
[14]: chores/chores-01.md#refactor-demo-_closure-forms--on_full-params
[15]: chores/chores-01.md#feat-demo-seam-lines-on-diff-cores-smt-last
[16]: chores/chores-01.md#refactor-drop-reserve_slot-keep-_with-ladder
[17]: chores/chores-01.md#refactor-drop-reserve_slot_spin-and-alloc_spin
[18]: chores/chores-01.md#docs-refresh-iiac-perf-numbers-seam-closed
