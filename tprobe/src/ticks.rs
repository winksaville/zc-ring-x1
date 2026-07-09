//! Hardware tick counter abstraction: thin wrapper over the
//! target architecture's fixed-rate monotonic counter.
//!
//! Probes call three functions; the per-arch impl lives in a
//! child module gated by `#[cfg(target_arch = ...)]`:
//!
//! - [`read_ticks`] — current counter value.
//! - [`ticks_per_ns`] — calibrated conversion ratio.
//! - [`require_ok`] — exit the process if the counter isn't
//!   usable for probe measurements.
//!
//! `x86_64` (`rdtsc`) and `aarch64` (`CNTVCT_EL0`) are
//! implemented today. RISC-V (`time` CSR) has an architecturally
//! invariant counter by ISA spec, so its `require_ok` will be
//! (nearly) a no-op once that impl lands.

#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "x86_64")]
use x86_64 as imp;

#[cfg(target_arch = "aarch64")]
mod aarch64;

#[cfg(target_arch = "aarch64")]
use aarch64 as imp;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!(
    "tprobe currently only supports target_arch = \"x86_64\" \
     and \"aarch64\". Add a per-arch impl module (RISC-V: time CSR) \
     and wire it into src/ticks.rs."
);

/// Read the current tick counter. Monotonic and fixed-rate.
#[inline(always)]
pub fn read_ticks() -> u64 {
    imp::read_ticks()
}

/// Conversion ratio: counter ticks per nanosecond. Calibrated
/// (x86_64) or read from hardware (aarch64, `CNTFRQ_EL0`).
/// Cached — the first call does the work.
pub fn ticks_per_ns() -> f64 {
    imp::ticks_per_ns()
}

/// Verify the tick counter is usable for probe measurements;
/// exit the process (code 1) with a diagnostic if not. The
/// checks performed depend on the target architecture.
pub fn require_ok() {
    imp::require_ok();
}
