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
wink@3900x 26-07-06T14:50:58.972Z:~/data/prgs/rust/iiac-perf (main+1)
$ iiac-perf -d 300 zcr
iiac-perf 0.16.0 — Rust latency microbenchmark harness

Calibration:
  framing/sample      11.12 ns  (timer pair, two-point fit)
  loop/iter            0.49 ns  (per inner-loop iteration)
  cal pin           core 0 (unpinned after cal; --no-pin-cal to skip)
  bench pin         none (unpinned)
  sleep inhibit     active (systemd-inhibit --what=sleep)
  config            none (built-in defaults)

zcr-with-1t: zc-ring-x1 reserve_slot_with round-trip (1 thread) [duration=300.0s outer=2,064,159,039 inner=40 calls=82,566,361,560 adj/call=0.77ns labels=both]:
                            first           last         range            count           mean       adjusted
  z3  0.001                2.3 ns         2.3 ns        0.0 ns          817,487         2.3 ns         1.5 ns
  z2  0.01                 2.5 ns         2.5 ns        0.0 ns       30,371,404         2.5 ns         1.7 ns
  p30 0.30                 2.5 ns         2.5 ns        0.0 ns    1,080,749,327         2.5 ns         1.7 ns
  p60 0.60                 2.7 ns         2.7 ns        0.0 ns      249,945,275         2.7 ns         2.0 ns
  p80 0.80                 2.8 ns         2.8 ns        0.0 ns      484,832,768         2.8 ns         2.0 ns
  n2  0.99                 3.0 ns         3.3 ns        0.3 ns      206,929,047         3.0 ns         2.3 ns
  n3  0.999                3.5 ns         3.8 ns        0.3 ns        9,206,831         3.6 ns         2.8 ns
  n4  0.999_9              4.0 ns         9.0 ns        5.0 ns        1,100,438         4.2 ns         3.4 ns
  n5  0.999_99             9.3 ns       116.2 ns      107.0 ns          185,914        93.8 ns        93.1 ns
  n6  0.999_999          116.5 ns     1,970.2 ns    1,853.7 ns           18,497       172.7 ns       172.0 ns
  n7  0.999_999_9      1,971.2 ns     2,097.2 ns      126.0 ns            1,844     1,993.1 ns     1,992.4 ns
  n8  0.999_999_99     2,099.2 ns     5,603.3 ns    3,504.1 ns              186     3,658.0 ns     3,657.3 ns
  n9  0.999_999_999    5,611.5 ns    10,076.2 ns    4,464.6 ns               19     6,219.5 ns     6,218.7 ns
  n10 0.999_999_999_9 10,641.4 ns    10,911.7 ns      270.3 ns                2    10,776.6 ns    10,775.8 ns
  mean                                                                                  2.7 ns         1.9 ns
  stdev                                                                                 2.6 ns
  mean min..n2                                                                          2.6 ns         1.9 ns
  stdev min..n2                                                                         0.2 ns

zcr-with-2t: zc-ring-x1 reserve_slot_with round-trip (2 threads, spin) [duration=300.0s outer=1,942,197,324 inner=1 calls=1,942,197,324 adj/call=11.61ns labels=both]:
                               first              last             range          count              mean          adjusted
  z4  0.000_1                20.0 ns           40.0 ns           20.0 ns        197,058           34.8 ns           23.2 ns
  z3  0.001                  50.0 ns           90.0 ns           40.0 ns      1,705,182           76.2 ns           64.6 ns
  z2  0.01                   99.0 ns           99.0 ns            0.0 ns        525,426           99.0 ns           87.4 ns
  p20 0.20                  100.0 ns          100.0 ns            0.0 ns    734,468,749          100.0 ns           88.4 ns
  p50 0.50                  109.1 ns          109.1 ns            0.0 ns    300,842,854          109.1 ns           97.4 ns
  p80 0.80                  110.0 ns          110.0 ns            0.0 ns    769,394,633          110.0 ns           98.4 ns
  n2  0.99                  119.0 ns          140.0 ns           21.0 ns    112,045,448          125.7 ns          114.1 ns
  n3  0.999                 150.0 ns          530.4 ns          380.4 ns     21,078,443          209.5 ns          197.9 ns
  n4  0.999_9               540.2 ns        3,727.4 ns        3,187.2 ns      1,745,782          734.7 ns          723.1 ns
  n5  0.999_99            3,737.6 ns        5,623.8 ns        1,886.2 ns        174,336        4,173.8 ns        4,162.2 ns
  n6  0.999_999           5,632.0 ns       81,395.7 ns       75,763.7 ns         17,474       30,586.4 ns       30,574.8 ns
  n7  0.999_999_9        81,461.2 ns      217,317.4 ns      135,856.1 ns          1,745       98,674.0 ns       98,662.4 ns
  n8  0.999_999_99      217,448.4 ns      996,671.5 ns      779,223.0 ns            175      379,072.2 ns      379,060.6 ns
  n9  0.999_999_999   1,004,535.8 ns    3,007,316.0 ns    2,002,780.2 ns             17    1,837,907.0 ns    1,837,895.4 ns
  n10 0.999_999_999_9 3,978,297.3 ns    4,005,560.3 ns       27,263.0 ns              2    3,991,928.8 ns    3,991,917.2 ns
  mean                                                                                           109.3 ns           97.7 ns
  stdev                                                                                          309.0 ns
  mean min..n2                                                                                   106.9 ns           95.3 ns
  stdev min..n2                                                                                    6.9 ns

wink@3900x 26-07-06T18:39:49.593Z:~/data/prgs/rust/iiac-perf (main+1)
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
