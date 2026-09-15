//! Throwaway compile/run check of the README example.

use zc_ring_x1::{Pool, PoolHeader, Ring, spsc::v3::segment_size};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

/// Runs the README snippet.
fn main() {
    // A pool of two segments, each a header line + 4 slots × 64 B.
    const SEG: usize = 64 + 4 * 64;
    #[repr(C, align(64))]
    struct Region([u8; size_of::<PoolHeader>() + 2 * SEG]);
    let mut region = Region([0; size_of::<PoolHeader>() + 2 * SEG]);
    assert_eq!(segment_size(64, 4), SEG as u64);

    let mut pool = Pool::init(&mut region.0, SEG as u32, 2).unwrap();
    let (mut producer, mut consumer) = Ring::init(&mut pool, 64, 4, 2).unwrap().split();

    let mut slot = producer.reserve_slot_with::<Msg>(|_| false).unwrap();
    slot.seq = 1;
    slot.val = 42;
    slot.commit(); // publish to the consumer

    let msg = consumer.reserve_slot_with::<Msg>(|_| false).unwrap();
    assert_eq!(msg.val, 42);
    msg.release(); // slot is free for reuse
    println!("readme example ok");
}
