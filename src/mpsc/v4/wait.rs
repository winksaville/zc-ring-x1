//! MPSC v4 waiting: [`WaitPolicy`], what a `send` asks at a full ring and a `recv` asks at an empty
//! one, and [`Waiter`], what a policy sleeps through. One trait serves both endpoints, so a policy
//! written once waits on either side.

/// `trait WaitPolicy` is what a [`send`](super::MpscProducer::send) does when the ring is full, and
/// what a [`recv`](super::MpscConsumer::recv) does when the ring is empty: look again, sleep first,
/// or give up.
///
/// - A closure `FnMut(u32) -> bool` is a `WaitPolicy`: its argument is the attempt count and its
///   result is [`on_wait`](WaitPolicy::on_wait)'s, so `|attempt| attempt < 100` gives up after a
///   hundred looks, `|_| false` makes one attempt, and [`policy::spin`](crate::policy::spin) never
///   gives up.
/// - A type implementing `WaitPolicy` can also sleep through [`Waiter`], and on a producer sees each
///   slot another producer took first.
/// - A `send` or a `recv` takes its policy by value. A policy whose state the caller reads
///   afterward, such as a count, implements `WaitPolicy` for `&mut` itself.
pub trait WaitPolicy {
    /// `on_wait` is called each time a `send` finds the ring full, or a `recv` finds the ring
    /// empty.
    ///
    /// # Parameters
    ///
    /// - `self`: the policy, by mutable reference, so it can keep state across calls, such as a
    ///   deadline or a count.
    /// - `attempt`: how many times this `send` or `recv` has found the ring full or empty before,
    ///   `0` the first time, saturating.
    /// - `waiter`: sleeps until the other side acts, for a policy that would rather sleep than
    ///   spin.
    ///
    /// # Returns
    ///
    /// - `true` to look at the ring again, or `false` to give up, and the `send` returns
    ///   `Err(Full)` or the `recv` returns `Err(Empty)`.
    fn on_wait(&mut self, attempt: u32, waiter: &Waiter<'_>) -> bool;

    /// `on_lost` is called each time another producer takes a slot this `send` was about to take.
    /// The `send` then looks again at once, and does not call `on_wait`: a lost slot is never a
    /// full ring. A `recv` never calls `on_lost`, since a ring has one consumer.
    ///
    /// # Parameters
    ///
    /// - `self`: the policy, by mutable reference.
    /// - `lost`: how many slots this `send` has lost in a row, `1` the first time, saturating.
    ///
    /// # Notes
    ///
    /// - By default `on_lost` does nothing. A policy backs off here, with
    ///   [`policy::backoff`](crate::policy::backoff), or counts contention.
    fn on_lost(&mut self, _lost: u32) {}
}

impl<F: FnMut(u32) -> bool> WaitPolicy for F {
    #[inline]
    fn on_wait(&mut self, attempt: u32, _waiter: &Waiter<'_>) -> bool {
        self(attempt)
    }
}

/// `struct Waiter` lets a [`WaitPolicy`] sleep until the other side acts: on a producer until the
/// consumer frees a slot, and on the consumer until a producer sends a message.
pub struct Waiter<'a> {
    sleeper: &'a dyn Sleeper,
}

impl<'a> Waiter<'a> {
    /// `Waiter::new` wraps the endpoint a policy sleeps through.
    pub(super) fn new(sleeper: &'a dyn Sleeper) -> Self {
        Waiter { sleeper }
    }

    /// `Waiter::sleep` sleeps until the other side acts, a wake comes early, or the ring's
    /// [`Wake`](crate::wake::Wake) times out, whichever is first. With
    /// [`NoWake`](crate::wake::NoWake) it is one spin hint.
    ///
    /// # Parameters
    ///
    /// - `self`: the waiter a policy was handed in [`on_wait`](WaitPolicy::on_wait).
    pub fn sleep(&self) {
        self.sleeper.sleep();
    }
}

/// `trait Sleeper` is the endpoint behind a [`Waiter`], with its ring's mode and wake erased, so
/// `Waiter` needs no type parameters.
pub(super) trait Sleeper {
    /// `sleep` is [`Waiter::sleep`].
    fn sleep(&self);
}
