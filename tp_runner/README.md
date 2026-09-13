# tp_runner

The generic runner half of the workspace's phase-probed
measurement examples: CLI config, thread pinning, and a
fixed-duration round-trip drive loop. The probes themselves
are the sibling `tprobe` crate; examples contribute only
their workload closures.

- `CommonArgs` / `Cfg` — the shared clap flags the binaries
  flatten into their own parsers, `-d`/`--duration <secs>`,
  `-t`/`--ticks`, `--decimals <n>` (default 1), and
  `--depth <list>`, and the runtime config built from them.
  `parse_pin` parses a `--pin MAIN,WORKER` value and
  `parse_depth` one `--depth` element.
- `LineBuf` — a cache-line-aligned heap region sized at
  runtime, so a cell's ring depth or pool size is a parameter.
- `pin_to_cpu` — `sched_setaffinity` pinning (Linux; no-op
  stub elsewhere).
- `drive` — the round-trip loop: send a counter, receive the
  echo; probing lives in the caller's closures; wall clock
  checked every 4096 iterations; the counter skips `STOP` so
  callers can use it as a shutdown sentinel.
- `report` — flavor header + phase reports in trip order.
- `perf` (Linux) — per-process hardware event counters via
  `perf_event_open` (`inherit`, user-mode only), with the
  AMD Zen 2 demand-fill raw encodings.
- `topo` — placement discovery from `/sys`: same-L3,
  cross-L3, SMT-sibling pin pairs plus unpinned.

Deliberately not a benchmark harness — no adaptive loop
sizing, overhead calibration, or bench registry. Needing
those is the cue to use iiac-perf instead.

## Build / test

```sh
cargo build -p tp_runner
```

## Run

Nothing to install — it's a library; the workspace's
`tp_matrix` crate (`tp-cell`, `tp-matrix`, `tp-stream`, and
`tp-pool`) exercises it end to end:

```sh
cargo run --release -p tp_matrix --bin tp-cell -- all -d 5 --pin 0,1
```
