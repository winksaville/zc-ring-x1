# custom.md - the project layer

The project's own layer over the other agent-files (see
[AGENTS.md](AGENTS.md#custommd)). Loaded last. On conflict, this file wins.

## Project conventions and overrides

Project-local conventions and overrides of the agent-files. An override names the section it
supersedes.

- The session lock is off: see [session-lock.md](agent-data/session-lock.md#the-instruction) to
  turn it on.
- Messaging: the `../vc-x1-messages` repo. Its `README.md` is the protocol and it governs, and
  a session reads what is pending for us there at acquaint, per its Read messages action.
  - Fetch: at an acquaint and at every cycle start the session does that README's Fetch action
    first, under its guards. Wink's permission for it is standing (2026-10-09), so it is not
    asked for.
