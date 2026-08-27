# Cycle protocol

This protocol uses [Prose form](prose.md#prose-form). It contains instructions on how a commit cycle
is accomplished.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

The artifact a cycle produces is whatever the bot generates from the conversation: code, prose, an
image, a song, a screenplay. The steps below use a Rust crate as the running example (the cargo
cycle, `Cargo.toml` versioning). Substitute your medium's equivalents. This project's manifest is
recorded in [versioning.md](versioning.md).

## Cycles

A cycle has three phases:

- **[Preparation](#preparation)**: the cycle's first commit, when it needs setup (a lightweight
  cycle omits it and starts at its first Work step). Sets up the cycle:
  - Bump the version-of-record (where it lives and the suffix scheme are project-specific, per
    [versioning.md](versioning.md)).
  - Pick up a `## Todo` item (typically the top-ranked, #1) into `## In Progress` as the cycle's
    [provisional items](#preparation): title, problem statement, solution statement, acceptance
    check, ladder, deliberation.
  - Create the cycle's topic bookmark.
  - Delete whatever `## Closed` holds first. The block is the cycle's only record
    ([Cycle-record](#cycle-record)).
- **[Work-N](#work-n)**: the commits that implement the change. As many as the change needs. Each
  runs through the [per-commit flow](#per-commit-flow).
- **[Close-out](#close-out)**: the cycle's last commit. Bookkeeping only:
  - Run the acceptance check and record what it showed.
  - Finalize the cycle-record and move the block to `## Closed`.
  - Land the cycle's bookmark.
  - Optionally update `notes/README.md` if functionality changed.

A cycle's commits are published to the project remote rung by rung, and the result must always be
published at close-out. See [Pushing](#pushing).

**Shape.** Single-step or multi-step is decided at the opening and fixed by the first push. A
single-step cycle's one commit carries all three phases under the bare cycle title, and a multi-step
cycle's ladder is never squashed, since each rung's `ochid:` trailer is a change id. See [Cycle
shape](../AGENTS.md#cycle-shape).

**Sub-cycles.** When a Work commit's scope grows enough to warrant its own ladder, it subdivides
into its own Preparation / Work / Close-out. The same three-phase shape applies recursively at every
depth, and a sub-ladder's rungs are titles like any other. See [Step naming](#step-naming) for how a
step is identified and [Sub-cycle ladders](#sub-cycle-ladders) for the local-ladder mechanics.

## Cycle-record

A cycle's record is its `TODO.md > ## In Progress` block and nothing else. It is written at
Preparation as the [provisional items](#preparation), revised as steps land, and finalized at
[Close-out](#close-out), whose commit moves it whole to `TODO.md > ## Closed` (a single-step cycle's
one commit writes it there directly), so the closing commit's tree carries the finished record and
`## In Progress` is empty between cycles. The next cycle's Preparation deletes `## Closed`'s
contents, so the file never grows. After that jj holds the record: `git log --grep "<cycle title>"`
finds the cycle's commits, and the landmark on `main` (the trapezoid merge, or the single-step
commit) holds the finished block in its `TODO.md > ## Closed`.

Nothing is backfilled. A rung carries no SHA, no version, and no placeholder for either: a commit
cannot record its own SHA, and a record written one push late was found unfilled at the next opening
more than once. The commit is the record of itself.

A design finding that must outlive the cycle is written into a `notes/` file by the rung that made
it, never left in the block, since the block is replaced.

`notes/chores/chores-NN.md` and `notes/done.md` are the records of earlier cycles and are frozen:
never appended, never opened, still linked ([Frozen
history](notes.md#frozen-history-chores-and-done)).

## Preparation

The cycle's first commit, when the cycle needs setup (a lightweight cycle omits it, per
[versioning.md](versioning.md#suffix-scheme)):

- **Create the cycle's topic bookmark** (commands in [Cycle
  bookmarks](jj.md#cycle-bookmarks-create-and-land)).
- **Bump the version-of-record.** Where it lives, the suffix scheme, and any derived files (a
  lockfile, a sourced manifest version) are project-specific. See [versioning.md](versioning.md).
- **Move a `## Todo` item** (if the cycle has one) into `## In Progress`, and write the
  cycle-record's **provisional items** there. All are required, all are revised as steps land, and
  all are finalized at close-out. The title is a heading one level below `## In Progress`'s own, and
  the other five are headings one level below the title (a plain cycle: `###` title, `####` items,
  and under a program heading, each one deeper):
  - the **title**, the cycle's name.
  - the **problem statement**: what is wrong, in a sentence or two.
  - the **solution statement**: what will be done about it, broad. Provisional here, since it is
    written before the work. The close-out's commit body carries the final one.
  - the **acceptance check**: the measure of "are you finished?". Not the per-commit validation,
    which asks whether the artifact still works. This asks whether the thing the cycle promised
    actually happened, specifically enough that a reader can run it.
  - the **ladder**: one rung per step, in the form `- [<title>][M] (marker)` described below.
  - the **deliberation**: how the five above were decided, alternatives weighed, costs accepted.
    `_None._` when there was nothing to deliberate, which is a real answer and different from having
    forgotten to write it.

The ladder is a linked table of contents. A rung is `- [<title>][M] (marker)` and carries no detail
beyond that:

- No SHA, no version, no placeholder for either: nothing is backfilled
  ([Cycle-record](#cycle-record)). The close-out just drops the `(current)` / `(done)` markers.
- The title links to the rung's subsection below, reference-style: `[M]` is a file-local slot whose
  definition is a same-file fragment, `[M]: #<slug>` in the file's `# References` (slug algorithm in
  [Markdown anchor links](notes.md#markdown-anchor-links)). Routing through the table keeps rung
  lines short.

The verbiage lives in the rung's subsection under a **`Ladder details`** heading following the
deliberation, headed by the rung's exact title, so it is greppable and an anchor other records can
link. Every rung has one, the close-out included, written in two beats:

- **Opened at laddering** with an abstract-sized intent statement: the rung's problem and solution
  in a sentence or two, provisional like the rest of the block, so a rung nobody has started is
  described by more than its title.
- **Completed at the rung's landing** with the conceptual delta (design points, consequences,
  deferrals). It never restates the landed commit body's problem/solution: the body is the record,
  and the subsection keeps only what the body does not say.

The closing rung differs only in its content. Its title is the cycle title plus " closing" (the
bookend form: [Cycle bookend titles](prose.md#conventional-commit-shape-ladder--commit)), it is
linked like its siblings, and its subsection opens at laddering with the one-line stub "Closing out
the cycle.", since its problem and solution are the block's own Problem and Solution items. At
close-out the subsection completes with what closing taught (acceptance surprises, validation
trip-ups), written in problem/solution form, or `_None._` when closing taught nothing.

**Why an acceptance check, and why it is provisional.** A cycle's own per-commit checklists can all
pass while its banner claim is false: a seven-cycle program opened against "end subprocess spawning"
and its close-out claimed the goal met, with about twenty spawn sites surviving, two inside the
facade the program built. Being provisional, the check can also be revised *toward* what was
achieved, which is the same failure by a slower route. So a changed check is one of the things the
deliberation exists to justify.

## Work-N

The cycle's work commits (`X.Y.Z-1`, `X.Y.Z-2`, ...) implement the change. As many as needed:

- Each commit runs through the **[per-commit flow](#per-commit-flow)**.
- **Interim pushes** are optional (backup, progress visibility).
- Close-out is the only mandatory push (see [Pushing](#pushing)).
- **Subdivide into a sub-cycle** if a Work commit's scope grows enough (see [Sub-cycle
  ladders](#sub-cycle-ladders)).

## Close-out

The cycle's last commit does bookkeeping only, and the commit body describes that bookkeeping, not
what happens post-squash:

- **Run the acceptance check** the Preparation stated, and record what it showed in the block,
  whether or not it passed. A check that was never run is a failed close-out, and a check that
  failed is a finding, not a reason to quietly restate the banner.
- **Finalize the cycle-record in place** ([Cycle-record](../AGENTS.md#cycle-record)): sync the title
  if the cycle's scope shifted (and every anchor back-reference), replace the provisional solution
  statement with what was actually done, drop the ladder's `(current)` / `(done)` markers since
  as-built implies done, and add any design subsections the deliberation grew. Then move the block
  whole to `## Closed`, leaving `## In Progress` reading `_No cycle currently in progress._`
  ([Cycle-record](#cycle-record)). Under a multi-cycle program (the program heading above the cycle
  title) the program heading and its ladder stay, the shipped rung flipped `(done)`.
- **Land the cycle's bookmark** on the user's go (commands in [Cycle
  bookmarks](jj.md#cycle-bookmarks-create-and-land)). Until this, nothing the cycle pushed is
  permanent.
- **Update `notes/README.md`** if functionality changed (new flags, new subcommands, changed
  behavior).

A single-step cycle does all of it in its one commit and has no shape to choose. A ladder's
published shape, trapezoid or keep separate, is chosen at close-out and executed at Land. See [Shape
at close-out push](#shape-at-close-out-push).

## Step naming

A step has a title and no number. The ladder lists its rungs in order, so position is already
recorded by the list, and a step is referred to by its title, verbatim-identical in the ladder rung
and the commit (see [Steps are named, not numbered](prose.md#steps-are-named-not-numbered)). A title
has to be unambiguous within its cycle and within its block, where it is also an anchor. It may
repeat across the repo's history.

The version-of-record still bumps for every step, and its suffix still encodes the phase, but that
encoding belongs to the manifest and appears nowhere in prose. It is the one number left in the
system, it names nothing, and nothing dereferences it. The full scheme (disambiguation, nesting,
optional Preparation, the project's version-of-record format, and the per-step bump) lives in
[versioning.md](versioning.md#suffix-scheme), which is the single source of truth for this repo's
versioning.

## Topic bookmarks are drafts

A topic bookmark is a draft until it lands on a permanent branch. Pushing to the bookmark makes the
work durable and visible, but it does not publish it. Landing on the permanent branch is
publication, and that is the line the rules divide at:

- **Before landing**, the series should be self-consistent when practical. Inserting or reordering a
  step changes the ladder, and the rungs that already committed an older version of it are brought
  along, so the branch reads as one coherent ladder rather than a record of how it was assembled.
- **After landing**, the commits are history and are not touched. No record cites a SHA
  ([Cycle-record](#cycle-record)), which is what makes rewriting a draft safe.

Mechanics, and why they cost so little here:

- **Amend content, never re-describe.** Editing `TODO.md` inside a rung and amending it is not a
  `jj describe`, so the never-re-describe rule stays intact. `ochid:` trailers survive, since they
  carry change ids rather than commit ids and a change id is stable across a rewrite.
- **Force-push the bookmark** afterwards, under the same approval any push needs.
- **Exceptions**, since "when practical" is not "always". Name the reason and move on:
  - the bookmark has already landed
  - another branch is stacked on it, so the rewrite becomes someone else's rebase
  - the ladder is long and only a trailing snapshot disagrees

A squash-form [sub-cycle ladder](#sub-cycle-ladders) never meets this, because nothing on it is
pushed and the close-out squash collapses it. The rule is for the multi-commit shape, whose rungs
publish one at a time.

## Per-commit flow

Every commit (Preparation, each Work commit, Close-out) goes through:

1. **Mark this commit `(current)`** as the first edit in `TODO.md > ## In Progress` (`TODO.md` is at
   the repo root).
2. **Do the work** (see [Iterative work](#iterative-work) for the loop-and-squash technique).
3. **Flip this commit `(current)` -> `(done)`** in `## In Progress`, before the cargo cycle and the
   commit. Write the rung's `Ladder details` subsection now, when it has conceptual content (see
   [Preparation](#preparation)).
4. **Bump the version-of-record** to this commit's version (the suffix scheme is in
   [versioning.md](versioning.md#suffix-scheme)). The Preparation's own bump already covers a
   Preparation commit.
5. **Validate the artifact**, a medium-specific step. If the medium has a runnable artifact, run it
   at every commit, doc-only ones included: step 4 changed the version, and running it is how that
   is verified. For the Rust example the cargo cycle is:
   1. `cargo fmt`
   2. `cargo clippy --all-targets -- -D warnings`
   3. `cargo test`
   4. `cargo install --path . --locked`
   5. (re-test if anything substantive changed)
6. **Work review.** Stop *before* writing any description, and tell the user "please review". The
   stop is its own message and carries no title or body, drafted or final: a description beside the
   work review collapses two stops into one, and describes work the review may still change. The
   user reviews the changes and we iterate until complete.
7. **Write the commit description**, only once the work review completes. See [Commit
   description](#commit-description).
8. **Commit Description review.** Show the title + body and stop. The user reviews the description.
   Iterate.
9. **Commit + push.** Hand the approved title/body to
   `vc-x1 push <bookmark> --title "..." --body "..."`, whose commit stages commit both repos and
   stamp the `ochid:` trailers. Never pre-commit the rung with `jj commit`: an empty `@` at push
   mints a stamped empty duplicate. Push approval is per-push, so step 8's review covers it only
   when the user's go explicitly includes the push.

**Two overrides apply:**

- **Deviation or question.** Any time the work deviates from the agreed plan, or a question arises,
  stop and surface it. Don't push through.
- **ESC-ESC.** The user can interrupt at any point to pull a review or question forward.

## Commit description

[Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: <short description>
<type>(scope): <short description>   # optional scope
```

**A commit names no version**, in its title or its body. The version-of-record (where it lives and
its bump cadence, see [versioning.md](versioning.md)) is useful for confirming you are running the
version you are testing. It is not an identifier, and a commit already records it in the manifest.
Writing it into the description copies it into text that cannot be edited: a version is only stable
once it lands on the permanent branch, and even then a history rewrite may renumber it, at which
point every description naming it is wrong forever. See [Versions live in the version-of-record
only](prose.md#versions-live-in-the-version-of-record-only).

### Title

- Length per [Line widths](prose.md#line-widths).
- Common types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`. Optional `(scope)` in
  parentheses after the type, per the spec.
- Favor terse phrasings.
- **Distinct per step.** Each of a cycle's commits gets its own descriptive title (no shared cycle
  title with a step marker). Share a greppable stem across the cycle's titles (e.g. `ring buffer`)
  so `git log --grep` collects them. See [Conventional-commit
  shape](prose.md#conventional-commit-shape-ladder--commit).
- **Unambiguous where it is resolved**: the title is the only identifier a record has, so it must be
  distinct within its cycle's block (a subsection heading is also an anchor). It may repeat across
  the repo's history. See [Steps are named, not numbered](prose.md#steps-are-named-not-numbered).

### Body

A **problem statement** then a **solution statement**, in [Prose form](prose.md#prose-form) (intro +
bullets), wrapped per [Line widths](prose.md#line-widths). The problem statement says what was wrong
and defines any word the title assumes. The solution statement says what was done about it. How the
two are arranged when the problem has several facets is [Commit-body
form](prose.md#commit-body-form): an intro paragraph, `*` bullets for facets, `-` bullets for
solutions. That and the rest of the content rules, including why the body carries no file list, are
in [prose.md](prose.md#prose-form) and are not repeated here.

Two repo-specific points:

- **Work-repo body**: the problem is the artifact's or the records' problem.
- **Bot-repo (`.claude`) body**: the statements describe in-session activity rather than work-repo
  changes.

### Trailer

`ochid:` as the last line of the body. See [Cross-repo linking (ochid
trailers)](jj.md#cross-repo-linking-ochid-trailers) in agent-data/jj.md for the convention.

For breaking changes, use the hyphenated `BREAKING-CHANGE:` trailer key. `BREAKING CHANGE:` (with a
space) is the only space-separated key the Conventional Commits spec allows. The hyphenated form is
also valid and avoids the space ambiguity.

## Reviewing changes

Work review looks at the **uncommitted working-copy diff**, on the way to commit. The user opens
diffs in their editor (Zed, VSCode), and jj commands are for terminal:

- `jj diff`: working-copy diff (uncommitted)
- `jj diff -r @-`: diff of the previous commit
- `jj diff --from <X> --to <Y>`: any two revisions
- `jj show -r <X>`: description + diff for one rev

Don't `jj edit -r @-` to view a past commit, because that marks it mutable and shifts `@`. Use
`jj diff -r @-` or `jj show -r @-`.

No preflight while a review iterates: `fmt` / `clippy` / `test` wait until the review settles, since
`fmt` mutates files in ways that interact badly with the user's mid-review edits. Validation runs
once, on the settled state, per the per-commit checklist.

See [Sub-cycle ladders](#sub-cycle-ladders) for the close-out squash recipe and recovery. Revset
primitives are in [`jj.md > Revsets`](jj.md#revsets).

## Pushing

### Policy

Push is **discretionary** during the cycle (backup, progress visibility) and **mandatory at
close-out**, since the cycle's result must be published.

**Approval is per-push.** Every push (any repo, any kind: cycle push, interim backup,
recovery/surgery force-push) happens only after the user has reviewed the changes to be published
and explicitly approved that specific push. Approval of a plan that *includes* a push does not
authorize the push itself. Stop and ask again at the moment of pushing.

**Default is interactive, and only an explicit scoped delegation waives the gates.** The gates above
(per-push approval, the commit-description review that shows title+body and stops, and the hard stop
after push/squash-push) are the *interactive default*. They yield when the user **explicitly**
delegates a complete, bounded task and authorizes carrying it through ("do all of X and push each
step, don't check in"). The bot then proceeds through that task's commits and pushes without
stopping, and continues past each push to the next step. Conditions:

- **Explicit grant.** Never inferred from a task merely being well-scoped. The user's words must
  authorize unattended completion. "Commit and push" (or "then push") names the destination, not a
  waiver: it authorizes the push *after* the normal work review and description review, not skipping
  them. Only wording that explicitly waives the stops ("don't check in", "no need to review", "carry
  it through unattended") waives them.
- **Bounded goal.** Covers the named task only. It does not carry to the next task or a vaguer
  follow-on.
- **Destructive ops still pause.** Delegation covers the task's ordinary commits and pushes. It does
  *not* pre-authorize a genuinely irreversible action (force-push over published history, history
  rewrite, deleting a remote branch). Those can permanently destroy work and aren't a normal cycle
  step, so the bot flags one before acting. An ordinary delegated cycle never reaches this.
- **Still transparent.** Report each commit/push as it lands (title + outcome) so the user can catch
  up.
- **When in doubt, ask.** Ambiguous authorization falls back to per-push approval.

**Delegation waives stops, never flow.** The stops (work review, description review, per-push
approval, the hard stop after the final push) are the synchronous half of review. The flow (the
records, the validation, the bookmark discipline) is what deferred review reads, so no delegation
waives it: a delegated cycle writes every record and validates every commit exactly as an
interactive one, and the user reviews after the fact what they would otherwise have reviewed in real
time. The tiers:

- **Interactive** (the default): every stop, as above.
- **Delegated cycle**: rungs push to the topic bookmark without per-push asks, and `main` is
  untouched by construction, so review happens at landing: read the line, then land it.
- **Delegated project**: landing is delegated too, review happens after, and corrections become new
  cycles.

Destructive ops pause in every tier, and landing is its own tier, delegated separately.

### Shape at close-out push

At close-out the cycle's *work* is done, and its *published shape* is the remaining choice, made at
push time. Surface the options and get user approval before pushing. Once on the target, changing
shape is a remote rewrite (force-push, needs approval), so choose deliberately.

- **Trapezoid** (a `git merge --no-ff` merge commit) *(current default)*: the target gains a merge
  commit (`X.Y.Z`) whose first parent is the trunk line and whose second parent is the cycle's
  ladder, so `git log --first-parent` reads one commit per cycle while every rung stays reachable.
  See [Trapezoid close-out recipe](#trapezoid-close-out-recipe) for the full sequence.
- **Keep separate**: one commit per cycle entry on `main`. Use when the decomposition itself is
  informative.

A trapezoid is reshaped between two pushes (the recipe below), and keep separate needs nothing.
Squashing the ladder is not a shape: each rung's `ochid:` trailer is a change id and a squash
discards all but one ([Cycle shape](../AGENTS.md#cycle-shape)). A single-step cycle has nothing to
reshape.

### Trapezoid close-out recipe

A [trapezoid](#shape-at-close-out-push) close-out is published in four steps: an ordinary close-out
push, a two-command reshape, and a second push that re-points the bookmark at the reshaped commit.

```
  main line   ...──<base>──────────────────<closeout>──
                      \                    /
  ladder             <rung-1>──...──<tip>─┘
```

- `<base>`: the **parent of the ladder's first rung**, which is the trunk position when the cycle
  opened. It becomes the first parent.
- `<tip>`: the cycle's last Work commit. It becomes the second parent.
- `<closeout>`: the close-out commit, created by step 1.

The steps. Only step 1 is a `vc-x1 push`, and the rest is jj, because after step 1 the commits
already exist and all that remains is reshaping and publishing them:

1. `vc-x1 push <bookmark> --title "..." --body "..."`
   - the ordinary close-out push. It commits both repos, stamps the
   `ochid:` trailers, and publishes `<closeout>` linearly.
2. `jj rebase -r <closeout> --onto <base> --onto <tip>`
   - `<closeout>` becomes the merge. Parent order is the
   argument order.
3. `jj new <closeout>`
   - puts an empty `@` above the merge.
4. `jj git push --bookmark <bookmark> -R .`
   - publishes the reshaped commit. The bookmark needs no `jj bookmark set`:
   it follows the rewrite in step 2 automatically. The bot repo is untouched, and its session tail
   goes out with a separate `vc-x1 squash-push` afterwards.

**Step 4 is not a `vc-x1 push`**, learned at a close-out that tried it. Push runs its whole pipeline
or none of it, and the bot repo is never quiet for long: by the time the reshape is done, `.claude`
holds the session writes from steps 1-3, so `commit-bot` wants to run and the message stage demands
a title for it. The result is a bot-side requirement blocking a work-side publish that needs nothing
but a moved ref. Publishing an already-made commit is a different operation from committing and
publishing, and only the latter is push's job.

#### Details

- **Verify two parents before step 4.**
  `jj log -r <closeout> -T 'parents.map(|p| p.change_id().short(8))'` must list both. jj preserves
  the second parent even though `<base>` is an ancestor of `<tip>` (observed at three consecutive
  close-outs), but a collapsed merge is indistinguishable from a correct one in `jj log --no-graph`
  and is only visible once published.
- **`<base>` is not always the previous close-out.** A docs or planning interlude between cycles
  sits on the trunk line and must stay there. Take the parent of the ladder's first rung, not the
  last close-out.
- **Step 3 is about `@`, not the bookmark.** The bookmark follows the rewrite in step 2 on its own.
  What step 2 leaves misplaced is the working copy: `jj rebase -r` re-parents descendants onto the
  rebased commit's **old** parent, so the empty `@` from step 1 lands beside the merge on `<tip>`,
  and the working tree reverts to pre-close-out content, which looks alarming and isn't. `jj new`
  puts `@` back on top of the merge so the tree is right and the next commit continues from there.
  Skipping it doesn't break the publish, it just leaves you working from the wrong parent.
- **Trailers survive.** The reshape changes `<closeout>`'s SHA but not its change ID, so the
  `ochid:` trailers stamped in step 1 stay valid in both directions. This is why the reshape is safe
  after the trailers are written.
- **Step 4 moves the bookmark sideways.** Step 1's SHA becomes unreachable, and anyone who fetched
  between the two pushes holds a dangling commit. Nothing records a SHA from that window, or from
  anywhere ([Cycle-record](#cycle-record)).
- **Immutability.** No flag is needed on a long-lived topic bookmark. Only when `<closeout>` is
  already on `trunk()` does the rebase need `--ignore-immutable`, and then the push force-updates
  the target.
- **The bot repo is left for afterwards.** Step 4 touches only the work repo, so `.claude` still
  holds every session write from the whole procedure. `vc-x1 squash-push` folds that tail into the
  bot commit, and its change id survives the squash, so the work-side `ochid:` keeps resolving.

#### Recovery

- **Nothing is published between steps 2 and 3**, so the local reshape is undoable with `jj undo` /
  `jj op restore`.
- **A collapsed or mis-parented merge** (step 2 verification fails): undo and redo step 2 with the
  corrected revisions. Do not push a shape you did not intend. After step 4 the remote boundary is
  crossed and recovery is forward-only.
- **Working copy left beside the merge** (step 3 skipped): `jj new <closeout>` after the fact.
  Nothing published is affected (the bookmark was never wrong), but any commit made in the meantime
  branches off `<tip>` and needs a rebase onto the merge.
- **A wrong bookmark position**, however it arose: `jj bookmark set <bookmark> -r <closeout>` before
  pushing. If step 4 already published it, the fix is a second sideways move, not a rewrite.

### vc-x1 push wrapper

`vc-x1 push <bookmark>` wraps per-push mechanics. See `vc-x1 push --help` for current flags.
`<bookmark>` names a work-repo bookmark only. The bot repo is always pinned to `main` (see [.claude
cadence](#claude-cadence)).

**Current limitation**: only fully supports the [Keep separate](#shape-at-close-out-push) shape.
Other shapes need manual jj steps. Planned improvements are project state, tracked in the project's
`TODO.md`. This protocol describes only the stable mechanism.

### .claude cadence

**Cadence**: one push = one bot-repo commit, paired with every work-repo commit in that push.

The `.claude` working copy accumulates session data across the cycle. Its change ID stays stable
across snapshots, `jj describe`, and the squash-push fold, so work-repo `ochid:` trailers resolve.

`.claude` is a linear journal: all session work lives on `main`, regardless of the work-repo
bookmark. **Do not create or maintain bot-repo bookmarks that mirror work-repo branches**, which
risks the bot steering session pushes to the wrong remote ref.

Ending a session: if the user runs `/exit` there will be session information created, which we don't
worry about. The user can close the terminal instead and `@` will remain empty.

### Bot communication at the reviews

Use plain prose, no insider jargon ("Gate N signal", "Checkpoint N", etc.):

- **At Work review**, summarize what changed and stop. "Work complete. Please review." No title or
  body in this message: the description is not yet written, and belongs to the next review.
- **At Commit Description review**, present `$TITLE` and `$BODY` explicitly, and ask permission to
  commit/push. Don't spell out the full `vc-x1 push ... --title ... --body ...` invocation by
  default.
- **At Post close-out review**, surface the shape options (trapezoid / keep separate) and the push
  target, then wait for the user's choice before any `jj rebase` / `jj git push` invocation.

### After push or squash-push: stop and wait

After a **push** (crossing the remote boundary, by hand or via the `vc-x1 push` wrapper, whose last
stage publishes the bot repo too) or a manual **squash-push** on the bot repo, stop for the turn: no
next step, edit, tool call, or text output until the user directs otherwise. **Even when the next
step seems obvious, wait.**

- **Scope**: the stop follows the user's directive, not the push. A standing directive covering more
  work ("finish the remaining ladder commits on your own") makes an intermediate push just a step,
  and the hard stop lands on the turn's *final* push.
- **Why**: the bot repo is a live journal, so everything after the invocation (its own record,
  closing words) lands in `@` as a trailing tail. Between delegated pushes the tail rides into the
  next cycle's bot commit. The final push's tail has no next commit, and the bot's own squash-push
  is itself session data (`@` refills immediately), so only the user, after the turn, can capture it
  (`vc-x1 squash-push -R .claude`).
- **Silence**: put all closing words *before* the final push. The harness rejects an empty turn, so
  it may force a visible token after the tool returns. If so, emit a bare acknowledgment only (e.g.
  "landed"), never a summary, verification, or next-step offer. There is no "harmless" closing line
  after the push. That is a known slip.
- **Flush**: when the user wants `@` empty (no tail), they run `vc-x1 squash-push -R .claude` after
  the bot goes quiet, which flushes all bot session information into the published commit. Repeat if
  new writes land (see [Recovery](#recovery)).

### Recovery

- **If push exits before its last stage** (`push-work` succeeded but the bot-repo publish
  (`squash-push-bot`) didn't run), run the squash+push by hand:

  ```
  vc-x1 squash-push -R .claude
  ```

  It runs in-process, so a failure is a visible non-zero exit, with no log file to chase.
- **Run squash-push again if `@` is non-empty** after a pass (also desirable after extra activity by
  the bot's agents).
  - Why: the bot keeps writing session data while the command runs, so the invocation's own record
    plus any closing response land after the squash.
  - Safe to repeat: bot session data is append-only, so a re-run never conflicts or overwrites.
    (This could change. It is not under the user's control.)
  - No guarantees: events outside the bot's control can leave `@` non-empty. The bot's back end may
    decide to squash/consolidate session data, which can take minutes and land after the pass. The
    remedy is the same: just run squash-push again. This is why a single pass is never guaranteed to
    leave `@` empty.
- **Nothing to clear after an out-of-band recovery.** Push keeps no saved state, so whatever you do
  to the repos by hand is simply the state the next run sees.
- **Late work-repo tweak after the work-repo push succeeded** (e.g. updating AGENTS.md or memory)
  requires `jj squash --ignore-immutable` and a re-push. That is a remote rewrite and needs explicit
  approval like any push.

## Iterative work

When work for a single commit (the **target**) benefits from incremental review, loop:

1. `jj new -R .`: fresh empty `@` on top of the target.
2. Make the next round of changes.
3. User reviews the round (see [Reviewing changes](#reviewing-changes)).
4. `jj squash -R .` folds into the target and creates a new empty `@`.
5. If not done, go to step 2.

Same jj mechanics as a [sub-cycle ladder](#sub-cycle-ladders), but at single-commit scope, so the
version doesn't change.

## Sub-cycle ladders

When a Work commit subdivides into a sub-cycle (see [Step naming](#step-naming), and
[versioning.md](versioning.md#suffix-scheme) for how the manifest's suffix nests), its Work commits
will live as a local jj `@` chain and **collapse into the sub-cycle's Close-out** before the parent
cycle continues. Ladder commits are scratch, for review and bisection only.

### Per-Work-commit contract within a ladder

For each Work commit in the ladder:

1. `jj new -R .`: create a fresh empty `@`.
2. Do the commit's work.
3. Run the fast validation (Rust example: `cargo test --bins`). **Non-negotiable**, because for
   code, build and clippy alone miss regressions until a later commit runs the full suite, raising
   bisection cost.
4. `jj describe -m "..." -m "..." -R .`: working title only. The sub-cycle Close-out collects
   everything into one final commit.

**Nothing here is pushed.** The ladder is local until the [Close-out
squash](#close-out-squash-the-ladder), so no ladder commit ever carries an `ochid:` trailer: the
trailer is stamped once, by `vc-x1 push`, on the squashed commit. Step 4 is therefore first-time
authoring of a scratch description, not a rewrite of a published or stamped one, and is the named
exception to [Re-describing](jj.md#re-describing-coordinate-first-and-keep-the-trailer).

### Navigating the ladder

Common moves:

- `jj log -r '<base>::' -R .`: see the whole ladder from its base.
- `jj edit -r <prefix> -R .`: jump `@` to any ladder commit by chid prefix, useful for bisection.
- `jj edit @-- -R .`: quick-jump back two commits.
- `jj diff -r <chid> -R .`: review one commit in isolation.

Modifications to any ladder commit rewrite it in place, and descendants auto-rebase.

### Close-out: squash the ladder

When all ladder Work commits are done and tests pass:

```
jj squash --from "<base>..@-" --into @ -u -R .
```

`<base>` is the parent of the first ladder commit, and `-u` keeps `@`'s description and discards the
sources'. After squash, history is linear (`<base> -> @`), and intermediate commits are
auto-abandoned.

Then `vc-x1 push <bookmark>` as for any other commit. This is the ladder's first and only publish,
and where its `ochid:` trailer is stamped. The scratch descriptions written at step 4 of the
[per-Work-commit contract](#per-work-commit-contract-within-a-ladder) never left the machine and
never carried one, which is why describing them freely is safe.

For N = 1 the squash is a no-op (`<base>..@-` is empty when `@-` is `<base>`). Push the single
commit directly.

### Recovery

If a ladder commit goes wrong, back out without losing prior commits:

- **Discard the current commit.** `jj abandon @ -R .` drops it, and you get a fresh empty `@` on the
  same parent.
- **Edit an earlier commit.** `jj edit -r <chid> -R .`, make corrections, then
  `jj edit -r <last-ladder-chid>` to return. Descendants auto-rebase.
- **Discard the entire ladder.** `jj op log -R .` shows the op history, and
  `jj op restore <op-id> -R .` reverts to that point. Full undo: removes *all* ladder work after the
  chosen op. Use only to start over.

# References

- [`jj.md > Revsets`](jj.md#revsets): revset primitives (chid/cid, `@`/`@-`/`@+`, `..`/`::` ranges,
  prefix matching).
- The per-commit `cargo test --bins` gate exists because a regression introduced in an early ladder
  commit can go uncaught until a later commit runs the full suite, raising bisection cost.
