# Todo

This file uses [Prose form](agent-data/prose.md#prose-form). It contains near term tasks with a
short description and uses links or reference links for more details.

## In Progress

### docs: adopt the family agent-files set

#### Problem

vc-x1 proposed one family set of agent-files, based on this repo's set as landed 2026-08-26 and
reworked as its cycle `docs: the family agent-files proposal` records (vc-x1 `main` a4309084fdfe,
the message in `vc-x1-messages/zc-ring-x1.md`). Our copy has drifted below it: it still carries
`cycle-checklists.md` and `cycle-protocol.md`, which nothing links anymore, cites `hard rule N` by
numbers that no longer exist, keeps the inline whys the set moved to rationale.md, and numbers its
`## Todo` entries. Beside that, `.vc-config.md` names this repo's family member as `vc-x1`, a copy
fossil, so the messaging acquaint check reads the wrong mailbox.

#### Solution

Adopt the set as proposed: copy vc-x1's `AGENTS.md` and `agent-data/*` over ours and delete the
two orphans, reshape `TODO.md` to the set's Todo format (entries as headings, `## Continuation
notes` and `## Waiting` added, the `fix-todo` mentions repointed), and correct the family member
name. Then answer vc-x1's record with an outcome once the cycle lands.

#### Acceptance check

`diff -r` of `AGENTS.md` and `agent-data/` against vc-x1's at `a4309084fdfe` is empty, and
`custom.md` differs only in project content. `vc-x1 validate` passes, every agent-file link
resolves, and `grep -rn "hard rule [0-9]" AGENTS.md agent-data` finds nothing. The messaging
acquaint check opens `../vc-x1-messages/zc-ring-x1.md`.

#### Ladder

- [docs: adopt the family agent-files set opening][1] (done)
- [fix(config): name this repo's family member][4] (done)
- [build: upgrade the dependencies to the latest compatible versions][6] (done)
- [docs: copy the set over the agent-files][2] (done)
- [docs: repoint the stale links into the agent-files][7]
- [docs: reshape TODO.md to the set's Todo format][3]
- [docs: adopt the family agent-files set closing][5]

#### Deliberation

- Accept the set whole, counter nothing in the copy: the rung copies vc-x1's files verbatim, so the
  three-way comparison the proposal names as its acceptance check goes empty for this member.
  - The three reservations from the review (the Size close-out step, Restart as a user step, and
    Bullet form's reach over existing prose) go in the reply as remarks, not as local edits, since a
    local edit would reopen the diff the proposal exists to close.
- The config fix is its own rung, first, not folded into the copy: it is not an agent-file, so
  `git log -- AGENTS.md agent-data/` keeps showing only rule changes, and it is the one line that
  breaks the messaging acquaint check today, so it lands before the set that runs the check.
- The dependency upgrade is a rung, not an entry: it arrived mid-opening, is small, and
  `vc-x1 validate` after it is its whole check, the rung-or-entry pick under the set's Unplanned
  work.
- The request became this block directly rather than a `## Todo` entry first: the opening is the
  same edit as moving an entry, and the record vc-x1's outcome will cite is this cycle's landmark.

#### Ladder details

##### docs: adopt the family agent-files set opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write this
block, bump the version-of-record, and rename the artifact to `-dev`.

##### fix(config): name this repo's family member

`.vc-config.md` named the family member `vc-x1`, carried over when the file was copied from that
repo, so the messaging acquaint check would have opened vc-x1's record file rather than ours.

* The `[family] member` key is the only place the member name is stated, and it was the copy's.
  - It now reads `zc-ring-x1`, so `<family.messages>/<family.member>.md` resolves to
    `../vc-x1-messages/zc-ring-x1.md`, the file vc-x1's proposal was written to.

##### build: upgrade the dependencies to the latest compatible versions

The manifests asked for `clap 4.6.1` and the lock held transitive entries behind what their reqs
allowed, so a fresh build used older code than the reqs permitted.

* The manifest reqs were behind the latest compatible releases.
  - `cargo upgrade` moved `clap` to `4.6.6` in `tp_runner` and `tp_matrix`, the only req with a
    newer compatible version. No other req had one, and nothing was held behind an incompatible
    (major) release.
* The lock's transitive entries were behind their reqs.
  - `cargo update` moved eleven entries, `zerocopy` `0.8.52` to `0.8.56` and `hdrhistogram`
    `7.5.4` to `7.6.0` among them, with `nom` `7` to `8` and `zlib-rs` arriving as `hdrhistogram`'s
    new transitive choices and `minimal-lexical` leaving with the old `nom`.

##### docs: copy the set over the agent-files

Our agent-files were the base the set was built from, and the set's diff against them is the
proposal. Taking the set whole is what makes the three-way comparison empty for this member.

* `AGENTS.md` and `agent-data/*` differed from the set in the ways the proposal lists.
  - Replaced by vc-x1's files at `a4309084fdfe`, taken from that commit rather than vc-x1's working
    tree, which had moved on. `commit-model.md` arrives, `cycle-checklists.md` and
    `cycle-protocol.md` go, and `agent-data/` and `AGENTS.md` are now byte-identical to the set.
* `custom.md` pointed at the old `## custom.md: the project layer` heading.
  - The anchor is `#custommd` now, and `custom.md` is identical to the set's too.
* `notes/README.md` linked the two retired files.
  - It names `jj.md`, `versioning.md`, and `cycle-model.md` instead. The other stale inbound
    anchors a link check found predate this rung and get the next one.

##### docs: repoint the stale links into the agent-files

A link check over the repo finds `AGENTS.md#prose-form` and kin in `ARCHITECTURE.md`,
`notes/ring-buffer-design.md`, `notes/bugs.md`, `notes/todo-backlog.md`, and `notes/jj-tips.md`,
anchors that moved into `prose.md`, `notes.md`, and `jj.md` before this cycle. Repoint the live
files. The frozen chores keep theirs.

##### docs: reshape TODO.md to the set's Todo format

`## Todo` entries become `###` headings in priority order, `## Continuation notes` and `## Waiting`
are added, and the `fix-todo` mentions in `TODO.md`, `notes/todo-backlog.md`, and `notes/bugs.md`
are repointed to the heading form.

##### docs: adopt the family agent-files set closing

Closing out the cycle.

## Todo

 Entries are in **strict priority rank**, #1 highest, descending. Reprioritize by moving an entry,
 then `vc-x1 fix-todo --no-dry-run TODO.md` to renumber. The numbers are positional rank, not stable
 IDs. To refer to a Todo, name it by its **title** (a greppable mention, since a numbered list item
 has no anchor to link to), not its number. Long-tail entries live in
 [todo-backlog.md](notes/todo-backlog.md). Use the [Prose Form in
 AGENTS.md](agent-data/prose.md#prose-form). Deeper detail goes in a `notes/` design file (link via
 `[N]` ref).

1. Descriptor queue endpoints: paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
   - own ring endpoint + registry access
   - the demo's ~20-line send path becomes ~3 lines
   - `resolve`'s unsafe is audited once inside the crate (recv safe by construction)
   - guard handed back on Full
   - design against both ring flavors (SPSC + MPSC)
   - the sender is also where each sender's private overflow pending list will live.
2. Overflow FIFO: on ring Full, append the message to a sender-private pending list instead of
   failing [details](notes/ring-buffer-design.md#overflow-fifo-future):
   - intrusive: the same embedded next-link the free-stack uses, so zero allocation
   - naturally bounded by pool capacity
   - composes per-sender with MPSC, see [Overflow
     readiness](notes/ring-buffer-design.md#overflow-readiness).
3. Seam-word SPSC variant: publish per-slot seq words so neither side ever reads the other's index
   line, Vyukov-style publish but load/store only (no CAS: the single producer's index stays
   endpoint-private) [[21]]:
   - motivation: cross-core, SPSC moves ~10.0 cache lines per round trip vs MPSC's ~6.7 and loses
     ~26 to 40%, and the whole gap is line-transfer economics
   - must keep the SMT/1t win (SPSC beats MPSC at 0,12 where transfers ≈ 0, so the protocol itself
     is cheaper)
   - costs a seq array in the layout (layout_version bump), so measure A/B with tp_roundtrip before
     adopting.
4. Batch alloc/free demo: alongside the one-message alloc_free_1t loops, a variant that allocs X
   messages (5, 10, ...) then frees them all, pool vs global allocator. We think the pool's rate
   stays constant (pop/push is O(1) regardless of live count, LIFO keeps the working set hot) while
   Box::new/drop slows as the batch outgrows malloc's thread-cache fast path, and the demo should
   show it.
5. Endpoint claims word: CAS-claimed producer/consumer roles in the ring header so a second
   attach/split claimant gets an error instead of silently violating SPSC, at the cost of a
   layout_version bump (or spends `_pad0`)
   [details](notes/ring-buffer-design.md#resolved-questions).
6. Typed endpoints: `Producer<T>` / `Consumer<T>` validating `T`'s geometry once at split instead of
   asserting on every reserve_slot_with [details](notes/ring-buffer-design.md#api).

## Ideas

- Perf benches live in [iiac-perf](https://github.com/winksaville/iiac-perf) (sibling repo
  `../iiac-perf`), not here. Its calibrated harness compares zc-ring against mpsc et al. directly
  (`zcring-1t`/`zcring-2t` mirroring `mpsc_1t`/`mpsc_2t`). An in-repo bench only if per-commit
  regression tracking proves necessary.

- Fan-in helper: consumer-side composition polling N SPSC rings under a pluggable service policy
  (priority, round-robin, weighted)
  [details](notes/ring-buffer-design.md#fan-in-composition-not-a-mode):
  - buildable today from shipped parts
  - likely offered alongside the MPSC ring eventually, no commitment yet.
- Study [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) before implementing message pools:
  battle-tested loan/send decoupling and pool-offset machinery. How it differs from this project is
  in [Prior art: iceoryx2](notes/ring-buffer-design.md#prior-art-iceoryx2).
- `#[global_allocator]` experiment over size-class pools: GlobalAlloc is `&self` + any-thread, so it
  needs shared-allocation pools (phase 2 gen-tagged head) or per-thread pools with a routing layer,
  and arbitrary `Layout` needs size-class selection + an oversize fallback. Frees from any thread
  are already natural (MPSC push). Classic mempool -> malloc arc. Measure the object-pool form in
  iiac-perf first.
- Private per-handle cache in front of the shared free-stack (tcache-over-arenas): alloc/free hit a
  thread-private list with plain load/store, and refill/flush moves batches to the CAS stack,
  amortizing one CAS over N messages. Motivating datum: 2 uncontended CAS = 8.7 ns of the pool's 9.9
  ns single-thread round trip (vs malloc tcache's zero atomics). Hold until iiac-perf shows per-op
  CAS matters in a composed workload. We think the pool's tail latency (p99, stddev) already beats
  malloc (no arena locks, no brk/mmap), and that matters more than the mean.
- `Message` trait over the payload cast boilerplate: const `MSG_ID` + the zerocopy bounds,
  receiver-side dispatch (read tag, match, cast) without per-call-site ceremony, and maybe a
  transport seam so an embedded pointer-descriptor profile slots in behind the same API
  [details](notes/ring-buffer-design.md#descriptor-and-registry-design-070).
- BufSlot auto-free on Drop (RAII, iceoryx2-style): kills the silent leak-on-drop footgun at the
  cost of guard-type asymmetry (ring guards' drop = do-nothing) and a ManuallyDrop dance in
  free/send paths. Decide when descriptor-queue send lands, since explicit free is easier to upgrade
  than to walk back.
- Blocking layer above the crate (futex, eventfd, async wakers) built on the header's user line,
  mechanism and contracts in [Blocking and user
  words](notes/ring-buffer-design.md#blocking-and-user-words). Possibly a companion wrapper crate so
  independent peers share one protocol.
- loom-based exhaustive ordering exploration of the SPSC protocol.
- Polish: `Error` implements `Display` + `core::error::Error`, and `occupancy()` / `is_empty()`
  accessors.
- Packed-slot variant (drop the cache-line-multiple slot constraint) for small-message space
  efficiency.
- Per-target / configurable `CACHE_LINE` (128 for Apple M-series false sharing, tiny for cache-less
  MCUs), safe since attach validates the header's `cache_line`. Decide values from iiac-perf
  measurements.
- Embedded floor: protocol is atomic load/store only (no CAS), so thumbv6m works today. Keep it that
  way where possible (endpoint claims wants CAS, so gate it), and 8/16-bit targets would need
  index-width genericization.
- Shared `Geometry` struct (`slot_size`, `capacity`, `mask`) held by Ring and passed whole to the
  endpoint constructors, slimming their signatures and Ring's fields.
- Black-box test split: move the public-API protocol tests (roundtrip, abandoned guards, threaded
  stress) to `tests/protocol.rs`, while white-box tests (u32 wrap, attach header internals) stay in
  lib.rs. Do it when a trybuild compile-fail harness lands there too (pins the "second reservation
  does not compile" guarantee).

# References

[1]: #docs-adopt-the-family-agent-files-set-opening
[2]: #docs-copy-the-set-over-the-agent-files
[3]: #docs-reshape-todomd-to-the-sets-todo-format
[4]: #fixconfig-name-this-repos-family-member
[5]: #docs-adopt-the-family-agent-files-set-closing
[6]: #build-upgrade-the-dependencies-to-the-latest-compatible-versions
[7]: #docs-repoint-the-stale-links-into-the-agent-files
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: notes/chores/chores-02.md#findings-the-gap-is-line-transfer-economics
