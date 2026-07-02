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

// Cache-line-aligned region: 192 B header + 4 slots × 64 B.
#[repr(C, align(64))]
struct Region([u8; 448]);
let mut region = Region([0; 448]);

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

## Testing

- `cargo test` — the full suite, including a threaded stress
  test and a u32-index-wrap test.
- `cargo run --example readme` — the Overview example above
  (committed as [examples/readme.rs](examples/readme.rs) so
  `cargo clippy --all-targets` keeps it compiling against the
  real API).
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
