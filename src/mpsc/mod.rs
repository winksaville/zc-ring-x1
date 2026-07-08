//! MPSC ring: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation;
//!   historical versions stay available for testing and
//!   performance comparison, pinned by explicit path
//!   (`mpsc::v0::MpscRing`).
//! - The re-export below selects the crate's default version;
//!   repoint it at another `vN` to change the default without
//!   touching type names or call sites.

pub mod v0;

pub use v0::{MpscConsumer, MpscHeader, MpscProducer, MpscReadSlot, MpscRing, mpsc_region_size};
