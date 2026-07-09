//! CPU topology discovery for measurement placements: derive
//! the interesting 2-thread pin pairs from /sys, anchored on
//! cpu 0.
//!
//! - same cache domain (another core sharing cpu 0's L3);
//! - cross cache domain (a core outside cpu 0's L3);
//! - SMT siblings (cpu 0's hyper-thread, shared L1/L2);
//! - unpinned (scheduler's choice).
//!
//! Pairs the machine doesn't have (no SMT, single L3 domain)
//! are simply absent; non-Linux gets only the unpinned entry.

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

/// Discover the placement cells for this machine, in
/// same-domain → cross-domain → SMT → unpinned order.
#[cfg(target_os = "linux")]
pub fn discover_placements() -> Vec<Placement> {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let siblings = read("/sys/devices/system/cpu/cpu0/topology/thread_siblings_list")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let online = read("/sys/devices/system/cpu/online")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default();
    let l3 = read("/sys/devices/system/cpu/cpu0/cache/index3/shared_cpu_list")
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_else(|| siblings.clone());

    let mut v = Vec::new();
    if let Some(c) = l3
        .iter()
        .copied()
        .find(|&c| c != 0 && !siblings.contains(&c))
    {
        v.push(Placement {
            label: format!("0,{c} CCX"),
            pin: Some((0, c)),
        });
    }
    if let Some(c) = online.iter().copied().find(|c| !l3.contains(c)) {
        v.push(Placement {
            label: format!("0,{c} x-CCX"),
            pin: Some((0, c)),
        });
    }
    if siblings.len() >= 2 && siblings[0] == 0 {
        let sib = siblings[1];
        v.push(Placement {
            label: format!("0,{sib} SMT"),
            pin: Some((0, sib)),
        });
    }
    v.push(Placement {
        label: "unpinned".to_string(),
        pin: None,
    });
    v
}

/// Non-Linux stub: no /sys topology — unpinned only.
#[cfg(not(target_os = "linux"))]
pub fn discover_placements() -> Vec<Placement> {
    vec![Placement {
        label: "unpinned".to_string(),
        pin: None,
    }]
}
