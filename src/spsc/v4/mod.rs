//! SPSC ring v4: v3's ring of segments over a ring that describes
//! itself in the region, so a second process can attach to it,
//! and v3 stays as built to measure against.
//!
//! - What v4 adds to v3: a control block at the front of segment
//!   0 (magic, layout version, geometry, the role claims word, and
//!   the table of every segment's pool buffer index), a table of
//!   offsets instead of pointers in the endpoints, [`Ring::attach`]
//!   from a pool and [`Ring::first_segment`], and the roles
//!   claimed by name, [`Ring::claim_producer`] and
//!   [`Ring::claim_consumer`], each held once anywhere by a holder
//!   the claim names. The protocol below is v3's, unchanged.
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
//! - Roles survive their holders: a destructor never touches
//!   shared memory, so an endpoint dropped or a process dead
//!   leaves its role held. Each endpoint checkpoints its state
//!   into the claims line at every segment switch and at
//!   `release`, so a claim of a released role continues where it
//!   stopped, and [`Ring::take_over_producer`] or
//!   [`Ring::take_over_consumer`] replaces a dead holder from its
//!   last switch and a scan of one segment's seq words. Nothing on
//!   the message path pays for either.
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
const LAYOUT_VERSION: u32 = 2;

/// A table entry naming no segment.
const NO_SEGMENT: u32 = u32::MAX;

/// A role word naming no holder, the role never claimed.
const ROLE_FREE: u32 = 0;

/// Set in a switch intent word while its switch is in flight:
/// the holder's stores for it are not all made.
const SWITCH_SET: u32 = 1 << 31;

/// Where an intent word holds the segment being switched into.
const SWITCH_TO_SHIFT: u32 = 8;

/// Where an intent word holds the bit the switch leaves in the
/// side's free-set word for the segment it flips.
const SWITCH_BIT_SHIFT: u32 = 16;

const _: () = assert!(MAX_SEGMENTS <= 1 << SWITCH_TO_SHIFT);
const _: () = assert!(MAX_SEGMENTS <= 1 << (SWITCH_BIT_SHIFT - SWITCH_TO_SHIFT));

/// A switch intent word: a switch from segment `from` into `to`
/// is in flight, flipping one segment's bit of the side's
/// free-set word to `bit`.
fn switch_intent(from: u32, to: u32, bit: u32) -> u32 {
    SWITCH_SET | from | (to << SWITCH_TO_SHIFT) | (bit << SWITCH_BIT_SHIFT)
}

/// A role word naming no holder, the role given back by
/// `release` with its state.
const ROLE_RELEASED: u32 = u32::MAX;

/// The first line of every segment: which ring it belongs to,
/// written by [`Ring::init`] and read back by `attach`.
///
/// - Every field is a `u32` atomic, so a process reads the line
///   with the ordering the magic's Acquire gives it.
/// - `given` is the consumer's give-back word, meaningful in
///   segment 0 only, the one word both sides share besides the
///   slots. It shares the line with the geometry because the
///   geometry is read at attach and never after.
/// - The two resume positions are each side's checkpoint of
///   where it left this segment, the private `resume` entries a
///   successor loads.
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
    /// The producer's resume position in this segment.
    prod_resume: AtomicU32,
    /// The consumer's resume position in this segment.
    cons_resume: AtomicU32,
}

/// The claims line of segment 0: who holds each role, and each
/// endpoint's checkpoint, the private state a successor loads.
///
/// - A role word is [`ROLE_FREE`], [`ROLE_RELEASED`], or the id
///   of the holder, which the app chooses and the crate never
///   interprets, so claim, release, and takeover are each one CAS
///   on it.
/// - Each switch writes its side's segment, and the producer its
///   take word, with the left segment's resume position in that
///   segment's info line. The consumer's give-back word is its
///   own checkpoint, as it is already shared. `release` adds the
///   position, the one thing a switch does not record.
/// - A switch is several stores and its holder can die between
///   any two, so it sets the side's intent word before its first
///   checkpoint store and clears it after its last shared store:
///   a clear word means the checkpoint is whole, and a set one
///   names the switch a takeover must finish or undo.
/// - Nothing on the message path reads or writes the line, only
///   the switch path and `release`.
#[repr(C)]
struct Claims {
    /// The producer role's holder.
    producer: AtomicU32,
    /// The consumer role's holder.
    consumer: AtomicU32,
    /// The producer's segment.
    prod_cur: AtomicU32,
    /// The producer's position in its segment, written by
    /// `release`.
    prod_pos: AtomicU32,
    /// The producer's segment-take word.
    prod_taken: AtomicU32,
    /// The producer's switch in flight, `0` when none.
    prod_switch: AtomicU32,
    /// The consumer's segment.
    cons_cur: AtomicU32,
    /// The consumer's position in its segment, written by
    /// `release`.
    cons_pos: AtomicU32,
    /// The consumer's switch in flight, `0` when none.
    cons_switch: AtomicU32,
}

/// The four lines at the front of every segment: the ring's
/// control block in segment 0, and the same layout in the others
/// so every segment's slots start at one offset.
///
/// - The pool buffer index of each segment is in the table, so a
///   process holding the pool and segment 0's index finds every
///   segment: the offsets-only rule, applied to the ring's table.
/// - The claims are their own line, so the CAS that takes a role
///   never shares a line with `given`.
#[repr(C)]
struct SegmentHeader {
    /// Line 0: the ring's identity and geometry, `given`, and the
    /// segment's resume positions.
    info: CacheAligned<Info>,
    /// Line 1: the role claims and checkpoints, segment 0's only.
    claims: CacheAligned<Claims>,
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
    /// the claims line live there.
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

    /// The claims line, the roles and the checkpoints.
    fn claims(&self) -> &Claims {
        &self.header0().claims
    }

    /// Segment `seg`'s info line, where its resume positions are.
    fn info(&self, seg: u32) -> &Info {
        // SAFETY: seg < seg_count, and the segment's header sits
        // just ahead of its slots, the front of a buffer that
        // lives as long as the pool region, every field atomic.
        unsafe {
            &(*(self
                .base
                .add(self.slots[seg as usize] - size_of::<SegmentHeader>())
                as *const SegmentHeader))
                .info
        }
    }

    /// Checkpoint a producer switch from `from` into `to`, ahead
    /// of the MOVED commit that publishes it.
    ///
    /// - `left_at` is the position after the MOVED slot, where a
    ///   later stint in `from` starts, and `taken` the take word
    ///   with `to` flipped.
    /// - The resume position goes first, ahead of the intent: an
    ///   entry for the segment still current is read by nobody.
    /// - Every store is Release, so a reader that sees one sees
    ///   the stores before it, the intent included.
    /// - Out of line and cold, as its two siblings are, so the
    ///   message path's code stays what it was without them.
    #[cold]
    #[inline(never)]
    fn producer_switch(&self, from: u32, to: u32, left_at: u32, taken: u32) {
        let c = self.claims();
        self.info(from)
            .prod_resume
            .store(left_at, Ordering::Release);
        c.prod_switch.store(
            switch_intent(from, to, (taken >> to) & 1),
            Ordering::Release,
        );
        c.prod_taken.store(taken, Ordering::Release);
        c.prod_cur.store(to, Ordering::Release);
    }

    /// Checkpoint a consumer switch from `from` into `to`, ahead
    /// of the release that frees the MOVED slot, as
    /// [`producer_switch`](Segments::producer_switch) does.
    ///
    /// - `given` is the give-back word with `from` flipped, stored
    ///   to the shared word after the release, so the intent
    ///   records the bit it leaves for `from`.
    #[cold]
    #[inline(never)]
    fn consumer_switch(&self, from: u32, to: u32, left_at: u32, given: u32) {
        let c = self.claims();
        self.info(from)
            .cons_resume
            .store(left_at, Ordering::Release);
        c.cons_switch.store(
            switch_intent(from, to, (given >> from) & 1),
            Ordering::Release,
        );
        c.cons_cur.store(to, Ordering::Release);
    }

    /// Clear an intent word once its switch's last shared store
    /// is made.
    #[cold]
    #[inline(never)]
    fn switch_done(intent: &AtomicU32) {
        // Release: a reader that sees the word clear sees the
        // switch's stores, the seq word's among them.
        intent.store(0, Ordering::Release);
    }

    /// Take a set intent word apart, refusing segments the ring
    /// does not have.
    fn intent(&self, word: u32) -> Result<Intent, Error> {
        let from = word & SEG_MASK;
        let to = (word >> SWITCH_TO_SHIFT) & SEG_MASK;
        let bit = (word >> SWITCH_BIT_SHIFT) & 1;
        let fits = word & SWITCH_SET != 0 && from < self.seg_count && to < self.seg_count;
        if !fits || from == to {
            return Err(Error::BadCheckpoint);
        }
        Ok(Intent { from, to, bit })
    }

    /// Every segment's resume position, one side's, from the info
    /// lines.
    fn resume_table(&self, side: impl Fn(&Info) -> &AtomicU32) -> [u32; MAX_SEGMENTS as usize] {
        let mut resume = [0; MAX_SEGMENTS as usize];
        for seg in 0..self.seg_count {
            resume[seg as usize] = side(self.info(seg)).load(Ordering::Acquire);
        }
        resume
    }

    /// The producer's state from the region, for a claim of a
    /// released role (`released`) or a takeover of a held one.
    ///
    /// - A set intent names the switch the holder died inside. Its
    ///   MOVED commit never happened when the slot it left is still
    ///   claimable: undo, and continue at that slot. Otherwise
    ///   finish, at the start of the segment entered, where the
    ///   holder had written nothing. Either way the repaired
    ///   checkpoint is written back and the intent cleared.
    /// - A clear intent leaves `cur` and `taken` exact, and the
    ///   position is the one `release` wrote or, for a takeover,
    ///   the scan's.
    #[cold]
    fn load_producer(&self, released: bool) -> Result<Checkpoint, Error> {
        let c = self.claims();
        let mut cur = c.prod_cur.load(Ordering::Acquire);
        let mut taken = c.prod_taken.load(Ordering::Acquire);
        let intent = c.prod_switch.load(Ordering::Acquire);
        let resume = self.resume_table(|info| &info.prod_resume);
        let pos = if intent != 0 {
            let Intent { from, to, bit } = self.intent(intent)?;
            let p = resume[from as usize].wrapping_sub(1);
            let pos = if self.seq(from, p).load(Ordering::Acquire) == seq_of(p) {
                cur = from;
                taken = (taken & !(1 << to)) | ((bit ^ 1) << to);
                p
            } else {
                cur = to;
                taken = (taken & !(1 << to)) | (bit << to);
                resume[to as usize]
            };
            c.prod_taken.store(taken, Ordering::Release);
            c.prod_cur.store(cur, Ordering::Release);
            Segments::switch_done(&c.prod_switch);
            pos
        } else if cur >= self.seg_count {
            return Err(Error::BadCheckpoint);
        } else if released {
            c.prod_pos.load(Ordering::Acquire)
        } else {
            // The window of positions the slots last held ends just
            // before the producer's.
            let (oldest, _) = self.window(cur)?;
            lift(resume[cur as usize], oldest.wrapping_add(self.capacity))
        };
        Ok(Checkpoint {
            cur,
            pos,
            free_set: taken,
            resume,
        })
    }

    /// The consumer's state from the region, as
    /// [`load_producer`](Segments::load_producer) loads the
    /// producer's.
    ///
    /// - A set intent: the release never happened when the slot it
    ///   left still holds the MOVED commit, so undo and read that
    ///   message again. Otherwise finish: the give-back word gets
    ///   the bit the intent names, which the holder may have died
    ///   before storing, and the consumer starts where it enters.
    /// - A clear intent: the position `release` wrote or, for a
    ///   takeover, the oldest committed slot the scan finds.
    #[cold]
    fn load_consumer(&self, released: bool) -> Result<Checkpoint, Error> {
        let c = self.claims();
        let mut cur = c.cons_cur.load(Ordering::Acquire);
        let mut given = self.given().load(Ordering::Acquire);
        let intent = c.cons_switch.load(Ordering::Acquire);
        let resume = self.resume_table(|info| &info.cons_resume);
        let pos = if intent != 0 {
            let Intent { from, to, bit } = self.intent(intent)?;
            let m = resume[from as usize].wrapping_sub(1);
            let moved =
                seq_of(m.wrapping_add(self.capacity).wrapping_add(1)) | MOVED | (to << SEG_SHIFT);
            let pos = if self.seq(from, m).load(Ordering::Acquire) == moved {
                cur = from;
                given = (given & !(1 << from)) | ((bit ^ 1) << from);
                m
            } else {
                cur = to;
                given = (given & !(1 << from)) | (bit << from);
                resume[to as usize]
            };
            self.given().store(given, Ordering::Release);
            c.cons_cur.store(cur, Ordering::Release);
            Segments::switch_done(&c.cons_switch);
            pos
        } else if cur >= self.seg_count {
            return Err(Error::BadCheckpoint);
        } else if released {
            c.cons_pos.load(Ordering::Acquire)
        } else {
            let (oldest, unread) = self.window(cur)?;
            let first = oldest.wrapping_add(self.capacity).wrapping_sub(unread);
            lift(resume[cur as usize], first)
        };
        Ok(Checkpoint {
            cur,
            pos,
            free_set: given,
            resume,
        })
    }

    /// The window of positions segment `seg`'s slots last held,
    /// from their seq words: its oldest position, modulo
    /// `2^SEQ_BITS`, and how many of its newest are committed.
    ///
    /// - Slot `i` last held position `q`, `q = i` modulo the depth,
    ///   released (seq `q + M`) or committed (seq `q + M + 1`), and
    ///   the positions are the `M` before the producer's in the
    ///   segment, the committed ones the newest, from the
    ///   consumer's on. The seq's low bits tell released from
    ///   committed, which at depth 1 they cannot.
    /// - A live producer committing while the scan reads can make
    ///   one read disagree with the rest, so the scan runs again,
    ///   and a live consumer's releases change no position. With
    ///   the other side dead the words settle, the producer's
    ///   within a ring's capacity of commits, so a bounded number
    ///   of disagreeing scans means words the ring never wrote.
    #[cold]
    fn window(&self, seg: u32) -> Result<(u32, u32), Error> {
        if self.capacity < 2 {
            return Err(Error::BadCapacity);
        }
        const SCANS: u32 = 1024;
        for _ in 0..SCANS {
            if let Some(found) = self.scan(seg) {
                return Ok(found);
            }
        }
        Err(Error::BadCheckpoint)
    }

    /// One scan of [`window`](Segments::window), `None` when the
    /// words do not form one window.
    fn scan(&self, seg: u32) -> Option<(u32, u32)> {
        let m = self.capacity;
        // Offsets from slot 0's position, signed over SEQ_BITS.
        let offset = |q: u32, base: u32| {
            let d = q.wrapping_sub(base) & SEQ_MASK;
            if d >= 1 << (SEQ_BITS - 1) {
                d as i64 - (1i64 << SEQ_BITS)
            } else {
                d as i64
            }
        };
        let (mut base, mut lo, mut hi) = (0u32, i64::MAX, i64::MIN);
        let (mut unread, mut lo_unread) = (0u32, i64::MAX);
        for i in 0..m {
            let v = self.seq(seg, i).load(Ordering::Acquire) & SEQ_MASK;
            let (q, committed) = if v & self.mask == i {
                (v.wrapping_sub(m) & SEQ_MASK, false)
            } else if v & self.mask == (i + 1) & self.mask {
                (v.wrapping_sub(m).wrapping_sub(1) & SEQ_MASK, true)
            } else {
                return None;
            };
            if i == 0 {
                base = q;
            }
            let d = offset(q, base);
            lo = lo.min(d);
            hi = hi.max(d);
            if committed {
                unread += 1;
                lo_unread = lo_unread.min(d);
            }
        }
        // M distinct positions, one per slot, span exactly M, and
        // the committed ones are the newest.
        let whole = hi - lo == (m - 1) as i64;
        let tail = unread == 0 || lo_unread == hi - unread as i64 + 1;
        if !whole || !tail {
            return None;
        }
        Some((base.wrapping_add(lo as u32) & SEQ_MASK, unread))
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

    /// The producer role's word in the claims line.
    fn producer_role(&self) -> &AtomicU32 {
        &self.header0().claims.producer
    }

    /// The consumer role's word in the claims line.
    fn consumer_role(&self) -> &AtomicU32 {
        &self.header0().claims.consumer
    }

    /// Give `role` back as released, if `holder` still holds it.
    ///
    /// - A CAS from the holder's own id, so an endpoint whose role
    ///   was taken over releases nothing.
    fn release_role(role: &AtomicU32, holder: u32) {
        // Release: the next claimant's AcqRel sees everything this
        // endpoint did.
        let _ = role.compare_exchange(holder, ROLE_RELEASED, Ordering::Release, Ordering::Relaxed);
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

/// A ring of segments over the application's pool, its two roles
/// claimed with [`Ring::claim_producer`] and
/// [`Ring::claim_consumer`].
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
                    // The producer starts holding segment 0, so its
                    // checkpoint does too.
                    header.claims.prod_taken.store(1, Ordering::Relaxed);
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
    /// - A role claimed from the attached ring starts where the
    ///   role's state says: at the start for a role never claimed,
    ///   where it stopped for a released one, and from the last
    ///   switch for one taken over.
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

    /// Claim the producer role for holder `id`.
    ///
    /// - `id` is the app's name for the holder, any `u32` but `0`
    ///   and `u32::MAX`, which are [`Error::BadHolder`]. The crate
    ///   records it and never interprets it.
    /// - One CAS on the control block's role word, so a role held
    ///   anywhere, in this process or another, is
    ///   [`Error::RoleTaken`]. Nothing on the message path reads
    ///   the word.
    /// - A role never claimed starts in segment 0 at position 0,
    ///   which it holds as taken. A released one resumes from the
    ///   checkpoint its [`release`](Producer::release) wrote, in
    ///   this process or another, exactly where it stopped.
    /// - The role stays held until [`Producer::release`]:
    ///   dropping the endpoint writes nothing.
    /// - A checkpoint that names no segment of the ring is
    ///   [`Error::BadCheckpoint`], with the role left as it was.
    pub fn claim_producer(&self, id: u32) -> Result<Producer<'a>, Error> {
        self.producer_for(id, false)
    }

    /// Claim the consumer role for holder `id`, the counterpart of
    /// [`claim_producer`](Ring::claim_producer).
    pub fn claim_consumer(&self, id: u32) -> Result<Consumer<'a>, Error> {
        self.consumer_for(id, false)
    }

    /// Take the producer role over for holder `id`, replacing its
    /// holder, whom the caller vouches is gone.
    ///
    /// - The crate never judges whether a holder is alive: the
    ///   takeover is the app's call, a supervisor's, and replacing
    ///   a live holder breaks the ring's one-producer contract.
    /// - One CAS from the role word as loaded, so of two takeovers
    ///   racing one wins and the other is [`Error::RoleTaken`]. A
    ///   free or released role is taken as a claim takes it.
    /// - A held role continues from the checkpoint of its holder's
    ///   last switch. A switch it died inside is finished or
    ///   undone by the one slot it left, and the position is found
    ///   by a scan of the segment's seq words, so at most the one
    ///   slot the dead holder reserved and never committed is
    ///   written again.
    /// - A ring of one-slot segments cannot place a position by
    ///   its seq words, so a held role of one is
    ///   [`Error::BadCapacity`], and a checkpoint or seq words the
    ///   ring could not have written are [`Error::BadCheckpoint`],
    ///   each with the role left as it was.
    pub fn take_over_producer(&self, id: u32) -> Result<Producer<'a>, Error> {
        self.producer_for(id, true)
    }

    /// Take the consumer role over for holder `id`, the
    /// counterpart of [`take_over_producer`](Ring::take_over_producer).
    ///
    /// - The replacement loses nothing: a message its holder read
    ///   and never released is still committed, and is read again.
    pub fn take_over_consumer(&self, id: u32) -> Result<Consumer<'a>, Error> {
        self.consumer_for(id, true)
    }

    /// Take the producer role and load its state, undoing the
    /// take when the state cannot be loaded.
    fn producer_for(&self, id: u32, take_over: bool) -> Result<Producer<'a>, Error> {
        let role = self.segs.producer_role();
        let before = take_role(role, id, take_over)?;
        if before == ROLE_FREE {
            return Ok(Producer::new(self.segs, id));
        }
        match self.segs.load_producer(before == ROLE_RELEASED) {
            Ok(cp) => Ok(Producer::resume(self.segs, id, cp)),
            Err(e) => {
                untake_role(role, id, before);
                Err(e)
            }
        }
    }

    /// Take the consumer role and load its state, as
    /// [`producer_for`](Ring::producer_for) does.
    fn consumer_for(&self, id: u32, take_over: bool) -> Result<Consumer<'a>, Error> {
        let role = self.segs.consumer_role();
        let before = take_role(role, id, take_over)?;
        if before == ROLE_FREE {
            return Ok(Consumer::new(self.segs, id));
        }
        match self.segs.load_consumer(before == ROLE_RELEASED) {
            Ok(cp) => Ok(Consumer::resume(self.segs, id, cp)),
            Err(e) => {
                untake_role(role, id, before);
                Err(e)
            }
        }
    }

    /// The pool buffer index of segment 0, where the ring's
    /// control block is: what a process hands to another so it
    /// can find the ring in the same pool.
    pub fn first_segment(&self) -> u32 {
        self.first_segment
    }
}

/// Write holder `id` into `role`, returning the word it replaced.
///
/// - A claim takes a free or released role, a takeover any role.
/// - One attempt: a CAS that fails lost to another claim or
///   takeover, and trying again would replace that winner.
fn take_role(role: &AtomicU32, id: u32, take_over: bool) -> Result<u32, Error> {
    if id == ROLE_FREE || id == ROLE_RELEASED {
        return Err(Error::BadHolder);
    }
    let before = role.load(Ordering::Acquire);
    if !take_over && before != ROLE_FREE && before != ROLE_RELEASED {
        return Err(Error::RoleTaken);
    }
    // AcqRel: a take that succeeds sees the checkpoint the role's
    // last holder wrote, and publishes its own id. A failed CAS
    // writes nothing, so it needs no undo.
    role.compare_exchange(before, id, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| Error::RoleTaken)
}

/// The free-running position nearest after `reference` whose low
/// [`SEQ_BITS`] are `seq`: the scan knows a position only modulo
/// `2^SEQ_BITS`, and the segment's resume position is where the
/// endpoint's stint there began.
fn lift(reference: u32, seq: u32) -> u32 {
    reference.wrapping_add(seq.wrapping_sub(reference) & SEQ_MASK)
}

/// Put `role` back to the word a take by `id` replaced, when the
/// state it took could not be loaded.
fn untake_role(role: &AtomicU32, id: u32, before: u32) {
    let _ = role.compare_exchange(id, before, Ordering::Release, Ordering::Relaxed);
}

/// An endpoint's private state as a successor loads it from the
/// region.
pub(super) struct Checkpoint {
    /// The segment the endpoint is in.
    pub(super) cur: u32,
    /// Its free-running position there.
    pub(super) pos: u32,
    /// The producer's take word or the consumer's give-back word.
    pub(super) free_set: u32,
    /// Where each segment was left.
    pub(super) resume: [u32; MAX_SEGMENTS as usize],
}

/// A switch intent word taken apart: the segment left, the
/// segment entered, and the free-set bit the switch leaves.
struct Intent {
    from: u32,
    to: u32,
    bit: u32,
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

    /// The producer's holder id in the tests.
    const PROD_ID: u32 = 1;

    /// The consumer's holder id in the tests.
    const CONS_ID: u32 = 2;

    /// Both roles of `ring`, the in-process pair.
    fn endpoints<'a>(ring: &Ring<'a>) -> (Producer<'a>, Consumer<'a>) {
        (
            ring.claim_producer(PROD_ID).unwrap(),
            ring.claim_consumer(CONS_ID).unwrap(),
        )
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

    /// The region's checkpoint agrees with both endpoints' private
    /// state, every part a switch writes, and no switch is in
    /// flight.
    fn assert_checkpoint(prod: &Producer<'_>, cons: &Consumer<'_>) {
        let (p, c) = (&prod.st, &cons.st);
        let claims = p.segs.claims();
        let load = |w: &AtomicU32| w.load(Ordering::Acquire);
        assert_eq!(load(&claims.prod_switch), 0);
        assert_eq!(load(&claims.cons_switch), 0);
        assert_eq!(load(&claims.prod_cur), p.cur);
        assert_eq!(load(&claims.prod_taken), p.taken);
        assert_eq!(load(&claims.cons_cur), c.cur);
        assert_eq!(load(p.segs.given()), c.given);
        for seg in 0..p.segs.seg_count {
            let info = p.segs.info(seg);
            assert_eq!(load(&info.prod_resume), p.resume[seg as usize]);
            assert_eq!(load(&info.cons_resume), c.resume[seg as usize]);
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
        // Both roles free, no switch in flight, and the checkpoint
        // the start state: segment 0 at position 0, held as taken.
        let c = &header0.claims;
        for word in [
            &c.producer,
            &c.consumer,
            &c.prod_cur,
            &c.prod_pos,
            &c.prod_switch,
            &c.cons_cur,
            &c.cons_pos,
            &c.cons_switch,
        ] {
            assert_eq!(word.load(Ordering::Relaxed), 0);
        }
        assert_eq!(c.prod_taken.load(Ordering::Relaxed), 1);
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
            assert_eq!(info.prod_resume.load(Ordering::Relaxed), 0);
            assert_eq!(info.cons_resume.load(Ordering::Relaxed), 0);
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
                assert_checkpoint(&prod, &cons);
                recv(&mut cons, next, next + n);
                assert_checkpoint(&prod, &cons);
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

    #[test]
    // Dropping the endpoint is the behavior under test. It has no
    // Drop impl by design (a destructor never touches shared
    // memory).
    #[allow(clippy::drop_non_drop)]
    fn claims_name_their_holder() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 2).unwrap();
        let roles = |ring: &Ring<'_>| {
            (
                ring.segs.producer_role().load(Ordering::Relaxed),
                ring.segs.consumer_role().load(Ordering::Relaxed),
            )
        };
        // The two ids that name no holder are refused.
        for id in [ROLE_FREE, ROLE_RELEASED] {
            assert_eq!(ring.claim_producer(id).err(), Some(Error::BadHolder));
            assert_eq!(ring.claim_consumer(id).err(), Some(Error::BadHolder));
        }
        assert_eq!(roles(&ring), (ROLE_FREE, ROLE_FREE));
        let (mut prod, mut cons) = endpoints(&ring);
        assert_eq!(roles(&ring), (PROD_ID, CONS_ID));
        assert_eq!(ring.claim_producer(7).err(), Some(Error::RoleTaken));
        assert_eq!(ring.claim_consumer(7).err(), Some(Error::RoleTaken));
        send(&mut prod, 0, 3);
        recv(&mut cons, 0, 3);
        // A dropped endpoint writes nothing: its role stays held.
        drop(prod);
        assert_eq!(roles(&ring), (PROD_ID, CONS_ID));
        assert_eq!(ring.claim_producer(7).err(), Some(Error::RoleTaken));
        // A released one is given back and claimed again, and the
        // dropped one is taken over, both continuing the ring.
        cons.release();
        assert_eq!(roles(&ring), (PROD_ID, ROLE_RELEASED));
        let mut cons = ring.claim_consumer(7).unwrap();
        let mut prod = ring.take_over_producer(8).unwrap();
        assert_eq!(roles(&ring), (8, 7));
        send(&mut prod, 3, 6);
        recv(&mut cons, 3, 6);
    }

    #[test]
    fn release_checkpoints_the_position() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 3).unwrap();
        let (mut prod, mut cons) = endpoints(&ring);
        // Past two switches, the consumer a message behind.
        send(&mut prod, 0, 10);
        recv(&mut cons, 0, 9);
        assert_checkpoint(&prod, &cons);
        let (p_pos, c_pos) = (prod.st.pos, cons.st.pos);
        assert_ne!(p_pos, c_pos);
        prod.release();
        cons.release();
        let claims = ring.segs.claims();
        assert_eq!(claims.prod_pos.load(Ordering::Acquire), p_pos);
        assert_eq!(claims.cons_pos.load(Ordering::Acquire), c_pos);
    }

    /// Send `*sent..to`, receiving into `*seen` whenever the ring
    /// is full, so a stream of any length moves through a ring of
    /// any geometry, order checked on the way.
    fn pump(
        prod: &mut Producer<'_>,
        cons: &mut Consumer<'_>,
        sent: &mut u64,
        seen: &mut u64,
        to: u64,
    ) {
        while *sent < to {
            match prod.reserve_slot_with::<Msg>(|_| false) {
                Ok(mut slot) => {
                    slot.seq = *sent;
                    slot.val = *sent * 10;
                    slot.commit();
                    *sent += 1;
                }
                Err(Full) => {
                    recv(cons, *seen, *seen + 1);
                    *seen += 1;
                }
            }
        }
    }

    /// The geometries the takeover tests run, a few under Miri.
    fn takeover_geometries() -> Vec<(u32, u32)> {
        if cfg!(miri) {
            vec![(2, 2), (4, 3)]
        } else {
            vec![(2, 2), (2, 5), (4, 2), (4, 3), (8, 4), (16, 2)]
        }
    }

    /// Where the takeover tests stop the first holder: past the
    /// first switch, at every count through two ring-fulls, or
    /// under Miri three of them.
    fn takeover_stops(cap: u32, count: u32) -> Vec<u64> {
        let (cap, full) = (cap as u64, (cap * count) as u64);
        if cfg!(miri) {
            vec![cap + 1, cap * 2 + 1, full + 3]
        } else {
            (cap + 1..=2 * full + 3).collect()
        }
    }

    /// Run `f` on two attached handles over one freshly
    /// initialized ring, as two processes hold it.
    fn with_attached(cap: u32, count: u32, f: impl FnOnce(&Ring<'_>, &Ring<'_>)) {
        let mut r = Region::new();
        let (base, len) = region(&mut r);
        let first = with_init_pool(base, len, |pool| {
            Ring::init(pool, 64, cap, count).unwrap().first_segment()
        });
        let (b1, b2) = (attach_pool(base, len), attach_pool(base, len));
        // SAFETY: first came from first_segment of a ring over this
        // pool, whose segments are still the ring's.
        let ring_1 = unsafe { Ring::attach(&b1, first) }.unwrap();
        // SAFETY: as above.
        let ring_2 = unsafe { Ring::attach(&b2, first) }.unwrap();
        f(&ring_1, &ring_2);
    }

    #[test]
    fn released_roles_resume_where_they_stopped() {
        // iiac-perf's scenario: three messages each way on two
        // segments of four slots, both roles released and claimed
        // again, from the same handle and from a second one, and
        // the ring continues across a switch.
        with_attached(4, 2, |ring_1, ring_2| {
            let (mut prod, mut cons) = endpoints(ring_1);
            send(&mut prod, 0, 3);
            recv(&mut cons, 0, 3);
            prod.release();
            cons.release();
            let mut prod = ring_1.claim_producer(PROD_ID).unwrap();
            let mut cons = ring_1.claim_consumer(CONS_ID).unwrap();
            send(&mut prod, 3, 7);
            recv(&mut cons, 3, 7);
            assert!(prod.switches() > 0);
            prod.release();
            cons.release();
            // The producer from the second handle, the consumer from
            // the first, a message left unread across the handoff.
            let mut prod = ring_2.claim_producer(PROD_ID + 10).unwrap();
            assert_eq!(ring_1.claim_producer(7).err(), Some(Error::RoleTaken));
            send(&mut prod, 7, 12);
            let mut cons = ring_1.claim_consumer(CONS_ID + 10).unwrap();
            recv(&mut cons, 7, 11);
            cons.release();
            let mut cons = ring_2.claim_consumer(CONS_ID).unwrap();
            recv(&mut cons, 11, 12);
            let (mut sent, mut seen) = (12, 12);
            pump(&mut prod, &mut cons, &mut sent, &mut seen, 40);
            recv(&mut cons, seen, sent);
        });
    }

    #[test]
    // Forgetting the guards and the endpoint is the death under
    // test: a process that dies runs no destructor. They have none
    // by design, so forgetting is dropping, and `forget` says what
    // the test means and stays a death if one is ever added.
    #[allow(clippy::forget_non_drop)]
    fn a_dead_consumer_is_taken_over() {
        for (cap, count) in takeover_geometries() {
            for stop in takeover_stops(cap, count) {
                with_attached(cap, count, |ring_1, ring_2| {
                    let (mut prod, mut cons) = endpoints(ring_1);
                    let (mut sent, mut seen) = (0u64, 0u64);
                    pump(&mut prod, &mut cons, &mut sent, &mut seen, stop);
                    recv(&mut cons, seen, sent - 1);
                    seen = sent - 1;
                    assert!(prod.switches() > 0, "{cap}x{count} stop {stop}");
                    // Dead holding a read of the one message left.
                    let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
                    assert_eq!(msg.seq, seen);
                    core::mem::forget(msg);
                    core::mem::forget(cons);
                    let mut cons = ring_2.take_over_consumer(CONS_ID + 1).unwrap();
                    assert_eq!(ring_2.claim_consumer(9).err(), Some(Error::RoleTaken));
                    pump(
                        &mut prod,
                        &mut cons,
                        &mut sent,
                        &mut seen,
                        stop + 3 * cap as u64,
                    );
                    recv(&mut cons, seen, sent);
                    assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                });
            }
        }
    }

    #[test]
    // As in `a_dead_consumer_is_taken_over`: forget is death.
    #[allow(clippy::forget_non_drop)]
    fn a_dead_producer_is_taken_over() {
        for (cap, count) in takeover_geometries() {
            for stop in takeover_stops(cap, count) {
                with_attached(cap, count, |ring_1, ring_2| {
                    let (mut prod, mut cons) = endpoints(ring_1);
                    let (mut sent, mut seen) = (0u64, 0u64);
                    pump(&mut prod, &mut cons, &mut sent, &mut seen, stop);
                    assert!(prod.switches() > 0, "{cap}x{count} stop {stop}");
                    // Room for one more, reserved, written, never
                    // committed. With no segment free the producer
                    // waits on its own next slot, so read until it
                    // frees.
                    while prod.reserve_slot_with::<Msg>(|_| false).is_err() {
                        recv(&mut cons, seen, seen + 1);
                        seen += 1;
                    }
                    let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
                    slot.seq = 999_999;
                    core::mem::forget(slot);
                    core::mem::forget(prod);
                    let mut prod = ring_2.take_over_producer(PROD_ID + 1).unwrap();
                    assert_eq!(ring_2.claim_producer(9).err(), Some(Error::RoleTaken));
                    // The stream resumes: every message in order, the
                    // uncommitted one never seen.
                    pump(
                        &mut prod,
                        &mut cons,
                        &mut sent,
                        &mut seen,
                        stop + 3 * cap as u64,
                    );
                    recv(&mut cons, seen, sent);
                    assert_eq!(cons.reserve_slot_with::<Msg>(|_| false).err(), Some(Empty));
                });
            }
        }
    }

    #[test]
    // As in `a_dead_consumer_is_taken_over`: forget is death.
    #[allow(clippy::forget_non_drop)]
    fn a_consumer_is_taken_over_while_the_producer_streams() {
        // The scan reads seq words a live producer is committing, on
        // its own thread, spinning when the ring fills.
        let total: u64 = if cfg!(miri) { 60 } else { 200_000 };
        for (cap, count) in takeover_geometries() {
            with_attached(cap, count, |ring_1, ring_2| {
                let (mut prod, mut cons) = endpoints(ring_1);
                std::thread::scope(|s| {
                    s.spawn(move || {
                        for i in 0..total {
                            let mut slot =
                                prod.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                            slot.seq = i;
                            slot.commit();
                        }
                    });
                    let mut next = 0u64;
                    for round in 1..=4u64 {
                        while next < total * round / 5 {
                            let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                            assert_eq!(msg.seq, next);
                            msg.release();
                            next += 1;
                        }
                        core::mem::forget(cons);
                        cons = ring_2.take_over_consumer(CONS_ID + round as u32).unwrap();
                    }
                    while next < total {
                        let msg = cons.reserve_slot_with::<Msg>(crate::policy::spin).unwrap();
                        assert_eq!(msg.seq, next, "{cap}x{count}");
                        msg.release();
                        next += 1;
                    }
                });
            });
        }
    }

    #[test]
    // As in `a_dead_consumer_is_taken_over`: forget is death.
    #[allow(clippy::forget_non_drop)]
    fn a_producer_dead_inside_a_switch_is_undone_or_finished() {
        // Two segments of two slots: message 1's commit switches,
        // with message 0 unread.
        for finished in [false, true] {
            with_attached(2, 2, |ring_1, ring_2| {
                let (mut prod, mut cons) = endpoints(ring_1);
                send(&mut prod, 0, 1);
                if finished {
                    // The MOVED commit made, the intent not cleared.
                    send(&mut prod, 1, 2);
                    assert_eq!(prod.st.cur, 1);
                    let intent = switch_intent(0, 1, (prod.st.taken >> 1) & 1);
                    ring_1
                        .segs
                        .claims()
                        .prod_switch
                        .store(intent, Ordering::Release);
                } else {
                    // The checkpoint and intent written, the commit not.
                    let mut slot = prod.reserve_slot_with::<Msg>(|_| false).unwrap();
                    slot.seq = 1;
                    slot.val = 10;
                    core::mem::forget(slot);
                    let taken = prod.st.taken ^ 2;
                    ring_1.segs.producer_switch(0, 1, 2, taken);
                }
                core::mem::forget(prod);
                let mut prod = ring_2.take_over_producer(PROD_ID + 1).unwrap();
                assert_eq!(prod.st.cur, if finished { 1 } else { 0 });
                let claims = ring_2.segs.claims();
                assert_eq!(claims.prod_switch.load(Ordering::Acquire), 0);
                assert_eq!(claims.prod_cur.load(Ordering::Acquire), prod.st.cur);
                assert_eq!(claims.prod_taken.load(Ordering::Acquire), prod.st.taken);
                let (mut sent, mut seen) = (if finished { 2 } else { 1 }, 0);
                pump(&mut prod, &mut cons, &mut sent, &mut seen, 20);
                recv(&mut cons, seen, sent);
            });
        }
    }

    #[test]
    // As in `a_dead_consumer_is_taken_over`: forget is death.
    #[allow(clippy::forget_non_drop)]
    fn a_consumer_dead_inside_a_switch_is_undone_or_finished() {
        // Two segments of two slots: message 1 is the MOVED one.
        for finished in [false, true] {
            with_attached(2, 2, |ring_1, ring_2| {
                let (mut prod, mut cons) = endpoints(ring_1);
                send(&mut prod, 0, 3);
                recv(&mut cons, 0, 1);
                let given = cons.st.given;
                if finished {
                    // Released, the give-back and the clear not made.
                    recv(&mut cons, 1, 2);
                    assert_eq!(cons.st.cur, 1);
                    let bit = cons.st.given & 1;
                    ring_1.segs.given().store(given, Ordering::Release);
                    let intent = switch_intent(0, 1, bit);
                    ring_1
                        .segs
                        .claims()
                        .cons_switch
                        .store(intent, Ordering::Release);
                } else {
                    // The checkpoint and intent written, the release not.
                    let msg = cons.reserve_slot_with::<Msg>(|_| false).unwrap();
                    assert_eq!(msg.seq, 1);
                    core::mem::forget(msg);
                    ring_1.segs.consumer_switch(0, 1, 2, given ^ 1);
                }
                core::mem::forget(cons);
                let mut cons = ring_2.take_over_consumer(CONS_ID + 1).unwrap();
                assert_eq!(cons.st.cur, if finished { 1 } else { 0 });
                let claims = ring_2.segs.claims();
                assert_eq!(claims.cons_switch.load(Ordering::Acquire), 0);
                assert_eq!(ring_2.segs.given().load(Ordering::Acquire), cons.st.given);
                // Undone, message 1 is read again. Finished, the
                // segment given back is the producer's to reuse.
                let (mut sent, mut seen) = (3, if finished { 2 } else { 1 });
                pump(&mut prod, &mut cons, &mut sent, &mut seen, 20);
                recv(&mut cons, seen, sent);
            });
        }
    }

    #[test]
    // As in `a_dead_consumer_is_taken_over`: forget is death.
    #[allow(clippy::forget_non_drop)]
    fn depth_one_resumes_but_is_not_taken_over() {
        with_attached(1, 3, |ring_1, ring_2| {
            let (mut prod, mut cons) = endpoints(ring_1);
            send(&mut prod, 0, 2);
            recv(&mut cons, 0, 1);
            prod.release();
            cons.release();
            let (mut prod, mut cons) = endpoints(ring_2);
            let (mut sent, mut seen) = (2, 1);
            pump(&mut prod, &mut cons, &mut sent, &mut seen, 9);
            core::mem::forget(prod);
            // A held role at depth 1: its position is not in the
            // seq words, and the role is left as it was.
            assert_eq!(ring_1.take_over_producer(5).err(), Some(Error::BadCapacity));
            assert_eq!(ring_1.segs.producer_role().load(Ordering::Acquire), PROD_ID);
            recv(&mut cons, seen, sent);
        });
    }

    #[test]
    fn a_checkpoint_naming_no_segment_is_refused() {
        with_attached(4, 2, |ring_1, ring_2| {
            let (prod, cons) = endpoints(ring_1);
            prod.release();
            cons.release();
            let claims = ring_1.segs.claims();
            claims.prod_cur.store(2, Ordering::Release);
            assert_eq!(ring_2.claim_producer(5).err(), Some(Error::BadCheckpoint));
            assert_eq!(claims.producer.load(Ordering::Acquire), ROLE_RELEASED);
            claims.prod_cur.store(0, Ordering::Release);
            // An intent naming a segment the ring does not have.
            claims
                .cons_switch
                .store(switch_intent(0, 3, 1), Ordering::Release);
            assert_eq!(ring_2.claim_consumer(5).err(), Some(Error::BadCheckpoint));
            assert_eq!(claims.consumer.load(Ordering::Acquire), ROLE_RELEASED);
            claims.cons_switch.store(0, Ordering::Release);
            let (mut prod, mut cons) = endpoints(ring_2);
            send(&mut prod, 0, 5);
            recv(&mut cons, 0, 5);
        });
    }

    #[test]
    fn release_after_takeover_releases_nothing() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, BUF as u32, BUFS as u32).unwrap();
        let ring = Ring::init(&mut pool, 64, 4, 2).unwrap();
        let (prod, cons) = endpoints(&ring);
        // Another holder in each role word, as a takeover leaves it.
        ring.segs.producer_role().store(7, Ordering::Relaxed);
        ring.segs.consumer_role().store(8, Ordering::Relaxed);
        prod.release();
        cons.release();
        assert_eq!(ring.segs.producer_role().load(Ordering::Relaxed), 7);
        assert_eq!(ring.segs.consumer_role().load(Ordering::Relaxed), 8);
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
        // and the claims are one line for both.
        let mut prod = ring_1.claim_producer(PROD_ID).unwrap();
        assert_eq!(ring_2.claim_producer(7).err(), Some(Error::RoleTaken));
        let mut cons = ring_2.claim_consumer(CONS_ID).unwrap();
        assert_eq!(ring_1.claim_consumer(7).err(), Some(Error::RoleTaken));
        let mut next = 0u64;
        for burst in [3u64, 9, 12, 5, 12, 12, 7] {
            send(&mut prod, next, next + burst);
            recv(&mut cons, next, next + burst);
            next += burst;
        }
        assert!(prod.switches() > 3 && prod.switches() == cons.switches());
        prod.release();
        cons.release();

        // The reverse pairing, on the second ring: a joined endpoint
        // starts at position 0, so a ring already run is not
        // rejoined.
        // SAFETY: as above.
        let ring_1 = unsafe { Ring::attach(&b1, first2) }.unwrap();
        let ring_2 = unsafe { Ring::attach(&b2, first2) }.unwrap();
        let mut prod = ring_2.claim_producer(PROD_ID).unwrap();
        let mut cons = ring_1.claim_consumer(CONS_ID).unwrap();
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
                        assert_checkpoint(&prod, &cons);
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
