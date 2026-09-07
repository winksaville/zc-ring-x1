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

### In-slot seq for spsc v1

The seam-word ring's seq words live in their own line, and v1 is slower per send than the MPSC
producer at every placement, the puzzle [SPSC v1: seam-word
ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) leaves open. Put the seq in the slot's
own line, a crate-owned slot header ahead of the user-owned body, and measure again on both
machines, `tp-matrix` and the demo's streams against v0 and MPSC:
- starts from the probe rung's baseline: packed seq array, neither side waiting, v0 ahead within an
  L3 and v1 ahead across one
- the prediction on record: it helps the round trip and may hurt streaming, since the slot line
  would then travel both ways every message where today the seq line amortises
- the cheap probe goes first: a fence after v1's commit, one line, to test the store-buffer reading
- the seq's width is measured (u32 against the native width) before the slot header fixes it
- the slot contract changes (the body sits behind the crate-sized header), so its shape is part of
  the finding.

### Segmented queue over spsc v1

The rings are fixed-length and a Full ring fails the send. A chain of v1 ring segments allocated
from a `Pool`, designed in [SPSC v1: seam-word
ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) and deferred when the ring's numbers
missed the bar:
- pool segments: take a pool buffer as the byte region a v1 `Ring::init` wants, sized by
  `region_size(slot_size, M)`, and nothing else the pool does not already have
- the chain: producer and consumer endpoints, the link word, the segment switch on Full and on
  Empty, freeing the drained segment, and a boundary-crossing test that returns every segment
- the size sweep: `M` from 1 to 256 in the cell, the seam cost as the slope, recorded with the
  default the crate picks
- supersedes Overflow FIFO if it lands
- goes ahead once a v1 form clears the bar, faster than MPSC v0 cross-core.

### Demo pin-pair picker

The demo's "diff cores" pair is the first cpu outside cpu0's L3, cross-L3 on the 3900X and same-L3
on the 7600X, so the two machines' lines with the same label were different experiments. The picker
wants a same-L3 placement and a cross-L3 one, each labelled by what it is, and the same for
`tp-matrix`'s placements.

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

## Closed

The last cycle's finished record, moved here whole by its closing commit and deleted by the next
opening ([Cycle-record](AGENTS.md#cycle-record)). Earlier cycles are in the landmark commit's copy
of this section, and the cycles before the rule in the frozen [notes/chores/](notes/chores) and
[notes/done.md](notes/done.md).

### agent-files(adoption): v0.2.3

#### Problem

The family's agreed agent-files are `v0.2.3` and we carry the pre-versioning set of 2026-08-28,
the copy iiac-perf took as the base of `v0.1.0`. Every set cycle since is unadopted here: set
versioning, the declared commit types, the Todo section order, the agent-dir lookup, and
`v0.2.3`'s eight corrections. `agent-data/messaging.md` is still carried although the accepted
messages rules moved it out of the set.

#### Solution

Copy iiac-perf's set at `d5d5e77a3bb1` byte for byte with `vc-x1 agent-files copy ../iiac-perf
-c`: `AGENTS.md`, `agent-data/*`, and `custom.md`, the marker `agent-files-v0.2.3` arriving and
`messaging.md` going. `TODO.md` is reordered to the Todo format the set now states, `## Closed`
below `## Bugs`. Nothing in the set is bumped: an adoption copies the source's version file.

#### Acceptance check

`vc-x1 agent-files diff ../iiac-perf -c` reports 0 differing, `ls agent-data` shows
`agent-files-v0.2.3` and no `messaging.md`, `vc-x1 agent-files version` prints `v0.2.3`,
`TODO.md`'s sections stand in the Todo format's order, and `vc-x1 validate` passes.

#### Ladder

- agent-files(adoption): v0.2.3 (done)

#### Deliberation

- Single-step: an adoption is a copy, and the diff is the family's work, reviewed twice by vc-x1
  before it landed.
- `v0.2.2` is skipped: its record asked for it, and `v0.2.3` supersedes it, so one copy takes both.
- `custom.md` is copied with `-c` on the user's instruction. It was already identical, the one
  messaging pointer line the family shares.
- The cycle's pushes, the bookmark and the commit, run under the user's delegation of 2026-09-07,
  "do the single-step cycle ... leave in the branch until I review", and Land waits on that
  review. That delegation is the waiver for the per-push approvals.
- No restart between the SPSC v1 landing and this cycle, on the user's call, so the session runs
  the flow under the set it started in and enacts one new rule in the file itself, the Todo
  section order, since an adopter's `TODO.md` must have the shape the adopted set states.
- Acceptance check: pass. `vc-x1 agent-files diff ../iiac-perf -c` reports 0 of 11 differing, the
  marker is the only non-`.md` file in `agent-data`, the set is 2315 lines, iiac-perf's own count
  for `v0.2.3`, and validation passes.

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: notes/chores/chores-02.md#findings-the-gap-is-line-transfer-economics
[1]: #feat-segmented-seam-word-spsc-v1-opening
[2]: #feat-add-the-spsc-v1-seam-word-ring
[3]: #perf-measure-spsc-v1-against-v0-and-mpsc
[7]: #feat-seam-word-spsc-v1-closing
[9]: #perf-probe-the-v1-streaming-loss
