//! AArch64 impl of the tick-counter abstraction: the Generic
//! Timer's virtual count (`CNTVCT_EL0`) for reads and its
//! self-reported frequency (`CNTFRQ_EL0`) for the conversion
//! ratio. Unlike x86, no calibration loop and no feature probe
//! are needed: the Generic Timer is a mandatory architectural
//! feature, invariant by spec (fixed frequency, keeps counting
//! across idle/power states), and its frequency is readable
//! directly from a register.

use std::sync::OnceLock;

/// Read the Generic Timer virtual count via `CNTVCT_EL0`.
#[inline(always)]
pub fn read_ticks() -> u64 {
    // Plain `mrs` without an `isb` barrier, matching the plain
    // (unfenced) `rdtsc` in the x86_64 impl. The read can be
    // speculated a few instructions early/late, but at Generic
    // Timer rates (54 MHz on the BCM2712 — ~18.5 ns per tick)
    // that blur is well under one tick.
    let ticks: u64;
    unsafe {
        core::arch::asm!(
            "mrs {t}, cntvct_el0",
            t = out(reg) ticks,
            options(nomem, nostack, preserves_flags),
        );
    }
    ticks
}

static TICKS_PER_NS: OnceLock<f64> = OnceLock::new();

/// Conversion ratio from the timer's self-reported frequency.
pub fn ticks_per_ns() -> f64 {
    *TICKS_PER_NS.get_or_init(|| cntfrq_hz() as f64 / 1e9)
}

/// `CNTFRQ_EL0` — Generic Timer frequency in Hz, programmed by
/// firmware at boot (54 MHz on the BCM2712 / Raspberry Pi 5).
/// The register is architecturally 32-bit; `mrs` into a 64-bit
/// register zero-extends.
fn cntfrq_hz() -> u64 {
    let hz: u64;
    unsafe {
        core::arch::asm!(
            "mrs {f}, cntfrq_el0",
            f = out(reg) hz,
            options(nomem, nostack, preserves_flags),
        );
    }
    hz
}

/// Exit if firmware left the timer frequency unprogrammed.
pub fn require_ok() {
    // The counter itself needs no invariance probe (see module
    // doc). The one historical failure mode is firmware leaving
    // CNTFRQ_EL0 unprogrammed (zero), which would make every
    // tick-to-ns conversion divide by zero downstream.
    if cntfrq_hz() == 0 {
        eprintln!(
            "error: CNTFRQ_EL0 reads 0 — firmware did not program \
             the Generic Timer frequency, so tick counts can't be \
             converted to nanoseconds; refusing to run."
        );
        std::process::exit(1);
    }
}
