//! The user guide's SPSC v3 program: a pool sized for the ring's
//! segments, the ring initialized and split, one producer thread
//! streaming typed messages to one consumer thread under a wait
//! policy, and the counters read at the end. Quoted in
//! `notes/user-guide.md`, run with
//! `cargo run --release --example guide_spsc_v3`.

use zc_ring_x1::spsc::v3::segment_size;
use zc_ring_x1::{Empty, Pool, PoolHeader, Ring};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message. Any `#[repr(C)]` type the zerocopy traits accept,
/// at most the slot body in size: `SLOT` less the crate's
/// `SLOT_HEADER_BYTES`, 64 less 16 here, aligned to at most 16.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    payload: [u8; 16],
}

/// Bytes per slot, a cache-line multiple. One line holds the
/// crate's 16-byte slot header and up to 48 bytes of message.
const SLOT: u32 = 64;

/// Slots per segment, a power of two.
const DEPTH: u32 = 64;

/// Segments in the ring, 1 to 32. With a consumer that keeps up
/// only the first is used, and the rest absorb a producer that
/// runs ahead.
const SEGMENTS: u32 = 4;

/// Messages the run moves.
const COUNT: u64 = 1_000_000;

/// One cache line of backing store, so a `Vec<Line>` is a
/// line-aligned region of any size the pool wants.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

/// A wait policy: spin briefly, then yield the thread, and never
/// give up. The crate ships `policy::spin`, and any
/// `FnMut(u32) -> bool` works, the argument the failed attempts
/// so far and the return whether to try again.
fn spin_then_yield(attempt: u32) -> bool {
    if attempt < 100 {
        core::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
    true
}

fn main() {
    // 1. A pool whose buffers hold one segment each, and enough of
    //    them for the ring. `segment_size` is the bytes a segment
    //    of DEPTH slots of SLOT bytes needs, header included.
    let seg_bytes = segment_size(SLOT, DEPTH);
    let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes * SEGMENTS as u64;
    let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
    let mut pool = Pool::init(
        store.as_mut_slice().as_mut_bytes(),
        seg_bytes as u32,
        SEGMENTS,
    )
    .expect("the store is sized for the header and the segments"); // OK: sized above from segment_size

    // 2. The ring takes its segments from the pool at init, and
    //    split hands out the two endpoints, each once. The pool is
    //    borrowed only during init.
    let (mut producer, mut consumer) = Ring::init(&mut pool, SLOT, DEPTH, SEGMENTS)
        .expect("the pool holds the segments") // OK: the pool was made for them
        .split();

    // 3. The endpoints are Send, so each moves to its own thread.
    let (sent, seen) = std::thread::scope(|s| {
        let producer = s.spawn(move || {
            for i in 0..COUNT {
                // Reserve the next slot as a `&mut Msg`, fill it in
                // place, and commit. Dropping the guard instead of
                // committing abandons the reservation.
                let mut slot = producer
                    .reserve_slot_with::<Msg>(spin_then_yield)
                    .expect("the policy never gives up"); // OK: spin_then_yield returns true
                slot.seq = i;
                slot.payload = [i as u8; 16];
                slot.commit();
            }
            (producer.switches(), producer.segment())
        });
        let consumer = s.spawn(move || {
            let mut checksum = 0u64;
            for i in 0..COUNT {
                // Reserve the oldest unread slot as a `&Msg`, read it,
                // and release it. Dropping the guard instead of
                // releasing re-delivers the same slot next time.
                let msg = consumer
                    .reserve_slot_with::<Msg>(spin_then_yield)
                    .expect("the policy never gives up"); // OK: spin_then_yield returns true
                assert_eq!(msg.seq, i, "messages arrive in order");
                checksum = checksum.wrapping_add(msg.payload[0] as u64);
                msg.release();
            }
            // A single non-blocking probe: `|_| false` gives up at
            // once, and an empty ring reports Empty.
            assert_eq!(
                consumer.reserve_slot_with::<Msg>(|_| false).err(),
                Some(Empty)
            );
            (consumer.switches(), consumer.segment(), checksum)
        });
        (
            producer.join().expect("producer ran"), // OK: a panic is the run's failure
            consumer.join().expect("consumer ran"), // OK: a panic is the run's failure
        )
    });

    // 4. The counters: switches are segment changes, zero when the
    //    consumer kept up, and both sides agree once everything sent
    //    has been read. `segment` is where each side ended.
    let (p_switches, p_segment) = sent;
    let (c_switches, c_segment, checksum) = seen;
    assert_eq!(p_switches, c_switches, "both sides count the same switches");
    assert_eq!(p_segment, c_segment, "both sides end in the same segment");
    println!(
        "spsc v3: {COUNT} messages, {SEGMENTS} segments of {DEPTH}, \
         {p_switches} switches, ended in segment {p_segment}, checksum {checksum}"
    );
}
