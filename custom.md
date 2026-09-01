# custom.md - the project layer

The project's own layer over the other agent-files (see
[AGENTS.md](AGENTS.md#custommd)). Loaded last. On conflict, this file wins.

## Project conventions and overrides

Project-local conventions and overrides of the agent-files. An override names the section it
supersedes.

- Messaging: the `../vc-x1-messages` repo. Its `README.md` is the protocol and it governs, and
  a session reads our inbox there at acquaint, per its Read messages action.
