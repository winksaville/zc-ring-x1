//! MPSC v3 consuming endpoint: [`MpscConsumer`] reserves the
//! oldest committed slot of its current segment through the
//! [`MpscReadSlot`] guard, v1's loop, and follows the seal to the
//! next segment when the current one has ended, giving the old
//! one back. The handle is the consumer role, given back with its
//! state by `release`.

use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, Immutable, KnownLayout};

use super::{
    CONSUMER, Checkpoint, MAX_SEGMENTS, MOVED, Mode, Multi, Segments, WAITING, check_body_type,
    seq_of, word, word_pos, word_seg,
};
use crate::Empty;
use crate::wake::{NoWake, Seen, Wake};

/// `struct ConsumerState` is the consumer's state, separate from [`MpscConsumer`] so
/// [`MpscReadSlot`] can borrow it without naming the region's lifetime or the mode.
struct ConsumerState {
    /// Geometry and segment addresses.
    segs: Segments,
    /// The segment being read.
    cur: u32,
    /// Position in `cur`, [`SEQ_BITS`](super::SEQ_BITS) wide.
    pos: u32,
    /// Where each segment was left, the position a reuse starts
    /// at, the same value the segment's seal hands the producer
    /// that takes it next.
    resume: [u32; MAX_SEGMENTS as usize],
    /// Segment switches so far, counted on the switch path only.
    switches: u64,
}

/// The consuming handle, the ring's one consumer role, CAS-free
/// on the message path. `reserve_slot_with` the oldest committed
/// slot, read in place, `release`.
///
/// - [`release`](MpscConsumer::release) gives the role back with
///   the consumer's state, so the next claim continues where this
///   one stopped. Dropping the handle writes nothing, so the role
///   stays held.
pub struct MpscConsumer<'a, M: Mode = Multi, W: Wake = NoWake> {
    /// Private state, borrowed by each guard.
    st: ConsumerState,
    _region: PhantomData<(&'a [u8], M, W)>,
}

// SAFETY: the handle owns the single-consumer role, and shared state
// (the header words, the slot seqs) is atomic with
// Release/Acquire handoff.
unsafe impl<M: Mode, W: Wake> Send for MpscConsumer<'_, M, W> {}

impl<'a, M: Mode, W: Wake> MpscConsumer<'a, M, W> {
    /// Continue from `cp`, the checkpoint the last consumer left,
    /// or the ring's start.
    pub(super) fn resume(segs: Segments, cp: Checkpoint) -> Self {
        MpscConsumer {
            st: ConsumerState {
                segs,
                cur: cp.cur,
                pos: cp.pos,
                resume: cp.resume,
                switches: 0,
            },
            _region: PhantomData,
        }
    }

    /// Give the consumer role back, with its state.
    ///
    /// - Writes the segment, the position, and each segment's resume
    ///   position into the control block, then clears the role, so
    ///   the next claim, in any process, continues exactly here.
    pub fn release(self) {
        let st = &self.st;
        st.segs.store_consumer(&Checkpoint {
            cur: st.cur,
            pos: st.pos,
            resume: st.resume,
        });
        // Release: the next claim's AcqRel sees the checkpoint and
        // every release of a slot before it.
        st.segs.roles().fetch_and(!CONSUMER, Ordering::Release);
    }

    /// Segment switches this consumer has made: how many seals it
    /// has followed since its claim. Once it has read everything
    /// sent, a consumer claimed on a new ring equals the producers'
    /// count.
    pub fn switches(&self) -> u64 {
        self.st.switches
    }

    /// The segment this consumer reads from, `0` to the ring's
    /// segment count less one.
    pub fn segment(&self) -> u32 {
        self.st.cur
    }

    /// Reserve the oldest committed slot as a `&T`, applying an
    /// injected wait policy: retry until a message arrives or the
    /// policy gives up, then [`Empty`].
    ///
    /// - The slot at the position is committed or not. Not means a
    ///   second look at the segment's seal: MOVED with the end
    ///   position equal to this one means the segment has ended,
    ///   so it is given back and the reading continues in the
    ///   named segment, progress again. Otherwise the slot is not
    ///   yet committed and the policy runs. So a segment is given
    ///   back at the reserve after its last release, not at that
    ///   release: the fast path never loads the seal, and a
    ///   [`Single`](super::Single) ring never does.
    /// - A seal naming a segment the ring does not have reads as
    ///   Empty, failing toward Empty as the rings do.
    /// - `on_empty` is called after each failed attempt with the
    ///   attempt count (0-based, saturating), and returning `false`
    ///   gives up. Pass `|_| false` for a single non-blocking
    ///   probe.
    /// - Guard semantics as v1's: drop without release re-delivers
    ///   the same slot.
    pub fn reserve_slot_with<T>(
        &mut self,
        on_empty: impl FnMut(u32) -> bool,
    ) -> Result<MpscReadSlot<'_, T, W>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        self.reserve(on_empty, false)
    }

    /// [`reserve_slot_with`](MpscConsumer::reserve_slot_with),
    /// sleeping on an empty ring until a producer commits, then
    /// calling `on_empty` after each wake.
    ///
    /// - The sleep is `W`'s: with [`NoWake`] it is a spin, and with
    ///   a futex it returns when a producer wakes it or at its
    ///   timeout, so `on_empty` also counts the timeouts, and
    ///   bounds the wait as it bounds a spin. Pass `|_| true` to
    ///   wait until a message arrives.
    /// - A wake can come between a producer's claim and its commit,
    ///   and the look after it then finds the slot claimed and not
    ///   yet committed, which does not sleep again, so the policy
    ///   runs until the commit lands.
    /// - Before each sleep, producers asleep on a full ring are
    ///   woken, so two sides never sleep on each other past a
    ///   missed check.
    pub fn reserve_slot_wait<T>(
        &mut self,
        on_empty: impl FnMut(u32) -> bool,
    ) -> Result<MpscReadSlot<'_, T, W>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        self.reserve(on_empty, true)
    }

    /// The reserve both entries share, sleeping on an empty ring
    /// when `sleep` is set.
    #[inline(always)]
    fn reserve<T>(
        &mut self,
        mut on_empty: impl FnMut(u32) -> bool,
        sleep: bool,
    ) -> Result<MpscReadSlot<'_, T, W>, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        let st = &mut self.st;
        let segs = &st.segs;
        check_body_type::<T>(segs.slot_size);
        let mut attempt = 0u32;
        loop {
            let c = st.pos;
            // Single: segment 0 always, known at compile time.
            let cur = if M::MULTI { st.cur } else { 0 };
            let committed = seq_of(c.wrapping_add(segs.commit_add));
            // Acquire pairs with the producer's Release commit:
            // observing committed means the fill is visible.
            let seq = segs.seq(cur, c).load(Ordering::Acquire);
            if seq == committed {
                break;
            }
            // Acquire pairs with the sealing producer's Release
            // store: seeing MOVED means the next segment's seal is
            // clear and its seqs are as the producers left them.
            // Single never switches, so it never loads a seal.
            let seal = if M::MULTI {
                segs.seal(cur).load(Ordering::Acquire)
            } else {
                0
            };
            if seal & MOVED != 0 && word_pos(seal) == c && word_seg(seal) < segs.seg_count {
                let k = word_seg(seal);
                st.resume[st.cur as usize] = c;
                // Release: the released seqs of this segment travel
                // with its give-back, the one read-modify-write on
                // this side, on the switch path only.
                segs.in_use().fetch_and(!(1 << st.cur), Ordering::AcqRel);
                st.cur = k;
                st.pos = st.resume[k as usize];
                st.switches += 1;
                // A segment given back is room a producer asleep on
                // a full ring can take.
                if W::WAKES {
                    segs.wake_producers::<W>();
                }
                continue;
            }
            // Not committed yet (or a peer-corrupted seq: degrade
            // toward Empty, never toward reading an unowned slot).
            // Before sleeping, everything read is released: wake any
            // producer asleep on a full ring, so the two sides never
            // sleep on each other. Not on the polling path, where a
            // fence per empty look would cost a round trip its
            // every message.
            if sleep {
                if W::WAKES {
                    segs.wake_producers::<W>();
                }
                sleep_empty::<W>(segs, cur, c, M::MULTI);
            }
            if !on_empty(attempt) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        }
        let msg = segs.body(st.cur, st.pos) as *const T;
        Ok(MpscReadSlot {
            st,
            msg,
            _slot: PhantomData,
        })
    }
}

/// Sleep on an empty ring until a producer claims past `pos` in
/// segment `cur`, a wake that comes early, or `W`'s timeout.
///
/// - Set the waiting flag on the claim word, then sleep only while
///   the word, flag aside, still names `cur` and `pos`: nothing is
///   claimed past what the consumer read, so no commit is on its
///   way, and every later claim CAS sees the flag and wakes this
///   side after its commit. A claim between the flag and the sleep
///   moves the word, and the sleep returns at once.
/// - `Single` compares the position alone, its segment bits being
///   always 0.
/// - The flag is cleared after, so producers stop waking a
///   consumer that is awake.
#[cold]
#[inline(never)]
fn sleep_empty<W: Wake>(segs: &Segments, cur: u32, pos: u32, multi: bool) {
    let claim = segs.claim();
    let prev = claim.fetch_or(WAITING, Ordering::SeqCst);
    let here = if multi {
        prev & !WAITING == word(cur, pos)
    } else {
        word_pos(prev) == seq_of(pos)
    };
    if here {
        W::wait(Seen::written(claim, prev | WAITING));
    }
    claim.fetch_and(!WAITING, Ordering::SeqCst);
}

/// A reserved read slot: `Deref` to read the message, then
/// [`release`](MpscReadSlot::release).
pub struct MpscReadSlot<'c, T, W: Wake = NoWake> {
    /// The consumer's state, for the release.
    st: &'c mut ConsumerState,
    /// The slot body, viewed as the message type. Raw on purpose,
    /// see the SPSC `ReadSlot`.
    msg: *const T,
    /// Owns the `&'c mut` borrow of the consumer.
    _slot: PhantomData<(&'c T, W)>,
}

impl<T, W: Wake> Deref for MpscReadSlot<'_, T, W> {
    type Target = T;
    /// Read access to the in-slot message.
    fn deref(&self) -> &T {
        // SAFETY: msg is in-bounds and aligned (check_body_type
        // against the body's offset in a line-aligned slot), any
        // byte pattern is a valid T (FromBytes bound), and the seq
        // protocol gives this guard read access until release.
        unsafe { &*self.msg }
    }
}

impl<T, W: Wake> MpscReadSlot<'_, T, W> {
    /// Free the slot for reuse.
    ///
    /// - The position advances first, private state, and the seq
    ///   store last (`Release`), the protocol-visible handoff
    ///   producers acquire.
    /// - Every half segment of releases, producers asleep on a full
    ///   ring are woken, when the [`Wake`] wakes: a fence per half
    ///   segment, not per message.
    pub fn release(self) {
        let st = self.st;
        let segs = &st.segs;
        let c = st.pos;
        st.pos = seq_of(c.wrapping_add(1));
        segs.seq(st.cur, c)
            .store(seq_of(c.wrapping_add(segs.capacity)), Ordering::Release);
        if W::WAKES && st.pos & segs.wake_mask == 0 {
            segs.wake_producers::<W>();
        }
    }
}
