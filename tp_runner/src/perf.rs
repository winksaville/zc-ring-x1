//! Linux perf-counter helper: per-process hardware event
//! counting for measurement cells, via `perf_event_open(2)`
//! (wrapped by the `perf-event2` crate).
//!
//! - [`ProcessCounter`] counts one event for the whole process:
//!   opened on the calling thread with `inherit`, so threads
//!   spawned *after* [`ProcessCounter::new_raw`] are counted
//!   too; user mode only (kernel/hypervisor excluded — the
//!   `:u` suffix in perf(1) terms). The kernel virtualizes the
//!   PMU per task, so other processes never pollute the count.
//! - Open before spawning workers, [`enable`], run the cell,
//!   [`disable`], [`read`].
//! - Raw AMD Zen 2 encodings for the demand-fill source events
//!   are provided as constants (event `0x43`, one umask bit per
//!   source), A/B-verified against `perf stat`. Other
//!   microarchitectures need their own encodings — check
//!   `perf list` and the kernel's event JSONs.
//!
//! [`enable`]: ProcessCounter::enable
//! [`disable`]: ProcessCounter::disable
//! [`read`]: ProcessCounter::read

use std::io;

use perf_event::events::Raw;
use perf_event::{Builder, Counter};

/// Encode an AMD raw PMU event: eventsel bits [7:0] + [11:8]
/// (the high nibble lands at config bits [35:32]) and the
/// umask at bits [15:8].
const fn raw_amd(event: u64, umask: u64) -> u64 {
    (event & 0xff) | (umask << 8) | ((event & 0xf00) << 24)
}

/// Zen 2 `ls_refills_from_sys.ls_mabresp_lcl_cache`: demand
/// data-cache fills served from another core's cache on the
/// local die — the cross-core cache-line-transfer signal.
pub const ZEN2_FILLS_LCL_CACHE: u64 = raw_amd(0x43, 0x02);

/// Zen 2 `ls_refills_from_sys.ls_mabresp_lcl_l2`: demand fills
/// served from the core's own L2.
pub const ZEN2_FILLS_LCL_L2: u64 = raw_amd(0x43, 0x01);

/// Zen 2 `ls_refills_from_sys.ls_mabresp_lcl_dram`: demand
/// fills served from local DRAM.
pub const ZEN2_FILLS_LCL_DRAM: u64 = raw_amd(0x43, 0x08);

/// One per-process hardware event counter: all threads of this
/// process spawned after construction, user mode only.
pub struct ProcessCounter {
    /// The underlying perf-event counter (fd).
    counter: Counter,
}

impl ProcessCounter {
    /// Open a counter for a raw PMU `config` (e.g. the `ZEN2_*`
    /// constants), disabled; call [`enable`](Self::enable) to
    /// start counting. Fails with `EACCES`-flavored errors when
    /// `kernel.perf_event_paranoid` forbids self-profiling.
    pub fn new_raw(config: u64) -> io::Result<ProcessCounter> {
        let counter = Builder::new(Raw::new(config))
            .observe_self()
            .inherit(true)
            .build()?;
        Ok(ProcessCounter { counter })
    }

    /// Start (or resume) counting.
    pub fn enable(&mut self) -> io::Result<()> {
        self.counter.enable()
    }

    /// Stop counting; the accumulated value stays readable.
    pub fn disable(&mut self) -> io::Result<()> {
        self.counter.disable()
    }

    /// Read the accumulated count (summed across all counted
    /// threads).
    pub fn read(&mut self) -> io::Result<u64> {
        self.counter.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perf_event::events::Hardware;

    /// Instructions retired across the opener and a spawned
    /// thread both count (inherit), and the count is large.
    /// Skips (with a note) where perf_event_open is forbidden.
    #[test]
    fn counts_self_and_spawned_thread() {
        let counter = Builder::new(Hardware::INSTRUCTIONS)
            .observe_self()
            .inherit(true)
            .build();
        let mut counter = match counter {
            Ok(c) => ProcessCounter { counter: c },
            Err(e) => {
                eprintln!("skipping: perf_event_open unavailable: {e}");
                return;
            }
        };
        counter.enable().unwrap();
        let t = std::thread::spawn(|| {
            let mut x = 0u64;
            for i in 0..1_000_000u64 {
                x = x.wrapping_add(i);
            }
            std::hint::black_box(x)
        });
        t.join().unwrap();
        counter.disable().unwrap();
        let n = counter.read().unwrap();
        // The spawned loop alone retires >1M instructions.
        assert!(n > 1_000_000, "count too small: {n}");
    }
}
