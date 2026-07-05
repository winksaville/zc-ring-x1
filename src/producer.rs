//! Producing endpoint: [`Producer`] reserves the next free
//! slot, the [`WriteSlot`] guard writes it in place and
//! commits.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use crate::{Header, USER_WORDS, check_type, slot_ptr};

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

    /// The header's app-owned scratch line: zeroed at init,
    /// never touched by the crate again.
    ///
    /// - Shared with the peer: treat contents as untrusted
    ///   data — store values, never addresses to dereference.
    /// - See the design doc's "Blocking and user words" for
    ///   the wakeup-protocol contract it exists to host.
    pub fn user(&self) -> &[AtomicU32; USER_WORDS] {
        &self.header.user
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

    /// [`reserve_slot`](Producer::reserve_slot) with an
    /// injected wait policy: retry until a slot frees up or
    /// the policy gives up.
    ///
    /// - `on_full` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Full)`.
    /// - The wait loop is the reservation itself (the guard
    ///   borrows the endpoint, so retrying a `reserve_slot`
    ///   call could not return it): `producer_idx` is ours
    ///   and loaded once; only `consumer_idx` is re-read per
    ///   attempt, so the no-wait fast path does exactly the
    ///   loads `reserve_slot` does.
    /// - See [`policy`](crate::policy) for shipped policies
    ///   and the composition model.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_full: impl FnMut(u32) -> bool,
    ) -> Result<WriteSlot<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.slot_size);
        // Ours alone: no peer moves it, load once.
        let p = self.header.producer_idx.load(Ordering::Relaxed);
        let mut attempt = 0u32;
        loop {
            // Same load + `>=` occupancy check as
            // reserve_slot (see there for the corruption
            // rationale).
            let c = self.header.consumer_idx.load(Ordering::Acquire);
            if p.wrapping_sub(c) < self.capacity {
                break;
            }
            if !on_full(attempt) {
                return Err(Full);
            }
            attempt = attempt.saturating_add(1);
        }
        // Raw pointer, not `&mut T` — see reserve_slot.
        let msg = slot_ptr(self.slots, p, self.mask, self.slot_size) as *mut T;
        Ok(WriteSlot {
            header: self.header,
            msg,
            next_idx: p.wrapping_add(1),
            _slot: PhantomData,
        })
    }

    /// [`reserve_slot`](Producer::reserve_slot), spinning
    /// until a slot is available —
    /// [`reserve_slot_with`](Producer::reserve_slot_with)
    /// under the never-quitting
    /// [`policy::spin`](crate::policy::spin), so no `Result`.
    pub fn reserve_slot_spin<T>(&mut self) -> WriteSlot<'_, T>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        match self.reserve_slot_with(crate::policy::spin) {
            Ok(slot) => slot,
            Err(Full) => unreachable!("policy::spin never gives up"),
        }
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
