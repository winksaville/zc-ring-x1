//! Producing endpoint: [`Producer`] claims the next slot by the
//! seq word at its front, the [`WriteSlot`] guard writes the
//! body behind it in place and commits through the same word.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{Header, Seq, check_body_type, seq_of, slot_parts};
use crate::{Full, USER_WORDS};

/// The producing endpoint: `reserve_slot_with`, write in
/// place, `commit`.
pub struct Producer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`Ring`](super::Ring)).
    slot_size: u32,
    /// Geometry snapshot: commit stores `pos + capacity + 1`.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the producer role. The shared state it
// touches (the slot seqs, its index) is atomic, and slot writes
// are handed off with Release/Acquire ordering.
unsafe impl Send for Producer<'_> {}

impl<'a> Producer<'a> {
    /// Build the handle from [`Ring::split`](super::Ring::split)'s
    /// geometry snapshot.
    pub(super) fn new(
        header: &'a Header,
        slots: *mut u8,
        slot_size: u32,
        capacity: u32,
        mask: u32,
    ) -> Self {
        Producer {
            header,
            slots,
            slot_size,
            capacity,
            mask,
            _region: PhantomData,
        }
    }

    /// The header's app-owned scratch line, the v0 endpoints'
    /// `user()` contract.
    pub fn user(&self) -> &[AtomicU32; USER_WORDS] {
        &self.header.user
    }

    /// Reserve the next free slot as a `&mut T`, applying an
    /// injected wait policy: retry until the slot frees up or
    /// the policy gives up, then [`Full`].
    ///
    /// - The slot at `p` is free when its seq is `p`: the
    ///   consumer released the previous lap by storing
    ///   `pos + M`, which is this lap's `p`. Anything else reads
    ///   as Full, never as a slot the protocol does not own.
    /// - Only `producer_idx` (ours, loaded once) and the slot's
    ///   own line are touched.
    /// - `T` must fit the slot body, `slot_size` less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), at an
    ///   alignment of at most that.
    /// - Guard semantics as v0's
    ///   [`WriteSlot`](crate::WriteSlot): one reservation at a
    ///   time, drop without commit abandons it.
    /// - `on_full` is called after each failed attempt with
    ///   the attempt count (0-based, saturating). Returning
    ///   `false` gives up. Pass `|_| false` for a single
    ///   non-blocking probe.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_full: impl FnMut(u32) -> bool,
    ) -> Result<WriteSlot<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_body_type::<T>(self.slot_size);
        // Ours alone: the consumer never reads or writes it.
        let p = self.header.producer_idx.load(Ordering::Relaxed);
        let (slot, body) = slot_parts(self.slots, p, self.mask, self.slot_size);
        let claimable = seq_of(p);
        let mut attempt = 0u32;
        loop {
            // Acquire pairs with the consumer's Release release:
            // observing `p` means its reads of the previous lap
            // are done.
            if slot.seq.load(Ordering::Acquire) == claimable {
                break;
            }
            if !on_full(attempt) {
                return Err(Full);
            }
            attempt = attempt.saturating_add(1);
        }
        Ok(WriteSlot {
            header: self.header,
            seq: &slot.seq,
            msg: body as *mut T,
            next_idx: p.wrapping_add(1),
            committed_seq: seq_of(p.wrapping_add(self.capacity).wrapping_add(1)),
            _slot: PhantomData,
        })
    }
}

/// A reserved write slot: `DerefMut` to write the message, then
/// [`commit`](WriteSlot::commit).
pub struct WriteSlot<'p, T> {
    /// The ring's control block (for the producer_idx store).
    header: &'p Header,
    /// The reserved slot's seq word (for the commit store).
    seq: &'p Seq,
    /// The slot body, viewed as the message type. Raw on
    /// purpose, see v0's `WriteSlot`.
    msg: *mut T,
    /// Value `producer_idx` takes on commit.
    next_idx: u32,
    /// Value the seq takes on commit (`pos + capacity + 1`),
    /// distinct from claimable and released at every capacity.
    committed_seq: super::SeqInt,
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
        // SAFETY: as in deref. &mut self gives exclusivity of
        // the minted reference.
        unsafe { &mut *self.msg }
    }
}

impl<T> WriteSlot<'_, T> {
    /// Publish the slot to the consumer.
    ///
    /// - `producer_idx` first (`Relaxed`, producer-private
    ///   resume state), the seq store last (`Release`, the
    ///   protocol-visible handoff the consumer acquires).
    pub fn commit(self) {
        self.header
            .producer_idx
            .store(self.next_idx, Ordering::Relaxed);
        self.seq.store(self.committed_seq, Ordering::Release);
    }
}
