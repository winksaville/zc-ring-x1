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

### feat: in-slot seq SPSC v2

#### Problem

The seam-word ring's seq words live in their own line, and v1 is slower per send than the MPSC
producer at every placement, the puzzle [SPSC v1: seam-word
ring](notes/ring-buffer-design.md#spsc-v1-seam-word-ring) leaves open. Every streaming number on
record is at one depth per tool, 64 in the demo and 8 in `tp-matrix`, so depth and protocol have
never been separated, and the streaming lines carry no fill counts, so the line traffic behind
them is inferred rather than measured.

#### Solution

A `spsc::v2` sibling ring with the seq in the slot's own line, a crate-owned slot header ahead of
the user-owned body, measured beside v0, v1, and MPSC at depths 1, 2, 8, and 64 in both tools, the
demo first and then a `tp-matrix` streaming cell that counts fills per message, with the findings
recorded in the design note:
- starts from the probe rung's baseline: packed seq array, neither side waiting, v0 ahead within an
  L3 and v1 ahead across one
- the prediction on record: it helps the round trip and may hurt streaming, since the slot line
  would then travel both ways every message where today the seq line amortises
- the cheap probe goes first: a fence after v1's commit, one line, to test the store-buffer reading
- the seq's width is measured (u32 against the native width) before the slot header fixes it
- the slot contract changes (the body sits behind the crate-sized header), so its shape is part of
  the finding.

#### Acceptance check

`vc-x1 validate` passes, including v2 ring tests at `M = 1`, `2`, and a larger power of two.
`tp-matrix` and the demo run all four flavors at depths 1, 2, 8, and 64 on the 3900X, the
streaming cell reports fills per message, and `notes/ring-buffer-design.md` carries the tables
with a why paragraph per placement, the fence probe's result, and the seq width chosen.

#### Ladder

- [feat: in-slot seq SPSC v2 opening][1] (done)
- [feat: runtime depth in the demo and tp-matrix][2] (done)
- [feat: add the spsc v2 in-slot seq ring][3] (done)
- [feat: spsc v2 as a fourth flavor][4] (done)
- [perf: probe a fence after the v1 commit][5] (done)
- [perf: measure spsc v2 across depths][6] (done)
- [feat: a streaming cell with fill counts][7] (done)
- [docs: sweep punctuation in the touched files][8] (done)
- [feat: in-slot seq SPSC v2 closing][9]

#### Deliberation

- v2 is a sibling module, not an edit of v1: the module layout exists for the A/B, and the user
  wants every version comparable at once, so v0, v1, v2, and MPSC all stay reachable by path and
  the crate's default re-export moves only if the numbers earn it.
- The seq sits in a 16-byte crate header at the front of the slot's first line, the body behind
  it, rather than a whole header line ahead of the body: the second is padded v1 at a different
  address, and padded v1 already measured worse. Sixteen bytes so the u32 against u64 flip fits
  without a layout change, and the header is the seq alone, the rest reserved, so the cycle
  carries one design change.
- Depth becomes a runtime parameter in both tools, the regions heap-allocated and sized by each
  ring's own `region_size`, since the const stack arrays fix one depth per build. The demo takes
  the sweep first, a table of flavor by depth per placement, and the `tp-matrix` streaming cell
  with fill counts follows, on the user's call (2026-09-07): more information is better until it
  interferes with the measuring, so the simple form goes first.
- The fence probe keeps its own rung ahead of the measurement, as the Todo entry ordered it: its
  answer says whether v2's commit wants a fence too, and it is a one-line flip measured in the
  same matrix.
- Punctuation: the demo, the `tp_matrix` sources, and the v1 sources carry banned characters, so
  touching them owes the conversion, paid in the penultimate rung as the prose rule says.
- The design note is left out of the punctuation sweep: some 190 banned characters is a rewrite,
  which the prose rule makes its own cycle, so a `## Todo` entry carries it and the rung converts
  the sources and the tool README, which are repunctuation.
- Waiver: the user's delegation of 2026-09-07, "you have permission to complete this cycle,
  including commits and pushes, but leave it on the branch", covers every push from the bookmark
  through the closing and the per-rung review stops, and does not cover Land, which waits on the
  user's review.

#### Ladder details

##### feat: in-slot seq SPSC v2 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
Todo entry into this block, bump the version-of-record, and rename the demo binary to `-dev`.

##### feat: runtime depth in the demo and tp-matrix

Both tools fix the ring depth at build time, so no run can compare depths. The rung makes depth a
runtime parameter and adds the demo's depth sweep table.

* The regions were const stack arrays sized by a `DEPTH` const, one per build.
  - The runner gains a line-aligned heap region sized at runtime by each ring's own size function,
    and the demo carries the same helper over zerocopy. Heap against stack changes nothing the
    loops measure, since the region is touched once at init.
  - v0 exports no region size function, its header being a fixed shape, so both tools compute it
    from the header's size. A v0 export is a deferral, not a need.
* Nothing let a run ask for a depth.
  - `--depth` on the shared args takes a comma list of powers of two, default 8, and every
    `tp-matrix` and `tp-cell` cell repeats per depth, the matrix tables gaining a depth column.
  - The demo's lines stay at depth 64 and a sweep follows them: the ring flavors at every placement
    and at depths 1, 2, 8, and 64, one markdown table per placement in ns per message.
* The MPSC ring accepts capacity 1 and its protocol collapses there, found when the sweep's first
  mpsc cell at depth 1 spun forever.
  - Filed in `notes/bugs.md`: committed `pos + 1` and released `pos + M` coincide at `M = 1`, the
    collapse v1's `M = 1` tests caught in its own first cut. Each flavor now names its floor, and
    the tools skip a cell below it with a note, the demo printing `-`.
  - The fix is unplanned work and stays out of this cycle, on the rule that unplanned work is the
    user's to place.

##### feat: add the spsc v2 in-slot seq ring

The seq and the slot it publishes live on different lines in v1. The rung adds the sibling ring
whose slot carries its own seq, with the tests v1 grew.

* The protocol needed a home for the seq inside the slot without a second design change.
  - `spsc::v2` is v1's protocol over a region of header then slots, every slot opening with a
    16-byte crate header, the seq at offset 0 and the rest reserved. The endpoint surface is v1's,
    so a caller or a bench flips between versions by path alone.
  - The seq's width is one type alias, `Seq`, the indices staying u32 so the word holds the same
    values at either width. The measurement rung flips it.
* The slot contract had to change, since the body no longer starts at the slot.
  - v2 checks `T` against the body, `slot_size - 16` at an alignment of at most 16, its own check
    beside the crate's, and a test pins the body's address to slot base plus the header.
* The tests v1 grew carry over whole, the `M = 1` alternation and the threaded stream at 1, 2, 4,
  and 16 among them, plus the cross-kind attach against a v1 region.
* The design note gains "SPSC v2: in-slot seq ring" with the prediction written down before the
  numbers: the round trip should gain and streaming may lose, since the slot line then travels
  both ways per message.

##### feat: spsc v2 as a fourth flavor

The tools know three flavors. The rung adds v2 to `tp-matrix`, `tp-cell`, and the demo.

* Every tool spelled the flavor list out in its own way.
  - `tp-matrix` and `tp-cell` gain `spsc-v2` from the one `FLAVORS` list, a cell instantiated
    from the shared SPSC macro over v2's `region_size`, so the round-trip A/B is the protocol
    alone.
  - The demo gains `spsc2_` lines beside the `spsc1_` ones at every placement and a `spsc-v2` row
    in each sweep table, and the occupancy probe a `v2` line, all from the macros the v1 rung
    wrote.
* The first numbers, on the 3900X, are a finding the measurement rung must confirm.
  - Streaming across the CCX boundary at depth 64, v2 moves a message in 12 ns against v1's 104
    and v0's 207, and holds that within an L3 and at the SMT pair, where v1 lost to v0 by 2x. In
    the round-trip cell v2 moves 3.5 to 4.0 lines per trip against v1's 6.85.
  - We think the streaming gain is not the line count but the line independence: v2 has no line
    both sides write per message except the slot itself, and consecutive slots are consecutive
    lines, so the transfers pipeline where v1's packed seq line and v0's index lines serialised
    them. The prediction on record, that streaming may lose, is refuted on this machine.

##### perf: probe a fence after the v1 commit

The v1 per-send loss has a store-buffer candidate on record. The rung measures v1 with a fence
after its commit store and records the answer.

* The candidate needed a one-line test rather than an argument.
  - A `CommitFence` switch in v1's producer, `None`, `Mfence` (a `SeqCst` fence after the commit
    store), or `Xchg` (the store itself `SeqCst`), built three times and run through `tp-cell` at
    the three pinned placements and the demo's v1 stream lines.
* The answer is no: the round-trip send costs, trip counts, and fills per trip did not move at any
  placement, and both fence forms slowed the SMT-pair stream by 30 to 70%.
  - The store buffer is struck from the candidates, leaving the private index store ahead of the
    seq store and the guard's code shape. The switch stays in the source at `None`, as the seq
    stride switch did, so the probe can be rerun.
  - So v2's commit takes no fence either: it has the same store shape, and the answer carries.

##### perf: measure spsc v2 across depths

The v2 ring exists with no numbers. The rung runs both tools at every depth on the 3900X, flips the
seq width, and records the tables and their why in the design note.

* The four flavors had never been measured together at one depth, let alone four.
  - `tp-matrix` at 5 s cells and the demo with its sweep ran at depths 1, 2, 8, and 64, and the
    design note's v2 section carries the tables: round trips and fills per trip per cell, the
    streaming ns per message per placement, and a why paragraph per placement.
* The round trip confirms the prediction and the streaming refutes it.
  - v2 moves 3.1 to 4.0 lines per round trip against v1's 6.0 to 8.1 and MPSC's 6.0 to 8.3, and
    completes the most trips at every cross-core placement and depth. Streaming across the CCX
    boundary at depth 64, v2 moves a message in 12 ns against v1's 103 and v0's 209, and matches
    v0 within an L3 and at the SMT pair, where v1 lost 2x.
  - We think the streaming win is line independence rather than line count: the only line both
    sides write per message is the slot, consecutive slots are consecutive lines, and so the
    transfers overlap where v0's index lines and v1's packed seq line serialised them. The
    streaming cell with fill counts is the test of that reading.
* The seq's width had to be measured before the layout fixed it.
  - u32 against u64 in the same cells and stream lines: no difference beyond run noise at any
    placement or depth, so u32 stays, the v1 width and the smaller word.
* Depth 1 and 2 are where the protocols separate in the round trip, and 8 against 64 is where v1
  and MPSC pay for a seq line spanning more slots.
  - At depth 1 and 2 v2 does its 4.0 fills and v1 and MPSC 8.1 to 8.5, since their seq line and
    the slot line both cross twice. At depth 8 and 64 both fall to 6.0 to 6.4 while v2 falls to
    3.1 to 3.7, and the trips per 5 s track the fills.

##### feat: a streaming cell with fill counts

The streaming lines carry no fill counts. The rung adds a `tp-matrix` streaming cell that counts
fills per message, and records what it shows.

* The demo's streams had no fill counter and the round-trip cell has no streaming.
  - `run_stream` in `tp_matrix` streams a counter from a spawned producer to a spawned consumer for
    the duration over one ring, both pinned as the placement says, with the fill counters open
    around the run, and `tp-stream` tables it over every flavor, placement, and depth as ns per
    message, messages moved, and fills per message.
* The cell's first numbers disagreed with the demo's, v2 slower and v1 faster, and the rung had to
  find out why before recording anything.
  - Run length, the thread shape, the fill counters, the payload width, and the crate boundary
    (fat LTO) were each tried and struck. Two variables remained, both measured.
  - The wait policy's inlining: the runner's `spin` is not `#[inline]` and is called from another
    crate, and that alone put v2's cross-CCX stream at 31 ns against 14 with the crate's inline
    `policy::spin`. The cell now uses the crate's.
  - The producer's loop shape: the cell's clock check every 4096 sends moves v1's cross-CCX
    stream from 104 ns per message at 1.8 fills, the demo's plain loop, to about 40 at 0.6. v1 is
    bistable there, lockstep on its packed seq line or the producer running ahead in bursts, and
    a periodic hiccup tips it. v0 and v2 read the same in both shapes.
* The streaming fill counts answer the measurement rung's open question.
  - v2 across the CCX moves 0.13 lines per message at depth 64, far below the two the prediction
    feared and below one, so the slot lines are not demand-fetched at all for most messages. We
    think the consumer's prefetcher pulls consecutive slot lines ahead of demand, since consecutive
    slots are consecutive lines, and that is the line independence the measurement rung named.

##### docs: sweep punctuation in the touched files

The files the cycle touched carry banned characters and prose semicolons. The rung converts them.

* The touched sources, the runner and cell crates, the tool README, and the manifests carried
  dashes, arrows, and prose semicolons in their comments and prose.
  - Each is converted by the prose rule's joins, a colon for a term and its definition, a comma or
    two sentences for an aside, `->` for an arrow, and the code and transcribed tool output are
    untouched. Two headings in the tool README lose their dash and take the colon form.
* The design note's count is a rewrite.
  - It goes to `## Todo` as its own cycle, as the rule says, and stays as it is here.

##### feat: in-slot seq SPSC v2 closing

Closing out the cycle.

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

_None._

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #feat-in-slot-seq-spsc-v2-opening
[2]: #feat-runtime-depth-in-the-demo-and-tp-matrix
[3]: #feat-add-the-spsc-v2-in-slot-seq-ring
[4]: #feat-spsc-v2-as-a-fourth-flavor
[5]: #perf-probe-a-fence-after-the-v1-commit
[6]: #perf-measure-spsc-v2-across-depths
[7]: #feat-a-streaming-cell-with-fill-counts
[8]: #docs-sweep-punctuation-in-the-touched-files
[9]: #feat-in-slot-seq-spsc-v2-closing
