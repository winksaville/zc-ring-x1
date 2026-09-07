//! SPSC ring v2: the in-slot seq, per the design doc's "SPSC
//! v2: in-slot seq ring" section.
//!
//! - v1's protocol with the seq moved into the slot it
//!   publishes: every slot opens with a crate-owned
//!   [`SLOT_HEADER_BYTES`] header holding the seq word, and the
//!   user's message sits behind it in the same cache line. The
//!   commit store and the message it publishes then travel on
//!   one line, where v1 moves the slot line and, amortised, the
//!   seq line.
//! - Same region shape as v0, a four-line [`Header`] then the
//!   slots, and no seq array. `region_size` is header plus
//!   slots.
//! - The slot contract changes: `T` must fit `slot_size -
//!   SLOT_HEADER_BYTES` and align to at most
//!   `SLOT_HEADER_BYTES`, checked at every reserve as the other
//!   rings check theirs.
//! - Own magic and layout version, so cross-attaching fails
//!   toward [`Error::BadMagic`]. Load/store only, `capacity`
//!   any power of two down to 1, committed `pos + M + 1`, all
//!   as v1.
//! - The seq's width is [`Seq`], one alias, so the u32 against
//!   native-width measurement is a one-line flip.

use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error, USER_WORDS};

mod consumer;
mod producer;

pub use consumer::{Consumer, ReadSlot};
pub use producer::{Producer, WriteSlot};

/// Layout marker written by [`Ring::init`]; distinct from the
/// v0, v1, and MPSC magics so cross-kind attach fails toward
/// [`Error::BadMagic`].
const MAGIC: u32 = 0x5A43_5233; // "ZCR3"

/// Bumped on any change to the v2 region layout; independent
/// of the other rings' layout versions.
const LAYOUT_VERSION: u32 = 1;

/// Capacity bound: `2^30`, as v1 and the MPSC ring.
const MAX_CAPACITY: u32 = 1 << 30;

/// Bytes the crate owns at the front of every slot: the seq
/// word first, the rest reserved.
///
/// - Sixteen rather than the seq's own width, so the seq can be
///   u32 or u64 without moving the body, and so a body may
///   align to 16.
/// - The body starts here, and a slot of N bytes carries
///   `N - SLOT_HEADER_BYTES` bytes of message.
pub const SLOT_HEADER_BYTES: usize = 16;

/// The seq word's atomic type, the one place its width is
/// chosen.
///
/// - `AtomicU32` is the v1 width, the baseline.
/// - `AtomicU64` is the native width on the machines measured,
///   the other arm of the width probe.
pub type Seq = AtomicU32;

/// The integer the seq holds, [`Seq`]'s plain form.
pub type SeqInt = u32;

const _: () = assert!(size_of::<Seq>() <= SLOT_HEADER_BYTES);
const _: () = assert!(align_of::<Seq>() <= SLOT_HEADER_BYTES);
const _: () = assert!(SLOT_HEADER_BYTES < CACHE_LINE_SIZE);

/// The crate-owned head of every slot.
///
/// - `seq` at offset 0, the protocol word.
/// - The rest reserved, zeroed by init and never read.
#[repr(C)]
pub struct SlotHeader {
    /// The slot's seq word: claimable at `pos`, committed at
    /// `pos + M + 1`, released at `pos + M`.
    seq: Seq,
    /// Reserved, so a wider seq or a later field has room.
    _reserved: [u8; SLOT_HEADER_BYTES - size_of::<Seq>()],
}

const _: () = assert!(size_of::<SlotHeader>() == SLOT_HEADER_BYTES);

/// Widen a free-running u32 index to the seq's integer.
///
/// - The indices stay u32 and wrap there, so the seq holds the
///   same values at either width, and a wider seq changes the
///   store's width alone.
fn seq_of(idx: u32) -> SeqInt {
    idx as SeqInt
}

/// Control block at offset 0 of a v2 region, v1's four-line
/// shape as its own type.
///
/// - line 0: geometry, written by [`Ring::init`] with `magic`
///   last (`Release`), read-only thereafter.
/// - line 1: `producer_idx`, the producer's private resume
///   state.
/// - line 2: `consumer_idx`, the consumer's, likewise private.
/// - line 3: `user`, app-owned scratch, the v0 contract.
/// - Every field is atomic: the region may be mapped by a
///   peer at any time.
#[repr(C)]
pub struct Header {
    /// Layout marker ([`MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`).
    magic: AtomicU32,
    /// Layout version ([`LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Slot size N in bytes, a [`CACHE_LINE_SIZE`] multiple.
    slot_size: AtomicU32,
    /// Slot count M, a power of two `<= 2^30`.
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

/// A validated view over a v2 ring region; split into the two
/// endpoint handles with [`Ring::split`].
///
/// - Geometry is snapshotted out of the header at init/attach,
///   as the other rings do.
/// - Region layout: [`Header`], then M slots of N bytes, each
///   opening with its [`SlotHeader`].
pub struct Ring<'a> {
    /// The region's control block.
    header: &'a Header,
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
    /// - `slot_size`: N bytes per slot, a [`CACHE_LINE_SIZE`]
    ///   multiple, of which [`SLOT_HEADER_BYTES`] are the
    ///   crate's.
    /// - `capacity`: M slots, a power of two `<= 2^30`, 1
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
        // SAFETY: in bounds, region.len() >= region_size.
        let slots = unsafe { base.add(size_of::<Header>()) };
        // `seq[i] = i`: every slot claimable for lap 0, and the
        // reserved bytes zeroed.
        for i in 0..capacity {
            // SAFETY: i < capacity, inside the slot array, and
            // the slot header's bytes are ours to write before
            // the ring is published.
            unsafe {
                let slot = slots.add(i as usize * slot_size as usize);
                core::ptr::write_bytes(slot, 0, SLOT_HEADER_BYTES);
                (*(slot as *const SlotHeader))
                    .seq
                    .store(seq_of(i), Ordering::Relaxed);
            }
        }
        // Published last: a peer that pre-mapped the region must
        // never observe MAGIC before the geometry and seqs it
        // relies on.
        header.magic.store(MAGIC, Ordering::Release);
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
        // SAFETY: in bounds, len >= region_size.
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
            Consumer::new(
                self.header,
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
fn header_ptr(base: *mut u8, len: usize) -> Result<*const Header, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<Header>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const Header)
}

/// Bytes needed for a v2 region with the given geometry: the
/// header, then the slots.
///
/// - Public, as v1's, so a pool that hands out ring segments
///   can size its buffers by it.
/// - Computed in u64 for the same 32-bit wrap reason as v0's.
pub fn region_size(slot_size: u32, capacity: u32) -> u64 {
    size_of::<Header>() as u64 + slot_size as u64 * capacity as u64
}

/// Geometry checks for [`Ring::init`] / [`Ring::attach`].
///
/// - Slot size as v0, and a line-multiple slot always has room
///   for the slot header. Capacity a power of two up to
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

/// Check `T` fits a slot's body; called once per
/// `reserve_slot_with` (both endpoints).
///
/// - The body is the slot behind [`SLOT_HEADER_BYTES`], so the
///   size bound is that much smaller than the slot and the
///   alignment bound is the header's size, the body's offset
///   in a line-aligned slot.
/// - Panics on a mismatch, a programming error rather than a
///   runtime condition, as the crate's `check_type` does.
fn check_body_type<T>(slot_size: u32) {
    assert!(
        size_of::<T>() <= slot_size as usize - SLOT_HEADER_BYTES,
        "T larger than the slot body"
    );
    assert!(
        align_of::<T>() <= SLOT_HEADER_BYTES,
        "T alignment exceeds the slot body's"
    );
}

/// The slot header and body for free-running index `idx`.
///
/// - Returns the slot's `SlotHeader` and the body pointer
///   behind it.
fn slot_parts<'s>(
    slots: *mut u8,
    idx: u32,
    mask: u32,
    slot_size: u32,
) -> (&'s SlotHeader, *mut u8) {
    let slot = crate::slot_ptr(slots, idx, mask, slot_size);
    // SAFETY: the slot is line-aligned and slot_size bytes, so
    // the header at its front is in bounds and aligned, and it
    // is atomic state shared by design; the body pointer is in
    // bounds since SLOT_HEADER_BYTES < CACHE_LINE_SIZE <=
    // slot_size.
    unsafe { (&*(slot as *const SlotHeader), slot.add(SLOT_HEADER_BYTES)) }
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

    /// Test region: header + 16 slots x 1 line, enough for every
    /// capacity the tests use.
    const REGION_BYTES: usize = size_of::<Header>() + (16 * CACHE_LINE_SIZE);

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
    fn region_is_header_then_slots() {
        for capacity in [1u32, 4, 16, 32] {
            assert_eq!(
                region_size(64, capacity),
                size_of::<Header>() as u64 + 64 * capacity as u64
            );
        }
    }

    #[test]
    fn body_sits_behind_the_slot_header() {
        // The reserved message starts SLOT_HEADER_BYTES into
        // its slot, and consecutive slots are slot_size apart.
        let mut r = Region::new();
        let base = r.0.as_ptr() as usize;
        let (mut prod, mut cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
        for i in 0..2usize {
            let slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            let want = base + size_of::<Header>() + i * 64 + SLOT_HEADER_BYTES;
            assert_eq!(&*slot as *const Msg as usize, want);
            slot.commit();
        }
        for i in 0..2usize {
            let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
            let want = base + size_of::<Header>() + i * 64 + SLOT_HEADER_BYTES;
            assert_eq!(&*msg as *const Msg as usize, want);
            msg.release();
        }
    }

    /// A message that fills a whole line, too big for a
    /// one-line slot's body.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    #[repr(C)]
    struct Line([u8; 64]);

    #[test]
    #[should_panic(expected = "T larger than the slot body")]
    fn body_type_must_fit_behind_the_header() {
        let mut r = Region::new();
        let (mut prod, _cons) = Ring::init(&mut r.0, 64, 4).unwrap().split();
        let _ = prod.reserve_slot_with::<Line>(|_| false);
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
        crate::spsc::v1::Ring::init(&mut r.0, 64, 4).unwrap();
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
            slot_parts(ring.slots, idx, ring.mask, ring.slot_size)
                .0
                .seq
                .store(seq_of(idx), Ordering::Relaxed);
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
