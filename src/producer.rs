//! Producing endpoint: [`Producer`] reserves the next free
//! slot, the [`WriteSlot`] guard writes it in place and
//! commits.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use crate::{Header, check_type, slot_ptr};

/// `reserve_slot` failed: every slot holds an
/// uncommitted-or-unread message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

/// The producing endpoint: `reserve_slot`, write in place,
/// `commit`.
pub struct Producer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`Ring`](crate::Ring)).
    slot_size: u32,
    /// Geometry snapshot (see [`Ring`](crate::Ring)).
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the producer role; the shared state it
// touches (indices) is atomic, and slot writes are handed off
// with Release/Acquire ordering.
unsafe impl Send for Producer<'_> {}

impl<'a> Producer<'a> {
    /// Build the handle from [`Ring::split`](crate::Ring::split)'s
    /// geometry snapshot.
    pub(crate) fn new(
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

    /// Reserve the next free slot as a `&mut T`, or [`Full`].
    ///
    /// - Only one slot may be reserved at a time: the guard
    ///   holds the `&mut Producer` borrow, so a second
    ///   `reserve_slot` before the guard is dropped or
    ///   committed does not compile.
    /// - Dropping the guard without [`WriteSlot::commit`]
    ///   abandons the reservation (nothing published).
    pub fn reserve_slot<T>(&mut self) -> Result<WriteSlot<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.slot_size);
        let p = self.header.producer_idx.load(Ordering::Relaxed);
        let c = self.header.consumer_idx.load(Ordering::Acquire);
        // `>=`, not `==`: a peer-corrupted consumer_idx makes
        // occupancy look huge — fail Full, never hand out a
        // slot the protocol doesn't own.
        if p.wrapping_sub(c) >= self.capacity {
            return Err(Full);
        }
        // Raw pointer, not `&mut T`: a reference field would be
        // argument-protected for the whole `commit(self)` call,
        // while commit's Release store lets the consumer read
        // the slot inside that window — UB (Miri-verified).
        // Deref mints short-lived references instead.
        let msg = slot_ptr(self.slots, p, self.mask, self.slot_size) as *mut T;
        Ok(WriteSlot {
            header: self.header,
            msg,
            next_idx: p.wrapping_add(1),
            _slot: PhantomData,
        })
    }
}

/// A reserved write slot: `DerefMut` to write the message, then
/// [`commit`](WriteSlot::commit).
pub struct WriteSlot<'p, T> {
    /// The ring's control block (for the commit store).
    header: &'p Header,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see the comment in [`Producer::reserve_slot`].
    msg: *mut T,
    /// Value `producer_idx` takes on commit.
    next_idx: u32,
    /// Owns the `&'p mut` borrow of the producer.
    _slot: PhantomData<&'p mut T>,
}

impl<T> Deref for WriteSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_type +
        // cache-line slot base), any byte pattern is a valid T
        // (FromBytes bound at reserve_slot), and the index
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
    /// Publish the slot to the consumer
    /// (`producer_idx + 1`, `Release`).
    pub fn commit(self) {
        self.header
            .producer_idx
            .store(self.next_idx, Ordering::Release);
    }
}
