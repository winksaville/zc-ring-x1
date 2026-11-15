# Rationale

The why behind the agent-files, one entry per rule that has one. AGENTS.md holds the rule and its
boundaries, and a session needs only that. The argument is for whoever would change a rule, and for
whoever reviews a proposal, and it is kept so a rule is not simplified away by an editor who does
not know its cost.

## How to read this file

- **Headings mirror AGENTS.md's**, same text, same level, so the anchors line up 1:1 and a rule
  reaches its why by one fixed pattern, `[why](agent-data/rationale.md#<same-slug>)`. A heading
  with nothing under it but `_None recorded._` is a rule whose why was never written down, which
  is a finding, not a gap to fill with a guess.
- **Per-file sections sit at the end**, one `##` per agent-file, subheadings mirroring that file's,
  reached by the same pattern (`[why](rationale.md#<same-slug>)` from inside `agent-data/`). The
  other agent-files carry no inline why, only the link (swept 2026-08-27).
- **An entry is the why, then the evidence**: back references to the cycle where the rule was paid
  for, the messages-repo record, the commit. Mostly pointers, not a re-telling. The "measured
  YYYY-MM-DD" lines live here, with the story.
- **A boundary sentence is not rationale.** A sentence saying what a rule does not cover is the
  rule, and stays in AGENTS.md. What moves here is argument: why the rule exists, what it cost to
  learn, what the alternatives were.
- **Speculation is marked** as everywhere else ("We think ...", prose.md's [Speculation
  marker](prose.md#speculation-marker)), so a reader can tell the measured from the inferred.

## Rules

The rules are an index, seventeen bullets, each a one-sentence summary and a link to the section
that states the rule, so a rule lives in one place and a citation points there. Until 2026-08-27
each rule was a numbered heading in AGENTS.md with one line and a link, 82 lines that read as
spread out rather than as a list, and every rule had a mirror heading here, eleven of them
`_None recorded._`. The names stay, since a review cites a rule by name. The sentence stays too
(2026-08-28): AGENTS.md is in the agent's context every turn and the linked section is not, and a
bare name reminds an agent of a rule it holds without stating one it does not, so the sentence is
what the agent checks against. Bullets, not numbers, since the rules are unordered and numbering
bred "hard rule N" citations that broke on every renumber. Flat, not grouped by file
(2026-08-28): each bullet's link already shows the file, so a per-file header repeated it and cost
a nesting level, and the order alone keeps this file's rules ahead of the outer files'. A section
that has a why links it at its head, so the index carries none.

A bend's record names its scope (2026-09-05) because a scoped waiver, "complete it as you see fit"
over a whole cycle, left the agent to decide alone whether Land was inside it. It decided not, and
the decision was right, but a rule that makes the agent draw that line is a rule that will be drawn
differently next time.

## Terminology

**Retired names.** "Bot repo" (2026-08-21), when the code respelled the side `agent`. "Instruction
files", an earlier name for the agent-files. "Ladder (sub-cycle)" for a local ladder, under [Local
ladders](#local-ladders). "Project layer" (2026-10-18), the project's own agent-files, retired when
the one file it named became local ([custom.md](#custommd)). "Trunk" (2026-11-10), the one bookmark
every cycle was assumed to land on, `[cycle] trunk` else `main`, retired for the target, named on
the command each time. "`--onto`" (2026-11-11), the closing's flag for the target, retired with
`commit --closing` for `cycle --close --trapezoid <target>` and `cycle --land <target>`.

**No target is assumed** (wink, 2026-11-10) because a bookmark made is likely, not certainly, the
one a cycle lands on, and a release branch is a target as much as `main` is. A default would guess,
and naming it costs one word per Land. It is stored nowhere, the cycle's bookmark being its
title's slug and its branch the commits no other bookmark reaches (wink + agent, 2026-11-11). What
is history, which `squash` and `trailers` refuse to rewrite without `--ignore-immutable`, is jj's
`immutable()`, configured in jj, so vc-x1 keeps no list of shared lines, and pushing a cycle's
bookmark for a backup does not make its rungs history: pushing is not dangerous in itself.

**Two kinds of push are named** (wink + agent, 2026-11-11) because "push" meant both a backup of a
draft and the act that makes a cycle permanent, and only the second ends a cycle.

## The dual-repo model

Simplified (wink, 2026-09-07) because the earlier text defined the work-repo by a resolution the
reader had to perform, a walk up to the nearest config and its `work` key resolved against that
file's directory, and then spent a paragraph on why a walk that reached the agent-repo's copy
first still landed on the work-repo. Fixing `[repos] work` at `"."` makes the workspace root the
config file's own directory by definition, so the walk and the two-sided registry are the
mechanism's business and stay in jj.md. `[repos] agent` is the one cell the project chooses, and
saying it may be a path relative to the root or an absolute one states the freedom the registry
exists to give, in place of a clause about nesting.

## Acquaint

Written down (2026-10-09) because acquaint was a word used in five places and defined in none, so
what a session did first was whatever the agent recalled. The repo check is its new step, paid for
that day: a cycle opened in a clone one cycle behind both remotes, the cycle before it having been
made on another machine and never fetched, and nothing looked at a remote until the push, which
published the work-repo and was rejected on the agent-repo. The repair was a rebase, a second
version bump, and a forced push of the bookmark.

The check precedes the `TODO.md` read because a clone that is behind holds a stale `TODO.md`, so
its continuation notes and its `## Todo` order are the ones another machine has already replaced.
"Ahead" stops as "behind" does, since an unpushed `main` in the agent-repo is a commit the
work-repo's `ochid:` trailer names and no one else can reach. The look is not asked for: it moves
nothing, and a check the agent must ask to make is one it skips.

The other three findings (wink, 2026-10-09) are what a session inherits besides a stale clone.
Uncommitted work is the previous session's unfinished act, and only its continuation notes say
which act, so a change they do not account for has no owner the agent can name and is the user's
to place. Work off `main` gets a suggestion and not a rebase because rebasing a pushed bookmark
is a remote rewrite at its next push, which takes approval like any push. Conflicts stop the
acquaint because every later step reads files a conflict may have left holding both sides.

The hooks check (wink, 2026-10-27) comes before the repo check because its guard is on the
commands the repo check runs. A machine that lacks the hooks, a second one or a fresh clone, is
found the first time a session starts there, and the install is the user's because it writes their
settings, which every session on the machine reads.

The tools check (wink, 2026-11-09) comes before the repo check because the repo check runs the
tools, and a session that learns at its first commit which build it should have run has already
stamped that commit ([Tools the session runs](#tools-the-session-runs)).

## Sync

One section holds the command (wink, 2026-10-13) because it is run at several steps, and a
command written three times is changed in two. The steps are the ones that depend on `main`: an
acquaint reads the records a stale `main` holds, an opening creates the cycle's bookmark on it, a
rung is built on it, and Land moves it.

- Acquaint looks and does not sync so that "left as found" has no exception: the user hears of a
  repo that is behind before anything moves. The first form was a `jj git fetch` and a look, and
  the look-only sync replaced it since it reports what a sync would do.
- Opening syncs because an acquaint's look is as old as the session. A cycle opened hours into
  one, or the second cycle of a session, would create its bookmark on a `main` the remote may
  have moved past, the failure the acquaint's check was written for, reached by a later door.
  "Whatever the cycle's form" is there so a lightweight cycle's first commit and a single-step
  cycle's one commit owe it too.
- Land syncs because `main` is the one target someone else can reach first, a cycle's other
  pushes going to its own bookmark. A `main` that moved stops the Land because the tree that
  would land is no longer the one reviewed and validated, and its version-of-record was bumped
  from a `main` since passed. How to recover is left to the first time it happens.
- The flags `--force` and `--rebase` are the user's because sync holds a repo back exactly when
  following the remote would tangle local work, and overriding that is a rewrite.
- A rung syncs at its step 0 (wink, 2026-10-15) because a rung's start is the cheapest moment to
  take in a moved `main`: the working copy is empty, a rebase of the bookmark carries only pushed
  rungs, and the sync never moves the bookmark, so it cannot hurt the rung. The closing's is
  required because its record and version are what lands, and the opening's is the opening's
  first step already.
  - The moment weighed first was before the push, since every push lands on the agent-repo's
    `main` directly, the agent-repo having no bookmark. Step 0 leaves a long rung's push open to
    an agent-repo `main` moved meanwhile, a cost accepted: one session writes the agent-repo, so
    its remote rarely moves, and a re-run push is safe.
- A rung's sync between is optional, the agent's or the user's call, because a rung pushes to its
  own bookmark and Land catches a moved work-repo `main` anyway, so it buys only an earlier
  warning, worth having on a long cycle or with another session pushing. "When a repo has not
  been synced in a while" was weighed as a required moment and left out, "a while" being nothing
  an agent can check.
- The cycle's bookmark is looked at and never synced because a rebase of a pushed bookmark is a
  remote rewrite at its next push.

## Cycle protocol

The record has one home and is written once. Earlier forms kept a working ladder in `TODO.md` and an
as-built ladder in chores, so every rung was written twice and every backfill applied twice, and
detail written twice drifts (the same argument that keeps the edit list out of the commit body).

### Cycles run on a bookmark

A cycle that pushes `main` directly makes every correction a coordinated force-push of published
history. Landing costs one command and buys free rewrites for the whole cycle. A single-step cycle
gets a bookmark for the same reason: a one-commit line is exactly where a pre-landing rewrite is
cheapest. "Development is not done on `main`" is stated outright because the trapezoid recipe once
allowed "a docs interlude between cycles" on `main`'s line, and five docs commits went to `main`
that way before a sixth was caught mid-draft and run as a cycle (2026-08-22). The owners will
sometimes cheat, and the rule is still the rule.

Pushing to the bookmark makes the work durable and visible, but landing on `main` is publication,
and that is the line the rules divide at. The series is kept self-consistent before landing so the
branch reads as one coherent ladder. Amending content rather than re-describing keeps [No
re-describe without coordinating](jj.md#re-describing-coordinate-first-and-keep-the-trailer)
intact and lets the `ochid:` trailers ride along: they carry change ids, which survive a rewrite.

### Cycle shape

The tool tells the shapes apart by counting, with no title to read, and a cycle need not decide its
shape before its first commit: it is single-step until a second commit makes it a ladder. A dead end
lands because its commits are named by agent-repo trailers, and landing keeps every rung reachable.

**One command, its stages flags** (wink, 2026-11-11): `commit` made a rung, an opening, a closing
that was a merge or a rewrite in place, moved the record, landed, pushed, and described an older
commit, and most of the flow's surprises were in how those flags combined. `cycle --open`,
`--commit`, `--close`, and `--land` take the cycle's acts out of it, flags and not subcommands
because every `vc-x1` command is `command --options`, and the opening is the first commit, seen
and not flagged, since `--open` and `--opening` differed by three letters and meant two things.

**Every cycle has an opening and a closing** (wink, 2026-11-11), and in a single-step cycle the
closing is squashed into the opening, so there is one flow and the shape picks the commits.

**The shape is named, the count checks it** (wink, 2026-11-13), where the count alone had decided
it since 2026-11-11. The count is unambiguous over no commit and over two or more, and over one it
guessed: a single-step cycle whose opening was committed early and a ladder with only its opening
made are the same branch, and the guess squashed the second's closing into its opening, retitling
it. Named at the closing, the shape is still decided only when it is known, and the count turns a
wrong name into a refusal. Found testing in `~/tmp/wt-21-6`.

**Linear is the user's to ask for**: a trapezoid joins a target that moved and keeps one commit per
cycle on the first-parent line, so the rules close with `--trapezoid`, and the flag being the
trapezoid's is wink's design (2026-11-10), a closing without it being a plain commit.

### Unplanned work

The docs interlude, a commit landed on `main`'s line between cycles, is retired (2026-08-27). A
single-step cycle is the same one commit with a bookmark, a record, and a landing, and the five
docs commits that went to `main` as interludes are the case against the exception ([Cycles run on
a bookmark](#cycles-run-on-a-bookmark)). The choice between a rung and an entry is the user's
because it depends on what they want from the running cycle at that moment, which no rule can
know.

The set-aside sentence (2026-09-05, a patch then, a jj commit since a cleanup of `tmp/` lost two patches): a rename arrived while the working copy held the next rung's
guide edit, and the inserted rung would have carried both. Setting the edit aside as a patch and
re-applying it after the push is what was done, and it was invented on the spot.

### Cycle-record

The chores move, the `## Done` entry, the `done.md` migration, and the SHA backfill were retired
together (2026-08-26) because each was a second copy of what jj already holds: the move rewrote the
block through four transforms, two of which fail silently, the Done entry restated the title and
version the commit carries, and the backfill wrote SHAs one push late and was found unfilled at the
next opening twice (measured 2026-08-21). `git log --grep` on the cycle title finds the commits, the
landmark on `main` holds the finished block in its tree, and the trailer names the session. What the
block cannot keep, a design finding, was already going to `notes/` design files, and now goes there
by rule. The finished block moves to `## Closed` rather than being deleted by the closing commit,
because a commit that finalizes and deletes in one diff leaves the final form in no tree, and it
moves out of `## In Progress` because a finished cycle under that heading reads as a lie. The next
opening deletes it because keeping every finished block is unbounded growth, one block per cycle
forever, which is what made the chores files need splitting at ~1000 lines. Nothing is lost by the
deletion: the landmark commit holds the block, and the entire conversation that produced it is in
the agent-repo, reached from the commit by its `ochid:` trailer, which we hope to have easy access
to soon. `tmp/` was considered and rejected because it is gitignored, which is the no-tree loss
again.


**The ladder is `zTOC.md`'s alone** (wink, 2026-11-04) because two copies, one in the block and one
in the index, were kept in step by hand at every rung, and zed's markdown preview follows a link to
another file but not one within the same file, so only the index's ladder could be clicked through.
The closing keeps it because the landmark should hold the finished ladder beside the finished block,
as `## Closed` is deleted by the next opening rather than by the closing.

**The ladder is made, not typed** (wink + agent, 2026-11-11): a rung was written twice, a line in
`zTOC.md` and a heading in `TODO.md`, and its markers flipped by hand, so the two could differ and
a marker be wrong. The heading is the one place, and the markers are computed from the commits.
The ladder stays markdown because `zTOC.md`'s purpose is that it is clickable, so its grammar is
strict and the tool enforces it, a coding requirement being a higher bar than a naming protocol:
every commit of a cycle matches a rung, or the tool errors.

### Opening

`## Waiting` is checked at the opening because that is the moment a choice is being made, and a
blocked entry cannot hold a place in the order: the first entry means "next", and a blocked entry
there would say "do this" to every reader when the answer is "cannot" (wink, 2026-08-27).

**The bookmark is made first, by `vc-x1 cycle --open`** (wink, 2026-11-10, `vc-x1 open` until
2026-11-11), because the record, the
ladder, and the bump are a cycle's first edits, and made before its bookmark they sit on no branch,
a single-step cycle's included. It goes on `@`, so it follows the change into the opening, and a
command makes it rather than `jj bookmark create` for the name, the title's slug by the anchor
algorithm, which is fiddly by hand. It refuses only a name in use, the one thing it could break, and
the opening's push publishes it, a rung's push.

**The solution statement is provisional** because it is written before the work.

**Why the acceptance check, and why it is provisional.** A cycle's per-commit checklists can all
pass while its banner claim is false: a seven-cycle program opened against "end subprocess spawning"
and its close-out claimed the goal met, with about twenty spawn sites surviving, two inside the
facade the program built (found 2026-08-06 at the 0.78.3 review, and retired by the 0.79.0 cycle,
chores-17's [refactor: retire the remaining jj
spawns](https://github.com/winksaville/vc-x1/blob/2dc8d969c3f3/notes/chores/chores-17.md#refactor-retire-the-remaining-jj-spawns)).
Being provisional,
the check can also be revised *toward* what was achieved, which is the same failure by a slower
route, so a changed check is one of the things the deliberation exists to justify.

**Deliberation comes before the opening commit** (wink, 2026-11-11), so the opening carries the
plan and its review is of the plan, before any work. The cost is a working copy uncommitted while
the plan is talked through, so the user is free to commit the setup first. `cycle --open` writes
the record's mechanical parts, whose shape the agent-files define, so the tool knows them without
knowing the work.

### The per-rung flow

**Validate at every commit, doc-only ones included**, because step 2 changed the version, and
running the validation is how that is verified. **The dev build is installed before the review**
(wink, 2026-11-09) because a review that runs the previous rung's build tests code that is not the
diff: installed only after the commit, the reviewer's `vc-x1-dev` was one rung behind, 0.85.21-1
during the review of 0.85.21-2. The install after the commit stays, so the next rung runs a build
of the committed tree rather than of a working copy. **No validation while a review iterates** because a
formatter mutates files in ways that interact badly with the user's mid-review edits, so it runs
once, on the settled state, after the last edit.

**A failure is read before validation runs again** (wink, 2026-10-15) because a pass on the second
run says nothing of why the first failed, and a failure that comes and goes is a defect the retry
hides: a race, an order dependence, state one test leaves for another. A cause named first is
tested by the run that follows, and a run made first is taken as the answer. **A failure that does
not repeat is a finding** because its output is the only evidence of such a defect, and the next
run replaces it, so it is reported and given a Todo entry before it is lost. The case (2026-10-11):
a full validation failed once with no code changed, the retry passed and was reported as the
result, and the cause, found later, was a test that changed a process-wide environment variable
while another test read it.

**The work-review stop carries no description** because a description beside the work review
collapses two stops into one and describes work the review may still change.

**The `(done)` flip waits for "done" to be true** because before it the user may still reject or
reshape the work the marker would claim.

**Never `jj edit -r @-` to view a past commit**: it marks the commit mutable and shifts `@`.

**The version is the project's** (wink, 2026-11-11): `vc-x1` manages cycles and not the work, and
a version's file and form are the work's, so it never bumps one, now or later. It can still notice
a rung committed without its bump, by a file and a pattern the config names, which needs no
knowledge of the manifest. The flag that commits anyway is named, `--same-version`, and not
`--yes`, which is the user's approval of a forced push and would become a switch for any question.
The marker steps left the flow with the ladder becoming the tool's.

### Committing vs pushing

A local commit can be reworked and a push cannot easily be taken back, so the safe act is the one done
by doing nothing, and publishing has one place to gate.

#### A cycle is local by default

Local is the default because the Land is where the decision is, so one approval belongs there.
Nothing published can name a rung, so reshaping a ladder costs nothing, and a link names a change
id, which a reshape keeps. The first cycle run this way changed its ladder several times at no cost.

**The user decides when each side commits and publishes** (wink, 2026-11-03) because the user is
always in control: the per-rung pairs came first, the local cycle after, and neither is the only
shape, so the rules say what each shape needs rather than choosing one.

**A rung commits as a pair** (wink, 2026-11-09) because nothing now needs the agent-repo still.
While `ocid:` pinned the agent commit's SHA, a work commit could name it only once that SHA was
final, so one agent commit a cycle, made at the Land, was the obvious shape. With change ids that
holds no longer, and a pair per rung keeps each rung's transcript beside its work, one `ochid:`
away, and splits a cycle's transcript rung by rung rather than into one large commit.

**No trailer holds a SHA** (wink, 2026-11-09) because a SHA changes with every rewrite. `ocid:`
pinned the agent commit's, so an agent commit was never folded into and the order of commits was
forced, and `pcid:` repeated git's parents and made a describe rewrite every commit above it.
Neither repo could be left clean. `orepo:` with `ochid:` says where the partner is and survives a
squash-push. Lost: GitHub's one click to the agent commit, and the transcript pinned by a hash.

### Commit description

No version in title or body because a version is stable only once it lands, and a history rewrite
can renumber it. No file list because the diff is the mechanical record. No deliberation because the
cycle-record, todo, and the session the `ochid:` trailer names hold that, each reachable from the
commit by construction.

No top-level `-` and a pointer body for bookends because the earlier form was read wrong by its own
author twice in one day (2026-08-22): an opening's body restated the cycle's problem and then hung
two solutions at top level, where the rule said they answered the intro and the reader saw solutions
to nothing. A form that needs the rule open to be read correctly is wrong, so the pairing became
mechanical and the bookend, which resolves nothing, got a shape with nothing to pair.

### Pushing

_None recorded._

#### Policy

**Delegation waives stops, never flow**, because the stops are the synchronous half of review and
the flow (the records, the validation, the bookmark discipline) is what deferred review reads. A
delegated cycle that skipped a record would leave the deferred reviewer nothing to read.

#### Before any push

**Every push needs approval** because a push publishes, and what is published is corrected only by
a coordinated force-push of history someone else may already hold. Approval is the one moment the
user can still say no at no cost.

#### At rest

The agent-repo is a live journal, so everything after a publishing command, its own record included,
lands after it, and the agent cannot bring its own `@` to a fixed point. So the session's tail is
folded by the user after the session, the one moment the journal is still, by a squash-push into
the last agent commit, which keeps the change id the `ochid:`s name (wink, 2026-11-09). An archive
wants both repos clean. After a Land the agent published, the tail rides the next opening.

A forced push asks rather than refuses (wink, 2026-11-09). A topic bookmark has few readers or
none until it is published for merging, and even then a mistake found after the push wants an
amend, so a rewrite is the user's choice, made each time. Only the `--yes` flag answers in
advance, since a configured default is set once and then protects nothing. The agent-repo is
personal but public, observed rather than cloned, so its forced update moves no one's history.
`push` asks as `squash-push` does, one rule for a forced push, and `cycle` and `commit` refuse one,
since theirs are fast-forwards and a forced one is a surprise (wink, 2026-11-09). jj-lib forces
every push, with a lease, so the bookmark push under them all refuses a forced push its caller did
not ask about, and none is made silently.

Either order is correct (wink, 2026-11-09). The agent-repo went first while `ocid:` pinned the
agent commit's SHA, so the work commit naming it was pushed only once that SHA was final. With
`orepo:` and `ochid:` each side names the other by change id, which neither push changes, so one
`squash-push -s b` publishes both.

Work first is preferred (wink, 2026-11-09): the work-repo is the shared one, so its push is the one
a concurrent push refuses, and refused first it leaves nothing published. The agent-repo, usually
one user's, rarely refuses, and published first it would name a work commit no one else can see
while the work side is repaired.

### Close-out

Continuation notes go in the closing because notes written after the Land have no clean home: a commit
after it puts a non-cycle commit on the target after the hard stop, and a squash into the closing
rewrites the landmark just landed and verified. The cost, notes written before the Land they describe,
suits a close-out. The shape is in the closing so it is in the commit that was reviewed, rather than a
reshape at Land made after it.

Each close-out step exists because a close-out once went wrong without it.

- The finished block moves to `## Closed` so the finalized record exists in a tree
  ([Cycle-record](#cycle-record)).
- It asks what must outlive the cycle because the block is replaced one opening later, and the
  rung-time rule (a design finding goes to `notes/` by the rung that made it) relies on noticing in
  the moment.
- A closed block is never amended because its tree is the landmark's, and a rewrite there is a
  rewrite of published history.
- The user restarts the agent after the close-out (wink, 2026-08-24) because context degrades over
  a long session, nothing is in flight once the cycle has landed, and an agent cannot restart
  itself, so the step says what must be written down before the exit.
- The size row is recorded only when an agent-file changed (wink, 2026-09-16) because the table
  is the count's history, and a row repeating the last one records nothing.
- The `(current)` / `(done)` markers stay in the closed block (wink, 2026-08-28) because they have
  value: the finished block is the ladder as it was worked, and a closed block is one every rung of
  which reads `(done)`, so a rung reading otherwise is a finding. The earlier "drop the markers"
  step was one more transform of the record, of the kind [Cycle-record](#cycle-record) retired.
- The single-step case is spelled out because a single-step close-out went wrong when it was left
  to analogy (2026-08-26): the agent treated the one commit as a closing rung.
  - The agent-repo commit took the title plus " closing" while the work-repo commit kept the bare
    title, and the agent-repo held two commits for the one rung, so the `ochid:` trailers did not
    match.
  - The In Progress block was deleted rather than moved, so the cycle reached neither
    `notes/chores/` nor `## Done`.
  - The repair was a hand re-describe and squash.
- Pushed titles keep their names through a rename (2026-09-05) because the sync step said only
  "sync the title if the scope shifted", and a mid-cycle rename of the benches met three pushed
  rung titles and a bookend pair on the old name. Retitling a pushed commit is a re-describe of
  a published one, so the sync can reach the block's text and the unpushed rungs and nothing
  else, and saying so keeps the bookend grep intact.
- The continuation notes are filed or kept before the reset (2026-09-05) because two acquaints in
  a row found bullets whose facts had no other home, a host's clock episode among them, and the
  rule as written told the reader to destroy the only copy.

**The acceptance check runs at both ends of the closing** (wink, 2026-11-11): first, so the closing
commit has a result to record, and last, on the closed tree, which is the tree that lands when the
Land is a fast-forward. Every work rung then has its own test and no more, where the last work
rung carrying the check made one rung unlike the others.

**The way back from a fault is the user's choice** (wink, 2026-11-11): reopen, fix inside, fix on
top, or their own, since the test itself may be what is wrong. So the rule is to stop and ask, and
the tool's part is to refuse none of them.

**A Land fast-forwards by default and may merge** (wink, 2026-11-11): the fast-forward lands the
tree that was tested, and a merge at the Land makes a commit no rung validated, which the user
accepts by asking for it.

### Local ladders

Retired name: "Ladder (sub-cycle)", which collided with the working record's `#### Ladder`. That
ladder is the cycle's rung list, and a local ladder is one rung's scratch history. The fast
validation per ladder commit is non-negotiable because a regression in an early ladder commit
otherwise goes uncaught until a later commit runs the full suite, raising bisection cost. The
scratch `jj describe` is the one permitted describe because the commit is never published and never
carries a trailer.

## Working practices

**One command per shell invocation** because bundling hides which step produced which output.

**Never mask a command's exit status**: a pipeline's status is the last command's, so a validating
command piped into `tail` / `grep` reports the filter's success, not its own. A trailing
`; echo "exit=$?"` prints the status while the invocation itself still exits 0, so the failure is
visible only to whoever reads the text. `failed=$rc` stays unquoted because it has no spaces to
protect, and the quotes can stop a harness permission rule from matching a command it would
otherwise allow (wink, 2026-08-05). One form, bare and last, since the Bash tool reports a nonzero
exit itself, so a printed status is never needed, and one form is easier to keep than three
(wink, 2026-10-27).

**Hooks guard the exit-status rule** (wink, 2026-10-27): the rule held on paper and was broken
through a whole session by a trailing `; echo rc=${PIPESTATUS[0]}`, because nothing failed when it
slipped and the user caught it by reading. `bash-guard.sh` makes the slip fail, with two text
matches, each a deny whose reason is the rule's form. Joins are left to the rule, since the
agent's commands carry heredocs and quoted programs whose `;` only a shell parser tells apart, and
a guard that stops legitimate scripts gets removed. Matching hooks run in parallel and `deny` wins
over `bash-allow.sh`'s `allow` (https://code.claude.com/docs/en/hooks, PreToolUse decision
control). The hooks are in the set so they travel with the rules they serve.

**Long output keeps a file and shows a tail** (wink + agent, 2026-11-11): a validation printed
every compile line and 869 test lines, the Bash tool returned its first part, and the agent, not
seeing the line that said it passed, ran the whole validation again, a minute spent on a result the
exit status had already given. The named commands are the rule and "in doubt" only its fallback,
since a judgment is skipped exactly when the agent is confident and wrong, and the names a project
adds are in its `custom.md` because the set names no project. The file is what makes one run
enough: a tail alone shows the verdict and loses the failing test above it, so a failure would
still be run twice.

**Use https remotes, not ssh.** Unconditional rather than "when the agent is sandboxed" because the
remote is chosen at clone time and whether a sandboxed agent will ever touch the repo is not
knowable then. A sandbox denies ssh twice over: reads of `~/.ssh` are blocked except the signing key
and `known_hosts`, so no auth key is available, and we think a host allowlist cannot admit port 22
at all, since ssh carries no SNI or Host header to match on. The network leg is a spawned `git`
child that inherits the sandbox, which is why the same config succeeds from a human's terminal and
fails from a session. Both wrong theories (size, timeouts) were held, and eliminated by test, before
the rule was written. Changing a remote's URL needs the user's go because it moves where the repo
publishes.

**Delegate mechanical subtasks to lesser models** because top-model tokens are the scarce resource.
**Don't use the per-project memory directory** because easy for everyone to find beats convenient
for the agent alone. **Mark speculation** so a reader can tell the measured from the inferred.

### A session's rules are its own agent-files

Written after a session, working from this project, committed and pushed in a sibling shared repo
under that repo's own protocol alone (2026-08-30): it read the sibling's README as displacing
these files' review stops, and the scoping clause that would have caught it sat inside a file it
had already decided not to open, so the scope had to move into the rules themselves. Stating rule
identity plus explicit delegation removes the conflict between "the sibling's README governs" and
"these files govern" rather than adjudicating it: each governs its own jurisdiction, the repo's
protocol the write's shape, the session's files the conduct around it. The dual-repo thought
experiment settled the boundary (wink): were the target a full dual-repo project, the answer is
to message its agent rather than write its repo, so a foreign repo's rules never bind by default.

### Stop and ask

A clarifying question costs seconds, while redoing misaligned work costs much more.

## File map

_None recorded._

## Changing the agent-files

An adopter's diff against the copy it started from is what a proposal is made of and what its
review reads, so drift is a diff, not a mystery: the review's input needs no maintenance and cannot
go stale. The adopter's own changes live in that copy too, each a cycle like any other (wink,
2026-10-18), so a change every clone needs is committed where the rule lives and nothing marks it as
the project's.

**A new repo starts from an adopter's set** (wink, 2026-11-03) because an adopter carries the set
already, so a template repository is a second copy to keep in step. **Its working copy's by
default** (wink, 2026-11-04) because the rules being worked on are the ones a test repo is made to
try, the obvious source, and `--landed` takes `main`'s when those are wanted.

**An experiment comes before a proposal** because a rule is argued best by having been run, and
the diff against the copy it started from keeps the trial visible while it runs.

**A proposal is one set of diffs, to the rules or beyond them** (wink, 2026-10-18) because a set
of files and a diff are what two repos can compare without talking, and because a proposal is not
limited to the set: what one adopter asks of another may be anything.

**Acceptance is negotiated, never owed** (wink, 2026-10-18) because the adopters are peers who
chose the set, and there is no maintainer above them. A no that carries its why, or the change that
would turn it, teaches the proposer something either way, so the goal is everyone accepting the
same thing and the requirement is only that each says where it stands. An adopter that keeps its
own way is a diff the review already tolerates, one that leaves takes its copy with it, and a rule
that said otherwise would be the one absolute in a set whose rules all bend
([Rules](../AGENTS.md#rules)).

**No agent-file carries a banner saying it is shared** (wink, 2026-10-18) because `AGENTS.md` is
read first, and this section covers every agent-file, so a banner atop each `agent-data/` file
restated it seven times, and the two files without one never missed it.

Until v0.3.0 this section also made a correction a direct edit of the payload, an agent-file change
its own commit, convention work its own cycle, and a rule adopted ahead a resident of its agent-file
rather than a holding file (measured 2026-08-19 to 2026-08-21, chores-17's [docs: empty
custom-family into the pinned set and
config](https://github.com/winksaville/vc-x1/blob/2dc8d969c3f3/notes/chores/chores-17.md#docs-empty-custom-family-into-the-pinned-set-and-config)).
The cycle **agent-files(proposal): v0.3.0** retired them, the proposal bullet covering what they
guarded (wink, 2026-10-18).

## custom.md

**custom.md is local and optional** (wink, 2026-10-18) because a committed one hands every clone of
a public repo one machine's layer, its sibling paths and its standing permissions, and whoever
clones it shares neither. So it is untracked, and the agent reads it when there is one.

**It is read first and overrides** because it is the workspace's own word, and **it holds what is
local to the workspace** because a change every clone needs is a cycle on the agent-file it belongs
in, so what is left is one machine's: its paths, its permissions, a lock switch. The session lock is
named there as an experiment, not an example (wink, 2026-10-18), since the intention was real, and
because nothing else in the set linked `session-lock.md` once `custom.md` stopped pointing at it.

**The rule is stated once**, in `AGENTS.md`, because a line in each `agent-data/` file's
boilerplate restated it and added nothing.

Until v0.3.0 `custom.md` was committed, the project's layer: it shipped holding its own shape, took
overrides and pointers, and kept the other agent-files identical to the payload's so a re-sync was
a copy. The cycle **agent-files(proposal): v0.3.0** retired that.

## code.md

### `// OK: ...` comments on `unwrap*` calls (Rust)

**The user is told of every `unwrap*` outside tests** so the site gets reviewed while it is one
line in the conversation rather than one of many in a diff, and so the acceptable uses are learned
case by case instead of the call riding in silently.

The `#[allow]` obligation follows the lints (2026-09-05) because the rule assumed every Rust
project enables them and one did not, so the obligation there was either meaningless or a demand
to enable lints as a side effect of adding a bench. The comment and the alert are what the
convention is for, and they hold without the lints.

## jj.md

### jj Basics

**jj, not git** because the workflow, bookmarks, the working-copy `@` model, and the ochid
trailers, is expressed in jj terms, and the git backend is only its storage. Driving the repo with
raw `git` invites state the jj documentation here does not describe.

### Revsets

**jj's own semantics are the one dialect** because an earlier house convention glossed `x..` as
"descendants of x excluding x", which is not jj's meaning, and durable text written under a second
dialect needs a decoder. Old transcripts and notes written under that gloss decode against it, and
text written since teaches only jj's.

### Cross-repo linking (ochid trailers)

An `ochid:` always means the other repo, so a label or URL in it told a tool nothing, and a change id
survives a rewrite where a SHA goes stale. The repo is said once, by `orepo:` above the `ochid:`s
it covers, so an agent commit naming partners in several repos can group them, and a reader on
either side knows where to look.

**Trailers are stamped by push, never hand-written**, because the change id a trailer names does
not exist until push makes the paired commit: an agent-repo ochid names `@-` after the push, and an
unpushed `@` gets a fresh chid in every clone ([Resolvability](jj.md#resolvability)). A trailer
written by hand is at best a guess at that id, and a wrong one breaks the only join between the two
histories.

### Re-describing: coordinate first, and keep the trailer

**A re-describe is coordinated** because it is a history rewrite that silently drops the cross-repo
link: the `ochid:` trailer lives in the description, and a new description without it leaves the
paired commit pointing at nothing. Hit at a coordinated amend (2026-07-29), where the trailer
survived only by being copied across by hand.

### Cycle bookmarks: create and land

**The rule and the commands are separated** because the commands are the part expected to change:
we think a `vc-x1 start-change <bookmark>` will eventually own the create half, replacing the
create bullets and nothing else, while the rule in AGENTS.md stays as it is.

## notes.md

### File reads: read the slice you need

**The routine read should stay small.** `TODO.md` grows every cycle, so the backlog and bugs live in
files under `notes/` rather than inline. The same "slice you need" rule applies to historical files.

## prose.md

### Prose form

The rule widened from typeable punctuation to the whole of prose form (2026-08-27).

- The prose rules arrived one at a time, each as its own hard rule or its own section, and a rule
  named for one of them was read as covering only that one.
- One spelling per term (wink, 2026-08-24): either always hyphenated or never, since mixing costs
  a reader a judgment call and buys nothing.
- Bullet form (wink, 2026-08-27): sentence bullets and thing-list bullets read differently, and a
  list that mixes them makes the reader decide which each one is.

### Semicolons

A touched file's conversion is paid in a penultimate rung (2026-09-05) rather than in the rung that
touched it. Paid in the touching rung, every hunk of that rung's diff had to be read to tell
repunctuation from a real change (wink, 2026-09-03), and a rename rung that touched two docs holding
62 prose semicolons would have buried a rename under a rewrite. The obligation is unchanged, what is
owed still follows from touching the file, and only the payment moves, so the wording keeps
"whether" and "when" apart. It names its exceptions, new prose written correct and a rewrite-sized
count becoming its own cycle, rather than hedging, since a hedge tells a reader a deviation is
allowed without telling them when. Dashes ride the same rung, since one rung had swept seven em
dashes into a content change beside six semicolons.

### Typeable punctuation only

Paid in the same penultimate rung as the semicolons, for the reason given there.

### Conventional-commit shape (ladder / commit)

**One title per step** because the title is the identifier: a record points at a step by its
title, a plain greppable mention, so the ladder rung, the details heading, and the commit title
must be verbatim identical for one mention to find all three, and no other identifier is kept
([Steps are named, not numbered](prose.md#steps-are-named-not-numbered)).

### Commit titles and descriptions

Split from [Conventional-commit shape](#conventional-commit-shape-ladder--commit) (wink,
2026-08-31): the shape section owns the structural identity, and its trailing vocabulary paragraph
gave the types and scopes no anchors to cite, so the vocabulary became its own section with a
subsection per slot.

- **Project-declared types** because a recurring kind of work deserves one searchable token, and a
  declaration naming the type, its scope vocabulary, and its description grammar is what lets a
  reader parse the formulaic titles and a title check admit them by name.
- **The declaration owns its scope slot**: the deviation from the typical conventional commit,
  where a scope names a component, is real, and stating that the declaration's vocabulary wins
  confines the deviation to declared types.

### Versions live in the version-of-record only

**The version is a build stamp**, answering "which commit produced this artifact", not a name for a
step. Written into prose it becomes a second identifier that history is free to invalidate: one
renumber of published versions turns every prose mention, transcript and pasted report into residue
that needs a decoder to read. A renumber cannot touch a title.

**The `agent-files` title is the exception** (wink, 2026-09-01) because the agent-files version is
not a build stamp: it names an agreed text, the adopters converging on that number is the point,
and a renumber is the failure the version exists to prevent, not a thing history is free to do. The
title needed a unique token, the member-list-and-date grammar ran past the title cap (59 and 61
characters against 50, reported by an adopter) and repeated the message record's fields, and the
version is unique by the discipline the artifact version already keeps, one bump per cycle.

### Agent-files name no project

**Agent-file text is copied to every adopter**, so a project narration reads as the reader's own
history ("This project adopted the convention on <date>" arrives in repos that adopted it on a
different date or never). And the citation goes stale the moment the named project retires its
records, while the rule outlives it.

### Speculation marker

**Unmarked speculation reads like evidence**, and a future reader (or the agent on a later session)
can pick it up as a known fact when it is not. Measured versus inferred is a distinction worth
keeping visible in the written record.

### Plain synopsis after technical explanations

**The technical form is precise but easy to misread**, and the plain form catches misunderstandings
early, when they are cheap.

## versioning.md

### Advancing X.Y.Z: patch by default

**The suffix already encodes a commit's phase, so `X.Y.Z` only has to mark the milestones a reader
should notice.** Two classification tests preceded the rule and failed the same way, by demanding a
judgment call at every cycle: functional-versus-docs, replaced because volume is not scope, and a
shape-versus-contents scope test, dropped (2026-08-23) after "shape" proved undecidable in practice,
a two-file docs edit arguable into "a pipeline reshaped" within a day of the test being cited. A
pre-1.0 line wants the patch bias: `0.Y.Z` makes no stability promise, so a fast-moving minor burns
the number's signal for nothing, while a minor that moves rarely and deliberately still says
something.

### Grammar and storage

**One dash, dots only**, because a Python linter/formatter in a sibling repo rejected every
multi-dash version outright, and the PEP 440 reference parser (`packaging` 26.2, tested
2026-07-30) confirms the boundary: `1.2.3-3.1`, `1.2.3-x+ab+cd`, `1.2.3-34-abc`, and any second
`+` are all invalid. `1.2.3+3.1` / `1.2.3+3.1.hotfix` parse at any depth. `1.2.3-1` parses but
silently becomes the post-release `1.2.3.post1`. The one-dash dotted grammar is the largest form
every medium accepts.

### Tools the session runs

**A dev build newer than the plain one is asked about** (wink, 2026-11-09) because the agent-files
can be ahead of the installed tool. A session under v0.3.1-3 ran the plain `vc-x1` 0.85.20, whose
banner named v0.3.1-3 while its commit stamped the `ocid:` and `pcid:` trailers the set had
retired, and only `vc-x1-dev` 0.85.21-2 stamped the `orepo:` form the set describes. The plain
build's banner names the set it embeds, not the behavior the set expects, so the versions are
compared and the choice is the user's.

### Agent-files version

**An empty file named by the version** (wink, 2026-09-01) because a listing then shows the number
without a read, a bump is a rename git records as one, and the pattern's add-history is the version
log. A one-line file needs a read and a diff for the same answers. A version in AGENTS.md's H1, or
a `## Version` section at its top, was rejected because a heading is an anchor and every bump
would break the links to it. The file is `agent-files-vX.Y.Z` rather than `version-vX.Y.Z` so the
name says what is versioned and does not stutter.

**The `v` spelling** (wink, 2026-09-01) because a bare number after a title's colon could be
anything, and the `v` says "version" in one character, the marker git tags made familiar. Cargo's
output set the rule that keeps it one spelling: bare for a program's own banner (`cargo 1.80.0`),
`v` for a name paired with its version in a line (`Compiling bar v1.0.0`), and the agent-files
version is always the pair.

**The suffix walks in the file name and never in a title** because a rung's title is its ladder
line, which carries no version ([Steps are named, not
numbered](prose.md#steps-are-named-not-numbered)), so a proposal cycle's rungs are told apart by
the bookends while the file says at every commit whether the set is mid-cycle, which is what a copy
taken from a draft bookmark needs to say on its face.

**Proposals bump and adoptions copy** because a proposal is the diff, and the bump is part of it,
while an adoption takes a text that already has its number: the same title in two repos then reads
as the copy and its origin.

**An adoption is a copy and ends on a diff** (wink, 2026-10-13) because the diff is the proof that
the copy is whole, and it fails on a file an adopter edited after copying. Until v0.3.0 the copy
took the adopters' shared project layer too, under `-c`, and it stopped when that layer became local
([custom.md](#custommd)). Since v0.3.0 the diff may be non-empty and still pass the adoption,
printing the differences, because acceptance may differ from the proposal ([Changing the
agent-files](../AGENTS.md#changing-the-agent-files)).

**A version names the text as proposed** (wink, 2026-10-18) because a number that named what each
adopter agreed to would name a different text in each repo once acceptance may differ from the
proposal. So the number says what was offered, the adopter's diff says what it kept, and the
version is read from a listing while the differences are read from the diff.

**Patch by default, with digit limits** (wink, 2026-09-07) because the earlier rule, patch for a
correction and minor for a rule change, would have moved the minor at nearly every proposal: a set
change is a rule change more often than not, and each is incompatible in the SemVer sense, since an
adopter on the old text and one on the new behave differently until the re-sync. A digit that
moves every time signals nothing, so the agent-files version counts rather than classifies, and the
one-digit patch and two-digit minor are what keep a counter readable at a glance in a file name
and a title.
