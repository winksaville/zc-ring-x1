//! Zero-copy no_std SPSC ring buffer over a caller-provided
//! memory region, per [notes/ring-buffer-design.md]:
//!
//! - The default [`Ring`] is `spsc::v3`, a ring of segments taken
//!   from a [`Pool`]. The single-region rings, `spsc::v0` through
//!   `spsc::v2`, stay available by path, and the points below
//!   describe them.
//! - One `#[repr(C)]` header spanning four cache lines
//!   (immutable geometry, producer index, consumer index, app
//!   user words), followed by M slots of N bytes.
//! - Indices are free-running `AtomicU32`s, masked only at slot
//!   access, and full is `p - c == M`, no sacrificial slot.
//! - Messages move in place: the producer writes through a
//!   [`WriteSlot`] `&mut T`, the consumer reads through a
//!   [`ReadSlot`] `&T`, zerocopy traits bound `T`, no
//!   serialization step. Each side reserves at most one slot at
//!   a time, and the guard holds the endpoint borrow until commit /
//!   release (or drop).
//! - The SPSC protocol lives in the `spsc` module, and a
//!   multi-producer sibling, [`MpscRing`], lives in the
//!   `mpsc` module (gated on CAS support), see its module
//!   docs for the claim/seq protocol and closure-send API.
//!   Primitive modules hold versioned sibling implementations
//!   (`spsc::v0`, ...) behind per-module default-version
//!   re-exports, and this crate root re-exports the defaults.
//! - `spsc::v4`, by path, is the ring of segments a second
//!   process can join: `Ring::attach` from a pool and a first
//!   segment, and the roles taken by name.
//! - Message pools live in the `pool` module: the default
//!   [`Pool`] is `pool::v0`, a single-stack pool of one buffer
//!   size, and `pool::v1` is a multi-stack pool, one stack per
//!   buffer size, by path. "Segment" is the rings' word and
//!   "stack" the pools'.
//! - How to use the segmented rings, [`Ring`] and
//!   `mpsc::v2::MpscRing`, from a pool to two threads is the
//!   [user guide], with two complete programs in `examples/`.
//!
//! [user guide]: https://github.com/winksaville/zc-ring-x1/blob/main/notes/user-guide.md
//! [notes/ring-buffer-design.md]: https://github.com/winksaville/zc-ring-x1/blob/main/notes/ring-buffer-design.md

// The crate is `no_std` unless testing or the `std` feature is on, which brings the deadline sends'
// clock to targets other than Linux.
#![cfg_attr(not(any(test, feature = "std")), no_std)]

use core::mem::{align_of, size_of};
use core::sync::atomic::AtomicU32;

// The MPSC ring needs CAS (the claim), so it is gated. The
// SPSC ring protocol stays load/store-only. (The pools'
// free-stacks also use CAS and are not gated, v0 having
// predated the gate, see notes/bugs.md.)
#[cfg(target_has_atomic = "32")]
#[cfg(any(target_os = "linux", feature = "std"))]
mod clock;
pub mod mpsc;
pub mod policy;
pub mod pool;
mod registry;
pub mod spsc;
pub mod wake;

#[cfg(any(target_os = "linux", feature = "std"))]
pub use clock::{microsecs_to_ticks, nanos_to_ticks};
#[cfg(target_has_atomic = "32")]
pub use mpsc::{MpscConsumer, MpscHeader, MpscProducer, MpscReadSlot, MpscRing, mpsc_region_size};
pub use pool::{BufSlot, Exhausted, Pool, PoolHeader, PoolView};
pub use registry::{Desc, DescMap, PoolId, PoolRegistry, RegistryError};
pub use spsc::{Consumer, Producer, ReadSlot, Ring, WriteSlot};

/// Cache-line size the layout is built around.
///
/// - Slot size must be a multiple of this, and slots and the
///   region itself must be aligned to it.
pub const CACHE_LINE_SIZE: usize = 64;

/// Number of `AtomicU32` words in the header's app-owned
/// `user` line.
pub const USER_WORDS: usize = 16;

/// `reserve_slot_with` / `send_with` failed: every slot holds
/// an uncommitted-or-unread message.
///
/// - Shared by the ring primitives (SPSC reserve, MPSC send),
///   defined in the crate core so no primitive depends on a
///   sibling's version module for its error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

/// `reserve_slot_with` failed: no unread messages.
///
/// - Shared by the ring primitives' consumer endpoints, see
///   [`Full`] for why it lives in the crate core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty;

/// `struct Ticks` is a duration: a count of ticks of the crate's monotonic clock, held in a `u64`.
///
/// - A send's `give_up`, `spin`, and `wait` are each a `Ticks`.
/// - A `Ticks` is not a point in time. A point in time is a [`Deadline`].
/// - A caller makes a `Ticks` with [`microsecs_to_ticks`] or [`nanos_to_ticks`], once, when it sets
///   up its times, so a send only adds and compares ticks and never converts a unit.
/// - A tick is one nanosecond today. A later clock may count the CPU's own counter instead, and a
///   caller that makes its `Ticks` with the conversions does not change.
/// - `Ticks` lives in the crate core, as [`Full`] does, because the sends and the waits share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(u64);

impl Ticks {
    /// `Ticks::ZERO` is a duration of no ticks. A `give_up` of `Ticks::ZERO` makes one attempt, and
    /// a `spin` of it sleeps at once.
    pub const ZERO: Ticks = Ticks(0);

    /// `Ticks::FOREVER` is a duration that never passes. A `give_up` or `wait` of `Ticks::FOREVER`
    /// never gives up, and a `spin` of it never sleeps.
    pub const FOREVER: Ticks = Ticks(u64::MAX);
}

/// `struct Deadline` is a point in time: a reading of the crate's monotonic clock, in ticks, held
/// in a `u64`, at which a wait gives up.
///
/// - A `Deadline` is not a duration. A duration is a [`Ticks`].
/// - The crate makes a `Deadline` at a send's first full ring: the clock's reading plus the send's
///   `Ticks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Deadline(u64);

impl Deadline {
    /// `Deadline::at` makes the deadline at `ticks` on the crate's clock.
    #[cfg_attr(not(any(target_os = "linux", feature = "std")), allow(dead_code))]
    pub(crate) const fn at(ticks: u64) -> Self {
        Deadline(ticks)
    }

    /// `Deadline::monotonic_nanos` returns the deadline in nanoseconds of `CLOCK_MONOTONIC`, the
    /// clock a Linux futex sleeps against.
    ///
    /// - A tick is one nanosecond today, so the value returned is the deadline's own count.
    pub fn monotonic_nanos(self) -> u64 {
        self.0
    }
}

/// Cache-line-aligned wrapper granting its field sole
/// ownership of the line.
///
/// - `repr(align(N))` accepts only an integer literal. It
///   cannot name [`CACHE_LINE_SIZE`], so the `64` is written out
///   and a const assert ties them back together.
#[repr(C, align(64))]
struct CacheAligned<T>(T);

const _: () = assert!(align_of::<CacheAligned<AtomicU32>>() == CACHE_LINE_SIZE);

impl<T> core::ops::Deref for CacheAligned<T> {
    type Target = T;
    /// Access the wrapped value.
    fn deref(&self) -> &T {
        &self.0
    }
}

/// Errors from region validation, the rings' `init` / `attach`,
/// [`Ring::init`], [`Pool::init`] / [`Pool::attach`], and
/// `pool::v1`'s `init` / `attach`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Region is not [`CACHE_LINE_SIZE`]-aligned.
    Misaligned,
    /// Region is smaller than header + slots/buffers.
    TooSmall,
    /// Slot size is zero or not a [`CACHE_LINE_SIZE`] multiple.
    BadSlotSize,
    /// Capacity is zero, not a power of two, over the ring's
    /// cap, or under its floor (the MPSC v0 ring's is 2), or a
    /// takeover of a held role on a ring of one-slot segments,
    /// whose seq words cannot place a position.
    BadCapacity,
    /// Pool: buffer size is zero or not a [`CACHE_LINE_SIZE`]
    /// multiple, or a multi-stack pool has two stacks of one
    /// size (on attach, stacks out of the pool's order).
    BadBufSize,
    /// Pool: buffer count is zero or `u32::MAX` (the
    /// free-stack NIL sentinel), or a multi-stack pool has no
    /// stack or a total count reaching the sentinel.
    BadBufCount,
    /// Attach: a multi-stack pool region built with another
    /// stack count.
    BadStackCount,
    /// Attach: magic mismatch, not a region of the expected
    /// kind.
    BadMagic,
    /// Attach: layout version mismatch.
    BadLayoutVersion,
    /// Attach: region built with a different [`CACHE_LINE_SIZE`].
    BadCacheLine,
    /// A ring of segments: the segment count is zero or over the
    /// most a ring holds.
    BadSegmentCount,
    /// A ring of segments: the pool had fewer free buffers than
    /// the ring's segments.
    Exhausted,
    /// A ring of segments, attach: the control block names a
    /// buffer outside the pool or one twice, or a segment's own
    /// header disagrees with the control block.
    BadSegment,
    /// A ring of segments: the role asked for is already held, in
    /// this process or another, or every producer role an MPSC v3
    /// ring allows is.
    RoleTaken,
    /// A ring of segments: a claim's holder id is `0` or
    /// `u32::MAX`, the role word's two values that name no holder.
    BadHolder,
    /// A ring of segments: a role's checkpoint names a segment the
    /// ring does not have, or a takeover's scan finds seq words
    /// the ring could not have written.
    BadCheckpoint,
    /// MPSC v3: a claim on a ring its release has closed, or a
    /// second release of it.
    RingClosed,
    /// MPSC v3: a release of a ring whose roles are not all given
    /// back.
    RingInUse,
    /// MPSC v3: a ring's most producers is `0`, which no producer
    /// could claim under.
    BadMaxProducers,
    /// MPSC v3, attach: the ring was built for the other segment
    /// mode, `Single` or `Multi`.
    BadMode,
}

/// Check `T` fits a slot, called once per `reserve_slot_with`
/// (both endpoints).
///
/// - Panics on a type-geometry mismatch. That is a programming
///   error, not a runtime condition.
fn check_type<T>(slot_size: u32) {
    assert!(size_of::<T>() <= slot_size as usize, "T larger than slot");
    assert!(
        core::mem::align_of::<T>() <= CACHE_LINE_SIZE,
        "T alignment exceeds slot alignment"
    );
}

/// Non-panicking form of [`check_type`], for
/// `to_slot`. There the pool compared against is selected by
/// untrusted input, which must not be able to select a panic.
fn type_fits<T>(slot_size: u32) -> bool {
    size_of::<T>() <= slot_size as usize && core::mem::align_of::<T>() <= CACHE_LINE_SIZE
}

/// Pointer to the slot for free-running index `idx`.
///
/// - Returned range is `slot_size` bytes, cache-line aligned
///   (base is, and slot_size is a line multiple).
fn slot_ptr(slots: *mut u8, idx: u32, mask: u32, slot_size: u32) -> *mut u8 {
    let pos = (idx & mask) as usize;
    // SAFETY: pos < capacity, so the offset is inside the slot
    // array validated at init/attach.
    unsafe { slots.add(pos * slot_size as usize) }
}
