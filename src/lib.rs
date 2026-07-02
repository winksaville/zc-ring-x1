//! Zero-copy no_std SPSC ring buffer over a caller-provided
//! memory region, per [notes/ring-buffer-design.md]:
//!
//! - One `#[repr(C, align(64))]` [`Header`] spanning three cache
//!   lines (immutable geometry, producer index, consumer index),
//!   followed by M slots of N bytes.
//! - Indices are free-running `AtomicU32`s, masked only at slot
//!   access; full is `p - c == M`, no sacrificial slot.
//! - Messages move in place: the producer writes through a
//!   [`Reserved`] `&mut T`, the consumer reads through a
//!   [`Peeked`] `&T` — zerocopy traits bound `T`, no
//!   serialization step.
//!
//! [notes/ring-buffer-design.md]: https://github.com/winksaville/zc-ring-x1/blob/main/notes/ring-buffer-design.md

#![cfg_attr(not(test), no_std)]

use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Cache-line size the layout is built around.
///
/// - Slot size must be a multiple of this; slots and the region
///   itself must be aligned to it.
pub const CACHE_LINE: usize = 64;

/// Layout marker written by [`Ring::init`]; rejects foreign
/// regions on attach.
const MAGIC: u32 = 0x5A43_5231; // "ZCR1"

/// Bumped on any change to the region layout.
const LAYOUT_VERSION: u32 = 1;

/// Control block at offset 0 of the region — one struct spanning
/// three cache lines so each concurrently-written index owns its
/// own line.
///
/// - line 0: geometry, immutable after [`Ring::init`].
/// - line 1: `producer_idx`, written only by the producer.
/// - line 2: `consumer_idx`, written only by the consumer.
#[repr(C, align(64))]
pub struct Header {
    /// Layout marker ([`MAGIC`]).
    magic: u32,
    /// Layout version ([`LAYOUT_VERSION`]).
    layout_version: u32,
    /// Slot size N in bytes — a [`CACHE_LINE`] multiple.
    slot_size: u32,
    /// Slot count M — a power of two `<= 2^31`.
    capacity: u32,
    _pad0: [u8; 48],
    /// Free-running count of messages committed.
    producer_idx: AtomicU32,
    _pad1: [u8; 60],
    /// Free-running count of messages released.
    consumer_idx: AtomicU32,
    _pad2: [u8; 60],
}

const _: () = assert!(size_of::<Header>() == 3 * CACHE_LINE);

impl Header {
    /// Slot-position mask (`capacity - 1`).
    fn mask(&self) -> u32 {
        self.capacity - 1
    }
}

/// Errors from [`Ring::init`] / [`Ring::attach`] region
/// validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Region is not [`CACHE_LINE`]-aligned.
    Misaligned,
    /// Region is smaller than header + slots.
    TooSmall,
    /// Slot size is zero or not a [`CACHE_LINE`] multiple.
    BadSlotSize,
    /// Capacity is zero, not a power of two, or `> 2^31`.
    BadCapacity,
    /// Attach: magic mismatch — not a ring region.
    BadMagic,
    /// Attach: layout version mismatch.
    BadLayoutVersion,
}

/// `reserve` failed: every slot holds an uncommitted-or-unread
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

/// `peek` failed: no unread messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty;

/// A validated view over a ring region; split into the two
/// endpoint handles with [`Ring::split`].
pub struct Ring<'a> {
    /// The region's control block.
    header: &'a Header,
    /// Base of the slot array (offset `size_of::<Header>()`).
    slots: *mut u8,
    _region: PhantomData<&'a [u8]>,
}

impl<'a> Ring<'a> {
    /// Initialize a fresh region and return the ring over it.
    ///
    /// - `slot_size` — N bytes per slot, a [`CACHE_LINE`]
    ///   multiple.
    /// - `capacity` — M slots, a power of two `<= 2^31`.
    /// - The region must be [`CACHE_LINE`]-aligned and at least
    ///   `size_of::<Header>() + M * N` bytes.
    pub fn init(region: &'a mut [u8], slot_size: u32, capacity: u32) -> Result<Self, Error> {
        validate_geometry(slot_size, capacity)?;
        if !(region.as_ptr() as usize).is_multiple_of(CACHE_LINE) {
            return Err(Error::Misaligned);
        }
        if region.len() < region_size(slot_size, capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: region is exclusively borrowed for 'a, long
        // enough (checked above), CACHE_LINE-aligned (checked
        // above, and CACHE_LINE == align_of::<Header>()), and
        // any byte pattern is a valid Header.
        let header = unsafe { &mut *(region.as_mut_ptr() as *mut Header) };
        header.magic = MAGIC;
        header.layout_version = LAYOUT_VERSION;
        header.slot_size = slot_size;
        header.capacity = capacity;
        header.producer_idx = AtomicU32::new(0);
        header.consumer_idx = AtomicU32::new(0);
        // SAFETY: in bounds — region.len() >= header + slots.
        let slots = unsafe { region.as_mut_ptr().add(size_of::<Header>()) };
        Ok(Ring {
            header,
            slots,
            _region: PhantomData,
        })
    }

    /// Attach to a region another process (or an earlier call)
    /// already initialized, validating its header.
    ///
    /// # Safety
    ///
    /// - `region` points to `len` bytes of memory that outlive
    ///   `'a`, genuinely shared and writable (e.g. a
    ///   `MAP_SHARED` mapping).
    /// - No other producer attaches if this side will produce;
    ///   likewise for the consumer side (SPSC contract).
    pub unsafe fn attach(region: *mut u8, len: usize) -> Result<Self, Error> {
        if !(region as usize).is_multiple_of(CACHE_LINE) {
            return Err(Error::Misaligned);
        }
        if len < size_of::<Header>() {
            return Err(Error::TooSmall);
        }
        // SAFETY: aligned + long enough (checked above); caller
        // guarantees the memory is live and shared.
        let header = unsafe { &*(region as *const Header) };
        if header.magic != MAGIC {
            return Err(Error::BadMagic);
        }
        if header.layout_version != LAYOUT_VERSION {
            return Err(Error::BadLayoutVersion);
        }
        validate_geometry(header.slot_size, header.capacity)?;
        if len < region_size(header.slot_size, header.capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: in bounds — len >= header + slots.
        let slots = unsafe { region.add(size_of::<Header>()) };
        Ok(Ring {
            header,
            slots,
            _region: PhantomData,
        })
    }

    /// Split into the producer and consumer endpoint handles.
    ///
    /// - Consuming `self` makes each handle exist at most once
    ///   per ring per process; cross-process, one producing and
    ///   one consuming process is the SPSC contract.
    pub fn split(self) -> (Producer<'a>, Consumer<'a>) {
        (
            Producer {
                header: self.header,
                slots: self.slots,
                _region: PhantomData,
            },
            Consumer {
                header: self.header,
                slots: self.slots,
                _region: PhantomData,
            },
        )
    }
}

/// Bytes needed for a region with the given geometry.
fn region_size(slot_size: u32, capacity: u32) -> usize {
    size_of::<Header>() + slot_size as usize * capacity as usize
}

/// Shared geometry checks for [`Ring::init`] / [`Ring::attach`].
fn validate_geometry(slot_size: u32, capacity: u32) -> Result<(), Error> {
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE) {
        return Err(Error::BadSlotSize);
    }
    if capacity == 0 || !capacity.is_power_of_two() || capacity > 1 << 31 {
        return Err(Error::BadCapacity);
    }
    Ok(())
}

/// Check `T` fits a slot; called once per reserve/peek.
///
/// - Panics on a type-geometry mismatch — that is a programming
///   error, not a runtime condition.
fn check_type<T>(header: &Header) {
    assert!(
        size_of::<T>() <= header.slot_size as usize,
        "T larger than slot"
    );
    assert!(
        core::mem::align_of::<T>() <= CACHE_LINE,
        "T alignment exceeds slot alignment"
    );
}

/// Pointer to the slot for free-running index `idx`.
///
/// - Returned range is `header.slot_size` bytes, cache-line
///   aligned (base is, and slot_size is a line multiple).
fn slot_ptr(header: &Header, slots: *mut u8, idx: u32) -> *mut u8 {
    let pos = (idx & header.mask()) as usize;
    // SAFETY: pos < capacity, so the offset is inside the slot
    // array validated at init/attach.
    unsafe { slots.add(pos * header.slot_size as usize) }
}

/// The producing endpoint: `reserve` a slot, write in place,
/// `commit`.
pub struct Producer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the slot array.
    slots: *mut u8,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the producer role; the shared state it
// touches (indices) is atomic, and slot writes are handed off
// with Release/Acquire ordering.
unsafe impl Send for Producer<'_> {}

impl<'a> Producer<'a> {
    /// Reserve the next free slot as a `&mut T`, or [`Full`].
    ///
    /// - Dropping the guard without [`Reserved::commit`]
    ///   abandons the reservation (nothing published).
    pub fn reserve<T>(&mut self) -> Result<Reserved<'_, T>, Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.header);
        let p = self.header.producer_idx.load(Ordering::Relaxed);
        let c = self.header.consumer_idx.load(Ordering::Acquire);
        if p.wrapping_sub(c) == self.header.capacity {
            return Err(Full);
        }
        let bytes = slot_ptr(self.header, self.slots, p);
        // SAFETY: index protocol gives the producer exclusive
        // access to this slot until commit; range validated at
        // init/attach.
        let bytes =
            unsafe { core::slice::from_raw_parts_mut(bytes, self.header.slot_size as usize) };
        #[allow(clippy::unwrap_used)]
        // OK: check_type guarantees size, slot base guarantees
        // alignment, so the prefix cast cannot fail.
        let (msg, _) = T::mut_from_prefix(bytes).unwrap();
        Ok(Reserved {
            header: self.header,
            msg,
            next_idx: p.wrapping_add(1),
        })
    }
}

/// A reserved slot: `DerefMut` to write the message, then
/// [`commit`](Reserved::commit).
pub struct Reserved<'p, T> {
    /// The ring's control block (for the commit store).
    header: &'p Header,
    /// The slot, viewed as the message type.
    msg: &'p mut T,
    /// Value `producer_idx` takes on commit.
    next_idx: u32,
}

impl<T> Deref for Reserved<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        self.msg
    }
}

impl<T> DerefMut for Reserved<'_, T> {
    /// Write access to the in-slot message.
    fn deref_mut(&mut self) -> &mut T {
        self.msg
    }
}

impl<T> Reserved<'_, T> {
    /// Publish the slot to the consumer
    /// (`producer_idx + 1`, `Release`).
    pub fn commit(self) {
        self.header
            .producer_idx
            .store(self.next_idx, Ordering::Release);
    }
}

/// The consuming endpoint: `peek` the oldest unread slot, read
/// in place, `release`.
pub struct Consumer<'a> {
    /// The ring's control block.
    header: &'a Header,
    /// Base of the slot array.
    slots: *mut u8,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the consumer role; see the Producer
// Send rationale.
unsafe impl Send for Consumer<'_> {}

impl<'a> Consumer<'a> {
    /// View the oldest unread slot as a `&T`, or [`Empty`].
    ///
    /// - Dropping the guard without [`Peeked::release`] leaves
    ///   the slot unread — the next peek returns it again.
    pub fn peek<T>(&mut self) -> Result<Peeked<'_, T>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        check_type::<T>(self.header);
        let c = self.header.consumer_idx.load(Ordering::Relaxed);
        let p = self.header.producer_idx.load(Ordering::Acquire);
        if p == c {
            return Err(Empty);
        }
        let bytes = slot_ptr(self.header, self.slots, c);
        // SAFETY: index protocol gives the consumer exclusive
        // access to this slot until release; range validated at
        // init/attach.
        let bytes = unsafe {
            core::slice::from_raw_parts(bytes as *const u8, self.header.slot_size as usize)
        };
        #[allow(clippy::unwrap_used)]
        // OK: check_type guarantees size, slot base guarantees
        // alignment, so the prefix cast cannot fail.
        let (msg, _) = T::ref_from_prefix(bytes).unwrap();
        Ok(Peeked {
            header: self.header,
            msg,
            next_idx: c.wrapping_add(1),
        })
    }
}

/// A peeked slot: `Deref` to read the message, then
/// [`release`](Peeked::release).
pub struct Peeked<'c, T> {
    /// The ring's control block (for the release store).
    header: &'c Header,
    /// The slot, viewed as the message type.
    msg: &'c T,
    /// Value `consumer_idx` takes on release.
    next_idx: u32,
}

impl<T> Deref for Peeked<'_, T> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        self.msg
    }
}

impl<T> Peeked<'_, T> {
    /// Free the slot for reuse (`consumer_idx + 1`, `Release`).
    pub fn release(self) {
        self.header
            .consumer_idx
            .store(self.next_idx, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test message; two words so a torn write would be visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// Cache-line-aligned backing store: header + 4 × 64B slots.
    #[repr(C, align(64))]
    struct Region([u8; 3 * CACHE_LINE + 4 * CACHE_LINE]);

    impl Region {
        fn new() -> Self {
            Region([0; 3 * CACHE_LINE + 4 * CACHE_LINE])
        }
    }

    #[test]
    fn init_rejects_bad_geometry() {
        let mut r = Region::new();
        assert_eq!(
            Ring::init(&mut r.0, 63, 4).err().unwrap(),
            Error::BadSlotSize
        );
        assert_eq!(
            Ring::init(&mut r.0, 0, 4).err().unwrap(),
            Error::BadSlotSize
        );
        assert_eq!(
            Ring::init(&mut r.0, 64, 3).err().unwrap(),
            Error::BadCapacity
        );
        assert_eq!(
            Ring::init(&mut r.0, 64, 0).err().unwrap(),
            Error::BadCapacity
        );
        assert_eq!(Ring::init(&mut r.0, 64, 8).err().unwrap(), Error::TooSmall);
        assert_eq!(
            Ring::init(&mut r.0[1..], 64, 4).err().unwrap(),
            Error::Misaligned
        );
    }

    #[test]
    fn attach_validates_header() {
        let mut r = Region::new();
        // Not initialized yet: magic is zero.
        let err = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
        Ring::init(&mut r.0, 64, 4).unwrap();
        let ring = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }.unwrap();
        assert_eq!(ring.header.slot_size, 64);
        assert_eq!(ring.header.capacity, 4);
    }

    #[test]
    fn roundtrip_full_empty() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        // Empty at start.
        assert!(cons.peek::<Msg>().is_err());

        // Fill all 4 slots, then Full.
        for i in 0..4u64 {
            let mut slot = prod.reserve::<Msg>().unwrap();
            slot.seq = i;
            slot.val = i * 10;
            slot.commit();
        }
        assert!(prod.reserve::<Msg>().is_err());

        // Drain in order; wrap: slot 0 is reused by seq 4.
        for i in 0..4u64 {
            let msg = cons.peek::<Msg>().unwrap();
            assert_eq!(
                *msg,
                Msg {
                    seq: i,
                    val: i * 10
                }
            );
            msg.release();
        }
        assert!(cons.peek::<Msg>().is_err());

        // One more write lands in the masked-around slot 0.
        let mut slot = prod.reserve::<Msg>().unwrap();
        slot.seq = 4;
        slot.commit();
        let msg = cons.peek::<Msg>().unwrap();
        assert_eq!(msg.seq, 4);
        msg.release();
    }

    #[test]
    // Dropping the guards is the behavior under test; they have
    // no Drop impl by design (abandon = do nothing).
    #[allow(clippy::drop_non_drop)]
    fn abandoned_guards_publish_nothing() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        // Reserve then drop: nothing published.
        let mut slot = prod.reserve::<Msg>().unwrap();
        slot.seq = 99;
        drop(slot);
        assert!(cons.peek::<Msg>().is_err());

        // Commit one, peek then drop: still unread, re-peekable.
        let mut slot = prod.reserve::<Msg>().unwrap();
        slot.seq = 1;
        slot.commit();
        let msg = cons.peek::<Msg>().unwrap();
        assert_eq!(msg.seq, 1);
        drop(msg);
        let msg = cons.peek::<Msg>().unwrap();
        assert_eq!(msg.seq, 1);
        msg.release();
    }

    #[test]
    fn threaded_spsc() {
        const COUNT: u64 = 100_000;
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    loop {
                        match prod.reserve::<Msg>() {
                            Ok(mut slot) => {
                                slot.seq = i;
                                slot.val = i * 3;
                                slot.commit();
                                break;
                            }
                            Err(Full) => std::hint::spin_loop(),
                        }
                    }
                }
            });
            s.spawn(move || {
                for i in 0..COUNT {
                    loop {
                        match cons.peek::<Msg>() {
                            Ok(msg) => {
                                assert_eq!(msg.seq, i);
                                assert_eq!(msg.val, i * 3);
                                msg.release();
                                break;
                            }
                            Err(Empty) => std::hint::spin_loop(),
                        }
                    }
                }
            });
        });
    }
}
