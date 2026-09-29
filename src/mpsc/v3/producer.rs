//! MPSC v3 producing endpoint: [`MpscProducer`] claims a
//! position by CAS on the packed claim word, a closure fills the
//! slot in place, and the commit happens on closure return, as
//! v1's. At a full segment it takes a free one and moves the ring
//! on, sealing the old segment behind it. The handle is one
//! counted producer role, given back by `release`.

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use super::{
    MOVED, Mode, Multi, Segments, WAITING, check_body_type, seq_of, word, word_pos, word_seg,
};
#[cfg(any(target_os = "linux", feature = "std"))]
use crate::Ticks;
#[cfg(any(target_os = "linux", feature = "std"))]
use crate::clock::now_ticks;
use crate::wake::{NoWake, Seen, Wake};
use crate::{Deadline, Full};

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

/// Wake the consumer asleep on the claim word, out of line and
/// cold, so the send loop compiles as it does without a wake.
#[cold]
#[inline(never)]
fn wake_consumer<W: Wake>(claim: &AtomicU32) {
    W::wake(claim);
}

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

    /// `send_with_x` claims the next free slot in the ring, spinning for up to `give_up` for a slot
    /// to become free. When no slot becomes free in that time, `send_with_x` returns `Err(Full)`.
    /// When a slot is free, or becomes free, `send_with_x` calls `fill` with a mutable reference to
    /// the message in the slot, `fill` writes the message there, and `send_with_x` commits the slot
    /// so the consumer can read it. `fill` is a closure, so the values it writes are ones it
    /// captures from the caller, as `value` is in the example below.
    ///
    /// Another producer may claim a free slot first. `send_with_x` then goes for the next slot at
    /// once and does not return, so a lost slot is never an error: `Err(Full)` means the ring was
    /// full when `send_with_x` last looked, after `give_up` had passed, and any slot freed in the
    /// meantime went to another producer. The `give_up` time starts when `send_with_x` first finds
    /// the ring full, so a send that finds a free slot never reads the clock.
    ///
    /// While `fill` runs, the slot is claimed but not committed, and the consumer, which reads
    /// slots in order, cannot read past it. A slow `fill` delays every message behind it, and a
    /// `fill` that panics leaves the slot claimed for good, so the consumer waits at it until the
    /// ring is restarted. A consumer asleep on an empty ring is woken after the commit.
    ///
    /// `send_with_x` is available on Linux, and on other targets with the `std` feature, which
    /// provides its clock.
    ///
    /// # Parameters
    ///
    /// - `self`: this producer, by shared reference, so one handle may be used from several threads
    ///   at once.
    /// - `give_up`: how long to wait while the ring is full, counted from the first attempt that
    ///   finds it full. [`Ticks::ZERO`] makes one attempt, and [`Ticks::FOREVER`] never gives up.
    ///   The caller makes `give_up` once, with [`microsecs_to_ticks`](crate::microsecs_to_ticks) or
    ///   [`nanos_to_ticks`](crate::nanos_to_ticks), not once per send.
    /// - `fill`: the closure that writes the message. `send_with_x` calls `fill` once, after the
    ///   slot is claimed, with a mutable reference to the slot's body, which still holds the bytes
    ///   of the message the slot carried last. `fill` must write every field, since the consumer
    ///   reads any field `fill` leaves as those stale bytes. `send_with_x` does not call `fill`
    ///   when it returns `Err(Full)`.
    ///
    /// # Type parameters
    ///
    /// - `T`: the message type the slot holds. The size of `T` must be at most the slot size less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), and its alignment at most
    ///   `SLOT_HEADER_BYTES`, or the send panics. For zero-copy, `T` is a [`Desc`](crate::Desc),
    ///   the handle of a pool buffer the producer wrote the message into before the send, so `fill`
    ///   is one small store and the message itself is never copied.
    ///
    /// # Returns
    ///
    /// - `Ok(())`: the message is committed, and the consumer can read it.
    /// - `Err(Full)`: the ring stayed full until `give_up` passed. No slot was claimed, and `fill`
    ///   was not called.
    ///
    /// # Example
    ///
    /// ```
    /// use zc_ring_x1::mpsc::v3::{MpscRing, Single, segment_size};
    /// use zc_ring_x1::{Pool, PoolHeader, Full, microsecs_to_ticks};
    /// use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};
    ///
    /// // The message: plain data, at most a slot's body in size.
    /// #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    /// #[repr(C)]
    /// struct Reading {
    ///     sensor: u32,
    ///     value: u32,
    /// }
    ///
    /// // One cache line of backing store, so a `Vec<Line>` is a line-aligned region.
    /// #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
    /// #[repr(C, align(64))]
    /// struct Line([u8; 64]);
    ///
    /// // A pool holding one segment of 4 slots, 64 bytes each, and a one-segment ring over it.
    /// let seg_bytes = segment_size(64, 4);
    /// let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes;
    /// let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
    /// let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), seg_bytes as u32, 1)
    ///     .expect("the store holds the pool"); // OK: sized above from segment_size
    /// let ring = MpscRing::<Single>::init(&mut pool, 64, 4, 1)
    ///     .expect("the pool holds the segment"); // OK: the pool was made for it
    /// let producer = ring.claim_producer().expect("a fresh ring"); // OK: no role is held yet
    /// let mut consumer = ring.claim_consumer().expect("a fresh ring"); // OK: no role is held yet
    ///
    /// // The caller converts its time to ticks once, not once per send.
    /// let give_up = microsecs_to_ticks(1_000);
    ///
    /// // Send a reading. The closure captures `value` and writes the message into the slot.
    /// let value = 42;
    /// let sent = producer.send_with_x::<Reading>(give_up, |msg| {
    ///     msg.sensor = 7;
    ///     msg.value = value;
    /// });
    /// assert_eq!(sent, Ok(()));
    ///
    /// // Fill the other 3 slots. Then, with nothing read, the next send waits `give_up`, 1 ms,
    /// // and returns `Err(Full)` without calling its closure.
    /// for value in 0..3 {
    ///     let sent = producer.send_with_x::<Reading>(give_up, |msg| {
    ///         msg.sensor = 7;
    ///         msg.value = value;
    ///     });
    ///     assert_eq!(sent, Ok(()));
    /// }
    /// assert_eq!(producer.send_with_x::<Reading>(give_up, |_| {}), Err(Full));
    ///
    /// // The consumer reads the first message in place, then releases its slot.
    /// let msg = consumer
    ///     .reserve_slot_with::<Reading>(|_| false)
    ///     .expect("a message is waiting"); // OK: four were sent
    /// assert_eq!((msg.sensor, msg.value), (7, 42));
    /// msg.release();
    /// ```
    #[cfg(any(target_os = "linux", feature = "std"))]
    pub fn send_with_x<T>(&self, give_up: Ticks, fill: impl FnOnce(&mut T)) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        // `end` is set at the first full ring, so a send that finds room reads no clock, and each
        // later check is one reading and a compare.
        let mut end: Option<u64> = None;
        self.send(
            |_| {
                if give_up != Ticks::FOREVER {
                    let now = now_ticks();
                    if now >= *end.get_or_insert(now.saturating_add(give_up.0)) {
                        return false;
                    }
                }
                core::hint::spin_loop();
                true
            },
            |_| {},
            fill,
            false,
        )
    }

    /// `send_with_backoff_x` claims the next free slot in the ring, first spinning for up to `spin`
    /// for a slot to become free, then sleeping for up to `wait` more until the consumer frees one.
    /// When no slot becomes free in that time, `send_with_backoff_x` returns `Err(Full)`. When a
    /// slot is free, or becomes free, `send_with_backoff_x` calls `fill` with a mutable reference
    /// to the message in the slot, `fill` writes the message there, and `send_with_backoff_x`
    /// commits the slot so the consumer can read it. `fill` is a closure, so the values it writes
    /// are ones it captures from the caller.
    ///
    /// Another producer may claim a free slot first. `send_with_backoff_x` then goes for the next
    /// slot at once and does not return, so a lost slot is never an error: `Err(Full)` means the
    /// ring was full when `send_with_backoff_x` last looked, after `spin` and `wait` had passed,
    /// and any slot freed in the meantime went to another producer. The time starts when
    /// `send_with_backoff_x` first finds the ring full, so a send that finds a free slot never
    /// reads the clock.
    ///
    /// The sleep is the ring's [`Wake`]. With [`NoWake`] there is no sleep, and
    /// `send_with_backoff_x` spins for `spin` and `wait` together. The consumer wakes all sleeping
    /// producers at once, at each half segment of releases. A sleep that ends early, woken for a
    /// slot another producer took, sleeps again to the same deadline, and `send_with_backoff_x`
    /// looks at the ring once more after the last sleep.
    ///
    /// While `fill` runs, the slot is claimed but not committed, and the consumer, which reads
    /// slots in order, cannot read past it. A slow `fill` delays every message behind it, and a
    /// `fill` that panics leaves the slot claimed for good, so the consumer waits at it until the
    /// ring is restarted. A consumer asleep on an empty ring is woken after the commit.
    ///
    /// `send_with_backoff_x` is available on Linux, and on other targets with the `std` feature,
    /// which provides its clock.
    ///
    /// # Parameters
    ///
    /// - `self`: this producer, by shared reference.
    /// - `spin`: how long to spin while the ring is full, counted from the first attempt that finds
    ///   it full, before sleeping. [`Ticks::ZERO`] sleeps at once, and [`Ticks::FOREVER`] never
    ///   sleeps.
    /// - `wait`: how long to sleep, in all, after the spin. [`Ticks::ZERO`] gives up when the spin
    ///   ends, and [`Ticks::FOREVER`] never gives up. The caller makes `spin` and `wait` once, with
    ///   [`microsecs_to_ticks`](crate::microsecs_to_ticks) or
    ///   [`nanos_to_ticks`](crate::nanos_to_ticks), not once per send.
    /// - `fill`: the closure that writes the message. `send_with_backoff_x` calls `fill` once,
    ///   after the slot is claimed, with a mutable reference to the slot's body, which still holds
    ///   the bytes of the message the slot carried last. `fill` must write every field, since the
    ///   consumer reads any field `fill` leaves as those stale bytes. `send_with_backoff_x` does
    ///   not call `fill` when it returns `Err(Full)`.
    ///
    /// # Type parameters
    ///
    /// - `T`: the message type the slot holds. The size of `T` must be at most the slot size less
    ///   [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), and its alignment at most
    ///   `SLOT_HEADER_BYTES`, or the send panics. For zero-copy, `T` is a [`Desc`](crate::Desc),
    ///   the handle of a pool buffer the producer wrote the message into before the send, so `fill`
    ///   is one small store and the message itself is never copied.
    ///
    /// # Returns
    ///
    /// - `Ok(())`: the message is committed, and the consumer can read it.
    /// - `Err(Full)`: the ring stayed full until `spin` and `wait` passed. No slot was claimed, and
    ///   `fill` was not called.
    ///
    /// # Example
    ///
    /// ```
    /// use zc_ring_x1::mpsc::v3::{MpscRing, Single, segment_size};
    /// use zc_ring_x1::wake::Futex;
    /// use zc_ring_x1::{Pool, PoolHeader, Ticks, microsecs_to_ticks};
    /// use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};
    ///
    /// // The message: plain data, at most a slot's body in size.
    /// #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    /// #[repr(C)]
    /// struct Reading {
    ///     sensor: u32,
    ///     value: u32,
    /// }
    ///
    /// // One cache line of backing store, so a `Vec<Line>` is a line-aligned region.
    /// #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Clone)]
    /// #[repr(C, align(64))]
    /// struct Line([u8; 64]);
    ///
    /// // A pool holding one segment of 4 slots, 64 bytes each, and a one-segment ring over it.
    /// let seg_bytes = segment_size(64, 4);
    /// let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes;
    /// let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
    /// let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), seg_bytes as u32, 1)
    ///     .expect("the store holds the pool"); // OK: sized above from segment_size
    /// let ring = MpscRing::<Single, Futex>::init(&mut pool, 64, 4, 1)
    ///     .expect("the pool holds the segment"); // OK: the pool was made for it
    /// let producer = ring.claim_producer().expect("a fresh ring"); // OK: no role is held yet
    /// let mut consumer = ring.claim_consumer().expect("a fresh ring"); // OK: no role is held yet
    ///
    /// // Fill the ring's 4 slots, so the next send finds it full.
    /// for value in 0..4 {
    ///     let sent = producer.send_with_x::<Reading>(Ticks::ZERO, |msg| {
    ///         msg.sensor = 7;
    ///         msg.value = value;
    ///     });
    ///     assert_eq!(sent, Ok(()));
    /// }
    ///
    /// std::thread::scope(|s| {
    ///     // The consumer, on its own thread, reads two messages after 10 ms. The consumer wakes
    ///     // sleeping producers every half segment of releases, two releases in a ring of 4.
    ///     s.spawn(move || {
    ///         std::thread::sleep(std::time::Duration::from_millis(10));
    ///         for _ in 0..2 {
    ///             let msg = consumer
    ///                 .reserve_slot_with::<Reading>(|_| false)
    ///                 .expect("a message is waiting"); // OK: four were sent
    ///             msg.release();
    ///         }
    ///     });
    ///
    ///     // Spin for up to 20 microseconds, then sleep for up to 1 second. The consumer's
    ///     // releases wake the producer after about 10 ms, and the send lands.
    ///     let spin = microsecs_to_ticks(20);
    ///     let wait = microsecs_to_ticks(1_000_000);
    ///     let sent = producer.send_with_backoff_x::<Reading>(spin, wait, |msg| {
    ///         msg.sensor = 7;
    ///         msg.value = 4;
    ///     });
    ///     assert_eq!(sent, Ok(()));
    /// });
    /// ```
    #[cfg(any(target_os = "linux", feature = "std"))]
    pub fn send_with_backoff_x<T>(
        &self,
        spin: Ticks,
        wait: Ticks,
        fill: impl FnOnce(&mut T),
    ) -> Result<(), Full>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        // `ends` holds the spin's end and the wait's end, set at the first full ring, so a send
        // that finds room reads no clock, and each later check is one reading and a compare or two.
        let mut ends: Option<(u64, u64)> = None;
        self.send(
            |_| {
                if spin == Ticks::FOREVER {
                    core::hint::spin_loop();
                    return true;
                }
                let now = now_ticks();
                let (spin_end, wait_end) = *ends.get_or_insert_with(|| {
                    let spin_end = now.saturating_add(spin.0);
                    (spin_end, spin_end.saturating_add(wait.0))
                });
                if now < spin_end {
                    core::hint::spin_loop();
                    return true;
                }
                let deadline = if wait == Ticks::FOREVER {
                    None
                } else if now >= wait_end {
                    return false;
                } else {
                    Some(Deadline::at(wait_end))
                };
                if W::WAKES {
                    self.sleep_full(deadline);
                } else {
                    core::hint::spin_loop();
                }
                true
            },
            |_| {},
            fill,
            false,
        )
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
                        self.sleep_full(None);
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
            wake_consumer::<W>(claim);
        }
        Ok(())
    }

    /// `sleep_full` sleeps on a full ring until the consumer frees room, a wake comes early, or the
    /// timeout passes: `deadline`, or `W`'s own timeout when `deadline` is `None`.
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
    fn sleep_full(&self, deadline: Option<Deadline>) {
        let segs = &self.segs;
        let c = segs.claims();
        c.prod_waiters.fetch_add(1, Ordering::SeqCst);
        core::sync::atomic::fence(Ordering::SeqCst);
        let seen = Seen::load(&c.prod_wake);
        if !self.has_room() {
            match deadline {
                Some(d) => W::wait_until(seen, d),
                None => W::wait(seen),
            }
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
    /// - Out of line and cold: inlined into the send loop it made a
    ///   ring that never switches run up to three times slower than
    ///   `Single` (design note, MPSC v3 measured), a path taken only
    ///   on a full ring costing every message.
    #[cold]
    #[inline(never)]
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
