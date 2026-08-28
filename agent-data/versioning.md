# Versioning

How this project versions its commits and the running artifact. The scheme, meaning the cycle suffix
spelling and the unique-per-commit aim, is generic and shared across projects: this file is copied
**verbatim**, with [Recording the version-of-record](#recording-the-version-of-record) covering each
medium by conditional rather than per-project edits.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## Terms

Three names, used as defined here across [AGENTS.md](../AGENTS.md) and the notes files:

- **version**: the per-commit version (e.g. `0.3.0-5.3.0`). It lives in the manifest. No ladder,
  todo entry, commit title, or commit body writes one (see [Versions live in the version-of-record
  only](prose.md#versions-live-in-the-version-of-record-only)), and its suffix encodes the cycle
  phase for whoever inspects the manifest (see [Suffix scheme](#suffix-scheme)).
- **version-of-record**: the authoritative stored copy of the version, in the project's manifest
  (see [Recording the version-of-record](#recording-the-version-of-record)). A built or running
  artifact derives from it.
- **versioning**: the topic, this scheme as a whole.

## A stamp, not a name

The version answers "which commit produced this artifact". It is not a name for a step: a step is
named by its title and located by its position in the ladder list (see [Steps are named, not
numbered](prose.md#steps-are-named-not-numbered)). So the suffix below is the only number in the
system, and since nothing dereferences it, reordering or inserting a step leaves the versions
already committed alone.

## Advancing X.Y.Z: patch by default

Which of the three numbers moves is not a classification problem
([why](rationale.md#advancing-xyz-patch-by-default)): **patch by default, minor by the user's call,
major by the project's promise.**

- **Patch by default**: every cycle advances `Z`, whatever it touched. Code or docs, large or small,
  none of it matters, so the choice costs no deliberation.
- **Minor is deliberate and rare**: the user names it at a cycle's opening when the change deserves
  a visible marker in the history, and the agent never infers it from a change's content or size.
- **Major is a project's own call**, since what `X` promises depends on the artifact and its users.
  A project that makes that promise records it as its local edit of this rule, and absent one the
  default above governs.

## Grammar and storage

One spelling everywhere the version is written:

```
<public>[-<suffix>]
```

- `<public>`: `X.Y.Z`, integers.
- `<suffix>`: dot-separated identifiers, each ASCII letters or digits: usually integers (`3`, `3.1`,
  `3.1.0`), with an alphanumeric id (`3.hotfix`) allowed sparingly.
- **Exactly one `-` in the whole version**, the one that opens the suffix: never a dash inside the
  suffix, never a `+` in this spelling. This is the portability invariant that makes the version
  storable in every medium below ([why](rationale.md#grammar-and-storage)).
- **`v` is a display prefix, not part of the version**: conversation and reports may write
  `v0.78.0-3.1` for scannability, while manifests store the bare form. (PEP 440 ignores a leading
  `v`. Cargo rejects one.)

Storage is a per-medium remap of that one spelling:

- **SemVer mediums** (Rust/Cargo): store verbatim. The suffix rides in the prerelease slot, valid at
  any dot depth.
- **PEP 440 mediums** (Python): remap the single `-` to `+`: `0.78.0-3.1` -> `0.78.0+3.1`.
  Mechanical and bijective because there is exactly one dash to find.
- **Other mediums**: verbatim if the format allows the one `-`, else the `+` remap. A new medium
  adds its case to [Recording the version-of-record](#recording-the-version-of-record).

Two reservations keep the remap sound:

- **The stored version identifies but does not order.** SemVer sorts a suffixed version *before* its
  bare release (matching cycle semantics: rungs precede close-out), while PEP 440 sorts the remapped
  form *after* it, and reinterprets a lone `-N` as a post-release. Opposite directions, so no
  cross-medium logic may compare stored versions. Ordering truth lives in the ladder and git
  history. Comparing the public triple alone (e.g. a version gate) is unaffected.
- **`+` is reserved** for the PEP 440 remap: no SemVer build-metadata use in Rust repos even though
  Cargo allows it, since spending `+` there breaks the bijection with the Python spelling. A repo
  that truly needs it declares the deviation in its `custom.md`.

## Recording the version-of-record

Where the version-of-record lives, how it's stored and surfaced, and how often it changes. Pick the
case that fits your medium:

- **Manifest**, where the version-of-record is stored:
  - if Rust, `Cargo.toml` `[package].version`
  - if Python, `pyproject.toml` `[project].version` (or the committed config it's sourced from)
  - otherwise wherever the medium records it (a generic `version.toml`, a book's frontmatter, ...).
    Add the case as needed
- **Notation**, how the `-` form is stored. See [Grammar and storage](#grammar-and-storage):
  - if the format allows `-` (TOML `version.toml`, `Cargo.toml`), store it verbatim
  - if it bars `-` (PEP 440's local segment, e.g. a Python project), remap to `+`, so `0.3.0-5.3.0`
    becomes `0.3.0+5.3.0`: same version, just the stored spelling
- **Reporter**, how a built artifact surfaces the version-of-record:
  - if a CLI app, `<cli-app> -V`
  - if a TUI/GUI, add to Help/About or display on the title
- **Cadence**, how often to bump: see [Unique per
  commit](#unique-per-commit-preference-not-requirement). This project follows the per-commit
  preference.

## Dev artifact name

When other projects consume the built artifact (e.g. the installed CLI) while this repo is under
active development, the dev build installs under a separate name so a mid-cycle install never
clobbers the binary consumers are running:

- **Name**: the manifest's package name carries a `-dev` suffix (`<name>` -> `<name>-dev`). If Rust,
  `[package].name`, so the per-commit flow's `cargo install` produces `<name>-dev` and leaves plain
  `<name>` untouched.
- **Constant, not per-step**: the step already lives in the version-of-record (`<name>-dev -V`
  reports the exact rung). A per-step name would churn the manifest every commit and litter the
  install dir with stale binaries.
- **Promotion**: plain `<name>` updates only by an explicit act: a separate clone built at the
  chosen commit with the plain name (or a copy of the dev binary), never by the per-commit flow's
  install.

**Single-name variant**: a repo striving to release makes the package name the binary's name, plain
`<name>` on `main` only:

- **The opening renames**: its bump sets `<name>-dev` beside the suffixed version, and every rung
  and the closing build under the dev name, the closing over the bare version.
- **Land restores**: the rename back to `<name>` is Land's act, so the stable name is only ever
  installed from what `main` will carry.
- **A build script guards the pairing** on every cargo verb: a suffixed version under the stable
  name fails the build, so no mid-cycle install can clobber the consumers' binary.

## Unique per commit (preference, not requirement)

Our general notion is that the version-of-record should change on **every commit**, so a built or
running artifact identifies the exact commit it came from.

- This is a preference, **not** a hard requirement. A project following the cycle protocol may bump
  less often: once per cycle, only at release, and so on. Record the choice in [Recording the
  version-of-record](#recording-the-version-of-record) if it differs.
- It is achievable because the cycle's versions (below) are **pre-assignable**, unlike the git SHA,
  which a commit cannot contain.

## Suffix scheme

The cycle (opening -> commits -> closing, per the [Cycle protocol](../AGENTS.md#cycle-protocol))
encodes each commit's phase in the version suffix, the **final identifier `0` marking a
Preparation**.

This is the manifest's own spelling, read by whoever inspects `Cargo.toml` or `-V` output. It is not
a name for a step: a step is identified by its title, and an in-flight ladder rung carries neither a
number nor a version (see [Steps are named, not numbered](prose.md#steps-are-named-not-numbered)).
The identifiers below count commits within a phase, and nothing dereferences one.

- `X.Y.Z-0`: Preparation
- `X.Y.Z-1`, `X.Y.Z-2`, ...: the commits between
- `X.Y.Z`: Close-out (bare version, no suffix)

**Preparation is optional.** A lightweight cycle, with no ladder and no setup commit, skips `-0` and
starts at `-1` (its first commit). A single-step cycle is one commit, the Close-out, and carries the
bare `X.Y.Z` ([Cycle shape](../AGENTS.md#cycle-shape)). The same holds at every level: a sub-cycle
needing no Preparation omits its `.0` (see Nesting). One that grows a Preparation later adds the `0`
step without renumbering siblings.

Disambiguation:

- `-10`: commit #10 (final identifier `10`), not a Preparation.
- `-1.0`: Preparation of the `-1` sub-cycle (final identifier `0`).

**Nesting.** Sub-cycles append another level, recursively:

- `X.Y.Z-3.0`: Preparation of the `-3` sub-cycle
- `X.Y.Z-3.1`, `X.Y.Z-3.2`: its commits between
- `X.Y.Z-3`: its Close-out
- `X.Y.Z-3.1.0`: Preparation of the `-3.1` sub-sub-cycle

Bump the version-of-record at the start of every step (the per-commit checklist carries the step),
so the manifest always records the commit it will be part of and, per the preference above, every
commit carries a distinct version.
