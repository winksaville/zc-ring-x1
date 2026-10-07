# zc-ring-x1

Zero copy ring buffer experiment 1

> **Development has moved to
> [zc-msg-x1](https://github.com/winksaville/zc-msg-x1)**,
> the follow-on focused on the MPSC messaging layer (ring +
> pools + endpoints). This repo remains the ring-protocol
> record, SPSC and MPSC, compared and measured. Why and
> what moves: [notes/zc-msg-x1.md](notes/zc-msg-x1.md).

This is the main repo of a dual-repo convention for using
a bot to help develop the code.

The beginnings of a tool to help is [vc-x1](https://github.com/winksaville/vc-x1)

## Overview

A `no_std` SPSC (single-producer / single-consumer) ring buffer
over a caller-provided memory region, moving typed messages with
zero copying at the boundary: the producer writes through a
`&mut T` directly into buffer memory, the consumer reads through
a `&T` in place. Typed views are validated casts via
[zerocopy](https://docs.rs/zerocopy), and the same `#[repr(C)]`
layout works between threads and, via shared memory, between
processes. The full design (requirements, memory layout, index
scheme, API, validation) lives in
[notes/ring-buffer-design.md](notes/ring-buffer-design.md), kept
in sync with `src/`.

- M slots × N bytes, both caller-chosen: M a power of two, N a
  cache-line multiple, every slot cache-line aligned.
- Free-running `AtomicU32` indices, masked only at slot access,
  no sacrificial slot, lock-free with Acquire/Release pairing.
- Guard API: `reserve_slot_with::<T>(policy)` on either
  endpoint: write, then `commit()` on the producer side, read,
  then `release()` on the consumer side. Dropping a guard abandons
  cleanly.
- IPC-ready: one contiguous region, no internal pointers, an
  init/attach handshake (magic published last, Release), and an
  all-atomic header so a misbehaving peer can corrupt data but
  never cause UB.
- An app-owned scratch line in the header (`user()` on both
  endpoints, 16 `AtomicU32`s): the crate zeroes it at init and
  never touches it again, a shared-memory home for app wakeup
  protocols. The crate itself never blocks or spins.
- Validated by tests (including a threaded stress and a
  u32-index-wrap proof) and [Miri](https://github.com/rust-lang/miri)
  on the non-threaded suite.

The crate-root `Ring` is `spsc::v3`, a ring of segments taken
from a pool at `init`: with a consumer that keeps up it lives in
one segment, and the others absorb a producer that runs ahead
([Segment lifecycle](notes/ring-buffer-design.md#segment-lifecycle)).
The points above describe the single-region rings, `spsc::v0`
through `spsc::v2`, which stay available by path and keep
`attach` and `user()`. `spsc::v4`, by path, is the ring of
segments with `attach`: its segment 0 carries a control block,
its endpoints keep offsets, and a second process joins through
the pool and claims its role for a named holder, held once
anywhere ([SPSC v4: attachable ring](#spsc-v4-attachable-ring)).
The multi-producer siblings are
`mpsc::v2::MpscRing`, the same ring of segments with any number
of producers sending through a fill closure, and `mpsc::v3`, v2
attachable with counted roles and waiting, both reached by path
since the crate-root `MpscRing` is still the single-region
`mpsc::v1`. Every version, what it adds, and which to use is
[The ring versions](#the-ring-versions). How to use the
segmented rings from a pool to two threads, with complete
programs, is the [user guide](notes/user-guide.md).

```rust
use zc_ring_x1::{Pool, PoolHeader, Ring};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

// A pool of two segments, each a header line + 4 slots × 64 B.
const SEG: usize = 64 + 4 * 64;
#[repr(C, align(64))]
struct Region([u8; size_of::<PoolHeader>() + 2 * SEG]);
let mut region = Region([0; size_of::<PoolHeader>() + 2 * SEG]);

let mut pool = Pool::init(&mut region.0, SEG as u32, 2).unwrap();
let (mut producer, mut consumer) =
    Ring::init(&mut pool, 64, 4, 2).unwrap().split();

let mut slot = producer.reserve_slot_with::<Msg>(|_| false).unwrap();
slot.seq = 1;
slot.val = 42;
slot.commit(); // publish to the consumer

let msg = consumer.reserve_slot_with::<Msg>(|_| false).unwrap();
assert_eq!(msg.val, 42);
msg.release(); // slot is free for reuse
```

Status: an experiment, SPSC and MPSC, in-process for the
segmented rings and between processes for the single-region
ones, where attaching to an existing shared-memory region is
`unsafe` (see `spsc::v2::Ring::attach`). Planned hardening and
follow-ons are tracked in [TODO.md](TODO.md).

## The ring versions

Every version is a live module, reached by its path, so each is measured beside the others, and
the crate root's `Ring` and `MpscRing` name the current defaults. Each version is a sibling of the
one before it, not a replacement: it changes one idea and keeps the rest. The design note's section
for each has the why and the measurements.

SPSC, one producer and one consumer:

| Version | What it is | Region | Joined from another process |
|---|---|---|---|
| `spsc::v0` | the first ring: a head and a tail index, each side polling the other's | one region | `attach` |
| `spsc::v1` | a seq word per slot in an array, so each side reads slot lines and its own index | one region | `attach` |
| `spsc::v2` | the seq word moved into its slot, one line per message | one region | `attach` |
| `spsc::v3`, the crate's `Ring` | v2's slots over a ring of segments from a pool, the spare segments taking a producer that runs ahead | segments | no |
| `spsc::v4` | v3 with a control block, so a process joins through the pool, and roles claimed by a named holder, released, and taken over | segments | `attach` and role claims |

MPSC, any number of producers and one consumer, a producer claiming its slot by a CAS on a shared
claim word and filling it through a closure:

| Version | What it is | Region | Joined from another process |
|---|---|---|---|
| `mpsc::v0` | Vyukov's bounded queue, depth 2 and up | one region | `attach` |
| `mpsc::v1`, the crate's `MpscRing` | v0 with seq values that also work at depth 1 | one region | `attach` |
| `mpsc::v2` | v1's claim over a ring of segments, the claim word doubling as the seal | segments | no |
| `mpsc::v3` | v2 with a control block, counted roles, the ring's release, compile-time modes, and waiting | segments | `attach` and role claims |
| `mpsc::v4` | v3 with a consumer that receives as a producer sends, a choice of how the endpoints wait, and plain names | segments | `attach` and roles |

`mpsc::v3` in more detail, the one ring that takes type parameters, `MpscRing<'a, M, W>`:

- The mode `M`: `Multi`, v2's switching over up to 32 segments, or `Single`, one segment, whose
  code has no switch path at all. While a `Multi` ring does not switch, it runs as fast as a
  `Single` one.
- The wake `W`: `NoWake`, where a wait is a spin, or `Futex` on Linux, where `send_spin_sleep` and
  `reserve_slot_wait` sleep in the kernel until the other side acts. `NoWake` compiles every wake
  check out.
- Roles are counted, not named: one consumer and producers up to a most, each claimed and
  released by one CAS, and a holder that dies is recovered by restarting the set the ring
  belongs to, not by a takeover.
- Producers send with `send_spin`, spinning on a full ring for up to a time, `send_spin_sleep`,
  spinning and then sleeping, or `send`, under a `SendPolicy` of the caller's, which decides at a
  full ring and hears of each lost claim race. Times are `Ticks`, made once by
  `microsecs_to_ticks` or `nanos_to_ticks`.

The measurement tools name each ring `xpsc-vN` after its path, and name the v3 variants by what
they add:

| Flavor | Ring |
|---|---|
| `mpsc-v3` | `MpscRing<Multi, NoWake>`, sending by `send` with a spinning policy |
| `mpsc-v3-single` | `MpscRing<Single, NoWake>` |
| `mpsc-v3-futex` | `MpscRing<Multi, Futex>`, still spinning, so it measures the wake checks' cost with nothing asleep |
| `mpsc-v3-backoff` | `MpscRing<Multi, NoWake>`, sending by `send` with a policy whose `on_lost` is `policy::backoff` |

Which to use, from the measurements so far:

- One producer between threads: `spsc::v2` where one region is enough, `spsc::v3` where a
  producer runs ahead in bursts.
- One producer between processes: `mpsc::v3`, faster than `spsc::v4` at depth 8 in the streams,
  8.9 against 13.0 ns a message on two cores of one L3 in `Single` mode, which a one-segment
  `Multi` ring matches within 8% on three machines, or `spsc::v4` where a dead holder must be
  taken over rather than restarted.
- Several producers: `mpsc::v3`, one segment where it holds the traffic, in either mode, and a
  backoff policy when producers contend. With producers doing nothing between sends a second producer costs more
  than it adds, since every send passes through the one claim word, and how much work a
  producer must do before more of them pay is the Todo `Producer work in the streams`.

## Message pool

The companion allocation primitive: fixed-size
cache-line-aligned buffers over a caller-provided region,
decoupling "get a message" from "send it": allocate a
buffer, hold it as long as you like, free (or eventually
send) it whenever. Design and roadmap:
[Messaging layer: pools and descriptor queues](notes/ring-buffer-design.md#messaging-layer-pools-and-descriptor-queues).

- Intrusive LIFO free-stack with zero per-buffer overhead: a
  *free* buffer's first word is its next-link, and an allocated
  buffer carries no header at all.
- `alloc::<T>()` returns a `BufSlot<T>`, the ownership
  token for one buffer, `Deref`/`DerefMut` to `T` in place.
  The pop is single-popper Treiber (`&mut self`, no ABA):
  one allocator per pool.
- `BufSlot::free` is an MPSC push: whichever thread or
  process ends up holding the guard may free it.
- `Pool::init` creates and validates a fresh region, and
  `Pool::attach` joins one initialized elsewhere, `unsafe`
  like `Ring::attach` (the caller vouches for the mapping),
  and its contract carries the single-allocator rule: at
  most one attached handle allocates, any handle frees.
- Messaging aside, a pool is a fast fixed-size object
  allocator in its own right: O(1) alloc/free, no locks, no
  syscalls, cache-aligned `T`s with LIFO reuse keeping the
  working set hot, usable today for app-owned objects that
  never travel.
- `BufSlot` does not borrow the pool, so any number of
  allocated buffers live at once. Dropping without `free`
  leaks the buffer (documented, like the ring's abandon but
  with the opposite consequence).
- Free-stack words are untrusted shared memory: pops are
  bounds-checked and corruption degrades to `Exhausted`,
  never out-of-bounds access.
- Same init/attach handshake, all-atomic header, and
  hostile-peer posture as the ring.

```rust
use zc_ring_x1::Pool;
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

#[derive(FromBytes, IntoBytes, KnownLayout)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

// Cache-line-aligned region: 128 B header + 4 bufs × 64 B.
#[repr(C, align(64))]
struct Region([u8; 384]);
let mut region = Region([0; 384]);

let mut pool = Pool::init(&mut region.0, 64, 4).unwrap();

let mut a = pool.alloc::<Msg>().unwrap();
let mut b = pool.alloc::<Msg>().unwrap(); // many live at once
a.seq = 1;
b.seq = 2;
b.free(); // any order, from any thread the guard moved to
a.free();
```

## Usage model: roles and buffer lifecycle

Who may do what, per object and per buffer state. The tests
exercise exactly what this model permits. Anything outside it
is a contract violation even where the compiler cannot reject
it.

**Roles.** Roles are per *object*, and one thread may hold
several roles across objects (a typical sender is one pool's
allocator and one ring's producer):

- **Ring**: exactly one producer and one consumer, ever
  (SPSC): in-process, `split()` hands out each endpoint
  once, and cross-process, one producing and one consuming
  process by contract. An endpoint reserves at most one
  slot at a time (guard holds the endpoint borrow).
- **Pool**: exactly one *allocator* at a time (the
  single-popper contract on `Pool::attach`, and in-process
  `alloc` takes `&mut self`), and **any number of freers**:
  whoever holds a `BufSlot` may free it, from any thread or
  process attached to the pool.

**Buffer lifecycle.** A pool buffer is always in exactly one
state, with one party allowed to touch it:

- **free**: on the free-stack, and the pool protocol owns it.
  Nobody reads or writes it except through alloc/free
  machinery (its first word is the next-link).
- **allocated**: popped by `alloc`, owned by the `BufSlot`
  holder: exclusive read/write through the guard, held as
  long as desired, moved between threads freely (`Send`).
  No other party, the pool included, may touch the
  bytes.
- **in-flight**: the descriptor form, where `to_desc` consumes
  the guard and ownership travels in the returned `Desc`
  (typically through a ring). The sender must no longer
  touch the buffer. Whoever takes the descriptor back with `to_slot` owns
  it, exactly once.
- **freed**: `BufSlot::free` pushes it back, and ownership
  returns to the pool protocol the instant the CAS lands.
  Use-after-free of the guard is unrepresentable (`free`
  consumes it), and dropping without `free` leaks the buffer.

**What "send" means.** Two forms compose:

- Moving the `BufSlot` itself (an ordinary Rust move, and
  `Send` lets it cross threads or a std channel).
- The descriptor flow (0.7.0): register the pool in a
  per-process `PoolRegistry`, convert the guard to an
  8-byte `Desc` with `to_desc`, and send *that* through
  a ring as an ordinary POD message, and the receiver
  takes it back to an owned guard with `to_slot`. The payload stays
  put in its pool buffer:

```rust,ignore
// Sender: pool allocator + ring producer.
let mut msg = pool.alloc::<Msg>()?;   // get a message
msg.seq = 42;                         // fill it in place
let desc = registry.to_desc(pool_id, msg)?; // guard -> Desc
let mut slot = producer.reserve_slot_with::<Desc>(|_| false)?;
*slot = desc;                         // 8 bytes, not the payload
slot.commit();

// Receiver: ring consumer + freer (another thread).
let slot = consumer.reserve_slot_with::<Desc>(|_| false)?;
let desc = *slot;
slot.release();                       // ring slot free again
// SAFETY: desc came from to_desc, arrived via the ring's
// commit -> reserve handoff, taken back exactly once.
let msg = unsafe { registry.to_slot::<Msg>(desc) }?;
//                  ... read msg ...
msg.free();                           // buffer back to its pool
```

One allocation's bytes are written once and never copied,
not by send, not by receive. (`to_slot` is the one `unsafe`:
validation rejects unknown ids / bad indices / wrong types,
but ownership uniqueness is the caller's promise. Paired
sender/receiver endpoints that encapsulate it, and shrink
this to loan / send / recv, are the next cycle, see
TODO.md.)

**Trust.** Unchanged from the ring: every word in shared
memory is untrusted input. The pool validates the head and
next-link at every pop and fails toward `Exhausted`, so a
hostile peer can degrade service (garbage messages, lost
buffers, spurious exhaustion), never cause UB on this side.

## SPSC v4: attachable ring

`spsc::v4` is the ring of segments a second process can join:
v3's protocol, over a ring that describes itself in the region.
The details are the design note's [SPSC
v4](notes/ring-buffer-design.md#spsc-v4-attachable-segments), and
the how-to the guide's [Joining from another
process](notes/user-guide.md#joining-from-another-process).

- **Found by an index**: segment 0's control block names the
  ring, its geometry, and every segment's pool buffer index, so
  a process holding the same pool and `ring.first_segment()`
  attaches with `Ring::attach`, every field validated.
- **Roles claimed by a holder**: `claim_producer(id)` and
  `claim_consumer(id)` write the app's id for the holder into
  the role's word, one CAS, so a role is held once anywhere and
  a second claim is `RoleTaken`. The id is the app's, the pid
  being the natural one, and the crate never interprets it.
- **Released, never dropped**: an endpoint has no `Drop`, since
  a destructor never touches shared memory, so dropping one
  leaves its role held and `release()` gives it back. Why, and
  the inbox model behind it, is the design note's [Holders and
  recovery](notes/ring-buffer-design.md#holders-and-recovery).
- **Proven between processes**: `zcr-test-ipm` sends one random
  value with its checksum from one process to another through a
  v4 ring in `/dev/shm/zcr-test-ipm-ring`, by hand as below, and
  `cargo test --test ipm` runs the pair twenty times, each
  round's two lines shown with `-- --show-output`.
- **Built to survive its holders**: the claims line carries each
  endpoint's checkpoint, so a claim on a released role resumes
  where it stopped, and `take_over_producer(id)` or
  `take_over_consumer(id)` replaces a holder the app vouches is
  dead, losing at most the one slot it reserved and never
  committed.

### Two processes by hand

`zcr-test-ipm` is installed with the crate (`cargo install --path .`), or run it from the
workspace with `cargo run --release --bin zcr-test-ipm -- <role>`. Linux only.

In one terminal, start the consumer. It builds the ring, prints `ready`, and waits ten seconds for
one message:

```sh
$ zcr-test-ipm consumer
ready
```

In a second terminal, within those ten seconds, start the producer. It sends a random value with
its checksum and exits:

```sh
$ zcr-test-ipm producer
sent value=0x53074c4fabcd8c41 checksum=0x46acb927225f8159
```

The consumer prints the same value, its checksum verified, and exits:

```sh
received value=0x53074c4fabcd8c41 checksum=0x46acb927225f8159 ok
```

- Both exit 0 on success. The consumer exits 1 on a bad checksum or when no message arrives in
  time, and the producer exits 1 when the region file does not exist.
- Start the consumer first: it recreates the region file on every run. A producer started alone
  finds the last run's file, claims its released role there, sends into a ring no consumer
  reads, and exits 0, which proves nothing.
- The value differs every run. The one shown is from the first run, 2026-09-26.

## MPSC v3: attachable ring with counted roles

`mpsc::v3` is the MPSC ring of segments processes join and leave: v2's protocol over a ring that
describes itself in the region. The details are the design note's [MPSC
v3](notes/ring-buffer-design.md#mpsc-v3-attachable-segments-with-counted-roles), and the how-to the
guide's [MPSC v3](notes/user-guide.md#mpsc-v3-joining-counted-roles-and-waiting).

- Counted roles: one consumer and producers up to a most, each claimed and released by one CAS on
  a roles word, with no holder ids. The consumer's release saves where it stopped for the next.
- A restart, not a takeover: a holder that dies leaves its role held, and the set the ring
  belongs to restarts. `release_ring` gives a ring no role holds back to its pool.
- Two modes chosen at compile time: `Single`, one segment and the fastest MPSC ring with slack,
  and `Multi`, v2's switching.
- Waiting: `send_spin_sleep` and `reserve_slot_wait` sleep on a full or empty ring through a
  `Wake` type, a futex on Linux, where `send_spin` and `reserve_slot_with` spin or give up.
- A complete program: [examples/guide_mpsc_v3.rs](examples/guide_mpsc_v3.rs) goes from a pool to
  the ring's release in two threads, a producer and a consumer that sleep on a full or empty ring,
  run with `cargo run --release --example guide_mpsc_v3`.
- Proven between processes: `zcr-test-ipm`'s MPSC mode, run by `cargo test --test ipm`, sends
  from two producer processes to a consumer that hands off to a second consumer process
  mid-stream, then releases the ring. By hand, each step after the one before it, the producers
  in terminals of their own:
  1. `zcr-test-ipm mpsc-consumer new 20000` makes the ring, prints `ready`, and reads 20000
     messages.
  2. `zcr-test-ipm mpsc-producer 0 20000` and `zcr-test-ipm mpsc-producer 1 20000`, started
     together once it is ready, send 20000 each and wait on the full ring when the first consumer
     has gone.
  3. `zcr-test-ipm mpsc-consumer join 20000`, once the first has exited, reads the rest, each
     producer's stream continuing where the first consumer stopped.
  4. `zcr-test-ipm mpsc-release` releases the ring once every role is given back.
- The same between processes for `mpsc::v4`, by the subcommands `mpsc4-consumer`, `mpsc4-producer`,
  and `mpsc4-release`, with `zcr-test-ipm mpsc4-attach-wrong-wait`, which attaches over the other
  wake protocol and passes only when it is refused.

## MPSC v4: v3 with matching sends and receives

`mpsc::v4` is `mpsc::v3` with an API made to be plain for someone new to it. v3 stays as built, to
measure against. The module docs open with a list of words and the steps of use, and the how-to is
the guide's [MPSC v4](notes/user-guide.md#mpsc-v4-matching-sends-and-receives-and-a-choice-of-waits).

- The consumer receives as a producer sends: `recv(policy, |msg| ...)` reads the slot in place by a
  closure and frees it when the closure returns, beside `send(policy, |msg| ...)`. There is no
  handle to release.
- The timed forms match by name and by parameter: `send_spin` and `recv_spin` spin for a time, and
  `send_spin_sleep` and `recv_spin_sleep` spin for a time and then sleep for a time or forever. One
  `WaitPolicy` serves both sides.
- A ring is made with two choices, its mode and how its endpoints wait, and reads as one of six:

  ```text
  MpscRing<Multi, SpinOnly>               MpscRing<Single, SpinOnly>
  MpscRing<Multi, Sleep<Futex>>           MpscRing<Single, Sleep<Futex>>
  MpscRing<Multi, SpinOrSleep<Futex>>     MpscRing<Single, SpinOrSleep<Futex>>
  ```

  - `SpinOnly` offers the spin forms, `Sleep<Futex>` the spin and sleep forms, and
    `SpinOrSleep<Futex>` both, for a ring where some endpoints only spin while others sleep.
  - A ring that sleeps costs every endpoint a check for sleepers on each message path. The module
    docs say what the checks are and what they have cost where measured.
- A role is taken with `ring.producer()` or `ring.consumer()` and given back with `release`.
- An `attach` over another wake protocol is `Error::BadWake`, so two processes cannot wake one ring
  two ways.
- A complete program: [examples/guide_mpsc_v4.rs](examples/guide_mpsc_v4.rs) is the typical
  zero-copy use in two threads. Each message is written into a buffer of a message pool, the ring
  carries the buffer's id, and the consumer reads the message where the producer wrote it. Run it
  with `cargo run --release --example guide_mpsc_v4`.
- Measured against v3 in the design note's [MPSC v4
  measured](notes/ring-buffer-design.md#mpsc-v4-measured): v4 runs as v3 does.

## Workspace and tools

The repo is a Cargo workspace: the ring crate at the root and
three local measurement crates beside it, dev tooling only,
never dependencies of the library.

| crate | what it is | `no_std` | docs |
|---|---|---|---|
| `zc-ring-x1` library | the SPSC and MPSC rings, the pool, descriptors | yes, std only under `cfg(test)` | this README, [notes/ring-buffer-design.md](notes/ring-buffer-design.md) |
| `zc-ring-x1-demo` binary | eyeball throughput lines and the depth sweep | no | [Testing](#testing) below |
| `tprobe` | hardware tick-counter probes and band reports | no | [tprobe/README.md](tprobe/README.md) |
| `tp_runner` | shared CLI flags, pinning, drive loop, perf counters, topology | no | [tp_runner/README.md](tp_runner/README.md) |
| `tp_matrix` | the measurement cells and four binaries | no | [tp_matrix/README.md](tp_matrix/README.md) |

Installing the root crate installs only the demo, so the tools
take a second command:

```sh
cargo install --path . --locked           # zc-ring-x1-demo
cargo install --path tp_matrix --locked   # tp-cell, tp-matrix, tp-stream, tp-pool
```

Which tool answers which question:

- `zc-ring-x1-demo`: a smoke run, single-shot ns per message
  for every flavor at each placement, and a depth sweep.
- `tp-matrix`: the round-trip cost per protocol phase and the
  x-core cache-line fills per trip, every flavor at every
  placement ([tp-matrix](tp_matrix/README.md#tp-matrix-the-whole-picture-one-command)).
- `tp-stream`: what a ring costs per message when the
  producer runs ahead and the ring holds many
  ([tp-stream](tp_matrix/README.md#tp-stream-the-streaming-matrix)).
- `tp-pool`: the messaging layer's loop, pool to queue to pool,
  over the descriptor rings and cordyceps's intrusive MPSC on
  one pool ([tp-pool](tp_matrix/README.md#tp-pool-the-pool-message-sweep)).
- `tp-cell`: one cell's full percentile bands, for a row that
  looks odd
  ([tp-cell](tp_matrix/README.md#tp-cell-one-cell-under-the-microscope)).

Where the numbers live: each ring version's measured tables
are in its section of
[notes/ring-buffer-design.md](notes/ring-buffer-design.md),
the pool sweep in
[Measured: pool-message sweep](notes/ring-buffer-design.md#measured-pool-message-sweep),
and calibrated benches with percentile tails in
[iiac-perf](https://github.com/winksaville/iiac-perf), below.

Dependencies and their `no_std` status. Only zerocopy reaches
a library build, and the library builds for a bare-metal
target such as `thumbv7em-none-eabi`:

| dependency | `no_std` | used by |
|---|---|---|
| zerocopy | yes, default features off | the library, `tp_matrix` |
| libc | yes | the demo's pinning and `tp_runner`, Linux only |
| cordyceps | yes, default features | the root crate's tests and `tp_matrix` |
| clap | no | `tp_runner`, `tp_matrix` |
| hdrhistogram | no | `tprobe` |
| perf-event2 | no | `tp_runner`, Linux only |

The cordyceps pieces, its contract tests in
[tests/cordyceps_mpsc.rs](tests/cordyceps_mpsc.rs) and the
pool-buffer node in `tp_matrix`, are prior art under study
([Prior art: cordyceps MpscQueue](notes/ring-buffer-design.md#prior-art-cordyceps-mpscqueue)).

## Performance runs using benches in iiac-perf each 5min (300s) duration

A 2026-07-06 run at iiac-perf 0.19.0, kept as that version's record.
iiac-perf has since renamed these benches to
`zcr-<flavor>-<version>-<threads>`, `zcr-spsc-v2-2t` for one,
and `iiac-perf zcr` still runs every one of them.

```
$ iiac-perf -d 300 zcr
iiac-perf 0.19.0 — Rust latency microbenchmark harness

Calibration:
  framing/sample      11.12 ns  (timer pair, two-point fit)
  loop/iter            0.49 ns  (per inner-loop iteration)
  cal pin           core 0 (unpinned after cal; --no-pin-cal to skip)
  bench pin         none (unpinned)
  sleep inhibit     active (systemd-inhibit --what=sleep)
  config            none (built-in defaults)

zcr-with-1t: zc-ring-x1 reserve_slot_with round-trip (1 thread) [duration=300.0s outer=1,760,431,907 inner=43 calls=75,698,572,001 adj/call=0.75ns labels=both]:
                            first           last          range          count           mean       adjusted
  z4  0.000_1              2.6 ns         2.8 ns         0.2 ns         65,864         2.8 ns         2.0 ns
  p20 0.20                 2.8 ns         2.8 ns         0.0 ns    534,949,247         2.8 ns         2.0 ns
  p40 0.40                 3.0 ns         3.0 ns         0.0 ns    237,033,440         3.0 ns         2.3 ns
  p70 0.70                 3.0 ns         3.0 ns         0.0 ns    726,450,495         3.0 ns         2.3 ns
  p90 0.90                 3.2 ns         3.2 ns         0.0 ns     28,846,450         3.2 ns         2.5 ns
  n2  0.99                 3.3 ns         3.7 ns         0.5 ns    216,021,259         3.4 ns         2.6 ns
  n3  0.999                4.0 ns         4.7 ns         0.7 ns     15,638,515         4.1 ns         3.3 ns
  n4  0.999_9              4.9 ns        92.3 ns        87.4 ns      1,249,240         8.3 ns         7.6 ns
  n5  0.999_99            92.5 ns       537.6 ns       445.1 ns        159,793       118.0 ns       117.2 ns
  n6  0.999_999          538.1 ns     2,012.2 ns     1,474.0 ns         15,853     1,061.7 ns     1,061.0 ns
  n7  0.999_999_9      2,013.2 ns     5,148.7 ns     3,135.5 ns          1,575     2,751.1 ns     2,750.3 ns
  n8  0.999_999_99     5,161.0 ns    13,983.7 ns     8,822.8 ns            158     6,629.1 ns     6,628.4 ns
  n9  0.999_999_999   14,098.4 ns    59,277.3 ns    45,178.9 ns             16    30,527.0 ns    30,526.2 ns
  n10 0.999_999_999_9 70,123.5 ns    74,580.0 ns     4,456.4 ns              2    72,351.7 ns    72,351.0 ns
  mean                                                                                 3.0 ns         2.3 ns
  stdev                                                                                6.6 ns
  mean z4..n2                                                                          3.0 ns         2.2 ns
  stdev z4..n2                                                                         0.2 ns

zcr-with-2t: zc-ring-x1 reserve_slot_with round-trip (2 threads, spin) [duration=300.0s outer=1,910,130,649 inner=1 calls=1,910,130,649 adj/call=11.61ns labels=both]:
                               first              last             range          count              mean          adjusted
  z4  0.000_1                20.0 ns           70.0 ns           50.0 ns        187,667           58.7 ns           47.1 ns
  z3  0.001                  79.0 ns           99.0 ns           20.0 ns      1,257,774           92.1 ns           80.5 ns
  p20 0.20                  100.0 ns          100.0 ns            0.0 ns    574,765,282          100.0 ns           88.4 ns
  p40 0.40                  109.1 ns          109.1 ns            0.0 ns    310,586,447          109.1 ns           97.4 ns
  p70 0.70                  110.0 ns          110.0 ns            0.0 ns    852,303,986          110.0 ns           98.4 ns
  n2  0.99                  119.0 ns          150.0 ns           31.0 ns    154,947,059          126.5 ns          114.9 ns
  n3  0.999                 160.1 ns          561.2 ns          401.0 ns     14,196,912          265.2 ns          253.6 ns
  n4  0.999_9               570.4 ns        4,268.0 ns        3,697.7 ns      1,695,147          853.8 ns          842.1 ns
  n5  0.999_99            4,280.3 ns       35,913.7 ns       31,633.4 ns        171,301        8,803.7 ns        8,792.1 ns
  n6  0.999_999          35,946.5 ns      164,102.1 ns      128,155.6 ns         17,161       80,297.6 ns       80,286.0 ns
  n7  0.999_999_9       164,233.2 ns      237,502.5 ns       73,269.2 ns          1,722      172,087.5 ns      172,075.9 ns
  n8  0.999_999_99      237,633.5 ns    1,016,594.4 ns      778,960.9 ns            172      371,434.4 ns      371,422.8 ns
  n9  0.999_999_999   1,018,167.3 ns    2,144,337.9 ns    1,126,170.6 ns             17    1,650,335.3 ns    1,650,323.7 ns
  n10 0.999_999_999_9 3,009,413.1 ns    3,011,510.3 ns        2,097.2 ns              2    3,010,461.7 ns    3,010,450.1 ns
  mean                                                                                           111.7 ns          100.1 ns
  stdev                                                                                          397.2 ns
  mean z4..n2                                                                                    108.2 ns           96.5 ns
  stdev z4..n2                                                                                     7.5 ns

zcr-mpsc-1t: zc-ring-x1 mpsc send_with round-trip (1 thread) [duration=300.0s outer=1,774,600,787 inner=24 calls=42,590,418,888 adj/call=0.95ns labels=both]:
                            first           last          range          count           mean       adjusted
  z4  0.000_1              4.6 ns         5.0 ns         0.4 ns             50         4.6 ns         3.6 ns
  p20 0.20                 5.0 ns         5.0 ns         0.0 ns    451,394,968         5.0 ns         4.1 ns
  p40 0.40                 5.4 ns         5.4 ns         0.0 ns    302,144,038         5.4 ns         4.4 ns
  p70 0.70                 5.4 ns         5.4 ns         0.0 ns    958,036,759         5.4 ns         4.5 ns
  n2  0.99                 5.8 ns         6.3 ns         0.5 ns     38,053,444         5.9 ns         4.9 ns
  n3  0.999                6.7 ns         7.1 ns         0.4 ns     23,919,203         6.7 ns         5.7 ns
  n4  0.999_9              7.5 ns       165.0 ns       157.5 ns        872,748        20.1 ns        19.1 ns
  n5  0.999_99           165.4 ns     1,335.3 ns     1,169.9 ns        161,827       293.2 ns       292.2 ns
  n6  0.999_999        1,336.3 ns     3,627.0 ns     2,290.7 ns         15,987     2,601.3 ns     2,600.3 ns
  n7  0.999_999_9      3,629.1 ns     4,157.4 ns       528.4 ns          1,586     3,723.2 ns     3,722.2 ns
  n8  0.999_999_99     4,159.5 ns     9,560.1 ns     5,400.6 ns            160     7,422.5 ns     7,421.6 ns
  n9  0.999_999_999    9,568.3 ns    24,018.9 ns    14,450.7 ns             15    13,249.7 ns    13,248.8 ns
  n10 0.999_999_999_9 47,874.0 ns    55,312.4 ns     7,438.3 ns              2    51,593.2 ns    51,592.3 ns
  mean                                                                                 5.4 ns         4.4 ns
  stdev                                                                               10.5 ns
  mean z4..n2                                                                          5.3 ns         4.4 ns
  stdev z4..n2                                                                         0.2 ns

zcr-mpsc-2t: zc-ring-x1 mpsc send_with round-trip (2 threads, spin) [duration=300.0s outer=2,342,074,592 inner=1 calls=2,342,074,592 adj/call=11.61ns labels=both]:
                               first              last             range          count              mean          adjusted
  z4  0.000_1                20.0 ns           29.0 ns            9.0 ns              2           24.5 ns           12.9 ns
  z2  0.01                   30.0 ns           30.0 ns            0.0 ns     45,038,279           30.0 ns           18.4 ns
  p10 0.10                   39.0 ns           50.0 ns           11.0 ns    219,727,369           47.5 ns           35.8 ns
  p20 0.20                   59.0 ns           69.1 ns           10.0 ns    170,805,129           61.9 ns           50.3 ns
  p30 0.30                   70.0 ns           79.0 ns            9.0 ns    315,501,059           73.3 ns           61.7 ns
  p50 0.50                   80.1 ns           80.1 ns            0.0 ns    760,539,235           80.1 ns           68.5 ns
  p70 0.70                   89.0 ns           89.0 ns            0.0 ns     88,636,053           89.0 ns           77.4 ns
  p80 0.80                   90.0 ns           90.0 ns            0.0 ns    319,258,230           90.0 ns           78.4 ns
  p90 0.90                   99.0 ns          110.0 ns           11.0 ns    191,091,122          101.5 ns           89.9 ns
  n2  0.99                  119.0 ns          200.1 ns           81.0 ns    209,377,431          133.1 ns          121.4 ns
  n3  0.999                 210.0 ns          420.1 ns          210.0 ns     19,984,750          265.4 ns          253.8 ns
  n4  0.999_9               430.1 ns        3,907.6 ns        3,477.5 ns      1,881,611          620.1 ns          608.5 ns
  n5  0.999_99            3,917.8 ns       34,668.5 ns       30,750.7 ns        210,881        7,557.6 ns        7,546.0 ns
  n6  0.999_999          34,701.3 ns      162,136.1 ns      127,434.8 ns         21,106       69,227.4 ns       69,215.8 ns
  n7  0.999_999_9       162,267.1 ns      190,578.7 ns       28,311.6 ns          2,101      169,034.3 ns      169,022.7 ns
  n8  0.999_999_99      190,840.8 ns      511,442.9 ns      320,602.1 ns            211      269,658.6 ns      269,647.0 ns
  n9  0.999_999_999     515,637.2 ns    2,134,900.7 ns    1,619,263.5 ns             21    1,149,726.1 ns    1,149,714.5 ns
  n10 0.999_999_999_9 2,422,210.6 ns    3,999,268.9 ns    1,577,058.3 ns              2    3,210,739.7 ns    3,210,728.1 ns
  mean                                                                                            85.5 ns           73.9 ns
  stdev                                                                                          343.9 ns
  mean z4..n2                                                                                     82.0 ns           70.4 ns
  stdev z4..n2                                                                                    22.9 ns
```

## Testing

- `cargo test`: the full suite, including a threaded stress
  test and a u32-index-wrap test.
- `cargo test --workspace`: the member crates too, and the
  cordyceps contract tests in
  [tests/cordyceps_mpsc.rs](tests/cordyceps_mpsc.rs).
- `cargo run --example occupancy_probe --release`: who waits
  for whom in the demo's streams, the probe the design note's
  flow-control reading rests on
  ([examples/occupancy_probe.rs](examples/occupancy_probe.rs)).
- `cargo run --example readme`: the Overview example above
  (committed as [examples/readme.rs](examples/readme.rs) so
  `cargo clippy --all-targets` keeps it compiling against the
  real API).
- `cargo run --example pool_readme`: the Message pool
  example above
  ([examples/pool_readme.rs](examples/pool_readme.rs), same
  convention).
- `cargo run --release --example guide_spsc_v3` and
  `guide_mpsc_v2`: the [user guide](notes/user-guide.md)'s
  two programs, one per segmented ring, a pool to two
  threads and the counters at the end, same convention.
- `cargo run --release`: the demo binary
  ([src/bin/zc-ring-x1-demo.rs](src/bin/zc-ring-x1-demo.rs)):
  a throughput scoreboard (msgs/sec and ns/msg) grouped so
  like compares with like: alloc/free baselines (pool vs
  global allocator), then three message flows (raw ring,
  composed ring + pool descriptors, std channel + pool) at
  each thread placement: single thread on the base cpu, then
  two threads at each placement the machine has, in the
  measurement tools' terms: CCX, two cores on one L3, x-CCX,
  cores on different L3s, SMT, one core's two cpus sharing
  its L1 and L2, and unpinned (pairs
  discovered from /sys at runtime, each line naming its
  cpus, and a placement the machine lacks is absent, the
  7600X having no x-CCX). Eyeball numbers (single runs, no
  mean/stdev), not a benchmark (calibrated measurement lives
  in iiac-perf). Installable: `cargo install --path .
  --locked`, then `zc-ring-x1-demo`, and `-V` prints the
  version-of-record so you know which build you are
  testing. `--base-cpu <n>` sets the base, the cpu the
  single-thread lines pin to and every pair starts from. The
  default is the last core's primary cpu, cpu 11
  on the 3900X and 5 on the 7600X, the quiet end of the
  kernel's fill order, and the partners are chosen the same
  way ([Measurement placements](notes/ring-buffer-design.md#measurement-placements-the-base-cpu-and-its-partners)).
  `-h` prints the usage. An example run on
  each machine, the 3900X (Zen 2, 12 cores over four CCXs)
  first, then the 7600X (Zen 4, six cores under one L3),
  both of 2026-10-07 from the build a cycle runs under,
  whose names carry `-dev`:

  ```text
  $ zc-ring-x1-demo-dev
  zc-ring-x1-dev 0.19.3
  demo: 1,000,000 messages each, depth 64, base cpu 11
  pool_alloc_free_1t (core 11):                   101,125,271 msgs/sec      9.9 ns/msg
  pool1_alloc_free_1t 1 stack (core 11):          109,571,113 msgs/sec      9.1 ns/msg
  pool1_alloc_free_1t 4 stacks, 1st (core 11):     92,512,980 msgs/sec     10.8 ns/msg
  pool1_alloc_free_1t 4 stacks, 4th (core 11):     93,724,831 msgs/sec     10.7 ns/msg
  global_alloc_free_1t (core 11):                 135,825,292 msgs/sec      7.4 ns/msg

  spsc_ring_one_msg_1t (core 11):                 388,099,322 msgs/sec      2.6 ns/msg
  spsc1_ring_one_msg_1t (core 11):                136,466,513 msgs/sec      7.3 ns/msg
  spsc2_ring_one_msg_1t (core 11):                143,529,537 msgs/sec      7.0 ns/msg
  spsc3_ring_one_msg_1t (core 11):                 48,696,890 msgs/sec     20.5 ns/msg
  spsc4_ring_one_msg_1t (core 11):                 84,998,604 msgs/sec     11.8 ns/msg
  mpsc0_ring_one_msg_1t (core 11):                 91,709,279 msgs/sec     10.9 ns/msg
  mpsc1_ring_one_msg_1t (core 11):                 94,091,150 msgs/sec     10.6 ns/msg
  mpsc2_ring_one_msg_1t (core 11):                 72,293,861 msgs/sec     13.8 ns/msg
  mpsc3_ring_one_msg_1t (core 11):                 72,110,333 msgs/sec     13.9 ns/msg
  mpsc3s_ring_one_msg_1t (core 11):                93,788,236 msgs/sec     10.7 ns/msg
  mpsc4_ring_one_msg_1t (core 11):                 87,858,955 msgs/sec     11.4 ns/msg
  mpsc4s_ring_one_msg_1t (core 11):                87,386,577 msgs/sec     11.4 ns/msg
  spsc_ring_one_pool_msg_1t (core 11):             94,018,372 msgs/sec     10.6 ns/msg
  std_mpsc_one_pool_msg_1t (core 11):              21,832,080 msgs/sec     45.8 ns/msg

  spsc_ring_one_msg_2t (11,10 CCX):                98,925,295 msgs/sec     10.1 ns/msg
  spsc1_ring_one_msg_2t (11,10 CCX):               30,309,355 msgs/sec     33.0 ns/msg
  spsc2_ring_one_msg_2t (11,10 CCX):              199,294,259 msgs/sec      5.0 ns/msg
  spsc3_ring_one_msg_2t (11,10 CCX):               48,741,074 msgs/sec     20.5 ns/msg
  spsc4_ring_one_msg_2t (11,10 CCX):               98,712,307 msgs/sec     10.1 ns/msg
  mpsc0_ring_one_msg_2t (11,10 CCX):               42,958,177 msgs/sec     23.3 ns/msg
  mpsc1_ring_one_msg_2t (11,10 CCX):               42,971,028 msgs/sec     23.3 ns/msg
  mpsc2_ring_one_msg_2t (11,10 CCX):               62,976,269 msgs/sec     15.9 ns/msg
  mpsc3_ring_one_msg_2t (11,10 CCX):               63,040,187 msgs/sec     15.9 ns/msg
  mpsc3s_ring_one_msg_2t (11,10 CCX):              61,339,559 msgs/sec     16.3 ns/msg
  mpsc4_ring_one_msg_2t (11,10 CCX):               77,165,846 msgs/sec     13.0 ns/msg
  mpsc4s_ring_one_msg_2t (11,10 CCX):              55,840,441 msgs/sec     17.9 ns/msg
  spsc_ring_one_pool_msg_2t (11,10 CCX):           14,369,705 msgs/sec     69.6 ns/msg
  std_mpsc_one_pool_msg_2t (11,10 CCX):            10,631,971 msgs/sec     94.1 ns/msg

  spsc_ring_one_msg_2t (11,8 x-CCX):                4,713,894 msgs/sec    212.1 ns/msg
  spsc1_ring_one_msg_2t (11,8 x-CCX):               9,136,563 msgs/sec    109.5 ns/msg
  spsc2_ring_one_msg_2t (11,8 x-CCX):              79,957,194 msgs/sec     12.5 ns/msg
  spsc3_ring_one_msg_2t (11,8 x-CCX):              40,070,031 msgs/sec     25.0 ns/msg
  spsc4_ring_one_msg_2t (11,8 x-CCX):              47,237,246 msgs/sec     21.2 ns/msg
  mpsc0_ring_one_msg_2t (11,8 x-CCX):              13,122,241 msgs/sec     76.2 ns/msg
  mpsc1_ring_one_msg_2t (11,8 x-CCX):              13,866,192 msgs/sec     72.1 ns/msg
  mpsc2_ring_one_msg_2t (11,8 x-CCX):              52,732,545 msgs/sec     19.0 ns/msg
  mpsc3_ring_one_msg_2t (11,8 x-CCX):              63,368,617 msgs/sec     15.8 ns/msg
  mpsc3s_ring_one_msg_2t (11,8 x-CCX):             67,588,250 msgs/sec     14.8 ns/msg
  mpsc4_ring_one_msg_2t (11,8 x-CCX):              61,789,268 msgs/sec     16.2 ns/msg
  mpsc4s_ring_one_msg_2t (11,8 x-CCX):             65,653,109 msgs/sec     15.2 ns/msg
  spsc_ring_one_pool_msg_2t (11,8 x-CCX):           4,342,190 msgs/sec    230.3 ns/msg
  std_mpsc_one_pool_msg_2t (11,8 x-CCX):            3,110,212 msgs/sec    321.5 ns/msg

  spsc_ring_one_msg_2t (11,23 SMT):               133,723,699 msgs/sec      7.5 ns/msg
  spsc1_ring_one_msg_2t (11,23 SMT):               76,695,821 msgs/sec     13.0 ns/msg
  spsc2_ring_one_msg_2t (11,23 SMT):              129,709,912 msgs/sec      7.7 ns/msg
  spsc3_ring_one_msg_2t (11,23 SMT):               49,146,862 msgs/sec     20.3 ns/msg
  spsc4_ring_one_msg_2t (11,23 SMT):               71,753,682 msgs/sec     13.9 ns/msg
  mpsc0_ring_one_msg_2t (11,23 SMT):               61,310,435 msgs/sec     16.3 ns/msg
  mpsc1_ring_one_msg_2t (11,23 SMT):               62,320,870 msgs/sec     16.0 ns/msg
  mpsc2_ring_one_msg_2t (11,23 SMT):               68,066,962 msgs/sec     14.7 ns/msg
  mpsc3_ring_one_msg_2t (11,23 SMT):               78,326,807 msgs/sec     12.8 ns/msg
  mpsc3s_ring_one_msg_2t (11,23 SMT):              88,870,435 msgs/sec     11.3 ns/msg
  mpsc4_ring_one_msg_2t (11,23 SMT):               83,270,276 msgs/sec     12.0 ns/msg
  mpsc4s_ring_one_msg_2t (11,23 SMT):              90,891,862 msgs/sec     11.0 ns/msg
  spsc_ring_one_pool_msg_2t (11,23 SMT):           28,976,039 msgs/sec     34.5 ns/msg
  std_mpsc_one_pool_msg_2t (11,23 SMT):            27,836,522 msgs/sec     35.9 ns/msg

  spsc_ring_one_msg_2t (unpinned):                  4,766,670 msgs/sec    209.8 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                 9,424,750 msgs/sec    106.1 ns/msg
  spsc2_ring_one_msg_2t (unpinned):                77,165,066 msgs/sec     13.0 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                36,324,684 msgs/sec     27.5 ns/msg
  spsc4_ring_one_msg_2t (unpinned):                88,771,718 msgs/sec     11.3 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                37,190,226 msgs/sec     26.9 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                36,611,685 msgs/sec     27.3 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):                59,368,183 msgs/sec     16.8 ns/msg
  mpsc3_ring_one_msg_2t (unpinned):                60,159,306 msgs/sec     16.6 ns/msg
  mpsc3s_ring_one_msg_2t (unpinned):               60,746,399 msgs/sec     16.5 ns/msg
  mpsc4_ring_one_msg_2t (unpinned):                68,737,139 msgs/sec     14.5 ns/msg
  mpsc4s_ring_one_msg_2t (unpinned):               61,105,873 msgs/sec     16.4 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            12,349,642 msgs/sec     81.0 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):             10,284,179 msgs/sec     97.2 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):          15,979,841 msgs/sec     62.6 ns/msg
  mpsc2_ring_one_msg_3t (2p+1c unpinned):          20,103,552 msgs/sec     49.7 ns/msg
  mpsc3_ring_one_msg_3t (2p+1c unpinned):          18,218,490 msgs/sec     54.9 ns/msg
  mpsc3s_ring_one_msg_3t (2p+1c unpinned):         19,705,477 msgs/sec     50.7 ns/msg
  mpsc4_ring_one_msg_3t (2p+1c unpinned):          17,976,488 msgs/sec     55.6 ns/msg
  mpsc4s_ring_one_msg_3t (2p+1c unpinned):         15,941,744 msgs/sec     62.7 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 with 1 segment(s)

  | 1t core 11             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.6 |     2.5 |     2.5 |     2.5 |
  | spsc-v1                |     7.3 |     7.3 |     7.3 |     7.3 |
  | spsc-v2                |     7.0 |     6.9 |     7.0 |     6.9 |
  | spsc-v3                |    24.4 |    20.5 |    20.6 |    20.6 |
  | spsc-v4                |    15.8 |    11.7 |    11.9 |    11.7 |
  | mpsc-v0                |       - |    10.9 |    10.9 |    10.9 |
  | mpsc-v1                |    10.7 |    10.6 |    10.7 |    10.7 |
  | mpsc-v2                |    13.9 |    13.9 |    13.8 |    14.1 |
  | mpsc-v3                |    13.9 |    13.8 |    13.9 |    13.9 |
  | mpsc-v3-single         |    10.6 |    10.7 |    10.7 |    10.6 |
  | mpsc-v4                |    11.4 |    11.4 |    11.5 |    11.4 |
  | mpsc-v4-single         |    11.3 |    11.4 |    11.4 |    11.4 |

  | 2t 11,10 CCX           |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    86.5 |    66.1 |    23.5 |     9.9 |
  | spsc-v1                |    83.5 |    60.6 |    37.8 |    32.3 |
  | spsc-v2                |    68.2 |    35.1 |     8.8 |     5.2 |
  | spsc-v3                |    71.4 |    46.6 |    18.7 |    20.4 |
  | spsc-v4                |    74.5 |    46.0 |    13.4 |    13.3 |
  | mpsc-v0                |       - |    67.3 |    34.7 |    25.4 |
  | mpsc-v1                |    96.4 |    67.1 |    35.2 |    25.4 |
  | mpsc-v2                |    74.3 |    36.9 |    14.6 |    15.3 |
  | mpsc-v3                |    72.5 |    37.1 |    10.7 |    12.6 |
  | mpsc-v3-single         |    71.9 |    36.7 |    10.9 |     7.9 |
  | mpsc-v4                |    72.5 |    40.2 |    12.9 |    14.4 |
  | mpsc-v4-single         |    74.1 |    37.0 |    11.1 |    15.8 |

  | 2t 11,8 x-CCX          |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |   355.9 |   227.6 |   162.5 |   212.1 |
  | spsc-v1                |   437.7 |   212.0 |   122.6 |   106.1 |
  | spsc-v2                |   202.2 |   106.6 |    30.5 |    11.8 |
  | spsc-v3                |   204.5 |   156.8 |    51.6 |    28.7 |
  | spsc-v4                |   202.2 |   168.1 |    45.3 |    17.1 |
  | mpsc-v0                |       - |   214.6 |   110.7 |    76.5 |
  | mpsc-v1                |   472.2 |   218.3 |   108.3 |    75.6 |
  | mpsc-v2                |   204.4 |   118.6 |    35.7 |    18.4 |
  | mpsc-v3                |   202.0 |   116.8 |    31.5 |    16.2 |
  | mpsc-v3-single         |   201.6 |   110.1 |    31.0 |    14.2 |
  | mpsc-v4                |   201.7 |   118.6 |    31.3 |    16.7 |
  | mpsc-v4-single         |   201.9 |   110.2 |    31.8 |    13.9 |

  | 2t 11,23 SMT           |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    30.8 |    15.1 |     7.5 |     7.0 |
  | spsc-v1                |    53.9 |    26.6 |    15.1 |    12.3 |
  | spsc-v2                |    40.3 |    21.6 |     7.2 |     7.3 |
  | spsc-v3                |    41.9 |    29.9 |    19.3 |    18.9 |
  | spsc-v4                |    40.4 |    23.0 |    13.3 |    13.3 |
  | mpsc-v0                |       - |    24.4 |    15.8 |    15.5 |
  | mpsc-v1                |    43.3 |    22.7 |    15.3 |    15.2 |
  | mpsc-v2                |    42.6 |    22.8 |    13.9 |    14.0 |
  | mpsc-v3                |    39.4 |    23.8 |    12.2 |    12.2 |
  | mpsc-v3-single         |    41.5 |    23.6 |    10.7 |    10.8 |
  | mpsc-v4                |    48.5 |    23.0 |    11.6 |    11.6 |
  | mpsc-v4-single         |    51.3 |    23.5 |    10.6 |    10.7 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |   125.1 |    71.1 |    24.6 |    11.5 |
  | spsc-v1                |    83.9 |    75.9 |    41.6 |    37.6 |
  | spsc-v2                |    72.9 |    42.3 |    13.5 |     7.3 |
  | spsc-v3                |    75.7 |    37.9 |    22.9 |    24.5 |
  | spsc-v4                |    73.7 |    41.2 |    17.0 |    14.2 |
  | mpsc-v0                |       - |    79.7 |    33.2 |    25.1 |
  | mpsc-v1                |    82.5 |    68.0 |    41.5 |    24.9 |
  | mpsc-v2                |    95.3 |    50.1 |    17.4 |    17.6 |
  | mpsc-v3                |    76.5 |    42.8 |    22.9 |    16.1 |
  | mpsc-v3-single         |    75.8 |    37.5 |    14.8 |    16.6 |
  | mpsc-v4                |    73.2 |    38.1 |    31.1 |    13.9 |
  | mpsc-v4-single         |    76.2 |    42.4 |    18.5 |    16.7 |

  segment stress: 1,000,000 messages per line, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 at 4 segments of 64 slots, then the switch cost at depth 1

  | line              | placement        |  shape |  ns/msg |  segs | switches | sw/msg | switch ns |
  |-------------------|------------------|-------:|--------:|------:|---------:|-------:|----------:|
  | spsc3 burst 1t    | core 11          |   4x64 |    22.8 |   4/4 |   11,719 |  0.012 |         - |
  | spsc4 burst 1t    | core 11          |   4x64 |    12.8 |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 burst 1t    | core 11          |   4x64 |    14.5 |   4/4 |   11,718 |  0.012 |         - |
  | mpsc3 burst 1t    | core 11          |   4x64 |    13.4 |   4/4 |   11,718 |  0.012 |         - |
  | mpsc4 burst 1t    | core 11          |   4x64 |    14.0 |   4/4 |   11,718 |  0.012 |         - |
  | spsc3 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   12,181 |  0.012 |         - |
  | spsc4 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   15,620 |  0.016 |         - |
  | mpsc3 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | mpsc4 lagging 2t  | 11,10 CCX        |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   11,721 |  0.012 |         - |
  | spsc4 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
  | mpsc2 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | mpsc3 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc4 lagging 2t  | 11,8 x-CCX       |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
  | spsc4 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc3 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | mpsc4 lagging 2t  | 11,23 SMT        |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   14,214 |  0.014 |         - |
  | spsc4 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   11,715 |  0.012 |         - |
  | mpsc2 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,621 |  0.016 |         - |
  | mpsc4 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 burst 1t    | core 11          |   1x32 |    23.0 |   1/1 |        0 |  0.000 |         - |
  | spsc3 burst 1t    | core 11          |   32x1 |    29.3 | 32/32 |  968,750 |  0.969 |       6.5 |
  | spsc4 burst 1t    | core 11          |   1x32 |    12.9 |   1/1 |        0 |  0.000 |         - |
  | spsc4 burst 1t    | core 11          |   32x1 |    27.7 | 32/32 |  968,750 |  0.969 |      15.2 |
  | mpsc2 burst 1t    | core 11          |   1x32 |    14.6 |   1/1 |        0 |  0.000 |         - |
  | mpsc2 burst 1t    | core 11          |   32x1 |    29.8 | 32/32 |  968,750 |  0.969 |      15.7 |
  | mpsc3 burst 1t    | core 11          |   1x32 |    13.4 |   1/1 |        0 |  0.000 |         - |
  | mpsc3 burst 1t    | core 11          |   32x1 |    28.2 | 32/32 |  968,750 |  0.969 |      15.2 |
  | mpsc4 burst 1t    | core 11          |   1x32 |    14.1 |   1/1 |        0 |  0.000 |         - |
  | mpsc4 burst 1t    | core 11          |   32x1 |    29.5 | 32/32 |  968,750 |  0.969 |      15.9 |
  | spsc3 stream 2t   | 11,8 x-CCX       |   1x32 |    40.9 |   1/1 |        0 |  0.000 |         - |
  | spsc3 stream 2t   | 11,8 x-CCX       |   32x1 |   192.5 | 32/32 |  999,793 |  1.000 |     151.7 |
  | spsc4 stream 2t   | 11,8 x-CCX       |   1x32 |    22.4 |   1/1 |        0 |  0.000 |         - |
  | spsc4 stream 2t   | 11,8 x-CCX       |   32x1 |   272.3 | 32/32 |  999,105 |  0.999 |     250.2 |
  | mpsc2 stream 2t   | 11,8 x-CCX       |   1x32 |    28.7 |   1/1 |        0 |  0.000 |         - |
  | mpsc2 stream 2t   | 11,8 x-CCX       |   32x1 |   372.5 | 32/32 |  998,889 |  0.999 |     344.2 |
  | mpsc3 stream 2t   | 11,8 x-CCX       |   1x32 |    27.7 |   1/1 |        0 |  0.000 |         - |
  | mpsc3 stream 2t   | 11,8 x-CCX       |   32x1 |   320.2 | 32/32 |  999,984 |  1.000 |     292.5 |
  | mpsc4 stream 2t   | 11,8 x-CCX       |   1x32 |    19.8 |   1/1 |        0 |  0.000 |         - |
  | mpsc4 stream 2t   | 11,8 x-CCX       |   32x1 |   380.6 | 32/32 |  999,466 |  0.999 |     361.0 |

  - line: the ring and the shape of the run. burst 1t: one thread fills every
    segment with the consumer idle, then drains, until the messages are moved.
    lagging 2t: the producer streams while the consumer reads two segments' worth
    between 20us pauses, so the producer runs ahead across segments at every
    pause. stream 2t: both spinning, the two_t loops' shape.
  - shape: segments x slots per segment. The first rows are the stress shape. The
    switch cost rows are the same 32 slots as one segment, which never switches,
    and as 32 segments of one slot, which switches on nearly every message.
  - ns/msg: elapsed over the messages moved, `-` where the line's pace is the
    consumer's pauses. segs: segments the producer wrote into, of the ring's.
    switches: segment switches, the producer's count, which the consumer's
    matched. sw/msg: switches per message, the burst's 3 per 256 at the stress
    shape.
  - switch ns: the cost of one switch, the gap in ns/msg between the two shapes
    over the gap in sw/msg, on the 32x1 row. Single-threaded it is the
    instructions alone. Streaming across cores it includes the cold segment
    crossing.
  ```

  ```text
  $ zc-ring-x1-demo-dev
  zc-ring-x1-dev 0.19.3
  demo: 1,000,000 messages each, depth 64, base cpu 5
  pool_alloc_free_1t (core 5):                    231,040,045 msgs/sec      4.3 ns/msg
  pool1_alloc_free_1t 1 stack (core 5):           181,169,693 msgs/sec      5.5 ns/msg
  pool1_alloc_free_1t 4 stacks, 1st (core 5):     176,676,509 msgs/sec      5.7 ns/msg
  pool1_alloc_free_1t 4 stacks, 4th (core 5):     147,589,668 msgs/sec      6.8 ns/msg
  global_alloc_free_1t (core 5):                  182,620,583 msgs/sec      5.5 ns/msg

  spsc_ring_one_msg_1t (core 5):                  520,569,794 msgs/sec      1.9 ns/msg
  spsc1_ring_one_msg_1t (core 5):                 180,287,768 msgs/sec      5.5 ns/msg
  spsc2_ring_one_msg_1t (core 5):                 178,896,029 msgs/sec      5.6 ns/msg
  spsc3_ring_one_msg_1t (core 5):                  73,388,274 msgs/sec     13.6 ns/msg
  spsc4_ring_one_msg_1t (core 5):                 115,308,361 msgs/sec      8.7 ns/msg
  mpsc0_ring_one_msg_1t (core 5):                 152,117,374 msgs/sec      6.6 ns/msg
  mpsc1_ring_one_msg_1t (core 5):                 139,340,390 msgs/sec      7.2 ns/msg
  mpsc2_ring_one_msg_1t (core 5):                 117,715,860 msgs/sec      8.5 ns/msg
  mpsc3_ring_one_msg_1t (core 5):                 124,633,406 msgs/sec      8.0 ns/msg
  mpsc3s_ring_one_msg_1t (core 5):                134,897,296 msgs/sec      7.4 ns/msg
  mpsc4_ring_one_msg_1t (core 5):                 111,324,309 msgs/sec      9.0 ns/msg
  mpsc4s_ring_one_msg_1t (core 5):                121,380,464 msgs/sec      8.2 ns/msg
  spsc_ring_one_pool_msg_1t (core 5):             117,245,706 msgs/sec      8.5 ns/msg
  std_mpsc_one_pool_msg_1t (core 5):               37,216,550 msgs/sec     26.9 ns/msg

  spsc_ring_one_msg_2t (5,4 CCX):                 129,018,024 msgs/sec      7.8 ns/msg
  spsc1_ring_one_msg_2t (5,4 CCX):                 62,625,948 msgs/sec     16.0 ns/msg
  spsc2_ring_one_msg_2t (5,4 CCX):                325,361,859 msgs/sec      3.1 ns/msg
  spsc3_ring_one_msg_2t (5,4 CCX):                 58,534,705 msgs/sec     17.1 ns/msg
  spsc4_ring_one_msg_2t (5,4 CCX):                119,315,126 msgs/sec      8.4 ns/msg
  mpsc0_ring_one_msg_2t (5,4 CCX):                 63,516,748 msgs/sec     15.7 ns/msg
  mpsc1_ring_one_msg_2t (5,4 CCX):                 64,629,058 msgs/sec     15.5 ns/msg
  mpsc2_ring_one_msg_2t (5,4 CCX):                141,259,198 msgs/sec      7.1 ns/msg
  mpsc3_ring_one_msg_2t (5,4 CCX):                160,606,656 msgs/sec      6.2 ns/msg
  mpsc3s_ring_one_msg_2t (5,4 CCX):               198,630,679 msgs/sec      5.0 ns/msg
  mpsc4_ring_one_msg_2t (5,4 CCX):                148,727,347 msgs/sec      6.7 ns/msg
  mpsc4s_ring_one_msg_2t (5,4 CCX):               180,251,956 msgs/sec      5.5 ns/msg
  spsc_ring_one_pool_msg_2t (5,4 CCX):             20,152,426 msgs/sec     49.6 ns/msg
  std_mpsc_one_pool_msg_2t (5,4 CCX):               9,430,245 msgs/sec    106.0 ns/msg

  spsc_ring_one_msg_2t (5,11 SMT):                171,639,167 msgs/sec      5.8 ns/msg
  spsc1_ring_one_msg_2t (5,11 SMT):                80,741,738 msgs/sec     12.4 ns/msg
  spsc2_ring_one_msg_2t (5,11 SMT):               237,241,789 msgs/sec      4.2 ns/msg
  spsc3_ring_one_msg_2t (5,11 SMT):                63,366,216 msgs/sec     15.8 ns/msg
  spsc4_ring_one_msg_2t (5,11 SMT):               111,310,777 msgs/sec      9.0 ns/msg
  mpsc0_ring_one_msg_2t (5,11 SMT):                82,718,055 msgs/sec     12.1 ns/msg
  mpsc1_ring_one_msg_2t (5,11 SMT):                84,714,389 msgs/sec     11.8 ns/msg
  mpsc2_ring_one_msg_2t (5,11 SMT):               108,923,693 msgs/sec      9.2 ns/msg
  mpsc3_ring_one_msg_2t (5,11 SMT):               126,409,177 msgs/sec      7.9 ns/msg
  mpsc3s_ring_one_msg_2t (5,11 SMT):              149,167,904 msgs/sec      6.7 ns/msg
  mpsc4_ring_one_msg_2t (5,11 SMT):               122,211,111 msgs/sec      8.2 ns/msg
  mpsc4s_ring_one_msg_2t (5,11 SMT):              146,678,516 msgs/sec      6.8 ns/msg
  spsc_ring_one_pool_msg_2t (5,11 SMT):            34,650,841 msgs/sec     28.9 ns/msg
  std_mpsc_one_pool_msg_2t (5,11 SMT):             17,411,530 msgs/sec     57.4 ns/msg

  spsc_ring_one_msg_2t (unpinned):                130,271,518 msgs/sec      7.7 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                58,193,695 msgs/sec     17.2 ns/msg
  spsc2_ring_one_msg_2t (unpinned):               285,914,998 msgs/sec      3.5 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                53,252,798 msgs/sec     18.8 ns/msg
  spsc4_ring_one_msg_2t (unpinned):               106,427,168 msgs/sec      9.4 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                60,295,305 msgs/sec     16.6 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                60,247,881 msgs/sec     16.6 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):               137,196,380 msgs/sec      7.3 ns/msg
  mpsc3_ring_one_msg_2t (unpinned):               158,076,974 msgs/sec      6.3 ns/msg
  mpsc3s_ring_one_msg_2t (unpinned):              175,619,854 msgs/sec      5.7 ns/msg
  mpsc4_ring_one_msg_2t (unpinned):               141,599,476 msgs/sec      7.1 ns/msg
  mpsc4s_ring_one_msg_2t (unpinned):              177,991,874 msgs/sec      5.6 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            18,864,920 msgs/sec     53.0 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):              9,637,176 msgs/sec    103.8 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):          25,048,231 msgs/sec     39.9 ns/msg
  mpsc2_ring_one_msg_3t (2p+1c unpinned):          23,395,776 msgs/sec     42.7 ns/msg
  mpsc3_ring_one_msg_3t (2p+1c unpinned):          23,394,103 msgs/sec     42.7 ns/msg
  mpsc3s_ring_one_msg_3t (2p+1c unpinned):         24,072,076 msgs/sec     41.5 ns/msg
  mpsc4_ring_one_msg_3t (2p+1c unpinned):          24,195,195 msgs/sec     41.3 ns/msg
  mpsc4s_ring_one_msg_3t (2p+1c unpinned):         23,681,230 msgs/sec     42.2 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 with 1 segment(s)

  | 1t core 5              |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.3 |     2.1 |     2.1 |     2.1 |
  | spsc-v1                |     5.4 |     5.7 |     5.6 |     5.7 |
  | spsc-v2                |     5.9 |     5.9 |     5.9 |     6.2 |
  | spsc-v3                |    15.6 |    13.7 |    13.6 |    13.8 |
  | spsc-v4                |    11.8 |     9.5 |     9.1 |     9.0 |
  | mpsc-v0                |       - |     6.6 |     6.7 |     6.7 |
  | mpsc-v1                |     7.2 |     7.2 |     7.3 |     7.3 |
  | mpsc-v2                |     8.5 |     8.4 |     8.5 |     8.4 |
  | mpsc-v3                |     7.9 |     7.9 |     7.9 |     7.9 |
  | mpsc-v3-single         |     7.1 |     7.2 |     7.3 |     7.3 |
  | mpsc-v4                |     8.9 |     8.9 |     8.9 |     8.9 |
  | mpsc-v4-single         |     8.3 |     8.2 |     8.3 |     8.3 |

  | 2t 5,4 CCX             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    67.2 |    51.7 |    14.5 |     6.7 |
  | spsc-v1                |    50.5 |    39.6 |    16.1 |    16.8 |
  | spsc-v2                |    40.6 |    23.4 |     6.3 |     3.7 |
  | spsc-v3                |    65.5 |    44.9 |    17.4 |    17.4 |
  | spsc-v4                |    48.4 |    37.9 |     8.8 |     8.3 |
  | mpsc-v0                |       - |    38.1 |    17.1 |    15.7 |
  | mpsc-v1                |    53.6 |    37.8 |    16.2 |    14.8 |
  | mpsc-v2                |    42.6 |    27.3 |     8.1 |     7.1 |
  | mpsc-v3                |    42.4 |    27.3 |     7.1 |     6.0 |
  | mpsc-v3-single         |    41.1 |    27.6 |     7.8 |     5.0 |
  | mpsc-v4                |    42.4 |    25.7 |     7.2 |     6.6 |
  | mpsc-v4-single         |    40.9 |    28.0 |     7.6 |     5.5 |

  | 2t 5,11 SMT            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    27.6 |    14.2 |     5.4 |     5.9 |
  | spsc-v1                |    42.9 |    24.4 |    13.3 |    12.3 |
  | spsc-v2                |    25.9 |    10.6 |     4.5 |     4.7 |
  | spsc-v3                |    56.0 |    26.4 |    16.0 |    16.1 |
  | spsc-v4                |    34.3 |    18.2 |     9.1 |     9.0 |
  | mpsc-v0                |       - |    22.8 |    12.7 |    12.0 |
  | mpsc-v1                |    40.0 |    22.5 |    12.6 |    11.8 |
  | mpsc-v2                |    32.7 |    17.3 |     9.0 |     9.3 |
  | mpsc-v3                |    32.6 |    16.4 |     7.8 |     7.8 |
  | mpsc-v3-single         |    31.0 |    10.6 |     6.9 |     6.6 |
  | mpsc-v4                |    27.3 |    15.8 |     8.2 |     8.3 |
  | mpsc-v4-single         |    31.0 |    11.6 |     6.7 |     7.0 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    54.2 |    40.6 |    13.7 |     7.1 |
  | spsc-v1                |    80.2 |    40.1 |    17.6 |    17.3 |
  | spsc-v2                |    40.6 |    26.0 |     7.8 |     3.8 |
  | spsc-v3                |    71.7 |    42.6 |    18.1 |    17.3 |
  | spsc-v4                |    46.5 |    36.4 |     9.2 |     8.2 |
  | mpsc-v0                |       - |    42.4 |    24.2 |    17.0 |
  | mpsc-v1                |    79.6 |    39.0 |    20.8 |    15.9 |
  | mpsc-v2                |    42.9 |    28.3 |     7.8 |     8.2 |
  | mpsc-v3                |    47.3 |    29.2 |     7.5 |     6.3 |
  | mpsc-v3-single         |    45.5 |    28.8 |     7.3 |     6.0 |
  | mpsc-v4                |    45.4 |    29.1 |     7.7 |     7.4 |
  | mpsc-v4-single         |    46.2 |    29.7 |     7.8 |     6.7 |

  segment stress: 1,000,000 messages per line, spsc-v3, spsc-v4, mpsc-v2, mpsc-v3, and mpsc-v4 at 4 segments of 64 slots, then the switch cost at depth 1

  | line              | placement        |  shape |  ns/msg |  segs | switches | sw/msg | switch ns |
  |-------------------|------------------|-------:|--------:|------:|---------:|-------:|----------:|
  | spsc3 burst 1t    | core 5           |   4x64 |    14.1 |   4/4 |   11,719 |  0.012 |         - |
  | spsc4 burst 1t    | core 5           |   4x64 |     9.1 |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 burst 1t    | core 5           |   4x64 |     8.9 |   4/4 |   11,718 |  0.012 |         - |
  | mpsc3 burst 1t    | core 5           |   4x64 |     8.1 |   4/4 |   11,718 |  0.012 |         - |
  | mpsc4 burst 1t    | core 5           |   4x64 |     8.4 |   4/4 |   11,718 |  0.012 |         - |
  | spsc3 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
  | spsc4 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
  | mpsc2 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc3 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc4 lagging 2t  | 5,4 CCX          |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | spsc3 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
  | spsc4 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | mpsc3 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | mpsc4 lagging 2t  | 5,11 SMT         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   11,718 |  0.012 |         - |
  | spsc4 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   11,719 |  0.012 |         - |
  | mpsc2 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc3 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,624 |  0.016 |         - |
  | mpsc4 lagging 2t  | unpinned         |   4x64 |       - |   4/4 |   15,622 |  0.016 |         - |
  | spsc3 burst 1t    | core 5           |   1x32 |    13.4 |   1/1 |        0 |  0.000 |         - |
  | spsc3 burst 1t    | core 5           |   32x1 |    16.7 | 32/32 |  968,750 |  0.969 |       3.4 |
  | spsc4 burst 1t    | core 5           |   1x32 |     8.7 |   1/1 |        0 |  0.000 |         - |
  | spsc4 burst 1t    | core 5           |   32x1 |    18.3 | 32/32 |  968,750 |  0.969 |       9.8 |
  | mpsc2 burst 1t    | core 5           |   1x32 |     8.4 |   1/1 |        0 |  0.000 |         - |
  | mpsc2 burst 1t    | core 5           |   32x1 |    15.1 | 32/32 |  968,750 |  0.969 |       6.9 |
  | mpsc3 burst 1t    | core 5           |   1x32 |     7.7 |   1/1 |        0 |  0.000 |         - |
  | mpsc3 burst 1t    | core 5           |   32x1 |    14.4 | 32/32 |  968,750 |  0.969 |       6.9 |
  | mpsc4 burst 1t    | core 5           |   1x32 |     7.9 |   1/1 |        0 |  0.000 |         - |
  | mpsc4 burst 1t    | core 5           |   32x1 |    15.2 | 32/32 |  968,750 |  0.969 |       7.5 |
  | spsc3 stream 2t   | 5,4 CCX          |   1x32 |    18.1 |   1/1 |        0 |  0.000 |         - |
  | spsc3 stream 2t   | 5,4 CCX          |   32x1 |    32.5 | 32/32 |  999,987 |  1.000 |      14.4 |
  | spsc4 stream 2t   | 5,4 CCX          |   1x32 |     8.8 |   1/1 |        0 |  0.000 |         - |
  | spsc4 stream 2t   | 5,4 CCX          |   32x1 |    33.6 | 32/32 |  999,983 |  1.000 |      24.8 |
  | mpsc2 stream 2t   | 5,4 CCX          |   1x32 |     7.6 |   1/1 |        0 |  0.000 |         - |
  | mpsc2 stream 2t   | 5,4 CCX          |   32x1 |    64.8 | 32/32 |  985,539 |  0.986 |      58.0 |
  | mpsc3 stream 2t   | 5,4 CCX          |   1x32 |     6.5 |   1/1 |        0 |  0.000 |         - |
  | mpsc3 stream 2t   | 5,4 CCX          |   32x1 |    65.4 | 32/32 |  995,041 |  0.995 |      59.1 |
  | mpsc4 stream 2t   | 5,4 CCX          |   1x32 |     8.7 |   1/1 |        0 |  0.000 |         - |
  | mpsc4 stream 2t   | 5,4 CCX          |   32x1 |    64.1 | 32/32 |  976,643 |  0.977 |      56.7 |

  - line: the ring and the shape of the run. burst 1t: one thread fills every
    segment with the consumer idle, then drains, until the messages are moved.
    lagging 2t: the producer streams while the consumer reads two segments' worth
    between 20us pauses, so the producer runs ahead across segments at every
    pause. stream 2t: both spinning, the two_t loops' shape.
  - shape: segments x slots per segment. The first rows are the stress shape. The
    switch cost rows are the same 32 slots as one segment, which never switches,
    and as 32 segments of one slot, which switches on nearly every message.
  - ns/msg: elapsed over the messages moved, `-` where the line's pace is the
    consumer's pauses. segs: segments the producer wrote into, of the ring's.
    switches: segment switches, the producer's count, which the consumer's
    matched. sw/msg: switches per message, the burst's 3 per 256 at the stress
    shape.
  - switch ns: the cost of one switch, the gap in ns/msg between the two shapes
    over the gap in sw/msg, on the 32x1 row. Single-threaded it is the
    instructions alone. Streaming across cores it includes the cold segment
    crossing.
  ```
- `cargo +nightly miri test`: the full suite under
  [Miri](https://github.com/rust-lang/miri), which checks the
  `unsafe` code against the memory model. It has caught two
  real Stacked Borrows bugs here (an init retag in 0.3.0-4, a
  guard aliasing race in 0.3.0-6, the latter only visible
  with the threaded test included). One-time setup:
  `rustup component add --toolchain nightly miri`. The
  threaded stress test runs a reduced message count under
  Miri (interpreted spin loops are slow).

## Releasing

**`cargo install` lockfile gotcha.** If you install your project's
binary with `cargo install --path .`, also pass `--locked`. By
default `cargo install` ignores `Cargo.lock` and re-resolves
dependencies from scratch, so it can silently build the binary from
a different, possibly broken, dependency graph than `cargo build`
/ `cargo test` ran against.

| command | reads `Cargo.lock`? | re-resolves? |
| --- | --- | --- |
| `cargo build` / `test` / `run`  | yes     | only if `Cargo.toml` changed |
| `cargo build --locked` (etc.)   | yes     | never (errors if it would need to) |
| `cargo install` (default)       | **no**  | always, from scratch |
| `cargo install --locked`        | yes     | never (errors if it would need to) |

There's no stable cargo config knob to make `--locked` the default
for `cargo install`, so the discipline lives in the per-commit cargo
cycle and shell aliases (e.g. `alias ci='cargo install --locked'`).

The trade-off is predictable-as-tested (`--locked`) vs picking up
upstream fixes via fresh resolves (default). Either is defensible:
pick what fits your project, then edit this section and the
per-commit cargo cycle to match. Background:
[cargo#7169](https://github.com/rust-lang/cargo/issues/7169),
[cargo#9436](https://github.com/rust-lang/cargo/issues/9436).

## jj Tips for Git Users

This project uses [Jujutsu (jj)](https://docs.jj-vcs.dev/latest/)
alongside git. New to jj? See
[Steve Klabnik](https://github.com/steveklabnik)'s
[Jujutsu tutorial](https://steveklabnik.github.io/jujutsu-tutorial).

Repo-specific how-tos (initial commit, pushing, modifying and
force-pushing a commit, revsets, and a useful-commands reference)
live in [notes/jj-tips.md](notes/jj-tips.md).

## Cross-repo Linking with Git Trailers

Commits in each repo use [git trailers](https://git-scm.com/docs/git-interpret-trailers)
to cross-reference their counterpart in the other repo via an
`ochid` (Other Change ID) trailer, the defining mechanism of the
dual-repo convention. For the full definition (trailer syntax,
the example shape, per-commit mechanics, and `.vc-config.md`)
see
[Cross-repo linking (ochid trailers)](agent-data/jj.md#cross-repo-linking-ochid-trailers).

## Contributing

Agent workflow, commit conventions, and code style are
canonical in [AGENTS.md](AGENTS.md) (which `CLAUDE.md` imports)
and the files it links under [agent-data/](agent-data/):

- [Cycle protocol](AGENTS.md#cycle-protocol): the opening,
  the per-rung flow, and the close-out. The `X.Y.Z-N`
  suffix scheme is in
  [versioning.md](agent-data/versioning.md#suffix-scheme).
- [Commit description](AGENTS.md#commit-description):
  Conventional Commits title rules and the commit-body form.
- [Code conventions](agent-data/code.md): doc comments on
  every file / fn / method, `// OK: ...` on `unwrap*` calls.

Task tracking lives in [TODO.md](TODO.md): the ranked list,
and the running cycle's record in its `## In Progress` block.
Earlier cycles' records are frozen in `notes/chores/` and
`notes/done.md`, and notes-specific formatting rules are in
[notes/README.md](notes/README.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
