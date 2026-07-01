# Versioning

How this project versions its commits and the running
artifact. The scheme — the cycle step-numbering and the
unique-per-commit aim — is generic and shared across projects:
this file is copied **verbatim**, with
[Recording the version-of-record](#recording-the-version-of-record)
covering each medium by conditional rather than per-project
edits.

## Terms

Three names, used as defined here across
[AGENTS.md](../AGENTS.md),
[cycle-protocol.md](cycle-protocol.md), and the notes files:

- **version** — the per-commit version (e.g. `0.3.0-5.3.0`),
  written with `-` in prose, ladders, chores, todo, and commit
  talk; its suffix encodes the cycle phase (see
  [Step numbering](#step-numbering)).
- **version-of-record** — the authoritative stored copy of the
  version, in the project's manifest (see
  [Recording the version-of-record](#recording-the-version-of-record));
  a built or running artifact derives from it.
- **versioning** — the topic: this scheme as a whole.

## Recording the version-of-record

Where the version-of-record lives, how it's stored and
surfaced, and how often it changes — pick the case that fits
your medium:

- **Manifest** — where the version-of-record is stored:
  - if Rust, `Cargo.toml` `[package].version`
  - if Python, `pyproject.toml` `[project].version` (or the
    committed config it's sourced from)
  - otherwise wherever the medium records it (a generic
    `version.toml`, a book's frontmatter, …) — add the case as
    needed
- **Notation** — how the `-` form is stored:
  - if the format allows `-` (TOML `version.toml`, `Cargo.toml`),
    store it verbatim
  - if it bars `-` (PEP 440's local segment, e.g. a Python
    project), remap to `+` — `0.3.0-5.3.0` → `0.3.0+5.3.0`: same
    version, just the stored spelling
- **Reporter** — how a built artifact surfaces the
  version-of-record:
  - if a CLI app, `<cli-app> -V`
  - if a TUI/GUI, add to Help/About or display on the title
- **Cadence** — how often to bump: see
  [Unique per commit](#unique-per-commit-preference-not-requirement);
  this project follows the per-commit preference.

## Unique per commit (preference, not requirement)

Our general notion is that the version-of-record should change
on **every commit**, so a built or running artifact identifies
the exact commit it came from.

- This is a preference, **not** a hard requirement. A project
  following the cycle protocol may bump less often — once per
  cycle, only at release, and so on. Record the choice in
  [Recording the version-of-record](#recording-the-version-of-record)
  if it differs.
- It is achievable because the cycle's versions (below) are
  **pre-assignable**, unlike the git SHA, which a commit cannot
  contain (see the cycle protocol's
  [Commits backfill](cycle-protocol.md#commits-backfill)).

## Step numbering

The cycle (Preparation -> Work -> Close-out; see
[cycle-protocol.md](cycle-protocol.md)) encodes each commit's
phase in the version suffix — the **final identifier `0` marks
a Preparation**:

- `X.Y.Z-0` — Preparation
- `X.Y.Z-1`, `X.Y.Z-2`, … — Work commits
- `X.Y.Z` — Close-out (bare version, no suffix)

**Preparation is optional.** A lightweight cycle — no ladder,
no setup commit — skips `-0` and starts at `-1` (its first
Work commit). The same holds at every level: a sub-cycle
needing no Preparation omits its `.0` (see Nesting). One that
grows a Preparation later adds the `0` step without
renumbering siblings.

Disambiguation:

- `-10` — Work commit #10 (final identifier `10`), not a
  Preparation.
- `-1.0` — Preparation of the `-1` sub-cycle (final
  identifier `0`).

**Nesting.** Sub-cycles append another level, recursively:

- `X.Y.Z-3.0` — Preparation of the `-3` sub-cycle
- `X.Y.Z-3.1`, `X.Y.Z-3.2` — its Work
- `X.Y.Z-3` — its Close-out
- `X.Y.Z-3.1.0` — Preparation of the `-3.1` sub-sub-cycle

Bump the version-of-record at the start of each phase, so the
active phase is recorded — and, per the preference above,
every commit carries a distinct version.
