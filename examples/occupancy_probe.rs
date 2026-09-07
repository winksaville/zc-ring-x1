//! Does the streaming loop actually stream? A probe, not a
//! product: the demo's `*_2t` lines assume the producer runs
//! ahead and the ring sits full, and the whole flow-control
//! reading of the v1 numbers rests on that. This counts who
//! waits for whom.
//!
//! Per message each side reports the failed attempts its wait
//! policy burned: a producer that finds the ring Full is being
//! held back by the consumer (the ring is saturated), a
//! consumer that finds it Empty is starved (the ring is near
//! empty and the loop is latency-bound, not streaming).
//!
//! `cargo run --release --example occupancy_probe -- <p-cpu> <c-cpu>`

use std::time::Instant;

use zc_ring_x1::{CACHE_LINE_SIZE, spsc};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Messages per run, as the demo moves.
const COUNT: u64 = 1_000_000;

/// Ring slots, as the demo's `DEPTH`.
const DEPTH: u32 = 64;

/// The demo's message, so the slot traffic matches.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

/// Region for the v0 ring: 4-line header + DEPTH one-line slots.
#[repr(C, align(64))]
struct Region([u8; 4 * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE]);

/// Region for the v1 ring: header + seq array at its widest +
/// DEPTH one-line slots, as the demo sizes it.
#[repr(C, align(64))]
struct SeqRegion(
    [u8; 4 * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE + DEPTH as usize * CACHE_LINE_SIZE],
);

/// Pin the calling thread to `cpu`, as the demo does.
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

/// Run the demo's two-thread stream over the ring at `$ring`,
/// counting the waits each side burned.
macro_rules! stream_probe {
    ($name:ident, $ring:path, $region:ident) => {
        /// Returns elapsed seconds, then (messages that waited,
        /// total failed attempts) for the producer and for the
        /// consumer.
        fn $name(p_cpu: usize, c_cpu: usize) -> (f64, (u64, u64), (u64, u64)) {
            let mut region = $region([0; size_of::<$region>()]);
            let (mut producer, mut consumer) =
                <$ring>::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH)
                    .unwrap() // OK: $region is sized/aligned for the ring header + DEPTH slots
                    .split();

            let start = Instant::now();
            let (p_stats, c_stats) = std::thread::scope(|s| {
                let p = s.spawn(move || {
                    pin_to_cpu(p_cpu);
                    let (mut waited, mut attempts) = (0u64, 0u64);
                    for i in 0..COUNT {
                        let mut last = 0u32;
                        let mut slot = producer
                            .reserve_slot_with::<Msg>(|a| {
                                last = a + 1;
                                std::hint::spin_loop();
                                true
                            })
                            .unwrap(); // OK: the policy never gives up
                        if last > 0 {
                            waited += 1;
                            attempts += last as u64;
                        }
                        slot.seq = i;
                        slot.commit();
                    }
                    (waited, attempts)
                });
                let c = s.spawn(move || {
                    pin_to_cpu(c_cpu);
                    let (mut waited, mut attempts) = (0u64, 0u64);
                    for i in 0..COUNT {
                        let mut last = 0u32;
                        let msg = consumer
                            .reserve_slot_with::<Msg>(|a| {
                                last = a + 1;
                                std::hint::spin_loop();
                                true
                            })
                            .unwrap(); // OK: the policy never gives up
                        if last > 0 {
                            waited += 1;
                            attempts += last as u64;
                        }
                        assert_eq!(msg.seq, i);
                        msg.release();
                    }
                    (waited, attempts)
                });
                (
                    p.join().expect("producer panicked"),
                    c.join().expect("consumer panicked"),
                )
            });
            (start.elapsed().as_secs_f64(), p_stats, c_stats)
        }
    };
}

stream_probe!(stream_v0, spsc::v0::Ring, Region);
stream_probe!(stream_v1, spsc::v1::Ring, SeqRegion);

/// Print one ring's line: throughput, then how often and how
/// hard each side waited.
fn report(label: &str, (secs, p, c): (f64, (u64, u64), (u64, u64))) {
    let ns = secs * 1e9 / COUNT as f64;
    let pct = |n: u64| n as f64 * 100.0 / COUNT as f64;
    println!(
        "{label:>4}: {ns:6.1} ns/msg   producer waited {:5.1}% of sends ({:5.1} attempts each)   \
         consumer waited {:5.1}% of recvs ({:5.1} attempts each)",
        pct(p.0),
        p.1 as f64 / p.0.max(1) as f64,
        pct(c.0),
        c.1 as f64 / c.0.max(1) as f64,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let p_cpu: usize = args.get(1).map_or(0, |a| a.parse().expect("p-cpu"));
    let c_cpu: usize = args.get(2).map_or(1, |a| a.parse().expect("c-cpu"));
    println!(
        "stream probe: {} messages, depth {DEPTH}, producer cpu {p_cpu}, consumer cpu {c_cpu}",
        COUNT
    );
    report("v0", stream_v0(p_cpu, c_cpu));
    report("v1", stream_v1(p_cpu, c_cpu));
}
