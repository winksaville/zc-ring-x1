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
- Cycle `feat: mpsc v3 deadline sends` is mid-ladder: `feat: mpsc v3 deadline sends behind std` is
  pushed, and the next rung is `refactor: port mpsc v3 callers to the deadline sends`, not yet
  started. The version hold is in effect, `0.18.5` with the `-dev` names, per the block's
  deliberation.
- For the port rung: the bins that call the `_x` sends need `std`, and `cargo install` skips a bin
  whose required features are off, so the validate list's install likely becomes `--features
  std`. The other v3 methods' docs, `send_with`, `send_with_backoff`, `send_wait`, and the
  consumer's, are still in the old form, not the Parameters form the two new sends now use.
- Open for the user: whether the Parameters doc form, every parameter documented including
  `self`, becomes a rule in `agent-data/code.md`, its own cycle, with no Todo entry yet. And the
  rank of the Todo `Zero-copy endpoints: send a buffer, not a message`, placed second by the agent
  since the user had not picked one.

## In Progress

A cycle's record has one home at a time, and while the cycle runs this is it. The block's
shape is the specimen in [cycle-model.md](agent-data/cycle-model.md), and the rules are in
[The In Progress block](agent-data/notes.md#the-in-progress-block).

### feat: mpsc v3 deadline sends

#### Problem

An MPSC v3 producer bounds a send on a full ring through closures: `send_with` takes an `on_full`
policy called with an attempt count, `send_with_backoff` adds an `on_lost` hook called after each
lost claim race, and `send_wait` sleeps between the policy's calls. A caller who wants "give up
after this long" writes a closure that reads a clock, and the crate is `no_std`, so it has no clock
to offer.

#### Solution

Two sends that take times in microseconds instead of closures, behind a new `std` feature, beside
the closure sends until porting their callers decides whether those go.

- `send_with_x(give_up, fill)`: spin on a full ring until there is room or `give_up` has passed,
  then `Full`. `Micros(0)` is one probe and `Micros::FOREVER` waits forever.
- `send_with_backoff_x(spin, wait, fill)`: spin for up to `spin`, then sleep on the producers' futex
  until `wait` more has passed, sleeping again on what is left after an early return, then `Full`.
  Under `NoWake` the sleep is a spin.
- Times are `Micros`, a microsecond count, and a wait's lost-wake guard is `Seen`, a word and the
  value it was last seen holding.
- `Wake` gains `wait_for`, a wait bounded by the caller's timeout, beside `wait` and
  `Futex<TIMEOUT_MS>`, which the untimed waits keep.
- The `std` feature is off by default, the clock is read in one function, and validation builds
  the library with the feature and without it.
- The v3 callers move to the `_x` sends. If none still needs a closure, `send_with`,
  `send_with_backoff`, and `send_wait` are deleted and the `_x` sends take the plain names.

#### Acceptance check

- A test shows `send_with_x` on a full ring returns `Full` no sooner than its deadline and within a
  bound after it, and succeeds once the consumer frees room.
- A test shows `send_with_backoff_x` under `Futex` sleeping on a full ring, woken when the consumer
  frees room, and returning `Full` at `wait_us` when no room comes.
- `cargo clippy` and `cargo test` pass with `--all-features`, and the library builds with the
  feature off.

#### Ladder

- [feat: mpsc v3 deadline sends opening][21] (done)
- [feat: mpsc v3 deadline sends behind std][22] (done)
- [refactor: port mpsc v3 callers to the deadline sends][23]
- [refactor: drop mpsc v3 closure sends][24]
- [feat: mpsc v3 deadline sends closing][25]

#### Deliberation

- Times, not closures: a send is bounded by a time in microseconds, the user's proposal of
  2026-09-28.
  - A caller who wants another policy composes it from probes, so the closures' flexibility moves
    to the call site rather than being lost.
  - The spin-then-sleep send is today's `send_wait` reshaped, not today's `send_with_backoff`,
    whose backoff is after a lost claim race, not on a full ring.
- Behind `std` for now, the user's call, until a `no_std` clock is chosen.
  - Off by default, since the crate is `no_std`, and the bins and tools that call the sends turn it
    on.
  - The clock is read in one function, so a `no_std` clock replaces it in one place. We think the
    first is Linux's `clock_gettime` through the `libc` the crate already links for the futex.
- Types for a wait's arguments, the user's call of 2026-09-28 at review, since a bare `expected`
  said nothing of what value to pass.
  - `Seen` pairs the word with its value, so the two cannot be mismatched, and its constructor
    says where the value came from: `load` before the look, or `written` by the caller's own
    read-modify-write.
  - `Micros(u32)` over `core::time::Duration`: 4 bytes, not 16, and the microseconds the sends
    were asked in, with `Micros::FOREVER` the time that never passes.
- `wait_for` beside `wait`, not in place of it, the user's call of 2026-09-28 in `feat: mpsc v3
  deadline sends behind std`.
  - The plan replaced `Futex<TIMEOUT_MS>` with a per-call timeout, but the consumer's
    `reserve_slot_wait` sleeps untimed too, and giving it a timeout changes the consumer's API,
    outside this cycle.
  - The cost: two timeout sources until the consumer has a deadline wait of its own, when
    `TIMEOUT_MS` can go.
- Fresh `_x` names during the cycle, the user's call, so old and new sit side by side and callers
  move one at a time.
- Delete only if the port leaves no holdout: the port is the evidence. The lost-race backoff,
  `on_lost`, is the likely holdout, and is either built in or kept.
- The wake count stays wake-all, the user's call: how many sleeping producers the consumer wakes is
  the consumer's choice, not a send's argument, and the next cycle takes it, the Todo `Wake count
  for sleeping producers`.
- v3 only: v0 through v2 keep their closure sends, as built, and iiac-perf's benches call them.
- No version bump on this cycle's commits, the user's explicit bend of 2026-09-28.
  - It covers: the opening's bump and each rung's, so the version-of-record stays `0.18.5`.
  - It does not cover: the closing, which sets the version, nor the dev name, which the opening
    sets as usual.
  - Why: the size of the bump depends on whether the closure sends are deleted, a break, and that
    is decided mid-cycle.

#### Ladder details

##### feat: mpsc v3 deadline sends opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, write this
block, add the wake-count Todo, and rename the package and bins to their `-dev` names, with no
version bump.

##### feat: mpsc v3 deadline sends behind std

A send bounded by time needs a closure that reads a clock the crate cannot offer. Add the `std`
feature, `send_with_x` and `send_with_backoff_x`, a per-call futex timeout, and validation of both
builds.

* A time-bounded send needs a clock, and the crate is `no_std`.
  - The `std` feature, off by default, turns `no_std` off and brings in `clock`, one `Stopwatch`
    type over `std::time::Instant`, so a `no_std` clock replaces it there and nowhere else.
  - Validation lints the library alone, which is the `no_std` build, then lints and tests
    everything with every feature.
* The sends keep the closure sends' loop.
  - Each is the shared `send` with an `on_full` closure that reads the clock, started at the first
    full ring, so a send that finds room never reads it.
  - `Micros::FOREVER`, `u32::MAX` microseconds, is a deadline that never passes and a spin that
    never ends.
* A wait's `expected` was a bare `u32`, and what value to pass was the caller's to know.
  - It is the futex's guard against a lost wake: the value the word held before the caller's last
    look, so a waker's change between the look and the sleep makes the sleep return at once.
  - `Seen` carries the word and that value together, `Seen::load` for the producer's wake counter
    and `Seen::written` for the claim word the consumer's `fetch_or` flagged.
  - `Micros` is the time type throughout, the crate root's, beside `Full`.
* The backoff send sleeps on what is left of its wait.
  - `sleep_full` takes an optional timeout, and `Wake::wait_for` bounds one sleep by it, a
    `FUTEX_WAIT` of that many microseconds, a spin hint under `NoWake`.
  - Under `NoWake` the sleep phase skips `sleep_full`, whose waiter count would put a shared
    atomic write in every spin, and spins instead.
* The waiting tests' bounds were seconds, so a broken wake hung a test that long before failing.
  - The user's call at review: none needs to be long, only clearly over a wake or a deadline. The
    wake bound is 250 ms, `SlowFutex`'s timeout and the deadline a woken send must beat are 500 ms,
    the last cycle's waiting tests included.
* The sends' docs assumed the reader knew the closure sends, and named neither every parameter nor
  what `fill` must do.
  - The user's call at review: every parameter documented, `self` included, under fixed sections,
    Parameters, Type parameters, Returns, and Notes, nothing left to "otherwise as" another method.
  - `fill` writes the message and must write all of it, since the slot still holds the last
    message's bytes, and `T` names `Desc` as the zero-copy use.
  - The zero-copy framing the queues are for, a thin layer sending a buffer's handle, is the new
    Todo `Zero-copy endpoints: send a buffer, not a message`, which absorbs `Descriptor queue
    endpoints`.
* Deferred to the port: once the demo and `zcr-test-ipm` call the sends they need `std`, and
  `cargo install` skips a bin whose required features are off, so the install likely becomes
  `--features std`.

##### refactor: port mpsc v3 callers to the deadline sends

The demo, `zcr-test-ipm`, the `tp_matrix` tools, the v3 tests, and the `policy.rs` docs call the
closure sends. Move each to an `_x` send, and record any that cannot move as a holdout.

##### refactor: drop mpsc v3 closure sends

Runs only when the port leaves no holdout. Delete `send_with`, `send_with_backoff`, and
`send_wait`, and rename the `_x` sends to the plain names.

##### feat: mpsc v3 deadline sends closing

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
- `wait` is one `Micros`, over `send_with_backoff_x` with a built-in spin.
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
- One SPSC ring per producer and a consumer that fans in, the design note's [Fan-in (composition, not
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
[21]: #feat-mpsc-v3-deadline-sends-opening
[22]: #feat-mpsc-v3-deadline-sends-behind-std
[23]: #refactor-port-mpsc-v3-callers-to-the-deadline-sends
[24]: #refactor-drop-mpsc-v3-closure-sends
[25]: #feat-mpsc-v3-deadline-sends-closing
