# Commit model

Two commit bodies from the `config loader` cycle in [cycle-model.md](cycle-model.md), shown as
specimens rather than a skeleton: a work rung's body and a bookend's. Copy the shape, not the
words. The rules are in [Commit-body form](prose.md#commit-body-form).

## A work rung

Title: `refactor: split loader from parser`

```
Config loading and parsing are one function, so the CLI cannot load a
config without parsing it and a test cannot parse one without a file on
disk. The move into its own module is what forces the split.

* The two jobs share one function and one error type.
  - `config::loader` finds and reads the file and returns the text with
    its path, so a caller that only wants to know which file was read
    stops there.
  - `config::parser` takes text and returns the schema, so a test hands
    it a string and never touches the filesystem.
* `main` still reaches into the parse result by field.
  - `main` calls `config::load` and receives the schema, so the field
    access moves behind the module boundary and the CLI no longer knows
    the file's shape.
```

The intro states this commit's problem and defines "split", the word the title assumes. Each `*`
is one facet of it, each `-` under a `*` answers that facet, and nothing names a file that the
diff already lists.

## A bookend

Title: `refactor: extract config loader opening`

```
Opens the cycle "refactor: extract config loader". Its record is the
In Progress block in TODO.md.
```

A bookend resolves no problem of its own, so its body is the intro paragraph alone, naming the
cycle by its title and pointing at its record. The closing's body is the same shape with "Closes".
