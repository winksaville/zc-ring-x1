# Bugs

This file uses [Prose form](../AGENTS.md#prose-form). It
lists known defects we're aware of but haven't scheduled a fix for.
Each entry describes what goes wrong, when, and the cost of
the failure. Entries are numbered (`1.` `2.` …) the same way
as `## Todo` in `todo.md`; run
`vc-x1 fix-todo --no-dry-run notes/bugs.md` to renumber after
insert / delete / reorder.

## Bugs

1. `reserve_slot` (both `Consumer` and `Producer`) is a strictly
   dominated API rung — `reserve_slot_with` does everything it does,
   never worse and sometimes better — and its naive "loop on the
   fallible call" usage carries a measurable latency + jitter cost.
   Consider removing both.
   - **What we measured.** iiac-perf's zcr round-trip benches
     (`-d 60`, unpinned 3900X, one message in flight) compare the
     tiers under real cross-core traffic. `zcr-raw-2t` (main loops on
     `reserve_slot` while waiting for the reply) lands a body mean
     ~130 ns, multimodal across ~120–150 ns, trimmed `min..n2` stdev
     ~15 ns. `zcr-with-2t` / `zcr-spin-2t` (same wait via
     `reserve_slot_with` / `reserve_slot_spin`) land ~110 ns, body
     clustered at 100/109/110 ns, trimmed stdev ~7 ns. So the raw
     retry pattern costs ~+30 ns and ~2× the body jitter. `with` and
     `spin` are statistically identical, as expected —
     `reserve_slot_spin` *is* `reserve_slot_with(policy::spin)`.
   - **Why (we think).** The only code difference on the wait path is
     that `reserve_slot`-in-a-loop re-reads the caller-*owned* index
     (`consumer_idx` on the read side, `producer_idx` on the write
     side) on every iteration, because each call reloads it;
     `reserve_slot_with` loads the owned index once and re-reads only
     the peer's index per attempt (its doc already states this).
     A relaxed atomic load can't be hoisted/coalesced out of a loop
     by LLVM, so the naive retry genuinely issues the extra load each
     spin. We think that lengthens and de-tightens the spin, so it
     samples the peer's store later and less regularly — a higher
     mean and a multimodal body in ~10 ns steps (≈ one Zen 2
     cross-core coherence hop). The two indices are each
     `CacheAligned` on separate lines, so this is **not** false
     sharing between them.
   - **Current thinking.** `reserve_slot` ≡
     `reserve_slot_with(|_| false)` (give up after the first failed
     attempt) — identical loads, identical result, and the `|_| false`
     loop folds away (worth a disasm confirm). For any retry beyond
     one probe, `_with` hoists the owned load and wins. The desirable
     always-non-blocking mode (e.g. driving the ring from a Tokio
     task) is `reserve_slot_with(|_| false)`; the better hybrid
     ("spin a bounded N, then return pending and let the runtime
     park") is `reserve_slot_with(|attempt| attempt < N)` — both
     things `reserve_slot` can't express. So the proposed shape is a
     two-rung ladder per endpoint: `reserve_slot_with` (fallible
     primitive) + `reserve_slot_spin` (forever-spin convenience),
     dropping `reserve_slot`. If a real consumer later makes
     `_with(|_| false)` common, add a thin `try_reserve_slot` alias
     (named like `Sender::try_send`) then — not before (no consumers
     exist yet).
   - **Downstream.** iiac-perf's `zcr-raw-1t` / `zcr-raw-2t` call
     `reserve_slot` directly and only exist to measure this naive
     pattern; removing the method drops (or relabels) those benches.
     Tracked on the iiac-perf side.

# References
