//! The user guide's MPSC v2 program: a pool sized for the ring's
//! segments, the ring initialized and split, several producer
//! threads sending typed messages to one consumer thread under a
//! wait policy, and the counters read at the end. Quoted in
//! `notes/user-guide.md`, run with
//! `cargo run --release --example guide_mpsc_v2`.

use zc_ring_x1::mpsc::v2::{MpscRing, segment_size};
use zc_ring_x1::{Empty, Pool, PoolHeader};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message. Any `#[repr(C)]` type the zerocopy traits accept,
/// at most the slot body in size: `SLOT` less the crate's
/// `SLOT_HEADER_BYTES`, 64 less 4 here.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    producer: u64,
}

/// Bytes per slot, a cache-line multiple.
const SLOT: u32 = 64;

/// Slots per segment, a power of two.
const DEPTH: u32 = 64;

/// Segments in the ring, 1 to 32.
const SEGMENTS: u32 = 4;

/// Producer threads.
const PRODUCERS: u64 = 3;

/// Messages each producer sends.
const EACH: u64 = 300_000;

/// One cache line of backing store, so a `Vec<Line>` is a
/// line-aligned region of any size the pool wants.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

/// A wait policy: spin briefly, then yield the thread, and never
/// give up.
fn spin_then_yield(attempt: u32) -> bool {
    if attempt < 100 {
        core::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
    true
}

fn main() {
    // 1. A pool whose buffers hold one segment each. MPSC v2's
    //    segment header is three lines where SPSC v3's is one, so
    //    size with this module's `segment_size`.
    let seg_bytes = segment_size(SLOT, DEPTH);
    let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes * SEGMENTS as u64;
    let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
    let mut pool = Pool::init(
        store.as_mut_slice().as_mut_bytes(),
        seg_bytes as u32,
        SEGMENTS,
    )
    .expect("the store is sized for the header and the segments"); // OK: sized above from segment_size

    // 2. Init and split. The producer handle is Clone, one clone per
    //    producing thread, and the consumer is unique.
    let (producer, mut consumer) = MpscRing::init(&mut pool, SLOT, DEPTH, SEGMENTS)
        .expect("the pool holds the segments") // OK: the pool was made for them
        .split();

    let (switches, per_producer) = std::thread::scope(|s| {
        // 3. Each producer thread takes a clone and sends with a
        //    closure that fills the slot in place. The commit happens
        //    when the closure returns, so there is no guard to
        //    forget, and a panic inside the closure tombstones the
        //    slot rather than wedging the ring.
        for id in 0..PRODUCERS {
            let producer = producer.clone();
            s.spawn(move || {
                for i in 0..EACH {
                    producer
                        .send_with::<Msg>(spin_then_yield, |m| {
                            m.seq = i;
                            m.producer = id;
                        })
                        .expect("the policy never gives up"); // OK: spin_then_yield returns true
                }
            });
        }
        // 4. The consumer reads every message as a `&Msg` and
        //    releases it. Messages from one producer arrive in that
        //    producer's order, and producers interleave freely.
        let consumer = s.spawn(move || {
            let mut next = [0u64; PRODUCERS as usize];
            for _ in 0..PRODUCERS * EACH {
                let msg = consumer
                    .reserve_slot_with::<Msg>(spin_then_yield)
                    .expect("the policy never gives up"); // OK: spin_then_yield returns true
                let p = msg.producer as usize;
                assert_eq!(msg.seq, next[p], "each producer's messages arrive in order");
                next[p] += 1;
                msg.release();
            }
            assert_eq!(
                consumer.reserve_slot_with::<Msg>(|_| false).err(),
                Some(Empty)
            );
            (consumer.switches(), next)
        });
        consumer.join().expect("consumer ran") // OK: a panic is the run's failure
    });

    // 5. The counters. The producer handle's count is the ring's,
    //    since a switch is the ring's move rather than one
    //    producer's, and it agrees with the consumer's once all is
    //    read. The original handle is still usable here: clones
    //    and the original are equals.
    assert_eq!(
        producer.switches(),
        switches,
        "both sides count the same switches"
    );
    println!(
        "mpsc v2: {PRODUCERS} producers x {EACH} messages, {SEGMENTS} segments of {DEPTH}, \
         {switches} switches, ended in segment {}, per producer {per_producer:?}",
        producer.segment()
    );
}
