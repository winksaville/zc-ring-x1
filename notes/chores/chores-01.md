# Chores-01

Chores-XX files use [Prose form](../../AGENTS.md#prose-form). They
contain discussions and notes on various chores in github compatible
markdown. There is also a [todo.md](../todo.md) file that tracks
tasks and in general there should be a chore section for each task
with the why and how this task will be completed.

## docs: completed dummy chore (0.1.0)

A completed dummy chore description.

## chore: dummy chore (TBD)

A dummy chore description.

## docs: versioning SSOT + generic conventions

Commits:

Adopt the converged generic convention set from fc, so the
template's `AGENTS.md`, `cycle-protocol.md`, and `versioning.md`
are byte-identical to fc's — future repos copy them verbatim and
edit nothing. The template gains the versioning SSOT and the
push-approval carve-out.

- New `notes/versioning.md`: the versioning single source of
  truth. Its `Recording the version-of-record` section states
  Manifest / Notation / Reporter / Cadence as
  conditionals-by-medium, so the file is fully generic — no
  per-project block.
- `cycle-protocol.md` gains the Commits-backfill machinery and
  the "explicit scoped delegation waives the gates" push-approval
  carve-out; `Numbering` defers to versioning.md.
- `AGENTS.md` absorbs fc's generic blocks (Committing-vs-pushing,
  one-command-per-invocation, Use-jj, Conventional-commit shape)
  and drops the `(<version>)` title suffix.
