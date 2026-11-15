# Agent-files size

The line count of the agent-files, one row per landing, so the set's size is tracked over time.
Smaller is the quasi-goal: a rule stated once is shorter than a rule stated three times, and a
shrinking count is evidence the set is converging, while a growing one is a prompt to ask what
arrived as a paragraph that should have been a line. The count is not a rule, and a rule is never
cut to move it.

The count is `wc -l` over `AGENTS.md`, `custom.md`, and `agent-data/*.md` less `rationale.md`,
since the rationale is the rules' why and grows with every rule that gains one, taken at the
close-out of a cycle that changed an agent-file and recorded here as the closing rung's last
edit, with the cycle title as the row's label. A cycle that touched no agent-file adds no row,
so the table is the history of the count and nothing else. Rows before 2026-09-16 counted
`rationale.md` too.

## Counts

| Landed | Cycle | Files | Lines | Note |
|---|---|---|---|---|
| 2026-08-28 | docs: adopt the family agent-files set | 11 | 2126 | the set as proposed, identical to vc-x1's at a4309084fdfe |
| 2026-08-28 | docs: keep the ladder markers in the closed block | 11 | 2129 | the drop-markers step gone, its rationale added |
| 2026-09-07 | agent-files(adoption): v0.2.3 | 10 | 2315 | iiac-perf's set at d5d5e77a3bb1 copied whole, messaging.md gone, the version marker added |
| 2026-09-10 | fix: mpsc handling of capacity 1 | 10 | 2315 | unchanged, no agent-file touched |
| 2026-09-11 | agent-files(adoption): v0.2.4 | 10 | 2315 | vc-x1's set at e378ce9ee494, the version file renamed and one clause of `custom.md` reworded, so the count holds |
| 2026-09-16 | agent-files(proposal): v0.2.5 | 9 | 1774 | a size row only when an agent-file changed, and `rationale.md` out of the count, so the files drop to 9 and the total from 2315 less its 541 |
| 2026-10-13 | agent-files(adoption): v0.2.8 | 10 | 1873 | vc-x1's set at 1139c742b8d4, v0.2.6 and v0.2.7 with it: `## Acquaint` and `## Sync` in `AGENTS.md`, `session-lock.md` new, two lines in `custom.md` |
| 2026-10-15 | agent-files(adoption): v0.2.9 | 10 | 1879 | vc-x1's set at 12a73d7135ab: a step 0 sync in the per-rung flow and a validation failure read before it runs again, in `AGENTS.md`, and the session lock line gone from `custom.md` |
| 2026-11-15 | agent-files(adoption): v0.3.1-13 | 10 | 2007 | vc-x1's draft set at bd1d20bf1131: `vc-x1 cycle` and the ladder in `zTOC.md`, hooks and a Long output rule in `AGENTS.md`, and a Long output entry in our own `custom.md` |

Per file at the last row, replaced at each close-out, the history being in the commits:

```
   570 AGENTS.md
    20 custom.md
    90 agent-data/code.md
    35 agent-data/commit-model.md
   141 agent-data/cycle-model.md
   332 agent-data/jj.md
   191 agent-data/notes.md
   384 agent-data/prose.md
    43 agent-data/session-lock.md
   201 agent-data/versioning.md
  2007 total
```
