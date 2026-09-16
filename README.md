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
one segment, and the others absorb a producer that runs ahead.
The points above describe the single-region rings, `spsc::v0`
through `spsc::v2`, which stay available by path and keep
`attach` and `user()`.

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

Status: an experiment. SPSC only. Attaching to an existing
shared-memory region is `unsafe` (see `spsc::v2::Ring::attach`), and planned
hardening and follow-ons are tracked in
[TODO.md](TODO.md).

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
- **in-flight**: the descriptor form, where `into_desc` consumes
  the guard and ownership travels in the returned `Desc`
  (typically through a ring). The sender must no longer
  touch the buffer. Whoever `resolve`s the descriptor owns
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
  8-byte `Desc` with `into_desc`, and send *that* through
  a ring as an ordinary POD message, and the receiver
  `resolve`s it back to an owned guard. The payload stays
  put in its pool buffer:

```rust,ignore
// Sender: pool allocator + ring producer.
let mut msg = pool.alloc::<Msg>()?;   // get a message
msg.seq = 42;                         // fill it in place
let desc = registry.into_desc(pool_id, msg)?; // guard -> Desc
let mut slot = producer.reserve_slot_with::<Desc>(|_| false)?;
*slot = desc;                         // 8 bytes, not the payload
slot.commit();

// Receiver: ring consumer + freer (another thread).
let slot = consumer.reserve_slot_with::<Desc>(|_| false)?;
let desc = *slot;
slot.release();                       // ring slot free again
// SAFETY: desc came from into_desc, arrived via the ring's
// commit -> reserve handoff, resolved exactly once.
let msg = unsafe { registry.resolve::<Msg>(desc) }?;
//                  ... read msg ...
msg.free();                           // buffer back to its pool
```

One allocation's bytes are written once and never copied,
not by send, not by receive. (`resolve` is the one `unsafe`:
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
- `cargo run --release`: the demo binary
  ([src/bin/zc-ring-x1-demo.rs](src/bin/zc-ring-x1-demo.rs)):
  a throughput scoreboard (msgs/sec and ns/msg) grouped so
  like compares with like: alloc/free baselines (pool vs
  global allocator), then three message flows (raw ring,
  composed ring + pool descriptors, std channel + pool) at
  each thread placement: single thread on the base cpu, then
  two threads at each placement the machine has, in the
  measurement tools' terms: CCX, two cores on one L3, x-CCX,
  cores on different L3s, SMT, one core's two hardware
  threads sharing its L1 and L2, and unpinned (pairs
  discovered from /sys at runtime, each line naming its
  cpus, and a placement the machine lacks is absent, the
  7600X having no x-CCX). Eyeball numbers (single runs, no
  mean/stdev), not a benchmark (calibrated measurement lives
  in iiac-perf). Installable: `cargo install --path .
  --locked`, then `zc-ring-x1-demo`, and `-V` prints the
  version-of-record so you know which build you are
  testing. `--base-cpu <n>` sets the base, the cpu the
  single-thread lines pin to and every pair starts from, 1
  by default since the kernel favors cpu 0 and it runs
  noisier. `-h` prints the usage. An example run on
  each machine, the 3900X (Zen 2, 12 cores over four CCXs)
  first, then the 7600X (Zen 4, six cores under one L3):

  ```text
  $ zc-ring-x1-demo
  zc-ring-x1 0.17.1-4
  demo: 1,000,000 messages each, depth 64, base cpu 1
  pool_alloc_free_1t (core 1):                    102,107,133 msgs/sec      9.8 ns/msg
  global_alloc_free_1t (core 1):                  144,542,341 msgs/sec      6.9 ns/msg

  spsc_ring_one_msg_1t (core 1):                  362,033,542 msgs/sec      2.8 ns/msg
  spsc1_ring_one_msg_1t (core 1):                 134,423,688 msgs/sec      7.4 ns/msg
  spsc2_ring_one_msg_1t (core 1):                 140,045,332 msgs/sec      7.1 ns/msg
  spsc3_ring_one_msg_1t (core 1):                  48,415,048 msgs/sec     20.7 ns/msg
  mpsc0_ring_one_msg_1t (core 1):                  95,978,786 msgs/sec     10.4 ns/msg
  mpsc1_ring_one_msg_1t (core 1):                  91,624,772 msgs/sec     10.9 ns/msg
  mpsc2_ring_one_msg_1t (core 1):                  69,402,663 msgs/sec     14.4 ns/msg
  spsc_ring_one_pool_msg_1t (core 1):              93,618,302 msgs/sec     10.7 ns/msg
  std_mpsc_one_pool_msg_1t (core 1):               28,698,049 msgs/sec     34.8 ns/msg

  spsc_ring_one_msg_2t (1,2 CCX):                 103,290,056 msgs/sec      9.7 ns/msg
  spsc1_ring_one_msg_2t (1,2 CCX):                 29,906,380 msgs/sec     33.4 ns/msg
  spsc2_ring_one_msg_2t (1,2 CCX):                205,150,045 msgs/sec      4.9 ns/msg
  spsc3_ring_one_msg_2t (1,2 CCX):                 48,711,583 msgs/sec     20.5 ns/msg
  mpsc0_ring_one_msg_2t (1,2 CCX):                 42,762,235 msgs/sec     23.4 ns/msg
  mpsc1_ring_one_msg_2t (1,2 CCX):                 42,666,597 msgs/sec     23.4 ns/msg
  mpsc2_ring_one_msg_2t (1,2 CCX):                 59,668,178 msgs/sec     16.8 ns/msg
  spsc_ring_one_pool_msg_2t (1,2 CCX):             14,346,242 msgs/sec     69.7 ns/msg
  std_mpsc_one_pool_msg_2t (1,2 CCX):               5,860,406 msgs/sec    170.6 ns/msg

  spsc_ring_one_msg_2t (1,3 x-CCX):                 4,705,286 msgs/sec    212.5 ns/msg
  spsc1_ring_one_msg_2t (1,3 x-CCX):                9,422,658 msgs/sec    106.1 ns/msg
  spsc2_ring_one_msg_2t (1,3 x-CCX):               76,260,106 msgs/sec     13.1 ns/msg
  spsc3_ring_one_msg_2t (1,3 x-CCX):               44,980,583 msgs/sec     22.2 ns/msg
  mpsc0_ring_one_msg_2t (1,3 x-CCX):               11,451,077 msgs/sec     87.3 ns/msg
  mpsc1_ring_one_msg_2t (1,3 x-CCX):               11,495,650 msgs/sec     87.0 ns/msg
  mpsc2_ring_one_msg_2t (1,3 x-CCX):               55,494,021 msgs/sec     18.0 ns/msg
  spsc_ring_one_pool_msg_2t (1,3 x-CCX):            4,523,394 msgs/sec    221.1 ns/msg
  std_mpsc_one_pool_msg_2t (1,3 x-CCX):             3,391,882 msgs/sec    294.8 ns/msg

  spsc_ring_one_msg_2t (1,13 SMT):                135,737,331 msgs/sec      7.4 ns/msg
  spsc1_ring_one_msg_2t (1,13 SMT):                77,265,446 msgs/sec     12.9 ns/msg
  spsc2_ring_one_msg_2t (1,13 SMT):               125,147,517 msgs/sec      8.0 ns/msg
  spsc3_ring_one_msg_2t (1,13 SMT):                50,036,681 msgs/sec     20.0 ns/msg
  mpsc0_ring_one_msg_2t (1,13 SMT):                61,115,030 msgs/sec     16.4 ns/msg
  mpsc1_ring_one_msg_2t (1,13 SMT):                62,109,428 msgs/sec     16.1 ns/msg
  mpsc2_ring_one_msg_2t (1,13 SMT):                67,994,929 msgs/sec     14.7 ns/msg
  spsc_ring_one_pool_msg_2t (1,13 SMT):            27,936,090 msgs/sec     35.8 ns/msg
  std_mpsc_one_pool_msg_2t (1,13 SMT):             17,807,126 msgs/sec     56.2 ns/msg

  spsc_ring_one_msg_2t (unpinned):                 47,658,513 msgs/sec     21.0 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                19,014,907 msgs/sec     52.6 ns/msg
  spsc2_ring_one_msg_2t (unpinned):               119,787,420 msgs/sec      8.3 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                52,329,793 msgs/sec     19.1 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                33,026,975 msgs/sec     30.3 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                31,760,180 msgs/sec     31.5 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):                59,239,182 msgs/sec     16.9 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            12,025,132 msgs/sec     83.2 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):              5,772,265 msgs/sec    173.2 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):          16,215,481 msgs/sec     61.7 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3 and mpsc-v2 with 1 segment(s)

  | 1t core 1              |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.8 |     2.8 |     2.8 |     2.8 |
  | spsc-v1                |     7.5 |     7.4 |     7.4 |     7.4 |
  | spsc-v2                |     6.9 |     6.8 |     6.8 |     6.8 |
  | spsc-v3                |    24.0 |    19.8 |    19.7 |    19.5 |
  | mpsc-v0                |       - |     9.8 |     9.7 |     9.7 |
  | mpsc-v1                |    10.1 |    10.2 |    10.0 |    10.0 |
  | mpsc-v2                |    13.1 |    13.1 |    13.1 |    13.1 |

  | 2t 1,2 CCX             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    88.5 |    65.3 |    22.9 |     9.8 |
  | spsc-v1                |    91.4 |    63.9 |    47.6 |    33.8 |
  | spsc-v2                |    69.7 |    35.0 |     8.7 |     6.0 |
  | spsc-v3                |    68.3 |    44.2 |    21.5 |    15.2 |
  | mpsc-v0                |       - |    62.6 |    30.9 |    24.4 |
  | mpsc-v1                |    94.8 |    62.6 |    30.5 |    24.6 |
  | mpsc-v2                |    71.2 |    35.2 |    12.2 |    14.4 |

  | 2t 1,3 x-CCX           |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |   339.0 |   201.3 |   153.2 |   209.8 |
  | spsc-v1                |   368.0 |   200.9 |   126.3 |   103.1 |
  | spsc-v2                |   195.0 |   103.9 |    29.5 |    11.2 |
  | spsc-v3                |   194.5 |   141.8 |    51.6 |    32.1 |
  | mpsc-v0                |       - |   188.4 |   108.8 |    90.6 |
  | mpsc-v1                |   460.3 |   192.0 |   101.4 |    87.0 |
  | mpsc-v2                |   192.4 |   110.9 |    30.5 |    16.5 |

  | 2t 1,13 SMT            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    29.6 |    13.3 |     6.9 |     6.7 |
  | spsc-v1                |    48.2 |    25.9 |    14.8 |    11.8 |
  | spsc-v2                |    39.0 |    20.1 |     7.0 |     7.0 |
  | spsc-v3                |    39.8 |    28.2 |    18.3 |    18.5 |
  | mpsc-v0                |       - |    23.5 |    15.1 |    15.0 |
  | mpsc-v1                |    41.2 |    22.6 |    14.8 |    14.8 |
  | mpsc-v2                |    41.0 |    22.7 |    13.5 |    13.5 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    97.7 |    79.0 |    64.4 |    12.9 |
  | spsc-v1                |   106.5 |    95.2 |    41.2 |    36.3 |
  | spsc-v2                |    98.6 |    39.6 |    15.2 |    12.3 |
  | spsc-v3                |    74.2 |    53.9 |    25.5 |    23.3 |
  | mpsc-v0                |       - |    60.9 |    34.3 |    28.8 |
  | mpsc-v1                |   100.5 |    80.1 |    35.5 |    24.3 |
  | mpsc-v2                |    77.7 |    42.1 |    20.8 |    17.5 |
  ```

  ```text
  $ zc-ring-x1-demo
  zc-ring-x1 0.17.1-4
  demo: 1,000,000 messages each, depth 64, base cpu 1
  pool_alloc_free_1t (core 1):                    217,928,783 msgs/sec      4.6 ns/msg
  global_alloc_free_1t (core 1):                  168,953,794 msgs/sec      5.9 ns/msg

  spsc_ring_one_msg_1t (core 1):                  311,138,187 msgs/sec      3.2 ns/msg
  spsc1_ring_one_msg_1t (core 1):                 181,486,688 msgs/sec      5.5 ns/msg
  spsc2_ring_one_msg_1t (core 1):                 181,801,026 msgs/sec      5.5 ns/msg
  spsc3_ring_one_msg_1t (core 1):                  74,754,517 msgs/sec     13.4 ns/msg
  mpsc0_ring_one_msg_1t (core 1):                 153,556,499 msgs/sec      6.5 ns/msg
  mpsc1_ring_one_msg_1t (core 1):                 158,045,520 msgs/sec      6.3 ns/msg
  mpsc2_ring_one_msg_1t (core 1):                 123,694,206 msgs/sec      8.1 ns/msg
  spsc_ring_one_pool_msg_1t (core 1):             121,017,598 msgs/sec      8.3 ns/msg
  std_mpsc_one_pool_msg_1t (core 1):               49,263,355 msgs/sec     20.3 ns/msg

  spsc_ring_one_msg_2t (1,2 CCX):                 136,094,231 msgs/sec      7.3 ns/msg
  spsc1_ring_one_msg_2t (1,2 CCX):                 59,333,588 msgs/sec     16.9 ns/msg
  spsc2_ring_one_msg_2t (1,2 CCX):                318,499,130 msgs/sec      3.1 ns/msg
  spsc3_ring_one_msg_2t (1,2 CCX):                 56,637,078 msgs/sec     17.7 ns/msg
  mpsc0_ring_one_msg_2t (1,2 CCX):                 63,488,451 msgs/sec     15.8 ns/msg
  mpsc1_ring_one_msg_2t (1,2 CCX):                 64,160,625 msgs/sec     15.6 ns/msg
  mpsc2_ring_one_msg_2t (1,2 CCX):                141,727,815 msgs/sec      7.1 ns/msg
  spsc_ring_one_pool_msg_2t (1,2 CCX):             19,659,758 msgs/sec     50.9 ns/msg
  std_mpsc_one_pool_msg_2t (1,2 CCX):               8,365,571 msgs/sec    119.5 ns/msg

  spsc_ring_one_msg_2t (1,7 SMT):                 176,559,719 msgs/sec      5.7 ns/msg
  spsc1_ring_one_msg_2t (1,7 SMT):                 81,477,865 msgs/sec     12.3 ns/msg
  spsc2_ring_one_msg_2t (1,7 SMT):                241,383,688 msgs/sec      4.1 ns/msg
  spsc3_ring_one_msg_2t (1,7 SMT):                 65,190,331 msgs/sec     15.3 ns/msg
  mpsc0_ring_one_msg_2t (1,7 SMT):                 83,631,591 msgs/sec     12.0 ns/msg
  mpsc1_ring_one_msg_2t (1,7 SMT):                 85,268,138 msgs/sec     11.7 ns/msg
  mpsc2_ring_one_msg_2t (1,7 SMT):                109,355,158 msgs/sec      9.1 ns/msg
  spsc_ring_one_pool_msg_2t (1,7 SMT):             34,607,328 msgs/sec     28.9 ns/msg
  std_mpsc_one_pool_msg_2t (1,7 SMT):              17,182,225 msgs/sec     58.2 ns/msg

  spsc_ring_one_msg_2t (unpinned):                135,531,326 msgs/sec      7.4 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                56,453,269 msgs/sec     17.7 ns/msg
  spsc2_ring_one_msg_2t (unpinned):               281,701,250 msgs/sec      3.5 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                53,018,938 msgs/sec     18.9 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                58,310,635 msgs/sec     17.1 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                57,746,302 msgs/sec     17.3 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):               138,389,533 msgs/sec      7.2 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            19,238,345 msgs/sec     52.0 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):              9,418,785 msgs/sec    106.2 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):          21,490,062 msgs/sec     46.5 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3 and mpsc-v2 with 1 segment(s)

  | 1t core 1              |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.4 |     2.1 |     2.1 |     2.1 |
  | spsc-v1                |     5.3 |     5.4 |     5.4 |     5.4 |
  | spsc-v2                |     5.4 |     5.7 |     5.7 |     5.7 |
  | spsc-v3                |    15.3 |    13.3 |    13.4 |    13.7 |
  | mpsc-v0                |       - |     6.7 |     7.0 |     7.0 |
  | mpsc-v1                |     6.5 |     6.5 |     6.4 |     6.5 |
  | mpsc-v2                |     8.2 |     8.3 |     8.3 |     8.3 |

  | 2t 1,2 CCX             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    46.7 |    41.5 |    13.7 |     6.7 |
  | spsc-v1                |    47.9 |    39.8 |    17.3 |    16.7 |
  | spsc-v2                |    41.1 |    22.8 |     6.6 |     3.5 |
  | spsc-v3                |    60.6 |    43.9 |    17.7 |    18.6 |
  | mpsc-v0                |       - |    39.1 |    21.2 |    16.0 |
  | mpsc-v1                |    52.7 |    39.1 |    22.7 |    15.7 |
  | mpsc-v2                |    42.6 |    25.1 |     7.7 |     7.2 |

  | 2t 1,7 SMT             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    27.5 |    14.4 |     5.0 |     5.7 |
  | spsc-v1                |    43.5 |    24.4 |    13.4 |    12.5 |
  | spsc-v2                |    26.5 |    10.7 |     4.5 |     4.5 |
  | spsc-v3                |    55.1 |    26.3 |    15.6 |    15.4 |
  | mpsc-v0                |       - |    22.7 |    12.6 |    12.1 |
  | mpsc-v1                |    39.7 |    22.5 |    12.4 |    11.7 |
  | mpsc-v2                |    32.7 |    17.2 |     8.9 |     8.9 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    56.3 |    39.8 |    14.3 |     6.7 |
  | spsc-v1                |    78.0 |    40.4 |    19.0 |    18.6 |
  | spsc-v2                |    40.5 |    26.6 |     8.5 |     3.8 |
  | spsc-v3                |    61.5 |    45.3 |    18.0 |    18.7 |
  | mpsc-v0                |       - |    42.6 |    19.1 |    15.8 |
  | mpsc-v1                |    81.5 |    41.1 |    19.8 |    15.7 |
  | mpsc-v2                |    44.6 |    27.2 |     8.1 |     8.1 |
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
