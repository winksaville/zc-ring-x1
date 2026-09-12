//! Exercises `cordyceps::MpscQueue`, Vyukov's intrusive MPSC,
//! as the linked-list prior art the pool-message sweep measures
//! beside the descriptor rings. Nothing here touches the crate:
//! the tests pin down the queue's contract, the parts the sweep
//! and the design note lean on, so a cordyceps upgrade that
//! moves one of them fails here first.
//!
//! The contract under test:
//!
//! - FIFO across one producer, and per-producer order across
//!   several, since a push is a swap on the head and a store of
//!   the previous node's link.
//! - `Empty` from a drained queue, `Busy` from a second consumer
//!   while a `Consumer` guard is held.
//! - `Inconsistent` from the window between a producer's head
//!   swap and its link store, which a threaded consumer sees
//!   and retries.
//! - Drop hands every node still enqueued back through its
//!   handle, the stub included.

use std::pin::Pin;
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use cordyceps::Linked;
use cordyceps::mpsc_queue::{Links, MpscQueue, TryDequeueError};

/// A preallocated message: its link, a producer tag, a sequence
/// number, and an optional drop counter for the hand-back test.
struct Node {
    links: Links<Node>,
    producer: u32,
    seq: u64,
    dropped: Option<Arc<AtomicUsize>>,
}

impl Node {
    fn new(producer: u32, seq: u64) -> Pin<Box<Node>> {
        Box::pin(Node {
            links: Links::new(),
            producer,
            seq,
            dropped: None,
        })
    }

    fn counted(seq: u64, counter: &Arc<AtomicUsize>) -> Pin<Box<Node>> {
        Box::pin(Node {
            links: Links::new(),
            producer: 0,
            seq,
            dropped: Some(Arc::clone(counter)),
        })
    }

    /// The stub the queue owns: never dequeued as a message.
    fn stub() -> Pin<Box<Node>> {
        Box::pin(Node {
            links: Links::new_stub(),
            producer: u32::MAX,
            seq: u64::MAX,
            dropped: None,
        })
    }

    fn counted_stub(counter: &Arc<AtomicUsize>) -> Pin<Box<Node>> {
        Box::pin(Node {
            links: Links::new_stub(),
            producer: u32::MAX,
            seq: u64::MAX,
            dropped: Some(Arc::clone(counter)),
        })
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if let Some(counter) = &self.dropped {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// SAFETY: the handle is a pinned box, so a node never moves while
// the queue links through it, `into_ptr` leaks the box and
// `from_ptr` reclaims exactly that leak, and `links` points at
// the node's own `Links` field.
unsafe impl Linked<Links<Node>> for Node {
    type Handle = Pin<Box<Node>>;

    fn into_ptr(handle: Pin<Box<Node>>) -> NonNull<Node> {
        // SAFETY: the box is leaked, so its contents stay pinned
        // until `from_ptr` reclaims them.
        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
    }

    unsafe fn from_ptr(ptr: NonNull<Node>) -> Pin<Box<Node>> {
        // SAFETY: the pointer came from `into_ptr`, a leaked pinned
        // box, and the queue hands each one back at most once.
        unsafe { Pin::new_unchecked(Box::from_raw(ptr.as_ptr())) }
    }

    unsafe fn links(target: NonNull<Node>) -> NonNull<Links<Node>> {
        // SAFETY: `target` is a live node, so its field address is
        // in bounds and non-null.
        unsafe { NonNull::new_unchecked(ptr::addr_of_mut!((*target.as_ptr()).links)) }
    }
}

fn queue() -> MpscQueue<Node> {
    MpscQueue::new_with_stub(Node::stub())
}

#[test]
fn fifo_one_producer() {
    let q = queue();
    for seq in 0..100 {
        q.enqueue(Node::new(1, seq));
    }
    for seq in 0..100 {
        let node = q.dequeue().expect("100 enqueued, fewer dequeued");
        assert_eq!(node.producer, 1);
        assert_eq!(node.seq, seq);
    }
    assert!(q.dequeue().is_none());
}

#[test]
fn empty_from_a_drained_queue() {
    let q = queue();
    assert_eq!(q.try_dequeue().err(), Some(TryDequeueError::Empty));
    assert!(q.dequeue().is_none());

    q.enqueue(Node::new(1, 0));
    assert_eq!(q.dequeue().map(|n| n.seq), Some(0));
    assert_eq!(q.try_dequeue().err(), Some(TryDequeueError::Empty));
}

#[test]
fn busy_under_a_held_consumer() {
    let q = queue();
    let consumer = q.try_consume().expect("no consumer held yet");

    assert_eq!(consumer.try_dequeue().err(), Some(TryDequeueError::Empty));
    assert_eq!(q.try_dequeue().err(), Some(TryDequeueError::Busy));
    assert!(q.try_consume().is_none());

    q.enqueue(Node::new(1, 7));
    assert_eq!(q.try_dequeue().err(), Some(TryDequeueError::Busy));
    assert_eq!(consumer.try_dequeue().map(|n| n.seq).ok(), Some(7));
    assert_eq!(consumer.try_dequeue().err(), Some(TryDequeueError::Empty));

    drop(consumer);
    assert_eq!(q.try_dequeue().err(), Some(TryDequeueError::Empty));
    assert!(q.try_consume().is_some());
}

/// Two producers, per-producer order at the consumer, and the
/// `Inconsistent` window counted rather than hidden: the
/// consumer uses `try_dequeue` and tallies each error it retries.
#[test]
fn two_producers_keep_per_producer_order() {
    const PER_PRODUCER: u64 = 200_000;
    const PRODUCERS: u32 = 2;

    let q = Arc::new(queue());
    let mut inconsistent = 0u64;
    let mut empty = 0u64;
    let mut next_seq = [0u64; PRODUCERS as usize];
    let mut received = 0u64;

    thread::scope(|s| {
        for producer in 0..PRODUCERS {
            let q = Arc::clone(&q);
            s.spawn(move || {
                for seq in 0..PER_PRODUCER {
                    q.enqueue(Node::new(producer, seq));
                }
            });
        }

        let consumer = q.consume();
        while received < PER_PRODUCER * u64::from(PRODUCERS) {
            match consumer.try_dequeue() {
                Ok(node) => {
                    let slot = &mut next_seq[node.producer as usize];
                    assert_eq!(node.seq, *slot, "producer {} out of order", node.producer);
                    *slot += 1;
                    received += 1;
                }
                Err(TryDequeueError::Inconsistent) => {
                    inconsistent += 1;
                    std::hint::spin_loop();
                }
                Err(TryDequeueError::Empty) => {
                    empty += 1;
                    std::hint::spin_loop();
                }
                Err(TryDequeueError::Busy) => panic!("the guard is held, Busy cannot happen"),
            }
        }
    });

    assert_eq!(next_seq, [PER_PRODUCER; PRODUCERS as usize]);
    assert!(q.dequeue().is_none());
    // Not asserted above zero: the window is a race, and a run
    // that never lands in it is a valid run.
    eprintln!(
        "cordyceps threaded: {received} received, {inconsistent} Inconsistent, {empty} Empty retries"
    );
}

#[test]
fn drop_hands_back_enqueued_nodes() {
    let counter = Arc::new(AtomicUsize::new(0));
    let q = MpscQueue::<Node>::new_with_stub(Node::counted_stub(&counter));
    for seq in 0..5 {
        q.enqueue(Node::counted(seq, &counter));
    }

    // Two handed out and held: theirs to drop, not the queue's.
    let held: Vec<Pin<Box<Node>>> = (0..2)
        .map(|_| q.dequeue().expect("five enqueued"))
        .collect();
    assert_eq!(counter.load(Ordering::Relaxed), 0);

    drop(q);
    // The three still enqueued and the stub.
    assert_eq!(counter.load(Ordering::Relaxed), 4);

    drop(held);
    assert_eq!(counter.load(Ordering::Relaxed), 6);
}
