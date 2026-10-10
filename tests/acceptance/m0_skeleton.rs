//! M0.1: the workspace skeleton has exactly the crates of `docs/architecture.md` §2, and every crate
//! root carries the never-crash attributes.

use std::collections::BTreeSet;
use std::path::Path;

use dalica_acceptance::{CRATE_ROOT_ATTRIBUTES, cargo};
use serde_json::Value;

const CRATES: [&str; 18] = [
    "core", "fs", "image", "meta", "sidecar", "edit", "scenes", "fetch", "faces", "library", "thumbs", "store", "scan", "create", "engine", "ui-kit",
    "ui", "mcp",
];
const OTHERS: [&str; 4] = ["dalica", "dalica-cli", "xtask", "dalica-acceptance"];

fn metadata() -> Value {
    let out = cargo().args(["metadata", "--no-deps", "--format-version", "1"]).output().unwrap();
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn workspace_packages(meta: &Value) -> Vec<&Value> {
    let members: BTreeSet<&str> = meta["workspace_members"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
    meta["packages"].as_array().unwrap().iter().filter(|p| members.contains(p["id"].as_str().unwrap())).collect()
}

#[test]
fn workspace_has_exactly_the_specified_crates() {
    let meta = metadata();
    let names: BTreeSet<String> = workspace_packages(&meta).iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
    let mut expected: BTreeSet<String> = CRATES.iter().map(|c| format!("dalica-{c}")).collect();
    expected.extend(OTHERS.iter().map(|s| s.to_string()));
    assert_eq!(names, expected);
    assert_eq!(names.len(), 22, "18 crates, 2 apps, xtask and dalica-acceptance");
}

#[test]
fn every_crate_root_has_the_never_crash_attributes() {
    let meta = metadata();
    let mut roots = 0;
    for package in workspace_packages(&meta) {
        for target in package["targets"].as_array().unwrap() {
            let kinds: Vec<&str> = target["kind"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
            if !kinds.iter().any(|k| matches!(*k, "lib" | "bin")) {
                continue;
            }
            let path = Path::new(target["src_path"].as_str().unwrap());
            let source = std::fs::read_to_string(path).unwrap();
            for attribute in CRATE_ROOT_ATTRIBUTES {
                assert!(source.lines().any(|line| line.trim() == attribute), "{} lacks `{attribute}`", path.display());
            }
            roots += 1;
        }
    }
    assert!(roots >= 22, "found only {roots} crate roots");
}

#[test]
fn every_crate_has_a_readme() {
    let root = dalica_acceptance::workspace_root();
    for c in CRATES {
        assert!(root.join("crates").join(c).join("README.md").is_file(), "crates/{c}/README.md is missing");
    }
}
