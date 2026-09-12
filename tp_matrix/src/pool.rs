//! The pool-message sweep cells: the messaging layer's loop,
//! take a message from the pool, fill it, push its reference,
//! receive it, process it, return it, run over the descriptor
//! rings and cordyceps's intrusive queue on one pool, so the
//! queue is the only variable between rows.
//!
//! - The pool bounds the traffic: with `pool_size` buffers at
//!   most that many messages are in flight, so a ring depth at
//!   or above it never reports Full, and the cordyceps row,
//!   whose queue is unbounded, has no depth at all.
//! - Every row allocs from and frees to the same pool: the
//!   rings carry a `Desc`, cordyceps carries the buffer's
//!   pointer with the queue's link inside the buffer, and its
//!   consumer maps the pointer back to the buffer's index for
//!   the free.
//! - A cell moves a fixed count of messages, so its number is
//!   elapsed over count, the thread spawn inside it alike for
//!   every row, and the fill counters over the whole cell give
//!   the lines crossed per message.

use std::ptr::{self, NonNull};
use std::time::Instant;

use cordyceps::Linked;
use cordyceps::mpsc_queue::{Links, MpscQueue, TryDequeueError};
use tp_runner::{LineBuf, pin_to_cpu, unpin_current};
use zc_ring_x1::{CACHE_LINE_SIZE, Desc, Pool, PoolHeader, PoolRegistry, policy};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::FillCounts;

/// The queue a pool cell runs the loop over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PoolFlavor {
    /// The SPSC v2 ring carrying descriptors.
    SpscV2,
    /// The MPSC v1 ring at 1p/1c carrying descriptors.
    MpscV1,
    /// cordyceps's `MpscQueue` linked through the pool's own
    /// buffers.
    Cordyceps,
}

/// Every pool flavor, in table order.
pub const POOL_FLAVORS: [PoolFlavor; 3] = [
    PoolFlavor::SpscV2,
    PoolFlavor::MpscV1,
    PoolFlavor::Cordyceps,
];

impl PoolFlavor {
    /// Lowercase name for labels.
    pub fn as_str(self) -> &'static str {
        match self {
            PoolFlavor::SpscV2 => "spsc-v2",
            PoolFlavor::MpscV1 => "mpsc-v1",
            PoolFlavor::Cordyceps => "cordyceps",
        }
    }

    /// Whether the flavor has a ring depth to sweep. The
    /// cordyceps queue is unbounded, so its row runs once per
    /// pool size.
    pub fn has_depth(self) -> bool {
        !matches!(self, PoolFlavor::Cordyceps)
    }
}

/// One pool cell's outcome.
pub struct PoolResult {
    /// Wall-clock seconds from before the threads spawn to the
    /// consumer's last free.
    pub secs: f64,
    /// Fill counters over the whole cell, when the platform
    /// provides them.
    pub fills: Option<FillCounts>,
    /// `Inconsistent` results the cordyceps consumer retried,
    /// zero for the ring rows.
    pub inconsistent: u64,
}

/// The message in a pool buffer for the ring rows: the sequence
/// number the consumer asserts, at the front of the buffer.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
}

/// A pool buffer as bytes, the type the cordyceps row allocs
/// as, since its node holds atomics zerocopy cannot derive
/// over.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct RawBuf([u8; CACHE_LINE_SIZE]);

/// The cordyceps node laid over a pool buffer.
///
/// - `pool_link` is the buffer's first word, the pool's own
///   free-stack link, which the pool writes while the buffer is
///   free and the node leaves alone.
/// - `links` is the queue's, one atomic next pointer.
/// - `seq` is the payload the consumer asserts.
///
/// Fields are reached through raw pointers only, never a
/// reference to the whole node, since the pool writes the first
/// word behind the node's back once the buffer is freed.
#[repr(C)]
struct Node {
    pool_link: u64,
    links: Links<Node>,
    seq: u64,
}

const _: () = assert!(size_of::<Node>() <= CACHE_LINE_SIZE);

/// The queue's handle to a node: the pointer alone, since the
/// pool owns the storage and the consumer frees by index.
struct NodePtr(NonNull<Node>);

// SAFETY: a node sits in a pool buffer that never moves while
// allocated, the handle is the pointer itself so `into_ptr` and
// `from_ptr` are identities, and `links` points at the node's
// own `Links` field.
unsafe impl Linked<Links<Node>> for Node {
    type Handle = NodePtr;

    fn into_ptr(handle: NodePtr) -> NonNull<Node> {
        handle.0
    }

    unsafe fn from_ptr(ptr: NonNull<Node>) -> NodePtr {
        NodePtr(ptr)
    }

    unsafe fn links(target: NonNull<Node>) -> NonNull<Links<Node>> {
        // SAFETY: `target` is a live node, so its field address
        // is in bounds and non-null.
        unsafe { NonNull::new_unchecked(ptr::addr_of_mut!((*target.as_ptr()).links)) }
    }
}

/// Bytes a pool of `pool_size` one-line buffers needs.
fn pool_region_size(pool_size: u32) -> u64 {
    size_of::<PoolHeader>() as u64 + pool_size as u64 * CACHE_LINE_SIZE as u64
}

/// Run one pool cell: `count` messages through the loop at
/// `flavor`, a pool of `pool_size` buffers, and ring `depth`
/// (ignored by the cordyceps row), the producer on `pin.0` and
/// the consumer on `pin.1`, with the fill counters open across
/// it.
pub fn run_pool_cell(
    flavor: PoolFlavor,
    pin: Option<(usize, usize)>,
    pool_size: u32,
    depth: u32,
    count: u64,
) -> PoolResult {
    unpin_current();
    #[cfg(target_os = "linux")]
    let fills = crate::Fills::open();
    let (secs, inconsistent) = match flavor {
        PoolFlavor::SpscV2 => (cell_spsc_v2(pin, pool_size, depth, count), 0),
        PoolFlavor::MpscV1 => (cell_mpsc_v1(pin, pool_size, depth, count), 0),
        PoolFlavor::Cordyceps => cell_cordyceps(pin, pool_size, count),
    };
    #[cfg(target_os = "linux")]
    let fills = fills.and_then(crate::Fills::finish);
    #[cfg(not(target_os = "linux"))]
    let fills = None;
    PoolResult {
        secs,
        fills,
        inconsistent,
    }
}

/// The SPSC v2 ring cell: descriptors cross a v2 ring of
/// `depth` line-sized slots.
fn cell_spsc_v2(pin: Option<(usize, usize)>, pool_size: u32, depth: u32, count: u64) -> f64 {
    use zc_ring_x1::spsc::v2::{Ring, region_size};
    let slot = CACHE_LINE_SIZE as u32;
    let mut region = LineBuf::new(region_size(slot, depth));
    let (mut tx, mut rx) = Ring::init(region.as_mut_bytes(), slot, depth)
        .expect("region sized by region_size and line-aligned") // OK: the region is the ring's own size and LineBuf is line-aligned
        .split();
    run_ring_cell(
        pin,
        pool_size,
        count,
        move |desc| {
            let mut slot = tx
                .reserve_slot_with::<Desc>(policy::spin)
                .expect("spin never gives up"); // OK: policy::spin never gives up
            *slot = desc;
            slot.commit();
        },
        move || {
            let slot = rx
                .reserve_slot_with::<Desc>(policy::spin)
                .expect("spin never gives up"); // OK: policy::spin never gives up
            let desc = *slot;
            slot.release();
            desc
        },
    )
}

/// The MPSC v1 ring cell at 1p/1c: descriptors cross a v1 ring
/// of `depth` line-sized slots.
fn cell_mpsc_v1(pin: Option<(usize, usize)>, pool_size: u32, depth: u32, count: u64) -> f64 {
    use zc_ring_x1::mpsc::v1::{MpscRing, mpsc_region_size};
    let slot = CACHE_LINE_SIZE as u32;
    let mut region = LineBuf::new(mpsc_region_size(slot, depth));
    let (tx, mut rx) = MpscRing::init(region.as_mut_bytes(), slot, depth)
        .expect("region sized by mpsc_region_size and line-aligned") // OK: the region is the ring's own size and LineBuf is line-aligned
        .split();
    run_ring_cell(
        pin,
        pool_size,
        count,
        move |desc| {
            tx.send_with::<Desc>(policy::spin, |m| *m = desc)
                .expect("spin never gives up"); // OK: policy::spin never gives up
        },
        move || {
            let slot = rx
                .reserve_slot_with::<Desc>(policy::spin)
                .expect("spin never gives up"); // OK: policy::spin never gives up
            let desc = *slot;
            slot.release();
            desc
        },
    )
}

/// The ring rows' loop over a pool of `pool_size` buffers: the
/// producer allocs, fills, converts the guard to a descriptor,
/// and hands it to `send`, and the consumer takes one from
/// `recv`, resolves it, asserts the sequence, and frees.
fn run_ring_cell(
    pin: Option<(usize, usize)>,
    pool_size: u32,
    count: u64,
    mut send: impl FnMut(Desc) + Send,
    mut recv: impl FnMut() -> Desc + Send,
) -> f64 {
    let mut pool_region = LineBuf::new(pool_region_size(pool_size));
    let mut pool = Pool::init(
        pool_region.as_mut_bytes(),
        CACHE_LINE_SIZE as u32,
        pool_size,
    )
    .expect("region sized for the pool and line-aligned"); // OK: the region is the pool's own size and LineBuf is line-aligned
    let mut registry = PoolRegistry::<1>::new();
    let pool_id = registry
        .register(pool.resolver())
        .expect("an empty registry has room"); // OK: capacity 1, nothing registered yet
    let registry = &registry;

    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some((p, _)) = pin {
                pin_to_cpu(p);
            }
            for i in 0..count {
                let mut buf = pool
                    .alloc_with::<Msg>(policy::spin)
                    .expect("spin never gives up"); // OK: policy::spin never gives up
                buf.seq = i;
                let desc = registry
                    .into_desc(pool_id, buf)
                    .map_err(|(_, e)| e)
                    .expect("the guard is from the registered pool"); // OK: pool_id came from this registry's register
                send(desc);
            }
        });
        let consumer = s.spawn(move || {
            if let Some((_, c)) = pin {
                pin_to_cpu(c);
            }
            for i in 0..count {
                let desc = recv();
                // SAFETY: the descriptor was minted by the
                // producer's into_desc and read after the ring's
                // commit -> reserve handoff, and each is resolved
                // exactly once.
                let msg = unsafe { registry.resolve::<Msg>(desc) }
                    .expect("descriptors come from the producer's into_desc"); // OK: only the producer mints them, on this pool
                assert_eq!(msg.seq, i, "pool stream order broken");
                msg.free();
            }
        });
        consumer.join().expect("consumer panicked"); // OK: a panic in the consumer is the cell's failure
    });
    start.elapsed().as_secs_f64()
}

/// The cordyceps cell: the queue links through the pool's
/// buffers. The producer allocs a buffer, lays the node over it,
/// and enqueues its pointer, and the consumer dequeues, asserts
/// the sequence, maps the pointer to the buffer's index, and
/// frees through the registry. Returns the seconds and the
/// `Inconsistent` results the consumer retried.
fn cell_cordyceps(pin: Option<(usize, usize)>, pool_size: u32, count: u64) -> (f64, u64) {
    let mut pool_region = LineBuf::new(pool_region_size(pool_size));
    let mut pool = Pool::init(
        pool_region.as_mut_bytes(),
        CACHE_LINE_SIZE as u32,
        pool_size,
    )
    .expect("region sized for the pool and line-aligned"); // OK: the region is the pool's own size and LineBuf is line-aligned
    let mut registry = PoolRegistry::<1>::new();
    let pool_id = registry
        .register(pool.resolver())
        .expect("an empty registry has room"); // OK: capacity 1, nothing registered yet
    let registry = &registry;

    // Buffer 0's address, so the consumer can turn a node
    // pointer back into an index: alloc one, read its address
    // and index, and hand it back.
    let base = {
        let probe = pool
            .alloc::<RawBuf>()
            .expect("a fresh pool has a free buffer"); // OK: nothing allocated yet and pool_size is at least 1
        let addr = ptr::from_ref::<RawBuf>(&probe) as usize;
        let desc = registry
            .into_desc(pool_id, probe)
            .map_err(|(_, e)| e)
            .expect("the guard is from the registered pool"); // OK: pool_id came from this registry's register
        // SAFETY: the descriptor was minted just above and is
        // resolved once, on this thread.
        unsafe { registry.resolve::<RawBuf>(desc) }
            .expect("the descriptor was minted just above") // OK: same pool, index in range
            .free();
        addr - desc.buf_idx as usize * CACHE_LINE_SIZE
    };

    // The stub the queue owns, leaked so it is static: a
    // handful of bytes per cell, and the queue's drop then
    // touches nothing the pool owns.
    let stub: &'static Node = Box::leak(Box::new(Node {
        pool_link: 0,
        links: Links::new_stub(),
        seq: 0,
    }));
    // SAFETY: the stub's links are `new_stub`, it is leaked so
    // it lives as long as the program, and nothing else touches
    // it.
    let queue = unsafe { MpscQueue::<Node>::new_with_static_stub(stub) };
    let queue = &queue;

    let start = Instant::now();
    let inconsistent = std::thread::scope(|s| {
        s.spawn(move || {
            if let Some((p, _)) = pin {
                pin_to_cpu(p);
            }
            for i in 0..count {
                let mut buf = pool
                    .alloc_with::<RawBuf>(policy::spin)
                    .expect("spin never gives up"); // OK: policy::spin never gives up
                let node = ptr::from_mut::<RawBuf>(&mut buf).cast::<Node>();
                // Keep the buffer allocated without a guard: the
                // consumer frees it by index.
                registry
                    .into_desc(pool_id, buf)
                    .map_err(|(_, e)| e)
                    .expect("the guard is from the registered pool"); // OK: pool_id came from this registry's register
                // SAFETY: the buffer is line-aligned and at least
                // a node long, allocated to this thread until the
                // consumer frees it, and no reference to it
                // exists.
                unsafe {
                    ptr::addr_of_mut!((*node).links).write(Links::new());
                    ptr::addr_of_mut!((*node).seq).write(i);
                    queue.enqueue(NodePtr(NonNull::new_unchecked(node)));
                }
            }
        });
        let consumer = s.spawn(move || {
            if let Some((_, c)) = pin {
                pin_to_cpu(c);
            }
            let guard = queue.consume();
            let mut inconsistent = 0u64;
            for i in 0..count {
                let node = loop {
                    match guard.try_dequeue() {
                        Ok(node) => break node,
                        Err(TryDequeueError::Inconsistent) => {
                            inconsistent += 1;
                            std::hint::spin_loop();
                        }
                        Err(TryDequeueError::Empty) => std::hint::spin_loop(),
                        Err(TryDequeueError::Busy) => {
                            unreachable!("the consumer guard is held")
                        }
                    }
                };
                let node = node.0.as_ptr();
                // SAFETY: the node is the consumer's from the
                // dequeue until the free below, and the producer
                // wrote `seq` before its enqueue's release.
                let seq = unsafe { ptr::addr_of!((*node).seq).read() };
                assert_eq!(seq, i, "pool stream order broken");
                let idx = ((node as usize - base) / CACHE_LINE_SIZE) as u32;
                let desc = Desc {
                    pool_id: pool_id.as_u32(),
                    buf_idx: idx,
                };
                // SAFETY: the index is the buffer the producer
                // allocated for this node, resolved once, after
                // the dequeue's acquire.
                unsafe { registry.resolve::<RawBuf>(desc) }
                    .expect("the index is a buffer of this pool") // OK: computed from a pool buffer's address
                    .free();
            }
            inconsistent
        });
        consumer.join().expect("consumer panicked") // OK: a panic in the consumer is the cell's failure
    });
    (start.elapsed().as_secs_f64(), inconsistent)
}
