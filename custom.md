# custom.md - the project layer

The project's own layer over the other agent-files (see
[AGENTS.md](AGENTS.md#custommd)). Loaded last. On conflict, this file wins.

## Project conventions and overrides

Project-local conventions and overrides of the agent-files. An override names the section it
supersedes.

- Messaging: the `../vc-x1-messages` repo. Its `README.md` is the protocol and it governs, and
  a session reads what is pending for us there at acquaint, per its Read messages action.
  - Fetch: at an acquaint and at every cycle start the session does that README's Fetch action
    first, under its guards. Wink's permission for it is standing (2026-10-09), so it is not
    asked for.
- Tags: a step added to [Land](agent-data/jj.md#cycle-bookmarks-create-and-land). Every Land
  tags the commit `main` moves to with its version, `jj tag set v<version> -r main`, and publishes
  the tag beside `main`, `jj git push --tag v<version>`, so a dependent can pin a git dependency
  by tag, as io-uring-x1 does (`m-12` in the messages repo). The user may say "no tag" at a
  Land's go, and that Land is not tagged.
- Long output: the commands this workspace adds to [Working
  practices](AGENTS.md#working-practices)' Long output rule, each run in its form.
  - `vc-x1 validate`, full and `--fast`
  - `cargo test`, `cargo clippy`, and `cargo build`
  - `jj log` without `-n`, and `jj diff` or `jj show` of a rung
