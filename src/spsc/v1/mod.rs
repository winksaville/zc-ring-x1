//! SPSC ring v1: the seam-word protocol, per the design doc's
//! "SPSC v1: seam-word ring" section.
//!
//! - Same region shape as v0 (a four-line [`Header`], then
//!   slots) plus a per-slot sequence array between them, as
//!   the MPSC ring has: `seq[pos]` publishes each slot on its
//!   own, so neither side ever reads the other side's index
//!   line. v0 moved ~10 cache lines per cross-core round trip
//!   against the MPSC ring's ~6.7 because each side polled the
//!   other's index; here the producer polls the slot's seq and
//!   the consumer the same word, and the index lines are each
//!   side's private resume state.
//! - MPSC's protocol minus the claim CAS: one producer owns
//!   `producer_idx`, so it claims a slot by loading the seq,
//!   never by compare-exchange. Load/store only, so the v0
//!   atomic floor holds (thumbv6m keeps working).
//! - Own magic and layout version: a region is one kind or
//!   the other, and cross-attaching fails toward
//!   [`Error::BadMagic`].
//! - `capacity` may be any power of two down to 1: the state
//!   lives in the seq, not in an index distance, so no
//!   sacrificial slot. Committed is `pos + M + 1`, not
//!   Vyukov's `pos + 1`, which at `M = 1` would equal the
//!   released value `pos + M` and let the producer overwrite
//!   an unread slot.

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error, USER_WORDS};

mod consumer;
mod producer;

pub use consumer::{Consumer, ReadSlot};
pub use producer::{Producer, WriteSlot};

/// Layout marker written by [`Ring::init`]; distinct from the
/// v0 and MPSC magics so cross-kind attach fails toward
/// [`Error::BadMagic`].
const MAGIC: u32 = 0x5A43_5232; // "ZCR2"

/// Bumped on any change to the v1 region layout; independent
/// of the v0 layout version.
const LAYOUT_VERSION: u32 = 1;

/// Capacity bound: `2^30`, as the MPSC ring, so the seq
/// arithmetic keeps the same headroom and a v1 region can be
/// read with the MPSC ring's numbers in mind.
const MAX_CAPACITY: u32 = 1 << 30;

/// Control block at offset 0 of a v1 region — the v0
/// [`Header`](crate::Header)'s four-line shape, its own type.
///
/// - line 0: geometry, written by [`Ring::init`] with `magic`
///   last (`Release`), read-only thereafter.
/// - line 1: `producer_idx`, the producer's resume state:
///   written and read only by the producer, never by the
///   consumer's hot path.
/// - line 2: `consumer_idx`, the consumer's resume state,
///   likewise private to it.
/// - line 3: `user`, app-owned scratch — same contract as
///   v0's user line.
/// - Every field is atomic: the region may be mapped by a
///   peer at any time.
#[repr(C)]
pub struct Header {
    /// Layout marker ([`MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`).
    magic: AtomicU32,
    /// Layout version ([`LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Slot size N in bytes — a [`CACHE_LINE_SIZE`] multiple.
    slot_size: AtomicU32,
    /// Slot count M — a power of two `<= 2^30`.
    capacity: AtomicU32,
    /// [`CACHE_LINE_SIZE`] this region was built with.
    cache_line_size: AtomicU32,
    /// Free-running count of messages committed; producer
    /// resume state, not read by the consumer.
    producer_idx: CacheAligned<AtomicU32>,
    /// Free-running count of messages released; consumer
    /// resume state, not read by the producer.
    consumer_idx: CacheAligned<AtomicU32>,
    /// App-owned scratch line ([`USER_WORDS`] words): zeroed by
    /// init, then never touched by the crate.
    user: CacheAligned<[AtomicU32; USER_WORDS]>,
}

const _: () = assert!(size_of::<Header>() == 4 * CACHE_LINE_SIZE);

/// A validated view over a v1 ring region; split into the two
/// endpoint handles with [`Ring::split`].
///
/// - Geometry is snapshotted out of the header at init/attach,
///   as v0 does: per-op paths never re-read fields a peer
///   could scribble on.
/// - Region layout: [`Header`], the seq array (`M ×
///   AtomicU32`, padded up to a cache line), then M slots of N
///   bytes.
pub struct Ring<'a> {
    /// The region's control block.
    header: &'a Header,
    /// Base of the per-slot sequence array.
    seqs: *const AtomicU32,
    /// Base of the slot array.
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
    /// - `capacity` — M slots, a power of two `<= 2^30`, 1
    ///   included.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at
    ///   least [`region_size`] bytes.
    pub fn init(region: &'a mut [u8], slot_size: u32, capacity: u32) -> Result<Self, Error> {
        validate_geometry(slot_size, capacity)?;
        let len = region.len();
        // Taken exactly once, as v0 does: a second
        // `as_mut_ptr()` would retag the slice and invalidate
        // `header` under Stacked Borrows.
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
        // SAFETY: in bounds — region.len() >= region_size.
        let seqs = unsafe { base.add(size_of::<Header>()) } as *const AtomicU32;
        // `seq[i] = i`: every slot claimable for lap 0.
        for i in 0..capacity {
            // SAFETY: i < capacity, inside the seq array.
            unsafe { &*seqs.add(i as usize) }.store(i, Ordering::Relaxed);
        }
        // Published last: a peer that pre-mapped the region must
        // never observe MAGIC before the geometry and seqs it
        // relies on.
        header.magic.store(MAGIC, Ordering::Release);
        // SAFETY: in bounds — region.len() >= region_size.
        let slots = unsafe { base.add(slots_offset(capacity)) };
        Ok(Ring {
            header,
            seqs,
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
        // Acquire pairs with init's Release store of magic.
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
        // SAFETY: in bounds — len >= region_size.
        let seqs = unsafe { region.add(size_of::<Header>()) } as *const AtomicU32;
        // SAFETY: as above.
        let slots = unsafe { region.add(slots_offset(capacity)) };
        Ok(Ring {
            header,
            seqs,
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
                self.seqs,
                self.slots,
                self.slot_size,
                self.capacity,
                self.mask,
            ),
            Consumer::new(
                self.header,
                self.seqs,
                self.slots,
                self.slot_size,
                self.capacity,
                self.mask,
            ),
        )
    }
}

/// Validate a region base pointer and cast it to the `Header`
/// it must start with; shared by [`Ring::init`] /
/// [`Ring::attach`].
///
/// - Same split of duties as v0's: alignment and header room
///   here, full-geometry length with the caller.
fn header_ptr(base: *mut u8, len: usize) -> Result<*const Header, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<Header>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const Header)
}

/// Bytes the seq array occupies, padded up to a cache line so
/// the slot array behind it stays line-aligned.
///
/// - u64: `capacity * 4` reaches `2^32` at the `2^30` cap,
///   which would wrap a 32-bit usize.
fn seq_bytes(capacity: u32) -> u64 {
    (capacity as u64 * size_of::<AtomicU32>() as u64).next_multiple_of(CACHE_LINE_SIZE as u64)
}

/// Byte offset of the slot array: header, then the padded seq
/// array.
///
/// - usize return: callers offset a pointer with it, and only
///   after the u64 region-size check has proven the region
///   fits in memory.
fn slots_offset(capacity: u32) -> usize {
    size_of::<Header>() + seq_bytes(capacity) as usize
}

/// Bytes needed for a v1 region with the given geometry.
///
/// - Public: a pool that hands out ring segments sizes its
///   buffers by it.
/// - Computed in u64 for the same 32-bit wrap reason as v0's
///   `region_size`.
pub fn region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<Header>() as u64 + seq_bytes(capacity) + slot_size as u64 * capacity as u64
}

/// Geometry checks for [`Ring::init`] / [`Ring::attach`].
///
/// - Slot size as v0; capacity a power of two up to
///   [`MAX_CAPACITY`], 1 allowed.
fn validate_geometry(slot_size: u32, capacity: u32) -> Result<(), Error> {
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::BadSlotSize);
    }
    if capacity == 0 || !capacity.is_power_of_two() || capacity > MAX_CAPACITY {
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

    /// Test region: header + seq line (up to 16 × 4 B) + 16
    /// slots × 1 line — enough for every capacity the tests use.
    const REGION_BYTES: usize = size_of::<Header>() + CACHE_LINE_SIZE + (16 * CACHE_LINE_SIZE);

    /// Cache-line-aligned backing store for the tests' rings.
    #[repr(C, align(64))]
    struct Region([u8; REGION_BYTES]);

    impl Region {
        fn new() -> Self {
            Region([0; REGION_BYTES])
        }
    }

    /// Fill `count` messages through `prod`, `seq = i`, `val = i
    /// * 10`, then assert Full.
    fn fill(prod: &mut Producer<'_>, count: u64) {
        for i in 0..count {
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i;
            slot.val = i * 10;
            slot.commit();
        }
        assert!(prod.reserve_slot_with::<Msg>(|_| false).is_err());
    }

    /// Drain `count` messages through `cons` in order, then
    /// assert Empty.
    fn drain(cons: &mut Consumer<'_>, count: u64) {
        for i in 0..count {
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
        assert_eq!(
            Ring::init(&mut r.0, 64, 1 << 31).err().unwrap(),
            Error::BadCapacity
        );
        assert_eq!(Ring::init(&mut r.0, 64, 32).err().unwrap(), Error::TooSmall);
        assert_eq!(
            Ring::init(&mut r.0[1..], 64, 4).err().unwrap(),
            Error::Misaligned
        );
        // 32-bit tripwire: N * M wraps a 32-bit usize; the u64
        // region_size must still reject it.
        assert_eq!(
            Ring::init(&mut r.0, 1 << 26, 1 << 6).err().unwrap(),
            Error::TooSmall
        );
    }

    #[test]
    fn region_size_pads_seqs_to_a_line() {
        // 1 to 16 seqs fit one line; 17 needs two.
        assert_eq!(region_size(64, 1), size_of::<Header>() as u64 + 64 + 64);
        assert_eq!(
            region_size(64, 16),
            size_of::<Header>() as u64 + 64 + 16 * 64
        );
        assert_eq!(
            region_size(64, 32),
            size_of::<Header>() as u64 + 128 + 32 * 64
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
        // A region built with a different CACHE_LINE_SIZE
        // (simulated by editing the recorded value) is rejected.
        ring.header.cache_line_size.store(128, Ordering::Relaxed);
        let err = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadCacheLine);
    }

    #[test]
    fn cross_kind_attach_fails_on_magic() {
        let mut r = Region::new();
        crate::spsc::v0::Ring::init(&mut r.0, 64, 4).unwrap();
        let err = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
    }

    #[test]
    fn attach_resumes_mid_stream() {
        // The header index lines are resume state: a second
        // attach after two commits and one release continues
        // where the first left off.
        let mut r = Region::new();
        {
            let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
            for i in 0..2u64 {
                let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
                slot.seq = i;
                slot.val = i * 10;
                slot.commit();
            }
            cons.reserve_slot_with::<Msg>(|_| false).unwrap().release();
        }
        let (mut prod, mut cons) = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .unwrap()
            .split();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 1);
        msg.release();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 2;
        slot.commit();
        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).unwrap().seq, 2);
    }

    #[test]
    fn user_words_zeroed_and_shared() {
        let mut r = Region::new();
        // Dirty the region so init's zeroing is observable.
        r.0.fill(0xAA);
        let ring = Ring::init(&mut r.0, 64, 4).unwrap();
        let (prod, mut cons) = ring.split();
        assert!(prod.user().iter().all(|w| w.load(Ordering::Relaxed) == 0));
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
        // u32 wrap (empty state: p == c, every slot claimable
        // for the lap that starts at `start`).
        let start = u32::MAX - 1;
        ring.header.producer_idx.store(start, Ordering::Relaxed);
        ring.header.consumer_idx.store(start, Ordering::Relaxed);
        for i in 0..4u32 {
            let idx = start.wrapping_add(i);
            unsafe { &*ring.seqs.add((idx & ring.mask) as usize) }.store(idx, Ordering::Relaxed);
        }
        let (mut prod, mut cons) = ring.split();
        // Fill across the wrap: positions 2, 3, 0, 1.
        fill(&mut prod, 4);
        drain(&mut cons, 4);
    }

    #[test]
    fn roundtrip_full_empty() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        fill(&mut prod, 4);
        drain(&mut cons, 4);
        // One more write lands in the masked-around slot 0.
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 4;
        slot.commit();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 4);
        msg.release();
    }

    #[test]
    fn capacity_one_alternates() {
        // M = 1: one seq word carries the whole state through
        // claimable, committed, and released, and the ring
        // alternates Full/Empty.
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 1).unwrap().split();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        for i in 0..5u64 {
            fill(&mut prod, 1);
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, 0);
            // Release lets the producer in again.
            msg.release();
            assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i + 100;
            slot.commit();
            assert!(prod.reserve_slot_with::<Msg>(|_| false).is_err());
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, i + 100);
            msg.release();
        }
    }

    #[test]
    fn capacity_two_and_sixteen() {
        for cap in [2u32, 16] {
            let mut r = Region::new();
            let (mut prod, mut cons) = Ring::init(&mut r.0, 64, cap).unwrap().split();
            // Two full laps and a partial one.
            fill(&mut prod, cap as u64);
            drain(&mut cons, cap as u64);
            fill(&mut prod, cap as u64);
            drain(&mut cons, cap as u64);
            fill(&mut prod, cap as u64);
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, 0);
            msg.release();
            // One slot free again.
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = cap as u64;
            slot.commit();
            assert!(prod.reserve_slot_with::<Msg>(|_| false).is_err());
        }
    }

    #[test]
    // Dropping the guards is the behavior under test; they have
    // no Drop impl by design (abandon = do nothing).
    #[allow(clippy::drop_non_drop)]
    fn abandoned_guards_publish_nothing() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 99;
        drop(slot);
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
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
    fn reserve_slot_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
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
        fill(&mut prod, 4);
        let err = prod
            .reserve_slot_with::<Msg>(|attempt| attempt < 2)
            .err()
            .unwrap();
        assert_eq!(err, Full);
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

    /// The two-thread stream at capacity `cap`, `count`
    /// messages, through `policy` on both ends.
    fn threaded(cap: u32, count: u64, policy: fn(u32) -> bool) {
        let mut r = Region::new();
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, cap).unwrap().split();
        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..count {
                    let mut slot = prod.reserve_slot_with::<Msg>(policy).unwrap();
                    slot.seq = i;
                    slot.val = i * 3;
                    slot.commit();
                }
            });
            s.spawn(move || {
                for i in 0..count {
                    let msg = cons.reserve_slot_with::<Msg>(policy).unwrap();
                    assert_eq!(msg.seq, i);
                    assert_eq!(msg.val, i * 3);
                    msg.release();
                }
            });
        });
    }

    #[test]
    fn threaded_spsc() {
        // Reduced under Miri: interpreted spin loops are slow,
        // and its scheduler explores interleavings at any count.
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        for cap in [1u32, 2, 4, 16] {
            threaded(cap, COUNT, crate::policy::spin);
        }
    }
}
