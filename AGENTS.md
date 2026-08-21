# AGENTS.md - Agent Instructions

The universal core of the agent instructions: the dual-repo model, the hard rules, the cycle
protocol, and a map of the rest. One of the [agent-files](#terminology), carried by every family
member. Each rule here is one line and a link: the line is the rule, the link is its mechanics,
and its why is in [rationale.md](agent-data/rationale.md) under the mirrored heading.

## Terminology

Repos: the two repos of [the dual-repo model](#the-dual-repo-model), "work-repo" and
"agent-repo", always hyphenated.

Agent-files: the instruction set an agent reads: `AGENTS.md`, `custom.md`, `agent-data/*`, and
anything `custom.md` points at.

Project layer: the project's own agent-files, `custom.md` and what it points at.

Cycle: one change, run from opening to closing as one commit or a ladder of them, each made by
`vc-x1 push` ([Cycle protocol](#cycle-protocol)). Single-step when the problem statement has one
straightforward solution step, its documentation in the same commit, otherwise multi-step. Both
run on a bookmark under the dev name.

Land: the sequence that makes a cycle permanent, on the user's go at the close-out
([Land](agent-data/jj.md#cycle-bookmarks-create-and-land)). Before it the cycle is a draft on
its bookmark, after it the commits are permanent and the records that wait on permanence come
due.

Trapezoid: the default close-out shape, a merge commit whose first parent is the trunk line and
whose second is the cycle's ladder ([Close-out shapes](agent-data/jj.md#close-out-shapes)).
Names both the merge commit and the figure the graph draws around it.

Artifact: the work-repo's built product. It carries a `-dev` name while a cycle runs
([Dev artifact name](agent-data/versioning.md#dev-artifact-name)) and is installed at Land.

Rationale: a rule's why, in [rationale.md](agent-data/rationale.md) under the heading that
mirrors the rule's, reached by `[why](agent-data/rationale.md#<same-slug>)`.

## The dual-repo model

Two separate jj-git colocated repos ([jj.md](agent-data/jj.md)):

1. Work-repo: the project root, `.`, holding the project's work product.
2. Agent-repo: `<project>/.claude`, the agent's session data, reached by Claude Code through a
   symlink at `~/.claude/projects/<mangled-project-path>` (`vc-x1 symlink` creates it).

## Rules

Rules are named so they can be referenced with a URL
([why](agent-data/rationale.md#hard-rules)). None is absolute: a rule bends only when wink says
so explicitly, at the moment or as a scoped delegation ([Stop and ask](#stop-and-ask) is the
path), and the exception is recorded in the cycle's records. No rule bends silently.

### Read custom.md first

Read [custom.md](custom.md). Its rules override all others.

### jj, not git

Version-control operations use jj. [jj basics](agent-data/jj.md#jj-basics).

### Push commits

A cycle rung is committed only by `vc-x1 push`.

### Approval per push

Every push needs approval except with an explicit waiver, [Before any push](#before-any-push).

### Hard stop after the final push

After the turn's final push nothing until the user speaks,
unless an explicit waiver. [At rest](#at-rest-push-stop-squash-push).

### No re-describe without coordinating

[Re-describing](agent-data/jj.md#re-describing-coordinate-first-and-keep-the-trailer) a commit
must be coordinated.

### No hand-written trailers

`vc-x1 push` stamps `ochid:` trailers, never write one by hand.
[ochid trailers](agent-data/jj.md#cross-repo-linking-ochid-trailers).

### Read the step before the action

Read [The per-rung flow](#the-per-rung-flow) before commit work and
[Before any push](#before-any-push) before a push, from the file, not from memory.

### Typeable punctuation

No em/en dash, ellipsis, or arrow characters in durable text, see
[Typeable punctuation](agent-data/prose.md#typeable-punctuation-only).

### One title per step

The ladder rungs, headers and commit title are verbatim identical, see
[the shape](agent-data/prose.md#conventional-commit-shape-ladder--chores--commit).

### Stop and ask

Stop and ask on ambiguous input, on any deviation from the agreed plan, and when 5+ minutes on
a simple task has produced no progress.

### Alert on unwrap

When writing Rust, inform the user of every `unwrap*` call outside tests
([code.md](agent-data/code.md#-ok--comments-on-unwrap-calls-rust)).

### Changing agent-files

A change meant for the family is edited into the local copy of the file it lives in, one not
meant for the family goes in `custom.md`, see
[Changing the agent-files](#changing-the-agent-files).

### Bookmark per cycle

A cycle runs on one bookmark in the work-repo, see [Cycles run on a bookmark](#cycles-run-on-a-bookmark).

## Cycle protocol

How a [cycle](#terminology) runs ([why](agent-data/rationale.md#cycle-protocol)). Its record is
`TODO.md > ## In Progress` while it runs and moves whole to `notes/chores/` when it closes. The
`.vc-config.md` `[validate]` table defines the commands that validate the work-repo.

### Cycles run on a bookmark

A cycle runs on one topic bookmark in the work repo, created at the opening and named by the
cycle title's slug ([why](agent-data/rationale.md#cycles-run-on-a-bookmark)). `main` advances
only when the finished cycle lands on it, so development is never done on `main`, a
single-step cycle included. The agent repo needs no bookmark. Commands in
[Cycle bookmarks](agent-data/jj.md#cycle-bookmarks-create-and-land), the long-lived case in
[Long-lived bookmarks][llb].

### Opening

The cycle's first commit, when it needs setup (a lightweight cycle starts at its first commit,
which then carries step 1). Before that commit ([why](agent-data/rationale.md#opening)):

1. Bookmark: create and publish the cycle's bookmark, a push that needs approval.
2. In Progress block: move the chosen `## Todo` entry into the block, shaped as
   [The In Progress block](agent-data/notes.md#the-in-progress-block) says, the specimen in
   [cycle-model.md](agent-data/cycle-model.md).
3. Sweep: sweep `## Done` ([Retiring Done entries][rde]).
4. Bump: bump the version-of-record to the opening's version
   ([Suffix scheme](agent-data/versioning.md#suffix-scheme)).
5. Rename: when the built artifact has consumers, rename `<name>` to `<name>-dev`
   ([Dev artifact name](agent-data/versioning.md#dev-artifact-name)). Land restores it.

Rungs are named, not numbered ([Steps are named, not numbered][snn]), and a multi-step cycle's
bookends are the cycle title plus " opening" and " closing" ([Cycle bookend titles][cbt]).

### The per-rung flow

Every commit (opening, each rung between, closing) goes through these steps, read from here
immediately before acting ([why](agent-data/rationale.md#the-per-rung-flow)):

1. Mark current: mark the rung `(current)` in `TODO.md > ## In Progress`, as the first edit.
2. Bump: bump the version-of-record to this commit's version
   ([Suffix scheme](agent-data/versioning.md#suffix-scheme)).
3. Work: do the work. On any deviation from the agreed plan, or any question, stop
   ([Stop and ask](#stop-and-ask)).
4. Ladder details: write what this rung changed, conceptually, into its subsection. The rung
   stays `(current)` until step 7.
5. Validate: `vc-x1 validate` before every review, doc-only commits included. The full run
   rewrites files (`cargo fmt`), so use `--fast` while a review iterates.
6. Work review: stop before writing any description and say "please review", as its own message
   with no title or body. Iterate until the user says "continue" / "go". The review is of the
   working-copy diff ([jj basics](agent-data/jj.md#jj-basics)).
7. Flip and describe: flip `(current)` to `(done)` the moment "done" is true, then write the
   description in [Commit-body form](agent-data/prose.md#commit-body-form), read from the file
   first ([Commit description](#commit-description)).
8. Description review: show the title + body and stop. Ask permission to commit and push without
   spelling out the invocation. The go covers the push only when it says so.
9. Commit + push: on the go, `vc-x1 push <bookmark> --title "..." --body "..."`
   ([Committing vs pushing](#committing-vs-pushing)), then
   [At rest](#at-rest-push-stop-squash-push).

### Committing vs pushing

A cycle rung is committed *by* `vc-x1 push`, never pre-committed with `jj commit`
([Push commits](#push-commits), [why](agent-data/rationale.md#committing-vs-pushing)). "Commit",
"push", and "commit + push" all mean `vc-x1 push`. A bare `jj commit` is asked for by name and is for local saves and
[local ladder](#local-ladders) intermediates. What push does is in [vc-x1 push][vpush].

### Commit description

The title is a Conventional Commit, distinct within its cycle [Commit description details][cdd]).
The body is in [Commit-body form](agent-data/prose.md#commit-body-form): no version, file list, or
deliberation ([why](agent-data/rationale.md#commit-description)).

### Pushing

Pushing is by `vc-x1 push`. The bookmark moves (create, land, trapezoid) use `jj git push` as
jj.md names, until vc-x1 owns them.

#### Before any push

- This specific push has the user's explicit approval.
- Validation ran, and passed, after the last edit.
- Closing words are written. Nothing follows the turn's final push.

#### At rest: push, stop, squash-push

The contract that keeps both repos clean,
[Hard stop after the final push](#hard-stop-after-the-final-push) its first item's tail
([why](agent-data/rationale.md#at-rest-push-stop-squash-push)):

1. The agent publishes: completing a step means issuing its publishing command. The agent says
   what is worth saying *before* the final publishing command, responds with the one word
   "Published", and does nothing further until the user speaks.
2. The user squash-pushes: `vc-x1 squash-push -R .claude` whenever they want both repos fully
   pushed.

"Clean" means both repos' `@` empty. A late work-repo tweak after the push is a remote rewrite
and takes approval like any push ([vc-x1 push][vpush]).

### Close-out

The cycle's last commit is bookkeeping and its body describes that bookkeeping:

1. Acceptance check: run the check the opening stated and record pass or fail. A failure is a
   finding, and why it failed is determined.
2. Finalize: sync the title if the scope shifted (and every anchor back-reference), replace the
   provisional solution statement with what was done, drop the `(current)` / `(done)` markers,
   add the design subsections the deliberation grew, complete the closing rung's subsection.
3. Move and record: move the block into `notes/chores/chores-NN.md`, add the `## Table of
   Contents` entry, write the `## Done` entry ([The close-out move][tcm]).
4. Validate: full validation, and update `notes/README.md` if functionality changed.
5. Close-out shape: choose with the user, record the choice in the closing rung's subsection,
   reshape nothing yet ([Close-out shapes](agent-data/jj.md#close-out-shapes)): trapezoid (the
   default), keep separate, or squash.
6. Land: on the user's go, restore the plain name, reshape per the choice, fast-forward `main`,
   install the artifact, delete the bookmark locally and remotely
   ([Bookmark per cycle](#bookmark-per-cycle), [Land](agent-data/jj.md#cycle-bookmarks-create-and-land)).

### Local ladders

A rung that wants incremental review runs as a local ladder: a chain of jj commits that never
leaves the machine and collapses into the rung before the cycle continues, each validated with
`vc-x1 validate --fast` ([Local ladders](agent-data/jj.md#local-ladders),
[why](agent-data/rationale.md#local-ladders)).

[cbt]: agent-data/prose.md#conventional-commit-shape-ladder--chores--commit
[cdd]: agent-data/prose.md#conventional-commit-shape-ladder--chores--commit
[llb]: agent-data/jj.md#long-lived-bookmarks-merge-only-by-default-deletable-once-merged
[rde]: agent-data/notes.md#retiring-done-entries
[snn]: agent-data/prose.md#steps-are-named-not-numbered
[tcm]: agent-data/notes.md#the-close-out-move
[vpush]: agent-data/jj.md#vc-x1-push-what-it-does-and-does-not-do

## Working practices

([why](agent-data/rationale.md#working-practices))

- One command per invocation: no bundled steps (`a && b; c`), except a genuine pipeline or a
  tight pair where the join is the point.
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
- Speculation: mark it in durable text with "We think ..."
  ([Speculation marker](agent-data/prose.md#speculation-marker)).
- Plain synopsis: end a technical explanation in conversation with one, marked "The plain
  version:" ([Plain synopsis](agent-data/prose.md#plain-synopsis-after-technical-explanations)).

## Changing the agent-files

The official copies are the template repository's payload, and every member repo carries its
own copy ([why](agent-data/rationale.md#changing-the-agent-files)).

- Payload read-only: only a *correction* (a factual error, a typo, a stale cross-reference)
  goes straight in.
- Intent picks the file: a family-wide rule change goes into the local copy of the pinned file,
  reviewed at convergence on the diff. One not meant for the family goes to `custom.md` and
  says why.
- Diff is the proposal: the diff between a member and the payload *is* its open proposal set.
- Own commit, own cycle: an agent-file change is its own commit, and convention work is its own
  cycle. A convention itch mid-feature becomes a backlog entry, never an inserted rung.
- Local experiments: a local agent-file may hold an unagreed experiment. Diff against the
  payload when that matters.
- Convergence: the family reviews the members' diffs, folds what it accepts into the payload,
  and every member re-syncs.
- Retirement: a resolved experiment retires like a finished Todo, adopted and rejected alike
  ([Retiring Done entries][rde]).
- Adopted ahead: a rule adopted ahead of its convention cycle lives in the pinned file it
  belongs to, never in a holding section of the project layer.

## custom.md

[custom.md](custom.md) is the project's own layer and is never universal
([why](agent-data/rationale.md#custommd-the-project-layer)). It ships holding only its own
shape, and a project adds what it needs.

- Overrides section: `## Project conventions and overrides` stays `_None._` unless a rule
  cannot be family-wide. A rule the project would keep is a proposal, and goes in the pinned
  file as a diff ([Changing the agent-files](#changing-the-agent-files)).
- Pointer entries: an entry that only points at a further file owes no justification, and a
  pinned file asking for something "in custom.md" is answered by following the pointer.
- Precedence: custom.md is loaded last and wins conflicts with the other agent-files.
