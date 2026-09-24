//! Multi-stack message pool: v0's fixed-size buffers and intrusive free-stack, one stack per buffer
//! size, over one caller-provided region.
//!
//! - A stack is one buffer size with its own buffers, linked as v0's free-stack is. `N` stacks,
//!   sorted smallest first, share one [`PoolHeader`] and one region, and v0 is the single-stack
//!   pool.
//! - "Stack" is the pools' word and "segment" the rings': a ring segment is a pool buffer that a
//!   ring of segments runs through.
//! - The alloc family keeps v0's shape and picks the stack by size: the smallest stack that fits,
//!   then the next larger one when that stack is empty. Each such miss is counted against the
//!   stack that was wanted ([`Pool::misses`]), so a user can tell which size wants more buffers.
//! - At `N = 1` the pick is the one size comparison v0's `alloc` already makes, and the fallback
//!   loop is empty, so the single-stack pool does v0's work.
//! - A [`BufSlot`] frees to its own stack, so a free never searches.
//! - Roles as v0's: one owning allocator pops (`&mut self`), any holder frees.

use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};
use zerocopy::{FromBytes, IntoBytes, KnownLayout};

use crate::{CACHE_LINE_SIZE, CacheAligned, Error};

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
    /// Snapshot of `header.buf_sizes`.
    buf_sizes: [u32; N],
    /// Snapshot of `header.buf_counts`.
    buf_counts: [u32; N],
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
    /// - `stacks`: `(buf_size, buf_count)` per stack, sizes [`CACHE_LINE_SIZE`] multiples in
    ///   strictly ascending order, counts nonzero, the total count `< u32::MAX`.
    /// - The region must be [`CACHE_LINE_SIZE`]-aligned and at least [`region_size`] bytes.
    pub fn init(region: &'a mut [u8], stacks: [(u32, u32); N]) -> Result<Self, Error> {
        let buf_sizes = stacks.map(|(size, _)| size);
        let buf_counts = stacks.map(|(_, count)| count);
        validate_stacks(&buf_sizes, &buf_counts)?;
        let len = region.len();
        // Taken exactly once, same Stacked Borrows retag hazard as v0's init.
        let base = region.as_mut_ptr();
        let header = header_ptr::<N>(base, len)?;
        if (len as u64) < region_size_of(&buf_sizes, &buf_counts) {
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
        for c in 0..N {
            header.buf_sizes[c].store(buf_sizes[c], Ordering::Relaxed);
            header.buf_counts[c].store(buf_counts[c], Ordering::Relaxed);
        }
        let pool = Pool {
            header,
            bases: stack_bases(base, &buf_sizes, &buf_counts),
            buf_sizes,
            buf_counts,
            misses: [0; N],
            _region: PhantomData,
        };
        // Link each stack's buffers: i -> i + 1, last -> NIL, head -> 0.
        for (c, &count) in buf_counts.iter().enumerate() {
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
        // Snapshot geometry once. Per-op paths never re-read.
        let buf_sizes: [u32; N] =
            core::array::from_fn(|c| header.buf_sizes[c].load(Ordering::Relaxed));
        let buf_counts: [u32; N] =
            core::array::from_fn(|c| header.buf_counts[c].load(Ordering::Relaxed));
        validate_stacks(&buf_sizes, &buf_counts)?;
        if (len as u64) < region_size_of(&buf_sizes, &buf_counts) {
            return Err(Error::TooSmall);
        }
        Ok(Pool {
            header,
            bases: stack_bases(region, &buf_sizes, &buf_counts),
            buf_sizes,
            buf_counts,
            misses: [0; N],
            _region: PhantomData,
        })
    }

    /// Take a buffer for a `T` from the smallest stack that fits, or a larger one when that
    /// stack is empty, as an owned [`BufSlot`], or [`Exhausted`].
    ///
    /// - Roles, the validated pop, and the guard's independence from the pool are v0's
    ///   [`alloc`](super::v0::Pool::alloc)'s.
    /// - An empty wanted stack counts one miss against it ([`misses`](Pool::misses)), whether a
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

    /// Buffer size in bytes of stack `stack` (geometry snapshot).
    pub fn buf_size(&self, stack: usize) -> u32 {
        self.buf_sizes[stack]
    }

    /// Buffer count of stack `stack` (geometry snapshot).
    pub fn buf_count(&self, stack: usize) -> u32 {
        self.buf_counts[stack]
    }

    /// Allocations, per stack, that wanted the stack and found it empty, since this handle was
    /// made.
    ///
    /// - A count that keeps rising says the stack wants more buffers.
    pub fn misses(&self) -> [u64; N] {
        self.misses
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
            if size <= self.buf_sizes[stack] as usize {
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
        let count = self.buf_counts[stack];
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
                    buf_size: self.buf_sizes[stack],
                    idx: head,
                    _slot: PhantomData,
                });
            }
        }
    }

    /// The next-free-buffer index cell of buffer `idx` of stack `stack`, its first word, the
    /// intrusive stack link, meaningful only while the buffer is free.
    ///
    /// - Callers pass `idx < buf_counts[stack]` (validated pops, init's linking loop).
    fn next_buf_idx(&self, stack: usize, idx: u32) -> &AtomicU32 {
        let p = self.buf_ptr(stack, idx) as *const AtomicU32;
        // SAFETY: idx < the stack's count keeps the buffer in the region validated at
        // init/attach. The base is cache-line aligned and sizes are line multiples, so the first
        // word is 4-aligned. All peers access it as an atomic.
        unsafe { &*p }
    }

    /// Pointer to buffer `idx` of stack `stack`, and callers pass `idx < buf_counts[stack]`.
    fn buf_ptr(&self, stack: usize, idx: u32) -> *mut u8 {
        // SAFETY: idx < the stack's count, so the offset stays inside the stack's buffer array
        // validated at init/attach.
        unsafe { self.bases[stack].add(idx as usize * self.buf_sizes[stack] as usize) }
    }
}

/// An allocated buffer, owned until [`free`](BufSlot::free): `DerefMut` to use it as a `T` in
/// place, or as all of its bytes for `BufSlot<[u8]>`.
///
/// - Does not borrow the [`Pool`], as v0's guard.
/// - Carries its stack's head, so free goes straight to the right stack.
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

/// Bytes needed for a multi-stack pool region with the given stacks, `(buf_size, buf_count)`
/// each, computed in u64 so a 32-bit target cannot wrap.
pub fn region_size<const N: usize>(stacks: [(u32, u32); N]) -> u64 {
    region_size_of(
        &stacks.map(|(size, _)| size),
        &stacks.map(|(_, count)| count),
    )
}

/// [`region_size`] over the split geometry arrays.
fn region_size_of<const N: usize>(buf_sizes: &[u32; N], buf_counts: &[u32; N]) -> u64 {
    let mut bytes = size_of::<PoolHeader<N>>() as u64;
    for c in 0..N {
        bytes += buf_sizes[c] as u64 * buf_counts[c] as u64;
    }
    bytes
}

/// The base of each stack's buffer array: the stacks follow the header in order, each
/// `buf_size * buf_count` bytes.
///
/// - Callers have validated the region's length against [`region_size_of`], so every base is in
///   bounds.
fn stack_bases<const N: usize>(
    base: *mut u8,
    buf_sizes: &[u32; N],
    buf_counts: &[u32; N],
) -> [*mut u8; N] {
    let mut offset = size_of::<PoolHeader<N>>();
    core::array::from_fn(|c| {
        // SAFETY: offset is at most the region size validated by the caller.
        let p = unsafe { base.add(offset) };
        offset += buf_sizes[c] as usize * buf_counts[c] as usize;
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

/// Shared stack checks for [`Pool::init`] and [`Pool::attach`].
///
/// - Sizes: nonzero [`CACHE_LINE_SIZE`] multiples, strictly ascending, so the first stack that
///   fits is the smallest.
/// - Counts: each nonzero, the total below [`NIL`], so a buffer index across the stacks never
///   reads as the sentinel.
/// - `N = 0` is refused as a bad count: a pool with no stack serves nothing.
fn validate_stacks<const N: usize>(
    buf_sizes: &[u32; N],
    buf_counts: &[u32; N],
) -> Result<(), Error> {
    if N == 0 {
        return Err(Error::BadBufCount);
    }
    let mut total = 0u64;
    for c in 0..N {
        let size = buf_sizes[c];
        if size == 0 || !(size as usize).is_multiple_of(CACHE_LINE_SIZE) {
            return Err(Error::BadBufSize);
        }
        if c > 0 && size <= buf_sizes[c - 1] {
            return Err(Error::BadBufSize);
        }
        if buf_counts[c] == 0 {
            return Err(Error::BadBufCount);
        }
        total += buf_counts[c] as u64;
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

    /// Two lines: too big for a one-line stack.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
    #[repr(C)]
    struct TwoLines {
        words: [u64; 2 * CACHE_LINE_SIZE / 8],
    }

    /// One cache line, the smallest buffer size.
    const LINE: u32 = CACHE_LINE_SIZE as u32;

    /// The three-stack geometry most tests use: 2 one-line, 2 two-line, 2 four-line buffers.
    const THREE: [(u32, u32); 3] = [(LINE, 2), (2 * LINE, 2), (4 * LINE, 2)];

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
        assert_eq!(region_size(THREE), (4 + 2 + 4 + 8) as u64 * LINE as u64);
    }

    #[test]
    fn init_rejects_bad_stacks() {
        let mut r = Region::new();
        let err = |r: &mut Region, stacks| Pool::<3>::init(&mut r.0, stacks).err();
        assert_eq!(
            err(&mut r, [(LINE, 2), (LINE, 2), (4 * LINE, 2)]),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(&mut r, [(2 * LINE, 2), (LINE, 2), (4 * LINE, 2)]),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(&mut r, [(LINE - 1, 2), (2 * LINE, 2), (4 * LINE, 2)]),
            Some(Error::BadBufSize)
        );
        assert_eq!(
            err(&mut r, [(LINE, 2), (2 * LINE, 0), (4 * LINE, 2)]),
            Some(Error::BadBufCount)
        );
        assert_eq!(
            err(
                &mut r,
                [
                    (LINE, u32::MAX / 2),
                    (2 * LINE, u32::MAX / 2),
                    (4 * LINE, 1)
                ]
            ),
            Some(Error::BadBufCount)
        );
        assert_eq!(
            err(&mut r, [(LINE, 2), (2 * LINE, 2), (4 * LINE, 8)]),
            Some(Error::TooSmall)
        );
        assert_eq!(
            Pool::<3>::init(&mut r.0[1..], THREE).err(),
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
        Pool::init(&mut r.0, THREE).unwrap();
        // One pointer for every attach, so no later retag invalidates the attached handle.
        let (base, len) = (r.0.as_mut_ptr(), r.0.len());
        let pool = unsafe { Pool::<3>::attach(base, len) }.unwrap();
        assert_eq!(pool.buf_size(2), 4 * LINE);
        assert_eq!(pool.buf_count(1), 2);
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
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
        let small = pool.alloc::<Msg>().unwrap();
        let mid = pool.alloc::<TwoLines>().unwrap();
        let bytes = pool.alloc_bytes(3 * CACHE_LINE_SIZE).unwrap();
        assert_eq!(small.buf_size, LINE);
        assert_eq!(mid.buf_size, 2 * LINE);
        // The size actually given, at least the size asked for.
        assert_eq!(bytes.len(), 4 * CACHE_LINE_SIZE);
        assert_eq!(bytes.as_ptr() as usize % CACHE_LINE_SIZE, 0);
        assert_eq!(pool.misses(), [0, 0, 0]);
        small.free();
        mid.free();
        bytes.free();
    }

    #[test]
    fn empty_stack_falls_back_and_counts_misses() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
        // Two one-line buffers, then the two-line stack serves, then the four-line stack.
        let bufs: [_; 6] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        let sizes = bufs.each_ref().map(|b| b.buf_size);
        assert_eq!(sizes, [LINE, LINE, 2 * LINE, 2 * LINE, 4 * LINE, 4 * LINE]);
        assert_eq!(pool.misses(), [4, 0, 0]);
        // Every stack that fits is empty.
        assert_eq!(pool.alloc::<Msg>().err(), Some(Exhausted));
        assert_eq!(pool.misses(), [5, 0, 0]);
        // A free goes back to its own stack: the one-line stack serves again, no miss.
        let [a, b, c, d, e, f] = bufs;
        a.free();
        let again = pool.alloc::<Msg>().unwrap();
        assert_eq!(again.buf_size, LINE);
        assert_eq!(pool.misses(), [5, 0, 0]);
        // A larger size never falls back to a smaller stack.
        c.free();
        let two = pool.alloc::<TwoLines>().unwrap();
        assert_eq!(two.buf_size, 2 * LINE);
        assert_eq!(pool.alloc::<TwoLines>().err(), Some(Exhausted));
        assert_eq!(pool.misses(), [5, 1, 0]);
        for buf in [again, b, d, e, f] {
            buf.free();
        }
        two.free();
    }

    #[test]
    fn free_is_lifo_per_stack() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
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
        let end = r.0.as_ptr() as usize + region_size(THREE) as usize;
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
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
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
        // A peer scribbles the one-line stack's head: that stack reads as empty, and the next
        // stack serves, never an out-of-bounds buffer.
        pool.header.heads[0].store(1000, Ordering::Relaxed);
        let buf = pool.alloc::<Msg>().unwrap();
        assert_eq!(buf.buf_size, 2 * LINE);
        assert_eq!(pool.misses(), [1, 0, 0]);
        buf.free();
    }

    #[test]
    fn single_stack_is_v0_shaped() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [(LINE, 4)]).unwrap();
        let bufs: [_; 4] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        assert_eq!(pool.alloc::<Msg>().err(), Some(Exhausted));
        assert_eq!(pool.misses(), [1]);
        bufs.into_iter().for_each(BufSlot::free);
    }

    #[test]
    #[should_panic(expected = "size larger than the largest buffer")]
    fn too_big_type_panics() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [(LINE, 4)]).unwrap();
        let _ = pool.alloc::<TwoLines>();
    }

    #[test]
    #[should_panic(expected = "size larger than the largest buffer")]
    fn too_big_bytes_panics() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();
        let _ = pool.alloc_bytes(4 * CACHE_LINE_SIZE + 1);
    }

    #[test]
    fn alloc_with_policy_counts_and_gives_up() {
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, [(LINE, 2), (2 * LINE, 1)]).unwrap();
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
        assert_eq!(pool.misses(), [4, 0]);
        held.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn threaded_alloc_here_free_there() {
        // The allocator thread takes buffers of two sizes and a freer thread returns them, so the
        // frees race the pops on both stacks' heads.
        const COUNT: u64 = if cfg!(miri) { 200 } else { 100_000 };
        let mut r = Region::new();
        let mut pool = Pool::init(&mut r.0, THREE).unwrap();

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
}
