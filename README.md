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
- Guard API: `reserve_slot_with::<T>(policy)` on either
  endpoint — write → `commit()` on the producer side, read →
  `release()` on the consumer side; dropping a guard abandons
  cleanly.
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
