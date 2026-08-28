# Code conventions

Conventions for source code. Read this before writing code. The Rust-specific sections apply
wherever the medium is a Rust crate (see [custom.md](../custom.md) for the project's medium).

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## Line width

Source lines, including doc comments and inline comments, wrap at the source width in prose.md's
[Line widths](prose.md#line-widths), which matches rustfmt's default `max_width`. The per-commit
flow's `cargo fmt` enforces it for code, and the same limit applies to the comment text it doesn't
reflow.

## Comments are prose

Doc comments and inline comments are prose and follow prose.md, its
[Semicolons](prose.md#semicolons) rule included: a commit that edits a source file converts that
file's comment semicolons in the same commit, whole file, code spans and the code itself exempt,
using the prose rule's joins (a period, a comma with a conjunction, or a restructure). Files not in
the commit's diff are left alone, since converting them is a sweep and sweeps are their own cycle.

## Doc comments on every file, function, and method

Every `.rs` file must begin with a `//!` module docstring. Every function and method must have a
`///` doc comment. Keep them brief: one sentence of purpose is often enough. The discipline is that
the comment exists, not that it be long. Doc comments follow the [Prose form](prose.md#prose-form)
shape (intro + bullets).

This is a deliberate override of the generic "write no comments" default that applies to inline `//`
comments. Doc comments on the module / item surface are expected, while inline explanatory comments
inside function bodies remain discouraged unless they capture a non-obvious WHY.

**Clap-derive args:** doc comments on `#[arg(...)]` fields drive `--help` output. Clap reflows by
default and collapses bullets into running prose. Add `#[arg(verbatim_doc_comment, ...)]` on any
field whose doc comment uses bullets so each `- ...` lands on its own line in the rendered help.

## `// OK: ...` comments on `unwrap*` calls (Rust)

([why](rationale.md#-ok--comments-on-unwrap-calls-rust))

The default in non-test code is to **not** use `.unwrap()`, `.expect(...)`, or the
`.unwrap_or*(...)` siblings. Prefer a shape that doesn't need them (`match`, `if let`, slice
patterns, infallible-by-construction APIs), which is usually also the clearer code. This is a lean,
not a ban: some sites are legitimately best expressed with one. Two kinds of risk, both in scope:

- `.unwrap()` / `.expect(...)`: a panic path.
- `.unwrap_or(...)` / `.unwrap_or_default()` / `.unwrap_or_else(...)`: no panic, but a silently
  substituted value that can hide a wrong result.

When one is used, three obligations attach:

- A trailing `// OK: ...` comment justifying why the call is acceptable:
  - `// OK: <specific reason>`: document the real precondition, invariant, or domain reason.
    Preferred whenever the reason isn't self-evident.
  - `// OK: obvious`: the default is self-evident from context (e.g.
    `desc.lines().next().unwrap_or("")`, where an empty desc gives an empty title).
  - Bare `// OK` is not used (reads like a truncated comment). Abbreviations (e.g. `SE`) are not
    used because they require a decoder ring for readers seeing the code out of context.
- **Alert the user in conversation** when introducing one, so the site gets reviewed and appropriate
  uses are learned. Don't let it ride in silently on a larger diff.
- For `.unwrap()` / `.expect(...)`, an `#[allow(...)]` at the site, because Rust projects enable the
  project-wide lints in `Cargo.toml`:

  ```toml
  [lints.clippy]
  unwrap_used = "warn"
  expect_used = "warn"
  ```

  Every panicking site is then opt-in and visible in the diff, and clippy (in the per-commit flow)
  catches any that slip through. The `_or*` siblings have no clippy lint. They are covered by the
  comment convention and the conversational alert. (The template repository's `CargoRust.toml` seeds
  a base `Cargo.toml` with this section already in place.)

```rust
let max = stderr_level.unwrap_or(LevelFilter::Info); // OK: default verbosity when -v/-vv absent
let first_line = desc.lines().next().unwrap_or("");  // OK: obvious

match matches.len() {
    1 => {
        #[allow(clippy::unwrap_used)]
        // OK: `1 =>` arm guarantees matches.len() == 1
        Ok(TitleMatch::One(matches.into_iter().next().unwrap()))
    }
    // ...
}
```

Tests (`#[cfg(test)]`) are exempt, since panicking on setup failure is the correct test behavior.
