//! MPSC v4 consuming endpoint: [`MpscConsumer`] receives the oldest message of its current segment,
//! read in place by a closure, v1's loop, and follows the seal to the next segment when the current
//! one has ended, giving the old one back. The handle is the consumer role, given back with its
//! state by `release`.

use core::marker::PhantomData;
use core::sync::atomic::Ordering;
use zerocopy::{FromBytes, Immutable, KnownLayout};

use super::wait::{Sleeper, WaitPolicy, Waiter};
use super::{
    CONSUMER, Checkpoint, MAX_SEGMENTS, MOVED, Mode, Multi, Segments, WAITING, check_body_type,
    seq_of, word, word_pos, word_seg,
};
#[cfg(any(target_os = "linux", feature = "std"))]
use crate::Ticks;
#[cfg(any(target_os = "linux", feature = "std"))]
use crate::clock::now_ticks;
use crate::wake::{Seen, Sleeps, SpinOnly, Spins, Waits};
use crate::{Deadline, Empty};

/// `struct ConsumerState` is the consumer's state.
struct ConsumerState {
    /// Geometry and segment addresses.
    segs: Segments,
    /// The segment being read.
    cur: u32,
    /// Position in `cur`, [`SEQ_BITS`](super::SEQ_BITS) wide.
    pos: u32,
    /// Where each segment was left, the position a reuse starts at, the same value the segment's
    /// seal hands the producer that takes it next.
    resume: [u32; MAX_SEGMENTS as usize],
    /// Segment switches so far, counted on the switch path only.
    switches: u64,
}

/// `struct MpscConsumer` is the consuming handle, the ring's one consumer role: a consuming thread
/// or process takes it with [`MpscRing::consumer`](super::MpscRing::consumer), then receives with
/// [`recv`](MpscConsumer::recv), [`recv_spin`](MpscConsumer::recv_spin), or
/// [`recv_spin_sleep`](MpscConsumer::recv_spin_sleep).
///
/// - The receives take `&mut self`: a ring has one consumer, and one message is read at a time.
/// - [`release`](MpscConsumer::release) gives the role back with the consumer's state, so the next
///   consumer continues where this one stopped. Dropping the handle writes nothing, so the role
///   stays held.
pub struct MpscConsumer<'a, M: Mode = Multi, W: Waits = SpinOnly> {
    /// Private state.
    st: ConsumerState,
    _region: PhantomData<(&'a [u8], M, W)>,
}

// SAFETY: the handle owns the single-consumer role, and shared state (the header words, the slot
// seqs) is atomic with Release/Acquire handoff.
unsafe impl<M: Mode, W: Waits> Send for MpscConsumer<'_, M, W> {}

impl<'a, M: Mode, W: Waits> MpscConsumer<'a, M, W> {
    /// Continue from `cp`, the checkpoint the last consumer left, or the ring's start.
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
    /// - Writes the segment, the position, and each segment's resume position into the control
    ///   block, then clears the role, so the next consumer, in any process, continues exactly here.
    pub fn release(self) {
        let st = &self.st;
        st.segs.store_consumer(&Checkpoint {
            cur: st.cur,
            pos: st.pos,
            resume: st.resume,
        });
        // Release: the next claim's AcqRel sees the checkpoint and every release of a slot before
        // it.
        st.segs.roles().fetch_and(!CONSUMER, Ordering::Release);
    }

    /// Segment switches this consumer has made: how many seals it has followed since it took the
    /// role. Once it has read everything sent, the first consumer of a ring equals the producers'
    /// count.
    pub fn switches(&self) -> u64 {
        self.st.switches
    }

    /// The segment this consumer reads from, `0` to the ring's segment count less one.
    pub fn segment(&self) -> u32 {
        self.st.cur
    }

    /// `recv_spin` is [`recv`](MpscConsumer::recv) with a policy that spins while the ring is
    /// empty, for up to `give_up`, then returns `Err(Empty)`. The time starts when `recv_spin`
    /// first finds the ring empty, so a receive that finds a message never reads the clock.
    ///
    /// `recv_spin` is offered on a ring over [`SpinOnly`], or over
    /// [`SpinOrSleep`](crate::wake::SpinOrSleep) where other endpoints sleep. On a ring over
    /// [`Sleep`](crate::wake::Sleep), receive with
    /// [`recv_spin_sleep`](MpscConsumer::recv_spin_sleep).
    ///
    /// `recv_spin` is available on Linux, and on other targets with the `std` feature, which
    /// provides its clock.
    ///
    /// # Parameters
    ///
    /// - `self`: this consumer, by mutable reference.
    /// - `give_up`: how long to spin while the ring is empty. [`Ticks::ZERO`] makes one attempt,
    ///   and [`Ticks::FOREVER`] never gives up. The caller makes `give_up` once, with
    ///   [`microsecs_to_ticks`](crate::microsecs_to_ticks) or
    ///   [`nanos_to_ticks`](crate::nanos_to_ticks), not once per receive.
    /// - `read_msg`: the closure that reads the message, as [`recv`](MpscConsumer::recv)'s.
    ///
    /// # Type parameters
    ///
    /// - `T`: the message type the slot holds, as [`recv`](MpscConsumer::recv)'s.
    /// - `R`: what `read_msg` returns, as [`recv`](MpscConsumer::recv)'s.
    ///
    /// # Returns
    ///
    /// - `Ok(r)`: `read_msg` read the message and returned `r`, and the slot is free.
    /// - `Err(Empty)`: the ring stayed empty until `give_up` passed. `read_msg` was not called.
    #[cfg(any(target_os = "linux", feature = "std"))]
    pub fn recv_spin<T, R>(
        &mut self,
        give_up: Ticks,
        read_msg: impl FnOnce(&T) -> R,
    ) -> Result<R, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
        W: Spins,
    {
        // `end` is set at the first empty ring, so a receive that finds a message reads no clock,
        // and each later check is one reading and a compare.
        let mut end: Option<u64> = None;
        self.recv_at(
            |_, _| {
                if give_up != Ticks::FOREVER {
                    let now = now_ticks();
                    if now >= *end.get_or_insert(now.saturating_add(give_up.0)) {
                        return false;
                    }
                }
                core::hint::spin_loop();
                true
            },
            read_msg,
        )
    }

    /// `recv_spin_sleep` is [`recv`](MpscConsumer::recv) with a policy that spins while the ring is
    /// empty, for up to `spin_time`, then sleeps for up to `sleep_time` more until a producer sends
    /// a message, then returns `Err(Empty)`. The time starts when `recv_spin_sleep` first finds the
    /// ring empty, so a receive that finds a message never reads the clock.
    ///
    /// `recv_spin_sleep` is offered on a ring over [`Sleep`](crate::wake::Sleep) or
    /// [`SpinOrSleep`](crate::wake::SpinOrSleep), and the sleep is on the wake that choice names,
    /// such as a futex. A ring over [`SpinOnly`] has no sleep, and offers
    /// [`recv_spin`](MpscConsumer::recv_spin). A producer wakes the sleeping consumer after it
    /// commits a message. A sleep that ends early sleeps again to the same deadline, and
    /// `recv_spin_sleep` looks at the ring once more after the last sleep.
    ///
    /// `recv_spin_sleep` is available on Linux, and on other targets with the `std` feature, which
    /// provides its clock.
    ///
    /// # Parameters
    ///
    /// - `self`: this consumer, by mutable reference.
    /// - `spin_time`: how long to spin while the ring is empty, before sleeping. [`Ticks::ZERO`]
    ///   sleeps at once, and [`Ticks::FOREVER`] never sleeps, a spin without end, which is what
    ///   [`SpinOrSleep`](crate::wake::SpinOrSleep) and `recv_spin` are for.
    /// - `sleep_time`: how long to sleep, in all, after the spin. [`Ticks::ZERO`] gives up when the
    ///   spin ends, and [`Ticks::FOREVER`] never gives up. The caller makes `spin_time` and
    ///   `sleep_time` once, with [`microsecs_to_ticks`](crate::microsecs_to_ticks) or
    ///   [`nanos_to_ticks`](crate::nanos_to_ticks), not once per receive.
    /// - `read_msg`: the closure that reads the message, as [`recv`](MpscConsumer::recv)'s.
    ///
    /// # Type parameters
    ///
    /// - `T`: the message type the slot holds, as [`recv`](MpscConsumer::recv)'s.
    /// - `R`: what `read_msg` returns, as [`recv`](MpscConsumer::recv)'s.
    ///
    /// # Returns
    ///
    /// - `Ok(r)`: `read_msg` read the message and returned `r`, and the slot is free.
    /// - `Err(Empty)`: the ring stayed empty until `spin_time` and `sleep_time` passed. `read_msg`
    ///   was not called.
    #[cfg(any(target_os = "linux", feature = "std"))]
    pub fn recv_spin_sleep<T, R>(
        &mut self,
        spin_time: Ticks,
        sleep_time: Ticks,
        read_msg: impl FnOnce(&T) -> R,
    ) -> Result<R, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
        W: Sleeps,
    {
        // `ends` holds the spin's end and the sleep's end, set at the first empty ring, so a
        // receive that finds a message reads no clock, and each later check is one reading and a
        // compare or two.
        let mut ends: Option<(u64, u64)> = None;
        self.recv_at(
            |_, look| {
                if spin_time == Ticks::FOREVER {
                    core::hint::spin_loop();
                    return true;
                }
                let now = now_ticks();
                let (spin_end, sleep_end) = *ends.get_or_insert_with(|| {
                    let spin_end = now.saturating_add(spin_time.0);
                    (spin_end, spin_end.saturating_add(sleep_time.0))
                });
                if now < spin_end {
                    core::hint::spin_loop();
                    return true;
                }
                let deadline = if sleep_time == Ticks::FOREVER {
                    None
                } else if now >= sleep_end {
                    return false;
                } else {
                    Some(Deadline::at(sleep_end))
                };
                look.sleep_empty(deadline);
                true
            },
            read_msg,
        )
    }

    /// `recv` takes the oldest message in the ring, asking `policy` what to do each time it finds
    /// the ring empty: look again, sleep first, or give up with `Err(Empty)`. When a message is
    /// there, or arrives, `recv` calls `read_msg` with a reference to the message in its slot,
    /// `read_msg` reads it there, and `recv` frees the slot for the producers and returns what
    /// `read_msg` returned. `recv` is the general receive: [`recv_spin`](MpscConsumer::recv_spin)
    /// and [`recv_spin_sleep`](MpscConsumer::recv_spin_sleep) are `recv` with a policy already
    /// written.
    ///
    /// While `read_msg` runs, the slot is still the message's, and no producer can write over it. A
    /// slow `read_msg` delays the producers once the ring fills behind it. A `read_msg` that panics
    /// leaves the message in its slot, so the next `recv` reads the same message again.
    ///
    /// A slot that a producer has taken but not yet committed reads as an empty ring: `recv` asks
    /// the policy and looks again, and messages are received in the order their slots were taken.
    ///
    /// # Parameters
    ///
    /// - `self`: this consumer, by mutable reference.
    /// - `policy`: the [`WaitPolicy`] `recv` asks when the ring is empty. A closure `|attempt| ...`
    ///   returning whether to look again is a policy, so `|_| false` makes one attempt and
    ///   [`policy::spin`](crate::policy::spin) never gives up.
    /// - `read_msg`: the closure that reads the message. `recv` calls `read_msg` once, with a
    ///   reference that is valid only until `read_msg` returns, so `read_msg` copies out what the
    ///   caller keeps and returns it. `recv` does not call `read_msg` when it returns `Err(Empty)`.
    ///
    /// # Type parameters
    ///
    /// - `T`: the message type the slot holds, the type the producer sent. The size of `T` must be
    ///   at most the slot size less [`SLOT_HEADER_BYTES`](super::SLOT_HEADER_BYTES), and its
    ///   alignment at most `SLOT_HEADER_BYTES`, or the receive panics. For zero-copy, `T` is a
    ///   [`Desc`](crate::Desc), the handle of the pool buffer that holds the message, and
    ///   `read_msg` returns it.
    /// - `R`: what `read_msg` returns, and `recv` returns in `Ok`.
    ///
    /// # Returns
    ///
    /// - `Ok(r)`: `read_msg` read the message and returned `r`, and the slot is free.
    /// - `Err(Empty)`: the policy gave up on an empty ring. `read_msg` was not called.
    #[inline(always)]
    pub fn recv<T, R>(
        &mut self,
        mut policy: impl WaitPolicy,
        read_msg: impl FnOnce(&T) -> R,
    ) -> Result<R, Empty>
    where
        T: FromBytes + KnownLayout + Immutable,
    {
        self.recv_at(
            |attempt, look| policy.on_wait(attempt, &Waiter::new(look)),
            read_msg,
        )
    }

    /// `recv_at` is the receive every entry shares: `on_empty` is called at each empty look with
    /// the attempt count and the look itself, which it may sleep on, to a deadline or not.
    ///
    /// - The slot at the position is committed or not. Not means a second look at the segment's
    ///   seal: MOVED with the end position equal to this one means the segment has ended, so it is
    ///   given back and the reading continues in the named segment, progress again. Otherwise the
    ///   slot is not yet committed and `on_empty` runs. So a segment is given back at the receive
    ///   after its last message, not at that message: the fast path never loads the seal, and a
    ///   [`Single`](super::Single) ring never does.
    /// - A seal naming a segment the ring does not have reads as Empty, failing toward Empty as the
    ///   rings do.
    /// - After `read_msg` the position advances first, private state, and the seq store is last
    ///   (`Release`), the protocol-visible handoff producers acquire.
    /// - Every half segment of messages, producers asleep on a full ring are woken, when the
    ///   [`Wake`] wakes: a fence per half segment, not per message.
    #[inline(always)]
    fn recv_at<T, R>(
        &mut self,
        mut on_empty: impl FnMut(u32, &EmptyLook<'_, W>) -> bool,
        read_msg: impl FnOnce(&T) -> R,
    ) -> Result<R, Empty>
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
            // Acquire pairs with the producer's Release commit: observing committed means the fill
            // is visible.
            let seq = segs.seq(cur, c).load(Ordering::Acquire);
            if seq == committed {
                break;
            }
            // Acquire pairs with the sealing producer's Release store: seeing MOVED means the next
            // segment's seal is clear and its seqs are as the producers left them. Single never
            // switches, so it never loads a seal.
            let seal = if M::MULTI {
                segs.seal(cur).load(Ordering::Acquire)
            } else {
                0
            };
            if seal & MOVED != 0 && word_pos(seal) == c && word_seg(seal) < segs.seg_count {
                let k = word_seg(seal);
                st.resume[st.cur as usize] = c;
                // Release: the released seqs of this segment travel with its give-back, the one
                // read-modify-write on this side, on the switch path only.
                segs.in_use().fetch_and(!(1 << st.cur), Ordering::AcqRel);
                st.cur = k;
                st.pos = st.resume[k as usize];
                st.switches += 1;
                // A segment given back is room a producer asleep on a full ring can take.
                if W::WAKES {
                    segs.wake_producers::<W>();
                }
                continue;
            }
            // Not committed yet (or a peer-corrupted seq: degrade toward Empty, never toward
            // reading an unowned slot).
            let look = EmptyLook {
                segs,
                cur,
                pos: c,
                multi: M::MULTI,
                _wake: PhantomData,
            };
            if !on_empty(attempt, &look) {
                return Err(Empty);
            }
            attempt = attempt.saturating_add(1);
        }
        let msg = segs.body(st.cur, st.pos) as *const T;
        // SAFETY: msg is in-bounds and aligned (check_body_type against the body's offset in a
        // line-aligned slot), any byte pattern is a valid T (FromBytes bound), and the seq protocol
        // gives this consumer read access until the seq store below.
        let r = read_msg(unsafe { &*msg });
        let c = st.pos;
        st.pos = seq_of(c.wrapping_add(1));
        segs.seq(st.cur, c)
            .store(seq_of(c.wrapping_add(segs.capacity)), Ordering::Release);
        if W::WAKES && st.pos & segs.wake_mask == 0 {
            segs.wake_producers::<W>();
        }
        Ok(r)
    }
}

/// `struct EmptyLook` is one look that found the ring empty: the segment and position the consumer
/// read, which is what a sleep waits on.
struct EmptyLook<'a, W: Waits> {
    /// Geometry and segment addresses.
    segs: &'a Segments,
    /// The segment the consumer read.
    cur: u32,
    /// The position the consumer read, not yet committed.
    pos: u32,
    /// Whether the ring is `Multi`.
    multi: bool,
    _wake: PhantomData<W>,
}

impl<W: Waits> EmptyLook<'_, W> {
    /// `sleep_empty` sleeps on the empty ring until a producer claims past the look's position, a
    /// wake comes early, or the timeout passes: `deadline`, or `W`'s own timeout when `deadline` is
    /// `None`. With [`SpinOnly`] it is one spin hint.
    ///
    /// - Before the sleep, everything read is released: any producer asleep on a full ring is
    ///   woken, so the two sides never sleep on each other. Not on the polling path, where a fence
    ///   per empty look would cost a round trip its every message.
    /// - Set the waiting flag on the claim word, then sleep only while the word, flag aside, still
    ///   names the look's segment and position: nothing is claimed past what the consumer read, so
    ///   no commit is on its way, and every later claim CAS sees the flag and wakes this side after
    ///   its commit. A claim between the flag and the sleep moves the word, and the sleep returns
    ///   at once.
    /// - A wake can come between a producer's claim and its commit, and the look after it then
    ///   finds the slot claimed and not yet committed, which does not sleep again.
    /// - `Single` compares the position alone, its segment bits being always 0.
    /// - The flag is cleared after, so producers stop waking a consumer that is awake.
    #[cold]
    #[inline(never)]
    fn sleep_empty(&self, deadline: Option<Deadline>) {
        if !W::WAKES {
            core::hint::spin_loop();
            return;
        }
        self.segs.wake_producers::<W>();
        let claim = self.segs.claim();
        let prev = claim.fetch_or(WAITING, Ordering::SeqCst);
        let here = if self.multi {
            prev & !WAITING == word(self.cur, self.pos)
        } else {
            word_pos(prev) == seq_of(self.pos)
        };
        if here {
            let seen = Seen::written(claim, prev | WAITING);
            match deadline {
                Some(d) => W::wait_until(seen, d),
                None => W::wait(seen),
            }
        }
        claim.fetch_and(!WAITING, Ordering::SeqCst);
    }
}

impl<W: Waits> Sleeper for EmptyLook<'_, W> {
    fn sleep(&self) {
        self.sleep_empty(None);
    }
}
