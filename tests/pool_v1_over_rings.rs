//! Messages of three types from multi-stack pools (`pool::v1`), carried as descriptors through
//! every ring, SPSC v0 to v3 and MPSC v0 to v2, and dispatched by type-tag on receipt.
//!
//! - A ring carries a `Desc`, plain data, so any ring should carry a multi-stack pool's messages
//!   unchanged. Each test here runs one ring as it ships.
//! - Every message starts with a type-tag, a number saying which [`Kind`] of message it is. The
//!   consumer knows no message's kind up front: it takes each descriptor back as bytes with
//!   `to_slot_bytes`, decodes the type-tag, and turns the bytes into the message's type with
//!   `into_typed`.
//! - An SPSC ring runs one producer. An MPSC ring runs two, each with its own pool, since a pool
//!   has one allocator, both pools in one registry, and the consumer checks each producer's
//!   messages arrive in order.
//! - The pools' stacks hold two buffers each, so producers wait on exhausted stacks and the
//!   consumer's frees recycle every buffer many times.

use zc_ring_x1::pool::v1::{Pool, PoolView, StackGeometry, region_size};
use zc_ring_x1::{CACHE_LINE_SIZE, Desc, PoolId, PoolRegistry, policy};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Messages each producer sends.
const COUNT: u64 = if cfg!(miri) { 60 } else { 3_000 };

/// One cache line, the rings' slot size and the smallest stack's buffer size.
const LINE: u32 = CACHE_LINE_SIZE as u32;

/// Ring depth, the slots per ring or per segment.
const DEPTH: u32 = 4;

/// Segments per segmented ring.
const SEGMENTS: u32 = 2;

/// A producer's pool: 64-, 128-, and 512-byte stacks, two buffers each, one stack per message
/// type.
const STACKS: [StackGeometry; 3] = [
    StackGeometry::new(LINE, 2),
    StackGeometry::new(2 * LINE, 2),
    StackGeometry::new(8 * LINE, 2),
];

/// Which kind of message a buffer holds, each variant's value its type-tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
enum Kind {
    /// A [`Ping`].
    Ping = 1,
    /// A [`Text`].
    Text = 2,
    /// A [`Blob`].
    Blob = 3,
}

impl Kind {
    /// Every kind, for decoding a type-tag.
    const ALL: [Kind; 3] = [Kind::Ping, Kind::Text, Kind::Blob];

    /// The type-tag a message of this kind starts with.
    const fn type_tag(self) -> u64 {
        self as u64
    }
}

impl TryFrom<u64> for Kind {
    type Error = u64;

    /// Decode a type-tag, or hand back one that names no kind.
    fn try_from(type_tag: u64) -> Result<Self, u64> {
        Kind::ALL
            .into_iter()
            .find(|kind| kind.type_tag() == type_tag)
            .ok_or(type_tag)
    }
}

/// The words every message starts with: its type-tag, which producer, and its sequence number.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Head {
    type_tag: u64,
    from: u64,
    seq: u64,
}

/// A small message, 24 bytes, served by the 64-byte stack.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Ping {
    head: Head,
}

/// A mid-sized message, 112 bytes, served by the 128-byte stack.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Text {
    head: Head,
    len: u64,
    bytes: [u8; 80],
}

/// A big message, 400 bytes, served by the 512-byte stack.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Blob {
    head: Head,
    words: [u64; 47],
}

/// The kind of message producer `from` sends as message `seq`: a fixed mix, so the consumer
/// knows what to expect.
fn kind_of(from: u64, seq: u64) -> Kind {
    Kind::ALL[((seq * 7 + from) % 3) as usize]
}

/// The payload word a [`Blob`] ends with, derived from its head so the consumer can check it.
fn blob_word(from: u64, seq: u64) -> u64 {
    (seq << 8) ^ from ^ 0xb10b
}

/// A ring's sending end, whatever the ring.
trait DescTx: Send {
    /// Send one descriptor, spinning while the ring is full.
    fn send(&mut self, desc: Desc);
}

/// A ring's receiving end, whatever the ring.
trait DescRx: Send {
    /// Receive one descriptor, spinning while the ring is empty.
    fn recv(&mut self) -> Desc;
}

/// [`DescTx`] and [`DescRx`] for an SPSC ring version: reserve, write, commit, and reserve,
/// read, release.
macro_rules! spsc_ends {
    ($v:ident) => {
        impl DescTx for zc_ring_x1::spsc::$v::Producer<'_> {
            /// Reserve a slot, write the descriptor, commit.
            fn send(&mut self, desc: Desc) {
                let mut slot = self.reserve_slot_with::<Desc>(policy::spin).unwrap();
                *slot = desc;
                slot.commit();
            }
        }

        impl DescRx for zc_ring_x1::spsc::$v::Consumer<'_> {
            /// Reserve a slot, copy the descriptor out, release.
            fn recv(&mut self) -> Desc {
                let slot = self.reserve_slot_with::<Desc>(policy::spin).unwrap();
                let desc = *slot;
                slot.release();
                desc
            }
        }
    };
}

/// [`DescTx`] and [`DescRx`] for an MPSC ring version: a closure send, and reserve, read,
/// release.
macro_rules! mpsc_ends {
    ($v:ident) => {
        impl DescTx for zc_ring_x1::mpsc::$v::MpscProducer<'_> {
            /// Claim a slot and fill it with the descriptor.
            fn send(&mut self, desc: Desc) {
                self.send_with::<Desc>(policy::spin, |slot| *slot = desc)
                    .unwrap();
            }
        }

        impl DescRx for zc_ring_x1::mpsc::$v::MpscConsumer<'_> {
            /// Reserve a slot, copy the descriptor out, release.
            fn recv(&mut self) -> Desc {
                let slot = self.reserve_slot_with::<Desc>(policy::spin).unwrap();
                let desc = *slot;
                slot.release();
                desc
            }
        }
    };
}

spsc_ends!(v0);
spsc_ends!(v1);
spsc_ends!(v2);
spsc_ends!(v3);
mpsc_ends!(v0);
mpsc_ends!(v1);
mpsc_ends!(v2);

/// One cache line of backing store, so a `Vec` of them is a line-aligned region of any length.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C, align(64))]
struct Line([u8; CACHE_LINE_SIZE]);

/// A zeroed, line-aligned heap region of at least `bytes` bytes.
fn region(bytes: u64) -> Vec<Line> {
    let lines = bytes.div_ceil(CACHE_LINE_SIZE as u64) as usize;
    (0..lines).map(|_| Line([0; CACHE_LINE_SIZE])).collect()
}

/// The registry every test uses: up to two multi-stack pools of three stacks.
type Registry<'a> = PoolRegistry<'a, 2, PoolView<'a, 3>>;

/// Send producer `from`'s [`COUNT`] messages, each allocated from `pool`, filled, turned into a
/// descriptor, and sent through `tx`.
fn produce(
    pool: &mut Pool<'_, 3>,
    reg: &Registry<'_>,
    id: PoolId,
    from: u64,
    tx: &mut impl DescTx,
) {
    for seq in 0..COUNT {
        let kind = kind_of(from, seq);
        let head = Head {
            type_tag: kind.type_tag(),
            from,
            seq,
        };
        // A stack may be empty until the consumer frees, so wait for a buffer.
        let desc = match kind {
            Kind::Ping => {
                let mut msg = pool.alloc_with::<Ping>(policy::spin).unwrap();
                msg.head = head;
                reg.to_desc(id, msg).map_err(|(_, e)| e)
            }
            Kind::Text => {
                let mut msg = pool.alloc_with::<Text>(policy::spin).unwrap();
                let text = format!("{from}:{seq}");
                msg.len = text.len() as u64;
                msg.bytes[..text.len()].copy_from_slice(text.as_bytes());
                msg.head = head;
                reg.to_desc(id, msg).map_err(|(_, e)| e)
            }
            Kind::Blob => {
                let mut msg = pool.alloc_with::<Blob>(policy::spin).unwrap();
                msg.words[46] = blob_word(from, seq);
                msg.head = head;
                reg.to_desc(id, msg).map_err(|(_, e)| e)
            }
        };
        tx.send(desc.unwrap());
    }
}

/// Receive every producer's messages through `rx`, each taken back as bytes, dispatched by its
/// type-tag, checked, and freed. Each producer's messages must arrive in its own order.
fn consume(reg: &Registry<'_>, rx: &mut impl DescRx, producers: u64) {
    let mut next = vec![0u64; producers as usize];
    for _ in 0..COUNT * producers {
        let desc = rx.recv();
        // SAFETY: each descriptor came from to_desc, arrived through the ring's commit ->
        // reserve handoff, and is taken back exactly once.
        let bytes = unsafe { reg.to_slot_bytes(desc) }.unwrap();
        let head = Head::read_from_prefix(&bytes[..]).unwrap().0;
        let (from, seq) = (head.from, head.seq);
        assert_eq!(seq, next[from as usize], "producer {from}'s order");
        next[from as usize] += 1;
        let kind = Kind::try_from(head.type_tag)
            .unwrap_or_else(|type_tag| panic!("message {from}:{seq}: unknown type-tag {type_tag}"));
        assert_eq!(kind, kind_of(from, seq), "message {from}:{seq}'s kind");
        match kind {
            Kind::Ping => bytes
                .into_typed::<Ping>()
                .map_err(|_| "ping")
                .unwrap()
                .free(),
            Kind::Text => {
                let msg = bytes.into_typed::<Text>().map_err(|_| "text").unwrap();
                let text = &msg.bytes[..msg.len as usize];
                assert_eq!(text, format!("{from}:{seq}").as_bytes());
                msg.free();
            }
            Kind::Blob => {
                let msg = bytes.into_typed::<Blob>().map_err(|_| "blob").unwrap();
                assert_eq!(msg.words[46], blob_word(from, seq));
                msg.free();
            }
        }
    }
}

/// Run `txs.len()` producers, each with its own multi-stack pool registered in one registry,
/// against one consumer on `rx`, each on its own thread. Every buffer must come back: each
/// pool then serves each stack's count again.
fn run(txs: Vec<impl DescTx>, mut rx: impl DescRx) {
    let producers = txs.len() as u64;
    let mut regions: Vec<_> = (0..producers)
        .map(|_| region(region_size(STACKS)))
        .collect();
    let mut pools: Vec<_> = regions
        .iter_mut()
        .map(|r| Pool::init(r.as_mut_bytes(), STACKS).unwrap())
        .collect();
    let mut reg = Registry::new();
    let ids: Vec<_> = pools
        .iter()
        .map(|p| reg.register(p.view()).unwrap())
        .collect();
    let reg = &reg;

    std::thread::scope(|s| {
        for (from, ((pool, id), mut tx)) in pools.iter_mut().zip(ids).zip(txs).enumerate() {
            s.spawn(move || produce(pool, reg, id, from as u64, &mut tx));
        }
        s.spawn(move || consume(reg, &mut rx, producers));
    });

    for pool in &mut pools {
        for stack in pool.stacks() {
            let bufs: Vec<_> = (0..stack.buf_count)
                .map(|_| pool.alloc_bytes(stack.buf_size as usize).unwrap())
                .collect();
            assert!(bufs.iter().all(|b| b.len() == stack.buf_size as usize));
            bufs.into_iter().for_each(|b| b.free());
        }
    }
}

/// A v0 pool region holding `SEGMENTS` segments of `segment` bytes, a segmented ring's size.
fn segment_pool_region(segment: u64) -> Vec<Line> {
    region(size_of::<zc_ring_x1::pool::v0::PoolHeader>() as u64 + segment * SEGMENTS as u64)
}

#[test]
fn spsc_v0_carries_mixed_messages() {
    let bytes = size_of::<zc_ring_x1::spsc::v0::Header>() as u64 + (LINE * DEPTH) as u64;
    let mut store = region(bytes);
    let ring = zc_ring_x1::spsc::v0::Ring::init(store.as_mut_bytes(), LINE, DEPTH).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx], rx);
}

#[test]
fn spsc_v1_carries_mixed_messages() {
    let mut store = region(zc_ring_x1::spsc::v1::region_size(LINE, DEPTH));
    let ring = zc_ring_x1::spsc::v1::Ring::init(store.as_mut_bytes(), LINE, DEPTH).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx], rx);
}

#[test]
fn spsc_v2_carries_mixed_messages() {
    let mut store = region(zc_ring_x1::spsc::v2::region_size(LINE, DEPTH));
    let ring = zc_ring_x1::spsc::v2::Ring::init(store.as_mut_bytes(), LINE, DEPTH).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx], rx);
}

#[test]
fn spsc_v3_carries_mixed_messages() {
    // The ring's segments come from a v0 pool, as v3 requires, and its messages from v1 pools.
    let segment = zc_ring_x1::spsc::v3::segment_size(LINE, DEPTH);
    let mut store = segment_pool_region(segment);
    let mut segs =
        zc_ring_x1::pool::v0::Pool::init(store.as_mut_bytes(), segment as u32, SEGMENTS).unwrap();
    let ring = zc_ring_x1::spsc::v3::Ring::init(&mut segs, LINE, DEPTH, SEGMENTS).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx], rx);
}

#[test]
fn mpsc_v0_carries_mixed_messages() {
    let mut store = region(zc_ring_x1::mpsc::v0::mpsc_region_size(LINE, DEPTH));
    let ring = zc_ring_x1::mpsc::v0::MpscRing::init(store.as_mut_bytes(), LINE, DEPTH).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx.clone(), tx], rx);
}

#[test]
fn mpsc_v1_carries_mixed_messages() {
    let mut store = region(zc_ring_x1::mpsc::v1::mpsc_region_size(LINE, DEPTH));
    let ring = zc_ring_x1::mpsc::v1::MpscRing::init(store.as_mut_bytes(), LINE, DEPTH).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx.clone(), tx], rx);
}

#[test]
fn mpsc_v2_carries_mixed_messages() {
    // The ring's segments come from a v0 pool, as v2 requires, and its messages from v1 pools.
    let segment = zc_ring_x1::mpsc::v2::segment_size(LINE, DEPTH);
    let mut store = segment_pool_region(segment);
    let mut segs =
        zc_ring_x1::pool::v0::Pool::init(store.as_mut_bytes(), segment as u32, SEGMENTS).unwrap();
    let ring = zc_ring_x1::mpsc::v2::MpscRing::init(&mut segs, LINE, DEPTH, SEGMENTS).unwrap();
    let (tx, rx) = ring.split();
    run(vec![tx.clone(), tx], rx);
}
