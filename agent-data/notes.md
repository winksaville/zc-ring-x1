# Notes file conventions

Conventions the agent follows when reading and writing notes files (`TODO.md`,
`notes/todo-backlog.md`, `notes/bugs.md`, and the frozen `notes/chores/chores-NN.md` and
`notes/done.md`, [Frozen history](#frozen-history-chores-and-done)). One source of truth lives here,
and [`notes/README.md`](../notes/README.md) points back. Read this before editing a notes file.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## File reads: read the slice you need

Long notes files are appended to over time. Read only the slice your task needs. Grep or read
further on demand.

- **`TODO.md`** (the routine acquaint read): the first ~60 lines covers intro + `## In Progress` +
  the top of the ranked `## Todo` (priorities, #1 highest). `Read` with `offset=0, limit=60`.
  `## Ideas` sits below `## Todo`. Read further only when chasing a lower-ranked entry, an Idea, a
  `[N]` ref, or auditing the whole list.
- **`notes/todo-backlog.md`**: the long-tail backlog (lower-priority entries below the ranked
  `## Todo`). Read only when picking up a backlog item, and grep to locate it first.
- **`notes/bugs.md`**: the bug list. Small, so read it whole when triaging a bug or chasing the
  `## Bugs` pointer in TODO.md.
- **`notes/done.md`** + **`notes/chores/chores-NN.md`**: frozen history. Scan headings first
  (`grep '^## ' notes/chores/chores-NN.md`), then read only the section you need.

**Why:** the routine read should stay small. `TODO.md` grows every cycle, so the backlog and bugs
live in files under `notes/` rather than inline. The same "slice you need" rule applies to
historical files.

## Notes references

Reference *citations* are double-bracketed so the brackets render: `[[N]]`, or `[[2]],[[3]]` for
several (comma-separated, not `[2,3]` or `[[2]][[3]]`). The `[N]:` definitions in a file's
`# References` section and inline `[text](url)` / `[text](#anchor)` links stay single-bracketed.

## Reference numbering

Every note file (`TODO.md`, `todo-backlog.md`, `bugs.md`, `chores-NN.md`, `done.md`) keeps a
file-local `# References` section at the bottom. Reference numbers are scoped to that file: `[1]` in
`chores-07.md` and `[1]` in `chores-01.md` are independent slots that may point at completely
different URLs.

Treat `[N]` like a **footnote**: the number is a local slot, only meaningful within its file's
`# References`. So a `[N]` *citation* (bare `[N]`, or doubled `[[N]]`) never reuses another file's
number. To cite a target a sibling file references, pick your own next-local slot and define it (the
same target may carry a different number in each file). A `[N]` *inside a code span* (`` `[72]` ``)
is different: that's a quoted identifier, literal text naming a ref-key (often from another file's
namespace), data, not a citation, so it needs no definition here. To point at a section of another
file from prose, use an inline link with an anchor, `[that section](../chores-07.md#...)`, not a
bare number.

A `# References` entry is usually a `/notes/<file>.md#anchor` (or `/ARCHITECTURE.md`) path, but may
also be a **same-file fragment**, `[N]: #<slug>`, a ladder rung's link to its own subsection ([The
In Progress block](#the-in-progress-block)).

A file's `# References` can be **re-packed** to a contiguous `[1]..[N]` in first-citation-appearance
order: walk the file's prose in document order (`TODO.md` is `## In Progress`, `## Closed`, then
`## Todo`) and number refs as their first `[[N]]` citation appears. This is a file-local rewrite, so
only that file's `[[N]]` citations and `[N]:` definitions move. Every target and sibling file is
untouched. A `[[N]]` inside a `` ` `` code span is a literal token, not a citation, and is left
alone. Do it opportunistically (when the namespace has drifted enough to annoy), not on a schedule.
`TODO.md` fragments fastest (entries land and get pruned every cycle) and is the usual candidate.
The frozen files are never re-packed.

## Markdown anchor links

GitHub anchor algorithm: lowercase, strip non-alphanumeric characters in place, map remaining spaces
to hyphens 1-for-1. Do **not** collapse adjacent whitespace, so `a + b` -> `a--b` (spaces on both
sides of `+`), but `a: b` -> `a-b` (only trailing space on `:`). General markdown reference:
[markdownguide.org](https://www.markdownguide.org). GitHub publishes no official spec for
auto-generated anchors. The de-facto reference implementation is
[github-slugger](https://github.com/Flet/github-slugger).

## Todo format

`TODO.md` is organized into `## In Progress`, `## Closed` (the last cycle's finished record,
[Cycle-record](../AGENTS.md#cycle-record)), `## Todo` (strict priority rank, #1 highest, with the
long-tail backlog in [todo-backlog.md](../notes/todo-backlog.md)), `## Ideas`, and `## Bugs`
(pointer to [bugs.md](../notes/bugs.md)) sections. Each item is a short description with reference
links to more detail.

`## Todo` and `## Bugs` entries carry explicit `1.` `2.` ... numbers in the source. For `## Todo`
the number is its **priority rank** (#1 highest, descending), and for `## Bugs` it's just an index.
They're for grepping and at-a-glance "let's do #1", **not stable IDs**: reorder (to reprioritize),
insert, or delete freely, then `vc-x1 fix-todo --no-dry-run` renumbers and normalizes
continuation-line indent, so any given number is positional. **To refer to a Todo durably, name it
by its title, a plain, greppable text mention.** Not its number (positional, renumbered) and not a
markdown link: a numbered list item has no anchor to link to.

Numbering helps a human orient in a long list but makes links difficult and fragile, especially an
external reference pointing in, which can't be auto-fixed when the list renumbers. A robust fix (a
number-free anchor, or a number-tolerant dereference that matches the title slug and wildcards the
numeric prefix, since a GitHub slug like `5-foo` is encoded, not opaque, so the title is
recoverable) is a `validate-numbering` design question, out of scope here.

`vc-x1 fix-todo` alone only previews, and `vc-x1 validate-todo` is the read-only check.

Example shape:

```
# Todo
1. Add new feature X [details](features.md#feature-x)
2. Fix bug Y [[1]]

[1]: bugs.md#bug-y
```

## The In Progress block

A cycle's record, `TODO.md > ## In Progress`, written at the opening
([Opening](../AGENTS.md#opening)), finalized and moved to `## Closed` by the closing commit, and
deleted by the next opening ([Cycle-record](../AGENTS.md#cycle-record)). The picked-up `## Todo`
item is moved here (never copied) and becomes the cycle-record's **provisional items**, all
required, all revised as rungs land. The title is a heading one level below `## In Progress` and the
other five are headings one level below the title (a plain cycle: `###` title, `####` items, and
under a program heading, each one deeper):

- **title**, the cycle's name, what `git log --grep` finds it by
- **problem statement**: what is wrong, a sentence or two
- **solution statement**: what will be done about it, broad. Provisional, and the close-out's commit
  body carries the final one
- **acceptance check**: the measure of "are you finished?", specific enough that a reader can run
  it. Not the per-commit validation, which asks whether the artifact still works. A changed check is
  one of the things the deliberation exists to justify
- **ladder**: one rung per step, `- [<title>][M]` plus `(current)` / `(done)`, with `[M]: #<slug>`
  in the file's `# References`. The closing rung, `<cycle title> closing`, is linked like the rest
- **deliberation**: how the five above were decided, one bullet per decision. The bullet's lead
  names the decision and its sentence states it, and the sub-bullets carry the reasons, the
  alternatives weighed, and the costs accepted, so a reader can skim the decisions and read the
  reasons only where they doubt one. `_None._` when there was nothing to deliberate, which is a real
  answer

A **`Ladder details`** area follows them: one subsection per rung, the closing included, headed by
the rung's exact title. Each opens at laddering with an abstract-sized intent statement (the rung's
problem and solution in a sentence or two) and completes at the rung's landing with the conceptual
delta: design points, consequences, deferrals, never a restatement of the landed commit body. The
closing rung's opens with the stub "Closing out the cycle." and completes at close-out with what
closing taught, in problem/solution form, or `_None._`.

A rung is `- [<title>][M] (marker)` and carries no detail beyond that: the title links to the rung's
subsection. A step is identified by its title (prose.md's [Steps are named, not
numbered](prose.md#steps-are-named-not-numbered)), so a title carries no number, no version, and no
SHA. The version-of-record still bumps for every rung and its suffix still encodes the stage, but
that encoding belongs to the manifest and appears nowhere in prose.

A single-step cycle's ladder is one unlinked rung, `- <cycle title> (marker)`, and the block has no
`Ladder details` area: a subsection headed by the title would collide with the title heading's
anchor ([Cycle shape](../AGENTS.md#cycle-shape)).

The block is the cycle's only record. A design finding that must outlive the cycle goes into a
`notes/` file by the rung that made it ([Cycle-record](../AGENTS.md#cycle-record)).

## Frozen history: chores and done

`notes/chores/chores-NN.md` and `notes/done.md` are the records of cycles that ran before the
cycle-record became `TODO.md > ## In Progress` alone ([Cycle-record](../AGENTS.md#cycle-record)).
They are frozen: nothing is appended, no section is opened, no entry is retired into them, and no
ref is backfilled. They stay in place because Todo entries and design notes link into them, and
those links stay valid. Read them as history, by the slice you need.
