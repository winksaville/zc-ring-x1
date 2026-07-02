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

Commits: [[2]],[[3]],[[4]],[[5]],[[6]],[[7]],[[8]],[[9]]

Design a zero-copy ring buffer in no_std Rust using the
zerocopy crate to transmute structs into and out of the
buffer, suitable for inter- and intra-process communication.
Design notes: [ring-buffer-design.md](../ring-buffer-design.md)
(kept in sync with `src/lib.rs`).

### As-built ladder

- 0.3.0-0 docs: pick up zero-copy ring buffer design
- 0.3.0-1 docs: ring buffer requirements + constraints
- 0.3.0-2 docs: ring buffer API + memory-layout design
- 0.3.0-3 feat: ring buffer prototype crate
- 0.3.0-4 fix: ring buffer soundness + attach handshake
- 0.3.0-5 docs: sync ring buffer design doc to as-built
- 0.3.0-6 fix: ring buffer guard aliasing under Miri
- 0.3.0 docs: zero-copy ring buffer design (close-out)

### Outcome

- Landed the design doc plus the no_std SPSC crate: M×N
  cache-line slots, free-running AtomicU32 indices,
  reserve/commit + peek/release guards over zerocopy-bounded
  typed views; 6 tests including a threaded stress and a
  u32-wrap proof; README overview with a compiled example.
- A max-effort review between -3 and -4 found three real
  issues — a Miri-confirmed Stacked Borrows violation in
  init, a 32-bit region-size overflow reachable from safe
  code, and an init/attach publish race — fixed in -4.
- The user running the *full* Miri suite found a fourth: the
  guards' reference fields are argument-protected during
  commit/release while the handoff store races them — fixed
  in -6 (guards hold raw pointers, references minted per
  access).
- Lesson recorded: -4's `--skip threaded` Miri run was a
  coverage hole — the only concurrency test was excluded from
  the only aliasing checker. The ladder now runs the full
  Miri suite (threaded count reduced under `cfg(miri)`).
- Follow-ons recorded: endpoint claims word and typed
  endpoints as Todo 1/2; wakeup story, loom, polish, and
  packed slots under Ideas.

## refactor: ring buffer symmetric reserve_slot API

Commits:

Rename the guard API so both endpoints use the same verb.
`peek` read as a free look — it hid that the consumer holds
an exclusive one-at-a-time guard, mirror image of the
producer's reservation. Both sides now run the identical
shape: `reserve_slot` → guard → commit / release.

- `producer.reserve_slot::<T>()` → `WriteSlot<'_, T>` →
  write via `&mut T` → `commit()`.
- `consumer.reserve_slot::<T>()` → `ReadSlot<'_, T>` →
  read via `&T` → `release()`.
- The one-slot-at-a-time rule is now stated in the doc
  comments and the design doc: the guard holds the
  endpoint's `&mut` borrow, so a second `reserve_slot`
  while a guard is live is a compile error (verified with
  a scratch E0499 negative test), not a runtime state.
- Direction lives in the closing verb and the guard type,
  not the reserve verb — commit publishes a write, release
  frees a read.
- Breaking API rename, so 0.3.0 → 0.4.0.

# References

[1]: https://github.com/winksaville/zc-ring-x1/commit/32fec004bd30 "32fec004bd300cc072a052fd0f80882a582c790f"
[2]: https://github.com/winksaville/zc-ring-x1/commit/a559e999f998 "a559e999f998bdeb8a0b0c44643ef1699953beb6"
[3]: https://github.com/winksaville/zc-ring-x1/commit/871506f3066e "871506f3066eaacbeb43244ff4196d4e2f9b1f1c"
[4]: https://github.com/winksaville/zc-ring-x1/commit/8d6dd8f000e4 "8d6dd8f000e4cc7d43bd11783489f4123abf94a5"
[5]: https://github.com/winksaville/zc-ring-x1/commit/734a618a21c5 "734a618a21c5bcce39b549dac247164fd320a8e6"
[6]: https://github.com/winksaville/zc-ring-x1/commit/dc28bcc0fce5 "dc28bcc0fce5d704145c6a444608c46ad68b0f78"
[7]: https://github.com/winksaville/zc-ring-x1/commit/828250ae38ec "828250ae38ec9bcef18fceaef0c3f420d60f5927"
[8]: https://github.com/winksaville/zc-ring-x1/commit/573bb0ac5b19 "573bb0ac5b19dba8dd158a73eadc19ba3ba6f416"
[9]: https://github.com/winksaville/zc-ring-x1/commit/69a00921a2eb "69a00921a2eb112d2ae2833da112159283c95c5c"
