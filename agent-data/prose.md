# Prose and durable text

How long-lived text is written on this project: the prose shape, the punctuation rules, and the
commit-title identity. Read this before writing durable text (notes files, commit messages, doc
comments, the cycle-record).

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## Prose form

([why](rationale.md#prose-form))

Long-lived prose on this project follows one basic shape: a short intro that explains the *why* or
the high-level *what*, sharpened to a *problem statement* where a surface calls for one (see
[Problem-first shape](#problem-first-shape)), then a `-` bullet list for the details. The width
numbers and the wrap discipline live in [Line widths](#line-widths) below, their one home. One fact
per bullet or sub-bullet beats a paragraph packing several. Avoid wall-of-prose paragraphs: they
hide the structure that bullets make scannable. Punctuation that joins clauses without naming their
relationship is the same failure at sentence scale. See [Semicolons](#semicolons) and [Typeable
punctuation only](#typeable-punctuation-only).

Surfaces that use this shape:

- Module / function / struct / field doc comments in `.rs` files. See [Doc
  comments](code.md#doc-comments-on-every-file-function-and-method).
- Commit message bodies (both work-repo and agent-repo). The title is the commit-specific add-on.
  See [The per-rung flow](../AGENTS.md#the-per-rung-flow).
- The cycle-record, `TODO.md > ## In Progress`. See [The In Progress
  block](notes.md#the-in-progress-block).
- Todo entries in `TODO.md` when an entry needs more than one line of detail. Pure one-liners are
  still fine.

Bullet *content* differs by surface:

- **Commit bodies**: the [Problem-first shape](#problem-first-shape) for finished work, arranged by
  [Commit-body form](#commit-body-form) below. What is specific to a commit:
  - the problem statement defines any word the title assumes, since the title is what a reader meets
    first and it answers the problem
  - **no file list.** The diff and `git show --stat` are the mechanical record, so restating them is
    a second copy that can drift from the first. An import of a thousand files is one change
  - these are claims a reader has to follow, so they are sentences rather than fragments. A bullet
    wanting a paragraph belongs in the cycle-record instead
  - the **deliberation** stays out: alternatives weighed, evidence, dates, costs accepted. Those
    live in the cycle-record, the `## Todo` entry, and the session the `ochid:` trailer names, each
    reachable from the commit by construction. The problem itself is a *why* and belongs here
- **Cycle-record / todo**: bullets are conceptual (design points, structural notes, the "what landed
  and why" at a notch above file-list granularity). Never a copy of the commit's edit list. The diff
  is the mechanical record, the commit body the problem and solution, the cycle-record the design
  thinking, and none restates another.
- **Doc comments**: bullets are whatever structure fits (fields, cases, invariants).

### Leads are labels, unmarked

In a list item or a numbered step, a lead is a short label ending in a colon, with no markup, and
the sentence after it is complete without it:

```
1. Backfill: fill every as-built ladder whose commits have landed, before anything else.
```

- Label names, sentence instructs: a reader's eye takes bold at the head of an item as the item's
  name and resumes at the plain text expecting a full sentence, so a bold lead that is the
  sentence's own verb or subject (`**Backfill** every as-built ladder ...`) loses its imperative to
  the reader who skips it. Measured 2026-08-21, at the review of the rule that example is from: the
  reviewer read "every as-built ladder" and stopped, with nothing telling them what to do.
- The inverted case: a rule stated in bold is the same failure inverted. When the bold is a whole
  sentence and the plain text is commentary, the reader who skips bold reads the commentary as the
  rule. The fix is the same: a short name as the label, the rule as the plain sentence. The hard
  rules in AGENTS.md are the instance, and the names they gained are greppable and survive a
  renumbering.
- Definitions: the same shape, `Term: what it means.` A period after a bold term (`**Term.** ...`)
  reads as a heading, which is the label reading again.
- No markup on the label: bold is what makes the eye treat the lead as a heading and skip it, and
  the agent-files are read by an agent, which needs no emphasis. The colon alone marks the label
  (wink, 2026-08-21).
- The price: one word of redundancy, paid on purpose: the label is skippable, the sentence is not.

### One spelling per term

A defined term has one spelling, everywhere: `work-repo` and `agent-repo` are always hyphenated,
`agent-files`, `version-of-record`, `dual-repo`, and `as-built` likewise, and no rule varies the
spelling by how the term is used. Terminology in AGENTS.md defines the terms, and this section is
where the spelling rule lives, since a glossary is read for meaning rather than for spelling.

### Bullet form

A bullet is a full sentence, capitalized and ending in a period or question mark, unless the list
is a plain list of things (files, names, options), which takes neither. A list is all one or all
the other. A sentence never opens with a lower-case name (`squash-push`, `rationale.md`), it is
recast so the name is not the first word.

### Line widths

Every width number lives here and nowhere else: the other files and sections link here rather than
restating one, so a change is a single edit and the copies cannot drift. Consolidated 2026-08-09,
when the body width moved and the restatements had to be hunted.

- **Prose**, all durable text: wrap at <=100 cols, bullet continuations indented two spaces.
- **Source**, doc comments and inline comments included: <=100 cols, which is rustfmt's default
  `max_width` (enforcement notes in [code.md](code.md#line-width)).
- **Commit titles**: <=50 chars.
- **Commit bodies**: <=75 cols, the Linux kernel patch standard. It replaced git's older 72
  convention here 2026-08-09, and published bodies keep the wrap they shipped with.

The widths are wrap defaults, not absolutes (the title cap excepted). Existing text re-wraps when
touched, no mass sweeps. Write to the full width: wrap near the limit rather than imitating the
narrow wrap of older text, and a line that reads better long stays long (an URL, a literal report
row, indented code in a comment).

### Problem-first shape

`## In Progress` cycle blocks, `## Todo` entries, and commit bodies use a sharper form of the same
shape: a problem, then how it is answered, then the steps that get there.

- **Problem statement** (the why): one or two sentences. Don't pad with intent, don't restate what
  follows it.
- **Solution statement** (the what/how): what is done about the problem, in broad terms, answering
  whatever question the problem statement raises. Surface-specific rules are in [Bullet content
  differs by surface](#prose-form) above.
- **Plan bullets** (the what/when), the steps. Formality differs by surface:
  - In Progress: a committed ladder, one step per commit. See [Conventional-commit
    shape](#conventional-commit-shape-ladder--commit) for the per-step title + `(current)` /
    `(done)` form.
  - Todo entries: rough informal bullets, no numbering, formalized only when the entry is picked up
    into a cycle.

**Timing decides whether the solution statement is provisional, not whether it is written.** A cycle
writes one at Preparation, before the work, and revises it as steps land. The close-out's commit
body carries the final one. A `## Todo` entry's is provisional in the same way. Only a commit body's
is settled, because a commit is finished by the time it has one. The earlier rule here said a plan
was for work not yet done and a solution for work that is, which left a cycle unable to say at its
opening what it intended to do.

### Commit-body form

The specimen is [commit-model.md](commit-model.md). A commit body is the [Problem-first
shape](#problem-first-shape) with one addition: a body whose
problem has several sub-problems arranges them by a fixed recursion, so a reader knows what any
bullet is from its marker and its depth. The earlier statement, "a problem statement then a solution
statement, both broad", left that arrangement to taste, and taste converged slowly and separately.

- **An intro paragraph states the general problem**, and defines any word the title assumes. It is
  mandatory. [Prose form](#prose-form) wants it regardless, and a body opening on a bullet is a body
  a `--body` flag can mistake for an option. The problem is *this commit's*, never the cycle's: the
  cycle's problem lives in the In Progress block, and a body whose problems outnumber what the diff
  resolves is describing something larger than the commit.
- **`*` bullets are the problem's facets**: sub-problems that decompose the intro's general problem,
  not a grab-bag of unrelated fixes. A body reaching for unrelated problem bullets is usually asking
  to be more than one commit.
- **`-` bullets are solutions**, and every `-` sits under a `*`, solving that facet. A `-` with no
  `*` above it reads as a solution to nothing, so there are no top-level solutions: a solution that
  retires every facet at once is written under each, or the facets are one.
- **One facet is the trivial commit**: the intro, one `*`, one or more `-` under it. Not a second
  form, the general form at its smallest.
- **A bookend body is a pointer**: an opening or closing commit resolves no problem of its own, so
  its body is the intro paragraph alone, naming the cycle by its title and pointing at its record,
  the In Progress block. No `*`, no `-`.

**The markers are typed on purpose**: `*` always means problem, `-` always means solution, and the
pairing is what a reader counts on: the `-` answers the `*` above it, with no rule to consult. The
typing also keeps history greppable (`^\* ` finds every facet, `^ - ` every solution). Bodies are
read as plain text, in `jj log` and in terminals, where the markers survive, and if a renderer ever
flattens them the indentation still carries the structure. So the mixed markers are deliberate, and
a linter's consistent-marker rule is wrong to normalize them.

Unchanged by this: no version in title or body, no file list, no deliberation, titles per
[Conventional-commit shape](#conventional-commit-shape-ladder--commit).

### Semicolons

Prose carries no semicolons. A semicolon appears only in code (code spans, fenced code, source
files), where it is syntax rather than prose. The structure a semicolon would have joined is written
explicitly instead:

- **Two claims** take a period, each half standing as its own sentence ("The diff empties. The
  history keeps the record.").
- **A continuation** takes a comma with a conjunction, when one half carries the other's thought
  onward rather than making its own claim.
- **A list hiding in prose** (`A; B; C` inside a sentence or bullet) breaks into sub-bullets so the
  structure shows. List items that themselves contain commas restructure the same way rather than
  separating with semicolons.

The code allowance is why a bare byte scan cannot enforce the rule: a checker blanks the code first,
then expects zero.

The agent-files (`AGENTS.md`, `custom*`, `agent-data/*`) carry no historical exemption and are swept
to zero. Any other historical file keeps its existing semicolons only until it is touched: a commit
that edits a file converts that whole file's prose semicolons in the same commit, code spans exempt,
using the joins above. Files outside the commit's diff are never converted, since that is a sweep
and sweeps are their own cycle. Source-file comments are prose under this rule (see
[code.md](code.md#comments-are-prose)).

### Typeable punctuation only

Durable text uses punctuation that can be typed at a terminal. The prohibition is on *authoring*,
not presence: a file may legitimately hold a banned character it transcribed (see below), so a byte
scan is not the rule and a sweep needs the authored/transcribed judgment. Banned from authoring:
`—`, `–`, `…`, `→`. None can be entered without a compose key or a paste, so none can be grepped
for, and an em dash next to option syntax reads as another flag. Like the semicolon rule above, the
agent-files are swept to zero and any other file converts whole when a commit touches it (specimens
and transcriptions aside, below). A banned character costs nothing to write and is paid on every
read, so the rule is not softer than that.

`…` becomes `...` and `→` becomes `->`. The dashes have no single replacement, because an em dash
usually stands in for a structural decision that was not made. Make the decision:

- **A bullet's title and its body sharing a line** is a heading and a paragraph. Make the body
  sub-bullets.
- **A term and its definition** (`jj diff`, `<base>`, a flag) takes a colon, which keeps a glossary
  or a command list at one line per entry.
- **A prose aside** takes a comma, parentheses, or two sentences. Often the aside should just go.

Converting a heading moves its anchor. The em dash strips but the spaces on both sides survive, so
`## A — B` slugs to `#a--b` while its colon form slugs to `#a-b` (see [Markdown anchor
links](notes.md#markdown-anchor-links)). Re-point inbound links in the same commit.

Scope is the same as [Speculation marker](#speculation-marker), plus commit titles and everything
under `src/`: doc comments, inline comments, and any user-visible string. Source is the surface a
human edits and greps most, so an untypeable character costs more there than in prose. A code span
is not exempt by itself. Naming the character is a specimen and stays, which is how this section
names them. A banned character doing a job is a use and converts: `` `.expect(…)` `` becomes
`` `.expect(...)` ``.

Text quoted from outside this repo's prose (tool output, an error message, an already-published
commit title) is transcribed, not written, so it keeps its characters, whether or not it sits in a
code span. It matters most for commit titles: converting one stops it matching `git log --grep` and
breaks the verbatim identity that [Conventional-commit
shape](#conventional-commit-shape-ladder--commit) requires between a commit title and its ladder
rung.

### Conventional-commit shape (ladder / commit)

([why](rationale.md#conventional-commit-shape-ladder--commit))

A ladder step and its commit description share a *title* shape, a [Conventional
Commits](https://www.conventionalcommits.org/en/v1.0.0/) title (`<type>: <desc>`, an optional
`(scope)` after the type: `feat(push): ...`) over [Prose form](#prose-form) detail. They differ in
the title's marker (below) and in bullet *content*: commit bodies take the [Commit-body
form](#commit-body-form), the ladder's are conceptual (see "Bullet *content* differs by surface"
above). The shared template:

```
<title>                          # <title> is the commit's `<type>: <desc>`
<optional prose intro>
  - <optional item>
    <optional prose intro>
      - <optional sub-item>
      ...
```

The two surfaces apply it as:

- **Ladder step** (`TODO.md` `## In Progress`): the rung is `- [<title>][M] (marker)`: the title
  reference-linked to the rung's subsection via `[M]: #<slug>` in the file's `# References` (the
  closing rung linked like the rest), and the `(current)` / `(done)` marker. Its position in the
  list is its position in the ladder. The last rung is the close-out and its text says so. Detail
  lives not on the rung but in the block's `Ladder details` subsection headed by the rung's exact
  title (see the protocol's [Opening](../AGENTS.md#opening)), bulleted, never `;`-joined inline.
- **Commit description**: no prefix. The title is the first line, and the body is the prose (see
  [Commit description](../AGENTS.md#commit-description)).

The title is **identical** across both for a given step, so a step's ladder entry and its commit
title line up verbatim. A `Ladder details` subsection heading carries the same title, a third
surface on every rung (the closing rung's exists only when close-out gotchas occurred: see the
protocol's [Opening](../AGENTS.md#opening)). Pick the commit title first and reuse it.

That identity is **per step**, not per cycle: each step in a cycle gets its own distinct descriptive
title, never one shared cycle title uniquified by a step marker. The block's title heading carries
the *cycle title*, the bare name no multi-step commit carries (see the bookends below). To keep a
cycle's commits collectable with one `git log --grep`, give the step titles a common greppable stem
(e.g. `config loader`).

**Cycle bookend titles**: a multi-step cycle's bookend commits are the cycle title plus a suffix, "
opening" and " closing", same type (`feat: dynamic warmup opening` /
`feat: dynamic warmup closing`), so one `git log --grep "<cycle title>"` returns the pair that
brackets the ladder. The bare cycle title is the cycle's *name*: the block's title heading carries
it, and no multi-step commit does, which is also what keeps the closing rung's subsection anchor
clear of the title heading's. A single-step cycle's one commit is the cycle and keeps the bare
title, in both repos and in its one ladder rung ([Cycle shape](../AGENTS.md#cycle-shape)). The type
repeats across the pair even though the bookends are mostly bookkeeping: identical prefixes make
them scannable. Rungs between keep their own titles on the stem.

**Commit description details**, beyond the shape: the title is a [Conventional
Commit](https://www.conventionalcommits.org/), `<type>: <short description>` with an optional
`(scope)`, at the width in [Line widths](#line-widths), common types `feat`, `fix`, `refactor`,
`test`, `docs`, `chore`. A scope names a component as its user would name it (a subcommand, a
repo), never a file or directory, and is omitted when there is none. The body is the [Commit-body
form](#commit-body-form) above, wrapped per Line widths, with no version in title or body, no file
list, and no deliberation. `vc-x1 push` gives both repos' commits the same title and body. `ochid:`
is the body's last line, stamped by push, and a breaking change uses the hyphenated
`BREAKING-CHANGE:` trailer key.

### Steps are named, not numbered

A step has a title and no number. Nothing in a ladder rung or a commit gives a step an ordinal: a
rung's place in the list already *is* its place in the ladder, so a number beside it would restate
the position and then have to be maintained.

- **The title is the identifier.** A record points at a step by its title, a plain greppable
  mention, which is why the title is verbatim-identical across the three surfaces.
- **Unambiguous, not globally unique.** Two titles must be distinguishable where a title is
  resolved: within its own cycle's block, so a ladder rung names one step and its subsection heading
  one anchor, since a repeated slug silently dedupes to the first one. Across the repo's history a
  title may repeat.
- **Nothing renumbers.** Inserting, reordering or dropping a step edits the ladder list and nothing
  else. On an unlanded topic bookmark the rungs that already committed an older ladder come along.
  See [Cycles run on a bookmark](../AGENTS.md#cycles-run-on-a-bookmark).
- **`## Todo` is ordered, not numbered**: priority is file order ([Todo
  format](notes.md#todo-format)), so no rank number exists to be cited (the numbered form retired
  2026-08-27).

### Versions live in the version-of-record only

No version appears in durable prose: not in a ladder rung, a commit title, or a commit body. The
manifest is the version's only written home (see [versioning.md](versioning.md)), and a commit's
version is read from that file at that commit
([why](rationale.md#versions-live-in-the-version-of-record-only)).

**No surface records a version.** Which commit a version names is read from the manifest at that
commit, and "what shipped in 0.42.0" is answered by `git log` on the manifest, not by a list kept in
prose. The earlier chores as-built rungs and `## Done` entries carried versions beside commit SHAs,
and retired with the records they lived in ([Cycle-record](../AGENTS.md#cycle-record)).

## Agent-files name no project

An agent-file states rules and mechanisms. It never names an adopter, an adopter's history, or an
adopter's versions. The vc-x1 *CLI* and its versions are tool facts every adopter shares and stay.
What is barred is an adopter's repo appearing in universal text
([why](rationale.md#agent-files-name-no-project)).

**How to apply:** state the rule and its mechanism in the agent-file, and leave the evidence trail
in the records of the project that earned it (the cycle-record, Todo entries), reachable from the
commit that changed the agent-file. Dates are fine, since a date names a moment, not an adopter. A
specimen in the scheme's own notation (an example version, an example bookmark name) is a use, not a
reference, per the same distinction [Versions live in the version-of-record
only](#versions-live-in-the-version-of-record-only) draws.

## Speculation marker

Durable text the agent writes (agent-files, `notes/`, commit bodies, the cycle-record) should stick
to observations and direct descriptions of the code or data. If a mechanism, hypothesis, or causal
claim enters the text, prefix it with "We think ..." (a royal "we") so a reader can tell the
measured from the inferred ([why](rationale.md#speculation-marker)).

**How to apply:** observations and factual descriptions need no marker. Prefix with "We think ..."
(or a close variant like "Our guess is ...") when the claim is a mechanism ("X wins because Y caches
better"), a cause ("the drift was due to thermal state"), a prediction ("this should scale
linearly"), or any reasoning not directly supported by the data on hand.

## Plain synopsis after technical explanations

When a conversational reply centers on a technical explanation (measurement theory, statistics,
hardware behavior), end it with a short plain-language synopsis, no jargon and no symbols, so the
reader can check their understanding against the technical version
([why](rationale.md#plain-synopsis-after-technical-explanations)).

**How to apply:** conversation only, not notes files (a notes entry should already lead with the
why). Mark it clearly (e.g. "The plain version:"). A reply that is already plain needs no synopsis.
