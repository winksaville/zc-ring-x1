//! Producing endpoint: [`Producer`] claims the next slot by its
//! seq word, the [`WriteSlot`] guard writes it in place and
//! commits through the same word.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::Header;
use crate::{Full, USER_WORDS, check_type, slot_ptr};

/// The producing endpoint: `reserve_slot_with`, write in
/// place, `commit`.
pub struct Producer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the per-slot sequence array.
    seqs: *const AtomicU32,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`Ring`](super::Ring)).
    slot_size: u32,
    /// Geometry snapshot; commit stores `pos + capacity + 1`.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the producer role; the shared state it
// touches (seqs, its index) is atomic, and slot writes are
// handed off with Release/Acquire ordering.
unsafe impl Send for Producer<'_> {}

impl<'a> Producer<'a> {
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
        Producer {
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

    /// Reserve the next free slot as a `&mut T`, applying an
    /// injected wait policy: retry until the slot frees up or
    /// the policy gives up → [`Full`].
    ///
    /// - The slot at `p` is free when `seq == p`: the consumer
    ///   released the previous lap by storing `pos + M`, which
    ///   is this lap's `p`. Anything else (not yet released, or
    ///   a peer-corrupted seq) reads as Full, never as a slot
    ///   the protocol does not own.
    /// - Only `producer_idx` (ours, loaded once) and the slot's
    ///   seq are touched: the consumer's index line is never
    ///   read.
    /// - Guard semantics as v0's
    ///   [`WriteSlot`](crate::WriteSlot): one reservation at a
    ///   time, drop without commit abandons it.
    /// - `on_full` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Full)`. Pass `|_| false`
    ///   for a single non-blocking probe.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_full: impl FnMut(u32) -> bool,
    ) -> Result<WriteSlot<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.slot_size);
        // Ours alone: the consumer never reads or writes it.
        let p = self.header.producer_idx.load(Ordering::Relaxed);
        let mut attempt = 0u32;
        loop {
            // Acquire pairs with the consumer's Release release:
            // observing `p` means its reads of the previous lap
            // are done.
            if self.seq(p).load(Ordering::Acquire) == p {
                break;
            }
            if !on_full(attempt) {
                return Err(Full);
            }
            attempt = attempt.saturating_add(1);
        }
        // Raw pointer, not `&mut T` — same argument-protector
        // rationale as v0's WriteSlot.
        let msg = slot_ptr(self.slots, p, self.mask, self.slot_size) as *mut T;
        Ok(WriteSlot {
            header: self.header,
            seq: self.seq(p),
            msg,
            next_idx: p.wrapping_add(1),
            committed_seq: p.wrapping_add(self.capacity).wrapping_add(1),
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
    seq: &'p AtomicU32,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see v0's `WriteSlot`.
    msg: *mut T,
    /// Value `producer_idx` takes on commit.
    next_idx: u32,
    /// Value the seq takes on commit (`pos + capacity + 1`):
    /// one past the next lap's claimable value, so committed
    /// and released stay distinct at `capacity == 1`, where
    /// Vyukov's `pos + 1` would equal `pos + M`.
    committed_seq: u32,
    /// Owns the `&'p mut` borrow of the producer.
    _slot: PhantomData<&'p mut T>,
}

impl<T> Deref for WriteSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_type +
        // cache-line slot base), any byte pattern is a valid T
        // (FromBytes bound at reserve_slot_with), and the seq
        // protocol gives this guard exclusive slot access until
        // commit.
        unsafe { &*self.msg }
    }
}

impl<T> DerefMut for WriteSlot<'_, T> {
    /// Write access to the in-slot message.
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in deref; &mut self gives exclusivity of
        // the minted reference.
        unsafe { &mut *self.msg }
    }
}

impl<T> WriteSlot<'_, T> {
    /// Publish the slot to the consumer.
    ///
    /// - `producer_idx` first (`Relaxed` — producer-private
    ///   resume state), the seq store last (`Release` — the
    ///   protocol-visible handoff the consumer acquires).
    pub fn commit(self) {
        self.header
            .producer_idx
            .store(self.next_idx, Ordering::Relaxed);
        self.seq.store(self.committed_seq, Ordering::Release);
    }
}
