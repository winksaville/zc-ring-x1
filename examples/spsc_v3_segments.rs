//! Watch `spsc::v3` use its segments: every segment count from 1
//! to 32 at depths 1, 8, 64, and 1024, first filling every segment
//! with the consumer idle, then streaming across two threads.
//!
//! Run with `cargo run --release --example spsc_v3_segments`.

use std::time::Instant;

use zc_ring_x1::spsc::v3::{Consumer, MAX_SEGMENTS, Producer, Ring, segment_size};
use zc_ring_x1::{Pool, PoolHeader, policy};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message: a sequence number the consumer checks.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
}

/// One cache line of heap backing store, so a `Vec` of them is a
/// line-aligned region of any size.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

/// The depths every segment count runs at.
const DEPTHS: [u32; 4] = [1, 8, 64, 1024];

/// Messages each two-thread stream moves.
const STREAM: u64 = 100_000;

/// Run `f` on a ring of `count` segments of `depth` one-line slots
/// over a pool holding exactly those segments.
fn with_ring<R>(count: u32, depth: u32, f: impl FnOnce(Producer<'_>, Consumer<'_>) -> R) -> R {
    let buf = segment_size(64, depth);
    let bytes = size_of::<PoolHeader>() as u64 + buf * count as u64;
    let mut store = vec![Line([0; 64]); bytes.div_ceil(64) as usize];
    let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), buf as u32, count)
        .expect("the store is sized for exactly the segments"); // OK: sized by segment_size and line-aligned
    let (prod, cons) = Ring::init(&mut pool, 64, depth, count)
        .expect("the pool holds exactly the segments") // OK: count <= MAX_SEGMENTS and depth a power of two
        .split();
    f(prod, cons)
}

/// Fill every segment with the consumer idle, then drain in order.
/// Returns the segments the producer wrote into and its switches.
fn fill(count: u32, depth: u32) -> (u32, u64) {
    with_ring(count, depth, |mut prod, mut cons| {
        let total = (count * depth) as u64;
        let mut used = 1u32 << prod.segment();
        for i in 0..total {
            let mut slot = prod
                .reserve_slot_with::<Msg>(|_| false)
                .expect("the ring holds every message"); // OK: total is the ring's capacity
            slot.seq = i;
            slot.commit();
            used |= 1 << prod.segment();
        }
        assert!(
            prod.reserve_slot_with::<Msg>(|_| false).is_err(),
            "not full"
        );
        for i in 0..total {
            let msg = cons
                .reserve_slot_with::<Msg>(|_| false)
                .expect("every message was sent"); // OK: total messages are waiting
            assert_eq!(msg.seq, i, "order broken");
            msg.release();
        }
        assert_eq!(cons.switches(), prod.switches(), "switch counts differ");
        (used.count_ones(), prod.switches())
    })
}

/// Stream [`STREAM`] messages from one thread to another, checking
/// order. Returns the switches and nanoseconds per message.
fn stream(count: u32, depth: u32) -> (u64, f64) {
    with_ring(count, depth, |mut prod, mut cons| {
        let start = Instant::now();
        let (sent, seen) = std::thread::scope(|s| {
            let producer = s.spawn(move || {
                for i in 0..STREAM {
                    let mut slot = prod
                        .reserve_slot_with::<Msg>(policy::spin)
                        .expect("spin never gives up"); // OK: policy::spin never gives up
                    slot.seq = i;
                    slot.commit();
                }
                prod.switches()
            });
            let consumer = s.spawn(move || {
                for i in 0..STREAM {
                    let msg = cons
                        .reserve_slot_with::<Msg>(policy::spin)
                        .expect("spin never gives up"); // OK: policy::spin never gives up
                    assert_eq!(msg.seq, i, "order broken");
                    msg.release();
                }
                cons.switches()
            });
            (
                producer.join().expect("producer panicked"), // OK: a panic is the run's failure
                consumer.join().expect("consumer panicked"), // OK: a panic is the run's failure
            )
        });
        assert_eq!(sent, seen, "switch counts differ");
        (sent, start.elapsed().as_secs_f64() * 1e9 / STREAM as f64)
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

/// Run both tables.
fn main() {
    println!(
        "spsc-v3: every segment count 1 to {MAX_SEGMENTS} at depths {:?}\n",
        DEPTHS
    );
    table(
        "Fill: the consumer idle, the producer writes every segment full, then the \
         consumer drains in order. Cells: segments used / switches.",
        |count, depth| {
            let (used, switches) = fill(count, depth);
            assert_eq!(used, count, "a segment was never used");
            format!("{used} / {switches}")
        },
    );
    table(
        &format!(
            "Stream: {STREAM} messages from one thread to another, order checked. \
             Cells: switches / ns per message."
        ),
        |count, depth| {
            let (switches, ns) = stream(count, depth);
            format!("{switches} / {ns:.1}")
        },
    );
    println!(
        "Every run kept order, used every segment when filled, and its producer and consumer \
         counted the same switches."
    );
}
