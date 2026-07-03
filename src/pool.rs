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
//!   header at all (pool-id provenance is deferred — see
//!   todo.md's In Progress block).
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

use crate::{CACHE_LINE, CacheAligned, Error, check_type};

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
    /// Buffer size in bytes — a [`CACHE_LINE`] multiple.
    buf_size: AtomicU32,
    /// Buffer count — nonzero, `< u32::MAX` ([`NIL`]).
    buf_count: AtomicU32,
    /// [`CACHE_LINE`] this region was built with; attach
    /// validates it like the rest of the geometry.
    cache_line_size: AtomicU32,
    /// Free-stack head: index of the first free buffer, or
    /// [`NIL`] when the pool is exhausted.
    first_free_idx: CacheAligned<AtomicU32>,
}

const _: () = assert!(size_of::<PoolHeader>() == 2 * CACHE_LINE);

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

impl<'a> Pool<'a> {
    /// Initialize a fresh region and return the pool over it,
    /// with every buffer on the free-stack.
    ///
    /// - `buf_size` — bytes per buffer, a [`CACHE_LINE`]
    ///   multiple.
    /// - `buf_count` — number of buffers, nonzero and
    ///   `< u32::MAX`.
    /// - The region must be [`CACHE_LINE`]-aligned and at
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
            .store(CACHE_LINE as u32, Ordering::Relaxed);
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
        if header.cache_line_size.load(Ordering::Relaxed) != CACHE_LINE as u32 {
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

    /// Buffer size in bytes (geometry snapshot).
    pub fn buf_size(&self) -> u32 {
        self.buf_size
    }

    /// Buffer count (geometry snapshot).
    pub fn buf_count(&self) -> u32 {
        self.buf_count
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
    if !(base as usize).is_multiple_of(CACHE_LINE) {
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
    if buf_size == 0 || !(buf_size as usize).is_multiple_of(CACHE_LINE) {
        return Err(Error::BadBufSize);
    }
    if buf_count == 0 || buf_count == NIL {
        return Err(Error::BadBufCount);
    }
    Ok(())
}
