//! CPU topology discovery for measurement placements: derive
//! the interesting 2-thread pin pairs from /sys, anchored on a
//! base cpu, `--base-cpu` in the tools that sweep placements.
//!
//! - same cache domain (another core sharing the base's L3)
//! - cross cache domain (a core outside the base's L3)
//! - SMT siblings (the base core's other cpu, shared L1/L2)
//! - unpinned (scheduler's choice)
//!
//! Partners prefer a core's primary cpu and the highest cpu
//! number, and the default base is the last core's primary
//! cpu, since the scheduler fills cpus from the bottom and the
//! top is the quiet end. The terms are the design note's
//! Terminology: a core is the physical unit, a cpu what the
//! kernel numbers, a core's cpus its SMT siblings.
//!
//! Pairs the machine doesn't have (no SMT, single L3 domain)
//! are simply absent, and non-Linux gets only the unpinned
//! entry.

/// The `--base-cpu` flag, flattened into the tools that sweep
/// placements (`tp-cell` pins explicitly with `--pin`).
#[derive(clap::Args, Debug)]
pub struct BaseCpuArg {
    /// The cpu every placement starts from: CCX is it and a
    /// core on its L3, x-CCX it and a core outside, SMT it and
    /// its sibling. The default is the last core's primary cpu,
    /// the quiet end of the kernel's fill order
    #[arg(long, value_name = "N", default_value_t = default_base_cpu())]
    pub base_cpu: usize,
}

/// A cpu's SMT sibling list from sysfs, the cpu alone when it
/// cannot be read.
#[cfg(target_os = "linux")]
fn siblings_of(cpu: usize) -> Vec<usize> {
    std::fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list"
    ))
    .ok()
    .map(|s| parse_cpu_list(&s))
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| vec![cpu])
}

/// A core's primary cpu: the lowest cpu in its sibling list.
#[cfg(target_os = "linux")]
fn is_primary_cpu(cpu: usize) -> bool {
    siblings_of(cpu).iter().min() == Some(&cpu)
}

/// The online cpus from sysfs, empty when unreadable.
#[cfg(target_os = "linux")]
fn online_cpus() -> Vec<usize> {
    std::fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .map(|s| parse_cpu_list(&s))
        .unwrap_or_default()
}

/// Partner order: a core's primary cpu before its secondary,
/// and the highest cpu number first. The scheduler's idlest-cpu
/// search fills cpus from the bottom, so the top is the quiet
/// end, and a primary cpu's sibling is idler than a secondary's
/// (design note, Measurement placements).
#[cfg(target_os = "linux")]
fn quiet_first(cpus: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = cpus.to_vec();
    v.sort_by_key(|&c| (!is_primary_cpu(c), std::cmp::Reverse(c)));
    v
}

/// The base cpu when `--base-cpu` is not given: the last
/// core's primary cpu, the quiet end of the kernel's fill
/// order, 11 on a 3900X and 5 on a 7600X. 0 when sysfs cannot
/// be read.
#[cfg(target_os = "linux")]
pub fn default_base_cpu() -> usize {
    online_cpus()
        .into_iter()
        .filter(|&c| is_primary_cpu(c))
        .max()
        .unwrap_or(0)
}

/// Non-Linux stub: no topology, so 0 and nothing pins.
#[cfg(not(target_os = "linux"))]
pub fn default_base_cpu() -> usize {
    0
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
/// numbers, and malformed pieces are skipped.
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
    let siblings = siblings_of(base);
    let online = online_cpus();
    let l3 = std::fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{base}/cache/index3/shared_cpu_list"
    ))
    .ok()
    .map(|s| parse_cpu_list(&s))
    .unwrap_or_else(|| siblings.clone());

    let mut v = Vec::new();
    if let Some(c) = quiet_first(&l3)
        .into_iter()
        .find(|&c| c != base && !siblings.contains(&c))
    {
        v.push(Placement {
            label: format!("{base},{c} CCX"),
            pin: Some((base, c)),
        });
    }
    if let Some(c) = quiet_first(&online).into_iter().find(|c| !l3.contains(c)) {
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
