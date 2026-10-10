//! `cargo xtask layers`: the layer table of `docs/architecture.md` §2 and its four rules.
//!
//! 1. A normal (or build) dependency on a workspace crate must point to a lower layer or a listed
//!    exception; every workspace crate must be registered in [`TABLE`].
//! 2. Dev-dependencies may point anywhere lower, plus `dalica-scenes` from any crate.
//! 3. Only L5 crates and apps (and the acceptance tests) may depend on UI crates.
//! 4. Only `crates/fs` may call `std::fs` write functions.
//!
//! The rules work on [`Metadata`] plus source text, so tests feed them synthetic workspaces.

use std::path::{Path, PathBuf};

use crate::metadata::{DepKind, Metadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
    /// `apps/*`: may depend on anything (except the acceptance tests).
    App,
    /// `tests/acceptance`: may depend on anything; nothing may depend on it.
    Test,
    /// `xtask`: build tooling, independent of the product crates.
    Tool,
}

impl Layer {
    /// Rank for "lower than" comparisons among product layers; `None` for apps, tests and tools.
    fn rank(self) -> Option<u8> {
        match self {
            Layer::L0 => Some(0),
            Layer::L1 => Some(1),
            Layer::L2 => Some(2),
            Layer::L3 => Some(3),
            Layer::L4 => Some(4),
            Layer::L5 => Some(5),
            Layer::App | Layer::Test | Layer::Tool => None,
        }
    }
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.rank() {
            Some(r) => write!(f, "L{r}"),
            None => f.write_str(match self {
                Layer::App => "app",
                Layer::Test => "test",
                _ => "tool",
            }),
        }
    }
}

/// Which workspace crates a crate may use as normal dependencies.
#[derive(Debug, Clone, Copy)]
pub enum Allowed {
    /// Any crate in a strictly lower product layer.
    Lower,
    /// Any lower crate plus these same-layer siblings.
    LowerPlus(&'static [&'static str]),
    /// Exactly these crates.
    Only(&'static [&'static str]),
    /// Any product crate (apps, acceptance tests).
    Anything,
}

/// The layer table (`docs/architecture.md` §2). A new crate is added here and to that table in the
/// same commit.
pub const TABLE: &[(&str, Layer, Allowed)] = &[
    ("dalica-core", Layer::L0, Allowed::Only(&[])),
    ("dalica-fs", Layer::L0, Allowed::Only(&["dalica-core"])),
    ("dalica-image", Layer::L1, Allowed::Lower),
    ("dalica-meta", Layer::L1, Allowed::LowerPlus(&["dalica-image"])),
    ("dalica-sidecar", Layer::L1, Allowed::Lower),
    ("dalica-edit", Layer::L1, Allowed::LowerPlus(&["dalica-image"])),
    ("dalica-scenes", Layer::L1, Allowed::Only(&["dalica-core"])),
    ("dalica-fetch", Layer::L1, Allowed::Lower),
    ("dalica-faces", Layer::L1, Allowed::Only(&["dalica-core"])),
    ("dalica-library", Layer::L2, Allowed::Only(&["dalica-core"])),
    ("dalica-thumbs", Layer::L2, Allowed::Only(&["dalica-image", "dalica-edit", "dalica-fs", "dalica-core"])),
    ("dalica-store", Layer::L3, Allowed::Lower),
    ("dalica-scan", Layer::L3, Allowed::Lower),
    ("dalica-create", Layer::L3, Allowed::Lower),
    ("dalica-engine", Layer::L4, Allowed::Lower),
    ("dalica-ui-kit", Layer::L5, Allowed::Only(&["dalica-core"])),
    ("dalica-ui", Layer::L5, Allowed::LowerPlus(&["dalica-ui-kit"])),
    ("dalica-mcp", Layer::L5, Allowed::Lower),
    ("dalica", Layer::App, Allowed::Anything),
    ("dalica-cli", Layer::App, Allowed::Anything),
    ("dalica-acceptance", Layer::Test, Allowed::Anything),
    ("xtask", Layer::Tool, Allowed::Only(&[])),
];

/// Dev-dependency allowed from any crate (rule 2): it depends only on `core`, so no cycles.
pub const SCENES: &str = "dalica-scenes";

/// External crates that only L5, apps and the acceptance tests may use (rule 3).
pub const UI_CRATES: &[&str] =
    &["egui", "eframe", "egui_extras", "egui-wgpu", "egui-winit", "egui_kittest", "egui_software_backend", "winit", "wgpu", "rfd", "muda"];

/// The one crate that may write to disk with `std::fs` (rule 4).
pub const FS_CRATE: &str = "dalica-fs";

/// `std::fs` items that write. Matched after a `fs::` path segment (or inside a `use std::fs::…`).
const FS_WRITE_ITEMS: &[&str] = &[
    "write",
    "rename",
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "copy",
    "create_dir",
    "create_dir_all",
    "hard_link",
    "soft_link",
    "set_permissions",
    "OpenOptions",
];
/// Write calls reachable without a `fs::` prefix once `File` or `OpenOptions` is imported.
const FILE_WRITE_CALLS: &[&str] = &["File::create", "File::create_new", "File::options", "OpenOptions::new"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub krate: String,
    pub rule: u8,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rule {}: {}: {}", self.rule, self.krate, self.message)
    }
}

pub fn entry(name: &str) -> Option<(Layer, Allowed)> {
    TABLE.iter().find(|(n, _, _)| *n == name).map(|(_, l, a)| (*l, *a))
}

fn is_ui_crate(name: &str) -> bool {
    UI_CRATES.contains(&name)
}

/// May `from` (layer `layer`, allowance `allowed`) use workspace crate `to` as a normal dependency?
fn normal_allowed(layer: Layer, allowed: Allowed, to: &str) -> bool {
    let Some((to_layer, _)) = entry(to) else { return false };
    if matches!(to_layer, Layer::App | Layer::Test | Layer::Tool) {
        return false;
    }
    let lower = matches!((layer.rank(), to_layer.rank()), (Some(a), Some(b)) if b < a);
    match allowed {
        Allowed::Lower => lower,
        Allowed::LowerPlus(extra) => lower || extra.contains(&to),
        Allowed::Only(list) => list.contains(&to),
        Allowed::Anything => true,
    }
}

/// Rules 1–3 on the dependency graph.
pub fn check_dependencies(meta: &Metadata) -> Vec<Violation> {
    let mut out = Vec::new();
    for package in meta.workspace_packages() {
        let name = package.name.as_str();
        let Some((layer, allowed)) = entry(name) else {
            out.push(Violation {
                krate: name.to_string(),
                rule: 1,
                message: "unregistered crate: add it to xtask/src/layers.rs and docs/architecture.md §2".into(),
            });
            continue;
        };
        for dep in &package.dependencies {
            let kind = dep.kind();
            if dep.name == name {
                continue;
            }
            if meta.is_workspace_package(&dep.name) {
                let ok = match kind {
                    DepKind::Normal | DepKind::Build => normal_allowed(layer, allowed, &dep.name),
                    DepKind::Dev => {
                        let test_crate = matches!(entry(&dep.name), Some((Layer::Test, _)));
                        !test_crate && (normal_allowed(layer, allowed, &dep.name) || dep.name == SCENES || dev_lower(layer, &dep.name))
                    }
                };
                if !ok {
                    let to_layer = entry(&dep.name).map(|(l, _)| l.to_string()).unwrap_or_else(|| "unregistered".into());
                    let rule = if kind == DepKind::Dev { 2 } else { 1 };
                    let why = if kind == DepKind::Dev {
                        "dev-dependencies may point only to lower layers or dalica-scenes"
                    } else {
                        "dependencies must point to a lower layer or a listed exception"
                    };
                    out.push(Violation {
                        krate: name.to_string(),
                        rule,
                        message: format!("{name} ({layer}) -> {} ({to_layer}) [{kind}]: {why}", dep.name),
                    });
                }
            } else if is_ui_crate(&dep.name) && !matches!(layer, Layer::L5 | Layer::App | Layer::Test) {
                out.push(Violation {
                    krate: name.to_string(),
                    rule: 3,
                    message: format!("{name} ({layer}) depends on UI crate `{}` [{kind}]: only L5 crates and apps may", dep.name),
                });
            }
        }
    }
    out
}

fn dev_lower(layer: Layer, to: &str) -> bool {
    let to_layer = entry(to).map(|(l, _)| l);
    match (layer, to_layer) {
        // Apps and tests may use any product crate in tests.
        (Layer::App | Layer::Test, Some(t)) => t.rank().is_some(),
        (_, Some(t)) => matches!((layer.rank(), t.rank()), (Some(a), Some(b)) if b < a),
        _ => false,
    }
}

/// Rule 4: `std::fs` writes in product code outside `dalica-fs`. Scans `src/**/*.rs` of every crate
/// and app except `dalica-fs`, the acceptance tests and `xtask`; `#[cfg(test)]` items at the end of a
/// file (the usual test module) are skipped, and so are `tests/`, `benches/` and `examples/`.
pub fn check_sources(meta: &Metadata) -> Vec<Violation> {
    let mut out = Vec::new();
    for package in meta.workspace_packages() {
        let exempt = package.name == FS_CRATE || matches!(entry(&package.name), Some((Layer::Test | Layer::Tool, _)));
        if exempt {
            continue;
        }
        for file in rust_files(&package.dir().join("src")) {
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let rel = file.strip_prefix(&meta.workspace_root).unwrap_or(&file).display().to_string();
            for (line, what) in fs_writes(&text) {
                out.push(Violation {
                    krate: package.name.clone(),
                    rule: 4,
                    message: format!("{rel}:{line}: `{what}` writes to disk outside dalica-fs; use the fs crate"),
                });
            }
        }
    }
    out
}

/// Every `.rs` file under `dir`, sorted, bounded (depth 16, 100 000 files).
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        if depth > 16 || out.len() > 100_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push((path, depth + 1));
            } else if ft.is_file() && path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Lines (1-based) of `text` that call a `std::fs` write function, with the matched item.
pub fn fs_writes(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        if line.starts_with("#[cfg(test)]") && raw.starts_with('#') {
            break;
        }
        if line.starts_with("//") {
            continue;
        }
        let code = line.split("//").next().unwrap_or(line);
        let use_line = code.strip_prefix("use std::fs::").or_else(|| code.strip_prefix("pub use std::fs::"));
        if let Some(item) = use_line.and_then(|rest| FS_WRITE_ITEMS.iter().find(|item| contains_word(rest, item))) {
            out.push((i + 1, format!("std::fs::{item}")));
            continue;
        }
        if let Some(item) = fs_path_write(code) {
            out.push((i + 1, format!("fs::{item}")));
            continue;
        }
        if let Some(call) = FILE_WRITE_CALLS.iter().find(|c| contains_word(code, c)) {
            out.push((i + 1, (*call).to_string()));
        }
    }
    out
}

/// A `fs::<write item>` path where `fs` is a whole path segment (so `dalica_fs::write` is fine).
fn fs_path_write(code: &str) -> Option<&'static str> {
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(pos) = code.get(start..).and_then(|s| s.find("fs::")) {
        let at = start + pos;
        let boundary = at == 0 || bytes.get(at - 1).is_some_and(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        let after = code.get(at + 4..).unwrap_or("");
        if let Some(item) = FS_WRITE_ITEMS.iter().find(|item| boundary && starts_with_word(after, item)) {
            return Some(item);
        }
        start = at + 4;
    }
    None
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn starts_with_word(s: &str, word: &str) -> bool {
    s.starts_with(word) && !s.as_bytes().get(word.len()).is_some_and(|b| is_ident(*b))
}

fn contains_word(s: &str, word: &str) -> bool {
    let bytes = s.as_bytes();
    let mut start = 0;
    while let Some(pos) = s.get(start..).and_then(|x| x.find(word)) {
        let at = start + pos;
        let before_ok = at == 0 || bytes.get(at - 1).is_some_and(|b| !is_ident(*b));
        let after_ok = !bytes.get(at + word.len()).is_some_and(|b| is_ident(*b));
        if before_ok && after_ok {
            return true;
        }
        start = at + word.len();
    }
    false
}

/// All four rules.
pub fn check(meta: &Metadata) -> Vec<Violation> {
    let mut out = check_dependencies(meta);
    out.extend(check_sources(meta));
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let meta = Metadata::load(root, false)?;
    let violations = check(&meta);
    for v in &violations {
        eprintln!("layers: {v}");
    }
    if violations.is_empty() {
        println!("layers: ok ({} crates checked)", meta.workspace_packages().len());
        Ok(())
    } else {
        Err(format!("layers: {} violation(s)", violations.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `(dependency, kind)`; kind `None` = normal.
    type Deps<'a> = &'a [(&'a str, Option<&'a str>)];

    /// A synthetic workspace: `(name, [(dep, kind)])` with every package under `root/<name>`.
    fn workspace(root: &Path, crates: &[(&str, Deps<'_>)]) -> Metadata {
        let packages: Vec<_> = crates
            .iter()
            .map(|(name, deps)| {
                let deps: Vec<_> = deps
                    .iter()
                    .map(|(d, kind)| {
                        let path = if d.starts_with("dalica") { Some(root.join(d).display().to_string()) } else { None };
                        json!({"name": d, "kind": kind, "path": path})
                    })
                    .collect();
                json!({
                    "name": name, "version": "0.0.0", "id": format!("path+file://{}#0.0.0", root.join(name).display()),
                    "manifest_path": root.join(name).join("Cargo.toml"), "dependencies": deps, "targets": []
                })
            })
            .collect();
        let members: Vec<_> = packages.iter().map(|p| p["id"].clone()).collect();
        let doc = json!({"packages": packages, "workspace_members": members, "workspace_root": root});
        Metadata::parse(doc.to_string().as_bytes()).unwrap()
    }

    fn rules(v: &[Violation]) -> Vec<(String, u8)> {
        v.iter().map(|v| (v.krate.clone(), v.rule)).collect()
    }

    #[test]
    fn real_table_is_consistent() {
        for (name, layer, allowed) in TABLE {
            if let Allowed::Only(list) | Allowed::LowerPlus(list) = allowed {
                for dep in *list {
                    assert!(entry(dep).is_some(), "{name} allows unregistered {dep}");
                    assert!(normal_allowed(*layer, *allowed, dep), "{name} -> {dep}");
                }
            }
        }
        assert_eq!(TABLE.iter().filter(|(n, _, _)| n.starts_with("dalica-") && *n != "dalica-cli" && *n != "dalica-acceptance").count(), 18);
    }

    #[test]
    fn library_to_engine_fails_rule_1() {
        let dir = tempfile::tempdir().unwrap();
        let meta = workspace(dir.path(), &[("dalica-library", &[("dalica-engine", None)]), ("dalica-engine", &[])]);
        let v = check_dependencies(&meta);
        assert_eq!(rules(&v), [("dalica-library".to_string(), 1)]);
        assert!(v[0].to_string().contains("dalica-library (L2) -> dalica-engine (L4)"), "{}", v[0]);
    }

    #[test]
    fn egui_in_engine_fails_rule_3() {
        let dir = tempfile::tempdir().unwrap();
        let meta = workspace(dir.path(), &[("dalica-engine", &[("egui", None)]), ("dalica-ui", &[("egui", None)])]);
        let v = check_dependencies(&meta);
        assert_eq!(rules(&v), [("dalica-engine".to_string(), 3)]);
        assert!(v[0].message.contains("`egui`"));
    }

    #[test]
    fn unregistered_crate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let meta = workspace(dir.path(), &[("dalica-mystery", &[]), ("dalica-core", &[])]);
        let v = check_dependencies(&meta);
        assert_eq!(rules(&v), [("dalica-mystery".to_string(), 1)]);
        assert!(v[0].message.contains("unregistered"));
    }

    #[test]
    fn scenes_as_normal_dependency_of_core_fails_but_dev_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let meta = workspace(dir.path(), &[("dalica-core", &[("dalica-scenes", None)]), ("dalica-scenes", &[("dalica-core", None)])]);
        let v = check_dependencies(&meta);
        assert_eq!(rules(&v), [("dalica-core".to_string(), 1)]);
        assert!(v[0].message.contains("dalica-core (L0) -> dalica-scenes (L1) [normal]"), "{}", v[0]);

        let meta = workspace(dir.path(), &[("dalica-core", &[("dalica-scenes", Some("dev"))]), ("dalica-scenes", &[("dalica-core", None)])]);
        assert!(check_dependencies(&meta).is_empty());
    }

    #[test]
    fn listed_exceptions_and_dev_rules() {
        let dir = tempfile::tempdir().unwrap();
        let meta = workspace(
            dir.path(),
            &[
                ("dalica-edit", &[("dalica-image", None)]),
                ("dalica-meta", &[("dalica-image", None)]),
                ("dalica-image", &[("dalica-core", None)]),
                ("dalica-core", &[]),
                // library stays pure even though fs is lower
                ("dalica-library", &[("dalica-fs", None), ("dalica-store", Some("dev"))]),
                ("dalica-fs", &[]),
                ("dalica-store", &[]),
                // nothing may depend on the acceptance tests, not even as a dev-dependency
                ("dalica-ui", &[("dalica-acceptance", Some("dev"))]),
                ("dalica-acceptance", &[("dalica-ui", None)]),
            ],
        );
        let v = check_dependencies(&meta);
        assert_eq!(rules(&v), [("dalica-library".to_string(), 1), ("dalica-library".to_string(), 2), ("dalica-ui".to_string(), 2)], "{v:#?}");
    }

    #[test]
    fn std_fs_write_in_store_fails_rule_4() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("dalica-store").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn save() {\n    let _ = std::fs::write(\"x\", b\"y\");\n}\n").unwrap();
        let fs_src = dir.path().join("dalica-fs").join("src");
        std::fs::create_dir_all(&fs_src).unwrap();
        std::fs::write(fs_src.join("lib.rs"), "pub fn save() { let _ = std::fs::write(\"x\", b\"y\"); }\n").unwrap();
        let meta = workspace(dir.path(), &[("dalica-store", &[]), ("dalica-fs", &[])]);
        let v = check_sources(&meta);
        assert_eq!(rules(&v), [("dalica-store".to_string(), 4)]);
        assert!(v[0].message.contains("dalica-store/src/lib.rs:2"), "{}", v[0]);
    }

    #[test]
    fn fs_write_detection() {
        let hits = |s: &str| fs_writes(s).into_iter().map(|(l, w)| format!("{l}:{w}")).collect::<Vec<_>>();
        assert_eq!(hits("std::fs::write(p, b)"), ["1:fs::write"]);
        assert_eq!(hits("use std::fs;\nfs::rename(a, b)?;"), ["2:fs::rename"]);
        assert_eq!(hits("fs::remove_dir_all(d)"), ["1:fs::remove_dir_all"]);
        assert_eq!(hits("use std::fs::{read, write};"), ["1:std::fs::write"]);
        assert_eq!(hits("let f = File::create(p)?;"), ["1:File::create"]);
        assert_eq!(hits("OpenOptions::new().append(true)"), ["1:OpenOptions::new"]);
        // reads, other crates' functions, comments and test modules are fine
        assert!(hits("std::fs::read(p)").is_empty());
        assert!(hits("dalica_fs::write(p, b)").is_empty());
        assert!(hits("std::fs::read_to_string(p)").is_empty());
        assert!(hits("// std::fs::write(p, b)").is_empty());
        assert!(hits("fs::writer()").is_empty());
        assert!(hits("fn a() {}\n#[cfg(test)]\nmod tests { fn t() { std::fs::write(p, b); } }").is_empty());
    }
}
