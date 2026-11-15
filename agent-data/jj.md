# jj and cross-repo linking

How version control is driven on this project (jj, never raw git) and how the two repos' commits
point at each other (the `ochid:` trailer). Read this before any jj operation beyond `st` / `log` /
`diff`, and always before touching a commit description.

## jj Basics

**Use jj, not git, for version-control operations** (status, log, diff, commit, push, history
rewrite, [why](rationale.md#jj-basics)). jj coexists with the git backend, so the repo *can* be
driven with raw `git`, but this project's workflow (bookmarks, the working-copy `@` model, ochid
trailers) is expressed in jj terms.
Reaching for `git` invites state that doesn't match the jj documentation here. There is no `jj mv`:
to move/rename a tracked file, just `mv` it on disk and jj detects the rename by content.

Below, `<repo>` is either side's path and `<agent-dir>` the agent-repo's, which the work-repo's
`.vc-config.md` names under `[repos] agent` ([.vc-config.md](#vc-configmd)) rather than any
fixed directory.

- `jj st -R .` / `jj st -R <agent-dir>`: show working copy status
- `jj log -R .` / `jj log -R <agent-dir>`: show commit log
- `jj commit -m "title" -m "body" -R <repo>`: finalize working copy into a commit
- `jj describe -m "title" -m "body" -R <repo>`: set description without committing
- `jj git push --bookmark <name> -R <repo>`: push a bookmark (no `--allow-new` flag: jj pushes new
  bookmarks without special flags)
- `jj bookmark set <name> -r <rev> -R <repo>`: create the bookmark, or move an existing one, at
  `<rev>`. Moving it backwards needs `--allow-backwards`
- `jj git push --named <name>=<rev> -R <repo>`: create *and* publish in one step, for a line that is
  not a cycle's, whose bookmark `vc-x1 cycle --open` makes ([Cycle
  bookmarks](#cycle-bookmarks-create-and-land))
- In jj, the working copy (@) is always a mutable commit being edited. `jj commit` finalizes it and
  creates a new empty working copy on top.
- The agent-repo always has uncommitted changes during an active session because session data
  updates continuously.
- `jj rebase` uses `--onto`/`-o` to name the destination(s).

Viewing, for a review: `jj diff -R .` is the working copy, `jj diff -r @- -R .` the previous commit,
`jj show -r <X> -R .` one revision's description and diff. Never `jj edit -r @-` to view a past
commit: it marks it mutable and shifts `@`.

## Revsets

How commits are addressed in `-r` arguments, condensed. jj's own semantics are the one dialect
([why](rationale.md#revsets)). The full language is `jj help -k revsets`, the single authority, and
the worked tutorial with terminal transcripts is `jj-tips.md`, hosted once in the template
repository.

- A revision is `@` (the working copy), a chid prefix, or a commit id. Unambiguous prefixes are
  accepted, and ambiguous ones are rejected, never guessed.
- Neighbors: `@-` parent, `@--` grandparent, `@+` child. A step past the end of the chain is the
  empty set, not an error.
- `::` is the primary range operator, symmetric and endpoint-inclusive:
  - `::x` ancestors of x, including x. `x::` descendants of x, including x
  - `x::y` the DAG path: descendants of x that are also ancestors of y
- `..` is git-compatible set subtraction, not a tighter `::`:
  - `x..y` is `::y ~ ::x`: ancestors of y that are not ancestors of x
  - `x..` is `~::x`: every visible commit that is not an ancestor of x. Open-ended, so on a repo
    with parked branches it includes commits unrelated to x
  - `..x` is `::x ~ root()`: ancestors of x, excluding the root
- Useful sets: `jj log` (default revset), `jj log -r ::@` (all ancestors of `@`),
  `jj log -r 'all()'` (all visible commits), `jj evolog -r X` (one change's rewrite history),
  `jj op log` (operation history).

## Cross-repo linking (ochid trailers)

The cross-reference between the work-repo and the agent-repo is what makes the dual-repo work: every
commit points at its counterpart in the other repo, so the "what" (code) and the "why / how"
(session) stay linked across time ([why](rationale.md#cross-repo-linking-ochid-trailers)). The
work-repo is the root of the record: lose the agent-repo and only context goes.

A **chid** is jj's change ID, a permanent identifier that survives rebases and `describe`s (unlike
the commit ID / git SHA, which changes on rewrite). Trailers are blank-line-separated `key: value`
lines at the end of the commit body:

```
orepo: https://github.com/owner/name.agent-session
ochid: xvzvruqowktp
```

- `orepo: <other-repo-url>`: the other repo, on both sides, a work commit naming the agent-repo and
  an agent commit the work-repo. It covers the `ochid:` lines after it, so the pair says exactly
  where each partner is.
- `ochid: <chid>`: the partner's change id in the other repo, a bare 12-character id on both sides.
  Published `/<chid>`, `/.claude/<chid>`, and `<repo-url>#<chid>` stay valid.

An agent commit carries one `ochid:` per work commit it covers, so a local cycle's Land makes one
agent commit naming them all. The URL is from `origin`, normalized to https, or `file:///<path>` for
a local one, and `--no-url-trailer` leaves it out. No trailer holds a SHA, so either side may be
committed first, and a rebase, a squash, or a squash-push leaves every link valid, so long as a
squash folds into the commit an `ochid:` names and never that commit away. An older commit's
`ocid:` and `pcid:`, which named commits by SHA, are not checked, and a restamp replaces them with
`orepo:`.

A trailer `vc-x1` does not stamp is kept on a rewrite, so `custom.md` may add its own. `vc-x1
trailers --update` moves a range of commits to today's form, and a Land runs the same pass over its
cycle.

Use `vc-x1 chid -s work,agent -L` to capture the change IDs (first line work-repo, second agent
repo).

The trailers are **stamped by the tool**, `vc-x1 cycle` and `vc-x1 commit`. Never hand-write them
into a commit body or `--title`/`--body`.

## vc-x1 commit and push: what they do and do not do

`vc-x1 cycle --commit -s b --title "..." --body "..."` commits both repos with the title and body,
the pair, stamps their trailers ([above](#cross-repo-linking-ochid-trailers)), rewrites the ladder
into the commit, and stays on this machine. `-s` is required, and `-s w` commits the work repo
alone, its trailer naming the agent `@`. `--push` publishes what was committed, as `vc-x1 push`
does, a draft push, and `vc-x1 show -s work -r @-` is the look at it before then. `vc-x1 commit -s
b --title "..." --body "..."` is the same pair for work outside a cycle, where `-r` names a commit
below `@`, described in place with its trailer stamped, and `--bookmark` the bookmark to advance.
Every argument is a flag. What to keep in mind:

- **No checks of the project's own.** vc-x1 runs no build or tests. Validation is the per-rung
  flow's job, run *before* the commit.
- **Rerunning a push is safe.** Push keeps no state and cannot resume: every stage no-ops when its
  work is already done, so a failed run is re-run, not resumed.
- **The trailers are stamped**, never hand-written into `--title` or `--body`.
- **The agent-repo is a linear journal.** Never create an agent-repo bookmark mirroring a work-repo
  one: it steers session pushes at the wrong remote ref.

A late work-repo tweak after a push (a forgotten edit) is folded by `vc-x1 squash` and re-pushed,
which is a remote rewrite and takes approval like any push. Pushing is not what makes a commit
history: `squash` warns of a published commit jj counts mutable, a cycle's bookmark pushed for a
backup among them, and refuses one in jj's `immutable()`, `main@origin`, tags, and what the user's
`immutable_heads()` adds, without `--ignore-immutable`, as jj refuses it.

## Re-describing: coordinate first, and keep the trailer

**Never `jj describe` a commit that already carries trailers, and never re-describe a published one
without coordinating with everyone involved first**
([why](rationale.md#re-describing-coordinate-first-and-keep-the-trailer)). It is a history rewrite,
and by hand it silently drops the cross-repo link. `vc-x1 squash --title --body` is the sanctioned
re-describe of `@-`, keeping the trailers. Describing a fresh local commit that has never been
described and carries no trailers is authoring a message rather than rewriting one, and is not
covered. That is a [local ladder](../AGENTS.md#local-ladders)'s scratch describe. `vc-x1 fix-desc`
repairs a dropped trailer by title match.

## Cycle bookmarks: create and land

The mechanics behind [Cycles run on a bookmark](../AGENTS.md#cycles-run-on-a-bookmark). That section
holds the rule and when it applies, and this one holds the commands
([why](rationale.md#cycle-bookmarks-create-and-land)).

**Create**, first at the cycle's opening: `vc-x1 cycle --open --title "<bare cycle title>"` makes
the bookmark on the work `@`, named by the title's slug (the anchor algorithm in [Markdown anchor
links](notes.md#markdown-anchor-links)), or `--bookmark <NAME>`, and kept local, then writes the
cycle's record. It refuses a name in use, locally or on `origin`, and a cycle already in progress,
`@` holding edits or not. The bookmark follows the change, so `vc-x1 cycle --commit` commits the
opening on it, and every later rung advances it and no other.

**Close**: `vc-x1 cycle --close -s b` with `--single-step` or `--multi-step`, from the work `@` on
the cycle's tip, makes the closing: a
ladder's closing commit, a merge of the target and the tip with `--trapezoid <target>`, or for a
single-step cycle the one commit, made from `@` or squashed into the opening, its `--body` given.
The target is read where it is now, so one that moved is joined. The record moves to `## Closed`
and the ladder is rewritten in the same commit.

**Land**, once the close-out is approved: the sequence that makes the cycle permanent. Its `vc-x1
push` is the push under [Approval per push](../AGENTS.md#before-any-push), with the closing words
before it and silence after ([At rest](../AGENTS.md#at-rest)):

1. Sync: `vc-x1 sync -s b`, and a `main` it moved stops the Land ([Sync](../AGENTS.md#sync)).
2. Integrate: `vc-x1 cycle --land <target> -s b`, local: it commits the agent `@` as the partner of
   the work commits that name it, none when every rung is a pair, fast-forwards the target, and
   deletes the cycle's local bookmark, every check made first. `--dry-run` says each step. The
   user may test here, before anything is published.
3. Publish: `vc-x1 push -s b --bookmark <target>`, the work repo first, the journal folded into the
   last agent commit while that is unpublished. `cycle --land <target> -s b --push` does steps 2 and
   3 as one, and `cycle --close --multi-step --land <target> -s b --push` the closing too.
   The agent publishes only with the user's permission, and otherwise the user does after the
   session, by `vc-x1 squash-push -s b` ([At rest](../AGENTS.md#at-rest)).
4. Remote bookmark: `vc-x1 delete-bookmark -s w --bookmark <bookmark> --skip local`, when the topic
   was ever pushed.
5. Install: `vc-x1 build`, a `vc-x1 run -- -V` smoke check, and `vc-x1 install`, so the plain name
   is replaced only by what is on the target.

- `cycle --land` onto another cycle's bookmark, a stack of cycles landing together, is the command
  and not the sequence: nothing is installed, and the cycle is permanent only when the stack lands
  on its target.
- Landing is the moment the cycle's commits become permanent. No record cites their SHAs
  ([Cycle-record](../AGENTS.md#cycle-record)): the landed commits are the record.
- A Land fast-forwards by default, so what lands is the tree the closed cycle was tested as. A
  target that moved is refused naming the ways on: `--merge`, a merge at the Land, which the user
  asks for and which lands a tree no rung validated, the closing made again as a trapezoid, its
  merge joining the target where it is, or a rebase by `jj rebase`, every rung then validated again.

**Reshape**, while the bookmark is a draft ([Cycles run on a
bookmark](../AGENTS.md#cycles-run-on-a-bookmark)):

- **Amend content, never re-describe.** Editing `TODO.md` in a rung and amending is not a
  `jj describe`, so [No re-describe without
  coordinating](#re-describing-coordinate-first-and-keep-the-trailer) stays intact.
- **A topic that was pushed is force-pushed**, under the same approval as any other push.

## Long-lived bookmarks: merge-only by default, deletable once merged

A long-lived bookmark (a program line pushed rung by rung across cycles) is not a cycle bookmark: it
carries published history, and the discipline protects that history while the bookmark is its only
holder.

- conflicts with `main` are resolved in merge commits, never by rewriting published rungs
- a rebase is never required for correctness: it is a linearity preference
- coordinated rebases stay available (the bar is on unilateral rewrites, not rewrites), at the known
  cost of staling the git-SHA citations in the records. Chids and ochid trailers survive a rebase
- once the bookmark's history is fully merged into `main` the bookmark is redundant and is deleted,
  locally and remotely

The contrast with a cycle bookmark is the whole point: that one is a draft and may be rewritten
freely until it lands (see [Cycles run on a bookmark](../AGENTS.md#cycles-run-on-a-bookmark)), this
one is published and may not. Refined 2026-08-03 from the earlier "treated as permanent, never
rebased" wording, after a fully merged long-lived bookmark was deleted without loss.

## Close-out shapes

The shape a ladder lands in is carried by its closing commit, so nothing is reshaped at
[Land](#cycle-bookmarks-create-and-land) ([Close-out](../AGENTS.md#close-out)). A single-step cycle
is one commit and lands as it is:

- **trapezoid**: a merge commit whose first parent is the target and whose second is the ladder, so
  `git log --first-parent` reads one commit per cycle while every rung stays reachable. It joins a
  target that moved. With one rung the figure is a triangle.
- **linear**, one commit per rung on the target, the closing a plain commit on the ladder, is
  `cycle --close --multi-step` without `--trapezoid`, the user's to ask for. A Land of it refuses a
  target that moved, since joining one would rewrite the reviewed ladder or need a merge.

Squash is not a shape: each rung's `ochid:` trailer is a change id, a squash keeps one and drops the
rest, and every agent-repo commit that pointed at a dropped rung is left dangling ([Cycle
shape](../AGENTS.md#cycle-shape)).

**Preview before closing.** The cycle's net change is the tree diff from `<base>` to the tip,
whatever shape the commits between are in: `jj diff --from <base> --to <tip>`, or the gitk range
described in
[Read a change in gitk at full context](#read-a-change-in-gitk-at-full-context).

## Read a change in gitk at full context

gitk renders a change three ways, and at full context each answers a different question. Raise
"Lines of context" to the longest file's length, or to a large number such as 1000 (`--context 1000`
is the CLI equivalent), to see the entire file, then select:

- **New version**: the file as it now is, with the added lines lit. This is what the permanent
  branch will carry, so it is the view for judging whether a change reads as one thing.
- **Old version**: the file as it was, with the removed lines lit. It answers what the change costs,
  which a diff states only as minus lines out of context.
- **Diff**: the two interleaved. At full context it is the whole file with the edit marked in place,
  which is the reading a reviewer wants when the edit is small and the file is not.

The above reads one commit at a time and is useful. Another mode is to see how several commits
together changed the tree since a given `<base>` commit. To do that:

- Hover over the commit you want as the `<base>`, right click and select "Mark this commit".
- Hover over any commit above the `<base>`, right click and select "Diff marked commit -> this".

Now New version, Old version and Diff show the net change of everything between `<base>` and `this`.
If `<base>` is the parent of the `opening` commit and `this` is the `closing`, that is the cycle's
net change ([Close-out shapes](#close-out-shapes)).

## Local ladders

The jj moves behind [Local ladders](../AGENTS.md#local-ladders), a rung's scratch chain of commits
that never leaves the machine. Ladder commits are scratch, for review and bisection only. Per ladder
commit:

1. `jj new -R .`: a fresh empty `@`.
2. Do the commit's work.
3. `vc-x1 validate --fast` (the `[validate] fast` table). Non-negotiable.
4. `jj describe -m "..." -R .`: a scratch working title. This first-time authoring is the one
   permitted describe.

At the end, squash the chain into the rung (below) and continue the per-rung flow from its
validation step. `vc-x1 cycle --commit` then makes the single commit and stamps its trailers, and
refuses while an intermediate is still on the branch, its title being no rung's. A
sub-cycle that deserves its own record nests the version suffix
([versioning.md](versioning.md#suffix-scheme)) and names its rungs like any other.

Navigating:

- `jj log -r '<base>::' -R .`: the whole ladder from its base.
- `jj edit -r <prefix> -R .`: jump `@` to any ladder commit by chid prefix, for bisection.
- `jj edit @-- -R .`: quick-jump back two commits.
- `jj diff -r <chid> -R .`: review one commit in isolation.

Modifying any ladder commit rewrites it in place, and descendants auto-rebase.

The squash, `jj squash --from "<base>..@-" --into @ -u -R .`, takes `<base>` as the parent of the
first ladder commit, and `-u` keeps `@`'s description and discards the sources'. After it, history
is linear (`<base> -> @`) and the intermediate commits are auto-abandoned. For N = 1 the range is
empty and the squash is a no-op.

Recovery:

- **Discard the current commit.** `jj abandon @ -R .` drops it, and you get a fresh empty `@` on the
  same parent.
- **Edit an earlier commit.** `jj edit -r <chid> -R .`, make corrections, then
  `jj edit -r <last-ladder-chid>` to return. Descendants auto-rebase.
- **Discard the entire ladder.** `jj op log -R .` shows the op history, and
  `jj op restore <op-id> -R .` reverts to that point. Full undo, so only to start over.

## Resolvability

A change ID travels with its commit: a **pushed** commit resolves to the same chid in every clone.
Cloning the agent-repo gave the published `main` tip the same chid as an existing clone. We think jj
carries the change ID in the git commit object, so it survives `jj git clone` / fetch.

The local-only case is the **working-copy `@`**: jj mints a fresh random chid for `@` in each clone,
so an unpushed `@` is a stable ochid target only on the machine that holds it. A local cycle's rungs
name the agent `@` for that reason: nothing is published until the Land commits it, and its change
id holds.

## .vc-config.md

Each repo contains a `.vc-config.md` whose toml blocks hold a `[repos]` registry that records the
workspace layout ([The dual-repo model](../AGENTS.md#the-dual-repo-model)). Values are ordinary
paths relative to the config file's directory, or absolute, so the two sides' blocks **differ**:
the entry that resolves to the config's own directory names its side, and the two sides must agree
on the same resolved work/agent pair:

```toml
# work side              # agent side
[repos]                  [repos]
work = "."               work = ".."
agent = "<agent-dir>"    agent = "."
```

`<agent-dir>` is the only cell the project chooses, the other three being fixed by the layout.
It stays a placeholder here because naming a directory would trade one project's bias for
another's, and where the agent-repo sits is exactly what this registry exists to say.

The `ochid:` trailer's bare id does not read this registry ([Cross-repo
linking](#cross-repo-linking-ochid-trailers)).
