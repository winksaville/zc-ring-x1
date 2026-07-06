//! Descriptors and the per-process pool registry — the
//! resolution half of the messaging layer: a [`Desc`] names a
//! buffer as `(pool id, buffer index)` so it can ride any
//! queue as an ordinary POD message, and the [`PoolRegistry`]
//! turns descriptors back into owned guards (see the design
//! doc's "Descriptor and registry design (0.7.0)").
//!
//! - [`PoolRegistry::into_desc`] (safe) consumes a guard into
//!   a descriptor; ownership travels on in the descriptor.
//! - [`PoolRegistry::resolve`] (unsafe) validates a received
//!   descriptor and mints the guard back; `unsafe` covers
//!   only what validation cannot check — ownership
//!   uniqueness.
//! - Fixed capacity, no allocation, no unregister: pool ids
//!   are registry slot indices and never dangle.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::pool::PoolResolver;
use crate::{BufSlot, type_fits};

/// A buffer's travel form: names its pool and buffer index so
/// any queue can carry it as an ordinary message.
///
/// - Plain data on purpose: `FromBytes` means a receiver
///   mints one from shared bytes anyway, so ownership
///   discipline lives in [`PoolRegistry::resolve`]'s
///   contract, not in this type.
/// - Fields are raw `u32`s (not [`PoolId`]) because the wire
///   form is untrusted by definition; validation happens at
///   resolve.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Desc {
    /// The registry slot of the buffer's pool.
    pub pool_id: u32,
    /// The buffer's index within that pool.
    pub buf_idx: u32,
}

/// A registered pool's identity within one process's
/// [`PoolRegistry`] — the value [`register`](PoolRegistry::register)
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
    /// `into_desc`: the guard's pool is not the id's entry.
    WrongPool,
    /// `resolve`: buffer index out of range for the pool.
    BadIndex,
    /// `resolve`: `T`'s geometry does not fit the pool's
    /// buffers. An `Err`, not the `alloc`-style panic: the
    /// descriptor selects which pool gets compared, and
    /// untrusted input must not select a panic.
    BadType,
}

/// Per-process table mapping pool ids to this process's
/// [`PoolResolver`] view of each pool.
///
/// - Fixed capacity `N` (const generic): no_std, zero
///   allocation.
/// - `register` assigns the next slot index as the pool's id;
///   there is no unregister, so ids never dangle.
/// - Registration (`&mut self`) is setup-phase; `into_desc` /
///   `resolve` take `&self`, so one registry is shared by
///   reference across threads.
pub struct PoolRegistry<'a, const N: usize> {
    /// Registered views, dense from slot 0.
    resolvers: [Option<PoolResolver<'a>>; N],
    /// Number of registered pools (next id to assign).
    len: usize,
}

impl<'a, const N: usize> PoolRegistry<'a, N> {
    /// An empty registry.
    pub const fn new() -> Self {
        Self {
            resolvers: [None; N],
            len: 0,
        }
    }

    /// Register a pool's resolver view; returns the assigned
    /// [`PoolId`] (the next slot index), or
    /// [`RegistryError::Full`].
    pub fn register(&mut self, resolver: PoolResolver<'a>) -> Result<PoolId, RegistryError> {
        if self.len == N {
            return Err(RegistryError::Full);
        }
        let id = self.len as u32;
        self.resolvers[self.len] = Some(resolver);
        self.len += 1;
        Ok(PoolId(id))
    }

    /// Consume a guard into its descriptor; ownership travels
    /// on in the [`Desc`] (the usage model's "in-flight"
    /// state).
    ///
    /// - O(1): checks the guard's pool identity (header
    ///   address) against the id's entry, catching id/pool
    ///   mispairing.
    /// - The error side hands the guard back, so a miss
    ///   cannot leak the buffer.
    pub fn into_desc<T>(
        &self,
        pool_id: PoolId,
        slot: BufSlot<'a, T>,
    ) -> Result<Desc, (BufSlot<'a, T>, RegistryError)> {
        let Some(resolver) = self.get(pool_id.0) else {
            return Err((slot, RegistryError::UnknownPoolId));
        };
        if !core::ptr::eq(resolver.header_ptr(), slot.header_ptr()) {
            return Err((slot, RegistryError::WrongPool));
        }
        Ok(Desc {
            pool_id: pool_id.0,
            buf_idx: slot.idx(),
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
    /// Validation cannot check ownership; the caller promises:
    ///
    /// - `desc` came from [`into_desc`](Self::into_desc) (or
    ///   an equivalent consumed guard) — it is not invented.
    /// - It arrived over a channel establishing happens-before
    ///   with the sender's writes (a ring commit → reserve
    ///   qualifies).
    /// - It is resolved exactly once — a second resolve mints
    ///   a second guard aliasing the same `&mut T`.
    pub unsafe fn resolve<T>(&self, desc: Desc) -> Result<BufSlot<'a, T>, RegistryError>
    where
        T: FromBytes + IntoBytes + KnownLayout,
    {
        let Some(resolver) = self.get(desc.pool_id) else {
            return Err(RegistryError::UnknownPoolId);
        };
        if desc.buf_idx >= resolver.buf_count() {
            return Err(RegistryError::BadIndex);
        }
        if !type_fits::<T>(resolver.buf_size()) {
            return Err(RegistryError::BadType);
        }
        // SAFETY: index and T geometry validated above;
        // ownership uniqueness and ordering are the caller's
        // contract (this fn's # Safety).
        Ok(unsafe { resolver.slot_from_idx(desc.buf_idx) })
    }

    /// Number of registered pools.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when no pool is registered.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The resolver registered under raw id `id`, if any.
    fn get(&self, id: u32) -> Option<&PoolResolver<'a>> {
        self.resolvers.get(id as usize)?.as_ref()
    }
}

impl<const N: usize> Default for PoolRegistry<'_, N> {
    /// Same as [`PoolRegistry::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CACHE_LINE_SIZE, Exhausted, Pool, PoolHeader, Ring};
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
        let id = reg.register(pool.resolver()).unwrap();
        assert_eq!(id.as_u32(), 0);
        assert_eq!(reg.len(), 1);

        let mut slot = pool.alloc::<Msg>().unwrap();
        slot.seq = 42;
        let desc = reg.into_desc(id, slot).map_err(|(_, e)| e).unwrap();
        assert_eq!(
            desc,
            Desc {
                pool_id: 0,
                buf_idx: 0
            }
        );

        // SAFETY: desc came from into_desc on this thread and
        // is resolved exactly once.
        let got = unsafe { reg.resolve::<Msg>(desc) }.unwrap();
        assert_eq!(got.seq, 42);
        got.free();

        // The freed buffer is back on the free-stack: all 4
        // allocate again, then exhaustion.
        let slots: [_; 4] = core::array::from_fn(|_| pool.alloc::<Msg>().unwrap());
        assert_eq!(pool.alloc::<Msg>().err().unwrap(), Exhausted);
        slots.into_iter().for_each(BufSlot::free);
    }

    #[test]
    fn into_desc_checks_pool_identity() {
        let mut ra = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut rb = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool_a = Pool::init(&mut ra.0, LINE, 4).unwrap();
        let pool_b = Pool::init(&mut rb.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<2>::new();
        let id_a = reg.register(pool_a.resolver()).unwrap();
        let id_b = reg.register(pool_b.resolver()).unwrap();

        // Mispaired id: rejected, and the guard comes back
        // usable — no leak.
        let slot = pool_a.alloc::<Msg>().unwrap();
        let (slot, err) = reg.into_desc(id_b, slot).unwrap_err();
        assert_eq!(err, RegistryError::WrongPool);
        let desc = reg.into_desc(id_a, slot).map_err(|(_, e)| e).unwrap();
        // SAFETY: desc came from into_desc, resolved once.
        unsafe { reg.resolve::<Msg>(desc) }.unwrap().free();
    }

    #[test]
    fn into_desc_unknown_pool_id() {
        let mut ra = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut rb = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let mut pool_a = Pool::init(&mut ra.0, LINE, 4).unwrap();
        let pool_b = Pool::init(&mut rb.0, LINE, 4).unwrap();
        // An id minted by a bigger registry has no entry in a
        // smaller one.
        let mut big = PoolRegistry::<2>::new();
        big.register(pool_a.resolver()).unwrap();
        let id_b = big.register(pool_b.resolver()).unwrap();
        let mut small = PoolRegistry::<1>::new();
        small.register(pool_a.resolver()).unwrap();

        let slot = pool_a.alloc::<Msg>().unwrap();
        let (slot, err) = small.into_desc(id_b, slot).unwrap_err();
        assert_eq!(err, RegistryError::UnknownPoolId);
        slot.free();
    }

    #[test]
    fn resolve_rejects_hostile_descs() {
        let mut pr = Region::<POOL_BYTES>([0; POOL_BYTES]);
        let pool = Pool::init(&mut pr.0, LINE, 4).unwrap();
        let mut reg = PoolRegistry::<1>::new();
        reg.register(pool.resolver()).unwrap();

        // SAFETY: every resolve here must fail validation and
        // mint nothing; no ownership is claimed.
        unsafe {
            let bad_pool = Desc {
                pool_id: 7,
                buf_idx: 0,
            };
            assert_eq!(
                reg.resolve::<Msg>(bad_pool).err().unwrap(),
                RegistryError::UnknownPoolId
            );
            let bad_idx = Desc {
                pool_id: 0,
                buf_idx: 4,
            };
            assert_eq!(
                reg.resolve::<Msg>(bad_idx).err().unwrap(),
                RegistryError::BadIndex
            );
            let ok_target = Desc {
                pool_id: 0,
                buf_idx: 0,
            };
            // T bigger than the pool's one-line buffers.
            assert_eq!(
                reg.resolve::<[u8; 2 * CACHE_LINE_SIZE]>(ok_target)
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
        reg.register(pool.resolver()).unwrap();
        assert_eq!(
            reg.register(pool.resolver()).unwrap_err(),
            RegistryError::Full
        );
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
        let id = reg.register(pool.resolver()).unwrap();
        let reg = &reg;
        let (mut producer, mut consumer) = Ring::init(&mut rr.0, LINE, 4).unwrap().split();

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
                    let desc = reg.into_desc(id, buf).map_err(|(_, e)| e).unwrap();
                    let mut slot = producer.reserve_slot_spin::<Desc>();
                    *slot = desc;
                    slot.commit();
                }
            });
            s.spawn(move || {
                for i in 0..COUNT {
                    let desc = {
                        let slot = consumer.reserve_slot_spin::<Desc>();
                        let desc = *slot;
                        slot.release();
                        desc
                    };
                    // SAFETY: the desc was consumed into the
                    // ring by the producer and read after the
                    // commit -> reserve handoff (happens-
                    // before); each is resolved exactly once.
                    let msg = unsafe { reg.resolve::<Msg>(desc) }.unwrap();
                    assert_eq!(msg.seq, i);
                    msg.free();
                }
            });
        });
    }
}
