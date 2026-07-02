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

## docs: template cleanup

Commits: [[1]]

Strip the vc-template-x1 boilerplate so the repo reads as its own
project rather than a template.

- `README.md` loses the template/cloning/assumptions sections and
  gains a one-line project intro (zero-copy ring buffer
  experiment 1).
- `notes/todo.md` drops the dummy template entries and their
  references; the ring-buffer design task becomes Todo #1.

## docs: zero-copy ring buffer design

Commits: [[2]],[[3]],[[4]],[[5]],[[6]],[[7]]

# References

[1]: https://github.com/winksaville/zc-ring-x1/commit/32fec004bd30 "32fec004bd300cc072a052fd0f80882a582c790f"
[2]: https://github.com/winksaville/zc-ring-x1/commit/a559e999f998 "a559e999f998bdeb8a0b0c44643ef1699953beb6"
[3]: https://github.com/winksaville/zc-ring-x1/commit/871506f3066e "871506f3066eaacbeb43244ff4196d4e2f9b1f1c"
[4]: https://github.com/winksaville/zc-ring-x1/commit/8d6dd8f000e4 "8d6dd8f000e4cc7d43bd11783489f4123abf94a5"
[5]: https://github.com/winksaville/zc-ring-x1/commit/734a618a21c5 "734a618a21c5bcce39b549dac247164fd320a8e6"
[6]: https://github.com/winksaville/zc-ring-x1/commit/dc28bcc0fce5 "dc28bcc0fce5d704145c6a444608c46ad68b0f78"
[7]: https://github.com/winksaville/zc-ring-x1/commit/828250ae38ec "828250ae38ec9bcef18fceaef0c3f420d60f5927"
