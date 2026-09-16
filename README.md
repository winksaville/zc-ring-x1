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
`attach` and `user()`. The multi-producer sibling is
`mpsc::v2::MpscRing`, the same ring of segments with any number
of producers sending through a fill closure, reached by path
since the crate-root `MpscRing` is still the single-region
`mpsc::v1`. How to use the two segmented rings from a pool to
two threads, with two complete programs, is the
[user guide](notes/user-guide.md).

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
  first, then the 7600X (Zen 4, six cores under one L3):

  ```text
  $ zc-ring-x1-demo
  zc-ring-x1 0.17.1-5
  demo: 1,000,000 messages each, depth 64, base cpu 11
  pool_alloc_free_1t (core 11):                    55,542,354 msgs/sec     18.0 ns/msg
  global_alloc_free_1t (core 11):                 138,928,599 msgs/sec      7.2 ns/msg

  spsc_ring_one_msg_1t (core 11):                 369,055,848 msgs/sec      2.7 ns/msg
  spsc1_ring_one_msg_1t (core 11):                137,210,123 msgs/sec      7.3 ns/msg
  spsc2_ring_one_msg_1t (core 11):                140,979,199 msgs/sec      7.1 ns/msg
  spsc3_ring_one_msg_1t (core 11):                 48,662,399 msgs/sec     20.5 ns/msg
  mpsc0_ring_one_msg_1t (core 11):                 93,216,469 msgs/sec     10.7 ns/msg
  mpsc1_ring_one_msg_1t (core 11):                 87,541,650 msgs/sec     11.4 ns/msg
  mpsc2_ring_one_msg_1t (core 11):                 67,279,212 msgs/sec     14.9 ns/msg
  spsc_ring_one_pool_msg_1t (core 11):             89,899,915 msgs/sec     11.1 ns/msg
  std_mpsc_one_pool_msg_1t (core 11):              28,041,914 msgs/sec     35.7 ns/msg

  spsc_ring_one_msg_2t (11,10 CCX):                63,372,476 msgs/sec     15.8 ns/msg
  spsc1_ring_one_msg_2t (11,10 CCX):               25,504,371 msgs/sec     39.2 ns/msg
  spsc2_ring_one_msg_2t (11,10 CCX):              195,149,024 msgs/sec      5.1 ns/msg
  spsc3_ring_one_msg_2t (11,10 CCX):               46,860,707 msgs/sec     21.3 ns/msg
  mpsc0_ring_one_msg_2t (11,10 CCX):               42,150,295 msgs/sec     23.7 ns/msg
  mpsc1_ring_one_msg_2t (11,10 CCX):               42,238,089 msgs/sec     23.7 ns/msg
  mpsc2_ring_one_msg_2t (11,10 CCX):               56,735,023 msgs/sec     17.6 ns/msg
  spsc_ring_one_pool_msg_2t (11,10 CCX):           13,693,746 msgs/sec     73.0 ns/msg
  std_mpsc_one_pool_msg_2t (11,10 CCX):             5,217,695 msgs/sec    191.7 ns/msg

  spsc_ring_one_msg_2t (11,8 x-CCX):                5,049,900 msgs/sec    198.0 ns/msg
  spsc1_ring_one_msg_2t (11,8 x-CCX):               7,940,920 msgs/sec    125.9 ns/msg
  spsc2_ring_one_msg_2t (11,8 x-CCX):              74,263,153 msgs/sec     13.5 ns/msg
  spsc3_ring_one_msg_2t (11,8 x-CCX):              19,136,483 msgs/sec     52.3 ns/msg
  mpsc0_ring_one_msg_2t (11,8 x-CCX):              10,067,719 msgs/sec     99.3 ns/msg
  mpsc1_ring_one_msg_2t (11,8 x-CCX):              11,916,643 msgs/sec     83.9 ns/msg
  mpsc2_ring_one_msg_2t (11,8 x-CCX):              47,591,662 msgs/sec     21.0 ns/msg
  spsc_ring_one_pool_msg_2t (11,8 x-CCX):           3,966,189 msgs/sec    252.1 ns/msg
  std_mpsc_one_pool_msg_2t (11,8 x-CCX):            2,968,124 msgs/sec    336.9 ns/msg

  spsc_ring_one_msg_2t (11,23 SMT):               144,312,701 msgs/sec      6.9 ns/msg
  spsc1_ring_one_msg_2t (11,23 SMT):               80,837,593 msgs/sec     12.4 ns/msg
  spsc2_ring_one_msg_2t (11,23 SMT):              135,450,350 msgs/sec      7.4 ns/msg
  spsc3_ring_one_msg_2t (11,23 SMT):               53,032,144 msgs/sec     18.9 ns/msg
  mpsc0_ring_one_msg_2t (11,23 SMT):               62,917,595 msgs/sec     15.9 ns/msg
  mpsc1_ring_one_msg_2t (11,23 SMT):               64,206,706 msgs/sec     15.6 ns/msg
  mpsc2_ring_one_msg_2t (11,23 SMT):               36,135,760 msgs/sec     27.7 ns/msg
  spsc_ring_one_pool_msg_2t (11,23 SMT):           27,025,353 msgs/sec     37.0 ns/msg
  std_mpsc_one_pool_msg_2t (11,23 SMT):            14,086,410 msgs/sec     71.0 ns/msg

  spsc_ring_one_msg_2t (unpinned):                 35,836,151 msgs/sec     27.9 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                19,737,585 msgs/sec     50.7 ns/msg
  spsc2_ring_one_msg_2t (unpinned):                97,605,902 msgs/sec     10.2 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                21,229,048 msgs/sec     47.1 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                11,937,908 msgs/sec     83.8 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                31,238,507 msgs/sec     32.0 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):                60,974,356 msgs/sec     16.4 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            12,375,406 msgs/sec     80.8 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):              4,098,484 msgs/sec    244.0 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):           7,528,351 msgs/sec    132.8 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3 and mpsc-v2 with 1 segment(s)

  | 1t core 11             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.6 |     2.5 |     2.5 |     2.5 |
  | spsc-v1                |     7.2 |     7.4 |    19.6 |     7.2 |
  | spsc-v2                |     7.0 |     7.1 |     7.6 |     7.6 |
  | spsc-v3                |    25.2 |    20.3 |    20.3 |    32.1 |
  | mpsc-v0                |       - |    10.6 |    10.7 |    10.8 |
  | mpsc-v1                |    11.0 |    11.0 |    11.0 |    22.9 |
  | mpsc-v2                |    14.1 |    14.4 |    14.2 |    14.3 |

  | 2t 11,10 CCX           |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    98.3 |    76.9 |    22.8 |     9.7 |
  | spsc-v1                |    74.4 |    68.8 |    42.8 |    35.7 |
  | spsc-v2                |    71.8 |    48.9 |     9.1 |     5.8 |
  | spsc-v3                |    82.9 |    43.3 |    18.9 |    15.0 |
  | mpsc-v0                |       - |    80.1 |    45.0 |    26.2 |
  | mpsc-v1                |   101.2 |    58.3 |    45.4 |    27.5 |
  | mpsc-v2                |    85.3 |    37.1 |    13.4 |    14.4 |

  | 2t 11,8 x-CCX          |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |   385.7 |   232.4 |   194.4 |   229.4 |
  | spsc-v1                |   480.7 |   233.5 |   142.6 |   107.9 |
  | spsc-v2                |   213.9 |   121.5 |    37.3 |    17.5 |
  | spsc-v3                |   216.2 |   170.3 |    68.4 |    31.6 |
  | mpsc-v0                |       - |   213.1 |   108.8 |    93.1 |
  | mpsc-v1                |   548.5 |   270.1 |   122.5 |    98.4 |
  | mpsc-v2                |   221.0 |   130.6 |    34.6 |    19.0 |

  | 2t 11,23 SMT           |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    33.1 |    15.5 |     7.1 |     7.1 |
  | spsc-v1                |    53.6 |    27.1 |    16.1 |    13.2 |
  | spsc-v2                |    44.2 |    23.0 |     8.0 |     7.8 |
  | spsc-v3                |    42.9 |    30.0 |    20.0 |    21.0 |
  | mpsc-v0                |       - |    27.2 |    17.1 |    15.4 |
  | mpsc-v1                |    45.0 |    24.7 |    17.9 |    18.3 |
  | mpsc-v2                |    53.5 |    27.9 |    16.9 |    17.1 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |   216.3 |    75.3 |    44.2 |    11.1 |
  | spsc-v1                |   183.2 |    79.8 |    46.9 |    34.1 |
  | spsc-v2                |   196.7 |   107.1 |    29.9 |    12.3 |
  | spsc-v3                |   143.4 |    76.6 |    20.9 |    29.2 |
  | mpsc-v0                |       - |   180.6 |    34.5 |    61.2 |
  | mpsc-v1                |   459.4 |   202.0 |   103.0 |    80.8 |
  | mpsc-v2                |   164.7 |   114.5 |    31.3 |    17.6 |
  ```

  ```text
  $ zc-ring-x1-demo
  zc-ring-x1 0.17.1-5
  demo: 1,000,000 messages each, depth 64, base cpu 5
  pool_alloc_free_1t (core 5):                    227,708,167 msgs/sec      4.4 ns/msg
  global_alloc_free_1t (core 5):                  179,801,818 msgs/sec      5.6 ns/msg

  spsc_ring_one_msg_1t (core 5):                  570,357,602 msgs/sec      1.8 ns/msg
  spsc1_ring_one_msg_1t (core 5):                 184,600,786 msgs/sec      5.4 ns/msg
  spsc2_ring_one_msg_1t (core 5):                 197,204,002 msgs/sec      5.1 ns/msg
  spsc3_ring_one_msg_1t (core 5):                  72,388,772 msgs/sec     13.8 ns/msg
  mpsc0_ring_one_msg_1t (core 5):                 158,175,640 msgs/sec      6.3 ns/msg
  mpsc1_ring_one_msg_1t (core 5):                 154,874,712 msgs/sec      6.5 ns/msg
  mpsc2_ring_one_msg_1t (core 5):                 120,334,235 msgs/sec      8.3 ns/msg
  spsc_ring_one_pool_msg_1t (core 5):             126,895,759 msgs/sec      7.9 ns/msg
  std_mpsc_one_pool_msg_1t (core 5):               49,253,225 msgs/sec     20.3 ns/msg

  spsc_ring_one_msg_2t (5,4 CCX):                 129,990,497 msgs/sec      7.7 ns/msg
  spsc1_ring_one_msg_2t (5,4 CCX):                 57,964,502 msgs/sec     17.3 ns/msg
  spsc2_ring_one_msg_2t (5,4 CCX):                333,848,015 msgs/sec      3.0 ns/msg
  spsc3_ring_one_msg_2t (5,4 CCX):                 57,274,257 msgs/sec     17.5 ns/msg
  mpsc0_ring_one_msg_2t (5,4 CCX):                 63,197,399 msgs/sec     15.8 ns/msg
  mpsc1_ring_one_msg_2t (5,4 CCX):                 66,149,111 msgs/sec     15.1 ns/msg
  mpsc2_ring_one_msg_2t (5,4 CCX):                137,871,378 msgs/sec      7.3 ns/msg
  spsc_ring_one_pool_msg_2t (5,4 CCX):             19,923,164 msgs/sec     50.2 ns/msg
  std_mpsc_one_pool_msg_2t (5,4 CCX):               8,283,694 msgs/sec    120.7 ns/msg

  spsc_ring_one_msg_2t (5,11 SMT):                165,661,186 msgs/sec      6.0 ns/msg
  spsc1_ring_one_msg_2t (5,11 SMT):                81,511,138 msgs/sec     12.3 ns/msg
  spsc2_ring_one_msg_2t (5,11 SMT):               241,918,881 msgs/sec      4.1 ns/msg
  spsc3_ring_one_msg_2t (5,11 SMT):                64,507,314 msgs/sec     15.5 ns/msg
  mpsc0_ring_one_msg_2t (5,11 SMT):                82,388,162 msgs/sec     12.1 ns/msg
  mpsc1_ring_one_msg_2t (5,11 SMT):                86,338,674 msgs/sec     11.6 ns/msg
  mpsc2_ring_one_msg_2t (5,11 SMT):               109,485,914 msgs/sec      9.1 ns/msg
  spsc_ring_one_pool_msg_2t (5,11 SMT):            34,763,183 msgs/sec     28.8 ns/msg
  std_mpsc_one_pool_msg_2t (5,11 SMT):             17,325,611 msgs/sec     57.7 ns/msg

  spsc_ring_one_msg_2t (unpinned):                124,253,994 msgs/sec      8.0 ns/msg
  spsc1_ring_one_msg_2t (unpinned):                55,432,747 msgs/sec     18.0 ns/msg
  spsc2_ring_one_msg_2t (unpinned):               288,657,603 msgs/sec      3.5 ns/msg
  spsc3_ring_one_msg_2t (unpinned):                58,592,372 msgs/sec     17.1 ns/msg
  mpsc0_ring_one_msg_2t (unpinned):                60,788,824 msgs/sec     16.5 ns/msg
  mpsc1_ring_one_msg_2t (unpinned):                61,822,189 msgs/sec     16.2 ns/msg
  mpsc2_ring_one_msg_2t (unpinned):               131,994,765 msgs/sec      7.6 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):            19,286,700 msgs/sec     51.8 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):              7,872,802 msgs/sec    127.0 ns/msg
  mpsc1_ring_one_msg_3t (2p+1c unpinned):          23,902,238 msgs/sec     41.8 ns/msg

  depth sweep: 1,000,000 messages per cell, ns/msg at depths 1, 2, 8, 64, spsc-v3 and mpsc-v2 with 1 segment(s)

  | 1t core 5              |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |     2.3 |     1.9 |     1.9 |     1.9 |
  | spsc-v1                |     5.3 |     5.5 |     5.5 |     5.4 |
  | spsc-v2                |     5.0 |     5.2 |     5.2 |     5.2 |
  | spsc-v3                |    15.4 |    13.4 |    13.6 |    13.4 |
  | mpsc-v0                |       - |     6.4 |     6.4 |     6.4 |
  | mpsc-v1                |     6.6 |     6.6 |     6.5 |     6.6 |
  | mpsc-v2                |     8.5 |     8.4 |     8.4 |     8.5 |

  | 2t 5,4 CCX             |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    67.1 |    40.7 |    14.1 |     7.4 |
  | spsc-v1                |    50.3 |    36.5 |    17.8 |    15.6 |
  | spsc-v2                |    40.6 |    25.0 |     7.5 |     3.1 |
  | spsc-v3                |    67.7 |    43.9 |    17.7 |    17.0 |
  | mpsc-v0                |       - |    39.2 |    17.8 |    17.3 |
  | mpsc-v1                |    84.3 |    38.8 |    21.0 |    16.2 |
  | mpsc-v2                |    43.1 |    28.3 |     7.7 |     7.0 |

  | 2t 5,11 SMT            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    27.5 |    14.6 |     5.4 |     6.1 |
  | spsc-v1                |    43.4 |    24.3 |    13.3 |    12.3 |
  | spsc-v2                |    20.2 |    10.8 |     4.9 |     4.5 |
  | spsc-v3                |    53.0 |    26.4 |    15.8 |    15.6 |
  | mpsc-v0                |       - |    22.8 |    12.9 |    12.0 |
  | mpsc-v1                |    39.8 |    22.4 |    12.5 |    11.7 |
  | mpsc-v2                |    32.7 |    17.1 |     9.0 |     9.1 |

  | 2t unpinned            |     d=1 |     d=2 |     d=8 |    d=64 |
  |------------------------|--------:|--------:|--------:|--------:|
  | spsc-v0                |    59.3 |    41.5 |    13.0 |     7.2 |
  | spsc-v1                |    75.7 |    42.2 |    17.5 |    17.1 |
  | spsc-v2                |    40.7 |    25.4 |     7.6 |     3.7 |
  | spsc-v3                |    59.8 |    44.9 |    17.9 |    17.1 |
  | mpsc-v0                |       - |    41.6 |    19.4 |    17.7 |
  | mpsc-v1                |    79.1 |    41.8 |    18.9 |    17.3 |
  | mpsc-v2                |    47.7 |    22.5 |     7.9 |     7.9 |
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
