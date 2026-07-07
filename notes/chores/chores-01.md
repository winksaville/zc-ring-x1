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

Commits: [[35]] (in vc-template-x1 — this work predates this
repo, which was created from the template; the content
arrived here in the Initial commit)

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

Commits: [[15]],[[16]],[[17]],[[18]],[[19]],[[20]]

Todo #1 (decouple "get a message" from "send it"): the ring's
reserve_slot fuses allocation, queue position, and a
publication window, so getting a buffer implied sending it
now. The pool is the allocation half of the messaging layer —
simplicity-first, to hone the API and give iiac-perf a
baseline.

- Region shape mirrors the ring: all-atomic two-line
  PoolHeader (cold geometry / contended `first_free_idx`),
  magic "ZCP1", magic-last init/attach handshake, geometry
  snapshots, hostile-peer posture (validated pops fail toward
  Exhausted, never out of bounds).
- Intrusive LIFO free-stack with zero per-buffer overhead: a
  free buffer's first word is its next-link; allocated
  buffers carry no header. Pool-id provenance deferred to the
  descriptor-queue cycle (pool layout_version bump).
- Ownership decoupling: `BufSlot` borrows the region
  lifetime, not the pool handle — many live guards at once;
  `alloc` takes `&mut self` as the single-popper token (no
  ABA); `free(self)` is the MPSC push, callable wherever the
  guard moved; drop-without-free leaks (RAII alternative
  recorded in Ideas).
- Usage model written before the tests (README: roles, buffer
  lifecycle, what "send" means pre-descriptor-queues) and the
  tests pin it: 11 pool tests incl. cross-magic attach
  rejection, corruption-repair, type edges (mixed sizes,
  align(32), too-big panics, ZST), threaded allocator/freer
  split; all under Miri.
- Naming honed in review: `cache_line_size` (verb-free),
  `first_free_idx`, `next_buf_idx`, `CACHE_LINE_SIZE` /
  `TEST_CACHE_LINE_SIZE`.

Ladder (all shipped, Keep separate):

- 0.6.0-0 chore: open message pool cycle
- 0.6.0-1 feat: message pool layout + init/attach
- 0.6.0-2 feat: message pool alloc/free
- 0.6.0-3 docs: message pool usage model
- 0.6.0-4 test: message pool protocol tests
- 0.6.0 feat: message pool allocator (close-out)

## feat: ring + pool demo binary

Commits: [[21]]

A runnable, visible proof for library visitors: both
primitives working across threads with throughput printed.

- `src/bin/zc-ring-x1-demo.rs`: a binary (not an example)
  so `cargo install --path . --locked` works and `-V`
  prints the version-of-record — you always know which
  build you are testing. Ring part (SPSC in-place messages,
  producer → consumer threads) and pool part (allocator
  thread fills BufSlots, freer thread frees; guards cross a
  std channel per the usage model's "send = move the
  guard").
- Prints msgs/sec per part — an eyeball smoke-baseline, not
  a benchmark (iiac-perf owns real measurement). The pool
  part's rate is channel-dominated, which is itself the
  argument for the descriptor-queue cycle; the demo says so.

## feat: demo alloc/free perf loops

Commits: [[22]]

Two single-thread loops join the demo scoreboard: the
pool's raw alloc→write→free cost, and the same loop through
the global allocator (Box::new/drop), black_box-guarded.

- Pool ≈ malloc single-threaded (~10 ns) is the expected
  result, not a defeat: malloc's hot path is the same
  thread-local LIFO free-list trick. The pool's case is
  shared-memory zero-copy, no_std, no syscalls ever, and
  determinism for the embedded floor — capabilities, not a
  tight-loop race.
- The scoreboard now decomposes the story: ring transfer
  tens of ns, pool ops ~10 ns, std channel hundreds — the
  descriptor-queue cycle's projected win is readable off
  the four lines.

## feat: demo cpu-pinned placement variants

Commits: [[23]]

Cross-thread numbers swung ~4× run to run because the
scheduler decides where the two threads land. Every demo
line now states its thread placement, and the 2t parts run
a placement ladder: unpinned, same physical core (SMT
siblings, shared L1/L2), different physical cores
(preferring one outside cpu0's L3 group — the far end).

- Pinning is `sched_setaffinity` via a Linux-only `libc`
  dependency; cpu pairs are discovered at runtime from
  `/sys/devices/system/cpu` topology (SMT sibling list, L3
  `shared_cpu_list`), so the demo adapts to any Linux box
  and prints a "skipped" line when the machine lacks the
  shape. Non-Linux builds compile with pinning stubbed out.
- Pinned runs execute in scoped threads that pin
  themselves — pinning the main thread would leak its
  affinity mask into every later part's spawned threads.
- Functions renamed to say what each line measures:
  `spsc_ring_one_msg_1t/_2t`, `std_mpsc_one_pool_msg_2t`
  (std's mpsc channel carrying pool-buffer guards, used
  SPSC), `pool_alloc_free_1t`, `global_alloc_free_1t`; the
  1t runs pin to core 0 so every line's placement is
  deterministic.
- The ladder decomposes the transfer cost: same-core ~8-12
  ns vs diff-cores ~100-300 ns for the identical ring code.
  We think the gap is cache-line handoff distance (shared
  L1/L2 vs cross-L3/CCD traffic), and the unpinned number
  wandering between the two ends is scheduler placement.
  Single runs, no mean/stdev — still an eyeball baseline,
  not a benchmark.
- `Msg` shrank to one word (`seq`): the loops measure
  creating, initializing, sending, and receiving a message,
  and writing a second word (`val`, the old torn-write
  canary) was payload cost unrelated to that. One word is
  the floor that is still real — an uninitialized message
  would isolate pure transfer but no real sender ships one;
  a possible future variant, not the default.

## feat: descriptor queues over the SPSC ring

Commits: [[24]],[[25]],[[26]],[[27]],[[28]]

Getting a pool buffer must not imply sending it, and the
ring alone fuses allocation, queue position, and
publication. Descriptors — small `(pool id, buffer index)`
values sent through the existing SPSC ring — separate them,
so any pool message travels over any queue. This cycle
built the in-process slice: the descriptor type, pool ids
via a per-process registry, and guard ↔ descriptor
conversion; cross-process setup (mapping exchange, pool-id
coordination) stays design
[details](../ring-buffer-design.md#descriptor-and-registry-design-070).

- The guard and the descriptor are the two exclusive forms
  of buffer ownership; converting is the send-side handoff.
  `into_desc` is safe (guard handed back on error, no leak
  path); `resolve` is the one `unsafe` — validation is
  all-`Err` (a hostile descriptor selects which pool gets
  compared, so it must not select a panic), and `unsafe`
  covers only what validation cannot check: ownership
  uniqueness plus happens-before delivery.
- `PoolResolver` is a view derived from the pool handle —
  no second region borrow, dodging the Stacked Borrows
  retag hazard init documents. Registries are per-process
  and private; only descriptors cross boundaries, and pool
  ids are the shared vocabulary (cross-process
  `register_at` under an agreed id is recorded against the
  pool-id-allocation open question).
- Protocol tests folded into the feat commit rather than a
  separate test step, so the API never landed untested;
  the composed cross-thread test (descs over the ring,
  buffers recycling) is the demo/iiac-perf flow in
  miniature. Full suite clean under Miri.
- Demo grew the composed flow with the same placement
  ladder as the raw ring, output regrouped so like
  compares with like; the 1t variant allocates outside the
  timed loop, isolating messaging cost (~8 ns/msg vs the
  std-channel counterpart's ~35) from the pool cycle.
- Design doc gained the System model overview (rules at a
  glance, every bullet linking to its expanding section)
  and a linkability convention: sub-topics wanted by
  inbound links become `####` headings (Provenance, Open
  questions), whose anchors follow their titles even when
  a section migrates.

### As-built ladder

- 0.7.0-0 chore: open descriptor queue cycle
- 0.7.0-1 docs: settle descriptor design questions
- 0.7.0-2 feat: pool registry + descriptor resolve
- 0.7.0-3 feat: demo descriptor flow ring + pool
- 0.7.0 feat: descriptor queues over the SPSC ring
  (close-out)

### Follow-on: endpoints and wait policies

Design captured from the post-demo review; promoted to
`## Todo` #1 and #2.

- The demo's send path is ~20 lines of ceremony. Target
  shape is ~3: `loan` → populate → `send`. Paired
  `DescSender` (ring producer + pool + registry access;
  `loan` carries the single-popper token structurally) and
  `DescReceiver` (consumer + registry). The deep win is
  safety, not line count: descriptors entering the ring
  only through the paired sender make `recv` satisfy
  resolve's contract by construction — the `unsafe` gets
  audited once inside the crate instead of at every call
  site. iceoryx2's loan/send shape, as the design doc's
  prior-art note anticipated.
- Wait policies as an injected closure: `_with(impl
  FnMut(u32) -> bool)` — attempt count in, keep-waiting
  out — uniformly over the three wait points (Exhausted /
  Full / Empty). Tiers: non-blocking primitive (unchanged,
  the only tier with protocol knowledge), the `_with`
  hook, shipped policies we dogfood (`_spin`) as the model
  users copy, and user compositions (their own
  `get::<Msg>()`, policy baked in, no closure in their
  signature). Polling only — true blocking (futex,
  eventfd) needs a peer wake over the user line and stays
  a separate layer.
- Validation order: the wait-policy hook lands first and
  iiac-perf measures raw loop vs closure vs `_spin` — the
  seam must be zero-cost before the endpoints build on it.

## feat: wait-policy hook + spin models

Commits: [[29]]

Todo #1: every wait in the demo and tests was a hand-rolled
spin loop — nine lines of ceremony per wait point. The hook
makes the wait an injected policy (`FnMut(u32) -> bool`:
attempt count in, keep-waiting out) and ships the one policy
we dogfood, per the tier model in
[Follow-on: endpoints and wait policies](#follow-on-endpoints-and-wait-policies).

- `_with` variants on `Pool::alloc` and both endpoints'
  `reserve_slot`; the policy is called after each failed
  attempt (0-based, saturating count), `false` returns the
  operation's usual error.
- On the endpoints the wait loop *is* the reservation (the
  guard borrows the endpoint, so retrying a `reserve_slot`
  call could not return it): the owned index loads once,
  only the peer's index re-reads per attempt, so the no-wait
  fast path does exactly the loads `reserve_slot` does.
- `policy::spin` is the only shipped policy; the `_spin`
  wrappers return the value directly (no `Result` — spin
  never quits).
- Demo keeps every existing loop raw and adds a three-way
  seam exemplar. Terms, defined here at first use:
  - **raw** — the hand-rolled wait loop: the baseline, the
    fastest form we know for the shape, what the other
    forms are measured against (and what iiac-perf ports
    first).
  - **exemplar** — one workload written in all three forms
    side by side (raw / `_closure` / `_spin`), the
    worked model of the comparison:
    `spsc_ring_one_msg_{1t,2t}`, at 1t (no waiting — pure
    per-call overhead, where ~3 ns/msg makes any cost a
    visible percentage) and same-core 2t (overhead under
    real waiting).
- The exemplar caught two real regressions before landing:
  a probe-then-reserve first draft doubled the index loads
  (1t: 2.8 → 6.2 ns), fixed by inlining the reservation
  into the wait loop; and non-generic `policy::spin` didn't
  cross-crate-inline into the monomorphized `_with`
  (`_spin` 10.2 → 3.3 ns after the fix + `#[inline]`).
  After both, the 1t ladder reads 2.8 / 2.9 / 3.3 —
  eyeball-zero for `_with`, near-zero for `_spin`.
- Remaining clause of the todo — confirm with calibrated
  mean/stdev in iiac-perf — happens in the sibling repo.

## refactor: demo _closure forms + on_full params

Commits: [[30]]

Naming pass from reading the 0.7.1 call sites: a scoreboard
label ending in `_with` dangles ("with what?"), and nothing
at a call site said what the closure decides or when it
runs.

- Demo exemplar forms renamed `_with` → `_closure`
  (`spsc_ring_one_msg_{1t,2t}_closure`): form names
  describe the caller's code shape, and the label now reads
  complete without an argument in sight. The API keeps
  `_with` — the std idiom (`get_or_insert_with`,
  `resize_with`) whose value is recognition; at real call
  sites the argument answers "with what?".
- The wait closure's *event* moved into the parameter name:
  `on_full` / `on_empty` / `on_exhausted`. It surfaces in
  docs, signature popups, and rust-analyzer inlay hints —
  the call site itself now shows when the closure runs.
  Policies stay event-neutral (`policy::spin` serves all
  three events); the event belongs to the call, the
  strategy to the policy.
- Considered and recorded for later if `_with` still
  itches: `reserve_slot_or_wait(on_full)` (behavior-named
  method), and replacing the bool with a `Wait::Again /
  GiveUp` enum (boolean blindness) — deferred because
  named policies already carry the meaning at real call
  sites, and pre-1.0 renames stay cheap.

## feat: demo seam lines on diff cores, SMT last

Commits: [[31]]

On a CPU without SMT (Pi 5's Cortex-A76, one thread per
core) the same-core section skips, and the `_closure` /
`_spin` seam lines ran only there — so a non-SMT machine
never printed the three-way comparison under real waiting.

- The diff-cores section now prints the full five-line set;
  on the Pi 5 the three forms land within a few percent of
  each other (~82–87 ns/msg across two A76 cores) — the
  eyeball zero-cost the seam check wants.
- The same-core (SMT) section moved last: it's the only
  hardware-conditional section, so a machine without SMT
  ends with the notice instead of a hole mid-output.
- Skip notices print on two lines (header, then indented
  reason) instead of padding to the scoreboard columns.
- The skip itself is correct behavior, recorded so it isn't
  "fixed" later: pinning works on any core (diff-cores runs
  everywhere), but two threads simultaneously on one
  physical core *is* SMT hardware. A probe pinning both
  spin-waiting threads to one cpu measured ~8k msgs/sec vs
  ~12M cross-core — we think each handoff waits out a
  scheduler timeslice, so that number measures the
  scheduler, not the ring.

## Performance runs using becnches in iiac-perf

Commits: [[32]]

300s (5 min) iiac-perf round-trip runs recorded for the ring's
three wait forms, which surfaced bug #1: the `raw`
loop-on-`reserve_slot` pattern reads both indices every spin,
while `_with` / `_spin` hoist the caller-owned index out of the
loop — measurably slower and jitterier at 2t.

- README.md gained the zcr-{raw,with,spin}-{1t,2t} benchmark
  blocks from those runs.
- notes/bugs.md filed bug #1 (drop the dominated `reserve_slot`
  rung), which the "refactor: drop reserve_slot, keep _with
  ladder" cycle below resolves.
- The `becnches` misspelling in the header is Wink's, kept
  verbatim from the commit title so the section is an exact
  `git log --grep` match — not a chores-side typo.

## refactor: drop reserve_slot, keep _with ladder

Commits: [[33]]

`reserve_slot` on both endpoints was a strictly dominated API
rung: `reserve_slot_with(|_| false)` does everything it did
with identical loads, and its naive loop-on-the-fallible-call
usage carried a measured +30 ns / ~2× body jitter under
cross-core traffic (bug #1, whose raw-pattern benches live in
iiac-perf's `zcr-raw-*`). Removing it leaves a two-rung ladder
per endpoint — `reserve_slot_with` (fallible primitive) +
`reserve_slot_spin` (forever-spin convenience).

- `reserve_slot_with` is now the self-contained primitive: its
  doc absorbed the single-probe (`|_| false`) note and the
  raw-pointer / `>=`-corruption rationale the deleted method
  used to host.
- Call sites split by intent: single-probe (the `_1t` demos,
  the assertion tests, the README examples) →
  `reserve_slot_with(|_| false)`; loop-on-fallible spin waits
  (the `_2t` demos, the registry tests) → `reserve_slot_spin`.
- `threaded_spsc` keeps looping on `reserve_slot_with(|_| false)`
  so the fallible primitive still has a threaded-contention
  test distinct from `threaded_spsc_spin_variants` (which
  covers the spin rung).
- No `try_reserve_slot` alias yet: nothing outside tests makes
  `_with(|_| false)` common, so per bug #1 the alias waits for
  a real consumer.

### ring-buffer-design.md: blocking intro reconciled

The `## Blocking and user words` intro still read "the crate
never blocks and never spins" — accurate before the
wait-policy hook, but `reserve_slot_spin` spins now, so a bare
method rename would have left an inline contradiction. The
intro was reframed instead: the ring's fallible core never
blocks, and *how* to wait is injected policy (a shipped
`policy::spin` drives `reserve_slot_spin`); anything that
actually sleeps still belongs to the layer above via the
`user()` words. We think the rest of that section (the
`user()` wakeup-protocol contract) is unaffected by the API
change.

### As-built ladder

- `0.8.0` refactor: drop reserve_slot, keep _with ladder

## refactor: drop reserve_slot_spin and alloc_spin

Commits: [[34]]

With `reserve_slot` gone, the endpoints' `reserve_slot_spin`
(and the pool's parallel `alloc_spin`) were the only remaining
`_spin` conveniences — each a three-line wrapper over
`*_with(policy::spin)` that hid the always-`Ok` unwrap. They
carry their weight only if callers reach for spin often; no
consumer outside the demo and tests does, so they go, leaving
one rung per operation (`*_with` + a shipped `policy::spin`
policy). `policy::spin` stays public — it is the migration
target and what the base demo lines now pass.

- Spin call sites migrate to `*_with(policy::spin).unwrap()`
  (`// OK: policy::spin never gives up` on non-test sites):
  the base 2t demos, the registry / lib / pool tests.
- `Pool::alloc_spin` removed for symmetry (todo named only
  `reserve_slot_spin`, but the same rationale applies); its
  lone test became `alloc_with_spin_returns_ok`.

### demo: the three-way wait-policy seam collapses

The demo carried a `raw` / `_closure` / `_spin` seam exemplar
— one workload in three wait forms, side by side, as an
eyeball zero-cost check. Two removals dissolved it:
`reserve_slot` (0.8.0) took the hand-rolled `raw` form, and
`reserve_slot_spin` (here) took `_spin`. What was left —
`_closure` (an inline spinning closure) vs the base line
(`policy::spin`) — is the same `reserve_slot_with` call twice
with a differently-spelled policy, so the `_closure` and
`_spin` demo fns were dropped as redundant. The base
`spsc_ring_one_msg_{1t,2t}` lines remain as the single
demonstration of the API. Calibrated seam measurement was
always iiac-perf's job (todo), not the demo's.

### README: spin benchmark blocks removed

The `zcr-spin-1t` / `zcr-spin-2t` measurement blocks named a
method that no longer exists. They were dropped rather than
relabeled; fresh iiac-perf numbers land in a later pass. The
`zcr-raw-*` / `zcr-with-*` blocks stay untouched.

### As-built ladder

- `0.9.0` refactor: drop reserve_slot_spin and alloc_spin

## docs: refresh iiac-perf numbers, seam closed

Commits: [[36]]

The `0.9.0` cycle removed the `zcr-spin-*` README blocks and
promised fresh iiac-perf numbers "in a later pass". This is that
pass, and it also closes the seam-measurement clause of Todo #1
(the shipped wait-policy hook's validation step).

- README.md perf section replaced with an `iiac-perf 0.16.0`
  run: the harness now expands the `zcr` shorthand to just
  `zcr-with-1t` / `zcr-with-2t`, since raw and spin no longer
  exist as zc-ring methods. Header command shortens from the
  six explicit bench names to `iiac-perf -d 300 zcr`.
- Todo #1's measurement is done: raw / `_with` / `_spin` were
  measured at 300s, and `_with` ≈ `_spin` within noise while
  the hand-rolled raw loop was the slowest (bug #1's
  read-both-indices-every-spin penalty). That result is the
  empirical justification for the `0.8.0` / `0.9.0` drops of
  `reserve_slot` and `reserve_slot_spin` — the seam collapsed
  because the dominated rungs were removed, not deferred.

## feat: mpsc ring sibling primitive

Commits:

N senders sharing one queue needs a multi-producer ring, and
the SPSC hot path (load/store-only, ~2 ns 1t) must not pay
for it — so MPSC lands as a separate sibling primitive,
measured A/B against SPSC in iiac-perf. Design:
[MPSC ring (sibling primitive)](../ring-buffer-design.md#mpsc-ring-sibling-primitive).

- Vyukov's bounded MPMC restricted to MP/SC:
  - producers CAS-claim `producer_idx`;
  - a per-slot seq array publishes each slot independently
    (claim order ≠ commit order under concurrency);
  - the consumer stays CAS-free, and producers never read
    `consumer_idx` — fullness comes from the slot seq.
- Closure send (`send_with`) makes claim abandonment
  unrepresentable; a panicking fill publishes a tombstoned
  commit (`pos + 1 + 2^31`) the consumer releases without
  delivering — an unwind cannot wedge the queue.
- Own magic (`"ZCM1"`) and layout version; the module is
  gated on `target_has_atomic = "32"`. Filing the gate found
  bug #1: the pool's free-stack CAS is ungated, so
  load/store-only targets already fail to build.
- Demo grew mpsc lines at every placement plus the 2p+1c
  shape; iiac-perf grew `zcr-mpsc-1t`/`zcr-mpsc-2t` (bench
  code in the sibling repo, committed there separately).
- Deferred to the design's measurement plan: the 2p+1c
  contention bench and the fan-in comparator — the
  round-trip harness needs a competing-producer shape
  first.

### Outcome: the 2t surprise

300s adjusted means: `zcr-with-1t` 2.3 ns vs `zcr-mpsc-1t`
4.4 ns — the uncontended claim CAS + seq publish costs
~2.1 ns, as expected. Cross-thread at 1p/1c the sign flips:
`zcr-mpsc-2t` 73.9 ns vs `zcr-with-2t` 100.1 ns — the MPSC
protocol measures *faster*, with a wider spread (p10 35.8 /
p90 89.9 vs SPSC's tight 88–98). We think the SPSC
round-trip bounces two index cache lines per handoff (each
side polls the line the other writes) while the MPSC
handoff's only shared hot word is the slot seq — neither
side ever reads the other's index line. Promoted to a
`## Todo` entry (explore via perf cache-transfer counters
and/or the padded-seq variant; if confirmed, a seam-word
variant might feed back into the SPSC protocol).

### As-built ladder

- `0.11.0-0` docs: mpsc ring design + plan
- `0.11.0-1` feat: mpsc ring layout + init/attach
- `0.11.0-2` feat: mpsc send_with + consumer recv
- `0.11.0-3` feat: demo mpsc flow lines
- `0.11.0-4` docs: mpsc iiac-perf numbers
- `0.11.0` feat: mpsc ring sibling primitive

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
[16]: https://github.com/winksaville/zc-ring-x1/commit/2b09687ddebd "2b09687ddebd4e6bdc0d68a12cff57ea580b959d"
[17]: https://github.com/winksaville/zc-ring-x1/commit/2069384698df "2069384698dfa62860193cea58aea62c4c5462c0"
[18]: https://github.com/winksaville/zc-ring-x1/commit/9f3ce4bc87bd "9f3ce4bc87bd86faa9941a7d40007674529dcc80"
[19]: https://github.com/winksaville/zc-ring-x1/commit/6177323244b4 "6177323244b4d80fe07a9efaa6d8647ca31b4787"
[20]: https://github.com/winksaville/zc-ring-x1/commit/67b6a553f80c "67b6a553f80c188debd62b41e47c81e11e7b2f33"
[21]: https://github.com/winksaville/zc-ring-x1/commit/31016b69c25f "31016b69c25f80a5b5d4050f2e866d0b46b5a248"
[22]: https://github.com/winksaville/zc-ring-x1/commit/044e346e40e0 "044e346e40e0da7e4e95c3761bd8a419428e4d30"
[23]: https://github.com/winksaville/zc-ring-x1/commit/2faa51644bd2 "2faa51644bd2540a4c6fbba71bae6ba21ea9e8be"
[24]: https://github.com/winksaville/zc-ring-x1/commit/606c8f9daa49 "606c8f9daa495c4cb3d21b93c24fc6c447f02b87"
[25]: https://github.com/winksaville/zc-ring-x1/commit/3226b9dc15d2 "3226b9dc15d26e71d647a659c8a7b933a66be7b2"
[26]: https://github.com/winksaville/zc-ring-x1/commit/dd5217d72434 "dd5217d724341220a2e201d4ff74c947d6ef5bf8"
[27]: https://github.com/winksaville/zc-ring-x1/commit/fe4f426b9f24 "fe4f426b9f24d6883cb2d1fd42e36ed1c09e39d9"
[28]: https://github.com/winksaville/zc-ring-x1/commit/20852d556100 "20852d556100cc862f61b0c80b05d4fef656e63c"
[29]: https://github.com/winksaville/zc-ring-x1/commit/88a696959c4e "88a696959c4e40c02a6f36e3c15a074d57daaefb"
[30]: https://github.com/winksaville/zc-ring-x1/commit/b23b06a5dae5 "b23b06a5dae5c21a94e60bcf2e94b883d146e359"
[31]: https://github.com/winksaville/zc-ring-x1/commit/e083cd429597 "e083cd4295978a75833af284e9dd1620c0ad63e8"
[32]: https://github.com/winksaville/zc-ring-x1/commit/35f144b980f7 "35f144b980f78b893892f9958c3c6b3fa0c2909e"
[33]: https://github.com/winksaville/zc-ring-x1/commit/0897d0a2edce "0897d0a2edced6719f2ce5e65cc263da7514ff47"
[34]: https://github.com/winksaville/zc-ring-x1/commit/af9dc6af764f "af9dc6af764f51c28ba9400a4f86ee4b6acee265"
[35]: https://github.com/winksaville/vc-template-x1/commit/d485bb5d34b7 "d485bb5d34b72453ccb44f16967730ace2e17dc9"
[36]: https://github.com/winksaville/zc-ring-x1/commit/3f07aec032f5 "3f07aec032f55b456912120aed8ca14a891986fe"
