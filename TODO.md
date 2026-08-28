# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

_None._

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

_No cycle currently in progress._

## Closed

The last cycle's finished record, moved here whole by its closing commit and deleted by the next
opening ([Cycle-record](AGENTS.md#cycle-record)). Earlier cycles are in the landmark commit's copy
of this section, and the cycles before the rule in the frozen [notes/chores/](notes/chores) and
[notes/done.md](notes/done.md).

### docs: keep the ladder markers in the closed block

#### Problem

The close-out's step 2 said to drop the `(current)` / `(done)` markers from the ladder before the
block moves to `## Closed`, and no rationale entry said why. The markers have value, so a step that
strips them loses it, and the record lands as a plain title list rather than the ladder as it was
worked.

#### Solution

The step is gone from `AGENTS.md`, `notes.md`'s ladder item says the markers stay when the block
moves, and `rationale.md`'s Close-out carries the why, which the earlier rule never had. A
single-step cycle, its one commit carrying the opening's duties, the change, and this record.

#### Acceptance check

`grep -n "drop the" AGENTS.md` finds nothing, this block's ladder rung still reads `(done)` in
`## Closed`, and `vc-x1 validate` passes.

Result: pass. The grep is empty, the rung below carries its marker, and validation passed.

#### Ladder

- docs: keep the ladder markers in the closed block (done)

#### Deliberation

- The change goes to the set's copy of the agent-files, not `custom.md`: the reason is not
  project-specific, so the diff against the payload is the proposal, per Changing the agent-files.
- Single-step: one line goes, one line and one rationale bullet arrive, and no step needs its own
  review.
- No `-dev` rename: the opening's rename and Land's restore would be one no-op in the one commit.
- The why is recorded as given, "the markers have value so they stay", with the reading that a
  closed block whose rungs all read `(done)` is a check in itself.

## Waiting`, `## Todo`, `## Ideas`,
    `## Bugs`. The three that hold nothing read `_None._`.
* The six `## Todo` entries were numbered list items, `N. Title: text`.
  - Each is a `###` heading titled by its lead phrase, its text the paragraph below, sub-bullets
    kept. Priority is file order, and the `fix-todo` instruction is gone from the intro. One
    untypeable character in an entry (`transfers are about 0`) converted with it.
* `## Ideas` entries stay bullets.
  - Unranked and never cited, so an anchor buys nothing there, and the intro now says how they are
    triaged, as vc-x1's does.
* `notes/todo-backlog.md`'s header described the rank-and-renumber scheme and `fix-todo`.
  - Rewritten to the set's wording, its dashes with it. It holds no entries. `notes/bugs.md` keeps
    its numbered entries and `fix-todo` note, since the set only asks that `## Bugs` point at it,
    and vc-x1's `bugs.md` is numbered too.

##### docs: adopt the family agent-files set closing

Closing out the cycle: run the acceptance check, finalize this block and move it to `## Closed`,
record the agent-files size, and bring the version-of-record to the bare `0.15.3`.

* The set's Close-out gained a Size step, which this repo had no file for.
  - `notes/agent-files-size.md` is created in vc-x1's shape with this cycle as its first row,
    2126 lines over 11 files, the same count as the set since the files are identical.
* Nothing in the block needs to outlive the cycle beyond what the size note now holds.
  - The link check that found the stale anchors was run by hand and is not kept, since the set's
    `validate` entry (vc-x1's `## Todo`) is the durable home for it.
* Close-out shape: trapezoid, the default. The ladder is five subjects, and `main` reads better as
  one commit per cycle with the rungs reachable behind it.
* `# Todo` over `## Todo`, and `# Bugs` over `## Bugs` in `notes/bugs.md`, slug to the same anchor,
  so a link to the section landed on the file's title (found by vc-x1, 2026-08-28).
  - The titles became `# Todo and cycle record` and `# Known bugs`, amended into this commit
    before the landing since the bookmark was still a draft.


## Waiting

Important work that cannot start yet. Each entry names what it waits on, in a form that can be
checked, and the rank it takes in `## Todo` once unblocked. Every opening checks each condition
and promotes what is met ([Opening](AGENTS.md#opening)).

_None._

## Todo

Entries are in priority order, the first highest, and reprioritizing is moving an entry. Each is a
`###` heading, so a citation is a link to its anchor. Long-tail entries live in
[todo-backlog.md](notes/todo-backlog.md). Use the [Prose form](agent-data/prose.md#prose-form).
Deeper detail goes in a `notes/` design file (link via `[N]` ref).

### Descriptor queue endpoints

Paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
- own ring endpoint + registry access
- the demo's ~20-line send path becomes ~3 lines
- `resolve`'s unsafe is audited once inside the crate (recv safe by construction)
- guard handed back on Full
- design against both ring flavors (SPSC + MPSC)
- the sender is also where each sender's private overflow pending list will live.

### Overflow FIFO

On ring Full, append the message to a sender-private pending list instead of failing
[details](notes/ring-buffer-design.md#overflow-fifo-future):
- intrusive: the same embedded next-link the free-stack uses, so zero allocation
- naturally bounded by pool capacity
- composes per-sender with MPSC, see [Overflow
  readiness](notes/ring-buffer-design.md#overflow-readiness).

### Seam-word SPSC variant

Publish per-slot seq words so neither side ever reads the other's index line, Vyukov-style publish
but load/store only (no CAS: the single producer's index stays endpoint-private) [[21]]:
- motivation: cross-core, SPSC moves ~10.0 cache lines per round trip vs MPSC's ~6.7 and loses
  ~26 to 40%, and the whole gap is line-transfer economics
- must keep the SMT/1t win (SPSC beats MPSC at 0,12 where transfers are about 0, so the protocol
  itself is cheaper)
- costs a seq array in the layout (layout_version bump), so measure A/B with tp_roundtrip before
  adopting.

### Batch alloc/free demo

Alongside the one-message alloc_free_1t loops, a variant that allocs X messages (5, 10, ...) then
frees them all, pool vs global allocator. We think the pool's rate stays constant (pop/push is O(1)
regardless of live count, LIFO keeps the working set hot) while Box::new/drop slows as the batch
outgrows malloc's thread-cache fast path, and the demo should show it.

### Endpoint claims word

CAS-claimed producer/consumer roles in the ring header so a second attach/split claimant gets an
error instead of silently violating SPSC, at the cost of a layout_version bump (or spends `_pad0`)
[details](notes/ring-buffer-design.md#resolved-questions).

### Typed endpoints

`Producer<T>` / `Consumer<T>` validating `T`'s geometry once at split instead of asserting on every
reserve_slot_with [details](notes/ring-buffer-design.md#api).

## Ideas

Unranked, not yet solid enough for `## Todo`. Triaged at an opening: promoted to `## Todo` or
[todo-backlog.md](notes/todo-backlog.md), folded into a picked-up cycle, or dropped.

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

## Bugs

_See [bugs.md](notes/bugs.md)._

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: notes/chores/chores-02.md#findings-the-gap-is-line-transfer-economics
