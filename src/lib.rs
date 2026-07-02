//! Zero-copy no_std SPSC ring buffer over a caller-provided
//! memory region, per [notes/ring-buffer-design.md]:
//!
//! - One `#[repr(C, align(64))]` [`Header`] spanning three cache
//!   lines (immutable geometry, producer index, consumer index),
//!   followed by M slots of N bytes.
//! - Indices are free-running `AtomicU32`s, masked only at slot
//!   access; full is `p - c == M`, no sacrificial slot.
//! - Messages move in place: the producer writes through a
//!   [`WriteSlot`] `&mut T`, the consumer reads through a
//!   [`ReadSlot`] `&T` — zerocopy traits bound `T`, no
//!   serialization step. Each side reserves at most one slot at
//!   a time; the guard holds the endpoint borrow until commit /
//!   release (or drop).
//!
//! [notes/ring-buffer-design.md]: https://github.com/winksaville/zc-ring-x1/blob/main/notes/ring-buffer-design.md

#![cfg_attr(not(test), no_std)]

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

mod consumer;
mod producer;

pub use consumer::{Consumer, Empty, ReadSlot};
pub use producer::{Full, Producer, WriteSlot};

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
/// - line 0: geometry, written by [`Ring::init`] with `magic`
///   last (`Release`), read-only thereafter.
/// - line 1: `producer_idx`, written only by the producer.
/// - line 2: `consumer_idx`, written only by the consumer.
/// - Every field is atomic: the region may be mapped by a peer
///   at any time, so even "immutable" fields must be free of
///   data races. `AtomicU32` has the same layout as `u32`, so
///   this does not affect `layout_version`.
#[repr(C, align(64))]
pub struct Header {
    /// Layout marker ([`MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`), so a
    /// peer that observes it also observes the geometry.
    magic: AtomicU32,
    /// Layout version ([`LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Slot size N in bytes — a [`CACHE_LINE`] multiple.
    slot_size: AtomicU32,
    /// Slot count M — a power of two `<= 2^31`.
    capacity: AtomicU32,
    _pad0: [u8; 48],
    /// Free-running count of messages committed.
    producer_idx: AtomicU32,
    _pad1: [u8; 60],
    /// Free-running count of messages released.
    consumer_idx: AtomicU32,
    _pad2: [u8; 60],
}

const _: () = assert!(size_of::<Header>() == 3 * CACHE_LINE);

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

/// A validated view over a ring region; split into the two
/// endpoint handles with [`Ring::split`].
///
/// - Geometry is snapshotted out of the header at
///   init/attach: per-op paths never re-read fields a peer
///   could scribble on, so a hostile peer can degrade the ring
///   to garbage messages or spurious Full/Empty, never to UB.
pub struct Ring<'a> {
    /// The region's control block.
    header: &'a Header,
    /// Base of the slot array (offset `size_of::<Header>()`).
    slots: *mut u8,
    /// Snapshot of `header.slot_size`.
    slot_size: u32,
    /// Snapshot of `header.capacity`.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
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
        let len = region.len();
        // Taken exactly once: a second `as_mut_ptr()` would
        // retag the slice and invalidate `header` under Stacked
        // Borrows (Miri-verified).
        let base = region.as_mut_ptr();
        let header = header_ptr(base, len)?;
        if (len as u64) < region_size(slot_size, capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: alignment + room for the Header checked by
        // header_ptr; region is exclusively borrowed for 'a;
        // any byte pattern is a valid Header (all-atomic
        // fields, plain-byte padding).
        let header = unsafe { &*header };
        header
            .layout_version
            .store(LAYOUT_VERSION, Ordering::Relaxed);
        header.slot_size.store(slot_size, Ordering::Relaxed);
        header.capacity.store(capacity, Ordering::Relaxed);
        header.producer_idx.store(0, Ordering::Relaxed);
        header.consumer_idx.store(0, Ordering::Relaxed);
        // Published last: a peer that pre-mapped the region must
        // never observe MAGIC before the geometry it validates.
        // (Re-initializing a region a peer is already attached
        // to is outside the contract.)
        header.magic.store(MAGIC, Ordering::Release);
        // SAFETY: in bounds — region.len() >= header + slots.
        let slots = unsafe { base.add(size_of::<Header>()) };
        Ok(Ring {
            header,
            slots,
            slot_size,
            capacity,
            mask: capacity - 1,
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
        let header = header_ptr(region, len)?;
        // SAFETY: alignment + room for the Header checked by
        // header_ptr; caller guarantees the memory is live and
        // shared.
        let header = unsafe { &*header };
        // Acquire pairs with init's Release store of magic: a
        // valid magic means the geometry stores are visible.
        if header.magic.load(Ordering::Acquire) != MAGIC {
            return Err(Error::BadMagic);
        }
        if header.layout_version.load(Ordering::Relaxed) != LAYOUT_VERSION {
            return Err(Error::BadLayoutVersion);
        }
        // Snapshot geometry once; per-op paths never re-read it.
        let slot_size = header.slot_size.load(Ordering::Relaxed);
        let capacity = header.capacity.load(Ordering::Relaxed);
        validate_geometry(slot_size, capacity)?;
        if (len as u64) < region_size(slot_size, capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: in bounds — len >= header + slots.
        let slots = unsafe { region.add(size_of::<Header>()) };
        Ok(Ring {
            header,
            slots,
            slot_size,
            capacity,
            mask: capacity - 1,
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
            Producer::new(
                self.header,
                self.slots,
                self.slot_size,
                self.capacity,
                self.mask,
            ),
            Consumer::new(self.header, self.slots, self.slot_size, self.mask),
        )
    }
}

/// Validate a region base pointer and cast it to the `Header`
/// it must start with; shared by [`Ring::init`] / [`Ring::attach`].
///
/// - Checks alignment and room for the `Header` itself. The
///   full-geometry length check stays with the caller: init
///   knows the geometry from its parameters, attach only after
///   reading it from the returned header.
/// - Returns a raw pointer — no deref here. Each caller
///   discharges its own safety obligation (init: exclusive
///   borrow; attach: the caller's shared-mapping contract).
fn header_ptr(base: *mut u8, len: usize) -> Result<*const Header, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<Header>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const Header)
}

/// Bytes needed for a region with the given geometry.
///
/// - Computed in u64: the usize product can wrap on 32-bit
///   targets (e.g. N = 2^26, M = 2^6), which would validate a
///   tiny region and let safe code write out of bounds.
fn region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<Header>() as u64 + slot_size as u64 * capacity as u64
}

/// Shared geometry checks for [`Ring::init`] / [`Ring::attach`].
fn validate_geometry(slot_size: u32, capacity: u32) -> Result<(), Error> {
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE) {
        return Err(Error::BadSlotSize);
    }
    // The `> 1 << 31` arm is unreachable (u32 powers of two are
    // <= 2^31); it stays to document the M bound the occupancy
    // math relies on.
    if capacity == 0 || !capacity.is_power_of_two() || capacity > 1 << 31 {
        return Err(Error::BadCapacity);
    }
    Ok(())
}

/// Check `T` fits a slot; called once per `reserve_slot` (both
/// endpoints).
///
/// - Panics on a type-geometry mismatch — that is a programming
///   error, not a runtime condition.
fn check_type<T>(slot_size: u32) {
    assert!(size_of::<T>() <= slot_size as usize, "T larger than slot");
    assert!(
        core::mem::align_of::<T>() <= CACHE_LINE,
        "T alignment exceeds slot alignment"
    );
}

/// Pointer to the slot for free-running index `idx`.
///
/// - Returned range is `slot_size` bytes, cache-line aligned
///   (base is, and slot_size is a line multiple).
fn slot_ptr(slots: *mut u8, idx: u32, mask: u32, slot_size: u32) -> *mut u8 {
    let pos = (idx & mask) as usize;
    // SAFETY: pos < capacity, so the offset is inside the slot
    // array validated at init/attach.
    unsafe { slots.add(pos * slot_size as usize) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

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
        // 32-bit tripwire: N * M wraps a 32-bit usize (2^26 *
        // 2^6 = 2^32); the u64 region_size must still reject it.
        assert_eq!(
            Ring::init(&mut r.0, 1 << 26, 1 << 6).err().unwrap(),
            Error::TooSmall
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
        assert_eq!(ring.slot_size, 64);
        assert_eq!(ring.capacity, 4);
        assert_eq!(ring.mask, 3);
    }

    #[test]
    fn indices_survive_u32_wrap() {
        let mut r = Region::new();
        let ring = Ring::init(&mut r.0, 64, 4).unwrap();
        // Simulate a long-running ring two commits shy of the
        // u32 wrap (empty state: p == c).
        let start = u32::MAX - 1;
        ring.header.producer_idx.store(start, Ordering::Relaxed);
        ring.header.consumer_idx.store(start, Ordering::Relaxed);
        let (mut prod, mut cons) = ring.split();

        // Fill: producer_idx runs MAX-1, MAX, 0, 1 — masked
        // slot positions 2, 3, 0, 1.
        for i in 0..4u64 {
            let mut slot = prod.reserve_slot::<Msg>().unwrap();
            slot.seq = i;
            slot.val = i;
            slot.commit();
        }
        assert!(prod.reserve_slot::<Msg>().is_err());

        // Drain in order across the wrap.
        for i in 0..4u64 {
            let msg = cons.reserve_slot::<Msg>().unwrap();
            assert_eq!(msg.seq, i);
            msg.release();
        }
        assert!(cons.reserve_slot::<Msg>().is_err());
    }

    #[test]
    fn roundtrip_full_empty() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        // Empty at start.
        assert!(cons.reserve_slot::<Msg>().is_err());

        // Fill all 4 slots, then Full.
        for i in 0..4u64 {
            let mut slot = prod.reserve_slot::<Msg>().unwrap();
            slot.seq = i;
            slot.val = i * 10;
            slot.commit();
        }
        assert!(prod.reserve_slot::<Msg>().is_err());

        // Drain in order; wrap: slot 0 is reused by seq 4.
        for i in 0..4u64 {
            let msg = cons.reserve_slot::<Msg>().unwrap();
            assert_eq!(
                *msg,
                Msg {
                    seq: i,
                    val: i * 10
                }
            );
            msg.release();
        }
        assert!(cons.reserve_slot::<Msg>().is_err());

        // One more write lands in the masked-around slot 0.
        let mut slot = prod.reserve_slot::<Msg>().unwrap();
        slot.seq = 4;
        slot.commit();
        let msg = cons.reserve_slot::<Msg>().unwrap();
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
        let mut slot = prod.reserve_slot::<Msg>().unwrap();
        slot.seq = 99;
        drop(slot);
        assert!(cons.reserve_slot::<Msg>().is_err());

        // Commit one, reserve_slot then drop: still unread, the
        // next reserve_slot returns it again.
        let mut slot = prod.reserve_slot::<Msg>().unwrap();
        slot.seq = 1;
        slot.commit();
        let msg = cons.reserve_slot::<Msg>().unwrap();
        assert_eq!(msg.seq, 1);
        drop(msg);
        let msg = cons.reserve_slot::<Msg>().unwrap();
        assert_eq!(msg.seq, 1);
        msg.release();
    }

    #[test]
    fn threaded_spsc() {
        // Reduced under Miri: interpreted spin loops are slow,
        // and its scheduler explores interleavings at any count.
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    loop {
                        match prod.reserve_slot::<Msg>() {
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
                        match cons.reserve_slot::<Msg>() {
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
