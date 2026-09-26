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

/// Map the region file shared, creating it at its size when
/// `create`, else opening the one the consumer made.
///
/// - The mapping is never unmapped: it lives until the process
///   exits, so the pool and ring over it may borrow it for
///   `'static`.
#[cfg(target_os = "linux")]
fn map(create: bool) -> Result<*mut u8, String> {
    use std::os::fd::AsRawFd;
    let len = region_len();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(create)
        .open(PATH)
        .map_err(|e| format!("open {PATH}: {e}"))?;
    if create {
        file.set_len(len as u64)
            .map_err(|e| format!("size {PATH}: {e}"))?;
    } else if (file.metadata().map_err(|e| e.to_string())?.len() as usize) < len {
        return Err(format!("{PATH} is smaller than the region"));
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
        return Err(format!("mmap {PATH}: {}", std::io::Error::last_os_error()));
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

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let role = std::env::args().nth(1);
    let run = match role.as_deref() {
        Some("consumer") => consumer(),
        Some("producer") => producer(),
        _ => Err("usage: zcr-test-ipm consumer | producer".to_string()),
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
