//! `cargo xtask deps`: the offline dependency audit of `docs/security.md` §8.
//!
//! - Licences (whole resolved graph): every crate's SPDX expression must be satisfiable from
//!   [`ALLOWED_LICENCES`]; `MPL-2.0` and crates without an SPDX licence only through [`LICENCE_EXCEPTIONS`].
//! - Product graph (what `dalica` and `dalica-cli` link, normal and build edges, resolved separately
//!   for Linux, Windows and macOS): no [`BANNED`] crate, no crate whose build script depends on a C
//!   toolchain crate ([`C_BUILD`]), and `*-sys` crates only from [`SYS_ALLOW`].
//! - Sources: crates.io, local paths, and git only from the Craft-app repositories at a full hash.
//! - Duplicate versions are printed as warnings.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::metadata::{Metadata, Package};

/// SPDX ids allowed for dependencies (`security.md` §8.3).
pub const ALLOWED_LICENCES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "BSL-1.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "CC0-1.0",
    "MIT-0",
];

/// Reviewed per-crate licence exceptions (`MPL-2.0`, or no SPDX expression), each with a reason.
pub const LICENCE_EXCEPTIONS: &[(&str, &str)] = &[];

/// Crates that build or link C, or bring their own crypto in C/assembly (`security.md` §8.1).
pub const BANNED: &[&str] =
    &["openssl", "openssl-sys", "native-tls", "ring", "aws-lc-rs", "aws-lc-sys", "libz-sys", "zstd-sys", "bzip2-sys", "lzma-sys"];

/// Build-script dependencies that mean "compiles C/C++ or links system libraries".
pub const C_BUILD: &[&str] = &["cc", "cmake", "bindgen", "pkg-config", "vcpkg", "autocfg-cc"];

/// Binding-only `*-sys` crates that compile no C, each with its reason (`security.md` §8.1).
pub const SYS_ALLOW: &[(&str, &str)] = &[
    ("windows-sys", "Win32 bindings only, no C build"),
    ("linux-raw-sys", "raw Linux syscall bindings, no C build"),
    ("core-foundation-sys", "macOS framework bindings, linked from the system"),
    ("objc2-encode", "Objective-C runtime bindings (objc2 family), no C build"),
    ("wayland-sys", "Wayland client library loaded at run time, no C build"),
    ("renderdoc-sys", "RenderDoc API loaded at run time, debugging only"),
    ("js-sys", "wasm bindings only"),
    ("web-sys", "wasm bindings only"),
    ("jni-sys", "Android bindings, target-gated, unused on desktop"),
    ("ndk-sys", "Android bindings, target-gated, unused on desktop"),
];

/// The apps whose graph is the product.
pub const PRODUCT_ROOTS: &[&str] = &["dalica", "dalica-cli"];

/// Targets the product graph is resolved for.
pub const TARGETS: &[&str] = &["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin"];

const CRATES_IO: &[&str] = &["registry+https://github.com/rust-lang/crates.io-index", "sparse+https://index.crates.io/"];
const GIT_ALLOWED: &[&str] = &["git+https://github.com/storytold/lightcraft", "git+https://github.com/storytold/photocraft"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub krate: String,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.krate, self.message)
    }
}

/// Is an SPDX expression satisfiable from the allow-list? `OR` (and the old `/`) needs one side,
/// `AND` both; parentheses group. `None` when the expression does not parse.
pub fn licence_ok(expr: &str) -> Option<bool> {
    let normalised = expr.replace('/', " OR ").replace('(', " ( ").replace(')', " ) ");
    let tokens: Vec<&str> = normalised.split_whitespace().collect();
    let mut pos = 0;
    let ok = parse_or(&tokens, &mut pos, 0)?;
    (pos == tokens.len()).then_some(ok)
}

fn parse_or(t: &[&str], pos: &mut usize, depth: u32) -> Option<bool> {
    let mut ok = parse_and(t, pos, depth)?;
    while t.get(*pos) == Some(&"OR") {
        *pos += 1;
        ok |= parse_and(t, pos, depth)?;
    }
    Some(ok)
}

fn parse_and(t: &[&str], pos: &mut usize, depth: u32) -> Option<bool> {
    let mut ok = parse_atom(t, pos, depth)?;
    while t.get(*pos) == Some(&"AND") {
        *pos += 1;
        ok &= parse_atom(t, pos, depth)?;
    }
    Some(ok)
}

fn parse_atom(t: &[&str], pos: &mut usize, depth: u32) -> Option<bool> {
    if depth > 16 {
        return None;
    }
    let tok = *t.get(*pos)?;
    *pos += 1;
    if tok == "(" {
        let ok = parse_or(t, pos, depth + 1)?;
        if t.get(*pos) != Some(&")") {
            return None;
        }
        *pos += 1;
        return Some(ok);
    }
    if matches!(tok, ")" | "OR" | "AND" | "WITH") {
        return None;
    }
    if t.get(*pos) == Some(&"WITH") {
        let exception = *t.get(*pos + 1)?;
        *pos += 2;
        return Some(ALLOWED_LICENCES.contains(&format!("{tok} WITH {exception}").as_str()));
    }
    Some(ALLOWED_LICENCES.contains(&tok))
}

fn source_ok(source: Option<&str>) -> bool {
    match source {
        None => true, // a local path
        Some(s) if CRATES_IO.contains(&s) => true,
        Some(s) => {
            GIT_ALLOWED.iter().any(|g| s.starts_with(g))
                && s.rsplit_once('#').is_some_and(|(_, hash)| hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()))
        }
    }
}

fn exception(name: &str) -> Option<&'static str> {
    LICENCE_EXCEPTIONS.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

/// Licence and source checks on every non-workspace package of the resolved graph.
pub fn check_licences_and_sources(meta: &Metadata) -> Vec<Violation> {
    let mut out = Vec::new();
    let workspace: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let resolved: Option<BTreeSet<&str>> = meta.resolve.as_ref().map(|r| r.nodes.iter().map(|n| n.id.as_str()).collect());
    for p in &meta.packages {
        if workspace.contains(p.id.as_str()) || resolved.as_ref().is_some_and(|r| !r.contains(p.id.as_str())) {
            continue;
        }
        let v = |message: String| Violation { krate: format!("{} {}", p.name, p.version), message };
        if !source_ok(p.source.as_deref()) {
            out.push(v(format!(
                "source `{}` is not crates.io, a path, or a Craft-app git repository at a full commit hash",
                p.source.as_deref().unwrap_or("")
            )));
        }
        if exception(&p.name).is_some() {
            continue;
        }
        match p.license.as_deref() {
            None => out.push(v("no SPDX licence expression; add a reviewed LICENCE_EXCEPTIONS entry or drop the crate".into())),
            Some(expr) => match licence_ok(expr) {
                Some(true) => {}
                Some(false) => {
                    out.push(v(format!("licence `{expr}` is not in the allow-list (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, …)")))
                }
                None => out.push(v(format!("cannot parse licence expression `{expr}`"))),
            },
        }
    }
    out
}

/// Ids of the packages linked into the product (normal and build edges from [`PRODUCT_ROOTS`]).
pub fn product_graph(meta: &Metadata) -> BTreeSet<String> {
    let Some(resolve) = &meta.resolve else { return BTreeSet::new() };
    let nodes: BTreeMap<&str, &crate::metadata::Node> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> =
        meta.workspace_packages().into_iter().filter(|p| PRODUCT_ROOTS.contains(&p.name.as_str())).map(|p| p.id.clone()).collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) || seen.len() > 100_000 {
            continue;
        }
        let Some(node) = nodes.get(id.as_str()) else { continue };
        for dep in &node.deps {
            let linked = dep.dep_kinds.is_empty() || dep.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev"));
            if linked {
                stack.push(dep.pkg.clone());
            }
        }
    }
    seen
}

/// Banned crates, C build scripts and unlisted `*-sys` crates in the product graph.
pub fn check_product(meta: &Metadata) -> Vec<Violation> {
    let mut out = Vec::new();
    let graph = product_graph(meta);
    let by_id: BTreeMap<&str, &Package> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: BTreeMap<&str, &crate::metadata::Node> = meta.resolve.iter().flat_map(|r| r.nodes.iter()).map(|n| (n.id.as_str(), n)).collect();
    for id in &graph {
        let Some(p) = by_id.get(id.as_str()) else { continue };
        let name = p.name.as_str();
        let v = |message: String| Violation { krate: format!("{} {}", p.name, p.version), message };
        if BANNED.contains(&name) {
            out.push(v("banned in the product (builds or links C, or C/assembly crypto; security.md §8.1)".into()));
        }
        if name.ends_with("-sys") && !SYS_ALLOW.iter().any(|(n, _)| *n == name) && !BANNED.contains(&name) {
            out.push(v("`*-sys` crate not in the allow-list; add it to SYS_ALLOW with a reason only if it compiles no C (maintainer review)".into()));
        }
        if let Some(node) = nodes.get(id.as_str()) {
            for dep in &node.deps {
                let build = dep.dep_kinds.iter().any(|k| k.kind.as_deref() == Some("build"));
                let dep_name = by_id.get(dep.pkg.as_str()).map(|d| d.name.as_str()).unwrap_or("");
                if build && C_BUILD.contains(&dep_name) {
                    out.push(v(format!("its build script depends on `{dep_name}`: no C/C++ builds in the product (security.md §8.1)")));
                }
            }
        }
    }
    out
}

/// `name: [versions]` for crates resolved at more than one version.
pub fn duplicates(meta: &Metadata) -> Vec<(String, Vec<String>)> {
    let mut versions: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let resolved: Option<BTreeSet<&str>> = meta.resolve.as_ref().map(|r| r.nodes.iter().map(|n| n.id.as_str()).collect());
    for p in &meta.packages {
        if resolved.as_ref().is_none_or(|r| r.contains(p.id.as_str())) {
            versions.entry(p.name.as_str()).or_default().insert(p.version.as_str());
        }
    }
    versions.into_iter().filter(|(_, v)| v.len() > 1).map(|(n, v)| (n.to_string(), v.into_iter().map(str::to_string).collect())).collect()
}

pub fn check(meta: &Metadata) -> Vec<Violation> {
    let mut out = check_licences_and_sources(meta);
    out.extend(check_product(meta));
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut violations = BTreeSet::new();
    let mut crates = 0;
    let mut product = BTreeSet::new();
    for target in TARGETS {
        let meta = Metadata::load_for_target(root, target)?;
        crates = crates.max(meta.packages.len());
        product.extend(product_graph(&meta));
        violations.extend(check(&meta).into_iter().map(|v| format!("[{target}] {v}")));
        for (name, versions) in duplicates(&meta) {
            eprintln!("deps: warning [{target}]: {name} at {} versions: {}", versions.len(), versions.join(", "));
        }
    }
    for v in &violations {
        eprintln!("deps: {v}");
    }
    if violations.is_empty() {
        println!("deps: ok ({crates} crates resolved, {} in the product graph, {} targets)", product.len(), TARGETS.len());
        Ok(())
    } else {
        Err(format!("deps: {} violation(s)", violations.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A package entry; `source` None = path.
    fn pkg(name: &str, licence: Option<&str>, source: Option<&str>) -> Value {
        json!({"name": name, "version": "1.0.0", "id": format!("{name} 1.0.0"), "license": licence, "source": source,
               "manifest_path": format!("/x/{name}/Cargo.toml"), "dependencies": []})
    }

    const IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    /// Workspace = the two apps (+ xtask); `edges` = (from, to, kind) over package names.
    fn meta(extra: Vec<Value>, edges: &[(&str, &str, Option<&str>)]) -> Metadata {
        let mut packages =
            vec![pkg("dalica", Some("MIT OR Apache-2.0"), None), pkg("dalica-cli", Some("MIT OR Apache-2.0"), None), pkg("xtask", None, None)];
        packages.extend(extra);
        let names: Vec<String> = packages.iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
        let nodes: Vec<Value> = names
            .iter()
            .map(|n| {
                let deps: Vec<Value> = edges
                    .iter()
                    .filter(|(from, _, _)| from == n)
                    .map(|(_, to, kind)| json!({"pkg": format!("{to} 1.0.0"), "dep_kinds": [{"kind": kind}]}))
                    .collect();
                json!({"id": format!("{n} 1.0.0"), "deps": deps})
            })
            .collect();
        let doc = json!({"packages": packages, "workspace_members": ["dalica 1.0.0", "dalica-cli 1.0.0", "xtask 1.0.0"],
                         "workspace_root": "/x", "resolve": {"nodes": nodes}});
        Metadata::parse(doc.to_string().as_bytes()).unwrap()
    }

    fn names(v: &[Violation]) -> Vec<String> {
        v.iter().map(|v| v.krate.clone()).collect()
    }

    #[test]
    fn licence_expressions() {
        assert_eq!(licence_ok("MIT"), Some(true));
        assert_eq!(licence_ok("MIT OR Apache-2.0"), Some(true));
        assert_eq!(licence_ok("MIT/Apache-2.0"), Some(true));
        assert_eq!(licence_ok("Apache-2.0 WITH LLVM-exception"), Some(true));
        assert_eq!(licence_ok("(MIT OR Apache-2.0) AND Unicode-3.0"), Some(true));
        assert_eq!(licence_ok("GPL-3.0-or-later"), Some(false));
        assert_eq!(licence_ok("MIT AND GPL-2.0"), Some(false));
        assert_eq!(licence_ok("GPL-2.0 OR MIT"), Some(true));
        assert_eq!(licence_ok("MPL-2.0"), Some(false));
        assert_eq!(licence_ok("LGPL-2.1+"), Some(false));
        assert_eq!(licence_ok("MIT OR"), None);
        assert_eq!(licence_ok("(MIT"), None);
    }

    #[test]
    fn rejects_a_gpl_crate() {
        let m = meta(vec![pkg("copyleft", Some("GPL-3.0"), Some(IO))], &[("dalica", "copyleft", None)]);
        let v = check(&m);
        assert_eq!(names(&v), ["copyleft 1.0.0"]);
        assert!(v[0].message.contains("GPL-3.0"), "{}", v[0]);
    }

    #[test]
    fn rejects_openssl_sys() {
        let m = meta(vec![pkg("openssl-sys", Some("MIT"), Some(IO))], &[("dalica-cli", "openssl-sys", None)]);
        let v = check(&m);
        assert_eq!(names(&v), ["openssl-sys 1.0.0"]);
        assert!(v[0].message.contains("banned"));
    }

    #[test]
    fn rejects_an_unknown_sys_crate_but_allows_listed_ones() {
        let m = meta(
            vec![pkg("foo-sys", Some("MIT"), Some(IO)), pkg("windows-sys", Some("MIT OR Apache-2.0"), Some(IO))],
            &[("dalica", "foo-sys", None), ("dalica", "windows-sys", None)],
        );
        let v = check(&m);
        assert_eq!(names(&v), ["foo-sys 1.0.0"]);
        assert!(v[0].message.contains("allow-list"));
        // A path dependency (no source) counts too.
        let m = meta(vec![pkg("evil-sys", Some("MIT"), None)], &[("dalica", "evil-sys", None)]);
        assert_eq!(names(&check(&m)), ["evil-sys 1.0.0"]);
    }

    #[test]
    fn rejects_a_crate_whose_build_script_depends_on_cc() {
        let m = meta(
            vec![pkg("zipper", Some("MIT"), Some(IO)), pkg("cc", Some("MIT OR Apache-2.0"), Some(IO))],
            &[("dalica", "zipper", None), ("zipper", "cc", Some("build"))],
        );
        let v = check(&m);
        assert_eq!(names(&v), ["zipper 1.0.0"]);
        assert!(v[0].message.contains("`cc`"));
    }

    #[test]
    fn dev_and_tooling_dependencies_are_not_the_product() {
        // A banned crate only reachable as a dev-dependency, or only from xtask, is not in the product.
        let m = meta(
            vec![pkg("ring", Some("ISC"), Some(IO)), pkg("cmake", Some("MIT OR Apache-2.0"), Some(IO)), pkg("helper", Some("MIT"), Some(IO))],
            &[("dalica", "ring", Some("dev")), ("xtask", "helper", None), ("helper", "cmake", Some("build"))],
        );
        assert!(check(&m).is_empty(), "{:?}", check(&m));
    }

    #[test]
    fn sources_and_exceptions() {
        assert!(source_ok(None));
        assert!(source_ok(Some(IO)));
        assert!(source_ok(Some("git+https://github.com/storytold/lightcraft?rev=88ea1a8#88ea1a8f0000000000000000000000000000abcd")));
        assert!(!source_ok(Some("git+https://github.com/storytold/lightcraft?branch=main#88ea1a8")));
        assert!(!source_ok(Some("git+https://example.com/x#88ea1a8f0000000000000000000000000000abcd")));
        let m = meta(vec![pkg("nolicence", None, Some(IO))], &[("dalica", "nolicence", None)]);
        assert!(check(&m)[0].message.contains("no SPDX"));
    }

    #[test]
    fn duplicate_versions_are_reported() {
        let mut a = pkg("dup", Some("MIT"), Some(IO));
        let mut b = pkg("dup", Some("MIT"), Some(IO));
        a["version"] = json!("1.0.0");
        b["version"] = json!("2.0.0");
        b["id"] = json!("dup 2.0.0");
        let mut m = meta(vec![a, b], &[("dalica", "dup", None)]);
        if let Some(r) = m.resolve.as_mut() {
            r.nodes.push(crate::metadata::Node { id: "dup 2.0.0".into(), deps: vec![] });
        }
        assert_eq!(duplicates(&m), [("dup".to_string(), vec!["1.0.0".to_string(), "2.0.0".to_string()])]);
    }
}
