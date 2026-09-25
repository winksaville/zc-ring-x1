//! Demo binary: both primitives working across threads,
//! with throughput printed: run `cargo run --release`, or
//! `cargo install --path . --locked` and run
//! `zc-ring-x1-demo`. `-V`/`--version` prints the
//! version-of-record so you know exactly which build you
//! are testing, `-h`/`--help` the usage, and `--base-cpu <n>`
//! sets the base, the cpu the single-thread lines pin to and
//! every placement starts from, the last core's primary cpu
//! by default, the quiet end of the kernel's fill order.
//!
//! - Part 1, the ring: an SPSC pair moves typed messages
//!   in place (reserve -> write -> commit, reserve -> read ->
//!   release), first both ends on one thread
//!   (the ring's own cost), then one producer thread to
//!   one consumer thread at each placement the machine has,
//!   in the measurement tools' terms: CCX, two cores on one
//!   L3, x-CCX, cores on different L3s, SMT, one core's two
//!   cpus sharing its L1 and L2, and unpinned.
//!   Every ring version runs beside it at each placement, the
//!   `spsc1_` to `spsc3_` and `mpsc0_` to `mpsc2_` lines
//!   (the MPSC ones by send_with closure fill), the segmented
//!   rings at one segment, plus a 2-producer + 1-consumer
//!   line: the shape only the MPSC ring can run.
//! - The depth sweep: the ring flavors again at every
//!   placement and at depths 1, 2, 8, and 64, one table per
//!   placement, so depth and protocol can be told apart, the
//!   segmented rings again at one segment.
//! - The segment stress, last, one table with a legend: spsc-v3
//!   and mpsc-v2 at four segments, a burst that fills every
//!   segment and drains on one thread, a lagging consumer at
//!   each placement, and the cost of one switch measured at
//!   depth 1 as a difference at equal capacity, 32 segments of
//!   one slot against one segment of 32.
//! - Part 2, the pool: an allocator thread allocs and
//!   fills `BufSlot`s and hands them to a freer thread.
//!   "send" today is moving the guard (see the README's
//!   usage model). Getting a buffer implies nothing about
//!   when it is sent or freed.
//! - The composed form (descriptors through the ring,
//!   payloads at rest in pool buffers) runs between them:
//!   alloc -> to_desc -> ring -> to_slot -> free, with the
//!   same placement ladder as the raw ring.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use zc_ring_x1::pool::v1::StackGeometry;
use zc_ring_x1::{
    BufSlot, CACHE_LINE_SIZE, Desc, Empty, Exhausted, Full, MpscRing, Pool, PoolRegistry,
    mpsc_region_size, policy,
};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Messages moved per part.
const COUNT: u64 = 1_000_000;

/// Ring slots / pool buffers (small on purpose: recycling
/// under pressure is the interesting case).
const DEPTH: u32 = 64;

/// The ring depths the sweep runs, DEPTH among them.
const DEPTHS: [u32; 4] = [1, 2, 8, 64];

/// Segments per ring for the segmented rings, spsc-v3 and
/// mpsc-v2, in the one_msg lines and the depth sweep: one, so
/// those lines measure the fast path alone and never switch.
const SWEEP_SEGMENTS: u32 = 1;

/// Segments per ring in the segment stress, where switching is
/// the point.
const STRESS_SEGMENTS: u32 = 4;

/// The lagging consumer's pause between bursts.
const LAG_PAUSE: Duration = Duration::from_micros(20);

/// The demo message: the sequence number the consumer
/// asserts, and `val` (spare payload, doubling as the
/// producer id in the multi-producer line).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

/// Region for the pool, or a v0 ring at DEPTH: biggest header
/// (4 lines) + DEPTH one-line slots/buffers.
#[repr(C, align(64))]
struct Region([u8; 4 * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE]);

/// One cache line of backing store, so a `Vec` of them is a
/// line-aligned region of any length, viewed as bytes through
/// zerocopy.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C, align(64))]
struct CacheLine([u8; CACHE_LINE_SIZE]);

/// A zeroed, line-aligned heap region of at least `bytes`
/// bytes, sized at runtime so a ring's depth is a parameter.
///
/// - Heap against stack changes nothing the loops measure: the
///   region is touched once at init and lives in cache after.
fn region(bytes: u64) -> Vec<CacheLine> {
    let lines = bytes.div_ceil(CACHE_LINE_SIZE as u64) as usize;
    (0..lines)
        .map(|_| CacheLine([0; CACHE_LINE_SIZE]))
        .collect()
}

/// Bytes a v0 ring region needs: the four-line header, then
/// the slots. v0 exports no size function, its header being a
/// fixed shape.
fn v0_region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<zc_ring_x1::spsc::v0::Header>() as u64 + slot_size as u64 * capacity as u64
}

/// Group a count into comma-separated thousands
/// (`1234567` -> `"1,234,567"`).
fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Print one part's line: msgs/sec (comma-grouped) and
/// ns/msg from the part's elapsed seconds.
fn report(label: &str, secs: f64) {
    let rate = commas((COUNT as f64 / secs) as u64);
    let ns_per_msg = secs * 1e9 / COUNT as f64;
    println!("{label:<46} {rate:>12} msgs/sec  {ns_per_msg:>7.1} ns/msg");
}

/// Parse a /sys cpu-list string ("0,12" or "0-2,6") into cpu
/// numbers. Malformed pieces are skipped.
#[cfg(target_os = "linux")]
fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',') {
        match part.split_once('-') {
            Some((lo, hi)) => {
                if let (Ok(lo), Ok(hi)) = (lo.parse::<usize>(), hi.parse::<usize>()) {
                    out.extend(lo..=hi);
                }
            }
            None => {
                if let Ok(n) = part.parse() {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// A `(producer cpu, consumer cpu)` pin for a 2t run.
/// `None` means the pair is unavailable / leave unpinned.
type PinPair = Option<(usize, usize)>;

/// A cpu's SMT sibling list from sysfs, the cpu alone when it
/// cannot be read.
#[cfg(target_os = "linux")]
fn siblings_of(cpu: usize) -> Vec<usize> {
    std::fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list"
    ))
    .ok()
    .map(|s| parse_cpu_list(&s))
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| vec![cpu])
}

/// A core's primary cpu: the lowest cpu in its sibling list.
#[cfg(target_os = "linux")]
fn is_primary_cpu(cpu: usize) -> bool {
    siblings_of(cpu).iter().min() == Some(&cpu)
}

/// The online cpus from sysfs, empty when unreadable.
#[cfg(target_os = "linux")]
fn online_cpus() -> Vec<usize> {
    std::fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default()
}

/// Partner order: a core's primary cpu before its secondary,
/// and the highest cpu number first. The scheduler's idlest-cpu
/// search fills cpus from the bottom, so the top is the quiet
/// end, and a primary cpu's sibling is idler than a secondary's
/// (design note, Measurement placements).
#[cfg(target_os = "linux")]
fn quiet_first(cpus: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = cpus.to_vec();
    v.sort_by_key(|&c| (!is_primary_cpu(c), std::cmp::Reverse(c)));
    v
}

/// The base cpu when `--base-cpu` is not given: the last
/// core's primary cpu, the quiet end of the kernel's fill
/// order, 11 on a 3900X and 5 on a 7600X. 0 when sysfs cannot
/// be read.
#[cfg(target_os = "linux")]
fn default_base_cpu() -> usize {
    online_cpus()
        .into_iter()
        .filter(|&c| is_primary_cpu(c))
        .max()
        .unwrap_or(0)
}

/// Non-Linux stub: no topology, so 0 and nothing pins.
#[cfg(not(target_os = "linux"))]
fn default_base_cpu() -> usize {
    0
}

/// Where a 2t run's threads sit: a label in the measurement
/// tools' form, `<p>,<c> CCX`, `<p>,<c> x-CCX`, `<p>,<c> SMT`,
/// or `unpinned`, and the pin behind it.
struct Placement {
    label: String,
    pin: PinPair,
}

/// Discover the placements for the pinned 2t runs from
/// /sys/devices/system/cpu, each starting at `base`, in the
/// measurement tools' order, and only those the machine has:
///
/// - `CCX`: `base` and another core sharing its L3, the near
///   cross-core handoff.
/// - `x-CCX`: `base` and a core outside its L3, the far one.
/// - `SMT`: `base` and its SMT sibling, one core's two
///   cpus sharing L1 and L2, the cheapest.
/// - `unpinned`: the scheduler's choice, always present.
#[cfg(target_os = "linux")]
fn discover_placements(base: usize) -> Vec<Placement> {
    let siblings = siblings_of(base);
    let online = online_cpus();
    let l3 = std::fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{base}/cache/index3/shared_cpu_list"
    ))
    .ok()
    .map(|s| parse_cpu_list(&s))
    .unwrap_or_else(|| siblings.clone());
    let mut v = Vec::new();
    if let Some(c) = quiet_first(&l3)
        .into_iter()
        .find(|&c| c != base && !siblings.contains(&c))
    {
        v.push(Placement {
            label: format!("{base},{c} CCX"),
            pin: Some((base, c)),
        });
    }
    if let Some(c) = quiet_first(&online).into_iter().find(|c| !l3.contains(c)) {
        v.push(Placement {
            label: format!("{base},{c} x-CCX"),
            pin: Some((base, c)),
        });
    }
    if let Some(c) = siblings.iter().copied().find(|&c| c != base) {
        v.push(Placement {
            label: format!("{base},{c} SMT"),
            pin: Some((base, c)),
        });
    }
    v.push(Placement {
        label: "unpinned".to_string(),
        pin: None,
    });
    v
}

/// Non-Linux stub: unpinned only.
#[cfg(not(target_os = "linux"))]
fn discover_placements(_base: usize) -> Vec<Placement> {
    vec![Placement {
        label: "unpinned".to_string(),
        pin: None,
    }]
}

/// The cpu every single-thread line pins to and every
/// placement starts from: `--base-cpu`, default
/// [`default_base_cpu`]. Set once in
/// `main` before any run, read at every pin, so the loops
/// behind the macros and the flavor tables keep their
/// signatures.
static BASE_CPU: AtomicUsize = AtomicUsize::new(0);

/// The base cpu, see [`BASE_CPU`].
fn base_cpu() -> usize {
    BASE_CPU.load(Ordering::Relaxed)
}

/// The usage text, printed by `-h` / `--help` and, to stderr,
/// on an argument the demo does not know.
const USAGE: &str = "\
usage: zc-ring-x1-demo [--base-cpu <n>]
       zc-ring-x1-demo -h | --help | -V | --version

  --base-cpu <n>  the cpu the single-thread lines pin to and every 2t
                  placement starts from: CCX is <n> and a core on its L3,
                  x-CCX is <n> and a core outside it, SMT is <n> and its
                  sibling. The default is the last core's primary
                  cpu, the quiet end of the kernel's fill order.
  -h, --help      print this and exit
  -V, --version   print the version-of-record and exit";

/// The parsed command line: the base cpu, or an early exit.
enum Args {
    Run { base_cpu: usize },
    Exit { code: i32 },
}

/// Parse the arguments after the program name. `-V` and `-h`
/// print and exit 0. An unknown argument, a `--base-cpu`
/// without a value, or one that is not a number prints the
/// usage to stderr and exits 1.
fn parse_args(args: impl Iterator<Item = String>) -> Args {
    let mut base_cpu = default_base_cpu();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-V" | "--version" => return Args::Exit { code: 0 },
            "-h" | "--help" => {
                println!("{USAGE}");
                return Args::Exit { code: 0 };
            }
            "--base-cpu" => match args.next().map(|v| v.parse::<usize>()) {
                Some(Ok(n)) => base_cpu = n,
                Some(Err(_)) => {
                    eprintln!("error: --base-cpu wants a cpu number");
                    eprintln!("{USAGE}");
                    return Args::Exit { code: 1 };
                }
                None => {
                    eprintln!("error: --base-cpu wants a value");
                    eprintln!("{USAGE}");
                    return Args::Exit { code: 1 };
                }
            },
            _ => {
                eprintln!("error: unknown argument `{a}`");
                eprintln!("{USAGE}");
                return Args::Exit { code: 1 };
            }
        }
    }
    Args::Run { base_cpu }
}

/// Pin the calling thread to `cpu` via sched_setaffinity.
/// Panics on failure (a demo run with a silently ignored pin
/// would report a mislabeled number).
#[cfg(target_os = "linux")]
fn pin_to_cpu(cpu: usize) {
    // SAFETY: cpu_set_t is a plain bitmask. CPU_ZERO/CPU_SET
    // initialize it fully before sched_setaffinity reads it.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        let rc = libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set);
        assert_eq!(rc, 0, "sched_setaffinity({cpu}) failed");
    }
}

/// Non-Linux stub: pinning is a no-op, so only the 1t runs
/// reach it (discover_placements is unpinned only), present
/// so the demo compiles everywhere.
#[cfg(not(target_os = "linux"))]
fn pin_to_cpu(_cpu: usize) {}

/// Bind one SPSC ring's `$producer` and `$consumer`, one-line
/// slots at `$depth`, its storage held in `$store` (and, for a
/// ring of segments, its pool in `$pool`) so it outlives them.
///
/// - `single $ring, $size`: a ring over one region sized by
///   `$size(slot_size, depth)`.
/// - `segmented $segments`: a v3 ring of `$segments` segments,
///   each `$depth` slots, over a pool holding exactly those
///   segments.
macro_rules! spsc_pair {
    ($producer:ident, $consumer:ident, $store:ident, $pool:ident, $depth:expr,
     single $ring:path, $size:path) => {
        let slot = CACHE_LINE_SIZE as u32;
        let mut $store = region($size(slot, $depth));
        let (mut $producer, mut $consumer) = <$ring>::init($store.as_mut_bytes(), slot, $depth)
            .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
            .split();
    };
    ($producer:ident, $consumer:ident, $store:ident, $pool:ident, $depth:expr,
     segmented $segments:expr) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::spsc::v3::segment_size(slot, $depth);
        let mut $store =
            region(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * $segments as u64);
        let mut $pool = Pool::init($store.as_mut_bytes(), buf as u32, $segments)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let (mut $producer, mut $consumer) =
            zc_ring_x1::spsc::v3::Ring::init(&mut $pool, slot, $depth, $segments)
                .unwrap() // OK: the pool holds exactly the segments, sized by segment_size
                .split();
    };
}

/// Define the SPSC one-message loops over the ring `$pair` builds
/// (see `spsc_pair`): `$one_t`
/// moves COUNT messages single thread, pinned to the base cpu, and
/// `$two_t` moves them producer-thread -> consumer-thread (`pin`
/// as in [`spsc_ring_one_msg_2t`]), both over a ring of `depth`
/// slots. Each returns elapsed seconds. The SPSC versions share
/// the endpoint surface and differ by path, so one body serves
/// them all and the lines read as the protocol seam alone.
///
/// The 1t loop runs in a scoped thread rather than pinning the
/// main thread: spawned threads inherit the main thread's
/// affinity mask, which would squeeze every later part onto
/// the base cpu.
macro_rules! spsc_loops {
    ($one_t:ident, $two_t:ident, $($pair:tt)+) => {
        fn $one_t(depth: u32) -> f64 {
            spsc_pair!(producer, consumer, store, pool, depth, $($pair)+);

            let start = Instant::now();
            std::thread::scope(|s| {
                s.spawn(move || {
                    pin_to_cpu(base_cpu());
                    for i in 0..COUNT {
                        match producer.reserve_slot_with::<Msg>(|_| false) {
                            Ok(mut slot) => {
                                slot.seq = i;
                                slot.commit();
                            }
                            Err(Full) => {
                                panic!(concat!(
                                    stringify!($one_t),
                                    ": producer Full SHOULD NOT HAPPEN"
                                ));
                            }
                        }
                        match consumer.reserve_slot_with::<Msg>(|_| false) {
                            Ok(msg) => {
                                assert_eq!(msg.seq, i);
                                msg.release();
                            }
                            Err(Empty) => {
                                panic!(concat!(
                                    stringify!($one_t),
                                    ": consumer Empty SHOULD NOT HAPPEN"
                                ));
                            }
                        }
                    }
                });
            });
            start.elapsed().as_secs_f64()
        }

        fn $two_t(pin: PinPair, depth: u32) -> f64 {
            spsc_pair!(producer, consumer, store, pool, depth, $($pair)+);

            let start = Instant::now();
            std::thread::scope(|s| {
                s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    for i in 0..COUNT {
                        let mut slot = producer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
                        slot.seq = i;
                        slot.commit();
                    }
                });
                s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    for i in 0..COUNT {
                        let msg = consumer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
                        assert_eq!(msg.seq, i);
                        msg.release();
                    }
                });
            });
            start.elapsed().as_secs_f64()
        }
    };
}

spsc_loops!(
    spsc_ring_one_msg_1t,
    spsc_ring_one_msg_2t,
    single zc_ring_x1::spsc::v0::Ring,
    v0_region_size
);
spsc_loops!(
    spsc1_ring_one_msg_1t,
    spsc1_ring_one_msg_2t,
    single zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size
);
spsc_loops!(
    spsc2_ring_one_msg_1t,
    spsc2_ring_one_msg_2t,
    single zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size
);
spsc_loops!(
    spsc3_ring_one_msg_1t,
    spsc3_ring_one_msg_2t,
    segmented SWEEP_SEGMENTS
);

/// Bind one MPSC ring's `$producer` and `$consumer`, one-line
/// slots at `$depth`, its storage held in `$store` (and, for a
/// ring of segments, its pool in `$pool`) so it outlives them.
///
/// - `single $ring, $size`: a ring over one region sized by
///   `$size(slot_size, depth)`.
/// - `segmented $segments`: a v2 ring of `$segments` segments,
///   each `$depth` slots, over a pool holding exactly those
///   segments.
macro_rules! mpsc_pair {
    ($producer:ident, $consumer:ident, $store:ident, $pool:ident, $depth:expr,
     single $ring:path, $size:path) => {
        let slot = CACHE_LINE_SIZE as u32;
        let mut $store = region($size(slot, $depth));
        let ($producer, mut $consumer) = <$ring>::init($store.as_mut_bytes(), slot, $depth)
            .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
            .split();
    };
    ($producer:ident, $consumer:ident, $store:ident, $pool:ident, $depth:expr,
     segmented $segments:expr) => {
        let slot = CACHE_LINE_SIZE as u32;
        let buf = zc_ring_x1::mpsc::v2::segment_size(slot, $depth);
        let mut $store =
            region(size_of::<zc_ring_x1::PoolHeader>() as u64 + buf * $segments as u64);
        let mut $pool = Pool::init($store.as_mut_bytes(), buf as u32, $segments)
            .unwrap(); // OK: the region is sized for exactly the segments and line-aligned
        let ($producer, mut $consumer) =
            zc_ring_x1::mpsc::v2::MpscRing::init(&mut $pool, slot, $depth, $segments)
                .unwrap() // OK: the pool holds exactly the segments, sized by segment_size
                .split();
    };
}

/// Define the MPSC one-message loops over the ring at `$ring`,
/// its region sized by `$size(slot_size, depth)`, the siblings
/// of the SPSC loops at the same placements: `$one_t` moves
/// COUNT messages single thread, pinned to the base cpu, so the two
/// lines read as the seam between the protocols (claim CAS +
/// seq vs load/store), and `$two_t` moves them producer-thread
/// -> consumer-thread (`pin` as in [`spsc_ring_one_msg_2t`]),
/// measuring what the MPSC protocol costs when you don't need
/// multiple producers. Each returns elapsed seconds. The MPSC
/// versions share the endpoint surface and differ by path, so
/// one body serves them all.
macro_rules! mpsc_loops {
    ($one_t:ident, $two_t:ident, $($pair:tt)+) => {
        fn $one_t(depth: u32) -> f64 {
            mpsc_pair!(producer, consumer, store, pool, depth, $($pair)+);

            let start = Instant::now();
            std::thread::scope(|s| {
                s.spawn(move || {
                    pin_to_cpu(base_cpu());
                    for i in 0..COUNT {
                        producer.send_with::<Msg>(|_| false, |m| m.seq = i).unwrap(); // OK: room is guaranteed, the consumer drains in lockstep
                        match consumer.reserve_slot_with::<Msg>(|_| false) {
                            Ok(msg) => {
                                assert_eq!(msg.seq, i);
                                msg.release();
                            }
                            Err(Empty) => {
                                panic!(concat!(
                                    stringify!($one_t),
                                    ": consumer Empty SHOULD NOT HAPPEN"
                                ));
                            }
                        }
                    }
                });
            });
            start.elapsed().as_secs_f64()
        }

        fn $two_t(pin: PinPair, depth: u32) -> f64 {
            mpsc_pair!(producer, consumer, store, pool, depth, $($pair)+);

            let start = Instant::now();
            std::thread::scope(|s| {
                s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    for i in 0..COUNT {
                        producer
                            .send_with::<Msg>(policy::spin, |m| m.seq = i)
                            .unwrap(); // OK: policy::spin never gives up
                    }
                });
                s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    for i in 0..COUNT {
                        let msg = consumer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
                        assert_eq!(msg.seq, i);
                        msg.release();
                    }
                });
            });
            start.elapsed().as_secs_f64()
        }
    };
}

mpsc_loops!(
    mpsc0_ring_one_msg_1t,
    mpsc0_ring_one_msg_2t,
    single zc_ring_x1::mpsc::v0::MpscRing,
    zc_ring_x1::mpsc::v0::mpsc_region_size
);
mpsc_loops!(
    mpsc1_ring_one_msg_1t,
    mpsc1_ring_one_msg_2t,
    single zc_ring_x1::mpsc::v1::MpscRing,
    zc_ring_x1::mpsc::v1::mpsc_region_size
);
mpsc_loops!(
    mpsc2_ring_one_msg_1t,
    mpsc2_ring_one_msg_2t,
    segmented SWEEP_SEGMENTS
);

/// Move COUNT messages (COUNT/2 per producer) from two
/// producer threads into one consumer, the line the SPSC
/// ring cannot produce: claim contention on the shared
/// producer index. Unpinned (a pinned variant would need a
/// third discovered cpu). Per-producer FIFO is asserted, the
/// interleave is whatever the claim race said. Returns
/// elapsed seconds.
fn mpsc1_ring_one_msg_3t() -> f64 {
    let slot = CACHE_LINE_SIZE as u32;
    let mut region = region(mpsc_region_size(slot, DEPTH));
    let (producer, mut consumer) = MpscRing::init(region.as_mut_bytes(), slot, DEPTH)
        .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
        .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        for p in 0..2u64 {
            let producer = producer.clone();
            s.spawn(move || {
                for i in 0..COUNT / 2 {
                    producer
                        .send_with::<Msg>(policy::spin, |m| {
                            m.seq = i;
                            m.val = p;
                        })
                        .unwrap(); // OK: policy::spin never gives up
                }
            });
        }
        s.spawn(move || {
            let mut next = [0u64; 2];
            for _ in 0..COUNT {
                let msg = consumer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
                let p = msg.val as usize;
                assert_eq!(msg.seq, next[p], "per-producer order broken");
                next[p] += 1;
                msg.release();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// The composed flow on one thread pinned to the base cpu: one pool
/// message allocated outside the timed loop. Each iteration
/// populates it, converts guard -> descriptor, rings the
/// descriptor across, and takes the guard back. Return
/// elapsed seconds.
///
/// - Isolates messaging cost (to_desc + ring + to_slot)
///   from the pool cycle: pool_alloc_free_1t reports that
///   separately.
/// - The guard and the descriptor are the two exclusive
///   forms of ownership, so the per-iteration conversion
///   cannot hoist: converting *is* the send-side handoff.
///   The 2t variant keeps alloc/free per message because
///   there ownership genuinely leaves the producer.
fn spsc_ring_one_pool_msg_1t() -> f64 {
    let mut ring_region = Region([0; size_of::<Region>()]);
    let mut pool_region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut pool_region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers
    let mut registry = PoolRegistry::<1>::new();
    let pool_id = registry.register(pool.view()).unwrap(); // OK: empty capacity-1 registry always has room
    let (mut producer, mut consumer) =
        zc_ring_x1::spsc::v2::Ring::init(&mut ring_region.0, CACHE_LINE_SIZE as u32, DEPTH)
            .unwrap() // OK: Region is sized/aligned for the ring header + DEPTH slots
            .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(base_cpu());
            let mut buf_slot = pool.alloc::<Msg>().unwrap(); // OK: fresh pool, DEPTH buffers free
            for i in 0..COUNT {
                buf_slot.seq = i;
                let desc = registry
                    .to_desc(pool_id, buf_slot)
                    .map_err(|(_, e)| e)
                    .unwrap(); // OK: pool_id came from this registry's register
                match producer.reserve_slot_with::<Desc>(|_| false) {
                    Ok(mut slot) => {
                        *slot = desc;
                        slot.commit();
                    }
                    Err(Full) => {
                        panic!("spsc_ring_one_pool_msg_1t: producer Full SHOULD NOT HAPPEN");
                    }
                }
                buf_slot = match consumer.reserve_slot_with::<Desc>(|_| false) {
                    Ok(slot) => {
                        let desc = *slot;
                        slot.release();
                        // SAFETY: the desc was consumed into
                        // the ring by to_desc above and is
                        // taken back exactly once, same thread.
                        let msg = unsafe { registry.to_slot::<Msg>(desc) }.unwrap(); // OK: desc came from to_desc on this pool
                        assert_eq!(msg.seq, i);
                        msg
                    }
                    Err(Empty) => {
                        panic!("spsc_ring_one_pool_msg_1t: consumer Empty SHOULD NOT HAPPEN");
                    }
                };
            }
            buf_slot.free();
        });
    });
    start.elapsed().as_secs_f64()
}

/// The composed flow producer-thread -> consumer-thread:
/// alloc + fill pool messages on the producer, descriptors
/// cross the SPSC ring, the consumer takes them back and frees.
/// Return elapsed seconds.
///
/// - `pin`: `Some((p, c))` pins the producer to cpu `p` and
///   the consumer to cpu `c`. `None` lets the scheduler place
///   them (the number then depends on where they land).
fn spsc_ring_one_pool_msg_2t(pin: PinPair) -> f64 {
    let mut ring_region = Region([0; size_of::<Region>()]);
    let mut pool_region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut pool_region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers
    let mut registry = PoolRegistry::<1>::new();
    let pool_id = registry.register(pool.view()).unwrap(); // OK: empty capacity-1 registry always has room
    let registry = &registry;
    let (mut producer, mut consumer) =
        zc_ring_x1::spsc::v2::Ring::init(&mut ring_region.0, CACHE_LINE_SIZE as u32, DEPTH)
            .unwrap() // OK: Region is sized/aligned for the ring header + DEPTH slots
            .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some((p, _)) = pin {
                pin_to_cpu(p);
            }
            for i in 0..COUNT {
                let mut buf_slot = loop {
                    match pool.alloc::<Msg>() {
                        Ok(buf_slot) => break buf_slot,
                        Err(Exhausted) => std::hint::spin_loop(),
                    }
                };
                buf_slot.seq = i;
                let desc = registry
                    .to_desc(pool_id, buf_slot)
                    .map_err(|(_, e)| e)
                    .unwrap(); // OK: pool_id came from this registry's register
                let mut slot = producer.reserve_slot_with::<Desc>(policy::spin).unwrap(); // OK: policy::spin never gives up
                *slot = desc;
                slot.commit();
            }
        });
        s.spawn(move || {
            if let Some((_, c)) = pin {
                pin_to_cpu(c);
            }
            for i in 0..COUNT {
                let desc = {
                    let slot = consumer.reserve_slot_with::<Desc>(policy::spin).unwrap(); // OK: policy::spin never gives up
                    let desc = *slot;
                    slot.release();
                    desc
                };
                // SAFETY: the desc was consumed into the ring
                // by the producer and read after the commit ->
                // reserve handoff (happens-before). Each is
                // taken back exactly once.
                let msg = unsafe { registry.to_slot::<Msg>(desc) }.unwrap(); // OK: descs here only come from the producer's to_desc
                assert_eq!(msg.seq, i);
                msg.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Alloc + fill COUNT messages on an allocator thread,
/// free them on a freer thread (guards cross a std
/// channel). Return elapsed seconds.
///
/// - `pin`: `Some((a, f))` pins the allocator to cpu `a`
///   and the freer to cpu `f`. `None` lets the scheduler
///   place them (the number then depends on where they
///   land).
fn std_mpsc_one_pool_msg_2t(pin: PinPair) -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers

    let (tx, rx) = std::sync::mpsc::sync_channel::<BufSlot<'_, Msg>>(DEPTH as usize);
    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some((a, _)) = pin {
                pin_to_cpu(a);
            }
            for i in 0..COUNT {
                loop {
                    match pool.alloc::<Msg>() {
                        Ok(mut buf_slot) => {
                            buf_slot.seq = i;
                            tx.send(buf_slot).unwrap();
                            break;
                        }
                        Err(Exhausted) => std::hint::spin_loop(),
                    }
                }
            }
        });
        s.spawn(move || {
            if let Some((_, f)) = pin {
                pin_to_cpu(f);
            }
            for i in 0..COUNT {
                let buf_slot = rx.recv().unwrap();
                assert_eq!(buf_slot.seq, i);
                buf_slot.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// The std-channel flow on one thread pinned to the base cpu: alloc
/// a pool message, move its guard through a sync_channel,
/// receive and free it. Return elapsed seconds.
fn std_mpsc_one_pool_msg_1t() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers
    let (tx, rx) = std::sync::mpsc::sync_channel::<BufSlot<'_, Msg>>(DEPTH as usize);

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(base_cpu());
            for i in 0..COUNT {
                let mut buf_slot = pool.alloc::<Msg>().unwrap(); // OK: alloc+free per iteration, DEPTH never exceeded
                buf_slot.seq = i;
                tx.send(buf_slot).unwrap(); // OK: capacity DEPTH, at most one in flight
                let buf_slot = rx.recv().unwrap(); // OK: just sent on this thread
                assert_eq!(buf_slot.seq, i);
                buf_slot.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Alloc -> write -> free COUNT messages on one thread pinned
/// to the base cpu, the pool's own cost, no channel, no second
/// thread. Return elapsed seconds.
///
/// Runs in a scoped thread like the other pinned parts, so
/// the main thread's affinity stays untouched.
fn pool_alloc_free_1t() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(base_cpu());
            for i in 0..COUNT {
                let mut buf_slot = pool.alloc::<Msg>().unwrap(); // OK: alloc+free per iteration, DEPTH never exceeded
                buf_slot.seq = i;
                std::hint::black_box(buf_slot.seq);
                buf_slot.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// A message eight cache lines long, for the largest stack of the four-stack pool: `seq` leads,
/// as in [`Msg`], so the loop writes the same word.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg8 {
    seq: u64,
    rest: [u64; 8 * CACHE_LINE_SIZE / 8 - 1],
}

/// A message whose sequence word the alloc/free loops write, so one loop runs over [`Msg`] and
/// [`Msg8`] alike.
trait Seq {
    /// Store the sequence word.
    fn set_seq(&mut self, seq: u64);
    /// Load the sequence word.
    fn seq(&self) -> u64;
}

impl Seq for Msg {
    /// Store `seq`.
    fn set_seq(&mut self, seq: u64) {
        self.seq = seq;
    }
    /// Load `seq`.
    fn seq(&self) -> u64 {
        self.seq
    }
}

impl Seq for Msg8 {
    /// Store `seq`.
    fn set_seq(&mut self, seq: u64) {
        self.seq = seq;
    }
    /// Load `seq`.
    fn seq(&self) -> u64 {
        self.seq
    }
}

/// The one-stack pool v1: DEPTH one-line buffers, the geometry of [`pool_alloc_free_1t`]'s v0
/// pool.
const POOL1_ONE_STACK: [StackGeometry; 1] = [StackGeometry::new(CACHE_LINE_SIZE as u32, DEPTH)];

/// The four-stack pool v1: DEPTH buffers each of one, two, four, and eight lines.
const POOL1_FOUR_STACKS: [StackGeometry; 4] = [
    StackGeometry::new(CACHE_LINE_SIZE as u32, DEPTH),
    StackGeometry::new(2 * CACHE_LINE_SIZE as u32, DEPTH),
    StackGeometry::new(4 * CACHE_LINE_SIZE as u32, DEPTH),
    StackGeometry::new(8 * CACHE_LINE_SIZE as u32, DEPTH),
];

/// [`pool_alloc_free_1t`]'s loop over a pool v1 of `stacks`, allocating a `T`: alloc -> write ->
/// free COUNT messages on one thread pinned to the base cpu. Return elapsed seconds.
///
/// - `serves`: the buffer size the row claims serves a `T`, checked once before the clock
///   starts, so the label cannot claim a stack the row does not hit.
/// - One stack against v0 is the cost of the stack choice where it should cost nothing.
/// - Four stacks with a `T` for the smallest is the cheapest choice, one comparison, and with a
///   `T` for the largest the costliest, a scan of all four.
/// - The wanted stack never empties, one buffer being out at a time, so no fallback runs.
fn pool1_alloc_free_1t<const N: usize, T>(stacks: [StackGeometry; N], serves: u32) -> f64
where
    T: FromBytes + IntoBytes + KnownLayout + Seq,
{
    let mut region = region(zc_ring_x1::pool::v1::region_size(stacks));
    let mut pool = zc_ring_x1::pool::v1::Pool::init(region.as_mut_bytes(), stacks).unwrap(); // OK: region sized by region_size, line-aligned
    let probe = pool.alloc::<T>().unwrap(); // OK: a fresh pool, every stack full
    assert_eq!(probe.buf_size(), serves as usize, "the row's stack");
    probe.free();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(base_cpu());
            for i in 0..COUNT {
                let mut buf_slot = pool.alloc::<T>().unwrap(); // OK: alloc+free per iteration, DEPTH never exceeded
                buf_slot.set_seq(i);
                std::hint::black_box(buf_slot.seq());
                buf_slot.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// The same loop through the global allocator (Box::new ->
/// write -> drop) for comparison, pinned to the base cpu. Return
/// elapsed seconds.
///
/// Runs in a scoped thread like the other pinned parts, so
/// the main thread's affinity stays untouched.
fn global_alloc_free_1t() -> f64 {
    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(base_cpu());
            for i in 0..COUNT {
                let mut buf = std::hint::black_box(Box::new(Msg { seq: 0, val: 0 }));
                buf.seq = i;
                std::hint::black_box(buf.seq);
                drop(buf);
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// One ring flavor's stream loops, for the depth sweep.
struct StreamFlavor {
    /// The label the table carries.
    name: &'static str,
    /// The smallest depth the protocol runs at: the MPSC v0
    /// ring rejects 1, where its protocol wedges, so its cell
    /// there is printed as `-`.
    min_depth: u32,
    /// The single-thread loop at a depth.
    one_t: fn(u32) -> f64,
    /// The two-thread loop at a placement and a depth.
    two_t: fn(PinPair, u32) -> f64,
}

/// The flavors the sweep runs, in table order, named `xpsc-vN`
/// after their module paths.
const STREAM_FLAVORS: [StreamFlavor; 7] = [
    StreamFlavor {
        name: "spsc-v0",
        min_depth: 1,
        one_t: spsc_ring_one_msg_1t,
        two_t: spsc_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "spsc-v1",
        min_depth: 1,
        one_t: spsc1_ring_one_msg_1t,
        two_t: spsc1_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "spsc-v2",
        min_depth: 1,
        one_t: spsc2_ring_one_msg_1t,
        two_t: spsc2_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "spsc-v3",
        min_depth: 1,
        one_t: spsc3_ring_one_msg_1t,
        two_t: spsc3_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "mpsc-v0",
        min_depth: 2,
        one_t: mpsc0_ring_one_msg_1t,
        two_t: mpsc0_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "mpsc-v1",
        min_depth: 1,
        one_t: mpsc1_ring_one_msg_1t,
        two_t: mpsc1_ring_one_msg_2t,
    },
    StreamFlavor {
        name: "mpsc-v2",
        min_depth: 1,
        one_t: mpsc2_ring_one_msg_1t,
        two_t: mpsc2_ring_one_msg_2t,
    },
];

/// One row of the segment stress table.
struct StressRow {
    /// The line: ring, shape, thread count.
    line: String,
    /// Where its threads sat.
    placement: String,
    /// Segments times depth.
    shape: String,
    /// Elapsed ns per message, `None` for a line paced by the
    /// consumer's pauses.
    ns: Option<f64>,
    /// Segments the producer wrote into, of the ring's.
    used: (u32, u32),
    /// Switches the producer counted, the consumer agreeing.
    switches: u64,
    /// The cost of one switch in ns, on the row that measures
    /// it.
    switch_ns: Option<f64>,
}

impl StressRow {
    /// Switches per message.
    fn per_msg(&self) -> f64 {
        self.switches as f64 / COUNT as f64
    }
}

/// Print the stress rows as one markdown table.
fn stress_table(rows: &[StressRow]) {
    println!(
        "| {:<17} | {:<16} | {:>6} | {:>7} | {:>5} | {:>8} | {:>6} | {:>9} |",
        "line", "placement", "shape", "ns/msg", "segs", "switches", "sw/msg", "switch ns"
    );
    println!(
        "|{}|{}|{}:|{}:|{}:|{}:|{}:|{}:|",
        "-".repeat(19),
        "-".repeat(18),
        "-".repeat(7),
        "-".repeat(8),
        "-".repeat(6),
        "-".repeat(9),
        "-".repeat(7),
        "-".repeat(10)
    );
    for r in rows {
        println!(
            "| {:<17} | {:<16} | {:>6} | {:>7} | {:>5} | {:>8} | {:>6.3} | {:>9} |",
            r.line,
            r.placement,
            r.shape,
            r.ns.map_or("-".to_string(), |ns| format!("{ns:.1}")),
            format!("{}/{}", r.used.0, r.used.1),
            commas(r.switches),
            r.per_msg(),
            r.switch_ns.map_or("-".to_string(), |ns| format!("{ns:.1}")),
        );
    }
}

/// The burst: one thread, pinned to the base cpu, fills the whole
/// ring of `segments` by `depth` with the consumer idle and then
/// drains it, and again until COUNT messages have moved. Every
/// fill crosses every segment, so this is the switch cost with
/// no thread in the way. Returns seconds, segments used, and
/// both switch counts.
macro_rules! burst_1t {
    ($name:ident, $send:ident, $recv:ident, $($pair:tt)+) => {
        fn $name(segments: u32, depth: u32) -> (f64, u32, (u64, u64)) {
            let capacity = (segments * depth) as u64;
            std::thread::scope(|s| {
                s.spawn(move || {
                    pin_to_cpu(base_cpu());
                    // Built here so its segments belong to this
                    // thread's pool, as the one_msg loops build
                    // theirs on the thread that runs them.
                    $($pair)+!(producer, consumer, store, pool, depth, segmented segments);
                    let mut used = 1u32 << producer.segment();
                    let mut next = 0u64;
                    let start = Instant::now();
                    while next < COUNT {
                        let n = capacity.min(COUNT - next);
                        for i in next..next + n {
                            $send!(producer, i);
                            used |= 1 << producer.segment();
                        }
                        for i in next..next + n {
                            $recv!(consumer, i);
                        }
                        next += n;
                    }
                    let secs = start.elapsed().as_secs_f64();
                    assert!(
                        consumer.reserve_slot_with::<Msg>(|_| false).is_err(),
                        concat!(stringify!($name), ": not drained")
                    );
                    (secs, used.count_ones(), (producer.switches(), consumer.switches()))
                })
                .join()
                .unwrap() // OK: a panic in the loop is the demo's failure
            })
        }
    };
}

/// The lagging consumer: the producer streams COUNT messages
/// spinning, the consumer reads two segments' worth and pauses
/// [`LAG_PAUSE`], so the producer runs ahead across segments at
/// every pause and the ring is Full only when every segment is.
/// Its pace is the consumer's pauses, so it returns no time:
/// segments used and both switch counts.
macro_rules! lagging_2t {
    ($name:ident, $send:ident, $recv:ident, $($pair:tt)+) => {
        fn $name(pin: PinPair, segments: u32, depth: u32) -> (u32, (u64, u64)) {
            let burst = 2 * depth as u64;
            $($pair)+!(producer, consumer, store, pool, depth, segmented segments);
            let (used, sent) = std::thread::scope(|s| {
                let producer = s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    let mut used = 1u32 << producer.segment();
                    for i in 0..COUNT {
                        $send!(producer, i);
                        used |= 1 << producer.segment();
                    }
                    (used, producer.switches())
                });
                let consumer = &mut consumer;
                s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    let mut i = 0u64;
                    while i < COUNT {
                        let n = burst.min(COUNT - i);
                        for j in i..i + n {
                            $recv!(consumer, j);
                        }
                        i += n;
                        std::thread::sleep(LAG_PAUSE);
                    }
                });
                producer.join().unwrap() // OK: a panic in the loop is the demo's failure
            });
            assert!(
                consumer.reserve_slot_with::<Msg>(|_| false).is_err(),
                concat!(stringify!($name), ": not drained")
            );
            (used.count_ones(), (sent, consumer.switches()))
        }
    };
}

/// The stream: the producer and the consumer on their own
/// threads, both spinning, COUNT messages through a ring of
/// `segments` by `depth`, the two_t loops' shape with the
/// switches counted. Returns seconds, segments used, and both
/// switch counts.
macro_rules! stream_2t {
    ($name:ident, $send:ident, $recv:ident, $($pair:tt)+) => {
        fn $name(pin: PinPair, segments: u32, depth: u32) -> (f64, u32, (u64, u64)) {
            $($pair)+!(producer, consumer, store, pool, depth, segmented segments);
            let start = Instant::now();
            let (used, sent) = std::thread::scope(|s| {
                let producer = s.spawn(move || {
                    if let Some((p, _)) = pin {
                        pin_to_cpu(p);
                    }
                    let mut used = 1u32 << producer.segment();
                    for i in 0..COUNT {
                        $send!(producer, i);
                        used |= 1 << producer.segment();
                    }
                    (used, producer.switches())
                });
                let consumer = &mut consumer;
                s.spawn(move || {
                    if let Some((_, c)) = pin {
                        pin_to_cpu(c);
                    }
                    for i in 0..COUNT {
                        $recv!(consumer, i);
                    }
                });
                producer.join().unwrap() // OK: a panic in the loop is the demo's failure
            });
            let secs = start.elapsed().as_secs_f64();
            (secs, used.count_ones(), (sent, consumer.switches()))
        }
    };
}

/// One SPSC send, spinning: reserve, write, commit.
macro_rules! spsc_send {
    ($producer:ident, $i:expr) => {{
        let mut slot = $producer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
        slot.seq = $i;
        slot.commit();
    }};
}

/// One MPSC send, spinning: the closure fill.
macro_rules! mpsc_send {
    ($producer:ident, $i:expr) => {{
        $producer
            .send_with::<Msg>(policy::spin, |m| m.seq = $i)
            .unwrap(); // OK: policy::spin never gives up
    }};
}

/// One receive, spinning, the sequence asserted: the endpoint
/// surface both consumers share.
macro_rules! ring_recv {
    ($consumer:ident, $i:expr) => {{
        let msg = $consumer.reserve_slot_with::<Msg>(policy::spin).unwrap(); // OK: policy::spin never gives up
        assert_eq!(msg.seq, $i);
        msg.release();
    }};
}

burst_1t!(spsc3_burst_1t, spsc_send, ring_recv, spsc_pair);
burst_1t!(mpsc2_burst_1t, mpsc_send, ring_recv, mpsc_pair);
lagging_2t!(spsc3_lagging_2t, spsc_send, ring_recv, spsc_pair);
lagging_2t!(mpsc2_lagging_2t, mpsc_send, ring_recv, mpsc_pair);
stream_2t!(spsc3_stream_2t, spsc_send, ring_recv, spsc_pair);
stream_2t!(mpsc2_stream_2t, mpsc_send, ring_recv, mpsc_pair);

/// The two shapes the switch cost is a difference between: the
/// same 32 slots as one segment, which never switches, and as 32
/// segments of one slot, which switches on nearly every message.
const COST_SHAPES: [(u32, u32); 2] = [(1, 32), (32, 1)];

/// Run a pair of [`COST_SHAPES`] lines through `run`, push both
/// rows, and put the cost of one switch, the gap in ns per
/// message over the gap in switches per message, on the second.
fn switch_cost(
    rows: &mut Vec<StressRow>,
    line: &str,
    placement: &str,
    mut run: impl FnMut(u32, u32) -> (f64, u32, (u64, u64)),
) {
    let mut pair: Vec<StressRow> = COST_SHAPES
        .iter()
        .map(|&(segments, depth)| {
            let (secs, used, (sent, seen)) = run(segments, depth);
            assert_eq!(sent, seen, "{line}: switch counts differ");
            StressRow {
                line: line.to_string(),
                placement: placement.to_string(),
                shape: format!("{segments}x{depth}"),
                ns: Some(secs * 1e9 / COUNT as f64),
                used: (used, segments),
                switches: sent,
                switch_ns: None,
            }
        })
        .collect();
    let (a, b) = (&pair[0], &pair[1]);
    let cost = (b.ns.unwrap_or(0.0) - a.ns.unwrap_or(0.0)) / (b.per_msg() - a.per_msg()); // OK: both ns are Some, set just above
    pair[1].switch_ns = Some(cost);
    rows.append(&mut pair);
}

/// Print a legend entry wrapped at 80 columns, `- ` on its
/// first line and two spaces under it on the rest.
fn legend(text: &str) {
    let mut line = String::from("-");
    for word in text.split_whitespace() {
        if line.len() + 1 + word.len() > 80 {
            println!("{line}");
            line = String::from(" ");
        }
        line.push(' ');
        line.push_str(word);
    }
    println!("{line}");
}

/// Run the segment stress and print it as one table: the burst
/// on one thread and the lagging consumer at each placement, at
/// [`STRESS_SEGMENTS`] segments of DEPTH, then the switch cost
/// at depth 1, single-threaded and streaming across cores.
fn segment_stress(placements: &[Placement]) {
    println!(
        "segment stress: {} messages per line, spsc-v3 and mpsc-v2 at {STRESS_SEGMENTS} segments \
         of {DEPTH} slots, then the switch cost at depth 1",
        commas(COUNT)
    );
    println!();
    let mut rows = Vec::new();
    let shape = format!("{STRESS_SEGMENTS}x{DEPTH}");
    for (line, run) in [
        (
            "spsc3 burst 1t",
            spsc3_burst_1t as fn(u32, u32) -> (f64, u32, (u64, u64)),
        ),
        ("mpsc2 burst 1t", mpsc2_burst_1t),
    ] {
        let (secs, used, (sent, seen)) = run(STRESS_SEGMENTS, DEPTH);
        assert_eq!(sent, seen, "{line}: switch counts differ");
        rows.push(StressRow {
            line: line.to_string(),
            placement: format!("core {}", base_cpu()),
            shape: shape.clone(),
            ns: Some(secs * 1e9 / COUNT as f64),
            used: (used, STRESS_SEGMENTS),
            switches: sent,
            switch_ns: None,
        });
    }
    for Placement {
        label: placement,
        pin,
    } in placements
    {
        for (line, run) in [
            (
                "spsc3 lagging 2t",
                spsc3_lagging_2t as fn(PinPair, u32, u32) -> (u32, (u64, u64)),
            ),
            ("mpsc2 lagging 2t", mpsc2_lagging_2t),
        ] {
            let (used, (sent, seen)) = run(*pin, STRESS_SEGMENTS, DEPTH);
            assert_eq!(sent, seen, "{line}: switch counts differ");
            rows.push(StressRow {
                line: line.to_string(),
                placement: placement.clone(),
                shape: shape.clone(),
                ns: None,
                used: (used, STRESS_SEGMENTS),
                switches: sent,
                switch_ns: None,
            });
        }
    }
    let one_t = format!("core {}", base_cpu());
    switch_cost(&mut rows, "spsc3 burst 1t", &one_t, spsc3_burst_1t);
    switch_cost(&mut rows, "mpsc2 burst 1t", &one_t, mpsc2_burst_1t);
    // The stream across cores at the farthest placement the
    // machine has, x-CCX, else CCX, else unpinned.
    let Placement {
        label: placement,
        pin,
    } = placements
        .iter()
        .find(|p| p.label.ends_with(" x-CCX"))
        .or_else(|| placements.iter().find(|p| p.label.ends_with(" CCX")))
        .unwrap_or_else(|| &placements[placements.len() - 1]);
    let (placement, pin) = (placement.clone(), *pin);
    switch_cost(
        &mut rows,
        "spsc3 stream 2t",
        &placement,
        |segments, depth| spsc3_stream_2t(pin, segments, depth),
    );
    switch_cost(
        &mut rows,
        "mpsc2 stream 2t",
        &placement,
        |segments, depth| mpsc2_stream_2t(pin, segments, depth),
    );
    stress_table(&rows);
    println!();
    legend(&format!(
        "line: the ring and the shape of the run. burst 1t: one thread fills every segment with \
         the consumer idle, then drains, until the messages are moved. lagging 2t: the producer \
         streams while the consumer reads two segments' worth between {}us pauses, so the producer \
         runs ahead across segments at every pause. stream 2t: both spinning, the two_t loops' \
         shape.",
        LAG_PAUSE.as_micros()
    ));
    legend(
        "shape: segments x slots per segment. The first rows are the stress shape. The switch \
         cost rows are the same 32 slots as one segment, which never switches, and as 32 segments \
         of one slot, which switches on nearly every message.",
    );
    legend(&format!(
        "ns/msg: elapsed over the messages moved, `-` where the line's pace is the consumer's \
         pauses. segs: segments the producer wrote into, of the ring's. switches: segment \
         switches, the producer's count, which the consumer's matched. sw/msg: switches per \
         message, the burst's 3 per {} at the stress shape.",
        STRESS_SEGMENTS * DEPTH
    ));
    legend(
        "switch ns: the cost of one switch, the gap in ns/msg between the two shapes over the gap \
         in sw/msg, on the 32x1 row. Single-threaded it is the instructions alone. Streaming across \
         cores it includes the cold segment crossing.",
    );
}

/// Where a sweep row's threads sit.
enum SweepPlacement {
    /// Both ends on one thread, pinned to the base cpu.
    OneT,
    /// Producer and consumer threads at `PinPair`.
    TwoT(PinPair),
}

/// Run every flavor at every depth of [`DEPTHS`] at each
/// placement the machine offers, and print one markdown table
/// per placement, ns per message in the depth columns.
///
/// - The same loops as the lines above, so a `d=64` cell and
///   the matching line agree up to run noise.
/// - Depth 1 is lockstep: every message is its own handoff, so
///   streaming there costs what a round trip does.
fn depth_sweep(two_t: &[Placement]) {
    let mut placements = vec![(format!("1t core {}", base_cpu()), SweepPlacement::OneT)];
    for Placement { label, pin } in two_t {
        placements.push((format!("2t {label}"), SweepPlacement::TwoT(*pin)));
    }
    let depth_list: Vec<String> = DEPTHS.iter().map(|d| d.to_string()).collect();
    println!(
        "depth sweep: {} messages per cell, ns/msg at depths {}, spsc-v3 and mpsc-v2 with {SWEEP_SEGMENTS} segment(s)",
        commas(COUNT),
        depth_list.join(", ")
    );
    for (label, placement) in &placements {
        println!();
        let mut header = format!("| {label:<22} |");
        let mut sep = format!("|{}|", "-".repeat(24));
        for d in &depth_list {
            header.push_str(&format!(" {:>7} |", format!("d={d}")));
            sep.push_str(&format!("{}:|", "-".repeat(8)));
        }
        println!("{header}");
        println!("{sep}");
        for flavor in &STREAM_FLAVORS {
            let mut row = format!("| {:<22} |", flavor.name);
            for &depth in &DEPTHS {
                if depth < flavor.min_depth {
                    row.push_str(&format!(" {:>7} |", "-"));
                    continue;
                }
                let secs = match placement {
                    SweepPlacement::OneT => (flavor.one_t)(depth),
                    SweepPlacement::TwoT(pin) => (flavor.two_t)(*pin, depth),
                };
                row.push_str(&format!(" {:>7.1} |", secs * 1e9 / COUNT as f64));
            }
            println!("{row}");
        }
    }
}

/// The 2t lines at one placement: every ring flavor, the
/// composed flow, and the std channel, each labelled with the
/// placement.
fn two_t_lines(label: &str, pin: PinPair) {
    report(
        &format!("spsc_ring_one_msg_2t ({label}):"),
        spsc_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("spsc1_ring_one_msg_2t ({label}):"),
        spsc1_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("spsc2_ring_one_msg_2t ({label}):"),
        spsc2_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("spsc3_ring_one_msg_2t ({label}):"),
        spsc3_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("mpsc0_ring_one_msg_2t ({label}):"),
        mpsc0_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("mpsc1_ring_one_msg_2t ({label}):"),
        mpsc1_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("mpsc2_ring_one_msg_2t ({label}):"),
        mpsc2_ring_one_msg_2t(pin, DEPTH),
    );
    report(
        &format!("spsc_ring_one_pool_msg_2t ({label}):"),
        spsc_ring_one_pool_msg_2t(pin),
    );
    report(
        &format!("std_mpsc_one_pool_msg_2t ({label}):"),
        std_mpsc_one_pool_msg_2t(pin),
    );
}

/// Run both parts and print their throughput, then the depth
/// sweep. `--base-cpu <n>` sets the base, `-h` /
/// `--help` prints the usage, and `-V` / `--version` prints the
/// version-of-record and exits.
fn main() {
    let banner = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));
    println!("{banner}");
    let base = match parse_args(std::env::args().skip(1)) {
        Args::Run { base_cpu } => base_cpu,
        Args::Exit { code } => std::process::exit(code),
    };
    BASE_CPU.store(base, Ordering::Relaxed);
    println!(
        "demo: {} messages each, depth {DEPTH}, base cpu {base}",
        commas(COUNT)
    );
    let placements = discover_placements(base);

    // Alloc/free baselines, then message flows: like
    // compares with like within each block.
    report(
        &format!("pool_alloc_free_1t (core {base}):"),
        pool_alloc_free_1t(),
    );
    report(
        &format!("pool1_alloc_free_1t 1 stack (core {base}):"),
        pool1_alloc_free_1t::<1, Msg>(POOL1_ONE_STACK, CACHE_LINE_SIZE as u32),
    );
    report(
        &format!("pool1_alloc_free_1t 4 stacks, 1st (core {base}):"),
        pool1_alloc_free_1t::<4, Msg>(POOL1_FOUR_STACKS, CACHE_LINE_SIZE as u32),
    );
    report(
        &format!("pool1_alloc_free_1t 4 stacks, 4th (core {base}):"),
        pool1_alloc_free_1t::<4, Msg8>(POOL1_FOUR_STACKS, 8 * CACHE_LINE_SIZE as u32),
    );
    report(
        &format!("global_alloc_free_1t (core {base}):"),
        global_alloc_free_1t(),
    );

    // Single thread, the base cpu.
    println!();
    report(
        &format!("spsc_ring_one_msg_1t (core {base}):"),
        spsc_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("spsc1_ring_one_msg_1t (core {base}):"),
        spsc1_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("spsc2_ring_one_msg_1t (core {base}):"),
        spsc2_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("spsc3_ring_one_msg_1t (core {base}):"),
        spsc3_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("mpsc0_ring_one_msg_1t (core {base}):"),
        mpsc0_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("mpsc1_ring_one_msg_1t (core {base}):"),
        mpsc1_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("mpsc2_ring_one_msg_1t (core {base}):"),
        mpsc2_ring_one_msg_1t(DEPTH),
    );
    report(
        &format!("spsc_ring_one_pool_msg_1t (core {base}):"),
        spsc_ring_one_pool_msg_1t(),
    );
    report(
        &format!("std_mpsc_one_pool_msg_1t (core {base}):"),
        std_mpsc_one_pool_msg_1t(),
    );

    // Two threads at each placement the machine has.
    for Placement { label, pin } in &placements {
        println!();
        two_t_lines(label, *pin);
    }

    // Three threads (2 producers + 1 consumer): the
    // multi-producer line only the MPSC ring can run.
    report(
        "mpsc1_ring_one_msg_3t (2p+1c unpinned):",
        mpsc1_ring_one_msg_3t(),
    );

    // The depth sweep, the ring flavors at every placement.
    println!();
    depth_sweep(&placements);

    // The segment stress, the segmented rings switching.
    println!();
    segment_stress(&placements);
}
