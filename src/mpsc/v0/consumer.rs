//! MPSC consuming endpoint: [`MpscConsumer`] reserves the
//! oldest committed slot through the [`MpscReadSlot`] guard —
//! the SPSC consumer's shape, plus tombstone skipping (an
//! unwound producer's slot is released without delivery).

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, Immutable, KnownLayout};

use super::{MpscHeader, TOMBSTONE};
use crate::{Empty, USER_WORDS, check_type, slot_ptr};

/// The consuming handle: single per ring, CAS-free —
/// `reserve_slot_with` the oldest committed slot, read in
/// place, `release`.
pub struct MpscConsumer<'a> {
    /// The ring's control block.
    header: &'a MpscHeader,
    /// Base of the per-slot sequence array.
    seqs: *const AtomicU32,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`MpscRing`](super::MpscRing)).
    slot_size: u32,
    /// Geometry snapshot; release stores `pos + capacity`.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the single-consumer role; shared
// state (seqs, indices) is atomic with Release/Acquire
// handoff.
unsafe impl Send for MpscConsumer<'_> {}

impl<'a> MpscConsumer<'a> {
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
        MpscConsumer {
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

    /// Reserve the oldest committed slot as a `&T`, applying
    /// an injected wait policy: retry until a message arrives
    /// or the policy gives up → [`Empty`].
    ///
    /// - The guard behaves as the SPSC
    ///   [`ReadSlot`](crate::ReadSlot): drop without
    ///   [`MpscReadSlot::release`] re-delivers the same slot.
    /// - Tombstoned slots (a producer panicked mid-fill) are
    ///   released and skipped inline; skipping is progress,
    ///   not an attempt, so the policy is not consulted for
    ///   them.
    /// - `on_empty` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Empty)`. Pass `|_| false`
    ///   for a single non-blocking probe.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_empty: impl FnMut(u32) -> bool,
    ) -> Result<MpscReadSlot<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        check_type::<T>(self.slot_size);
        let mut attempt = 0u32;
        // Ours alone: producers never read it, only we store it.
        let mut c = self.header.consumer_idx.load(Ordering::Relaxed);
        loop {
            let expected = c.wrapping_add(1);
            // Acquire pairs with the producer's Release commit:
            // observing expected means the fill is visible.
            let seq = self.seq(c).load(Ordering::Acquire);
            let diff = seq.wrapping_sub(expected);
            if diff == 0 {
                break;
            }
            if diff == TOMBSTONE {
                // Unwound producer: release the slot without
                // delivering and move on.
                self.header.consumer_idx.store(expected, Ordering::Relaxed);
                self.seq(c)
                    .store(c.wrapping_add(self.capacity), Ordering::Release);
                c = expected;
                continue;
            }
            // Not committed yet (or a peer-corrupted seq —
            // degrade toward Empty, never toward reading an
            // unowned slot).
            if !on_empty(attempt) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        }
        // Raw pointer, not `&T` — same argument-protector
        // rationale as the SPSC ReadSlot.
        let msg = slot_ptr(self.slots, c, self.mask, self.slot_size) as *const T;
        Ok(MpscReadSlot {
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
/// [`release`](MpscReadSlot::release).
pub struct MpscReadSlot<'c, T> {
    /// The ring's control block (for the consumer_idx store).
    header: &'c MpscHeader,
    /// The reserved slot's seq word (for the release store).
    seq: &'c AtomicU32,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see the SPSC `ReadSlot`.
    msg: *const T,
    /// Value `consumer_idx` takes on release.
    next_idx: u32,
    /// Value the seq takes on release (`pos + capacity`):
    /// claimable again one lap later.
    released_seq: u32,
    /// Owns the `&'c` borrow of the consumer.
    _slot: PhantomData<&'c T>,
}

impl<T> Deref for MpscReadSlot<'_, T> {
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

impl<T> MpscReadSlot<'_, T> {
    /// Free the slot for reuse.
    ///
    /// - `consumer_idx` first (`Relaxed` — consumer-private
    ///   resume state, racing readers of it don't exist), the
    ///   seq store last (`Release` — the protocol-visible
    ///   handoff producers acquire). A crash between the two
    ///   is outside the contract, as any torn-state crash is.
    pub fn release(self) {
        self.header
            .consumer_idx
            .store(self.next_idx, Ordering::Relaxed);
        self.seq.store(self.released_seq, Ordering::Release);
    }
}
