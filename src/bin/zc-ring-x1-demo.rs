//! Demo binary: both primitives working across threads,
//! with throughput printed — run `cargo run --release`, or
//! `cargo install --path . --locked` and run
//! `zc-ring-x1-demo`. `-V`/`--version` prints the
//! version-of-record so you know exactly which build you
//! are testing.
//!
//! - Part 1, the ring: an SPSC pair moves typed messages
//!   in place (reserve_slot → write → commit; reserve_slot
//!   → read → release), one producer thread to one
//!   consumer thread.
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

/// The demo message; two words so a torn write would show.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
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
    println!("{label:<32} {rate:>12} msgs/sec  {ns_per_msg:>7.1} ns/msg");
}

/// Move COUNT messages producer-thread → consumer-thread
/// through the ring; return elapsed seconds.
fn ring_demo() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let (mut producer, mut consumer) = Ring::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH)
        .unwrap()
        .split();

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            for i in 0..COUNT {
                loop {
                    match producer.reserve_slot::<Msg>() {
                        Ok(mut slot) => {
                            slot.seq = i;
                            slot.val = i * 3;
                            slot.commit();
                            break;
                        }
                        Err(Full) => std::hint::spin_loop(),
                    }
                }
            }
        });
        s.spawn(move || {
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
fn pool_demo() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap();

    let (tx, rx) = std::sync::mpsc::sync_channel::<BufSlot<'_, Msg>>(DEPTH as usize);
    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            for i in 0..COUNT {
                loop {
                    match pool.alloc::<Msg>() {
                        Ok(mut buf) => {
                            buf.seq = i;
                            buf.val = i * 3;
                            tx.send(buf).unwrap();
                            break;
                        }
                        Err(Exhausted) => std::hint::spin_loop(),
                    }
                }
            }
        });
        s.spawn(move || {
            for i in 0..COUNT {
                let buf = rx.recv().unwrap();
                assert_eq!(buf.seq, i);
                buf.free();
            }
        });
    });
    start.elapsed().as_secs_f64()
}

/// Alloc → write → free COUNT messages on one thread — the
/// pool's own cost, no channel, no second thread; return
/// elapsed seconds.
fn pool_alloc_free_demo() -> f64 {
    let mut region = Region([0; size_of::<Region>()]);
    let mut pool = Pool::init(&mut region.0, CACHE_LINE_SIZE as u32, DEPTH).unwrap();

    let start = Instant::now();
    for i in 0..COUNT {
        let mut buf = pool.alloc::<Msg>().unwrap();
        buf.seq = i;
        buf.val = i * 3;
        std::hint::black_box(buf.val);
        buf.free();
    }
    start.elapsed().as_secs_f64()
}

/// The same loop through the global allocator (Box::new →
/// write → drop) for comparison; return elapsed seconds.
fn global_alloc_free_demo() -> f64 {
    let start = Instant::now();
    for i in 0..COUNT {
        let mut buf = std::hint::black_box(Box::new(Msg { seq: 0, val: 0 }));
        buf.seq = i;
        buf.val = i * 3;
        std::hint::black_box(buf.val);
        drop(buf);
    }
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
    report("ring (SPSC, in-place):", ring_demo());
    report("pool (alloc here -> free there):", pool_demo());
    report("pool (alloc/free, 1 thread):", pool_alloc_free_demo());
    report(
        "global (Box::new/drop, 1 thread):",
        global_alloc_free_demo(),
    );
    println!("(composed descriptor flow arrives with the descriptor-queue cycle)");
}
