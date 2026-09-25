# Notes

This directory contains various notes and documentation related to the project.
Each file is organized by topic for easy reference.

The task list is `../TODO.md`, and its `## In Progress` block is the running cycle's record
([Cycle-record](../AGENTS.md#cycle-record)). `chores/chores-*.md` and `done.md` are
the records of earlier cycles, frozen as history
([Frozen history](../agent-data/notes.md#frozen-history-chores-and-done)).

Project design docs:

- [ring-buffer-design.md](ring-buffer-design.md): the
  zero-copy ring buffer, its MPSC sibling, the seam-word
  SPSC v1, the in-slot seq SPSC v2, the ring of segments
  SPSC v3, the equality-seq MPSC v1, and the ring of
  segments MPSC v2 with their measurements at four depths,
  the pool-message sweep against cordyceps's intrusive MPSC, and
  the multi-stack pool with the type-tag a receiver dispatches
  on (terminology, requirements, layout, API, validation), kept
  in sync with `src/`.
- [user-guide.md](user-guide.md): how to use SPSC v3 and MPSC
  v2 from a pool to two threads, sizing, init and split,
  sending, receiving, wait policies, the segment lifecycle,
  the counters, limits, and errors, with two complete
  programs in `examples/`.
- [../tp_matrix/README.md](../tp_matrix/README.md): the
  measurement tools, `tp-cell`, `tp-matrix`, the streaming
  `tp-stream`, and the pool-message sweep `tp-pool`, and what
  their numbers are sensitive to.
- [../tprobe/notes/design.md](../tprobe/notes/design.md):
  the tprobe measurement crate (probe primitives, ticks,
  report renderer, the tprobe/tp_runner split). The crate
  keeps its own notes so they travel on extraction.


## Workflow and conventions

Bot-facing workflow and conventions live in
[`../AGENTS.md`](../AGENTS.md):

- [Notes file conventions](../agent-data/notes.md):
  Todo format, Reference numbering, Notes references
  (`[[N]]` citation style), Markdown anchor links, the In
  Progress block, Frozen history.
- [Code Conventions](../agent-data/code.md): doc
  comments, `// OK: ...` on `unwrap*` calls, ask-on-ambiguity,
  stuck detection.

- [prose-check.py](prose-check.py): the check behind the prose punctuation rules
  ([Semicolons](../agent-data/prose.md#semicolons), [Typeable punctuation
  only](../agent-data/prose.md#typeable-punctuation-only)). It blanks what is code, reports every
  authored banned character and prose semicolon in the tracked files, and runs in `vc-x1
  validate`, full and fast. Frozen history, the agent-files, and the transcriptions it lists are
  excluded.

Per-cycle workflow lives in [`../AGENTS.md`](../AGENTS.md#cycle-protocol) and the files it
links under `../agent-data/`: [jj.md](../agent-data/jj.md) for the commands,
[versioning.md](../agent-data/versioning.md) for the `X.Y.Z-N` suffix scheme, and
[cycle-model.md](../agent-data/cycle-model.md) for the In Progress block's specimen.
