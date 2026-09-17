//! MPSC v2 producing endpoint: [`MpscProducer`] claims a
//! position by CAS on the packed claim word, a closure fills the
//! slot in place, and the commit happens on closure return, as
//! v1's. At a full segment it takes a free one and moves the ring
//! on, sealing the old segment behind it.

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{MOVED, Segments, TOMBSTONE, check_body_type, seq_of, word, word_pos, word_seg};
use crate::Full;

/// A producing handle: `Clone` one per producing thread, then
/// `send_with`.
///
/// - `send_with` takes `&self`: exclusivity comes from the claim
///   CAS, not the borrow, so one handle may also be shared by
///   reference.
pub struct MpscProducer<'a> {
    /// Geometry and segment addresses.
    pub(super) segs: Segments,
    _region: PhantomData<&'a [u8]>,
}

impl Clone for MpscProducer<'_> {
    /// A second producing handle over the same ring, and the claim
    /// CAS serializes them.
    fn clone(&self) -> Self {
        MpscProducer { ..*self }
    }
}

// SAFETY: any number of producers is the protocol contract: the
// shared state (the header words, the slot seqs) is atomic, slot
// claims are exclusive by CAS, and slot writes are handed off
// with Release/Acquire ordering.
unsafe impl Send for MpscProducer<'_> {}
// SAFETY: send_with is &self and every access is protected as
// above, so shared references across threads are equally fine.
unsafe impl Sync for MpscProducer<'_> {}

/// Commits-on-drop guard armed while the fill closure runs: a
/// panic unwinding through `send_with` must not abandon the
/// claimed position, so drop-without-disarm publishes the
/// tombstoned commit the consumer releases without delivering.
struct TombstoneOnUnwind<'s> {
    /// The claimed slot's seq word.
    seq: &'s AtomicU32,
    /// The commit value (`pos + M + 1`).
    commit: u32,
}

impl Drop for TombstoneOnUnwind<'_> {
    /// Unwind path only, and the normal path disarms with
    /// `mem::forget`.
    fn drop(&mut self) {
        self.seq.store(self.commit | TOMBSTONE, Ordering::Release);
    }
}

/// Why a switch attempt did not move the ring.
enum NoSwitch {
    /// No segment is free: the ring is Full.
    NoFree,
    /// The claim word moved under the attempt: another producer
    /// switched, or the slot was released and claimed.
    Lost,
}

impl<'a> MpscProducer<'a> {
    /// Build the handle from [`MpscRing::split`](super::MpscRing::split)'s
    /// geometry snapshot.
    pub(super) fn new(segs: Segments) -> Self {
        MpscProducer {
            segs,
            _region: PhantomData,
        }
    }

    /// Segment switches the ring's producers have made: how many
    /// times a producer at a full segment moved the ring to a
    /// free one. Once the consumer has read everything sent, it
    /// equals the consumer's count.
    pub fn switches(&self) -> u64 {
        self.segs.switches().load(Ordering::Relaxed) as u64
    }

    /// The segment the ring's producers write into, `0` to the
    /// ring's segment count less one.
    pub fn segment(&self) -> u32 {
        word_seg(self.segs.claim().load(Ordering::Relaxed))
    }

    /// Claim the next position, fill it in place, commit on
    /// closure return, and retry a full ring under the injected wait
    /// policy, then [`Full`].
    ///
    /// - `fill` writes the message through `&mut T`, and commit is by
    ///   construction, so there is no abandonment state. If `fill`
    ///   panics, the unwind publishes a tombstoned commit the
    ///   consumer skips.
    /// - `on_full` is called after each failed attempt with the
    ///   attempt count (0-based, saturating), and returning `false`
    ///   gives up. Pass `|_| false` for a single non-blocking
    ///   probe. Full means the current segment's next slot is
    ///   unread and no segment is free.
    /// - Losing a claim race, or a switch race, is not a policy
    ///   call: the ring made progress, and this producer retries
    ///   with the fresher claim word.
    /// - `T` must fit the slot body, `slot_size` less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), at an
    ///   alignment of at most that.
    pub fn send_with<T>(
        &self,
        mut on_full: impl FnMut(u32) -> bool,
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let segs = &self.segs;
        check_body_type::<T>(segs.slot_size);
        let claim = segs.claim();
        // SeqCst on every claim word access, as v1's index: the
        // claim is only exclusive if these are linearizable.
        let mut w = claim.load(Ordering::SeqCst);
        let mut attempt = 0u32;
        let (seg, pos) = loop {
            let seg = word_seg(w);
            let pos = word_pos(w);
            if seg >= segs.seg_count {
                // A scribbled claim word: the ring is wedged, and
                // this degrades toward Full, never toward a slot
                // the ring does not have.
                return Err(Full);
            }
            // Acquire pairs with the consumer's Release in
            // release(): a claimable seq means the previous lap's
            // reads are done.
            let seq = segs.seq(seg, pos).load(Ordering::Acquire);
            if seq == pos {
                // Claimable. Weak CAS: a spurious failure just
                // retries with the fresher word.
                match claim.compare_exchange_weak(
                    w,
                    word(seg, pos.wrapping_add(1)),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break (seg, pos),
                    Err(actual) => w = actual,
                }
                continue;
            }
            // Not claimable at pos: a stale word or a full
            // segment, told apart by re-reading, as v1 does.
            let cur = claim.load(Ordering::SeqCst);
            if cur != w {
                w = cur;
                continue;
            }
            match self.switch(w) {
                Ok(()) | Err(NoSwitch::Lost) => {}
                Err(NoSwitch::NoFree) => {
                    if !on_full(attempt) {
                        return Err(Full);
                    }
                    attempt = attempt.saturating_add(1);
                }
            }
            w = claim.load(Ordering::SeqCst);
        };
        // (seg, pos) is exclusively ours until the seq commit
        // store.
        let msg = segs.body(seg, pos) as *mut T;
        let commit = seq_of(pos.wrapping_add(segs.commit_add));
        let guard = TombstoneOnUnwind {
            seq: segs.seq(seg, pos),
            commit,
        };
        // SAFETY: msg is in-bounds and aligned (check_body_type
        // against the body's offset in a line-aligned slot), any
        // byte pattern is a valid T (FromBytes bound), and the
        // claim CAS gives exclusive slot access until the commit
        // store below.
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

    /// Move the ring from the full segment and position `w` names
    /// to a free segment.
    ///
    /// - Take the lowest free segment by setting its bit in the
    ///   in-use word, which serializes producers switching at
    ///   once and succeeds only where the bit was clear, clear its
    ///   seal, and CAS the claim word from `w` to the new segment
    ///   at its resume position, the position its seal held.
    /// - On success, seal the old segment: MOVED, the new segment,
    ///   and `w`'s position as the end. Every claim in the old
    ///   segment lies before it.
    /// - On a lost claim CAS, restore the seal and clear the bit:
    ///   the ring moved under the attempt, and the caller retries
    ///   with the fresh word.
    fn switch(&self, w: u32) -> Result<(), NoSwitch> {
        let segs = &self.segs;
        let in_use = segs.in_use();
        loop {
            // Acquire: a consumer's give-back is visible with its
            // released seqs.
            let free = !in_use.load(Ordering::Acquire) & segs.all();
            if free == 0 {
                return Err(NoSwitch::NoFree);
            }
            let k = free.trailing_zeros();
            if in_use.fetch_or(1 << k, Ordering::AcqRel) & (1 << k) != 0 {
                // Another producer set it first.
                continue;
            }
            // Segment k is ours among producers, and the consumer
            // cannot enter it until a seal names it, so nobody else
            // reads or writes its seal here.
            let seal = segs.seal(k);
            let start = word_pos(seal.load(Ordering::Acquire));
            // Cleared before the claim CAS: a clear after it could
            // land after a later producer's seal of k, once the ring
            // has filled k and left it again, and wipe that seal.
            seal.store(0, Ordering::Relaxed);
            match segs.claim().compare_exchange(
                w,
                word(k, start),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    // Release pairs with the consumer's Acquire seal
                    // load: seeing MOVED means k's seal is clear.
                    segs.seal(word_seg(w))
                        .store(MOVED | word(k, word_pos(w)), Ordering::Release);
                    segs.switches().fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(_) => {
                    // Put k back as it was: its resume position for
                    // the next taker, then its bit.
                    seal.store(start, Ordering::Relaxed);
                    in_use.fetch_and(!(1 << k), Ordering::AcqRel);
                    return Err(NoSwitch::Lost);
                }
            }
        }
    }
}
