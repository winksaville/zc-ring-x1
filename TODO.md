# Todo and cycle record

This file contains near term tasks with a short description and reference links to more details.
Its shape is [Todo format](agent-data/notes.md#todo-format).

## Continuation notes

Where the agent was, for the agent that comes next: working copy state, the step in flight, an
open question. Ephemeral, never a record. Written before a restart or when a session is about to
lose context, read first at acquaint, acted on, and reset to `_None._` by the reader.

- iiac-perf may want to hear that a message now crosses processes, `feat: test inter-process
  message`. `m-7` is committed and pushed, `0aacfd9b` in `../vc-x1-messages`, and waits on its
  reply.
- Open for the user: `take_over_*(dead, id)` and a holder query, left as `take_over_*(id)`, with no
  Todo entry.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### feat: attachable MPSC v3

#### Problem

The MPSC rings are in-process only: `split` hands out the endpoints, no control block lets a second
process find a ring, and producers cannot come and go. A full or empty ring can only be polled, as
no side can sleep until the other acts.

#### Solution

`mpsc::v3`, v2's claim and seq protocol over a ring that describes itself in the region, as
`spsc::v4` does, with a simpler role model than v4's.

- One claims word: a closed bit, a consumer bit, and a producer count, the most producers set at
  `init`, `u16::MAX` by default. Claiming a role, releasing it, and releasing the ring are each one
  CAS on it.
- No holder ids, no takeover, no intent words, and no scan: a holder that dies is recovered by a
  restart, the narrower contract of `docs: spsc v4 not perfect`.
- The consumer checkpoints its segment, position, and resume positions at `release`. Producers
  keep no state, so a producer claim is a count.
- `release_ring` fails while any role is held, and otherwise closes the ring, clears its magic, and
  frees its segments to the pool. Anyone holding the ring may call it, and when is the creator's
  call.
- Two hot paths chosen at compile time: `Single`, one segment, where a full ring goes straight to
  the policy, and `Multi`, v2's switching. The mode is in the control block and `attach` checks it.
- Waiting: a full producer or an empty consumer can sleep until the other side acts, through a
  small wake trait the crate calls, with a futex implementation on Linux, and error and spin stay
  the `_with` policies they are.
- No unwind guard: v2's `TombstoneOnUnwind` does not carry over, so a panic mid-send is a failure
  like any other, recovered by a restart.

#### Acceptance check

- Two producer processes and one consumer process exchange messages over an attached v3 ring in
  `/dev/shm`, every message checked.
- Roles are claimed, released, and claimed again, in one process and across processes, and the
  consumer resumes where it stopped.
- `release_ring` refuses while a role is held and succeeds once none is, and a later `attach`
  fails.
- A consumer waiting on an empty ring and a producer waiting on a full one are woken by the other
  side.
- `Single` and `Multi` are measured beside v1 and v2 in the tp_matrix tables.

#### Ladder

- [feat: attachable MPSC v3 opening][21] (done)
- [docs: mpsc v3 design and restart domains][22] (done)
- [feat: mpsc v3 as a copy of v2][23] (done)
- [feat: mpsc v3 control block and attach][24] (done)
- [feat: mpsc v3 claims word and roles][25] (done)
- [feat: mpsc v3 ring release][26] (done)
- [feat: mpsc v3 single and multi segment paths][27] (done)
- [feat: mpsc v3 wait and wake][28]
- [feat: mpsc v3 inter-process test][29]
- [perf: mpsc v3 in the measurement tools][30]
- [docs: mpsc v3 in the design note and guide][31]
- [feat: attachable MPSC v3 closing][32]

#### Deliberation

- Restart, not takeover: a dead holder is recovered by restarting the set it belongs to, the user's
  call on 2026-09-27, after `docs: spsc v4 not perfect` found v4's takeover uncertain.
  - So no holder ids, since without takeover nothing can replace a live holder, and `release(self)`
    consumes the endpoint.
  - A producer that dies between claim and commit leaves its slot claimed and jams the ring, and
    the consumer's set restarts. Accepted, the user's call.
- The ring's release: only when no role is held, anyone may call it, and the creator decides when,
  the user's call. Any process may free to a pool, so this needs no shared allocation.
- `Single` and `Multi` as a compile-time choice, starting from v2 as it is, the user's call.
- The unwind guard is removed, the user's call, which settles the open question the Todo entry
  `Attachable MPSC with claimed roles` carried. v0 through v2 keep theirs, in-process baselines
  left as built.
- A `u16` producer count, the user's call.
- Wait and wake in this cycle, as its own rung, the user's call.
- The design section is the first rung after the opening, the user's call.
- Run unattended, the user's explicit waiver on 2026-09-27, confirmed in the session.
  - It covers: the per-rung work review and description review, and the approval of each push to
    the `mpsc-v3` bookmark, from the opening through `docs: mpsc v3 in the design note and guide`.
  - It does not cover: the closing, Land, any push to `main`, or any write to another repo.
  - Where a question would stop the cycle, the agent takes the option that keeps the plan and
    records it in the rung's subsection, and a change of scope stops the cycle.

#### Ladder details

##### feat: attachable MPSC v3 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
Todo entry into this block, bump the version-of-record, and rename to the dev names.

##### docs: mpsc v3 design and restart domains

The design note gains an `## MPSC v3` section: the initial design this cycle builds and the
long-term possibilities, restart domains and what crossing one costs.

- The design fixed what the plan left open, each the agent's call under the waiver:
  - Seven header lines per segment: v2's seal, claim, and in-use lines, v4's info line and table,
    and a claims line holding the roles word, the consumer's checkpoint, and the producers' wait
    words.
  - The consumer checkpoints at `release` only, every segment's resume position included, since
    without a takeover nothing reads it between.
  - Waiting keeps the policy closures: `send_wait` and `reserve_slot_wait` sleep between attempts
    and call the same policy after each wake, so a caller bounds a wait as it bounds a spin.
  - The consumer sleeps on the claim word, bit 31 its waiting flag, so a producer learns of it from
    the claim CAS it already makes. Producers sleep on a sequence word, and the consumer checks
    their count behind a fence every half segment of releases and when it first finds the ring
    empty, not at every release, where the fence would cost every message.
  - `Futex` goes through `libc`, already the package's Linux dependency.
- The design note's Holders and recovery records the unwind guard's decision.

##### feat: mpsc v3 as a copy of v2

v2's files copied to `src/mpsc/v3/` unchanged but for the names, so each later rung's diff is only
what v3 changes.

- The copy keeps v2's tests, so v3 passes them as v2 does until the rungs after it change what it
  is, and its module docs say so.

##### feat: mpsc v3 control block and attach

Segment 0 holds the ring's identity, geometry, mode, and table of segments, so a process holding
the pool and the first segment's index can attach.

- Six header lines for now: the info line and the table join v2's seal, claim, and in-use lines.
  The claims line comes with the roles and the mode with the modes, each into the info line or a
  line of its own, so the layout grows to the design note's seven lines by the rungs that use it.
- The endpoints keep v2's per-process pointers rather than v4's offsets: each process builds its
  own from the table's buffer indices and its pool's buffer array, so nothing process-local is
  shared, and the message path is v2's.
- `attach` validates as v4's does, and its tests are v4's, the pairing across handles done by
  `split` until the roles land.

##### feat: mpsc v3 claims word and roles

The roles are claimed and released through one claims word, and `split`, producer `Clone`, and the
unwind guard go.

- The claims line joins the header as line 4, so the header is the design note's seven lines, and
  the info line gains the most producers and the consumer's resume position per segment.
- The consumer's switch count is its own since its claim, not checkpointed: the count is a
  diagnostic, and the producers' shared count is the ring's.
- New errors: `RingClosed` for a claim on a released ring, ready for the release rung, and
  `BadMaxProducers` for a ring no producer could join. `RoleTaken` also covers a ring at its most
  producers.
- The unwind guard's two tests became `a_panicking_fill_jams_the_ring`, which pins the price: the
  consumer stops at the panicked slot.
- A finding: a ring of segments cannot hold its full `seg_count * seg_capacity` unread, since a
  segment is given back only at the consumer's reserve after its last release. v2 is the same, and
  the resume test keeps its unread count well under capacity.

##### feat: mpsc v3 ring release

`release_ring` closes a ring no role holds and returns its segments to the pool.

- `release_ring(self, &pool)` consumes the handle, and on `RingInUse` or `RingClosed` the ring is
  unchanged, so a caller re-attaches rather than getting the handle back, since an `Err` carrying
  the ring would be a large error type.
- The pool is checked to be the ring's by the address of segment 0, so a release to another
  region's pool is `BadSegment` before anything is written.
- The segments are freed through the pool's view, one guard minted per table entry, the way the
  registry takes buffers back.
- A handle kept past the release, in this process or another, reads a closed ring only until the
  pool reuses the segments, the same contract `attach`'s safety states.

##### feat: mpsc v3 single and multi segment paths

The segment handling becomes a type parameter, so a one-segment ring compiles without the switch
and seal paths.

- A sealed `Mode` trait with an associated `const MULTI: bool`, implemented by `Single` and `Multi`,
  rather than a const generic: the mode reads as a name at the call site, `MpscRing::<Single>`, and
  the trait carries the control block's code beside the flag. Every `if M::MULTI` folds at compile
  time.
- The ring and both endpoints take the mode, `Multi` by default. A default does not apply to a
  path in an expression, so `init` and `attach` are called as `MpscRing::<Multi>::init`, which
  names the mode everywhere a ring is made.
- `Single` needs `seg_count` 1, at `init` and at `attach`. Its producer takes segment 0 whatever the
  claim word's segment bits hold and goes straight to the policy at a full ring, and its consumer
  never loads a seal.
- The mode joins the info line, and `attach` of the other mode is the new `Error::BadMode`, checked
  in every segment's header as the geometry is.

##### feat: mpsc v3 wait and wake

A full producer and an empty consumer can sleep until woken, through a wake trait, with a futex
implementation on Linux.

##### feat: mpsc v3 inter-process test

`zcr-test-ipm` gains an MPSC mode, two producer processes into one consumer process.

##### perf: mpsc v3 in the measurement tools

v3 in tp_matrix beside v1 and v2, both modes.

##### docs: mpsc v3 in the design note and guide

The measured results in the design note, and v3 in the user guide.

##### feat: attachable MPSC v3 closing

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

### Cycle block: the ladder last, above its rung subsections

In a multi-step cycle's block the ladder sits between the acceptance check and the deliberation,
and the rung subsections follow under their own `#### Ladder details` heading, so the ladder and
the subsections it links are apart. The user's preference, 2026-09-26: problem, solution,
acceptance check, deliberation, then `#### Ladder` directly above the rung subsections, with no
`#### Ladder details` heading.

- The agent-files that state the order: `agent-data/notes.md`'s The In Progress block,
  `agent-data/cycle-model.md`'s specimen, and anything citing `Ladder details`, as an
  `agent-files` proposal cycle, its own commit and its own cycle.
- A single-step cycle's block already takes the order, from `feat: test inter-process message`,
  where the user moved it by hand.

### Find a ring by name

A process that reads a ring owns it, and every producer joins it, but how a producer finds the
ring is not decided: its address is `(region, first_segment)`, and nothing maps a name to that.
The design note's [Naming and transport](notes/ring-buffer-design.md#naming-and-transport) settles
the shape, one `find(name)` returning an endpoint so the resolver is all a transport replaces, and
leaves the mechanism open with the Setup plane question.

- Three candidates, from the discussion on 2026-09-26:
  - The filesystem as the directory: a name is a path, `/dev/shm/<name>`, one inbox region per
    name, `first_segment` at a well-known place in the region. No daemon, the OS handles
    permissions and collisions, and a stale name is a file someone removes.
  - A directory region: a well-known shared object holding name, `(region, first_segment)`, and
    owner id entries, inserted by CAS. No daemon, but a stale entry needs the owner word and
    sweeper the pool's crash recovery needs.
  - A broker: a process owning the names that passes region fds over a Unix socket, the most
    dynamic and the nearest to a network resolver, and one more process to supervise.
- Suggested first: the filesystem, behind the one `find`, so replacing it changes one function.
- Open with it: where `first_segment` lives in the region, what a name whose owner died means,
  and cross-process pool ids.
- After `feat: test inter-process message`, whose app hard-codes the region's path and the ring's
  first segment: `find` is what replaces those two constants.

### Shared allocation: a pool any process can allocate from

A pool has one allocator, the single-popper rule of its free-stack, so two processes that each
originate messages need a pool each, and a process may only ever reuse buffers another allocated.
The design note's phase 2, a head the poppers CAS as an (index, generation) pair so a stale view
fails, lets any number of processes allocate from one pool.

- `pool::v2`: v1's multi-stack pool with each stack's head widened to `(index, generation)`,
  the generation bumped on every pop and both halves CAS'd as one word, `alloc` on `&self` and
  the handle `Sync`, so several threads or processes hold allocating handles over one pool. Its
  own magic and layout, so v0 and v1 stay as built, the baselines to measure against.
- The head word: 64 bits, a 32-bit index beside a 32-bit generation, so a shared pool needs
  `target_has_atomic = "64"` and the single-popper pools keep building wherever 32-bit atomics
  do. A packed 32-bit head, the index and the generation sharing the word, is the option for a
  32-bit target at the cost of buffer count and generation width, and is not the first build.
- Free is unchanged, any process pushes with one CAS as now, and the popper's validation stays:
  every link bounds-checked, pops per attempt capped.
- Costs to measure: the 64-bit CAS against v1's 32-bit one in the demo's alloc/free rows, and N
  allocators contending on one head line, which the note's mitigation answers with a small
  private pool per allocator and the shared one as the fallback.
- The per-handle miss counters stay per handle, so a process reads its own misses, not the
  pool's, and the docs say so.
- Trigger: the first workload where two processes must originate messages from one pool without
  a lending protocol. The inter-application test does not need it: one process allocates, the
  other returns or forwards, and a hub or a credit scheme covers more shapes with one allocator.
- From the discussion on 2026-09-25 of processes sharing a pool: a descriptor is the ownership
  token, any process holding one may read, forward, or free the buffer through its own attached
  view, and only allocation is single-owner, so this entry is the one limit on sharing a pool.

### Pool buffers survive their holders

A process that dies holding pool buffers, allocated and never sent, or received and never freed,
leaves them out of the pool for good: the pool knows no holders, so nothing can tell a dead
holder's buffer from a live one's. The ring's roles survive their holders since `fix: spsc v4
roles survive their holders`, and this is the pool half of the same requirement, a crashed
process losing its in-progress work and nothing else (the design note's [Holders and
recovery](notes/ring-buffer-design.md#holders-and-recovery)).

- An owner word in the in-buffer header, the holder id of whoever holds the buffer, written at
  alloc and at each handoff, beside the length and the count that header already owes ([Message
  header shape](notes/ring-buffer-design.md#message-header-shape)).
- A sweeper: given a holder id the app declares dead, it returns that holder's buffers to the
  pool. The crate records and returns, and never judges liveness, as the ring's takeover.
- Waits on two things: `### Shared allocation`, since a pool with one allocator has one owner to
  sweep for, and the in-buffer header's layout, which has no entry of its own yet and is decided
  with the first of this entry or the header's length.
- What a handoff costs: a store of the owner word per send and receive, on the message path,
  which the ring's recovery avoided. Measuring it, and whether a coarser record, per holder
  rather than per buffer, would do, is part of this entry.
- From the requirement of 2026-09-25, written at the close of `fix: spsc v4 roles survive their
  holders`.

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

- The user on 2026-09-26: the stream tests should cover the set of placements the current cpu
  has, SMT, CCX, x-CCX where it exists, and unpinned, as `tp-stream` does.
- A sort option for `tp-stream`'s table, the user's on 2026-09-26: `--sort` taking a key order
  over depth, placement, and flavor, the default likely `depth,placement,flavor`, so each
  flavor's row sits beside the others at the same depth and placement. Today the table runs in
  cell order, placement then flavor then depth, which scatters the rows a comparison wants
  together. The same option fits `tp-matrix`.

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
- Since `feat: attachable SPSC v4` (2026-09-25): v4 rows run beside v3's in the tools, and v4
  streams 0.6 to 2.8 ns per message slower than v3 where no switch happens while its
  single-thread loop is faster, a two-thread cost the code delta does not explain. This entry
  measures both rings and looks for that gap, the v4 rows in the design note as the mark.
- Answered for v4 on 2026-09-26, in `feat: spsc v4 endpoints checkpoint at each switch`: the
  switch checkpoint passes the table by reference to an out-of-line call, which made the copy
  certain and v4 regress, and borrowing `&st.segs` in both endpoints took v4 at depth 64 from
  14.6 to 10.0 ns on the CCX pair and from 19.1 to 12.5 on the SMT pair, under v3's 13.7 and
  17.1 in the same run. The gap was the copy. v3 still copies, and this entry is what changes it.

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

- `spsc::v4` has it since `feat: attachable SPSC v4` (2026-09-25), and since `fix: spsc v4 roles
  survive their holders` (2026-09-26) as `claim_producer(id)` and `claim_consumer(id)` naming a
  holder, given back by `release` and never by a drop. This entry is what remains for the
  single-region rings, and v4's shape is the model.

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

_None._

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[21]: #feat-attachable-mpsc-v3-opening
[22]: #docs-mpsc-v3-design-and-restart-domains
[23]: #feat-mpsc-v3-as-a-copy-of-v2
[24]: #feat-mpsc-v3-control-block-and-attach
[25]: #feat-mpsc-v3-claims-word-and-roles
[26]: #feat-mpsc-v3-ring-release
[27]: #feat-mpsc-v3-single-and-multi-segment-paths
[28]: #feat-mpsc-v3-wait-and-wake
[29]: #feat-mpsc-v3-inter-process-test
[30]: #perf-mpsc-v3-in-the-measurement-tools
[31]: #docs-mpsc-v3-in-the-design-note-and-guide
[32]: #feat-attachable-mpsc-v3-closing
