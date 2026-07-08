//! SPSC ring: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation;
//!   versions coexist as peers so iiac-perf can pin and
//!   compare them by explicit path (`spsc::v0::Ring`).
//! - The champion alias below picks the crate's default; move
//!   it to another version to change the default without
//!   touching type names or call sites.

pub mod v0;

pub use v0::{Consumer, Header, Producer, ReadSlot, Ring, WriteSlot};
