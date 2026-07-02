# Cycle protocol

This protocol uses [Prose form](../AGENTS.md#prose-form). It
contains instructions on how a commit cycle is accomplished.

The artifact a cycle produces is whatever the bot generates from
the conversation — code, prose, an image, a song, a screenplay.
The steps below use a Rust crate as the running example (the
cargo cycle, `Cargo.toml` versioning); substitute your medium's
equivalents — this project's manifest is recorded in
[versioning.md](versioning.md).

## Cycles

A cycle has three phases:

- **[Preparation](#preparation)** (`X.Y.Z-0`) — the cycle's
  first commit, when it needs setup (a lightweight cycle omits
  it and starts at `-1` — see
  [versioning.md](versioning.md#step-numbering)). Sets up the
  cycle:
  - Bump the version-of-record to `X.Y.Z-0` (where it lives
    and the suffix scheme are project-specific — see
    [versioning.md](versioning.md)).
  - Pick up a `## Todo` item (typically the top-ranked,
    #1) into `## In Progress` (bold title + succinct problem
    statement + plan ladder).
  - Open the [chores section](#chores-sections).
- **[Work-N](#work-n)** (`X.Y.Z-1`, `X.Y.Z-2`, …) — the
  commits that implement the change. As many as the change
  needs; each runs through the
  [per-commit flow](#per-commit-flow).
- **[Close-out](#close-out)** (bare `X.Y.Z`) — the cycle's
  last commit. Bookkeeping only:
  - Move the picked-up item to a one-line entry in
    `## Done`.
  - Move the `## In Progress` block into the
    [chores section](#chores-sections).
  - Optionally update `notes/README.md` if functionality
    changed.

A cycle's commits are published to the project remote
either incrementally or as one batch at close-out; the
result must always be published at close-out. See
[Pushing](#pushing).

**Sub-cycles.** When a Work commit's scope grows enough to
warrant its own ladder, it subdivides — `X.Y.Z-3.0`
Preparation, `X.Y.Z-3.1` / `X.Y.Z-3.2` Work, `X.Y.Z-3`
Close-out. The same three-phase shape applies recursively
at every depth. See [Numbering](#numbering) for the
suffix rule and [Sub-cycle ladders](#sub-cycle-ladders)
for the local-ladder mechanics.

## Chores sections

A **chores section** is a `##` section in
`notes/chores/chores-NN.md` recording landed work. In
general, every commit that lands on the permanent branch
should have a reference to it on a `Commits:` list in a
chores file.

The phrase **"Open" the chores section** means append a
`##` header to the current `notes/chores/chores-NN.md`
with the title it records (e.g. `## refactor: foo bar`),
followed by an **empty `Commits:`** line. The narrative
body stays empty until close-out (see [Close-out](#close-out));
the `Commits:` line is backfilled later, once the commit is
permanent (see [Commits backfill](#commits-backfill) below).

Fuller chores conventions (content rules, header sync,
design subsection pattern, `Commits:` formatting) live in
AGENTS.md [Chores conventions](../AGENTS.md#chores-conventions).

### Commits backfill

A chores section's `Commits:` line cites the commit(s) it
records, by SHA — but a SHA isn't stable until the commit
lands on a **permanent branch** (`main`, or a long-lived
release/patch branch that won't be rewritten); a rebase or
squash rewrites it on the way. So:

- A chores section is **opened with an empty `Commits:`** line.
- **Backfill once the commit is on a permanent branch**,
  where its SHA is final. A commit can't record its own SHA
  (that would change the hash), so the fill always lands one
  push later: **each push backfills the `Commits:` of the
  commits the previous push made permanent.** On a topic
  branch the sections instead wait until the branch lands —
  so no SHA is ever written that a later rebase could
  invalidate.

Use `[[N]]` refs — several as `[[N]],[[M]]` only when one
section records multiple commits (a merge non-ff close-out) —
with the commit URL + 40-hex SHA in the file's `# References`
(format in AGENTS.md
[Chores commit references](../AGENTS.md#chores-commit-references)).
A section's `##` title matches its commit title, so a rare
deliberate rewrite of a permanent-branch commit re-syncs via
`git log --grep "<title>"`.

The per-push cadence is a project choice, not dogma — a
**per-close-out** model (recording a cycle's SHAs at its
close-out) is equally valid. The one invariant: a recorded
SHA must be permanent.

## Preparation

The cycle's first commit (`X.Y.Z-0`), when the cycle needs
setup (a lightweight cycle omits it — see
[versioning.md](versioning.md#step-numbering)):

- **Bump the version-of-record** to `X.Y.Z-0`. Where it
  lives, the suffix scheme, and any derived files (a
  lockfile, a sourced manifest version) are
  project-specific — see [versioning.md](versioning.md).
- **Move a `## Todo` item** (if the cycle has one) into
  `## In Progress` and the todo item should have:
  - A **bold title line** — that will be the chores
    section header, minus the `## ` prefix.
  - A **succinct problem statement**; add if one is needed
  - A **plan ladder**.
- **Open the [chores section](#chores-sections)** —
  append a `##` header with the title it records — the
  cycle's anticipated close-out title.

## Work-N

The cycle's work commits (`X.Y.Z-1`, `X.Y.Z-2`, …)
implement the change. As many as needed:

- Each commit runs through the
  **[per-commit flow](#per-commit-flow)**.
- **Interim pushes** are optional (backup, progress
  visibility).
- Close-out is the only mandatory push (see
  [Pushing](#pushing)).
- **Subdivide into a sub-cycle** if a Work commit's
  scope grows enough (see
  [Sub-cycle ladders](#sub-cycle-ladders)).

## Close-out

The cycle's last commit (bare `X.Y.Z`) does bookkeeping
only — the commit body describes that bookkeeping, not
what happens post-squash:

- **Move the picked-up item** from `## In Progress` to a
  one-line entry (with a chores `[N]` ref) in `## Done`.
- **Move the `## In Progress` block into the
  [chores section](#chores-sections)** (the title-only
  placeholder opened during Preparation):
  - Cut the problem statement + plan ladder out of
    `## In Progress` (drop the bold title line — the
    chores `##` header already carries that string).
  - Paste under the chores `##` header.
  - Sync the chores header to the **final** commit title
    if the cycle's scope shifted; update every anchor
    back-reference.
  - Add an `### As-built ladder` listing the cycle's
    commits; optional `### Outcome` notes.
  - Replace `## In Progress` with
    `_No cycle currently in progress._`.
- **Update `notes/README.md`** if functionality changed
  (new flags, new subcommands, changed behavior).

Whether to **squash** the cycle into one commit before the
publishing push, or push as-is, is decided at push time —
see [Pushing](#pushing).

## Numbering

Each commit's phase is encoded in the version suffix — `-0`
Preparation, `-1`/`-2`/… Work, bare `X.Y.Z` Close-out,
recursively for sub-cycles. The full scheme — disambiguation,
nesting, optional Preparation, the project's version-of-record
format, and the per-phase bump — lives in
[versioning.md](versioning.md#step-numbering), which is the
single source of truth for this repo's versioning.

## Per-commit flow

Every commit (Preparation, each Work commit, Close-out) goes
through:

1. **Mark this commit `(current)`** as the first edit in
   `notes/todo.md > ## In Progress`.
2. **Do the work** (see [Iterative work](#iterative-work)
   for the loop-and-squash technique).
3. **Flip this commit `(current)` → `(done)`** in `## In
   Progress` — before the cargo cycle and the commit.
4. **Validate the artifact** — a medium-specific step, skip-able
   for notes-only commits, mandatory at close-out. For the Rust
   example the cargo cycle is:
   1. `cargo fmt`
   2. `cargo clippy --all-targets -- -D warnings`
   3. `cargo test`
   4. `cargo install --path . --locked`
   5. (re-test if anything substantive changed)
5. **Work review.** Stop *before* writing any description;
   tell the user "ready to commit." The user reviews the
   changes and we iterate until complete.
6. **Write the commit description** — see
   [Commit description](#commit-description).
7. **Commit Description review.** Show the title + body
   and stop. The user reviews the description. Iterate.
8. **Commit.** `jj commit -m "title" -m "body" -R .` for
   the app repo, `-R .claude` for the bot repo (`-R` last
   keeps the verb visible):

   ```
   jj commit -m \
   "<type>: <short description>" \
   -m "<intro paragraph>

   - file1: gist
   - file2: gist

   ochid: /.claude/<chid>" \
   -R .
   ```

**Two overrides apply:**

- **Deviation or question** — any time the work deviates
  from the agreed plan, or a question arises, stop and
  surface it; don't push through.
- **ESC-ESC** — the user can interrupt at any point to pull
  a review or question forward.

## Commit description

[Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: <short description>
```

Titles carry **no trailing `(<version>)` suffix**. The
version-of-record — where it lives and its bump cadence, see
[versioning.md](versioning.md) — is useful for confirming
you're running the version you're testing, not a per-commit
title marker. We think per-commit version-in-title is
unstable on large projects: many changes are in flight
simultaneously, and a
version is only stable once merged into the main repo — so
titles don't carry one.

### Title

- ≤50 chars total.
- Common types: `feat`, `fix`, `refactor`, `test`,
  `docs`, `chore`.
- Favor terse phrasings.
- **Distinct per step** — each of a cycle's commits gets its
  own descriptive title (no shared cycle title with a step
  marker). Share a greppable stem across the cycle's titles
  (e.g. `ring buffer`) so `git log --grep` collects them; the
  chores section header matches the close-out title. See
  AGENTS.md
  [Conventional-commit shape](../AGENTS.md#conventional-commit-shape-ladder--chores--commit).

### Body

[Prose form](../AGENTS.md#prose-form) (intro + bullets),
wrap ≤72. Bullet content differs per repo:

- **App-repo body**: file-by-file. One bullet per file
  changed (file plus a one-line gist). Sub-bullets for
  files with multiple distinct changes:

  ```
  - path/to/file1
    - first distinct change
    - second distinct change, wrapping to next line
       with continuation indented 5
  ```

  The file-by-file list is the source of truth for the
  cycle's mechanical change record. Chores carries the
  narrative + design, not a copy of it. Promote any
  "why" beyond one sentence to a chores `###` subsection.

- **Session-repo (`.claude`) body**: bullets describe
  in-session activity rather than code changes.

### Trailer

`ochid:` as the last line of the body — see
[Cross-repo linking (ochid trailers)](../AGENTS.md#cross-repo-linking-ochid-trailers)
in AGENTS.md for the convention.

For breaking changes, use the hyphenated `BREAKING-CHANGE:`
trailer key. `BREAKING CHANGE:` (with a space) is the only
space-separated key the Conventional Commits spec allows; the
hyphenated form is also valid and avoids the space ambiguity.

## Reviewing changes

Work review looks at the **uncommitted working-copy diff**,
on the way to commit. The user opens diffs in their
editor (Zed, VSCode); jj commands are for terminal:

- `jj diff` — working-copy diff (uncommitted)
- `jj diff -r @-` — diff of the previous commit
- `jj diff --from <X> --to <Y>` — any two revisions
- `jj show -r <X>` — description + diff for one rev

Don't `jj edit -r @-` to view a past commit — that marks
it mutable and shifts `@`; use `jj diff -r @-` or
`jj show -r @-`.

See [Sub-cycle ladders](#sub-cycle-ladders) for the
close-out squash recipe and recovery; revset primitives
are in [`jj-tips.md`](jj-tips.md#revsets).

## Pushing

### Policy

Push is **discretionary** during the cycle (backup,
progress visibility) and **mandatory at close-out** —
the cycle's result must be published.

**Approval is per-push.** Every push — any repo, any kind:
cycle push, interim backup, recovery/surgery force-push —
happens only after the user has reviewed the changes to be
published and explicitly approved that specific push.
Approval of a plan that *includes* a push does not authorize
the push itself; stop and ask again at the moment of pushing.

**Default is interactive; an explicit scoped delegation waives
the gates.** The gates above — per-push approval, the
commit-description review (show title+body and stop), and the
hard stop after push/finalize — are the *interactive default*.
They yield when the user **explicitly** delegates a complete,
bounded task and authorizes carrying it through ("do all of X
and push each step, don't check in"). The bot then proceeds
through that task's commits and pushes without stopping, and
continues past each push to the next step. Conditions:

- **Explicit grant** — never inferred from a task merely being
  well-scoped; the user's words must authorize unattended
  completion. "Commit and push" (or "then push") names the
  destination, not a waiver: it authorizes the push *after*
  the normal work review and description review, not skipping
  them. Only wording that explicitly waives the stops ("don't
  check in", "no need to review", "carry it through
  unattended") waives them.
- **Bounded goal** — covers the named task only; does not carry
  to the next task or a vaguer follow-on.
- **Destructive ops still pause** — delegation covers the task's
  ordinary commits and pushes; it does *not* pre-authorize a
  genuinely irreversible action (force-push over published
  history, history rewrite, deleting a remote branch). Those can
  permanently destroy work and aren't a normal cycle step, so the
  bot flags one before acting. An ordinary delegated cycle never
  reaches this.
- **Still transparent** — report each commit/push as it lands
  (title + outcome) so the user can catch up.
- **When in doubt, ask** — ambiguous authorization falls back to
  per-push approval.

### Shape at close-out push

At close-out the cycle's *work* is done; its *published
shape* is the remaining choice, made at push time. Surface
the options and get user approval before pushing — once on
the target, changing shape is a remote rewrite (force-push,
needs approval), so choose deliberately.

- **Squash to one commit** — single entry on the target.
  Right for straightforward changes where the Work-N is
  focused on one or two files.
- **Merge non-ff** *(current default)* — `main` gains a
  merge commit (`X.Y.Z`); cycle commits stay reachable via
  two parents. `jj log -r ..@ -n <N>` shows the trapezoidal
  shape. See [Merge non-ff recipe](#merge-non-ff-recipe)
  for the full setup sequence.
- **Keep separate** — one commit per cycle entry on
  `main`. Use when the decomposition itself is
  informative. Each chores section keeps its own header /
  `Commits:` ref; no consolidation churn.

Set up squash/merge before invoking `vc-x1 push`; use
`jj git push` directly for non-standard shapes.

### Merge non-ff recipe

Setting up a [Merge non-ff](#shape-at-close-out-push)
close-out is a fixed sequence. `<closeout>` is the cycle's
close-out commit, `<prev>` the previous cycle's close-out
(the current `main` tip), `<work-tip>` the cycle's last
Work commit:

1. **Rebase the close-out into a merge** — `jj rebase -r
   <closeout> --onto <prev> --onto <work-tip>`.
   - `-r <closeout>` keeps the `<closeout>` commit in place.
   - `--onto <prev>` becomes its first parent (trunk).
   - `--onto <work-tip>` becomes the second parent.

   Together these make `<closeout>` a merge of
   `<prev>` + `<work-tip>`, forming a trapezoidal commit.
2. **Use `jj new <merge>`** to add an empty `@` above the
   merge. The rebase left `@` *on* the now-content-bearing
   merge, which git/IDE diff views show as uncommitted;
   `jj new` restores the clean empty `@` on top.
3. **Push** — `jj git push --bookmark main -R .`.

**Post-hoc caveat.** If the cycle was already pushed
[Keep separate](#shape-at-close-out-push) its commits are
immutable: the rebase needs `--ignore-immutable` and the
push force-updates `main`.
The standard sequence assumes the merge is set up *before*
the close-out push.

### vc-x1 push wrapper

`vc-x1 push <bookmark>` wraps per-push mechanics. See
`vc-x1 push --help` for current flags.

**Current limitation**: only fully supports the
[Keep separate](#shape-at-close-out-push) shape; other
shapes need manual jj steps. Improvements tracked /
planned:

- N:1 code↔bot for Merge non-ff (`## Todo` entry P1).
- Symmetric squash (planned, to be captured in `-2`).
- Per-repo bookmark names (planned).

### .claude cadence

**Cadence**: one push = one `.claude` commit, paired
with every code commit in that push.

The `.claude` working copy accumulates session data
across the cycle; its change ID stays stable across
snapshots, `jj describe`, and the finalize commit, so
code-side `ochid:` trailers resolve.

`.claude` is a linear journal — all session work lives
on `main`, regardless of the app-side bookmark. **Do not
create or maintain `.claude` bookmarks that mirror
app-side branches** — risks the bot steering session
pushes to the wrong remote ref.

Ending a session: if the user runs `/exit` there will be
session information created, which we don't worry about.
The user can close the terminal instead and `@` will
remain empty.

### Bot communication at the reviews

Use plain prose — no insider jargon ("Gate N signal",
"Checkpoint N", etc.):

- **At Work review** — summarize what changed and stop.
  "Work complete. Please review."
- **At Commit Description review** — present `$TITLE`
  and `$BODY` explicitly; ask permission to commit/push.
  Don't spell out the full `vc-x1 push ... --title ...
  --body ...` invocation by default.
- **At Post close-out review** — surface the shape
  options (squash / merge / keep) and the push target;
  wait for the user's choice before any `jj squash` /
  `jj rebase` / `jj git push` invocation.

### After push or finalize: stop and wait

After you **push** (cross the remote boundary) or launch
**finalize** on the `.claude` repo (`vc-x1 finalize --repo
.claude --squash --push <bookmark> --delay 10 --detach`) — by
hand or via the `vc-x1 push` wrapper — you **MUST NEVER**
proceed (next step, edit, tool call, text output) until the
user explicitly directs you to continue. **Even when the next
step seems obvious — wait.** Treat push-or-finalize as a hard
stop for the whole turn.

`vc-x1 push` performs both as its tail stages — `push-app`
then the detached `vc-x1 finalize` on `.claude` — so there is
no gap left to speak in once the push is invoked. Put **all**
closing words *before* invoking it. The harness rejects an empty
turn, so it may force a visible token after the tool returns; if
so, emit a bare acknowledgment only (e.g. "landed") — never a
summary, verification, or next-step offer.

Why (`.claude` is the live journal finalize snapshots →
squashes → pushes):

- **`--detach`**: lets the tool return immediately so the
  harness can keep flushing the *trailing* session-data — the
  tool call's own transcript and any words emitted *before*
  the invoke — into the snapshot.
- **`--delay 10`**: gives the harness time to complete that
  flush before the snapshot is taken. It is **not** time for
  the bot to do more work.
- **silence**: the bot must not summarize or perform any work
  after the tool returns. The harness may force a visible token
  (it rejects an empty turn) — keep it to a bare acknowledgment,
  never a summary or a next step.

**Known slip**: the bot has emitted a summary after launching
finalize. There is no "harmless" closing line — if it's worth
saying, say it *before* the invoke.

### Recovery

- **If push exits before `finalize-claude`** (e.g.
  failure between `push-app` and `finalize-claude`), run
  finalize by hand:

  ```
  vc-x1 finalize --repo .claude --squash --push <bookmark> --delay 10 --detach --log /tmp/vc-x1-finalize.log
  ```

- **Run finalize again if `@` is non-empty** after the
  finalize's squash-and-commit (also desirable after
  extra activity by the bot's agents).
  - Why: finalize snapshots `.claude` at `--delay`, but the
    bot keeps writing session data while it runs — the tool's
    own invocation plus the bot's closing response land
    *after* the snapshot.
  - Safe to repeat: bot session data is append-only, so a
    re-run never conflicts or overwrites. (This could
    change; it is not under the user's control.)
  - Fix: run `vc-x1 finalize --repo .claude --squash --push
    <bookmark> --delay 10 --detach` another time to capture
    that tail; one extra pass is generally enough.
  - No guarantees: events outside the bot's control can leave
    `@` non-empty — e.g. the bot's back end may decide to
    squash/consolidate session data, which can take minutes
    and land after the snapshot. The remedy is the same: just
    run finalize again. This is why a single pass is never
    guaranteed to leave `@` empty.
- **Clear push's saved state** after any out-of-band
  recovery — `rm .vc-x1/push-state.toml` or `vc-x1 push
  <bookmark> --restart` — otherwise push resumes from a
  stale stage.
- **Late code-repo tweak after `push-app` succeeded**
  (e.g. updating AGENTS.md or memory) requires `jj
  squash --ignore-immutable` and a re-push; that is a
  remote rewrite and needs explicit approval like any
  push.

## Iterative work

When work for a single commit (the **target**) benefits
from incremental review, loop:

1. `jj new -R .` — fresh empty `@` on top of the target.
2. Make the next round of changes.
3. User reviews the round (see
   [Reviewing changes](#reviewing-changes)).
4. `jj squash -R .` folds into the target and creates a
   new empty `@`.
5. If not done, go to step 2.

Same jj mechanics as a
[sub-cycle ladder](#sub-cycle-ladders), but at
single-commit scope — the version
doesn't change.

## Sub-cycle ladders

When a Work commit subdivides into a sub-cycle (see
[Numbering](#numbering) for suffix nesting), its Work
commits typically live as a local jj `@` chain and
**collapse into the sub-cycle's Close-out** before the
parent cycle continues. Ladder commits are scratch —
for review and bisection only.

### Per-Work-commit contract within a ladder

For each Work commit in the ladder:

1. `jj new -R .` — create a fresh empty `@`.
2. Do the commit's work.
3. Run the fast validation (Rust example: `cargo test
   --bins`). **Non-negotiable** — for code, build and clippy
   alone miss regressions until a later commit runs the full
   suite, raising bisection cost.
4. `jj describe -m "..." -m "..." -R .` — working title
   only; the sub-cycle Close-out collects everything
   into one final commit.

### Navigating the ladder

Common moves:

- `jj log -r '<base>::' -R .` — see the whole ladder
  from its base.
- `jj edit -r <prefix> -R .` — jump `@` to any ladder
  commit by chid prefix; useful for bisection.
- `jj edit @-- -R .` — quick-jump back two commits.
- `jj diff -r <chid> -R .` — review one commit in
  isolation.

Modifications to any ladder commit rewrite it in place;
descendants auto-rebase.

### Close-out: squash the ladder

When all ladder Work commits are done and tests pass:

```
jj squash --from "<base>..@-" --into @ -u -R .
```

`<base>` is the parent of the first ladder commit; `-u`
keeps `@`'s description and discards the sources'.
After squash, history is linear: `<base> → @`;
intermediate commits are auto-abandoned.

Then `vc-x1 push <bookmark>` as for any other commit.

For N = 1 the squash is a no-op (`<base>..@-` is empty
when `@-` is `<base>`); push the single commit directly.

### Recovery

If a ladder commit goes wrong, back out without losing
prior commits:

- **Discard the current commit.** `jj abandon @ -R .`
  drops it; you get a fresh empty `@` on the same
  parent.
- **Edit an earlier commit.** `jj edit -r <chid> -R .`,
  make corrections, then `jj edit -r <last-ladder-chid>`
  to return. Descendants auto-rebase.
- **Discard the entire ladder.** `jj op log -R .` shows
  the op history; `jj op restore <op-id> -R .` reverts
  to that point. Full undo — removes *all* ladder work
  after the chosen op. Use only to start over.

# References

- [`jj-tips.md`](jj-tips.md#revsets) — revset primitives
  (chid/cid, `@`/`@-`/`@+`, `..`/`::` ranges, prefix matching).
- [`substep-test.sh`](substep-test.sh) — script that
  scaffolds a 4-revision ladder under `/tmp/substep-test`
  for squash-recipe experiments.
- The per-commit `cargo test --bins` gate exists because a
  regression introduced in an early ladder commit can go
  uncaught until a later commit runs the full suite, raising
  bisection cost.
