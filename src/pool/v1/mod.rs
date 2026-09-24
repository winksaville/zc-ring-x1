//! Multi-stack message pool: v0's fixed-size buffers and intrusive free-stack, one stack per buffer
//! size, over one caller-provided region.
//!
//! - A stack is one buffer size with its own buffers, linked as v0's free-stack is. `N` stacks
//!   share one [`PoolHeader`] and one region, and v0 is the single-stack pool.
//! - The caller describes each stack with a [`StackGeometry`], in any order. How the stacks are
//!   laid out and searched is the pool's choice: today sorted smallest first and scanned.
//! - "Stack" is the pools' word and "segment" the rings': a ring segment is a pool buffer that a
//!   ring of segments runs through.
//! - The alloc family keeps v0's shape and picks the stack by size: the smallest stack that fits,
//!   then the next larger one when that stack is empty. Each such miss is counted against the
//!   stack that was wanted ([`Pool::stats`]), so a user can tell which size wants more buffers.
//! - At `N = 1` the pick is the one size comparison v0's `alloc` already makes, and the fallback
//!   loop is empty, so the single-stack pool does v0's work.
//! - A [`BufSlot`] frees to its own stack, so a free never searches.
//! - Roles as v0's: one owning allocator pops (`&mut self`), any holder frees.

use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use crate::registry::{DescMap, sealed};
use crate::{CACHE_LINE_SIZE, CacheAligned, Error, RegistryError, type_fits};

pub use super::v0::Exhausted;

/// Layout marker written by [`Pool::init`], distinct from v0's so neither pool attaches the
/// other's region.
const POOL_MAGIC: u32 = 0x5A43_5053; // "ZCPS"

/// Bumped on any change to the multi-stack pool region layout.
const POOL_LAYOUT_VERSION: u32 = 1;

/// Stack terminator: no next free buffer.
///
/// - Also why the total buffer count is `< u32::MAX`: every buffer index, across the stacks,
///   must be distinguishable from the sentinel.
const NIL: u32 = u32::MAX;

/// One stack's geometry: how big its buffers are and how many it holds, as [`Pool::init`] takes
/// it and [`Pool::stacks`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackGeometry {
    /// Bytes per buffer, a nonzero [`CACHE_LINE_SIZE`] multiple, and no two stacks of a pool
    /// share one.
    pub buf_size: u32,
    /// Buffers in the stack, nonzero, and the pool's total below `u32::MAX`.
    pub buf_count: u32,
}

impl StackGeometry {
    /// A stack of `buf_count` buffers of `buf_size` bytes each.
    pub const fn new(buf_size: u32, buf_count: u32) -> Self {
        StackGeometry {
            buf_size,
            buf_count,
        }
    }
}

/// One stack's allocation statistics, as [`Pool::stats`] reports them, labelled by the stack's
/// geometry rather than a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackStats {
    /// The stack these counts are for.
    pub geometry: StackGeometry,
    /// Allocations that wanted this stack and found it empty, since the handle was made. A count
    /// that keeps rising says the stack wants more buffers.
    pub misses: u64,
}

/// Control block at offset 0 of a multi-stack pool region: cold geometry, then one contended
/// head per stack.
///
/// - Geometry: written by [`Pool::init`] with `magic` last (`Release`), read-only thereafter.
///   Per-op paths use the handle's snapshot.
/// - `heads`: stack `c`'s head is `heads[c]`, each the sole owner of its cache line, so
///   frees to different stacks do not contend.
/// - Every field is atomic, as v0's: a peer may be mapped at any time, and scribbles must be
///   garbage values, never UB.
#[repr(C)]
pub struct PoolHeader<const N: usize> {
    /// Layout marker ([`POOL_MAGIC`]), stored last by init (`Release`), loaded first by attach
    /// (`Acquire`).
    magic: AtomicU32,
    /// Pool layout version ([`POOL_LAYOUT_VERSION`]).
    layout_version: AtomicU32,
    /// Number of stacks, `N`, validated by attach.
    stack_count: AtomicU32,
    /// [`CACHE_LINE_SIZE`] this region was built with.
    cache_line_size: AtomicU32,
    /// Buffer size of each stack in bytes, [`CACHE_LINE_SIZE`] multiples, strictly ascending.
    buf_sizes: [AtomicU32; N],
    /// Buffer count of each stack, nonzero.
    buf_counts: [AtomicU32; N],
    /// Stack heads: the stack-local index of each stack's first free buffer, or [`NIL`]
    /// when the stack is empty.
    heads: [CacheAligned<AtomicU32>; N],
}

const _: () = assert!(size_of::<PoolHeader<1>>() == 2 * CACHE_LINE_SIZE);

/// A validated view over a multi-stack pool region.
///
/// - Geometry is snapshotted out of the header at init/attach, as v0's. Stack links live in
///   shared buffer memory and are validated at every pop.
/// - `misses` belongs to the handle: allocation has one owner, so a plain count suffices.
pub struct Pool<'a, const N: usize> {
    /// The region's control block.
    header: &'a PoolHeader<N>,
    /// Base of each stack's buffer array.
    bases: [*mut u8; N],
    /// Snapshot of the header's geometry, sorted smallest first.
    stacks: [StackGeometry; N],
    /// Allocations whose wanted stack was empty, per stack.
    misses: [u64; N],
    _region: PhantomData<&'a [u8]>,
}

// SAFETY: the handle owns the allocator role. The shared state it touches (the heads, the
// next-links) is atomic, and buffer ownership is handed off with Release/Acquire ordering, as v0's.
unsafe impl<const N: usize> Send for Pool<'_, N> {}

impl<'a, const N: usize> Pool<'a, N> {
    /// Initialize a fresh region and return the pool over it, with every buffer on its stack.
    ///
    /// - `stacks`: one [`StackGeometry`] per stack, in any order.
    ///   - `buf_size`: bytes per buffer, a nonzero [`CACHE_LINE_SIZE`] multiple. Two stacks of
    ///     one size are refused as [`Error::BadBufSize`].
    ///   - `buf_count`: buffers in the stack, nonzero, and the total below `u32::MAX`, else
    ///     [`Error::BadBufCount`].
    /// - The pool orders the stacks itself, today smallest first.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at least [`region_size`] bytes.
    pub fn init(region: &'a mut [u8], stacks: [StackGeometry; N]) -> Result<Self, Error> {
        let mut stacks = stacks;
        stacks.sort_unstable_by_key(|stack| stack.buf_size);
        validate_stacks(&stacks)?;
        let len = region.len();
        // Taken exactly once, same Stacked Borrows retag hazard as v0's init.
        let base = region.as_mut_ptr();
        let header = header_ptr::<N>(base, len)?;
        if (len as u64) < region_size(stacks) {
            return Err(Error::TooSmall);
        }
        // SAFETY: alignment and room for the header checked by header_ptr, the region is
        // exclusively borrowed for 'a, and any byte pattern is a valid PoolHeader (all-atomic
        // fields, plain-byte padding).
        let header = unsafe { &*header };
        header
            .layout_version
            .store(POOL_LAYOUT_VERSION, Ordering::Relaxed);
        header.stack_count.store(N as u32, Ordering::Relaxed);
        header
            .cache_line_size
            .store(CACHE_LINE_SIZE as u32, Ordering::Relaxed);
        for (c, stack) in stacks.iter().enumerate() {
            header.buf_sizes[c].store(stack.buf_size, Ordering::Relaxed);
            header.buf_counts[c].store(stack.buf_count, Ordering::Relaxed);
        }
        let pool = Pool {
            header,
            bases: stack_bases(base, &stacks),
            stacks,
            misses: [0; N],
            _region: PhantomData,
        };
        // Link each stack's buffers: i -> i + 1, last -> NIL, head -> 0.
        for (c, stack) in stacks.iter().enumerate() {
            let count = stack.buf_count;
            for i in 0..count {
                let next = if i + 1 == count { NIL } else { i + 1 };
                pool.next_buf_idx(c, i).store(next, Ordering::Relaxed);
            }
            pool.header.heads[c].store(0, Ordering::Relaxed);
        }
        // Published last: a peer that pre-mapped the region must never observe the magic before
        // the geometry it validates.
        pool.header.magic.store(POOL_MAGIC, Ordering::Release);
        Ok(pool)
    }

    /// Attach to a multi-stack pool region another process (or an earlier call) already
    /// initialized, validating its header against `N`.
    ///
    /// # Safety
    ///
    /// - `region` points to `len` bytes of memory that outlive `'a`, genuinely shared and
    ///   writable (e.g. a `MAP_SHARED` mapping).
    /// - At most one attached handle acts as the pool's allocator (single-popper contract). Any
    ///   handle may free.
    pub unsafe fn attach(region: *mut u8, len: usize) -> Result<Self, Error> {
        let header = header_ptr::<N>(region, len)?;
        // SAFETY: alignment and room for the header checked by header_ptr. The caller guarantees
        // the memory is live and shared.
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
        if header.stack_count.load(Ordering::Relaxed) != N as u32 {
            return Err(Error::BadStackCount);
        }
        // Snapshot geometry once. Per-op paths never re-read. The region's stacks must already
        // be in the pool's order, since init wrote them sorted.
        let stacks: [StackGeometry; N] = core::array::from_fn(|c| {
            StackGeometry::new(
                header.buf_sizes[c].load(Ordering::Relaxed),
                header.buf_counts[c].load(Ordering::Relaxed),
            )
        });
        validate_stacks(&stacks)?;
        if (len as u64) < region_size(stacks) {
            return Err(Error::TooSmall);
        }
        Ok(Pool {
            header,
            bases: stack_bases(region, &stacks),
            stacks,
            misses: [0; N],
            _region: PhantomData,
        })
    }

    /// Take a buffer for a `T` from the smallest stack that fits, or a larger one when that
    /// stack is empty, as an owned [`BufSlot`], or [`Exhausted`].
    ///
    /// - Roles, the validated pop, and the guard's independence from the pool are v0's
    ///   [`alloc`](super::v0::Pool::alloc)'s.
    /// - An empty wanted stack counts one miss against it ([`stats`](Pool::stats)), whether a
    ///   larger stack then serves the call or none does.
    /// - A `T` larger than the largest stack, or aligned beyond a cache line, is a programming
    ///   error and panics, as in v0.
    pub fn alloc<T>(&mut self) -> Result<BufSlot<'a, T>, Exhausted>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        assert!(
            core::mem::align_of::<T>() <= CACHE_LINE_SIZE,
            "T alignment exceeds slot alignment"
        );
        self.alloc_sized(size_of::<T>())
    }

    /// Take a buffer of at least `size` bytes, from the smallest stack that fits or a larger one
    /// when that stack is empty, as an owned [`BufSlot`] over all of its bytes, or [`Exhausted`].
    ///
    /// - The guard derefs to the stack's whole buffer, so its length is the size actually given,
    ///   at least `size`.
    /// - For a layout sized at runtime that no `T` describes. Typed views over the bytes are
    ///   zero-copy casts.
    /// - A `size` larger than the largest stack is a programming error and panics.
    pub fn alloc_bytes(&mut self, size: usize) -> Result<BufSlot<'a, [u8]>, Exhausted> {
        self.alloc_sized(size)
    }

    /// [`alloc`](Pool::alloc) with an injected wait policy: retry until a buffer frees up or the
    /// policy gives up.
    ///
    /// - `on_exhausted` is called after each failed attempt with the attempt count (0-based,
    ///   saturating), and returning `false` gives up with `Err(Exhausted)`.
    /// - Every failed attempt is a miss against the wanted stack.
    pub fn alloc_with<T>(
        &mut self,
        mut on_exhausted: impl FnMut(u32) -> bool,
    ) -> Result<BufSlot<'a, T>, Exhausted>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let mut attempt = 0u32;
        loop {
            match self.alloc::<T>() {
                Ok(buf_slot) => return Ok(buf_slot),
                Err(Exhausted) => {
                    if !on_exhausted(attempt) {
                        return Err(Exhausted);
                    }
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    /// The pool's stacks (geometry snapshot), each labelled by its own geometry. Their order is
    /// the pool's, so a caller finds a stack by its size, never by a position.
    pub fn stacks(&self) -> [StackGeometry; N] {
        self.stacks
    }

    /// A [`PoolView`] of this pool, a view that cannot pop, for registration in a
    /// [`PoolRegistry`](crate::PoolRegistry).
    ///
    /// - Derived from this handle, as v0's, so no second region borrow.
    /// - Numbers the buffers across the stacks for descriptors: stack `s`'s buffers follow the
    ///   buffers of every smaller stack.
    pub fn view(&self) -> PoolView<'a, N> {
        let mut start = 0u32;
        let starts = core::array::from_fn(|s| {
            let first = start;
            // Cannot wrap: init/attach bound the total count below NIL.
            start += self.stacks[s].buf_count;
            first
        });
        PoolView {
            header: self.header,
            bases: self.bases,
            stacks: self.stacks,
            starts,
        }
    }

    /// Each stack's allocation statistics, labelled by its geometry: the allocations that
    /// wanted the stack and found it empty, since this handle was made.
    ///
    /// - A count that keeps rising says the stack wants more buffers.
    pub fn stats(&self) -> [StackStats; N] {
        core::array::from_fn(|s| StackStats {
            geometry: self.stacks[s],
            misses: self.misses[s],
        })
    }

    /// The alloc family's shared body: pick the wanted stack for `size`, pop from it, and on an
    /// empty stack count the miss and try each larger stack in turn.
    ///
    /// - At `N = 1` the fallback range is empty and compiles away.
    fn alloc_sized<T: ?Sized>(&mut self, size: usize) -> Result<BufSlot<'a, T>, Exhausted> {
        let wanted = self.stack_for(size);
        if let Ok(buf_slot) = self.pop(wanted) {
            return Ok(buf_slot);
        }
        self.misses[wanted] += 1;
        for stack in wanted + 1..N {
            if let Ok(buf_slot) = self.pop(stack) {
                return Ok(buf_slot);
            }
        }
        Err(Exhausted)
    }

    /// The smallest stack whose buffers hold `size` bytes.
    ///
    /// - Panics when none does: the caller asked for a size the pool was not built for, a
    ///   programming error, as v0's `check_type`.
    fn stack_for(&self, size: usize) -> usize {
        for stack in 0..N {
            if size <= self.stacks[stack].buf_size as usize {
                return stack;
            }
        }
        panic!("size larger than the largest buffer");
    }

    /// The validated pop of stack `stack`, minting a guard of the caller's view `T`.
    ///
    /// - As v0's pop: the head and its next-link live in peer-writable memory, so both are
    ///   bounds-checked, and corruption degrades to `Exhausted` for the stack, never to an
    ///   out-of-bounds buffer.
    /// - A racing free moves the head and the CAS retries. With a single popper there is no ABA
    ///   hazard.
    fn pop<T: ?Sized>(&self, stack: usize) -> Result<BufSlot<'a, T>, Exhausted> {
        let head_cell: &'a AtomicU32 = &self.header.heads[stack];
        let count = self.stacks[stack].buf_count;
        loop {
            // Acquire pairs with free's Release CAS: seeing a head index means seeing that
            // buffer's next-link store (and the freer's last writes) too.
            let head = head_cell.load(Ordering::Acquire);
            if head == NIL || head >= count {
                return Err(Exhausted);
            }
            let next = self.next_buf_idx(stack, head).load(Ordering::Relaxed);
            if next != NIL && next >= count {
                return Err(Exhausted);
            }
            if head_cell
                .compare_exchange(head, next, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(BufSlot {
                    head: head_cell,
                    buf: self.buf_ptr(stack, head),
                    buf_size: self.stacks[stack].buf_size,
                    idx: head,
                    _slot: PhantomData,
                });
            }
        }
    }

    /// The next-free-buffer index cell of buffer `idx` of stack `stack`, its first word, the
    /// intrusive stack link, meaningful only while the buffer is free.
    ///
    /// - Callers pass `idx < stacks[stack].buf_count` (validated pops, init's linking loop).
    fn next_buf_idx(&self, stack: usize, idx: u32) -> &AtomicU32 {
        let p = self.buf_ptr(stack, idx) as *const AtomicU32;
        // SAFETY: idx < the stack's count keeps the buffer in the region validated at
        // init/attach. The base is cache-line aligned and sizes are line multiples, so the first
        // word is 4-aligned. All peers access it as an atomic.
        unsafe { &*p }
    }

    /// Pointer to buffer `idx` of stack `stack`, and callers pass `idx < stacks[stack].buf_count`.
    fn buf_ptr(&self, stack: usize, idx: u32) -> *mut u8 {
        // SAFETY: idx < the stack's count, so the offset stays inside the stack's buffer array
        // validated at init/attach.
        unsafe { self.bases[stack].add(idx as usize * self.stacks[stack].buf_size as usize) }
    }
}

/// A view over a multi-stack pool that cannot pop: maps guards to descriptor indices and validated
/// indices back to owned [`BufSlot`] guards on behalf of a [`PoolRegistry`](crate::PoolRegistry).
///
/// - Created by [`Pool::view`], with the same header ref, bases, and geometry snapshot.
/// - A descriptor index numbers the buffers across the stacks, smallest stack first, so it is
///   the stack's first index, `starts[s]`, plus the stack-local index.
/// - Cannot pop: taking buffers stays with the owning [`Pool`] handle, the single popper.
#[derive(Clone, Copy)]
pub struct PoolView<'a, const N: usize> {
    /// The pool's control block.
    header: &'a PoolHeader<N>,
    /// Base of each stack's buffer array.
    bases: [*mut u8; N],
    /// The pool's geometry snapshot, sorted smallest first.
    stacks: [StackGeometry; N],
    /// Descriptor index of each stack's first buffer.
    starts: [u32; N],
}

// SAFETY: the view is read-only over its own fields. The only shared-memory mutation reachable
// through it is the minted guards' free CAS, the stacks' any-thread push side. Allocation is not
// reachable from a view.
//
// Send and Sync are marker traits with no methods, so each impl is empty: the `unsafe impl` is
// the whole statement, a promise the compiler cannot infer past the raw pointers.
unsafe impl<const N: usize> Send for PoolView<'_, N> {}
// SAFETY: all methods take &self and touch shared state only through atomics, as above.
unsafe impl<const N: usize> Sync for PoolView<'_, N> {}

// Empty on purpose: `Sealed` has no methods, and this impl is the crate opting v1's view into
// `DescMap`, which code outside the crate cannot do.
impl<const N: usize> sealed::Sealed for PoolView<'_, N> {}

impl<'a, const N: usize> DescMap<'a> for PoolView<'a, N> {
    type Slot<T: ?Sized + 'a> = BufSlot<'a, T>;

    /// The guard's descriptor index when its head is one of this pool's stack heads: one
    /// comparison per stack, one at `N = 1`.
    fn desc_idx<T: ?Sized + 'a>(&self, slot: &BufSlot<'a, T>) -> Option<u32> {
        for s in 0..N {
            if core::ptr::eq(slot.head, &*self.header.heads[s]) {
                return Some(self.starts[s] + slot.idx);
            }
        }
        None
    }

    /// Find the stack whose index range holds `idx`, check `T` against that stack's size, and
    /// mint the guard.
    unsafe fn to_slot<T>(&self, idx: u32) -> Result<BufSlot<'a, T>, RegistryError>
    where
        T: FromBytes + IntoBytes + KnownLayout + 'a,
    {
        // The stacks' index ranges are contiguous and ascending, so the first range ending past
        // idx holds it. No end wraps: the total count is below NIL.
        for s in 0..N {
            if idx < self.starts[s] + self.stacks[s].buf_count {
                let local = idx - self.starts[s];
                if !type_fits::<T>(self.stacks[s].buf_size) {
                    return Err(RegistryError::BadType);
                }
                // SAFETY: local < the stack's count keeps the buffer inside the stack's array
                // validated at init/attach.
                let buf =
                    unsafe { self.bases[s].add(local as usize * self.stacks[s].buf_size as usize) };
                return Ok(BufSlot {
                    head: &self.header.heads[s],
                    buf,
                    buf_size: self.stacks[s].buf_size,
                    idx: local,
                    _slot: PhantomData,
                });
            }
        }
        Err(RegistryError::BadIndex)
    }
}

/// An allocated buffer, owned until [`free`](BufSlot::free): `DerefMut` to use it as a `T` in
/// place, or as all of its bytes for `BufSlot<[u8]>`.
///
/// - Does not borrow the [`Pool`], as v0's guard.
/// - Carries its stack's head, so free goes straight to the right stack, and the head names the
///   stack for [`PoolView`]'s descriptor index.
/// - Dropping without `free` leaks the buffer until the pool is re-initialized.
pub struct BufSlot<'p, T: ?Sized> {
    /// The owning stack's head (for the free CAS).
    head: &'p AtomicU32,
    /// Base of the owned buffer. Raw, and references are minted per access, same aliasing
    /// rationale as the ring guards.
    buf: *mut u8,
    /// The stack's buffer size, the length a `BufSlot<[u8]>` derefs to.
    buf_size: u32,
    /// This buffer's stack-local index (the value free pushes).
    idx: u32,
    /// The guard acts as a `&mut T` into the region.
    _slot: PhantomData<&'p mut T>,
}

// SAFETY: the guard owns its buffer exclusively (the pop removed it from every shared structure).
// Free's CAS is the only shared-state touch and is properly ordered.
unsafe impl<T: ?Sized + Send> Send for BufSlot<'_, T> {}

impl<T> Deref for BufSlot<'_, T> {
    type Target = T;
    /// Read access to the buffer as a `T`.
    fn deref(&self) -> &T {
        // SAFETY: buf is in-bounds and cache-line aligned (validated geometry), T fits (alloc
        // picked a stack whose buffers hold size_of::<T>() bytes), any byte pattern is a valid T
        // (FromBytes bound at alloc), and the pop gave this guard sole ownership until free.
        unsafe { &*(self.buf as *const T) }
    }
}

impl<T> DerefMut for BufSlot<'_, T> {
    /// Write access to the buffer as a `T`.
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in deref, and &mut self gives exclusivity of the minted reference.
        unsafe { &mut *(self.buf as *mut T) }
    }
}

impl Deref for BufSlot<'_, [u8]> {
    type Target = [u8];
    /// Read access to the whole buffer as bytes.
    fn deref(&self) -> &[u8] {
        // SAFETY: buf is the base of a buffer of buf_size bytes inside the validated region, any
        // byte pattern is a valid u8, and the pop gave this guard sole ownership until free.
        unsafe { core::slice::from_raw_parts(self.buf, self.buf_size as usize) }
    }
}

impl DerefMut for BufSlot<'_, [u8]> {
    /// Write access to the whole buffer as bytes.
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in deref, and &mut self gives exclusivity of the minted slice.
        unsafe { core::slice::from_raw_parts_mut(self.buf, self.buf_size as usize) }
    }
}

impl<T: ?Sized> BufSlot<'_, T> {
    /// The size of the buffer given, in bytes: at least the size asked for, and the size of the
    /// stack that served it. The buffer starts on a cache line.
    pub fn buf_size(&self) -> usize {
        self.buf_size as usize
    }

    /// Push the buffer back onto its stack.
    ///
    /// - Any holder may free: this is the stack's MPSC push side. Only allocation is
    ///   single-popper.
    /// - The CAS retries only when another free (or the allocator's pop) moves the head first.
    pub fn free(self) {
        // The buffer's first word becomes its next-link again.
        let link = self.buf as *const AtomicU32;
        // SAFETY: buf is in-bounds and 4-aligned (cache-line aligned base). The guard still owns
        // the buffer, and all peers access this word as an atomic.
        let link = unsafe { &*link };
        loop {
            let head = self.head.load(Ordering::Relaxed);
            link.store(head, Ordering::Relaxed);
            // Release pairs with alloc's Acquire loads: the popper that sees idx also sees the
            // link store above (and this holder's last buffer writes).
            if self
                .head
                .compare_exchange(head, self.idx, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }
}

/// Bytes needed for a multi-stack pool region with the given stacks, in any order, computed in
/// u64 so a 32-bit target cannot wrap.
pub fn region_size<const N: usize>(stacks: [StackGeometry; N]) -> u64 {
    let mut bytes = size_of::<PoolHeader<N>>() as u64;
    for stack in stacks {
        bytes += stack.buf_size as u64 * stack.buf_count as u64;
    }
    bytes
}

/// The base of each stack's buffer array: the stacks follow the header in order, each
/// `buf_size * buf_count` bytes.
///
/// - Callers have validated the region's length against [`region_size`], so every base is in
///   bounds.
fn stack_bases<const N: usize>(base: *mut u8, stacks: &[StackGeometry; N]) -> [*mut u8; N] {
    let mut offset = size_of::<PoolHeader<N>>();
    core::array::from_fn(|c| {
        // SAFETY: offset is at most the region size validated by the caller.
        let p = unsafe { base.add(offset) };
        offset += stacks[c].buf_size as usize * stacks[c].buf_count as usize;
        p
    })
}

/// Validate a region base pointer and cast it to the `PoolHeader` it must start with, shared by
/// [`Pool::init`] and [`Pool::attach`].
///
/// - Checks alignment and room for the header itself. The full-geometry length check stays with
///   the caller.
fn header_ptr<const N: usize>(base: *mut u8, len: usize) -> Result<*const PoolHeader<N>, Error> {
    if !(base as usize).is_multiple_of(CACHE_LINE_SIZE) {
        return Err(Error::Misaligned);
    }
    if len < size_of::<PoolHeader<N>>() {
        return Err(Error::TooSmall);
    }
    Ok(base as *const PoolHeader<N>)
}

/// Shared stack checks for [`Pool::init`], after its sort, and [`Pool::attach`].
///
/// - Sizes: nonzero [`CACHE_LINE_SIZE`] multiples, strictly ascending, so the first stack that
///   fits is the smallest. After init's sort, only two stacks of one size fail the order.
/// - Counts: each nonzero, the total below [`NIL`], so a buffer index across the stacks never
///   reads as the sentinel.
/// - `N = 0` is refused as a bad count: a pool with no stack serves nothing.
fn validate_stacks<const N: usize>(stacks: &[StackGeometry; N]) -> Result<(), Error> {
    if N == 0 {
        return Err(Error::BadBufCount);
    }
    let mut total = 0u64;
    for c in 0..N {
        let size = stacks[c].buf_size;
        if size == 0 || !(size as usize).is_multiple_of(CACHE_LINE_SIZE) {
            return Err(Error::BadBufSize);
        }
        if c > 0 && size <= stacks[c - 1].buf_size {
            return Err(Error::BadBufSize);
        }
        if stacks[c].buf_count == 0 {
            return Err(Error::BadBufCount);
        }
        total += stacks[c].buf_count as u64;
    }
    if total >= NIL as u64 {
        return Err(Error::BadBufCount);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    /// Test message, two words so a torn write would be visible.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
        val: u64,
    }

    /// 128 bytes: too big for a 64-byte stack.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    #[repr(C)]
    struct TwoLines {
        words: [u64; 2 * CACHE_LINE_SIZE / 8],
    }

    /// One cache line, the smallest buffer size.
    const LINE: u32 = CACHE_LINE_SIZE as u32;

    /// The geometry most tests use, one pool of three stacks: two 64-byte buffers, two
    /// 128-byte, and two 256-byte, with a 64-byte cache line.
    const THREE_STACKS: [StackGeometry; 3] = [geom(LINE, 2), geom(2 * LINE, 2), geom(4 * LINE, 2)];

    /// A stack of `buf_count` buffers of `buf_size` bytes, short for the tests' tables.
    const fn geom(buf_size: u32, buf_count: u32) -> StackGeometry {
        StackGeometry::new(buf_size, buf_count)
    }

    /// Cache-line-aligned backing store, big enough for the tests' pools.
    #[repr(C, align(64))]
    struct Region([u8; 32 * CACHE_LINE_SIZE]);

    impl Region {
        /// A zeroed region.
        fn new() -> Self {
            Region([0; 32 * CACHE_LINE_SIZE])
        }
    }

    #[test]
    fn header_sizes() {
        assert_eq!(size_of::<PoolHeader<1>>(), 2 * CACHE_LINE_SIZE);
        assert_eq!(size_of::<PoolHeader<3>>(), 4 * CACHE_LINE_SIZE);
        assert_eq!(
            region_size(THREE_STACKS),
            (4 + 2 + 4 + 8) as u64 * LINE as u64
        );
    }

    #[test]
    fn init_rejects_bad_stacks() {
        let mut r = Region::new();
        let err = |r: &mut Region, stacks| Pool::<3>::init(&mut r.0, stacks).err();
        // Two stacks of one size, in any order.
        assert_eq!(
            err(&mut r, [geom(LINE, 2), geom(LINE, 2), geom(4 * LINE, 2)]),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(
                &mut r,
                [geom(4 * LINE, 2), geom(LINE, 2), geom(4 * LINE, 1)]
            ),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(
                &mut r,
                [geom(LINE - 1, 2), geom(2 * LINE, 2), geom(4 * LINE, 2)]
            ),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(
                &mut r,
                [geom(LINE, 2), geom(2 * LINE, 0), geom(4 * LINE, 2)]
            ),
            Some(Error::BadBufCount)
        );
        assert_eq!(
            err(
                &mut r,
                [
                    geom(LINE, u32::MAX / 2),
                    geom(2 * LINE, u32::MAX / 2),
                    geom(4 * LINE, 1)
                ]
            ),
            Some(Error::BadBufCount)
        );
        assert_eq!(
            err(
                &mut r,
                [geom(LINE, 2), geom(2 * LINE, 2), geom(4 * LINE, 8)]
            ),
            Some(Error::TooSmall)
        );
        assert_eq!(
            Pool::<3>::init(&mut r.0[1..], THREE_STACKS).err(),
            Some(Error::Misaligned)
        );
        assert_eq!(
            Pool::<0>::init(&mut r.0, []).err(),
            Some(Error::BadBufCount)
        );
    }

    #[test]
    fn attach_validates_header() {
        let mut r = Region::new();
        let err = unsafe { Pool::<3>::attach(r.0.as_mut_ptr(), r.0.len()) }.err();
        assert_eq!(err, Some(Error::BadMagic));
        // A v0 pool region is not a v1 one.
        crate::pool::v0::Pool::init(&mut r.0, LINE, 4).unwrap();
        let err = unsafe { Pool::<1>::attach(r.0.as_mut_ptr(), r.0.len()) }.err();
        assert_eq!(err, Some(Error::BadMagic));

        let mut r = Region::new();
        Pool::init(&mut r.0, THREE_STACKS).unwrap();
        // One pointer for every attach, so no later retag invalidates the attached handle.
        let (base, len) = (r.0.as_mut_ptr(), r.0.len());
        let pool = unsafe { Pool::<3>::attach(base, len) }.unwrap();
        assert_eq!(pool.stacks(), THREE_STACKS);
        // Another stack count is another layout.
        let err = unsafe { Pool::<2>::attach(base, len) }.err();
        assert_eq!(err, Some(Error::BadStackCount));
        pool.header.cache_line_size.store(128, Ordering::Relaxed);
        let err = unsafe { Pool::<3>::attach(base, len) }.err();
        assert_eq!(err, Some(Error::BadCacheLine));
    }

    #[test]
    fn alloc_picks_the_smallest_stack_that_fits() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let small = pool.alloc::<Msg>().unwrap();
        let mid = pool.alloc::<TwoLines>().unwrap();
        let bytes = pool.alloc_bytes(3 * CACHE_LINE_SIZE).unwrap();
        assert_eq!(small.buf_size, LINE);
        assert_eq!(mid.buf_size, 2 * LINE);
        // The size actually given, at least the size asked for.
        assert_eq!(bytes.len(), 4 * CACHE_LINE_SIZE);
        assert_eq!(bytes.as_ptr() as usize % CACHE_LINE_SIZE, 0);
        assert_eq!(pool.misses, [0, 0, 0]);
        small.free();
        mid.free();
        bytes.free();
    }

    #[test]
    fn empty_stack_falls_back_and_counts_misses() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        // Two 64-byte buffers, then the 128-byte stack serves, then the 256-byte stack.
        let bufs: [_; 6] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        let sizes = bufs.each_ref().map(|b| b.buf_size);
        assert_eq!(sizes, [LINE, LINE, 2 * LINE, 2 * LINE, 4 * LINE, 4 * LINE]);
        assert_eq!(pool.misses, [4, 0, 0]);
        // Every stack that fits is empty.
        assert_eq!(pool.alloc::<Msg>().err(), Some(Exhausted));
        assert_eq!(pool.misses, [5, 0, 0]);
        // A free goes back to its own stack: the 64-byte stack serves again, no miss.
        let [a, b, c, d, e, f] = bufs;
        a.free();
        let again = pool.alloc::<Msg>().unwrap();
        assert_eq!(again.buf_size, LINE);
        assert_eq!(pool.misses, [5, 0, 0]);
        // A larger size never falls back to a smaller stack.
        c.free();
        let two = pool.alloc::<TwoLines>().unwrap();
        assert_eq!(two.buf_size, 2 * LINE);
        assert_eq!(pool.alloc::<TwoLines>().err(), Some(Exhausted));
        assert_eq!(pool.misses, [5, 1, 0]);
        for buf in [again, b, d, e, f] {
            buf.free();
        }
        two.free();
    }

    #[test]
    fn free_is_lifo_per_stack() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let a = pool.alloc::<Msg>().unwrap();
        let b = pool.alloc::<Msg>().unwrap();
        let x = pool.alloc::<TwoLines>().unwrap();
        let (a_ptr, b_ptr) = (&*a as *const Msg, &*b as *const Msg);
        a.free();
        x.free();
        b.free(); // b freed last in its stack, so b is its stack's top
        let first = pool.alloc::<Msg>().unwrap();
        let second = pool.alloc::<Msg>().unwrap();
        assert_eq!(&*first as *const Msg, b_ptr);
        assert_eq!(&*second as *const Msg, a_ptr);
        first.free();
        second.free();
    }

    #[test]
    fn stacks_do_not_overlap() {
        let mut r = Region::new();
        let end = r.0.as_ptr() as usize + region_size(THREE_STACKS) as usize;
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut all: [_; 6] = [
            pool.alloc_bytes(1).unwrap(),
            pool.alloc_bytes(1).unwrap(),
            pool.alloc_bytes(2 * CACHE_LINE_SIZE).unwrap(),
            pool.alloc_bytes(2 * CACHE_LINE_SIZE).unwrap(),
            pool.alloc_bytes(4 * CACHE_LINE_SIZE).unwrap(),
            pool.alloc_bytes(4 * CACHE_LINE_SIZE).unwrap(),
        ];
        for (i, buf) in all.iter_mut().enumerate() {
            buf.fill(i as u8);
        }
        for (i, buf) in all.iter().enumerate() {
            assert!(buf.iter().all(|&byte| byte == i as u8));
        }
        assert_eq!(all[5].as_ptr() as usize + all[5].len(), end);
        all.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn corrupted_stack_falls_back() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        // A peer scribbles the 64-byte stack's head: that stack reads as empty, and the next
        // stack serves, never an out-of-bounds buffer.
        pool.header.heads[0].store(1000, Ordering::Relaxed);
        let buf = pool.alloc::<Msg>().unwrap();
        assert_eq!(buf.buf_size, 2 * LINE);
        assert_eq!(pool.misses, [1, 0, 0]);
        buf.free();
    }

    #[test]
    fn single_stack_is_v0_shaped() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [geom(LINE, 4)]).unwrap();
        let bufs: [_; 4] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        assert_eq!(pool.alloc::<Msg>().err(), Some(Exhausted));
        assert_eq!(pool.misses, [1]);
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn init_orders_the_stacks_itself() {
        // The same three stacks, given largest first.
        let mut r = Region::new();
        let reversed = [geom(4 * LINE, 2), geom(2 * LINE, 2), geom(LINE, 2)];
        let mut pool = Pool::init(&mut r.0, reversed).unwrap();
        // The pool reports them in its own order, and serves as if given smallest first.
        assert_eq!(pool.stacks(), THREE_STACKS);
        let bufs = exhaust_three(&mut pool);
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn stats_label_each_stack_by_its_geometry() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let bufs = exhaust_three(&mut pool);
        // The five misses belong to the 64-byte stack, found by its size, not a position.
        let stats = pool.stats();
        let small = stats
            .iter()
            .find(|s| s.geometry.buf_size == SMALL as u32)
            .unwrap();
        assert_eq!(small.misses, 5);
        assert_eq!(small.geometry, geom(LINE, 2));
        assert!(
            stats
                .iter()
                .filter(|s| s.geometry != small.geometry)
                .all(|s| s.misses == 0)
        );
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn buf_size_reports_the_size_given() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        // A typed guard, whose deref is the `T`, still reports its whole buffer.
        let msg = pool.alloc::<Msg>().unwrap();
        let two = pool.alloc::<TwoLines>().unwrap();
        assert_eq!((msg.buf_size(), two.buf_size()), (SMALL, MID));
        msg.free();
        two.free();
    }

    #[test]
    #[should_panic(expected = "size larger than the largest buffer")]
    fn too_big_type_panics() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [geom(LINE, 4)]).unwrap();
        let _ = pool.alloc::<TwoLines>();
    }

    #[test]
    #[should_panic(expected = "size larger than the largest buffer")]
    fn too_big_bytes_panics() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let _ = pool.alloc_bytes(4 * CACHE_LINE_SIZE + 1);
    }

    #[test]
    fn alloc_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [geom(LINE, 2), geom(2 * LINE, 1)]).unwrap();
        let held: [_; 3] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        let mut seen = Vec::new();
        let err = pool
            .alloc_with::<Msg>(|attempt| {
                seen.push(attempt);
                attempt < 2
            })
            .err();
        assert_eq!(err, Some(Exhausted));
        assert_eq!(seen, [0, 1, 2]);
        // One miss for the third alloc, then one per failed attempt.
        assert_eq!(pool.misses, [4, 0]);
        held.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn threaded_alloc_here_free_there() {
        // The allocator thread takes buffers of two sizes and a freer thread returns them, so the
        // frees race the pops on both stacks' heads.
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();

        let (tx, rx) = std::sync::mpsc::channel::<BufSlot<'_, Msg>>();
        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    let size = if i % 2 == 0 { 1 } else { 2 * CACHE_LINE_SIZE };
                    loop {
                        match pool.alloc_sized::<Msg>(size) {
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
                    b.free();
                }
            });
        });
    }

    #[test]
    fn desc_round_trip_across_stacks() {
        use crate::{Desc, PoolRegistry};
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut reg = PoolRegistry::<1, _>::new();
        let id = reg.register(pool.view()).unwrap();

        let mut small = pool.alloc::<Msg>().unwrap();
        let mut mid = pool.alloc::<TwoLines>().unwrap();
        let mut big = pool.alloc_bytes(3 * CACHE_LINE_SIZE).unwrap();
        small.seq = 1;
        mid.words[0] = 2;
        big[0] = 3;
        // Descriptor indices number the buffers across the stacks, smallest stack first.
        let small = reg.to_desc(id, small).map_err(|(_, e)| e).unwrap();
        let mid = reg.to_desc(id, mid).map_err(|(_, e)| e).unwrap();
        let big = reg.to_desc(id, big).map_err(|(_, e)| e).unwrap();
        let idxs = [small, mid, big].map(|d: Desc| d.buf_idx);
        assert_eq!(idxs, [0, 2, 4]);

        // SAFETY: each desc came from to_desc on this thread and is taken back exactly once.
        let (small, mid, big) = unsafe {
            (
                reg.to_slot::<Msg>(small).unwrap(),
                reg.to_slot::<TwoLines>(mid).unwrap(),
                reg.to_slot::<[u8; 4 * CACHE_LINE_SIZE]>(big).unwrap(),
            )
        };
        assert_eq!((small.seq, mid.words[0], big[0]), (1, 2, 3));
        assert_eq!(
            (small.buf_size, mid.buf_size, big.buf_size),
            (LINE, 2 * LINE, 4 * LINE)
        );
        small.free();
        mid.free();
        big.free();

        // Each buffer taken back went back to its own stack: all six allocate with no miss.
        let all: [_; 6] =
            [1, 1, 2, 2, 4, 4].map(|lines| pool.alloc_bytes(lines * CACHE_LINE_SIZE).unwrap());
        assert_eq!(pool.misses, [0, 0, 0]);
        all.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn to_desc_checks_pool_identity() {
        use crate::{PoolRegistry, RegistryError};
        let mut ra = Region::new();
        let mut rb = Region::new();
        let mut pool_a = Pool::init(&mut ra.0, THREE_STACKS).unwrap();
        let pool_b = Pool::init(&mut rb.0, THREE_STACKS).unwrap();
        let mut reg = PoolRegistry::<2, _>::new();
        let id_a = reg.register(pool_a.view()).unwrap();
        let id_b = reg.register(pool_b.view()).unwrap();

        // A guard from any stack of pool a is not pool b's, and comes back usable.
        let slot = pool_a.alloc::<TwoLines>().unwrap();
        let (slot, err) = reg.to_desc(id_b, slot).unwrap_err();
        assert_eq!(err, RegistryError::WrongPool);
        let desc = reg.to_desc(id_a, slot).map_err(|(_, e)| e).unwrap();
        // SAFETY: desc came from to_desc, taken back once.
        unsafe { reg.to_slot::<TwoLines>(desc) }.unwrap().free();
    }

    #[test]
    fn to_slot_rejects_hostile_descs() {
        use crate::{Desc, PoolRegistry, RegistryError};
        let mut r = Region::new();
        let pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut reg = PoolRegistry::<1, _>::new();
        reg.register(pool.view()).unwrap();
        let desc = |buf_idx| Desc {
            pool_id: 0,
            buf_idx,
        };

        // SAFETY: every to_slot here must fail validation and mint nothing.
        unsafe {
            assert_eq!(
                reg.to_slot::<Msg>(desc(6)).err(),
                Some(RegistryError::BadIndex)
            );
            assert_eq!(
                reg.to_slot::<Msg>(desc(u32::MAX)).err(),
                Some(RegistryError::BadIndex)
            );
            // Index 1 is the 64-byte stack's last buffer: a 128-byte T does not fit it, though
            // it fits the stack after.
            assert_eq!(
                reg.to_slot::<TwoLines>(desc(1)).err(),
                Some(RegistryError::BadType)
            );
        }
    }

    /// Descriptors carry buffers of two stacks through an SPSC ring, producer thread to consumer
    /// thread, the buffers recycling under pressure.
    #[test]
    fn desc_over_ring_cross_thread() {
        use crate::{Desc, PoolRegistry};
        const COUNT: u64 = if cfg!(miri) { 200 } else { 10_000 };
        let mut r = Region::new();
        let mut rr = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut reg = PoolRegistry::<1, _>::new();
        let id = reg.register(pool.view()).unwrap();
        let reg = &reg;
        let (mut producer, mut consumer) = crate::spsc::v2::Ring::init(&mut rr.0, LINE, 4)
            .unwrap()
            .split();

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    let size = if i % 2 == 0 { 1 } else { 2 * CACHE_LINE_SIZE };
                    let mut buf = loop {
                        match pool.alloc_sized::<Msg>(size) {
                            Ok(buf) => break buf,
                            Err(Exhausted) => std::hint::spin_loop(),
                        }
                    };
                    buf.seq = i;
                    let desc = reg.to_desc(id, buf).map_err(|(_, e)| e).unwrap();
                    let mut slot = producer
                        .reserve_slot_with::<Desc>(crate::policy::spin)
                        .unwrap();
                    *slot = desc;
                    slot.commit();
                }
            });
            s.spawn(move || {
                for i in 0..COUNT {
                    let desc = {
                        let slot = consumer
                            .reserve_slot_with::<Desc>(crate::policy::spin)
                            .unwrap();
                        let desc = *slot;
                        slot.release();
                        desc
                    };
                    // SAFETY: the desc was consumed into the ring by the producer and read after
                    // the commit -> reserve handoff, and each is taken back exactly once.
                    let msg = unsafe { reg.to_slot::<Msg>(desc) }.unwrap();
                    assert_eq!(msg.seq, i);
                    msg.free();
                }
            });
        });
    }

    /// The small stack's buffer size in [`THREE_STACKS`], in bytes.
    const SMALL: usize = CACHE_LINE_SIZE;
    /// The mid stack's buffer size in [`THREE_STACKS`], in bytes.
    const MID: usize = 2 * CACHE_LINE_SIZE;
    /// The big stack's buffer size in [`THREE_STACKS`], in bytes.
    const BIG: usize = 4 * CACHE_LINE_SIZE;

    /// Check what a user may rely on from `alloc_bytes(size)`: at least `size` bytes, starting on
    /// a cache line, and here exactly `expect` bytes, the size of the stack that should serve.
    fn assert_buf(buf: &BufSlot<'_, [u8]>, size: usize, expect: usize) {
        assert!(buf.len() >= size);
        assert_eq!(buf.as_ptr() as usize % CACHE_LINE_SIZE, 0);
        assert_eq!(buf.len(), expect, "a {size}-byte request");
    }

    /// Take every buffer of the [`THREE_STACKS`] pool by one-byte requests, so a scenario starts
    /// with every stack empty. The first two come from the small stack, the other four are
    /// fallbacks to the mid and big stacks, and the seventh request finds nothing: five misses,
    /// all against the small stack.
    fn exhaust_three<'a>(pool: &mut Pool<'a, 3>) -> Vec<BufSlot<'a, [u8]>> {
        let bufs: Vec<_> = (0..6).map(|_| pool.alloc_bytes(1).unwrap()).collect();
        let sizes: Vec<_> = bufs.iter().map(|b| b.len()).collect();
        assert_eq!(sizes, [SMALL, SMALL, MID, MID, BIG, BIG]);
        assert_eq!(pool.alloc_bytes(1).err(), Some(Exhausted));
        assert_eq!(pool.misses, [5, 0, 0]);
        bufs
    }

    /// Remove and return a held buffer of `bytes` bytes, so a scenario can give back a buffer
    /// of a chosen stack.
    fn take_sized<'a>(bufs: &mut Vec<BufSlot<'a, [u8]>>, bytes: usize) -> BufSlot<'a, [u8]> {
        let i = bufs.iter().position(|b| b.len() == bytes).unwrap();
        bufs.swap_remove(i)
    }

    #[test]
    fn small_free_never_serves_big() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut bufs = exhaust_three(&mut pool);

        // Give back one small buffer, so only the small stack has a free buffer.
        take_sized(&mut bufs, SMALL).free();

        // A big and a mid request: neither falls back to the smaller stack, and each counts a
        // miss against the stack it wanted.
        assert_eq!(pool.alloc_bytes(BIG).err(), Some(Exhausted));
        assert_eq!(pool.alloc_bytes(MID).err(), Some(Exhausted));
        assert_eq!(pool.misses, [5, 1, 1]);

        // The small buffer is still there for a request it fits, with no miss.
        let small = pool.alloc_bytes(1).unwrap();
        assert_buf(&small, 1, SMALL);
        assert_eq!(pool.misses, [5, 1, 1]);

        small.free();
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn big_free_serves_small() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut bufs = exhaust_three(&mut pool);

        // Give back one big buffer, so only the big stack has a free buffer.
        take_sized(&mut bufs, BIG).free();

        // A one-byte request falls back past the empty small and mid stacks to the big buffer,
        // one miss against the small stack.
        let got = pool.alloc_bytes(1).unwrap();
        assert_buf(&got, 1, BIG);
        assert_eq!(pool.misses, [6, 0, 0]);

        got.free();
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn fallback_takes_the_next_larger_first() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        let mut bufs = exhaust_three(&mut pool);

        // Give back a mid buffer, then a big one. A single LIFO list would hand out the big one
        // first, since it was freed last, so getting the mid one first can only be the stack
        // order at work.
        take_sized(&mut bufs, MID).free();
        take_sized(&mut bufs, BIG).free();

        // One-byte requests take the next larger stack first: mid, then big, then nothing, each
        // a miss against the small stack.
        let first = pool.alloc_bytes(1).unwrap();
        let second = pool.alloc_bytes(1).unwrap();
        assert_buf(&first, 1, MID);
        assert_buf(&second, 1, BIG);
        assert_eq!(pool.alloc_bytes(1).err(), Some(Exhausted));
        assert_eq!(pool.misses, [8, 0, 0]);

        first.free();
        second.free();
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn a_size_lands_in_the_first_stack_that_holds_it() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE_STACKS).unwrap();
        // (bytes asked, buffer expected): zero and every exact fit land in their own stack, one
        // byte more in the next, and nothing here is a fallback, so no miss.
        let cases = [
            (0, SMALL),
            (SMALL, SMALL),
            (SMALL + 1, MID),
            (MID, MID),
            (MID + 1, BIG),
            (BIG, BIG),
        ];
        for (size, expect) in cases {
            let buf = pool.alloc_bytes(size).unwrap();
            assert_buf(&buf, size, expect);
            buf.free();
        }
        assert_eq!(pool.misses, [0, 0, 0]);
    }

    /// One cache line of backing store, so a `Vec` of them is a line-aligned region of any
    /// length, for pools whose geometry a seed picks.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    #[repr(C, align(64))]
    struct Line([u8; CACHE_LINE_SIZE]);

    /// A zeroed, line-aligned heap region of at least `bytes` bytes.
    fn heap_region(bytes: u64) -> Vec<Line> {
        let lines = bytes.div_ceil(CACHE_LINE_SIZE as u64) as usize;
        (0..lines).map(|_| Line([0; CACHE_LINE_SIZE])).collect()
    }

    /// A seeded LCG: one seed replays a whole run.
    struct Lcg(u64);

    impl Lcg {
        /// The next 31 random bits.
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        /// A random value in `0..bound`.
        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }
    }

    /// The environment variable that replays one seed, decimal or `0x` hex.
    const SEED_VAR: &str = "ZC_POOL_SEED";

    /// The seeds a randomized test runs: the one in [`SEED_VAR`] alone when it is set, else the
    /// fixed seeds and one fresh random seed, so each run also explores a new path.
    fn seeds() -> Vec<u64> {
        if let Ok(text) = std::env::var(SEED_VAR) {
            let seed = match text.strip_prefix("0x") {
                Some(hex) => u64::from_str_radix(hex, 16),
                None => text.parse(),
            };
            return vec![seed.unwrap_or_else(|_| panic!("{SEED_VAR}={text} is not a u64"))];
        }
        use std::hash::BuildHasher;
        let fresh = std::collections::hash_map::RandomState::new().hash_one(0u64);
        vec![
            0x2545_f491_4f6c_dd1d,
            0x9e37_79b9_7f4a_7c15,
            0xd1b5_4a32_d192_ed03,
            fresh,
        ]
    }

    /// Reports how to replay a failure: dropped during a panic, it names the test, the seed,
    /// the thread's role, and the step it had reached.
    struct Replay {
        /// The test's name, for the `cargo test` filter.
        test: &'static str,
        /// The run's seed.
        seed: u64,
        /// Which thread: the model, the allocator, or a freer.
        role: String,
        /// The step the thread had reached.
        step: core::cell::Cell<u64>,
    }

    impl Replay {
        /// A guard for `role` in `test` under `seed`, at step 0.
        fn new(test: &'static str, seed: u64, role: &str) -> Self {
            Replay {
                test,
                seed,
                role: role.into(),
                step: core::cell::Cell::new(0),
            }
        }
    }

    impl Drop for Replay {
        /// Print the replay line when the thread is unwinding from a failure.
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!(
                    "{}: seed {:#x}, {} failed at step {}. Replay: {SEED_VAR}={:#x} cargo test \
                     --lib {}",
                    self.test,
                    self.seed,
                    self.role,
                    self.step.get(),
                    self.seed,
                    self.test
                );
            }
        }
    }

    /// `N` stacks of random geometry: ascending sizes one to four lines apart, and counts of
    /// one to four buffers, small so the stacks empty often.
    fn random_stacks<const N: usize>(rng: &mut Lcg) -> [StackGeometry; N] {
        let mut lines = 0;
        core::array::from_fn(|_| {
            lines += 1 + rng.below(4) as u32;
            geom(lines * LINE, 1 + rng.below(4) as u32)
        })
    }

    /// `stacks` in a random order, so `init` is handed the geometry unsorted.
    fn shuffled<const N: usize>(
        mut stacks: [StackGeometry; N],
        rng: &mut Lcg,
    ) -> [StackGeometry; N] {
        for i in (1..N).rev() {
            stacks.swap(i, rng.below(i as u64 + 1) as usize);
        }
        stacks
    }

    /// A random request size: a stack picked at random, then a size in its range, above the
    /// next smaller stack's size, so every stack is wanted about equally often.
    fn random_size(rng: &mut Lcg, sizes: &[usize]) -> usize {
        let target = rng.below(sizes.len() as u64) as usize;
        let low = if target == 0 {
            0
        } else {
            sizes[target - 1] + 1
        };
        low + rng.below((sizes[target] - low + 1) as u64) as usize
    }

    /// Run `f` with a stack count the seed picks, 1, 2, 3, 4, or 8, since `N` is a type.
    macro_rules! with_random_n {
        ($rng:expr, $f:ident($($arg:expr),*)) => {
            match $rng.below(5) {
                0 => $f::<1>($($arg),*),
                1 => $f::<2>($($arg),*),
                2 => $f::<3>($($arg),*),
                3 => $f::<4>($($arg),*),
                _ => $f::<8>($($arg),*),
            }
        };
    }

    /// Random allocs by size and frees in random order over one pool, checked step by step
    /// against a plain model of the rule: the smallest stack that fits, else the next larger
    /// with a free buffer, a miss against the wanted stack whenever it is empty, and
    /// `Exhausted` when no stack from the wanted one up has a buffer. Returns the fallbacks and
    /// the `Exhausted`s it saw, for the caller's coverage check.
    ///
    /// - Each held buffer carries its step in its first and last word, checked at its free, so
    ///   two guards over one buffer would show.
    fn model_run<const N: usize>(seed: u64, stacks: [StackGeometry; N], steps: u64) -> (u64, u64) {
        let replay = Replay::new("allocation_matches_the_model", seed, "the model");
        let mut rng = Lcg(seed);
        let sizes = stacks.map(|stack| stack.buf_size as usize);
        let mut region = heap_region(region_size(stacks));
        // The pool gets the stacks in a random order, the model keeps them sorted.
        let given = shuffled(stacks, &mut rng);
        let mut pool = Pool::init(region.as_mut_bytes(), given).unwrap();
        assert_eq!(pool.stacks(), stacks);

        // The model: free buffers and misses per stack.
        let mut free = stacks.map(|stack| stack.buf_count);
        let mut misses = [0u64; N];
        let mut held: Vec<(BufSlot<'_, [u8]>, usize, u64)> = Vec::new();
        let (mut fallbacks, mut exhausted) = (0, 0);

        for step in 0..steps {
            replay.step.set(step);
            if !held.is_empty() && rng.below(5) < 2 {
                let (buf, stack, tag) = held.swap_remove(rng.below(held.len() as u64) as usize);
                let last = buf.len() - 8;
                let head = u64::read_from_prefix(&buf[..]).unwrap().0;
                let tail = u64::read_from_prefix(&buf[last..]).unwrap().0;
                assert_eq!((head, tail), (tag, tag), "buffer overwritten");
                buf.free();
                free[stack] += 1;
                continue;
            }
            let size = random_size(&mut rng, &sizes);
            let wanted = sizes.iter().position(|&s| size <= s).unwrap();
            if free[wanted] == 0 {
                misses[wanted] += 1;
            }
            let served = (wanted..N).find(|&s| free[s] > 0);
            match (pool.alloc_bytes(size), served) {
                (Ok(mut buf), Some(stack)) => {
                    assert_eq!(buf.len(), sizes[stack], "a {size}-byte request");
                    let last = buf.len() - 8;
                    buf[..8].copy_from_slice(&step.to_ne_bytes());
                    buf[last..].copy_from_slice(&step.to_ne_bytes());
                    free[stack] -= 1;
                    held.push((buf, stack, step));
                    fallbacks += u64::from(stack != wanted);
                }
                (Err(Exhausted), None) => exhausted += 1,
                (got, want) => panic!(
                    "a {size}-byte request got {:?}, the model says stack {want:?}",
                    got.map(|b| b.len())
                ),
            }
            assert_eq!(pool.misses, misses);
        }

        // Every buffer back: each stack serves exactly its count again, with no miss.
        held.into_iter().for_each(|(buf, _, _)| buf.free());
        let before = pool.misses;
        for StackGeometry {
            buf_size: size,
            buf_count: count,
        } in stacks
        {
            let bufs: Vec<_> = (0..count)
                .map(|_| pool.alloc_bytes(size as usize).unwrap())
                .collect();
            assert!(bufs.iter().all(|b| b.len() == size as usize));
            bufs.into_iter().for_each(BufSlot::free);
        }
        assert_eq!(pool.misses, before);
        (fallbacks, exhausted)
    }

    /// [`model_run`] with a stack count and geometry the seed picks.
    fn model_run_random<const N: usize>(seed: u64, rng: &mut Lcg, steps: u64) -> (u64, u64) {
        model_run::<N>(seed, random_stacks::<N>(rng), steps)
    }

    /// The model check, over a fixed four-stack geometry and over a geometry each seed picks.
    #[test]
    fn allocation_matches_the_model() {
        const STEPS: u64 = if cfg!(miri) { 300 } else { 20_000 };
        const STACKS: [StackGeometry; 4] = [
            geom(LINE, 3),
            geom(2 * LINE, 2),
            geom(4 * LINE, 4),
            geom(8 * LINE, 1),
        ];
        let (mut fallbacks, mut exhausted) = (0, 0);
        for seed in seeds() {
            let (f, e) = model_run(seed, STACKS, STEPS);
            let mut rng = Lcg(seed ^ 0xa5a5_a5a5_a5a5_a5a5);
            let (rf, re) = with_random_n!(rng, model_run_random(seed, &mut rng, STEPS));
            fallbacks += f + rf;
            exhausted += e + re;
        }
        // The walks reached the paths under test, not only the plain pops.
        assert!(fallbacks > STEPS / 50 && exhausted > STEPS / 50);
    }

    /// One allocator thread and `freers` freer threads over one pool, all driven by `seed`.
    ///
    /// - The allocator takes random sizes, retrying on `Exhausted`, and hands each buffer to a
    ///   random freer. It checks each buffer against its request (at least the size, on a cache
    ///   line, from the wanted stack or a larger one), and keeps its own count of misses, which
    ///   `misses()` must match exactly, since only the allocator counts them.
    /// - Each freer holds what it receives and frees held buffers in random order, and frees
    ///   one whenever nothing arrives, so the allocator always makes progress.
    /// - Each buffer carries its message number in its first and last word and its request size
    ///   in its second, checked by the freer, so two guards over one buffer would show.
    /// - At the end every buffer is back, and each stack serves exactly its count.
    fn threaded_run<const N: usize>(seed: u64, stacks: [StackGeometry; N], freers: u64, msgs: u64) {
        const TEST: &str = "threaded_random_alloc_and_free";
        let sizes = stacks.map(|stack| stack.buf_size as usize);
        let mut region = heap_region(region_size(stacks));
        let given = shuffled(stacks, &mut Lcg(seed ^ 0x5bd1_e995));
        let mut pool = Pool::init(region.as_mut_bytes(), given).unwrap();
        assert_eq!(pool.stacks(), stacks);

        std::thread::scope(|s| {
            let mut senders = Vec::new();
            for k in 0..freers {
                let (tx, rx) = std::sync::mpsc::channel::<BufSlot<'_, [u8]>>();
                senders.push(tx);
                s.spawn(move || {
                    let replay = Replay::new(TEST, seed, &format!("freer {k}"));
                    let mut rng = Lcg(seed ^ (k + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                    let mut held: Vec<BufSlot<'_, [u8]>> = Vec::new();
                    let free_one = |held: &mut Vec<BufSlot<'_, [u8]>>, rng: &mut Lcg| {
                        let buf = held.swap_remove(rng.below(held.len() as u64) as usize);
                        let last = buf.len() - 8;
                        let head = u64::read_from_prefix(&buf[..]).unwrap().0;
                        let size = u64::read_from_prefix(&buf[8..]).unwrap().0;
                        let tail = u64::read_from_prefix(&buf[last..]).unwrap().0;
                        assert_eq!(head, tail, "buffer of message {head} overwritten");
                        assert!(
                            buf.len() as u64 >= size,
                            "message {head}: {size} bytes asked"
                        );
                        buf.free();
                    };
                    let mut received = 0;
                    loop {
                        replay.step.set(received);
                        match rx.try_recv() {
                            Ok(buf) => {
                                received += 1;
                                held.push(buf);
                                if rng.below(3) == 0 {
                                    free_one(&mut held, &mut rng);
                                }
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                if held.is_empty() {
                                    std::thread::yield_now();
                                } else {
                                    free_one(&mut held, &mut rng);
                                }
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    while !held.is_empty() {
                        free_one(&mut held, &mut rng);
                    }
                });
            }

            let pool = &mut pool;
            s.spawn(move || {
                let replay = Replay::new(TEST, seed, "the allocator");
                let mut rng = Lcg(seed);
                let mut misses = [0u64; N];
                for msg in 0..msgs {
                    replay.step.set(msg);
                    let size = random_size(&mut rng, &sizes);
                    let wanted = sizes.iter().position(|&s| size <= s).unwrap();
                    let mut buf = loop {
                        match pool.alloc_bytes(size) {
                            Ok(buf) => break buf,
                            Err(Exhausted) => {
                                misses[wanted] += 1;
                                std::thread::yield_now();
                            }
                        }
                    };
                    let stack = sizes.iter().position(|&s| s == buf.len()).unwrap();
                    assert!(
                        stack >= wanted,
                        "a {size}-byte request served by stack {stack}"
                    );
                    assert_eq!(buf.as_ptr() as usize % CACHE_LINE_SIZE, 0);
                    if stack != wanted {
                        misses[wanted] += 1;
                    }
                    assert_eq!(pool.misses, misses);
                    let last = buf.len() - 8;
                    buf[..8].copy_from_slice(&msg.to_ne_bytes());
                    buf[8..16].copy_from_slice(&(size as u64).to_ne_bytes());
                    buf[last..].copy_from_slice(&msg.to_ne_bytes());
                    senders[rng.below(freers) as usize].send(buf).unwrap();
                }
            });
        });

        // Every buffer back: each stack serves exactly its count again.
        let replay = Replay::new(TEST, seed, "the final check");
        replay.step.set(msgs);
        for StackGeometry {
            buf_size: size,
            buf_count: count,
        } in stacks
        {
            let bufs: Vec<_> = (0..count)
                .map(|_| pool.alloc_bytes(size as usize).unwrap())
                .collect();
            assert!(bufs.iter().all(|b| b.len() == size as usize));
            bufs.into_iter().for_each(BufSlot::free);
        }
    }

    /// [`threaded_run`] with a stack count and geometry the seed picks.
    fn threaded_run_random<const N: usize>(seed: u64, rng: &mut Lcg, freers: u64, msgs: u64) {
        threaded_run::<N>(seed, random_stacks::<N>(rng), freers, msgs);
    }

    /// Two threads (an allocator and one freer) and three (an allocator and two freers), each
    /// over a random geometry per seed, the frees racing the pops on every stack's head.
    #[test]
    fn threaded_random_alloc_and_free() {
        const MSGS: u64 = if cfg!(miri) { 100 } else { 20_000 };
        for seed in seeds() {
            for freers in [1, 2] {
                let mut rng = Lcg(seed ^ freers);
                with_random_n!(rng, threaded_run_random(seed, &mut rng, freers, MSGS));
            }
        }
    }
}
