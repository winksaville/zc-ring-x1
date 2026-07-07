# Bugs

This file uses [Prose form](../AGENTS.md#prose-form). It
lists known defects we're aware of but haven't scheduled a fix for.
Each entry describes what goes wrong, when, and the cost of
the failure. Entries are numbered (`1.` `2.` …) the same way
as `## Todo` in `todo.md`; run
`vc-x1 fix-todo --no-dry-run notes/bugs.md` to renumber after
insert / delete / reorder.

## Bugs

1. Pool free-stack CAS is not cfg-gated: `src/pool.rs` calls
   `compare_exchange` unconditionally, so the whole crate
   fails to build on load/store-only targets (thumbv6m) —
   contradicting the design's "the atomic floor rises only
   for pool users". The MPSC module gates on
   `target_has_atomic = "32"`; pool (and registry, which
   uses Pool) should gate the same way. Found while adding
   the gate for MPSC (0.11.0-1).

# References
