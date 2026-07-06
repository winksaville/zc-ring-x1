# zc-ring-x1

Zero copy ring buffer experiment 1

This is the main repo of a dual-repo convention for using
a bot to help develop the code.

The beginnings of a tool to help is [vc-x1](https://github.com/winksaville/vc-x1)

## Overview

A `no_std` SPSC (single-producer / single-consumer) ring buffer
over a caller-provided memory region, moving typed messages with
zero copying at the boundary — the producer writes through a
`&mut T` directly into buffer memory, the consumer reads through
a `&T` in place. Typed views are validated casts via
[zerocopy](https://docs.rs/zerocopy); the same `#[repr(C)]`
layout works between threads and, via shared memory, between
processes. The full design — requirements, memory layout, index
scheme, API, validation — lives in
[notes/ring-buffer-design.md](notes/ring-buffer-design.md), kept
in sync with `src/`.

- M slots × N bytes, both caller-chosen: M a power of two, N a
  cache-line multiple, every slot cache-line aligned.
- Free-running `AtomicU32` indices, masked only at slot access;
  no sacrificial slot; lock-free with Acquire/Release pairing.
- Guard API: `reserve_slot_with::<T>(policy)` (or the
  `reserve_slot_spin::<T>()` convenience) on either endpoint —
  write → `commit()` on the producer side, read → `release()` on
  the consumer side; dropping a guard abandons cleanly.
- IPC-ready: one contiguous region, no internal pointers, an
  init/attach handshake (magic published last, Release), and an
  all-atomic header so a misbehaving peer can corrupt data but
  never cause UB.
- An app-owned scratch line in the header (`user()` on both
  endpoints, 16 `AtomicU32`s): the crate zeroes it at init and
  never touches it again — a shared-memory home for app wakeup
  protocols; the crate itself never blocks or spins.
- Validated by tests (including a threaded stress and a
  u32-index-wrap proof) and [Miri](https://github.com/rust-lang/miri)
  on the non-threaded suite.

```rust
use zc_ring_x1::Ring;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

// Cache-line-aligned region: 256 B header + 4 slots × 64 B.
#[repr(C, align(64))]
struct Region([u8; 512]);
let mut region = Region([0; 512]);

let (mut producer, mut consumer) =
    Ring::init(&mut region.0, 64, 4).unwrap().split();

let mut slot = producer.reserve_slot_with::<Msg>(|_| false).unwrap();
slot.seq = 1;
slot.val = 42;
slot.commit(); // publish to the consumer

let msg = consumer.reserve_slot_with::<Msg>(|_| false).unwrap();
assert_eq!(msg.val, 42);
msg.release(); // slot is free for reuse
```

Status: an experiment. SPSC only; attaching to an existing
shared-memory region is `unsafe` (see `Ring::attach`); planned
hardening and follow-ons are tracked in
[notes/todo.md](notes/todo.md).

## Message pool

The companion allocation primitive: fixed-size
cache-line-aligned buffers over a caller-provided region,
decoupling "get a message" from "send it" — allocate a
buffer, hold it as long as you like, free (or eventually
send) it whenever. Design and roadmap:
[Messaging layer: pools and descriptor queues](notes/ring-buffer-design.md#messaging-layer-pools-and-descriptor-queues).

- Intrusive LIFO free-stack with zero per-buffer overhead: a
  *free* buffer's first word is its next-link; an allocated
  buffer carries no header at all.
- `alloc::<T>()` returns a `BufSlot<T>` — the ownership
  token for one buffer, `Deref`/`DerefMut` to `T` in place.
  The pop is single-popper Treiber (`&mut self`, no ABA):
  one allocator per pool.
- `BufSlot::free` is an MPSC push: whichever thread or
  process ends up holding the guard may free it.
- `Pool::init` creates and validates a fresh region;
  `Pool::attach` joins one initialized elsewhere — `unsafe`
  like `Ring::attach` (the caller vouches for the mapping),
  and its contract carries the single-allocator rule: at
  most one attached handle allocates, any handle frees.
- Messaging aside, a pool is a fast fixed-size object
  allocator in its own right: O(1) alloc/free, no locks, no
  syscalls, cache-aligned `T`s with LIFO reuse keeping the
  working set hot — usable today for app-owned objects that
  never travel.
- `BufSlot` does not borrow the pool — any number of
  allocated buffers live at once; dropping without `free`
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
exercise exactly what this model permits; anything outside it
is a contract violation even where the compiler cannot reject
it.

**Roles.** Roles are per *object*, and one thread may hold
several roles across objects (a typical sender is one pool's
allocator and one ring's producer):

- **Ring** — exactly one producer and one consumer, ever
  (SPSC): in-process, `split()` hands out each endpoint
  once; cross-process, one producing and one consuming
  process by contract. An endpoint reserves at most one
  slot at a time (guard holds the endpoint borrow).
- **Pool** — exactly one *allocator* at a time (the
  single-popper contract on `Pool::attach`; in-process,
  `alloc` takes `&mut self`), and **any number of freers**:
  whoever holds a `BufSlot` may free it, from any thread or
  process attached to the pool.

**Buffer lifecycle.** A pool buffer is always in exactly one
state, with one party allowed to touch it:

- **free** — on the free-stack; the pool protocol owns it.
  Nobody reads or writes it except through alloc/free
  machinery (its first word is the next-link).
- **allocated** — popped by `alloc`, owned by the `BufSlot`
  holder: exclusive read/write through the guard, held as
  long as desired, moved between threads freely (`Send`).
  No other party — including the pool — may touch the
  bytes.
- **in-flight** — the descriptor form: `into_desc` consumes
  the guard and ownership travels in the returned `Desc`
  (typically through a ring). The sender must no longer
  touch the buffer; whoever `resolve`s the descriptor owns
  it, exactly once.
- **freed** — `BufSlot::free` pushes it back; ownership
  returns to the pool protocol the instant the CAS lands.
  Use-after-free of the guard is unrepresentable (`free`
  consumes it); dropping without `free` leaks the buffer.

**What "send" means.** Two forms compose:

- Moving the `BufSlot` itself (an ordinary Rust move;
  `Send` lets it cross threads or a std channel).
- The descriptor flow (0.7.0): register the pool in a
  per-process `PoolRegistry`, convert the guard to an
  8-byte `Desc` with `into_desc`, and send *that* through
  a ring as an ordinary POD message; the receiver
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

One allocation's bytes are written once and never copied —
not by send, not by receive. (`resolve` is the one `unsafe`:
validation rejects unknown ids / bad indices / wrong types,
but ownership uniqueness is the caller's promise. Paired
sender/receiver endpoints that encapsulate it — and shrink
this to loan / send / recv — are the next cycle; see
notes/todo.md.)

**Trust.** Unchanged from the ring: every word in shared
memory is untrusted input. The pool validates the head and
next-link at every pop and fails toward `Exhausted`; a
hostile peer can degrade service (garbage messages, lost
buffers, spurious exhaustion), never cause UB on this side.

## Performance runs using benches in iiac-perf each 5min (300s) duration
```
wink@3900x 26-07-06T15:13:37.833Z:~/data/prgs/rust/iiac-perf (main+1)
$ iiac-perf -d 300 zcr-raw-1t zcr-with-1t zcr-spin-1t zcr-raw-2t zcr-with-2t zcr-spin-2t
iiac-perf 0.15.0 — Rust latency microbenchmark harness

Calibration:
  framing/sample      11.12 ns  (timer pair, two-point fit)
  loop/iter            0.49 ns  (per inner-loop iteration)
  cal pin           core 0 (unpinned after cal; --no-pin-cal to skip)
  bench pin         none (unpinned)
  sleep inhibit     active (systemd-inhibit --what=sleep)
  config            none (built-in defaults)

zcr-raw-1t: zc-ring-x1 raw round-trip (1 thread) [duration=300.0s outer=1,440,771,241 inner=35 calls=50,426,993,435 adj/call=0.81ns labels=both]:
                            first           last         range          count           mean       adjusted
  z4  0.000_1              4.3 ns         4.3 ns        0.0 ns          6,171         4.3 ns         3.5 ns
  p30 0.30                 4.6 ns         4.6 ns        0.0 ns    738,479,210         4.6 ns         3.8 ns
  p70 0.70                 4.9 ns         4.9 ns        0.0 ns    381,068,110         4.9 ns         4.1 ns
  p90 0.90                 5.1 ns         5.1 ns        0.0 ns    275,496,171         5.1 ns         4.3 ns
  n2  0.99                 5.4 ns         5.4 ns        0.0 ns     28,576,262         5.4 ns         4.6 ns
  n3  0.999                5.7 ns         6.3 ns        0.6 ns     15,526,980         5.9 ns         5.1 ns
  n4  0.999_9              6.6 ns       111.9 ns      105.4 ns      1,478,478        12.8 ns        12.0 ns
  n5  0.999_99           112.3 ns       142.1 ns       29.8 ns        125,532       119.9 ns       119.1 ns
  n6  0.999_999          142.3 ns     2,265.1 ns    2,122.8 ns         12,814       386.3 ns       385.5 ns
  n7  0.999_999_9      2,267.1 ns     2,449.4 ns      182.3 ns          1,369     2,297.9 ns     2,297.1 ns
  n8  0.999_999_99     2,455.6 ns     6,594.6 ns    4,139.0 ns            130     5,358.1 ns     5,357.3 ns
  n9  0.999_999_999    6,668.3 ns    10,190.8 ns    3,522.6 ns             13     7,158.2 ns     7,157.4 ns
  n10 0.999_999_999_9 27,820.0 ns    27,820.0 ns        0.0 ns              1    27,820.0 ns    27,819.2 ns
  mean                                                                                4.8 ns         4.0 ns
  stdev                                                                               3.8 ns
  mean min..n2                                                                        4.8 ns         4.0 ns
  stdev min..n2                                                                       0.2 ns

zcr-with-1t: zc-ring-x1 reserve_slot_with round-trip (1 thread) [duration=300.0s outer=1,367,394,942 inner=38 calls=51,961,007,796 adj/call=0.78ns labels=both]:
                            first           last         range          count           mean       adjusted
  z4  0.000_1              4.2 ns         4.2 ns        0.0 ns            161         4.2 ns         3.4 ns
  p20 0.20                 4.5 ns         4.5 ns        0.0 ns    360,925,518         4.5 ns         3.7 ns
  p60 0.60                 4.7 ns         4.7 ns        0.0 ns    895,657,195         4.7 ns         4.0 ns
  n2  0.99                 5.0 ns         5.5 ns        0.5 ns    100,429,486         5.1 ns         4.4 ns
  n3  0.999                5.8 ns         6.1 ns        0.3 ns      8,726,583         5.9 ns         5.1 ns
  n4  0.999_9              6.3 ns       103.6 ns       97.3 ns      1,514,195        12.5 ns        11.7 ns
  n5  0.999_99           103.9 ns       162.7 ns       58.8 ns        128,130       110.6 ns       109.9 ns
  n6  0.999_999          162.9 ns     2,117.6 ns    1,954.7 ns         12,254     1,425.4 ns     1,424.6 ns
  n7  0.999_999_9      2,119.7 ns     2,474.0 ns      354.3 ns          1,284     2,177.9 ns     2,177.1 ns
  n8  0.999_999_99     2,476.0 ns     6,615.0 ns    4,139.0 ns            122     5,327.8 ns     5,327.1 ns
  n9  0.999_999_999    6,651.9 ns     9,592.8 ns    2,940.9 ns             13     7,390.1 ns     7,389.3 ns
  n10 0.999_999_999_9 13,434.9 ns    13,434.9 ns        0.0 ns              1    13,434.9 ns    13,434.1 ns
  mean                                                                                4.7 ns         4.0 ns
  stdev                                                                               5.8 ns
  mean min..n2                                                                        4.7 ns         3.9 ns
  stdev min..n2                                                                       0.2 ns

zcr-spin-1t: zc-ring-x1 reserve_slot_spin round-trip (1 thread) [duration=300.0s outer=1,499,844,275 inner=32 calls=47,995,016,800 adj/call=0.84ns labels=both]:
                            first           last         range            count           mean       adjusted
  z4  0.000_1              4.4 ns         4.4 ns        0.0 ns               14         4.4 ns         3.5 ns
  p10 0.10                 4.7 ns         4.7 ns        0.0 ns      291,777,769         4.7 ns         3.9 ns
  p60 0.60                 5.0 ns         5.0 ns        0.0 ns    1,033,313,169         5.0 ns         4.2 ns
  n2  0.99                 5.3 ns         5.6 ns        0.3 ns      159,905,642         5.4 ns         4.6 ns
  n3  0.999                5.9 ns         6.6 ns        0.6 ns       13,575,384         6.2 ns         5.3 ns
  n4  0.999_9              6.9 ns       121.8 ns      114.9 ns        1,124,174        14.6 ns        13.7 ns
  n5  0.999_99           122.1 ns       151.6 ns       29.4 ns          133,236       127.9 ns       127.1 ns
  n6  0.999_999          151.9 ns     2,490.4 ns    2,338.4 ns           13,326       366.4 ns       365.6 ns
  n7  0.999_999_9      2,492.4 ns     2,654.2 ns      161.8 ns            1,412     2,515.2 ns     2,514.3 ns
  n8  0.999_999_99     2,656.3 ns     7,491.6 ns    4,835.3 ns              134     5,650.3 ns     5,649.5 ns
  n9  0.999_999_999    7,495.7 ns    11,804.7 ns    4,309.0 ns               14     8,460.0 ns     8,459.2 ns
  n10 0.999_999_999_9 12,656.6 ns    12,656.6 ns        0.0 ns                1    12,656.6 ns    12,655.8 ns
  mean                                                                                  5.0 ns         4.2 ns
  stdev                                                                                 3.9 ns
  mean min..n2                                                                          5.0 ns         4.2 ns
  stdev min..n2                                                                         0.2 ns

zcr-raw-2t: zc-ring-x1 raw round-trip (2 threads, spin) [duration=300.0s outer=1,674,401,787 inner=1 calls=1,674,401,787 adj/call=11.61ns labels=both]:
                               first              last             range          count              mean          adjusted
  z4  0.000_1                20.0 ns           99.0 ns           79.0 ns         88,909           82.0 ns           70.4 ns
  z3  0.001                 100.0 ns          100.0 ns            0.0 ns      1,360,537          100.0 ns           88.4 ns
  z2  0.01                  109.1 ns          119.0 ns           10.0 ns      6,342,651          110.5 ns           98.9 ns
  p20 0.20                  120.1 ns          120.1 ns            0.0 ns    531,691,241          120.1 ns          108.5 ns
  p40 0.40                  129.0 ns          129.0 ns            0.0 ns    136,846,551          129.0 ns          117.4 ns
  p50 0.50                  130.0 ns          130.0 ns            0.0 ns    185,455,509          130.0 ns          118.4 ns
  p60 0.60                  139.0 ns          140.0 ns            1.0 ns    181,543,208          140.0 ns          128.4 ns
  p80 0.80                  150.0 ns          150.0 ns            0.0 ns    532,605,110          150.0 ns          138.4 ns
  n2  0.99                  160.1 ns          170.1 ns           10.0 ns     92,939,202          166.1 ns          154.5 ns
  n3  0.999                 180.1 ns          200.1 ns           20.0 ns      3,868,138          183.2 ns          171.6 ns
  n4  0.999_9               210.0 ns        3,838.0 ns        3,627.9 ns      1,493,933          583.1 ns          571.5 ns
  n5  0.999_99            3,846.1 ns        4,821.0 ns          974.8 ns        150,073        4,143.1 ns        4,131.5 ns
  n6  0.999_999           4,829.2 ns       14,901.2 ns       10,072.1 ns         15,049        6,307.4 ns        6,295.8 ns
  n7  0.999_999_9        14,909.4 ns      173,801.5 ns      158,892.0 ns          1,509       47,338.7 ns       47,327.1 ns
  n8  0.999_999_99      173,932.5 ns      252,313.6 ns       78,381.1 ns            150      203,502.4 ns      203,490.8 ns
  n9  0.999_999_999     308,019.2 ns    2,005,925.9 ns    1,697,906.7 ns             15      838,598.7 ns      838,587.0 ns
  n10 0.999_999_999_9 2,631,925.8 ns    3,303,014.4 ns      671,088.6 ns              2    2,967,470.1 ns    2,967,458.5 ns
  mean                                                                                           137.1 ns          125.5 ns
  stdev                                                                                          173.9 ns
  mean min..n2                                                                                   136.1 ns          124.5 ns
  stdev min..n2                                                                                   14.4 ns

zcr-with-2t: zc-ring-x1 reserve_slot_with round-trip (2 threads, spin) [duration=300.0s outer=1,939,577,260 inner=1 calls=1,939,577,260 adj/call=11.61ns labels=both]:
                             first            last           range            count            mean        adjusted
  z4  0.000_1              60.0 ns         89.0 ns         29.0 ns          120,087         76.9 ns         65.3 ns
  z3  0.001                90.0 ns         99.0 ns          9.0 ns          337,768         91.1 ns         79.5 ns
  p20 0.20                100.0 ns        100.0 ns          0.0 ns      453,567,412        100.0 ns         88.4 ns
  p40 0.40                109.1 ns        109.1 ns          0.0 ns      270,088,945        109.1 ns         97.4 ns
  p70 0.70                110.0 ns        110.0 ns          0.0 ns    1,088,271,545        110.0 ns         98.4 ns
  n2  0.99                119.0 ns        140.0 ns         21.0 ns      108,409,852        123.6 ns        112.0 ns
  n3  0.999               150.0 ns        180.1 ns         30.1 ns       16,669,309        165.3 ns        153.7 ns
  n4  0.999_9             190.1 ns      3,776.5 ns      3,586.4 ns        1,917,422        464.8 ns        453.2 ns
  n5  0.999_99          3,778.6 ns      4,730.9 ns        952.3 ns          175,615      4,074.0 ns      4,062.4 ns
  n6  0.999_999         4,739.1 ns     12,345.3 ns      7,606.3 ns           17,365      5,735.8 ns      5,724.2 ns
  n7  0.999_999_9      12,353.5 ns    167,247.9 ns    154,894.3 ns            1,748     34,554.8 ns     34,543.2 ns
  n8  0.999_999_99    167,378.9 ns    220,463.1 ns     53,084.2 ns              173    193,322.9 ns    193,311.3 ns
  n9  0.999_999_999   221,773.8 ns    385,351.7 ns    163,577.9 ns               17    265,320.6 ns    265,309.0 ns
  n10 0.999_999_999_9 385,876.0 ns    553,123.8 ns    167,247.9 ns                2    469,499.9 ns    469,488.3 ns
  mean                                                                                     109.6 ns         97.9 ns
  stdev                                                                                     95.9 ns
  mean min..n2                                                                             108.3 ns         96.7 ns
  stdev min..n2                                                                              5.8 ns

zcr-spin-2t: zc-ring-x1 reserve_slot_spin round-trip (2 threads, spin) [duration=300.0s outer=1,943,706,240 inner=1 calls=1,943,706,240 adj/call=11.61ns labels=both]:
                             first            last           range            count            mean        adjusted
  z4  0.000_1              60.0 ns         90.0 ns         30.0 ns          233,489         88.2 ns         76.6 ns
  z3  0.001                99.0 ns         99.0 ns          0.0 ns           28,303         99.0 ns         87.4 ns
  p20 0.20                100.0 ns        100.0 ns          0.0 ns      430,216,244        100.0 ns         88.4 ns
  p40 0.40                109.1 ns        109.1 ns          0.0 ns      311,656,855        109.1 ns         97.4 ns
  p70 0.70                110.0 ns        110.0 ns          0.0 ns    1,114,601,304        110.0 ns         98.4 ns
  n2  0.99                119.0 ns        140.0 ns         21.0 ns       71,119,124        123.7 ns        112.1 ns
  n3  0.999               150.0 ns        180.1 ns         30.1 ns       14,150,068        164.5 ns        152.9 ns
  n4  0.999_9             190.1 ns      3,778.6 ns      3,588.5 ns        1,506,594        477.6 ns        466.0 ns
  n5  0.999_99          3,786.8 ns      4,780.0 ns        993.3 ns          174,678      4,106.4 ns      4,094.8 ns
  n6  0.999_999         4,788.2 ns     12,656.6 ns      7,868.4 ns           17,637      5,804.5 ns      5,792.9 ns
  n7  0.999_999_9      12,664.8 ns    168,165.4 ns    155,500.5 ns            1,749     33,716.1 ns     33,704.5 ns
  n8  0.999_999_99    168,296.4 ns    221,904.9 ns     53,608.4 ns              176    192,789.0 ns    192,777.4 ns
  n9  0.999_999_999   223,477.8 ns    385,351.7 ns    161,873.9 ns               17    264,634.4 ns    264,622.8 ns
  n10 0.999_999_999_9 385,613.8 ns    391,381.0 ns      5,767.2 ns                2    388,497.4 ns    388,485.8 ns
  mean                                                                                     109.3 ns         97.6 ns
  stdev                                                                                     94.9 ns
  mean min..n2                                                                             108.1 ns         96.5 ns
  stdev min..n2                                                                              5.2 ns

wink@3900x 26-07-06T15:43:41.979Z:~/data/prgs/rust/iiac-perf (main+1)
```

## Testing

- `cargo test` — the full suite, including a threaded stress
  test and a u32-index-wrap test.
- `cargo run --example readme` — the Overview example above
  (committed as [examples/readme.rs](examples/readme.rs) so
  `cargo clippy --all-targets` keeps it compiling against the
  real API).
- `cargo run --example pool_readme` — the Message pool
  example above
  ([examples/pool_readme.rs](examples/pool_readme.rs), same
  convention).
- `cargo run --release` — the demo binary
  ([src/bin/zc-ring-x1-demo.rs](src/bin/zc-ring-x1-demo.rs)):
  a throughput scoreboard (msgs/sec and ns/msg) grouped so
  like compares with like — alloc/free baselines (pool vs
  global allocator), then three message flows (raw ring,
  composed ring + pool descriptors, std channel + pool) at
  each thread placement: single thread on core 0, two
  threads unpinned, two SMT siblings sharing one physical
  core, and two different physical cores (pairs discovered
  from /sys at runtime; each line names the cpus it ran
  on). Eyeball numbers — single runs, no mean/stdev — not a
  benchmark (calibrated measurement lives in iiac-perf).
  Installable: `cargo install --path . --locked`, then
  `zc-ring-x1-demo`; `-V` prints the version-of-record so
  you know which build you are testing. An example run:

  ```text
  $ zc-ring-x1-demo
  zc-ring-x1 0.7.0
  demo: 1,000,000 messages each, depth 64
  pool_alloc_free_1t (core 0):                 102,119,813 msgs/sec      9.8 ns/msg
  global_alloc_free_1t (core 0):               148,147,577 msgs/sec      6.8 ns/msg

  spsc_ring_one_msg_1t (core 0):               388,935,863 msgs/sec      2.6 ns/msg
  spsc_ring_one_pool_msg_1t (core 0):          132,426,985 msgs/sec      7.6 ns/msg
  std_mpsc_one_pool_msg_1t (core 0):            28,185,250 msgs/sec     35.5 ns/msg

  spsc_ring_one_msg_2t (unpinned):              20,647,303 msgs/sec     48.4 ns/msg
  spsc_ring_one_pool_msg_2t (unpinned):          6,004,837 msgs/sec    166.5 ns/msg
  std_mpsc_one_pool_msg_2t (unpinned):           3,859,666 msgs/sec    259.1 ns/msg

  spsc_ring_one_msg_2t (same core 0+12):       133,307,240 msgs/sec      7.5 ns/msg
  spsc_ring_one_pool_msg_2t (same core 0+12):   26,101,551 msgs/sec     38.3 ns/msg
  std_mpsc_one_pool_msg_2t (same core 0+12):    17,164,074 msgs/sec     58.3 ns/msg

  spsc_ring_one_msg_2t (diff cores 0+3):         5,309,068 msgs/sec    188.4 ns/msg
  spsc_ring_one_pool_msg_2t (diff cores 0+3):    3,222,906 msgs/sec    310.3 ns/msg
  std_mpsc_one_pool_msg_2t (diff cores 0+3):     3,258,778 msgs/sec    306.9 ns/msg
  ```
- `cargo +nightly miri test` — the full suite under
  [Miri](https://github.com/rust-lang/miri), which checks the
  `unsafe` code against the memory model; it has caught two
  real Stacked Borrows bugs here (an init retag in 0.3.0-4, a
  guard aliasing race in 0.3.0-6 — the latter only visible
  with the threaded test included). One-time setup:
  `rustup component add --toolchain nightly miri`. The
  threaded stress test runs a reduced message count under
  Miri (interpreted spin loops are slow).

## Releasing

**`cargo install` lockfile gotcha.** If you install your project's
binary with `cargo install --path .`, also pass `--locked`. By
default `cargo install` ignores `Cargo.lock` and re-resolves
dependencies from scratch, so it can silently build the binary from
a different — possibly broken — dependency graph than `cargo build`
/ `cargo test` ran against.

| command | reads `Cargo.lock`? | re-resolves? |
| --- | --- | --- |
| `cargo build` / `test` / `run`  | yes     | only if `Cargo.toml` changed |
| `cargo build --locked` (etc.)   | yes     | never; errors if it would need to |
| `cargo install` (default)       | **no**  | always, from scratch |
| `cargo install --locked`        | yes     | never; errors if it would need to |

There's no stable cargo config knob to make `--locked` the default
for `cargo install`, so the discipline lives in the per-commit cargo
cycle and shell aliases (e.g. `alias ci='cargo install --locked'`).

The trade-off is predictable-as-tested (`--locked`) vs picking up
upstream fixes via fresh resolves (default). Either is defensible —
pick what fits your project, then edit this section and the
per-commit cargo cycle to match. Background:
[cargo#7169](https://github.com/rust-lang/cargo/issues/7169),
[cargo#9436](https://github.com/rust-lang/cargo/issues/9436).

## jj Tips for Git Users

This project uses [Jujutsu (jj)](https://docs.jj-vcs.dev/latest/)
alongside git. New to jj? See
[Steve Klabnik](https://github.com/steveklabnik)'s
[Jujutsu tutorial](https://steveklabnik.github.io/jujutsu-tutorial).

Repo-specific how-tos — initial commit, pushing, modifying and
force-pushing a commit, revsets, and a useful-commands reference —
live in [notes/jj-tips.md](notes/jj-tips.md).

## Cross-repo Linking with Git Trailers

Commits in each repo use [git trailers](https://git-scm.com/docs/git-interpret-trailers)
to cross-reference their counterpart in the other repo via an
`ochid` (Other Change ID) trailer — the defining mechanism of the
dual-repo convention. For the full definition — trailer syntax,
the example shape, per-commit mechanics, and `.vc-config.toml` —
see
[Cross-repo linking (ochid trailers)](AGENTS.md#cross-repo-linking-ochid-trailers).

## Contributing

Bot-following workflow, commit conventions, and code style are
canonical in [AGENTS.md](AGENTS.md) (which `CLAUDE.md` imports)
and [notes/cycle-protocol.md](notes/cycle-protocol.md):

- [Cycle numbering](notes/cycle-protocol.md#numbering) — the
  `X.Y.Z-N` phase-suffix convention (Preparation / Work /
  Close-out).
- [Commit description](notes/cycle-protocol.md#commit-description)
  — Conventional Commits title rules (no version suffix,
  distinct per step) and per-repo body shape.
- [Per-commit flow](notes/cycle-protocol.md#per-commit-flow) —
  the cargo cycle plus the Work and Commit-Description review
  gates.
- [Code Conventions](AGENTS.md#code-conventions) — doc comments on
  every file / fn / method, `// OK: …` on `unwrap*` calls,
  ask-on-ambiguity, stuck detection.

Task tracking and release details live under [notes/](notes/):
near-term tasks in [notes/todo.md](notes/todo.md), per-release
details in `notes/chores/chores-*.md`, and notes-specific
formatting rules in [notes/README.md](notes/README.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
