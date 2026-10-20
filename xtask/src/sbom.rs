//! `cargo xtask sbom`: a CycloneDX 1.5 JSON software bill of materials for every release package
//! (`docs/security.md` §8.5, `docs/releasing.md`), attached to the GitHub release next to
//! `SHA256SUMS.txt`.
//!
//! Each package's components are the product graph `cargo xtask deps` builds ([`product_graph`]:
//! normal and build edges from `dalica` and `dalica-cli`, an optional dependency only when its
//! parent's resolved features turn it on), resolved with `cargo metadata --filter-platform` for
//! each target the package is built for, so a target-gated crate (`nusb`, Linux and Windows only)
//! is listed only where it builds and a feature that is off by default (`hevc`, `heif`) brings
//! nothing. The macOS universal package is the union of its two targets; every component says for
//! which targets it is built. Each component carries its licence expression, its purl (crates.io)
//! and the SHA-256 of its `.crate` from `Cargo.lock`. Offline once the crates are fetched; the
//! output is sorted and carries no random serial number, so the same commit gives the same file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::deps::product_graph;
use crate::metadata::{Metadata, Package};

/// One release package: the platform set of `release.yml` and the targets its binaries are built
/// for.
#[derive(Debug, Clone, Copy)]
pub struct ReleasePackage {
    /// The platform part of the artifact names (`dalica-<v>-<name>.*`).
    pub name: &'static str,
    pub targets: &'static [&'static str],
}

/// The release packages `release.yml` builds (`docs/releasing.md` → What gets built).
pub const RELEASE_PACKAGES: &[ReleasePackage] = &[
    ReleasePackage { name: "linux-x86_64", targets: &["x86_64-unknown-linux-gnu"] },
    ReleasePackage { name: "linux-aarch64", targets: &["aarch64-unknown-linux-gnu"] },
    ReleasePackage { name: "windows-x64", targets: &["x86_64-pc-windows-msvc"] },
    ReleasePackage { name: "macos-universal", targets: &["aarch64-apple-darwin", "x86_64-apple-darwin"] },
];

/// The app the SBOM describes (its `metadata.component`).
const ROOT: &str = "dalica";

const CRATES_IO: &[&str] = &["registry+https://github.com/rust-lang/crates.io-index", "sparse+https://index.crates.io/"];

/// A crate of a package's product graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub version: String,
    /// The SPDX expression of its manifest (`/` written as `OR`), if it has one.
    pub licence: Option<String>,
    /// `None` for a workspace (path) crate.
    pub source: Option<String>,
    /// SHA-256 of the `.crate` file, from `Cargo.lock`.
    pub checksum: Option<String>,
    /// The targets whose graph holds it.
    pub targets: BTreeSet<String>,
    /// The `bom-ref`s of the components it depends on (normal and build edges in the graph).
    pub depends_on: BTreeSet<String>,
}

impl Component {
    /// Its `bom-ref`: the purl for a crates.io crate, `name@version` otherwise (stable across
    /// machines, unlike a path package's id).
    pub fn bom_ref(&self) -> String {
        bom_ref(&self.name, &self.version, self.source.as_deref())
    }

    pub fn purl(&self) -> Option<String> {
        purl(&self.name, &self.version, self.source.as_deref())
    }
}

fn purl(name: &str, version: &str, source: Option<&str>) -> Option<String> {
    source.filter(|s| CRATES_IO.contains(s)).map(|_| format!("pkg:cargo/{name}@{version}"))
}

fn bom_ref(name: &str, version: &str, source: Option<&str>) -> String {
    purl(name, version, source).unwrap_or_else(|| format!("{name}@{version}"))
}

fn licence(p: &Package) -> Option<String> {
    p.license.as_deref().map(|l| l.split('/').map(str::trim).collect::<Vec<_>>().join(" OR "))
}

/// `(name, version, source)` → checksum, from the `[[package]]` tables of a `Cargo.lock`.
pub fn lock_checksums(lock: &str) -> BTreeMap<(String, String, String), String> {
    let mut out = BTreeMap::new();
    let mut entry: BTreeMap<&str, String> = BTreeMap::new();
    let mut flush = |entry: &mut BTreeMap<&str, String>| {
        if let (Some(n), Some(v), Some(s), Some(c)) = (entry.get("name"), entry.get("version"), entry.get("source"), entry.get("checksum")) {
            out.insert((n.clone(), v.clone(), s.clone()), c.clone());
        }
        entry.clear();
    };
    for line in lock.lines().map(str::trim) {
        if line.starts_with('[') {
            flush(&mut entry);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if let Some(key) = ["name", "version", "source", "checksum"].into_iter().find(|k| *k == key) {
            entry.insert(key, value.trim().trim_matches('"').to_string());
        }
    }
    flush(&mut entry);
    out
}

/// `(name, version)` of each crate a release build compiles.
pub type Built = BTreeSet<(String, String)>;

/// One target of a package: its triple, its `cargo metadata`, and what its release build compiles
/// (`None`: all of the product graph).
pub type TargetGraph<'a> = (&'a str, &'a Metadata, Option<&'a Built>);

/// The components of one package: the product graph of each target, kept to the crates its release
/// build compiles, merged. `cargo metadata` resolves features across the whole workspace (a feature
/// another member or a dev-dependency turns on, such as `cc`'s `parallel`, adds an optional
/// dependency the release build leaves out), so `built` (from `cargo tree`, which resolves the
/// features of `cargo build -p dalica -p dalica-cli` alone) trims it. A crate the build compiles that
/// the product graph lacks is an error: the SBOM never leaves out what ships.
pub fn components(per_target: &[TargetGraph], checksums: &BTreeMap<(String, String, String), String>) -> Result<BTreeMap<String, Component>, String> {
    let mut out: BTreeMap<String, Component> = BTreeMap::new();
    for (target, meta, built) in per_target {
        let by_id: BTreeMap<&str, &Package> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
        let keep = |id: &String| built.is_none_or(|b| by_id.get(id.as_str()).is_some_and(|p| b.contains(&(p.name.clone(), p.version.clone()))));
        let graph: BTreeSet<String> = product_graph(meta).into_iter().filter(keep).collect();
        if let Some(built) = built {
            let ours: Built = graph.iter().filter_map(|id| by_id.get(id.as_str())).map(|p| (p.name.clone(), p.version.clone())).collect();
            let missing: Vec<String> = built.difference(&ours).map(|(n, v)| format!("{n} {v}")).collect();
            if !missing.is_empty() {
                return Err(format!("sbom: [{target}] the release build compiles crates the product graph lacks: {}", missing.join(", ")));
            }
        }
        let nodes: BTreeMap<&str, &crate::metadata::Node> = meta.resolve.iter().flat_map(|r| r.nodes.iter()).map(|n| (n.id.as_str(), n)).collect();
        for id in &graph {
            let Some(p) = by_id.get(id.as_str()) else { continue };
            let depends_on: BTreeSet<String> = nodes
                .get(id.as_str())
                .map(|n| {
                    n.deps
                        .iter()
                        .filter(|d| d.dep_kinds.is_empty() || d.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev")))
                        .filter(|d| graph.contains(&d.pkg))
                        .filter_map(|d| by_id.get(d.pkg.as_str()))
                        .map(|d| bom_ref(&d.name, &d.version, d.source.as_deref()))
                        .collect()
                })
                .unwrap_or_default();
            let key = bom_ref(&p.name, &p.version, p.source.as_deref());
            let c = out.entry(key).or_insert_with(|| Component {
                name: p.name.clone(),
                version: p.version.clone(),
                licence: licence(p),
                source: p.source.clone(),
                checksum: p.source.as_ref().and_then(|s| checksums.get(&(p.name.clone(), p.version.clone(), s.clone()))).cloned(),
                targets: BTreeSet::new(),
                depends_on: BTreeSet::new(),
            });
            c.targets.insert((*target).to_string());
            c.depends_on.extend(depends_on);
        }
    }
    Ok(out)
}

/// What `cargo build --release -p dalica -p dalica-cli --target <target>` compiles: `cargo tree`'s
/// packages over normal and build edges.
pub fn built(root: &Path, target: &str) -> Result<Built, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo)
        .current_dir(root)
        .args(["tree", "--locked", "-e", "normal,build", "--target", target, "-p", "dalica", "-p", "dalica-cli", "--prefix", "none", "-f", "{p}"])
        .output()
        .map_err(|e| format!("sbom: cannot run cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(format!("sbom: cargo tree failed for {target}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(parse_tree(&String::from_utf8_lossy(&out.stdout)))
}

/// `name vX.Y.Z [(…)] [(*)]` lines of `cargo tree --prefix none -f {p}`.
pub fn parse_tree(text: &str) -> Built {
    text.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let name = w.next()?;
            let version = w.next()?.strip_prefix('v')?;
            Some((name.to_string(), version.to_string()))
        })
        .collect()
}

fn component_json(c: &Component, kind: &str) -> Value {
    let mut v = json!({
        "type": kind,
        "bom-ref": c.bom_ref(),
        "name": c.name,
        "version": c.version,
        "scope": "required",
        "licenses": match &c.licence {
            Some(expr) => json!([{ "expression": expr }]),
            None => json!([{ "license": { "name": "unknown: no SPDX expression in its manifest" } }]),
        },
        "properties": [
            { "name": "dalica:targets", "value": c.targets.iter().cloned().collect::<Vec<_>>().join(" ") },
            { "name": "dalica:source", "value": c.source.as_deref().unwrap_or("workspace") },
        ],
    });
    if let Some(purl) = c.purl() {
        v["purl"] = json!(purl);
    }
    if let Some(sum) = &c.checksum {
        v["hashes"] = json!([{ "alg": "SHA-256", "content": sum }]);
    }
    v
}

/// The CycloneDX document of one package. `timestamp` (RFC 3339) only when given, so the same
/// commit gives the same bytes.
pub fn bom(package: &ReleasePackage, components: &BTreeMap<String, Component>, timestamp: Option<&str>) -> Result<Value, String> {
    let root = components
        .values()
        .find(|c| c.name == ROOT && c.source.is_none())
        .ok_or_else(|| format!("sbom: `{ROOT}` is not in the product graph of {}", package.name))?;
    let mut metadata = json!({
        "tools": { "components": [{ "type": "application", "name": "cargo xtask sbom", "version": root.version }] },
        "component": component_json(root, "application"),
        "properties": [
            { "name": "dalica:package", "value": package.name },
            { "name": "dalica:targets", "value": package.targets.join(" ") },
        ],
    });
    if let Some(t) = timestamp {
        metadata["timestamp"] = json!(t);
    }
    let others: Vec<Value> = components
        .values()
        .filter(|c| c.bom_ref() != root.bom_ref())
        .map(|c| component_json(c, if c.source.is_none() && c.name == "dalica-cli" { "application" } else { "library" }))
        .collect();
    let dependencies: Vec<Value> =
        components.values().map(|c| json!({ "ref": c.bom_ref(), "dependsOn": c.depends_on.iter().collect::<Vec<_>>() })).collect();
    Ok(json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": metadata,
        "components": others,
        "dependencies": dependencies,
    }))
}

/// The components of every release package, from the real workspace.
/// Each release package with its components.
pub type Packages = Vec<(ReleasePackage, BTreeMap<String, Component>)>;

pub fn all(root: &Path) -> Result<Packages, String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("sbom: cannot read Cargo.lock: {e}"))?;
    let checksums = lock_checksums(&lock);
    let mut cache: BTreeMap<&str, (Metadata, Built)> = BTreeMap::new();
    for t in RELEASE_PACKAGES.iter().flat_map(|p| p.targets.iter()) {
        if !cache.contains_key(t) {
            cache.insert(t, (Metadata::load_for_target(root, t)?, built(root, t)?));
        }
    }
    let mut out = Vec::new();
    for package in RELEASE_PACKAGES {
        let per_target: Vec<TargetGraph> = package.targets.iter().filter_map(|t| cache.get(t).map(|(m, b)| (*t, m, Some(b)))).collect();
        out.push((*package, components(&per_target, &checksums)?));
    }
    Ok(out)
}

/// The file name of a package's SBOM.
pub fn file_name(version: &str, package: &str) -> String {
    format!("dalica-{version}-{package}.cdx.json")
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: cargo xtask sbom [--out <dir>] [--list]";
    let mut out_dir: PathBuf = root.join("target").join("sbom");
    let mut list = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out_dir = it.next().map(PathBuf::from).ok_or(USAGE)?,
            "--list" => list = true,
            other => return Err(format!("sbom: unknown argument `{other}` ({USAGE})")),
        }
    }
    let out_dir = if out_dir.is_absolute() { out_dir } else { std::env::current_dir().map_err(|e| e.to_string())?.join(out_dir) };
    // The release workflow's build date (`DALICA_BUILD_DATE`, YYYY-MM-DD), if set.
    let timestamp = std::env::var("DALICA_BUILD_DATE").ok().filter(|d| d.len() == 10).map(|d| format!("{d}T00:00:00Z"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("sbom: cannot create {}: {e}", out_dir.display()))?;
    for (package, comps) in all(root)? {
        let doc = bom(&package, &comps, timestamp.as_deref())?;
        let version = doc["metadata"]["component"]["version"].as_str().unwrap_or("0.0.0").to_string();
        let path = out_dir.join(file_name(&version, package.name));
        let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{text}\n")).map_err(|e| format!("sbom: cannot write {}: {e}", path.display()))?;
        let unlicensed = comps.values().filter(|c| c.licence.is_none()).count();
        if list {
            for t in package.targets {
                println!("sbom: [{} {t}]", package.name);
                for c in comps.values().filter(|c| c.targets.contains(*t)) {
                    println!("  {} {}  {}", c.name, c.version, c.licence.as_deref().unwrap_or("(no SPDX licence)"));
                }
            }
        }
        let per_target: Vec<String> =
            package.targets.iter().map(|t| format!("{t}: {}", comps.values().filter(|c| c.targets.contains(*t)).count())).collect();
        println!(
            "sbom: {} ({} components; {}; {} B){}",
            path.display(),
            comps.len(),
            per_target.join(", "),
            text.len() + 1,
            if unlicensed > 0 { format!("; {unlicensed} without an SPDX licence") } else { String::new() }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Packages {
        all(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
    }

    fn names_for<'a>(comps: &'a BTreeMap<String, Component>, target: &str) -> BTreeSet<(&'a str, &'a str)> {
        comps.values().filter(|c| c.targets.contains(target)).map(|c| (c.name.as_str(), c.version.as_str())).collect()
    }

    /// For each release target, the SBOM lists exactly the crates `cargo tree` prints for the
    /// release build (`-p dalica -p dalica-cli`, normal and build edges, default features), each
    /// with a licence.
    #[test]
    fn every_target_lists_exactly_what_cargo_tree_builds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (package, comps) in real() {
            for target in package.targets {
                let tree = built(root, target).unwrap();
                assert!(tree.len() > 100, "{target}: {}", tree.len());
                let ours: Built = names_for(&comps, target).into_iter().map(|(n, v)| (n.into(), v.into())).collect();
                assert_eq!(ours, tree, "{target}");
                assert!(comps.values().all(|c| c.licence.is_some()), "{target}: a component without a licence");
            }
        }
    }

    /// `nusb` only where the camera transport builds (Linux, Windows), never the off-by-default
    /// HEVC decoder; both apps in every package; the universal package is both macOS targets.
    #[test]
    fn target_gated_and_feature_gated_crates() {
        for (package, comps) in real() {
            let has = |n: &str| comps.values().any(|c| c.name == n);
            assert_eq!(has("nusb"), !package.name.starts_with("macos"), "{}", package.name);
            assert!(!has("rusty_h265"), "{}", package.name);
            assert!(has("dalica") && has("dalica-cli"), "{}", package.name);
            let dalica = comps.values().find(|c| c.name == "dalica").unwrap();
            assert_eq!(dalica.targets.len(), package.targets.len(), "{}", package.name);
        }
    }

    #[test]
    fn the_document_is_cyclonedx_with_licences_purls_hashes_and_edges() {
        let lock = "[[package]]\nname = \"rsa\"\nversion = \"0.9.8\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"abc123\"\n\n[[package]]\nname = \"dalica\"\nversion = \"1.0.0\"\n";
        let sums = lock_checksums(lock);
        assert_eq!(sums.len(), 1);
        let io = "registry+https://github.com/rust-lang/crates.io-index";
        let doc = json!({
            "packages": [
                {"name": "dalica", "version": "1.0.0", "id": "dalica", "license": "MIT/Apache-2.0", "manifest_path": "/x/a/Cargo.toml"},
                {"name": "dalica-cli", "version": "1.0.0", "id": "dalica-cli", "license": "MIT OR Apache-2.0", "manifest_path": "/x/b/Cargo.toml"},
                {"name": "rsa", "version": "0.9.8", "id": "rsa", "license": "MIT OR Apache-2.0", "source": io, "manifest_path": "/r/Cargo.toml"},
                {"name": "tester", "version": "1.0.0", "id": "tester", "license": "GPL-3.0", "source": io, "manifest_path": "/t/Cargo.toml"}
            ],
            "workspace_members": ["dalica", "dalica-cli"],
            "workspace_root": "/x",
            "resolve": {"nodes": [
                {"id": "dalica", "deps": [{"pkg": "rsa", "dep_kinds": [{"kind": null}]}, {"pkg": "tester", "dep_kinds": [{"kind": "dev"}]}]},
                {"id": "dalica-cli", "deps": []},
                {"id": "rsa", "deps": []},
                {"id": "tester", "deps": []}
            ]}
        });
        let meta = Metadata::parse(doc.to_string().as_bytes()).unwrap();
        let package = ReleasePackage { name: "test-pkg", targets: &["t1", "t2"] };
        let comps = components(&[("t1", &meta, None), ("t2", &meta, None)], &sums).unwrap();
        // The dev-only crate is not shipped.
        assert_eq!(comps.keys().cloned().collect::<Vec<_>>(), ["dalica-cli@1.0.0", "dalica@1.0.0", "pkg:cargo/rsa@0.9.8"]);
        let bom = bom(&package, &comps, None).unwrap();
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["specVersion"], "1.5");
        assert!(bom["metadata"].get("timestamp").is_none());
        assert_eq!(bom["metadata"]["component"]["name"], "dalica");
        assert_eq!(bom["metadata"]["component"]["licenses"][0]["expression"], "MIT OR Apache-2.0");
        let rsa = bom["components"].as_array().unwrap().iter().find(|c| c["name"] == "rsa").unwrap();
        assert_eq!(rsa["purl"], "pkg:cargo/rsa@0.9.8");
        assert_eq!(rsa["hashes"][0]["content"], "abc123");
        assert_eq!(rsa["properties"][0]["value"], "t1 t2");
        let edges = bom["dependencies"].as_array().unwrap().iter().find(|d| d["ref"] == "dalica@1.0.0").unwrap();
        assert_eq!(edges["dependsOn"], json!(["pkg:cargo/rsa@0.9.8"]));
        assert_eq!(super::bom(&package, &comps, Some("2026-10-20T00:00:00Z")).unwrap()["metadata"]["timestamp"], "2026-10-20T00:00:00Z");
        // Kept to what the release build compiles: a crate it leaves out is dropped with its edges;
        // one it compiles that the graph lacks is an error.
        let built: Built = parse_tree("dalica v1.0.0 (/x/a)\ndalica-cli v1.0.0 (/x/b)\ndalica v1.0.0 (/x/a) (*)\n");
        let trimmed = components(&[("t1", &meta, Some(&built))], &sums).unwrap();
        assert_eq!(trimmed.keys().cloned().collect::<Vec<_>>(), ["dalica-cli@1.0.0", "dalica@1.0.0"]);
        assert!(trimmed["dalica@1.0.0"].depends_on.is_empty());
        let mut more = built.clone();
        more.insert(("tester".into(), "1.0.0".into()));
        assert!(components(&[("t1", &meta, Some(&more))], &sums).unwrap_err().contains("tester 1.0.0"));
        // No app in the graph: an error, never a document without its subject.
        assert!(super::bom(&package, &BTreeMap::new(), None).is_err());
    }

    #[test]
    fn file_names_follow_the_artifacts() {
        assert_eq!(file_name("1.0.0", "macos-universal"), "dalica-1.0.0-macos-universal.cdx.json");
    }
}
