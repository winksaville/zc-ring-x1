//! SPSC ring v0: the load/store-only single-producer
//! single-consumer protocol, per
//! [notes/ring-buffer-design.md]:
//!
//! - One `#[repr(C)]` [`Header`] spanning four cache lines
//!   (immutable geometry, producer index, consumer index, app
//!   user words), followed by M slots of N bytes.
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

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error, USER_WORDS};

mod consumer;
mod producer;

pub use consumer::{Consumer, ReadSlot};
pub use producer::{Producer, WriteSlot};

/// Layout marker written by [`Ring::init`]; rejects foreign
/// regions on attach.
const MAGIC: u32 = 0x5A43_5231; // "ZCR1"

/// Bumped on any change to the region layout.
const LAYOUT_VERSION: u32 = 2;

/// Control block at offset 0 of the region — one struct spanning
/// four cache lines so each concurrently-written index owns its
/// own line.
///
/// - line 0: geometry, written by [`Ring::init`] with `magic`
///   last (`Release`), read-only thereafter. Cold: per-op
///   paths use the handles' snapshots.
/// - line 1: `producer_idx`, written only by the producer.
/// - line 2: `consumer_idx`, written only by the consumer.
/// - line 3: `user`, app-owned scratch — last, so an app
///   overrun walks into its own slot data, not an index line.
/// - Every field is atomic: the region may be mapped by a peer
///   at any time, so even "immutable" fields must be free of
///   data races. `AtomicU32` has the same layout as `u32`, so
///   this does not affect `layout_version`.
#[repr(C)]
pub struct Header {
    /// Layout marker ([`MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`), so a
    /// peer that observes it also observes the geometry.
    magic: AtomicU32,
    /// Layout version ([`LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Slot size N in bytes — a [`CACHE_LINE_SIZE`] multiple.
    slot_size: AtomicU32,
    /// Slot count M — a power of two `<= 2^31`.
    capacity: AtomicU32,
    /// [`CACHE_LINE_SIZE`] this region was built with. The line size
    /// is a layout parameter (padding, offsets, slot granule),
    /// so builds must agree; attach validates it like the rest
    /// of the geometry.
    cache_line_size: AtomicU32,
    /// Free-running count of messages committed.
    producer_idx: CacheAligned<AtomicU32>,
    /// Free-running count of messages released.
    consumer_idx: CacheAligned<AtomicU32>,
    /// App-owned scratch line ([`USER_WORDS`] words): zeroed by
    /// init, then never read, written, or interpreted by the
    /// crate. See the design doc's "Blocking and user words".
    user: CacheAligned<[AtomicU32; USER_WORDS]>,
}

const _: () = assert!(size_of::<Header>() == 4 * CACHE_LINE_SIZE);

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
    /// - `slot_size` — N bytes per slot, a [`CACHE_LINE_SIZE`]
    ///   multiple.
    /// - `capacity` — M slots, a power of two `<= 2^31`.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at least
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
        header
            .cache_line_size
            .store(CACHE_LINE_SIZE as u32, Ordering::Relaxed);
        header.producer_idx.store(0, Ordering::Relaxed);
        header.consumer_idx.store(0, Ordering::Relaxed);
        for word in header.user.iter() {
            word.store(0, Ordering::Relaxed);
        }
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
        if header.cache_line_size.load(Ordering::Relaxed) != CACHE_LINE_SIZE as u32 {
            return Err(Error::BadCacheLine);
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
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
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
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE_SIZE) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Empty, Full};
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message; two words so a torn write would be visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// Test region size: header + (4 slots × 1 line each).
    const REGION_BYTES: usize = size_of::<Header>() + (4 * CACHE_LINE_SIZE);

    /// Cache-line-aligned backing store for the tests' rings.
    #[repr(C, align(64))]
    struct Region([u8; REGION_BYTES]);

    impl Region {
        fn new() -> Self {
            Region([0; REGION_BYTES])
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
        // A region built with a different CACHE_LINE_SIZE (simulated
        // by editing the recorded value) is rejected.
        ring.header.cache_line_size.store(128, Ordering::Relaxed);
        let err = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadCacheLine);
    }

    #[test]
    fn user_words_zeroed_and_shared() {
        let mut r = Region::new();
        // Dirty the region so init's zeroing is observable.
        r.0.fill(0xAA);
        let ring = Ring::init(&mut r.0, 64, 4).unwrap();
        let (prod, mut cons) = ring.split();
        assert!(prod.user().iter().all(|w| w.load(Ordering::Relaxed) == 0));
        // Both endpoints see the same line; a producer-side
        // store is visible through the consumer's accessor.
        prod.user()[0].store(7, Ordering::Release);
        assert_eq!(cons.user()[0].load(Ordering::Acquire), 7);
        // The user line must not alias ring state: scribbling
        // all of it leaves the ring empty and functional.
        for w in cons.user().iter() {
            w.store(u32::MAX, Ordering::Relaxed);
        }
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
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
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i;
            slot.val = i;
            slot.commit();
        }
        assert!(prod.reserve_slot_with::<Msg>(|_| false).is_err());

        // Drain in order across the wrap.
        for i in 0..4u64 {
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, i);
            msg.release();
        }
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
    }

    #[test]
    fn roundtrip_full_empty() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        // Empty at start.
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());

        // Fill all 4 slots, then Full.
        for i in 0..4u64 {
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i;
            slot.val = i * 10;
            slot.commit();
        }
        assert!(prod.reserve_slot_with::<Msg>(|_| false).is_err());

        // Drain in order; wrap: slot 0 is reused by seq 4.
        for i in 0..4u64 {
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(
                *msg,
                Msg {
                    seq: i,
                    val: i * 10
                }
            );
            msg.release();
        }
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());

        // One more write lands in the masked-around slot 0.
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 4;
        slot.commit();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
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
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 99;
        drop(slot);
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());

        // Commit one, reserve then drop: still unread, the
        // next reservation returns it again.
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 1;
        slot.commit();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 1);
        drop(msg);
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
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
                        match prod.reserve_slot_with::<Msg>(|_| false) {
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
                        match cons.reserve_slot_with::<Msg>(|_| false) {
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

    #[test]
    fn reserve_slot_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        // Empty ring: the consumer's policy sees 0-based
        // attempts and its `false` becomes Err(Empty).
        let mut seen = Vec::new();
        let err = cons
            .reserve_slot_with::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err()
            .unwrap();
        assert_eq!(err, Empty);
        assert_eq!(seen, [0, 1, 2]);

        // Fill the ring: the producer's policy gives up with
        // Err(Full).
        for i in 0..4 {
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i;
            slot.commit();
        }
        let err = prod
            .reserve_slot_with::<Msg>(|attempt| attempt < 2)
            .err()
            .unwrap();
        assert_eq!(err, Full);

        // With room / messages available the policy is never
        // consulted.
        let msg = cons
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with a message available"))
            .unwrap();
        assert_eq!(msg.seq, 0);
        msg.release();
        let mut slot = prod
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with room available"))
            .unwrap();
        slot.seq = 4;
        slot.commit();
    }

    #[test]
    fn threaded_spsc_spin_variants() {
        // The threaded_spsc flow through the shipped
        // policy::spin on both ends — the shipped-policy model
        // in use.
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    let mut slot = prod.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                    slot.seq = i;
                    slot.val = i * 3;
                    slot.commit();
                }
            });
            s.spawn(move || {
                for i in 0..COUNT {
                    let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                    assert_eq!(msg.seq, i);
                    assert_eq!(msg.val, i * 3);
                    msg.release();
                }
            });
        });
    }
}
