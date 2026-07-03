//! The README's Message pool example, committed so
//! `cargo clippy --all-targets` keeps it compiling against
//! the real API (same convention as `examples/readme.rs`).

use zc_ring_x1::Pool;
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

/// The README example's message type.
#[derive(FromBytes, IntoBytes, KnownLayout)]
#[repr(C)]
struct Msg {
    seq: u64,
    val: u64,
}

/// Cache-line-aligned region: 128 B header + 4 bufs × 64 B.
#[repr(C, align(64))]
struct Region([u8; 384]);

/// Run the README pool snippet: two live buffers, freed in
/// the reverse order they were allocated.
fn main() {
    let mut region = Region([0; 384]);

    let mut pool = Pool::init(&mut region.0, 64, 4).unwrap();

    let mut a = pool.alloc::<Msg>().unwrap();
    let mut b = pool.alloc::<Msg>().unwrap(); // many live at once
    a.seq = 1;
    b.seq = 2;
    b.free(); // any order, from any thread the guard moved to
    a.free();

    println!("pool example ran: alloc x2, free x2");
}
