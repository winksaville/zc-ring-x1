# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

_No cycle currently in progress._

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

- docs: zero-copy ring buffer design [[1]]
- refactor: ring buffer symmetric reserve_slot API [[2]]
- docs: commit-and-push is not a review waiver [[3]]
- refactor: ring buffer endpoint modules [[4]]
- feat: ring buffer user line + blocking contract [[5]]

# References

[1]: chores/chores-01.md#docs-zero-copy-ring-buffer-design
[2]: chores/chores-01.md#refactor-ring-buffer-symmetric-reserve_slot-api
[3]: chores/chores-01.md#docs-commit-and-push-is-not-a-review-waiver
[4]: chores/chores-01.md#refactor-ring-buffer-endpoint-modules
[5]: chores/chores-01.md#feat-ring-buffer-user-line--blocking-contract
