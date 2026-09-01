//! Consuming endpoint: [`Consumer`] finds the oldest committed
//! slot by its seq word, the [`ReadSlot`] guard reads it in
//! place and releases through the same word.

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, Immutable, KnownLayout};

use super::Header;
use crate::{Empty, USER_WORDS, check_type, slot_ptr};

/// The consuming endpoint: `reserve_slot_with` the oldest
/// committed slot, read in place, `release`.
pub struct Consumer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the per-slot sequence array.
    seqs: *const AtomicU32,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`Ring`](super::Ring)).
    slot_size: u32,
    /// Geometry snapshot; release stores `pos + capacity`.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the consumer role; see the Producer
// Send rationale.
unsafe impl Send for Consumer<'_> {}

impl<'a> Consumer<'a> {
    /// Build the handle from [`Ring::split`](super::Ring::split)'s
    /// geometry snapshot.
    pub(super) fn new(
        header: &'a Header,
        seqs: *const AtomicU32,
        slots: *mut u8,
        slot_size: u32,
        capacity: u32,
        mask: u32,
    ) -> Self {
        Consumer {
            header,
            seqs,
            slots,
            slot_size,
            capacity,
            mask,
            _region: PhantomData,
        }
    }

    /// The header's app-owned scratch line — same contract as
    /// the v0 endpoints' `user()`.
    pub fn user(&self) -> &[AtomicU32; USER_WORDS] {
        &self.header.user
    }

    /// The seq word for free-running position `idx`.
    fn seq(&self, idx: u32) -> &AtomicU32 {
        // SAFETY: idx is masked to < capacity, and the array is
        // capacity × SEQ_STRIDE bytes, validated at init/attach.
        unsafe {
            &*self
                .seqs
                .byte_add((idx & self.mask) as usize * super::SEQ_STRIDE)
        }
    }

    /// Reserve the oldest unread slot as a `&T`, applying an
    /// injected wait policy: retry until a message arrives or
    /// the policy gives up → [`Empty`].
    ///
    /// - The slot at `c` is committed when `seq == c + M + 1`.
    ///   Anything else (not yet committed, or a peer-corrupted
    ///   seq) reads as Empty, never as a readable slot.
    /// - Only `consumer_idx` (ours, loaded once) and the slot's
    ///   seq are touched: the producer's index line is never
    ///   read.
    /// - Guard semantics as v0's [`ReadSlot`](crate::ReadSlot):
    ///   one reservation at a time, drop without release
    ///   re-delivers the same slot.
    /// - `on_empty` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Empty)`. Pass `|_| false`
    ///   for a single non-blocking probe.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_empty: impl FnMut(u32) -> bool,
    ) -> Result<ReadSlot<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        check_type::<T>(self.slot_size);
        // Ours alone: the producer never reads or writes it.
        let c = self.header.consumer_idx.load(Ordering::Relaxed);
        let expected = c.wrapping_add(self.capacity).wrapping_add(1);
        let mut attempt = 0u32;
        loop {
            // Acquire pairs with the producer's Release commit:
            // observing expected means the fill is visible.
            if self.seq(c).load(Ordering::Acquire) == expected {
                break;
            }
            if !on_empty(attempt) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        }
        // Raw pointer, not `&T` — same argument-protector
        // rationale as v0's ReadSlot.
        let msg = slot_ptr(self.slots, c, self.mask, self.slot_size) as *const T;
        Ok(ReadSlot {
            header: self.header,
            seq: self.seq(c),
            msg,
            next_idx: c.wrapping_add(1),
            released_seq: c.wrapping_add(self.capacity),
            _slot: PhantomData,
        })
    }
}

/// A reserved read slot: `Deref` to read the message, then
/// [`release`](ReadSlot::release).
pub struct ReadSlot<'c, T> {
    /// The ring's control block (for the consumer_idx store).
    header: &'c Header,
    /// The reserved slot's seq word (for the release store).
    seq: &'c AtomicU32,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see v0's `ReadSlot`.
    msg: *const T,
    /// Value `consumer_idx` takes on release.
    next_idx: u32,
    /// Value the seq takes on release (`pos + capacity`):
    /// claimable again one lap later.
    released_seq: u32,
    /// Owns the `&'c` borrow of the consumer.
    _slot: PhantomData<&'c T>,
}

impl<T> Deref for ReadSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_type +
        // cache-line slot base), any byte pattern is a valid T
        // (FromBytes bound), and the seq protocol gives this
        // guard read access until release.
        unsafe { &*self.msg }
    }
}

impl<T> ReadSlot<'_, T> {
    /// Free the slot for reuse.
    ///
    /// - `consumer_idx` first (`Relaxed` — consumer-private
    ///   resume state), the seq store last (`Release` — the
    ///   protocol-visible handoff the producer acquires).
    pub fn release(self) {
        self.header
            .consumer_idx
            .store(self.next_idx, Ordering::Relaxed);
        self.seq.store(self.released_seq, Ordering::Release);
    }
}
