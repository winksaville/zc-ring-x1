# AGENTS.md - Agent Instructions

The universal core of the agent instructions: the dual-repo model, the hard rules, the cycle
protocol, and a map of the rest. One of the [agent-files](#terminology), carried by every adopter.
The rules are an index: each is a sentence and a link to the section that holds it, and a
section's why is in [rationale.md](agent-data/rationale.md) under the mirrored heading.

## Terminology

Repos: the two repos of [the dual-repo model](#the-dual-repo-model), "work-repo" and "agent-repo",
always hyphenated.

Agent-files: the instruction set an agent reads, `AGENTS.md` and `agent-data/*`. `TODO.md` is not
one, since its content is the project's record, but the agent-files require that there is one, of
the shape in [Todo format](agent-data/notes.md#todo-format), and a `zTOC.md` beside it holding the
cycle's ladder and the files worth knowing ([zTOC format](agent-data/notes.md#ztoc-format)).

Agent-files version: the set's own version, `vX.Y.Z`, the name of an empty
`agent-data/agent-files-vX.Y.Z` file, bumped by `agent-files` proposal cycles and copied by
adoptions ([Agent-files version](agent-data/versioning.md#agent-files-version)).

Set: the agent-files as an adopter carries them, the copy a new repo starts from, applied by
`scripts/vc-dr-apply.sh` from that adopter's working copy, or with `--landed` from its `main`.
Adopter: a project carrying a copy of the set.
Proposer: the adopter a proposal comes from. There is no maintainer and no template repository, only
adopters and proposers, and the copy a proposal starts from is whichever adopter's the proposer
took.

Cycle: one change, run from opening to closing as one commit or a ladder of them, each made by
`vc-x1 cycle`, whose stages are its flags ([Cycle protocol](#cycle-protocol)). Its shape is its
branch's commit count, a single-step cycle being one commit and a multi-step one more ([Cycle
shape](#cycle-shape)). Both run on a bookmark, and both stay on this machine until the Land unless
the user decides otherwise.

Land: the sequence that makes a cycle permanent, on the user's go at the close-out: sync, integrate,
publish, install ([Land](agent-data/jj.md#cycle-bookmarks-create-and-land)). `vc-x1 cycle --land`
names its integrate step alone, and a rule writes "Land" for the sequence and `cycle --land` for
the command. Land is onto the cycle's target, a fast-forward unless the user asks for a merge.
Before it the cycle is a draft on its bookmark, after it the commits are permanent.

Target: the bookmark a cycle lands on, named on the command each time, `cycle --land <target>` and
a trapezoid's `--trapezoid <target>`, stored nowhere, and never assumed, since a bookmark made is
likely, not certainly, the one a cycle lands on. `main` by convention, and a release branch is a
target as much as `main` is.

Draft push and publish: the two kinds of push. A draft push is of the cycle's own bookmark, at any
rung or after the closing, and leaves the cycle a draft. The publish is the Land's push of the
target, which makes the cycle permanent and removes the draft.

Trapezoid: the default close-out shape, a merge commit whose first parent is the target's line and
whose second is the cycle's ladder ([Close-out shapes](agent-data/jj.md#close-out-shapes)). Names
both the merge commit and the figure the graph draws around it.

Artifact: the work-repo's built product. A cycle runs its own dev build, installed beside the plain
name ([Dev artifact name](agent-data/versioning.md#dev-artifact-name)), and the plain name is
installed at Land.

Rationale: a rule's why, in [rationale.md](agent-data/rationale.md) under the heading that mirrors
the rule's, reached by `[why](agent-data/rationale.md#<same-slug>)`. Read when changing a rule,
not when following one.

## The dual-repo model

Two separate jj-git colocated repos ([jj.md](agent-data/jj.md)), located by a `.vc-config.md`,
never by the current directory ([.vc-config.md](agent-data/jj.md#vc-configmd),
[why](agent-data/rationale.md#the-dual-repo-model)):

1. Work-repo: the project's work product. Its directory is the workspace root, the directory
   holding the work-repo's `.vc-config.md`, whose `[repos] work` is `"."`.
2. Agent-repo: the agent's session data, in the directory that file's `[repos] agent` names, a
   path relative to the workspace root or an absolute one, so it may sit anywhere. Written
   `<agent-dir>` wherever a command below needs it, and reached by Claude Code through a symlink
   at `~/.claude/projects/<mangled-project-path>`, which `vc-x1 init` creates for a dual
   workspace and `vc-x1 symlink` creates on demand.

## Rules

The rules, indexed: each a one-sentence summary and a link to the section that states it, this
file's rules first and then the outer files' in their order ([why](agent-data/rationale.md#rules)).
The section is the rule, the sentence its handle. None is absolute: a rule bends only when the
user says so explicitly, at the moment or as a scoped delegation ([Stop and ask](#stop-and-ask) is
the path), and the exception is recorded in the cycle's records, naming what the bend covers and
what it does not, so a waiver over a cycle's pushes says whether Land is inside it. No rule bends
silently.

- Read custom.md first: when a `custom.md` exists, the agent must read it first, it overrides
  anything in the agent-files, and it holds what is local to the workspace
  ([custom.md](#custommd)).
- Acquaint before work: a session acquaints first, and what its repo check finds is reported and
  left as found until the user says how to proceed ([Acquaint](#acquaint)).
- Sync where `main` matters: a look at acquaint and a sync at the opening, the closing, and Land,
  each reported, and a repo held back stops the step ([Sync](#sync)).
- A session's rules are its own agent-files: the ones it started in, binding in every repo it
  writes, and another repo's rules bind only by delegation ([A session's rules are its own
  agent-files](#a-sessions-rules-are-its-own-agent-files)).
- Bookmark per cycle: a cycle runs on one topic bookmark in the work-repo, and `main` advances
  only when the cycle lands ([Cycles run on a bookmark](#cycles-run-on-a-bookmark)).
- Shape named, count checked: the closing names the cycle single-step or multi-step, its branch's
  commit count checks the name, and a ladder lands as a trapezoid, never squashed ([Cycle
  shape](#cycle-shape)).
- Local by default: a cycle's commits stay on this machine until the Land, and the user decides when
  each side commits and publishes ([A cycle is local by default](#a-cycle-is-local-by-default)).
- Read the step before the action: [The per-rung flow](#the-per-rung-flow) before commit work,
  [Before any push](#before-any-push) before a push, from the file, not from memory.
- Commit by the tool: a cycle's commits are made only by `vc-x1 cycle`, a rung by `--commit` and the
  closing by `--close` ([Committing vs pushing](#committing-vs-pushing)).
- The ladder is the tool's: a rung is written once, as its heading under `#### Rungs`, and no
  marker is typed ([Cycle-record](#cycle-record)).
- The version is the project's: `vc-x1` never bumps it, and refuses a commit that leaves it
  unchanged when the config asks ([The per-rung flow](#the-per-rung-flow)).
- Approval per push: only a push publishes, and every push needs the user's explicit approval, or an
  explicit waiver ([Before any push](#before-any-push)).
- Hard stop after the final push: after the turn's final push nothing until the user speaks,
  unless an explicit waiver ([At rest](#at-rest)).
- One command per invocation: no bundled steps, except a genuine pipeline or a tight pair
  ([Working practices](#working-practices)).
- Exit status: the command under test runs bare and last, `set -o pipefail` first when piped, and
  `bash-guard.sh` denies the masks it can see ([Working practices](#working-practices)).
- Long output: a command known to print more than the Bash tool returns keeps its output in a file
  and shows its tail, and a pass is its exit status ([Working practices](#working-practices)).
- Stop and ask: on ambiguous input, on any deviation from the agreed plan, and when 5+ minutes
  on a simple task has produced no progress ([Stop and ask](#stop-and-ask)).
- Changing the agent-files: a change to the rules is tried in the local copy, then proposed as one
  set of diffs, which an adopter accepts as is, negotiates, or declines ([Changing the
  agent-files](#changing-the-agent-files)).
- jj, not git: version-control operations use jj ([jj basics](agent-data/jj.md#jj-basics)).
- No hand-written trailers: the tool stamps the trailers, never write one by hand
  ([ochid trailers](agent-data/jj.md#cross-repo-linking-ochid-trailers)).
- No re-describe without coordinating: never `jj describe` a published or trailer-carrying
  commit ([Re-describing](agent-data/jj.md#re-describing-coordinate-first-and-keep-the-trailer)).
- Prose style: durable text is in the prose form, typeable punctuation included ([Prose
  form](agent-data/prose.md#prose-form)).
- One title per step: the ladder rung and the commit title are verbatim identical
  ([Conventional-commit shape](agent-data/prose.md#conventional-commit-shape-ladder--commit)).
- Alert on unwrap: say so when introducing an `unwrap` / `expect` / `unwrap_or*` site, with its
  `// OK: ...` comment ([`// OK` comments](agent-data/code.md#-ok--comments-on-unwrap-calls-rust)).

## Acquaint

What a session does first, at its start and whenever the user says "acquaint" or "reacquaint",
in this order ([why](agent-data/rationale.md#acquaint)):

1. custom.md: when there is one, read it, and do what it asks for at acquaint
   ([custom.md](#custommd)).
2. Hooks: `bash agent-data/hooks.sh check`, and what it finds missing, different, or
   unregistered is reported for the user to install ([Working practices](#working-practices)).
3. Tools: `vc-x1 -V` and `vc-x1-dev -V`, both reported, and a dev build newer than the plain one
   is the user's to choose ([Dev artifact name](agent-data/versioning.md#dev-artifact-name)).
4. Repo check: look at each repo of the workspace for the four findings below, and report what is
   found before anything else. Nothing is opened, pushed, rebased, committed, or discarded until
   the user says how to proceed.
   - Not up to date: a repo whose `main` is behind, ahead of, or diverged from its remote's, as
     the look reports it ([Sync](#sync)), run again with `--bookmark <bookmark>` when a cycle is
     in flight. A look that fails is reported as the check not made.
   - Uncommitted work: a work-repo `@` that is not empty, as `jj st` prints it. What the
     continuation notes account for, the notes themselves or a rung in flight they describe, is
     theirs to direct at step 5, and any other change is a finding. The agent-repo's `@` holds the
     session's own data and is never one.
   - Work off `main`: commits on a bookmark, a cycle's or any other, that do not descend from the
     remote's `main`. The user is told, and a rebase onto `main` is suggested.
   - Conflicts: a conflicted commit or working copy, as `jj st` and `jj log -r 'conflicts()'` show
     them. The acquaint stops there and the user is informed.
5. TODO.md: read the acquaint slice ([File
   reads](agent-data/notes.md#file-reads-read-the-slice-you-need)), the continuation notes first,
   each acted on, filed or kept, and the section reset ([Todo
   format](agent-data/notes.md#todo-format)).
6. Report: say where things stand, the cycle in flight, its ladder in `zTOC.md`, or the top of `##
   Todo`, and wait.

## Sync

One command, `vc-x1 sync -s b`, brings both repos up to date, each `main` at the commit
its remote's `main` is at. It runs at the moments a cycle depends on `main`, and what it prints
is reported ([why](agent-data/rationale.md#sync)):

- Acquaint looks: with `--dry-run`, which moves nothing, so each repo is reported as found.
- Opening syncs: as its first step, which is its rung's step 0, so the cycle's bookmark is
  created on an up-to-date `main`.
- Closing syncs: at its rung's step 0, so the record and version it carries are made from an
  up-to-date `main`. A rung between may sync at its step 0, the agent's or the user's call.
- Land syncs: as its first step, and a `main` the sync moved stops the Land.

A sync that fails or holds a repo back stops the step until the user says how to proceed, as does
one at a rung's step 0 that moved a `main`, and `--force` and `--rebase` are the user's to ask
for.

## Cycle protocol

How a [cycle](#terminology) runs ([why](agent-data/rationale.md#cycle-protocol)). Its record is
`TODO.md > ## In Progress` and nothing else ([Cycle-record](#cycle-record)). The `.vc-config.md`
`[validate]` table defines the commands that validate the work-repo.

### Cycles run on a bookmark

A cycle runs on one topic bookmark in the work-repo, made first by `vc-x1 cycle --open` and named
by the cycle title's slug ([why](agent-data/rationale.md#cycles-run-on-a-bookmark)). `main`
advances only when the finished cycle lands on it, so development is never done on `main`, a
single-step cycle included. The agent-repo needs no bookmark. Commands in [Cycle
bookmarks](agent-data/jj.md#cycle-bookmarks-create-and-land), the long-lived case in [Long-lived
bookmarks][llb].

### Cycle shape

A cycle's shape is named at its closing, `--single-step` or `--multi-step`, and its branch's commit
count checks the name, so nothing is decided before the closing and nothing is guessed at it
([why](agent-data/rationale.md#cycle-shape)). Every cycle has an opening and a closing, and the
shape says whether they are one commit.

- Every cycle starts with `vc-x1 cycle --open`, which makes its bookmark on the work `@` and
  writes its record, a single-step cycle included. A cycle's first commit, by `cycle --commit`, is
  its opening, which the tool sees and no flag says.
- Single-step, no commit on the branch, the work in `@`: `cycle --close --single-step --body
  "..."` commits `@` as its one commit, under the bare cycle title, and it carries the opening's
  duties, the work, and the close-out's duties.
- Single-step, one commit, an opening committed first: `cycle --close --single-step` squashes the
  closing into it, `@` folded in, the title losing " opening" and gaining no " closing".
- Multi-step, a ladder: `cycle --close --multi-step` makes the closing commit, from the work `@` on
  the cycle's tip, over one commit or more, and is refused over none. Single-step is refused over
  two or more. `--trapezoid <target>` makes it the merge of the target and the tip, each revision
  listed, `cycle --close <REV,...>`, is another head joined, and `--land <target>` lands it in the
  same invocation.
- A ladder lands as a trapezoid, never squashed: every rung's `ochid:` trailer is a change id, and a
  squash discards all but one. Without `--trapezoid` the closing is linear, a plain commit on the
  ladder, which is the user's to ask for.
- The titles and bodies default. The opening's is the cycle title plus " opening" and `[opening]
  body`, a ladder's closing the title plus " closing" and `[closing] body`, and a single-step
  cycle's one commit gives its `--body`, since it holds the work. A work rung gives both.
- A dead end still closes and lands: a cycle abandoned after its opening keeps what is worth
  keeping, reverts the rest, and records what was tried, why it stopped, and what replaces it. Its
  closing is `cycle --close --multi-step --dead-end`, titled with the cycle title plus " dead-end
  closing".

### Unplanned work

Work that arrives while a cycle runs goes one of two ways, and the user picks which
([why](agent-data/rationale.md#unplanned-work)):

- A rung inserted into the ladder, usually when it is inside the cycle's subject or blocks it.
- A `## Todo` or `## Waiting` entry, run as its own cycle later.

A rung inserted while the working copy already holds the next rung's edits sets those edits aside
first, as a jj commit with a local bookmark, never a patch file, so the inserted rung's commit
carries its own change alone, and they return once it is made.

### Cycle-record

A cycle's record is its `TODO.md > ## In Progress` block and nothing else, the cycle-record
([why](agent-data/rationale.md#cycle-record)).

- Items: title, problem, solution, acceptance check, deliberation, and the rung subsections, the
  ladder over them being `zTOC.md`'s, all provisional until close-out ([The In Progress
  block](agent-data/notes.md#the-in-progress-block)).
- Life: written at the opening, revised as rungs land, finalized by the closing commit, which moves
  it whole to `## Closed` (a single-step cycle's one commit writes it there directly), deleted by
  the next opening.
  - `## In Progress` reads `_No cycle currently in progress._` between cycles.
  - Its ladder, each rung with its marker, is `zTOC.md`'s, made by the tool from the rung headings
    under `#### Rungs` and rewritten into every cycle commit, each marker computed from the cycle's
    commits. It is kept by the closing so the landmark holds it and replaced by the next opening
    ([zTOC format](agent-data/notes.md#ztoc-format)).
  - Every commit of a cycle is a rung's: a commit whose title is no rung heading is refused, so an
    unplanned rung gets its heading before its commit.
  - The closing commit's tree carries the final form, and the file never grows.
- After that jj holds it: `git log --grep "<cycle title>"` finds the commits, on the title the
  pushed bookends carry when a rename came mid-cycle, and the landmark on `main` (the trapezoid
  merge, or the single-step commit) holds the finished block in its `TODO.md > ## Closed`.
- No backfill: a rung carries no `[[N]]` placeholder, no SHA, and no version.
- Never amended: a late finding about a closed cycle is recorded where it is found, citing the
  landmark.
- May be reworked while local: while a cycle's work commits are local and its agent `@` uncommitted,
  an earlier commit's record may be reworked. By default it is unnecessary, a later rung revising
  the record, and it is permitted when the error is significant, a word left to whoever is reading.
  A reworked commit's tree is validated again.
- Design findings that must outlive the cycle go into a `notes/` file by the rung that made them,
  never left in the block.
- Frozen history: `notes/chores/` and `notes/done.md` are never appended, still linked.

### Opening

The cycle's first commit carries its plan: the record `cycle --open` scaffolds is deliberated and
laddered before it is committed, so the opening's review is of the plan, where a wrong direction
costs the least. The user is free to commit the setup first. A single-step cycle does all of it in
its one commit ([Cycle shape](#cycle-shape)). Before that commit
([why](agent-data/rationale.md#opening)):

1. Sync: sync both repos ([Sync](#sync)), whatever the cycle's form.
2. Open: `vc-x1 cycle --open --title "<bare cycle title>"` makes the bookmark on the work `@`,
   kept local, first, so every edit of the cycle is on its branch. It then writes the record: what
   `## Closed` holds is deleted, the `## Todo` entry of that title is moved into the `## In
   Progress` scaffold ([The In Progress block](agent-data/notes.md#the-in-progress-block), the
   specimen in [cycle-model.md](agent-data/cycle-model.md)), and `zTOC.md`'s ladder is replaced
   with the opening and the closing.
3. Waiting: check each `## Waiting` entry's condition, and promote what is met into `## Todo` at
   the rank it names.
4. Deliberate and plan: write the problem, the solution, the acceptance check, and the
   deliberation, and ladder the work rungs, each a `##### ` heading under `#### Rungs` with its
   intent. `vc-x1 cycle --ladder` makes the ladder from the headings. A small cycle's deliberation
   reads `_None._`, and the plan stays provisional, later rungs revising it.
5. Bump: bump the version-of-record to the opening's version ([Suffix
   scheme](agent-data/versioning.md#suffix-scheme)), by the project's own tool, and the agent-files
   version with it when the cycle is an `agent-files` proposal ([Agent-files
   version](agent-data/versioning.md#agent-files-version)).

Rungs are named, not numbered ([Steps are named, not numbered][snn]), and a multi-step cycle's
bookends are the cycle title plus " opening" and " closing" ([Cycle bookend titles][cbt]).

### The per-rung flow

Every commit (opening, each rung between, closing) goes through these steps, read from here
immediately before acting ([why](agent-data/rationale.md#the-per-rung-flow)). `vc-x1 cycle
--status` says where the cycle stands and which rung is next:

0. Sync: sync both repos ([Sync](#sync)), required at the opening and the closing, optional
   between, the agent's or the user's call.
1. Bump: bump the version-of-record to this commit's version ([Suffix
   scheme](agent-data/versioning.md#suffix-scheme)), by the project's own tool, the agent-files
   version beside it in an `agent-files` proposal cycle. The version is the project's and `vc-x1`
   never writes it. With `[version]` in the config it refuses a commit that leaves the version
   unchanged, and `--same-version`, which commits anyway, is the user's to allow.
2. Work: do the work. On any deviation from the agreed plan, or any question, stop ([Stop and
   ask](#stop-and-ask)).
3. Rung subsection: write what this rung changed, conceptually, into its subsection under `####
   Rungs`.
4. Validate: `vc-x1 validate` before every review, doc-only commits included. The full run rewrites
   files (`cargo fmt`), so use `--fast` while a review iterates. A failure is read and its cause
   named before validation runs again, and a failure that does not repeat is a finding, reported
   and given a Todo entry. A rung that changes the artifact then installs the working copy's dev
   build, `vc-x1 install --profile dev`, so the review runs the code under review, and again after
   each review fix ([Dev artifact name](agent-data/versioning.md#dev-artifact-name)). A rung has
   its own test and no more, the acceptance check being the closing's.
5. Work review: stop before writing any description and say "please review", as its own message
   with no title or body. The review is of the working-copy diff ([jj
   basics](agent-data/jj.md#jj-basics)). Iterate over steps 2 through 5 until the user approves
   with "continue" or "go".
6. Describe: write the description in [Commit-body form](agent-data/prose.md#commit-body-form),
   read from the file first with its specimen, [commit-model.md](agent-data/commit-model.md)
   ([Commit description](#commit-description)). A local description can still be changed before
   the Land, so it has no review of its own.
7. Commit: after approval, the work and agent repos are committed locally by `vc-x1 cycle --commit
   -s b --title "..." --body "..."`, a pair ([Committing vs pushing](#committing-vs-pushing)). The
   cycle's first commit is its opening and needs neither title nor body. The ladder is rewritten
   into the commit, the rung `(done)` and the next `(current)`, so no marker is typed or flipped.
   The closing is `vc-x1 cycle --close -s b` and its shape ([Cycle shape](#cycle-shape),
   [Close-out](#close-out)). A rung that
   changes the artifact then installs its dev build again, so the next rung runs the committed
   code, not the working copy's ([Dev artifact name](agent-data/versioning.md#dev-artifact-name)).

### Committing vs pushing

A cycle's commits are made *by* `vc-x1 cycle`, a rung by `--commit` and the closing by `--close`,
never by `jj commit`, and published only by a push
([why](agent-data/rationale.md#committing-vs-pushing)).

- `cycle --commit`, `--close`, and `--land` stay on this machine and need no approval. Each names
  its scope, `-s`, as every command that writes does: `b` the pair, `w` the work half, and `p` a
  plain repo.
- `--push` on any of them publishes, exactly like `vc-x1 push`, as does `squash-push`. With
  `--commit` or `--close` it is a draft push, of the cycle's bookmark, and with `--land` it is the
  publish, of the target.
- `vc-x1 squash` continues a rung, folding the working copy into it. `squash --title --body` is the
  sanctioned re-describe of `@-`. `vc-x1 commit` is the plain commit of a pair, for work outside a
  cycle, and `commit -r REV --title --body` describes a commit made by hand.
- A bare `jj commit` is asked for by name and is for local saves and [local ladder](#local-ladders)
  intermediates, which collapse before the rung commits. What the commands do is in [vc-x1 commit
  and push][vpush].

#### A cycle is local by default

A cycle's commits stay on this machine until its Land, and the user decides when each side is
committed and when either is published, at any rung, every push still taking its own approval
([why](agent-data/rationale.md#a-cycle-is-local-by-default)).

- The default: every commit is `vc-x1 cycle --commit -s b`, a pair, the agent commit holding the
  session to that moment, and the opening's bookmark created and not pushed.
- Any mix of work and agent commits is valid. `cycle --commit -s b` makes a 1:1 pair, `cycle
  --commit -s w` rungs then `commit -s a --covers <REVSET>` or the Land make N:1, and several agent
  commits naming one work commit make 1:M. The rule is the links: a work commit's `ochid:` names
  each agent change it pairs with, an agent commit's names each work commit it covers, each below
  an `orepo:`
  naming the other repo, and a squash folds into a commit an `ochid:` names, never that commit
  away, since jj keeps the change id of the commit squashed into ([Cross-repo
  linking](agent-data/jj.md#cross-repo-linking-ochid-trailers)).
- A backup, a move between machines, or someone else to see the cycle publishes mid-cycle by a
  draft push, `push -s b`, and `-s w` rungs first get their agent commit, `commit -s a --covers
  <REVSET>`.
- The Land commits the agent `@` first, one `ochid:` per work commit it has not yet covered, none
  when every rung is a pair, an older work commit's `ocid:` and `pcid:` giving way to `orepo:`,
  trees unchanged.

### Commit description

The title is a Conventional Commit, distinct within its cycle ([Commit titles and
descriptions][cdd]).
The body is in [Commit-body form](agent-data/prose.md#commit-body-form), two plain paragraphs: no
version, file list, or deliberation ([why](agent-data/rationale.md#commit-description)).

### Pushing

Pushing is by `vc-x1 cycle --push -s b`, which publishes what the cycle's state calls for, a
draft's bookmark or a Land made here, by a stage's `--push`, or by `vc-x1 push`, and the Land's
bookmark moves are `cycle --land`'s and `delete-bookmark`'s.

#### Before any push

([why](agent-data/rationale.md#before-any-push))

- This specific push has the user's explicit approval, or an explicit waiver covers it. `vc-x1 push
  --yes` is for that approval alone, since an agent can type it as easily as anything else.
- A push that would be forced, a published commit rewritten, is asked about by `push` and
  `squash-push` and refused by `cycle` and `commit`, and the `--yes` flag answers, so the agent
  passes it only for a forced push the user approved.
- Validation ran, and passed, after the last edit.
- Closing words are written. Nothing follows the turn's final push.

#### At rest

The contract that keeps the work-repo clean, the rule **Hard stop after the final push** its first
item's tail ([why](agent-data/rationale.md#at-rest)):

1. The agent publishes: a step that publishes is complete when its command is issued. The agent says
   what is worth saying *before* the final publishing command, responds with the one word
   "Published", and does nothing further until the user speaks.
2. Clean by squash-push: a Land is published by the agent only with the user's permission. A run of
   delegated cycles follows each `cycle --land --push` with the next cycle's opening, which carries
   the session's tail. A Land the agent made locally is published by the user after the session:
   `vc-x1 squash-push -s b` pushes the work target first, the shared repo and so the push likely to
   be refused, then folds the tail into the Land's agent commit and pushes it, so each `main` moves
   once, forward.

"Clean" means the work `@` is empty, and once the user's squash-push has run the agent's `@` too.
Before it the agent's `@` is the session's data and no finding, as [Acquaint](#acquaint) says. A
late work-repo tweak after a push is a remote rewrite and takes approval like any push ([vc-x1
commit and push][vpush]).

### Close-out

The cycle's last commit is bookkeeping and its body describes that bookkeeping. A single-step cycle
does all of it in its one commit, step 5 aside ([Cycle shape](#cycle-shape)):

1. Acceptance check: run the check the opening stated, as the first thing in closing, and record
   pass or fail. A rung may run it and the closing record the result, since some checks run only
   mid-ladder. A failure is a finding, and why it failed is determined.
2. Finalize the cycle-record in place ([Cycle-record](#cycle-record)):
   - sync the title if the scope shifted, and every anchor back-reference with it. A published rung
     title and the bookend pair keep their names, so a rename then reaches the block's text and the
     rungs not yet published only
   - replace the provisional solution statement with what was done
   - add the design subsections the deliberation grew
   - complete the closing rung's subsection
   - ask what in the block must outlive the cycle, and write it into the `notes/` file it
     belongs to
   - the block is moved whole to `## Closed`, leaving `## In Progress` reading
     `_No cycle currently in progress._`, and `zTOC.md`'s ladder is rewritten with every marker
     final, both by `vc-x1 cycle --close`.
3. Validate: full validation, and update `notes/README.md` if functionality changed.
4. Size: when an agent-file changed, record the line count in `notes/agent-files-size.md`,
   smaller being the quasi-goal. A cycle that touched none adds no row.
5. Close-out shape ([Close-out shapes](agent-data/jj.md#close-out-shapes)): a ladder closes as a
   trapezoid, `cycle --close -s b --multi-step --trapezoid <target>`, the closing commit carrying
   the shape, so
   nothing is reshaped at Land.
6. Continuation notes: anything the next agent needs is written into `TODO.md > ## Continuation
   notes`, the first section, before the closing commit, which carries them, so the work-repo is
   clean from the Land on. A fact known only after the Land is not written: the note names what the
   next session checks, `main` at the closing, the bookmarks gone, the installed version's commit.
   The next acquaint reads it first, files or keeps each item, and resets the section ([Todo
   format](agent-data/notes.md#todo-format)).
7. Test the closed cycle: the acceptance check again, as the last thing, on the closed tree. It is
   the first run on that tree when a merge brought the target's changes in, and otherwise a
   confirmation the user may skip. A fault found here stops the flow ([Stop and
   ask](#stop-and-ask)), and the way back is the user's choice: reopen the cycle, fix inside the
   closing, fix on top, or their own, the test itself being what may be wrong.
8. Land: on the user's go, the sequence in [Land](agent-data/jj.md#cycle-bookmarks-create-and-land).
   A Land is a fast-forward, and a target that moved is refused naming the ways on: a merge at the
   Land, `cycle --land <target> -s b --merge`, which the user asks for and which lands a tree no
   rung validated, the closing made again, or a rebase.
9. Restart: the user restarts the agent.

### Local ladders

A rung that wants incremental review runs as a local ladder: a chain of jj commits that never leaves
the machine and collapses into the rung before the cycle continues, each validated with
`vc-x1 validate --fast` ([Local ladders](agent-data/jj.md#local-ladders),
[why](agent-data/rationale.md#local-ladders)).

[cbt]: agent-data/prose.md#conventional-commit-shape-ladder--commit
[cdd]: agent-data/prose.md#commit-titles-and-descriptions
[llb]: agent-data/jj.md#long-lived-bookmarks-merge-only-by-default-deletable-once-merged
[snn]: agent-data/prose.md#steps-are-named-not-numbered
[vpush]: agent-data/jj.md#vc-x1-commit-and-push-what-they-do-and-do-not-do

## Working practices

([why](agent-data/rationale.md#working-practices))

- One command per invocation: no bundled steps (`a && b; c`), except a genuine pipeline or a tight
  pair where the join is the point.
- Exit status: never mask a command's exit status. The command under test runs bare and last in
  its invocation, since the Bash tool reports a nonzero exit, with `set -o pipefail` first when it
  is piped. No `&&` after a piped stage, no trailing `; echo "exit=$?"`. To report and still fail:
  `cmd || { rc=$?; echo failed=$rc; exit $rc; }`.
- Long output: the Bash tool returns only part of a long output, and the part it drops may hold
  the result. A validation, build, or test run, an unbounded log or diff, each command `custom.md`
  names, and any command whose length is in doubt runs as
  `set -o pipefail; <command> 2>&1 | tee tmp/last.log | tail -25`. A pass is the exit status, so
  nothing runs again to see one, and a failure is read from the file, the slice it needs.
- Hooks: the Claude Code hooks are in `agent-data/` and `bash agent-data/hooks.sh install`, the
  user's to run, puts them in `~/.claude/hooks/` and registers them in `~/.claude/settings.json`.
  `bash-guard.sh` denies the two masks it can see, an echo of `$?` or `PIPESTATUS` and a `cargo`
  or `vc-x1` command piped into `tail`, `head`, `grep`, or `rg` without `pipefail`. `bash-allow.sh`
  skips the prompt for `cargo`, `jj`, and `vc-x1`, and `inhibit-sleep.sh` keeps the machine awake
  while a session works.
- Scratch files: where a thing goes is decided by whether it can be reproduced.
  - `tmp/`: disposable, cleared at any time, gitignored, `mkdir -p tmp` on demand. `/tmp` is for
    out-of-project temporaries.
  - `tmp-durable/`, gitignored beside it: what we would rather not lose and can reproduce, a built
    index for one.
  - `notes/`: what must last, committed and pushed. It holds no pointer into `tmp-durable/`.
  - Code set aside: a jj commit with a local bookmark, not pushed, so either of us can address it,
    never a patch file.
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

### A session's rules are its own agent-files

A session's rules are the agent-files of the project it started in, and its `custom.md`, and rules
living in any other repo are ignored unless these files or the user direct otherwise
([why](agent-data/rationale.md#a-sessions-rules-are-its-own-agent-files)). So another repo's own
protocol (a shared store's README, another project's agent-files) governs a write to it only as
far as the delegation reaches, and this file's conduct rules, the step reads, the reviews, the
per-push approval, and the stops, bind every commit and push the session makes, in any repo,
cycle or not. A repo with a live agent of its own is not written at all: message its agent.

### Stop and ask

Stop and ask on ambiguous input, on any deviation from the agreed plan, and when 5+ minutes on a
simple task has produced no progress ([why](agent-data/rationale.md#stop-and-ask)).

## Changing the agent-files

A new repo starts from an adopter's set, its working copy's by default, applied by
`scripts/vc-dr-apply.sh`, and every adopter carries its own copy
([why](agent-data/rationale.md#changing-the-agent-files)).

- Local experiments: a local agent-file may hold an unagreed experiment, diffed against the copy it
  started from while the rules are being tried. After the experiment there may be a proposal.
- Proposing: a proposal is one set of diffs, to the rules or beyond them, that an adopter may or
  may not accept as is.
- Convergence: the adopter reviews the diff between the proposal and its files and accepts it as is
  or proposes changes. It is a negotiation whose goal is everyone accepting the same thing, and
  that goal is not a requirement.
- Acceptance: the adopter and the proposer agree on the contents, identical or not.
- Retirement: a resolved experiment retires like a finished Todo, adopted and rejected alike: the
  cycle that resolved it is its record.

## custom.md

When a `custom.md` exists at the workspace root, the agent must read it first, and it overrides
anything in the agent-files ([why](agent-data/rationale.md#custommd)). It is the place for what is
local to the workspace, one machine's paths and permissions, since a change every clone needs is a
cycle on the agent-file it belongs in. The session lock,
[session-lock.md](agent-data/session-lock.md), is an experiment it may carry.
