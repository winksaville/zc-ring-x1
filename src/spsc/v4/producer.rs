//! Producing endpoint: [`Producer`] claims the next slot of its
//! current segment by the seq word at its front, and the
//! [`WriteSlot`] guard writes the body in place and commits
//! through the same word, switching segments when the next slot
//! is taken.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{MAX_SEGMENTS, MOVED, SEG_SHIFT, Segments, check_body_type, seq_of};
use crate::Full;

/// The producer's private state, held apart from the handle so a
/// guard can borrow it without naming the region's lifetime.
pub(super) struct ProducerState {
    /// Geometry and segment addresses.
    pub(super) segs: Segments,
    /// The id this producer's claim wrote into the role word.
    pub(super) holder: u32,
    /// The segment being written.
    pub(super) cur: u32,
    /// Free-running position in `cur`.
    pub(super) pos: u32,
    /// Where each segment was left, the position a reuse starts
    /// at. The consumer keeps the same values of its own.
    pub(super) resume: [u32; MAX_SEGMENTS as usize],
    /// One bit per segment, flipped on each take: free where it
    /// agrees with the consumer's give-back word.
    pub(super) taken: u32,
    /// The slot at `pos` was seen claimable and so still is: only
    /// the producer turns claimable into anything else.
    pub(super) claimable: bool,
    /// Segment switches so far, counted on the switch path only.
    pub(super) switches: u64,
}

/// The producing endpoint: `reserve_slot_with`, write in place,
/// `commit`.
///
/// - It has no `Drop`: a destructor never touches shared memory,
///   so dropping it leaves the role held, and giving the role back
///   is [`release`](Producer::release).
pub struct Producer<'a> {
    /// Private state, borrowed by each guard.
    pub(super) st: ProducerState,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the producer role. The shared state it
// touches (slot seqs, the give-back word) is atomic, and slot
// writes are handed off with Release/Acquire ordering.
unsafe impl Send for Producer<'_> {}

impl<'a> Producer<'a> {
    /// Start in segment 0, held as taken, for holder `holder`.
    pub(super) fn new(segs: Segments, holder: u32) -> Self {
        Producer {
            st: ProducerState {
                segs,
                holder,
                cur: 0,
                pos: 0,
                resume: [0; MAX_SEGMENTS as usize],
                taken: 1,
                claimable: false,
                switches: 0,
            },
            _region: PhantomData,
        }
    }

    /// Give the producer role back as released, so a later claim
    /// may take it.
    ///
    /// - Writes the one part of the checkpoint a switch does not,
    ///   the position, so the checkpoint is exact.
    /// - One CAS on the role word from this producer's id, so a
    ///   producer whose role was taken over releases nothing.
    pub fn release(self) {
        let segs = &self.st.segs;
        segs.claims().prod_pos.store(self.st.pos, Ordering::Release);
        Segments::release_role(segs.producer_role(), self.st.holder);
    }

    /// Segment switches this producer has made: how many of its
    /// commits carried a message into a new segment.
    pub fn switches(&self) -> u64 {
        self.st.switches
    }

    /// The segment this producer writes into, `0` to the ring's
    /// segment count less one.
    pub fn segment(&self) -> u32 {
        self.st.cur
    }

    /// Reserve the next free slot as a `&mut T`, applying an
    /// injected wait policy: retry until the slot frees up or the
    /// policy gives up, then [`Full`].
    ///
    /// - The slot at `p` is free when its seq is `p`, as v2's. A
    ///   slot the last commit already saw claimable is not loaded
    ///   again.
    /// - Full here means the current segment's next slot is still
    ///   unread and no segment was free at the last commit: the
    ///   ring waits in place as one ring does.
    /// - `T` must fit the slot body, `slot_size` less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), at an
    ///   alignment of at most that.
    /// - Guard semantics as v2's: one reservation at a time, drop
    ///   without commit abandons it.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_full: impl FnMut(u32) -> bool,
    ) -> Result<WriteSlot<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let st = &mut self.st;
        check_body_type::<T>(st.segs.slot_size);
        let p = st.pos;
        if !st.claimable {
            let claimable = seq_of(p);
            let mut attempt = 0u32;
            loop {
                // Acquire pairs with the consumer's Release release:
                // observing `p` means its read of the slot is done.
                if st.segs.seq(st.cur, p).load(Ordering::Acquire) == claimable {
                    break;
                }
                if !on_full(attempt) {
                    return Err(Full);
                }
                attempt = attempt.saturating_add(1);
            }
            st.claimable = true;
        }
        let msg = st.segs.body(st.cur, p) as *mut T;
        Ok(WriteSlot {
            st,
            msg,
            _slot: PhantomData,
        })
    }
}

/// A reserved write slot: `DerefMut` to write the message, then
/// [`commit`](WriteSlot::commit).
pub struct WriteSlot<'p, T> {
    /// The producer's state, for the commit and any switch.
    st: &'p mut ProducerState,
    /// The slot body, viewed as the message type.
    msg: *mut T,
    /// Owns the `&'p mut` borrow of the producer.
    _slot: PhantomData<&'p mut T>,
}

impl<T> Deref for WriteSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_body_type
        // against the body's offset in a line-aligned slot), any
        // byte pattern is a valid T (FromBytes bound at
        // reserve_slot_with), and the seq protocol gives this
        // guard exclusive slot access until commit.
        unsafe { &*self.msg }
    }
}

impl<T> DerefMut for WriteSlot<'_, T> {
    /// Write access to the in-slot message.
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in deref. &mut self gives exclusivity of the
        // minted reference.
        unsafe { &mut *self.msg }
    }
}

impl<T> WriteSlot<'_, T> {
    /// Publish the slot to the consumer.
    ///
    /// - Looks at the next slot first. Claimable: an ordinary
    ///   commit, and the next reserve skips its load.
    /// - Not claimable, the segment about to be full: with a free
    ///   segment, take it and commit with MOVED and its number,
    ///   so this message is the last in the old segment. With
    ///   none, commit plainly and let the next reserve wait.
    /// - A switch checkpoints ahead of the commit, under its
    ///   intent word, and clears the word after it.
    /// - The seq store is the last protocol store (`Release`),
    ///   after every private update, the protocol-visible
    ///   handoff.
    pub fn commit(self) {
        let st = self.st;
        let segs = &st.segs;
        let p = st.pos;
        let next = p.wrapping_add(1);
        let slot = segs.seq(st.cur, p);
        let mut word = seq_of(p.wrapping_add(segs.capacity).wrapping_add(1));
        // At depth 1 the next slot is this one, still claimable
        // at `p`, so it never reads as claimable at `next`.
        st.claimable = segs.seq(st.cur, next).load(Ordering::Acquire) == seq_of(next);
        st.pos = next;
        let mut switched = false;
        if !st.claimable {
            let free = !(st.taken ^ segs.given().load(Ordering::Acquire)) & segs.all();
            if free != 0 {
                let k = free.trailing_zeros();
                st.taken ^= 1 << k;
                word |= MOVED | (k << SEG_SHIFT);
                st.resume[st.cur as usize] = next;
                segs.producer_switch(st.cur, k, next, st.taken);
                st.cur = k;
                st.pos = st.resume[k as usize];
                st.switches += 1;
                switched = true;
            }
        }
        slot.store(word, Ordering::Release);
        if switched {
            Segments::switch_done(&segs.claims().prod_switch);
        }
    }
}
