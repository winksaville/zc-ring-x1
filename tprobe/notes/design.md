# tprobe design

Design rationale for the tprobe crate — the reasoning that
doesn't belong in doc comments. The crate provides hardware
tick-counter probes for phase-level latency measurement; its
sibling `tp_runner` provides the generic runner the probed
examples share.

## Two probe primitives, kept separate

- `TProbe` — the caller brackets a phase with two
  `ticks::read_ticks()` and records the delta. The hot path
  is one histogram write; the fit for high-rate loops.
- `TProbeSpan` — `start(site_id)` / `end(id)` over a
  deferred-processing record buffer. Preserves record order
  and supports non-stack interleaving by construction; the
  evolution space for per-site grouping, bounded buffers,
  and trace retention.
- They stay separate types because the span API's
  buffer-per-sample model trades hot-path throughput for
  flexibility — mixing the paths on one type forces awkward
  trade-offs both ways.
- `TProbeSpan`'s buffer grows without bound (24 B/record),
  so it is the wrong tool for tens of millions of samples;
  prefer `TProbe` there.

## Ticks, not nanoseconds, on the hot path

Recording raw tick deltas defers the tick→ns mul-shift to
report time. `ticks` abstracts the per-arch counter:

- x86_64 — `rdtsc`; `require_ok` demands invariant TSC
  (CPUID) and, on Linux, that the kernel selected `tsc` as
  its clocksource (sysfs read); calibration derives
  ticks/ns from a ~10 ms spin against `std::time::Instant`.
- aarch64 — `CNTVCT_EL0`, frequency from `CNTFRQ_EL0`; no
  calibration loop needed (architecturally invariant).

## One report renderer, three units

`band_table` renders every probe: percentile band rows
(min/p1/…/p99/max) with first/last/range/count/mean columns
plus whole-run and trimmed (`min-p99`) summary stats. The
display `Unit` is chosen at report time:

- `ns` (default) / `tk` (raw ticks) for time probes;
- `ct` for counts probes (`TProbe::new_counts`) — unitless
  values (e.g. spin attempts) that must never go through
  tick→ns conversion.

## The tprobe / tp_runner split

Probes (this crate) are the measurement primitives; the
runner (CLI config, thread pinning, the fixed-duration
round-trip drive loop, report ordering) is generic example
machinery, not measurement — so it lives in the sibling
`tp_runner` crate. `tp_runner::drive` deliberately measures
nothing itself: probing lives in the caller's closures, so
each experiment controls exactly what a probe brackets and
records after its phase-end tick read, off the measured
path.

Neither crate is a benchmark harness: no adaptive loop
sizing, overhead calibration, or bench registry. Needing
those is the cue to use iiac-perf (the sibling repo these
probes derive from) instead of growing them here.
