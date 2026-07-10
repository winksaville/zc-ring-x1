# zc-msg-x1 — follow-on repo plan

Development moves to
[zc-msg-x1](https://github.com/winksaville/zc-msg-x1), a new
repo focused on the MPSC messaging layer (ring + pools +
descriptor-queue endpoints). zc-ring-x1 is frozen as the
ring-protocol lab notebook: two working protocols (SPSC +
MPSC), the tp-matrix comparison harness, and the measured
findings stay here intact — nothing is deleted.

## Why MPSC-only

- Cross-core, SPSC moves ~10.0 cache lines per round trip vs
  MPSC's ~6.7 and loses ~26–40%; the whole gap is
  line-transfer economics [[1]]. SPSC's remaining wins are
  same-core/SMT placements and the load-store-only floor.
- SPSC's one-accessor rule complicates
  [Execution contexts](ring-buffer-design.md#execution-contexts):
  the "threads sharing an endpoint" (mutex) and "ISR sharing
  an endpoint" (irqsave + try-and-bail) cases exist only
  because SPSC has exactly one producer slot. MPSC's answer
  is uniform — every context gets its own Sender; the CAS
  claim serializes lock-free.
- The planned endpoints (below) then design against one ring
  flavor instead of two.
- Prior art: io_uring's completion side chose the same shape
  — spill to a kernel-side overflow list and throttle the
  source — while its submission side fails fast to the
  caller.

## No-CAS targets (RP2040 / thumbv6m / RV32IMC)

Dropping SPSC drops the load-store-only floor; the plan for
CAS-less targets is
[portable-atomic](https://crates.io/crates/portable-atomic):

- native atomics where the ISA has them (zero cost);
- critical-section CAS emulation where it doesn't —
  DI/EI on single-core M0, hardware-spinlock
  critical-section on dual-core RP2040;
- RP2350 needs nothing: Hazard3 is RV32IMAC (LR/SC) and
  Cortex-M33 is ARMv8-M — CAS native in both modes;
- likely an optional feature so mainstream targets stay
  literally `zerocopy + core`.

## Endpoint design decisions

Types `Sender` / `Receiver` (not DescSender/DescReceiver —
the descriptor is the wire format, not the user concept; and
`Send` as a type name collides with the auto trait). Methods
`try_send` / `send` / `recv`:

- `try_send` drains the pending FIFO first, never appends to
  it; if the ring is still Full it returns
  `Err(Full(BufSlot))` — the guard comes back, so
  leak-on-failure is impossible by construction.
- `send(BufSlot)` is infallible: ring if possible, else
  append to a sender-private intrusive pending FIFO
  [details](ring-buffer-design.md#overflow-fifo-future).
  The message was already allocated, so the append cannot
  fail; backpressure moves to `alloc` — where a pool system
  wants it.
- The intrusive link requirement is free: every pool buffer
  header already carries the free-stack next-link, and one
  link field serves every buffer state.
- Per-sender pending FIFOs compose with MPSC (each Sender
  owns its own; per-producer order is all MPSC promises
  anyway — see
  [Overflow readiness](ring-buffer-design.md#overflow-readiness)).

## Library shape: benches + examples, no demo bin

zc-ring and zc-msg are libraries; a `src/bin` demo ships
with the crate and drags dev-only dependencies into the
install surface. zc-msg-x1 instead uses:

- `benches/` with `harness = false` — the pinned round-trip
  cells, pool loops, and perf-counter runs become bench
  targets with their own `main` (tp_runner-style), run via
  `cargo bench`; they compile against dev-dependencies, so
  the lib stays `zerocopy + core`.
- `examples/` + doctests — usage demonstration; the
  endpoints' ~3-line send path belongs as a doctest on the
  crate root and on `Sender::send` / `Receiver::recv` —
  documentation, example, and CI-enforced test in one.

## Seeding zc-msg-x1

- Import from zc-ring-x1: `mpsc`, `pool`, `registry`,
  `policy`, and the demo's non-SPSC measurement code
  (reshaped into benches/examples). First commit records
  provenance: "imported from zc-ring-x1 @ <sha>".
- Stays here: `src/spsc` and `tp_matrix` (the SPSC-vs-MPSC
  comparison is this repo's story).
- `tprobe` / `tp_runner`: path-dep to the sibling repo for
  now; extract `tprobe` to its own repo when a second
  consumer materializes. For the "instrument any existing
  project" ambition, evaluate `tracing` + `tracing-timing`
  first — tprobe's defensible niche is the ns-scale hot
  path where tracing's span overhead swamps the signal.
- Notes scaffolding (AGENTS.md, cycle-protocol.md,
  versioning.md, jj-tips.md) copies essentially verbatim;
  fresh todo.md / chores-01.md; the design doc is curated —
  MPSC + messaging-layer sections move, SPSC sections stay,
  with back-links here for the measured rationale.
- Version restarts at 0.1.0.

## Graduation (future)

When endpoints + overflow prove out, graduate to a
production crate: curated fresh history, real name
(`zc-ring` / `zc-msg`), `-x1` repos stay behind as lab
notebooks. A future decision point, not a now-task.

# References

[1]: chores/chores-02.md#findings-the-gap-is-line-transfer-economics
