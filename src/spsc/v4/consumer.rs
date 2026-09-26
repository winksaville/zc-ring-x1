//! Consuming endpoint: [`Consumer`] finds the oldest committed
//! slot of its current segment by the seq word at its front, and
//! the [`ReadSlot`] guard reads the body in place and releases
//! through the same word, following the producer to the next
//! segment after a MOVED message.

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, Immutable, KnownLayout};

use super::{
    MAX_SEGMENTS, MOVED, SEG_MASK, SEG_SHIFT, SEQ_MASK, Segments, check_body_type, seq_of,
};
use crate::Empty;

/// The consumer's private state, held apart from the handle so a
/// guard can borrow it without naming the region's lifetime.
pub(super) struct ConsumerState {
    /// Geometry and segment addresses.
    pub(super) segs: Segments,
    /// The id this consumer's claim wrote into the role word.
    pub(super) holder: u32,
    /// The segment being read.
    pub(super) cur: u32,
    /// Free-running position in `cur`.
    pub(super) pos: u32,
    /// Where each segment was left, the position a reuse starts
    /// at, the same values the producer keeps.
    pub(super) resume: [u32; MAX_SEGMENTS as usize],
    /// The give-back word's value: this side is its only writer.
    pub(super) given: u32,
    /// Segment switches so far, counted on the switch path only.
    pub(super) switches: u64,
}

/// The consuming endpoint: `reserve_slot_with` the oldest
/// committed slot, read in place, `release`.
///
/// - It has no `Drop`, as [`Producer`](super::Producer) has none:
///   dropping it leaves the role held, and giving the role back is
///   [`release`](Consumer::release).
pub struct Consumer<'a> {
    /// Private state, borrowed by each guard.
    pub(super) st: ConsumerState,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the consumer role. See the Producer
// Send rationale.
unsafe impl Send for Consumer<'_> {}

impl<'a> Consumer<'a> {
    /// Start in segment 0, where the producer starts, for holder
    /// `holder`.
    pub(super) fn new(segs: Segments, holder: u32) -> Self {
        Consumer {
            st: ConsumerState {
                segs,
                holder,
                cur: 0,
                pos: 0,
                resume: [0; MAX_SEGMENTS as usize],
                given: 0,
                switches: 0,
            },
            _region: PhantomData,
        }
    }

    /// Give the consumer role back as released, the counterpart
    /// of [`Producer::release`](super::Producer::release),
    /// writing its position first.
    pub fn release(self) {
        let segs = &self.st.segs;
        segs.claims().cons_pos.store(self.st.pos, Ordering::Release);
        Segments::release_role(segs.consumer_role(), self.st.holder);
    }

    /// Segment switches this consumer has made: how many MOVED
    /// messages it has released. Once it has read everything the
    /// producer sent, it equals the producer's count.
    pub fn switches(&self) -> u64 {
        self.st.switches
    }

    /// The segment this consumer reads from, `0` to the ring's
    /// segment count less one.
    pub fn segment(&self) -> u32 {
        self.st.cur
    }

    /// Reserve the oldest unread slot as a `&T`, applying an
    /// injected wait policy: retry until a message arrives or the
    /// policy gives up, then [`Empty`].
    ///
    /// - One load of the slot's word tells everything: its seq bits
    ///   at `c + M + 1` mean a message, and a MOVED bit with it
    ///   means the producer's last in this segment. Anything else
    ///   reads as Empty.
    /// - A MOVED word naming a segment the ring does not have reads
    ///   as Empty, failing toward Empty as the rings do.
    /// - Guard semantics as v2's: one reservation at a time, drop
    ///   without release re-delivers the same slot.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_empty: impl FnMut(u32) -> bool,
    ) -> Result<ReadSlot<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        let st = &mut self.st;
        check_body_type::<T>(st.segs.slot_size);
        let c = st.pos;
        let expected = seq_of(c.wrapping_add(st.segs.capacity).wrapping_add(1));
        let seq = st.segs.seq(st.cur, c);
        let mut attempt = 0u32;
        let word = loop {
            // Acquire pairs with the producer's Release commit:
            // observing the committed seq means the fill is visible.
            let word = seq.load(Ordering::Acquire);
            if word & SEQ_MASK == expected
                && (word & MOVED == 0 || (word >> SEG_SHIFT) & SEG_MASK < st.segs.seg_count)
            {
                break word;
            }
            if !on_empty(attempt) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        };
        let msg = st.segs.body(st.cur, c) as *const T;
        Ok(ReadSlot {
            st,
            msg,
            word,
            _slot: PhantomData,
        })
    }
}

/// A reserved read slot: `Deref` to read the message, then
/// [`release`](ReadSlot::release).
pub struct ReadSlot<'c, T> {
    /// The consumer's state, for the release and any switch.
    st: &'c mut ConsumerState,
    /// The slot body, viewed as the message type.
    msg: *const T,
    /// The committed word the reserve loaded.
    word: u32,
    /// Owns the `&'c mut` borrow of the consumer.
    _slot: PhantomData<&'c T>,
}

impl<T> Deref for ReadSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_body_type
        // against the body's offset in a line-aligned slot), any
        // byte pattern is a valid T (FromBytes bound), and the seq
        // protocol gives this guard read access until release.
        unsafe { &*self.msg }
    }
}

impl<T> ReadSlot<'_, T> {
    /// Free the slot for reuse.
    ///
    /// - The released seq store comes first (`Release`), clearing
    ///   any MOVED bits, so a segment given back holds only
    ///   claimable seqs.
    /// - After a MOVED message: checkpoint the switch under its
    ///   intent word before the release, give the old segment back
    ///   by flipping its bit in the give-back word (`Release`),
    ///   clear the intent, then continue in the named segment
    ///   where it was left.
    pub fn release(self) {
        let st = self.st;
        let segs = &st.segs;
        let c = st.pos;
        let next = c.wrapping_add(1);
        let moved = self.word & MOVED != 0;
        let k = (self.word >> SEG_SHIFT) & SEG_MASK;
        if moved {
            st.given ^= 1 << st.cur;
            st.resume[st.cur as usize] = next;
            segs.consumer_switch(st.cur, k, next, st.given);
        }
        segs.seq(st.cur, c)
            .store(seq_of(c.wrapping_add(segs.capacity)), Ordering::Release);
        st.pos = next;
        if moved {
            segs.given().store(st.given, Ordering::Release);
            Segments::switch_done(&segs.claims().cons_switch);
            st.cur = k;
            st.pos = st.resume[k as usize];
            st.switches += 1;
        }
    }
}
