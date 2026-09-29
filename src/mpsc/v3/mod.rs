//! MPSC v3 ring: v2's ring of segments over a ring that describes
//! itself in the region, so a second process can attach to it,
//! per the design doc's "MPSC v3: attachable segments with counted
//! roles" section. v2 stays as built to measure against.
//!
//! - What v3 adds to v2 so far: a control block at the front of
//!   segment 0 (magic, layout version, geometry, the roles word,
//!   the consumer's checkpoint, and the table of every segment's
//!   pool buffer index), [`MpscRing::attach`] from a pool,
//!   [`MpscRing::first_segment`], and the roles claimed by count,
//!   [`MpscRing::claim_producer`] and [`MpscRing::claim_consumer`].
//!   [`MpscRing::release_ring`] gives a ring no role holds back to
//!   the pool, and the segment handling is a mode type chosen at
//!   compile time, [`Single`] or [`Multi`]. A full producer or an
//!   empty consumer can sleep until the other side acts, through a
//!   [`Wake`] type, [`NoWake`] by default.
//! - Modes: [`Multi`] is v2's switching over up to
//!   [`MAX_SEGMENTS`] segments. [`Single`] is one segment, where a
//!   full ring goes straight to the policy and the consumer never
//!   loads a seal, so its paths compile without the switch. The
//!   mode is in the control block, and an `attach` of the other
//!   mode is [`Error::BadMode`].
//! - Roles: one consumer and up to the ring's most producers, each
//!   claimed and released by one CAS on the roles word. A producer
//!   keeps no state of its own, and the consumer checkpoints its
//!   state at `release`, so a later claim, in any process,
//!   continues where it stopped. Neither endpoint has a `Drop`.
//! - Segments: up to [`MAX_SEGMENTS`] taken from the application's
//!   [`Pool`] at [`MpscRing::init`], each a header of seven lines
//!   then `seg_capacity` slots opening with the seq word.
//! - Words: 32 bits everywhere. A position is [`SEQ_BITS`] wide,
//!   and the slot word holds v1's values in those bits, claimable
//!   at `pos`, committed at `pos + M + 1`, released at `pos + M`.
//!   There is no tombstone: a producer that panics inside its fill
//!   leaves its slot claimed, as one killed there does, and the
//!   ring is recovered by a restart. The claim word packs the
//!   current segment over the next position, and a seal packs
//!   MOVED, the next segment, and the end position the same way.
//! - The claim word is the seal: every producer claims as v1
//!   claims, and a stale view fails the CAS, so no claim lands in
//!   a segment the ring has left.
//! - A producer at a full segment takes a free one by setting its
//!   bit in the in-use word, moves the claim word to it, and seals
//!   the old segment's header. The consumer reads the seal only when a
//!   slot is not committed, so its fast path
//!   is v1's one load.
//! - Waiting: a producer's `send` asks its `SendPolicy` what to do at a full ring, and the policy
//!   may sleep through `Room`, as `send_spin_sleep` does, where `send_spin` only spins. The
//!   consumer's `reserve_slot_wait` sleeps between attempts where `reserve_slot_with` spins or
//!   gives up, and calls the same policy after each wake.
//!   - The consumer sleeps on the claim word, whose bit 31 is its
//!     waiting flag. It sets the flag and sleeps only while the
//!     word, flag aside, names its own segment and position, so no
//!     slot is claimed past what it read. A producer's claim CAS
//!     returns the flag at no cost, and the producer wakes the
//!     consumer after its commit.
//!   - A producer at a full ring counts itself into the producers'
//!     waiting word, looks again, and sleeps on the wake sequence
//!     word. The consumer checks the count behind a SeqCst fence
//!     at every half segment of releases, at each segment it gives
//!     back, and before it sleeps itself, and bumps the sequence and
//!     wakes them all: a fence every half segment, not every
//!     message. A producer sleeps only on a full ring, and draining
//!     a full segment crosses a half-segment mark, so the checks
//!     miss no sleeper.
//!   - With [`NoWake`] every check folds away and a wait polls.
//! - Gated with the rest of `mpsc` on `target_has_atomic = "32"`.

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::spsc::v3::{check_body_type, seq_of, validate_geometry};
use crate::wake::{NoWake, Wake};
use crate::{CACHE_LINE_SIZE, CacheAligned, Error, Pool};

mod consumer;
mod producer;

pub use crate::spsc::v2::SLOT_HEADER_BYTES;
pub use crate::spsc::v3::{MAX_SEG_CAPACITY, MAX_SEGMENTS, SEQ_BITS};
pub use consumer::{MpscConsumer, MpscReadSlot};
pub use producer::{MpscProducer, Room, SendPolicy};

/// The position's bits within a word.
const SEQ_MASK: u32 = (1 << SEQ_BITS) - 1;

/// Where a segment number sits in a word, above the position.
const SEG_SHIFT: u32 = SEQ_BITS;

/// A segment number, once shifted down.
const SEG_MASK: u32 = MAX_SEGMENTS - 1;

/// Set in a seal word: the segment ended at the word's position,
/// and the ring went on in the word's segment.
const MOVED: u32 = 1 << 31;

/// Set in the claim word while the consumer sleeps or is about to:
/// the producer whose claim CAS sees it wakes the consumer after
/// its commit.
const WAITING: u32 = 1 << 31;

const _: () = assert!(SEG_SHIFT + MAX_SEGMENTS.trailing_zeros() <= 31);

/// Layout marker written by [`MpscRing::init`] into every
/// segment's header, distinct from the other rings' and the
/// pools'.
const MAGIC: u32 = 0x5A43_4D33; // "ZCM3"

/// Bumped on any change to the segment layout.
const LAYOUT_VERSION: u32 = 1;

/// A table entry naming no segment.
const NO_SEGMENT: u32 = u32::MAX;

/// Set in the roles word once the ring is released: no role can
/// be claimed.
const CLOSED: u32 = 1 << 31;

/// Set in the roles word while the consumer role is held.
const CONSUMER: u32 = 1 << 30;

/// The roles word's count of producers held.
const PRODUCERS: u32 = 0xFFFF;

/// How a ring handles its segments, chosen at compile time: the
/// type parameter of [`MpscRing`] and its endpoints.
///
/// - Sealed: [`Single`] and [`Multi`] are the two modes.
pub trait Mode: sealed::Sealed + 'static {
    /// Whether the ring switches segments, the one question the
    /// message paths ask, answered at compile time.
    const MULTI: bool;
    /// The mode's value in the control block.
    const CODE: u32;
}

/// One segment: a full ring goes straight to the policy and the
/// consumer never loads a seal.
pub struct Single;

/// Up to [`MAX_SEGMENTS`] segments, v2's switching: a producer at
/// a full segment takes a free one.
pub struct Multi;

impl Mode for Single {
    const MULTI: bool = false;
    const CODE: u32 = 1;
}

impl Mode for Multi {
    const MULTI: bool = true;
    const CODE: u32 = 2;
}

mod sealed {
    /// Keeps [`Mode`](super::Mode) to the crate's two modes.
    pub trait Sealed {}
    impl Sealed for super::Single {}
    impl Sealed for super::Multi {}
}

/// The word naming segment `seg` and position `pos`.
#[inline]
fn word(seg: u32, pos: u32) -> u32 {
    (seg << SEG_SHIFT) | seq_of(pos)
}

/// The segment a word names.
#[inline]
fn word_seg(w: u32) -> u32 {
    (w >> SEG_SHIFT) & SEG_MASK
}

/// The position a word names.
#[inline]
fn word_pos(w: u32) -> u32 {
    w & SEQ_MASK
}

/// The first line of every segment: which ring it belongs to,
/// written by [`MpscRing::init`] and read back by `attach`.
///
/// - Every field is a `u32` atomic, so a process reads the line
///   with the ordering the magic's Acquire gives it.
#[repr(C)]
struct Info {
    magic: AtomicU32,
    layout_version: AtomicU32,
    slot_size: AtomicU32,
    seg_capacity: AtomicU32,
    seg_count: AtomicU32,
    /// This segment's number in the ring.
    seg_num: AtomicU32,
    /// The ring's [`Mode::CODE`].
    mode: AtomicU32,
    /// The most producers the ring allows, segment 0's.
    max_producers: AtomicU32,
    /// The consumer's resume position in this segment, written by
    /// its `release`.
    cons_resume: AtomicU32,
}

/// The claims line of segment 0: who holds the roles, and the
/// consumer's checkpoint.
///
/// - The roles word is [`CLOSED`], [`CONSUMER`], and the count of
///   producers held under [`PRODUCERS`], so a claim, a release, and
///   the ring's release are each one CAS on one word.
/// - The consumer's segment and position, with each segment's
///   resume position in its info line, are what the consumer's
///   `release` leaves for the next claim.
/// - The producers' waiting count and wake sequence, read by the
///   consumer every half segment of releases when its [`Wake`]
///   wakes, and written by a producer about to sleep.
/// - Nothing else on the message path reads or writes the line.
#[repr(C)]
struct Claims {
    /// The roles word.
    roles: AtomicU32,
    /// The consumer's segment, written by its `release`.
    cons_cur: AtomicU32,
    /// The consumer's position in its segment, written by its
    /// `release`.
    cons_pos: AtomicU32,
    /// Producers asleep, or about to be, on a full ring.
    prod_waiters: AtomicU32,
    /// The producers' wake sequence, bumped by the consumer before
    /// it wakes them, the word they sleep on.
    prod_wake: AtomicU32,
}

/// The seven lines at the front of every segment: the ring's
/// control block in segment 0, and the same layout in the others
/// so every segment's slots start at one offset.
///
/// - Only segment 0's `claim`, `in_use`, `claims`, and `table`
///   lines are used. The claim word is the contended line and has it alone.
/// - The pool buffer index of each segment is in the table, so a
///   process holding the pool and segment 0's index finds every
///   segment.
#[repr(C)]
struct SegmentHeader {
    /// Line 0: the ring's identity and geometry.
    info: CacheAligned<Info>,
    /// Line 1: the seal: MOVED, the next segment, and the end
    /// position, stored by the producer that moved the ring on,
    /// cleared by the producer that next takes this segment, and
    /// read by the consumer when a slot is not committed.
    seal: CacheAligned<AtomicU32>,
    /// Line 2, segment 0: the current segment and the next
    /// position to claim, CAS-claimed by every producer.
    claim: CacheAligned<AtomicU32>,
    /// Line 3, segment 0: the in-use word and the switch count,
    /// both touched on the switch path only.
    in_use: CacheAligned<InUseLine>,
    /// Line 4, segment 0: the roles and the consumer's checkpoint.
    claims: CacheAligned<Claims>,
    /// Lines 5 and 6, segment 0: the pool buffer index of segment
    /// `i` at `table[i]`, [`NO_SEGMENT`] past `seg_count`.
    table: CacheAligned<[AtomicU32; MAX_SEGMENTS as usize]>,
}

/// The in-use line's two words.
#[repr(C)]
struct InUseLine {
    /// One bit per segment, set while the segment is in use. A
    /// producer takes a free segment with a `fetch_or` that
    /// succeeds only where the bit was clear, and the consumer
    /// gives one back with a `fetch_and`. v3's pair of parity
    /// words is sound for one producer and not for several: two
    /// producers with views one store apart can agree a segment
    /// in use is free.
    in_use: AtomicU32,
    /// The ring's count of producer switches.
    switches: AtomicU32,
}

const _: () = assert!(size_of::<SegmentHeader>() == 7 * CACHE_LINE_SIZE);

/// Bytes a segment needs: its header lines, then the slots.
///
/// - The pool handed to [`MpscRing::init`] needs buffers at least
///   this large.
/// - Computed in u64 for the same 32-bit wrap reason as the
///   rings' region sizes.
pub fn segment_size(slot_size: u32, seg_capacity: u32) -> u64 {
    size_of::<SegmentHeader>() as u64 + slot_size as u64 * seg_capacity as u64
}

/// A ring's geometry and its segments' addresses in this process,
/// the state every endpoint starts from.
///
/// - Built from the table of pool buffer indices by `init` and
///   `attach` alike, so each process holds its own addresses for
///   the same segments.
/// - Borrowed on every path, never copied: v3's fast-path
///   finding.
#[derive(Clone, Copy)]
struct Segments {
    /// The slot array of each segment, `seg_count` of them.
    slots: [*mut u8; MAX_SEGMENTS as usize],
    /// The header of each segment, `seg_count` of them.
    headers: [*const SegmentHeader; MAX_SEGMENTS as usize],
    /// Slot size N in bytes.
    slot_size: u32,
    /// Segment depth M, a power of two.
    capacity: u32,
    /// Slot-position mask (`capacity - 1`).
    mask: u32,
    /// Segments in the ring.
    seg_count: u32,
    /// `capacity + 1`, precomputed: the commit value is
    /// `pos + M + 1`, one add on the hot path.
    commit_add: u32,
    /// Releases between the consumer's checks for sleeping
    /// producers, less one: half a segment, at least one.
    wake_mask: u32,
}

impl Segments {
    /// The addresses of the segments `indices` names, in the pool
    /// whose buffer array is at `base`, built the same way by
    /// `init` and `attach`.
    fn load(
        base: *mut u8,
        buf_size: usize,
        indices: &[u32; MAX_SEGMENTS as usize],
        slot_size: u32,
        seg_capacity: u32,
        seg_count: u32,
    ) -> Self {
        let mut slots = [core::ptr::null_mut(); MAX_SEGMENTS as usize];
        let mut headers = [core::ptr::null(); MAX_SEGMENTS as usize];
        for seg in 0..seg_count as usize {
            // SAFETY: every index was handed out by the pool or
            // validated against its count, so the buffer is inside
            // the buffer array, and it holds a segment_size segment.
            unsafe {
                let seg_base = base.add(indices[seg] as usize * buf_size);
                slots[seg] = seg_base.add(size_of::<SegmentHeader>());
                headers[seg] = seg_base as *const SegmentHeader;
            }
        }
        Segments {
            slots,
            headers,
            slot_size,
            capacity: seg_capacity,
            mask: seg_capacity - 1,
            seg_count,
            commit_add: seg_capacity + 1,
            wake_mask: (seg_capacity / 2).max(1) - 1,
        }
    }

    /// The seq word of the slot at position `idx` in segment
    /// `seg`.
    #[inline]
    fn seq(&self, seg: u32, idx: u32) -> &AtomicU32 {
        let slot = crate::slot_ptr(self.slots[seg as usize], idx, self.mask, self.slot_size);
        // SAFETY: seg < seg_count and the slot is in bounds of
        // that segment's buffer, line-aligned, so its first word
        // is an aligned AtomicU32, shared atomic state by design.
        unsafe { &*(slot as *const AtomicU32) }
    }

    /// The body of the slot at position `idx` in segment `seg`,
    /// behind its [`SLOT_HEADER_BYTES`].
    #[inline]
    fn body(&self, seg: u32, idx: u32) -> *mut u8 {
        let slot = crate::slot_ptr(self.slots[seg as usize], idx, self.mask, self.slot_size);
        // SAFETY: SLOT_HEADER_BYTES < CACHE_LINE_SIZE <= slot_size,
        // so the body starts inside the slot.
        unsafe { slot.add(SLOT_HEADER_BYTES) }
    }

    /// Segment `seg`'s header.
    #[inline]
    fn header(&self, seg: u32) -> &SegmentHeader {
        // SAFETY: seg < seg_count, and the header is the front of
        // a buffer that lives as long as the pool region the ring
        // borrows from, every field atomic.
        unsafe { &*self.headers[seg as usize] }
    }

    /// Segment `seg`'s seal word.
    #[inline]
    fn seal(&self, seg: u32) -> &AtomicU32 {
        &self.header(seg).seal
    }

    /// The claim word.
    #[inline]
    fn claim(&self) -> &AtomicU32 {
        &self.header(0).claim
    }

    /// The in-use word.
    #[inline]
    fn in_use(&self) -> &AtomicU32 {
        &self.header(0).in_use.in_use
    }

    /// The ring's count of producer switches.
    #[inline]
    fn switches(&self) -> &AtomicU32 {
        &self.header(0).in_use.switches
    }

    /// The claims line.
    fn claims(&self) -> &Claims {
        &self.header(0).claims
    }

    /// The roles word.
    fn roles(&self) -> &AtomicU32 {
        &self.claims().roles
    }

    /// The consumer's state from the region, the checkpoint the
    /// last consumer's `release` wrote, or the start of a ring
    /// never consumed, where every word is zero.
    fn load_consumer(&self) -> Result<Checkpoint, Error> {
        let c = self.claims();
        let cur = c.cons_cur.load(Ordering::Acquire);
        if cur >= self.seg_count {
            return Err(Error::BadCheckpoint);
        }
        let mut resume = [0; MAX_SEGMENTS as usize];
        for seg in 0..self.seg_count {
            resume[seg as usize] = self.header(seg).info.cons_resume.load(Ordering::Acquire);
        }
        Ok(Checkpoint {
            cur,
            pos: c.cons_pos.load(Ordering::Acquire),
            resume,
        })
    }

    /// Write the consumer's state for the next claim, ahead of the
    /// release of its role.
    fn store_consumer(&self, cp: &Checkpoint) {
        for seg in 0..self.seg_count {
            self.header(seg)
                .info
                .cons_resume
                .store(cp.resume[seg as usize], Ordering::Relaxed);
        }
        let c = self.claims();
        c.cons_cur.store(cp.cur, Ordering::Relaxed);
        c.cons_pos.store(cp.pos, Ordering::Relaxed);
    }

    /// Wake the producers asleep on a full ring, if any.
    ///
    /// - The fence orders this side's releases before the count's
    ///   load, against a producer that counts itself in and then
    ///   looks at the ring: either the producer sees the release,
    ///   or this sees the producer.
    #[cold]
    #[inline(never)]
    fn wake_producers<W: Wake>(&self) {
        core::sync::atomic::fence(Ordering::SeqCst);
        let c = self.claims();
        if c.prod_waiters.load(Ordering::Relaxed) != 0 {
            c.prod_wake.fetch_add(1, Ordering::Release);
            W::wake(&c.prod_wake);
        }
    }

    /// Every segment's bit.
    #[inline]
    fn all(&self) -> u32 {
        if self.seg_count == MAX_SEGMENTS {
            u32::MAX
        } else {
            (1 << self.seg_count) - 1
        }
    }
}

/// The consumer's state, what its `release` leaves and a claim
/// loads.
pub(crate) struct Checkpoint {
    /// The segment being read.
    pub(crate) cur: u32,
    /// Position in `cur`.
    pub(crate) pos: u32,
    /// Where the consumer left each segment.
    pub(crate) resume: [u32; MAX_SEGMENTS as usize],
}

/// A ring of segments over the application's pool, its roles
/// claimed with [`MpscRing::claim_producer`] and
/// [`MpscRing::claim_consumer`], its segments handled as mode `M`
/// has them, [`Multi`] by default, and its waits slept by `W`,
/// [`NoWake`] by default.
pub struct MpscRing<'a, M: Mode = Multi, W: Wake = NoWake> {
    /// Geometry and segment addresses.
    segs: Segments,
    /// The pool buffer index of segment 0, where the control
    /// block is.
    first_segment: u32,
    _region: PhantomData<(&'a [u8], M, W)>,
}

impl<'a, M: Mode, W: Wake> MpscRing<'a, M, W> {
    /// Take `seg_count` segments from `pool` and initialize each
    /// as an empty ring of `seg_capacity` slots of `slot_size`
    /// bytes.
    ///
    /// - `slot_size`: N bytes per slot, a [`CACHE_LINE_SIZE`]
    ///   multiple, of which [`SLOT_HEADER_BYTES`] are the crate's.
    /// - `seg_capacity`: M slots per segment, a power of two up to
    ///   [`MAX_SEG_CAPACITY`], 1 included.
    /// - `seg_count`: 1 to [`MAX_SEGMENTS`], and exactly 1 for
    ///   [`Single`], else [`Error::BadSegmentCount`].
    /// - The pool's buffers must hold [`segment_size`], else
    ///   [`Error::TooSmall`]. A pool without `seg_count` free
    ///   buffers gives [`Error::Exhausted`], with the buffers
    ///   taken so far freed.
    /// - The pool is borrowed only here. The segments stay
    ///   allocated for the life of the pool region, as a
    ///   [`BufSlot`](crate::BufSlot) dropped without `free` does.
    /// - Every segment's header names the ring, and segment 0's
    ///   holds the table of segments, so a process holding the
    ///   pool and [`first_segment`](MpscRing::first_segment) can
    ///   find the ring.
    /// - Up to `u16::MAX` producers, the most the roles word counts.
    ///   [`init_with_max_producers`](MpscRing::init_with_max_producers)
    ///   sets fewer.
    pub fn init(
        pool: &mut Pool<'a>,
        slot_size: u32,
        seg_capacity: u32,
        seg_count: u32,
    ) -> Result<Self, Error> {
        Self::init_with_max_producers(pool, slot_size, seg_capacity, seg_count, u16::MAX)
    }

    /// [`init`](MpscRing::init) with at most `max_producers`
    /// producers held at once, `0` being
    /// [`Error::BadMaxProducers`].
    pub fn init_with_max_producers(
        pool: &mut Pool<'a>,
        slot_size: u32,
        seg_capacity: u32,
        seg_count: u32,
        max_producers: u16,
    ) -> Result<Self, Error> {
        if max_producers == 0 {
            return Err(Error::BadMaxProducers);
        }
        validate_geometry(slot_size, seg_capacity, seg_count)?;
        if !M::MULTI && seg_count != 1 {
            return Err(Error::BadSegmentCount);
        }
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
            // SAFETY: the index names a buffer of at least
            // segment_size bytes the pool just handed out,
            // line-aligned, and nothing else can reach it until
            // a role is claimed: the header lines and each slot's
            // header are ours to write.
            unsafe {
                let seg_base = base.add(indices[seg] as usize * buf_size);
                core::ptr::write_bytes(seg_base, 0, size_of::<SegmentHeader>());
                let header = &*(seg_base as *const SegmentHeader);
                let info = &header.info;
                info.layout_version.store(LAYOUT_VERSION, Ordering::Relaxed);
                info.slot_size.store(slot_size, Ordering::Relaxed);
                info.seg_capacity.store(seg_capacity, Ordering::Relaxed);
                info.seg_count.store(seg_count, Ordering::Relaxed);
                info.seg_num.store(seg as u32, Ordering::Relaxed);
                info.mode.store(M::CODE, Ordering::Relaxed);
                if seg == 0 {
                    info.max_producers
                        .store(max_producers as u32, Ordering::Relaxed);
                    for (entry, &idx) in header.table.iter().zip(indices.iter()) {
                        entry.store(idx, Ordering::Relaxed);
                    }
                    // The ring starts in segment 0 at position 0,
                    // segment 0 in use.
                    header.claim.store(word(0, 0), Ordering::Relaxed);
                    header.in_use.in_use.store(1, Ordering::Relaxed);
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
                info.magic.store(MAGIC, Ordering::Release);
            }
        }
        Ok(MpscRing {
            segs: Segments::load(base, buf_size, &indices, slot_size, seg_capacity, seg_count),
            first_segment: indices[0],
            _region: PhantomData,
        })
    }

    /// Join a ring another process (or an earlier call) initialized
    /// over `pool`, from the pool buffer index of its segment 0.
    ///
    /// - Reads the control block, checks every field and every
    ///   table entry against the pool's geometry, and every
    ///   segment's own header against the block, so a hostile
    ///   region is an `Err`, never an out-of-bounds access.
    /// - A ring built for the other mode is [`Error::BadMode`].
    ///
    /// # Safety
    ///
    /// - `first_segment` came from
    ///   [`first_segment`](MpscRing::first_segment) of a ring
    ///   initialized over this pool's region, and its segments are
    ///   still the ring's: validation cannot tell a ring's segment
    ///   from a buffer since freed and reused, and the ring writes
    ///   seq words into every segment it is told it has.
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
        if block.info.mode.load(Ordering::Relaxed) != M::CODE {
            return Err(Error::BadMode);
        }
        let slot_size = block.info.slot_size.load(Ordering::Relaxed);
        let seg_capacity = block.info.seg_capacity.load(Ordering::Relaxed);
        let seg_count = block.info.seg_count.load(Ordering::Relaxed);
        validate_geometry(slot_size, seg_capacity, seg_count)?;
        if !M::MULTI && seg_count != 1 {
            return Err(Error::BadSegmentCount);
        }
        if (buf_size as u64) < segment_size(slot_size, seg_capacity) {
            return Err(Error::TooSmall);
        }
        if block.info.seg_num.load(Ordering::Relaxed) != 0 {
            return Err(Error::BadSegment);
        }
        let max_producers = block.info.max_producers.load(Ordering::Relaxed);
        if max_producers == 0 || max_producers > PRODUCERS {
            return Err(Error::BadMaxProducers);
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
                && info.seg_num.load(Ordering::Relaxed) == seg as u32
                && info.mode.load(Ordering::Relaxed) == M::CODE;
            if !agrees {
                return Err(Error::BadSegment);
            }
        }
        Ok(MpscRing {
            segs: Segments::load(base, buf_size, &indices, slot_size, seg_capacity, seg_count),
            first_segment,
            _region: PhantomData,
        })
    }

    /// The pool buffer index of segment 0, where the ring's
    /// control block is: what a process hands to another so it
    /// can find the ring in the same pool.
    pub fn first_segment(&self) -> u32 {
        self.first_segment
    }

    /// Release the ring: close it and give its segments back to
    /// `pool`, the pool it was initialized or attached over.
    ///
    /// - Only when no role is held: one CAS on the roles word from
    ///   no role held to closed, so no claim can land in the ring
    ///   while it is released. A role held is [`Error::RingInUse`],
    ///   and a ring already released is [`Error::RingClosed`]. On
    ///   either the ring is unchanged, and
    ///   [`attach`](MpscRing::attach) gives a handle back.
    /// - Every segment's magic is cleared before its buffer is
    ///   freed, so a later `attach` is [`Error::BadMagic`].
    /// - Anyone holding the ring may call it, in any process, since
    ///   any process may free to a pool. When is the creator's call.
    /// - A `pool` over another region is [`Error::BadSegment`], with
    ///   the ring left as it was.
    /// - Other handles to the ring, in this process or another, must
    ///   not be used after: a claim through one reads a closed ring
    ///   until the pool hands its segments out again, and then
    ///   whatever they hold.
    pub fn release_ring(self, pool: &Pool<'a>) -> Result<(), Error> {
        let segs = &self.segs;
        let buf_size = pool.buf_size() as usize;
        let same = self.first_segment < pool.buf_count()
            && core::ptr::eq(
                pool.bufs_ptr()
                    .wrapping_add(self.first_segment as usize * buf_size),
                segs.headers[0] as *mut u8,
            );
        if !same {
            return Err(Error::BadSegment);
        }
        // AcqRel: the release sees everything every role's last
        // release left, and a claim that loses to it sees closed.
        if let Err(r) =
            segs.roles()
                .compare_exchange(0, CLOSED, Ordering::AcqRel, Ordering::Acquire)
        {
            return Err(if r & CLOSED != 0 {
                Error::RingClosed
            } else {
                Error::RingInUse
            });
        }
        let mut indices = [NO_SEGMENT; MAX_SEGMENTS as usize];
        for (seg, idx) in indices.iter_mut().enumerate().take(segs.seg_count as usize) {
            *idx = segs.header(0).table[seg].load(Ordering::Relaxed);
        }
        for seg in 0..segs.seg_count {
            segs.header(seg).info.magic.store(0, Ordering::Release);
        }
        let view = pool.view();
        for &idx in indices.iter().take(segs.seg_count as usize) {
            // SAFETY: the table's indices were handed out by the pool
            // at init or validated against its count at attach, each
            // once, the ring has held them since, and the closed
            // roles word keeps any other handle from claiming, so
            // each buffer is freed once.
            unsafe { view.slot_from_idx::<[u8]>(idx) }.free();
        }
        Ok(())
    }

    /// Claim a producer role.
    ///
    /// - One CAS on the roles word counts the producer in, so any
    ///   number up to the ring's most may hold one at once, in this
    ///   process and others. At the most it is
    ///   [`Error::RoleTaken`], and on a released ring
    ///   [`Error::RingClosed`].
    /// - A producer keeps no state of its own, each send claiming
    ///   its slot on the shared claim word, so producers come and go
    ///   freely.
    /// - The role stays held until [`MpscProducer::release`]:
    ///   dropping the endpoint writes nothing.
    pub fn claim_producer(&self) -> Result<MpscProducer<'a, M, W>, Error> {
        let roles = self.segs.roles();
        let max = self
            .segs
            .header(0)
            .info
            .max_producers
            .load(Ordering::Relaxed);
        let mut r = roles.load(Ordering::Acquire);
        loop {
            if r & CLOSED != 0 {
                return Err(Error::RingClosed);
            }
            if r & PRODUCERS >= max {
                return Err(Error::RoleTaken);
            }
            // AcqRel: a claim sees the ring as the last release of
            // any role left it.
            match roles.compare_exchange_weak(r, r + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Ok(MpscProducer::new(self.segs)),
                Err(actual) => r = actual,
            }
        }
    }

    /// Claim the consumer role.
    ///
    /// - One CAS on the roles word, so the role held anywhere is
    ///   [`Error::RoleTaken`], and on a released ring
    ///   [`Error::RingClosed`].
    /// - The consumer continues from the checkpoint the last
    ///   consumer's [`release`](MpscConsumer::release) wrote, in
    ///   this process or another, exactly where it stopped, and a
    ///   ring never consumed starts at its start.
    /// - A checkpoint that names no segment of the ring is
    ///   [`Error::BadCheckpoint`], with the role left free.
    /// - The role stays held until [`MpscConsumer::release`]:
    ///   dropping the endpoint writes nothing.
    pub fn claim_consumer(&self) -> Result<MpscConsumer<'a, M, W>, Error> {
        let roles = self.segs.roles();
        let mut r = roles.load(Ordering::Acquire);
        loop {
            if r & CLOSED != 0 {
                return Err(Error::RingClosed);
            }
            if r & CONSUMER != 0 {
                return Err(Error::RoleTaken);
            }
            // AcqRel: the claim sees the checkpoint the last
            // consumer's release wrote.
            match roles.compare_exchange_weak(r, r | CONSUMER, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(actual) => r = actual,
            }
        }
        match self.segs.load_consumer() {
            Ok(cp) => Ok(MpscConsumer::resume(self.segs, cp)),
            Err(e) => {
                roles.fetch_and(!CONSUMER, Ordering::Release);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Empty, Exhausted, Full, PoolHeader, Ticks};
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message, two words so a torn write would be visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// Test pool buffer: a segment of up to 16 one-line slots
    /// behind its seven header lines.
    const BUF: usize = 7 * CACHE_LINE_SIZE + 16 * CACHE_LINE_SIZE;

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
    fn send<M: Mode, W: Wake>(prod: &MpscProducer<'_, M, W>, from: u64, to: u64) {
        for i in from..to {
            prod.send::<Msg>(
                |_| false,
                |m| {
                    m.seq = i;
                    m.val = i * 10;
                },
            )
            .unwrap();
        }
    }

    /// Receive `from..to` in order.
    fn recv<M: Mode, W: Wake>(cons: &mut MpscConsumer<'_, M, W>, from: u64, to: u64) {
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

    /// Claim both roles of a fresh ring, as an in-process caller
    /// does.
    fn endpoints<'a, M: Mode, W: Wake>(
        ring: &MpscRing<'a, M, W>,
    ) -> (MpscProducer<'a, M, W>, MpscConsumer<'a, M, W>) {
        (
            ring.claim_producer().unwrap(),
            ring.claim_consumer().unwrap(),
        )
    }

    /// `struct SleepThen` is a policy that sleeps through [`Room`] at each full look, then asks its
    /// closure whether to look again, as the old `send_wait` does.
    struct SleepThen<F>(F);

    impl<F: FnMut(u32) -> bool> SendPolicy for SleepThen<F> {
        fn on_full(&mut self, attempt: u32, room: &Room<'_>) -> bool {
            room.sleep();
            (self.0)(attempt)
        }
    }

    /// `struct LostThen` is a policy that spins at a full ring and hands each lost slot to its
    /// closure.
    struct LostThen<L>(L);

    impl<L: FnMut(u32)> SendPolicy for LostThen<L> {
        fn on_full(&mut self, attempt: u32, _room: &Room<'_>) -> bool {
            crate::policy::spin(attempt)
        }

        fn on_lost(&mut self, lost: u32) {
            (self.0)(lost)
        }
    }

    /// Segments free by the in-use word.
    fn free_segments<M: Mode>(prod: &MpscProducer<'_, M>) -> u32 {
        let segs = &prod.segs;
        !segs.in_use().load(Ordering::Acquire) & segs.all()
    }

    #[test]
    fn init_rejects_bad_geometry() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let err = |slot, cap, count, pool: &mut Pool<'_>| {
            MpscRing::<Multi>::init(pool, slot, cap, count).err()
        };
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
        // 32 one-line slots do not fit a 23-line buffer.
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
                7 * CACHE_LINE_SIZE as u64 + 64 * cap as u64
            );
        }
    }

    #[test]
    fn control_block_names_the_ring() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, 4, 3).unwrap();
        let segs = ring.segs;
        let header0 = segs.header(0);
        assert_eq!(
            ring.first_segment(),
            header0.table[0].load(Ordering::Relaxed)
        );
        assert_eq!(segs.claim().load(Ordering::Relaxed), word(0, 0));
        assert_eq!(segs.in_use().load(Ordering::Relaxed), 1);
        // No role held, the consumer's checkpoint the ring's start,
        // and the most producers the default.
        let c = segs.claims();
        for w in [&c.roles, &c.cons_cur, &c.cons_pos] {
            assert_eq!(w.load(Ordering::Relaxed), 0);
        }
        assert_eq!(
            header0.info.max_producers.load(Ordering::Relaxed),
            u16::MAX as u32
        );
        let base = pool.bufs_ptr();
        for (i, entry) in header0.table.iter().enumerate() {
            let idx = entry.load(Ordering::Relaxed);
            assert_eq!(idx == NO_SEGMENT, i >= 3, "table entry {i}");
            if i < 3 {
                assert!(idx < BUFS as u32, "entry {i} names buffer {idx}");
                // The table entry and the private address agree.
                assert_eq!(
                    segs.headers[i],
                    base.wrapping_add(idx as usize * BUF) as *const _
                );
            }
        }
        // Every segment's first line names the ring and itself.
        for seg in 0..3u32 {
            let info = &segs.header(seg).info;
            assert_eq!(info.magic.load(Ordering::Acquire), MAGIC);
            assert_eq!(info.layout_version.load(Ordering::Relaxed), LAYOUT_VERSION);
            assert_eq!(info.slot_size.load(Ordering::Relaxed), 64);
            assert_eq!(info.seg_capacity.load(Ordering::Relaxed), 4);
            assert_eq!(info.seg_count.load(Ordering::Relaxed), 3);
            assert_eq!(info.seg_num.load(Ordering::Relaxed), seg);
            assert_eq!(info.cons_resume.load(Ordering::Relaxed), 0);
        }
    }

    /// The test region's base and length, for the attach tests.
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
        let first = with_init_pool(base, len, |pool| {
            MpscRing::<Multi>::init(pool, 64, 4, 3)
                .unwrap()
                .first_segment()
        });
        // Two attached handles, as two processes would hold.
        let (b1, b2) = (attach_pool(base, len), attach_pool(base, len));
        // SAFETY: the index came from first_segment of a ring over
        // this pool, whose segments are still the ring's.
        let ring_1 = unsafe { MpscRing::<Multi>::attach(&b1, first) }.unwrap();
        let ring_2 = unsafe { MpscRing::<Multi>::attach(&b2, first) }.unwrap();
        assert_eq!(ring_2.first_segment(), first);
        assert_eq!(ring_2.segs.headers, ring_1.segs.headers);
        // The producer from one handle, the consumer from the other,
        // and the roles are one word for both.
        let prod = ring_1.claim_producer().unwrap();
        let mut cons = ring_2.claim_consumer().unwrap();
        assert_eq!(ring_1.claim_consumer().err(), Some(Error::RoleTaken));
        let mut next = 0u64;
        for burst in [3u64, 9, 12, 5, 12, 12, 7] {
            send(&prod, next, next + burst);
            recv(&mut cons, next, next + burst);
            next += burst;
        }
        assert!(prod.switches() > 3 && prod.switches() == cons.switches());
    }

    #[test]
    fn attach_rejects_hostile_control_blocks() {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let (first, spare) = with_init_pool(base, len, |pool| {
            let ring = MpscRing::<Multi>::init(pool, 64, 4, 3).unwrap();
            // A buffer that is no segment: free, its first word the
            // free-stack link.
            let spare = pool.alloc_bytes().unwrap();
            let spare_idx = spare.idx();
            spare.free();
            (ring.first_segment(), spare_idx)
        });
        let b = attach_pool(base, len);
        // SAFETY: each index names a buffer of this pool, and the
        // ring's segments are still the ring's.
        let attach = |idx| unsafe { MpscRing::<Multi>::attach(&b, idx) }.err();
        // SAFETY: first names the ring's segment 0.
        let ring = unsafe { MpscRing::<Multi>::attach(&b, first) }.unwrap();
        let block = ring.segs.header(0);

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

        // A ring no producer could join.
        block.info.max_producers.store(0, Ordering::Relaxed);
        assert_eq!(attach(first), Some(Error::BadMaxProducers));
        block
            .info
            .max_producers
            .store(u16::MAX as u32, Ordering::Relaxed);

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
        assert!(attach(first).is_none());
    }

    #[test]
    fn init_rejects_no_producers() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        assert_eq!(
            MpscRing::<Multi>::init_with_max_producers(&mut pool, 64, 4, 2, 0).err(),
            Some(Error::BadMaxProducers)
        );
    }

    #[test]
    // Dropping an endpoint is the behavior under test.
    #[allow(clippy::drop_non_drop)]
    fn roles_are_claimed_by_count() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init_with_max_producers(&mut pool, 64, 4, 2, 2).unwrap();
        let roles = |ring: &MpscRing<'_>| ring.segs.roles().load(Ordering::Relaxed);
        assert_eq!(roles(&ring), 0);

        // Producers up to the most, then RoleTaken, and a release
        // makes room.
        let p1 = ring.claim_producer().unwrap();
        let p2 = ring.claim_producer().unwrap();
        assert_eq!(roles(&ring), 2);
        assert_eq!(ring.claim_producer().err(), Some(Error::RoleTaken));
        p1.release();
        assert_eq!(roles(&ring), 1);
        let p3 = ring.claim_producer().unwrap();

        // One consumer, and it may come and go.
        let cons = ring.claim_consumer().unwrap();
        assert_eq!(roles(&ring), CONSUMER | 2);
        assert_eq!(ring.claim_consumer().err(), Some(Error::RoleTaken));
        cons.release();
        let cons = ring.claim_consumer().unwrap();

        // Dropping an endpoint writes nothing: its role stays held.
        drop(p2);
        assert_eq!(roles(&ring), CONSUMER | 2);
        drop(cons);
        assert_eq!(ring.claim_consumer().err(), Some(Error::RoleTaken));
        p3.release();
        assert_eq!(roles(&ring), CONSUMER | 1);
    }

    #[test]
    fn a_released_consumer_resumes_where_it_stopped() {
        // Consumers and producers claimed and released from two
        // attached handles, as processes would, stopping mid-segment
        // and across switches: every message arrives once, in order.
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let first = with_init_pool(base, len, |pool| {
            MpscRing::<Multi>::init(pool, 64, 4, 3)
                .unwrap()
                .first_segment()
        });
        let (b1, b2) = (attach_pool(base, len), attach_pool(base, len));
        // SAFETY: the index came from first_segment of a ring over
        // this pool, whose segments are still the ring's.
        let rings = [
            unsafe { MpscRing::<Multi>::attach(&b1, first) }.unwrap(),
            unsafe { MpscRing::<Multi>::attach(&b2, first) }.unwrap(),
        ];
        let (mut sent, mut read) = (0u64, 0u64);
        for (round, (burst, take)) in [(3u64, 1u64), (4, 5), (5, 3), (3, 6), (6, 4), (2, 4)]
            .into_iter()
            .enumerate()
        {
            let ring = &rings[round % 2];
            let prod = ring.claim_producer().unwrap();
            send(&prod, sent, sent + burst);
            sent += burst;
            prod.release();
            let mut cons = rings[(round + 1) % 2].claim_consumer().unwrap();
            recv(&mut cons, read, read + take);
            read += take;
            cons.release();
        }
        assert_eq!(read, sent);
        let mut cons = rings[0].claim_consumer().unwrap();
        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
        // Every segment but the current one is back.
        let probe = rings[1].claim_producer().unwrap();
        assert_eq!(free_segments(&probe).count_ones(), 2);
        assert!(probe.switches() > 3);
        assert_eq!(probe.segment(), cons.segment());
    }

    #[test]
    fn a_checkpoint_naming_no_segment_is_refused() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, 4, 2).unwrap();
        ring.segs.claims().cons_cur.store(2, Ordering::Relaxed);
        assert_eq!(ring.claim_consumer().err(), Some(Error::BadCheckpoint));
        // The role is left free.
        assert_eq!(ring.segs.roles().load(Ordering::Relaxed), 0);
        ring.segs.claims().cons_cur.store(1, Ordering::Relaxed);
        assert!(ring.claim_consumer().is_ok());
    }

    #[test]
    fn a_ring_is_released_only_with_no_role_held() {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let first = with_init_pool(base, len, |pool| {
            MpscRing::<Multi>::init(pool, 64, 4, 3)
                .unwrap()
                .first_segment()
        });
        let (b1, b2) = (attach_pool(base, len), attach_pool(base, len));
        // SAFETY: the index came from first_segment of a ring over
        // this pool, whose segments are still the ring's, for every
        // attach until the release below.
        let attach = |pool| unsafe { MpscRing::<Multi>::attach(pool, first) };

        // A producer or the consumer held: in use, and the ring is
        // unchanged.
        let other = attach(&b2).unwrap();
        let prod = other.claim_producer().unwrap();
        assert_eq!(
            attach(&b1).unwrap().release_ring(&b1).err(),
            Some(Error::RingInUse)
        );
        send(&prod, 0, 5);
        prod.release();
        let mut cons = other.claim_consumer().unwrap();
        assert_eq!(
            attach(&b1).unwrap().release_ring(&b1).err(),
            Some(Error::RingInUse)
        );
        recv(&mut cons, 0, 5);
        cons.release();

        // A pool over another region is refused.
        let mut r2 = Region::new();
        let (base2, len2) = region(&mut r2);
        with_init_pool(base2, len2, |_| {});
        let stranger = attach_pool(base2, len2);
        assert_eq!(
            attach(&b1).unwrap().release_ring(&stranger).err(),
            Some(Error::BadSegment)
        );

        // No role held: released, from another handle than the one
        // that used it, and its segments are the pool's again.
        attach(&b1).unwrap().release_ring(&b1).unwrap();
        assert_eq!(attach(&b1).err(), Some(Error::BadMagic));
        assert_eq!(other.claim_producer().err(), Some(Error::RingClosed));
        assert_eq!(other.claim_consumer().err(), Some(Error::RingClosed));
        // SAFETY: the region is live, and this is the only handle
        // allocating from it.
        let mut owner = unsafe { Pool::attach(base, len) }.unwrap();
        let all: Vec<_> = (0..BUFS).map(|_| owner.alloc_bytes().unwrap()).collect();
        assert_eq!(owner.alloc_bytes().err(), Some(Exhausted));
        all.into_iter().for_each(crate::BufSlot::free);
    }

    #[test]
    fn a_released_ring_is_released_once() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, 4, 2).unwrap();
        // A second handle to the same ring, as another process may
        // hold: its release after the first finds the ring closed.
        let stale: MpscRing<'_, Multi> = MpscRing {
            segs: ring.segs,
            first_segment: ring.first_segment(),
            _region: PhantomData,
        };
        ring.release_ring(&pool).unwrap();
        assert_eq!(stale.release_ring(&pool).err(), Some(Error::RingClosed));
    }

    #[test]
    fn single_takes_one_segment() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        assert_eq!(
            MpscRing::<Single>::init(&mut pool, 64, 4, 2).err(),
            Some(Error::BadSegmentCount)
        );
        let ring = MpscRing::<Single>::init(&mut pool, 64, 4, 1).unwrap();
        assert_eq!(
            ring.segs.header(0).info.mode.load(Ordering::Relaxed),
            Single::CODE
        );
    }

    #[test]
    fn single_is_one_ring() {
        // Full at the segment's depth, with no switch, lap after lap.
        for cap in [1u32, 2, 16] {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let (prod, mut cons) =
                endpoints(&MpscRing::<Single>::init(&mut pool, 64, cap, 1).unwrap());
            let cap = cap as u64;
            for lap in 0..5u64 {
                send(&prod, lap * cap, lap * cap + cap);
                assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
                recv(&mut cons, lap * cap, lap * cap + cap);
                assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
            }
            assert_eq!(prod.switches(), 0);
            assert_eq!(cons.switches(), 0);
            assert_eq!(prod.segment(), 0);
        }
    }

    #[test]
    fn attach_checks_the_mode() {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let (single, multi) = with_init_pool(base, len, |pool| {
            (
                MpscRing::<Single>::init(pool, 64, 4, 1)
                    .unwrap()
                    .first_segment(),
                MpscRing::<Multi>::init(pool, 64, 4, 1)
                    .unwrap()
                    .first_segment(),
            )
        });
        let b = attach_pool(base, len);
        // SAFETY: both indices came from first_segment of rings over
        // this pool, whose segments are still the rings'.
        unsafe {
            assert_eq!(
                MpscRing::<Multi>::attach(&b, single).err(),
                Some(Error::BadMode)
            );
            assert_eq!(
                MpscRing::<Single>::attach(&b, multi).err(),
                Some(Error::BadMode)
            );
            let ring = MpscRing::<Single>::attach(&b, single).unwrap();
            let (prod, mut cons) = endpoints(&ring);
            send(&prod, 0, 4);
            recv(&mut cons, 0, 4);
        }
    }

    #[test]
    fn single_streams_across_threads() {
        // One, two, and four producers on their own threads into a
        // one-segment ring, all spinning: per-producer order holds.
        let total: u64 = if cfg!(miri) { 50 } else { 20_000 };
        let depths: &[u32] = if cfg!(miri) { &[1, 8] } else { &[1, 2, 8, 16] };
        for &depth in depths {
            for producers in [1usize, 2, 4] {
                let mut r = Region::new();
                let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
                let ring = MpscRing::<Single>::init(&mut pool, 64, depth, 1).unwrap();
                let mut cons = ring.claim_consumer().unwrap();
                let prods = (0..producers)
                    .map(|_| ring.claim_producer().unwrap())
                    .collect();
                stream_threads(prods, &mut cons, total);
                assert_eq!(cons.switches(), 0);
            }
        }
    }

    /// Each producer on its own thread sending `count` messages with
    /// `send` and a [`SleepThen`] policy, and the consumer reading with
    /// `reserve_slot_wait`, all waiting without end: per-producer
    /// order holds and every message arrives.
    fn stream_waiting<M: Mode, W: Wake>(
        prods: Vec<MpscProducer<'_, M, W>>,
        cons: &mut MpscConsumer<'_, M, W>,
        count: u64,
    ) {
        let producers = prods.len();
        std::thread::scope(|s| {
            for (p, prod) in prods.into_iter().enumerate() {
                s.spawn(move || {
                    for i in 0..count {
                        prod.send::<Msg>(SleepThen(|_| true), |m| {
                            m.seq = i;
                            m.val = p as u64;
                        })
                        .unwrap(); // OK: a policy of |_| true never gives up
                    }
                });
            }
            s.spawn(move || {
                let mut next = vec![0u64; producers];
                for _ in 0..producers as u64 * count {
                    let msg = cons.reserve_slot_wait::<Msg>(|_| true).unwrap(); // OK: a policy of |_| true never gives up
                    let p = msg.val as usize;
                    assert_eq!(msg.seq, next[p], "per-producer order broken");
                    next[p] += 1;
                    msg.release();
                }
            });
        });
    }

    #[test]
    fn waiting_streams_across_threads() {
        // Producers and a consumer that wait rather than spin, over
        // both modes and both wakes, at depths where every message
        // fills the ring and where few do.
        let total: u64 = if cfg!(miri) { 20 } else { 5_000 };
        for depth in [1u32, 2, 8] {
            for producers in [1usize, 2, 4] {
                let mut r = Region::new();
                let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
                let ring = MpscRing::<Single, NoWake>::init(&mut pool, 64, depth, 1).unwrap();
                let mut cons = ring.claim_consumer().unwrap();
                let prods = (0..producers)
                    .map(|_| ring.claim_producer().unwrap())
                    .collect();
                stream_waiting(prods, &mut cons, total);
                #[cfg(target_os = "linux")]
                {
                    use crate::wake::Futex;
                    let mut r = Region::new();
                    let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
                    let ring = MpscRing::<Single, Futex<5>>::init(&mut pool, 64, depth, 1).unwrap();
                    let mut cons = ring.claim_consumer().unwrap();
                    let prods = (0..producers)
                        .map(|_| ring.claim_producer().unwrap())
                        .collect();
                    stream_waiting(prods, &mut cons, total);
                    let ring = MpscRing::<Multi, Futex<5>>::init(&mut pool, 64, depth, 3).unwrap();
                    let mut cons = ring.claim_consumer().unwrap();
                    let prods = (0..producers)
                        .map(|_| ring.claim_producer().unwrap())
                        .collect();
                    stream_waiting(prods, &mut cons, total);
                    // Every sleeper woke and cleared its trace.
                    assert_eq!(ring.segs.claim().load(Ordering::Relaxed) & WAITING, 0);
                    let c = ring.segs.claims();
                    assert_eq!(c.prod_waiters.load(Ordering::Relaxed), 0);
                }
            }
        }
    }

    /// A futex whose timeout is far longer than any test step, so a
    /// wait that ends quickly ended by a wake.
    #[cfg(target_os = "linux")]
    type SlowFutex = crate::wake::Futex<500>;

    /// Well under [`SlowFutex`]'s timeout, well over a step's time.
    #[cfg(target_os = "linux")]
    const WOKEN: std::time::Duration = std::time::Duration::from_millis(250);

    #[cfg(target_os = "linux")]
    #[test]
    fn a_waiting_consumer_is_woken_by_a_send() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi, SlowFutex>::init(&mut pool, 64, 4, 2).unwrap();
        let (prod, mut cons) = endpoints(&ring);
        std::thread::scope(|s| {
            let reader = s.spawn(move || {
                let start = std::time::Instant::now();
                let mut wakes = 0u32;
                let msg = cons
                    .reserve_slot_wait::<Msg>(|attempt| {
                        wakes = attempt + 1;
                        true
                    })
                    .unwrap();
                assert_eq!(msg.seq, 7);
                msg.release();
                (start.elapsed(), wakes)
            });
            std::thread::sleep(std::time::Duration::from_millis(50));
            prod.send::<Msg>(|_| false, |m| m.seq = 7).unwrap();
            let (waited, wakes) = reader.join().unwrap();
            assert!(waited < WOKEN, "woken by its timeout, not the send");
            // A sleep and a wake, and perhaps a short spin between a
            // claim and its commit, never a poll.
            assert!(wakes < 100, "{wakes} wakes");
        });
        assert_eq!(ring.segs.claim().load(Ordering::Relaxed) & WAITING, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_waiting_producer_is_woken_by_releases() {
        // A full ring of each mode: the producer sleeps, and the
        // consumer's releases, half a segment of them, or the
        // segment it gives back, wake it.
        fn run<M: Mode>(count: u32) {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let ring = MpscRing::<M, SlowFutex>::init(&mut pool, 64, 4, count).unwrap();
            let (prod, mut cons) = endpoints(&ring);
            let full = 4 * count as u64;
            send(&prod, 0, full);
            assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
            std::thread::scope(|s| {
                let writer = s.spawn(move || {
                    let start = std::time::Instant::now();
                    prod.send::<Msg>(SleepThen(|_| true), |m| {
                        m.seq = full;
                        m.val = full * 10;
                    })
                    .unwrap();
                    start.elapsed()
                });
                std::thread::sleep(std::time::Duration::from_millis(50));
                recv(&mut cons, 0, full);
                let waited = writer.join().unwrap();
                assert!(waited < WOKEN, "woken by its timeout, not a release");
                recv(&mut cons, full, full + 1);
            });
            assert_eq!(ring.segs.claims().prod_waiters.load(Ordering::Relaxed), 0);
        }
        run::<Single>(1);
        run::<Multi>(2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_wait_is_bounded_by_its_policy() {
        // Nothing arrives: each timeout is a policy call, and the
        // policy ends the wait.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Single, crate::wake::Futex<1>>::init(&mut pool, 64, 2, 1).unwrap();
        let (prod, mut cons) = endpoints(&ring);
        let mut seen = Vec::new();
        let err = cons
            .reserve_slot_wait::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err();
        assert_eq!(err, Some(Empty));
        assert_eq!(seen, [0, 1, 2]);
        send(&prod, 0, 2);
        let mut seen = Vec::new();
        let err = prod
            .send::<Msg>(
                SleepThen(|attempt| {
                    seen.push(attempt);
                    attempt < 2
                }),
                |_| {},
            )
            .err();
        assert_eq!(err, Some(Full));
        assert_eq!(seen, [0, 1, 2]);
    }

    /// `LONG` is well over any deadline a test below sets, so a send that returns sooner returned
    /// because of room or its own deadline.
    #[cfg(any(target_os = "linux", feature = "std"))]
    const LONG: std::time::Duration = std::time::Duration::from_millis(250);

    /// `SLOW_US` is [`SlowFutex`]'s timeout in microseconds: a deadline well over [`WOKEN`], so a
    /// send that returns sooner was woken.
    #[cfg(target_os = "linux")]
    const SLOW_US: u64 = 500_000;

    #[cfg(any(target_os = "linux", feature = "std"))]
    #[test]
    fn a_deadline_send_gives_up_at_its_time() {
        // A full ring and no consumer: a zero deadline probes once, and a longer one spins until it
        // passes, both then Full. With room, the send lands and reads no clock.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Single>::init(&mut pool, 64, 4, 1).unwrap();
        let (prod, mut cons) = endpoints(&ring);
        send(&prod, 0, 3);
        prod.send_spin::<Msg>(Ticks::ZERO, |m| {
            m.seq = 3;
            m.val = 30;
        })
        .unwrap();
        assert_eq!(prod.send_spin::<Msg>(Ticks::ZERO, |_| {}).err(), Some(Full));
        let start = std::time::Instant::now();
        assert_eq!(
            prod.send_spin::<Msg>(crate::microsecs_to_ticks(20_000), |_| {})
                .err(),
            Some(Full)
        );
        let waited = start.elapsed();
        assert!(waited >= std::time::Duration::from_millis(20), "{waited:?}");
        assert!(waited < LONG, "{waited:?}");
        // With NoWake the backoff send spins through both times.
        let start = std::time::Instant::now();
        assert_eq!(
            prod.send_spin_sleep::<Msg>(
                crate::microsecs_to_ticks(100),
                crate::microsecs_to_ticks(10_000),
                |_| {}
            )
            .err(),
            Some(Full)
        );
        let waited = start.elapsed();
        assert!(
            waited >= std::time::Duration::from_micros(10_100),
            "{waited:?}"
        );
        assert!(waited < LONG, "{waited:?}");
        recv(&mut cons, 0, 4);
    }

    #[cfg(any(target_os = "linux", feature = "std"))]
    #[test]
    fn a_deadline_send_lands_when_room_comes() {
        // A full ring: a send that never gives up spins until the consumer frees room, then lands
        // after everything before it.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Single>::init(&mut pool, 64, 4, 1).unwrap();
        let (prod, mut cons) = endpoints(&ring);
        send(&prod, 0, 4);
        std::thread::scope(|s| {
            let writer = s.spawn(move || {
                prod.send_spin::<Msg>(Ticks::FOREVER, |m| {
                    m.seq = 4;
                    m.val = 40;
                })
            });
            std::thread::sleep(std::time::Duration::from_millis(50));
            recv(&mut cons, 0, 4);
            assert_eq!(writer.join().unwrap(), Ok(()));
            recv(&mut cons, 4, 5);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_deadline_sleeper_is_woken_by_releases() {
        // A full ring of each mode: the producer spins not at all, sleeps, and is woken by the
        // consumer's releases long before its deadline or SlowFutex's timeout.
        fn run<M: Mode>(count: u32) {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let ring = MpscRing::<M, SlowFutex>::init(&mut pool, 64, 4, count).unwrap();
            let (prod, mut cons) = endpoints(&ring);
            let full = 4 * count as u64;
            send(&prod, 0, full);
            let waiters = &ring.segs.claims().prod_waiters;
            std::thread::scope(|s| {
                let writer = s.spawn(move || {
                    let start = std::time::Instant::now();
                    let sent = prod.send_spin_sleep::<Msg>(
                        Ticks::ZERO,
                        crate::microsecs_to_ticks(SLOW_US),
                        |m| {
                            m.seq = full;
                            m.val = full * 10;
                        },
                    );
                    (sent, start.elapsed())
                });
                // Asleep before the room comes, so the room wakes it.
                let start = std::time::Instant::now();
                while waiters.load(Ordering::SeqCst) == 0 {
                    assert!(start.elapsed() < LONG, "the producer never slept");
                    std::thread::yield_now();
                }
                recv(&mut cons, 0, full);
                let (sent, waited) = writer.join().unwrap();
                assert_eq!(sent, Ok(()));
                assert!(waited < WOKEN, "woken by its timeout, not a release");
                recv(&mut cons, full, full + 1);
            });
            assert_eq!(waiters.load(Ordering::Relaxed), 0);
        }
        run::<Single>(1);
        run::<Multi>(2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_deadline_sleep_gives_up_at_its_time() {
        // A full ring and no consumer: the producer spins, sleeps on what is left of its wait, and
        // gives up when it passes, far sooner than SlowFutex's own timeout.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Single, SlowFutex>::init(&mut pool, 64, 2, 1).unwrap();
        let (prod, _cons) = endpoints(&ring);
        send(&prod, 0, 2);
        let start = std::time::Instant::now();
        assert_eq!(
            prod.send_spin_sleep::<Msg>(
                crate::microsecs_to_ticks(100),
                crate::microsecs_to_ticks(20_000),
                |_| {}
            )
            .err(),
            Some(Full)
        );
        let waited = start.elapsed();
        assert!(
            waited >= std::time::Duration::from_micros(20_100),
            "{waited:?}"
        );
        assert!(waited < LONG, "{waited:?}");
        assert_eq!(ring.segs.claims().prod_waiters.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn backoff_streams_across_threads() {
        // Four producers backing off after each lost claim, into one
        // consumer: every message arrives in each producer's order,
        // and each loss is counted from 1.
        const COUNT: u64 = if cfg!(miri) { 50 } else { 20_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, 4, 2).unwrap();
        let mut cons = ring.claim_consumer().unwrap();
        let prods: Vec<_> = (0..4).map(|_| ring.claim_producer().unwrap()).collect();
        let first_losses = std::sync::atomic::AtomicU64::new(0);
        let first_losses = &first_losses;
        std::thread::scope(|s| {
            for (p, prod) in prods.into_iter().enumerate() {
                s.spawn(move || {
                    for i in 0..COUNT {
                        prod.send::<Msg>(
                            LostThen(|lost| {
                                assert!(lost >= 1);
                                if lost == 1 {
                                    first_losses.fetch_add(1, Ordering::Relaxed);
                                }
                                crate::policy::backoff(lost);
                            }),
                            |m| {
                                m.seq = i;
                                m.val = p as u64;
                            },
                        )
                        .unwrap(); // OK: policy::spin never gives up
                    }
                });
            }
            s.spawn(move || {
                let mut next = [0u64; 4];
                for _ in 0..4 * COUNT {
                    let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap(); // OK: policy::spin never gives up
                    let p = msg.val as usize;
                    assert_eq!(msg.seq, next[p], "per-producer order broken");
                    next[p] += 1;
                    msg.release();
                }
            });
        });
        // Losses are the scheduler's to make, so only their count is
        // shown, never asserted.
        println!("first losses: {}", first_losses.load(Ordering::Relaxed));
    }

    #[test]
    fn words_pack_segment_over_position() {
        assert_eq!(word_seg(word(31, 5)), 31);
        assert_eq!(word_pos(word(31, 5)), 5);
        assert_eq!(word_pos(word(0, SEQ_MASK + 3)), 2);
        assert_eq!(word(31, SEQ_MASK) & MOVED, 0);
    }

    #[test]
    fn one_segment_is_a_ring() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 4, 1).unwrap());
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        for lap in 0..3u64 {
            send(&prod, lap * 4, lap * 4 + 4);
            assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
            recv(&mut cons, lap * 4, lap * 4 + 4);
            assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
        }
        assert_eq!(prod.switches(), 0);
        assert_eq!(cons.switches(), 0);
    }

    #[test]
    fn segments_extend_the_ring() {
        // With the consumer idle the producer fills every segment,
        // switching as each is full, and only then reports Full.
        for (cap, count) in [(1u32, 2u32), (4, 3), (16, 4)] {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let (prod, mut cons) =
                endpoints(&MpscRing::<Multi>::init(&mut pool, 64, cap, count).unwrap());
            let total = (cap * count) as u64;
            send(&prod, 0, total);
            assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
            assert_eq!(free_segments(&prod), 0);
            assert_eq!(prod.switches(), (count - 1) as u64);
            recv(&mut cons, 0, total);
            assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
            // Every segment but the current one is back.
            assert_eq!(free_segments(&prod).count_ones(), count - 1);
            assert_eq!(free_segments(&prod) & (1 << prod.segment()), 0);
            assert_eq!(prod.segment(), cons.segment());
            assert_eq!(cons.switches(), prod.switches());
        }
    }

    #[test]
    fn segments_cycle_far_past_one_pool() {
        // Many laps through every segment, in bursts that force
        // switches, at depth 1 and larger.
        for (cap, count) in [(1u32, 2u32), (1, 5), (2, 3), (4, 2), (16, 8)] {
            let mut r = Region::new();
            let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
            let (prod, mut cons) =
                endpoints(&MpscRing::<Multi>::init(&mut pool, 64, cap, count).unwrap());
            let burst = (cap * count) as u64;
            let mut next = 0u64;
            for round in 0..200u64 {
                // Vary the burst so segments are left mid-lap.
                let n = 1 + (round * 7) % burst;
                send(&prod, next, next + n);
                recv(&mut cons, next, next + n);
                next += n;
                assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                assert_eq!(free_segments(&prod).count_ones(), count - 1);
                assert_eq!(cons.switches(), prod.switches());
            }
            assert!(next > 10 * burst);
        }
    }

    #[test]
    fn depth_one_switches_every_send() {
        // A released slot is claimable, so a consumer that keeps
        // up causes no switch even at depth 1. One message behind,
        // every send finds its segment's one slot unread and
        // switches, and with three segments there is always one
        // free: the segment behind the consumer is given back at
        // the reserve after its message, one poll late.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 1, 3).unwrap());
        for i in 0..10u64 {
            send(&prod, i, i + 1);
            recv(&mut cons, i, i + 1);
        }
        assert_eq!(prod.switches(), 0);
        send(&prod, 10, 11);
        for i in 11..20u64 {
            let before = prod.segment();
            send(&prod, i, i + 1);
            assert_ne!(prod.segment(), before);
            assert_eq!(prod.switches(), i - 10);
            recv(&mut cons, i - 1, i);
        }
        recv(&mut cons, 19, 20);
        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
        assert_eq!(cons.switches(), prod.switches());
        assert_eq!(cons.segment(), prod.segment());
        assert_eq!(free_segments(&prod).count_ones(), 2);
    }

    #[test]
    fn no_free_segment_waits_on_the_current_one() {
        // Two segments of one slot: the third send finds no free
        // segment, so the producer waits in place until the
        // consumer frees a slot.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 1, 2).unwrap());
        send(&prod, 0, 2);
        assert_eq!(prod.segment(), 1);
        assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
        // Segment 0 is read, but given back only at the next
        // reserve, so the producer still finds nothing free.
        recv(&mut cons, 0, 1);
        assert_eq!(free_segments(&prod), 0);
        assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
        // That reserve follows the seal and gives segment 0 back,
        // and its release frees segment 1's slot.
        recv(&mut cons, 1, 2);
        assert_eq!(cons.segment(), 1);
        assert_eq!(free_segments(&prod), 1);
        // A released slot in the current segment is claimable, so
        // no switch. The next send finds it unread and switches.
        send(&prod, 2, 3);
        assert_eq!(prod.segment(), 1);
        assert_eq!(prod.switches(), 1);
        send(&prod, 3, 4);
        assert_eq!(prod.segment(), 0);
        assert_eq!(prod.switches(), 2);
        recv(&mut cons, 2, 4);
        assert_eq!(cons.segment(), 0);
        assert_eq!(cons.switches(), prod.switches());
    }

    #[test]
    fn policies_count_and_give_up() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 2, 1).unwrap());
        let mut seen = Vec::new();
        let err = cons
            .reserve_slot_with::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err();
        assert_eq!(err, Some(Empty));
        assert_eq!(seen, [0, 1, 2]);
        send(&prod, 0, 2);
        let mut seen = Vec::new();
        let err = prod
            .send::<Msg>(
                |attempt| {
                    seen.push(attempt);
                    attempt < 2
                },
                |_| {},
            )
            .err();
        assert_eq!(err, Some(Full));
        assert_eq!(seen, [0, 1, 2]);
        let msg = cons
            .reserve_slot_with::<Msg>(|_| panic!("policy consulted with a message available"))
            .unwrap();
        msg.release();
        prod.send::<Msg>(|_| panic!("policy consulted with room available"), |_| {})
            .unwrap();
    }

    #[test]
    // Dropping the guard is the behavior under test.
    #[allow(clippy::drop_non_drop)]
    fn abandoned_read_guard_redelivers() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 1, 2).unwrap());
        send(&prod, 0, 2);
        // The first read is the last message of segment 0, and an
        // abandoned read re-delivers it, the switch after it.
        let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
        assert_eq!(msg.seq, 0);
        drop(msg);
        recv(&mut cons, 0, 2);
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        assert_eq!(cons.switches(), 1);
    }

    #[test]
    fn a_panicking_fill_jams_the_ring() {
        // No unwind guard: a fill that panics leaves its slot
        // claimed and never committed, as a producer killed there
        // does, so the consumer stops at it and the messages after
        // it are never delivered. The ring is recovered by a
        // restart.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 4, 2).unwrap());
        send(&prod, 0, 1);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prod.send::<Msg>(|_| false, |_: &mut Msg| panic!("fill panics"))
        }));
        assert!(unwound.is_err());
        send(&prod, 1, 3);
        recv(&mut cons, 0, 1);
        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
    }

    #[test]
    fn positions_survive_the_wrap() {
        // Both sides two positions shy of the 26-bit wrap in every
        // segment, each segment's seqs claimable for the lap that
        // starts there, then several laps across it.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, 4, 3).unwrap();
        let start = SEQ_MASK - 1;
        for seg in 0..3 {
            for i in 0..4u32 {
                let idx = start.wrapping_add(i);
                ring.segs
                    .seq(seg, idx)
                    .store(seq_of(idx), Ordering::Relaxed);
            }
            // The next taker resumes where the seal says.
            ring.segs.seal(seg).store(start, Ordering::Relaxed);
        }
        ring.segs.claim().store(word(0, start), Ordering::Relaxed);
        let (prod, mut cons) = endpoints(&ring);
        cons.st.pos = start;
        cons.st.resume = [start; MAX_SEGMENTS as usize];
        // Batches of at most the ring's 12, so no send finds Full.
        for (from, to) in [(0u64, 12u64), (12, 24), (24, 30), (30, 41)] {
            send(&prod, from, to);
            recv(&mut cons, from, to);
            assert_eq!(free_segments(&prod).count_ones(), 2);
        }
        // The positions did cross the wrap.
        assert!(cons.st.pos < start);
        assert!(word_pos(prod.segs.claim().load(Ordering::Relaxed)) < start);
    }

    #[test]
    fn a_stale_seal_does_not_end_a_reused_segment() {
        // Segment 0 is sealed, given back, and taken again: its
        // old seal must not end its second use at the old end
        // position.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 2, 2).unwrap());
        send(&prod, 0, 3);
        recv(&mut cons, 0, 3);
        assert_eq!(cons.segment(), 1);
        // Fill segment 1, switch back to 0, and run a whole lap of
        // 0 past its old end position of 2.
        send(&prod, 3, 6);
        assert_eq!(prod.segment(), 0);
        recv(&mut cons, 3, 6);
        assert_eq!(cons.segment(), 0);
        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
        assert_eq!(cons.switches(), prod.switches());
    }

    /// `producers` threads each sending `count` messages through
    /// `seg_count` segments of `cap` slots, spinning on both ends,
    /// per-producer FIFO checked at the consumer.
    fn stream(producers: u64, cap: u32, seg_count: u32, count: u64) {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::<Multi>::init(&mut pool, 64, cap, seg_count).unwrap();
        let mut cons = ring.claim_consumer().unwrap();
        let prods: Vec<_> = (0..producers)
            .map(|_| ring.claim_producer().unwrap())
            .collect();
        let probe = ring.claim_producer().unwrap();
        stream_threads(prods, &mut cons, count);
        // Every seal was consumed once, and every segment but the
        // current one is back.
        assert_eq!(cons.switches(), probe.switches());
        assert_eq!(free_segments(&probe).count_ones(), seg_count - 1);
        assert_eq!(probe.segment(), cons.segment());
    }

    /// Each producer on its own thread sending `count` messages, and
    /// the consumer on its own, all spinning, per-producer FIFO
    /// checked at the consumer.
    fn stream_threads<M: Mode>(
        prods: Vec<MpscProducer<'_, M>>,
        cons: &mut MpscConsumer<'_, M>,
        count: u64,
    ) {
        let producers = prods.len() as u64;
        std::thread::scope(|s| {
            for (p, prod) in prods.into_iter().enumerate() {
                s.spawn(move || {
                    for i in 0..count {
                        prod.send::<Msg>(crate::policy::spin, |m| {
                            m.seq = i;
                            m.val = p as u64;
                        })
                        .unwrap(); // OK: policy::spin never gives up
                    }
                    prod.release();
                });
            }
            s.spawn(move || {
                // Global arrival order is claim order, and only
                // per-producer FIFO is promised.
                let mut next = vec![0u64; producers as usize];
                for _ in 0..producers * count {
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

    /// One cache line of heap backing store, so a `Vec` of them is
    /// a line-aligned region of any size.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
    #[repr(C, align(64))]
    struct Line([u8; CACHE_LINE_SIZE]);

    /// The segment counts, depths, and producer counts the matrix
    /// covers: all of them, or under Miri a corner, its
    /// interpreter being slow.
    fn matrix() -> (Vec<u32>, Vec<u32>, Vec<u64>) {
        if cfg!(miri) {
            (vec![1, 2, 32], vec![1, 8], vec![1, 2])
        } else {
            (
                (1..=MAX_SEGMENTS).collect(),
                vec![1, 8, 64, 1024],
                vec![1, 2, 4],
            )
        }
    }

    /// Run `f` on a ring of `count` segments of `depth` one-line
    /// slots over a pool holding exactly those segments.
    fn with_ring(count: u32, depth: u32, f: impl FnOnce(&MpscRing<'_>)) {
        let buf = segment_size(64, depth);
        let bytes = size_of::<PoolHeader>() as u64 + buf * count as u64;
        let mut store = vec![Line([0; CACHE_LINE_SIZE]); bytes.div_ceil(64) as usize];
        let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), buf as u32, count).unwrap();
        f(&MpscRing::<Multi>::init(&mut pool, 64, depth, count).unwrap());
    }

    #[test]
    fn every_count_and_depth_fills_and_drains() {
        // With the consumer idle a producer writes into every
        // segment in turn, switching once between each pair, and
        // the consumer follows it through the same switches.
        let (counts, depths, _) = matrix();
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |ring| {
                    let (prod, mut cons) = endpoints(ring);
                    let total = (count * depth) as u64;
                    let mut used = 1u32 << prod.segment();
                    for i in 0..total {
                        send(&prod, i, i + 1);
                        used |= 1 << prod.segment();
                    }
                    assert_eq!(prod.send::<Msg>(|_| false, |_| {}).err(), Some(Full));
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
        let (counts, depths, _) = matrix();
        let rounds = if cfg!(miri) { 5 } else { 40 };
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |ring| {
                    let (prod, mut cons) = endpoints(ring);
                    let capacity = (count * depth) as u64;
                    let mut next = 0u64;
                    for round in 0..rounds {
                        let n = 1 + (round * 7919) % capacity;
                        send(&prod, next, next + n);
                        recv(&mut cons, next, next + n);
                        next += n;
                        assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                        assert_eq!(cons.switches(), prod.switches());
                        assert_eq!(free_segments(&prod).count_ones(), count - 1);
                    }
                });
            }
        }
    }

    #[test]
    fn every_count_depth_and_producer_count_streams_across_threads() {
        // One, two, and four producers on their own threads and a
        // consumer on its own, all spinning: per-producer order
        // holds, every message arrives, and the switch counts agree.
        let (counts, depths, producers) = matrix();
        let total: u64 = if cfg!(miri) { 50 } else { 10_000 };
        for &count in &counts {
            for &depth in &depths {
                for &producers in &producers {
                    with_ring(count, depth, |ring| {
                        let mut cons = ring.claim_consumer().unwrap();
                        let prods = (0..producers)
                            .map(|_| ring.claim_producer().unwrap())
                            .collect();
                        stream_threads(prods, &mut cons, total);
                        let probe = ring.claim_producer().unwrap();
                        assert_eq!(
                            cons.switches(),
                            probe.switches(),
                            "{count} segments at depth {depth}, {producers} producers"
                        );
                        assert_eq!(free_segments(&probe).count_ones(), count - 1);
                        assert_eq!(probe.segment(), cons.segment());
                    });
                }
            }
        }
    }

    #[test]
    fn threaded_two_producers() {
        // Reduced under Miri: interpreted spin loops are slow.
        const COUNT: u64 = if cfg!(miri) { 100 } else { 50_000 };
        for (cap, count) in [(1u32, 2u32), (1, 3), (2, 2), (4, 3), (16, 2)] {
            stream(2, cap, count, COUNT);
        }
    }

    #[test]
    fn threaded_four_producers() {
        const COUNT: u64 = if cfg!(miri) { 50 } else { 20_000 };
        for (cap, count) in [(1u32, 3u32), (4, 2), (16, 4)] {
            stream(4, cap, count, COUNT);
        }
    }

    #[test]
    fn threaded_shared_reference_producers() {
        // The Sync path: two threads share one &MpscProducer
        // instead of each claiming its own.
        const COUNT: u64 = if cfg!(miri) { 100 } else { 50_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = endpoints(&MpscRing::<Multi>::init(&mut pool, 64, 4, 2).unwrap());
        let prod = &prod;
        std::thread::scope(|s| {
            for p in 0..2u64 {
                s.spawn(move || {
                    for i in 0..COUNT {
                        prod.send::<Msg>(crate::policy::spin, |m| {
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
