# AGENTS.md - Bot Instructions

## Project Structure

This project uses **two separate jj-git repos**:

1. **App repo** (`/` — project root): the project's generated
   artifact — code, prose, image, song, whatever it produces.
2. **Bot session repo** (`/.claude/`): Contains Claude Code session data.

Both repos are managed with `jj` (Jujutsu), which coexists with git.

**Committing vs pushing.** Default to `vc-x1 push` — it commits and
publishes both repos together, each carrying one `ochid:` pointing at
the other (see [Cycle Protocol](#cycle-protocol) and
[ochid trailers](#cross-repo-linking-ochid-trailers)). Use a bare
`jj commit` (see [jj Basics](#jj-basics)) only when:

- the change will never be published — no `ochid:`;
- the commit will be squashed away before its series is pushed
  (loop-and-squash) — no `ochid:`; it disappears in the squash;
- the commit will be pushed later as a non-top commit — `jj commit`
  and add its `ochid:` now, because `vc-x1 push` stamps only the
  topmost commit it pushes, never the ancestors.

**What "commit" and "push" mean.** In an instruction, "commit",
"push", and "commit + push" all mean `vc-x1 push` — land *and*
publish — unless stated otherwise. A bare `jj commit` is asked for
by name: "local commit", "save locally", "just `jj commit`" (the
cases above). So the spoken default is publish; the local-only
save is the named exception. The approval around a push —
interactive by default, waived only by an explicit scoped
delegation — is the cycle protocol's
[Pushing policy](notes/cycle-protocol.md#policy).

## Repo Paths (relative from project root)

- App repo: `.` (project root)
- Bot session repo: `.claude`
  (symlink from `~/.claude/projects/<path-to-project-root>/.claude`)

## Working Directory

Prefer staying in the project root. Use `-R` flags or absolute paths
to target other directories rather than `cd`. If `cd` seems necessary,
discuss with the user first — losing track of cwd causes subtle
command failures downstream.

When invoking shell commands, prefer the shortest unambiguous path —
usually relative to cwd (`ls notes/`, not
`ls /home/wink/data/prgs/rust/vc-x1/notes/`). Long absolute paths
clutter the transcript without adding information. Out-of-workspace
paths (`/tmp/...`, `~/.config/...`) stay absolute. Read/Edit/Write
tool args also stay absolute — that's a tool-boundary constraint,
not a stylistic choice.

Run **one command per shell invocation** — don't bundle several
steps into a single compound script (`a && b; c`). Bundling hides
which step produced which output and makes a failure ambiguous;
executing one at a time keeps the details visible and reviewable.
Exceptions are a genuine pipeline (`grep | sort`) or a tight,
inseparable pair where the join is the point.

## File reads — read the slice you need

Long notes files are appended to over time. Read only the
slice your task needs; grep or read further on demand.

- **`notes/todo.md`** (the routine acquaint read) — first
  ~60 lines covers intro + `## In Progress` + the top of the
  ranked `## Todo` (priorities, #1 highest). `Read` with
  `offset=0, limit=60`. `## Ideas` sits below `## Todo`; read
  further only when chasing a lower-ranked entry, an Idea, a
  `[N]` ref, or auditing the whole list.
- **`notes/todo-backlog.md`** — the long-tail backlog
  (lower-priority entries below the ranked `## Todo`). Read
  only when picking up a backlog item; grep to locate it
  first.
- **`notes/bugs.md`** — the bug list. Small; read whole
  when triaging a bug or chasing the `## Bugs` pointer in
  todo.md.
- **`notes/done.md`** + **`notes/chores/chores-NN.md`** —
  historical / append-mostly. Scan headings first
  (`grep '^## ' notes/chores/chores-NN.md`), then read only
  the section you need.

**Why:** the routine read should stay small. `notes/todo.md`
grows every cycle, so the backlog and bugs live in sibling
files rather than inline; the same "slice you need" rule
applies to historical files.

## Memory

Do not use the bot's per-project memory directory
(`~/.claude/projects/<path>/memory/`). In a dual-repo setup with
AGENTS.md it provides no capability AGENTS.md doesn't already cover,
and it loses on discoverability:

- **AGENTS.md** — at the repo root, a well-known location for bot
  instructions, committed, reviewable, visible to every collaborator
  (human or bot).
- **Memory directory** — hidden under the user's home, tied to one
  machine, invisible to anyone but the bot, never diffed or reviewed.

Easy for everyone to find beats convenient for the bot alone. Put
durable context in AGENTS.md (or committed `notes/`) instead.

## Speculation marker

Durable text the bot writes — AGENTS.md, `notes/`, commit bodies,
chores sections — should stick to observations and direct descriptions
of the code or data. If a mechanism, hypothesis, or causal claim
enters the text, prefix it with "We think ..." (a royal "we") so a
reader can tell the measured from the inferred.

**Why:** unmarked speculation reads like evidence, and a future reader
(or the bot on a later session) can pick it up as a known fact when
it's not. Measured / inferred is a distinction worth keeping visible
in the written record.

**How to apply:** observations and factual descriptions need no
marker. Prefix with "We think ..." (or a close variant like
"Our guess is ...") when the claim is a mechanism ("X wins
because Y caches better"), a cause ("the drift was due to thermal
state"), a prediction ("this should scale linearly"), or any
reasoning not directly supported by the data on hand.

## jj Basics

**Use jj, not git, for version-control operations** (status, log,
diff, commit, push, history rewrite). jj coexists with the git
backend, so the repo *can* be driven with raw `git`, but this
project's workflow — bookmarks, the working-copy `@` model, ochid
trailers — is expressed in jj terms; reaching for `git` invites
state that doesn't match the jj documentation here. There is no
`jj mv`: to move/rename a tracked file, just `mv` it on disk and
jj detects the rename by content.

- `jj st -R .` / `jj st -R .claude` — show working copy status
- `jj log -R .` / `jj log -R .claude` — show commit log
- `jj commit -m "title" -m "body" -R <repo>` — finalize working copy into a commit
- `jj describe -m "title" -m "body" -R <repo>` — set description without committing
- `jj git push --bookmark <name> -R <repo>` — push a bookmark (no
  `--allow-new` flag; jj pushes new bookmarks without special flags)
- In jj, the working copy (@) is always a mutable commit being edited.
  `jj commit` finalizes it and creates a new empty working copy on top.
- The `.claude` repo always has uncommitted changes during an active
  session because session data updates continuously.
- `jj rebase` uses `--onto`/`-o` to name the destination(s).

## Cross-repo linking (ochid trailers)

The cross-reference between the app repo and the `.claude` bot
repo is what makes the dual-repo work: every commit points at its
counterpart in the other repo, so the "what" (code) and the
"why / how" (session) stay linked across time. That pointer is the
**ochid** (Other Change ID) git trailer.

A **chid** is jj's change ID — a permanent identifier that
survives rebases and `describe`s (unlike the commit ID / git SHA,
which changes on rewrite). An **ochid** trailer carries the
counterpart commit's chid as a workspace-root-relative path:

- Paths start with `/` — the workspace root, i.e. the app repo
  (vc-template-x1). `/.claude` is the bot session sub-repo.
- `ochid: /<chid>` references a change in the **app repo**.
- `ochid: /.claude/<chid>` references a change in the **`.claude`
  repo**.

Trailers are blank-line-separated `key: value` lines at the end of
the commit body, using the chid's **12-character** prefix:

```
ochid: /.claude/xvzvruqowktp   # points to a .claude repo change
ochid: /wtpmottvxqzl           # points to an app repo change
```

How many, and which direction:

- **Code-side commits** each carry one
  `ochid: /.claude/<.claude-chid>` — the `.claude` change ID.
- **The `.claude` commit** carries one `ochid: /<code-chid>` per
  code commit in that push. More than one occurs on a Merge
  non-ff close-out (one ochid per Work commit in the cycle).

Use `vc-x1 chid -s code,bot -L` to capture the change IDs (first
line app, second `.claude`).

### Resolvability

A change ID travels with its commit: a **pushed** commit resolves
to the same chid in every clone — cloning the `.claude` repo gave
the published `main` tip the same chid as an existing clone. We
think jj carries the change ID in the git commit object, so it
survives `jj git clone` / fetch.

The local-only case is the **working-copy `@`**: jj mints a fresh
random chid for `@` in each clone, so an unpushed `@` is never a
stable ochid target. This is why a `.claude` ochid names `@-`
(the last committed change), not `@`.

### .vc-config.toml

Each repo contains a `.vc-config.toml` that identifies its
location within the workspace, so tools resolve these paths
without repeating the workspace path in every trailer:

```toml
# In vc-template-x1 (workspace root):
[workspace]
path = "/"

# In .claude (sub-repo):
[workspace]
path = "/.claude"
```

## Prose form

Long-lived prose on this project follows one basic shape: a short intro
that explains the *why* or the high-level *what*, then a `-` bullet
list for the details. Wrap lines at ≤72 cols (bullet continuations
indent two spaces). Avoid wall-of-prose paragraphs — they hide the
structure that bullets make scannable.

Surfaces that use this shape:

- Module / function / struct / field doc comments in `.rs` files —
  see [Doc comments](#doc-comments-on-every-file-function-and-method).
- Commit message bodies (both app-repo and session-repo). The
  ≤50-col title is the commit-specific add-on; see
  [Per-commit flow](notes/cycle-protocol.md#per-commit-flow).
- Chore descriptions in `notes/chores/chores-NN.md` — see
  [Chores section content](#chores-section-content--no-edit-list-git-is-the-record).
- Todo and Done entries in `notes/todo.md` when an entry needs more
  than one line of detail. Pure one-liners are still fine.

Bullet *content* differs by surface:

- **Commit bodies** — bullets are file-by-file: one bullet per file
  changed, file plus a one-line gist (e.g.
  `README.md: new Overview intro`). Source of truth for the
  mechanical edit list.
- **Chores / todo / done** — bullets are conceptual (design points,
  structural notes, the "what landed and why" at a notch above
  file-list granularity). Never a copy of the commit's edit list —
  see [Chores section content](#chores-section-content--no-edit-list-git-is-the-record).
- **Doc comments** — bullets are whatever structure fits (fields,
  cases, invariants).

### Problem + plan shape

`## In Progress` cycle blocks, chores section intros, and `## Todo`
entries use a sharper form of the same shape:

- **Problem statement** (the why) — one or two sentences;
  don't pad with intent, don't restate the plan.
- **Plan bullets** (the what/when) — formality differs by
  surface:
  - In Progress / chores: a committed ladder, one step per
    commit — see
    [Conventional-commit shape](#conventional-commit-shape-ladder--chores--commit)
    for the per-step title + `(current)` / `(done)` form.
  - Todo entries: rough informal bullets, no numbering;
    formalized only when the entry is picked up into a
    cycle.

### Semicolons inside bullets

A bullet that joins multiple clauses with semicolons (`A; B; C`)
is a list hiding inside running prose — break the clauses into
sub-bullets so the structure shows. Semicolons in running prose
(intro paragraphs, sentence-joins) are fine. Not absolute: very
short clauses or tight pairs can stay joined inside a bullet when
breaking would be more noise than signal.

### Conventional-commit shape (ladder / chores / commit)

A ladder step, its chores section, and its commit description
share a *title* shape — a
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
title over [Prose form](#prose-form) detail. They differ in the
title's prefix / marker (below) and in bullet *content* — commit
bodies are file-by-file, ladder / chores conceptual (see "Bullet
*content* differs by surface" above). The shared template:

```
<optional - X.Y.Z[-N]> <title>   # <title> is the commit's `<type>: <desc>`
<optional prose intro>
  - <optional item>
    <optional prose intro>
      - <optional sub-item>
      ...
```

The three surfaces apply it as:

- **Ladder step** (`notes/todo.md` `## In Progress`): the title is
  prefixed with the version — `X.Y.Z-N <title>` — and
  carries a `(current)` / `(done)` marker. The bare three-element
  `X.Y.Z` (no `-N`) is the close-out step. Detail is bulleted,
  never `;`-joined inline.
- **Chores section** (`notes/chores/chores-NN.md`): no version
  prefix — the `##` header *is* the bare title, with a
  `Commits: [[ref]]` line first under it (empty until backfilled —
  see [Chores commit references](#chores-commit-references)).
- **Commit description**: no version prefix — the title is the
  ≤50-col first line; the body is the prose (file-by-file for the
  app repo, per [Per-commit flow](notes/cycle-protocol.md#per-commit-flow)).

The title is **identical** across all three for a given step, so a
step's ladder entry, its chores `##` header, and its commit title
line up verbatim — pick the commit title first and reuse it.

That identity is **per step**, not per cycle: each step in a
cycle gets its own distinct descriptive title — never one shared
cycle title uniquified by a step marker. The cycle's chores
section header carries the anticipated *close-out* title. To keep
a cycle's commits collectable with one `git log --grep`, give the
step titles a common greppable stem (e.g. `config loader`).

## Notes file conventions

Conventions the bot follows when writing notes files
(`notes/todo.md`, `notes/todo-backlog.md`, `notes/bugs.md`,
`notes/chores/chores-NN.md`, `notes/done.md`). One source
of truth lives here; [`notes/README.md`](notes/README.md)
points back.

### Notes references

Reference *citations* are double-bracketed so the brackets render
— `[[N]]`, or `[[2]],[[3]]` for several (comma-separated, not
`[2,3]` or `[[2]][[3]]`). The `[N]:` definitions in a file's
`# References` section and inline `[text](url)` / `[text](#anchor)`
links stay single-bracketed.

### Reference numbering

Every note file (`todo.md`, `todo-backlog.md`, `bugs.md`,
`chores-NN.md`, `done.md`) keeps a file-local `# References`
section at the bottom. Reference numbers are scoped to that
file — `[1]` in `chores-07.md` and `[1]` in `chores-01.md`
are independent slots that may point at completely different
URLs. New chores files start their numbering at `[1]`.

Treat `[N]` like a **footnote**: the number is a local slot, only
meaningful within its file's `# References`. So a `[N]` *citation*
(bare `[N]`, or doubled `[[N]]`) never reuses another file's number; to cite a target a sibling
file references, pick your own next-local slot and define it
(the same target may carry a different number in each file). A
`[N]` *inside a code span* (`` `[72]` ``) is different — that's
a quoted identifier: literal text naming a ref-key (often from
another file's namespace), data, not a citation; it needs no
definition here. To point at a section of another file from
prose, use an inline link with an anchor —
`[that section](../chores-07.md#…)` — not a bare number.

A `chores-NN.md` `# References` entry is usually a
`/notes/<file>.md#anchor` (or `/ARCHITECTURE.md`) path, but may
also be a **commit reference** —
`[N]: <commit-url-with-12-hex-SHA> "<full-40-hex-SHA>"`, cited by
a section's `Commits:` line. See
[Chores commit references](#chores-commit-references) for the
why and the exact shape.

A file's `# References` can be **re-packed** to a contiguous
`[1]..[N]` in first-citation-appearance order — walk the file's
prose in document order (`todo.md`: `## Todo` then `## Done`;
`chores-NN.md`: top to bottom) and number refs as their first
`[[N]]` citation appears. This is a file-local rewrite — only
that file's `[[N]]` citations and `[N]:` definitions move; every
target and sibling file is untouched. A `[[N]]` inside a `` ` ``
code span is a literal token, not a citation, and is left alone.
Do it opportunistically (when the namespace has drifted enough to
annoy), not on a schedule: `todo.md` fragments fastest (entries
land and get pruned every cycle) and is the usual candidate;
`chores-NN.md` / `done.md` are append-mostly and only need it
after an unusual event (e.g. a bulk retrofit that allocated slots
out of document order).

### Markdown anchor links

GitHub anchor algorithm: lowercase, strip non-alphanumeric
characters in place, map remaining spaces to hyphens 1-for-1. Do
**not** collapse adjacent whitespace — so `a + b` → `a--b` (spaces
on both sides of `+`), but `a: b` → `a-b` (only trailing space on
`:`). General markdown reference:
[markdownguide.org](https://www.markdownguide.org). GitHub
publishes no official spec for auto-generated anchors; the
de-facto reference implementation is
[github-slugger](https://github.com/Flet/github-slugger).

### Todo format

`todo.md` is organized into `## In Progress`, `## Todo`
(strict priority rank, #1 highest; long-tail backlog in
[todo-backlog.md](notes/todo-backlog.md)), `## Ideas`,
`## Bugs` (pointer to [bugs.md](notes/bugs.md)), and
`## Done` sections. Each item is a short description with
reference links to more detail.

`## Todo` and `## Bugs` entries carry explicit `1.` `2.` …
numbers in the source. For `## Todo` the number is its
**priority rank** (#1 highest, descending); for `## Bugs`
it's just an index. They're for grepping and at-a-glance
"let's do #1" — **not stable IDs**: reorder (to
reprioritize), insert, or delete freely, then
`vc-x1 fix-todo --no-dry-run` renumbers and normalizes
continuation-line indent, so any given number is positional.
**To refer to a Todo durably, name it by its title — a
plain, greppable text mention.** Not its number (positional,
renumbered) and not a markdown link: a numbered list item has
no anchor to link to.

Numbering helps a human orient in a long list but makes links
difficult and fragile — especially an external reference
pointing in, which can't be auto-fixed when the list
renumbers. A robust fix (a number-free anchor, or a
number-tolerant dereference that matches the title slug and
wildcards the numeric prefix — a GitHub slug like `5-foo` is
encoded, not opaque, so the title is recoverable) is a
`validate-numbering` design question, out of scope here.

`vc-x1 fix-todo` alone only previews; `vc-x1 validate-todo`
is the read-only check. The `## Done` section keeps `-`
bullets — items aren't referenced by number once completed.

Example shape:

```
# Todo
1. Add new feature X [details](features.md#feature-x)
2. Fix bug Y [[1]]

# Done
- Fixed issue Z [[2]],[[3]]

[1]: bugs.md#bug-y
[2]: issues.md#issue-z
[3]: fixes.md#fix-z
```

### Retiring Done entries

`todo.md`'s `## Done` section is a rolling buffer of recently
shipped work, not a permanent log. Move entries into `done.md`
at two natural beats:

- **Closing a ladder** — when the final `X.Y.Z` (no suffix)
  commit ships, decide which prior entries are no longer
  needed for nearby context and migrate them.
- **Opening a new ladder** — at `X.Y.Z-0`, do the same sweep
  before bumping the version-of-record.

Migration mechanics:

- Move the bullet itself from `todo.md > ## Done` to
  `done.md` (preserving the original ref number).
- Copy any references the moved entries cite into
  `done.md`'s `# References` section (those refs are
  file-local, so coexisting with `todo.md`'s namespace is
  fine).
- Prune any references in `todo.md > # References` no longer
  cited by anything in `## In Progress` / `## Todo` /
  `## Done`. This frees the numbers for future reuse.

## Chores conventions

### Headings and entries that record a commit

A commit's title is reused verbatim across its records — see
[Conventional-commit shape](#conventional-commit-shape-ladder--chores--commit)
for the rule. Beyond the chores `##` header, that same string is
used for the matching `todo.md > ## Done` entry and any `[N]`
reference to that section. Titles carry **no `(<version>)`
suffix** — see
[Commit description](notes/cycle-protocol.md#commit-description)
for why.
E.g. the chores header `## refactor: extract config loader`
and the Done line `- refactor: extract config loader [[3]]`.
The `## Done` entry uses the close-out commit's title.

This does **not** apply to organizational headings (`## Todo`,
`## In Progress`, `# References`) or to design `###` subsections
inside a chores section — those are named for whatever fits.
Among the commit-recording ones, exact match is the strong
default (nothing absolute): a near-miss just makes it harder to
line a record up with its commit.

A commit-recording header is provisional while the work is in
progress; the *last* edit before `vc-x1 push` syncs it — and the
`## Done` entry / `[N]` anchor for that commit — to the final
commit title. See [Markdown anchor links](#markdown-anchor-links)
for the slug algorithm; the pre-commit checklist catches a
dangling `#anchor`, and a future `vc-x1 validate-repo` should too
(and should verify the recorded title matches the commit).

Any pre-existing sections and `## Done` entries that predate
this convention keep their free-form text; the convention
applies going forward.

### Chores section content — no edit list; git is the record

A chores section is: a `Commits:` line (first line under the
header — see below), then [Prose form](#prose-form) (intro +
bullets) for what landed and why, and any `###` design
subsections. Bullets here are **conceptual** — design points,
structural notes — never a per-file edit list. That lives in the
commit message body, which is the source of truth for "what
changed mechanically" (immutable, `git show`-able, naturally
scoped to the commit). The chores section is the source of truth
for the design thinking; the two cross-link, neither restates the
other.

When the intro starts wanting to explain a mechanism,
hypothesis, or wrinkle, don't inflate it — promote that to its
own `###` subsection inside the same `chores-NN.md`. If the
wrinkle is a live design concern (something that *should*
change, not just be recorded), also add a `notes/todo.md` item
with a `[N]` ref pointing at that subsection (todo→chores is the
normal ref direction).

**Why:** a chores edit list and the commit body were specified
to be the same content in two places — and detail written twice
drifts. Git owns the mechanical record; chores owns the
narrative; `Commits:` links them.

### Chores commit references

The first line under a chores section header is a `Commits:`
line citing the git commit(s) that section records:

```
## refactor: extract config loader

Commits: [[3]]

<intro paragraph...>
```

`Commits:` uses the file-local `[N]` reference machinery (see
[Reference numbering](#reference-numbering)),
**double-bracketed** so the brackets render — `Commits: [[3]]`,
or `Commits: [[3]],[[5]]` for several. (`[[3]]` shows as a
literal `[`, the `[3]` link, then a literal `]`; the inner
`[3]` resolves against its `[3]:` definition — CommonMark /
GitHub / VS Code all do this.) The `# References` definition
puts the **commit URL** as the destination, with the **full
40-hex SHA** in the title slot:

```
[3]: https://github.com/<owner>/<repo>/commit/<12-hex> "<40-hex>"
```

- The 12-hex short SHA in the URL keeps it short; GitHub /
  GitLab resolve a unique prefix to the canonical commit page
  (GitLab's path has a `/-/` before `commit/`).
- The full SHA in the title is host-agnostic and unambiguous —
  it survives a repo host change, `git show <40-hex>` works in
  any clone, and external tooling scraping the notes (a
  database, say) gets the canonical identifier.

**Timing.** A commit's SHA isn't stable until it lands on a
**permanent branch** (`main`, or a long-lived release/patch
branch) — a rebase or squash rewrites it on the way. A commit
can't record its own SHA, so the fill lands one push later:
every section opens with an **empty `Commits:`**, and each push
backfills the `Commits:` of the commits the previous push made
permanent. On a topic branch a section waits until the branch
lands. The commit itself is the record, and
`git log --grep "<title>"` finds it. See cycle-protocol
[Commits backfill](notes/cycle-protocol.md#commits-backfill).

## Cycle Protocol

Every change runs as a **cycle**: Preparation (`X.Y.Z-0`) →
Work commits (`X.Y.Z-1`, `X.Y.Z-2`, …) → Close-out (bare
`X.Y.Z`). Each commit runs through a per-commit flow whose
validation step is medium-specific — the Rust example is the
cargo cycle (fmt → clippy → test → install); skip-able for
notes-only commits, mandatory at close-out. The full protocol
— numbering, per-commit flow, reviewing changes, close-out,
pushing, ochid trailers, sub-cycles — lives in
[`notes/cycle-protocol.md`](notes/cycle-protocol.md). Read
it before any commit work — and before any push, cycle or
not: pushes always need per-push user approval (see
[Pushing policy](notes/cycle-protocol.md#policy)).

**Versioning.** How this repo numbers cycles, where the
version-of-record lives, and its bump cadence are
project-specific and documented in
[versioning.md](notes/versioning.md) — the single source of
truth that AGENTS.md and cycle-protocol.md refer to
abstractly.

**Hard stop after push/finalize.** Once a push or `.claude`
finalize is invoked (`vc-x1 push` does both), do no further
work — no verification, no summary, no next-step offers —
until the user speaks. Put all closing words *before* the
invoke. The harness rejects an empty turn, so it may force a
visible token after the tool returns; if so, emit a bare
acknowledgment only (e.g. "landed") — never a summary or more
work. Post-push verification happens next turn at the user's
direction. See
[After push or finalize](notes/cycle-protocol.md#after-push-or-finalize-stop-and-wait).

**`vc-x1 push` behaviors to keep in mind.** Two, independent
of project language:

- **Preflight** runs the cargo cycle (fmt → clippy → test). On
  a cargo project it works as-is, so `vc-x1 push <bookmark>`
  runs it before the review gate. On a project with no cargo
  cycle — or any time preflight can't run — invoke `vc-x1 push
  <bookmark> --from message ...` to skip preflight and the
  interactive review gate, doing the diff review in
  conversation first.
- **ochid trailers** are injected by `vc-x1 push` itself —
  don't hand-write them into the commit body or
  `--title`/`--body`.


## Code Conventions

### Doc comments on every file, function, and method

Every `.rs` file must begin with a `//!` module docstring. Every
function and method must have a `///` doc comment. Keep them brief —
one sentence of purpose is often enough; the discipline is that the
comment exists, not that it be long. Doc comments follow the
[Prose form](#prose-form) shape (intro + bullets).

This is a deliberate override of the generic "write no comments"
default that applies to inline `//` comments. Doc comments on the
module / item surface are expected; inline explanatory comments
inside function bodies remain discouraged unless they capture a
non-obvious WHY.

**Clap-derive args:** doc comments on `#[arg(...)]` fields drive
`--help` output. Clap reflows by default and collapses bullets
into running prose. Add `#[arg(verbatim_doc_comment, ...)]` on
any field whose doc comment uses bullets so each `- …` lands on
its own line in the rendered help.

### `// OK: …` comments on `unwrap*` calls (Rust)

Non-test code that calls `.unwrap()`, `.unwrap_or(…)`,
`.unwrap_or_default()`, or `.unwrap_or_else(…)` must have a trailing
`// OK: …` comment that justifies why the call is acceptable.

- `// OK: <specific reason>` — document the real precondition,
  invariant, or domain reason. Preferred whenever the reason isn't
  self-evident.
- `// OK: obvious` — the default is self-evident from context (e.g.
  `desc.lines().next().unwrap_or("")` — empty desc → empty title).

Bare `// OK` is not used (reads like a truncated comment).
Abbreviations (e.g. `SE`) are not used because they require a decoder
ring for readers seeing the code out of context.

For provably-unreachable `.unwrap()` calls, also prefix with
`#[allow(clippy::unwrap_used)]` so the site stays silent if we enable
the project-wide `clippy::unwrap_used` lint later.

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

Tests (`#[cfg(test)]`) are exempt — panicking on setup failure is the
correct test behavior.

### Ask for clarification on ambiguous input

When user input is ambiguous or missing necessary detail, stop and
ask a specific question. Do not proceed on a guess and hope the
result lands right — a clarifying question costs a few seconds;
redoing misaligned work costs much more.

### Recognize when stuck

If a simple task has eaten 5+ minutes of thinking or back-and-forth
without progress, stop. Summarize what's blocking — unclear
requirements, unfamiliar API surface, conflicting signals — and ask.
Continued flailing produces worse outcomes than a direct "I'm stuck
on X."
