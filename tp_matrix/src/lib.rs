//! Phase-probed 1p/1c round-trip measurement cells over the
//! zc-ring-x1 primitives.
//!
//! One **cell** is a main -> worker -> main round trip at a
//! given ring flavor and thread placement, driven for a fixed
//! duration. Each protocol phase is measured by its own
//! [`TProbe`] and, on Linux, the cross-core cache-fill counters
//! are collected in-process (no perf(1) needed):
//!
//! - `main send` / `worker send`: the producer's reserve +
//!   fill + commit, including any stall acquiring peer-written
//!   cache lines. The ring is never full here (one message in
//!   flight, at any depth from 1 up), so no send ever waits
//!   for space.
//! - `worker recv` / `main recv`: the consumer's spin wait +
//!   read + release. These absorb the in-flight half trip.
//! - `... recv spin` / `... recv attempts`: the wait inside the
//!   recv phase, decomposed: spin time (first failed attempt ->
//!   reserve success) and the attempt count, recorded only for
//!   reserves that actually waited.
//!
//! A **streaming cell** is the other shape: the producer sends
//! a counter as fast as the ring admits for a fixed duration
//! and the consumer drains it, the depth in play as slack, and
//! the fill counters divided by the messages moved say how
//! many lines crossed per message while streaming, the number
//! the round-trip cell cannot give.
//!
//! The binaries:
//!
//! - `tp-cell` runs one cell and prints the probe reports.
//! - `tp-matrix` runs every flavor × placement cell and emits
//!   markdown tables.
//! - `tp-stream` runs the streaming cell over the same matrix.

use std::time::{Duration, Instant};

use tp_runner::{LINE_BYTES, LineBuf, STOP, drive, pin_to_cpu, spin, unpin_current};
use tprobe::TProbe;
use tprobe::ticks;
use zc_ring_x1::CACHE_LINE_SIZE;

// The runner's line-aligned regions must be aligned the way
// the rings want them.
const _: () = assert!(LINE_BYTES == CACHE_LINE_SIZE);

/// Bytes a v0 region needs: the four-line header, then the
/// slots. v0 exports no size function of its own, its header
/// being a fixed shape.
fn v0_region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<zc_ring_x1::spsc::v0::Header>() as u64 + slot_size as u64 * capacity as u64
}

/// The ring flavor a cell measures, named `xpsc-vN` after its
/// module path, every version included so the A/B is one run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// The SPSC v0 ring (`reserve_slot_with` both ends).
    SpscV0,
    /// The SPSC v1 seam-word ring (same surface, per-slot seq).
    SpscV1,
    /// The SPSC v2 in-slot seq ring (same surface, the seq at
    /// the front of its slot).
    SpscV2,
    /// The MPSC v0 ring at 1p/1c (`send_with` producers).
    MpscV0,
    /// The MPSC v1 equality-seq ring at 1p/1c (same surface,
    /// runs at depth 1).
    MpscV1,
}

/// Every flavor, in report order.
pub const FLAVORS: [Flavor; 5] = [
    Flavor::SpscV0,
    Flavor::SpscV1,
    Flavor::SpscV2,
    Flavor::MpscV0,
    Flavor::MpscV1,
];

impl Flavor {
    /// Lowercase name for labels and CLI parsing.
    pub fn as_str(self) -> &'static str {
        match self {
            Flavor::SpscV0 => "spsc-v0",
            Flavor::SpscV1 => "spsc-v1",
            Flavor::SpscV2 => "spsc-v2",
            Flavor::MpscV0 => "mpsc-v0",
            Flavor::MpscV1 => "mpsc-v1",
        }
    }

    /// The smallest depth the flavor's protocol runs at. The
    /// MPSC v0 ring's committed and released seq values coincide
    /// at capacity 1 and its `init` rejects it, so its cells
    /// start at 2. v1 fixed that and runs from 1.
    pub fn min_depth(self) -> u32 {
        match self {
            Flavor::MpscV0 => 2,
            _ => 1,
        }
    }
}

/// The cache-fill counter totals for one cell (Linux, `None`
/// in [`CellResult`] when unavailable).
pub struct FillCounts {
    /// Demand fills served from another core's cache: the
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
    /// Open + enable all three, returning `None` (with a
    /// one-line note) where perf_event_open is unavailable.
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
    /// Wait only: first failed attempt -> reserve success,
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
/// `probes`, unless the value is [`STOP`], which passes
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
/// `flavor` and ring `depth` with the worker on `pin.1`, and
/// return probes + counters. The caller's thread affinity is
/// left as the cell set it.
pub fn run_cell(
    flavor: Flavor,
    dur: Duration,
    pin: Option<(usize, usize)>,
    depth: u32,
) -> CellResult {
    match pin {
        Some((main_cpu, _)) => pin_to_cpu(main_cpu),
        None => unpin_current(),
    }
    #[cfg(target_os = "linux")]
    let fills = Fills::open();
    let worker = pin.map(|(_, w)| w);
    let probes = match flavor {
        Flavor::SpscV0 => run_spsc_v0(dur, worker, depth),
        Flavor::SpscV1 => run_spsc_v1(dur, worker, depth),
        Flavor::SpscV2 => run_spsc_v2(dur, worker, depth),
        Flavor::MpscV0 => run_mpsc_v0(dur, worker, depth),
        Flavor::MpscV1 => run_mpsc_v1(dur, worker, depth),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    let rts = probes[0].count();
    CellResult { probes, rts, fills }
}

/// Define an SPSC cell body over the ring at `$ring`, its
/// regions sized by `$size(slot_size, depth)`: two rings, both
/// ends `reserve_slot_with` under the [`spin`] policy (recv
/// sites instrumented). The SPSC versions share the endpoint
/// surface and differ by path, so one body serves them all and
/// the A/B measures the protocol alone.
macro_rules! spsc_cell {
    ($name:ident, $ring:path, $size:path, $flavor:expr) => {
        fn $name(dur: Duration, worker_cpu: Option<usize>, depth: u32) -> [TProbe; 8] {
            use $ring as Ring;
            let slot = CACHE_LINE_SIZE as u32;
            let mut req_region = LineBuf::new($size(slot, depth));
            let mut resp_region = LineBuf::new($size(slot, depth));
            let (mut req_tx, mut req_rx) = Ring::init(req_region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
                .split();
            let (mut resp_tx, mut resp_rx) = Ring::init(resp_region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
                .split();

            std::thread::scope(|s| {
                let worker = s.spawn(move || {
                    if let Some(cpu) = worker_cpu {
                        pin_to_cpu(cpu);
                    }
                    let mut recv = RecvProbes::new($flavor, "worker");
                    let mut send_probe = TProbe::new(&format!(
                        "{} worker send (reserve+commit)",
                        $flavor.as_str()
                    ));
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

                let mut send_probe =
                    TProbe::new(&format!("{} main send (reserve+commit)", $flavor.as_str()));
                let mut recv = RecvProbes::new($flavor, "main");
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
    };
}

spsc_cell!(
    run_spsc_v0,
    zc_ring_x1::spsc::v0::Ring,
    v0_region_size,
    Flavor::SpscV0
);
spsc_cell!(
    run_spsc_v1,
    zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size,
    Flavor::SpscV1
);
spsc_cell!(
    run_spsc_v2,
    zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size,
    Flavor::SpscV2
);

/// Define an MPSC cell body over the ring at `$ring`, its
/// regions sized by `$size(slot_size, depth)`: two rings at
/// 1p/1c, producers `send_with` (closure fill), the consumer
/// `reserve_slot_with`, both under the [`spin`] policy (recv
/// sites instrumented). The MPSC versions share the endpoint
/// surface and differ by path, as the SPSC ones do, so one body
/// serves both and the A/B measures the protocol alone.
macro_rules! mpsc_cell {
    ($name:ident, $ring:path, $size:path, $flavor:expr) => {
        fn $name(dur: Duration, worker_cpu: Option<usize>, depth: u32) -> [TProbe; 8] {
            use $ring as MpscRing;
            let slot = CACHE_LINE_SIZE as u32;
            let mut req_region = LineBuf::new($size(slot, depth));
            let mut resp_region = LineBuf::new($size(slot, depth));
            let (req_tx, mut req_rx) = MpscRing::init(req_region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
                .split();
            let (resp_tx, mut resp_rx) = MpscRing::init(resp_region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
                .split();

            std::thread::scope(|s| {
                let worker = s.spawn(move || {
                    if let Some(cpu) = worker_cpu {
                        pin_to_cpu(cpu);
                    }
                    let mut recv = RecvProbes::new($flavor, "worker");
                    let mut send_probe =
                        TProbe::new(&format!("{} worker send (send_with)", $flavor.as_str()));
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

                let mut send_probe =
                    TProbe::new(&format!("{} main send (send_with)", $flavor.as_str()));
                let mut recv = RecvProbes::new($flavor, "main");
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
    };
}

mpsc_cell!(
    run_mpsc_v0,
    zc_ring_x1::mpsc::v0::MpscRing,
    zc_ring_x1::mpsc::v0::mpsc_region_size,
    Flavor::MpscV0
);
mpsc_cell!(
    run_mpsc_v1,
    zc_ring_x1::mpsc::v1::MpscRing,
    zc_ring_x1::mpsc::v1::mpsc_region_size,
    Flavor::MpscV1
);

/// One streaming cell's outcome.
pub struct StreamResult {
    /// Messages the consumer received, the stop sentinel not
    /// counted.
    pub msgs: u64,
    /// Wall-clock seconds from the first send to the consumer
    /// seeing the stop sentinel.
    pub secs: f64,
    /// Fill counters over the whole stream, when the platform
    /// provides them.
    pub fills: Option<FillCounts>,
}

/// Messages between the producer's wall-clock checks, as
/// `drive` spaces its checks.
const STREAM_CHECK_EVERY: u64 = 4096;

/// Run one streaming cell: open the fill counters, stream a
/// counter for `dur` at `flavor` and ring `depth` from a
/// producer thread on `pin.0` to a consumer thread on `pin.1`,
/// and return the count, the elapsed time, and the counters.
///
/// - Both ends are spawned threads that pin themselves, as the
///   demo's streams do, so the calling thread's affinity is
///   untouched and the two sides start alike.
/// - The producer sends as fast as the ring admits under the
///   [`spin`] policy, so the ring sits full whenever the
///   consumer is the slower side, and the consumer asserts the
///   counter's order.
/// - The elapsed time ends when the consumer has seen the stop
///   sentinel, so the last message's drain is inside it.
pub fn run_stream(
    flavor: Flavor,
    dur: Duration,
    pin: Option<(usize, usize)>,
    depth: u32,
) -> StreamResult {
    unpin_current();
    #[cfg(target_os = "linux")]
    let fills = Fills::open();
    let (msgs, secs) = match flavor {
        Flavor::SpscV0 => stream_spsc_v0(dur, pin, depth),
        Flavor::SpscV1 => stream_spsc_v1(dur, pin, depth),
        Flavor::SpscV2 => stream_spsc_v2(dur, pin, depth),
        Flavor::MpscV0 => stream_mpsc_v0(dur, pin, depth),
        Flavor::MpscV1 => stream_mpsc_v1(dur, pin, depth),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    StreamResult { msgs, secs, fills }
}

/// The consumer side of a streaming cell: drain `recv` until
/// the stop sentinel, asserting the counter's order, and
/// return the count received.
fn drain_stream(mut recv: impl FnMut() -> u64) -> u64 {
    let mut expected: u64 = 0;
    loop {
        let v = recv();
        if v == STOP {
            return expected;
        }
        assert_eq!(v, expected, "stream order broken");
        expected += 1;
    }
}

/// Define an SPSC streaming cell body over the ring at `$ring`,
/// its region sized by `$size(slot_size, depth)`: one ring,
/// the producer spawned on `pin.0`, the consumer on `pin.1`.
/// Returns the messages moved and the seconds.
macro_rules! spsc_stream {
    ($name:ident, $ring:path, $size:path) => {
        fn $name(dur: Duration, pin: Option<(usize, usize)>, depth: u32) -> (u64, f64) {
            use $ring as Ring;
            let slot = CACHE_LINE_SIZE as u32;
            let mut region = LineBuf::new($size(slot, depth));
            let (mut tx, mut rx) = Ring::init(region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
                .split();
            let start = Instant::now();
            let msgs = std::thread::scope(|s| {
                s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    let mut counter: u64 = 0;
                    loop {
                        for _ in 0..STREAM_CHECK_EVERY {
                            let mut slot = tx
                                .reserve_slot_with::<u64>(zc_ring_x1::policy::spin)
                                .expect("spin never gives up");
                            *slot = counter;
                            slot.commit();
                            counter += 1;
                        }
                        if start.elapsed() >= dur {
                            break;
                        }
                    }
                    let mut slot = tx
                        .reserve_slot_with::<u64>(zc_ring_x1::policy::spin)
                        .expect("spin never gives up");
                    *slot = STOP;
                    slot.commit();
                });
                let consumer = s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    drain_stream(|| {
                        let slot = rx
                            .reserve_slot_with::<u64>(zc_ring_x1::policy::spin)
                            .expect("spin never gives up");
                        let v = *slot;
                        slot.release();
                        v
                    })
                });
                consumer.join().expect("consumer panicked")
            });
            (msgs, start.elapsed().as_secs_f64())
        }
    };
}

spsc_stream!(stream_spsc_v0, zc_ring_x1::spsc::v0::Ring, v0_region_size);
spsc_stream!(
    stream_spsc_v1,
    zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size
);
spsc_stream!(
    stream_spsc_v2,
    zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size
);

/// Define an MPSC streaming cell body over the ring at `$ring`,
/// its region sized by `$size(slot_size, depth)`: one ring at
/// 1p/1c, the producer `send_with` spawned on `pin.0`, the
/// consumer on `pin.1`. Returns the messages moved and the
/// seconds.
macro_rules! mpsc_stream {
    ($name:ident, $ring:path, $size:path) => {
        fn $name(dur: Duration, pin: Option<(usize, usize)>, depth: u32) -> (u64, f64) {
            use $ring as MpscRing;
            let slot = CACHE_LINE_SIZE as u32;
            let mut region = LineBuf::new($size(slot, depth));
            let (tx, mut rx) = MpscRing::init(region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
                .split();
            let start = Instant::now();
            let msgs = std::thread::scope(|s| {
                s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    let mut counter: u64 = 0;
                    loop {
                        for _ in 0..STREAM_CHECK_EVERY {
                            tx.send_with::<u64>(zc_ring_x1::policy::spin, |m| *m = counter)
                                .expect("spin never gives up");
                            counter += 1;
                        }
                        if start.elapsed() >= dur {
                            break;
                        }
                    }
                    tx.send_with::<u64>(zc_ring_x1::policy::spin, |m| *m = STOP)
                        .expect("spin never gives up");
                });
                let consumer = s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    drain_stream(|| {
                        let slot = rx
                            .reserve_slot_with::<u64>(zc_ring_x1::policy::spin)
                            .expect("spin never gives up");
                        let v = *slot;
                        slot.release();
                        v
                    })
                });
                consumer.join().expect("consumer panicked")
            });
            (msgs, start.elapsed().as_secs_f64())
        }
    };
}

mpsc_stream!(
    stream_mpsc_v0,
    zc_ring_x1::mpsc::v0::MpscRing,
    zc_ring_x1::mpsc::v0::mpsc_region_size
);
mpsc_stream!(
    stream_mpsc_v1,
    zc_ring_x1::mpsc::v1::MpscRing,
    zc_ring_x1::mpsc::v1::mpsc_region_size
);
