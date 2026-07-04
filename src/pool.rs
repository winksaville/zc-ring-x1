//! Message pool: fixed-size buffers over a caller-provided
//! region, handed out through an intrusive LIFO free-stack —
//! the allocation half of the messaging layer, decoupling
//! "get a message" from "send it" (see the design doc's
//! "Messaging layer: pools and descriptor queues").
//!
//! - One `#[repr(C)]` [`PoolHeader`] spanning two cache lines
//!   (immutable geometry, free-stack head), followed by
//!   `buf_count` buffers of `buf_size` bytes each.
//! - The free-stack is intrusive with zero per-buffer
//!   overhead: a *free* buffer's first word holds the index
//!   of the next free buffer; an allocated buffer carries no
//!   header at all (in-buffer provenance is deferred — the
//!   descriptor is the travel form; see the design doc's
//!   "Descriptor and registry design (0.7.0)").
//! - One owning allocator pops ([`Pool::alloc`], `&mut self`
//!   as the in-process single-popper token); any holder
//!   frees ([`BufSlot::free`], the MPSC push side). The
//!   [`BufSlot`] guard does not borrow the pool, so any
//!   number of allocated buffers may be live at once —
//!   the decoupling the messaging layer exists for.

use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error, check_type};

/// `alloc` failed: no free buffer (or a peer-corrupted
/// free-stack index — validation fails toward exhaustion,
/// never toward handing out memory the pool doesn't own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted;

/// Layout marker written by [`Pool::init`]; rejects foreign
/// (including ring) regions on attach.
const POOL_MAGIC: u32 = 0x5A43_5031; // "ZCP1"

/// Bumped on any change to the pool region layout.
const POOL_LAYOUT_VERSION: u32 = 1;

/// Free-stack terminator: no next free buffer.
///
/// - Also why `buf_count < u32::MAX`: every valid index must
///   be distinguishable from the sentinel.
const NIL: u32 = u32::MAX;

/// Control block at offset 0 of a pool region — two cache
/// lines: cold geometry, then the contended free-stack head.
///
/// - line 0: geometry, written by [`Pool::init`] with `magic`
///   last (`Release`), read-only thereafter. Cold: per-op
///   paths use the handle's snapshot.
/// - line 1: `first_free_idx`, CAS-contended by every freer and
///   the allocator — sole owner of its line.
/// - Every field is atomic for the same reason as the ring
///   [`Header`](crate::Header): a peer may be mapped at any
///   time, and scribbles must be garbage values, never UB.
#[repr(C)]
pub struct PoolHeader {
    /// Layout marker ([`POOL_MAGIC`]); stored last by init
    /// (`Release`), loaded first by attach (`Acquire`).
    magic: AtomicU32,
    /// Pool layout version ([`POOL_LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Buffer size in bytes — a [`CACHE_LINE_SIZE`] multiple.
    buf_size: AtomicU32,
    /// Buffer count — nonzero, `< u32::MAX` ([`NIL`]).
    buf_count: AtomicU32,
    /// [`CACHE_LINE_SIZE`] this region was built with; attach
    /// validates it like the rest of the geometry.
    cache_line_size: AtomicU32,
    /// Free-stack head: index of the first free buffer, or
    /// [`NIL`] when the pool is exhausted.
    first_free_idx: CacheAligned<AtomicU32>,
}

const _: () = assert!(size_of::<PoolHeader>() == 2 * CACHE_LINE_SIZE);

/// A validated view over a pool region.
///
/// - Geometry is snapshotted out of the header at
///   init/attach: per-op paths never re-read fields a peer
///   could scribble on. Free-stack links live in shared
///   buffer memory and are validated at every pop instead.
pub struct Pool<'a> {
    /// The region's control block.
    header: &'a PoolHeader,
    /// Base of the buffer array
    /// (offset `size_of::<PoolHeader>()`).
    bufs: *mut u8,
    /// Snapshot of `header.buf_size`.
    buf_size: u32,
    /// Snapshot of `header.buf_count`.
    buf_count: u32,
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the allocator role; the shared
// state it touches (first_free_idx, next-links) is atomic,
// and buffer ownership is handed off with Release/Acquire
// ordering — same rationale as the ring endpoints.
unsafe impl Send for Pool<'_> {}

impl<'a> Pool<'a> {
    /// Initialize a fresh region and return the pool over it,
    /// with every buffer on the free-stack.
    ///
    /// - `buf_size` — bytes per buffer, a [`CACHE_LINE_SIZE`]
    ///   multiple.
    /// - `buf_count` — number of buffers, nonzero and
    ///   `< u32::MAX`.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at
    ///   least `size_of::<PoolHeader>() + buf_count *
    ///   buf_size` bytes.
    pub fn init(region: &'a mut [u8], buf_size: u32, buf_count: u32) -> Result<Self, Error> {
        validate_pool_geometry(buf_size, buf_count)?;
        let len = region.len();
        // Taken exactly once — same Stacked Borrows retag
        // hazard as Ring::init.
        let base = region.as_mut_ptr();
        let header = pool_header_ptr(base, len)?;
        if (len as u64) < pool_region_size(buf_size, buf_count) {
            return Err(Error::TooSmall);
        }
        // SAFETY: alignment + room for the PoolHeader checked
        // by pool_header_ptr; region is exclusively borrowed
        // for 'a; any byte pattern is a valid PoolHeader
        // (all-atomic fields, plain-byte padding).
        let header = unsafe { &*header };
        header
            .layout_version
            .store(POOL_LAYOUT_VERSION, Ordering::Relaxed);
        header.buf_size.store(buf_size, Ordering::Relaxed);
        header.buf_count.store(buf_count, Ordering::Relaxed);
        header
            .cache_line_size
            .store(CACHE_LINE_SIZE as u32, Ordering::Relaxed);
        // SAFETY: in bounds — len >= header + buffers.
        let bufs = unsafe { base.add(size_of::<PoolHeader>()) };
        let pool = Pool {
            header,
            bufs,
            buf_size,
            buf_count,
            _region: PhantomData,
        };
        // Link every buffer into the free-stack: i -> i + 1,
        // last -> NIL, head -> 0.
        for i in 0..buf_count {
            let next = if i + 1 == buf_count { NIL } else { i + 1 };
            pool.next_buf_idx(i).store(next, Ordering::Relaxed);
        }
        pool.header.first_free_idx.store(0, Ordering::Relaxed);
        // Published last: a peer that pre-mapped the region
        // must never observe the magic before the geometry it
        // validates (same handshake as Ring::init).
        pool.header.magic.store(POOL_MAGIC, Ordering::Release);
        Ok(pool)
    }

    /// Attach to a pool region another process (or an earlier
    /// call) already initialized, validating its header.
    ///
    /// # Safety
    ///
    /// - `region` points to `len` bytes of memory that outlive
    ///   `'a`, genuinely shared and writable (e.g. a
    ///   `MAP_SHARED` mapping).
    /// - At most one attached handle acts as the pool's
    ///   allocator (single-popper contract); any handle may
    ///   free.
    pub unsafe fn attach(region: *mut u8, len: usize) -> Result<Self, Error> {
        let header = pool_header_ptr(region, len)?;
        // SAFETY: alignment + room for the PoolHeader checked
        // by pool_header_ptr; caller guarantees the memory is
        // live and shared.
        let header = unsafe { &*header };
        // Acquire pairs with init's Release store of magic.
        if header.magic.load(Ordering::Acquire) != POOL_MAGIC {
            return Err(Error::BadMagic);
        }
        if header.layout_version.load(Ordering::Relaxed) != POOL_LAYOUT_VERSION {
            return Err(Error::BadLayoutVersion);
        }
        if header.cache_line_size.load(Ordering::Relaxed) != CACHE_LINE_SIZE as u32 {
            return Err(Error::BadCacheLine);
        }
        // Snapshot geometry once; per-op paths never re-read.
        let buf_size = header.buf_size.load(Ordering::Relaxed);
        let buf_count = header.buf_count.load(Ordering::Relaxed);
        validate_pool_geometry(buf_size, buf_count)?;
        if (len as u64) < pool_region_size(buf_size, buf_count) {
            return Err(Error::TooSmall);
        }
        // SAFETY: in bounds — len >= header + buffers.
        let bufs = unsafe { region.add(size_of::<PoolHeader>()) };
        Ok(Pool {
            header,
            bufs,
            buf_size,
            buf_count,
            _region: PhantomData,
        })
    }

    /// Pop the first free buffer off the free-stack as an
    /// owned [`BufSlot`], or [`Exhausted`].
    ///
    /// - `&mut self` is the in-process single-popper token;
    ///   cross-process, at most one attached handle allocates
    ///   (see [`Pool::attach`]). The guard does **not** borrow
    ///   the pool — any number of allocated buffers may be
    ///   live at once; per-buffer exclusivity comes from the
    ///   free-stack protocol (each pop yields a distinct
    ///   index), not the borrow checker.
    /// - Validated pop: the head and its next-link live in
    ///   peer-writable memory, so both are bounds-checked;
    ///   corruption degrades to `Exhausted` (mirroring the
    ///   ring's fail-toward-`Full` occupancy check), never to
    ///   an out-of-bounds buffer.
    /// - A racing free moves the head and the CAS retries;
    ///   with a single popper an in-flight head node cannot be
    ///   removed underneath us, so there is no ABA hazard.
    /// - `T` geometry is asserted on each call (a mismatch is
    ///   a programming error, so it panics — same doctrine as
    ///   `reserve_slot`). A zero-sized `T` is legal and still
    ///   consumes a whole buffer: the buffer is the
    ///   allocation granule.
    pub fn alloc<T>(&mut self) -> Result<BufSlot<'a, T>, Exhausted>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        check_type::<T>(self.buf_size);
        loop {
            // Acquire pairs with free's Release CAS: seeing a
            // head index means seeing that buffer's next-link
            // store (and the freer's last writes) too.
            let head = self.header.first_free_idx.load(Ordering::Acquire);
            if head == NIL {
                return Err(Exhausted);
            }
            if head >= self.buf_count {
                return Err(Exhausted);
            }
            let next = self.next_buf_idx(head).load(Ordering::Relaxed);
            if next != NIL && next >= self.buf_count {
                return Err(Exhausted);
            }
            if self
                .header
                .first_free_idx
                .compare_exchange(head, next, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(BufSlot {
                    header: self.header,
                    buf: self.buf_ptr(head),
                    idx: head,
                    _slot: PhantomData,
                });
            }
        }
    }

    /// [`alloc`](Pool::alloc) with an injected wait policy:
    /// retry until a buffer frees up or the policy gives up.
    ///
    /// - `wait` is called after each failed attempt with the
    ///   attempt count (0-based, saturating); returning
    ///   `false` gives up → `Err(Exhausted)`.
    /// - See [`policy`](crate::policy) for shipped policies
    ///   and the composition model.
    pub fn alloc_with<T>(
        &mut self,
        mut wait: impl FnMut(u32) -> bool,
    ) -> Result<BufSlot<'a, T>, Exhausted>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let mut attempt = 0u32;
        loop {
            match self.alloc::<T>() {
                Ok(buf_slot) => return Ok(buf_slot),
                Err(Exhausted) => {
                    if !wait(attempt) {
                        return Err(Exhausted);
                    }
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    /// [`alloc`](Pool::alloc), spinning until a buffer is
    /// available — [`alloc_with`](Pool::alloc_with) under the
    /// never-quitting [`policy::spin`](crate::policy::spin),
    /// so no `Result`.
    pub fn alloc_spin<T>(&mut self) -> BufSlot<'a, T>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        match self.alloc_with(crate::policy::spin) {
            Ok(buf_slot) => buf_slot,
            Err(Exhausted) => unreachable!("policy::spin never gives up"),
        }
    }

    /// Buffer size in bytes (geometry snapshot).
    pub fn buf_size(&self) -> u32 {
        self.buf_size
    }

    /// Buffer count (geometry snapshot).
    pub fn buf_count(&self) -> u32 {
        self.buf_count
    }

    /// A non-allocating [`PoolResolver`] view of this pool,
    /// for registration in a
    /// [`PoolRegistry`](crate::PoolRegistry).
    ///
    /// - Derived from this handle (same provenance as its own
    ///   pointers), so no second region borrow and no Stacked
    ///   Borrows retag hazard — `init`/`attach` take the
    ///   region pointer exactly once.
    /// - Any number of views may exist; none can allocate, so
    ///   the single-popper contract stays with this handle.
    pub fn resolver(&self) -> PoolResolver<'a> {
        PoolResolver {
            header: self.header,
            bufs: self.bufs,
            buf_size: self.buf_size,
            buf_count: self.buf_count,
        }
    }

    /// The next-free-buffer index cell of buffer `idx` — its
    /// first word, the intrusive free-stack link, meaningful
    /// only while the buffer is free.
    ///
    /// - Returns the atomic cell, not the value; symmetric
    ///   with the header's `first_free_idx`.
    /// - Callers pass `idx < buf_count` (validated pops /
    ///   init's linking loop), keeping the deref in bounds.
    fn next_buf_idx(&self, idx: u32) -> &AtomicU32 {
        let p = self.buf_ptr(idx) as *const AtomicU32;
        // SAFETY: idx < buf_count keeps the buffer in the
        // region validated at init/attach; the base is
        // cache-line aligned and buf_size is a line multiple,
        // so the first word is 4-aligned; all peers access it
        // as an atomic.
        unsafe { &*p }
    }

    /// Pointer to buffer `idx`; callers pass
    /// `idx < buf_count`.
    fn buf_ptr(&self, idx: u32) -> *mut u8 {
        // SAFETY: idx < buf_count, so the offset stays inside
        // the buffer array validated at init/attach.
        unsafe { self.bufs.add(idx as usize * self.buf_size as usize) }
    }
}

/// A non-allocating view over a pool: resolves validated
/// buffer indices to owned [`BufSlot`] guards on behalf of a
/// [`PoolRegistry`](crate::PoolRegistry).
///
/// - Created by [`Pool::resolver`]; carries the same header
///   ref, buffer base, and geometry snapshot as its pool.
/// - Cannot allocate — the single-popper token stays with the
///   owning [`Pool`] handle.
/// - Identifies its pool by header address (see
///   [`PoolRegistry::into_desc`](crate::PoolRegistry::into_desc)).
#[derive(Clone, Copy)]
pub struct PoolResolver<'a> {
    /// The pool's control block.
    header: &'a PoolHeader,
    /// Base of the buffer array (copy of [`Pool::bufs`]).
    bufs: *mut u8,
    /// Snapshot of `header.buf_size`.
    buf_size: u32,
    /// Snapshot of `header.buf_count`.
    buf_count: u32,
}

// SAFETY: the view is read-only over its own fields; the only
// shared-memory mutation reachable through it is the minted
// guards' free CAS, which is the free-stack's any-thread MPSC
// push side. Allocation (the single-popper role) is not
// reachable from a resolver.
unsafe impl Send for PoolResolver<'_> {}
// SAFETY: all methods take &self and touch shared state only
// through atomics (as above), so concurrent use from multiple
// threads adds no non-atomic shared access.
unsafe impl Sync for PoolResolver<'_> {}

impl<'a> PoolResolver<'a> {
    /// The pool's header address — its identity for registry
    /// lookups.
    pub(crate) fn header_ptr(&self) -> *const PoolHeader {
        self.header
    }

    /// Buffer count (geometry snapshot).
    pub(crate) fn buf_count(&self) -> u32 {
        self.buf_count
    }

    /// Buffer size in bytes (geometry snapshot).
    pub(crate) fn buf_size(&self) -> u32 {
        self.buf_size
    }

    /// Mint the owned guard for buffer `idx`.
    ///
    /// # Safety
    ///
    /// - The caller has validated `idx < buf_count` and `T`'s
    ///   geometry against this pool.
    /// - The caller holds the buffer's ownership (the index
    ///   came from a consumed guard via
    ///   [`PoolRegistry::into_desc`](crate::PoolRegistry::into_desc),
    ///   arrived with happens-before ordering, and is resolved
    ///   exactly once) — minting a second live guard for one
    ///   buffer aliases `&mut T`.
    pub(crate) unsafe fn slot_from_idx<T>(&self, idx: u32) -> BufSlot<'a, T>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        // SAFETY: idx < buf_count (caller contract), so the
        // offset stays inside the buffer array validated at
        // init/attach.
        let buf = unsafe { self.bufs.add(idx as usize * self.buf_size as usize) };
        BufSlot {
            header: self.header,
            buf,
            idx,
            _slot: PhantomData,
        }
    }
}

/// An allocated buffer, owned until [`free`](BufSlot::free):
/// `DerefMut` to use it as a `T` in place.
///
/// - Does not borrow the [`Pool`] — hold any number, as long
///   as you like; getting a buffer implies nothing about
///   when (or whether) it is sent or freed.
/// - Dropping without `free` leaks the buffer until the pool
///   is re-initialized (the abandon analog of the ring's
///   guards, with the opposite consequence).
pub struct BufSlot<'p, T> {
    /// The pool's control block (for the free CAS).
    header: &'p PoolHeader,
    /// Base of the owned buffer. Raw, and references are
    /// minted per access — same aliasing rationale as the
    /// ring guards.
    buf: *mut u8,
    /// This buffer's index (the value free pushes).
    idx: u32,
    /// The guard acts as a `&mut T` into the region.
    _slot: PhantomData<&'p mut T>,
}

// SAFETY: the guard owns its buffer exclusively (the pop
// removed it from every shared structure); free's CAS is the
// only shared-state touch and is properly ordered.
unsafe impl<T: Send> Send for BufSlot<'_, T> {}

impl<T> Deref for BufSlot<'_, T> {
    type Target = T;
    /// Read access to the buffer as a `T`.
    fn deref(&self) -> &T {
        // SAFETY: buf is in-bounds and cache-line aligned
        // (validated geometry), any byte pattern is a valid T
        // (FromBytes bound at alloc), and the pop gave this
        // guard sole ownership until free.
        unsafe { &*(self.buf as *const T) }
    }
}

impl<T> DerefMut for BufSlot<'_, T> {
    /// Write access to the buffer as a `T`.
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in deref; &mut self gives exclusivity of
        // the minted reference.
        unsafe { &mut *(self.buf as *mut T) }
    }
}

impl<T> BufSlot<'_, T> {
    /// The owning pool's header address — matched against a
    /// registry entry's [`PoolResolver::header_ptr`] by
    /// [`PoolRegistry::into_desc`](crate::PoolRegistry::into_desc).
    pub(crate) fn header_ptr(&self) -> *const PoolHeader {
        self.header
    }

    /// This buffer's index (the descriptor payload).
    pub(crate) fn idx(&self) -> u32 {
        self.idx
    }

    /// Push the buffer back onto the pool's free-stack.
    ///
    /// - Any holder may free — this is the free-stack's MPSC
    ///   push side; only allocation is single-popper.
    /// - The CAS retries only when another free (or the
    ///   allocator's pop) moves the head first.
    pub fn free(self) {
        // The buffer's first word becomes its next-link again.
        let link = self.buf as *const AtomicU32;
        // SAFETY: buf is in-bounds and 4-aligned (cache-line
        // aligned base); the guard still owns the buffer, and
        // all peers access this word as an atomic.
        let link = unsafe { &*link };
        loop {
            let head = self.header.first_free_idx.load(Ordering::Relaxed);
            link.store(head, Ordering::Relaxed);
            // Release pairs with alloc's Acquire loads: the
            // popper that sees idx also sees the link store
            // above (and this holder's last buffer writes).
            if self
                .header
                .first_free_idx
                .compare_exchange(head, self.idx, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }
}

/// Validate a region base pointer and cast it to the
/// `PoolHeader` it must start with; shared by
/// [`Pool::init`] / [`Pool::attach`].
///
/// - Checks alignment and room for the header itself; the
///   full-geometry length check stays with the caller (init
///   knows the geometry from parameters, attach only after
///   reading the header).
fn pool_header_ptr(base: *mut u8, len: usize) -> Result<*const PoolHeader, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<PoolHeader>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const PoolHeader)
}

/// Bytes needed for a pool region with the given geometry —
/// computed in u64 for the same 32-bit wrap reason as the
/// ring's `region_size`.
fn pool_region_size(buf_size: u32, buf_count: u32) -> u64 {
    size_of::<PoolHeader>() as u64 + buf_size as u64 * buf_count as u64
}

/// Shared geometry checks for [`Pool::init`] /
/// [`Pool::attach`].
///
/// - No power-of-two constraint: the free-stack never masks
///   an index.
fn validate_pool_geometry(buf_size: u32, buf_count: u32) -> Result<(), Error> {
    if buf_size == 0 || !(buf_size as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::BadBufSize);
    }
    if buf_count == 0 || buf_count == NIL {
        return Err(Error::BadBufCount);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message; two words so a torn write would be
    /// visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// Buffer size used by most pool tests: one cache line.
    const TEST_CACHE_LINE_SIZE: u32 = CACHE_LINE_SIZE as u32;

    /// Test region size: header + (4 buffers × 1 line each).
    const REGION_BYTES: usize = size_of::<PoolHeader>() + (4 * CACHE_LINE_SIZE);

    /// Cache-line-aligned backing store for the tests' pools.
    #[repr(C, align(64))]
    struct Region([u8; REGION_BYTES]);

    impl Region {
        fn new() -> Self {
            Region([0; REGION_BYTES])
        }
    }

    #[test]
    fn init_rejects_bad_geometry() {
        let mut r = Region::new();
        assert_eq!(
            Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE - 1, 4)
                .err()
                .unwrap(),
            Error::BadBufSize
        );
        assert_eq!(Pool::init(&mut r.0, 0, 4).err().unwrap(), Error::BadBufSize);
        assert_eq!(
            Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 0).err().unwrap(),
            Error::BadBufCount
        );
        assert_eq!(
            Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, u32::MAX)
                .err()
                .unwrap(),
            Error::BadBufCount
        );
        assert_eq!(
            Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 8).err().unwrap(),
            Error::TooSmall
        );
        assert_eq!(
            Pool::init(&mut r.0[1..], TEST_CACHE_LINE_SIZE, 4)
                .err()
                .unwrap(),
            Error::Misaligned
        );
        // 32-bit tripwire: size * count wraps a 32-bit usize
        // (2^26 * 2^6 = 2^32); u64 region math must reject it.
        assert_eq!(
            Pool::init(&mut r.0, 1 << 26, 1 << 6).err().unwrap(),
            Error::TooSmall
        );
    }

    #[test]
    fn attach_validates_header() {
        let mut r = Region::new();
        // Not initialized yet: magic is zero.
        let err = unsafe { Pool::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);
        // A ring region is not a pool region: its magic must
        // be rejected (the two kinds must never cross-attach).
        // 2 slots: the ring header (4 lines) + 2×64 fits the
        // pool-sized test region.
        crate::Ring::init(&mut r.0, TEST_CACHE_LINE_SIZE, 2).unwrap();
        let err = unsafe { Pool::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadMagic);

        let mut r = Region::new();
        Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        let pool = unsafe { Pool::attach(r.0.as_mut_ptr(), r.0.len()) }.unwrap();
        assert_eq!(pool.buf_size(), TEST_CACHE_LINE_SIZE);
        assert_eq!(pool.buf_count(), 4);
        // A region built with a different CACHE_LINE_SIZE
        // (simulated by editing the recorded value) is
        // rejected.
        pool.header.cache_line_size.store(128, Ordering::Relaxed);
        let err = unsafe { Pool::attach(r.0.as_mut_ptr(), r.0.len()) }
            .err()
            .unwrap();
        assert_eq!(err, Error::BadCacheLine);
    }

    #[test]
    fn alloc_holds_many_then_exhausts() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();

        // All four live at once — the decoupling: no guard
        // blocks the next alloc.
        let mut bufs = Vec::new();
        for i in 0..4u64 {
            let mut b = pool.alloc::<Msg>().unwrap();
            b.seq = i;
            b.val = i * 10;
            bufs.push(b);
        }
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);

        // Distinct buffers: every write is still intact.
        for (i, b) in bufs.iter().enumerate() {
            assert_eq!(
                **b,
                Msg {
                    seq: i as u64,
                    val: i as u64 * 10
                }
            );
        }
        for b in bufs {
            b.free();
        }
        // Fully recycled: four allocs succeed again.
        let a = pool.alloc::<Msg>().unwrap();
        let b = pool.alloc::<Msg>().unwrap();
        let c = pool.alloc::<Msg>().unwrap();
        let d = pool.alloc::<Msg>().unwrap();
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);
        a.free();
        b.free();
        c.free();
        d.free();
    }

    #[test]
    fn free_is_lifo() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();

        let a = pool.alloc::<Msg>().unwrap();
        let b = pool.alloc::<Msg>().unwrap();
        let a_ptr = &*a as *const Msg;
        let b_ptr = &*b as *const Msg;
        a.free();
        b.free(); // b freed last, so b is the stack top
        let first = pool.alloc::<Msg>().unwrap();
        let second = pool.alloc::<Msg>().unwrap();
        assert_eq!(&*first as *const Msg, b_ptr);
        assert_eq!(&*second as *const Msg, a_ptr);
        first.free();
        second.free();
    }

    #[test]
    // Dropping the guard is the behavior under test; BufSlot
    // has no Drop impl by design (drop = leak, documented).
    #[allow(clippy::drop_non_drop)]
    fn dropped_guard_leaks_its_buffer() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();

        drop(pool.alloc::<Msg>().unwrap()); // leaked, not freed
        // The pool now serves only three buffers.
        let a = pool.alloc::<Msg>().unwrap();
        let b = pool.alloc::<Msg>().unwrap();
        let c = pool.alloc::<Msg>().unwrap();
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);
        a.free();
        b.free();
        c.free();
    }

    #[test]
    fn corrupted_stack_fails_toward_exhausted() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();

        // Peer scribbles an out-of-bounds head (not NIL).
        pool.header.first_free_idx.store(1000, Ordering::Relaxed);
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);

        // Valid head, corrupted next-link: still refused —
        // the pop validates both before touching anything.
        pool.header.first_free_idx.store(0, Ordering::Relaxed);
        pool.next_buf_idx(0).store(77, Ordering::Relaxed);
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);

        // Repairing the link restores service — degraded,
        // never bricked.
        pool.next_buf_idx(0).store(NIL, Ordering::Relaxed);
        let a = pool.alloc::<Msg>().unwrap();
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);
        a.free();
    }

    #[test]
    fn wider_buffers_and_mixed_types() {
        // A two-line buffer pool: types of different sizes
        // (and a type filling the buffer exactly) share it,
        // and a freed buffer is reused as a different type.
        const BIG_BUF: usize = 2 * CACHE_LINE_SIZE;
        #[repr(C, align(64))]
        struct Region2([u8; size_of::<PoolHeader>() + 4 * BIG_BUF]);
        let mut r = Region2([0; size_of::<PoolHeader>() + 4 * BIG_BUF]);

        /// Fills the two-line buffer exactly.
        #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
        #[repr(C)]
        struct Big {
            words: [u64; BIG_BUF / 8],
        }

        let mut pool = Pool::init(&mut r.0, BIG_BUF as u32, 4).unwrap();

        // Different-sized types live in the pool at once.
        let mut small = pool.alloc::<Msg>().unwrap();
        let mut big = pool.alloc::<Big>().unwrap();
        small.seq = 7;
        big.words[0] = 1;
        big.words[BIG_BUF / 8 - 1] = 2; // last word: full span writable
        assert_eq!(small.seq, 7);
        assert_eq!(big.words[BIG_BUF / 8 - 1], 2);

        // A freed buffer is reused as a different type.
        let small_ptr = &*small as *const Msg as usize;
        small.free();
        let reused = pool.alloc::<Big>().unwrap();
        assert_eq!(&*reused as *const Big as usize, small_ptr);
        reused.free();
        big.free();
    }

    #[test]
    fn internally_aligned_types_work() {
        /// Alignment 32 (≤ CACHE_LINE_SIZE), no padding, so the
        /// zerocopy derives accept it; the buffer base is
        /// line-aligned, satisfying any align ≤ CACHE_LINE_SIZE.
        #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
        #[repr(C, align(32))]
        struct Aligned {
            words: [u64; 4],
        }
        assert_eq!(core::mem::align_of::<Aligned>(), 32);

        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        let mut a = pool.alloc::<Aligned>().unwrap();
        assert_eq!(&*a as *const Aligned as usize % 32, 0);
        a.words = [1, 2, 3, 4];
        assert_eq!(a.words[3], 4);
        a.free();
    }

    #[test]
    #[should_panic(expected = "T larger than slot")]
    fn too_big_type_panics() {
        /// Two lines: does not fit a one-line buffer.
        #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
        #[repr(C)]
        struct TooBig {
            words: [u64; 16],
        }
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        // A type that cannot fit is a programming error, not
        // a runtime condition: alloc panics (check_type).
        let _ = pool.alloc::<TooBig>();
    }

    #[test]
    fn zero_sized_type_consumes_a_buffer() {
        /// Zero bytes, alignment 1.
        #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
        #[repr(C)]
        struct Zst;

        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        // Legal, and each one occupies a whole buffer — the
        // buffer is the allocation granule.
        let a = pool.alloc::<Zst>().unwrap();
        let b = pool.alloc::<Zst>().unwrap();
        let c = pool.alloc::<Zst>().unwrap();
        let d = pool.alloc::<Zst>().unwrap();
        assert_eq!(pool.alloc::<Zst>().err().unwrap(), Exhausted);
        a.free();
        b.free();
        c.free();
        d.free();
    }

    #[test]
    fn threaded_alloc_here_free_there() {
        // Allocator thread allocs and hands guards to a freer
        // thread; recycling means total allocations far
        // exceed buf_count. Reduced under Miri (interpreted
        // spin loops are slow).
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();

        let (tx, rx) = std::sync::mpsc::channel::<BufSlot<'_, Msg>>();
        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    loop {
                        match pool.alloc::<Msg>() {
                            Ok(mut b) => {
                                b.seq = i;
                                b.val = i * 3;
                                tx.send(b).unwrap();
                                break;
                            }
                            Err(Exhausted) => std::hint::spin_loop(),
                        }
                    }
                }
            });
            s.spawn(move || {
                for i in 0..COUNT {
                    let b = rx.recv().unwrap();
                    assert_eq!(b.seq, i);
                    assert_eq!(b.val, i * 3);
                    b.free(); // the freer role: not the allocator
                }
            });
        });
    }

    #[test]
    fn alloc_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        // Drain the pool so alloc_with must wait.
        let held: [_; 4] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());

        // The policy sees 0-based attempt counts and its
        // `false` becomes Err(Exhausted).
        let mut seen = Vec::new();
        let err = pool
            .alloc_with::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err()
            .unwrap();
        assert_eq!(err, Exhausted);
        assert_eq!(seen, [0, 1, 2]);

        // A freed buffer ends the wait with Ok.
        let mut freed = false;
        let mut held = held.into_iter();
        let buf_slot = pool
            .alloc_with::<Msg>(|_| {
                if !freed {
                    held.next().unwrap().free();
                    freed = true;
                }
                true
            })
            .unwrap();
        buf_slot.free();
        held.for_each(BufSlot::free);
    }

    #[test]
    fn alloc_spin_returns_directly() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, TEST_CACHE_LINE_SIZE, 4).unwrap();
        let mut buf_slot = pool.alloc_spin::<Msg>();
        buf_slot.seq = 7;
        assert_eq!(buf_slot.seq, 7);
        buf_slot.free();
    }
}
