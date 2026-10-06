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
//! - `tp-pool` runs the pool-message loop, the messaging
//!   layer's shape, over the descriptor rings and cordyceps's
//!   intrusive queue on one pool ([`pool`]).

pub mod pool;

use std::time::{Duration, Instant};

use tp_runner::{LINE_BYTES, LineBuf, STOP, drive, pin_to_cpu, spin, unpin_current};
use tprobe::TProbe;
use tprobe::ticks;
use zc_ring_x1::CACHE_LINE_SIZE;

/// The `xfills` legend entry's meaning, shared by every tool
/// that prints the column.
pub const XFILLS_MEANING: &str = "x-core cache-line fills: cache lines pulled into a core \
     from another core's cache, near 0 when the threads share a core's caches, as SMT \
     siblings do";

/// The `placement` legend entry's meaning, shared by the tools
/// that sweep placements.
pub const PLACEMENT_MEANING: &str = "the CPUs the two threads are pinned to and how they \
     share caches: CCX two cores on one L3, x-CCX cores on different L3s, SMT one core's two \
     cpus sharing its L1 and L2, or unpinned";

/// The narrowest a legend wraps to, so a narrow table's legend
/// stays readable.
const LEGEND_MIN_WIDTH: usize = 60;

/// The widest a legend wraps to, so a wide table's legend reads
/// as prose.
const LEGEND_MAX_WIDTH: usize = 80;

/// Print a column legend, one markdown list item per column,
/// `- `name`: meaning`, each wrapped at word boundaries to
/// `width`, clamped to [`LEGEND_MIN_WIDTH`] and
/// [`LEGEND_MAX_WIDTH`], with continuation
/// lines indented two spaces so the item stays one list item.
pub fn print_legend(width: usize, entries: &[(&str, &str)]) {
    let width = width.clamp(LEGEND_MIN_WIDTH, LEGEND_MAX_WIDTH);
    for (name, meaning) in entries {
        let mut line = format!("- `{name}`:");
        for word in meaning.split_whitespace() {
            if line.len() + 1 + word.len() > width {
                println!("{line}");
                line = format!("  {word}");
            } else {
                line.push(' ');
                line.push_str(word);
            }
        }
        println!("{line}");
    }
}

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
    /// The SPSC v3 ring of segments over a pool (same surface,
    /// the depth each segment's, the segment count a knob).
    SpscV3,
    /// The SPSC v4 ring, v3's segments with a control block in
    /// the region and offsets for its table, so it can be
    /// attached (same surface, roles taken by name).
    SpscV4,
    /// The MPSC v0 ring at 1p/1c (`send_with` producers).
    MpscV0,
    /// The MPSC v1 equality-seq ring at 1p/1c (same surface,
    /// runs at depth 1).
    MpscV1,
    /// The MPSC v2 ring of segments over a pool at 1p/1c (same
    /// surface, the depth each segment's, the segment count a
    /// knob).
    MpscV2,
    /// The MPSC v3 ring, v2's segments attachable with counted
    /// roles, in its `Multi` mode (same surface, the segment count
    /// a knob, the roles claimed).
    MpscV3,
    /// The MPSC v3 ring in its `Single` mode: one segment, the
    /// segment count ignored.
    MpscV3Single,
    /// The MPSC v3 ring in its `Multi` mode waking with a futex
    /// (on Linux, else as `mpsc-v3`), so the consumer's wake checks
    /// are on its path though nothing sleeps.
    MpscV3Futex,
    /// The MPSC v3 ring in its `Multi` mode, each producer backing
    /// off after a lost claim race by `policy::backoff`.
    MpscV3Backoff,
    /// The MPSC v4 ring, v3 with a consumer that receives by `recv`, as a producer sends, in its
    /// `Multi` mode over `SpinOnly` (same surface through an adapter, the segment count a knob).
    MpscV4,
    /// The MPSC v4 ring in its `Single` mode: one segment, the segment count ignored.
    MpscV4Single,
    /// The MPSC v4 ring in its `Multi` mode over `SpinOrSleep` of a futex (on Linux, else as
    /// `mpsc-v4`), every endpoint spinning, so the wake's checks are on the paths though nothing
    /// sleeps.
    MpscV4Futex,
    /// The MPSC v4 ring in its `Multi` mode, each producer backing off after a lost claim race by
    /// `policy::backoff`.
    MpscV4Backoff,
}

/// Every flavor, in report order.
pub const FLAVORS: [Flavor; 16] = [
    Flavor::SpscV0,
    Flavor::SpscV1,
    Flavor::SpscV2,
    Flavor::SpscV3,
    Flavor::SpscV4,
    Flavor::MpscV0,
    Flavor::MpscV1,
    Flavor::MpscV2,
    Flavor::MpscV3,
    Flavor::MpscV3Single,
    Flavor::MpscV3Futex,
    Flavor::MpscV3Backoff,
    Flavor::MpscV4,
    Flavor::MpscV4Single,
    Flavor::MpscV4Futex,
    Flavor::MpscV4Backoff,
];

impl Flavor {
    /// Lowercase name for labels and CLI parsing.
    pub fn as_str(self) -> &'static str {
        match self {
            Flavor::SpscV0 => "spsc-v0",
            Flavor::SpscV1 => "spsc-v1",
            Flavor::SpscV2 => "spsc-v2",
            Flavor::SpscV3 => "spsc-v3",
            Flavor::SpscV4 => "spsc-v4",
            Flavor::MpscV0 => "mpsc-v0",
            Flavor::MpscV1 => "mpsc-v1",
            Flavor::MpscV2 => "mpsc-v2",
            Flavor::MpscV3 => "mpsc-v3",
            Flavor::MpscV3Single => "mpsc-v3-single",
            Flavor::MpscV3Futex => "mpsc-v3-futex",
            Flavor::MpscV3Backoff => "mpsc-v3-backoff",
            Flavor::MpscV4 => "mpsc-v4",
            Flavor::MpscV4Single => "mpsc-v4-single",
            Flavor::MpscV4Futex => "mpsc-v4-futex",
            Flavor::MpscV4Backoff => "mpsc-v4-backoff",
        }
    }

    /// Whether the flavor is an MPSC ring, the flavors a stream
    /// with several producers runs.
    pub fn is_mpsc(self) -> bool {
        matches!(
            self,
            Flavor::MpscV0
                | Flavor::MpscV1
                | Flavor::MpscV2
                | Flavor::MpscV3
                | Flavor::MpscV3Single
                | Flavor::MpscV3Futex
                | Flavor::MpscV3Backoff
                | Flavor::MpscV4
                | Flavor::MpscV4Single
                | Flavor::MpscV4Futex
                | Flavor::MpscV4Backoff
        )
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

    /// Why a depth below [`Flavor::min_depth`] is skipped, for
    /// the skip lines.
    pub fn floor_note(self) -> String {
        match self {
            Flavor::MpscV0 => "mpsc-v0 needs depth >= 2, mpsc-v1 runs depth 1".to_string(),
            _ => format!("{} needs depth >= {}", self.as_str(), self.min_depth()),
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
    /// Segment switches across both rings, for the segmented
    /// flavor, `None` for the others.
    pub switches: Option<u64>,
}

/// The three per-cell fill counters, opened before the worker
/// spawns so `inherit` covers it.
#[cfg(target_os = "linux")]
pub(crate) struct Fills {
    lcl_cache: tp_runner::perf::ProcessCounter,
    lcl_l2: tp_runner::perf::ProcessCounter,
    lcl_dram: tp_runner::perf::ProcessCounter,
}

#[cfg(target_os = "linux")]
impl Fills {
    /// Open + enable all three, returning `None` (with a
    /// one-line note) where perf_event_open is unavailable.
    pub(crate) fn open() -> Option<Fills> {
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
                    eprintln!("note: fill counters unavailable ({e}); xfills will read -");
                }
                None
            }
        }
    }

    /// Disable and read the totals.
    pub(crate) fn finish(mut self) -> Option<FillCounts> {
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
/// left as the cell set it. `segments` is the segmented flavor's
/// count per ring, ignored by the others.
pub fn run_cell(
    flavor: Flavor,
    dur: Duration,
    pin: Option<(usize, usize)>,
    depth: u32,
    segments: u32,
) -> CellResult {
    match pin {
        Some((main_cpu, _)) => pin_to_cpu(main_cpu),
        None => unpin_current(),
    }
    #[cfg(target_os = "linux")]
    let fills = Fills::open();
    let worker = pin.map(|(_, w)| w);
    let (probes, switches) = match flavor {
        Flavor::SpscV0 => run_spsc_v0(dur, worker, depth, segments),
        Flavor::SpscV1 => run_spsc_v1(dur, worker, depth, segments),
        Flavor::SpscV2 => run_spsc_v2(dur, worker, depth, segments),
        Flavor::SpscV3 => run_spsc_v3(dur, worker, depth, segments),
        Flavor::SpscV4 => run_spsc_v4(dur, worker, depth, segments),
        Flavor::MpscV0 => run_mpsc_v0(dur, worker, depth, segments),
        Flavor::MpscV1 => run_mpsc_v1(dur, worker, depth, segments),
        Flavor::MpscV2 => run_mpsc_v2(dur, worker, depth, segments),
        Flavor::MpscV3 => run_mpsc_v3(dur, worker, depth, segments),
        Flavor::MpscV3Single => run_mpsc_v3_single(dur, worker, depth, segments),
        Flavor::MpscV3Futex => run_mpsc_v3_futex(dur, worker, depth, segments),
        Flavor::MpscV3Backoff => run_mpsc_v3_backoff(dur, worker, depth, segments),
        Flavor::MpscV4 => run_mpsc_v4(dur, worker, depth, segments),
        Flavor::MpscV4Single => run_mpsc_v4_single(dur, worker, depth, segments),
        Flavor::MpscV4Futex => run_mpsc_v4_futex(dur, worker, depth, segments),
        Flavor::MpscV4Backoff => run_mpsc_v4_backoff(dur, worker, depth, segments),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    let rts = probes[0].count();
    CellResult {
        probes,
        rts,
        fills,
        switches,
    }
}

/// A producer's segment switches, for the flavors that have
/// segments: `None` from a single-region ring.
trait SegmentSwitches {
    /// Switches so far, or `None` when the ring has no segments.
    fn segment_switches(&self) -> Option<u64>;
}

impl SegmentSwitches for zc_ring_x1::spsc::v0::Producer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        None
    }
}

impl SegmentSwitches for zc_ring_x1::spsc::v1::Producer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        None
    }
}

impl SegmentSwitches for zc_ring_x1::spsc::v2::Producer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        None
    }
}

impl SegmentSwitches for zc_ring_x1::spsc::v3::Producer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        Some(self.switches())
    }
}

impl SegmentSwitches for zc_ring_x1::spsc::v4::Producer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        Some(self.switches())
    }
}

impl SegmentSwitches for zc_ring_x1::mpsc::v0::MpscProducer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        None
    }
}

impl SegmentSwitches for zc_ring_x1::mpsc::v1::MpscProducer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        None
    }
}

impl SegmentSwitches for zc_ring_x1::mpsc::v2::MpscProducer<'_> {
    fn segment_switches(&self) -> Option<u64> {
        Some(self.switches())
    }
}

impl<M: zc_ring_x1::mpsc::v3::Mode, W: zc_ring_x1::wake::Wake> SegmentSwitches
    for zc_ring_x1::mpsc::v3::MpscProducer<'_, M, W>
{
    fn segment_switches(&self) -> Option<u64> {
        M::MULTI.then(|| self.switches())
    }
}

/// `struct V3Send` is an MPSC v3 producer with a `send_with`, which the cell and stream bodies
/// shared with v0 to v2 call, forwarding to v3's `send`, whose policy a closure is.
struct V3Send<P>(P);

impl<M: zc_ring_x1::mpsc::v3::Mode, W: zc_ring_x1::wake::Wake>
    V3Send<zc_ring_x1::mpsc::v3::MpscProducer<'_, M, W>>
{
    /// `send_with` is v3's `send` with `on_full` as its policy.
    #[inline]
    fn send_with<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        write_msg: impl FnOnce(&mut T),
    ) -> Result<(), zc_ring_x1::Full>
    where
        T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout,
    {
        self.0.send(on_full, write_msg)
    }
}

impl<P: SegmentSwitches> SegmentSwitches for V3Send<P> {
    fn segment_switches(&self) -> Option<u64> {
        self.0.segment_switches()
    }
}

/// `struct Backoff` is an MPSC v3 producer whose `send_with` backs off after each lost slot, so
/// the cell and stream bodies run it unchanged.
struct Backoff<P>(P);

/// `struct BackoffPolicy` is `on_full` for a full ring and
/// [`policy::backoff`](zc_ring_x1::policy::backoff) for each lost slot.
struct BackoffPolicy<F>(F);

impl<F: FnMut(u32) -> bool> zc_ring_x1::mpsc::v3::SendPolicy for BackoffPolicy<F> {
    #[inline]
    fn on_full(&mut self, attempt: u32, _room: &zc_ring_x1::mpsc::v3::Room<'_>) -> bool {
        (self.0)(attempt)
    }

    #[inline]
    fn on_lost(&mut self, lost: u32) {
        zc_ring_x1::policy::backoff(lost)
    }
}

impl<M: zc_ring_x1::mpsc::v3::Mode, W: zc_ring_x1::wake::Wake>
    Backoff<zc_ring_x1::mpsc::v3::MpscProducer<'_, M, W>>
{
    /// `send_with` is v3's `send` with [`BackoffPolicy`].
    #[inline]
    fn send_with<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        write_msg: impl FnOnce(&mut T),
    ) -> Result<(), zc_ring_x1::Full>
    where
        T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout,
    {
        self.0.send(BackoffPolicy(on_full), write_msg)
    }
}

impl<P: SegmentSwitches> SegmentSwitches for Backoff<P> {
    fn segment_switches(&self) -> Option<u64> {
        self.0.segment_switches()
    }
}

/// The wake the `mpsc-v3-futex` flavor measures: a futex on Linux,
/// where it exists.
#[cfg(target_os = "linux")]
type V3Futex = zc_ring_x1::wake::Futex<10>;

/// The wake the `mpsc-v3-futex` flavor measures: none off Linux,
/// so the flavor runs as `mpsc-v3`.
#[cfg(not(target_os = "linux"))]
type V3Futex = zc_ring_x1::wake::NoWake;

impl<M: zc_ring_x1::mpsc::v4::Mode, W: zc_ring_x1::wake::Waits> SegmentSwitches
    for zc_ring_x1::mpsc::v4::MpscProducer<'_, M, W>
{
    fn segment_switches(&self) -> Option<u64> {
        M::MULTI.then(|| self.switches())
    }
}

/// `struct V4Send` is an MPSC v4 producer with a `send_with`, as [`V3Send`] is v3's, forwarding to
/// v4's `send`, whose policy a closure is.
struct V4Send<P>(P);

impl<M: zc_ring_x1::mpsc::v4::Mode, W: zc_ring_x1::wake::Waits>
    V4Send<zc_ring_x1::mpsc::v4::MpscProducer<'_, M, W>>
{
    /// `send_with` is v4's `send` with `on_full` as its policy.
    #[inline]
    fn send_with<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        write_msg: impl FnOnce(&mut T),
    ) -> Result<(), zc_ring_x1::Full>
    where
        T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout,
    {
        self.0.send(on_full, write_msg)
    }
}

impl<P: SegmentSwitches> SegmentSwitches for V4Send<P> {
    fn segment_switches(&self) -> Option<u64> {
        self.0.segment_switches()
    }
}

/// `struct V4Backoff` is an MPSC v4 producer whose `send_with` backs off after each lost slot, as
/// [`Backoff`] is v3's.
struct V4Backoff<P>(P);

/// `struct V4BackoffPolicy` is `on_full` for a full ring and
/// [`policy::backoff`](zc_ring_x1::policy::backoff) for each lost slot.
struct V4BackoffPolicy<F>(F);

impl<F: FnMut(u32) -> bool> zc_ring_x1::mpsc::v4::WaitPolicy for V4BackoffPolicy<F> {
    #[inline]
    fn on_wait(&mut self, attempt: u32, _waiter: &zc_ring_x1::mpsc::v4::Waiter<'_>) -> bool {
        (self.0)(attempt)
    }

    #[inline]
    fn on_lost(&mut self, lost: u32) {
        zc_ring_x1::policy::backoff(lost)
    }
}

impl<M: zc_ring_x1::mpsc::v4::Mode, W: zc_ring_x1::wake::Waits>
    V4Backoff<zc_ring_x1::mpsc::v4::MpscProducer<'_, M, W>>
{
    /// `send_with` is v4's `send` with [`V4BackoffPolicy`].
    #[inline]
    fn send_with<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        write_msg: impl FnOnce(&mut T),
    ) -> Result<(), zc_ring_x1::Full>
    where
        T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout,
    {
        self.0.send(V4BackoffPolicy(on_full), write_msg)
    }
}

impl<P: SegmentSwitches> SegmentSwitches for V4Backoff<P> {
    fn segment_switches(&self) -> Option<u64> {
        self.0.segment_switches()
    }
}

/// `struct V4Recv` is an MPSC v4 consumer with a `reserve_slot_with`, which the cell and stream
/// bodies shared with v0 to v3 call. v4 has no read guard: its `recv` reads the message in a
/// closure and frees the slot when the closure returns, so `reserve_slot_with` copies the value
/// out and hands it back in a [`V4Slot`].
struct V4Recv<C>(C);

/// `struct V4Slot` is the value a [`V4Recv`] read, standing in for the read guard of v0 to v3. Its
/// slot is already free, so `release` does nothing.
struct V4Slot<T>(T);

impl<T> core::ops::Deref for V4Slot<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> V4Slot<T> {
    /// `release` is the guard's `release`, with nothing left to do.
    #[inline]
    fn release(self) {}
}

impl<M: zc_ring_x1::mpsc::v4::Mode, W: zc_ring_x1::wake::Waits>
    V4Recv<zc_ring_x1::mpsc::v4::MpscConsumer<'_, M, W>>
{
    /// `reserve_slot_with` is v4's `recv` with `on_empty` as its policy and a copy for its read.
    #[inline]
    fn reserve_slot_with<T>(
        &mut self,
        on_empty: impl FnMut(u32) -> bool,
    ) -> Result<V4Slot<T>, zc_ring_x1::Empty>
    where
        T: Copy + zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable,
    {
        self.0.recv(on_empty, |m: &T| V4Slot(*m))
    }
}

/// The wait the `mpsc-v4-futex` flavor measures: spin or sleep on a futex on Linux, where it
/// exists, with `mpsc-v3-futex`'s timeout.
#[cfg(target_os = "linux")]
type V4Futex = zc_ring_x1::wake::SpinOrSleep<zc_ring_x1::wake::Futex<10>>;

/// The wait the `mpsc-v4-futex` flavor measures: spin only off Linux, so the flavor runs as
/// `mpsc-v4`.
#[cfg(not(target_os = "linux"))]
type V4Futex = zc_ring_x1::wake::SpinOnly;

/// Bind one SPSC ring's `$tx` and `$rx` endpoints, one-line
/// slots at `$depth`, the storage held in `$store` (and, for a
/// ring of segments, the pool in `$pool`) so it outlives them.
///
/// - `single $ring, $size`: a ring over one region sized by
///   `$size(slot_size, depth)`, `$segments` unused.
/// - `segmented $ring, $size`: a ring of `$segments` segments,
///   each `$depth` slots, over a pool holding exactly those
///   segments, `$size` its `segment_size`, the endpoints by
///   `split` (v3).
/// - `segmented_roles $ring, $size`: the same over a ring whose
///   endpoints are taken by name, `producer` and `consumer` (v4).
macro_rules! spsc_pair {
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     single $ring:path, $size:path) => {
        let _ = $segments;
        let slot = CACHE_LINE_SIZE as u32;
        let mut $store = LineBuf::new($size(slot, $depth));
        let (mut $tx, mut $rx) = <$ring>::init($store.as_mut_bytes(), slot, $depth)
            .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
            .split();
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     segmented $ring:path, $size:path) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = $size(slot, $depth);
        let mut $store = LineBuf::new(
            size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * $segments as u64,
        );
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, $segments)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let (mut $tx, mut $rx) = <$ring>::init(&mut $pool, slot, $depth, $segments)
            .unwrap() // OK: the pool holds exactly the segments, sized by segment_size
            .split();
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     segmented_roles $ring:path, $size:path) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = $size(slot, $depth);
        let mut $store = LineBuf::new(
            size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * $segments as u64,
        );
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, $segments)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ring = <$ring>::init(&mut $pool, slot, $depth, $segments)
            .unwrap(); // OK: the pool holds exactly the segments, sized by segment_size
        let mut $tx = ring.claim_producer(1).unwrap(); // OK: a fresh ring, no role held
        let mut $rx = ring.claim_consumer(2).unwrap(); // OK: a fresh ring, no role held
    };
}

/// Define an SPSC cell body over the ring pair `$pair` builds (see
/// `spsc_pair`): two rings, both ends `reserve_slot_with` under
/// the [`spin`] policy (recv sites instrumented). The SPSC
/// versions share the endpoint surface and differ by path, so one
/// body serves them all and the A/B measures the protocol alone.
macro_rules! spsc_cell {
    ($name:ident, $flavor:expr, $($pair:tt)+) => {
        fn $name(
            dur: Duration,
            worker_cpu: Option<usize>,
            depth: u32,
            segments: u32,
        ) -> ([TProbe; 8], Option<u64>) {
            spsc_pair!(req_tx, req_rx, req_store, req_pool, depth, segments, $($pair)+);
            spsc_pair!(resp_tx, resp_rx, resp_store, resp_pool, depth, segments, $($pair)+);

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
                    (recv, send_probe, resp_tx.segment_switches())
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
                let (worker_recv, worker_send, resp_switches) =
                    worker.join().expect("worker panicked");
                let switches = req_tx
                    .segment_switches()
                    .zip(resp_switches)
                    .map(|(req, resp)| req + resp);
                (
                    [
                        send_probe,
                        worker_recv.phase,
                        worker_recv.spin,
                        worker_recv.attempts,
                        worker_send,
                        recv.phase,
                        recv.spin,
                        recv.attempts,
                    ],
                    switches,
                )
            })
        }
    };
}

spsc_cell!(
    run_spsc_v0,
    Flavor::SpscV0,
    single zc_ring_x1::spsc::v0::Ring,
    v0_region_size
);
spsc_cell!(
    run_spsc_v1,
    Flavor::SpscV1,
    single zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size
);
spsc_cell!(
    run_spsc_v2,
    Flavor::SpscV2,
    single zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size
);
spsc_cell!(
    run_spsc_v3,
    Flavor::SpscV3,
    segmented zc_ring_x1::spsc::v3::Ring,
    zc_ring_x1::spsc::v3::segment_size
);
spsc_cell!(
    run_spsc_v4,
    Flavor::SpscV4,
    segmented_roles zc_ring_x1::spsc::v4::Ring,
    zc_ring_x1::spsc::v4::segment_size
);

/// Bind one MPSC ring's `$tx` and `$rx` endpoints, one-line
/// slots at `$depth`, the storage held in `$store` (and, for a
/// ring of segments, the pool in `$pool`) so it outlives them.
///
/// - `single $ring, $size`: a ring over one region sized by
///   `$size(slot_size, depth)`, `$segments` unused.
/// - `segmented`: a v2 ring of `$segments` segments, each
///   `$depth` slots, over a pool holding exactly those segments.
/// - `v3 $mode, $wake`: a v3 ring of that mode and wake, `$segments`
///   segments for `Multi` and one for `Single`, over a pool holding
///   exactly those, the roles claimed.
/// - `v3_backoff`: the `v3` `Multi` ring with no wake, its producer a [`Backoff`].
/// - `v4 $mode, $wait`: a v4 ring of that mode and wait, sized as `v3`'s, the roles taken, the
///   consumer a [`V4Recv`].
/// - `v4_backoff`: the `v4` `Multi` ring over `SpinOnly`, its producer a [`V4Backoff`].
macro_rules! mpsc_pair {
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     single $ring:path, $size:path) => {
        let _ = $segments;
        let slot = CACHE_LINE_SIZE as u32;
        let mut $store = LineBuf::new($size(slot, $depth));
        let ($tx, mut $rx) = <$ring>::init($store.as_mut_bytes(), slot, $depth)
            .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
            .split();
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, segmented) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v2::segment_size(slot, $depth);
        let mut $store = LineBuf::new(
            size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * $segments as u64,
        );
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, $segments)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ($tx, mut $rx) =
            zc_ring_x1::mpsc::v2::MpscRing::init(&mut $pool, slot, $depth, $segments)
                .unwrap() // OK: the pool holds exactly the segments, sized by segment_size
                .split();
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     v3 $mode:ty, $wake:ty) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v3::segment_size(slot, $depth);
        let count: u32 = if <$mode as zc_ring_x1::mpsc::v3::Mode>::MULTI {
            $segments
        } else {
            1
        };
        let mut $store =
            LineBuf::new(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * count as u64);
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, count)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ring =
            zc_ring_x1::mpsc::v3::MpscRing::<$mode, $wake>::init(&mut $pool, slot, $depth, count)
                .unwrap(); // OK: the pool holds exactly the segments, sized by segment_size
        let $tx = V3Send(ring.claim_producer().unwrap()); // OK: a fresh ring holds no role
        let mut $rx = ring.claim_consumer().unwrap(); // OK: a fresh ring holds no role
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     v3_backoff) => {
        mpsc_pair!(inner, $rx, $store, $pool, $depth, $segments,
            v3 zc_ring_x1::mpsc::v3::Multi, zc_ring_x1::wake::NoWake);
        let $tx = Backoff(inner.0);
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     v4 $mode:ty, $wait:ty) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v4::segment_size(slot, $depth);
        let count: u32 = if <$mode as zc_ring_x1::mpsc::v4::Mode>::MULTI {
            $segments
        } else {
            1
        };
        let mut $store =
            LineBuf::new(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * count as u64);
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, count)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ring =
            zc_ring_x1::mpsc::v4::MpscRing::<$mode, $wait>::init(&mut $pool, slot, $depth, count)
                .unwrap(); // OK: the pool holds exactly the segments, sized by segment_size
        let $tx = V4Send(ring.producer().unwrap()); // OK: a fresh ring holds no role
        let mut $rx = V4Recv(ring.consumer().unwrap()); // OK: a fresh ring holds no role
    };
    ($tx:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr,
     v4_backoff) => {
        mpsc_pair!(inner, $rx, $store, $pool, $depth, $segments,
            v4 zc_ring_x1::mpsc::v4::Multi, zc_ring_x1::wake::SpinOnly);
        let $tx = V4Backoff(inner.0);
    };
}

/// Define an MPSC cell body over the ring pair `$pair` builds
/// (see `mpsc_pair`): two rings at 1p/1c, producers `send_with`
/// (closure fill), the consumer `reserve_slot_with`, both under
/// the [`spin`] policy (recv sites instrumented). The MPSC
/// versions share the endpoint surface and differ by path, as
/// the SPSC ones do, so one body serves them all and the A/B
/// measures the protocol alone.
macro_rules! mpsc_cell {
    ($name:ident, $flavor:expr, $($pair:tt)+) => {
        fn $name(
            dur: Duration,
            worker_cpu: Option<usize>,
            depth: u32,
            segments: u32,
        ) -> ([TProbe; 8], Option<u64>) {
            mpsc_pair!(req_tx, req_rx, req_store, req_pool, depth, segments, $($pair)+);
            mpsc_pair!(resp_tx, resp_rx, resp_store, resp_pool, depth, segments, $($pair)+);

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
                    (recv, send_probe, resp_tx.segment_switches())
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
                let (worker_recv, worker_send, resp_switches) =
                    worker.join().expect("worker panicked");
                let switches = req_tx
                    .segment_switches()
                    .zip(resp_switches)
                    .map(|(req, resp)| req + resp);
                (
                    [
                        send_probe,
                        worker_recv.phase,
                        worker_recv.spin,
                        worker_recv.attempts,
                        worker_send,
                        recv.phase,
                        recv.spin,
                        recv.attempts,
                    ],
                    switches,
                )
            })
        }
    };
}

mpsc_cell!(
    run_mpsc_v0,
    Flavor::MpscV0,
    single zc_ring_x1::mpsc::v0::MpscRing,
    zc_ring_x1::mpsc::v0::mpsc_region_size
);
mpsc_cell!(
    run_mpsc_v1,
    Flavor::MpscV1,
    single zc_ring_x1::mpsc::v1::MpscRing,
    zc_ring_x1::mpsc::v1::mpsc_region_size
);
mpsc_cell!(run_mpsc_v2, Flavor::MpscV2, segmented);
mpsc_cell!(
    run_mpsc_v3,
    Flavor::MpscV3,
    v3 zc_ring_x1::mpsc::v3::Multi,
    zc_ring_x1::wake::NoWake
);
mpsc_cell!(
    run_mpsc_v3_single,
    Flavor::MpscV3Single,
    v3 zc_ring_x1::mpsc::v3::Single,
    zc_ring_x1::wake::NoWake
);
mpsc_cell!(run_mpsc_v3_backoff, Flavor::MpscV3Backoff, v3_backoff);
mpsc_cell!(
    run_mpsc_v3_futex,
    Flavor::MpscV3Futex,
    v3 zc_ring_x1::mpsc::v3::Multi,
    V3Futex
);
mpsc_cell!(
    run_mpsc_v4,
    Flavor::MpscV4,
    v4 zc_ring_x1::mpsc::v4::Multi,
    zc_ring_x1::wake::SpinOnly
);
mpsc_cell!(
    run_mpsc_v4_single,
    Flavor::MpscV4Single,
    v4 zc_ring_x1::mpsc::v4::Single,
    zc_ring_x1::wake::SpinOnly
);
mpsc_cell!(run_mpsc_v4_backoff, Flavor::MpscV4Backoff, v4_backoff);
mpsc_cell!(
    run_mpsc_v4_futex,
    Flavor::MpscV4Futex,
    v4 zc_ring_x1::mpsc::v4::Multi,
    V4Futex
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
    /// Segment switches, for the segmented flavor, `None` for the
    /// others.
    pub switches: Option<u64>,
    /// How often each side waited: the sends that found the ring
    /// full and the reads that found it empty.
    pub waits: Waits,
}

/// How often a stream's sides waited, each counted once per send or
/// read at its first failed look, on the waiting path only.
#[derive(Clone, Copy, Default)]
pub struct Waits {
    /// Sends, every producer's, the stop sentinels included.
    pub sends: u64,
    /// Sends that found the ring full at least once.
    pub full: u64,
    /// Reads, the stop sentinels included.
    pub reads: u64,
    /// Reads that found the ring empty at least once.
    pub empty: u64,
}

/// A spin policy that also counts the waits it starts: `count` goes
/// up at the first call of a wait, and the spin is
/// [`zc_ring_x1::policy::spin`]'s.
#[inline]
fn counting_spin(count: &mut u64) -> impl FnMut(u32) -> bool + '_ {
    move |attempt| {
        if attempt == 0 {
            *count += 1;
        }
        zc_ring_x1::policy::spin(attempt)
    }
}

/// Messages between the producer's wall-clock checks, as
/// `drive` spaces its checks.
pub(crate) const STREAM_CHECK_EVERY: u64 = 4096;

/// Where a stream's threads sit: the consumer's cpu and each
/// producer's, `None` for a thread the scheduler places.
pub struct StreamPins {
    /// The consumer's cpu.
    pub consumer: Option<usize>,
    /// Each producer's cpu, one entry per producer.
    pub producers: Vec<Option<usize>>,
}

impl StreamPins {
    /// One producer and the consumer, from a two-thread placement's
    /// `(producer, consumer)` pair.
    pub fn pair(pin: Option<(usize, usize)>) -> Self {
        StreamPins {
            consumer: pin.map(|(_, c)| c),
            producers: vec![pin.map(|(p, _)| p)],
        }
    }

    /// The two-thread pair the SPSC cells take, `None` unless both
    /// ends are pinned.
    fn pair_of(&self) -> Option<(usize, usize)> {
        self.producers[0].zip(self.consumer)
    }
}

/// Run one streaming cell: open the fill counters, stream a
/// counter for `dur` at `flavor` and ring `depth` from the
/// producer threads to a consumer thread, each on the cpu `pins`
/// names, and return the count, the elapsed time, and the
/// counters.
///
/// - Every end is a spawned thread that pins itself, as the demo's
///   streams do, so the calling thread's affinity is untouched and
///   the sides start alike.
/// - The producers send as fast as the ring admits under the
///   [`spin`] policy, so the ring sits full whenever the consumer
///   is the slower side, and the consumer asserts each producer's
///   order.
/// - The elapsed time ends when the consumer has seen every
///   producer's stop sentinel, so the last message's drain is
///   inside it.
/// - More than one producer is an MPSC flavor only.
///
/// # Panics
///
/// - More than one producer for an SPSC flavor, or none, or over
///   [`MAX_PRODUCERS`].
pub fn run_stream(
    flavor: Flavor,
    dur: Duration,
    pins: &StreamPins,
    depth: u32,
    segments: u32,
) -> StreamResult {
    let producers = pins.producers.len() as u32;
    assert!(
        (1..=MAX_PRODUCERS).contains(&producers) && (producers == 1 || flavor.is_mpsc()),
        "{} cannot stream from {producers} producers",
        flavor.as_str()
    );
    let pin = pins.pair_of();
    unpin_current();
    #[cfg(target_os = "linux")]
    let fills = Fills::open();
    let (msgs, secs, switches, waits) = match flavor {
        Flavor::SpscV0 => stream_spsc_v0(dur, pin, depth, segments),
        Flavor::SpscV1 => stream_spsc_v1(dur, pin, depth, segments),
        Flavor::SpscV2 => stream_spsc_v2(dur, pin, depth, segments),
        Flavor::SpscV3 => stream_spsc_v3(dur, pin, depth, segments),
        Flavor::SpscV4 => stream_spsc_v4(dur, pin, depth, segments),
        Flavor::MpscV0 => stream_mpsc_v0(dur, pins, depth, segments),
        Flavor::MpscV1 => stream_mpsc_v1(dur, pins, depth, segments),
        Flavor::MpscV2 => stream_mpsc_v2(dur, pins, depth, segments),
        Flavor::MpscV3 => stream_mpsc_v3(dur, pins, depth, segments),
        Flavor::MpscV3Single => stream_mpsc_v3_single(dur, pins, depth, segments),
        Flavor::MpscV3Futex => stream_mpsc_v3_futex(dur, pins, depth, segments),
        Flavor::MpscV3Backoff => stream_mpsc_v3_backoff(dur, pins, depth, segments),
        Flavor::MpscV4 => stream_mpsc_v4(dur, pins, depth, segments),
        Flavor::MpscV4Single => stream_mpsc_v4_single(dur, pins, depth, segments),
        Flavor::MpscV4Futex => stream_mpsc_v4_futex(dur, pins, depth, segments),
        Flavor::MpscV4Backoff => stream_mpsc_v4_backoff(dur, pins, depth, segments),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    StreamResult {
        msgs,
        secs,
        fills,
        switches,
        waits,
    }
}

/// The most producers a stream runs, the width of the producer
/// field in a streamed value.
pub const MAX_PRODUCERS: u32 = 64;

/// Where a streamed value carries its producer, above the
/// producer's counter.
const PRODUCER_SHIFT: u32 = 48;

/// The consumer side of a stream from `producers` producers:
/// drain `recv` until each has sent the stop sentinel, asserting
/// each producer's counter order, and return the count received.
///
/// - One producer is [`drain_stream`], so the one-producer cells
///   run what they ran before the producer field.
fn drain_streams(producers: u32, mut recv: impl FnMut() -> u64) -> u64 {
    if producers == 1 {
        return drain_stream(recv);
    }
    let mut next = vec![0u64; producers as usize];
    let (mut stops, mut total) = (0, 0u64);
    while stops < producers {
        let v = recv();
        if v == STOP {
            stops += 1;
            continue;
        }
        let p = (v >> PRODUCER_SHIFT) as usize;
        assert_eq!(
            v & ((1 << PRODUCER_SHIFT) - 1),
            next[p],
            "producer {p}'s order broken"
        );
        next[p] += 1;
        total += 1;
    }
    total
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

/// Define an SPSC streaming cell body over the ring `$pair` builds
/// (see `spsc_pair`): one ring, the producer spawned on `pin.0`,
/// the consumer on `pin.1`. Returns the messages moved and the
/// seconds.
macro_rules! spsc_stream {
    ($name:ident, $($pair:tt)+) => {
        fn $name(
            dur: Duration,
            pin: Option<(usize, usize)>,
            depth: u32,
            segments: u32,
        ) -> (u64, f64, Option<u64>, Waits) {
            spsc_pair!(tx, rx, store, pool, depth, segments, $($pair)+);
            let start = Instant::now();
            let ((msgs, empty), (switches, full, sends)) = std::thread::scope(|s| {
                let producer = s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    let mut full = 0u64;
                    let mut counter: u64 = 0;
                    loop {
                        for _ in 0..STREAM_CHECK_EVERY {
                            let mut slot = tx
                                .reserve_slot_with::<u64>(counting_spin(&mut full))
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
                        .reserve_slot_with::<u64>(counting_spin(&mut full))
                        .expect("spin never gives up");
                    *slot = STOP;
                    slot.commit();
                    (tx.segment_switches(), full, counter + 1)
                });
                let consumer = s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    let mut empty = 0u64;
                    let msgs = drain_stream(|| {
                        let slot = rx
                            .reserve_slot_with::<u64>(counting_spin(&mut empty))
                            .expect("spin never gives up");
                        let v = *slot;
                        slot.release();
                        v
                    });
                    (msgs, empty)
                });
                let got = consumer.join().expect("consumer panicked");
                (got, producer.join().expect("producer panicked"))
            });
            let waits = Waits {
                sends,
                full,
                reads: msgs + 1,
                empty,
            };
            (msgs, start.elapsed().as_secs_f64(), switches, waits)
        }
    };
}

spsc_stream!(stream_spsc_v0, single zc_ring_x1::spsc::v0::Ring, v0_region_size);
spsc_stream!(
    stream_spsc_v1,
    single zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size
);
spsc_stream!(
    stream_spsc_v2,
    single zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size
);
spsc_stream!(
    stream_spsc_v3,
    segmented zc_ring_x1::spsc::v3::Ring,
    zc_ring_x1::spsc::v3::segment_size
);
spsc_stream!(
    stream_spsc_v4,
    segmented_roles zc_ring_x1::spsc::v4::Ring,
    zc_ring_x1::spsc::v4::segment_size
);

/// Bind `$n` producers of one MPSC ring as `$txs` and its
/// consumer as `$rx`, as `mpsc_pair` binds one: v0 through v2's
/// producer cloned, v3's roles claimed, v4's taken.
macro_rules! mpsc_pair_n {
    ($txs:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, $n:expr,
     v3 $mode:ty, $wake:ty) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v3::segment_size(slot, $depth);
        let count: u32 = if <$mode as zc_ring_x1::mpsc::v3::Mode>::MULTI {
            $segments
        } else {
            1
        };
        let mut $store =
            LineBuf::new(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * count as u64);
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, count)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ring =
            zc_ring_x1::mpsc::v3::MpscRing::<$mode, $wake>::init(&mut $pool, slot, $depth, count)
                .unwrap(); // OK: the pool holds exactly the segments, sized by segment_size
        let $txs: Vec<_> = (0..$n)
            .map(|_| V3Send(ring.claim_producer().unwrap())) // OK: at most MAX_PRODUCERS, under the ring's most
            .collect();
        let mut $rx = ring.claim_consumer().unwrap(); // OK: a fresh ring holds no consumer
    };
    ($txs:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, $n:expr,
     v3_backoff) => {
        mpsc_pair_n!(inner, $rx, $store, $pool, $depth, $segments, $n,
            v3 zc_ring_x1::mpsc::v3::Multi, zc_ring_x1::wake::NoWake);
        let $txs: Vec<_> = inner.into_iter().map(|tx| Backoff(tx.0)).collect();
    };
    ($txs:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, $n:expr,
     v4 $mode:ty, $wait:ty) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v4::segment_size(slot, $depth);
        let count: u32 = if <$mode as zc_ring_x1::mpsc::v4::Mode>::MULTI {
            $segments
        } else {
            1
        };
        let mut $store =
            LineBuf::new(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * count as u64);
        let mut $pool = zc_ring_x1::Pool::init($store.as_mut_bytes(), buf as u32, count)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ring =
            zc_ring_x1::mpsc::v4::MpscRing::<$mode, $wait>::init(&mut $pool, slot, $depth, count)
                .unwrap(); // OK: the pool holds exactly the segments, sized by segment_size
        let $txs: Vec<_> = (0..$n)
            .map(|_| V4Send(ring.producer().unwrap())) // OK: at most MAX_PRODUCERS, under the ring's most
            .collect();
        let mut $rx = V4Recv(ring.consumer().unwrap()); // OK: a fresh ring holds no consumer
    };
    ($txs:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, $n:expr,
     v4_backoff) => {
        mpsc_pair_n!(inner, $rx, $store, $pool, $depth, $segments, $n,
            v4 zc_ring_x1::mpsc::v4::Multi, zc_ring_x1::wake::SpinOnly);
        let $txs: Vec<_> = inner.into_iter().map(|tx| V4Backoff(tx.0)).collect();
    };
    ($txs:ident, $rx:ident, $store:ident, $pool:ident, $depth:expr, $segments:expr, $n:expr,
     $($pair:tt)+) => {
        mpsc_pair!(tx, $rx, $store, $pool, $depth, $segments, $($pair)+);
        let $txs: Vec<_> = (0..$n).map(|_| tx.clone()).collect();
    };
}

/// Define an MPSC streaming cell body over the ring `$pair`
/// builds (see `mpsc_pair_n`): one ring, one producer per entry of
/// `pins.producers` sending by `send_with`, each spawned on its
/// cpu, and the consumer on `pins.consumer`. Each producer carries its
/// number above its counter, and sends the stop sentinel when the
/// duration is up. Returns the messages moved and the seconds.
macro_rules! mpsc_stream {
    ($name:ident, $($pair:tt)+) => {
        fn $name(
            dur: Duration,
            pins: &StreamPins,
            depth: u32,
            segments: u32,
        ) -> (u64, f64, Option<u64>, Waits) {
            let producers = pins.producers.len() as u32;
            mpsc_pair_n!(txs, rx, store, pool, depth, segments, producers, $($pair)+);
            let start = Instant::now();
            let ((msgs, empty), sides) = std::thread::scope(|s| {
                let mut handles = Vec::new();
                for (p, tx) in txs.into_iter().enumerate() {
                    let cpu = pins.producers[p];
                    handles.push(s.spawn(move || {
                        if let Some(cpu) = cpu {
                            pin_to_cpu(cpu);
                        }
                        let tag = (p as u64) << PRODUCER_SHIFT;
                        let mut full = 0u64;
                        let mut counter: u64 = 0;
                        loop {
                            for _ in 0..STREAM_CHECK_EVERY {
                                tx.send_with::<u64>(counting_spin(&mut full), |m| {
                                    *m = tag | counter
                                })
                                .expect("spin never gives up");
                                counter += 1;
                            }
                            if start.elapsed() >= dur {
                                break;
                            }
                        }
                        tx.send_with::<u64>(counting_spin(&mut full), |m| *m = STOP)
                            .expect("spin never gives up");
                        (tx.segment_switches(), full, counter + 1)
                    }));
                }
                let consumer_cpu = pins.consumer;
                let consumer = s.spawn(move || {
                    if let Some(c) = consumer_cpu {
                        pin_to_cpu(c);
                    }
                    let mut empty = 0u64;
                    let msgs = drain_streams(producers, || {
                        let slot = rx
                            .reserve_slot_with::<u64>(counting_spin(&mut empty))
                            .expect("spin never gives up");
                        let v = *slot;
                        slot.release();
                        v
                    });
                    (msgs, empty)
                });
                let got = consumer.join().expect("consumer panicked");
                let sides: Vec<_> = handles
                    .into_iter()
                    .map(|h| h.join().expect("producer panicked"))
                    .collect();
                (got, sides)
            });
            // The switch count is the ring's, so any producer's says it.
            let switches = sides[0].0;
            let waits = Waits {
                sends: sides.iter().map(|side| side.2).sum(),
                full: sides.iter().map(|side| side.1).sum(),
                reads: msgs + producers as u64,
                empty,
            };
            (msgs, start.elapsed().as_secs_f64(), switches, waits)
        }
    };
}

mpsc_stream!(
    stream_mpsc_v0,
    single zc_ring_x1::mpsc::v0::MpscRing,
    zc_ring_x1::mpsc::v0::mpsc_region_size
);
mpsc_stream!(
    stream_mpsc_v1,
    single zc_ring_x1::mpsc::v1::MpscRing,
    zc_ring_x1::mpsc::v1::mpsc_region_size
);
mpsc_stream!(stream_mpsc_v2, segmented);
mpsc_stream!(
    stream_mpsc_v3,
    v3 zc_ring_x1::mpsc::v3::Multi,
    zc_ring_x1::wake::NoWake
);
mpsc_stream!(
    stream_mpsc_v3_single,
    v3 zc_ring_x1::mpsc::v3::Single,
    zc_ring_x1::wake::NoWake
);
mpsc_stream!(stream_mpsc_v3_futex, v3 zc_ring_x1::mpsc::v3::Multi, V3Futex);
mpsc_stream!(stream_mpsc_v3_backoff, v3_backoff);
mpsc_stream!(
    stream_mpsc_v4,
    v4 zc_ring_x1::mpsc::v4::Multi,
    zc_ring_x1::wake::SpinOnly
);
mpsc_stream!(
    stream_mpsc_v4_single,
    v4 zc_ring_x1::mpsc::v4::Single,
    zc_ring_x1::wake::SpinOnly
);
mpsc_stream!(stream_mpsc_v4_futex, v4 zc_ring_x1::mpsc::v4::Multi, V4Futex);
mpsc_stream!(stream_mpsc_v4_backoff, v4_backoff);
