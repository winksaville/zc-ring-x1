//! MPSC ring: multi-producer single-consumer sibling of the
//! SPSC ring, per the design doc's "MPSC ring (sibling
//! primitive)" section:
//!
//! - Same region shape as the SPSC ring — a four-line
//!   [`MpscHeader`] then slots — plus a per-slot sequence
//!   array between them: `seq[pos]` publishes each slot
//!   independently, because claim order and commit order
//!   differ under concurrent producers.
//! - Producers CAS-claim `producer_idx`; the consumer stays
//!   CAS-free. Fullness is read from the slot seq, so
//!   producers never load `consumer_idx`.
//! - Own magic and layout version: a region is one kind or
//!   the other, and cross-attaching fails toward
//!   [`Error::BadMagic`].
//! - The claim CAS raises the atomic floor for MPSC users
//!   only — this module is gated on `target_has_atomic =
//!   "32"`; the SPSC ring stays load/store-only.

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error, USER_WORDS};

mod consumer;
mod producer;

pub use consumer::{MpscConsumer, MpscReadSlot};
pub use producer::MpscProducer;

/// Layout marker written by [`MpscRing::init`]; distinct from
/// the SPSC magic so cross-kind attach fails toward
/// [`Error::BadMagic`].
const MPSC_MAGIC: u32 = 0x5A43_4D31; // "ZCM1"

/// Bumped on any change to the MPSC region layout;
/// independent of the SPSC layout version.
const MPSC_LAYOUT_VERSION: u32 = 1;

/// Capacity bound: `2^30`, not the SPSC `2^31` — the seq
/// tombstone encoding reserves index-arithmetic headroom (see
/// the design doc's "MPSC open questions").
const MPSC_MAX_CAPACITY: u32 = 1 << 30;

/// Tombstone offset: an unwound `send_with` commits
/// `pos + 1 + TOMBSTONE` instead of `pos + 1`, marking a
/// claimed slot the consumer must release without delivering.
///
/// - Unambiguous at both wait points: the consumer at `c`
///   legitimately sees only `seq - (c+1)` of `-1` (not yet
///   committed) or `0` (committed), so exactly `2^31` means
///   tombstone; a producer's wrapping-signed diff reads a
///   tombstoned previous lap as negative — "full" — until the
///   consumer skips it, which is the correct backpressure.
pub(crate) const TOMBSTONE: u32 = 1 << 31;

/// Control block at offset 0 of an MPSC region — same
/// four-line shape as the SPSC [`Header`](crate::Header), but
/// its own type: the layouts evolve independently and the
/// index-ownership story differs.
///
/// - line 0: geometry, written by [`MpscRing::init`] with
///   `magic` last (`Release`), read-only thereafter.
/// - line 1: `producer_idx` — CAS-claimed by *all* producers
///   (the contended line), never read by the consumer's hot
///   path.
/// - line 2: `consumer_idx` — written only by the consumer;
///   never read by producers (fullness comes from the slot
///   seq), so it is resume state and diagnostic occupancy.
/// - line 3: `user`, app-owned scratch — same contract as the
///   SPSC user line.
/// - Every field is atomic: the region may be mapped by a
///   peer at any time.
#[repr(C)]
pub struct MpscHeader {
    /// Layout marker ([`MPSC_MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`).
    magic: AtomicU32,
    /// Layout version ([`MPSC_LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Slot size N in bytes — a [`CACHE_LINE_SIZE`] multiple.
    slot_size: AtomicU32,
    /// Slot count M — a power of two `<= 2^30`.
    capacity: AtomicU32,
    /// [`CACHE_LINE_SIZE`] this region was built with.
    cache_line_size: AtomicU32,
    /// Free-running count of slot positions claimed by
    /// producers (CAS).
    producer_idx: CacheAligned<AtomicU32>,
    /// Free-running count of messages released; consumer-owned
    /// resume state, not read by producers.
    consumer_idx: CacheAligned<AtomicU32>,
    /// App-owned scratch line ([`USER_WORDS`] words): zeroed by
    /// init, then never touched by the crate.
    user: CacheAligned<[AtomicU32; USER_WORDS]>,
}

const _: () = assert!(size_of::<MpscHeader>() == 4 * CACHE_LINE_SIZE);

/// A validated view over an MPSC ring region.
///
/// - Geometry is snapshotted out of the header at init/attach,
///   as the SPSC ring does: per-op paths never re-read fields
///   a peer could scribble on.
/// - Region layout: [`MpscHeader`], the seq array (`M ×
///   AtomicU32`, padded up to a cache line), then M slots of N
///   bytes.
pub struct MpscRing<'a> {
    /// The region's control block.
    header: &'a MpscHeader,
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

impl<'a> MpscRing<'a> {
    /// Initialize a fresh region and return the ring over it.
    ///
    /// - `slot_size` — N bytes per slot, a [`CACHE_LINE_SIZE`]
    ///   multiple.
    /// - `capacity` — M slots, a power of two `<= 2^30`.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at
    ///   least [`mpsc_region_size`] bytes.
    pub fn init(region: &'a mut [u8], slot_size: u32, capacity: u32) -> Result<Self, Error> {
        validate_mpsc_geometry(slot_size, capacity)?;
        let len = region.len();
        // Taken exactly once — see `Ring::init` for the Stacked
        // Borrows rationale.
        let base = region.as_mut_ptr();
        let header = mpsc_header_ptr(base, len)?;
        if (len as u64) < mpsc_region_size(slot_size, capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: alignment + room for the MpscHeader checked
        // by mpsc_header_ptr; region is exclusively borrowed
        // for 'a; any byte pattern is a valid MpscHeader
        // (all-atomic fields, plain-byte padding).
        let header = unsafe { &*header };
        header
            .layout_version
            .store(MPSC_LAYOUT_VERSION, Ordering::Relaxed);
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
        // SAFETY: in bounds — len covers header + seq array.
        let seqs = unsafe { base.add(size_of::<MpscHeader>()) } as *const AtomicU32;
        // seq[i] = i marks every slot claimable by the position
        // that will first reach it.
        for i in 0..capacity {
            // SAFETY: i < capacity, inside the seq array
            // validated by the region-size check.
            unsafe { &*seqs.add(i as usize) }.store(i, Ordering::Relaxed);
        }
        // Published last: a peer that pre-mapped the region must
        // never observe the magic before the geometry and seq
        // stores it validates and relies on.
        header.magic.store(MPSC_MAGIC, Ordering::Release);
        // SAFETY: in bounds — len covers header + seqs + slots.
        let slots = unsafe { base.add(slots_offset(capacity)) };
        Ok(MpscRing {
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
    /// - At most one consumer attaches; any number of
    ///   producers may (MPSC contract).
    pub unsafe fn attach(region: *mut u8, len: usize) -> Result<Self, Error> {
        let header = mpsc_header_ptr(region, len)?;
        // SAFETY: alignment + room for the MpscHeader checked
        // by mpsc_header_ptr; caller guarantees the memory is
        // live and shared.
        let header = unsafe { &*header };
        // Acquire pairs with init's Release store of magic: a
        // valid magic means the geometry and seq stores are
        // visible.
        if header.magic.load(Ordering::Acquire) != MPSC_MAGIC {
            return Err(Error::BadMagic);
        }
        if header.layout_version.load(Ordering::Relaxed) != MPSC_LAYOUT_VERSION {
            return Err(Error::BadLayoutVersion);
        }
        if header.cache_line_size.load(Ordering::Relaxed) != CACHE_LINE_SIZE as u32 {
            return Err(Error::BadCacheLine);
        }
        // Snapshot geometry once; per-op paths never re-read it.
        let slot_size = header.slot_size.load(Ordering::Relaxed);
        let capacity = header.capacity.load(Ordering::Relaxed);
        validate_mpsc_geometry(slot_size, capacity)?;
        if (len as u64) < mpsc_region_size(slot_size, capacity) {
            return Err(Error::TooSmall);
        }
        // SAFETY: in bounds — len covers header + seqs + slots.
        let seqs = unsafe { region.add(size_of::<MpscHeader>()) } as *const AtomicU32;
        // SAFETY: as above.
        let slots = unsafe { region.add(slots_offset(capacity)) };
        Ok(MpscRing {
            header,
            seqs,
            slots,
            slot_size,
            capacity,
            mask: capacity - 1,
            _region: PhantomData,
        })
    }

    /// Split into one producer handle and the consumer handle.
    ///
    /// - The producer is `Clone`: one clone per producing
    ///   thread (MPSC contract). The consumer is unique per
    ///   process; cross-process, exactly one consuming process
    ///   is the caller's contract (see [`MpscRing::attach`]).
    pub fn split(self) -> (MpscProducer<'a>, MpscConsumer<'a>) {
        (
            MpscProducer::new(
                self.header,
                self.seqs,
                self.slots,
                self.slot_size,
                self.mask,
            ),
            MpscConsumer::new(
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

/// Validate a region base pointer and cast it to the
/// [`MpscHeader`] it must start with; shared by init / attach.
///
/// - Same split of duties as the SPSC `header_ptr`: alignment
///   and header room here, full-geometry length with the
///   caller.
fn mpsc_header_ptr(base: *mut u8, len: usize) -> Result<*const MpscHeader, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<MpscHeader>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const MpscHeader)
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
///   after the u64 region-size check has proven the region —
///   hence this offset — fits in memory.
fn slots_offset(capacity: u32) -> usize {
    size_of::<MpscHeader>() + seq_bytes(capacity) as usize
}

/// Bytes needed for an MPSC region with the given geometry.
///
/// - Computed in u64 for the same 32-bit wrap reason as the
///   SPSC `region_size`.
pub fn mpsc_region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<MpscHeader>() as u64 + seq_bytes(capacity) + slot_size as u64 * capacity as u64
}

/// Geometry checks for [`MpscRing::init`] / [`MpscRing::attach`].
///
/// - Slot size as the SPSC ring; capacity additionally capped
///   at [`MPSC_MAX_CAPACITY`] for tombstone headroom.
fn validate_mpsc_geometry(slot_size: u32, capacity: u32) -> Result<(), Error> {
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::BadSlotSize);
    }
    if capacity == 0 || !capacity.is_power_of_two() || capacity > MPSC_MAX_CAPACITY {
        return Err(Error::BadCapacity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ring;

    /// Test region: header + seq line (4 × 4 B padded to 64) +
    /// 4 slots × 1 line.
    const REGION_BYTES: usize = size_of::<MpscHeader>() + CACHE_LINE_SIZE + (4 * CACHE_LINE_SIZE);

    /// Cache-line-aligned backing store for the tests' rings.
    #[repr(C, align(64))]
    struct Region([u8; REGION_BYTES]);

    impl Region {
        fn new() -> Self {
            Region([0; REGION_BYTES])
        }
    }

    #[test]
    fn mpsc_region_size_accounts_for_seq_array() {
        // 4 slots: header 256 + seq 16→64 + slots 256 = 576.
        assert_eq!(mpsc_region_size(64, 4), 576);
        // 32 slots: seq 128 is already a line multiple.
        assert_eq!(
            mpsc_region_size(64, 32),
            (size_of::<MpscHeader>() + 128 + 32 * 64) as u64
        );
    }

    #[test]
    fn mpsc_init_rejects_bad_geometry() {
        let mut r = Region::new();
        assert_eq!(
            MpscRing::init(&mut r.0, 63, 4).err().unwrap(),
            Error::BadSlotSize
        );
        assert_eq!(
            MpscRing::init(&mut r.0, 0, 4).err().unwrap(),
            Error::BadSlotSize
        );
        assert_eq!(
            MpscRing::init(&mut r.0, 64, 3).err().unwrap(),
            Error::BadCapacity
        );
        assert_eq!(
            MpscRing::init(&mut r.0, 64, 0).err().unwrap(),
            Error::BadCapacity
        );
        // Over the MPSC cap (fine for SPSC): tombstone headroom.
        assert_eq!(
            MpscRing::init(&mut r.0, 64, 1 << 31).err().unwrap(),
            Error::BadCapacity
        );
        assert_eq!(
            MpscRing::init(&mut r.0, 64, 8).err().unwrap(),
            Error::TooSmall
        );
        assert_eq!(
            MpscRing::init(&mut r.0[1..], 64, 4).err().unwrap(),
            Error::Misaligned
        );
        // 32-bit tripwire, as the SPSC test: N * M wraps a
        // 32-bit usize; the u64 region size must still reject.
        assert_eq!(
            MpscRing::init(&mut r.0, 1 << 26, 1 << 6).err().unwrap(),
            Error::TooSmall
        );
        // An SPSC-sized region (no seq line) is too small here.
        let spsc_bytes = size_of::<crate::Header>() + 4 * 64;
        assert_eq!(
            MpscRing::init(&mut r.0[..spsc_bytes], 64, 4).err().unwrap(),
            Error::TooSmall
        );
    }

    #[test]
    fn mpsc_init_seeds_seq_array() {
        let mut r = Region::new();
        // Dirty the region so the seq stores are observable.
        r.0.fill(0xAA);
        let ring = MpscRing::init(&mut r.0, 64, 4).unwrap();
        for i in 0..4 {
            // SAFETY: i < capacity; test mirrors init's bounds.
            let seq = unsafe { &*ring.seqs.add(i as usize) };
            assert_eq!(seq.load(Ordering::Relaxed), i);
        }
        assert_eq!(ring.slot_size, 64);
        assert_eq!(ring.capacity, 4);
        assert_eq!(ring.mask, 3);
    }

    #[test]
    fn mpsc_attach_validates_header() {
        let mut r = Region::new();
        // Not initialized yet: magic is zero.
        let err = unsafe { MpscRing::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
        MpscRing::init(&mut r.0, 64, 4).unwrap();
        let ring = unsafe { MpscRing::attach(r.0.as_mut_ptr(), r.0.len()) }.unwrap();
        assert_eq!(ring.slot_size, 64);
        assert_eq!(ring.capacity, 4);
        // A different recorded cache line is rejected.
        ring.header.cache_line_size.store(128, Ordering::Relaxed);
        let err = unsafe { MpscRing::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadCacheLine);
    }

    #[test]
    fn cross_kind_attach_fails_bad_magic() {
        // An SPSC-initialized region is not an MPSC region…
        let mut r = Region::new();
        Ring::init(&mut r.0, 64, 4).unwrap();
        let err = unsafe { MpscRing::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
        // …and an MPSC-initialized region is not an SPSC one.
        let mut r = Region::new();
        MpscRing::init(&mut r.0, 64, 4).unwrap();
        let err = unsafe { Ring::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
    }

    use crate::{Empty, Full};
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message; two words so a torn write would be
    /// visible, `val` doubles as the producer id in threaded
    /// tests.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    #[test]
    fn mpsc_roundtrip_full_empty() {
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();

        // Empty at start.
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());

        // Fill all 4 slots, then Full on a single probe.
        for i in 0..4u64 {
            prod.send_with::<Msg>(
                |_| false,
                |m| {
                    m.seq = i;
                    m.val = i * 10;
                },
            )
            .unwrap();
        }
        assert_eq!(
            prod.send_with::<Msg>(|_| false, |_| {}).err().unwrap(),
            Full
        );

        // Drain in order; then empty again.
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

        // One more send lands in the masked-around slot 0.
        prod.send_with::<Msg>(
            |_| false,
            |m| {
                m.seq = 4;
                m.val = 0;
            },
        )
        .unwrap();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 4);
        msg.release();
    }

    #[test]
    fn mpsc_policies_count_and_give_up() {
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();

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
        // Err(Full), and the failed send has zero footprint —
        // the fill closure never ran.
        for i in 0..4u64 {
            prod.send_with::<Msg>(|_| false, |m| m.seq = i).unwrap();
        }
        let mut seen = Vec::new();
        let err = prod
            .send_with::<Msg>(
                |attempt| {
                    seen.push(attempt);
                    attempt < 2
                },
                |_| panic!("fill ran on a full ring"),
            )
            .err()
            .unwrap();
        assert_eq!(err, Full);
        assert_eq!(seen, [0, 1, 2]);

        // With room / messages available the policy is never
        // consulted.
        let msg = cons
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with a message available"))
            .unwrap();
        assert_eq!(msg.seq, 0);
        msg.release();
        prod.send_with::<Msg>(
            |_| panic!("policy consulted with room available"),
            |m| m.seq = 4,
        )
        .unwrap();
    }

    #[test]
    // Dropping the guard is the behavior under test.
    #[allow(clippy::drop_non_drop)]
    fn mpsc_abandoned_read_guard_redelivers() {
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();

        prod.send_with::<Msg>(|_| false, |m| m.seq = 1).unwrap();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 1);
        drop(msg);
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 1);
        msg.release();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
    }

    #[test]
    fn mpsc_panicking_fill_tombstones_and_skips() {
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();

        // A committed message, a panicked fill, another
        // committed message.
        prod.send_with::<Msg>(|_| false, |m| m.seq = 1).unwrap();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prod.send_with::<Msg>(
                |_| false,
                |m| {
                    m.seq = 99; // partially filled, then…
                    panic!("fill panics");
                },
            )
        }));
        assert!(unwound.is_err());
        prod.send_with::<Msg>(|_| false, |m| m.seq = 2).unwrap();

        // The consumer sees 1 then 2; the tombstoned slot is
        // released without delivery.
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 1);
        msg.release();
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 2);
        msg.release();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());

        // The skipped slot is claimable again: a full lap
        // still fits 4 messages.
        for i in 3..7u64 {
            prod.send_with::<Msg>(|_| false, |m| m.seq = i).unwrap();
        }
        assert!(prod.send_with::<Msg>(|_| false, |_| {}).is_err());
        for i in 3..7u64 {
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, i);
            msg.release();
        }
    }

    #[test]
    fn mpsc_indices_survive_u32_wrap() {
        let mut r = Region::new();
        let ring = MpscRing::init(&mut r.0, 64, 4).unwrap();
        // Simulate a long-running ring two commits shy of the
        // u32 wrap: indices at MAX-1, and each slot's seq must
        // equal the free-running position that will next claim
        // it (positions MAX-1, MAX, 0, 1 → slots 2, 3, 0, 1).
        let start = u32::MAX - 1;
        ring.header.producer_idx.store(start, Ordering::Relaxed);
        ring.header.consumer_idx.store(start, Ordering::Relaxed);
        for (slot, pos) in [(2u32, u32::MAX - 1), (3, u32::MAX), (0, 0), (1, 1)] {
            // SAFETY: slot < capacity; test mirrors init.
            unsafe { &*ring.seqs.add(slot as usize) }.store(pos, Ordering::Relaxed);
        }
        let (prod, mut cons) = ring.split();

        // Fill across the wrap, then Full.
        for i in 0..4u64 {
            prod.send_with::<Msg>(|_| false, |m| m.seq = i).unwrap();
        }
        assert!(prod.send_with::<Msg>(|_| false, |_| {}).is_err());

        // Drain in order across the wrap.
        for i in 0..4u64 {
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            assert_eq!(msg.seq, i);
            msg.release();
        }
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
    }

    #[test]
    fn threaded_mpsc_two_producers() {
        // Reduced under Miri: interpreted spin loops are slow.
        const COUNT: u64 = if cfg!(miri) { 100 } else { 50_000 };
        const PRODUCERS: u64 = 2;
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();

        std::thread::scope(|s| {
            for p in 0..PRODUCERS {
                let prod = prod.clone();
                s.spawn(move || {
                    for i in 0..COUNT {
                        prod.send_with::<Msg>(crate::policy::spin, |m| {
                            m.seq = i;
                            m.val = p;
                        })
                        .unwrap(); // OK: policy::spin never gives up
                    }
                });
            }
            s.spawn(move || {
                // Global arrival order is claim order; only
                // per-producer FIFO is promised.
                let mut next = [0u64; PRODUCERS as usize];
                for _ in 0..PRODUCERS * COUNT {
                    let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap(); // OK: policy::spin never gives up
                    let p = msg.val as usize;
                    assert_eq!(msg.seq, next[p], "per-producer order broken");
                    next[p] += 1;
                    msg.release();
                }
                assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
            });
        });
    }

    #[test]
    fn threaded_mpsc_shared_reference_producers() {
        // The Sync path: two threads share one &MpscProducer
        // instead of cloning.
        const COUNT: u64 = if cfg!(miri) { 100 } else { 50_000 };
        let mut r = Region::new();
        let (prod, mut cons) = MpscRing::init(&mut r.0, 64, 4).unwrap().split();
        let prod = &prod;

        std::thread::scope(|s| {
            for p in 0..2u64 {
                s.spawn(move || {
                    for i in 0..COUNT {
                        prod.send_with::<Msg>(crate::policy::spin, |m| {
                            m.seq = i;
                            m.val = p;
                        })
                        .unwrap(); // OK: policy::spin never gives up
                    }
                });
            }
            s.spawn(move || {
                let mut next = [0u64; 2];
                for _ in 0..2 * COUNT {
                    let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap(); // OK: policy::spin never gives up
                    let p = msg.val as usize;
                    assert_eq!(msg.seq, next[p]);
                    next[p] += 1;
                    msg.release();
                }
            });
        });
    }
}
