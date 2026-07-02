//! Throwaway compile/run check of the README example.

use zc_ring_x1::Ring;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

/// Runs the README snippet.
fn main() {
    // Cache-line-aligned region: 192 B header + 4 slots × 64 B.
    #[repr(C, align(64))]
    struct Region([u8; 448]);
    let mut region = Region([0; 448]);

    let (mut producer, mut consumer) = Ring::init(&mut region.0, 64, 4).unwrap().split();

    let mut slot = producer.reserve_slot::<Msg>().unwrap();
    slot.seq = 1;
    slot.val = 42;
    slot.commit(); // publish to the consumer

    let msg = consumer.reserve_slot::<Msg>().unwrap();
    assert_eq!(msg.val, 42);
    msg.release(); // slot is free for reuse
    println!("readme example ok");
}
