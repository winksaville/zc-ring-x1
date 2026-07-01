# Todo

This file uses [Prose form](../AGENTS.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

**docs: zero-copy ring buffer design**

Design a zero-copy ring buffer in no_std Rust using the
zerocopy crate to transmute structs into and out of the
buffer, suitable for inter- and intra-process communication.

Plan:
- 0.3.0-0 docs: pick up zero-copy ring buffer design (done)
- 0.3.0-1 docs: ring buffer requirements + constraints
- 0.3.0-2 docs: ring buffer API + memory-layout design
- 0.3.0-3 feat: ring buffer prototype crate
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


## Done

Completed tasks are moved from `## Todo` to here, `## Done`, as they are completed
and older `## Done` sections are moved to [done.md](done.md) to keep this file small.


# References
