//! SPSC ring v3: a ring of segments, a design built to be
//! measured rather than a proven one.
//!
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
//! - No `attach`: the ring's state spans a pool and its
//!   segments, and the endpoints are in-process.

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

/// The line at the front of every segment.
///
/// - Segment 0's holds the consumer's give-back word, the one
///   word both sides share besides the slots. The others are
///   reserved, so every segment has the same layout.
type SegmentHeader = CacheAligned<AtomicU32>;

const _: () = assert!(size_of::<SegmentHeader>() == CACHE_LINE_SIZE);

/// Bytes a segment needs: its header line, then the slots.
///
/// - The pool handed to [`Ring::init`] needs buffers at least
///   this large.
/// - Computed in u64 for the same 32-bit wrap reason as the
///   rings' region sizes.
pub fn segment_size(slot_size: u32, seg_capacity: u32) -> u64 {
    size_of::<SegmentHeader>() as u64 + slot_size as u64 * seg_capacity as u64
}

/// A ring's geometry and its segments' addresses, the state both
/// endpoints start from.
#[derive(Clone, Copy)]
struct Segments {
    /// The slot array of each segment, `seg_count` of them.
    slots: [*mut u8; MAX_SEGMENTS as usize],
    /// The consumer's give-back word, in segment 0's header.
    given: *const AtomicU32,
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
        let slot = crate::slot_ptr(self.slots[seg as usize], idx, self.mask, self.slot_size);
        // SAFETY: seg < seg_count and the slot is in bounds of
        // that segment's buffer, line-aligned, so its first word
        // is an aligned AtomicU32, shared atomic state by design.
        unsafe { &*(slot as *const AtomicU32) }
    }

    /// The body of the slot at free-running `idx` in segment
    /// `seg`, behind its [`SLOT_HEADER_BYTES`].
    fn body(&self, seg: u32, idx: u32) -> *mut u8 {
        let slot = crate::slot_ptr(self.slots[seg as usize], idx, self.mask, self.slot_size);
        // SAFETY: SLOT_HEADER_BYTES < CACHE_LINE_SIZE <= slot_size,
        // so the body starts inside the slot.
        unsafe { slot.add(SLOT_HEADER_BYTES) }
    }

    /// The consumer's give-back word.
    fn given(&self) -> &AtomicU32 {
        // SAFETY: points at segment 0's header word, which lives
        // as long as the pool region the ring borrows from.
        unsafe { &*self.given }
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
    /// Geometry and segment addresses.
    segs: Segments,
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
        for (seg, &base) in bases.iter().take(seg_count as usize).enumerate() {
            // SAFETY: base is a buffer of at least segment_size
            // bytes the pool just handed out, line-aligned, and
            // nothing else can reach it until the ring is split:
            // the header line and each slot's header are ours to
            // write.
            unsafe {
                core::ptr::write_bytes(base, 0, CACHE_LINE_SIZE);
                let seg_slots = base.add(size_of::<SegmentHeader>());
                for i in 0..seg_capacity {
                    let slot = seg_slots.add(i as usize * slot_size as usize);
                    core::ptr::write_bytes(slot, 0, SLOT_HEADER_BYTES);
                    // `seq[i] = i`: every slot claimable for lap 0.
                    (*(slot as *const AtomicU32)).store(seq_of(i), Ordering::Relaxed);
                }
                slots[seg] = seg_slots;
            }
        }
        // Segment 0's header word, the first word of its buffer.
        let given = bases[0] as *const AtomicU32;
        Ok(Ring {
            segs: Segments {
                slots,
                given,
                slot_size,
                capacity: seg_capacity,
                mask: seg_capacity - 1,
                seg_count,
            },
            _region: core::marker::PhantomData,
        })
    }

    /// Split into the producer and consumer endpoint handles.
    ///
    /// - Consuming `self` makes each handle exist at most once
    ///   per ring. Both start in segment 0, which the producer
    ///   holds as taken.
    pub fn split(self) -> (Producer<'a>, Consumer<'a>) {
        (Producer::new(self.segs), Consumer::new(self.segs))
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

    /// Test pool buffer: a segment of up to 16 one-line slots.
    const BUF: usize = CACHE_LINE_SIZE + 16 * CACHE_LINE_SIZE;

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
        // 32 one-line slots do not fit a 17-line buffer.
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
                CACHE_LINE_SIZE as u64 + 64 * cap as u64
            );
        }
    }

    #[test]
    fn one_segment_is_a_ring() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, 4, 1).unwrap().split();
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
            let (mut prod, mut cons) = Ring::init(&mut pool, 64, cap, count).unwrap().split();
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
            let (mut prod, mut cons) = Ring::init(&mut pool, 64, cap, count).unwrap().split();
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
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, 1, 2).unwrap().split();
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
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, 1, 2).unwrap().split();
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
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, 1, 2).unwrap().split();
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
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, 2, 1).unwrap().split();
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
        let (mut prod, mut cons) = ring.split();
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
        let (prod, cons) = Ring::init(&mut pool, 64, depth, count).unwrap().split();
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
        let (mut prod, mut cons) = Ring::init(&mut pool, 64, cap, count).unwrap().split();
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
