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

### fix: spsc v4 roles survive their holders

#### Problem

A v4 endpoint's `Drop` releases its role claim, so a role can be taken again on a ring that has
run, and the new endpoint, starting in segment 0 at position 0, can never work: the retaken
consumer reports `Empty` with a message committed and waiting, the retaken producer `Full` on an
empty ring, and both hang under a spin policy. Nothing says so at the claim. Reported by iiac-perf
in `m-7`, with two more pushbacks: `producer()` reads as a getter for what is a one-time,
cross-process claim, and every in-process caller pays two unwraps for claims that cannot fail on a
fresh ring, which is refused. Behind the bug is the requirement the fix must meet: a process that
holds a role and crashes is replaced, its role taken over by another process that continues the
ring, and a role handed over on purpose the same way.

#### Solution

A claim names its holder, an endpoint checkpoints its state into the region at every segment
switch and at its release, a claim resumes a released role and a takeover replaces a dead holder's
from the checkpoint and a scan of one segment, and a destructor never touches shared memory.

- The claims line, one cache line of segment 0's control block, holds per role a state (free,
  held, released), a holder id the app chooses and the crate never interprets, and the
  checkpoint: `cur`, `pos`, and the producer's `taken` and `claimable` or the consumer's `given`.
  Each segment's info line holds its two resume positions. The control block does not grow.
- `Drop` writes nothing. `release(self)` on either endpoint writes an exact checkpoint and flips
  held to released. Switches write `cur`, `taken` or `given`, and the segment's resume position,
  one or two stores on the switch path, which already writes shared memory, and nothing on the
  message path.
- `claim_producer(id)` and `claim_consumer(id)` replace `producer()` and `consumer()`: on a free
  role the endpoint starts at zero, on a released role it loads the checkpoint and continues, in
  this process or another, and on a held role it is `Err(RoleTaken)`.
- `take_over_producer(id)` and `take_over_consumer(id)` replace a held claim, the caller vouching
  the holder is gone: they load the switch-time checkpoint and find `pos` by scanning the
  checkpointed segment's seq words, the consumer's next slot the oldest committed one, the
  producer's the first claimable one. A replacement consumer loses nothing, since the slot the
  dead one was reading is still committed, and a replacement producer loses at most the slot the
  dead one had reserved and not committed.
- No `split`: two claims are the API, in-process as across processes.
- The design note states the rule, a destructor never touches shared memory, the inbox model (a
  ring belongs to the process that reads it, in a region it owns, named `(region, first_segment)`,
  every producer a joiner), the handoff as release then claim, the takeover as the supervisor's
  call with the crate recording and recovering and never judging liveness, and the pool half of
  the requirement, an owner word in the in-buffer header and a sweeper, as the cycles after
  shared allocation. The guide's joining section follows.

#### Acceptance check

- iiac-perf's scenario as a test: a ring of two segments of four slots, three messages sent and
  received, both roles released, and each claimed again, from the same `Ring` and from a second
  attached one, continues the ring, the next messages in order across a segment switch. A claim
  while a role is held is `Err(RoleTaken)`, and a dropped endpoint, never released, leaves its
  role held. Under Miri too.
- The takeover as a test: each endpoint forgotten mid-run (`mem::forget`) after at least one
  switch, the consumer with a message committed and unread, the producer with a slot reserved and
  uncommitted, and `take_over_*` from a second attached handle continues: every message the dead
  consumer had not released arrives, in order, and the producer's stream resumes with at most
  the uncommitted slot rewritten. Under Miri too.
- `tp_matrix` and the demo build their v4 pairs by two claims, `grep` finds no `split` in
  `src/spsc/v4` and no `Drop` in `src/` outside tests that writes shared memory, and `tp-stream`'s
  v4 rows at depths 8 and up read within run-to-run noise of the tables in the design note's v4
  section, since the message path is untouched.
- The design note names the rule, the inbox model, the handoff, and the takeover, and the guide's
  joining section says how a role is released, reclaimed, and taken over.

#### Deliberation

- The requirement, the user's on 2026-09-25: a pool shared by processes cannot lose what a
  crashed process held. A crashed consumer is restarted or replaced and takes over its duties,
  its in-progress work lost and nothing else. "A claim is for life" and then "release parks the
  role" were proposed and found short of it, since neither answers a holder that never releases.
- Three mechanisms, and only the crate's two are here: a claim names its holder and endpoints
  checkpoint (this cycle), and buffers name their holder so a sweeper reclaims them (an owner
  word in the in-buffer header, with the length and the count that header already owes, the
  cycles after shared allocation). Judging that a holder is dead stays out of a no_std crate: the
  id is the app's, `take_over_*` is the app vouching, and a supervisor process that restarts
  consumers is where the judgment lives.
- Checkpoint at the switch, not per message: `cur`, `taken`, `given`, and the resume table change
  only on the switch path, which writes shared memory already, so recording them there costs one
  or two stores per switch. `pos` within the segment is the one thing left unrecorded, and the
  seq words hold it, so a takeover scans at most `seg_capacity` words once. The message path is
  untouched, which the acceptance check measures.
- What a takeover loses, exactly: a consumer nothing, since an unreleased slot is still committed
  and read again, a producer at most one reserved, uncommitted slot, rewritten. The same for a
  release, whose checkpoint is exact.
- The claims word gains a third state per role, released, so a claim can tell "never taken" from
  "parked" from "held", and only a takeover replaces a held one.
- The rule, not just the fix: the v4 `Drop` was the crate's only destructor writing shared memory,
  and its failure mode was the bug, since a process that dies never runs a destructor and one
  that runs it leaves a lie. Written as a rule so the next attachable ring, an MPSC, starts from
  it, and so teardown and takeover are always deliberate calls.
- Names: `claim_*` says a role is taken, `release` that it is given back with its state,
  `take_over_*` that a holder is being replaced. `producer()` read as a getter, iiac-perf's point,
  taken.
- `split` stays out: iiac-perf's point, that in-process callers cannot fail a claim on a fresh
  ring yet pay two unwraps, was taken at first and then refused by the user on 2026-09-25 on the
  multi-process shape: each app creates its own ring and consumes it, and joins the other's as
  producer, so no process holds both roles of one ring and a call that claims both is for a
  program nobody writes. Two claims, each a `Result`, is the API everywhere.
- The model this settles, from the same discussion: a pool is a shared allocator with no roles,
  any process allocates from any pool it maps (the shared-allocation Todo, behind the
  inter-application test), rings have the roles, and the shapes are meant to survive a network
  transport, recorded in the design note's open questions at this opening.
- The bench's leak, raised beside the pushbacks, is out of scope: the endpoints borrow the region
  and a detached `spawn` needs `'static`, which only a leaked region gives. A std-only owning
  `Region` wrapper is the answer and a follow-up, the piece the inter-application test wants.
- Ahead of `### Test an inter-application message`: that test is the API's first consumer, so
  the API settles first.
- Waiver, the user's on 2026-09-26: "complete this cycle up to but not including the closing and
  I'll review after breakfast". It covers the work reviews, the description reviews, and the
  pushes of `feat: spsc v4 endpoints checkpoint at each switch`, `feat: spsc v4 claim resumes and
  takeover replaces`, and `docs: destructors never touch shared memory`, each pushed to the cycle's
  bookmark, which stays a draft. It does not cover the closing, the close-out shape, or Land, and
  every other rule holds: validation before each push, the stops on a deviation that changes what
  the user agreed.
- From a message, not a Todo entry: `m-7` in the messages repo is the source, so the opening
  moves no entry. The reply `m-7-1` said all three were taken with a claim for life and promised
  the landmark's sha-link at Land, and the correcting line `m-7-2` followed: `split` refused, and
  the fix now a named, checkpointed claim with release and takeover.

#### Ladder

- [fix: spsc v4 roles survive their holders opening][1] (done)
- [feat: spsc v4 claims name their holder][2] (done)
- [feat: spsc v4 endpoints checkpoint at each switch][3] (done)
- [feat: spsc v4 claim resumes and takeover replaces][4] (done)
- [docs: destructors never touch shared memory][5] (done)
- [fix: spsc v4 roles survive their holders closing][6]

##### fix: spsc v4 roles survive their holders opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write
this block from `m-7`, bump the version-of-record, and rename the artifact to `-dev`,
`tp_matrix`'s dependency following. `## Waiting` held nothing to promote. The continuation notes
from the previous cycle described its Land, which is done, so they are reset. The reply `m-7-1`
was written in the messages repo under its protocol, uncommitted there, since committing is the
closer's by default. At the user's call the design note also gained, in this commit, the
decisions of the day's discussion beyond the fix: the shared pool as the default (under Pool
topology and phasing), the in-buffer header's fields, MPMC's two variants, and Naming and
transport, each as an open question so the next cycles find them.

##### feat: spsc v4 claims name their holder

The region's new words laid out, and the claim naming its holder. Nothing yet writes or loads a
checkpoint.

- A role is one word per role in the claims line, `0` free, `u32::MAX` released, anything else
  the holder's id, so claim, release, and takeover are each one CAS on it and two takeovers
  cannot both win. The user's choice on 2026-09-26 over a state word beside an id word, which
  would let a reader see held before the id lands. The two reserved values cost the app two ids,
  refused as the new `Error::BadHolder`.
- The claims line also holds the checkpoint words, the producer's `cur`, `pos`, `taken`, and
  `claimable` and the consumer's `cur` and `pos`, and each segment's info line its two resume
  positions. The consumer's `given` needs no checkpoint word: the shared give-back word is it.
  The layout version is 2.
- `claim_producer(id)` and `claim_consumer(id)` replace `producer()` and `consumer()`, CAS the
  role word from free to the id, and refuse a held role as `RoleTaken`. A released role is
  refused the same way until the resume rung, the user's choice, so the start-at-zero bug has no
  way back in between.
- Neither endpoint has a `Drop`, so a dropped endpoint leaves its role held. `release(self)` CASes
  the role word from its own id to released, so an endpoint whose role was taken over releases
  nothing.
- The tools and the demo claim their pairs as holders 1 and 2.
- The design note's v4 section, at the user's call, pulled forward from the docs rung: the
  control block's new words, the roles bullet rewritten as claims by a named holder with how ids
  are chosen and kept unique and a short how-to, and "Join, not resume" pointing at the rest of
  the cycle. The README gained a short v4 section, a broad overview pointing at the design note.
  The guide, the module docs beyond the names, and the design note's stale "a claim is for life"
  under Naming and transport stay the docs rung's.
- A Todo entry, `### Find a ring by name`, after the inter-application test, from the discussion
  of how a process finds a ring, at the user's call.

##### feat: spsc v4 endpoints checkpoint at each switch

Every switch writes its side's checkpoint into the region, under an intent word that says when the
checkpoint is whole, and `release` adds the position, so a released role carries everything a
successor needs and a dead holder's names the one switch it may have left half done.

- A switch is several stores, and a holder that dies between two leaves a checkpoint that
  disagrees with the seq words, whichever order they take. The user proposed a mutex. A lock
  excludes no one here, since only the holder writes and the role-word CAS already serializes
  successors, and a dead holder's lock stays held with the state inside it half written, so the
  lock's flag was kept and the lock was not: each side's intent word is set, naming the segment
  left, the segment entered, and the free-set bit flipped, before the switch's first checkpoint
  store, and cleared after its last shared store. The user's choice on 2026-09-26.
- The producer writes the left segment's resume position, the intent, `taken`, and `cur` ahead of
  the MOVED commit, the consumer the resume position, the intent, and `cur` ahead of the release,
  and each clears its intent after, the consumer's after the give-back store. All Release.
- `claimable` is not kept, a successor assuming false loads one seq word more, and init writes
  the producer's start `taken` of 1, so the checkpoint is the start state before any switch.
- The checkpoint made the per-message copy of the segment table certain, since the switch path
  passes it by reference to an out-of-line call, and v4 regressed at depths where no switch
  happens. Both endpoints now borrow `&st.segs`, as MPSC v2 does, and v4 streams under v3 at
  depth 8 and up, the numbers in the design note and the `### SPSC v3 fast path` Todo.
- The design note's v4 roles bullet describes the checkpoint, the intent word, and the cost. The
  user's request for stream tests over every placement joined `### Improve stream tests` as a
  Todo rather than a rung, since it is that entry and does not block the cycle.
- Tests: every switch in the burst tests leaves the checkpoint equal to both endpoints' private
  state with no intent set, and `release` writes each side's position. The repair of a set intent
  is the next rung's.

##### feat: spsc v4 claim resumes and takeover replaces

A claim on a released role loads the checkpoint and continues, `take_over_*` replaces a held role
from the switch-time checkpoint and a scan of the checkpointed segment, and the tests of the
acceptance check's first two clauses.

- A claim takes a free or a released role, a takeover any role, each by one CAS from the word it
  loaded and one attempt, so of two racing takeovers one wins and the other is `RoleTaken`
  rather than replacing the winner. A state that cannot be loaded puts the role word back.
- The signature stays the plan's, `take_over_*(id)`. The discussion's `take_over_*(dead, id)`
  and a holder query are left to the review, since the one-attempt CAS already makes a
  simultaneous race safe and only a supervisor that vouches without checking is unguarded.
- A set intent is finished or undone by the one slot the switch left: the producer's still
  claimable means its MOVED commit never happened, the consumer's still the MOVED word that its
  release never did. Undone, the producer rewrites that slot and the consumer reads that message
  again. Finished, each starts where the switch entered, and the consumer stores the give-back
  bit the intent names. The repaired checkpoint is written back and the intent cleared.
- A clear intent leaves the segment and free-set exact, and the position is `release`'s or, for
  a takeover, the scan's: each slot's seq names the position it last held and whether it is
  committed, the `M` positions end just before the producer's, and the committed ones are the
  newest, from the consumer's on. The scan runs again when a live producer's commit lands
  mid-read, and gives up as `BadCheckpoint` after 1024 scans that do not form one window, which
  a ring whose other side is dead cannot cause.
- At depth 1 a slot's seq cannot tell released from committed, so a takeover of a held role
  there is `BadCapacity`, the depth-1 gap raised at the checkpoint rung. A released role at
  depth 1 resumes, its position exact.
- A checkpoint or intent naming a segment the ring does not have is the new `BadCheckpoint`.
- Tests: iiac-perf's scenario from the same handle and a second, a dead consumer holding a read
  and a dead producer holding an uncommitted slot taken over at every stop point through two
  ring-fulls on six geometries, a consumer taken over four times while the producer streams on
  its own thread, both sides dead inside a switch before and after its shared store, depth 1,
  and a bad checkpoint. Under Miri too, with fewer geometries and stop points.
- The README's v4 section says how a role resumes and is taken over. The guide and the design
  note are the docs rung's.

##### docs: destructors never touch shared memory

The rule, the inbox model, the handoff, the takeover and whose judgment it is, and the pool half
named as later cycles, in the design note, the guide's joining section and errors table following,
and the module docs on the new names. The v4 roles bullet and the README's v4 section came
earlier, in `feat: spsc v4 claims name their holder`, and this rung brings them up to the takeover.

- The design note gains `### Holders and recovery`, beside the usage model: the rule, the inbox
  model, handoff and takeover with whose judgment it is, and the pool half, an owner word and a
  sweeper after shared allocation. Its v4 section's "Join, not resume" became "Resume and
  takeover", with the scan, what a takeover loses, and the depth-1 rule, and Naming and
  transport's "a claim is for life" became release or takeover.
- The guide's joining section claims by id under the inbox model, and gains "Handing a role
  over" and "Replacing a dead holder". Its errors table has the claim and takeover errors.
- The module docs' "Attach is a join, not a resume" became "Roles survive their holders", and
  `attach`'s doc says where a claimed role starts.
- Finding for the closing: the rule as the acceptance check states it, no `Drop` in `src/`
  outside tests that writes shared memory, fails on the MPSC rings, v0 through v2, whose
  `TombstoneOnUnwind` guard publishes a tombstoned commit when a panic unwinds through
  `send_with`. It predates the cycle, runs only on unwind in a process that survives, and
  finishes a protocol step. The design note names it as the rule's open exception. Keeping it
  and scoping the check, or removing it in an MPSC cycle, is the user's call at the close-out.

##### fix: spsc v4 roles survive their holders closing

Closing out the cycle. Owed at Land: a line on `m-7` to iiac-perf with the landmark's sha-link,
and a Todo entry for the pool half of crash recovery, an owner word in the in-buffer header and a
sweeper, placed after the header question.

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
- After `### Test an inter-application message`, whose producer needs some answer to how it
  learns `first_segment` and which will show what `find` must do.

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

- `spsc::v4` has it since `feat: attachable SPSC v4` (2026-09-25): `producer()` and `consumer()`
  claim through a line of its control block, released on drop. This entry is what remains for the
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

# References

[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
[1]: #fix-spsc-v4-roles-survive-their-holders-opening
[2]: #feat-spsc-v4-claims-name-their-holder
[3]: #feat-spsc-v4-endpoints-checkpoint-at-each-switch
[4]: #feat-spsc-v4-claim-resumes-and-takeover-replaces
[5]: #docs-destructors-never-touch-shared-memory
[6]: #fix-spsc-v4-roles-survive-their-holders-closing
