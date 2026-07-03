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

Commits: [[10]]

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

## docs: commit-and-push is not a review waiver

Commits: [[11]]

Clarify the push-delegation policy after a real slip: the bot
read "run miri then commit and push" as a scoped delegation
and pushed without the work review or the description review.
The protocol's own fallback ("when in doubt, ask") should
have applied.

- The **Explicit grant** bullet now states that "commit and
  push" names the destination, not a waiver — it authorizes
  the push *after* the normal reviews; only wording that
  explicitly waives the stops ("don't check in", "no need to
  review") waives them.
- Same wording copied to vc-template-x1's cycle-protocol.md
  (committed there separately), keeping the shared files
  verbatim copies.

## refactor: ring buffer endpoint modules

Commits: [[12]]

Split the single-file crate so each endpoint lives with its
guard: the Producer/WriteSlot and Consumer/ReadSlot pairings
are structural (one module each) rather than positional in
one long file. API-neutral — the crate root re-exports both
modules, so every existing path still works.

- `src/lib.rs` keeps the region-level machinery: Header,
  Error, Ring (init/attach/split), geometry helpers, and the
  test module.
- `src/producer.rs`: Producer, WriteSlot, Full.
  `src/consumer.rs`: Consumer, ReadSlot, Empty.
- `Ring::split` builds the endpoints via `pub(crate)`
  constructors — a parent module cannot construct a child's
  private-field struct directly.
- Motivated by the reserve_slot rename discussion: commit /
  release cannot move onto the endpoints (the guard holds the
  endpoint's `&mut` borrow), so cohesion lands at the module
  level instead.

## feat: ring buffer user line + blocking contract

Commits: [[13]]

Give apps a shared-memory home for wakeup protocols without
the crate ever blocking or spinning. The header gains a
fourth, app-owned cache line of 16 `AtomicU32` user words —
zeroed at init, exposed via `user()` on both endpoints, never
interpreted by the crate — and the design doc gains the
layered-blocking contract built on it.

- Blocking is policy and lives above the crate:
  `reserve_slot` returning Full/Empty stays the entire core
  API. The design doc's new "Blocking and user words" section
  records the lost-wakeup discipline (set flag → re-check →
  sleep; commit → check flag → wake), values-not-addresses,
  and timeouts-for-dead-peers.
- User line is **last** so an app overrun walks into its own
  slot data, not an index line; geometry line 0 is cold
  (handles snapshot it), so ordering ours-then-theirs costs
  nothing.
- Header alignment is now expressed per-field via a
  `CacheAligned<T>` wrapper (Deref to the inner value);
  compiler computes inter-line padding, replacing hand-counted
  `_pad` arrays. `repr(align(N))` takes only integer literals,
  so const asserts tie the literal 64 to `CACHE_LINE`.
- Layout change: header 192 → 256 bytes, `LAYOUT_VERSION` 2.
- The header also records the build's `cache_line`, validated
  at attach (`BadCacheLine`): assume nothing, verify at the
  boundary. This makes a future per-target or embedded-tier
  `CACHE_LINE` safe — mismatched builds error instead of
  silently disagreeing on offsets.
- Embedded observation recorded in the design doc: the
  protocol is atomic load/store only (no CAS), so the floor
  reaches thumbv6m today; per-target line sizes and index
  widths are Ideas entries.
- We think the endpoint-claims Todo can spend line 0's spare
  44 bytes when it lands, bumping to layout v3.

## docs: messaging layer design (pools + queues)

Commits: [[14]]

Design (notes-only) for the layer above the ring, born from
the observation that `reserve_slot` fuses three acts a
messaging system needs separated — allocating message memory,
assigning queue position, and opening a publication window —
so "get a message" wrongly implies "send it now". The design
doc gains a "Messaging layer: pools and descriptor queues"
section; todo.md gains the matching ranked entries and Ideas.

- Architecture: the ring stays the unchanged primitive;
  pools own message memory, queues carry `(pool id, offset)`
  descriptors, provenance travels in a per-buffer header.
- Requirements stated up front: any quantity of queues and
  pools, any message over any queue, inter- and intra-process
  zero-copy (offsets only, no pointers in shared structures),
  heterogeneous attach-validated pools, SPSC now / MPSC
  later, single allocator now / shared allocation eventually.
- Free path: intrusive LIFO free-stack (single-popper
  Treiber — no ABA; validated pops). We think LIFO beats a
  free-ring on working-set size. First CAS in the design;
  the ring protocol stays load/store-only.
- Overflow FIFO (future): ring-Full sends append to a
  sender-private intrusive pending list through the same
  embedded next-link; naturally bounded by pool capacity, so
  backpressure reappears as allocation failure.
- Prior art: iceoryx2 covers most of this layer as std
  middleware; recorded what to study and what keeps this
  project distinct (no_std floor, hostile-peer posture,
  primitives-not-framework).

## feat: message pool allocator

Commits: [[15]]

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
[10]: https://github.com/winksaville/zc-ring-x1/commit/d8bcb77af33b "d8bcb77af33bd6f551e3dd848613ec8b2e70e7cc"
[11]: https://github.com/winksaville/zc-ring-x1/commit/af563403aa46 "af563403aa46d029f6f7cf1465a67f28aab2beea"
[12]: https://github.com/winksaville/zc-ring-x1/commit/0cdcfd95859c "0cdcfd95859c8c6c7fd3f64d3c77e401f78dcd9e"
[13]: https://github.com/winksaville/zc-ring-x1/commit/c814e98c6168 "c814e98c6168b83b4c66398e03aa505a801f0db9"
[14]: https://github.com/winksaville/zc-ring-x1/commit/20633c31fb18 "20633c31fb187e5b947422e2292b7d996dc73e6a"
[15]: https://github.com/winksaville/zc-ring-x1/commit/1d6d388950f8 "1d6d388950f836f820cd5983a61984a4a8ee176e"
