//! Demo binary: both primitives working across threads,
//! with throughput printed: run `cargo run --release`, or
//! `cargo install --path . --locked` and run
//! `zc-ring-x1-demo`. `-V`/`--version` prints the
//! version-of-record so you know exactly which build you
//! are testing.
//!
//! - Part 1, the ring: an SPSC pair moves typed messages
//!   in place (reserve -> write -> commit, reserve -> read ->
//!   release), first both ends on one thread
//!   (the ring's own cost), then one producer thread to
//!   one consumer thread: unpinned, pinned to two different
//!   physical cores, and pinned to one physical core's two
//!   SMT siblings (shared L1/L2, when the CPU has SMT).
//!   The SPSC v1 seam-word ring (`spsc1_` lines), the SPSC v2
//!   in-slot seq ring (`spsc2_` lines), and the MPSC sibling
//!   run beside it at each placement
//!   (send_with closure fill), plus a 2-producer + 1-consumer
//!   line: the shape only the MPSC ring can run.
//! - The depth sweep, last: the ring flavors again at every
//!   placement and at depths 1, 2, 8, and 64, one table per
//!   placement, so depth and protocol can be told apart.
//! - Part 2, the pool: an allocator thread allocs and
//!   fills `BufSlot`s and hands them to a freer thread.
//!   "send" today is moving the guard (see the README's
//!   usage model). Getting a buffer implies nothing about
//!   when it is sent or freed.
//! - The composed form (descriptors through the ring,
//!   payloads at rest in pool buffers) runs between them:
//!   alloc -> into_desc -> ring -> resolve -> free, with the
//!   same placement ladder as the raw ring.

use std::time::Instant;

use zc_ring_x1::{
    BufSlot, CACHE_LINE_SIZE, Desc, Empty, Exhausted, Full, MpscRing, Pool, PoolRegistry, Ring,
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

/// Discover two cpu pairs for the pinned 2t runs from
/// /sys/devices/system/cpu. `None` when the machine lacks the
/// shape (or not Linux).
///
/// - `.0`: same physical core, cpu0 and its SMT sibling
///   (shared L1/L2, the cheapest handoff).
/// - `.1`: different physical cores, cpu0 and a core outside
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
/// reach it (discover_pin_pairs returns no pairs), present
/// so the demo compiles everywhere.
#[cfg(not(target_os = "linux"))]
fn pin_to_cpu(_cpu: usize) {}

/// Define the SPSC one-message loops over the ring at `$ring`,
/// its region sized by `$size(slot_size, depth)`: `$one_t`
/// moves COUNT messages single thread, pinned to cpu 0, and
/// `$two_t` moves them producer-thread -> consumer-thread (`pin`
/// as in [`spsc_ring_one_msg_2t`]), both over a ring of `depth`
/// slots. Each returns elapsed seconds. The SPSC versions share
/// the endpoint surface and differ by path, so one body serves
/// them all and the lines read as the protocol seam alone.
///
/// The 1t loop runs in a scoped thread rather than pinning the
/// main thread: spawned threads inherit the main thread's
/// affinity mask, which would squeeze every later part onto
/// cpu 0.
macro_rules! spsc_loops {
    ($one_t:ident, $two_t:ident, $ring:path, $size:path) => {
        fn $one_t(depth: u32) -> f64 {
            let slot = CACHE_LINE_SIZE as u32;
            let mut region = region($size(slot, depth));
            let (mut producer, mut consumer) = <$ring>::init(region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
                .split();

            let start = Instant::now();
            std::thread::scope(|s| {
                s.spawn(move || {
                    pin_to_cpu(0);
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
            let slot = CACHE_LINE_SIZE as u32;
            let mut region = region($size(slot, depth));
            let (mut producer, mut consumer) = <$ring>::init(region.as_mut_bytes(), slot, depth)
                .unwrap() // OK: the region is sized by the ring's own size function and line-aligned
                .split();

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
    zc_ring_x1::spsc::v0::Ring,
    v0_region_size
);
spsc_loops!(
    spsc1_ring_one_msg_1t,
    spsc1_ring_one_msg_2t,
    zc_ring_x1::spsc::v1::Ring,
    zc_ring_x1::spsc::v1::region_size
);
spsc_loops!(
    spsc2_ring_one_msg_1t,
    spsc2_ring_one_msg_2t,
    zc_ring_x1::spsc::v2::Ring,
    zc_ring_x1::spsc::v2::region_size
);

/// Move COUNT messages single thread through the MPSC ring,
/// pinned to cpu 0, the sibling of spsc_ring_one_msg_1t, so
/// the two lines read as the seam between the protocols
/// (claim CAS + seq vs load/store). Return elapsed seconds.
fn mpsc_ring_one_msg_1t(depth: u32) -> f64 {
    let slot = CACHE_LINE_SIZE as u32;
    let mut region = region(mpsc_region_size(slot, depth));
    let (producer, mut consumer) = MpscRing::init(region.as_mut_bytes(), slot, depth)
        .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
        .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
            for i in 0..COUNT {
                producer.send_with::<Msg>(|_| false, |m| m.seq = i).unwrap(); // OK: room is guaranteed, the consumer drains in lockstep
                match consumer.reserve_slot_with::<Msg>(|_| false) {
                    Ok(msg) => {
                        assert_eq!(msg.seq, i);
                        msg.release();
                    }
                    Err(Empty) => {
                        panic!("mpsc_ring_one_msg_1t: consumer Empty SHOULD NOT HAPPEN");
                    }
                }
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Move COUNT messages producer-thread -> consumer-thread
/// through the MPSC ring, the sibling of
/// spsc_ring_one_msg_2t at the same placements, measuring
/// what the MPSC protocol costs when you don't need multiple
/// producers. Return elapsed seconds.
///
/// - `pin`: `Some((p, c))` pins the producer to cpu `p` and
///   the consumer to cpu `c`. `None` lets the scheduler place
///   them (the number then depends on where they land).
fn mpsc_ring_one_msg_2t(pin: PinPair, depth: u32) -> f64 {
    let slot = CACHE_LINE_SIZE as u32;
    let mut region = region(mpsc_region_size(slot, depth));
    let (producer, mut consumer) = MpscRing::init(region.as_mut_bytes(), slot, depth)
        .unwrap() // OK: the region is sized by mpsc_region_size and line-aligned
        .split();

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

/// Move COUNT messages (COUNT/2 per producer) from two
/// producer threads into one consumer, the line the SPSC
/// ring cannot produce: claim contention on the shared
/// producer index. Unpinned (a pinned variant would need a
/// third discovered cpu). Per-producer FIFO is asserted, the
/// interleave is whatever the claim race said. Returns
/// elapsed seconds.
fn mpsc_ring_one_msg_3t() -> f64 {
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

/// The composed flow on one thread pinned to cpu 0: one pool
/// message allocated outside the timed loop. Each iteration
/// populates it, converts guard -> descriptor, rings the
/// descriptor across, and resolves the guard back. Return
/// elapsed seconds.
///
/// - Isolates messaging cost (into_desc + ring + resolve)
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
    let pool_id = registry.register(pool.resolver()).unwrap(); // OK: empty capacity-1 registry always has room
    let (mut producer, mut consumer) =
        Ring::init(&mut ring_region.0, CACHE_LINE_SIZE as u32, DEPTH)
            .unwrap() // OK: Region is sized/aligned for the ring header + DEPTH slots
            .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
            let mut buf_slot = pool.alloc::<Msg>().unwrap(); // OK: fresh pool, DEPTH buffers free
            for i in 0..COUNT {
                buf_slot.seq = i;
                let desc = registry
                    .into_desc(pool_id, buf_slot)
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
                        // the ring by into_desc above and is
                        // resolved exactly once, same thread.
                        let msg = unsafe { registry.resolve::<Msg>(desc) }.unwrap(); // OK: desc came from into_desc on this pool
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
/// cross the SPSC ring, the consumer resolves and frees.
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
    let pool_id = registry.register(pool.resolver()).unwrap(); // OK: empty capacity-1 registry always has room
    let registry = &registry;
    let (mut producer, mut consumer) =
        Ring::init(&mut ring_region.0, CACHE_LINE_SIZE as u32, DEPTH)
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
                    .into_desc(pool_id, buf_slot)
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
                // resolved exactly once.
                let msg = unsafe { registry.resolve::<Msg>(desc) }.unwrap(); // OK: descs here only come from the producer's into_desc
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

/// The std-channel flow on one thread pinned to cpu 0: alloc
/// a pool message, move its guard through a sync_channel,
/// receive and free it. Return elapsed seconds.
fn std_mpsc_one_pool_msg_1t() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap(); // OK: Region is sized/aligned for the pool header + DEPTH buffers
    let (tx, rx) = std::sync::mpsc::sync_channel::<BufSlot<'_, Msg>>(DEPTH as usize);

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            pin_to_cpu(0);
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
/// to cpu 0, the pool's own cost, no channel, no second
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
            pin_to_cpu(0);
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

/// The same loop through the global allocator (Box::new ->
/// write -> drop) for comparison, pinned to cpu 0. Return
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

/// The flavors the sweep runs, in table order.
const STREAM_FLAVORS: [StreamFlavor; 4] = [
    StreamFlavor {
        name: "spsc",
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
        name: "mpsc",
        min_depth: 2,
        one_t: mpsc_ring_one_msg_1t,
        two_t: mpsc_ring_one_msg_2t,
    },
];

/// Where a sweep row's threads sit.
enum SweepPlacement {
    /// Both ends on one thread, pinned to cpu 0.
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
fn depth_sweep(smt: PinPair, far: PinPair) {
    let mut placements = vec![
        ("1t core 0".to_string(), SweepPlacement::OneT),
        ("2t unpinned".to_string(), SweepPlacement::TwoT(None)),
    ];
    if let Some((p, c)) = far {
        placements.push((format!("2t diff cores {p}+{c}"), SweepPlacement::TwoT(far)));
    }
    if let Some((p, c)) = smt {
        placements.push((format!("2t same core {p}+{c}"), SweepPlacement::TwoT(smt)));
    }
    let depth_list: Vec<String> = DEPTHS.iter().map(|d| d.to_string()).collect();
    println!(
        "depth sweep: {} messages per cell, ns/msg at depths {}",
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

/// Run both parts and print their throughput, then the depth
/// sweep. `-V` / `--version` prints the version-of-record and
/// exits.
fn main() {
    let banner = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));
    println!("{banner}");
    if std::env::args().any(|a| a == "-V" || a == "--version") {
        return;
    }
    println!("demo: {} messages each, depth {DEPTH}", commas(COUNT));
    let (smt, far) = discover_pin_pairs();

    // Alloc/free baselines, then message flows: like
    // compares with like within each block.
    report("pool_alloc_free_1t (core 0):", pool_alloc_free_1t());
    report("global_alloc_free_1t (core 0):", global_alloc_free_1t());

    // Single thread, core 0.
    println!();
    report(
        "spsc_ring_one_msg_1t (core 0):",
        spsc_ring_one_msg_1t(DEPTH),
    );
    report(
        "spsc1_ring_one_msg_1t (core 0):",
        spsc1_ring_one_msg_1t(DEPTH),
    );
    report(
        "spsc2_ring_one_msg_1t (core 0):",
        spsc2_ring_one_msg_1t(DEPTH),
    );
    report(
        "mpsc_ring_one_msg_1t (core 0):",
        mpsc_ring_one_msg_1t(DEPTH),
    );
    report(
        "spsc_ring_one_pool_msg_1t (core 0):",
        spsc_ring_one_pool_msg_1t(),
    );
    report(
        "std_mpsc_one_pool_msg_1t (core 0):",
        std_mpsc_one_pool_msg_1t(),
    );

    // Two threads, scheduler-placed.
    println!();
    report(
        "spsc_ring_one_msg_2t (unpinned):",
        spsc_ring_one_msg_2t(None, DEPTH),
    );
    report(
        "spsc1_ring_one_msg_2t (unpinned):",
        spsc1_ring_one_msg_2t(None, DEPTH),
    );
    report(
        "spsc2_ring_one_msg_2t (unpinned):",
        spsc2_ring_one_msg_2t(None, DEPTH),
    );
    report(
        "mpsc_ring_one_msg_2t (unpinned):",
        mpsc_ring_one_msg_2t(None, DEPTH),
    );
    report(
        "spsc_ring_one_pool_msg_2t (unpinned):",
        spsc_ring_one_pool_msg_2t(None),
    );
    report(
        "std_mpsc_one_pool_msg_2t (unpinned):",
        std_mpsc_one_pool_msg_2t(None),
    );

    // Three threads (2 producers + 1 consumer): the
    // multi-producer line only the MPSC ring can run.
    report(
        "mpsc_ring_one_msg_3t (2p+1c unpinned):",
        mpsc_ring_one_msg_3t(),
    );

    // Two threads, different physical cores.
    println!();
    match far {
        Some((p, c)) => {
            report(
                &format!("spsc_ring_one_msg_2t (diff cores {p}+{c}):"),
                spsc_ring_one_msg_2t(far, DEPTH),
            );
            report(
                &format!("spsc1_ring_one_msg_2t (diff cores {p}+{c}):"),
                spsc1_ring_one_msg_2t(far, DEPTH),
            );
            report(
                &format!("spsc2_ring_one_msg_2t (diff cores {p}+{c}):"),
                spsc2_ring_one_msg_2t(far, DEPTH),
            );
            report(
                &format!("mpsc_ring_one_msg_2t (diff cores {p}+{c}):"),
                mpsc_ring_one_msg_2t(far, DEPTH),
            );
            report(
                &format!("spsc_ring_one_pool_msg_2t (diff cores {p}+{c}):"),
                spsc_ring_one_pool_msg_2t(far),
            );
            report(
                &format!("std_mpsc_one_pool_msg_2t (diff cores {p}+{c}):"),
                std_mpsc_one_pool_msg_2t(far),
            );
        }
        None => {
            println!("2t diff-cores runs:");
            println!("  skipped, only one core found");
        }
    }

    // Two threads, one physical core (SMT siblings).
    println!();
    match smt {
        Some((p, c)) => {
            report(
                &format!("spsc_ring_one_msg_2t (same core {p}+{c}):"),
                spsc_ring_one_msg_2t(smt, DEPTH),
            );
            report(
                &format!("spsc1_ring_one_msg_2t (same core {p}+{c}):"),
                spsc1_ring_one_msg_2t(smt, DEPTH),
            );
            report(
                &format!("spsc2_ring_one_msg_2t (same core {p}+{c}):"),
                spsc2_ring_one_msg_2t(smt, DEPTH),
            );
            report(
                &format!("mpsc_ring_one_msg_2t (same core {p}+{c}):"),
                mpsc_ring_one_msg_2t(smt, DEPTH),
            );
            report(
                &format!("spsc_ring_one_pool_msg_2t (same core {p}+{c}):"),
                spsc_ring_one_pool_msg_2t(smt),
            );
            report(
                &format!("std_mpsc_one_pool_msg_2t (same core {p}+{c}):"),
                std_mpsc_one_pool_msg_2t(smt),
            );
        }
        None => {
            println!("2t same-core runs:");
            println!("  skipped, no SMT sibling found");
        }
    }

    // The depth sweep, the ring flavors at every placement.
    println!();
    depth_sweep(smt, far);
}
