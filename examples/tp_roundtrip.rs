//! Phase-probed 1p/1c round trip: localize where the SPSC vs
//! MPSC cross-thread latency gap lives.
//!
//! Main sends a counter to a worker over one ring and waits for
//! the echo on a second ring; each of the four protocol phases
//! is measured by its own [`TProbe`] (two `read_ticks` bracket
//! the endpoint call), so the reports separate the sides of the
//! handoff:
//!
//! - `main send` / `worker send` — the producer's reserve +
//!   fill + commit, including any stall acquiring peer-written
//!   cache lines. The ring is never full here (one message in
//!   flight, 8 slots), so no send ever waits for space.
//! - `worker recv` / `main recv` — the consumer's spin wait +
//!   read + release. These absorb the in-flight half trip;
//!   `worker recv` also absorbs main's inter-iteration framing
//!   (probe records, the every-N clock check).
//!
//! Usage: `tp_roundtrip [spsc|mpsc|both] [-d secs]
//! [--pin main,worker] [-t]` — defaults: both flavors, 5 s
//! each, unpinned, report in ns (`-t` for raw ticks).

use std::time::{Duration, Instant};

use tprobe::TProbe;
use tprobe::ticks;
use zc_ring_x1::{CACHE_LINE_SIZE, MpscRing, Ring};

/// Ring slots per direction — a power of two, comfortably above
/// the one message ever in flight.
const DEPTH: u32 = 8;

/// Shutdown sentinel; the worker exits on receipt without
/// replying. The counter skips it.
const STOP: u64 = u64::MAX;

/// Iterations between wall-clock checks in the main loop, so
/// the per-iteration cost of `Instant::now` stays off the
/// common path.
const CLOCK_CHECK_EVERY: u64 = 4096;

/// Region for one SPSC ring: 4-line header + DEPTH one-line
/// slots.
#[repr(C, align(64))]
struct Region([u8; 4 * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE]);

/// Region for one MPSC ring: header + per-slot seq array
/// (DEPTH × 4 B, padded to a line) + DEPTH one-line slots.
#[repr(C, align(64))]
struct MpscRegion(
    [u8; 4 * CACHE_LINE_SIZE
        + (DEPTH as usize * 4).next_multiple_of(CACHE_LINE_SIZE)
        + DEPTH as usize * CACHE_LINE_SIZE],
);

/// Parsed CLI configuration.
struct Cfg {
    /// Run the SPSC flavor.
    spsc: bool,
    /// Run the MPSC flavor.
    mpsc: bool,
    /// Wall-clock budget per flavor.
    duration: Duration,
    /// `Some((main_cpu, worker_cpu))` pins both threads.
    pin: Option<(usize, usize)>,
    /// Report raw ticks instead of nanoseconds.
    ticks: bool,
}

/// Parse args (see module doc for the grammar); exits with a
/// usage message on anything unrecognized.
fn parse_args() -> Cfg {
    let mut cfg = Cfg {
        spsc: true,
        mpsc: true,
        duration: Duration::from_secs(5),
        pin: None,
        ticks: false,
    };
    let usage = || -> ! {
        eprintln!("usage: tp_roundtrip [spsc|mpsc|both] [-d secs] [--pin main,worker] [-t]");
        std::process::exit(2);
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "spsc" => (cfg.spsc, cfg.mpsc) = (true, false),
            "mpsc" => (cfg.spsc, cfg.mpsc) = (false, true),
            "both" => (cfg.spsc, cfg.mpsc) = (true, true),
            "-d" | "--duration" => {
                let secs: f64 = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage());
                cfg.duration = Duration::from_secs_f64(secs);
            }
            "--pin" => {
                let v = args.next().unwrap_or_else(|| usage());
                let (m, w) = v.split_once(',').unwrap_or_else(|| usage());
                let m = m.parse().unwrap_or_else(|_| usage());
                let w = w.parse().unwrap_or_else(|_| usage());
                cfg.pin = Some((m, w));
            }
            "-t" | "--ticks" => cfg.ticks = true,
            _ => usage(),
        }
    }
    cfg
}

/// Pin the calling thread to `cpu` via sched_setaffinity;
/// panics on failure (a run with a silently ignored pin would
/// report a mislabeled number).
#[cfg(target_os = "linux")]
fn pin_to_cpu(cpu: usize) {
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
fn pin_to_cpu(_cpu: usize) {}

/// The spin wait policy both ends use: hint and keep trying.
fn spin(_attempt: u32) -> bool {
    core::hint::spin_loop();
    true
}

/// Print one flavor's header, then the four phase reports in
/// trip order: main send → worker recv → worker send →
/// main recv.
fn report(flavor: &str, cfg: &Cfg, probes: [TProbe; 4]) {
    let pin = match cfg.pin {
        Some((m, w)) => format!("main={m},worker={w}"),
        None => "none".to_string(),
    };
    println!(
        "{flavor} round trip [duration={:.1}s pin={pin}]:",
        cfg.duration.as_secs_f64(),
    );
    for p in probes {
        p.report(cfg.ticks);
    }
}

/// Drive `dur` worth of round trips: main sends the counter on
/// `send`, receives the echo via `recv`; phases recorded into
/// `send_probe` / `recv_probe`. Returns when the budget is
/// spent (checked every [`CLOCK_CHECK_EVERY`] iterations).
///
/// Generic over the flavors' send/recv closures so the SPSC and
/// MPSC drivers share the loop skeleton and clock policy.
fn drive(
    dur: Duration,
    send_probe: &mut TProbe,
    recv_probe: &mut TProbe,
    mut send: impl FnMut(u64),
    mut recv: impl FnMut() -> u64,
) {
    let start = Instant::now();
    let mut counter: u64 = 0;
    loop {
        for _ in 0..CLOCK_CHECK_EVERY {
            counter = counter.wrapping_add(1);
            if counter == STOP {
                counter = 1;
            }
            let s = ticks::read_ticks();
            send(counter);
            send_probe.record(ticks::read_ticks().wrapping_sub(s));
            let s = ticks::read_ticks();
            let v = recv();
            recv_probe.record(ticks::read_ticks().wrapping_sub(s));
            assert_eq!(v, counter, "echo mismatch");
        }
        if start.elapsed() >= dur {
            return;
        }
    }
}

/// SPSC flavor: two `Ring`s, both ends `reserve_slot_with`
/// under the [`spin`] policy.
fn run_spsc(cfg: &Cfg) {
    let mut req_region = Region([0; size_of::<Region>()]);
    let mut resp_region = Region([0; size_of::<Region>()]);
    let (mut req_tx, mut req_rx) = Ring::init(&mut req_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the header + DEPTH slots
        .split();
    let (mut resp_tx, mut resp_rx) = Ring::init(&mut resp_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the header + DEPTH slots
        .split();

    let probes = std::thread::scope(|s| {
        let worker_cpu = cfg.pin.map(|(_, w)| w);
        let worker = s.spawn(move || {
            if let Some(cpu) = worker_cpu {
                pin_to_cpu(cpu);
            }
            let mut recv_probe = TProbe::new("spsc worker recv (reserve+release)");
            let mut send_probe = TProbe::new("spsc worker send (reserve+commit)");
            loop {
                let s = ticks::read_ticks();
                let slot = req_rx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                let v = *slot;
                slot.release();
                let e = ticks::read_ticks();
                if v == STOP {
                    break;
                }
                recv_probe.record(e.wrapping_sub(s));
                let s = ticks::read_ticks();
                let mut slot = resp_tx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                *slot = v;
                slot.commit();
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            }
            (recv_probe, send_probe)
        });

        if let Some((main_cpu, _)) = cfg.pin {
            pin_to_cpu(main_cpu);
        }
        let mut send_probe = TProbe::new("spsc main send (reserve+commit)");
        let mut recv_probe = TProbe::new("spsc main recv (reserve+release)");
        drive(
            cfg.duration,
            &mut send_probe,
            &mut recv_probe,
            |v| {
                let mut slot = req_tx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                *slot = v;
                slot.commit();
            },
            || {
                let slot = resp_rx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                let v = *slot;
                slot.release();
                v
            },
        );
        let mut slot = req_tx
            .reserve_slot_with::<u64>(spin)
            .expect("spin never gives up");
        *slot = STOP;
        slot.commit();
        let (worker_recv, worker_send) = worker.join().expect("worker panicked");
        [send_probe, worker_recv, worker_send, recv_probe]
    });
    report("spsc", cfg, probes);
}

/// MPSC flavor: two `MpscRing`s at 1p/1c — producers
/// `send_with` (closure fill), the consumer `reserve_slot_with`,
/// both under the [`spin`] policy.
fn run_mpsc(cfg: &Cfg) {
    let mut req_region = MpscRegion([0; size_of::<MpscRegion>()]);
    let mut resp_region = MpscRegion([0; size_of::<MpscRegion>()]);
    let (req_tx, mut req_rx) = MpscRing::init(&mut req_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: MpscRegion is sized/aligned for header + seqs + DEPTH slots
        .split();
    let (resp_tx, mut resp_rx) = MpscRing::init(&mut resp_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: MpscRegion is sized/aligned for header + seqs + DEPTH slots
        .split();

    let probes = std::thread::scope(|s| {
        let worker_cpu = cfg.pin.map(|(_, w)| w);
        let worker = s.spawn(move || {
            if let Some(cpu) = worker_cpu {
                pin_to_cpu(cpu);
            }
            let mut recv_probe = TProbe::new("mpsc worker recv (reserve+release)");
            let mut send_probe = TProbe::new("mpsc worker send (send_with)");
            loop {
                let s = ticks::read_ticks();
                let slot = req_rx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                let v = *slot;
                slot.release();
                let e = ticks::read_ticks();
                if v == STOP {
                    break;
                }
                recv_probe.record(e.wrapping_sub(s));
                let s = ticks::read_ticks();
                resp_tx
                    .send_with::<u64>(spin, |m| *m = v)
                    .expect("spin never gives up");
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            }
            (recv_probe, send_probe)
        });

        if let Some((main_cpu, _)) = cfg.pin {
            pin_to_cpu(main_cpu);
        }
        let mut send_probe = TProbe::new("mpsc main send (send_with)");
        let mut recv_probe = TProbe::new("mpsc main recv (reserve+release)");
        drive(
            cfg.duration,
            &mut send_probe,
            &mut recv_probe,
            |v| {
                req_tx
                    .send_with::<u64>(spin, |m| *m = v)
                    .expect("spin never gives up");
            },
            || {
                let slot = resp_rx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                let v = *slot;
                slot.release();
                v
            },
        );
        req_tx
            .send_with::<u64>(spin, |m| *m = STOP)
            .expect("spin never gives up");
        let (worker_recv, worker_send) = worker.join().expect("worker panicked");
        [send_probe, worker_recv, worker_send, recv_probe]
    });
    report("mpsc", cfg, probes);
}

/// Entry point: parse args, run the requested flavors.
fn main() {
    let cfg = parse_args();
    if cfg.spsc {
        run_spsc(&cfg);
    }
    if cfg.mpsc {
        run_mpsc(&cfg);
    }
}
