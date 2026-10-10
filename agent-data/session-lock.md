# The session lock

An optional rule, off unless a project's [custom.md](../custom.md) turns it on: an agent session
holds an exclusive `flock` on `.agent-running` at the workspace root for as long as it runs, so a
second session started in the same workspace stops at its acquaint.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## The instruction

Turning the lock on is two edits: this entry goes into
`custom.md > ## Project conventions and overrides`, first in its list and in place of the entry
that says the lock is off, and `/.agent-running` goes into the work-repo's `.gitignore`.

```
- Lock: at acquaint, before anything else, a session runs `flock -xn .agent-running true` at the
  workspace root, alone. If it fails, the session responds with the one sentence "Another session
  holds `.agent-running`, stopping." and runs no further command, not even to find the holder. If
  it succeeds, the session starts `flock -xn .agent-running sleep infinity` as a background
  command, leaves it running, and says "Lock taken." If that command exits while the session
  runs, the same sentence and the same stop. A session that already holds one does neither. A
  resumed session is told its holder was stopped with the earlier process, and it runs both
  again before anything else, as at acquaint. This keeps a second session from working in the
  workspace, so two cannot push or amend at once.
```

Turning it off is putting back the entry that says the lock is off and points here.

## What is known

- Nothing releases the lock. The kernel drops a `flock` when the process holding it exits, and
  the session's background commands end when the session does.
- It closes no race and needs none closed: a `flock` has one winner whoever asks. A session that
  loses between its probe and its holder learns of it seconds later, when the holder exits.
- Its weakness is that an agent has to read the instruction and follow it.
- The instruction has two faults, and neither has a fix:
  - A session that has forgotten it holds the lock probes again at a reacquaint, fails against
    its own holder, and stops as if another session held it.
  - An edit to the instruction reaches only the sessions started after it.
- The tool is Linux's: macOS has the `flock` call and does not ship the tool, and Windows has
  neither.
- Without the lock, two sessions working in one workspace lose no data and leave a wrong record
  (tested 2026-10-12), and one session per workspace is the user's to keep.
