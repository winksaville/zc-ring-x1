# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

**docs: zero-copy ring buffer design**

Design a zero-copy ring buffer in no_std Rust using the
zerocopy crate to transmute structs into and out of the
buffer, suitable for inter- and intra-process communication.
Design notes: [ring-buffer-design.md](ring-buffer-design.md).

Plan:
- 0.3.0-0 docs: pick up zero-copy ring buffer design (done)
- 0.3.0-1 docs: ring buffer requirements + constraints (done)
- 0.3.0-2 docs: ring buffer API + memory-layout design (done)
- 0.3.0-3 feat: ring buffer prototype crate (done)
- 0.3.0-4 fix: ring buffer soundness + attach handshake (done)
- 0.3.0-5 docs: sync ring buffer design doc to as-built (done)
- 0.3.0 docs: zero-copy ring buffer design

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
   reserve/peek [details](ring-buffer-design.md#api).

## Ideas

- Blocking/wakeup story for reserve/peek (futex, eventfd);
  spin-only today.
- loom-based exhaustive ordering exploration of the SPSC
  protocol.
- Polish: `Error` implements `Display` + `core::error::Error`;
  `occupancy()` / `is_empty()` accessors.
- Packed-slot variant (drop the cache-line-multiple slot
  constraint) for small-message space efficiency.

## Done

Completed tasks are moved from `## Todo` to here, `## Done`, as they are completed
and older `## Done` sections are moved to [done.md](done.md) to keep this file small.


# References
