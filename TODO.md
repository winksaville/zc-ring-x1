# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

- After the cycle "docs: pay the punctuation debt" lands, a single-step cycle, `docs: the generic
  Queue idea`, adds a `## Ideas` bullet and a section in `notes/ring-buffer-design.md`: a
  `Queue<P>` with a sealed `Single` / `Multi` producer marker as a thin facade over `spsc::v3` and
  `mpsc::v2`, `T` staying per call. Its open questions:
  - Is the closure `send_with` the common send, with `reserve_slot_with` a `Single` extra?
  - Is the ISR kind a guard axis (CAS or critical section) rather than a producer count? It would
    give thumbv6m, which has no CAS, a multi-producer queue, the gap between [Execution
    contexts](notes/ring-buffer-design.md#execution-contexts) and the embedded-floor Idea.
  - Related: the `### Typed endpoints` Todo, and a consumer-kind axis left room for.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### docs: pay the punctuation debt

#### Problem

The prose rules ban semicolons and the untypeable characters (em dash, en dash, ellipsis, arrow)
from authored text ([Semicolons](agent-data/prose.md#semicolons), [Typeable punctuation
only](agent-data/prose.md#typeable-punctuation-only)), and a historical file pays when a cycle
touches it. About 35 files still owe, some 660 lines with a banned character and some 360 with a
prose semicolon, `notes/ring-buffer-design.md` the largest, so every small edit to one of them
is either sidestepped or drags a sweep behind it. The generic Queue idea on 2026-09-17 was the
latest edit to sidestep, and the user's call was to pay the whole debt instead.

#### Solution

Sweep every owing file, one rung per group of files so each review is of one kind of text, then
add a checker that blanks code and expects zero, so the debt cannot return unseen.

#### Acceptance check

The checker, run over every tracked file outside the exclusions named in the deliberation, reports
zero authored banned characters and zero prose semicolons, and `vc-x1 validate` passes, doctests
included. Every inbound link to a heading whose anchor moved resolves.

#### Ladder

- [docs: pay the punctuation debt opening][1] (done)
- [docs: punctuation debt in ring-buffer-design.md][2] (done)
- [docs: punctuation debt in the notes and READMEs][3]
- [docs: punctuation debt in the src comments][4]
- [docs: punctuation debt in the tp crates][5]
- [chore: a prose punctuation debt checker][6]
- [docs: pay the punctuation debt closing][7]

#### Deliberation

- Multi-step: about a thousand sites over 35 files is not one reviewable step.
  - The rungs follow the kinds of text, so the first review settles the conventions the later
    rungs repeat.
  - The design file is a rung alone, being the largest and the one whose em dashes are mostly
    structure.
- Its own cycle, apart from the generic Queue idea: the sweep and the idea are different work, and
  `git log --grep` for the idea should not land in a punctuation diff.
  - The idea follows as a single-step cycle, held in `## Continuation notes` until then.
- Type `docs`: comments and notes are documentation, and `style` is not a common type and is not
  declared in `custom.md`. An earlier rung title used `style`, and it stays as published.
- Exclusions:
  - Frozen history, `notes/chores/` and `notes/done.md`, is left as it is, read as never touched
    ([Frozen history](agent-data/notes.md#frozen-history-chores-and-done)). `tprobe/notes/chores/`
    goes with it.
  - `LICENSE-APACHE`, `.gitignore`, `Cargo.toml` files, and `Cargo.lock` are not prose.
  - The agent-files are not swept here: their hits are specimens naming the characters, and an
    agent-file change is its own cycle.
  - Transcribed text keeps its characters: tool output, published commit titles, quoted external
    text.
- A checker rung: a byte scan cannot enforce the rule, so the check blanks code spans, fenced
  code, and source code outside comments first. Whether `[validate]` runs it is decided at that
  rung.
- Delegation: the sweep goes to a lesser model per file or chunk, under written rules, and is
  reviewed here.
  - The first plan kept the design file's em dashes here, each being a decision. At the rung 75 of
    its 185 turned out to be one pattern, a bold lead and a dash, so the rules could carry them
    and the review took the rest.
  - A word-level comparison with punctuation stripped is the review's safety net: a sweep may add
    conjunctions and nothing else.

#### Ladder details

##### docs: pay the punctuation debt opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write
this block, and bump the version-of-record. No `## Todo` entry moved, the work having arrived
unplanned, and `## Waiting` held nothing to promote.

##### docs: punctuation debt in ring-buffer-design.md

The design file owes the most, 193 lines with a banned character and 128 with a prose semicolon.
Each is resolved by the joins the prose rules name, and inbound links to a moved anchor are
re-pointed in the same rung.

- The conventions this rung settled for the rest of the ladder:
  - A bold lead and a dash, `- **Label** - text` in the old spelling, becomes `- Label: text`,
    per [Leads are labels, unmarked](agent-data/prose.md#leads-are-labels-unmarked), and a second
    colon on the same line is recast as a comma or a sentence.
  - An aside takes commas, parentheses, or two sentences, and a semicolon takes the joins in
    [Semicolons](agent-data/prose.md#semicolons).
  - A comment inside a fenced code block is prose and pays, the code beside it does not. An arrow
    inside a code span is a use and becomes `->`.
  - The multiplication sign is typeable enough to stay, not being on the banned list.
- No heading held a banned character, so no anchor moved.
- The 23 lines over 100 columns are the 23 the file had, tables and literal rows.
- The file went out as seven chunks at `##` boundaries under `tmp/`, one agent each, and was
  reassembled by concatenation. The added words were conjunctions only: and, so, since, but,
  which.

##### docs: punctuation debt in the notes and READMEs

The remaining markdown outside frozen history: `README.md`, `TODO.md`, `notes/`, and the
`tprobe` and `tp_runner` docs.

##### docs: punctuation debt in the src comments

The doc comments and inline comments under `src/`, `examples/`, and `tests/`, where a comment is
prose and the code beside it is not.

##### docs: punctuation debt in the tp crates

The comments under `tprobe/`, `tp_runner/`, and `tp_matrix/`.

##### chore: a prose punctuation debt checker

Nothing detects a new banned character or prose semicolon. A script blanks what is code and
expects zero elsewhere, and is the cycle's acceptance check.

##### docs: pay the punctuation debt closing

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

_None._

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #docs-pay-the-punctuation-debt-opening
[2]: #docs-punctuation-debt-in-ring-buffer-designmd
[3]: #docs-punctuation-debt-in-the-notes-and-readmes
[4]: #docs-punctuation-debt-in-the-src-comments
[5]: #docs-punctuation-debt-in-the-tp-crates
[6]: #chore-a-prose-punctuation-debt-checker
[7]: #docs-pay-the-punctuation-debt-closing
