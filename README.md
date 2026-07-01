# zc-ring-x1

This is the main repo of a dual-repo convention for using
a bot to help develop a project — code, but equally prose, an
image, a song, a screenplay, anything the bot generates from a
conversation. The goal is that this main repo contains the
"what" (the artifact), while the partner bot repo contains the
"why" and "how" (the conversation). The key to the convention is
each change is cross-referenced to the other. Thus there is a
coherent story of the development of the project across time.

The beginnings of that tool is [vc-x1](https://github.com/winksaville/vc-x1)
which currently does achieve this goal, but is being used as a
first test bed.

## This is a template — customize freely

`CLAUDE.md` and this `README.md` are just starting points.
Read both and edit either to match the conventions you want for your
project. In particular, decisions like the `cargo install` behavior
(default re-resolve vs `--locked`) under [Releasing](#releasing) are
yours to make — pick what fits and update the docs (and the
per-commit cargo cycle) so the choice is recorded for future readers.

## Assumptions

- **The convention is medium-agnostic.** The artifact can be
  code, prose, an image, a song, a screenplay — anything the bot
  generates from a conversation. The docs use a Rust crate as the
  running example (the per-commit cargo cycle, `Cargo.toml`
  versioning); substitute your medium's equivalents. This repo
  itself ships docs and notes only — no build system — so its
  version lives in [`version.toml`](version.toml) and it skips
  the cargo cycle.
- **The `vc-x1` companion tool.** Steps that invoke `vc-x1`
  (`fix-todo`, `validate-todo`, `chid`, `push`, `finalize`) need
  [vc-x1](https://github.com/winksaville/vc-x1) installed; the
  underlying jj steps are documented so the flow also works
  without it.

## Cloning

**TBD.** Earlier text here described the bot session repo as a git
submodule; that is stale — it is a standalone repo, cloned with
`jj git clone`, not a submodule. Bootstrapping a new project from a
template is moving to `vc-x1 init <template-url>`, which reads a
`.init-file-list.txt` from the template to lay down the generic
files. This section will be rewritten once that lands.

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
  — Conventional Commits + `(version)` suffix and per-repo body
  shape.
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
