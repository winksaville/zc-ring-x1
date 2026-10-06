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
- Scratch left for the user to keep or delete: `~/tmp/zc-ab` on the 7600X and the Pi, source trees
  and `tp-stream` builds, the Pi's demo build and test build. Local copies of the tables are in
  `tmp/ab/`.
- The consumer's v3 methods' docs are still in the old form, not the Parameters form the
  producer's sends now use.
- Open for the user: an overview page for reviewers of the v3 sends, now that the API is final.
- Open for the user: whether the Parameters doc form, every parameter documented including `self`,
  becomes a rule in `agent-data/code.md`, its own cycle, with no Todo entry yet. And the ranks of
  the Todos this cycle added, placed by the agent.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### feat: mpsc v4

#### Problem

MPSC v3's consumer cannot wait at an empty ring the way its producer waits at a full one. The
producer has `send` over a policy, `send_spin`, and `send_spin_sleep`, each time a `Ticks`. The
consumer has `reserve_slot_with`, a poll whose policy counts attempts, and `reserve_slot_wait`,
which sleeps at every empty look with no spin before it and no deadline, bounded by the wake's own
timeout alone. A caller cannot compose the timed form, since `now_ticks` and `Deadline::at` are
`pub(crate)`. iiac-perf asked for it in the message thread `m-8`: their round-trip benches never
fill a ring, so the only waits they measure are the consumer's.

#### Solution

Start `mpsc::v4` as a copy of v3, and give its consumer the producer's waits. v3 stays as built, the
reference to measure against.

- The mode stays: v4 has `Single` and `Multi` as v3 does.
- The consumer gets a general reserve over a policy, and the timed spin and spin-then-sleep written
  as policies for it, as `send_spin` and `send_spin_sleep` are for `send`.
- The two sides are symmetric: where a producer wait and a consumer wait have the same semantics
  they have the same name, parameters, and meanings. Where the semantics differ, the cycle finds why
  and what would make them the same, and where they cannot be made the same the names differ.
- The tools gain v4 flavors, and v4 is measured against the matching v3 flavors under one named
  build profile.

#### Acceptance check

- `jj diff --from main --to feat-mpsc-v4 src/mpsc/v3` prints nothing.
- `cargo test --all-features` passes, with v4 tests of the consumer's waits over `NoWake` and
  `Futex`: no spin, a timed spin, and a spin forever, each with no sleep, a timed sleep, and a sleep
  until a commit.
- Every wait method of the v4 producer has a consumer counterpart of the same name form and
  parameter meanings, or the design note says why the two differ.
- `tp-stream` runs each v4 flavor, and the design note holds its rows beside the matching v3 rows,
  each table naming its build profile.
- `cargo run --release --example guide_mpsc_v4` runs to its last line.
- The thread `m-8` holds a reply that links the landed cycle.

#### Ladder

- [feat: mpsc v4 opening][1] (done)
- [feat: mpsc v4 as a copy of mpsc v3][2]
- [feat: mpsc v4 consumer policy][3]
- [feat: mpsc v4 in the tools][4]
- [docs: mpsc v4 consumer waits measured][5]
- [docs: mpsc v4 guide and example][6]
- [feat: mpsc v4 closing][7]

#### Deliberation

- A new version, not a change to v3, the user's call of 2026-10-06: no existing code changes, so
  v3's callers, iiac-perf's benches among them, keep building.
  - Weighed: the timed methods added to v3 over a private mechanism, the smaller change, with the
    symmetric form left for later. It would have changed v3's API twice.
- The mode stays, the user's call of 2026-10-06: the first measurements of dropping it indicate a
  performance hit.
  - The mode drop was this version's first scope, the Todo then titled `MPSC v4: v3 without Single
    and Multi`. That entry stays in `## Todo`, retitled `MPSC without Single and Multi`.
- Symmetric, and identical where the semantics are the same, the user's rule of 2026-10-06, stated
  in the solution.
- No new waiter: the consumer already carries the ring's `W: Wake` and sleeps through `W::wait`, and
  `Wake` already has `wait_until`. What the consumer lacks is the deadline and a policy that can
  choose to sleep.
- The copy is its own rung, so the consumer policy rung's diff shows that change alone.
- The guard stays: the producer takes a closure and the consumer returns a guard, and a guard
  dropped without `release` re-delivers its slot. We think this is a difference in semantics the
  symmetry rule leaves alone, to be confirmed at the consumer policy rung.
- One policy trait for both sides or two is open, decided at the consumer policy rung. The
  producer's `on_lost` has no consumer counterpart.
- The build profile for the tables is one codegen unit and fat LTO, the profile the Todo
  `Measurement builds and the producer-consumer rhythm` found alike on three machines.
- The version advances by a patch, the default.

#### Ladder details

##### feat: mpsc v4 opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write this
block, bump the version-of-record, and rename the package and bins to their `-dev` names.

- The block is written new, not moved from a `## Todo` entry: the entry that named v4 was the mode
  drop, which v4 does not do, so that entry is retitled and stays.
- The design note's one mention of that entry follows its new title.

##### feat: mpsc v4 as a copy of mpsc v3

v4 needs a starting point that is v3 exactly. Copy `src/mpsc/v3/` to `src/mpsc/v4/`, renamed, with
its tests passing and nothing else changed.

##### feat: mpsc v4 consumer policy

The v4 consumer has a poll and an unbounded sleep, where the producer has a policy and two timed
sends. Give the consumer a general reserve over a policy that can sleep to a deadline, and the timed
spin and spin-then-sleep written for it, named by the symmetry rule.

##### feat: mpsc v4 in the tools

No tool can run v4. Add v4 flavors to `tp-stream` and the other tools, beside v3's.

##### docs: mpsc v4 consumer waits measured

Whether the copy and the new reserve cost anything is not known. Measure each v4 flavor against its
v3 flavor under one named build profile, and put the rows in the design note.

##### docs: mpsc v4 guide and example

The user guide, README.md, and `examples/` teach v3 only. Add v4's section and one complete program,
`examples/guide_mpsc_v4.rs`.

##### feat: mpsc v4 closing

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

### Measurement builds and the producer-consumer rhythm

A stream's ns per message measures two cores in a rhythm, and a build that changes either side's
loop slightly can move the rhythm, and so the number, by far more than the change being measured.
The port of the MPSC v3 callers to the policy `send` showed it: under the default release profile
the same functionally identical loops measured up to 1.8 ns apart, in either direction by machine
and row, and under one codegen unit and fat LTO they measured alike on every row of three
machines. `tp-stream -d 1`, three alternating runs per build, 2026-09-29:

- One codegen unit and fat LTO against the default profile, the port's build: the 3900X's SMT
  rows about 35% faster and cross-CCX about 20%, the 7600X's SMT rows 25 to 35% faster but its
  same-CCX and unpinned rows 65 to 87% slower, and the Pi 5's within -4 to +2%.
- The 7600X's same-CCX slowdown is a change of rhythm, not of work: cache-line transfers per
  message rise from 2.0 to 2.4, and the ring is empty 7 to 15% of the time where it was 0.03%, so
  producer and consumer fall out of step and contend on the shared lines.
- So a build profile is no speedup to adopt, and one to fix: every measurement names its profile,
  a comparison between two builds uses the same profile, runs alternate, and more than one machine
  is measured before a change is called a cost or a win.
- Loop alignment weighed with it: 64-byte loop alignment alone moved the 3900X's cross-CCX rows
  about 10% in flavors a change never touched, `mpsc-v2` 51.1 to 46.2 and `spsc-v4` 57.3 to 50.8.
- What sets the rhythm is the open question: which side's loop falls behind, and whether a
  producer that does work between sends, the Todo `Producer work in the streams`, steadies it, as
  a real workload would.
- The A/B runner used here, `tmp/ab/`: two source trees built under two profiles on a remote
  machine over ssh, three alternating rounds, and a median table, worth keeping under `notes/`.
- `tp-stream`'s cache-line transfer column reads 0 on the Pi, and we think its probe counts
  transfers only on x86.
- From `feat: mpsc v3 deadline sends`, the user's reasoning that identical loops fully inlined
  should run alike, and the user's call to measure on the 7600X and the Pi.

### MPSC without Single and Multi

MPSC v3 chooses its mode at compile time, `Single`, one segment and no switch path, or `Multi`,
v2's switching, and the measurements cannot tell them apart: a one-segment `Multi` ring runs
within 8% of `Single`, faster on some machines and placements and slower on others. The mode costs
a type parameter on every v3 type, a field and a check in the control block, and a second flavor in
every tool, for no measured gain. Start a new MPSC version as a copy of the latest with the mode
dropped, and keep the one it copies as built, the reference to measure against and to bring a mode
back from.

- Not `mpsc::v4`, the user's call of 2026-10-06 at the opening of `feat: mpsc v4`: the first
  measurements of dropping the mode indicate a performance hit, so v4 keeps `Single` and `Multi`,
  and this waits on measurements that settle it.
- One mode, v3's `Multi`, and the `M` type parameter gone, so a ring of one segment does what
  `Single` does now. `MpscRing<'a, W>`, and likewise the endpoints.
- `W` and its compile-time `W::WAKES` checks stay, the user's call of 2026-09-30.
- The control block's mode field: kept and fixed at `Multi`, or dropped with a new layout version,
  decided in the cycle.
- Measured against v3's `mpsc-v3`, `mpsc-v3-single`, and `mpsc-v3-futex` in the same tables, with
  each table naming its build profile, per the Todo `Measurement builds and the producer-consumer
  rhythm`.
- Weighed and set aside for now: the wake as a runtime kind in place of the trait, `Spin` or
  `Futex`, set at `init` and written into the control block. Every process attaching reads it, so
  two processes cannot wake a ring two ways, a mismatch the trait cannot detect, and the types lose
  `W`. The cost would be a well-predicted branch where `W::WAKES` now compiles checks out, and an
  enum closed to wakes from outside the crate.
- The measurements, `tp-stream --segments 1`, depth 8, default profile, two runs each, 2026-09-30,
  `Multi` over `Single`: the 7600X 1.07 same-CCX, 0.97 SMT, 1.08 unpinned, the Pi 5 1.04 same-CCX
  and unpinned, the 3900X 0.96 same-CCX, 1.00 cross-CCX, 0.92 SMT and unpinned.
- From `feat: mpsc v3 deadline sends`, the user's proposal of 2026-09-30 to simplify where the
  measurements are within the noise.

### Wake count for sleeping producers

The consumer wakes every producer asleep on a full ring, `FUTEX_WAKE` at `i32::MAX`, at each half
segment of releases, and most go back to sleep. Wake a count instead, about the room freed, passed
through `Wake::wake`, with a woken producer that still sees room and sleepers waking the next, so
none is stranded until its timeout.

- The next cycle after `feat: mpsc v3 deadline sends`, the user's call of 2026-09-28.
- The consumer's side, not a send's argument: a sleeper cannot choose how many the waker wakes.
- Measuring it needs a ring that fills, producers that do real work or a slow consumer, the Todo
  `Producer work in the streams`.

### Zero-copy endpoints: send a buffer, not a message

Zero-copy is the point of these queues, and the rings' API does not show it: a send writes its
message into the ring slot through a closure, which works for any message that fits a slot but
copies the message's bytes there and holds the slot claimed while it does. A thin layer over the
rings makes the zero-copy path the plain one: the producer writes a message into a pool buffer on
its own time, and the ring carries only the buffer's handle, so the message is written once and
never copied, whatever its size.

- The producer: `loan(size)` a buffer from its own pool, write the message, then `send(&self,
  wait, buf) -> Result<(), Full<Buf>>`. The buffer's guard goes in by value, so ownership visibly
  moves, and comes back on a full ring rather than being lost.
- The consumer: `recv(&mut self, wait) -> Result<Buf, Empty>`, the handle checked and turned back
  into a guard over the same bytes in its own mapping, to read in place, forward, or free.
- `wait` is one `Ticks`, over `send_with_backoff_x` with a built-in spin.
- The wire form: a pool id and the buffer's byte offset in that pool, never an address, since
  each process maps the pool at its own. Buffer sizes are powers of two, so a receiver checks an
  offset with a compare and a mask and resolves it with an add, the user's rule of 2026-09-28
  that every cycle counts, measured in a small device's battery life, not in one message's time.
  Today's `Desc` carries a buffer index, which costs a multiply to resolve.
- A trusted mode, `unsafe`, skipping the receiver's checks, for a device whose processes all trust
  each other.
- A pool id both processes agree on by construction: today's is each process's registry slot in
  registration order, so two processes agree only by registering alike.
- For a battery device, sleeping beats spinning by far: the docs say so, and the default spin is
  short.
- Documented as the way to use the queues: the `mpsc::v3` module docs and the user guide's
  zero-copy section, the ring's own closure sends described as the engine and as fine for small
  fixed messages, and a complete example in `examples/` to copy.
- From the Todo `Descriptor queue endpoints` [[11]], which this replaces: the demo's ~20-line send
  path becomes ~3 lines, `to_slot`'s unsafe is audited once inside the crate, both ring flavors,
  SPSC and MPSC, are served, and the sender holds each sender's private overflow pending list.
- Bounded by the Todos `Shared allocation: a pool any process can allocate from`, one allocator
  per pool, so a pool per producer, and `Pool buffers survive their holders`.
- Ranked after `Wake count for sleeping producers`, from `feat: mpsc v3 deadline sends` on
  2026-09-28.

### Clock choices for the deadline sends

A deadline send reads the clock on every check of a full ring, and on Linux that is the vDSO's
`clock_gettime`: a seqlock, the hardware counter, a multiply and a shift to scale it, then the
crate's fold of seconds into nanoseconds, about 20 ns. Offer the clock as a choice and measure
the choices side by side before any is the default beyond today's.

- A `Clock` trait, as `Wake` is one: `now`, the two conversions to ticks, and the conversion of a
  deadline to `CLOCK_MONOTONIC` nanoseconds for the futex.
- `Ticks<C: Clock>` carries its clock, so a caller picks the clock by the conversion it calls,
  `Monotonic::micros_to_ticks(100)` or `Counter::micros_to_ticks(100)`, and a send infers the
  clock from its `Ticks`. No ring type parameter and no cargo feature, since features unify across
  a build and the comparison needs both clocks in one binary. `Monotonic` is the default,
  `Ticks<C = Monotonic>`, so today's callers barely change.
- A clock is not a ring version: it never touches shared memory, and a deadline is private to one
  send.
- The choices:
  - `Monotonic`, today's: `clock_gettime` through `libc` on Linux, `Instant` with `std`.
  - `Counter`: the CPU's own counter, `rdtsc` on x86-64, `CNTVCT_EL0` on Arm64, `rdtime` on
    RISC-V, so a check is one counter read and a compare. The frequency: Arm64 reads it from
    `CNTFRQ_EL0`, x86 calibrates once against `CLOCK_MONOTONIC`, RISC-V's is the device tree's
    timebase. Fallbacks to `Monotonic`: x86 without an invariant TSC, a VM that traps `rdtsc`, a
    RISC-V board that traps `rdtime` to firmware. It also brings the sends to bare-metal targets,
    which have no `libc` clock.
  - `Monotonic` read every N spins, say 16, which spreads the read's cost over the loop with no
    per-architecture code.
  - Sleep only, `spin` of `Ticks::ZERO`, as the energy baseline.
- What to measure, in tp_matrix beside `mpsc-v3` and `mpsc-v3-futex`: the reaction from a freed
  slot to a landed send, how far past its deadline a send gives up, and the CPU and energy spent
  waiting, `perf stat` and RAPL on x86, measured power on a small Arm board.
- A prediction on record before measuring: a clock is read only while the ring is full, and a
  spinning core burns until room or its deadline whatever a read costs, so a cheaper clock buys
  reaction time, not energy, and sleeping is where a battery device saves. We think sleep only
  dominates energy and the clock matters only for reaction.
- Every cycle counts, measured in a small device's battery life, the user's rule, and the reason
  to measure rather than assume. From `feat: mpsc v3 deadline sends` on 2026-09-29.

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

### MPSC v3 message path gaps

`Multi` trailed `Single` in streams that never switch until `fix: mpsc v3 multi matches single without
a switch` kept the switch attempt out of the send loop. What that left is on the futex flavor and at
many producers. The design note's [MPSC v3 measured](notes/ring-buffer-design.md#mpsc-v3-measured)
has the rows.

- The futex flavor trails `mpsc-v3` by 2 to 4 ns/msg at one producer, 12.5 against 10.6 on the SMT
  pair, with nothing asleep, more than a fence every half segment and a flag test explain. The
  producer's wake is already out of line, so the consumer's release check is the next to look at.
- At ten producers on own cores v3 and `Single` trail v2, 292 against 250 ns/msg at depth 8, and the
  futex flavor leads at depth 64, 207 against v2's 240, an order the one-producer rows do not
  predict.
- A mark to beat: the rows in the design note, `tp-stream`, 3900X, 2026-09-27, 0.18.5-13.
- From `feat: attachable MPSC v3`.

### Producer work in the streams

The streams' producers do nothing between sends, so every MPSC measurement is of the claim word
alone, and a second producer costs more than it adds: 108M messages a second from one producer,
19M from two on cores of their own, 5.6M from three, the user's runs of 2026-09-28. Real producers
work between sends, and the more work, the more producers should pay, until the claim word or the
consumer binds.

- Real work, not a timed spin: `--work-bytes K`, each producer fills a K-byte payload in place and
  checksums it, the zero-copy write doing the work, with `--slot-lines` for slots big enough to
  hold it.
- `--verify on|off`: the consumer recomputes each checksum or only reads the message, so the
  consumer can be kept fast to show the producers' scaling, or made to bind, where `full %` rises
  and spin-then-sleep on a full ring finally has a case.
- A model to test: per message, W the producer's work, C its claim and commit, T the claim word's
  handoff, a cache line's transfer, and R the consumer's cost. Throughput is about the least of
  N / (W + C), 1 / T, and 1 / R, so producers pay until N times T reaches W + C.
- Predictions, on record before measuring: at small K more producers still lose, at a few hundred
  bytes two to four beat one, at large K throughput scales with N until the claim word binds,
  sooner across L3s than within one, shared cores best, and with `--verify on` the consumer binds
  first.
- Placements named by where the producers sit, all in one L3 or split across L3s: at three
  producers `own cores near` turned mixed, two in the consumer's L3 and one outside, and ran
  slower than `own cores x-L3`, all three in one other L3, 179 against 87 ns a message, since
  what matters is whether the producers share an L3 with each other.
- With the consumer binding, producers finally sleep on a full ring, so measure the wake there:
  `wake_producers` wakes every sleeper at each half segment of releases, `FUTEX_WAKE` at
  `i32::MAX`, and most go back to sleep, against waking one producer per freed slot, a count
  passed through `Wake::wake`, a stranded sleeper covered by the next check or its timeout. Under
  producers that do no work the ring never fills, so the wake has not been measured.
- Then the same workload in `zcr-test-ipm`'s MPSC mode, the scaling shown between processes.
- Ranked ahead of `MPSC claim contention`, whose weight it decides: if producers with real work
  scale, the claim word matters only at small messages.
- From `feat: attachable MPSC v3`, on 2026-09-28.

### MPSC claim contention

Every MPSC ring, v0 through v3, claims a slot by a CAS loop on one shared word, and with producers
sending flat out the word decides everything: ten producers move 3 to 4M messages a second between
them where one moves 75 to 110M, 10 to 21 cross-core line transfers a message, the ring near empty
and the consumer waiting on the producers. The rows are the design note's [MPSC v3
measured](notes/ring-buffer-design.md#mpsc-v3-measured) and the user's `-p 10` run of 2026-09-27.

- Claim by `fetch_add`: no failed attempt, about one transfer a claim, but a producer claims before
  knowing its slot is free and cannot take the claim back, so giving up on a full ring needs a
  tombstone or a claim that cannot fail. It is also fair: the ticket fixes each producer's place
  at the claim, so waiting, spinning or asleep, can delay a producer but never let another pass
  it, where the CAS loop hands each slot to whoever wins the race.
- Fairness, the property that matters, whatever it is defined as: no producer should always lose.
  A CAS loop has no fairness at all, the next slot going to the core nearest the claim word's
  line, so a producer farther away, across an L3 or behind an SMT pair, can lose nearly every
  race, the user's concern of 2026-09-28. Arrival order is hard to define across cores, so the
  working definitions to weigh are a bounded wait, no producer waiting past K claims by others,
  and a share, each producer's messages within some ratio of the others' when all send flat out.
- Measure it before choosing: the streams check each producer's order and count its messages,
  and do not report the counts. Each producer's share of the messages, and its longest wait in
  claims, per placement, says whether a producer starves today and which fix a definition asks
  for. Waking one sleeper instead of all changes none of it, since a producer that never slept
  can still win the slot first.
- Claim several slots at once: contention divided by the batch, for producers that send in bursts.
- One SPSC ring per producer and a consumer that fans in, the design note's [Fan-in (composition,
  not
  a mode)](notes/ring-buffer-design.md#fan-in-composition-not-a-mode): nothing written by two
  producers, so the count of producers scales until the consumer saturates, at the price of
  polling N rings and of order only per producer.
- Backoff after a lost race is measured first, in `perf: pin every producer`, as the cheap measure
  the others are weighed against.
- A protocol change, so its own cycle, from `feat: attachable MPSC v3` on 2026-09-27.

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

### MPSC read slot borrows fields, not a state struct

MPSC's read slot needs only the segments, the current segment, and the position, so it could
borrow those three fields of the handle and `ConsumerState` could go, the guard's type staying
`MpscReadSlot<'c, T, W>`.

- Costs: a guard of about four words instead of two, on the hot path, so a bench run decides, and
  MPSC no longer matching SPSC, whose guards use nearly all the state and keep the struct.
- Raised in the deliberation of `refactor: make mpsc v3 producer and consumer states private`,
  2026-10-02.

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

[1]: #feat-mpsc-v4-opening
[2]: #feat-mpsc-v4-as-a-copy-of-mpsc-v3
[3]: #feat-mpsc-v4-consumer-policy
[4]: #feat-mpsc-v4-in-the-tools
[5]: #docs-mpsc-v4-consumer-waits-measured
[6]: #docs-mpsc-v4-guide-and-example
[7]: #feat-mpsc-v4-closing
[11]: notes/chores/chores-01.md#follow-on-endpoints-and-wait-policies
