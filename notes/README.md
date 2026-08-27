# Notes

This directory contains various notes and documentation related to the project.
Each file is organized by topic for easy reference.

The task list is `../TODO.md`, and its `## In Progress` block is the running cycle's record
([Cycle-record](../AGENTS.md#cycle-record)). `chores/chores-*.md` and `done.md` are
the records of earlier cycles, frozen as history
([Frozen history](../agent-data/notes.md#frozen-history-chores-and-done)).

Project design docs:

- [ring-buffer-design.md](ring-buffer-design.md) — the
  zero-copy ring buffer and its MPSC sibling (terminology,
  requirements, layout, API, validation), kept in sync with
  `src/`.
- [../tprobe/notes/design.md](../tprobe/notes/design.md) —
  the tprobe measurement crate (probe primitives, ticks,
  report renderer, the tprobe/tp_runner split). The crate
  keeps its own notes so they travel on extraction.


## Workflow and conventions

Bot-facing workflow and conventions live in
[`../AGENTS.md`](../AGENTS.md):

- [Notes file conventions](../agent-data/notes.md)
  — Todo format, Reference numbering, Notes references
  (`[[N]]` citation style), Markdown anchor links, the In
  Progress block, Frozen history.
- [Code Conventions](../agent-data/code.md) — doc
  comments, `// OK: …` on `unwrap*` calls, ask-on-ambiguity,
  stuck detection.

Per-cycle workflow lives in [`../AGENTS.md`](../AGENTS.md#cycle-protocol) and the files it
links under `../agent-data/`: [cycle-checklists.md](../agent-data/cycle-checklists.md),
[cycle-protocol.md](../agent-data/cycle-protocol.md), [jj.md](../agent-data/jj.md), and
[versioning.md](../agent-data/versioning.md) for the `X.Y.Z-N` suffix scheme.
