# Cycle checklists

Checklists for the moments where slips happen: committing, pushing, closing out. The full protocol,
with rationale and recovery procedures, is [cycle-protocol.md](cycle-protocol.md). On any
disagreement, that file wins. Re-read the relevant checklist immediately before the action. Don't
run it from memory.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## The cycle at a glance

Every change runs as a **cycle** with three phases: Preparation -> Work -> Close-out. The phases
always happen, but there are two styles. A **multi-step** cycle commits them individually, as a
ladder of steps (the Preparation commit is optional). A **single-step** cycle folds all three into
one commit when the change is straightforward, so that one commit is the close-out and carries its
duties. So a single-step cycle is one commit, and a multi-step is minimum two (a Work commit plus
the close-out), typically three or more (the definition is [AGENTS.md's
Terminology](../AGENTS.md#terminology)). The style is fixed by the cycle's first push and never
changed by reshaping ([Cycle shape](../AGENTS.md#cycle-shape)). The ladder lists a cycle's steps in
order and identifies each by its title, with no number and no version. Where the version-of-record
lives and how often it is bumped are in [versioning.md](versioning.md). Read
[cycle-protocol.md](cycle-protocol.md) before any commit work, and before any push, cycle or not.

## Cycles run on a bookmark

A cycle runs on one topic bookmark in the work repo, created at the opening and named by the cycle
title's slug (the anchor algorithm in [Markdown anchor links](notes.md#markdown-anchor-links)).
`main` advances only when the finished cycle lands on it, and nothing pushes straight to `main`. The
bot repo needs no bookmark: its `main` rides the tip of its linear narrative.

- **The bookmark is the unit of review.** Everything the cycle does is visible as one line against
  `main`, and until it lands the whole line is a draft that can be reshaped ([Topic bookmarks are
  drafts](#topic-bookmarks-are-drafts)). Landing is the single approval that makes the cycle
  permanent.
- **Create at the opening**, land at the close-out. Commands are in [Cycle
  bookmarks](jj.md#cycle-bookmarks-create-and-land), and the opening's other duties are in the
  [Opening checklist](#opening-checklist) below.
- **A single-step cycle still gets one.** The saving is not worth the exception, and a one-commit
  line is exactly the case where a pre-landing rewrite is cheapest.
- **The permanent branch is whatever this project calls it** (`main` here). A long-lived program
  bookmark is a different animal, governed by [Long-lived
  bookmarks](jj.md#long-lived-bookmarks-merge-only-by-default-deletable-once-merged).

**Why:** a cycle that pushes `main` directly makes every correction a coordinated force-push of
published history. Landing costs one command and buys free rewrites for the whole cycle.

## Opening checklist

At the cycle's opening, before the first Work commit. A single-step cycle does all of it in its one
commit, after item 1 ([Cycle shape](../AGENTS.md#cycle-shape)):

1. Create the cycle's bookmark ([Cycle bookmarks](jj.md#cycle-bookmarks-create-and-land)).
2. Move the picked-up item into `TODO.md > ## In Progress` and write the cycle-record's
   **provisional items** there, all required. The title is a heading one level below
   `## In Progress`'s own level, and the other five are headings one level below the title (a plain
   cycle: `###` title, `####` items, and under a program heading, each one deeper):
   - **title**, the cycle's name
   - **problem statement**: what is wrong, a sentence or two
   - **solution statement**: what will be done about it, broad
   - **acceptance check**: the measure of "are you finished?"
   - **ladder**: one rung per step, `- [<title>][M]` plus `(current)` / `(done)`, with
     `[M]: #<slug>` in the file's `# References`. The closing rung, `<cycle title> closing`, is
     linked like the rest
   - **deliberation**: how the five above were decided (`_None._` when there was nothing to
     deliberate)
   A `Ladder details` area follows them: one subsection per rung, closing included, headed by the
   rung's exact title, opened at laddering with the rung's intent and completed as rungs land, the
   closing rung's at close-out (see the protocol's [Preparation](cycle-protocol.md#preparation)).
3. Bump the version-of-record.

Item 2 first deletes whatever `## Closed` holds. The block is the cycle's only record
([Cycle-record](../AGENTS.md#cycle-record)).

**Why the acceptance check:** a cycle's own checklists are per-commit instruments and every one of
them can pass while the banner claim is false. Measured in a seven-cycle program opened against "end
subprocess spawning": its close-out claimed the goal met, with about twenty spawn sites surviving,
two inside the facade the program built. A measurement or performance claim is the shape most
exposed, since "we measured it" is easy to believe and hard to notice you have not done. Being
provisional, the check can also be revised *toward* what was achieved, which is the same failure by
a slower route. A changed check is one of the things the deliberation justifies.

## Committing vs pushing

A cycle rung is committed *by* `vc-x1 push`. Never pre-commit it with `jj commit`. Push's commit
stages commit both repos with the approved title/body and stamp each new commit's `ochid:` trailer
(see [ochid trailers](jj.md#cross-repo-linking-ochid-trailers)). A pre-committed rung leaves `@`
empty and push mints a stamped empty duplicate. In an instruction, "commit", "push", and "commit +
push" all mean `vc-x1 push`. A bare `jj commit` is asked for by name ("local commit", "just
`jj commit`") and is only for work that never publishes (local-only saves and loop-and-squash
intermediates), with no `ochid:`. The approval around a push, interactive by default and waived only
by an explicit scoped delegation, is the cycle protocol's [Pushing
policy](cycle-protocol.md#policy).

## Topic bookmarks are drafts

A topic bookmark is a draft until it lands on a permanent branch. Pushed there is not published. So
keep the series inside it self-consistent: inserting or reordering a step edits the ladder in the
rungs that already committed an older version of it, not only at the tip.

- **Amend content, never re-describe.** Editing `TODO.md` in a rung and amending is not a
  `jj describe`, so hard rule 4 stays intact and the `ochid:` trailers ride along untouched (they
  carry change ids, which survive a rewrite).
- **Then force-push the bookmark**, under the same approval as any other push.
- **Exceptions, where self-consistency is not worth its cost**:
  - the bookmark has already landed
  - another branch is stacked on it, so the rewrite becomes someone else's rebase
  - the ladder is long and only a trailing snapshot disagrees

The squash-form ladder below never meets this, since nothing on it is pushed. The rule is for the
multi-commit shape, whose rungs publish one at a time. Full statement in the protocol's [Topic
bookmarks are drafts](cycle-protocol.md#topic-bookmarks-are-drafts).

## Per-commit checklist

Every commit (Preparation, each Work commit, Close-out), per the protocol's [Per-commit
flow](cycle-protocol.md#per-commit-flow):

1. Mark the rung `(current)` in `TODO.md > ## In Progress`, as the first edit.
2. Do the work. On any deviation from the agreed plan, or any question, stop and surface it.
3. Flip `(current)` -> `(done)`, before validation and the commit, and complete the rung's
   `Ladder details` subsection with the conceptual delta (its intent stub was opened when the rung
   was laddered, and the ladder itself stays a bare ToC). See the protocol's
   [Preparation](cycle-protocol.md#preparation).
4. Bump the version-of-record to this commit's version (the suffix scheme is in
   [versioning.md](versioning.md)). The opening checklist's bump already covers a Preparation
   commit.
5. Validate the artifact at every commit, doc-only ones included. The medium's commands are in
   [custom.md](../custom.md).
6. Stop and ask the user, "please review", as this is the bottom of the review loop. Do not present
   a description as we iterate until the user reviews and says "continue|go|.." indicating the work
   review is likely complete.
7. Once the work review is complete, write the description: a conventional title, then a body in
   prose.md's [Commit-body form](prose.md#commit-body-form) (an intro paragraph stating the general
   problem, `*` bullets for its facets, `-` bullets for solutions, a `-` solving the nearest
   enclosing problem), sized per [Line widths](prose.md#line-widths). No version in either, no file
   list (the diff is the mechanical record), and no deliberation (the cycle record, todo, and the
   session hold that). See [Commit description](cycle-protocol.md#commit-description).
8. Show title + body and stop for review. This review covers the push only when the user's go
   explicitly includes it.
9. On the user's go: `vc-x1 push <bookmark> --title "..." --body "..."`. Never pre-commit with
   `jj commit`. Never hand-write `ochid:` trailers.

## Ladder (sub-cycle) checklist

Within a sub-cycle ladder, per the protocol's [per-Work-commit
contract](cycle-protocol.md#per-work-commit-contract-within-a-ladder):

1. `jj new -R .`: fresh empty `@`.
2. Do the commit's work.
3. Run the fast validation (named in [custom.md](../custom.md)). Non-negotiable.
4. `jj describe -m "..." -m "..." -R .`: scratch working title. This first-time authoring is the one
   permitted describe. The commit is never published and never carries a trailer.

Nothing on a ladder is pushed. The close-out squash
(`jj squash --from "<base>..@-" --into @ -u -R .`) collapses it, and `vc-x1 push` then publishes the
single commit.

## Before any push

- This specific push has the user's explicit approval. Approval of a plan that includes a push is
  not push approval. "Commit and push" names the destination, not a waiver of the reviews. Only an
  explicit scoped delegation ("do all of X, don't check in") waives the stops, for that bounded task
  only. Delegation waives stops, never flow (records, validation, the bookmark discipline, per the
  protocol's [Policy](cycle-protocol.md#policy)), and destructive ops still pause.
- Validation ran, and passed, after the last edit.
- Closing words are already written. Nothing follows the turn's final push (next checklist).

## After the final push: hard stop

Once the turn's final push or bot-repo squash-push is invoked, do no further work: no verification,
no summary, no next-step offers, no edits, until the user speaks. Put all closing words *before* the
invoke. The harness rejects an empty turn, so it may force a visible token after the tool returns.
If so, emit a bare acknowledgment only (e.g. "landed"), never a summary or more work. Post-push
verification happens next turn at the user's direction. A standing delegation makes intermediate
pushes just steps. The hard stop lands on the turn's *final* push. See [After push or
squash-push](cycle-protocol.md#after-push-or-squash-push-stop-and-wait).

## Close-out checklist

The cycle's last step, per the protocol's [Close-out](cycle-protocol.md#close-out). A single-step
cycle does all of it in its one commit, item 5 aside:

1. **Run the acceptance check** the opening stated, and record what it showed in the
   `## In Progress` block, whether or not it passed. A check that was never run is a failed
   close-out, and a check that failed is a finding, not a reason to quietly restate the banner.
2. **Finalize the cycle-record in place** ([Cycle-record](../AGENTS.md#cycle-record)): sync the
   title if the scope shifted, replace the provisional solution statement with what was done, drop
   the ladder's `(current)` / `(done)` markers, add any design subsections, and complete the closing
   rung's subsection: gotchas in problem/solution form, `_None._` when closing surfaced none. Then
   move the block whole to `## Closed`, leaving `## In Progress` reading
   `_No cycle currently in progress._` ([Cycle-record](../AGENTS.md#cycle-record)).
3. Full validation, mandatory.
4. Update `notes/README.md` if functionality changed.
5. At push time, surface the shape options (trapezoid / keep separate) and wait for the user's
   choice. Squash is not an option, since it breaks the rungs' `ochid:` links ([Cycle
   shape](../AGENTS.md#cycle-shape)). The trapezoid recipe is [in the
   protocol](cycle-protocol.md#trapezoid-close-out-recipe). Its step 4 is `jj git push`, not
   `vc-x1 push`.
6. **Land the bookmark** on the user's go. Until this, nothing the cycle pushed is permanent. Once
   `main` contains the bookmark, delete it, locally and remotely. See [Cycle
   bookmarks](jj.md#cycle-bookmarks-create-and-land).

## vc-x1 push behaviors to keep in mind

Three, independent of project language:

- **No checks of the project's own.** vc-x1 assumes nothing about a repo's contents beyond `.jj` and
  `.vc-config.md`, and runs no build or tests. The medium's validation is the per-commit flow's job,
  run *before* invoking `vc-x1 push`. The one check that remains is `push-work` verifying the
  bookmark's remote refs are tracked, which is its own precondition.
- **Rerunning is safe.** Push keeps no state and cannot resume. Every stage no-ops when its work is
  already done, so a failed run is re-run, not resumed. If a run fails, push stops and reports.
  Getting the repos back to a sensible state is the user's call, not something the tool infers.
- **ochid trailers** are injected by `vc-x1 push` itself, so don't hand-write them into the commit
  body or `--title`/`--body`.
