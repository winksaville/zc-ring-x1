# Chores-02

Chores-XX files use [Prose form](../../AGENTS.md#prose-form). They
contain discussions and notes on various chores in github compatible
markdown. There is also a [todo.md](../todo.md) file that tracks
tasks and in general there should be a chore section for each task
with the why and how this task will be completed.

Continues [chores-01.md](chores-01.md), opened as that file
approached ~1000 lines. Reference numbering restarts at `[1]`
(file-local — see
[Reference numbering](../../AGENTS.md#reference-numbering)).

## refactor: sibling module dirs (spsc, mpsc, pool)

Commits:

The three ring/pool primitives sit at uneven depths: `mpsc/`
is already a directory, but the SPSC ring is spread across
top-level `src/producer.rs` / `src/consumer.rs` plus its
core types in `src/lib.rs`, and the pool is a single
`src/pool.rs`. Give SPSC and pool their own directories so
all three read as siblings and `src/lib.rs` narrows to the
genuinely shared core.

Provisional plan (rough — the ladder is nailed down with the
user before the cycle opens):

- `src/spsc/` — move `producer.rs`, `consumer.rs`, and the
  SPSC-specific core out of `lib.rs`: `Header`, `Ring`,
  `MAGIC`, `LAYOUT_VERSION`, `header_ptr`, `region_size`,
  `validate_geometry`.
- `src/pool/` — promote `pool.rs` to `pool/mod.rs` (split
  further only if a real seam appears).
- `src/lib.rs` — keeps only what all three share:
  `CacheAligned`, `CACHE_LINE_SIZE`, `USER_WORDS`, `Error`,
  `check_type` / `type_fits`, `slot_ptr`; plus the crate's
  public re-exports.

### Open design questions

Settle these with the user before writing the ladder:

- **Public name surface** — keep the flat re-exports
  (`Producer`, `Consumer`, `Ring`) for source compat, or
  namespace them (`spsc::Ring`, `mpsc::MpscRing`)? Namespacing
  would let the `Mpsc*` prefixes drop, at the cost of a
  breaking rename.
- **Where `registry` lives** — the descriptor registry is
  SPSC-flavored today; does it move under `spsc/`, stay
  top-level as cross-cutting, or wait for the descriptor-queue
  endpoints work (Todo #1)?
- **Shared core home** — leave the shared bits inline in
  `lib.rs`, or lift them into a `core`/`common` submodule so
  `lib.rs` is purely wiring + re-exports?
- **Pool as a directory now vs later** — `pool.rs` is one
  file with no internal seam yet; a bare `pool/mod.rs` buys
  symmetry but nothing structural. Worth it, or leave pool a
  file until it actually splits?

# References
