# Agent-files size

The line count of the agent-files, one row per landing, so the set's size is tracked over time.
Smaller is the quasi-goal: a rule stated once is shorter than a rule stated three times, and a
shrinking count is evidence the set is converging, while a growing one is a prompt to ask what
arrived as a paragraph that should have been a line. The count is not a rule, and a rule is never
cut to move it.

The count is `wc -l AGENTS.md custom.md agent-data/*.md`, taken at close-out and recorded here as
the closing rung's last edit, with the cycle title as the row's label.

## Counts

| Landed | Cycle | Files | Lines | Note |
|---|---|---|---|---|
| 2026-08-28 | docs: adopt the family agent-files set | 11 | 2126 | the set as proposed, identical to vc-x1's at a4309084fdfe |
| 2026-08-28 | docs: keep the ladder markers in the closed block | 11 | 2129 | the drop-markers step gone, its rationale added |
| 2026-09-07 | agent-files(adoption): v0.2.3 | 10 | 2315 | iiac-perf's set at d5d5e77a3bb1 copied whole, messaging.md gone, the version marker added |
| 2026-09-10 | fix: mpsc handling of capacity 1 | 10 | 2315 | unchanged, no agent-file touched |

Per file at the last row, replaced at each close-out, the history being in the commits:

```
   384 AGENTS.md
    12 custom.md
    94 agent-data/code.md
    42 agent-data/commit-model.md
    76 agent-data/cycle-model.md
   391 agent-data/jj.md
   166 agent-data/notes.md
   405 agent-data/prose.md
   541 agent-data/rationale.md
   204 agent-data/versioning.md
  2315 total
```
