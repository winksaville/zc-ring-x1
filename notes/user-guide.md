# User guide: SPSC v3 and MPSC v2

How to use the two segmented rings, from a pool to a running pair of endpoints, for a reader
who has the crate and not the design note. The design is in
[ring-buffer-design.md](ring-buffer-design.md), the two rings under [SPSC v3: ring of
segments](ring-buffer-design.md#spsc-v3-ring-of-segments) and [MPSC v2: ring of
segments](ring-buffer-design.md#mpsc-v2-ring-of-segments). The two programs this guide walks
through are [examples/guide_spsc_v3.rs](../examples/guide_spsc_v3.rs) and
[examples/guide_mpsc_v2.rs](../examples/guide_mpsc_v2.rs), built by every `cargo test` and run
with `cargo run --release --example guide_spsc_v3` and `guide_mpsc_v2`.

## What the rings are

Both rings move typed messages between threads with no copy at the boundary: the producer writes
through a `&mut T` straight into ring memory, and the consumer reads through a `&T` in place. A
ring is a set of *segments*, each a ring of its own of the same depth, taken from an application
pool at `init`. With a consumer that keeps up the ring lives in one segment, and the others absorb
a producer that runs ahead, so the ring never reports Full until every segment is full.

- **SPSC v3**, `zc_ring_x1::Ring`, the crate's default ring: one producer, one consumer. The
  producer reserves a slot, writes it, and commits. It is the faster of the two where one producer
  is enough.
- **MPSC v2**, `zc_ring_x1::mpsc::v2::MpscRing`, reached by path: any number of producers, one
  consumer. A producer sends with a closure that fills the slot, and the commit happens when the
  closure returns. The crate root's `MpscRing` is still MPSC v1, the single-region ring, so name
  v2 by path.

Both are `no_std`, allocate nothing after `init`, and are in-process only: neither has an
`attach`, since a ring's state spans a pool and its segments. **SPSC v4**, `zc_ring_x1::spsc::v4`,
is SPSC v3 with an `attach`, for a ring shared between processes, in [Joining from another
process](#joining-from-another-process) below. The single-region rings, `spsc::v0` to `v2` and
`mpsc::v0` and `v1`, keep `attach` as well and are outside this guide.

## The message type

A message is any `#[repr(C)]` type that derives the zerocopy traits, `FromBytes`, `IntoBytes`,
`KnownLayout`, and `Immutable`, so that any byte pattern is a valid value and a reference into
ring memory is sound:

```rust
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    seq: u64,
    payload: [u8; 16],
}
```

It must fit the slot body: the slot size less `SLOT_HEADER_BYTES`, which is 16, at an alignment
of at most 16. A 64-byte slot carries up to 48 bytes of message. A type that does not fit panics
at the first reserve or send, not at `init`, so keep the two sizes together in one place.

## Sizing

Three numbers describe a ring, and one function turns them into a pool:

- `slot_size`: bytes per slot, a multiple of `CACHE_LINE_SIZE`, 64. One line is the smallest and
  holds a 48-byte message.
- `seg_capacity`: slots per segment, a power of two from 1 to `MAX_SEG_CAPACITY`. This is the
  depth a single-region ring would have.
- `seg_count`: segments, 1 to `MAX_SEGMENTS`, which is 32. One segment is a single-region ring
  with the segmented fast path. More are the slack for a producer that runs ahead, and every one
  is taken from the pool at `init` and held for the life of the pool, whether it is ever used or
  not.

`segment_size(slot_size, seg_capacity)` is the bytes one segment needs, header included, and the
pool's buffers must be at least that large. SPSC v3's header is one cache line and MPSC v2's is
three, so each module has its own `segment_size`, and a pool sized with the wrong one fails
`init` with `Error::TooSmall`. The pool region is `size_of::<PoolHeader>()`, two lines, plus
`seg_count` buffers of that size, and it must be line-aligned. The examples build one on the heap
from a vector of aligned lines:

```rust
let seg_bytes = segment_size(SLOT, DEPTH);
let region_bytes = size_of::<PoolHeader>() as u64 + seg_bytes * SEGMENTS as u64;
let mut store = vec![Line([0; 64]); region_bytes.div_ceil(64) as usize];
let mut pool = Pool::init(store.as_mut_slice().as_mut_bytes(), seg_bytes as u32, SEGMENTS)?;
```

A static region works the same way, as the README's `Region` type shows. The pool may hold more
buffers than the ring takes, and the same pool may back several rings, or messages of its own.

## Init and split

```rust
let (mut producer, mut consumer) = Ring::init(&mut pool, SLOT, DEPTH, SEGMENTS)?.split();
let (producer, mut consumer) = MpscRing::init(&mut pool, SLOT, DEPTH, SEGMENTS)?.split();
```

`init` takes `seg_count` buffers from the pool, initializes each as an empty segment, and borrows
the pool only for the call. `split` consumes the ring and hands out the endpoints once. For SPSC
v3 that is one `Producer` and one `Consumer`, each used through `&mut self`. For MPSC v2 the
`MpscProducer` is `Clone` and `Sync`, so make one clone per producing thread or share one by
reference, and `send_with` takes `&self`. The consumer is unique in both.

`init` fails with `Error::TooSmall` when the pool's buffers are smaller than `segment_size`,
`Error::Exhausted` when the pool has fewer free buffers than `seg_count`, with the buffers taken
so far returned, `Error::BadSlotSize`, `Error::BadCapacity`, or `Error::BadSegmentCount` for a
geometry outside the ranges above.

## Sending

**SPSC v3** reserves, writes, and commits:

```rust
let mut slot = producer.reserve_slot_with::<Msg>(spin_then_yield)?;
slot.seq = i;
slot.payload = [i as u8; 16];
slot.commit();
```

`reserve_slot_with` returns a `WriteSlot<Msg>`, a guard that is `DerefMut` to the message in
ring memory. `commit` publishes it. Dropping the guard without committing abandons the
reservation, and the next reserve returns the same slot. One reservation is live at a time, since
the guard holds the producer's borrow.

**MPSC v2** sends with a closure:

```rust
producer.send_with::<Msg>(spin_then_yield, |m| {
    m.seq = i;
    m.producer = id;
})?;
```

The closure gets the `&mut Msg`, and the commit is on its return, so there is no guard to forget
and no abandoned state. If the closure panics, the unwind commits a tombstone the consumer skips,
so the ring stays usable. A producer losing a claim race to another producer simply retries, and
the policy is not consulted for that.

## Receiving

The same on both rings:

```rust
let msg = consumer.reserve_slot_with::<Msg>(spin_then_yield)?;
assert_eq!(msg.seq, i);
msg.release();
```

`reserve_slot_with` returns a `ReadSlot<Msg>`, a guard that is `Deref` to the message. `release`
frees the slot for the producer. Dropping the guard without releasing re-delivers the same slot
at the next reserve, so a consumer that wants to peek may drop and reserve again. One reservation
is live at a time.

Messages from one producer arrive in the order it sent them. On MPSC v2, producers interleave in
the order their claims landed, which is not the order their `send_with` calls began.

## Wait policies, Full, and Empty

Every reserve and send takes a wait policy, an `FnMut(u32) -> bool`. After each failed attempt
the ring calls it with the attempts so far, from 0, and tries again if it returns `true`. When it
returns `false` the call gives up with `Full` on the producer side or `Empty` on the consumer's.
The crate never blocks or sleeps on its own, so the policy is where the waiting strategy lives:

- `|_| false` is a single non-blocking probe.
- `zc_ring_x1::policy::spin` spins forever with a CPU hint, the demo's and the tools' policy.
- The examples' `spin_then_yield` spins a hundred times and then yields the thread, never giving
  up. A bounded retry, a deadline, or a sleep are the same shape, a closure at the call site.

```rust
fn spin_then_yield(attempt: u32) -> bool {
    if attempt < 100 {
        core::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
    true
}
```

`Full` means the current segment's next slot is still unread and no other segment is free: every
segment is full. `Empty` means no committed message is waiting. Neither loses anything, and the
same call succeeds later once the other side moves. True blocking, a futex or an async waker,
is a layer above this crate, and the single-region rings' header user line exists for building
one.

## Threads

Both endpoints are `Send`, so each moves to its own thread, and the examples use scoped threads
so the pool outlives the ring without an `Arc`:

```rust
std::thread::scope(|s| {
    s.spawn(move || { /* producer */ });
    s.spawn(move || { /* consumer */ });
});
```

For MPSC v2, clone the producer once per thread inside the scope, and the original stays usable
in the parent, for the counters or for sending. The pool must outlive the endpoints, which the
borrow checker enforces through the ring's lifetime.

## The segment lifecycle

What happens to the segments while the ring runs, the facts a user acts on. The design note's
[Segment lifecycle](ring-buffer-design.md#segment-lifecycle) states them with the mechanics.

- Every segment is taken at `init` and held for the life of the pool region. A segment the
  consumer has drained goes back to the ring's own free set, never to the pool, so the memory
  cost is fixed at `init`.
- The ring lives in one segment at a time, segment 0 to start. With a consumer that keeps up no
  switch ever happens.
- A switch happens only at a full segment. No segment is left part-filled, and the producer takes
  the lowest free one.
- Full is no free segment. The ring waits in place under the policy.
- A segment is given back after the consumer passes its end, so nothing is reclaimed under a
  producer.
- A drained ring is a fresh ring in another segment: one in use, the rest free.

## The counters

Each endpoint has `switches()`, the segment switches it has made, and `segment()`, the segment it
is in. Switches are counted on the switch path only, so a run where the consumer kept up reads
zero, and once the consumer has read everything sent the producer's and the consumer's counts
agree. On MPSC v2 the producer's count is the ring's, shared by every clone. A high count against
the messages moved says the consumer is lagging and the segments are doing their job, and a count
of zero says one segment would have done.

## Limits

- 32 segments at most, and a segment capacity of at most `MAX_SEG_CAPACITY`, 2 to the 24.
- A message of at most `slot_size - 16` bytes, aligned to at most 16, checked at the first
  reserve or send by a panic.
- MPSC v2 needs a 32-bit compare-and-swap, so the `mpsc` module is gated on
  `target_has_atomic = "32"`. SPSC v3 uses loads and stores only.
- No `attach` on SPSC v3 and MPSC v2, so no sharing between processes. SPSC v4 has it, and the
  single-region rings keep theirs.
- One reservation per endpoint at a time, by the guard's borrow.

## Joining from another process

`spsc::v4::Ring` is SPSC v3 with a control block at the front of its segment 0, so a process
that maps the same pool region can find the ring and take a role in it.

```rust
// The process that builds the ring, over a pool it initialized.
let ring = spsc::v4::Ring::init(&mut pool, SLOT, DEPTH, SEGMENTS)?;
let mut consumer = ring.consumer()?;
let first = ring.first_segment(); // hand this to the other process

// The other process, over the same region mapped as it maps it.
let pool = unsafe { Pool::attach(base, len) }?;
let ring = unsafe { spsc::v4::Ring::attach(&pool, first) }?;
let mut producer = ring.producer()?;
```

- There is no `split`: `producer()` and `consumer()` each take their role by a compare-and-swap
  on the control block, from a `Ring` that `init` or `attach` returned, and the second taker of a
  role anywhere, in this process or another, gets `Error::RoleTaken`. Dropping the endpoint
  releases the role. A process that ends without dropping leaves its role held.
- `first_segment` is the pool buffer index of the ring's segment 0, a number the building process
  hands the other by whatever means it has, an argument, a file, or a message.
- `attach` validates the control block and every segment's own header against the pool, so a
  hostile region is an `Err`, and it is `unsafe` for what validation cannot check: that the index
  came from a ring over this pool whose segments are still its own.
- A joined endpoint starts in segment 0 at position 0, as one from `init` does, so join before
  the role has run. A ring already run is not rejoined.
- Everything after the join, sending, receiving, the policies, and the segment lifecycle, is
  SPSC v3's, and the rows for both are in the measurement tools.

## Errors and panics

| Where | What | Meaning |
|---|---|---|
| `Pool::init` | `Error::Misaligned`, `TooSmall`, `BadBufSize`, `BadBufCount` | the region or the geometry is wrong |
| `Ring::init`, `MpscRing::init` | `Error::BadSlotSize`, `BadCapacity`, `BadSegmentCount` | a slot size not a line multiple, a capacity not a power of two or out of range, a segment count out of range |
| `Ring::init`, `MpscRing::init` | `Error::TooSmall` | the pool's buffers do not hold `segment_size` |
| `Ring::init`, `MpscRing::init` | `Error::Exhausted` | the pool had fewer free buffers than segments, and those taken were returned |
| `spsc::v4::Ring::attach` | `Error::BadMagic`, `BadLayoutVersion`, `BadSegment`, `TooSmall`, and the geometry errors | the index names no v4 ring, another layout, a table or a segment header that disagrees with the pool, or a segment the pool's buffers cannot hold |
| `spsc::v4::Ring::producer`, `consumer` | `Error::RoleTaken` | the role is held, in this process or another |
| producer reserve or send | `Full` | the policy gave up with every segment full |
| consumer reserve | `Empty` | the policy gave up with nothing committed |
| first reserve or send | panic | `T` larger than the slot body or aligned beyond 16 |

## The two programs, end to end

[examples/guide_spsc_v3.rs](../examples/guide_spsc_v3.rs) moves a million messages from one
thread to another through four segments of 64 one-line slots, checks their order, and prints the
switches and the segment both sides ended in:

```text
spsc v3: 1000000 messages, 4 segments of 64, 525 switches, ended in segment 2, checksum 127493856
```

[examples/guide_mpsc_v2.rs](../examples/guide_mpsc_v2.rs) runs three producer threads of three
hundred thousand messages each into one consumer, checks each producer's order, and prints the
same:

```text
mpsc v2: 3 producers x 300000 messages, 4 segments of 64, 59 switches, ended in segment 0, per producer [300000, 300000, 300000]
```

The switch counts are the 3900X's on one run and vary with the scheduler. A run that never
switches is the ring working as designed, not a fault.
