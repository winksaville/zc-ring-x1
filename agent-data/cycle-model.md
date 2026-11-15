# Cycle model

A cycle's record shown as specimens rather than skeletons: the `config loader` cycle, a ladder
mid-run, and the `version flag` cycle, a single-step cycle after its closing. Copy the shape, not
the words. The rules are in [The In Progress block](notes.md#the-in-progress-block) and [Cycle
shape](../AGENTS.md#cycle-shape).

Who writes what:

- `vc-x1 cycle --open --title` writes the scaffold: the title, the five item headings, the `##
  Todo` entry's text under Problem, the opening and closing rung subsections, and the ladder of
  the two in `zTOC.md`.
- The user and the agent write the rest, the planning being theirs: the items, and each work rung
  as a `#####` heading under `#### Rungs` with its intent, between the bookends.
- The tool makes the ladder from those headings, `cycle --ladder` and every cycle commit, each
  rung's number its position and each marker computed from the commits, so no marker is typed.
- `cycle --close` moves the block to `## Closed`, a single-step cycle's without its `#### Rungs`.

## A ladder, mid-run

The `config loader` cycle with its opening committed and its one work rung current, `TODO.md >
## In Progress`:

```
## In Progress

### refactor: extract config loader

#### Problem

Config parsing lives inside the CLI entry point, so every subcommand that wants a setting reaches
through `main` for it, and a test cannot load a config without building the CLI.

#### Solution

Move config loading into its own module with a single `load` entry point that the CLI and the tests
call alike.

#### Acceptance check

`cargo test` passes with a new test that loads a config from a fixture file without touching `main`,
and `vc-x1 validate-config` reports the same findings as before the move.

#### Deliberation

- One work rung, not two: the parser and the loader were going to be split first and moved second,
  but the move is what forces the split, so doing them together is fewer edits.
- The acceptance check compares `validate-config`'s output rather than asserting on internals, so a
  later rename inside the module does not invalidate it.

#### Rungs

The rung subsections, in the ladder's order. The ladder itself is in `zTOC.md > ## Ladder`.

##### refactor: extract config loader opening

The cycle's first commit, carrying its plan: the bookmark, the last cycle's record deleted, and the
Todo entry moved into this block.

##### refactor: split loader from parser

Loading and parsing are one function, so the CLI cannot load without parsing and a test cannot parse
without a file on disk.

##### refactor: extract config loader closing

Closing out the cycle.
```

The opening and closing subsections are the scaffold's, and the work rung's is the planner's,
written before its commit as an intent of a sentence or two. At the rung's commit the intent gains
its conceptual delta:

```
##### refactor: split loader from parser

Loading and parsing are one function, so the CLI cannot load without parsing and a test cannot parse
without a file on disk.

* The two jobs share one function and one error type.
  - `config/loader.rs` finds and reads the file, returning the text and its path, so a caller that
    only wants to know which file was read stops here.
  - `config/parser.rs` takes text and returns the schema, so a test hands it a string and never
    touches the filesystem.
    - the error type stays shared for now, since splitting it would change the messages
      `validate-config` prints and the acceptance check compares them
* `main` still reaches into the parse result by field.
  - `main` calls `config::load` and receives the schema, so the field access moves behind the module
    boundary and the CLI no longer knows the file's shape.
```

The same cycle's ladder, `zTOC.md > ## Ladder`, made by the tool, linking across to the
subsections:

```
## Ladder

- 0 (done) [refactor: extract config loader opening](TODO.md#refactor-extract-config-loader-opening)
- 1 (current) [refactor: split loader from parser](TODO.md#refactor-split-loader-from-parser)
- [refactor: extract config loader closing](TODO.md#refactor-extract-config-loader-closing)
```

Its closing, `cycle --close --multi-step --trapezoid main`, makes the closing commit, every marker
`(done)` and the block moved whole to `## Closed`, its `#### Rungs` kept.

## A single-step cycle, closed

The `version flag` cycle, one commit made by `cycle --close --single-step`, as `TODO.md > ## Closed`
holds it:

```
## Closed

### feat: add a version flag

#### Problem

The binary has no way to say which version it is, so a bug report cannot name the build.

#### Solution

`-V` prints the name and the version-of-record, read at compile time.

#### Acceptance check

`<binary> -V` prints the version `Cargo.toml` holds.

#### Deliberation

_None._
```

There are no rung subsections: the scaffold wrote the bookends, and the closing dropped them, a
single-step cycle's one commit being neither. Its ladder is one unlinked rung with the bare title,
since a subsection headed by the title would collide with the title heading's anchor:

```
## Ladder

- 0 (done) feat: add a version flag
```
