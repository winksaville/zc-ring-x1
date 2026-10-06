//! An MPSC v4 program in two threads, pool to release, in the typical zero-copy use: each message
//! is written into a buffer of a message pool, the ring carries the buffer's id, and the consumer
//! reads the message where the producer wrote it, so the message is never copied.
//!
//! - Two pools: one holds the ring's segments, and one holds the messages.
//! - Each side spins for a short time, then sleeps, when the ring is full or empty.
//! - Linux only: the sides sleep on a futex.
//! - Run with `cargo run --release --example guide_mpsc_v4`.

use zc_ring_x1::mpsc::v4::{MpscRing, Multi, segment_size};
use zc_ring_x1::wake::{Futex, Sleep};
use zc_ring_x1::{Desc, Exhausted, Pool, PoolHeader, PoolRegistry, Ticks, microsecs_to_ticks};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The message: plain data, at most a message pool buffer in size.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Reading {
    sensor: u32,
    value: u32,
    samples: [u32; 12],
}

/// Bytes per slot of the ring, a cache-line multiple. A slot holds an id, a `Desc`, which is small,
/// so one line is plenty.
const SLOT: u32 = 64;

/// Slots per segment, a power of two. Small, so the ring fills and the producer sleeps.
const DEPTH: u32 = 8;

/// Segments in the ring, 1 to 32.
const SEGMENTS: u32 = 2;

/// Bytes per buffer of the message pool, a cache-line multiple that holds a `Reading`.
const MESSAGE_BYTES: u32 = 64;

/// Buffers in the message pool: how many messages can be written and not yet read at once.
const MESSAGE_BUFFERS: u32 = 32;

/// Messages the producer sends.
const MESSAGES: u32 = 100_000;

/// One cache line of backing store, so a `Vec<Line>` is a line-aligned region of any size a pool
/// wants.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
#[repr(C, align(64))]
struct Line([u8; 64]);

/// A line-aligned store for a pool of `count` buffers of `bytes` each.
fn store(bytes: u64, count: u32) -> Vec<Line> {
    let region_bytes = size_of::<PoolHeader>() as u64 + bytes * count as u64;
    vec![Line([0; 64]); region_bytes.div_ceil(64) as usize]
}

fn main() {
    // 1. A pool whose buffers hold one segment of the ring each, sized with this module's
    //    `segment_size`.
    let seg_bytes = segment_size(SLOT, DEPTH);
    let mut ring_store = store(seg_bytes, SEGMENTS);
    let mut ring_pool = Pool::init(
        ring_store.as_mut_slice().as_mut_bytes(),
        seg_bytes as u32,
        SEGMENTS,
    )
    .expect("the store is sized for the header and the segments"); // OK: sized above from segment_size

    // 2. A pool for the messages, and a registry that turns one of its buffers into an id, a
    //    `Desc`, and an id back into the buffer.
    let mut message_store = store(MESSAGE_BYTES as u64, MESSAGE_BUFFERS);
    let mut messages = Pool::init(
        message_store.as_mut_slice().as_mut_bytes(),
        MESSAGE_BYTES,
        MESSAGE_BUFFERS,
    )
    .expect("the store is sized for the header and the buffers"); // OK: sized above
    let mut registry = PoolRegistry::<1>::new();
    let pool_id = registry
        .register(messages.view())
        .expect("the registry holds one pool"); // OK: it is empty and holds one
    let registry = &registry;

    // 3. The ring: `Multi`, so it moves to a free segment when one fills, and `Sleep<Futex>`, so an
    //    endpoint that waits spins for a time and then sleeps on a futex. `Single` in place of
    //    `Multi` is a ring of one segment, and `SpinOnly` in place of `Sleep<Futex>` is a ring where
    //    nothing sleeps.
    let ring = MpscRing::<Multi, Sleep<Futex>>::init(&mut ring_pool, SLOT, DEPTH, SEGMENTS)
        .expect("the pool holds the segments"); // OK: the pool was made for them

    // 4. The roles: one consumer, and producers up to the ring's most. Each endpoint moves to the
    //    thread that uses it.
    let producer = ring.producer().expect("a fresh ring"); // OK: no role is held yet
    let mut consumer = ring.consumer().expect("a fresh ring"); // OK: no role is held yet

    // The caller converts its time to ticks once, not once per send or receive.
    let spin_20us = microsecs_to_ticks(20);

    let switches = std::thread::scope(|s| {
        // 5. The producer thread writes each message into a buffer of the message pool, turns the
        //    buffer into its id, and sends the id. On a full ring it spins for up to 20
        //    microseconds, then sleeps until the consumer frees a slot.
        s.spawn(move || {
            for value in 0..MESSAGES {
                // Every buffer is in flight until the consumer frees one.
                let mut message = loop {
                    match messages.alloc::<Reading>() {
                        Ok(buffer) => break buffer,
                        Err(Exhausted) => std::hint::spin_loop(),
                    }
                };
                message.sensor = 7;
                message.value = value;
                message.samples = [value; 12];
                let id = registry
                    .to_desc(pool_id, message)
                    .map_err(|(_, e)| e)
                    .expect("the buffer is the registered pool's"); // OK: it was allocated from it
                producer
                    .send_spin_sleep::<Desc>(spin_20us, Ticks::FOREVER, |slot| *slot = id)
                    .expect("the send never gives up"); // OK: its sleep time is FOREVER
            }
            producer.release();
        });

        // 6. The consumer thread receives each id, turns it back into the buffer, reads the message
        //    where the producer wrote it, and frees the buffer for the next message. On an empty
        //    ring it spins, then sleeps until a producer sends. Messages arrive in the order they
        //    were sent.
        let consumer = s.spawn(move || {
            for value in 0..MESSAGES {
                let id = consumer
                    .recv_spin_sleep::<Desc, _>(spin_20us, Ticks::FOREVER, |slot| *slot)
                    .expect("the receive never gives up"); // OK: its sleep time is FOREVER
                // SAFETY: the id was made by the producer from a buffer it owned and sent once, it
                // is read here after the send's commit, and it is turned back exactly once.
                let message = unsafe { registry.to_slot::<Reading>(id) }
                    .expect("the id names a buffer of the registered pool"); // OK: the producer made it
                assert_eq!(
                    (message.sensor, message.value, message.samples[11]),
                    (7, value, value),
                    "messages arrive in order"
                );
                message.free();
            }
            let switches = consumer.switches();
            consumer.release();
            switches
        });
        consumer.join().expect("consumer ran") // OK: a panic is the run's failure
    });

    // 7. With both roles given back, the ring's segments go back to their pool.
    ring.release_ring(&ring_pool).expect("no role is held"); // OK: both threads released theirs

    println!(
        "mpsc v4: {MESSAGES} messages by id, never copied, {SEGMENTS} segments of {DEPTH}, \
         {switches} switches, roles and ring released"
    );
}
