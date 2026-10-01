//! zcr-test-ipm: one message from one process to another over an
//! `spsc::v4` ring in shared memory, the proof that the crate
//! does what it is for.
//!
//! - `zcr-test-ipm consumer` creates the region file, builds a
//!   pool and a ring in it, claims the consumer role, prints
//!   `ready`, and waits for one message.
//! - `zcr-test-ipm producer`, started after, maps the same file,
//!   attaches the pool and the ring, claims the producer role, and
//!   sends a random value with a checksum of it.
//! - The consumer checks the checksum, prints what it received,
//!   and exits 0, or non-zero on a bad checksum or a timeout.
//! - Everything is hard-coded, the path, the geometry, the ring's
//!   first segment, and the holder ids, since the one aim is to
//!   show a message crossing a process boundary intact.
//! - The MPSC mode, over an `mpsc::v3` ring in its own region file,
//!   shows producers and consumers coming and going across
//!   processes:
//!   - `zcr-test-ipm mpsc-consumer new|join <messages>` creates the
//!     region and the ring (`new`) or attaches (`join`), claims the
//!     consumer, prints `ready`, reads `messages` messages, sleeping
//!     on an empty ring, checks each checksum and each producer's
//!     order, releases the role, and prints each producer's first
//!     and last sequence number.
//!   - `zcr-test-ipm mpsc-producer <id> <count>` attaches, claims a
//!     producer, sends `count` messages numbered from 0, sleeping on
//!     a full ring, and releases.
//!   - `zcr-test-ipm mpsc-release` attaches and releases the ring,
//!     which fails while any role is held.

#[cfg(target_os = "linux")]
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The region file, in `/dev/shm` so the mapping is memory.
#[cfg(target_os = "linux")]
const PATH: &str = "/dev/shm/zcr-test-ipm-ring";

/// Slot size: one cache line.
#[cfg(target_os = "linux")]
const SLOT: u32 = 64;

/// Slots per segment.
#[cfg(target_os = "linux")]
const DEPTH: u32 = 8;

/// Segments in the ring, each a buffer of the pool, which holds
/// exactly these.
#[cfg(target_os = "linux")]
const SEGMENTS: u32 = 2;

/// The pool buffer index of the ring's segment 0. A fresh pool
/// hands out its buffers in a fixed order, so the consumer asserts
/// it and the producer attaches by it, and `Ring::attach` refuses
/// a region where it names no ring.
#[cfg(target_os = "linux")]
const FIRST_SEGMENT: u32 = 0;

/// The consumer's holder id.
#[cfg(target_os = "linux")]
const CONSUMER_ID: u32 = 1;

/// The producer's holder id.
#[cfg(target_os = "linux")]
const PRODUCER_ID: u32 = 2;

/// How long the consumer waits for the message.
#[cfg(target_os = "linux")]
const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The message: a value and a checksum of it, so a torn or
/// garbled message is caught.
#[cfg(target_os = "linux")]
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct Msg {
    value: u64,
    checksum: u64,
}

/// FNV-1a over the value's bytes.
#[cfg(target_os = "linux")]
fn checksum(value: u64) -> u64 {
    value
        .to_le_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// A value nobody could have left in the region: the time and the
/// pid through one xorshift round.
#[cfg(target_os = "linux")]
fn random() -> u64 {
    // A clock before 1970 leaves the pid alone to vary it.
    let nanos = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as u64,
        Err(_) => 0,
    };
    let mut x = (nanos ^ ((std::process::id() as u64) << 32)) | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// Bytes the region holds: the pool's header and its segments.
#[cfg(target_os = "linux")]
fn region_len() -> usize {
    size_of::<zc_ring_x1::PoolHeader>()
        + SEGMENTS as usize * zc_ring_x1::spsc::v4::segment_size(SLOT, DEPTH) as usize
}

/// Map the SPSC region file, as [`map_at`] does.
#[cfg(target_os = "linux")]
fn map(create: bool) -> Result<*mut u8, String> {
    map_at(PATH, region_len(), create)
}

/// Map the region file at `path`, `len` bytes, shared, creating it
/// at its size when `create`, else opening the one a consumer
/// made.
///
/// - The mapping is never unmapped: it lives until the process
///   exits, so the pool and ring over it may borrow it for
///   `'static`.
#[cfg(target_os = "linux")]
fn map_at(path: &str, len: usize, create: bool) -> Result<*mut u8, String> {
    use std::os::fd::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(create)
        .open(path)
        .map_err(|e| format!("open {path}: {e}"))?;
    if create {
        file.set_len(len as u64)
            .map_err(|e| format!("size {path}: {e}"))?;
    } else if (file.metadata().map_err(|e| e.to_string())?.len() as usize) < len {
        return Err(format!("{path} is smaller than the region"));
    }
    // SAFETY: a fresh mapping of `len` bytes of an open file the
    // process may read and write, shared so the other process
    // sees the same memory. Closing the file keeps the mapping.
    let base = unsafe {
        libc::mmap(
            core::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return Err(format!("mmap {path}: {}", std::io::Error::last_os_error()));
    }
    Ok(base as *mut u8)
}

/// Build the ring, claim the consumer, and wait for one message.
#[cfg(target_os = "linux")]
fn consumer() -> Result<(), String> {
    use zc_ring_x1::{Pool, spsc::v4::Ring};
    let base = map(true)?;
    // SAFETY: the mapping is `region_len` bytes, page-aligned,
    // never unmapped, and this process's only view of it until the
    // producer attaches in another.
    let region = unsafe { core::slice::from_raw_parts_mut(base, region_len()) };
    let seg = zc_ring_x1::spsc::v4::segment_size(SLOT, DEPTH) as u32;
    let mut pool = Pool::init(region, seg, SEGMENTS).map_err(|e| format!("pool: {e:?}"))?;
    let ring = Ring::init(&mut pool, SLOT, DEPTH, SEGMENTS).map_err(|e| format!("ring: {e:?}"))?;
    if ring.first_segment() != FIRST_SEGMENT {
        return Err(format!(
            "ring's first segment is {}, not {FIRST_SEGMENT}",
            ring.first_segment()
        ));
    }
    let mut cons = ring
        .claim_consumer(CONSUMER_ID)
        .map_err(|e| format!("claim consumer: {e:?}"))?;
    println!("ready");
    use std::io::Write;
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + WAIT;
    let msg = cons
        .reserve_slot_with::<Msg>(|_| {
            std::thread::sleep(std::time::Duration::from_millis(1));
            std::time::Instant::now() < deadline
        })
        .map_err(|_| format!("no message within {WAIT:?}"))?;
    let (value, sum) = (msg.value, msg.checksum);
    msg.release();
    if sum != checksum(value) {
        return Err(format!(
            "received value={value:#018x} checksum={sum:#018x}, bad, want {:#018x}",
            checksum(value)
        ));
    }
    println!("received value={value:#018x} checksum={sum:#018x} ok");
    Ok(())
}

/// Attach the consumer's ring, claim the producer, and send one
/// message.
#[cfg(target_os = "linux")]
fn producer() -> Result<(), String> {
    use zc_ring_x1::{Pool, spsc::v4::Ring};
    let base = map(false)?;
    // SAFETY: the mapping is `region_len` bytes, shared and
    // writable, never unmapped, and this handle never allocates,
    // so the consumer's pool keeps its one allocator.
    let pool = unsafe { Pool::attach(base, region_len()) }.map_err(|e| format!("pool: {e:?}"))?;
    // SAFETY: FIRST_SEGMENT is the consumer's ring's segment 0 in
    // this pool, asserted there, and its segments are still the
    // ring's: the consumer allocates nothing else.
    let ring = unsafe { Ring::attach(&pool, FIRST_SEGMENT) }.map_err(|e| format!("ring: {e:?}"))?;
    let mut prod = ring
        .claim_producer(PRODUCER_ID)
        .map_err(|e| format!("claim producer: {e:?}"))?;
    let value = random();
    let mut slot = prod
        .reserve_slot_with::<Msg>(|_| false)
        .map_err(|_| "ring full".to_string())?;
    slot.value = value;
    slot.checksum = checksum(value);
    slot.commit();
    prod.release();
    println!(
        "sent value={value:#018x} checksum={:#018x}",
        checksum(value)
    );
    Ok(())
}

/// The MPSC region file.
#[cfg(target_os = "linux")]
const MPSC_PATH: &str = "/dev/shm/zcr-test-ipm-mpsc";

/// Segments in the MPSC ring, each a buffer of the pool, which
/// holds exactly these.
#[cfg(target_os = "linux")]
const MPSC_SEGMENTS: u32 = 3;

/// The MPSC ring: segments switched, and sleeps on a futex whose
/// timeout turns a dead peer into a poll.
#[cfg(target_os = "linux")]
type MpscRing = zc_ring_x1::mpsc::v3::MpscRing<
    'static,
    zc_ring_x1::mpsc::v3::Multi,
    zc_ring_x1::wake::Futex<10>,
>;

/// An MPSC message: its producer, its number in that producer's
/// stream, a value, and a checksum over all three.
#[cfg(target_os = "linux")]
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
struct MpscMsg {
    producer: u64,
    seq: u64,
    value: u64,
    checksum: u64,
}

/// The checksum of an MPSC message's three fields.
#[cfg(target_os = "linux")]
fn mpsc_checksum(producer: u64, seq: u64, value: u64) -> u64 {
    checksum(producer ^ checksum(seq ^ checksum(value)))
}

/// Bytes the MPSC region holds: the pool's header and its segments.
#[cfg(target_os = "linux")]
fn mpsc_region_len() -> usize {
    size_of::<zc_ring_x1::PoolHeader>()
        + MPSC_SEGMENTS as usize * zc_ring_x1::mpsc::v3::segment_size(SLOT, DEPTH) as usize
}

/// Attach the MPSC region's pool and ring.
#[cfg(target_os = "linux")]
fn mpsc_attach() -> Result<MpscRing, String> {
    let base = map_at(MPSC_PATH, mpsc_region_len(), false)?;
    // SAFETY: the mapping is the region's length, shared and
    // writable, never unmapped, and this handle never allocates.
    let pool = unsafe { zc_ring_x1::Pool::attach(base, mpsc_region_len()) }
        .map_err(|e| format!("pool: {e:?}"))?;
    // The pool handle lives as long as the process, as the
    // mapping does.
    let pool = Box::leak(Box::new(pool));
    // SAFETY: FIRST_SEGMENT is the ring's segment 0 in this pool,
    // asserted by the consumer that made it, and its segments are
    // the ring's until a release, after which the magic is gone.
    unsafe { MpscRing::attach(pool, FIRST_SEGMENT) }.map_err(|e| format!("ring: {e:?}"))
}

/// Create (`new`) or attach (`join`) the MPSC ring, claim the
/// consumer, read `messages` messages, and release.
#[cfg(target_os = "linux")]
fn mpsc_consumer(how: &str, messages: u64) -> Result<(), String> {
    let ring = match how {
        "new" => {
            let base = map_at(MPSC_PATH, mpsc_region_len(), true)?;
            // SAFETY: the mapping is the region's length,
            // page-aligned, never unmapped, and this process's only
            // view of it until another attaches.
            let region = unsafe { core::slice::from_raw_parts_mut(base, mpsc_region_len()) };
            let seg = zc_ring_x1::mpsc::v3::segment_size(SLOT, DEPTH) as u32;
            let pool = zc_ring_x1::Pool::init(region, seg, MPSC_SEGMENTS)
                .map_err(|e| format!("pool: {e:?}"))?;
            let pool = Box::leak(Box::new(pool));
            let ring = MpscRing::init(pool, SLOT, DEPTH, MPSC_SEGMENTS)
                .map_err(|e| format!("ring: {e:?}"))?;
            if ring.first_segment() != FIRST_SEGMENT {
                return Err(format!(
                    "ring's first segment is {}, not {FIRST_SEGMENT}",
                    ring.first_segment()
                ));
            }
            ring
        }
        "join" => mpsc_attach()?,
        _ => return Err("mpsc-consumer takes new or join".to_string()),
    };
    let mut cons = ring
        .claim_consumer()
        .map_err(|e| format!("claim consumer: {e:?}"))?;
    println!("ready");
    use std::io::Write;
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    // Each producer's first and last number seen, in order.
    let mut seen: std::collections::BTreeMap<u64, (u64, u64)> = Default::default();
    let deadline = std::time::Instant::now() + WAIT;
    for n in 0..messages {
        let msg = cons
            .reserve_slot_wait::<MpscMsg>(|_| std::time::Instant::now() < deadline)
            .map_err(|_| format!("message {n} not within {WAIT:?}"))?;
        let (p, seq, value, sum) = (msg.producer, msg.seq, msg.value, msg.checksum);
        msg.release();
        if sum != mpsc_checksum(p, seq, value) {
            return Err(format!("message {n}: bad checksum from producer {p}"));
        }
        match seen.get_mut(&p) {
            Some((_, last)) if seq == *last + 1 => *last = seq,
            Some((_, last)) => {
                return Err(format!("producer {p}: {seq} after {last}"));
            }
            None => {
                seen.insert(p, (seq, seq));
            }
        }
    }
    cons.release();
    for (p, (first, last)) in &seen {
        println!("received producer={p} first={first} last={last}");
    }
    println!("received {messages} ok");
    Ok(())
}

/// Attach the MPSC ring, claim a producer, send `count` messages
/// numbered from 0, and release.
#[cfg(target_os = "linux")]
fn mpsc_producer(id: u64, count: u64) -> Result<(), String> {
    let ring = mpsc_attach()?;
    let prod = ring
        .claim_producer()
        .map_err(|e| format!("claim producer: {e:?}"))?;
    // Each message may wait up to WAIT for room, asleep on the futex from the first full look.
    let sleep_time = zc_ring_x1::microsecs_to_ticks(WAIT.as_micros() as u64);
    for seq in 0..count {
        let value = random() ^ seq;
        prod.send_spin_sleep::<MpscMsg>(zc_ring_x1::Ticks::ZERO, sleep_time, |m| {
            m.producer = id;
            m.seq = seq;
            m.value = value;
            m.checksum = mpsc_checksum(id, seq, value);
        })
        .map_err(|_| format!("message {seq} not sent within {WAIT:?}"))?;
    }
    prod.release();
    println!("sent producer={id} count={count}");
    Ok(())
}

/// Attach the MPSC ring and release it.
#[cfg(target_os = "linux")]
fn mpsc_release() -> Result<(), String> {
    let base = map_at(MPSC_PATH, mpsc_region_len(), false)?;
    // SAFETY: as in mpsc_attach, and the release frees buffers,
    // which any process may do.
    let pool = unsafe { zc_ring_x1::Pool::attach(base, mpsc_region_len()) }
        .map_err(|e| format!("pool: {e:?}"))?;
    // SAFETY: as in mpsc_attach.
    let ring =
        unsafe { MpscRing::attach(&pool, FIRST_SEGMENT) }.map_err(|e| format!("ring: {e:?}"))?;
    ring.release_ring(&pool)
        .map_err(|e| format!("release: {e:?}"))?;
    println!("released");
    Ok(())
}

/// The `n`th argument as a number.
#[cfg(target_os = "linux")]
fn arg_num(n: usize) -> Result<u64, String> {
    std::env::args()
        .nth(n)
        .ok_or_else(|| format!("argument {n} missing"))?
        .parse()
        .map_err(|e| format!("argument {n}: {e}"))
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let role = std::env::args().nth(1);
    let run = match role.as_deref() {
        Some("consumer") => consumer(),
        Some("producer") => producer(),
        Some("mpsc-consumer") => std::env::args()
            .nth(2)
            .ok_or_else(|| "mpsc-consumer takes new or join".to_string())
            .and_then(|how| mpsc_consumer(&how, arg_num(3)?)),
        Some("mpsc-producer") => arg_num(2).and_then(|id| mpsc_producer(id, arg_num(3)?)),
        Some("mpsc-release") => mpsc_release(),
        _ => Err(
            "usage: zcr-test-ipm consumer | producer | mpsc-consumer new|join <messages> \
             | mpsc-producer <id> <count> | mpsc-release"
                .to_string(),
        ),
    };
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zcr-test-ipm: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("zcr-test-ipm: Linux only, it maps /dev/shm");
    std::process::exit(1);
}
