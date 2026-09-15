//! Watch `mpsc::v2` use its segments: every segment count from 1
//! to 32 at depths 1, 8, 64, and 1024, first filling every segment
//! with the consumer idle, then streaming from one, two, and four
//! producer threads to a consumer thread.
//!
//! Run with `cargo run --release --example mpsc_v2_segments`.

use std::time::Instant;

use zc_ring_x1::mpsc::v2::{MAX_SEGMENTS, MpscConsumer, MpscProducer, MpscRing, segment_size};
use zc_ring_x1::{Pool, PoolHeader, policy};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message: the producer and its sequence number, which the
/// consumer checks per producer.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    producer: u64,
}

/// One cache line of heap backing store, so a `Vec` of them is a
/// line-aligned region of any size.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

/// The depths every segment count runs at.
const DEPTHS: [u32; 4] = [1, 8, 64, 1024];

/// The producer counts the streams run with.
const PRODUCERS: [u64; 3] = [1, 2, 4];

/// Messages each stream moves, over all its producers.
const STREAM: u64 = 100_000;

/// Run `f` on a ring of `count` segments of `depth` one-line slots
/// over a pool holding exactly those segments.
fn with_ring<R>(
    count: u32,
    depth: u32,
    f: impl FnOnce(MpscProducer<'_>, MpscConsumer<'_>) -> R,
) -> R {
    let buf = segment_size(64, depth);
    let bytes = size_of::<PoolHeader>() as u64 + buf * count as u64;
    let mut store = vec![Line([0; 64]); bytes.div_ceil(64) as usize];
    let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), buf as u32, count)
        .expect("the store is sized for exactly the segments"); // OK: sized by segment_size and line-aligned
    let (prod, cons) = MpscRing::init(&mut pool, 64, depth, count)
        .expect("the pool holds exactly the segments") // OK: count <= MAX_SEGMENTS and depth a power of two
        .split();
    f(prod, cons)
}

/// Fill every segment with the consumer idle, then drain in order.
/// Returns the segments the producer wrote into and its switches.
fn fill(count: u32, depth: u32) -> (u32, u64) {
    with_ring(count, depth, |prod, mut cons| {
        let total = (count * depth) as u64;
        let mut used = 1u32 << prod.segment();
        for i in 0..total {
            prod.send_with::<Msg>(
                |_| false,
                |m| {
                    m.seq = i;
                    m.producer = 0;
                },
            )
            .expect("the ring holds every message"); // OK: total is the ring's capacity
            used |= 1 << prod.segment();
        }
        assert!(
            prod.send_with::<Msg>(|_| false, |_| {}).is_err(),
            "not full"
        );
        for i in 0..total {
            let msg = cons
                .reserve_slot_with::<Msg>(|_| false)
                .expect("every message was sent"); // OK: total messages are waiting
            assert_eq!(msg.seq, i, "order broken");
            msg.release();
        }
        assert!(
            cons.reserve_slot_with::<Msg>(|_| false).is_err(),
            "not empty"
        );
        assert_eq!(cons.switches(), prod.switches(), "switch counts differ");
        (used.count_ones(), prod.switches())
    })
}

/// Stream [`STREAM`] messages from `producers` threads to one
/// consumer thread, checking per-producer order. Returns the
/// switches and nanoseconds per message.
fn stream(count: u32, depth: u32, producers: u64) -> (u64, f64) {
    with_ring(count, depth, |prod, mut cons| {
        let each = STREAM / producers;
        let start = Instant::now();
        std::thread::scope(|s| {
            for p in 0..producers {
                let prod = prod.clone();
                s.spawn(move || {
                    for i in 0..each {
                        prod.send_with::<Msg>(policy::spin, |m| {
                            m.seq = i;
                            m.producer = p;
                        })
                        .expect("spin never gives up"); // OK: policy::spin never gives up
                    }
                });
            }
            let cons = &mut cons;
            s.spawn(move || {
                let mut next = vec![0u64; producers as usize];
                for _ in 0..each * producers {
                    let msg = cons
                        .reserve_slot_with::<Msg>(policy::spin)
                        .expect("spin never gives up"); // OK: policy::spin never gives up
                    let p = msg.producer as usize;
                    assert_eq!(msg.seq, next[p], "per-producer order broken");
                    next[p] += 1;
                    msg.release();
                }
                assert!(
                    cons.reserve_slot_with::<Msg>(|_| false).is_err(),
                    "not empty"
                );
            });
        });
        let ns = start.elapsed().as_secs_f64() * 1e9 / (each * producers) as f64;
        assert_eq!(cons.switches(), prod.switches(), "switch counts differ");
        (prod.switches(), ns)
    })
}

/// Print a table with a row per segment count and a column per
/// depth, each cell from `cell`.
fn table(title: &str, mut cell: impl FnMut(u32, u32) -> String) {
    println!("{title}\n");
    let mut header = String::from("| segments |");
    let mut sep = String::from("|---------:|");
    for depth in DEPTHS {
        header.push_str(&format!(" {:>16} |", format!("depth {depth}")));
        sep.push_str(&format!("{}:|", "-".repeat(17)));
    }
    println!("{header}\n{sep}");
    for count in 1..=MAX_SEGMENTS {
        let mut row = format!("| {count:>8} |");
        for depth in DEPTHS {
            row.push_str(&format!(" {:>16} |", cell(count, depth)));
        }
        println!("{row}");
    }
    println!();
}

/// Run the fill table and a stream table per producer count.
fn main() {
    println!(
        "mpsc-v2: every segment count 1 to {MAX_SEGMENTS} at depths {:?}\n",
        DEPTHS
    );
    table(
        "Fill: the consumer idle, one producer writes every segment full, then the \
         consumer drains in order. Cells: segments used / switches.",
        |count, depth| {
            let (used, switches) = fill(count, depth);
            assert_eq!(used, count, "a segment was never used");
            format!("{used} / {switches}")
        },
    );
    for producers in PRODUCERS {
        table(
            &format!(
                "Stream: {STREAM} messages from {producers} producer thread(s) to a consumer \
                 thread, per-producer order checked. Cells: switches / ns per message."
            ),
            |count, depth| {
                let (switches, ns) = stream(count, depth, producers);
                format!("{switches} / {ns:.1}")
            },
        );
    }
    println!(
        "Every run kept per-producer order, used every segment when filled, and its producers \
         and consumer counted the same switches."
    );
}
