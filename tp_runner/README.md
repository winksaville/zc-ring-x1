# tp_runner

The generic runner half of the workspace's phase-probed
measurement examples: CLI config, thread pinning, and a
fixed-duration round-trip drive loop. The probes themselves
are the sibling `tprobe` crate; examples contribute only
their workload closures.

- `Cfg::parse` — shared CLI grammar: `-d`/`--duration <secs>`,
  `--pin <main,worker>`, `-t`/`--ticks`,
  `--decimals <n>` (default 1); other positionals
  pass through for the example to interpret.
- `pin_to_cpu` — `sched_setaffinity` pinning (Linux; no-op
  stub elsewhere).
- `drive` — the round-trip loop: send a counter, receive the
  echo; probing lives in the caller's closures; wall clock
  checked every 4096 iterations; the counter skips `STOP` so
  callers can use it as a shutdown sentinel.
- `report` — flavor header + phase reports in trip order.

Deliberately not a benchmark harness — no adaptive loop
sizing, overhead calibration, or bench registry. Needing
those is the cue to use iiac-perf instead.

## Build / test

```sh
cargo build -p tp_runner
```

## Run

Nothing to install — it's a library; run it through an
example, e.g.:

```sh
cargo run --release --example tp_roundtrip -- both -d 5 --pin 0,1
```
