//! An MPSC v3 program in two threads, pool to release: a pool sized for the ring's segments, the
//! ring, the two roles claimed, a producer thread sending typed messages to a consumer thread, each
//! side sleeping when the ring is full or empty, and the roles and the ring given back.
//!
//! - Linux only: the sides sleep on a futex.
//! - Run with `cargo run --release --example guide_mpsc_v3`.

use zc_ring_x1::mpsc::v3::{MpscRing, Multi, segment_size};
use zc_ring_x1::wake::Futex;
use zc_ring_x1::{Pool, PoolHeader, Ticks, microsecs_to_ticks};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message: plain data, at most a slot's body in size, `SLOT` less the crate's
/// `SLOT_HEADER_BYTES`, 64 less 16 here, aligned to at most 16.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Reading {
    sensor: u32,
    value: u32,
}

/// Bytes per slot, a cache-line multiple.
const SLOT: u32 = 64;

/// Slots per segment, a power of two. Small, so the ring fills and the producer sleeps.
const DEPTH: u32 = 8;

/// Segments in the ring, 1 to 32.
const SEGMENTS: u32 = 2;

/// Messages the producer sends.
const MESSAGES: u32 = 100_000;

/// One cache line of backing store, so a `Vec<Line>` is a line-aligned region of any size the pool
/// wants.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

fn main() {
    // 1. A pool whose buffers hold one segment each, sized with this module's `segment_size`.
    let seg_bytes = segment_size(SLOT, DEPTH);
    let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes * SEGMENTS as u64;
    let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
    let mut pool = Pool::init(
        store.as_mut_slice().as_mut_bytes(),
        seg_bytes as u32,
        SEGMENTS,
    )
    .expect("the store is sized for the header and the segments"); // OK: sized above from segment_size

    // 2. The ring, in the mode that switches segments, with a wake that sleeps on a futex.
    let ring = MpscRing::<Multi, Futex>::init(&mut pool, SLOT, DEPTH, SEGMENTS)
        .expect("the pool holds the segments"); // OK: the pool was made for them

    // 3. The roles, claimed by count: one consumer, and producers up to the ring's most. Each
    //    handle moves to the thread that uses it.
    let producer = ring.claim_producer().expect("a fresh ring"); // OK: no role is held yet
    let mut consumer = ring.claim_consumer().expect("a fresh ring"); // OK: no role is held yet

    // The caller converts its time to ticks once, not once per send.
    let spin_time = microsecs_to_ticks(20);

    let switches = std::thread::scope(|s| {
        // 4. The producer thread sends with a closure that writes the message into the slot. On a
        //    full ring it spins for up to 20 microseconds, then sleeps until the consumer frees a
        //    slot. It gives its role back when it is done.
        s.spawn(move || {
            for value in 0..MESSAGES {
                producer
                    .send_spin_sleep::<Reading>(spin_time, Ticks::FOREVER, |msg| {
                        msg.sensor = 7;
                        msg.value = value;
                    })
                    .expect("the send never gives up"); // OK: its sleep time is FOREVER
            }
            producer.release();
        });

        // 5. The consumer thread reads every message in place as a `&Reading` and releases its
        //    slot, sleeping while the ring is empty. Messages arrive in the order they were sent.
        //    Its release saves where it stopped, for the next consumer.
        let consumer = s.spawn(move || {
            for value in 0..MESSAGES {
                let msg = consumer
                    .reserve_slot_wait::<Reading>(|_| true)
                    .expect("the policy never gives up"); // OK: it returns true
                assert_eq!(
                    (msg.sensor, msg.value),
                    (7, value),
                    "messages arrive in order"
                );
                msg.release();
            }
            let switches = consumer.switches();
            consumer.release();
            switches
        });
        consumer.join().expect("consumer ran") // OK: a panic is the run's failure
    });

    // 6. With both roles given back, the ring's segments go back to the pool.
    ring.release_ring(&pool).expect("no role is held"); // OK: both threads released theirs

    println!(
        "mpsc v3: {MESSAGES} messages, {SEGMENTS} segments of {DEPTH}, {switches} switches, \
         roles and ring released"
    );
}
