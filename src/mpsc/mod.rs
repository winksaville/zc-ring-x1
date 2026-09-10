//! MPSC ring: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation;
//!   historical versions stay available for testing and
//!   performance comparison, pinned by explicit path
//!   (`mpsc::v0::MpscRing`, `mpsc::v1::MpscRing`).
//! - The re-export below selects the crate's default version;
//!   repoint it at another `vN` to change the default without
//!   touching type names or call sites. v1 since 2026-09-10:
//!   v0's cost with capacity down to 1 (the design doc's "MPSC
//!   v1: equality-seq ring").

pub mod v0;
pub mod v1;

pub use v1::{MpscConsumer, MpscHeader, MpscProducer, MpscReadSlot, MpscRing, mpsc_region_size};
