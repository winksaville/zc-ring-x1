//! Pass the zc-ring-x1 version these tools build against into
//! their banners as `ZC_RING_X1_VERSION`, since the tools' own
//! version does not move with the rings they measure.

use std::fs;

fn main() {
    let manifest = "../Cargo.toml";
    println!("cargo:rerun-if-changed={manifest}");
    let text = fs::read_to_string(manifest).expect("the workspace root manifest is readable"); // OK: tp_matrix lives in the zc-ring-x1 workspace
    let mut in_package = false;
    let mut version = "unknown".to_string();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package
            && let Some(rest) = line.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            version = value.trim().trim_matches('"').to_string();
            break;
        }
    }
    println!("cargo:rustc-env=ZC_RING_X1_VERSION={version}");
}
