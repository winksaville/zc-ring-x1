//! Generic runner for phase-probed measurement examples: CLI
//! config, thread pinning, and a fixed-duration round-trip
//! drive loop over injected send/recv closures.
//!
//! - [`Cfg`] / [`Cfg::parse`] — the shared CLI grammar
//!   (`-d`/`--duration`, `--pin main,worker`, `-t`/`--ticks`,
//!   `--decimals <n>`);
//!   anything else lands in `positionals` for the example to
//!   interpret ([`usage_exit`] for rejects).
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

/// Parsed CLI configuration for a probed example run.
pub struct Cfg {
    /// Wall-clock budget per flavor.
    pub duration: Duration,
    /// `Some((main_cpu, worker_cpu))` pins both threads.
    pub pin: Option<(usize, usize)>,
    /// Report raw ticks instead of nanoseconds.
    pub ticks: bool,
    /// Fractional digits on report value columns.
    pub decimals: usize,
    /// Non-flag arguments, in order, for the caller to
    /// interpret (e.g. flavor names).
    pub positionals: Vec<String>,
}

/// Print `usage` and exit 2.
pub fn usage_exit(usage: &str) -> ! {
    eprintln!("usage: {usage}");
    std::process::exit(2);
}

impl Cfg {
    /// Parse the process args against the shared grammar;
    /// [`usage_exit`]s on a malformed flag. Defaults: 5 s,
    /// unpinned, ns reporting, 1 decimal.
    pub fn parse(usage: &str) -> Cfg {
        let mut cfg = Cfg {
            duration: Duration::from_secs(5),
            pin: None,
            ticks: false,
            decimals: 1,
            positionals: Vec::new(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "-d" | "--duration" => {
                    let secs: f64 = args
                        .next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage_exit(usage));
                    cfg.duration = Duration::from_secs_f64(secs);
                }
                "--pin" => {
                    let v = args.next().unwrap_or_else(|| usage_exit(usage));
                    let (m, w) = v.split_once(',').unwrap_or_else(|| usage_exit(usage));
                    let m = m.parse().unwrap_or_else(|_| usage_exit(usage));
                    let w = w.parse().unwrap_or_else(|_| usage_exit(usage));
                    cfg.pin = Some((m, w));
                }
                "-t" | "--ticks" => cfg.ticks = true,
                "--decimals" => {
                    cfg.decimals = args
                        .next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage_exit(usage));
                }
                _ if a.starts_with('-') => usage_exit(usage),
                _ => cfg.positionals.push(a),
            }
        }
        cfg
    }
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
