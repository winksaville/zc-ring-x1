//! Phase-probed 1p/1c round trip: localize where the SPSC vs
//! MPSC cross-thread latency gap lives.
//!
//! Main sends a counter to a worker over one ring and waits for
//! the echo on a second ring; each protocol phase is measured
//! by its own [`TProbe`] (two `read_ticks` bracket the endpoint
//! call), so the reports separate the sides of the handoff:
//!
//! - `main send` / `worker send` — the producer's reserve +
//!   fill + commit, including any stall acquiring peer-written
//!   cache lines. The ring is never full here (one message in
//!   flight, 8 slots), so no send ever waits for space.
//! - `worker recv` / `main recv` — the consumer's spin wait +
//!   read + release. These absorb the in-flight half trip;
//!   `worker recv` also absorbs main's inter-iteration framing
//!   (probe records, the every-N clock check).
//! - `… recv spin` / `… recv attempts` — the wait inside the
//!   recv phase, decomposed: spin time (first failed attempt →
//!   reserve success) and the attempt count, recorded only for
//!   reserves that actually waited. The zero-spin fraction is
//!   the count difference vs the recv phase probe. Histogram
//!   writes happen after the phase-end tick read; the only
//!   in-phase overhead is one extra tick read when a spin
//!   occurred.
//!
//! The generic machinery (CLI grammar, pinning, drive loop,
//! reporting) is the sibling `tp_runner` crate; this example
//! contributes only the two ring flavors.

use tp_runner::{Cfg, STOP, drive, pin_to_cpu, report, spin, usage_exit};
use tprobe::TProbe;
use tprobe::ticks;
use zc_ring_x1::{CACHE_LINE_SIZE, MpscRing, Ring};

/// The CLI grammar, interpreted by [`Cfg::parse`] plus this
/// example's flavor positionals.
const USAGE: &str =
    "tp_roundtrip [spsc|mpsc|both] [-d secs] [--pin main,worker] [-t] [--decimals n]";

/// Ring slots per direction — a power of two, comfortably above
/// the one message ever in flight.
const DEPTH: u32 = 8;

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

/// The three probes each recv site records into.
struct RecvProbes {
    /// The whole recv phase (spin wait + read + release).
    phase: TProbe,
    /// Wait only: first failed attempt → reserve success;
    /// recorded only when the reserve actually waited.
    spin: TProbe,
    /// Attempt count per waiting reserve (counts probe).
    attempts: TProbe,
}

impl RecvProbes {
    /// Build the three probes for the `side` ("main"/"worker")
    /// of a `flavor` ("spsc"/"mpsc") run.
    fn new(flavor: &str, side: &str) -> Self {
        RecvProbes {
            phase: TProbe::new(&format!("{flavor} {side} recv (reserve+release)")),
            spin: TProbe::new(&format!("{flavor} {side} recv spin (wait only)")),
            attempts: TProbe::new_counts(&format!("{flavor} {side} recv attempts")),
        }
    }
}

/// One instrumented receive: `reserve` performs the endpoint's
/// reserve/read/release under a spin policy that must stamp
/// `spin_start` on its first failed attempt and keep `attempts`
/// current, then returns the received value. Records the phase
/// (and, when a wait happened, spin time + attempts) into
/// `probes` — unless the value is [`STOP`], which passes
/// through unrecorded.
fn instrumented_recv(
    probes: &mut RecvProbes,
    reserve: impl FnOnce(&mut u32, &mut u64) -> u64,
) -> u64 {
    let mut attempts: u32 = 0;
    let mut spin_start: u64 = 0;
    let s = ticks::read_ticks();
    let v = reserve(&mut attempts, &mut spin_start);
    let spin_end = if attempts > 0 { ticks::read_ticks() } else { 0 };
    let e = ticks::read_ticks();
    if v == STOP {
        return v;
    }
    probes.phase.record(e.wrapping_sub(s));
    if attempts > 0 {
        probes.spin.record(spin_end.wrapping_sub(spin_start));
        probes.attempts.record(attempts as u64);
    }
    v
}

/// SPSC flavor: two `Ring`s, both ends `reserve_slot_with`
/// under the [`spin`] policy (recv sites use the instrumented
/// variant).
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
            let mut recv = RecvProbes::new("spsc", "worker");
            let mut send_probe = TProbe::new("spsc worker send (reserve+commit)");
            loop {
                let v = instrumented_recv(&mut recv, |attempts, spin_start| {
                    let slot = req_rx
                        .reserve_slot_with::<u64>(|a| {
                            if a == 0 {
                                *spin_start = ticks::read_ticks();
                            }
                            *attempts = a + 1;
                            core::hint::spin_loop();
                            true
                        })
                        .expect("spin never gives up");
                    let v = *slot;
                    slot.release();
                    v
                });
                if v == STOP {
                    break;
                }
                let s = ticks::read_ticks();
                let mut slot = resp_tx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                *slot = v;
                slot.commit();
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            }
            (recv, send_probe)
        });

        if let Some((main_cpu, _)) = cfg.pin {
            pin_to_cpu(main_cpu);
        }
        let mut send_probe = TProbe::new("spsc main send (reserve+commit)");
        let mut recv = RecvProbes::new("spsc", "main");
        drive(
            cfg.duration,
            |v| {
                let s = ticks::read_ticks();
                let mut slot = req_tx
                    .reserve_slot_with::<u64>(spin)
                    .expect("spin never gives up");
                *slot = v;
                slot.commit();
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            },
            || {
                instrumented_recv(&mut recv, |attempts, spin_start| {
                    let slot = resp_rx
                        .reserve_slot_with::<u64>(|a| {
                            if a == 0 {
                                *spin_start = ticks::read_ticks();
                            }
                            *attempts = a + 1;
                            core::hint::spin_loop();
                            true
                        })
                        .expect("spin never gives up");
                    let v = *slot;
                    slot.release();
                    v
                })
            },
        );
        let mut slot = req_tx
            .reserve_slot_with::<u64>(spin)
            .expect("spin never gives up");
        *slot = STOP;
        slot.commit();
        let (worker_recv, worker_send) = worker.join().expect("worker panicked");
        [
            send_probe,
            worker_recv.phase,
            worker_recv.spin,
            worker_recv.attempts,
            worker_send,
            recv.phase,
            recv.spin,
            recv.attempts,
        ]
    });
    report("spsc", cfg, probes);
}

/// MPSC flavor: two `MpscRing`s at 1p/1c — producers
/// `send_with` (closure fill), the consumer `reserve_slot_with`,
/// both under the [`spin`] policy (recv sites use the
/// instrumented variant).
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
            let mut recv = RecvProbes::new("mpsc", "worker");
            let mut send_probe = TProbe::new("mpsc worker send (send_with)");
            loop {
                let v = instrumented_recv(&mut recv, |attempts, spin_start| {
                    let slot = req_rx
                        .reserve_slot_with::<u64>(|a| {
                            if a == 0 {
                                *spin_start = ticks::read_ticks();
                            }
                            *attempts = a + 1;
                            core::hint::spin_loop();
                            true
                        })
                        .expect("spin never gives up");
                    let v = *slot;
                    slot.release();
                    v
                });
                if v == STOP {
                    break;
                }
                let s = ticks::read_ticks();
                resp_tx
                    .send_with::<u64>(spin, |m| *m = v)
                    .expect("spin never gives up");
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            }
            (recv, send_probe)
        });

        if let Some((main_cpu, _)) = cfg.pin {
            pin_to_cpu(main_cpu);
        }
        let mut send_probe = TProbe::new("mpsc main send (send_with)");
        let mut recv = RecvProbes::new("mpsc", "main");
        drive(
            cfg.duration,
            |v| {
                let s = ticks::read_ticks();
                req_tx
                    .send_with::<u64>(spin, |m| *m = v)
                    .expect("spin never gives up");
                send_probe.record(ticks::read_ticks().wrapping_sub(s));
            },
            || {
                instrumented_recv(&mut recv, |attempts, spin_start| {
                    let slot = resp_rx
                        .reserve_slot_with::<u64>(|a| {
                            if a == 0 {
                                *spin_start = ticks::read_ticks();
                            }
                            *attempts = a + 1;
                            core::hint::spin_loop();
                            true
                        })
                        .expect("spin never gives up");
                    let v = *slot;
                    slot.release();
                    v
                })
            },
        );
        req_tx
            .send_with::<u64>(spin, |m| *m = STOP)
            .expect("spin never gives up");
        let (worker_recv, worker_send) = worker.join().expect("worker panicked");
        [
            send_probe,
            worker_recv.phase,
            worker_recv.spin,
            worker_recv.attempts,
            worker_send,
            recv.phase,
            recv.spin,
            recv.attempts,
        ]
    });
    report("mpsc", cfg, probes);
}

/// Entry point: parse args, resolve flavor positionals, run.
fn main() {
    let cfg = Cfg::parse(USAGE);
    let (mut spsc, mut mpsc) = (true, true);
    for p in &cfg.positionals {
        match p.as_str() {
            "spsc" => (spsc, mpsc) = (true, false),
            "mpsc" => (spsc, mpsc) = (false, true),
            "both" => (spsc, mpsc) = (true, true),
            _ => usage_exit(USAGE),
        }
    }
    if spsc {
        run_spsc(&cfg);
    }
    if mpsc {
        run_mpsc(&cfg);
    }
}
