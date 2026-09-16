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

### Segment lifecycle in the design note

The segmented rings' switching, give-back, and free set are documented as protocol bullets in the
design note's SPSC v3 and MPSC v2 sections and the v2 module doc, but the lifecycle a reader
asks about is assembled from them rather than stated: a switch happens only at a full segment,
so no segment is left part-filled; a freed segment returns to the ring's free set, never to the
pool; the segment the consumer ends in stays in use, so a drained ring is a fresh ring in another
segment; and the free set is a bitmask taken lowest-first, not a queue. Write it as a short
"segment lifecycle" subsection under MPSC v2, with v3's differences (switch decided at the commit,
give-back at the MOVED release, the two-word free set), a sentence in the README's segment
paragraph, and a pointer from the module doc. Docs only, single-step. Raised 2026-09-16 at the
close-out of `feat: the demo's base cpu and pin-pair picker`.

### Paired columns in the tp_matrix tables

`tp-matrix` prints each phase cell as `mean/stdev` in one column, and `tp-pool` prints two tables per
placement with the same rows and columns, `ns/msg` and `xfills/msg`. Both would read better as one
heading over two columns. Markdown has one header row and no colspan, and the three layouts weighed
on 2026-09-13 each fell short:

- Pair names in the one header row, `m.send` then `±`, `1 ns` then `1 xf`: valid markdown, but the
  pairing is carried by names alone.
- A group row: group names in the header and sub-names as the first body row, which a markdown
  renderer shows as a data row.
- Plain aligned text with a spanning heading: reads best in a terminal, but a paste is no longer a
  markdown table.

### Segmented pools

A pool has one buffer size, so an application wanting messages of several sizes builds and registers
several pools by hand. A segmented pool holds sub-pools of different buffer sizes, and its
`alloc(size)` takes a buffer from the smallest sub-pool that fits, returning the buffer's location
and actual size. Typed access becomes a zero-copy cast on those bytes: the whole buffer as a `T`,
or `T`s at offsets inside it. `alloc::<T>()` stays as `alloc(size_of::<T>())` plus the cast.

- A sub-pool can be a registered pool of its own, so `Desc { pool_id, buf_idx }` already names the
  sub-pool a buffer came from, and `free` already returns it there.
- Buffers start on a cache line, so any `T` aligned to at most a line fits any sub-pool.
- The user's direction on 2026-09-14, raised while settling how v3's segments come from the pool.

### SPSC v3 fast path

With the consumer keeping up, `spsc::v3` should cost what v2 costs, and on 2026-09-15 it cost about
three times as much where no segment switch happens, the demo's one-thread loop reading 24 ns per
message against v2's 7.5.

- Found: `WriteSlot::commit` and `ReadSlot::release` begin `let segs = st.segs;`, copying the ring's
  whole segment table, about 280 bytes, on every message. `let segs = &st.segs;` in both took the
  same-CCX stream at depth 64 from 18.4 to 10.7 ns per message, against v2's 4.5 to 5.7, and the SMT
  pair from 21.8 to 14.3, against 8.2.
- Found on 2026-09-15 in the MPSC v2 cycle, whose v2 shares v3's shape: the segment table's
  accessors, the seq helper, and the word packers are small non-generic functions in the parent
  module, called from the producer and consumer child modules. A release build without LTO puts
  each module in its own codegen unit, so without `#[inline]` those are real calls on every send
  and receive. Inlining v2's took its one-thread demo loop from 20.0 to 13.2 ns per message
  against v1's 10.1. Only `seq_of` was inlined for v3, and v3's own accessors in `Segments` still
  are not. `slot_ptr` and `check_type` in `lib.rs` are the same kind of call for every ring, v0
  and v1 included, and were left alone so the comparisons stay as they were.
- Apply that, then find what keeps v3 behind v2 on the fast path, measured against v2 at each step.
- The user's call on 2026-09-15, deferred from the segmented queue cycle so that cycle makes
  multiple segments work first.

### SPSC v3 attach

`spsc::v3::Ring` has no `attach`, so a v3 ring cannot be joined from another process or resumed.
v0 through v2 can, since a ring's whole state is in its region. v3's state spans a pool and a chain
of segments, so attach needs a control block in the region recording both sides' current segment.
Wait for a user that needs it, and until then name `spsc::v2::Ring`.

### MPSC v2 as the default

`MpscRing` is v1 while `Ring` is v3, so the crate's SPSC default is segmented and its MPSC default
is not. Flip the re-export to `mpsc::v2` once v2 matches v1 where no switch happens, measured in the
tools, since the v3 verdict on 2026-09-15 was that a segmented default that does not match costs
every path that never needs a second segment.

- Where v2 stands on 2026-09-15, both machines: it streams two to five times faster than v1 from
  depth 8 up and pulls half the lines, its send matches v1's everywhere but the SMT pair, and its
  receive in the round trip runs up to 30% slower from depth 8 up on the 3900X and around depth 64
  on the 7600X. We think the seq word in the slot line is the cause: the consumer spins on the line
  the producer then writes twice, the body and the seq, where v1's producer fills the slot line
  unwatched and stores the seq beside it. The one-thread loop also carries 3 ns over v1 not yet
  found. Both are the fast-path work this entry waits on, and the switch's own cost is the entry
  `Cheaper segment switches`.

### Multi-producer measurement

The tools run every MPSC flavor at one producer and one consumer, so a claim word contended by
several producers is never measured. A `--producers N` flag for `tp-matrix` and `tp-stream` would
run N pinned producers into one consumer, and v2's switch would then be measured under contention.

### Cheaper segment switches

The demo's segment stress table prices one switch at depth 1 across cores at 140 ns for spsc-v3
and 270 to 290 for mpsc-v2 on the 3900X's cross-CCX pair, and 16 and 58 on the 7600X's same-L3
pair, on 2026-09-15, against 3 to 7 and 7 to 13 single-threaded. At depth 1 across cores the switch
is most of the message, eight times the no-switch shape on the 7600X. Where the consumer keeps
up, from depth 8 on, it is paid once in hundreds of messages or never.

- The lines that cross per switch, beyond the new segment's slot line that the no-switch shape
  walks too: for spsc-v3 the consumer's give-back word, one transfer; for mpsc-v2 the old
  segment's seal and the in-use word twice, since both sides read-modify-write it, three to four.
  The two machines agree on that once the placement's cost per transfer is taken out, over 100 ns
  cross-CCX on the 3900X and 15 to 20 within the 7600X's one L3.
- Candidates: a consumer-owned give-back word for v2 again, now that the taking side is sound by
  itself with the in-use word, so the consumer's give-back is a store to a line producers only
  read; the seal riding in the slot word as v3's MOVED does, which v2 cannot do at the commit since
  another producer may hold the last slot; a prefetch of the next segment's first line at the
  take. Each measured on the stress table's switch-cost rows.

### Comparison queues in the demo: cordyceps, crossbeam, iceoryx2

The demo compares the crate's rings only with `std::sync::mpsc`, and the user asked on 2026-09-15
for cordyceps, crossbeam, and iceoryx2 beside them. Its own cycle, since each is a dependency
decision and a harness shape:

- cordyceps is a dev-dependency today, used by `tp-pool`, and the demo is the installed binary, so
  it would become a dependency of the crate; crossbeam and iceoryx2 would be new ones, and
  iceoryx2 is a shared-memory framework with its own runtime and setup.
- cordyceps's intrusive MPSC and crossbeam's channels move a pointer or a value, not a message in
  place, so their line is a pool buffer or a boxed message crossing, `tp-pool`'s shape, not the
  ring lines'. iceoryx2 is publish-subscribe over shared memory with no direct depth knob.
- Which lines and placements they join, and whether the demo or a `tp-pool` sweep is the place, is
  the design question the `tp-pool` cycle answered once for cordyceps.

### Sweep punctuation in the design note

`notes/ring-buffer-design.md` carries some 190 banned characters and its share of prose
semicolons, and the in-slot seq cycle touched it without paying them, since a count that size is a
rewrite rather than a repunctuation and the prose rule makes that its own cycle ([Typeable
punctuation only](agent-data/prose.md#typeable-punctuation-only)). Convert the file whole, re-point
the inbound links of any heading whose anchor moves, and touch nothing else.

- The count on 2026-09-16, when `feat: the demo's base cpu and pin-pair picker` added the
  Measurement placements section and the placement terms without paying: 130 prose semicolons
  outside code spans and blocks. The new sections carry none.

### Descriptor queue endpoints

Paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
- own ring endpoint + registry access
- the demo's ~20-line send path becomes ~3 lines
- `resolve`'s unsafe is audited once inside the crate (recv safe by construction)
- guard handed back on Full
- design against both ring flavors (SPSC + MPSC)
- the sender is also where each sender's private overflow pending list will live.

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

### feat: the demo's base cpu and pin-pair picker

#### Problem

The demo pins every single-thread line to cpu 0 and builds its two pin pairs from cpu0's
topology, all of it hard-coded, and the measurement tools start from cpu 0 the same way. Cpu 0 is
the kernel's favorite and runs noisier, so a bench there is not a good choice, and the demo has
no way to move it, no `--help`, and no usage beyond `-V`. The demo's "diff cores" pair is the
first cpu outside cpu0's L3, cross-L3 on the 3900X and same-L3 on the 7600X, so the two machines'
lines with the same label were different experiments, while the tools already name their
placements `CCX`, `x-CCX`, and `SMT`.

#### Solution

Done in six rungs. The demo takes `--base-cpu <n>`, `-h` / `--help`, and rejects an unknown
argument with the usage, the base a process-wide atomic read at every pin. Its picker is the
tools' one with a base: `CCX`, `x-CCX`, `SMT`, then `unpinned`, only those the machine has, and
the 2t lines, the depth sweep, and the segment stress take the list. `tp-matrix`, `tp-stream`,
and `tp-pool` take `--base-cpu` through `tp_runner`'s discovery, and the shared `-d` default is
1 s. The default base went to 1 and then, on the housekeeping counts, to the last core's primary
cpu, 11 on the 3900X and 5 on the 7600X, with partners ordered primary cpus first and highest
number first, the rationale and both machines' counts in the design note's Measurement
placements. A glossary in its Terminology fixes core, cpu, SMT siblings, primary and secondary
cpu, cluster, and cache layers, and every README example run is re-done on both machines.

#### Acceptance check

`zc-ring-x1-demo --help` prints the usage and exits 0, `zc-ring-x1-demo --bogus` prints it and
exits 1. On the 3900X the default demo run labels its 1t lines `core 11` and its pairs
`11,10 CCX`, `11,8 x-CCX`, and `11,23 SMT`, and `--base-cpu 9` gives `9,11 CCX`, `9,8 x-CCX`,
`9,21 SMT`. On the 7600X the default is base 5 with `5,4 CCX` and `5,11 SMT`, and the `x-CCX`
rows are skipped as lacking. `tp-matrix`, `tp-stream`, and `tp-pool` take `--base-cpu` and label
their placements from it the same way, and each README example run on both machines is at the
new default, the tools at 1 s.

Passed on 2026-09-16 at the closing: `--help` exits 0 and `--bogus` exits 1 on the installed demo.
The 3900X default run in the README shows `core 11`, `11,10 CCX`, `11,8 x-CCX`, `11,23 SMT`, and
`--base-cpu 9` gives `9,11 CCX`, `9,8 x-CCX`, `9,21 SMT`. The 7600X run shows base 5, `5,4 CCX`,
`5,11 SMT`, and no x-CCX row. The three tools label from the base the same way, and every README
example run is at the new defaults.

#### Ladder

- [feat: the demo's base cpu and pin-pair picker opening][1] (done)
- [feat: a base cpu flag and help for the demo][2] (done)
- [feat: same-L3, cross-L3, and SMT placements in the demo][3] (done)
- [feat: a base cpu for the measurement tools][4] (done)
- [perf: the demo and the tools off cpu 0 on both machines][5] (done)
- [perf: a quiet default base and quiet partners][7] (done)
- [docs: cores, cpus, and cache layers, the placement terms][8] (done)
- [feat: the demo's base cpu and pin-pair picker closing][6] (done)

#### Deliberation

- Grown from a single-step cycle, `feat: the demo's base cpu flag and help`, at its work review on
  2026-09-16: the user's vote was a ladder that does `Demo pin-pair picker` too, since the review
  of a `1+3` pair showed the "diff cores" label hiding what the pick was. Nothing had pushed but the
  bookmark, so the shape was still open, and the bookmark was retitled with the cycle.
- Whole binary, not the segment stress alone, the user's call: the base is shared by the picker
  and every pinned line, so a flag that reached one section would leave the rest on cpu 0.
- The base as a process-wide atomic in the demo, set once in `main`: the single-thread loops sit
  behind macros and function-pointer tables, so a parameter would change every signature.
- Default base 1, tool-wide, the user's call, "for now": cpu 0 is the kernel's, and 1 is the
  first cpu that is not. On the 3900X it gives `1,2 CCX`, `1,3 x-CCX`, `1,13 SMT`.
- The tools' default duration 5 s to 1 s, tp-pool untouched: a cell is a fixed-duration spin at
  millions of trips a second, so 1 s still has samples in the millions and the tables print
  stdev, and `-d 5` stays for calm numbers. tp-pool already runs 0.1 s cells, median of three,
  and its 108 s is the cell count.
- Every README example run re-done in one rung, both machines: at the new defaults a machine's
  set is about 30 s each for tp-matrix and tp-stream, 108 s for tp-pool, and a few minutes for
  the demo, cheap enough that no table stays dated on cpu 0.
- Pairs prefer cpus above the base: the first run at base 1 paired `1,0 CCX`, the first other
  core on the L3 being cpu 0, so the default base had put a pair back on the cpu it was leaving.
  Both pickers try the cpus above the base first, then the rest, so base 3's `x-CCX` is `3,6`
  rather than `3,0`, and the acceptance check was corrected with it.
- No `-dev` rename, as in the earlier cycles.
- A quiet base is the last core, the user's call on 2026-09-16 after the housekeeping counts: the
  scheduler's idlest-cpu search fills cpus from the bottom, so cpu 1 carries nearly cpu 0's timer
  and reschedule load and the high-numbered cores ten times less. The default base is the last
  core's primary cpu, 11 on the 3900X and 5 on the 7600X, and partners prefer a core's primary
  cpu and the highest number, so the pairs stay at the quiet end. The rationale and the
  counts are in the design note.
- Placement terms, the user's on 2026-09-16 at the quiet-base rung's review: "first thread" and
  "second thread" collide with software threads, and the repo had four spellings for one thing.
  A core is the physical unit, a cpu what the kernel presents and pins to, a core's cpus its SMT
  siblings, the lowest its primary cpu and the other its secondary, a cluster the cpus sharing a
  cache layer, and caches are layers, L1, L2, L3. The glossary goes into the design note's
  Terminology and the rename through code, usage, legends, and both READMEs. The rung also pays
  the one prose semicolon `tp_runner/src/topo.rs` owed, the user's call, since it touches the
  file anyway.
- Waiver, the user's on 2026-09-16 at the opening's review: the work reviews, description reviews,
  and per-push approvals of the opening and the four work rungs are waived, the user reviewing on
  return. It does not cover the closing rung or Land.

#### Ladder details

##### feat: the demo's base cpu and pin-pair picker opening

The cycle's setup commit: retitle and publish the bookmark, delete `## Closed`'s contents, move
the Todo entry into this block, reset the continuation notes, and bump the version-of-record.

- `## Waiting` is `_None._`, nothing to promote.
- The single-step draft's flag work was set aside as a patch, `tmp/rung2.patch`, so this commit
  carries the setup alone, and returns as the next rung.

##### feat: a base cpu flag and help for the demo

The demo hard-codes cpu 0 at every pin and in its picker, and has no usage. A `--base-cpu <n>`,
default 0 in this rung, that every pin and both pairs start from, the labels naming it, plus
`-h` / `--help` and the usage on an unknown argument.

* The single-thread loops sit behind macros and function-pointer tables.
  - The base is a process-wide atomic set once in `main` before any run and read at every pin,
    so no loop's signature changes. The picker alone takes the base as a parameter.
* The usage has to be the one text at three exits.
  - One constant, printed to stdout on `-h` and to stderr, under an error line, on an unknown
    argument, a missing value, or a value that is not a number. Parsing is a hand loop over the
    arguments, since the demo has no clap dependency and three flags do not earn one.
* Every label said `core 0`.
  - The 1t lines, the sweep's `1t core N` heading, and the stress table's placement column print
    the base, and the demo's header line names it.

##### feat: same-L3, cross-L3, and SMT placements in the demo

The demo's two pairs, "diff cores" and "same core", are one experiment on the 3900X and another
on the 7600X. The picker becomes the tools' one with a base: `CCX`, `x-CCX`, and `SMT`, each
skipped where the machine lacks it, in the 2t lines, the depth sweep, and the segment stress.

* The demo named its pairs by distance, "diff cores" and "same core", and the tools by cache.
  - A `Placement` is a label in the tools' form and a pin, and the demo's picker is the tools'
    discovery with a base: `CCX`, `x-CCX`, `SMT`, then `unpinned`, only those the machine has.
    The 3900X gains a `CCX` pair, and the 7600X's `x-CCX` row is absent instead of mislabelled.
* `main` held one block of nine lines per pair, and the sweep and the stress each rebuilt the
  placement list from the two pairs.
  - The nine lines are one function over a placement, `main` loops it, and the sweep and the
    stress take the list. The stress's streaming rows sit at the farthest placement the machine
    has, `x-CCX`, else `CCX`, else unpinned, where before they sat at the "far" pair.
* The README's placement paragraph and the demo's usage said "SMT siblings" and "different cores".
  - Both say the tools' terms.

##### feat: a base cpu for the measurement tools

`tp_runner`'s placement discovery reads cpu0's topology. It takes a base, `tp-matrix`,
`tp-stream`, and `tp-pool` grow `--base-cpu`, and the shared default duration drops to 1 s.

* The discovery read cpu0's sysfs paths and wrote 0 into every label.
  - It takes the base, reads that cpu's sibling list and L3 list, and the SMT pair is the base
    and any sibling that is not it, where before it required the base to be the first sibling.
* Three tools sweep placements and one pins explicitly.
  - A `BaseCpuArg` beside `CommonArgs` in `tp_runner::topo`, flattened into `tp-matrix`,
    `tp-stream`, and `tp-pool`, so `tp-cell` shows no flag it ignores. Its default is a constant
    the next rung moves to 1.
* The shared `-d` default was 5 s.
  - It is 1 s, and its help says why 1 is enough and when 5 is wanted. `tp-pool` keeps its own
    0.1 s and median of three.

##### perf: the demo and the tools off cpu 0 on both machines

The default base becomes 1 in the demo and the tools, and every README example run, the demo's
and the tools', is re-done on the 3900X and the 7600X at the new defaults.

* The default was 0 in two places, the demo's atomic and the tools' flag.
  - Each is a `DEFAULT_BASE_CPU` constant at 1, the usage and the READMEs saying so.
* The first run at base 1 paired `1,0 CCX`.
  - Both pickers try the cpus above the base first, the deliberation's finding.
* The README example runs were 0.15.8 on cpu 0, and the tools README's snippets were on cpu 0.
  - The demo blocks are re-done on both machines at 0.17.1-4, the 3900X at `1,2 CCX`, `1,3
    x-CCX`, `1,13 SMT`, the 7600X at `1,2 CCX` and `1,7 SMT` with no x-CCX, and the tools
    snippets carry rows from the 3900X at 1 s. The 7600X has no rsync, so the tree went over by
    tar through ssh into `~/zc-ring-x1-run`, which can be deleted.

##### perf: a quiet default base and quiet partners

Base 1 is nearly as noisy as cpu 0, and the pickers' partners went up from the base, so base 9's
x-CCX partner was cpu 12, cpu 0's sibling. The default base becomes the last core's primary cpu,
partners prefer primary cpus and the highest number, the rationale goes into the design
note, and the README examples are re-run.

* Base 1 sits in cpu 0's CCX and draws the same scheduler traffic.
  - The default is the last core's primary cpu, computed from sysfs at start, 11 on
    the 3900X and 5 on the 7600X. The first cpu of the highest L3 group was rejected, since on
    the one-L3 7600X it is cpu 0.
* Partners went up from the base, so the last CCX's x-CCX partner was cpu 12, cpu 0's sibling.
  - Both pickers order candidates primary cpus first and highest number first, so the pairs are
    `11,10 CCX`, `11,8 x-CCX`, `11,23 SMT` and `5,4 CCX`, `5,11 SMT`. The rule, the counts, and
    the rejected orders are in [Measurement placements](notes/ring-buffer-design.md#measurement-placements-the-base-cpu-and-its-partners).
* The tools' placement column widened to ten characters with a two-digit base.
  - The tools README's snippets carry the wider column, re-run on the 3900X with the demo blocks
    on both machines.
* The design note owes 130 prose semicolons, a rewrite rather than a repunctuation.
  - Left for its own cycle, as the semicolon rule says, and raised at the close-out.

##### docs: cores, cpus, and cache layers, the placement terms

The repo says "hardware thread", "SMT sibling", "logical CPU", and "first thread" for the same
thing, and the last collides with software threads. A glossary in the design note's Terminology
fixes core, cpu, SMT siblings, primary and secondary cpu, cluster, and cache layers, and the
rename runs through the code, the usage texts, the legends, both READMEs, and the placements
section. No number moves, so no re-run.

* Four spellings for one thing, and one of them a software word.
  - Six glossary entries in the design note's Terminology: core, cpu, SMT siblings with primary
    and secondary cpu, cluster, cache layers, and bare metal, the last so the RP2350 has a place
    without joining the tools. The tools' legend says "one core's two cpus", `tp-cell`'s help
    says "cpu numbers", and the pickers' helper is `is_primary_cpu`.
* The pickers' L3 grouping is the Zen shape and the note did not say so.
  - The placements section says a part whose cluster shares L2 finds no CCX pair, and names the
    kernel's cluster list as the fix when such a machine arrives.
* `tp_runner/src/topo.rs` owed one prose semicolon.
  - Paid, a comma and a conjunction.

##### feat: the demo's base cpu and pin-pair picker closing

Closing out the cycle. What closing taught:

* The cycle grew twice past its plan, from a single-step flag to a ladder of six.
  - Each growth came from a review of a number: a `1+3` pair showed the label hiding the pick,
    a `1,0 CCX` pair showed the default undoing itself, and the housekeeping counts showed base 1
    was no quieter than base 0. A placement is worth a table before it is worth a default.
* Nothing outlives the block that is not already in the design note.
  - The rationale, the counts, the rule, and the terms are in Measurement placements and
    Terminology. The design note's own semicolons stay with `Sweep punctuation in the design
    note`, updated with this cycle's count.
* Land installs the tools too.
  - `cargo install --path tp_matrix --locked` beside the root install, since the tools' pickers
    changed and the installed ones are from the last cycle.

- No size row, the user's call at the closing's review on 2026-09-16: no agent-file changed, so
  `notes/agent-files-size.md` is not touched, a bend of the close-out's step 4 for this cycle
  only.

Close-out shape: trapezoid, the default.

# References

[1]: #feat-the-demos-base-cpu-and-pin-pair-picker-opening
[2]: #feat-a-base-cpu-flag-and-help-for-the-demo
[3]: #feat-same-l3-cross-l3-and-smt-placements-in-the-demo
[4]: #feat-a-base-cpu-for-the-measurement-tools
[5]: #perf-the-demo-and-the-tools-off-cpu-0-on-both-machines
[6]: #feat-the-demos-base-cpu-and-pin-pair-picker-closing
[7]: #perf-a-quiet-default-base-and-quiet-partners
[8]: #docs-cores-cpus-and-cache-layers-the-placement-terms
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
