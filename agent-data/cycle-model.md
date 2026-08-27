# Cycle model

A cycle's `## In Progress` block, shown as a specimen rather than a skeleton: the `config loader`
cycle, mid-run, with its opening landed and its first work rung current. Copy the shape, not the
words. The rules are in [The In Progress block](notes.md#the-in-progress-block).

One thing the specimen cannot show:

- a single-step cycle drops both bookends and all the steps are in the one commit: the ladder is one
  unlinked rung with the bare title, and there is no `Ladder details` area ([Cycle
  shape](../AGENTS.md#cycle-shape)).

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
and `vc-x1 config --validate` reports the same findings as before the move.

#### Ladder

- [refactor: extract config loader opening][1] (done)
- [refactor: split loader from parser][2] (current)
- [refactor: extract config loader closing][3]

#### Deliberation

- one work rung, not two: the parser and the loader were going to be split first and moved second,
  but the move is what forces the split, so doing them together is fewer edits
- the acceptance check compares `config --validate` output rather than asserting on internals, so a
  later rename inside the module does not invalidate it

#### Ladder details

##### refactor: extract config loader opening

The cycle's setup commit: create and publish the bookmark, delete `## Closed`'s contents, move the
Todo entry into this block, and bump the version-of-record.

##### refactor: split loader from parser

Loading and parsing are one function, so the CLI cannot load without parsing and a test cannot parse
without a file on disk.

* The two jobs share one function and one error type.
  - `config/loader.rs` finds and reads the file, returning the text and its path, so a caller that
    only wants to know which file was read stops here.
  - `config/parser.rs` takes text and returns the schema, so a test hands it a string and never
    touches the filesystem.
    - the error type stays shared for now, since splitting it would change the messages
      `config --validate` prints and the acceptance check compares them
* `main` still reaches into the parse result by field.
  - `main` calls `config::load` and receives the schema, so the field access moves behind the module
    boundary and the CLI no longer knows the file's shape.

##### refactor: extract config loader closing

Closing out the cycle.

# References

[1]: #refactor-extract-config-loader-opening
[2]: #refactor-split-loader-from-parser
[3]: #refactor-extract-config-loader-closing
