# jj and cross-repo linking

How version control is driven on this project (jj, never raw git) and how the two repos' commits
point at each other (the `ochid:` trailer). Read this before any jj operation beyond `st` / `log` /
`diff`, and always before touching a commit description.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## jj Basics

**Use jj, not git, for version-control operations** (status, log, diff, commit, push, history
rewrite, [why](rationale.md#jj-basics)). jj coexists with the git backend, so the repo *can* be
driven with raw `git`, but this project's workflow (bookmarks, the working-copy `@` model, ochid
trailers) is expressed in jj terms.
Reaching for `git` invites state that doesn't match the jj documentation here. There is no `jj mv`:
to move/rename a tracked file, just `mv` it on disk and jj detects the rename by content.

- `jj st -R .` / `jj st -R .claude`: show working copy status
- `jj log -R .` / `jj log -R .claude`: show commit log
- `jj commit -m "title" -m "body" -R <repo>`: finalize working copy into a commit
- `jj describe -m "title" -m "body" -R <repo>`: set description without committing
- `jj git push --bookmark <name> -R <repo>`: push a bookmark (no `--allow-new` flag: jj pushes new
  bookmarks without special flags)
- `jj bookmark set <name> -r <rev> -R <repo>`: create the bookmark, or move an existing one, at
  `<rev>`. Moving it backwards needs `--allow-backwards`
- `jj git push --named <name>=<rev> -R <repo>`: create *and* publish in one step, which is the usual
  way a cycle's bookmark is born (see [Cycle bookmarks](#cycle-bookmarks-create-and-land))
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
(session) stay linked across time. That pointer is the **ochid** (Other Change ID) git trailer
([why](rationale.md#cross-repo-linking-ochid-trailers)).

A **chid** is jj's change ID, a permanent identifier that survives rebases and `describe`s (unlike
the commit ID / git SHA, which changes on rewrite). An **ochid** trailer carries the counterpart
commit's chid as a workspace-root-relative path:

- Paths start with `/`, the workspace root, i.e. the work-repo (the project root). `/.claude` is the
  agent sub-repo.
- `ochid: /<chid>` references a change in the **work-repo**.
- `ochid: /.claude/<chid>` references a change in the **agent-repo**.

Trailers are blank-line-separated `key: value` lines at the end of the commit body, using the chid's
**12-character** prefix:

```
ochid: /.claude/xvzvruqowktp   # points to an agent-repo change
ochid: /wtpmottvxqzl           # points to a work-repo change
```

How many, and which direction:

- **Work-repo commits** each carry one `ochid: /.claude/<agent-chid>`, the agent-repo's change ID.
- **The agent-repo commit** carries one `ochid: /<work-chid>` per work-repo commit in that push. The
  count is per *push*, not per cycle. A trapezoid close-out whose rungs were pushed 1:1 as they
  landed still carries exactly one. More than one occurs when a single push publishes several
  work-repo commits.

Use `vc-x1 chid -s work,agent -L` to capture the change IDs (first line work-repo, second agent
repo).

`ochid:` trailers are **stamped by `vc-x1 push`**. Never hand-write them into a commit body or
`--title`/`--body`.

## vc-x1 push: what it does and does not do

`vc-x1 push <bookmark> --yes --title "..." --body "..."` commits both repos with the approved title
and body (`--yes` because push's gates need a tty the agent sandbox lacks, and the user's approval
of the shown title and body already covers them), stamps each new commit's `ochid:` trailer
([above](#cross-repo-linking-ochid-trailers)), pushes the work bookmark, and squash-pushes the agent
repo's `main`. Three behaviors to keep in mind:

- **No checks of the project's own.** vc-x1 runs no build or tests. Validation is the per-rung
  flow's job, run *before* the push. The one check that remains is `push-work` verifying the
  bookmark's remote refs are tracked.
- **Rerunning is safe.** Push keeps no state and cannot resume: every stage no-ops when its work is
  already done, so a failed run is re-run, not resumed. If push exits after `push-work` but before
  the agent-repo publish, `vc-x1 squash-push -R .claude` by hand is the rest of it.
- **`ochid:` trailers are stamped by push** ([No hand-written
  trailers](#cross-repo-linking-ochid-trailers)), never hand-written into `--title` or `--body`.
- **The agent-repo is a linear journal.**
  - One push is one agent-repo commit on its `main`, paired with the work-repo commit, whatever
    bookmark the work-repo is on.
  - Never create an agent-repo bookmark mirroring a work-repo one: it steers session pushes at
    the wrong remote ref.
- **Squash-push again if `@` is non-empty** after a pass.
  - The agent keeps writing session data while the command runs, so its own record lands after
    the squash.
  - Session data is append-only, so a re-run never conflicts.
  - A single pass is never guaranteed to leave `@` empty.

A late work-repo tweak after the push (a forgotten edit) needs `jj squash --ignore-immutable` and a
re-push, which is a remote rewrite and takes approval like any push.

## Re-describing: coordinate first, and keep the trailer

**Never `jj describe` a commit that is already published or already carries trailers without
coordinating with everyone involved first**
([why](rationale.md#re-describing-coordinate-first-and-keep-the-trailer)). It is a history rewrite,
and it silently drops the cross-repo link. Describing a fresh local commit that has never been
described and carries no trailers is authoring a message rather than rewriting one, and is not
covered. That is a [local
ladder](../AGENTS.md#local-ladders)'s scratch describe.

When a re-describe is agreed, copy any `ochid:` trailers into the new body by hand (the "don't
hand-write trailers" rule covers push authoring a message from scratch, not preserving one already
stamped). Hit at a coordinated amend (2026-07-29), where the trailer survived only that way.
`vc-x1 fix-desc` repairs a dropped one by title match.

## Cycle bookmarks: create and land

The mechanics behind [Cycles run on a bookmark](../AGENTS.md#cycles-run-on-a-bookmark). That section
holds the rule and when it applies, and this one holds the commands
([why](rationale.md#cycle-bookmarks-create-and-land)).

**Create**, at the cycle's opening, with the bookmark named by the cycle title's slug (the anchor
algorithm in [Markdown anchor links](notes.md#markdown-anchor-links), so the block's title heading
and the bookmark derive from one bare title):

- `jj git push --named <bookmark>=@- -R .` is the common case: it creates the bookmark at the last
  committed change and publishes it in one invocation.
- Any other revision works as the `=<rev>` right-hand side. When `<rev>` is not `@-`, follow with
  `jj new <rev> -R .` so the working copy actually sits on the new line. Otherwise the bookmark
  exists and the next commit lands somewhere else.
- `jj bookmark set <bookmark> -r <rev> -R .` creates it without publishing, for a line that is not
  ready to be seen.

**Land**, once the close-out is approved: the sequence that makes the cycle permanent. Every
`jj git push` in it is a push under [Approval per push](../AGENTS.md#before-any-push) and [Hard
stop after the final push](../AGENTS.md#at-rest-push-stop-squash-push), its own approval, the
closing words before the final invocation, silence after ([At
rest](../AGENTS.md#at-rest-push-stop-squash-push)):

1. Restore the plain name, when the project renamed ([Dev artifact
   name](versioning.md#dev-artifact-name)): rename `<name>-dev` back to `<name>` in the manifest,
   `vc-x1 validate --fast` so the lockfile follows, and `jj squash` folds the edit into the closing
   (`@` sits directly above it).
2. Reshape per the recorded choice: trapezoid runs the [recipe below](#trapezoid-close-out-recipe),
   keep separate needs nothing. A single-step cycle has no choice recorded and its one commit lands
   as it is.
3. Fast-forward: `jj bookmark set main -r <closeout> -R .`, then `jj git push --bookmark main -R .`.
   This push publishes the reshaped commit, so the topic bookmark itself is never re-pushed.
4. Install: promote the artifact from `main`, the cycle's last act, run when nothing can enter the
   cycle anymore.
5. Delete the bookmark, locally and remotely: `jj bookmark delete <bookmark>`, then
   `jj git push --bookmark <bookmark>` ([Bookmark per
   cycle](../AGENTS.md#cycles-run-on-a-bookmark)). The long-lived case below gets the same
   disposal once fully merged.

- The fast-forward needs no `--allow-backwards`. Needing it means the bookmark is not a descendant
  of `main`, and the situation wants a look, not a flag.
- Landing is the moment the cycle's commits become permanent. No record cites their SHAs
  ([Cycle-record](../AGENTS.md#cycle-record)): the landed commits are the record.

**Reshape**, while the bookmark is a draft ([Cycles run on a
bookmark](../AGENTS.md#cycles-run-on-a-bookmark)):

- **Amend content, never re-describe.** Editing `TODO.md` in a rung and amending is not a
  `jj describe`, so [No re-describe without
  coordinating](#re-describing-coordinate-first-and-keep-the-trailer) stays intact.
- **Then force-push the bookmark**, under the same approval as any other push.
- **Exceptions**, named and moved past: the bookmark has already landed, another branch is stacked
  on it, or the ladder is long and only a trailing snapshot disagrees.

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

The two shapes a ladder can land in, chosen by the user and recorded at close-out, executed at
[Land](#cycle-bookmarks-create-and-land) ([Close-out](../AGENTS.md#close-out)). A single-step cycle
is one commit and lands as it is:

- **trapezoid**, the current default: a merge commit whose first parent is the trunk and whose
  second is the ladder, so `git log --first-parent` reads one commit per cycle while every rung
  stays reachable. Reshaped at Land by the [recipe below](#trapezoid-close-out-recipe) and published
  by the landing fast-forward itself.
- **keep separate**, one commit per rung on `main`, when the decomposition itself is informative.

Squash is not a shape: each rung's `ochid:` trailer is a change id, a squash keeps one and drops the
rest, and every agent-repo commit that pointed at a dropped rung is left dangling ([Cycle
shape](../AGENTS.md#cycle-shape)).

**Preview before choosing.** The cycle's net change is the tree diff from `<base>` to the tip,
whatever shape the commits between are in: `jj diff --from <base> --to <tip>`, or the gitk range
described in [Read a change in gitk at full context](#read-a-change-in-gitk-at-full-context). The
trapezoid keeps each rung reachable while `git log --first-parent` shows this net diff once landed,
and keep separate shows the rungs themselves. The choice is made on what `main` will carry.

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
net change, and it can help you decide what shape to choose ([Close-out shapes](#close-out-shapes)).

## Trapezoid close-out recipe

The reshape behind the trapezoid shape, run at [Land](#cycle-bookmarks-create-and-land) step 2: the
close-out commit, pushed linearly by the closing rung, becomes a merge whose first parent is the
trunk line and whose second parent is the cycle's ladder. Nothing here publishes, the merge goes out
with Land's fast-forward.

```
  main line   ...--<base>------------------<closeout>--
                      \                    /
  ladder             <rung-1>--...--<tip>-+
```

- `<base>`: the **parent of the ladder's first rung**, the trunk position when the cycle opened. It
  becomes the first parent. Not always the previous close-out: a cycle landed linearly since
  (keep-separate shape, or a single-step cycle) sits on the trunk line and must stay there.
- `<tip>`: the cycle's last commit before the close-out. It becomes the second parent.
- `<closeout>`: the close-out commit, reshaped here.

1. `jj rebase -r <closeout> --onto <base> --onto <tip>`: `<closeout>` becomes the merge. Parent
   order is the argument order.
2. `jj new <closeout>`: an empty `@` above the merge. The bookmark followed the rewrite on its own.
   What step 1 leaves misplaced is the working copy: `jj rebase -r` re-parents descendants onto the
   rebased commit's old parent, so the empty `@` lands beside the merge on `<tip>` and the tree
   reverts to pre-close-out content, which looks alarming and is not.
3. Verify two parents: `jj log -r <closeout> -T 'parents.map(|p| p.change_id().short(8))'` must list
   both. jj preserves the second parent even though `<base>` is an ancestor of `<tip>` (observed at
   three consecutive close-outs), but a collapsed merge is indistinguishable from a correct one in
   `jj log --no-graph`.

Details:

- **Trailers survive.** The restore's squash and the reshape change `<closeout>`'s SHA but not its
  change id, so the `ochid:` trailers stamped at the closing push stay valid in both directions.
- **Immutability.** No flag is needed: a topic bookmark's commits are not on `trunk()` until Land's
  fast-forward.

Recovery:

- **Nothing is published until Land's fast-forward**, so the whole reshape is undoable with
  `jj undo` / `jj op restore`.
- **A collapsed or mis-parented merge**: undo and redo step 1 with the corrected revisions. Do not
  land a shape you did not intend. After the fast-forward the remote boundary is crossed and
  recovery is forward-only.
- **Working copy left beside the merge** (step 2 skipped): `jj new <closeout>` after the fact. Any
  commit made in the meantime branches off `<tip>` and needs a rebase onto the merge.

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
validation step. `vc-x1 push` then publishes the single commit and stamps its one `ochid:`. A
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
so an unpushed `@` is never a stable ochid target. This is why an agent-repo ochid names `@-` (the
last committed change), not `@`.

## .vc-config.md

Each repo contains a `.vc-config.md` whose toml blocks hold a `[repos]` registry that records the
workspace layout. Values are ordinary paths relative to the config file's directory (absolute
allowed, discouraged), so the two sides' blocks **differ**: the entry that resolves to the config's
own directory names its side, and the two sides must agree on the same resolved work/agent pair:

```toml
# work side          # agent side
[repos]              [repos]
work = "."           work = ".."
agent = ".claude"    agent = "."
```

Ochid trailer prefixes are fixed per-side labels (`/` work, `/.claude` agent) resolved by side
detection, not filesystem paths.
