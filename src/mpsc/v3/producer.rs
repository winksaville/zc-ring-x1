//! MPSC v3 producing endpoint: [`MpscProducer`] claims a
//! position by CAS on the packed claim word, a closure fills the
//! slot in place, and the commit happens on closure return, as
//! v1's. At a full segment it takes a free one and moves the ring
//! on, sealing the old segment behind it. The handle is one
//! counted producer role, given back by `release`.

use core::marker::PhantomData;
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{
    MOVED, Mode, Multi, Segments, WAITING, check_body_type, seq_of, word, word_pos, word_seg,
};
use crate::Full;
use crate::wake::{NoWake, Wake};

/// A producing handle, one counted producer role: claim one per
/// producing thread or process with
/// [`MpscRing::claim_producer`](super::MpscRing::claim_producer),
/// then `send_with`.
///
/// - `send_with` takes `&self`: exclusivity comes from the claim
///   CAS, not the borrow, so one handle may also be shared by
///   reference.
/// - Not `Clone`: each handle is one count in the roles word, and
///   [`release`](MpscProducer::release) gives it back. Dropping
///   the handle writes nothing, so the count stays held.
pub struct MpscProducer<'a, M: Mode = Multi, W: Wake = NoWake> {
    /// Geometry and segment addresses.
    pub(super) segs: Segments,
    _region: PhantomData<(&'a [u8], M, W)>,
}

// SAFETY: any number of producers is the protocol contract: the
// shared state (the header words, the slot seqs) is atomic, slot
// claims are exclusive by CAS, and slot writes are handed off
// with Release/Acquire ordering.
unsafe impl<M: Mode, W: Wake> Send for MpscProducer<'_, M, W> {}
// SAFETY: send_with is &self and every access is protected as
// above, so shared references across threads are equally fine.
unsafe impl<M: Mode, W: Wake> Sync for MpscProducer<'_, M, W> {}

/// Why a switch attempt did not move the ring.
enum NoSwitch {
    /// No segment is free: the ring is Full.
    NoFree,
    /// The claim word moved under the attempt: another producer
    /// switched, or the slot was released and claimed.
    Lost,
}

impl<'a, M: Mode, W: Wake> MpscProducer<'a, M, W> {
    /// Build the handle for a claimed role from the ring's
    /// geometry snapshot.
    pub(super) fn new(segs: Segments) -> Self {
        MpscProducer {
            segs,
            _region: PhantomData,
        }
    }

    /// Give the producer role back, counting it out of the roles
    /// word.
    ///
    /// - The producer keeps no state of its own, so nothing is
    ///   checkpointed: a later claim, in any process, sends on
    ///   where the ring is.
    pub fn release(self) {
        // Release: the ring's release, which needs no role held,
        // sees everything this producer committed.
        self.segs.roles().fetch_sub(1, Ordering::Release);
    }

    /// Segment switches the ring's producers have made: how many
    /// times a producer at a full segment moved the ring to a
    /// free one. Once the consumer has read everything sent, it
    /// equals the consumer's count.
    pub fn switches(&self) -> u64 {
        self.segs.switches().load(Ordering::Relaxed) as u64
    }

    /// The segment the ring's producers write into, `0` to the
    /// ring's segment count less one.
    pub fn segment(&self) -> u32 {
        word_seg(self.segs.claim().load(Ordering::Relaxed))
    }

    /// Claim the next position, fill it in place, commit on
    /// closure return, and retry a full ring under the injected wait
    /// policy, then [`Full`].
    ///
    /// - `fill` writes the message through `&mut T`, and commit is by
    ///   construction, so there is no abandonment state. If `fill`
    ///   panics, its slot stays claimed and never committed, as if
    ///   the producer had been killed there: the consumer waits at
    ///   it, and the ring is recovered by a restart.
    /// - `on_full` is called after each failed attempt with the
    ///   attempt count (0-based, saturating), and returning `false`
    ///   gives up. Pass `|_| false` for a single non-blocking
    ///   probe. Full means the current segment's next slot is
    ///   unread and no segment is free.
    /// - Losing a claim race, or a switch race, is not a policy
    ///   call: the ring made progress, and this producer retries
    ///   with the fresher claim word.
    /// - `T` must fit the slot body, `slot_size` less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), at an
    ///   alignment of at most that.
    /// - A consumer asleep on the empty ring is woken after the
    ///   commit, when its [`Wake`] wakes.
    pub fn send_with<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        self.send(on_full, |_| {}, fill, false)
    }

    /// [`send_with`](MpscProducer::send_with), calling `on_lost`
    /// after each claim race this producer loses, with the count of
    /// losses in a row, 1 for the first. A weak CAS failing
    /// spuriously counts as a loss.
    ///
    /// - A lost race is no policy call in `send_with`: the ring made
    ///   progress, and the loser reads the claim word again at once.
    ///   With many producers that read pulls the contended line from
    ///   the winner on every loss, so `on_lost` is where a producer
    ///   backs off, [`policy::backoff`](crate::policy::backoff) the
    ///   model.
    pub fn send_with_backoff<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        on_lost: impl FnMut(u32),
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        self.send(on_full, on_lost, fill, false)
    }

    /// [`send_with`](MpscProducer::send_with), sleeping on a full
    /// ring until the consumer frees room, then calling `on_full`
    /// after each wake.
    ///
    /// - The sleep is `W`'s: with [`NoWake`] it is a spin, and with
    ///   a futex it returns when the consumer wakes it or at its
    ///   timeout, so `on_full` also counts the timeouts, and bounds
    ///   the wait as it bounds a spin. Pass `|_| true` to wait until
    ///   there is room.
    /// - The consumer checks for sleepers every half segment of
    ///   releases, not every release, so a sleeping producer is
    ///   woken within half a segment of room.
    pub fn send_wait<T>(
        &self,
        on_full: impl FnMut(u32) -> bool,
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        self.send(on_full, |_| {}, fill, true)
    }

    /// The send the entries share, calling `on_lost` after each
    /// lost claim race and sleeping on a full ring when `sleep` is
    /// set.
    #[inline(always)]
    fn send<T>(
        &self,
        mut on_full: impl FnMut(u32) -> bool,
        mut on_lost: impl FnMut(u32),
        fill: impl FnOnce(&mut T),
        sleep: bool,
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let segs = &self.segs;
        check_body_type::<T>(segs.slot_size);
        let claim = segs.claim();
        // SeqCst on every claim word access, as v1's index: the
        // claim is only exclusive if these are linearizable.
        let mut w = claim.load(Ordering::SeqCst);
        let mut attempt = 0u32;
        let mut lost = 0u32;
        let (seg, pos) = loop {
            // Single: segment 0 always, whatever the word's segment
            // bits hold, so the check below folds away.
            let seg = if M::MULTI { word_seg(w) } else { 0 };
            let pos = word_pos(w);
            if M::MULTI && seg >= segs.seg_count {
                // A scribbled claim word: the ring is wedged, and
                // this degrades toward Full, never toward a slot
                // the ring does not have.
                return Err(Full);
            }
            // Acquire pairs with the consumer's Release in
            // release(): a claimable seq means the previous lap's
            // reads are done.
            let seq = segs.seq(seg, pos).load(Ordering::Acquire);
            if seq == pos {
                // Claimable. Weak CAS: a spurious failure just
                // retries with the fresher word. The consumer's
                // waiting flag carries over, cleared only by the
                // consumer.
                match claim.compare_exchange_weak(
                    w,
                    word(seg, pos.wrapping_add(1)) | (w & WAITING),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break (seg, pos),
                    Err(actual) => {
                        w = actual;
                        lost = lost.saturating_add(1);
                        on_lost(lost);
                    }
                }
                continue;
            }
            // Not claimable at pos: a stale word or a full
            // segment, told apart by re-reading, as v1 does.
            let cur = claim.load(Ordering::SeqCst);
            if cur != w {
                w = cur;
                continue;
            }
            // Single has no segment to switch to: the ring is Full.
            let switched = if M::MULTI {
                self.switch(w)
            } else {
                Err(NoSwitch::NoFree)
            };
            match switched {
                Ok(()) | Err(NoSwitch::Lost) => {}
                Err(NoSwitch::NoFree) => {
                    if sleep {
                        self.sleep_full();
                    }
                    if !on_full(attempt) {
                        return Err(Full);
                    }
                    attempt = attempt.saturating_add(1);
                }
            }
            w = claim.load(Ordering::SeqCst);
        };
        // (seg, pos) is exclusively ours until the seq commit
        // store.
        let msg = segs.body(seg, pos) as *mut T;
        let commit = seq_of(pos.wrapping_add(segs.commit_add));
        let seq = segs.seq(seg, pos);
        // SAFETY: msg is in-bounds and aligned (check_body_type
        // against the body's offset in a line-aligned slot), any
        // byte pattern is a valid T (FromBytes bound), and the
        // claim CAS gives exclusive slot access until the commit
        // store below.
        fill(unsafe { &mut *msg });
        // Release pairs with the consumer's Acquire seq load:
        // observing pos + M + 1 means the filled bytes are
        // visible.
        seq.store(commit, Ordering::Release);
        // The claim CAS returned the word it replaced, flag and
        // all, so learning the consumer sleeps costs nothing.
        if W::WAKES && w & WAITING != 0 {
            W::wake(claim);
        }
        Ok(())
    }

    /// Sleep on a full ring until the consumer frees room, a wake
    /// that comes early, or `W`'s timeout.
    ///
    /// - Count in, then look again: the consumer's check fences
    ///   its releases before it reads the count, and the fence here
    ///   orders the count before the look, so either the consumer
    ///   sees this producer or this producer sees the room.
    /// - The wake sequence is read before the look, so a wake
    ///   between the look and the sleep moves the word and the
    ///   sleep returns at once.
    #[cold]
    #[inline(never)]
    fn sleep_full(&self) {
        let segs = &self.segs;
        let c = segs.claims();
        c.prod_waiters.fetch_add(1, Ordering::SeqCst);
        core::sync::atomic::fence(Ordering::SeqCst);
        let seen = c.prod_wake.load(Ordering::SeqCst);
        if !self.has_room() {
            W::wait(&c.prod_wake, seen);
        }
        c.prod_waiters.fetch_sub(1, Ordering::SeqCst);
    }

    /// Whether a claim could land now: the claim word's slot is
    /// claimable, or a `Multi` ring has a free segment.
    fn has_room(&self) -> bool {
        let segs = &self.segs;
        let w = segs.claim().load(Ordering::SeqCst);
        let seg = if M::MULTI { word_seg(w) } else { 0 };
        if seg >= segs.seg_count {
            return true;
        }
        let pos = word_pos(w);
        segs.seq(seg, pos).load(Ordering::SeqCst) == pos
            || (M::MULTI && !segs.in_use().load(Ordering::SeqCst) & segs.all() != 0)
    }

    /// Move the ring from the full segment and position `w` names
    /// to a free segment.
    ///
    /// - Take the lowest free segment by setting its bit in the
    ///   in-use word, which serializes producers switching at
    ///   once and succeeds only where the bit was clear, clear its
    ///   seal, and CAS the claim word from `w` to the new segment
    ///   at its resume position, the position its seal held.
    /// - On success, seal the old segment: MOVED, the new segment,
    ///   and `w`'s position as the end. Every claim in the old
    ///   segment lies before it.
    /// - On a lost claim CAS, restore the seal and clear the bit:
    ///   the ring moved under the attempt, and the caller retries
    ///   with the fresh word.
    fn switch(&self, w: u32) -> Result<(), NoSwitch> {
        let segs = &self.segs;
        let in_use = segs.in_use();
        loop {
            // Acquire: a consumer's give-back is visible with its
            // released seqs.
            let free = !in_use.load(Ordering::Acquire) & segs.all();
            if free == 0 {
                return Err(NoSwitch::NoFree);
            }
            let k = free.trailing_zeros();
            if in_use.fetch_or(1 << k, Ordering::AcqRel) & (1 << k) != 0 {
                // Another producer set it first.
                continue;
            }
            // Segment k is ours among producers, and the consumer
            // cannot enter it until a seal names it, so nobody else
            // reads or writes its seal here.
            let seal = segs.seal(k);
            let start = word_pos(seal.load(Ordering::Acquire));
            // Cleared before the claim CAS: a clear after it could
            // land after a later producer's seal of k, once the ring
            // has filled k and left it again, and wipe that seal.
            seal.store(0, Ordering::Relaxed);
            match segs.claim().compare_exchange(
                w,
                word(k, start) | (w & WAITING),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    // Release pairs with the consumer's Acquire seal
                    // load: seeing MOVED means k's seal is clear.
                    segs.seal(word_seg(w))
                        .store(MOVED | word(k, word_pos(w)), Ordering::Release);
                    segs.switches().fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(_) => {
                    // Put k back as it was: its resume position for
                    // the next taker, then its bit.
                    seal.store(start, Ordering::Relaxed);
                    in_use.fetch_and(!(1 << k), Ordering::AcqRel);
                    return Err(NoSwitch::Lost);
                }
            }
        }
    }
}
