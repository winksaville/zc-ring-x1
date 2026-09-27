//! MPSC ring: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation.
//!   Historical versions stay available for testing and
//!   performance comparison, pinned by explicit path
//!   (`mpsc::v0::MpscRing`, `mpsc::v1::MpscRing`,
//!   `mpsc::v2::MpscRing`, `mpsc::v3::MpscRing`).
//! - The re-export below selects the crate's default version.
//!   Repoint it at another `vN` to change the default without
//!   touching type names or call sites. v1 since 2026-09-10:
//!   v0's cost with capacity down to 1 (the design doc's "MPSC
//!   v1: equality-seq ring"). v2 is v1 over spsc v3's
//!   segments (the design doc's "MPSC v2: ring of segments"),
//!   reached by path until it matches v1 where no switch
//!   happens. v3 is v2 attachable from another process, with
//!   counted roles (the design doc's "MPSC v3: attachable
//!   segments with counted roles"), reached by path.

pub mod v0;
pub mod v1;
pub mod v2;
pub mod v3;

pub use v1::{MpscConsumer, MpscHeader, MpscProducer, MpscReadSlot, MpscRing, mpsc_region_size};
