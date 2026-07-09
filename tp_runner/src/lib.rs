//! Generic runner for phase-probed measurement examples: CLI
//! config, thread pinning, and a fixed-duration round-trip
//! drive loop over injected send/recv closures.
//!
//! - [`Cfg`] / [`CommonArgs`] — the runtime configuration and
//!   the shared clap flags (`-d`/`--duration`, `-t`/`--ticks`,
//!   `--decimals <n>`) the binaries flatten into their own
//!   `Parser` structs; [`parse_pin`] for a `--pin MAIN,WORKER`
//!   value.
//! - [`pin_to_cpu`] — sched_setaffinity pinning (Linux; no-op
//!   stub elsewhere).
//! - [`drive`] — the round-trip loop: send a counter, receive
//!   the echo; probing lives in the caller's closures.
//! - [`report`] — flavor header + the phase reports in trip
//!   order.
//! - [`perf`] (Linux) — per-process hardware event counters
//!   via `perf_event_open`, for cache-fill counting inside
//!   measurement cells.
//!
//! Deliberately not a benchmark harness: no adaptive loop
//! sizing, overhead calibration, or bench registry — for that
//! scale of machinery use iiac-perf.

#[cfg(target_os = "linux")]
pub mod perf;
pub mod topo;

use std::time::{Duration, Instant};

use tprobe::TProbe;

/// Sentinel available to callers as a shutdown message;
/// [`drive`]'s counter skips it so payload values never
/// collide with it.
pub const STOP: u64 = u64::MAX;

/// Iterations between wall-clock checks in [`drive`], keeping
/// the per-iteration cost of `Instant::now` off the common
/// path.
const CLOCK_CHECK_EVERY: u64 = 4096;

/// Runtime configuration for a probed measurement run, built
/// from the parsed CLI args.
pub struct Cfg {
    /// Wall-clock budget per flavor.
    pub duration: Duration,
    /// `Some((main_cpu, worker_cpu))` pins both threads.
    pub pin: Option<(usize, usize)>,
    /// Report raw ticks instead of nanoseconds.
    pub ticks: bool,
    /// Fractional digits on report value columns.
    pub decimals: usize,
}

/// The CLI flags shared by the probed measurement binaries;
/// `#[command(flatten)]` into each binary's `Parser` struct.
#[derive(clap::Args, Debug)]
pub struct CommonArgs {
    /// Wall-clock seconds per cell (each flavor × placement
    /// combination runs this long)
    #[arg(
        short = 'd',
        long = "duration",
        value_name = "SECS",
        default_value_t = 5.0
    )]
    pub duration: f64,

    /// Report raw TSC ticks instead of nanoseconds
    ///
    /// Probes store hardware tick deltas; by default reports
    /// convert them to ns via the calibrated ticks-per-ns
    /// ratio. This flag shows the stored ticks unconverted.
    #[arg(short = 't', long)]
    pub ticks: bool,

    /// Fractional digits on report value columns
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub decimals: usize,
}

impl CommonArgs {
    /// Build the runtime [`Cfg`], attaching the binary-specific
    /// `pin` (only `tp-cell` exposes one).
    pub fn to_cfg(&self, pin: Option<(usize, usize)>) -> Cfg {
        Cfg {
            duration: Duration::from_secs_f64(self.duration),
            pin,
            ticks: self.ticks,
            decimals: self.decimals,
        }
    }
}

/// clap value parser for `--pin MAIN,WORKER`: two
/// comma-separated logical CPU numbers.
pub fn parse_pin(s: &str) -> Result<(usize, usize), String> {
    let (m, w) = s
        .split_once(',')
        .ok_or_else(|| format!("expected MAIN,WORKER (e.g. 0,1), got `{s}`"))?;
    let m = m
        .trim()
        .parse()
        .map_err(|_| format!("MAIN is not a CPU number: `{m}`"))?;
    let w = w
        .trim()
        .parse()
        .map_err(|_| format!("WORKER is not a CPU number: `{w}`"))?;
    Ok((m, w))
}

/// Pin the calling thread to `cpu` via sched_setaffinity;
/// panics on failure (a run with a silently ignored pin would
/// report a mislabeled number).
#[cfg(target_os = "linux")]
pub fn pin_to_cpu(cpu: usize) {
    // SAFETY: cpu_set_t is a plain bitmask; CPU_ZERO/CPU_SET
    // initialize it fully before sched_setaffinity reads it.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        let rc = libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set);
        assert_eq!(rc, 0, "sched_setaffinity({cpu}) failed");
    }
}

/// Non-Linux stub: `--pin` becomes a no-op.
#[cfg(not(target_os = "linux"))]
pub fn pin_to_cpu(_cpu: usize) {}

/// Reset the calling thread's affinity to every online CPU —
/// the undo for [`pin_to_cpu`], needed when one process runs
/// pinned and unpinned cells in sequence.
#[cfg(target_os = "linux")]
pub fn unpin_current() {
    // SAFETY: cpu_set_t is a plain bitmask; CPU_ZERO/CPU_SET
    // initialize it fully before sched_setaffinity reads it.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        let n = libc::sysconf(libc::_SC_NPROCESSORS_ONLN).max(1) as usize;
        for cpu in 0..n {
            libc::CPU_SET(cpu, &mut set);
        }
        let rc = libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set);
        assert_eq!(rc, 0, "sched_setaffinity(all) failed");
    }
}

/// Non-Linux stub: affinity is untouched, so nothing to undo.
#[cfg(not(target_os = "linux"))]
pub fn unpin_current() {}

/// A spin wait policy: hint and keep trying. Matches the
/// `on_full`/`on_empty` closure shape of the ring endpoints.
pub fn spin(_attempt: u32) -> bool {
    core::hint::spin_loop();
    true
}

/// Drive `dur` worth of round trips: send the counter via
/// `send`, receive the echo via `recv`. Returns when the
/// budget is spent (checked every [`CLOCK_CHECK_EVERY`]
/// iterations). The counter skips [`STOP`] so callers can use
/// it as a shutdown sentinel afterwards.
///
/// All probing lives in the closures — the loop itself
/// measures nothing, so callers control exactly what each
/// probe brackets (and record into their histograms *after*
/// their phase-end tick reads, off the measured path).
pub fn drive(dur: Duration, mut send: impl FnMut(u64), mut recv: impl FnMut() -> u64) {
    let start = Instant::now();
    let mut counter: u64 = 0;
    loop {
        for _ in 0..CLOCK_CHECK_EVERY {
            counter = counter.wrapping_add(1);
            if counter == STOP {
                counter = 1;
            }
            send(counter);
            let v = recv();
            assert_eq!(v, counter, "echo mismatch");
        }
        if start.elapsed() >= dur {
            return;
        }
    }
}

/// Print one flavor's header, then its phase reports in the
/// order given (conventionally trip order: main send → worker
/// recv → worker send → main recv).
pub fn report(flavor: &str, cfg: &Cfg, probes: impl IntoIterator<Item = TProbe>) {
    let pin = match cfg.pin {
        Some((m, w)) => format!("main={m},worker={w}"),
        None => "none".to_string(),
    };
    println!(
        "{flavor} round trip [duration={:.1}s pin={pin}]:",
        cfg.duration.as_secs_f64(),
    );
    for p in probes {
        p.report(cfg.ticks, cfg.decimals);
    }
}
