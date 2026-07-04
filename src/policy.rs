//! Shipped wait policies — the models users copy.
//!
//! A wait policy is any `FnMut(u32) -> bool`: the `_with`
//! methods ([`Pool::alloc_with`](crate::Pool::alloc_with),
//! [`Producer::reserve_slot_with`](crate::Producer::reserve_slot_with),
//! [`Consumer::reserve_slot_with`](crate::Consumer::reserve_slot_with))
//! call it after each failed attempt with the attempt count
//! (0-based, saturating); returning `false` gives up and the
//! operation returns its usual error.
//!
//! - The crate ships only the policies its own demo and
//!   tests use; compose your own (bounded retries, deadline,
//!   spin-then-yield) as capturing closures at the call
//!   site, or bake one into your own wrapper method.
//! - Policies are polling strategies. True blocking (futex,
//!   eventfd, async wakers) needs a peer wake over the
//!   header's user line and is a layer above this crate.

/// Spin forever: hint the CPU and keep waiting.
///
/// `#[inline]`: non-generic, so without the hint a
/// monomorphized `_with` in another crate calls it out of
/// line — measurable on the demo's ~3 ns loops.
#[inline]
pub fn spin(_attempt: u32) -> bool {
    core::hint::spin_loop();
    true
}
