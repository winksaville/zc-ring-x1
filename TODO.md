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

### feat: attachable SPSC v4

#### Problem

A v3 ring's table of segments, the pointers every message goes through, exists only in the process
that ran `init`, so no second process can join the ring, and the design cannot be measured against
one that keeps the table in shared memory without changing v3 itself.

#### Solution

`spsc::v4`, v3's protocol verbatim over a ring that describes itself in the region, so a second
process attaches through the pool, and v3 stays as built, the baseline to measure against.

- Offsets, not pointers: `Segments` holds each segment's byte offset from the pool's buffer array
  and one base pointer, so the table is the same in every process and a slot access is one add
  over v3's.
- The control block: segment 0's header grows from one line to four: magic, layout version, the
  geometry, `seg_count`, and `given` on the first, the role claims word on the second, then the
  segment table, 32 pool buffer indices, on the last two. Every segment reserves the four lines,
  so the layout stays uniform and `segment_size` is v3's plus 192 bytes.
- `Ring::attach(&pool, first_segment)` reads the control block through an attached `Pool`,
  validates every field and index against the pool's geometry, and builds `Segments` through the
  loader `init` uses. `Ring::first_segment()` gives the index the initializing process hands to
  the other. Every hostile control block is an `Err`, never a panic.
- Roles by name, no `split`: `ring.producer()` and `ring.consumer()` each claim their role by a
  CAS on a claims word in the control block, from a `Ring` that `init` or `attach` returned, and
  a role already held anywhere, in this process or another, is `Err(RoleTaken)`. Dropping an
  endpoint releases its role.
- Join, not resume: a claimed endpoint starts in segment 0 at position 0, as v3's do after
  `split`, so attach is for a process joining before its role has run.
- The default `Ring` stays v3 until v4 measures, and the pool gains `pub(crate)` accessors for
  its base and a buffer's offset by index.

#### Acceptance check

- The attach test: a ring initialized through one `Pool` handle, a second `Ring` attached through
  `Pool::attach` over the same region, the producer from one and the consumer from the other,
  messages across several segment switches received in order, then the reverse pairing, and each
  hostile control block an `Err`. Under Miri too.
- The claims: a second `producer()` while the first is held, from the same `Ring` or a second
  attached one, is `Err(RoleTaken)`, and succeeds once the first is dropped, the consumer alike.
- The comparison: `tp-matrix` and `tp-stream` rows for v3 and v4 at depths 1, 8, 64, and 1024,
  across the CCX and on the SMT pair, recorded in the design note. Prediction on record: v4
  within run-to-run noise of v3 where no switch happens.

#### Ladder

- [feat: attachable SPSC v4 opening][1] (done)
- [feat: spsc v4 as a copy of v3][2] (done)
- [feat: spsc v4 control block and offsets][3] (done)
- [feat: spsc v4 attach and role claims][4]
- [perf: spsc v4 in the measurement tools][5]
- [docs: spsc v4 in the design note and guide][6]
- [feat: attachable SPSC v4 closing][7]

#### Deliberation

- A v4, not a change to v3: the user's call on 2026-09-25, so the two measure side by side, as
  each ring version has against the one before it.
  - v3's fast-path Todo stays a Todo for both, so the v3/v4 difference is the offsets and the
    control block alone. Fixing the per-message table copy in v4 only would confound the
    comparison.
- Offsets in `Segments`: the user's idea on 2026-09-25. Pointers in the table are already
  `base + offset` computed at `init`, so storing the offset and one base changes nothing about
  correctness, and the table becomes plain data that is the same in every process.
  - It is the design note's own rule, Offsets only, everywhere, applied to the ring's table.
  - The cost is one add per slot access, measured by the tools rung rather than assumed, and two
    more lines at the front of each segment.
  - The shared table holds the pool's buffer indices, `u32` each, since a byte offset needs `u64`
    and the pool already validates an index and turns it into a pointer. The private `Segments`
    holds byte offsets, computed once at load, so the hot path stays at one add.
- A control block in segment 0, four lines: one line cannot hold the table (32 indices are
  128 bytes), the claims word wants a line of its own, and a separate pool buffer would waste
  most of one. Every segment reserving the
  same lines keeps the layout uniform, memory being the only cost.
  - The shape is meant for MPSC v2's successor as well, whose `SegmentHeader` is already a
    three-line struct with a seal, a claim word, and an in-use word, so a later attachable MPSC
    puts the same block ahead of them.
- Roles by name instead of `split`: the user's call on 2026-09-25. `split` hands every attacher
  both endpoints and leaves the SPSC contract to the caller's discipline, where a claimed role is
  an `Err` a second producer sees, and a process holds only the endpoint it uses.
  - The `### Endpoint claims word` Todo asked for this in the single-region rings at the cost of a
    layout bump. v4's control block is new, so it takes the claims word for free, and that Todo
    keeps its entry for v0 through v2.
  - The claims line is its own cache line, so the CAS at attach and the store at drop never share
    a line with `given`. One CAS at claim and one store at drop, nothing on the message path.
  - A crashed process leaves its role claimed. Recovery, a forced claim or a reset, is named as
    deferred, since a fresh region per run is the inter-application test's case.
- Join, not resume: v2's `attach` has the same limit, its endpoints starting at position 0, and
  recovering a mid-run position from the seq words is a design of its own. Named in the design
  note, not built here.
- Attach first, as its own cycle: the `### Test an inter-application message` Todo needs it, and
  the user chose this ordering over folding attach into the test's cycle or testing over v2
  first, so each record has one subject.
- The copy is its own rung, so the offsets rung's diff shows the design change and nothing else.
  - No `examples/spsc_v4_segments.rs`: the examples share no code, each a standalone program, so a
    copy would be a third near-duplicate showing nothing the tools' rows do not. The first program
    that needs v4 is the inter-application bin of the next cycle.
- The tools rung is what makes "side by side" true: `tp-matrix`, `tp-stream`, and the demo gain
  v4 beside v3, and the comparison is the acceptance check's second clause.
- The either-pool Todo had reserved the name spsc4. It is retitled to "next versions" at this
  opening.

#### Ladder details

##### feat: attachable SPSC v4 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
`### SPSC v3 attach` Todo entry into this block in its v4 form, bump the version-of-record, and
rename the artifact to `-dev`, `tp_matrix`'s dependency following the package name. `## Waiting`
held nothing to promote. The inter-application test's
decisions went into its Todo entry, and the either-pool Todo was retitled.

- Waiver, the user's on 2026-09-25 at this opening: "proceed completing this cycle except for
  landing on main". It covers every rung's work review, description review, and push, the closing
  included, and it does not cover Land, so `main` waits for the user.

##### feat: spsc v4 as a copy of v3

`src/spsc/v4` as a verbatim copy of v3 with its tests, so the next rung's diff is the design
change alone. No example is copied.

- The copy differs from v3 in its module docs alone: the intro names what the copy is for and
  that v4 behaves as v3 until the rungs after it land, and the guide reference says the guide
  is v3's. `spsc/mod.rs` lists the module, and the default `Ring` stays v3.
- v3 has no magic to make distinct: nothing in its region names the ring, which is the control
  block rung's problem. `mpsc::v2` keeps importing v3's `check_body_type`, `seq_of`, and
  `validate_geometry`, and the copy has its own.
- The 14 copied tests pass.

##### feat: spsc v4 control block and offsets

`Segments` as offsets from the pool's buffer array, and segment 0's four-line control block
written by `init`: the geometry line (magic, layout version, geometry, `seg_count`, `given`), the
claims line, and the two lines of the segment table.

- `SegmentHeader` is a `repr(C)` struct of three cache-aligned parts: `info`, a line of seven
  `AtomicU32`s (magic `"ZCR4"`, layout version, `slot_size`, `seg_capacity`, `seg_count`, the
  segment's own number, and `given`), `claims`, one word on its own line, and `table`, 32 words
  over two lines. Every segment carries the four lines, so `segment_size` grew by 192 bytes and
  the slots of every segment start at one offset.
  - `info` is written in every segment, so each names the ring it belongs to and its number.
    `claims` and `table` are meaningful in segment 0 only, `NO_SEGMENT` (`u32::MAX`) filling the
    table past `seg_count`.
  - `init` stores the magic last, with Release, so a reader that sees it sees the block. The
    claims word is zero until the next rung uses it.
- `Segments` is `base`, the pool's buffer array in this process, `slots`, each segment's slot
  array as a byte offset from `base`, and `header0`, segment 0's header as an offset. `seq` and
  `body` add the offset to the base, and `given` is `header0`'s field rather than a stored
  pointer. The table is `[usize; 32]`, the same 256 bytes v3's pointers were, so the per-message
  copy the fast-path Todo names is unchanged between the two.
- The offsets are from the buffer array, not the region base as the plan said: the pool's
  `bufs` raw pointer carries the region's provenance, so adding an offset to it reaches any
  buffer, where a pointer derived from the `&PoolHeader` reference could reach the header alone
  under Stacked Borrows. `Pool::bufs_ptr()` is the `pub(crate)` accessor, and `BufSlot::idx()`
  already existed for the table's entries.
- `Ring::first_segment()` is the buffer index of segment 0, held in the `Ring` so the attach rung
  can hand it out and take it in.
- Tests: `control_block_names_the_ring` reads every field of every segment's `info`, the table
  against the private offsets, and the claims word, and the layout test and the pool buffer size
  follow the four lines. All 15 pass, under Miri too.

##### feat: spsc v4 attach and role claims

`Ring::attach(&pool, first_segment)` and `Ring::first_segment()`, the pool's `pub(crate)` base and
offset accessors, the shared loader, `producer()` and `consumer()` claiming through the claims
word with release on drop, `split` removed, and the tests of the acceptance check's first two
clauses.

##### perf: spsc v4 in the measurement tools

v4 rows beside v3's in `tp-matrix`, `tp-stream`, and the demo, and the measurement of the
acceptance check's second clause.

##### docs: spsc v4 in the design note and guide

A "SPSC v4: attachable segments" section in the design note with the comparison, v3's Limits
bullet pointing at it, the user guide's attach section, and the README's ring list.

##### feat: attachable SPSC v4 closing

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

### Test an inter-application message

This is one of the primary initial goals of this project and we've
never tested if it works. The minimal test I can think of is an SPSC
between two apps with the producer sending one message to a consumer.
The consumer will be started first and then the producer sends a
message that is a random number and a checksum of that number to
prove the message arrived intact.

- Decided on 2026-09-25, at the opening of `feat: attachable SPSC v4`, which this waits on:
  - The ring is `spsc::v4`, the attachable ring of segments, once that cycle lands.
  - The apps are one new bin, `src/bin/zc-ring-x1-ipc.rs`, with `consumer` and `producer`
    subcommands, runnable by hand in two terminals, and an integration test in `tests/` that
    spawns it through `CARGO_BIN_EXE`, consumer first, and checks the two outputs agree. A test
    re-executing itself was rejected as the harder pattern to follow.
  - The region is a file under the target directory, mapped `MAP_SHARED` through `libc`, which
    becomes a Linux dev-dependency. The consumer creates and sizes it, the producer attaches.

### Improve stream tests

The stream tests are all x-CCX on 3900x:
| line              | placement        |  shape |  ns/msg |  segs | switches | sw/msg | switch ns |
|-------------------|------------------|-------:|--------:|------:|---------:|-------:|----------:|
| spsc3 burst 1t    | core 11          |   4x64 |    22.5 |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 burst 1t    | core 11          |   4x64 |    15.2 |   4/4 |   11,718 |  0.012 |         - |
| spsc3 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
| spsc3 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
| spsc3 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
| spsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
| mpsc2 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
| spsc3 burst 1t    | core 11          |   1x32 |    22.4 |   1/1 |        0 |  0.000 |         - |
| spsc3 burst 1t    | core 11          |   32x1 |    29.5 | 32/32 |  968,750 |  0.969 |       7.3 |
| mpsc2 burst 1t    | core 11          |   1x32 |    15.2 |   1/1 |        0 |  0.000 |         - |
| mpsc2 burst 1t    | core 11          |   32x1 |    30.6 | 32/32 |  968,750 |  0.969 |      15.8 |
| spsc3 stream 2t   | 11,8 x-CCX       |   1x32 |    29.9 |   1/1 |        0 |  0.000 |         - |
| spsc3 stream 2t   | 11,8 x-CCX       |   32x1 |   170.9 | 32/32 |  999,978 |  1.000 |     141.0 |
| mpsc2 stream 2t   | 11,8 x-CCX       |   1x32 |    19.4 |   1/1 |        0 |  0.000 |         - |
| mpsc2 stream 2t   | 11,8 x-CCX       |   32x1 |   293.4 | 32/32 |  998,537 |  0.999 |     274.4 |

But CCX on 7600:
| line              | placement        |  shape |  ns/msg |  segs | switches | sw/msg | switch ns |
|-------------------|------------------|-------:|--------:|------:|---------:|-------:|----------:|
| spsc3 burst 1t    | core 5           |   4x64 |    14.3 |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 burst 1t    | core 5           |   4x64 |     9.3 |   4/4 |   11,718 |  0.012 |         - |
| spsc3 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
| spsc3 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
| spsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
| mpsc2 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
| spsc3 burst 1t    | core 5           |   1x32 |    13.6 |   1/1 |        0 |  0.000 |         - |
| spsc3 burst 1t    | core 5           |   32x1 |    16.9 | 32/32 |  968,750 |  0.969 |       3.4 |
| mpsc2 burst 1t    | core 5           |   1x32 |     8.7 |   1/1 |        0 |  0.000 |         - |
| mpsc2 burst 1t    | core 5           |   32x1 |    17.0 | 32/32 |  968,750 |  0.969 |       8.6 |
| spsc3 stream 2t   | 5,4 CCX          |   1x32 |    17.7 |   1/1 |        0 |  0.000 |         - |
| spsc3 stream 2t   | 5,4 CCX          |   32x1 |    32.2 | 32/32 |  999,978 |  1.000 |      14.5 |
| mpsc2 stream 2t   | 5,4 CCX          |   1x32 |     7.6 |   1/1 |        0 |  0.000 |         - |
| mpsc2 stream 2t   | 5,4 CCX          |   32x1 |    64.3 | 32/32 |  976,943 |  0.977 |      58.0 |

Thus aren't comparable, we should probably stream all the placement variants?

### Unwrap lints for the library

The library has no `unwrap` or `expect` outside tests, but only by discipline. The user
prohibits them in real code, so a lint should enforce it
([`// OK` comments](agent-data/code.md#-ok--comments-on-unwrap-calls-rust)).

- `[lints.clippy]` in `Cargo.toml`: `unwrap_used = "warn"` and `expect_used = "warn"`, so
  validation's `-D warnings` fails any new site in library code.
- Tests are exempt, and the demo and the examples opt out with a crate-level `#![allow(...)]`,
  their setup panics being the right response there.
- The `unwrap_or*` family has no lint and stays under the `// OK:` comment convention.
- Raised by the user on 2026-09-24, at `refactor: segmented pool stack geometry`.

### Next SPSC and MPSC versions over either pool

`spsc::v3` and `mpsc::v2` take their segments from a `pool::v0::Pool` only, so a multi-stack
pool cannot supply them. New versions take either pool, and v3 and v2 stay as they are, the
baselines to measure against.

- A sealed segment-source trait, "a buffer of at least N bytes, or none", implemented by both
  pools. v1's must not panic on a size larger than its largest stack.
- The new versions: copies of the current rings whose `init` takes any segment source. The pool
  is used only in `init`, so the endpoints and every hot path are the copied code.
- Expectation: the new SPSC over a single-stack v1 pool times as its baseline over a v0 pool, and
  the new MPSC likewise, within noise. Rows for each pair: the old ring over v0, the new ring over v0 (the copy
  alone), over a single-stack v1, and over a multi-stack v1 with one stack for segments beside
  the message stacks. The demo first, iiac-perf for the fine comparison.
- Examples: one SPSC and one MPSC program with one v1 pool supplying both the ring's segments
  and messages of several types, dispatched by type-tag on receipt, each MPSC producer with its own
  pool, since a pool has one allocator.
- The user's direction on 2026-09-24, during `feat: segmented pool v1`: new versions rather than
  a generic `init` on v3 and v2, so the original code stays to measure against.
- Retitled on 2026-09-25 from "spsc4 and mpsc3": SPSC v4 became the attachable ring
  (`feat: attachable SPSC v4`), so the numbers here are whatever comes next.

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
  walks too:
  - For spsc-v3 the consumer's give-back word, one transfer.
  - For mpsc-v2 the old segment's seal and the in-use word twice, since both sides
    read-modify-write it, three to four.
  - The two machines agree on that once the placement's cost per transfer is taken out, over 100
    ns cross-CCX on the 3900X and 15 to 20 within the 7600X's one L3.
- Candidates, each measured on the stress table's switch-cost rows:
  - A consumer-owned give-back word for v2 again, now that the taking side is sound by itself
    with the in-use word, so the consumer's give-back is a store to a line producers only read.
  - The seal riding in the slot word as v3's MOVED does, which v2 cannot do at the commit since
    another producer may hold the last slot.
  - A prefetch of the next segment's first line at the take.

### Comparison queues in the demo: cordyceps, crossbeam, iceoryx2

The demo compares the crate's rings only with `std::sync::mpsc`, and the user asked on 2026-09-15
for cordyceps, crossbeam, and iceoryx2 beside them. Its own cycle, since each is a dependency
decision and a harness shape:

- cordyceps is a dev-dependency today, used by `tp-pool`, and the demo is the installed binary, so
  it would become a dependency of the crate. Crossbeam and iceoryx2 would be new ones, and
  iceoryx2 is a shared-memory framework with its own runtime and setup.
- cordyceps's intrusive MPSC and crossbeam's channels move a pointer or a value, not a message in
  place, so their line is a pool buffer or a boxed message crossing, `tp-pool`'s shape, not the
  ring lines'. iceoryx2 is publish-subscribe over shared memory with no direct depth knob.
- Which lines and placements they join, and whether the demo or a `tp-pool` sweep is the place, is
  the design question the `tp-pool` cycle answered once for cordyceps.

### Descriptor queue endpoints

Paired DescSender (loan + send) / DescReceiver (recv) [[11]]:
- own ring endpoint + registry access
- the demo's ~20-line send path becomes ~3 lines
- `to_slot`'s unsafe is audited once inside the crate (recv safe by construction)
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

### Pool inlining and an iiac-perf comparison

The demo's alloc/free rows measure the compiler's inlining more than the pools, as the cycle
`feat: segmented pool v1` found at its bench rung: v0's hot helpers cannot inline across the
crate boundary, and v1's `alloc` stops inlining at four stacks. The demo's single timed loop per
row is also too crude for differences near a nanosecond.

- v0: `#[inline]` on `next_buf_idx`, `buf_ptr`, and the pop, so the baseline is not handicapped.
- v1: the miss path in a `#[cold]` out-of-line fallback, so `alloc` inlines at any stack count.
- Measure v0 and v1 at one and four stacks in [iiac-perf](https://github.com/winksaville/iiac-perf),
  whose harness calibrates and reports distributions. Variants selected by a type parameter on
  the pool, rather than copies of the module, would let one harness binary compare them.
- Raised by the user on 2026-09-24, at the bench rung of `feat: segmented pool v1`.

### Pool vocabulary: alloc and guard

No pool allocates memory: the region is fixed at `init`, and `alloc` pops a free buffer off a
stack. The `alloc` family's name suggests otherwise.

- Candidates: `take` / `take_with` / `take_bytes`, paired with `free` or a `give_back`.
- Reaches both pools, the registry docs, the demo, `tp_matrix`, the guide, and the README.
- "Guard": the docs call a `BufSlot` and the ring slots guards about 266 times, never defined,
  and to most readers a guard is a lock. Define it where readers start, or retire it for
  "slot", which the type names already use. No new text uses "guard" meanwhile, the user's call
  on 2026-09-25.
- Raised by the user on 2026-09-24, at the review of `feat: segmented pool in the registry`.

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

- Generic Queue: `Queue<P>`, a sealed `Single` / `Multi` producer marker over SPSC v3 and MPSC v2
  with `T` per call, open on the common send and on an ISR guard axis that would give thumbv6m
  a multi-producer queue [details](notes/ring-buffer-design.md#generic-queue-idea).
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
- `Message` trait over the payload cast boilerplate: const `TYPE_TAG` + the zerocopy bounds,
  receiver-side dispatch (read the [type-tag](notes/ring-buffer-design.md#type-tag), decode to
  a `Kind`, match, cast) without per-call-site ceremony, and maybe a transport seam so an
  embedded pointer-descriptor profile slots in behind the same API
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

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #feat-attachable-spsc-v4-opening
[2]: #feat-spsc-v4-as-a-copy-of-v3
[3]: #feat-spsc-v4-control-block-and-offsets
[4]: #feat-spsc-v4-attach-and-role-claims
[5]: #perf-spsc-v4-in-the-measurement-tools
[6]: #docs-spsc-v4-in-the-design-note-and-guide
[7]: #feat-attachable-spsc-v4-closing
