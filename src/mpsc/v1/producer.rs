//! MPSC producing endpoint: [`MpscProducer`] claims a slot
//! position by CAS, a closure fills it in place, and the
//! commit happens on closure return — abandonment is
//! unrepresentable (see the design doc's "MPSC API: closure
//! send").

use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{MpscHeader, TOMBSTONE};
use crate::{Full, USER_WORDS, check_type, slot_ptr};

/// A producing handle: `Clone` one per producing thread, then
/// `send_with`.
///
/// - `send_with` takes `&self` — exclusivity comes from the
///   claim CAS, not the borrow, so one handle may also be
///   shared by reference.
pub struct MpscProducer<'a> {
    /// The ring's control block.
    header: &'a MpscHeader,
    /// Base of the per-slot sequence array.
    seqs: *const AtomicU32,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`MpscRing`](super::MpscRing)).
    slot_size: u32,
    /// `capacity + 1`, precomputed: the commit value is
    /// `pos + M + 1`, and this keeps it one add on the hot path.
    commit_add: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
}

impl Clone for MpscProducer<'_> {
    /// A second producing handle over the same ring; the claim
    /// CAS serializes them.
    fn clone(&self) -> Self {
        MpscProducer { ..*self }
    }
}

// SAFETY: any number of producers is the protocol contract —
// the shared state (producer_idx, seqs) is atomic, slot claims
// are exclusive by CAS, and slot writes are handed off with
// Release/Acquire ordering.
unsafe impl Send for MpscProducer<'_> {}
// SAFETY: send_with is &self and every access is protected as
// above, so shared references across threads are equally fine.
unsafe impl Sync for MpscProducer<'_> {}

/// Commits-on-drop guard armed while the fill closure runs: a
/// panic unwinding through `send_with` must not abandon the
/// claimed position (the consumer would wait there forever),
/// so drop-without-disarm publishes the tombstoned commit the
/// consumer releases without delivering.
struct TombstoneOnUnwind<'s> {
    /// The claimed slot's seq word.
    seq: &'s AtomicU32,
    /// The commit value (`pos + M + 1`).
    commit: u32,
}

impl Drop for TombstoneOnUnwind<'_> {
    /// Unwind path only — the normal path disarms with
    /// `mem::forget`.
    fn drop(&mut self) {
        // Release: the (garbage) slot bytes must still be a
        // well-defined handoff; the consumer skips the message
        // but touches the seq protocol state.
        self.seq
            .store(self.commit.wrapping_add(TOMBSTONE), Ordering::Release);
    }
}

impl<'a> MpscProducer<'a> {
    /// Build the handle from [`MpscRing::split`](super::MpscRing::split)'s
    /// geometry snapshot.
    pub(super) fn new(
        header: &'a MpscHeader,
        seqs: *const AtomicU32,
        slots: *mut u8,
        slot_size: u32,
        capacity: u32,
        mask: u32,
    ) -> Self {
        MpscProducer {
            header,
            seqs,
            slots,
            slot_size,
            commit_add: capacity + 1,
            mask,
        }
    }

    /// The header's app-owned scratch line — same contract as
    /// the SPSC endpoints' `user()`.
    pub fn user(&self) -> &[AtomicU32; USER_WORDS] {
        &self.header.user
    }

    /// The seq word for free-running position `idx`.
    fn seq(&self, idx: u32) -> &AtomicU32 {
        // SAFETY: idx is masked to < capacity, inside the seq
        // array validated at init/attach.
        unsafe { &*self.seqs.add((idx & self.mask) as usize) }
    }

    /// Claim the next slot position, fill it in place, commit
    /// on closure return; retry a full ring under the injected
    /// wait policy → [`Full`].
    ///
    /// - `fill` writes the message through `&mut T`; commit is
    ///   by construction — there is no abandonment state (an
    ///   SPSC-style guard could be dropped, and in MPSC an
    ///   abandoned *claim* wedges the queue).
    /// - `on_full` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Full)`. Pass `|_| false` for
    ///   a single non-blocking probe. A send that returns
    ///   `Full` has zero protocol footprint (the overflow-FIFO
    ///   seam — see the design doc's "Overflow readiness").
    /// - Losing a claim race to another producer is *not* a
    ///   policy call: the system made progress; this producer
    ///   just retries with the fresher index.
    /// - If `fill` panics, the unwind publishes a tombstoned
    ///   commit: the consumer releases the slot without
    ///   delivering it, and the queue keeps flowing.
    pub fn send_with<T>(
        &self,
        mut on_full: impl FnMut(u32) -> bool,
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.slot_size);
        let mut pos = self.header.producer_idx.load(Ordering::Relaxed);
        let mut attempt = 0u32;
        loop {
            // Acquire pairs with the consumer's Release in
            // release(): a claimable seq means the previous
            // lap's reads are done.
            let seq = self.seq(pos).load(Ordering::Acquire);
            if seq == pos {
                // Claimable. Weak CAS: a spurious failure just
                // retries with the fresher index.
                match self.header.producer_idx.compare_exchange_weak(
                    pos,
                    pos.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => pos = actual,
                }
            } else {
                // Not claimable at pos. Equality, not a signed
                // diff: with committed at pos + M + 1 the
                // previous lap's committed value is pos + 1,
                // which a diff would read as a lost race, and
                // anything but pos is either a stale pos or a
                // full ring. Re-read the index to tell them
                // apart: if it moved, another producer claimed
                // pos (or tombstoned it), no policy call. If
                // not, the slot's previous occupant is not yet
                // released.
                let cur = self.header.producer_idx.load(Ordering::Relaxed);
                if cur != pos {
                    pos = cur;
                    continue;
                }
                if !on_full(attempt) {
                    return Err(Full);
                }
                attempt = attempt.saturating_add(1);
            }
        }
        // pos is exclusively ours until the seq commit store.
        let msg = slot_ptr(self.slots, pos, self.mask, self.slot_size) as *mut T;
        // pos + M + 1, not Vyukov's pos + 1: at M = 1 that would
        // equal the released value pos + M.
        let commit = pos.wrapping_add(self.commit_add);
        let guard = TombstoneOnUnwind {
            seq: self.seq(pos),
            commit,
        };
        // SAFETY: msg is in-bounds and aligned (check_type +
        // cache-line slot base), any byte pattern is a valid T
        // (FromBytes bound), and the claim CAS gives exclusive
        // slot access until the commit store below.
        fill(unsafe { &mut *msg });
        // Normal path: disarm the unwind guard, then publish.
        let seq = guard.seq;
        core::mem::forget(guard);
        // Release pairs with the consumer's Acquire seq load:
        // observing pos + M + 1 means the filled bytes are
        // visible.
        seq.store(commit, Ordering::Release);
        Ok(())
    }
}
