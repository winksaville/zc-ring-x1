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
- Guard API: `reserve_slot::<T>()` on either endpoint — write →
  `commit()` on the producer side, read → `release()` on the
  consumer side; dropping a guard abandons cleanly.
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

let mut slot = producer.reserve_slot::<Msg>().unwrap();
slot.seq = 1;
slot.val = 42;
slot.commit(); // publish to the consumer

let msg = consumer.reserve_slot::<Msg>().unwrap();
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
let mut slot = producer.reserve_slot::<Desc>()?;
*slot = desc;                         // 8 bytes, not the payload
slot.commit();

// Receiver: ring consumer + freer (another thread).
let slot = consumer.reserve_slot::<Desc>()?;
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
