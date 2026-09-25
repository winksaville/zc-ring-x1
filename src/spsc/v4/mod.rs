//! SPSC ring v4: v3's ring of segments over a ring that describes
//! itself in the region, so a second process can attach to it,
//! and v3 stays as built to measure against.
//!
//! - What v4 adds to v3: a control block at the front of segment
//!   0 (magic, layout version, geometry, the role claims word, and
//!   the table of every segment's pool buffer index), a table of
//!   offsets instead of pointers in the endpoints, [`Ring::attach`]
//!   from a pool and [`Ring::first_segment`], and the roles taken
//!   by name, [`Ring::producer`] and [`Ring::consumer`], each held
//!   once anywhere. The protocol below is v3's, unchanged.
//! - A ring holds up to [`MAX_SEGMENTS`] segments, each a ring of
//!   its own of the same depth, every one taken from the
//!   application's pool at [`Ring::init`]. Nothing allocates,
//!   frees, or re-initializes while the ring runs.
//! - With a consumer that keeps up, the ring lives in one segment
//!   and each side loads one seq word per message, as v2 does.
//!   The other segments are insurance for a producer that runs
//!   ahead, and switching between them is the only cost v3 adds.
//! - Slots open with v2's in-slot seq word, 32 bits so v3 builds
//!   wherever v0 through v2 do. The low [`SEQ_BITS`] hold v2's
//!   claimable `pos`, committed `pos + M + 1`, and released
//!   `pos + M`, and the high bits a MOVED flag and the next
//!   segment's number, so one load tells the consumer empty, a
//!   message, or a message and then segment `k`.
//! - The producer decides a switch at its own commit, the one
//!   moment it alone writes the slot's word: when the next slot
//!   is not claimable it takes a free segment and commits with
//!   MOVED. A slot seen claimable stays claimable until the
//!   producer claims it, so that look-ahead replaces the next
//!   reserve's load.
//! - Free segments need no CAS: the producer keeps a private
//!   word flipping a segment's bit when it takes it, the consumer
//!   a shared word flipping it when it gives it back, so a
//!   segment is free where the two agree.
//! - Each side keeps a private resume position per segment. Both
//!   leave a segment at the same slot, so a reused segment's
//!   seqs are already claimable where the producer picks up.
//! - Attach is a join, not a resume: an endpoint taken after
//!   `attach` starts in segment 0 at position 0, as one taken
//!   after `init` does, so a process joins before its role has
//!   run.
//! - How to use it, from a pool to two threads and to a second
//!   process, is the user guide, `notes/user-guide.md`, and what
//!   happens to the segments over a run is the design note's
//!   Segment lifecycle subsection, v3's and v4's alike.

use core::mem::{align_of, size_of};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::pool::Pool;
use crate::{CACHE_LINE_SIZE, CacheAligned, Error};

mod consumer;
mod producer;

pub use crate::spsc::v2::SLOT_HEADER_BYTES;
pub use consumer::{Consumer, ReadSlot};
pub use producer::{Producer, WriteSlot};

/// The most segments a ring holds: one bit each in the free
/// words, and five bits of the seq word name one.
pub const MAX_SEGMENTS: u32 = 32;

/// Seq word bits holding the seq value. The rest carry MOVED
/// and the next segment's number.
pub const SEQ_BITS: u32 = 26;

/// The seq value's bits within the word.
const SEQ_MASK: u32 = (1 << SEQ_BITS) - 1;

/// Where the next segment's number sits in the word.
const SEG_SHIFT: u32 = SEQ_BITS;

/// The next segment's number, once shifted down.
const SEG_MASK: u32 = MAX_SEGMENTS - 1;

/// Set in a committed word whose message is the producer's last
/// in its segment.
const MOVED: u32 = 1 << 31;

/// Segment depth bound: seq values wrap at `2^SEQ_BITS`, and
/// claimable, committed, and released must stay distinct.
pub const MAX_SEG_CAPACITY: u32 = 1 << 24;

const _: () = assert!(SEG_SHIFT + MAX_SEGMENTS.trailing_zeros() <= 31);
const _: () = assert!(MAX_SEG_CAPACITY + 1 < SEQ_MASK);

/// The seq value for free-running position `idx`, wrapped to
/// [`SEQ_BITS`].
#[inline]
pub(crate) fn seq_of(idx: u32) -> u32 {
    idx & SEQ_MASK
}

/// Layout marker written by [`Ring::init`] into every segment's
/// header, distinct from the other rings' and the pools'.
const MAGIC: u32 = 0x5A43_5234; // "ZCR4"

/// Bumped on any change to the segment layout.
const LAYOUT_VERSION: u32 = 1;

/// A table entry naming no segment.
const NO_SEGMENT: u32 = u32::MAX;

/// The producer's bit in the claims word.
const PRODUCER_CLAIM: u32 = 1;

/// The consumer's bit in the claims word.
const CONSUMER_CLAIM: u32 = 2;

/// The first line of every segment: which ring it belongs to,
/// written by [`Ring::init`] and read back by `attach`.
///
/// - Every field is a `u32` atomic, so a process reads the line
///   with the ordering the magic's Acquire gives it.
/// - `given` is the consumer's give-back word, meaningful in
///   segment 0 only, the one word both sides share besides the
///   slots. It shares the line with the geometry because the
///   geometry is read at attach and never after.
#[repr(C)]
struct Info {
    magic: AtomicU32,
    layout_version: AtomicU32,
    slot_size: AtomicU32,
    seg_capacity: AtomicU32,
    seg_count: AtomicU32,
    /// This segment's number in the ring.
    seg_num: AtomicU32,
    given: AtomicU32,
}

/// The four lines at the front of every segment: the ring's
/// control block in segment 0, and the same layout in the others
/// so every segment's slots start at one offset.
///
/// - The pool buffer index of each segment is in the table, so a
///   process holding the pool and segment 0's index finds every
///   segment: the offsets-only rule, applied to the ring's table.
/// - The claims word is its own line, so the CAS that takes a
///   role never shares a line with `given`.
#[repr(C)]
struct SegmentHeader {
    /// Line 0: the ring's identity and geometry, and `given`.
    info: CacheAligned<Info>,
    /// Line 1: the role claims word, segment 0's only.
    claims: CacheAligned<AtomicU32>,
    /// Lines 2 and 3: the pool buffer index of segment `i` at
    /// `table[i]`, [`NO_SEGMENT`] past `seg_count`, segment 0's
    /// only.
    table: CacheAligned<[AtomicU32; MAX_SEGMENTS as usize]>,
}

const _: () = assert!(size_of::<SegmentHeader>() == 4 * CACHE_LINE_SIZE);

/// Bytes a segment needs: its header lines, then the slots.
///
/// - The pool handed to [`Ring::init`] needs buffers at least
///   this large.
/// - Computed in u64 for the same 32-bit wrap reason as the
///   rings' region sizes.
pub fn segment_size(slot_size: u32, seg_capacity: u32) -> u64 {
    size_of::<SegmentHeader>() as u64 + slot_size as u64 * seg_capacity as u64
}

/// A ring's geometry and where its segments are, the state both
/// endpoints start from.
///
/// - The segments are byte offsets from the pool's buffer array,
///   the same numbers in every process, and `base` is that array
///   in this one, so the table could be shared as it is and a
///   slot access is one add over v3's.
#[derive(Clone, Copy)]
struct Segments {
    /// The pool's buffer array in this process.
    base: *mut u8,
    /// The slot array of each segment, `seg_count` of them, as an
    /// offset from `base`.
    slots: [usize; MAX_SEGMENTS as usize],
    /// Segment 0's header as an offset from `base`: `given` and
    /// the claims word live there.
    header0: usize,
    /// Slot size N in bytes.
    slot_size: u32,
    /// Segment depth M, a power of two.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    /// Segments in the ring.
    seg_count: u32,
}

impl Segments {
    /// The seq word of the slot at free-running `idx` in segment
    /// `seg`.
    fn seq(&self, seg: u32, idx: u32) -> &AtomicU32 {
        let slot = crate::slot_ptr(self.slot_base(seg), idx, self.mask, self.slot_size);
        // SAFETY: seg < seg_count and the slot is in bounds of
        // that segment's buffer, line-aligned, so its first word
        // is an aligned AtomicU32, shared atomic state by design.
        unsafe { &*(slot as *const AtomicU32) }
    }

    /// The body of the slot at free-running `idx` in segment
    /// `seg`, behind its [`SLOT_HEADER_BYTES`].
    fn body(&self, seg: u32, idx: u32) -> *mut u8 {
        let slot = crate::slot_ptr(self.slot_base(seg), idx, self.mask, self.slot_size);
        // SAFETY: SLOT_HEADER_BYTES < CACHE_LINE_SIZE <= slot_size,
        // so the body starts inside the slot.
        unsafe { slot.add(SLOT_HEADER_BYTES) }
    }

    /// Segment `seg`'s slot array in this process.
    #[inline]
    fn slot_base(&self, seg: u32) -> *mut u8 {
        // SAFETY: seg < seg_count, and the offset was computed by
        // init or attach from a buffer index the pool validated,
        // so it stays inside the buffer array.
        unsafe { self.base.add(self.slots[seg as usize]) }
    }

    /// Segment 0's header, the ring's control block.
    fn header0(&self) -> &SegmentHeader {
        // SAFETY: header0 is the front of segment 0's buffer,
        // line-aligned, which lives as long as the pool region the
        // ring borrows from, and every field is atomic.
        unsafe { &*(self.base.add(self.header0) as *const SegmentHeader) }
    }

    /// The consumer's give-back word.
    fn given(&self) -> &AtomicU32 {
        &self.header0().info.given
    }

    /// The table every endpoint starts from, built the same way
    /// by `init` and `attach`: each segment's slot array as an
    /// offset from the pool's buffer array.
    fn load(
        base: *mut u8,
        buf_size: usize,
        indices: &[u32; MAX_SEGMENTS as usize],
        slot_size: u32,
        seg_capacity: u32,
        seg_count: u32,
    ) -> Self {
        let mut slots = [0usize; MAX_SEGMENTS as usize];
        for seg in 0..seg_count as usize {
            slots[seg] = indices[seg] as usize * buf_size + size_of::<SegmentHeader>();
        }
        Segments {
            base,
            slots,
            header0: indices[0] as usize * buf_size,
            slot_size,
            capacity: seg_capacity,
            mask: seg_capacity - 1,
            seg_count,
        }
    }

    /// Release a role's bit in the claims word, the endpoint's
    /// drop.
    fn release_role(&self, bit: u32) {
        // Release: the next claimant's AcqRel sees everything this
        // endpoint did.
        self.header0().claims.fetch_and(!bit, Ordering::Release);
    }

    /// Every segment's bit.
    fn all(&self) -> u32 {
        if self.seg_count == MAX_SEGMENTS {
            u32::MAX
        } else {
            (1 << self.seg_count) - 1
        }
    }
}

/// A ring of segments over the application's pool, split into
/// the two endpoint handles with [`Ring::split`].
pub struct Ring<'a> {
    /// Geometry and segment offsets.
    segs: Segments,
    /// The pool buffer index of segment 0, where the control
    /// block is.
    first_segment: u32,
    _region: core::marker::PhantomData<&'a [u8]>,
}

impl<'a> Ring<'a> {
    /// Take `seg_count` segments from `pool` and initialize each
    /// as an empty ring of `seg_capacity` slots of `slot_size`
    /// bytes.
    ///
    /// - `slot_size`: N bytes per slot, a [`CACHE_LINE_SIZE`]
    ///   multiple, of which [`SLOT_HEADER_BYTES`] are the crate's.
    /// - `seg_capacity`: M slots per segment, a power of two up to
    ///   [`MAX_SEG_CAPACITY`], 1 included.
    /// - `seg_count`: 1 to [`MAX_SEGMENTS`].
    /// - The pool's buffers must hold [`segment_size`], else
    ///   [`Error::TooSmall`]. A pool without `seg_count` free
    ///   buffers gives [`Error::Exhausted`], with the buffers
    ///   taken so far freed.
    /// - The pool is borrowed only here. The segments stay
    ///   allocated for the life of the pool region, as a
    ///   [`BufSlot`](crate::BufSlot) dropped without `free` does.
    /// - Every segment's header names the ring, and segment 0's
    ///   holds the table of segments, so a process holding the
    ///   pool and [`first_segment`](Ring::first_segment) can find
    ///   the ring.
    pub fn init(
        pool: &mut Pool<'a>,
        slot_size: u32,
        seg_capacity: u32,
        seg_count: u32,
    ) -> Result<Self, Error> {
        validate_geometry(slot_size, seg_capacity, seg_count)?;
        if (pool.buf_size() as u64) < segment_size(slot_size, seg_capacity) {
            return Err(Error::TooSmall);
        }
        let mut taken: [Option<crate::BufSlot<'a, [u8]>>; MAX_SEGMENTS as usize] =
            core::array::from_fn(|_| None);
        let mut indices = [NO_SEGMENT; MAX_SEGMENTS as usize];
        for seg in 0..seg_count as usize {
            match pool.alloc_bytes() {
                Ok(buf) => {
                    indices[seg] = buf.idx();
                    taken[seg] = Some(buf);
                }
                Err(_) => {
                    taken.into_iter().flatten().for_each(crate::BufSlot::free);
                    return Err(Error::Exhausted);
                }
            }
        }
        // The segments stay allocated: their guards go out of
        // scope with `taken`, never freed.
        let base = pool.bufs_ptr();
        let buf_size = pool.buf_size() as usize;
        for seg in 0..seg_count as usize {
            let offset = indices[seg] as usize * buf_size;
            // SAFETY: the offset names a buffer of at least
            // segment_size bytes the pool just handed out,
            // line-aligned, and nothing else can reach it until a
            // role is taken: the header lines and each slot's
            // header are ours to write.
            unsafe {
                let seg_base = base.add(offset);
                core::ptr::write_bytes(seg_base, 0, size_of::<SegmentHeader>());
                let header = &*(seg_base as *const SegmentHeader);
                header
                    .info
                    .layout_version
                    .store(LAYOUT_VERSION, Ordering::Relaxed);
                header.info.slot_size.store(slot_size, Ordering::Relaxed);
                header
                    .info
                    .seg_capacity
                    .store(seg_capacity, Ordering::Relaxed);
                header.info.seg_count.store(seg_count, Ordering::Relaxed);
                header.info.seg_num.store(seg as u32, Ordering::Relaxed);
                if seg == 0 {
                    for (entry, &idx) in header.table.iter().zip(indices.iter()) {
                        entry.store(idx, Ordering::Relaxed);
                    }
                }
                let seg_slots = seg_base.add(size_of::<SegmentHeader>());
                for i in 0..seg_capacity {
                    let slot = seg_slots.add(i as usize * slot_size as usize);
                    core::ptr::write_bytes(slot, 0, SLOT_HEADER_BYTES);
                    // `seq[i] = i`: every slot claimable for lap 0.
                    (*(slot as *const AtomicU32)).store(seq_of(i), Ordering::Relaxed);
                }
                // The magic last (Release): a reader that sees it
                // sees the rest of the block.
                header.info.magic.store(MAGIC, Ordering::Release);
            }
        }
        Ok(Ring {
            segs: Segments::load(base, buf_size, &indices, slot_size, seg_capacity, seg_count),
            first_segment: indices[0],
            _region: core::marker::PhantomData,
        })
    }

    /// Join a ring another process (or an earlier call) initialized
    /// over `pool`, from the pool buffer index of its segment 0.
    ///
    /// - Reads the control block, checks every field and every
    ///   table entry against the pool's geometry, and every
    ///   segment's own header against the block, so a hostile
    ///   region is an `Err`, never an out-of-bounds access.
    /// - The endpoints then taken start in segment 0 at position
    ///   0, as after `init`, so attach is for a process joining
    ///   before its role has run. Recovering a mid-run position is
    ///   not done here.
    ///
    /// # Safety
    ///
    /// - `first_segment` came from [`first_segment`](Ring::first_segment)
    ///   of a ring initialized over this pool's region, and its
    ///   segments are still the ring's: validation cannot tell a
    ///   ring's segment from a buffer since freed and reused, and
    ///   the ring writes seq words into every segment it is told
    ///   it has.
    pub unsafe fn attach(pool: &Pool<'a>, first_segment: u32) -> Result<Self, Error> {
        let buf_count = pool.buf_count();
        if first_segment >= buf_count {
            return Err(Error::BadSegment);
        }
        let base = pool.bufs_ptr();
        let buf_size = pool.buf_size() as usize;
        // SAFETY: first_segment < buf_count, so the header is the
        // line-aligned front of a buffer inside the pool's region,
        // and every field read is atomic.
        let block =
            unsafe { &*(base.add(first_segment as usize * buf_size) as *const SegmentHeader) };
        // Acquire pairs with init's Release store of the magic.
        if block.info.magic.load(Ordering::Acquire) != MAGIC {
            return Err(Error::BadMagic);
        }
        if block.info.layout_version.load(Ordering::Relaxed) != LAYOUT_VERSION {
            return Err(Error::BadLayoutVersion);
        }
        let slot_size = block.info.slot_size.load(Ordering::Relaxed);
        let seg_capacity = block.info.seg_capacity.load(Ordering::Relaxed);
        let seg_count = block.info.seg_count.load(Ordering::Relaxed);
        validate_geometry(slot_size, seg_capacity, seg_count)?;
        if (buf_size as u64) < segment_size(slot_size, seg_capacity) {
            return Err(Error::TooSmall);
        }
        if block.info.seg_num.load(Ordering::Relaxed) != 0 {
            return Err(Error::BadSegment);
        }
        let mut indices = [NO_SEGMENT; MAX_SEGMENTS as usize];
        for seg in 0..seg_count as usize {
            let idx = block.table[seg].load(Ordering::Relaxed);
            if idx >= buf_count || indices[..seg].contains(&idx) {
                return Err(Error::BadSegment);
            }
            indices[seg] = idx;
        }
        if indices[0] != first_segment {
            return Err(Error::BadSegment);
        }
        // Every segment's own header must agree with the block.
        for (seg, &idx) in indices.iter().enumerate().take(seg_count as usize) {
            // SAFETY: idx < buf_count, as checked above.
            let info =
                unsafe { &(*(base.add(idx as usize * buf_size) as *const SegmentHeader)).info };
            let agrees = info.magic.load(Ordering::Acquire) == MAGIC
                && info.layout_version.load(Ordering::Relaxed) == LAYOUT_VERSION
                && info.slot_size.load(Ordering::Relaxed) == slot_size
                && info.seg_capacity.load(Ordering::Relaxed) == seg_capacity
                && info.seg_count.load(Ordering::Relaxed) == seg_count
                && info.seg_num.load(Ordering::Relaxed) == seg as u32;
            if !agrees {
                return Err(Error::BadSegment);
            }
        }
        Ok(Ring {
            segs: Segments::load(base, buf_size, &indices, slot_size, seg_capacity, seg_count),
            first_segment,
            _region: core::marker::PhantomData,
        })
    }

    /// Take the producer role.
    ///
    /// - One CAS on the control block's claims word, so a role
    ///   held anywhere, in this process or another, is
    ///   [`Error::RoleTaken`], and dropping the endpoint releases
    ///   it. Nothing on the message path reads the word.
    /// - Starts in segment 0 at position 0, which it holds as
    ///   taken.
    pub fn producer(&self) -> Result<Producer<'a>, Error> {
        self.claim(PRODUCER_CLAIM)?;
        Ok(Producer::new(self.segs))
    }

    /// Take the consumer role, the counterpart of
    /// [`producer`](Ring::producer).
    pub fn consumer(&self) -> Result<Consumer<'a>, Error> {
        self.claim(CONSUMER_CLAIM)?;
        Ok(Consumer::new(self.segs))
    }

    /// Set `bit` in the claims word, unless it was set.
    fn claim(&self, bit: u32) -> Result<(), Error> {
        // AcqRel: a claim that succeeds sees the state a released
        // endpoint left, and its own release publishes the same.
        // Setting a set bit changes nothing, so a failure needs no
        // undo.
        let before = self.segs.header0().claims.fetch_or(bit, Ordering::AcqRel);
        if before & bit != 0 {
            return Err(Error::RoleTaken);
        }
        Ok(())
    }

    /// The pool buffer index of segment 0, where the ring's
    /// control block is: what a process hands to another so it
    /// can find the ring in the same pool.
    pub fn first_segment(&self) -> u32 {
        self.first_segment
    }
}

/// Geometry checks for [`Ring::init`].
pub(crate) fn validate_geometry(
    slot_size: u32,
    seg_capacity: u32,
    seg_count: u32,
) -> Result<(), Error> {
    if slot_size == 0 || !(slot_size as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::BadSlotSize);
    }
    if seg_capacity == 0 || !seg_capacity.is_power_of_two() || seg_capacity > MAX_SEG_CAPACITY {
        return Err(Error::BadCapacity);
    }
    if seg_count == 0 || seg_count > MAX_SEGMENTS {
        return Err(Error::BadSegmentCount);
    }
    Ok(())
}

/// Check `T` fits a slot's body, called once per
/// `reserve_slot_with` (both endpoints), as v2 checks.
pub(crate) fn check_body_type<T>(slot_size: u32) {
    assert!(
        size_of::<T>() <= slot_size as usize - SLOT_HEADER_BYTES,
        "T larger than the slot body"
    );
    assert!(
        align_of::<T>() <= SLOT_HEADER_BYTES,
        "T alignment exceeds the slot body's"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Empty, Exhausted, Full, PoolHeader};
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message, two words so a torn write would be visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// Test pool buffer: a segment of up to 16 one-line slots
    /// behind its four header lines.
    const BUF: usize = 4 * CACHE_LINE_SIZE + 16 * CACHE_LINE_SIZE;

    /// Buffers in the test pool.
    const BUFS: usize = 8;

    /// Backing store for the tests' pools.
    #[repr(C, align(64))]
    struct Region([u8; size_of::<PoolHeader>() + BUFS * BUF]);

    impl Region {
        fn new() -> Box<Self> {
            Box::new(Region([0; size_of::<PoolHeader>() + BUFS * BUF]))
        }
    }

    /// Send `from..to` as `seq = i`, `val = i * 10`.
    fn send(prod: &mut Producer<'_>, from: u64, to: u64) {
        for i in from..to {
            let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
            slot.seq = i;
            slot.val = i * 10;
            slot.commit();
        }
    }

    /// Both roles of `ring`, the in-process pair.
    fn endpoints<'a>(ring: &Ring<'a>) -> (Producer<'a>, Consumer<'a>) {
        (ring.producer().unwrap(), ring.consumer().unwrap())
    }

    /// Receive `from..to` in order.
    fn recv(cons: &mut Consumer<'_>, from: u64, to: u64) {
        for i in from..to {
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
    }

    /// Segments free by the producer's reckoning.
    fn free_segments(prod: &Producer<'_>) -> u32 {
        let st = &prod.st;
        !(st.taken ^ st.segs.given().load(Ordering::Acquire)) & st.segs.all()
    }

    #[test]
    fn init_rejects_bad_geometry() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let err = |slot, cap, count, pool: &mut Pool<'_>| Ring::init(pool, slot, cap, count).err();
        assert_eq!(err(63, 4, 2, &mut pool), Some(Error::BadSlotSize));
        assert_eq!(err(0, 4, 2, &mut pool), Some(Error::BadSlotSize));
        assert_eq!(err(64, 3, 2, &mut pool), Some(Error::BadCapacity));
        assert_eq!(err(64, 0, 2, &mut pool), Some(Error::BadCapacity));
        assert_eq!(
            err(64, MAX_SEG_CAPACITY * 2, 2, &mut pool),
            Some(Error::BadCapacity)
        );
        assert_eq!(err(64, 4, 0, &mut pool), Some(Error::BadSegmentCount));
        assert_eq!(
            err(64, 4, MAX_SEGMENTS + 1, &mut pool),
            Some(Error::BadSegmentCount)
        );
        // 32 one-line slots do not fit a 20-line buffer.
        assert_eq!(err(64, 32, 2, &mut pool), Some(Error::TooSmall));
        // More segments than the pool has: nothing is kept.
        assert_eq!(
            err(64, 16, BUFS as u32 + 1, &mut pool),
            Some(Error::Exhausted)
        );
        let all: Vec<_> = (0..BUFS).map(|_| pool.alloc_bytes().unwrap()).collect();
        assert_eq!(pool.alloc_bytes().err(), Some(Exhausted));
        all.into_iter().for_each(crate::BufSlot::free);
    }

    #[test]
    fn segment_is_header_then_slots() {
        for cap in [1u32, 4, 16] {
            assert_eq!(
                segment_size(64, cap),
                4 * CACHE_LINE_SIZE as u64 + 64 * cap as u64
            );
        }
    }

    #[test]
    fn control_block_names_the_ring() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 3).unwrap();
        let segs = ring.segs;
        let header0 = segs.header0();
        assert_eq!(
            ring.first_segment(),
            header0.table[0].load(Ordering::Relaxed)
        );
        assert_eq!(header0.claims.load(Ordering::Relaxed), 0);
        for (i, entry) in header0.table.iter().enumerate() {
            let idx = entry.load(Ordering::Relaxed);
            assert_eq!(idx == NO_SEGMENT, i >= 3, "table entry {i}");
            if i < 3 {
                assert!(idx < BUFS as u32, "entry {i} names buffer {idx}");
                // The table entry and the private offset agree.
                assert_eq!(
                    segs.slots[i],
                    idx as usize * BUF + size_of::<SegmentHeader>()
                );
            }
        }
        // Every segment's first line names the ring and itself.
        for seg in 0..3u32 {
            // SAFETY: the header is the front of a live segment.
            let info = unsafe {
                &(*(segs
                    .base
                    .add(segs.slots[seg as usize] - size_of::<SegmentHeader>())
                    as *const SegmentHeader))
                    .info
            };
            assert_eq!(info.magic.load(Ordering::Acquire), MAGIC);
            assert_eq!(info.layout_version.load(Ordering::Relaxed), LAYOUT_VERSION);
            assert_eq!(info.slot_size.load(Ordering::Relaxed), 64);
            assert_eq!(info.seg_capacity.load(Ordering::Relaxed), 4);
            assert_eq!(info.seg_count.load(Ordering::Relaxed), 3);
            assert_eq!(info.seg_num.load(Ordering::Relaxed), seg);
            assert_eq!(info.given.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn one_segment_is_a_ring() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, 4, 1).unwrap());
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        for lap in 0..3u64 {
            send(&mut prod, lap * 4, lap * 4 + 4);
            assert_eq!(prod.reserve_slot_with::<Msg>(|_| false).err(), Some(Full));
            recv(&mut cons, lap * 4, lap * 4 + 4);
            assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
        }
    }

    #[test]
    fn segments_extend_the_ring() {
        // With the consumer idle the producer fills every segment,
        // switching as each is about to be full, and only then
        // reports Full.
        for (cap, count) in [(1u32, 2u32), (4, 3), (16, 4)] {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, cap, count).unwrap());
            let total = (cap * count) as u64;
            send(&mut prod, 0, total);
            assert_eq!(prod.reserve_slot_with::<Msg>(|_| false).err(), Some(Full));
            assert_eq!(free_segments(&prod), 0);
            recv(&mut cons, 0, total);
            assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
            // Every segment but the current one is back.
            assert_eq!(free_segments(&prod).count_ones(), count - 1);
            assert_eq!(free_segments(&prod) & (1 << prod.st.cur), 0);
            assert_eq!(prod.st.cur, cons.st.cur);
        }
    }

    #[test]
    fn segments_cycle_far_past_one_pool() {
        // Many laps through every segment, in bursts that force
        // switches, at depth 1 and larger.
        for (cap, count) in [(1u32, 2u32), (1, 5), (2, 3), (4, 2), (16, 8)] {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, cap, count).unwrap());
            let burst = (cap * count) as u64;
            let mut next = 0u64;
            for round in 0..200u64 {
                // Vary the burst so segments are left mid-lap.
                let n = 1 + (round * 7) % burst;
                send(&mut prod, next, next + n);
                recv(&mut cons, next, next + n);
                next += n;
                assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                assert_eq!(free_segments(&prod).count_ones(), count - 1);
            }
            assert!(next > 10 * burst);
        }
    }

    #[test]
    fn depth_one_switches_every_commit() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, 1, 2).unwrap());
        for i in 0..10u64 {
            let before = prod.st.cur;
            send(&mut prod, i, i + 1);
            assert_ne!(prod.st.cur, before);
            recv(&mut cons, i, i + 1);
            assert_eq!(cons.st.cur, prod.st.cur);
        }
    }

    #[test]
    fn no_free_segment_waits_on_the_current_one() {
        // Two segments of one slot: the second commit finds no
        // free segment and commits plainly, so the producer waits
        // in place until the consumer frees that very slot.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, 1, 2).unwrap());
        send(&mut prod, 0, 2);
        assert_eq!(prod.st.cur, 1);
        assert_eq!(prod.reserve_slot_with::<Msg>(|_| false).err(), Some(Full));
        recv(&mut cons, 0, 1);
        // Segment 0 is back, but the producer still waits on
        // segment 1's slot, not yet read.
        assert_eq!(free_segments(&prod), 1);
        assert_eq!(prod.reserve_slot_with::<Msg>(|_| false).err(), Some(Full));
        recv(&mut cons, 1, 2);
        send(&mut prod, 2, 3);
        assert_eq!(prod.st.cur, 0);
        recv(&mut cons, 2, 3);
    }

    #[test]
    // Dropping the guards is the behavior under test. They have
    // no Drop impl by design (abandon = do nothing).
    #[allow(clippy::drop_non_drop)]
    fn abandoned_guards_publish_nothing() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, 1, 2).unwrap());
        let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
        slot.seq = 99;
        drop(slot);
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        send(&mut prod, 0, 1);
        // An abandoned read re-delivers, the switch with it.
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 0);
        drop(msg);
        recv(&mut cons, 0, 1);
        send(&mut prod, 1, 2);
        recv(&mut cons, 1, 2);
    }

    #[test]
    fn reserve_slot_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, 2, 1).unwrap());
        let mut seen = Vec::new();
        let err = cons
            .reserve_slot_with::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err();
        assert_eq!(err, Some(Empty));
        assert_eq!(seen, [0, 1, 2]);
        send(&mut prod, 0, 2);
        let err = prod.reserve_slot_with::<Msg>(|attempt| attempt < 2).err();
        assert_eq!(err, Some(Full));
        let msg = cons
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with a message available"))
            .unwrap();
        msg.release();
        let slot = prod
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with room available"))
            .unwrap();
        slot.commit();
    }

    #[test]
    fn positions_survive_u32_wrap() {
        // Both sides two commits shy of the u32 wrap in every
        // segment, each segment's seqs claimable for the lap that
        // starts there, then several laps across it.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 3).unwrap();
        let start = u32::MAX - 1;
        for seg in 0..3 {
            for i in 0..4u32 {
                let idx = start.wrapping_add(i);
                ring.segs
                    .seq(seg, idx)
                    .store(seq_of(idx), Ordering::Relaxed);
            }
        }
        let (mut prod, mut cons) = endpoints(&ring);
        for st_pos in [&mut prod.st.pos, &mut cons.st.pos] {
            *st_pos = start;
        }
        prod.st.resume = [start; MAX_SEGMENTS as usize];
        cons.st.resume = [start; MAX_SEGMENTS as usize];
        // Batches of at most the ring's 12, so no send finds Full.
        for (from, to) in [(0u64, 12u64), (12, 24), (24, 30), (30, 41)] {
            send(&mut prod, from, to);
            recv(&mut cons, from, to);
            assert_eq!(free_segments(&prod).count_ones(), 2);
        }
        // The positions did cross the wrap.
        assert!(prod.st.pos < start && cons.st.pos < start);
    }

    /// One cache line of heap backing store, so a `Vec` of them is
    /// a line-aligned region of any size.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
    #[repr(C, align(64))]
    struct Line([u8; CACHE_LINE_SIZE]);

    /// The segment counts and depths the matrix covers: all of
    /// them, or under Miri a corner, its interpreter being slow.
    #[test]
    fn roles_are_claimed_once() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 2).unwrap();
        let prod = ring.producer().unwrap();
        assert_eq!(ring.producer().err(), Some(Error::RoleTaken));
        let cons = ring.consumer().unwrap();
        assert_eq!(ring.consumer().err(), Some(Error::RoleTaken));
        assert_eq!(ring.segs.header0().claims.load(Ordering::Relaxed), 3);
        drop(prod);
        assert_eq!(ring.segs.header0().claims.load(Ordering::Relaxed), 2);
        let mut prod = ring.producer().unwrap();
        drop(cons);
        let mut cons = ring.consumer().unwrap();
        send(&mut prod, 0, 3);
        recv(&mut cons, 0, 3);
    }

    /// A region's one raw pointer and length, for a pool
    /// initialized and then attached over it.
    ///
    /// - Stacked Borrows: the handle `init` returns holds pointers
    ///   under the `&mut` it took, and a write through an attached
    ///   handle, which holds the region's own pointer, invalidates
    ///   them, so a test drops the initializing handle before it
    ///   attaches and never writes through both, the hazard the
    ///   pools' `attach` notes.
    fn region(r: &mut Region) -> (*mut u8, usize) {
        (r.0.as_mut_ptr(), r.0.len())
    }

    /// Initialize the test pool over `base` and run `f` on it,
    /// dropping the handle after.
    fn with_init_pool<R>(base: *mut u8, len: usize, f: impl FnOnce(&mut Pool<'_>) -> R) -> R {
        // SAFETY: base and len are the region, and the slice is
        // the only use of the region while it lives.
        let mut pool = Pool::init(
            unsafe { &mut *core::ptr::slice_from_raw_parts_mut(base, len) },
            BUF as u32,
            BUFS as u32,
        )
        .unwrap();
        f(&mut pool)
    }

    /// Attach a pool handle over `base`.
    fn attach_pool<'a>(base: *mut u8, len: usize) -> Pool<'a> {
        // SAFETY: the same live region, and no attached handle
        // allocates, so no second popper exists.
        unsafe { Pool::attach(base, len) }.unwrap()
    }

    #[test]
    fn attach_joins_the_ring() {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let (first, first2) = with_init_pool(base, len, |pool| {
            let ring = Ring::init(pool, 64, 4, 3).unwrap();
            let ring2 = Ring::init(pool, 64, 4, 2).unwrap();
            (ring.first_segment(), ring2.first_segment())
        });
        // Two attached handles, as two processes would hold.
        let (b1, b2) = (attach_pool(base, len), attach_pool(base, len));
        // SAFETY: the indices came from first_segment of rings over
        // this pool, whose segments are still the rings'.
        let ring_1 = unsafe { Ring::attach(&b1, first) }.unwrap();
        let ring_2 = unsafe { Ring::attach(&b2, first) }.unwrap();
        assert_eq!(ring_2.first_segment(), first);
        assert_eq!(ring_2.segs.slots, ring_1.segs.slots);

        // The producer from one handle, the consumer from the other,
        // and the claims are one word for both.
        let mut prod = ring_1.producer().unwrap();
        assert_eq!(ring_2.producer().err(), Some(Error::RoleTaken));
        let mut cons = ring_2.consumer().unwrap();
        assert_eq!(ring_1.consumer().err(), Some(Error::RoleTaken));
        let mut next = 0u64;
        for burst in [3u64, 9, 12, 5, 12, 12, 7] {
            send(&mut prod, next, next + burst);
            recv(&mut cons, next, next + burst);
            next += burst;
        }
        assert!(prod.switches() > 3 && prod.switches() == cons.switches());
        drop(prod);
        drop(cons);

        // The reverse pairing, on the second ring: a joined endpoint
        // starts at position 0, so a ring already run is not
        // rejoined.
        // SAFETY: as above.
        let ring_1 = unsafe { Ring::attach(&b1, first2) }.unwrap();
        let ring_2 = unsafe { Ring::attach(&b2, first2) }.unwrap();
        let mut prod = ring_2.producer().unwrap();
        let mut cons = ring_1.consumer().unwrap();
        let mut next = 0u64;
        for burst in [5u64, 8, 8, 3, 8] {
            send(&mut prod, next, next + burst);
            recv(&mut cons, next, next + burst);
            next += burst;
        }
        assert!(prod.switches() > 0 && prod.switches() == cons.switches());
    }

    #[test]
    fn attach_rejects_hostile_control_blocks() {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let (first, spare) = with_init_pool(base, len, |pool| {
            let ring = Ring::init(pool, 64, 4, 3).unwrap();
            // A buffer that is no segment: free, its first word the
            // free-stack link.
            let spare = pool.alloc_bytes().unwrap();
            let spare_idx = spare.idx();
            spare.free();
            (ring.first_segment(), spare_idx)
        });
        let b = attach_pool(base, len);
        let attach = |idx| unsafe { Ring::attach(&b, idx) }.err();
        // SAFETY: first names the ring's segment 0.
        let ring = unsafe { Ring::attach(&b, first) }.unwrap();
        let block = ring.segs.header0();

        // Not a buffer of the pool, and a buffer that is no segment.
        assert_eq!(attach(BUFS as u32), Some(Error::BadSegment));
        assert_eq!(attach(spare), Some(Error::BadMagic));

        // A field at a time, restored after each.
        let version = &block.info.layout_version;
        version.store(LAYOUT_VERSION + 1, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadLayoutVersion));
        version.store(LAYOUT_VERSION, Ordering::Relaxed);

        block.info.slot_size.store(63, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSlotSize));
        block.info.slot_size.store(64, Ordering::Relaxed);

        block.info.seg_count.store(0, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSegmentCount));
        block.info.seg_count.store(3, Ordering::Relaxed);

        // A segment the pool's buffers cannot hold.
        block.info.seg_capacity.store(32, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::TooSmall));
        block.info.seg_capacity.store(4, Ordering::Relaxed);

        // Segment 0 claiming to be another segment.
        block.info.seg_num.store(1, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSegment));
        block.info.seg_num.store(0, Ordering::Relaxed);

        // A table naming a buffer outside the pool, one twice, and
        // two whose headers are each other's.
        let second = block.table[1].load(Ordering::Relaxed);
        let third = block.table[2].load(Ordering::Relaxed);
        block.table[1].store(BUFS as u32, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSegment));
        block.table[1].store(first, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSegment));
        block.table[1].store(third, Ordering::Relaxed);
        block.table[2].store(second, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadSegment));
        block.table[1].store(second, Ordering::Relaxed);
        block.table[2].store(third, Ordering::Relaxed);

        // Restored, it attaches.
        assert!(unsafe { Ring::attach(&b, first) }.is_ok());
    }

    fn matrix() -> (Vec<u32>, Vec<u32>) {
        if cfg!(miri) {
            (vec![1, 2, 32], vec![1, 8])
        } else {
            ((1..=MAX_SEGMENTS).collect(), vec![1, 8, 64, 1024])
        }
    }

    /// Run `f` on a ring of `count` segments of `depth` one-line
    /// slots over a pool holding exactly those segments.
    fn with_ring(count: u32, depth: u32, f: impl FnOnce(Producer<'_>, Consumer<'_>)) {
        let buf = segment_size(64, depth);
        let bytes = size_of::<PoolHeader>() as u64 + buf * count as u64;
        let mut store = vec![Line([0; CACHE_LINE_SIZE]); bytes.div_ceil(64) as usize];
        let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), buf as u32, count).unwrap();
        let (prod, cons) = endpoints(&Ring::init(&mut pool, 64, depth, count).unwrap());
        f(prod, cons);
    }

    #[test]
    fn every_count_and_depth_fills_and_drains() {
        // With the consumer idle the producer writes into every
        // segment in turn, switching once between each pair, and
        // the consumer follows it through the same switches.
        let (counts, depths) = matrix();
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |mut prod, mut cons| {
                    let total = (count * depth) as u64;
                    let mut used = 1u32 << prod.segment();
                    for i in 0..total {
                        send(&mut prod, i, i + 1);
                        used |= 1 << prod.segment();
                    }
                    assert_eq!(prod.reserve_slot_with::<Msg>(|_| false).err(), Some(Full));
                    assert_eq!(
                        used.count_ones(),
                        count,
                        "{count} segments at depth {depth}"
                    );
                    assert_eq!(prod.switches(), (count - 1) as u64);
                    recv(&mut cons, 0, total);
                    assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                    assert_eq!(cons.switches(), prod.switches());
                    assert_eq!(cons.segment(), prod.segment());
                    assert_eq!(free_segments(&prod).count_ones(), count - 1);
                });
            }
        }
    }

    #[test]
    fn every_count_and_depth_survives_uneven_bursts() {
        // Bursts of every size up to the ring's capacity leave
        // segments mid-lap, and after each drain both ends agree.
        let (counts, depths) = matrix();
        let rounds = if cfg!(miri) { 5 } else { 40 };
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |mut prod, mut cons| {
                    let capacity = (count * depth) as u64;
                    let mut next = 0u64;
                    for round in 0..rounds {
                        let n = 1 + (round * 7919) % capacity;
                        send(&mut prod, next, next + n);
                        recv(&mut cons, next, next + n);
                        next += n;
                        assert_eq!(cons.switches(), prod.switches());
                        assert_eq!(free_segments(&prod).count_ones(), count - 1);
                    }
                });
            }
        }
    }

    #[test]
    fn every_count_and_depth_streams_across_threads() {
        // A producer and a consumer on their own threads, both
        // spinning: order holds and the switch counts agree.
        let (counts, depths) = matrix();
        let total: u64 = if cfg!(miri) { 50 } else { 10_000 };
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |mut prod, mut cons| {
                    let (sent, seen) = std::thread::scope(|s| {
                        let producer = s.spawn(move || {
                            for i in 0..total {
                                let mut slot =
                                    prod.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                                slot.seq = i;
                                slot.commit();
                            }
                            prod.switches()
                        });
                        let consumer = s.spawn(move || {
                            for i in 0..total {
                                let msg =
                                    cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                                assert_eq!(msg.seq, i);
                                msg.release();
                            }
                            cons.switches()
                        });
                        (producer.join().unwrap(), consumer.join().unwrap())
                    });
                    assert_eq!(sent, seen, "{count} segments at depth {depth}");
                    if depth == 1 && count > 1 {
                        // Every commit at depth 1 finds its one
                        // slot taken, so it switches when it can.
                        assert!(sent > 0);
                    }
                });
            }
        }
    }

    /// The two-thread stream through `count` segments of depth
    /// `cap`, `total` messages, spinning on both ends.
    fn threaded(cap: u32, count: u32, total: u64) {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = endpoints(&Ring::init(&mut pool, 64, cap, count).unwrap());
        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..total {
                    let mut slot = prod.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                    slot.seq = i;
                    slot.val = i * 3;
                    slot.commit();
                }
            });
            s.spawn(move || {
                for i in 0..total {
                    let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                    assert_eq!(msg.seq, i);
                    assert_eq!(msg.val, i * 3);
                    msg.release();
                }
            });
        });
    }

    #[test]
    fn threaded_segments() {
        // Reduced under Miri: interpreted spin loops are slow,
        // and its scheduler explores interleavings at any count.
        const TOTAL: u64 = if cfg!(miri) { 200 } else { 100_000 };
        for (cap, count) in [(1u32, 2u32), (1, 3), (2, 2), (4, 3), (16, 2)] {
            threaded(cap, count, TOTAL);
        }
    }
}
