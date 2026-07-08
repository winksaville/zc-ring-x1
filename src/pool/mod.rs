//! Message pool: versioned sibling implementations.
//!
//! - Each `vN` submodule is a complete, live implementation;
//!   historical versions stay available for testing and
//!   performance comparison, pinned by explicit path
//!   (`pool::v0::Pool`).
//! - The re-export below selects the crate's default version;
//!   repoint it at another `vN` to change the default without
//!   touching type names or call sites.

pub mod v0;

pub use v0::{BufSlot, Exhausted, Pool, PoolHeader, PoolResolver};
