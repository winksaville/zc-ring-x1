//! Phase-probed 1p/1c round-trip measurement cells over the
//! zc-ring-x1 primitives.
//!
//! One **cell** is a main → worker → main round trip at a given
//! ring flavor and thread placement, driven for a fixed
//! duration; each protocol phase is measured by its own
//! [`TProbe`] and, on Linux, the cross-core cache-fill counters
//! are collected in-process (no perf(1) needed):
//!
//! - `main send` / `worker send` — the producer's reserve +
//!   fill + commit, including any stall acquiring peer-written
//!   cache lines. The ring is never full here (one message in
//!   flight, 8 slots), so no send ever waits for space.
//! - `worker recv` / `main recv` — the consumer's spin wait +
//!   read + release; these absorb the in-flight half trip.
//! - `… recv spin` / `… recv attempts` — the wait inside the
//!   recv phase, decomposed: spin time (first failed attempt →
//!   reserve success) and the attempt count, recorded only for
//!   reserves that actually waited.
//!
//! The binaries: `tp-cell` runs one cell and prints the probe
//! reports; `tp-matrix` runs every flavor × placement cell and
//! emits markdown tables.

use std::time::Duration;

use tp_runner::{STOP, drive, pin_to_cpu, spin, unpin_current};
use tprobe::TProbe;
use tprobe::ticks;
use zc_ring_x1::{CACHE_LINE_SIZE, MpscRing, Ring};

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

/// The ring flavor a cell measures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// The SPSC ring (`reserve_slot_with` both ends).
    Spsc,
    /// The MPSC ring at 1p/1c (`send_with` producers).
    Mpsc,
}

impl Flavor {
    /// Lowercase name for labels and CLI parsing.
    pub fn as_str(self) -> &'static str {
        match self {
            Flavor::Spsc => "spsc",
            Flavor::Mpsc => "mpsc",
        }
    }
}

/// The cache-fill counter totals for one cell (Linux; `None`
/// in [`CellResult`] when unavailable).
pub struct FillCounts {
    /// Demand fills served from another core's cache — the
    /// cross-core line-transfer signal.
    pub lcl_cache: u64,
    /// Demand fills served from the core's own L2.
    pub lcl_l2: u64,
    /// Demand fills served from local DRAM.
    pub lcl_dram: u64,
}

/// One cell's outcome: the eight probes in trip order and the
/// fill counters.
pub struct CellResult {
    /// Trip order: main send, worker recv, worker recv spin,
    /// worker recv attempts, worker send, main recv, main recv
    /// spin, main recv attempts.
    pub probes: [TProbe; 8],
    /// Round trips completed (== every probe's phase count).
    pub rts: u64,
    /// Fill counters, when the platform provides them.
    pub fills: Option<FillCounts>,
}

/// The three per-cell fill counters, opened before the worker
/// spawns so `inherit` covers it.
#[cfg(target_os = "linux")]
struct Fills {
    lcl_cache: tp_runner::perf::ProcessCounter,
    lcl_l2: tp_runner::perf::ProcessCounter,
    lcl_dram: tp_runner::perf::ProcessCounter,
}

#[cfg(target_os = "linux")]
impl Fills {
    /// Open + enable all three; `None` (with a one-line note)
    /// where perf_event_open is unavailable.
    fn open() -> Option<Fills> {
        use tp_runner::perf::{
            ProcessCounter, ZEN2_FILLS_LCL_CACHE, ZEN2_FILLS_LCL_DRAM, ZEN2_FILLS_LCL_L2,
        };
        let open = |config| ProcessCounter::new_raw(config);
        match (
            open(ZEN2_FILLS_LCL_CACHE),
            open(ZEN2_FILLS_LCL_L2),
            open(ZEN2_FILLS_LCL_DRAM),
        ) {
            (Ok(mut lcl_cache), Ok(mut lcl_l2), Ok(mut lcl_dram)) => {
                lcl_cache.enable().ok()?;
                lcl_l2.enable().ok()?;
                lcl_dram.enable().ok()?;
                Some(Fills {
                    lcl_cache,
                    lcl_l2,
                    lcl_dram,
                })
            }
            (r, _, _) => {
                if let Err(e) = r {
                    eprintln!("note: fill counters unavailable ({e}); fills/RT will be absent");
                }
                None
            }
        }
    }

    /// Disable and read the totals.
    fn finish(mut self) -> Option<FillCounts> {
        self.lcl_cache.disable().ok()?;
        self.lcl_l2.disable().ok()?;
        self.lcl_dram.disable().ok()?;
        Some(FillCounts {
            lcl_cache: self.lcl_cache.read().ok()?,
            lcl_l2: self.lcl_l2.read().ok()?,
            lcl_dram: self.lcl_dram.read().ok()?,
        })
    }
}

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
    /// of a `flavor` run.
    fn new(flavor: Flavor, side: &str) -> Self {
        let flavor = flavor.as_str();
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

/// Run one measurement cell: pin (or unpin) the calling thread,
/// open the fill counters, drive `dur` worth of round trips at
/// `flavor` with the worker on `pin.1`, and return probes +
/// counters. The caller's thread affinity is left as the cell
/// set it.
pub fn run_cell(flavor: Flavor, dur: Duration, pin: Option<(usize, usize)>) -> CellResult {
    match pin {
        Some((main_cpu, _)) => pin_to_cpu(main_cpu),
        None => unpin_current(),
    }
    #[cfg(target_os = "linux")]
    let fills = Fills::open();
    let probes = match flavor {
        Flavor::Spsc => run_spsc(dur, pin.map(|(_, w)| w)),
        Flavor::Mpsc => run_mpsc(dur, pin.map(|(_, w)| w)),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    let rts = probes[0].count();
    CellResult { probes, rts, fills }
}

/// SPSC cell body: two `Ring`s, both ends `reserve_slot_with`
/// under the [`spin`] policy (recv sites instrumented).
fn run_spsc(dur: Duration, worker_cpu: Option<usize>) -> [TProbe; 8] {
    let mut req_region = Region([0; size_of::<Region>()]);
    let mut resp_region = Region([0; size_of::<Region>()]);
    let (mut req_tx, mut req_rx) = Ring::init(&mut req_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the header + DEPTH slots
        .split();
    let (mut resp_tx, mut resp_rx) = Ring::init(&mut resp_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the header + DEPTH slots
        .split();

    std::thread::scope(|s| {
        let worker = s.spawn(move || {
            if let Some(cpu) = worker_cpu {
                pin_to_cpu(cpu);
            }
            let mut recv = RecvProbes::new(Flavor::Spsc, "worker");
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

        let mut send_probe = TProbe::new("spsc main send (reserve+commit)");
        let mut recv = RecvProbes::new(Flavor::Spsc, "main");
        drive(
            dur,
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
    })
}

/// MPSC cell body: two `MpscRing`s at 1p/1c — producers
/// `send_with` (closure fill), the consumer `reserve_slot_with`,
/// both under the [`spin`] policy (recv sites instrumented).
fn run_mpsc(dur: Duration, worker_cpu: Option<usize>) -> [TProbe; 8] {
    let mut req_region = MpscRegion([0; size_of::<MpscRegion>()]);
    let mut resp_region = MpscRegion([0; size_of::<MpscRegion>()]);
    let (req_tx, mut req_rx) = MpscRing::init(&mut req_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: MpscRegion is sized/aligned for header + seqs + DEPTH slots
        .split();
    let (resp_tx, mut resp_rx) = MpscRing::init(&mut resp_region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: MpscRegion is sized/aligned for header + seqs + DEPTH slots
        .split();

    std::thread::scope(|s| {
        let worker = s.spawn(move || {
            if let Some(cpu) = worker_cpu {
                pin_to_cpu(cpu);
            }
            let mut recv = RecvProbes::new(Flavor::Mpsc, "worker");
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

        let mut send_probe = TProbe::new("mpsc main send (send_with)");
        let mut recv = RecvProbes::new(Flavor::Mpsc, "main");
        drive(
            dur,
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
    })
}
