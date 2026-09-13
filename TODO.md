# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

- No cycle is open. `feat: cordyceps MpscQueue beside the mpsc rings` landed as a trapezoid and
  `docs: map the workspace and its tools` as one commit on 2026-09-12, the demo installed at
  0.15.11. `../vc-x1-messages` is pushed, m-3-5 and m-5-2 among its lines, and nothing was pending
  for us there when the session ended.
- The next cycle, agreed with the user on 2026-09-12 and not yet opened: `cordyceps-ex-1`, a
  workspace member beside `tp_matrix`, not a new repo. The cordyceps contract tests and the
  pool-buffer node adapter from `tp_matrix/src/pool.rs` move into it, with a minimal one-producer,
  one-consumer pool example. The root crate drops its cordyceps dev-dependency, and `tp-pool`
  keeps its cordyceps row by depending on the new crate for the adapter. The prior-art section may
  move to the crate's own notes, as `tprobe` keeps its own. A short ladder at 0.15.12, no `-dev`
  rename. Its opening writes the In Progress block directly, no Todo entry exists for it.
- Direction, the user's call on 2026-09-12: the demo stays as it is, with no action list or help
  CLI here. The positional action list and help, the pool and depth sweeps, and cordyceps as a
  bench belong in iiac-perf, which takes `tp_matrix/src/pool.rs` and the design note's
  `Measured: pool-message sweep` section as its model and cross-check. The handoff is a messages
  thread to iiac-perf with sha-links to the landed commits, not yet opened.
- Caveats: the `tp-pool` cordyceps consumer spins on `Inconsistent` with no bound, so a preempted
  producer stalls it. The first two full sweeps on 2026-09-12 read about twice slow, we think from
  load outside the sandbox, so a full sweep counts only when a second full run agrees. The `-dev`
  binaries from the cordyceps cycle are still in `~/.cargo/bin`.
- `tmp/intrusive-rust-link-lists.md`, the linked-list question, is answered by the landed cordyceps
  cycle and can be deleted.

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

### Segmented queue SPSC v3

Every ring is fixed-length and Full fails the send. The segment chain was designed for v1 in [SPSC
v1: seam-word ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) and held until a ring
cleared the bar, v2 cleared it on 2026-09-07, so the layer goes on v2. The cycle, drafted on
2026-09-08 and not yet agreed, runs as `feat: segmented queue SPSC v3`:
- Solution: a `spsc::v3` sibling module whose queue is a chain of v2 ring segments, each one pool
  buffer holding a v2 region.
  - Geometry: `Queue::init(region, slot_size, seg_capacity, seg_count)` builds a `Pool` over the
    caller's region with buffer size `v2::region_size(slot_size, seg_capacity)` and `seg_count`
    buffers, takes the first segment, and splits into `Producer` and `Consumer`. Total capacity is
    the product, and the pool bound is the queue bound, so Full means the pool is exhausted.
  - Endpoints: the producer holds the `Pool` plus a v2 producer over its current segment, the
    consumer a `PoolResolver` plus a v2 consumer over its segment. The guards are v2's `WriteSlot`
    and `ReadSlot` re-exported, so the hot path inside a segment is v2's untouched.
  - Link word: word 0 of the segment header's `user` line holds the next segment's pool buffer
    index, sentinel `u32::MAX` for none. Not the free-stack word, which is buffer word 0 and
    doubles as the v2 magic, so a free scribbling it costs nothing.
  - Producer on Full: try the current segment once, on Full allocate a segment, `v2::Ring::init`
    it, store its index in the old segment's link with Release, move. Pool exhausted is the queue's
    Full, and the wait policy runs over both retries, the current segment first.
  - Consumer on Empty: try the current segment once, on Empty load the link with Acquire. A set
    link means the old segment is drained, since every commit in it happened before the link store
    and Empty says the slot at the consumer's position is uncommitted, so the consumer moves and
    frees the old segment. An unset link means the producer is still here, and the wait policy
    runs.
  - Pool change, the one the design note allowed: `alloc::<()>` already hands out a whole buffer,
    so add `BufSlot::as_mut_bytes` for the init and a crate-private buffer pointer on the resolver
    for the consumer's attach and free.
  - Held out: no `attach` this cycle. The endpoints are in-process, since resuming needs a queue
    control block holding both sides' current segment, a later entry if a consumer for it appears.
- Acceptance check: a test streaming across many segment boundaries with a pool smaller than the
  message count, at `M = 1` and larger, that afterwards finds every segment but the live one back
  in the pool, and the size sweep on the 3900X in the design note with v3 at `M = depth` matching
  v2 within run noise.
- Prediction, on record: within a segment v3 is v2. Each seam costs the producer an alloc CAS, the
  init's header and seq lines, and a link store, and the consumer a header line for the link, a
  free CAS on the shared pool line, and a cold prefetcher on the new buffer. At `M = 1` roughly
  three times v2's two lines per message, and the slope from `M = 64` down shows where the seam
  stops mattering. We think it is flat by `M = 8`.
- Ladder, multi-step on one topic bookmark:
  - `feat: segmented queue SPSC v3 opening`
  - `feat: bytes and a buffer pointer from a pool buffer`, the pool accessors with tests
  - `feat: spsc v3 segment chain`, the module, both endpoints, the link protocol, the tests above
    plus `M = 1`, u32 wrap, and a threaded stress with a two-buffer pool that forces the exhausted
    wait and the unset-link retry
  - `feat: spsc v3 in the measurement tools`, a flavor in `tp-cell`, `tp-matrix`, `tp-stream`, and
    the demo's sweep, with a segment-capacity knob. Depth stays the total capacity and the segment
    count is depth over `M`, so `M = depth` is the v2 baseline inside v3
  - `perf: sweep the segment size`, `M` from 1 to depth at depths 64 and 256 across the three
    pinned placements, the numbers into a new design-note section and the crate's default `M`
    chosen from them, the 7600X pasted in by the user
  - `feat: segmented queue SPSC v3 closing`, which deletes the Overflow FIFO entry this supersedes
- Decisions the user has not yet given: `Ring` stays v2 and v3 is reached by path as
  `spsc::v3::Queue`, attach deferred, trapezoid at close-out.
- supersedes Overflow FIFO if it lands.

### Demo pin-pair picker

The demo's "diff cores" pair is the first cpu outside cpu0's L3, cross-L3 on the 3900X and same-L3
on the 7600X, so the two machines' lines with the same label were different experiments. The picker
wants a same-L3 placement and a cross-L3 one, each labelled by what it is, and the same for
`tp-matrix`'s placements.

### Sweep punctuation in the design note

`notes/ring-buffer-design.md` carries some 190 banned characters and its share of prose
semicolons, and the in-slot seq cycle touched it without paying them, since a count that size is a
rewrite rather than a repunctuation and the prose rule makes that its own cycle ([Typeable
punctuation only](agent-data/prose.md#typeable-punctuation-only)). Convert the file whole, re-point
the inbound links of any heading whose anchor moves, and touch nothing else.

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

- MPSC v2, the in-slot seq lesson: v2's seq at the front of its slot halved the SPSC round trip's
  lines and freed the stream from the shared seq line, and the MPSC ring still carries Vyukov's
  seq array. A sibling `mpsc::v2` with the seq in the slot, the claim CAS on the index as now, is
  the next MPSC experiment, after the segmented queue.
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

### docs: map the workspace and its tools

#### Problem

The root README documents the ring, the pool, the demo, and iiac-perf, and never mentions the
workspace it sits in: `tprobe`, `tp_runner`, and `tp_matrix` with its four binaries, `tp-cell`,
`tp-matrix`, `tp-stream`, and `tp-pool`, are documented only in their own READMEs, which nothing at
the top links to. Installing the root crate does not install the tools, where the measured numbers
live is not said, and no file records which dependencies are `no_std`. Some of what is written has
gone stale: the iiac-perf run uses bench names iiac-perf has since renamed, the tp_runner README
describes a `Cfg::parse` clap replaced, and the tprobe and tp_runner READMEs run `tp-cell both`,
which `tp-cell` rejects.

#### Solution

Done as planned. The root README gained a workspace section: a table of the five crates and
binaries with each one's `no_std` status and a link to its docs, both install commands, a line per
tool on the question it answers with a link to its section, where the measured numbers live, and a
dependency table with each dependency's `no_std` status and user. The iiac-perf run is dated
2026-07-06 and says the benches' names changed, the Testing section names the workspace test run
and the occupancy probe, tp_runner's README describes the clap flags and `LineBuf` in place of
`Cfg::parse`, both member READMEs run `tp-cell all` where `both` was rejected, and the manifest's
workspace comment names all four tool binaries. cordyceps is linked, not described.

#### Acceptance check

From the root README a reader reaches every workspace crate and every installed binary by one
link, finds both install commands and the dependency table with `no_std` status, and every command
the READMEs show for `tp-cell` runs. `vc-x1 validate` passes.

Passed (2026-09-12): the workspace section links `tprobe`, `tp_runner`, and `tp_matrix` by README
and each of the four tool binaries by its section anchor, the demo by the Testing section, the
install block and the dependency table are in place, `tp-cell all -d 0.2 --pin 0,1` ran all five
flavors, and `vc-x1 validate` passed.

#### Ladder

- docs: map the workspace and its tools (done)

#### Deliberation

- Single-step: documentation only, one straightforward step, so one commit carries the opening,
  the work, and the close-out, no `-dev` rename since no artifact changes.
- A map, not new prose: the member crates' READMEs already describe them, so the root README links
  to each rather than repeating it, and grows by a section rather than by copies.
- The stale iiac-perf run is dated and relabeled rather than deleted or rerun: it is a record of
  that version, the current bench names are one command away, and a rerun is iiac-perf's.
- cordyceps light: the next cycle splits the cordyceps tests and adapter into an example crate, so
  this one points at them and leaves the prose to that cycle.
- The continuation note on the landed cycle is dropped, and the one on the unpushed messages commit
  stays until that commit is pushed.
- `## Waiting` is `_None._`, nothing to promote.
- The `no_std` column was checked, not recalled: zerocopy, libc, and cordyceps declare `no_std` in
  their crate roots and clap, hdrhistogram, and perf-event2 do not, and the library built for
  `thumbv7em-none-eabi` during the conversation that planned this cycle.
- Corrections found while mapping went in rather than to the backlog: a README command that fails
  is a factual error, and the prose rule lets a correction go straight in.

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
