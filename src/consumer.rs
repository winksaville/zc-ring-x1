//! Consuming endpoint: [`Consumer`] reserves the oldest unread
//! slot, the [`ReadSlot`] guard reads it in place and releases.

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, Immutable, KnownLayout};

use crate::{Header, USER_WORDS, check_type, slot_ptr};

/// `reserve_slot_with` failed: no unread messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty;

/// The consuming endpoint: `reserve_slot_with` the oldest
/// unread slot, read in place, `release`.
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

    /// Reserve the oldest unread slot as a `&T`, applying an
    /// injected wait policy: retry until a message arrives or
    /// the policy gives up → [`Empty`].
    ///
    /// - Only one slot may be reserved at a time: the guard
    ///   holds the `&mut Consumer` borrow, so a second
    ///   reservation before the guard is dropped or released
    ///   does not compile.
    /// - Dropping the guard without [`ReadSlot::release`]
    ///   leaves the slot unread — the next reservation
    ///   returns it again.
    /// - `on_empty` is called after each failed attempt with
    ///   the attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Empty)`. Pass `|_| false`
    ///   for a single non-blocking probe.
    /// - The wait loop is the reservation itself (the guard
    ///   borrows the endpoint, so retrying could not return
    ///   it): `consumer_idx` is ours and loaded once; only
    ///   `producer_idx` is re-read per attempt.
    /// - See [`policy`](crate::policy) for shipped policies
    ///   and the composition model.
    pub fn reserve_slot_with<T>(
        &mut self,
        mut on_empty: impl FnMut(u32) -> bool,
    ) -> Result<ReadSlot<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        check_type::<T>(self.slot_size);
        // Ours alone: no peer moves it, load once.
        let c = self.header.consumer_idx.load(Ordering::Relaxed);
        let mut attempt = 0u32;
        loop {
            let p = self.header.producer_idx.load(Ordering::Acquire);
            if p != c {
                break;
            }
            if !on_empty(attempt) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        }
        // Raw pointer, not `&T`: release's store frees the slot
        // for producer writes while a reference field would
        // still be argument-protected. Deref mints short-lived
        // references instead.
        let msg = slot_ptr(self.slots, c, self.mask, self.slot_size) as *const T;
        Ok(ReadSlot {
            header: self.header,
            msg,
            next_idx: c.wrapping_add(1),
            _slot: PhantomData,
        })
    }

    /// Reserve the oldest unread slot, spinning
    /// until a message arrives —
    /// [`reserve_slot_with`](Consumer::reserve_slot_with)
    /// under the never-quitting
    /// [`policy::spin`](crate::policy::spin), so no `Result`.
    pub fn reserve_slot_spin<T>(&mut self) -> ReadSlot<'_, T>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        match self.reserve_slot_with(crate::policy::spin) {
            Ok(slot) => slot,
            Err(Empty) => unreachable!("policy::spin never gives up"),
        }
    }
}

/// A reserved read slot: `Deref` to read the message, then
/// [`release`](ReadSlot::release).
pub struct ReadSlot<'c, T> {
    /// The ring's control block (for the release store).
    header: &'c Header,
    /// The slot, viewed as the message type. Raw on purpose —
    /// see the comment in [`Consumer::reserve_slot_with`].
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
        // (FromBytes bound at reserve_slot_with), and the index
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
