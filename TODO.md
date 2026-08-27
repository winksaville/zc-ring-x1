# Todo

This file uses [Prose form](agent-data/prose.md#prose-form). It
contains near term tasks with a short description and
uses links or reference links for more details.

## In Progress

### docs: fix close-out and sweep punctuation

#### Problem

The last cycle was single-step and its close-out went wrong: the work-repo commit carried the
bare title while the agent-repo carried the title plus " closing", the `ochid:` trailers did
not match, and the agent-repo held two commits for the one rung. The repair was by hand
(`jj desc -R .claude`, a squash, `vc-x1 validate-desc`). The cycle's record also never
reached `notes/chores/` or a `## Done` entry: the In Progress block was deleted at close-out
instead of moved. The agent-files describe the close-out for the multi-step shape and leave the
single-step case to two asides (cycle-model.md, prose.md), so the agent improvised. Beneath
that, the record the close-out was meant to write (the chores move, the `## Done` entry, the
`done.md` migration, the SHA backfill) was a second copy of what jj holds, and each copy was a
place to slip.

#### Solution

The agent-files gained a `Cycle shape` section: single-step or multi-step is decided at the
opening and fixed by the first push, a single-step cycle does all of it in its one commit
under the bare title in both repos, a pushed single-step commit can be promoted to an opening
only by a coordinated re-describe, and a ladder lands as trapezoid or keep separate, `squash`
retired because rung `ochid:` trailers are change ids. They gained `Cycle-record`: the In
Progress block is the cycle's only record, finalized and moved to `## Closed` by the closing
commit and deleted by the next opening, nothing backfilled, `notes/chores/` and `notes/done.md`
frozen as history. Every Opening and Close-out surface (AGENTS.md, cycle-checklists.md,
cycle-protocol.md, cycle-model.md, notes.md, prose.md, jj.md, versioning.md, rationale.md)
was rewritten to those two rules, the superseded `notes/cycle-protocol.md` and
`notes/versioning.md` removed, and README, ARCHITECTURE, notes/README, and done.md repointed.

#### Acceptance check

Reading AGENTS.md's Close-out and its linked files answers, for a single-step cycle, each of:
the commit title (bare, identical in both repos), the number of pushes (one), the record's
destination, and the Land sequence, without consulting the multi-step case by analogy. No
agent-file offers `squash` as a close-out shape, and the shape is named as decided at the
opening. The next single-step cycle lands with `vc-x1 validate-desc -n 3` clean in both repos
and identical titles.

#### Ladder

- [docs: fix close-out and sweep punctuation opening][1] (done)
- [docs: cycle shape and cycle-record rules][2] (done)
- [docs: retire the notes copies and sweep their semicolons][3] (done)
- [docs: sweep typeable punctuation][4] (done)
- [docs: rewrap the agent-files to the prose width][6]
- [docs: fix close-out and sweep punctuation closing][5]

#### Deliberation

- Shape: multi-step, decided before the first push, after two reshapes.
  - Drafted multi-step, reshaped to single-step when the shape rule made the first push the
    deciding moment, reshaped back when the work grew to four subjects (the shape rules, the
    cycle-record, the notes cleanup, the punctuation sweeps) that no one commit body could
    carry honestly.
  - Nothing had been pushed on either bookmark, so the shape was still open.
  - The single-step working copy is a local stash commit on the first bookmark,
    `docs-fix-single-step-close-out`, which stays until Land. The rungs pull their files
    from it.
- Title: `docs: fix close-out and sweep punctuation`, changed with the reshape from
  `docs: fix single-step close-out`.
  - The sweeps and the cycle-record are half the work, and the bookmark carries the slug.
- Squash retired as a close-out shape, and the shape fixed at the first push.
  - The `ochid:` trailers are change ids. Squashing a work-repo ladder keeps one chid and
    drops the rest, so every agent-repo commit that pointed at a dropped rung dangles
    (`validate-desc` reports `not found`).
  - Repairing that means re-describing N agent-repo commits, which the rules forbid, and
    squashing the agent-repo to match discards the per-rung session narrative it exists to
    keep.
  - Trapezoid and keep separate keep every chid alive, and `git log --first-parent` already
    reads the trapezoid as one commit per cycle, which is what squash was buying.
  - Folded into this cycle rather than its own because it is the same subject, close-out
    shape, in the same files.
- Record retirement: Todo "Simplify the cycle record" folded in, at the user's call.
  - First held out as its own cycle. Writing the single-step close-out out as explicit steps
    showed the chores move, the `## Done` entry, and the backfill were copies of what jj
    holds.
  - The existing `notes/chores/` and `notes/done.md` freeze as history rather than being
    removed, since Todo entries link into them.
- `## Closed` instead of deletion, one deviation from that Todo's wording.
  - A commit that finalizes and deletes the block in one diff leaves the final form in no
    tree, so the closing commit moves it to `## Closed` and the next opening deletes it,
    which keeps the file from growing.
  - A finished cycle left under `## In Progress` reads as a lie, and `tmp/` is gitignored,
    which is the no-tree loss again.
- Re-wrap rung: added at the second rung, last before the closing, its own rung.
  - Eight agent-files carry lines over the 100-column prose width and cycle-protocol.md is
    wrapped at the old 60 to 75 width nearly throughout. The width rule re-wraps text when
    touched, and this cycle touches every agent-file, so the re-flow is due.
  - Its own rung because a re-flow touches nearly every line and would bury the substantive
    diffs. Last because the punctuation sweep changes line lengths and any earlier re-flow
    would be redone.
  - Scope is the agent-files and TODO.md. Other touched files are considered once the diff
    shows what the sweep actually reaches.
- Acceptance check: a reading test rather than a run.
  - The only run is the next single-step cycle, recorded as the check's second half. This
    cycle, being multi-step, exercises the multi-step path instead, the trapezoid included.

#### Ladder details

##### docs: fix close-out and sweep punctuation opening

The cycle's setup commit: create and publish the bookmark, move the Todo entry into this block,
bump the version-of-record, and rename the package to its dev name.

* Eleven zero-byte read-only dotfiles (`.bashrc`, `.gitconfig`, `.idea`, `.vscode`, ...) appear
  at the repo root and show as added in every `jj st`. They are bind mounts the agent sandbox
  places there, so jj cannot delete them, and a checkout of a tree that tracks them to one that
  does not leaves the working copy stale.
  - `.gitignore` names them and they are untracked, so no tree tracks them and no checkout has
    to remove them.

##### docs: cycle shape and cycle-record rules

The agent-files say what a single-step cycle is and that it keeps the bare title, but nowhere
say what its one commit does at close-out, offer a `squash` close-out shape that breaks the
`ochid:` links, and keep a cycle's record in three copies (the block, the chores move, the
`## Done` entry). Write `Cycle shape` and `Cycle-record` into AGENTS.md and the files it links,
and rewrite every Opening and Close-out surface to them.

* The single-step close-out was left to analogy with the multi-step one.
  - `Cycle shape`: single-step or multi-step is decided at the opening and fixed by the first
    push, a single-step cycle does all of it in its one commit under the bare title in both
    repos, and a pushed single-step commit becomes an opening only by a coordinated
    re-describe. Opening and Close-out each open with the single-step case.
* `squash` was offered as a close-out shape, and its rung `ochid:` links are change ids that a
  squash discards.
  - Retired from every shape list, Land, the preview paragraph, and the recipe. Trapezoid and
    keep separate remain.
* The record was kept in three copies, each a place to slip: the block, the chores move with
  its four transforms, and the `## Done` entry with its `done.md` migration and SHA backfill.
  - `Cycle-record`: the block is the only record, moved whole to `## Closed` by the closing
    commit and deleted by the next opening. Rungs carry no `[[N]]` placeholder, SHA, or
    version. Design findings go to `notes/` files by the rung that made them. `notes/chores/`
    and `notes/done.md` freeze as history. notes.md loses Done entry form, Retiring Done
    entries, and Chores conventions, and gains Frozen history.
* Rules were named by counts and descriptions: "the six items", "The cycle record", a
  deliberation with no stated shape.
  - `cycle-record` is the term and the heading, "six" is gone from every rule sentence, and
    the deliberation is one bullet per decision with the reasons as sub-bullets.
* The typeable-punctuation rule contradicted itself on history: "no convert-on-touch" and
  "converts when touched" in one section.
  - Convert-on-touch, the same as the semicolon rule.

##### docs: retire the notes copies and sweep their semicolons

`notes/cycle-protocol.md` and `notes/versioning.md` are pre-agent-data copies of files now
under `agent-data/`, and README, ARCHITECTURE, notes/README, and done.md describe chores as
the live record and link into the copies. Remove the copies, repoint the four, and convert the
prose semicolons the touch rule makes due in the files this rung edits.

* Two files under `notes/` duplicated agent-data files, with anchors that went stale when the
  agent-data copies were rewritten.
  - Removed. README's Contributing section, ARCHITECTURE's file map, notes/README's workflow
    section, and done.md's header point at AGENTS.md, `agent-data/`, and the frozen history
    instead, and README's ochid and config links follow their targets.
* README.md, TODO.md, ARCHITECTURE.md, and notes/README.md carried prose semicolons, due under
  the touch rule once this cycle edits them.
  - Converted with the joins the rule names: a period for two claims, a comma with a
    conjunction for a continuation, bare sub-bullets for a list. Fenced code keeps its own.

##### docs: sweep typeable punctuation

The files this cycle touches carry about seventy em dashes, en dashes, ellipses, and arrows,
which the typeable-punctuation rule makes due on touch. Convert each with the structural
decision the rule asks for, re-pointing any anchor a heading conversion moves.

* README.md, TODO.md, ARCHITECTURE.md, and notes/README.md carried 71 banned characters, all
  authored: em dashes almost throughout, one en dash in a range, two ellipses, and two
  arrows.
  - A bullet whose label and body share a line takes a colon (the file maps, the lifecycle
    states, the test commands). A prose aside takes a comma, parentheses, or a second
    sentence. The range reads "26 to 40%", the ellipses become `...`, and the arrow `->`.
  - No heading carried one, so no anchor moved. The one em dash left is transcribed
    `iiac-perf` output inside a fence, which the rule keeps.

##### docs: rewrap the agent-files to the prose width

Eight agent-files carry lines over the 100-column prose width, and cycle-protocol.md is
wrapped at the old 60 to 75 width nearly throughout. Re-flow the agent-files and TODO.md to
the width in prose.md's Line widths with a markdown-aware pass that leaves headings, fenced
code, tables, reference definitions, and long URLs alone, checked by comparing the word
sequence before and after.

##### docs: fix close-out and sweep punctuation closing

Closing out the cycle.

## Closed

_None._

## Todo

 Entries are in **strict priority rank**, #1 highest,
 descending. Reprioritize by moving an entry, then
 `vc-x1 fix-todo --no-dry-run TODO.md` to renumber.
 The numbers are positional rank, not stable IDs. To refer
 to a Todo, name it by its **title** (a greppable mention,
 since a numbered list item has no anchor to link to), not its
 number. Long-tail entries
 live in [todo-backlog.md](notes/todo-backlog.md). Use the
 [Prose Form in AGENTS.md](agent-data/prose.md#prose-form). Deeper
 detail goes in a `notes/` design file (link via `[N]` ref).

1. Descriptor queue endpoints: paired DescSender (loan +
   send) / DescReceiver (recv) [[11]]:
   - own ring endpoint + registry access
   - the demo's ~20-line send path becomes ~3 lines
   - `resolve`'s unsafe is audited once inside the crate
     (recv safe by construction)
   - guard handed back on Full
   - design against both ring flavors (SPSC + MPSC)
   - the sender is also where each sender's private
     overflow pending list will live.
2. Overflow FIFO: on ring Full, append the message to a
   sender-private pending list instead of failing
   [details](notes/ring-buffer-design.md#overflow-fifo-future):
   - intrusive: the same embedded next-link the
     free-stack uses, so zero allocation
   - naturally bounded by pool capacity
   - composes per-sender with MPSC, see
     [Overflow readiness](notes/ring-buffer-design.md#overflow-readiness).
3. Seam-word SPSC variant: publish per-slot seq words so
   neither side ever reads the other's index line,
   Vyukov-style publish but load/store only (no CAS: the
   single producer's index stays endpoint-private) [[21]]:
   - motivation: cross-core, SPSC moves ~10.0 cache lines
     per round trip vs MPSC's ~6.7 and loses ~26 to 40%, and the
     whole gap is line-transfer economics
   - must keep the SMT/1t win (SPSC beats MPSC at 0,12
     where transfers ≈ 0, so the protocol itself is cheaper)
   - costs a seq array in the layout (layout_version bump),
     so measure A/B with tp_roundtrip before adopting.
4. Batch alloc/free demo: alongside the one-message
   alloc_free_1t loops, a variant that allocs X messages
   (5, 10, ...) then frees them all, pool vs global
   allocator. We think the pool's rate stays constant
   (pop/push is O(1) regardless of live count, LIFO keeps
   the working set hot) while Box::new/drop slows as the
   batch outgrows malloc's thread-cache fast path, and the
   demo should show it.
5. Endpoint claims word: CAS-claimed producer/consumer roles
   in the ring header so a second attach/split claimant gets
   an error instead of silently violating SPSC, at the cost of a
   layout_version bump (or spends `_pad0`)
   [details](notes/ring-buffer-design.md#resolved-questions).
6. Typed endpoints: `Producer<T>` / `Consumer<T>` validating
   `T`'s geometry once at split instead of asserting on every
   reserve_slot_with [details](notes/ring-buffer-design.md#api).

## Ideas

- Perf benches live in
  [iiac-perf](https://github.com/winksaville/iiac-perf)
  (sibling repo `../iiac-perf`), not here. Its calibrated
  harness compares zc-ring against mpsc et al. directly
  (`zcring-1t`/`zcring-2t` mirroring `mpsc_1t`/`mpsc_2t`).
  An in-repo bench only if per-commit regression tracking
  proves necessary.

- Fan-in helper: consumer-side composition polling N SPSC
  rings under a pluggable service policy (priority,
  round-robin, weighted)
  [details](notes/ring-buffer-design.md#fan-in-composition-not-a-mode):
  - buildable today from shipped parts
  - likely offered alongside the MPSC ring eventually, no
    commitment yet.
- Study [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2)
  before implementing message pools: battle-tested loan/send
  decoupling and pool-offset machinery. How it differs from
  this project is in
  [Prior art: iceoryx2](notes/ring-buffer-design.md#prior-art-iceoryx2).
- `#[global_allocator]` experiment over size-class pools:
  GlobalAlloc is `&self` + any-thread, so it needs
  shared-allocation pools (phase 2 gen-tagged head) or
  per-thread pools with a routing layer, and arbitrary `Layout`
  needs size-class selection + an oversize fallback. Frees
  from any thread are already natural (MPSC push). Classic
  mempool -> malloc arc. Measure the object-pool form in
  iiac-perf first.
- Private per-handle cache in front of the shared free-stack
  (tcache-over-arenas): alloc/free hit a thread-private list
  with plain load/store, and refill/flush moves batches to the
  CAS stack, amortizing one CAS over N messages. Motivating
  datum: 2 uncontended CAS = 8.7 ns of the pool's 9.9 ns
  single-thread round trip (vs malloc tcache's zero
  atomics). Hold until iiac-perf shows per-op CAS matters in
  a composed workload. We think the pool's tail latency
  (p99, stddev) already beats malloc (no arena locks, no
  brk/mmap), and that matters more than the mean.
- `Message` trait over the payload cast boilerplate: const
  `MSG_ID` + the zerocopy bounds, receiver-side dispatch
  (read tag, match, cast) without per-call-site ceremony,
  and maybe a transport seam so an embedded
  pointer-descriptor profile slots in behind the same API
  [details](notes/ring-buffer-design.md#descriptor-and-registry-design-070).
- BufSlot auto-free on Drop (RAII, iceoryx2-style): kills the
  silent leak-on-drop footgun at the cost of guard-type
  asymmetry (ring guards' drop = do-nothing) and a
  ManuallyDrop dance in free/send paths. Decide when
  descriptor-queue send lands, since explicit free is easier to
  upgrade than to walk back.
- Blocking layer above the crate (futex, eventfd, async
  wakers) built on the header's user line, mechanism and
  contracts in
  [Blocking and user words](notes/ring-buffer-design.md#blocking-and-user-words).
  Possibly a companion wrapper crate so independent peers
  share one protocol.
- loom-based exhaustive ordering exploration of the SPSC
  protocol.
- Polish: `Error` implements `Display` + `core::error::Error`,
  and `occupancy()` / `is_empty()` accessors.
- Packed-slot variant (drop the cache-line-multiple slot
  constraint) for small-message space efficiency.
- Per-target / configurable `CACHE_LINE` (128 for Apple
  M-series false sharing, tiny for cache-less MCUs), safe
  since attach validates the header's `cache_line`. Decide
  values from iiac-perf measurements.
- Embedded floor: protocol is atomic load/store only (no
  CAS), so thumbv6m works today. Keep it that way where
  possible (endpoint claims wants CAS, so gate it), and 8/16-bit
  targets would need index-width genericization.
- Shared `Geometry` struct (`slot_size`, `capacity`, `mask`)
  held by Ring and passed whole to the endpoint
  constructors, slimming their signatures and Ring's fields.
- Black-box test split: move the public-API protocol tests
  (roundtrip, abandoned guards, threaded stress) to
  `tests/protocol.rs`, while white-box tests (u32 wrap, attach
  header internals) stay in lib.rs. Do it when a trybuild
  compile-fail harness lands there too (pins the
  "second reservation does not compile" guarantee).

# References

[1]: #docs-fix-close-out-and-sweep-punctuation-opening
[2]: #docs-cycle-shape-and-cycle-record-rules
[3]: #docs-retire-the-notes-copies-and-sweep-their-semicolons
[4]: #docs-sweep-typeable-punctuation
[5]: #docs-fix-close-out-and-sweep-punctuation-closing
[6]: #docs-rewrap-the-agent-files-to-the-prose-width
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: notes/chores/chores-02.md#findings-the-gap-is-line-transfer-economics
