//! Descriptors and the per-process pool registry, the
//! travel half of the messaging layer: a [`Desc`] names a
//! buffer as `(pool id, buffer index)` so it can ride any
//! queue as an ordinary POD message, and the [`PoolRegistry`]
//! turns descriptors back into owned guards (see the design
//! doc's "Descriptor and registry design (0.7.0)").
//!
//! - [`PoolRegistry::to_desc`] (safe) consumes a guard into
//!   a descriptor, and ownership travels on in the descriptor.
//! - [`PoolRegistry::to_slot`] (unsafe) validates a received
//!   descriptor and mints the guard back, and `unsafe` covers
//!   only what validation cannot check, ownership
//!   uniqueness.
//! - Fixed capacity, no allocation, no unregister: pool ids
//!   are registry slot indices and never dangle.
//! - Generic over the pool kind through [`DescMap`]: a
//!   registry holds the pool views of one kind, v0's by default,
//!   and dispatch is static, so each kind's path is its own
//!   code.

use core::marker::PhantomData;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::pool::PoolView;
use crate::{BufSlot, type_fits};

pub(crate) mod sealed {
    /// Keeps [`DescMap`](super::DescMap) implemented by this
    /// crate's pools only, since its `to_slot` mints owned
    /// guards on the implementor's word.
    pub trait Sealed {}
}

/// A pool's view, which cannot pop, as a [`PoolRegistry`] holds it:
/// maps a guard to its descriptor index and a validated index
/// back to a guard.
///
/// - Sealed: implemented by `pool::v0`'s and `pool::v1`'s
///   [`PoolView`]s.
/// - `Slot<T>` is the guard the pool's allocs mint, so a
///   registry takes and returns the pool's own guard type.
pub trait DescMap<'a>: sealed::Sealed + Copy {
    /// The pool's owned buffer guard.
    type Slot<T: ?Sized + 'a>;

    /// The descriptor index of `slot` when it came from this
    /// pool, or `None` when it came from another.
    fn desc_idx<T: ?Sized + 'a>(&self, slot: &Self::Slot<T>) -> Option<u32>;

    /// Validate descriptor index `idx` and `T`'s fit, and mint
    /// the owned guard.
    ///
    /// # Safety
    ///
    /// As [`PoolRegistry::to_slot`]: the index came from a
    /// consumed guard, arrived with happens-before ordering, and
    /// is taken back exactly once.
    unsafe fn to_slot<T>(&self, idx: u32) -> Result<Self::Slot<T>, RegistryError>
    where
        T: FromBytes + IntoBytes + KnownLayout + 'a;

    /// Validate descriptor index `idx` and mint the owned guard
    /// over the buffer's bytes, all of them.
    ///
    /// # Safety
    ///
    /// As [`to_slot`](DescMap::to_slot).
    unsafe fn to_slot_bytes(&self, idx: u32) -> Result<Self::Slot<[u8]>, RegistryError>;
}

// Empty on purpose: `Sealed` has no methods, and this impl is
// the crate opting v0's view into `DescMap`, which code outside
// the crate cannot do.
impl sealed::Sealed for PoolView<'_> {}

impl<'a> DescMap<'a> for PoolView<'a> {
    type Slot<T: ?Sized + 'a> = BufSlot<'a, T>;

    /// The guard's index when its pool's header is this one's.
    fn desc_idx<T: ?Sized + 'a>(&self, slot: &BufSlot<'a, T>) -> Option<u32> {
        core::ptr::eq(self.header_ptr(), slot.header_ptr()).then(|| slot.idx())
    }

    /// Bounds-check the index and `T` against the one buffer
    /// size, then mint.
    unsafe fn to_slot<T>(&self, idx: u32) -> Result<BufSlot<'a, T>, RegistryError>
    where
        T: FromBytes + IntoBytes + KnownLayout + 'a,
    {
        if idx >= self.buf_count() {
            return Err(RegistryError::BadIndex);
        }
        if !type_fits::<T>(self.buf_size()) {
            return Err(RegistryError::BadType);
        }
        // SAFETY: index and T geometry validated above.
        // Ownership uniqueness and ordering are the caller's
        // contract.
        Ok(unsafe { self.slot_from_idx(idx) })
    }

    /// Bounds-check the index, then mint the byte guard.
    unsafe fn to_slot_bytes(&self, idx: u32) -> Result<BufSlot<'a, [u8]>, RegistryError> {
        if idx >= self.buf_count() {
            return Err(RegistryError::BadIndex);
        }
        // SAFETY: index validated above, and every buffer is
        // valid as bytes. Ownership uniqueness and ordering are
        // the caller's contract.
        Ok(unsafe { self.slot_from_idx(idx) })
    }
}

/// A buffer's travel form: names its pool and buffer index so
/// any queue can carry it as an ordinary message.
///
/// - Plain data on purpose: `FromBytes` means a receiver
///   mints one from shared bytes anyway, so ownership
///   discipline lives in [`PoolRegistry::to_slot`]'s
///   contract, not in this type.
/// - Fields are raw `u32`s (not [`PoolId`]) because the wire
///   form is untrusted by definition, and validation happens at
///   `to_slot`.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Desc {
    /// The registry slot of the buffer's pool.
    pub pool_id: u32,
    /// The buffer's index within that pool.
    pub buf_idx: u32,
}

/// A registered pool's identity within one process's
/// [`PoolRegistry`], the value [`register`](PoolRegistry::register)
/// returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolId(u32);

impl PoolId {
    /// The raw id (a registry slot index).
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Errors from [`PoolRegistry`] operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    /// `register`: the fixed-capacity table is full.
    Full,
    /// The pool id names no registered pool.
    UnknownPoolId,
    /// `to_desc`: the guard's pool is not the id's entry.
    WrongPool,
    /// `to_slot`: buffer index out of range for the pool.
    BadIndex,
    /// `to_slot`: `T`'s geometry does not fit the pool's
    /// buffers. An `Err`, not the `alloc`-style panic: the
    /// descriptor selects which pool gets compared, and
    /// untrusted input must not select a panic.
    BadType,
}

/// Per-process table mapping pool ids to this process's
/// view of each pool, `R`, v0's [`PoolView`] by
/// default.
///
/// - Fixed capacity `N` (const generic): no_std, zero
///   allocation.
/// - `register` assigns the next slot index as the pool's id, and
///   there is no unregister, so ids never dangle.
/// - Registration (`&mut self`) is setup-phase, and `to_desc` /
///   `to_slot` take `&self`, so one registry is shared by
///   reference across threads.
pub struct PoolRegistry<'a, const N: usize, R: DescMap<'a> = PoolView<'a>> {
    /// Registered views, dense from slot 0.
    views: [Option<R>; N],
    /// Number of registered pools (next id to assign).
    len: usize,
    /// The pools' region lifetime, which `R` carries.
    _pools: PhantomData<&'a ()>,
}

impl<'a, const N: usize, R: DescMap<'a>> PoolRegistry<'a, N, R> {
    /// An empty registry.
    pub const fn new() -> Self {
        Self {
            views: [None; N],
            len: 0,
            _pools: PhantomData,
        }
    }

    /// Register a pool's view, and returns the assigned
    /// [`PoolId`] (the next slot index), or
    /// [`RegistryError::Full`].
    pub fn register(&mut self, view: R) -> Result<PoolId, RegistryError> {
        if self.len == N {
            return Err(RegistryError::Full);
        }
        let id = self.len as u32;
        self.views[self.len] = Some(view);
        self.len += 1;
        Ok(PoolId(id))
    }

    /// Consume a guard into its descriptor, and ownership travels
    /// on in the [`Desc`] (the usage model's "in-flight"
    /// state).
    ///
    /// - Checks the guard's pool identity against the id's
    ///   entry, catching id/pool mispairing: one comparison for
    ///   a v0 pool, one per stack for a v1 pool.
    /// - The error side hands the guard back, so a miss
    ///   cannot leak the buffer.
    pub fn to_desc<T: ?Sized + 'a>(
        &self,
        pool_id: PoolId,
        slot: R::Slot<T>,
    ) -> Result<Desc, (R::Slot<T>, RegistryError)> {
        let Some(view) = self.get(pool_id.0) else {
            return Err((slot, RegistryError::UnknownPoolId));
        };
        let Some(buf_idx) = view.desc_idx(&slot) else {
            return Err((slot, RegistryError::WrongPool));
        };
        Ok(Desc {
            pool_id: pool_id.0,
            buf_idx,
        })
    }

    /// Validate a received descriptor and mint its owned
    /// guard.
    ///
    /// Every validation failure is an `Err`, never a panic:
    /// unknown pool id, index out of range, `T` geometry
    /// mismatch (see [`RegistryError::BadType`] for why this
    /// differs from `alloc`).
    ///
    /// # Safety
    ///
    /// Validation cannot check ownership, and the caller promises:
    ///
    /// - `desc` came from [`to_desc`](Self::to_desc) (or
    ///   an equivalent consumed guard). It is not invented.
    /// - It arrived over a channel establishing happens-before
    ///   with the sender's writes (a ring commit -> reserve
    ///   qualifies).
    /// - It is taken back exactly once. A second `to_slot` mints
    ///   a second guard aliasing the same `&mut T`.
    pub unsafe fn to_slot<T>(&self, desc: Desc) -> Result<R::Slot<T>, RegistryError>
    where
        T: FromBytes + IntoBytes + KnownLayout + 'a,
    {
        let Some(view) = self.get(desc.pool_id) else {
            return Err(RegistryError::UnknownPoolId);
        };
        // SAFETY: the view validates the index and T's
        // geometry. Ownership uniqueness and ordering are the
        // caller's contract (this fn's # Safety).
        unsafe { view.to_slot(desc.buf_idx) }
    }

    /// Validate a received descriptor and mint its owned guard
    /// over the buffer's bytes, all of them, for a receiver that
    /// learns the message's type from the bytes.
    ///
    /// - The byte guard's `into_typed` then gives the typed
    ///   guard, so a receiver reads a tag and matches on it.
    /// - Every validation failure is an `Err`, as for
    ///   [`to_slot`](Self::to_slot).
    ///
    /// # Safety
    ///
    /// As [`to_slot`](Self::to_slot): the desc came from
    /// [`to_desc`](Self::to_desc), arrived with happens-before
    /// ordering, and is taken back exactly once, by this call or
    /// `to_slot`, never both.
    pub unsafe fn to_slot_bytes(&self, desc: Desc) -> Result<R::Slot<[u8]>, RegistryError> {
        let Some(view) = self.get(desc.pool_id) else {
            return Err(RegistryError::UnknownPoolId);
        };
        // SAFETY: the view validates the index. Ownership
        // uniqueness and ordering are the caller's contract.
        unsafe { view.to_slot_bytes(desc.buf_idx) }
    }

    /// Number of registered pools.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when no pool is registered.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The view registered under raw id `id`, if any.
    fn get(&self, id: u32) -> Option<&R> {
        self.views.get(id as usize)?.as_ref()
    }
}

impl<'a, const N: usize, R: DescMap<'a>> Default for PoolRegistry<'a, N, R> {
    /// Same as [`PoolRegistry::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CACHE_LINE_SIZE, Exhausted, Pool, PoolHeader};
    use core::mem::size_of;

    /// Test message: one word carrying a sequence number.
    #[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
    #[repr(C)]
    struct Msg {
        seq: u64,
    }

    /// One-line buffers / slots for every test.
    const LINE: u32 = CACHE_LINE_SIZE as u32;

    /// Pool region: header + 4 one-line buffers.
    const POOL_BYTES: usize = size_of::<PoolHeader>() + 4 * CACHE_LINE_SIZE;

    /// Ring region: header (4 lines) + 4 one-line slots.
    const RING_BYTES: usize = 8 * CACHE_LINE_SIZE;

    /// Cache-line-aligned backing store for pool/ring regions.
    #[repr(C, align(64))]
    struct Region<const B: usize>([u8; B]);

    #[test]
    fn desc_round_trip() {
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<2>::new();
        assert!(reg.is_empty());
        let id = reg.register(pool.view()).unwrap();
        assert_eq!(id.as_u32(), 0);
        assert_eq!(reg.len(), 1);

        let mut slot = pool.alloc::<Msg>().unwrap();
        slot.seq = 42;
        let desc = reg.to_desc(id, slot).map_err(|(_, e)| e).unwrap();
        assert_eq!(
            desc,
            Desc {
                pool_id: 0,
                buf_idx: 0
            }
        );

        // SAFETY: desc came from to_desc on this thread and
        // is taken back exactly once.
        let got = unsafe { reg.to_slot::<Msg>(desc) }.unwrap();
        assert_eq!(got.seq, 42);
        got.free();

        // The freed buffer is back on the free-stack: all 4
        // allocate again, then exhaustion.
        let slots: [_; 4] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);
        slots.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn to_slot_bytes_then_into_typed() {
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<1>::new();
        let id = reg.register(pool.view()).unwrap();

        let mut slot = pool.alloc::<Msg>().unwrap();
        slot.seq = 42;
        let desc = reg.to_desc(id, slot).map_err(|(_, e)| e).unwrap();
        // SAFETY: desc came from to_desc on this thread and is taken back once.
        let bytes = unsafe { reg.to_slot_bytes(desc) }.unwrap();
        // The whole buffer, and the message in its first bytes.
        assert_eq!(bytes.len(), CACHE_LINE_SIZE);
        assert_eq!(u64::read_from_prefix(&bytes[..]).unwrap().0, 42);
        // Too big for the buffer: handed back. A fit: typed.
        let bytes = bytes
            .into_typed::<[u8; 2 * CACHE_LINE_SIZE]>()
            .err()
            .unwrap();
        let msg = bytes.into_typed::<Msg>().map_err(|_| "msg").unwrap();
        assert_eq!(msg.seq, 42);
        msg.free();
        // SAFETY: an out-of-range index must fail validation and mint nothing.
        let bad = Desc {
            pool_id: 0,
            buf_idx: 4,
        };
        assert_eq!(
            unsafe { reg.to_slot_bytes(bad) }.err(),
            Some(RegistryError::BadIndex)
        );
    }

    #[test]
    fn to_desc_checks_pool_identity() {
        let mut ra = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut rb = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool_a = Pool::init(&mut ra.0, LINE, 4).unwrap();
        let pool_b = Pool::init(&mut rb.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<2>::new();
        let id_a = reg.register(pool_a.view()).unwrap();
        let id_b = reg.register(pool_b.view()).unwrap();

        // Mispaired id: rejected, and the guard comes back
        // usable, no leak.
        let slot = pool_a.alloc::<Msg>().unwrap();
        let (slot, err) = reg.to_desc(id_b, slot).unwrap_err();
        assert_eq!(err, RegistryError::WrongPool);
        let desc = reg.to_desc(id_a, slot).map_err(|(_, e)| e).unwrap();
        // SAFETY: desc came from to_desc, taken back once.
        unsafe { reg.to_slot::<Msg>(desc) }.unwrap().free();
    }

    #[test]
    fn to_desc_unknown_pool_id() {
        let mut ra = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut rb = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool_a = Pool::init(&mut ra.0, LINE, 4).unwrap();
        let pool_b = Pool::init(&mut rb.0, LINE, 4).unwrap();
        // An id minted by a bigger registry has no entry in a
        // smaller one.
        let mut big = PoolRegistry::<2>::new();
        big.register(pool_a.view()).unwrap();
        let id_b = big.register(pool_b.view()).unwrap();
        let mut small = PoolRegistry::<1>::new();
        small.register(pool_a.view()).unwrap();

        let slot = pool_a.alloc::<Msg>().unwrap();
        let (slot, err) = small.to_desc(id_b, slot).unwrap_err();
        assert_eq!(err, RegistryError::UnknownPoolId);
        slot.free();
    }

    #[test]
    fn to_slot_rejects_hostile_descs() {
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<1>::new();
        reg.register(pool.view()).unwrap();

        // SAFETY: every to_slot here must fail validation and
        // mint nothing, and no ownership is claimed.
        unsafe {
            let bad_pool = Desc {
                pool_id: 7,
                buf_idx: 0,
            };
            assert_eq!(
                reg.to_slot::<Msg>(bad_pool).err().unwrap(),
                RegistryError::UnknownPoolId
            );
            let bad_idx = Desc {
                pool_id: 0,
                buf_idx: 4,
            };
            assert_eq!(
                reg.to_slot::<Msg>(bad_idx).err().unwrap(),
                RegistryError::BadIndex
            );
            let ok_target = Desc {
                pool_id: 0,
                buf_idx: 0,
            };
            // T bigger than the pool's one-line buffers.
            assert_eq!(
                reg.to_slot::<[u8; 2 * CACHE_LINE_SIZE]>(ok_target)
                    .err()
                    .unwrap(),
                RegistryError::BadType
            );
        }
    }

    #[test]
    fn register_full() {
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<1>::new();
        reg.register(pool.view()).unwrap();
        assert_eq!(reg.register(pool.view()).unwrap_err(), RegistryError::Full);
    }

    /// The composed protocol: descriptors carry pool-buffer
    /// ownership through the SPSC ring, producer thread to
    /// consumer thread, buffers recycling under pressure
    /// (COUNT >> buf_count).
    #[test]
    fn desc_over_ring_cross_thread() {
        const COUNT: u64 = if cfg!(miri) { 200 } else { 10_000 };
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut rr = Region::<RING_BYTES>([0; RING_BYTES]);
        let mut pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<1>::new();
        let id = reg.register(pool.view()).unwrap();
        let reg = &reg;
        let (mut producer, mut consumer) = crate::spsc::v2::Ring::init(&mut rr.0, LINE, 4)
            .unwrap()
            .split();

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..COUNT {
                    let mut buf = loop {
                        match pool.alloc::<Msg>() {
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
                    // SAFETY: the desc was consumed into the
                    // ring by the producer and read after the
                    // commit -> reserve handoff (happens-
                    // before), and each is taken back exactly once.
                    let msg = unsafe { reg.to_slot::<Msg>(desc) }.unwrap();
                    assert_eq!(msg.seq, i);
                    msg.free();
                }
            });
        });
    }
}
