//! Demo binary: both primitives working across threads,
//! with throughput printed — run `cargo run --release`, or
//! `cargo install --path . --locked` and run
//! `zc-ring-x1-demo`. `-V`/`--version` prints the
//! version-of-record so you know exactly which build you
//! are testing.
//!
//! - Part 1, the ring: an SPSC pair moves typed messages
//!   in place (reserve_slot → write → commit; reserve_slot
//!   → read → release) — first both ends on one thread
//!   (the ring's own cost), then one producer thread to
//!   one consumer thread: unpinned, pinned to one physical
//!   core's two SMT siblings (shared L1/L2), and pinned to
//!   two different physical cores.
//! - Part 2, the pool: an allocator thread allocs and
//!   fills `BufSlot`s and hands them to a freer thread —
//!   "send" today is moving the guard (see the README's
//!   usage model); getting a buffer implies nothing about
//!   when it is sent or freed.
//! - The composed form — descriptors through the ring,
//!   payloads at rest in pool buffers — is the next cycle;
//!   this demo shows each primitive honestly on its own.

use std::time::Instant;

use zc_ring_x1::{BufSlot, CACHE_LINE_SIZE, Empty, Exhausted, Full, Pool, Ring};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Messages moved per part.
const COUNT: u64 = 1_000_000;

/// Ring slots / pool buffers (small on purpose: recycling
/// under pressure is the interesting case).
const DEPTH: u32 = 64;

/// The demo message; one word carrying the sequence number
/// the consumer asserts.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
#[repr(C)]
struct Msg {
    seq: u64,
}

/// Region for either primitive: biggest header (ring, 4
/// lines) + DEPTH one-line slots/buffers.
#[repr(C, align(64))]
struct Region([u8; 4 * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE]);

/// Group a count into comma-separated thousands
/// (`1234567` → `"1,234,567"`).
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
    println!("{label:<43} {rate:>12} msgs/sec  {ns_per_msg:>7.1} ns/msg");
}

/// Parse a /sys cpu-list string ("0,12" or "0-2,6") into cpu
/// numbers; malformed pieces are skipped.
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

/// A `(producer cpu, consumer cpu)` pin for a 2t run;
/// `None` means the pair is unavailable / leave unpinned.
type PinPair = Option<(usize, usize)>;

/// Discover two cpu pairs for the pinned 2t runs from
/// /sys/devices/system/cpu; `None` when the machine lacks the
/// shape (or not Linux).
///
/// - `.0` — same physical core: cpu0 and its SMT sibling
///   (shared L1/L2, the cheapest handoff).
/// - `.1` — different physical cores: cpu0 and a core outside
///   cpu0's L3 group when one exists (the farthest handoff),
///   else any non-sibling core.
#[cfg(target_os = "linux")]
fn discover_pin_pairs() -> (PinPair, PinPair) {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let siblings = read("/sys/devices/system/cpu/cpu0/topology/thread_siblings_list")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let smt = (siblings.len() >= 2).then(|| (siblings[0], siblings[1]));
    let online = read("/sys/devices/system/cpu/online")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let l3 = read("/sys/devices/system/cpu/cpu0/cache/index3/shared_cpu_list")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_else(|| siblings.clone());
    let far = online
        .iter()
        .copied()
        .find(|c| !l3.contains(c))
        .or_else(|| {
            online
                .iter()
                .copied()
                .find(|c| *c != 0 && !siblings.contains(c))
        });
    (smt, far.map(|c| (0, c)))
}

/// Non-Linux stub: no pinned runs.
#[cfg(not(target_os = "linux"))]
fn discover_pin_pairs() -> (PinPair, PinPair) {
    (None, None)
}

/// Pin the calling thread to `cpu` via sched_setaffinity;
/// panics on failure (a demo run with a silently ignored pin
/// would report a mislabeled number).
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

/// Non-Linux stub: pinning is a no-op, so only the 1t runs
/// reach it (discover_pin_pairs returns no pairs); present
/// so the demo compiles everywhere.
#[cfg(not(target_os = "linux"))]
fn pin_to_cpu(_cpu: usize) {}

/// Move COUNT messages single thread through the ring,
/// pinned to cpu 0 for consistency with the pinned 2t runs;
/// return elapsed seconds.
///
/// The loop runs in a scoped thread rather than pinning the
/// main thread: spawned threads inherit the main thread's
/// affinity mask, which would squeeze every later part onto
/// cpu 0.
fn spsc_ring_one_msg_1t() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let (mut producer, mut consumer) = Ring::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the ring header + DEPTH slots
        .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
            for i in 0..COUNT {
                match producer.reserve_slot::<Msg>() {
                    Ok(mut slot) => {
                        slot.seq = i;
                        slot.commit();
                    }
                    Err(Full) => {
                        panic!("spsc_ring_one_msg_1t: producer Full SHOULD NOT HAPPEN");
                    }
                }
                match consumer.reserve_slot::<Msg>() {
                    Ok(msg) => {
                        assert_eq!(msg.seq, i);
                        msg.release();
                    }
                    Err(Empty) => {
                        panic!("spsc_ring_one_msg_1t: consumer Empty SHOULD NOT HAPPEN");
                    }
                }
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Move COUNT messages producer-thread → consumer-thread
/// through the ring; return elapsed seconds.
///
/// - `pin` — `Some((p, c))` pins the producer to cpu `p` and
///   the consumer to cpu `c`; `None` lets the scheduler place
///   them (the number then depends on where they land).
fn spsc_ring_one_msg_2t(pin: PinPair) -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let (mut producer, mut consumer) = Ring::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap() // OK: Region is sized/aligned for the ring header + DEPTH slots
        .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some((p, _)) = pin {
                pin_to_cpu(p);
            }
            for i in 0..COUNT {
                loop {
                    match producer.reserve_slot::<Msg>() {
                        Ok(mut slot) => {
                            slot.seq = i;
                            slot.commit();
                            break;
                        }
                        Err(Full) => std::hint::spin_loop(),
                    }
                }
            }
        });
        s.spawn(move || {
            if let Some((_, c)) = pin {
                pin_to_cpu(c);
            }
            for i in 0..COUNT {
                loop {
                    match consumer.reserve_slot::<Msg>() {
                        Ok(msg) => {
                            assert_eq!(msg.seq, i);
                            msg.release();
                            break;
                        }
                        Err(Empty) => std::hint::spin_loop(),
                    }
                }
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Alloc + fill COUNT messages on an allocator thread,
/// free them on a freer thread (guards cross a std
/// channel); return elapsed seconds.
///
/// - `pin` — `Some((a, f))` pins the allocator to cpu `a`
///   and the freer to cpu `f`; `None` lets the scheduler
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
                        Ok(mut buf) => {
                            buf.seq = i;
                            tx.send(buf).unwrap();
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
                let buf = rx.recv().unwrap();
                assert_eq!(buf.seq, i);
                buf.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Alloc → write → free COUNT messages on one thread pinned
/// to cpu 0 — the pool's own cost, no channel, no second
/// thread; return elapsed seconds.
///
/// Runs in a scoped thread like the other pinned parts, so
/// the main thread's affinity stays untouched.
fn pool_alloc_free_1t() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
            for i in 0..COUNT {
                let mut buf = pool.alloc::<Msg>().unwrap(); // OK: alloc+free per iteration, DEPTH never exceeded
                buf.seq = i;
                std::hint::black_box(buf.seq);
                buf.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// The same loop through the global allocator (Box::new →
/// write → drop) for comparison, pinned to cpu 0; return
/// elapsed seconds.
///
/// Runs in a scoped thread like the other pinned parts, so
/// the main thread's affinity stays untouched.
fn global_alloc_free_1t() -> f64 {
    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
            for i in 0..COUNT {
                let mut buf = std::hint::black_box(Box::new(Msg { seq: 0 }));
                buf.seq = i;
                std::hint::black_box(buf.seq);
                drop(buf);
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Run both parts and print their throughput; `-V` /
/// `--version` prints the version-of-record and exits.
fn main() {
    let banner = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));
    println!("{banner}");
    if std::env::args().any(|a| a == "-V" || a == "--version") {
        return;
    }
    println!("demo: {} messages each, depth {DEPTH}", commas(COUNT));
    report("spsc_ring_one_msg_1t (core 0):", spsc_ring_one_msg_1t());
    report(
        "spsc_ring_one_msg_2t (unpinned):",
        spsc_ring_one_msg_2t(None),
    );
    let (smt, far) = discover_pin_pairs();
    match smt {
        Some((p, c)) => report(
            &format!("spsc_ring_one_msg_2t (same core {p}+{c}):"),
            spsc_ring_one_msg_2t(smt),
        ),
        None => {
            println!("spsc_ring_one_msg_2t (same core):           skipped, no SMT sibling found")
        }
    }
    match far {
        Some((p, c)) => report(
            &format!("spsc_ring_one_msg_2t (diff cores {p}+{c}):"),
            spsc_ring_one_msg_2t(far),
        ),
        None => {
            println!("spsc_ring_one_msg_2t (diff cores):          skipped, only one core found")
        }
    }
    report(
        "std_mpsc_one_pool_msg_2t (unpinned):",
        std_mpsc_one_pool_msg_2t(None),
    );
    match smt {
        Some((p, c)) => report(
            &format!("std_mpsc_one_pool_msg_2t (same core {p}+{c}):"),
            std_mpsc_one_pool_msg_2t(smt),
        ),
        None => {
            println!("std_mpsc_one_pool_msg_2t (same core):       skipped, no SMT sibling found")
        }
    }
    match far {
        Some((p, c)) => report(
            &format!("std_mpsc_one_pool_msg_2t (diff cores {p}+{c}):"),
            std_mpsc_one_pool_msg_2t(far),
        ),
        None => {
            println!("std_mpsc_one_pool_msg_2t (diff cores):      skipped, only one core found")
        }
    }
    report("pool_alloc_free_1t (core 0):", pool_alloc_free_1t());
    report("global_alloc_free_1t (core 0):", global_alloc_free_1t());
    println!("(composed descriptor flow arrives with the descriptor-queue cycle)");
}
