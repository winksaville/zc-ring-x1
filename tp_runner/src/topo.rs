//! CPU topology discovery for measurement placements: derive
//! the interesting 2-thread pin pairs from /sys, anchored on a
//! base cpu, `--base-cpu` in the tools that sweep placements.
//!
//! - same cache domain (another core sharing the base's L3)
//! - cross cache domain (a core outside the base's L3)
//! - SMT siblings (the base's hyper-thread, shared L1/L2)
//! - unpinned (scheduler's choice)
//!
//! Pairs the machine doesn't have (no SMT, single L3 domain)
//! are simply absent, and non-Linux gets only the unpinned
//! entry.

/// The base cpu when `--base-cpu` is not given.
pub const DEFAULT_BASE_CPU: usize = 0;

/// The `--base-cpu` flag, flattened into the tools that sweep
/// placements (`tp-cell` pins explicitly with `--pin`).
#[derive(clap::Args, Debug)]
pub struct BaseCpuArg {
    /// The cpu every placement starts from: CCX is it and a
    /// core on its L3, x-CCX it and a core outside, SMT it and
    /// its sibling
    #[arg(long, value_name = "N", default_value_t = DEFAULT_BASE_CPU)]
    pub base_cpu: usize,
}

/// One placement cell: a display label and the `(main, worker)`
/// pin pair (`None` = unpinned).
pub struct Placement {
    /// Table label, e.g. `"0,1 CCX"`, `"0,12 SMT"`,
    /// `"unpinned"`.
    pub label: String,
    /// `(main_cpu, worker_cpu)`, or `None` for unpinned.
    pub pin: Option<(usize, usize)>,
}

/// Parse a /sys cpu-list string ("0,12" or "0-2,6") into cpu
/// numbers; malformed pieces are skipped.
#[cfg(target_os = "linux")]
fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',') {
        match part.split_once('-') {
            Some((lo, hi)) => {
                if let (Ok(lo), Ok(hi)) = (lo.parse::<usize>(), hi.parse::<usize>()) {
                    out.extend(lo..=hi);
                }
            }
            None => {
                if let Ok(n) = part.parse() {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// Discover the placement cells for this machine from `base`,
/// in same-domain -> cross-domain -> SMT -> unpinned order.
#[cfg(target_os = "linux")]
pub fn discover_placements(base: usize) -> Vec<Placement> {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let siblings = read(&format!(
        "/sys/devices/system/cpu/cpu{base}/topology/thread_siblings_list"
    ))
    .map(|s| parse_cpu_list(&s))
    .unwrap_or_default();
    let online = read("/sys/devices/system/cpu/online")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let l3 = read(&format!(
        "/sys/devices/system/cpu/cpu{base}/cache/index3/shared_cpu_list"
    ))
    .map(|s| parse_cpu_list(&s))
    .unwrap_or_else(|| siblings.clone());

    let mut v = Vec::new();
    if let Some(c) = l3
        .iter()
        .copied()
        .find(|&c| c != base && !siblings.contains(&c))
    {
        v.push(Placement {
            label: format!("{base},{c} CCX"),
            pin: Some((base, c)),
        });
    }
    if let Some(c) = online.iter().copied().find(|c| !l3.contains(c)) {
        v.push(Placement {
            label: format!("{base},{c} x-CCX"),
            pin: Some((base, c)),
        });
    }
    if let Some(sib) = siblings.iter().copied().find(|&c| c != base) {
        v.push(Placement {
            label: format!("{base},{sib} SMT"),
            pin: Some((base, sib)),
        });
    }
    v.push(Placement {
        label: "unpinned".to_string(),
        pin: None,
    });
    v
}

/// Non-Linux stub: no /sys topology, unpinned only.
#[cfg(not(target_os = "linux"))]
pub fn discover_placements(_base: usize) -> Vec<Placement> {
    vec![Placement {
        label: "unpinned".to_string(),
        pin: None,
    }]
}
