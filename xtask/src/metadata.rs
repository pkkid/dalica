//! The parts of `cargo metadata` the gates read. Parsed leniently: unknown fields are ignored and
//! missing optional fields default, so a newer cargo does not break the gates.

// The licence, source and resolve-graph fields are read by `deps`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_root: PathBuf,
    #[serde(default)]
    pub resolve: Option<Resolve>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub id: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// The `[features]` table: each feature and what it enables (`dep:x`, `x/f`, `x?/f`, `f`).
    #[serde(default)]
    pub features: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dependency {
    /// The package name (not the rename).
    pub name: String,
    /// `None` = normal, `Some("dev")`, `Some("build")`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Set for path dependencies.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Built only when a feature enables it.
    #[serde(default)]
    pub optional: bool,
    /// The name it is used by when renamed (`package = …`), which its features refer to.
    #[serde(default)]
    pub rename: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

impl Dependency {
    pub fn kind(&self) -> DepKind {
        match self.kind.as_deref() {
            Some("dev") => DepKind::Dev,
            Some("build") => DepKind::Build,
            _ => DepKind::Normal,
        }
    }
}

impl std::fmt::Display for DepKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DepKind::Normal => "normal",
            DepKind::Dev => "dev",
            DepKind::Build => "build",
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub deps: Vec<NodeDep>,
    /// Features enabled for this package in the resolved graph.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    #[serde(default)]
    pub dep_kinds: Vec<NodeDepKind>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeDepKind {
    #[serde(default)]
    pub kind: Option<String>,
}

impl Metadata {
    /// Runs `cargo metadata` in `root`. `with_deps` adds the resolved dependency graph.
    pub fn load(root: &Path, with_deps: bool) -> Result<Metadata, String> {
        Self::run(root, if with_deps { &[] } else { &["--no-deps"] })
    }

    /// The resolved graph as built for one target triple.
    pub fn load_for_target(root: &Path, target: &str) -> Result<Metadata, String> {
        Self::run(root, &["--filter-platform", target])
    }

    fn run(root: &Path, extra: &[&str]) -> Result<Metadata, String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.current_dir(root).args(["metadata", "--format-version", "1"]).args(extra);
        let out = cmd.output().map_err(|e| format!("cannot run cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Self::parse(&out.stdout)
    }

    pub fn parse(json: &[u8]) -> Result<Metadata, String> {
        serde_json::from_slice(json).map_err(|e| format!("cannot parse cargo metadata: {e}"))
    }

    /// The workspace's own packages, in metadata order.
    pub fn workspace_packages(&self) -> Vec<&Package> {
        self.packages.iter().filter(|p| self.workspace_members.iter().any(|m| m == &p.id)).collect()
    }

    pub fn is_workspace_package(&self, name: &str) -> bool {
        self.workspace_packages().iter().any(|p| p.name == name)
    }
}

impl Package {
    /// The directory holding the package's `Cargo.toml`.
    pub fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }
}
