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
further on demand ([why](rationale.md#file-reads-read-the-slice-you-need)).

- **`TODO.md`** (the routine acquaint read): the first ~60 lines covers intro + `## In Progress` +
  the top of `## Todo`, its entries in priority order. `Read` with `offset=0, limit=60`.
  `## Ideas` sits below `## Todo`. Read further only when chasing a lower entry, an Idea, a
  `[N]` ref, or auditing the whole list.
- **`notes/todo-backlog.md`**: the long-tail backlog (lower-priority entries below `## Todo`). Read
  only when picking up a backlog item, and grep to locate it first.
- **`notes/bugs.md`**: the bug list. Small, so read it whole when triaging a bug or chasing the
  `## Bugs` pointer in `TODO.md`.
- **`notes/done.md`** + **`notes/chores/chores-NN.md`**: frozen history. Scan headings first
  (`grep '^## ' notes/chores/chores-NN.md`), then read only the section you need.

## Notes references

Reference *citations* are double-bracketed so the brackets render: `[[N]]`, or `[[2]],[[3]]` for
several (comma-separated, not `[2,3]` or `[[2]][[3]]`). The `[N]:` definitions in a file's
`# References` section and inline `[text](url)` / `[text](#anchor)` links stay single-bracketed.

## Reference numbering

Every note file keeps a file-local `# References` section at the bottom, and its numbers are scoped
to that file: the same `[1]` in two files is two independent slots, each pointing wherever its own
file defines.

- Treat `[N]` as a **footnote**, a slot meaningful only inside its own file's `# References`, so a
  citation never reuses another file's number. To cite a target a sibling file references, take your
  own next free slot and define it, and the same target may carry a different number in each file.
- A `[N]` *inside a code span* (`` `[72]` ``) is a quoted identifier, data rather than a citation,
  so it needs no definition.
- To point at a section of another file, use an inline link with an anchor rather than a bare
  number.
- A definition is usually a path with an anchor, and may also be a **same-file fragment**,
  `[N]: #<slug>`, which is how a ladder rung links its own subsection ([The In Progress
  block](#the-in-progress-block)).
- A file's `# References` may be **re-packed** to a contiguous `[1]..[N]` in first-citation order,
  walking the file's prose top to bottom. A file-local rewrite: no other file moves, and a new ref
  may take the next free number, out of order being fine. Ask before re-packing, and never re-pack
  [frozen history](#frozen-history-chores-and-done).

## Markdown anchor links

GitHub anchor algorithm: lowercase, strip non-alphanumeric characters in place, map remaining spaces
to hyphens 1-for-1. Do **not** collapse adjacent whitespace, so `a + b` -> `a--b` (spaces on both
sides of `+`), but `a: b` -> `a-b` (only trailing space on `:`). General markdown reference:
[markdownguide.org](https://www.markdownguide.org). GitHub publishes no official spec for
auto-generated anchors. The de-facto reference implementation is
[github-slugger](https://github.com/Flet/github-slugger).

## Todo format

`TODO.md` has these sections, in this order. Each item in them is a short description with
reference links to more detail.

- `## Continuation notes`: where the agent was, for the agent that comes next. Ephemeral, never a
  record, `_None._` by default, written before a restart or a loss of context, and reset by the
  agent that reads it, after filing each fact into its home or keeping the bullet whose fact has
  none, so a reset never destroys the only copy of anything.
- `## In Progress`: the running cycle's record ([Cycle-record](../AGENTS.md#cycle-record)).
- `## Waiting`: important work that cannot start yet. Each entry names what it waits on and its
  rank once unblocked, and every opening checks the conditions.
- `## Todo`: entries in priority order, the first highest. The long-tail backlog is in
  [todo-backlog.md](../notes/todo-backlog.md).
- `## Ideas`: unranked.
- `## Bugs`: a pointer to [bugs.md](../notes/bugs.md).
- `## Closed`: the last cycle's finished record.
- `# References`: the file-local reference definitions, the file's last section ([Reference
  numbering](#reference-numbering)).

Every adopter has one `TODO.md` of this shape. It is not an agent-file, since its content is the
project's record, and the payload ships it as a skeleton: `## In Progress` reading
`_No cycle currently in progress._`, the other sections empty.

An entry is a `###` heading, its title, followed by its text. Priority is file order, the first
entry the highest, and reprioritizing is moving the entry. The title is unique within its file and
is the entry's anchor, so a citation is a link, `[title](TODO.md#<slug>)`, that the anchor check
verifies ([Prose form](prose.md#prose-form) for the text). Entries carry no number: a rank number
renumbers on every move and every citation that holds one goes stale, which is why the numbered
form was retired (2026-08-27). An entry's sub-entries, when it groups several, are bullets with a
bold title, cited by the bold text.

Example shape:

```
## Todo

### Add new feature X

The feature, in a sentence or two ([details](features.md#feature-x)).

### Fix bug Y

What is wrong and where [[1]].

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
  in the file's `# References`. The markers stay when the block moves to `## Closed`. `<title>` is
  the rung's commit title, `<type>: <desc>` per
  [Conventional-commit shape](prose.md#conventional-commit-shape-ladder--commit), so a moved
  `## Todo` entry is retitled. The closing rung, `<cycle title> closing`, is linked like the rest
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
