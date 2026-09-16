//! MPSC v2 ring: v1's claim and seq protocol over SPSC v3's
//! segments, per the design doc's "MPSC v2: ring of segments"
//! section. A sibling of v0 and v1 under the same module layout,
//! so the three measure side by side, and neither of them
//! changes for it.
//!
//! - Segments: up to [`MAX_SEGMENTS`] taken from the application's
//!   [`Pool`] at [`MpscRing::init`], each a header of three lines
//!   then `seg_capacity` slots opening with the seq word, as
//!   `spsc::v3` lays its segments out. Nothing allocates while the
//!   ring runs, and there is no `attach`.
//! - Words: 32 bits everywhere. A position is [`SEQ_BITS`] wide,
//!   and the slot word holds v1's values in those bits, claimable
//!   at `pos`, committed at `pos + M + 1`, released at `pos + M`,
//!   with v1's tombstone at bit 31. The claim word packs the
//!   current segment over the next position, and a seal packs
//!   MOVED, the next segment, and the end position the same way.
//! - The claim word is the seal: every producer claims as v1
//!   claims, and a stale view fails the CAS, so no claim lands in
//!   a segment the ring has left.
//! - A producer at a full segment takes a free one by setting its
//!   bit in the in-use word, moves the claim word to it, and seals
//!   the old segment's header. The consumer reads the seal only when a
//!   slot is neither committed nor tombstoned, so its fast path
//!   is v1's one load.
//! - Gated with the rest of `mpsc` on `target_has_atomic = "32"`.
//! - How to use it, from a pool to several producer threads, is
//!   the user guide, `notes/user-guide.md`, and what happens to
//!   the segments over a run is the design note's Segment
//!   lifecycle subsection, both with `examples/guide_mpsc_v2.rs`.

use core::marker::PhantomData;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::spsc::v3::{check_body_type, seq_of, validate_geometry};
use crate::{CACHE_LINE_SIZE, CacheAligned, Error, Pool};

mod consumer;
mod producer;

pub use crate::spsc::v2::SLOT_HEADER_BYTES;
pub use crate::spsc::v3::{MAX_SEG_CAPACITY, MAX_SEGMENTS, SEQ_BITS};
pub use consumer::{MpscConsumer, MpscReadSlot};
pub use producer::MpscProducer;

/// The position's bits within a word.
const SEQ_MASK: u32 = (1 << SEQ_BITS) - 1;

/// Where a segment number sits in a word, above the position.
const SEG_SHIFT: u32 = SEQ_BITS;

/// A segment number, once shifted down.
const SEG_MASK: u32 = MAX_SEGMENTS - 1;

/// Set in a seal word: the segment ended at the word's position,
/// and the ring went on in the word's segment.
const MOVED: u32 = 1 << 31;

/// Tombstone offset in a slot word: an unwound `send_with`
/// commits `pos + M + 1 + TOMBSTONE`, which the consumer
/// releases without delivering, as v1's.
///
/// - Distinct from every value a side can see: the position
///   bits of a committed word never carry bit 31.
pub(crate) const TOMBSTONE: u32 = 1 << 31;

const _: () = assert!(SEG_SHIFT + MAX_SEGMENTS.trailing_zeros() <= 31);

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

/// The three lines at the front of every segment.
///
/// - Only segment 0's `claim` and `in_use` lines are used, so
///   every segment has the same layout. The claim word is the
///   contended line and has it alone.
#[repr(C)]
struct SegmentHeader {
    /// The seal: MOVED, the next segment, and the end position,
    /// stored by the producer that moved the ring on, cleared by
    /// the producer that next takes this segment, and read by
    /// the consumer when a slot is not committed.
    seal: CacheAligned<AtomicU32>,
    /// Segment 0: the current segment and the next position to
    /// claim, CAS-claimed by every producer.
    claim: CacheAligned<AtomicU32>,
    /// Segment 0: the in-use word and the switch count, both
    /// touched on the switch path only.
    in_use: CacheAligned<InUseLine>,
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

const _: () = assert!(size_of::<SegmentHeader>() == 3 * CACHE_LINE_SIZE);

/// Bytes a segment needs: its header lines, then the slots.
///
/// - The pool handed to [`MpscRing::init`] needs buffers at least
///   this large.
/// - Computed in u64 for the same 32-bit wrap reason as the
///   rings' region sizes.
pub fn segment_size(slot_size: u32, seg_capacity: u32) -> u64 {
    size_of::<SegmentHeader>() as u64 + slot_size as u64 * seg_capacity as u64
}

/// A ring's geometry and its segments' addresses, the state
/// every endpoint starts from.
///
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
}

impl Segments {
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
        // borrows from.
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

/// A ring of segments over the application's pool, split into
/// the producer and consumer handles with [`MpscRing::split`].
pub struct MpscRing<'a> {
    /// Geometry and segment addresses.
    segs: Segments,
    _region: PhantomData<&'a [u8]>,
}

impl<'a> MpscRing<'a> {
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
        let mut bases = [core::ptr::null_mut::<u8>(); MAX_SEGMENTS as usize];
        for seg in 0..seg_count as usize {
            match pool.alloc_bytes() {
                Ok(buf) => {
                    bases[seg] = buf.as_mut_ptr();
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
        let mut slots = [core::ptr::null_mut(); MAX_SEGMENTS as usize];
        let mut headers = [core::ptr::null(); MAX_SEGMENTS as usize];
        for (seg, &base) in bases.iter().take(seg_count as usize).enumerate() {
            // SAFETY: base is a buffer of at least segment_size
            // bytes the pool just handed out, line-aligned, and
            // nothing else can reach it until the ring is split:
            // the header lines and each slot's header are ours to
            // write.
            unsafe {
                core::ptr::write_bytes(base, 0, size_of::<SegmentHeader>());
                let seg_slots = base.add(size_of::<SegmentHeader>());
                for i in 0..seg_capacity {
                    let slot = seg_slots.add(i as usize * slot_size as usize);
                    core::ptr::write_bytes(slot, 0, SLOT_HEADER_BYTES);
                    // `seq[i] = i`: every slot claimable for lap 0.
                    (*(slot as *const AtomicU32)).store(seq_of(i), Ordering::Relaxed);
                }
                slots[seg] = seg_slots;
                headers[seg] = base as *const SegmentHeader;
            }
        }
        let segs = Segments {
            slots,
            headers,
            slot_size,
            capacity: seg_capacity,
            mask: seg_capacity - 1,
            seg_count,
            commit_add: seg_capacity + 1,
        };
        // The ring starts in segment 0 at position 0, segment 0
        // in use. The threads that use the ring start after this,
        // so Relaxed is enough here.
        segs.claim().store(word(0, 0), Ordering::Relaxed);
        segs.in_use().store(1, Ordering::Relaxed);
        Ok(MpscRing {
            segs,
            _region: PhantomData,
        })
    }

    /// Split into one producer handle and the consumer handle.
    ///
    /// - The producer is `Clone`: one clone per producing thread,
    ///   or one handle shared by reference. The consumer is
    ///   unique.
    pub fn split(self) -> (MpscProducer<'a>, MpscConsumer<'a>) {
        (MpscProducer::new(self.segs), MpscConsumer::new(self.segs))
    }
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

    /// Test pool buffer: a segment of up to 16 one-line slots.
    const BUF: usize = 3 * CACHE_LINE_SIZE + 16 * CACHE_LINE_SIZE;

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
    fn send(prod: &MpscProducer<'_>, from: u64, to: u64) {
        for i in from..to {
            prod.send_with::<Msg>(
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
    fn recv(cons: &mut MpscConsumer<'_>, from: u64, to: u64) {
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

    /// Segments free by the in-use word.
    fn free_segments(prod: &MpscProducer<'_>) -> u32 {
        let segs = &prod.segs;
        !segs.in_use().load(Ordering::Acquire) & segs.all()
    }

    #[test]
    fn init_rejects_bad_geometry() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let err =
            |slot, cap, count, pool: &mut Pool<'_>| MpscRing::init(pool, slot, cap, count).err();
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
        // 32 one-line slots do not fit a 19-line buffer.
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
                3 * CACHE_LINE_SIZE as u64 + 64 * cap as u64
            );
        }
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 4, 1).unwrap().split();
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        for lap in 0..3u64 {
            send(&prod, lap * 4, lap * 4 + 4);
            assert_eq!(prod.send_with::<Msg>(|_| false, |_| {}).err(), Some(Full));
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
            let (prod, mut cons) = MpscRing::init(&mut pool, 64, cap, count).unwrap().split();
            let total = (cap * count) as u64;
            send(&prod, 0, total);
            assert_eq!(prod.send_with::<Msg>(|_| false, |_| {}).err(), Some(Full));
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
            let (prod, mut cons) = MpscRing::init(&mut pool, 64, cap, count).unwrap().split();
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 1, 3).unwrap().split();
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 1, 2).unwrap().split();
        send(&prod, 0, 2);
        assert_eq!(prod.segment(), 1);
        assert_eq!(prod.send_with::<Msg>(|_| false, |_| {}).err(), Some(Full));
        // Segment 0 is read, but given back only at the next
        // reserve, so the producer still finds nothing free.
        recv(&mut cons, 0, 1);
        assert_eq!(free_segments(&prod), 0);
        assert_eq!(prod.send_with::<Msg>(|_| false, |_| {}).err(), Some(Full));
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 2, 1).unwrap().split();
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
            .send_with::<Msg>(
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
        prod.send_with::<Msg>(|_| panic!("policy consulted with room available"), |_| {})
            .unwrap();
    }

    #[test]
    // Dropping the guard is the behavior under test.
    #[allow(clippy::drop_non_drop)]
    fn abandoned_read_guard_redelivers() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 1, 2).unwrap().split();
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
    fn panicking_fill_tombstones_and_skips() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 4, 2).unwrap().split();
        // A committed message, a panicked fill, another committed
        // message.
        send(&prod, 0, 1);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prod.send_with::<Msg>(
                |_| false,
                |m| {
                    m.seq = 99;
                    panic!("fill panics");
                },
            )
        }));
        assert!(unwound.is_err());
        send(&prod, 1, 2);
        recv(&mut cons, 0, 2);
        assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
        // The skipped slot is claimable again: a full lap of the
        // segment still fits 4 messages before the switch.
        send(&prod, 2, 6);
        assert_eq!(prod.switches(), 0);
        send(&prod, 6, 7);
        assert_eq!(prod.switches(), 1);
        recv(&mut cons, 2, 7);
        assert_eq!(cons.switches(), 1);
    }

    #[test]
    fn tombstone_before_a_seal_is_skipped() {
        // The last slot of a segment tombstoned, then the switch:
        // the consumer skips it, sees the seal at the end position,
        // and follows.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 2, 2).unwrap().split();
        send(&prod, 0, 1);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prod.send_with::<Msg>(|_| false, |_: &mut Msg| panic!("fill panics"))
        }));
        assert!(unwound.is_err());
        send(&prod, 1, 3);
        assert_eq!(prod.segment(), 1);
        recv(&mut cons, 0, 3);
        assert_eq!(cons.segment(), 1);
        assert_eq!(cons.switches(), 1);
        assert_eq!(free_segments(&prod), 1);
    }

    #[test]
    fn positions_survive_the_wrap() {
        // Both sides two positions shy of the 26-bit wrap in every
        // segment, each segment's seqs claimable for the lap that
        // starts there, then several laps across it.
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = MpscRing::init(&mut pool, 64, 4, 3).unwrap();
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
        let (prod, mut cons) = ring.split();
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 2, 2).unwrap().split();
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
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, cap, seg_count)
            .unwrap()
            .split();
        std::thread::scope(|s| {
            for p in 0..producers {
                let prod = prod.clone();
                s.spawn(move || {
                    for i in 0..count {
                        prod.send_with::<Msg>(crate::policy::spin, |m| {
                            m.seq = i;
                            m.val = p;
                        })
                        .unwrap(); // OK: policy::spin never gives up
                    }
                });
            }
            let cons = &mut cons;
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
        // Every seal was consumed once, and every segment but the
        // current one is back.
        assert_eq!(cons.switches(), prod.switches());
        assert_eq!(free_segments(&prod).count_ones(), seg_count - 1);
        assert_eq!(prod.segment(), cons.segment());
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
    fn with_ring(count: u32, depth: u32, f: impl FnOnce(MpscProducer<'_>, MpscConsumer<'_>)) {
        let buf = segment_size(64, depth);
        let bytes = size_of::<PoolHeader>() as u64 + buf * count as u64;
        let mut store = vec![Line([0; CACHE_LINE_SIZE]); bytes.div_ceil(64) as usize];
        let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), buf as u32, count).unwrap();
        let (prod, cons) = MpscRing::init(&mut pool, 64, depth, count).unwrap().split();
        f(prod, cons);
    }

    #[test]
    fn every_count_and_depth_fills_and_drains() {
        // With the consumer idle a producer writes into every
        // segment in turn, switching once between each pair, and
        // the consumer follows it through the same switches.
        let (counts, depths, _) = matrix();
        for &count in &counts {
            for &depth in &depths {
                with_ring(count, depth, |prod, mut cons| {
                    let total = (count * depth) as u64;
                    let mut used = 1u32 << prod.segment();
                    for i in 0..total {
                        send(&prod, i, i + 1);
                        used |= 1 << prod.segment();
                    }
                    assert_eq!(prod.send_with::<Msg>(|_| false, |_| {}).err(), Some(Full));
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
                with_ring(count, depth, |prod, mut cons| {
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
                    with_ring(count, depth, |prod, mut cons| {
                        std::thread::scope(|s| {
                            for p in 0..producers {
                                let prod = prod.clone();
                                s.spawn(move || {
                                    for i in 0..total {
                                        prod.send_with::<Msg>(crate::policy::spin, |m| {
                                            m.seq = i;
                                            m.val = p;
                                        })
                                        .unwrap(); // OK: policy::spin never gives up
                                    }
                                });
                            }
                            let cons = &mut cons;
                            s.spawn(move || {
                                let mut next = vec![0u64; producers as usize];
                                for _ in 0..producers * total {
                                    let msg =
                                        cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap(); // OK: policy::spin never gives up
                                    let p = msg.val as usize;
                                    assert_eq!(msg.seq, next[p], "per-producer order broken");
                                    next[p] += 1;
                                    msg.release();
                                }
                                assert!(cons.reserve_slot_with::<Msg>(|_| false).is_err());
                            });
                        });
                        assert_eq!(
                            cons.switches(),
                            prod.switches(),
                            "{count} segments at depth {depth}, {producers} producers"
                        );
                        assert_eq!(free_segments(&prod).count_ones(), count - 1);
                        assert_eq!(prod.segment(), cons.segment());
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
        // instead of cloning.
        const COUNT: u64 = if cfg!(miri) { 100 } else { 50_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (prod, mut cons) = MpscRing::init(&mut pool, 64, 4, 2).unwrap().split();
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
