# AGENTS.md - Agent Instructions

The universal core of the agent instructions: the dual-repo model, the hard rules, the cycle
protocol, and a map of the rest. One of the [agent-files](#terminology), carried by every adopter.
The rules are an index: each is a sentence and a link to the section that holds it, and a
section's why is in [rationale.md](agent-data/rationale.md) under the mirrored heading.

## Terminology

Repos: the two repos of [the dual-repo model](#the-dual-repo-model), "work-repo" and "agent-repo",
always hyphenated.

Agent-files: the instruction set an agent reads: `AGENTS.md`, `custom.md`, `agent-data/*`, and
anything `custom.md` points at. `TODO.md` is not one, since its content is the project's record,
but the agent-files require that there is one, of the shape in [Todo
format](agent-data/notes.md#todo-format).

Project layer: the project's own agent-files, `custom.md` and what it points at.

Set: the agent-files as the template repository's payload carries them, the copy every adopter
starts from and re-syncs to. Adopter: a project carrying a copy of the set. Maintainer: whoever
owns the template repository and decides what the payload takes.

Cycle: one change, run from opening to closing as one commit or a ladder of them, each made by
`vc-x1 push` ([Cycle protocol](#cycle-protocol)). Single-step when the problem statement has one
straightforward solution step, its documentation in the same commit, otherwise multi-step. Both run
on a bookmark under the dev name, and the shape is fixed at the cycle's first push ([Cycle
shape](#cycle-shape)).

Land: the sequence that makes a cycle permanent, on the user's go at the close-out
([Land](agent-data/jj.md#cycle-bookmarks-create-and-land)). Before it the cycle is a draft on its
bookmark, after it the commits are permanent.

Trapezoid: the default close-out shape, a merge commit whose first parent is the trunk line and
whose second is the cycle's ladder ([Close-out shapes](agent-data/jj.md#close-out-shapes)). Names
both the merge commit and the figure the graph draws around it.

Artifact: the work-repo's built product. It carries a `-dev` name while a cycle runs ([Dev artifact
name](agent-data/versioning.md#dev-artifact-name)) and is installed at Land.

Rationale: a rule's why, in [rationale.md](agent-data/rationale.md) under the heading that mirrors
the rule's, reached by `[why](agent-data/rationale.md#<same-slug>)`. Read when changing a rule,
not when following one.

## The dual-repo model

Two separate jj-git colocated repos ([jj.md](agent-data/jj.md)):

1. Work-repo: the project root, `.`, holding the project's work product.
2. Agent-repo: `<project>/.claude`, the agent's session data, reached by Claude Code through a
   symlink at `~/.claude/projects/<mangled-project-path>` (`vc-x1 symlink` creates it).

## Rules

The rules, indexed: each a one-sentence summary and a link to the section that states it, this
file's rules first and then the outer files' in their order ([why](agent-data/rationale.md#rules)).
The section is the rule, the sentence its handle. None is absolute: a rule bends only when the
user says so explicitly, at the moment or as a scoped delegation ([Stop and ask](#stop-and-ask) is
the path), and the exception is recorded in the cycle's records. No rule bends silently.

- Read custom.md first: read [custom.md](custom.md), whose rules override all others.
- Bookmark per cycle: a cycle runs on one topic bookmark in the work-repo, and `main` advances
  only when the cycle lands ([Cycles run on a bookmark](#cycles-run-on-a-bookmark)).
- Shape at the first push: single-step or multi-step is fixed by the cycle's first push, and a
  ladder lands as a trapezoid or kept separate, never squashed ([Cycle shape](#cycle-shape)).
- Read the step before the action: [The per-rung flow](#the-per-rung-flow) before commit work,
  [Before any push](#before-any-push) before a push, from the file, not from memory.
- Push commits: a cycle rung is committed only by `vc-x1 push` ([Committing vs
  pushing](#committing-vs-pushing)).
- Approval per push: every push needs the user's explicit approval, or an explicit waiver
  ([Before any push](#before-any-push)).
- Hard stop after the final push: after the turn's final push nothing until the user speaks,
  unless an explicit waiver ([At rest](#at-rest-push-stop-squash-push)).
- Stop and ask: on ambiguous input, on any deviation from the agreed plan, and when 5+ minutes
  on a simple task has produced no progress ([Stop and ask](#stop-and-ask)).
- Changing the agent-files: an agent-file change is its own commit and convention work its own
  cycle, and intent picks the file, the set's copy for the set, `custom.md` for this project
  only ([Changing the agent-files](#changing-the-agent-files)).
- jj, not git: version-control operations use jj ([jj basics](agent-data/jj.md#jj-basics)).
- No hand-written trailers: `vc-x1 push` stamps `ochid:` trailers, never write one by hand
  ([ochid trailers](agent-data/jj.md#cross-repo-linking-ochid-trailers)).
- No re-describe without coordinating: never `jj describe` a published or trailer-carrying
  commit ([Re-describing](agent-data/jj.md#re-describing-coordinate-first-and-keep-the-trailer)).
- Prose style: durable text is in the prose form, typeable punctuation included ([Prose
  form](agent-data/prose.md#prose-form)).
- One title per step: the ladder rung and the commit title are verbatim identical
  ([Conventional-commit shape](agent-data/prose.md#conventional-commit-shape-ladder--commit)).
- Alert on unwrap: say so when introducing an `unwrap` / `expect` / `unwrap_or*` site, with its
  `// OK: ...` comment ([`// OK` comments](agent-data/code.md#-ok--comments-on-unwrap-calls-rust)).

## Cycle protocol

How a [cycle](#terminology) runs ([why](agent-data/rationale.md#cycle-protocol)). Its record is
`TODO.md > ## In Progress` and nothing else ([Cycle-record](#cycle-record)). The `.vc-config.md`
`[validate]` table defines the commands that validate the work-repo.

### Cycles run on a bookmark

A cycle runs on one topic bookmark in the work-repo, created at the opening and named by the cycle
title's slug ([why](agent-data/rationale.md#cycles-run-on-a-bookmark)). `main` advances only when
the finished cycle lands on it, so development is never done on `main`, a single-step cycle
included. The agent-repo needs no bookmark. Commands in [Cycle
bookmarks](agent-data/jj.md#cycle-bookmarks-create-and-land), the long-lived case in [Long-lived
bookmarks][llb].

### Cycle shape

Single-step or multi-step is decided at the opening and fixed by the cycle's first push, with the
one coordinated exception below ([why](agent-data/rationale.md#cycle-shape)). Before that push the
In Progress block may be rewritten to either shape. After it:

- A single-step cycle is one commit, made by one push, carrying the opening's duties, the work, and
  the close-out's duties, titled with the bare cycle title in both repos. When the work turns out to
  need more steps, two choices, since the bookmark is still a draft:
  - land the commit as it is and run the additional work as one or more further cycles
  - turn the commit into the opening: amend its version-of-record to the `-0` form, re-title it with
    " opening" as a coordinated re-describe that keeps the `ochid:` trailer ([No re-describe without
    coordinating](agent-data/jj.md#re-describing-coordinate-first-and-keep-the-trailer)), and
    complete the cycle as multi-step.
- A multi-step cycle is a ladder and lands as a trapezoid or kept separate, never squashed: every
  rung's `ochid:` trailer is a change id, and a squash discards all but one. A ladder that shrinks
  to one rung still closes as a ladder, since its opening already pushed.

### Unplanned work

Work that arrives while a cycle runs goes one of two ways, and the user picks which
([why](agent-data/rationale.md#unplanned-work)):

- A rung inserted into the ladder, usually when it is inside the cycle's subject or blocks it.
- A `## Todo` or `## Waiting` entry, run as its own cycle later.

### Cycle-record

A cycle's record is its `TODO.md > ## In Progress` block and nothing else, the cycle-record
([why](agent-data/rationale.md#cycle-record)).

- Items: title, problem, solution, acceptance check, ladder, deliberation, and `Ladder details`, all
  provisional until close-out ([The In Progress block](agent-data/notes.md#the-in-progress-block)).
- Life: written at the opening, revised as rungs land, finalized by the closing commit, which moves
  it whole to `## Closed` (a single-step cycle's one commit writes it there directly), deleted by
  the next opening.
  - `## In Progress` reads `_No cycle currently in progress._` between cycles.
  - The closing commit's tree carries the final form, and the file never grows.
- After that jj holds it: `git log --grep "<cycle title>"` finds the commits, and the landmark on
  `main` (the trapezoid merge, or the single-step commit) holds the finished block in its
  `TODO.md > ## Closed`.
- No backfill: a rung carries no `[[N]]` placeholder, no SHA, and no version.
- Never amended: a late finding about a closed cycle is recorded where it is found, citing the
  landmark.
- Design findings that must outlive the cycle go into a `notes/` file by the rung that made them,
  never left in the block.
- Frozen history: `notes/chores/` and `notes/done.md` are never appended, still linked.

### Opening

The cycle's first commit, when it needs setup (a lightweight cycle starts at its first commit, which
then carries step 1). A single-step cycle does all of it in its one commit, after step 1 ([Cycle
shape](#cycle-shape)). Before that commit ([why](agent-data/rationale.md#opening)):

1. Bookmark: create and publish the cycle's bookmark, a push that needs approval.
2. Waiting: check each `## Waiting` entry's condition, and promote what is met into `## Todo` at
   the rank it names.
3. In Progress block: delete whatever `## Closed` holds, then move the chosen `## Todo` entry into
   `## In Progress`, shaped as [The In Progress block](agent-data/notes.md#the-in-progress-block)
   says, the specimen in [cycle-model.md](agent-data/cycle-model.md).
4. Bump: bump the version-of-record to the opening's version ([Suffix
   scheme](agent-data/versioning.md#suffix-scheme)).
5. Rename: when the built artifact has consumers, rename `<name>` to `<name>-dev` ([Dev artifact
   name](agent-data/versioning.md#dev-artifact-name)). Land restores it.

Rungs are named, not numbered ([Steps are named, not numbered][snn]), and a multi-step cycle's
bookends are the cycle title plus " opening" and " closing" ([Cycle bookend titles][cbt]).

### The per-rung flow

Every commit (opening, each rung between, closing) goes through these steps, read from here
immediately before acting ([why](agent-data/rationale.md#the-per-rung-flow)):

1. Mark current: mark the rung `(current)` in `TODO.md > ## In Progress`, as the first edit.
2. Bump: bump the version-of-record to this commit's version ([Suffix
   scheme](agent-data/versioning.md#suffix-scheme)).
3. Work: do the work. On any deviation from the agreed plan, or any question, stop ([Stop and
   ask](#stop-and-ask)).
4. Ladder details: write what this rung changed, conceptually, into its subsection. The rung stays
   `(current)` until step 7.
5. Validate: `vc-x1 validate` before every review, doc-only commits included. The full run rewrites
   files (`cargo fmt`), so use `--fast` while a review iterates.
6. Work review: stop before writing any description and say "please review", as its own message with
   no title or body. Iterate until the user says "continue" / "go". The review is of the
   working-copy diff ([jj basics](agent-data/jj.md#jj-basics)).
7. Flip and describe: flip `(current)` to `(done)` the moment "done" is true, then write the
   description in [Commit-body form](agent-data/prose.md#commit-body-form), read from the file first
   with its specimen, [commit-model.md](agent-data/commit-model.md) ([Commit
   description](#commit-description)).
8. Description review: show the title + body and stop. Ask permission to commit and push without
   spelling out the invocation. The go covers the push only when it says so.
9. Commit + push: on the go, `vc-x1 push <bookmark> --title "..." --body "..."` ([Committing vs
   pushing](#committing-vs-pushing)), then [At rest](#at-rest-push-stop-squash-push).

### Committing vs pushing

A cycle rung is committed *by* `vc-x1 push`, never pre-committed with `jj commit`
([why](agent-data/rationale.md#committing-vs-pushing)). "Commit", "push",
and "commit + push" all mean `vc-x1 push`. A bare `jj commit` is asked for by name and is for local
saves and [local ladder](#local-ladders) intermediates. What push does is in [vc-x1 push][vpush].

### Commit description

The title is a Conventional Commit, distinct within its cycle [Commit description details][cdd]).
The body is in [Commit-body form](agent-data/prose.md#commit-body-form): no version, file list, or
deliberation ([why](agent-data/rationale.md#commit-description)).

### Pushing

Pushing is by `vc-x1 push`. The bookmark moves (create, land, trapezoid) use `jj git push` as jj.md
names, until vc-x1 owns them.

#### Before any push

([why](agent-data/rationale.md#before-any-push))

- This specific push has the user's explicit approval, or an explicit waiver covers it.
- Validation ran, and passed, after the last edit.
- Closing words are written. Nothing follows the turn's final push.

#### At rest: push, stop, squash-push

The contract that keeps both repos clean, the rule **Hard stop after the final push** its first
item's tail ([why](agent-data/rationale.md#at-rest-push-stop-squash-push)):

1. The agent publishes: completing a step means issuing its publishing command. The agent says what
   is worth saying *before* the final publishing command, responds with the one word "Published",
   and does nothing further until the user speaks.
2. The user squash-pushes: `vc-x1 squash-push -R .claude` whenever they want both repos fully
   pushed.

"Clean" means both repos' `@` empty. A late work-repo tweak after the push is a remote rewrite and
takes approval like any push ([vc-x1 push][vpush]).

### Close-out

The cycle's last commit is bookkeeping and its body describes that bookkeeping. A single-step cycle
does all of it in its one commit, step 5 aside ([Cycle shape](#cycle-shape)):

1. Acceptance check: run the check the opening stated and record pass or fail. A failure is a
   finding, and why it failed is determined.
2. Finalize the cycle-record in place ([Cycle-record](#cycle-record)):
   - sync the title if the scope shifted, and every anchor back-reference with it
   - replace the provisional solution statement with what was done
   - drop the `(current)` / `(done)` markers
   - add the design subsections the deliberation grew
   - complete the closing rung's subsection
   - ask what in the block must outlive the cycle, and write it into the `notes/` file it
     belongs to
   - move the block whole to `## Closed`, leaving `## In Progress` reading
     `_No cycle currently in progress._`.
3. Validate: full validation, and update `notes/README.md` if functionality changed.
4. Size: record the agent-files line count in `notes/agent-files-size.md`, smaller being the
   quasi-goal.
5. Close-out shape ([Close-out shapes](agent-data/jj.md#close-out-shapes)):
   - choose with the user: trapezoid (the default) or keep separate
   - record the choice in the closing rung's subsection
   - reshape nothing yet, Land does.
6. Land: on the user's go, restore the plain name, reshape per the choice, fast-forward `main`,
   install the artifact, delete the bookmark locally and remotely ([Bookmark per
   cycle](#cycles-run-on-a-bookmark), [Land](agent-data/jj.md#cycle-bookmarks-create-and-land)).
7. Restart: the user restarts the agent, and before the exit anything the next agent needs is
   written into `TODO.md > ## Continuation notes`, the first section, which the next acquaint reads
   first and resets.

### Local ladders

A rung that wants incremental review runs as a local ladder: a chain of jj commits that never leaves
the machine and collapses into the rung before the cycle continues, each validated with
`vc-x1 validate --fast` ([Local ladders](agent-data/jj.md#local-ladders),
[why](agent-data/rationale.md#local-ladders)).

[cbt]: agent-data/prose.md#conventional-commit-shape-ladder--commit
[cdd]: agent-data/prose.md#conventional-commit-shape-ladder--commit
[llb]: agent-data/jj.md#long-lived-bookmarks-merge-only-by-default-deletable-once-merged
[snn]: agent-data/prose.md#steps-are-named-not-numbered
[vpush]: agent-data/jj.md#vc-x1-push-what-it-does-and-does-not-do

## Working practices

([why](agent-data/rationale.md#working-practices))

- One command per invocation: no bundled steps (`a && b; c`), except a genuine pipeline or a tight
  pair where the join is the point.
- Exit status: never mask a command's exit status. No piping a validating command into `tail` /
  `grep` (`${PIPESTATUS[0]}` when a pipe is wanted), no `&&` after a piped stage, no trailing
  `; echo "exit=$?"`. To report and still fail: `cmd || { rc=$?; echo failed=$rc; exit $rc; }`.
- Scratch files: repo-local `tmp/` (gitignored, `mkdir -p tmp` on demand). `/tmp` is for
  out-of-project temporaries.
- Slice reads: read the slice you need from long notes files. The acquaint read is `TODO.md`
  `offset=0, limit=60` ([notes.md](agent-data/notes.md#file-reads-read-the-slice-you-need)).
- https remotes: use https remotes, and check for an ssh remote first when a push dies at the
  network leg. A remote URL change needs the user's go.
- Delegate: mechanical subtasks go to lesser models (Opus/Haiku/Sonnet).
- No memory directory: `~/.claude/projects/<path>/memory/` is unused, the agent-files are.
- Speculation: mark it in durable text with "We think ..." ([Speculation
  marker](agent-data/prose.md#speculation-marker)).
- Plain synopsis: end a technical explanation in conversation with one, marked "The plain version:"
  ([Plain synopsis](agent-data/prose.md#plain-synopsis-after-technical-explanations)).

### Stop and ask

Stop and ask on ambiguous input, on any deviation from the agreed plan, and when 5+ minutes on a
simple task has produced no progress ([why](agent-data/rationale.md#stop-and-ask)).

## Changing the agent-files

The official copies are the template repository's payload, and every adopter carries its own
copy ([why](agent-data/rationale.md#changing-the-agent-files)).

- Payload read-only: only a *correction* (a factual error, a typo, a stale cross-reference) goes
  straight in.
- Intent picks the file: a rule change meant for the set goes into the local copy of the agent-file
  it lives in, reviewed at convergence on the diff. One meant for this project only goes to
  `custom.md` and says why.
- Diff is the proposal: the diff between an adopter and the payload *is* its open proposal set.
- Own commit, own cycle: an agent-file change is its own commit, and convention work is its own
  cycle. A convention itch mid-feature becomes a backlog entry, never an inserted rung.
- Local experiments: a local agent-file may hold an unagreed experiment. Diff against the payload
  when that matters.
- Convergence: the maintainer reviews the adopters' diffs, folds what it accepts into the payload,
  and every adopter re-syncs.
- Retirement: a resolved experiment retires like a finished Todo, adopted and rejected alike: the
  cycle that resolved it is its record.
- Adopted ahead: a rule adopted ahead of its convention cycle lives in the agent-file it belongs
  to, never in a holding section of the project layer.

## custom.md

[custom.md](custom.md) is the project's own layer and is never universal
([why](agent-data/rationale.md#custommd)). It ships holding only its own shape, and a project adds
what it needs. Anyone may change any agent-file. custom.md is provided so an adopter can experiment
with, or override, a rule in one file when that is practical, and keep its other agent-files
identical to the payload's, so a re-sync is a copy:

- Overriding: a rule the adopter cannot keep as written goes under `## Project conventions and
  overrides`, naming the section it supersedes.
- Editing instead: an adopter with reasons custom.md cannot serve edits the agent-files directly,
  and its diff from the payload is its proposal ([Changing the
  agent-files](#changing-the-agent-files)). Defining what the set itself is, as its first adopters
  are doing, is one such reason. Such an adopter's custom.md holds pointers to project files and
  nothing else, and `## Project conventions and overrides` stays `_None._`.
- Pointer entries: an entry that only points at a further file owes no justification, and an
  agent-file asking for something "in custom.md" is answered by following the pointer.
- Precedence: custom.md is loaded last and wins conflicts with the other agent-files.
