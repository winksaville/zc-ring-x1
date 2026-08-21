# Rationale

The why behind the agent-files, one entry per rule that has one. AGENTS.md holds the
rule and its boundaries, and a session needs only that. The argument is for whoever would change
a rule, and for the family at convergence, and it is kept so a rule is not simplified away by an
editor who does not know its cost.

Universal file, shared with the template repository. A proposed change is edited here and
converges at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)).
Project-local content goes in [custom.md](../custom.md).

## How to read this file

- **Headings mirror AGENTS.md's**, same text, same level, so the anchors line up 1:1 and a rule
  reaches its why by one fixed pattern, `[why](agent-data/rationale.md#<same-slug>)`. A heading
  with nothing under it but `_None recorded._` is a rule whose why was never written down, which
  is a finding, not a gap to fill with a guess.
- **Per-file sections sit at the end**, one `##` per agent-file, subheadings mirroring that
  file's, reached by the same pattern (`[why](rationale.md#<same-slug>)` from inside
  `agent-data/`). The other agent-files' remaining inline **Why:** paragraphs migrate as they
  are touched, the full sweep filed as its own cycle.
- **An entry is the why, then the evidence**: back references to the chores section where the
  rule was paid for, the messages-repo record, the commit. Mostly pointers,
  not a re-telling. The "measured YYYY-MM-DD" lines live here, with the story.
- **A boundary sentence is not rationale.** A sentence saying what a rule does not cover is the
  rule, and stays in AGENTS.md. What moves here is argument: why the rule exists, what it cost
  to learn, what the alternatives were.
- **Speculation is marked** as everywhere else ("We think ...", prose.md's
  [Speculation marker](prose.md#speculation-marker)), so a reader can tell the measured from
  the inferred.

## Rules

### Hard rules

The rules are named so a review can reference one by URL, and each has a heading below
mirroring AGENTS.md's.

### Read custom.md first

_None recorded._

### jj, not git

_None recorded._

### Push commits

_None recorded._

### Approval per push

_None recorded._

### Hard stop after the final push

_None recorded._

### No re-describe without coordinating

_None recorded._

### No hand-written trailers

_None recorded._

### Read the step before the action

_None recorded._

### Typeable punctuation

_None recorded._

### One title per step

_None recorded._

### Stop and ask

A clarifying question costs seconds, while redoing misaligned work costs much more.

### Alert on unwrap

_None recorded._

### Changing agent-files

_None recorded._

### Bookmark per cycle

_None recorded._


## Terminology

**Retired names.** "Bot repo" (2026-08-21), when the code respelled the side `agent`.
"Instruction files", which named the agent-files back when `custom.md` was the only editable
one. "Ladder (sub-cycle)" for a local ladder, under [Local ladders](#local-ladders).

## The dual-repo model

_None recorded._

## Cycle protocol

The record has one home at a time so it is never written twice. The alternative keeps a
working ladder in `TODO.md` and an as-built ladder in chores, so every rung is written twice
and every backfill applied twice, and detail written twice drifts (the same argument that
keeps the edit list out of the commit body, notes.md's
[Chores section content](notes.md#chores-section-content-no-edit-list-git-is-the-record)).

### Cycles run on a bookmark

A cycle that pushes `main` directly makes every correction a coordinated force-push of
published history. Landing costs one command and buys free rewrites for the whole cycle. A
single-step cycle gets a bookmark for the same reason: a one-commit line is exactly where a
pre-landing rewrite is cheapest. "Development is not done on `main`" is stated outright because
the trapezoid recipe once allowed "a docs interlude between cycles" on the trunk line, and five
docs commits went to `main` that way before a sixth was caught mid-draft and run as a cycle
(2026-08-22). The owners will sometimes cheat, and the rule is still the rule.

Pushing to the bookmark makes the work durable and visible, but landing on `main` is
publication, and that is the line the rules divide at. The series is kept self-consistent
before landing so the branch reads as one coherent ladder. Amending content rather than
re-describing keeps hard rule 4 intact and lets the `ochid:` trailers ride along: they carry
change ids, which survive a rewrite.

### Opening

**Backfill first.** The 0.80.0 and 0.80.1 as-built rungs were both found unfilled at the
0.80.2 opening (measured 2026-08-21): backfill was named only at close-out, as "the edits ride
the next push", which names no owner, and the Opening's steps never mentioned it, so the only
place the rule lived was the one moment hard rule 3 forbids acting on it. The previous cycle's
rungs are the usual hits of the check. Not folded into the Done sweep, which is already a
compound step, and a step with two halves is where the second half hides. Recorded in the
"docs: halve AGENTS.md into rationale.md" chores section.

**The bookmark create is a push** because `vc-x1 push` requires the bookmark's remote refs to
be tracked, so the create has to publish, and a publish takes push approval.

**The solution statement is provisional** because it is written before the work.

**Why the acceptance check, and why it is provisional.** A cycle's per-commit checklists can
all pass while its banner claim is false: a seven-cycle program opened against "end subprocess
spawning" and its close-out claimed the goal met, with about twenty spawn sites surviving, two
inside the facade the program built (found 2026-08-06 at the 0.78.3 review, and retired by
the 0.79.0 cycle, chores-17's
[refactor: retire the remaining jj spawns](../notes/chores/chores-17.md#refactor-retire-the-remaining-jj-spawns)).
Being provisional, the check can also be revised
*toward* what was achieved, which is the same failure by a slower route, so a changed check is
one of the things the deliberation exists to justify.

### The per-rung flow

**Validate at every commit, doc-only ones included**, because step 4 changed the version, and
running the validation is how that is verified. **No validation while a review iterates**
because a formatter mutates files in ways that interact badly with the user's mid-review edits,
so it runs once, on the settled state, after the last edit.

**The work-review stop carries no description** because a description beside the work review
collapses two stops into one and describes work the review may still change.

**The `(done)` flip waits for "done" to be true** because before it the user may still reject
or reshape the work the marker would claim.

**Never `jj edit -r @-` to view a past commit**: it marks the commit mutable and shifts `@`.

### Committing vs pushing

Push's commit stages commit both repos and stamp each new commit's `ochid:` trailer, so a
pre-committed rung leaves `@` empty and push mints a stamped empty duplicate (the empty-`@`
push minting orphan agent-repo commits was measured 2026-08-15, in the "docs: trial the
iiac-perf convergence proposals" chores section). **No checks of the project's own** because
vc-x1 assumes nothing about a repo beyond `.jj` and its config.

### Commit description

No version in title or body because a version is stable only once it lands, and a history
rewrite can renumber it. No file list because the diff is the mechanical record. No
deliberation because chores, todo, and the session the `ochid:` trailer names hold that, each
reachable from the commit by construction.

No top-level `-` and a pointer body for bookends because the earlier form was read wrong by
its own author twice in one day (2026-08-22): an opening's body restated the cycle's problem
and then hung two solutions at top level, where the rule said they answered the intro and the
reader saw solutions to nothing. A form that needs the rule open to be read correctly is
wrong, so the pairing became mechanical and the bookend, which resolves nothing, got a shape
with nothing to pair.

### Pushing

_None recorded._

#### Policy

**Delegation waives stops, never flow**, because the stops are the synchronous half of review
and the flow (the records, the validation, the bookmark discipline) is what deferred review
reads. A delegated cycle that skipped a record would leave the deferred reviewer nothing to
read.

#### Before any push

_None recorded._

#### At rest: push, stop, squash-push

The agent-repo (`.claude`) is a live journal, so everything after a `vc-x1 push` invocation, its
own record and any closing words, lands in the agent-repo's `@` as a trailing tail. That tail is
why the agent cannot squash-push the agent-repo: the squash-push is itself an action that adds
to the tail, so it never reaches a fixed point. Thus the user must do the squash-push in the
agent-repo anytime the agent acts visibly or behind the scenes. Importantly, a squash-push does
not alter the change id in the agent-repo commit, so the `ochid` in the work-repo commit
continues to resolve.

### Close-out

_None recorded._

### Chores sections

Anchors survive the heading-level shift because GitHub slugs derive from the heading's text,
not its level. The renumbered refs and the rebased links are checked by hand because both fail
silently: a mis-renumbered ref and an un-rebased link render as plain text or a 404 rather than
erroring.

#### Commits backfill

An as-built rung cites its commit by SHA and records the version that commit carried, and
neither is stable until the commit lands on a permanent branch: a rebase or squash rewrites
the SHA on the way, and a history rewrite can renumber the version. A commit cannot record its
own SHA, which is why the fill is always one push later.

### Local ladders

Retired name: "Ladder (sub-cycle)", which collided with the working record's `#### Ladder`.
That ladder is the cycle's rung list, and a local ladder is one rung's scratch history. The
fast validation per ladder commit is non-negotiable because a regression in an early ladder
commit otherwise goes uncaught until a later commit runs the full suite, raising bisection
cost. The scratch `jj describe` is the one permitted describe because the commit is never
published and never carries a trailer.

## Working practices

**One command per shell invocation** because bundling hides which step produced which output.

**Never mask a command's exit status**: a pipeline's status is the last command's, so a
validating command piped into `tail` / `grep` reports the filter's success, not its own. A
trailing `; echo "exit=$?"` prints the status while the invocation itself still exits 0, so
the failure is visible only to whoever reads the text. `failed=$rc` stays unquoted because it
has no spaces to protect, and the quotes can stop a harness permission rule from matching a
command it would otherwise allow (wink, 2026-08-05).

**Use https remotes, not ssh.** Unconditional rather than "when the agent is sandboxed"
because the remote is chosen at clone time and whether a sandboxed agent will ever touch the
repo is not knowable then. A sandbox denies ssh twice over: reads of `~/.ssh` are blocked
except the signing key and `known_hosts`, so no auth key is available, and we think a host
allowlist cannot admit port 22 at all, since ssh carries no SNI or Host header to match on.
The network leg is a spawned `git` child that inherits the sandbox, which is why the same
config succeeds from a human's terminal and fails from a session. Both wrong theories (size,
timeouts) were held, and eliminated by test, before the rule was written. Changing a remote's
URL needs the user's go because it moves where the repo publishes.

**Delegate mechanical subtasks to lesser models** because top-model tokens are the scarce
resource. **Don't use the per-project memory directory** because easy for everyone to find
beats convenient for the agent alone. **Mark speculation** so a reader can tell the measured
from the inferred.

## File map

_None recorded._

## Changing the agent-files

A member's diff against the template repository's payload is what that member has proposed,
so drift is a diff, not a mystery: the proposal set needs no maintenance and cannot go stale.

**A correction goes straight into the payload** because a wrong fact has no second opinion to
gather, and leaving it in place misleads every member on first read.

**An agent-file change is its own commit** so `git log -- AGENTS.md agent-data/` reads as a
list of rule changes rather than unrelated feature titles, and the commit's `ochid:` trailer
links the agent-repo session that reasoned it out. The diff says what differs now. The history
says when, by whom, and why.

**Convention work runs as its own cycle** because rung by rung, rule changes bury a feature
cycle's records under work its title never promised.

**A rule adopted ahead lives in the pinned file, never a holding section**, because a member
that collects adopted-ahead rules in `custom.md` hides them from the one review that decides
them, and a session that skips the project layer misses binding behavior. Both measured,
2026-08-19 to 2026-08-21, when one member's project layer (`custom-family.md`) held the
family's messaging rules and the validation commands, retired by the 0.80.0 cycle, chores-17's
[docs: empty custom-family into the pinned set and config](../notes/chores/chores-17.md#docs-empty-custom-family-into-the-pinned-set-and-config).

## custom.md: the project layer

It ships holding nothing but its own shape so a project that changes nothing still has a valid
one. A project-kept rule goes to the pinned file rather than here because writing it here hides
it from exactly the review that should decide it. A pointer entry owes no justification because
it supersedes nothing, and holding a wider context behind one pointer keeps the rest of the
file identical to the payload's.

## versioning.md

### Advancing X.Y.Z: patch by default

**The suffix already encodes a commit's phase, so `X.Y.Z` only has to mark the milestones a
reader should notice.** Two classification tests preceded the rule and failed the same way, by
demanding a judgment call at every cycle: functional-versus-docs, replaced because volume is
not scope, and a shape-versus-contents scope test, dropped (2026-08-23) after "shape" proved
undecidable in practice, a two-file docs edit arguable into "a pipeline reshaped" within a day
of the test being cited. A pre-1.0 line wants the patch bias: `0.Y.Z` makes no stability
promise, so a fast-moving minor burns the number's signal for nothing, while a minor that moves
rarely and deliberately still says something.
