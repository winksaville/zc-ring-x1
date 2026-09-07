//! SPSC ring: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation. Historical versions stay
//!   available for testing and performance comparison, pinned by explicit path
//!   (`spsc::v0::Ring`, `spsc::v1::Ring`, `spsc::v2::Ring`).
//! - The re-export below selects the crate's default version. Repoint it at another
//!   `vN` to change the default without touching type names or call sites.

pub mod v0;
pub mod v1;
pub mod v2;

pub use v0::{Consumer, Header, Producer, ReadSlot, Ring, WriteSlot};
