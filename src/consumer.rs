//! Consuming endpoint: [`Consumer`] reserves the oldest unread
//! slot, the [`ReadSlot`] guard reads it in place and releases.

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, Immutable, KnownLayout};

use crate::{Header, USER_WORDS, check_type, slot_ptr};

/// `reserve_slot` failed: no unread messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty;

/// The consuming endpoint: `reserve_slot` the oldest unread
/// slot, read in place, `release`.
pub struct Consumer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the slot array.
    slots: *mut u8,
    /// Geometry snapshot (see [`Ring`](crate::Ring)).
    slot_size: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the consumer role; see the Producer
// Send rationale.
unsafe impl Send for Consumer<'_> {}

impl<'a> Consumer<'a> {
    /// Build the handle from [`Ring::split`](crate::Ring::split)'s
    /// geometry snapshot.
    pub(crate) fn new(header: &'a Header, slots: *mut u8, slot_size: u32, mask: u32) -> Self {
        Consumer {
            header,
            slots,
            slot_size,
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

    /// Reserve the oldest unread slot as a `&T`, or [`Empty`].
    ///
    /// - Only one slot may be reserved at a time: the guard
    ///   holds the `&mut Consumer` borrow, so a second
    ///   `reserve_slot` before the guard is dropped or released
    ///   does not compile.
    /// - Dropping the guard without [`ReadSlot::release`]
    ///   leaves the slot unread — the next `reserve_slot`
    ///   returns it again.
    pub fn reserve_slot<T>(&mut self) -> Result<ReadSlot<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        check_type::<T>(self.slot_size);
        let c = self.header.consumer_idx.load(Ordering::Relaxed);
        let p = self.header.producer_idx.load(Ordering::Acquire);
        if p == c {
            return Err(Empty);
        }
        // Raw pointer, not `&T`, for the same reason as in the
        // producer's reserve_slot: release's store frees the
        // slot for producer writes while a reference field
        // would still be argument-protected.
        let msg = slot_ptr(self.slots, c, self.mask, self.slot_size) as *const T;
        Ok(ReadSlot {
            header: self.header,
            msg,
            next_idx: c.wrapping_add(1),
            _slot: PhantomData,
        })
    }
}

/// A reserved read slot: `Deref` to read the message, then
/// [`release`](ReadSlot::release).
pub struct ReadSlot<'c, T> {
    /// The ring's control block (for the release store).
    header: &'c Header,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see the comment in [`Consumer::reserve_slot`].
    msg: *const T,
    /// Value `consumer_idx` takes on release.
    next_idx: u32,
    /// Owns the `&'c` borrow of the consumer.
    _slot: PhantomData<&'c T>,
}

impl<T> Deref for ReadSlot<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_type +
        // cache-line slot base), any byte pattern is a valid T
        // (FromBytes bound at reserve_slot), and the index
        // protocol gives this guard read access until release.
        unsafe { &*self.msg }
    }
}

impl<T> ReadSlot<'_, T> {
    /// Free the slot for reuse (`consumer_idx + 1`, `Release`).
    pub fn release(self) {
        self.header
            .consumer_idx
            .store(self.next_idx, Ordering::Release);
    }
}
