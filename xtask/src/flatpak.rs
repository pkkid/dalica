//! `cargo xtask flatpak [--check | --flathub <tag> [--out <dir>]]`: the vendored crate sources of
//! the from-source Flatpak manifest (`packaging/linux/flatpak/org.dalica.Dalica.yml`,
//! `docs/releasing.md` → Flathub).
//!
//! Flatpak builds run without the network, so every crate in `Cargo.lock` becomes a
//! `flatpak-builder` source: the `.crate` archive from static.crates.io with its lock checksum,
//! unpacked into `cargo/vendor/<name>-<version>`, plus the `.cargo-checksum.json` cargo's directory
//! source wants, and one `cargo/config.toml` that replaces crates.io with that directory. This is
//! the layout of flatpak-builder-tools' `flatpak-cargo-generator`, written in Rust so that we own it
//! and the gate needs no Python and no network.
//!
//! - no flag: writes `packaging/linux/flatpak/cargo-sources.json` (gitignored: it is derived from
//!   `Cargo.lock` and would go stale with every lock change), for a local `flatpak-builder` run;
//! - `--check` (in every gate): the sources generate from `Cargo.lock` (every package comes from
//!   crates.io with a checksum; a git or other registry dependency needs this generator extended),
//!   the manifest keeps its local-source markers, and its craft-fonts commit is release.yml's pin;
//! - `--flathub <tag>`: writes the submission files to `--out` (default `target/flathub`): the
//!   manifest with the local `dir` source replaced by a git source pinned to the tag's commit, that
//!   commit and its date as `DALICA_BUILD_SHA`/`DALICA_BUILD_DATE`, and `cargo-sources.json`.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const USAGE: &str = "usage: cargo xtask flatpak [--check | --flathub <tag> [--out <dir>]]";
const MANIFEST: &str = "packaging/linux/flatpak/org.dalica.Dalica.yml";
const SOURCES: &str = "packaging/linux/flatpak/cargo-sources.json";
const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const REPOSITORY: &str = "https://github.com/pkkid/dalica.git";
const BEGIN: &str = "# BEGIN local source";
const END: &str = "# END local source";

/// One `[[package]]` of `Cargo.lock`.
#[derive(Debug, Default, PartialEq)]
struct Locked {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn parse_lock(lock: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut cur: Option<Locked> = None;
    for line in lock.lines().map(str::trim) {
        if line.starts_with('[') {
            out.extend(cur.take());
            if line == "[[package]]" {
                cur = Some(Locked::default());
            }
            continue;
        }
        let (Some(p), Some((key, value))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "name" => p.name = value,
            "version" => p.version = value,
            "source" => p.source = Some(value),
            "checksum" => p.checksum = Some(value),
            _ => {}
        }
    }
    out.extend(cur);
    out
}

/// The flatpak-builder sources that vendor every crate of `lock`; workspace members (no `source`)
/// come from the checkout itself.
pub fn sources(lock: &str) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for p in parse_lock(lock) {
        let Some(source) = &p.source else { continue };
        let id = format!("{} {}", p.name, p.version);
        if source != CRATES_IO {
            problems.push(format!("{id}: source {source} is not crates.io (extend xtask/src/flatpak.rs to vendor it)"));
            continue;
        }
        let Some(sum) = p.checksum.as_deref().filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())) else {
            problems.push(format!("{id}: no SHA-256 checksum in Cargo.lock"));
            continue;
        };
        let safe = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b));
        if !safe(&p.name) || !safe(&p.version) {
            problems.push(format!("{id}: unexpected characters in the name or version"));
            continue;
        }
        let dest = format!("cargo/vendor/{}-{}", p.name, p.version);
        out.push(json!({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": format!("https://static.crates.io/crates/{0}/{0}-{1}.crate", p.name, p.version),
            "sha256": sum,
            "dest": dest,
        }));
        out.push(json!({
            "type": "inline",
            "contents": json!({ "package": sum, "files": {} }).to_string(),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        }));
    }
    if !problems.is_empty() {
        return Err(format!("flatpak: Cargo.lock cannot be vendored:\n  {}", problems.join("\n  ")));
    }
    out.push(json!({
        "type": "inline",
        "contents": "[source.vendored-sources]\ndirectory = \"cargo/vendor\"\n\n[source.crates-io]\nreplace-with = \"vendored-sources\"\n",
        "dest": "cargo",
        "dest-filename": "config.toml",
    }));
    Ok(out)
}

/// `cargo-sources.json`'s text.
fn sources_json(lock: &str) -> Result<String, String> {
    let list = sources(lock)?;
    serde_json::to_string_pretty(&list).map(|s| s + "\n").map_err(|e| format!("flatpak: {e}"))
}

/// The manifest with the lines from [`BEGIN`] to [`END`] replaced by a git source at `tag`/`commit`
/// and the empty provenance variables filled in.
fn flathub_manifest(manifest: &str, tag: &str, commit: &str, date: &str) -> Result<String, String> {
    let lines: Vec<&str> = manifest.lines().collect();
    let begin = lines.iter().position(|l| l.trim().starts_with(BEGIN)).ok_or_else(|| format!("flatpak: {MANIFEST} lacks the line `{BEGIN}`"))?;
    let end = lines
        .iter()
        .position(|l| l.trim().starts_with(END))
        .filter(|e| *e > begin)
        .ok_or_else(|| format!("flatpak: {MANIFEST} lacks the line `{END}` after `{BEGIN}`"))?;
    let indent: String = lines.get(begin).map(|l| l.chars().take_while(|c| *c == ' ').collect()).unwrap_or_default();
    let mut out = Vec::new();
    let (mut sha, mut day) = (false, false);
    for (i, line) in lines.iter().enumerate() {
        if i == begin {
            out.push(format!("{indent}- type: git"));
            out.push(format!("{indent}  url: {REPOSITORY}"));
            out.push(format!("{indent}  tag: {tag}"));
            out.push(format!("{indent}  commit: {commit}"));
        } else if i > begin && i <= end {
            continue;
        } else if line.trim() == "DALICA_BUILD_SHA: ''" {
            out.push(line.replace("''", &format!("'{commit}'")));
            sha = true;
        } else if line.trim() == "DALICA_BUILD_DATE: ''" {
            out.push(line.replace("''", &format!("'{date}'")));
            day = true;
        } else {
            out.push((*line).to_string());
        }
    }
    if !sha || !day {
        return Err(format!("flatpak: {MANIFEST} lacks the lines `DALICA_BUILD_SHA: ''` and `DALICA_BUILD_DATE: ''`"));
    }
    Ok(out.join("\n") + "\n")
}

/// The `commit:` of the craft-fonts source in the manifest and `CRAFT_FONTS_REF` in release.yml.
fn font_pins(manifest: &str, workflow: &str) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = manifest.lines().map(str::trim).collect();
    let in_manifest = lines
        .iter()
        .position(|l| l.starts_with("url:") && l.contains("storytold/craft-fonts"))
        .and_then(|i| lines.get(i + 1..)?.iter().take_while(|l| !l.starts_with("- ")).find_map(|l| l.strip_prefix("commit:")))
        .map(|c| c.trim().to_string());
    let in_workflow = workflow.lines().find_map(|l| l.trim().strip_prefix("CRAFT_FONTS_REF:")).map(|c| c.trim().to_string());
    (in_manifest, in_workflow)
}

fn check(root: &Path) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("flatpak: cannot read {p}: {e}"));
    let lock = read("Cargo.lock")?;
    let manifest = read(MANIFEST)?;
    let n = sources(&lock)?.len().saturating_sub(1) / 2;
    flathub_manifest(&manifest, "v0.0.0", "0", "2026-01-01")?;
    match font_pins(&manifest, &read(RELEASE_WORKFLOW)?) {
        (Some(a), Some(b)) if a == b => {}
        (a, b) => return Err(format!("flatpak: the craft-fonts commit in {MANIFEST} ({a:?}) is not CRAFT_FONTS_REF in {RELEASE_WORKFLOW} ({b:?})")),
    }
    println!("flatpak: ok ({n} crates vendored from Cargo.lock; the manifest's craft-fonts pin is release.yml's)");
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| format!("flatpak: git: {e}"))?;
    if !out.status.success() {
        return Err(format!("flatpak: git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("flatpak: cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("flatpak: cannot write {}: {e}", path.display()))
}

/// The submission files for `tag`, built from the tag's own Cargo.lock and manifest.
fn flathub(root: &Path, tag: &str, out: &Path) -> Result<(), String> {
    let commit = git(root, &["rev-parse", "--verify", &format!("{tag}^{{commit}}")])?;
    let date = git(root, &["log", "-1", "--format=%cs", &commit])?;
    let lock = git(root, &["show", &format!("{commit}:Cargo.lock")])?;
    // A tag from before this manifest existed takes today's.
    let manifest = git(root, &["show", &format!("{commit}:{MANIFEST}")])
        .or_else(|_| std::fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("flatpak: cannot read {MANIFEST}: {e}")))?;
    let yml = out.join("org.dalica.Dalica.yml");
    write(&yml, &flathub_manifest(&manifest, tag, &commit, &date)?)?;
    write(&out.join("cargo-sources.json"), &sources_json(&(lock + "\n"))?)?;
    println!("flatpak: wrote {} and cargo-sources.json for {tag} ({commit}, {date})", yml.display());
    Ok(())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            let lock = std::fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("flatpak: cannot read Cargo.lock: {e}"))?;
            write(&root.join(SOURCES), &sources_json(&lock)?)?;
            println!("flatpak: wrote {SOURCES}");
            Ok(())
        }
        ["--check"] => check(root),
        ["--flathub", tag] => flathub(root, tag, &root.join("target/flathub")),
        ["--flathub", tag, "--out", out] => flathub(root, tag, Path::new(out)),
        ["-h" | "--help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn lock(extra: &str) -> String {
        format!(
            "version = 4\n\n[[package]]\nname = \"dalica\"\nversion = \"1.0.0\"\ndependencies = [\n \"adler2\",\n]\n\n\
             [[package]]\nname = \"adler2\"\nversion = \"2.0.1\"\nsource = \"{CRATES_IO}\"\nchecksum = \"{SUM}\"\n{extra}"
        )
    }

    #[test]
    fn every_registry_crate_is_vendored_with_its_checksum() {
        let list = sources(&lock("")).unwrap();
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!(list[0]["url"], "https://static.crates.io/crates/adler2/adler2-2.0.1.crate");
        assert_eq!(list[0]["sha256"], SUM);
        assert_eq!(list[0]["dest"], "cargo/vendor/adler2-2.0.1");
        assert_eq!(list[1]["dest-filename"], ".cargo-checksum.json");
        assert!(list[1]["contents"].as_str().unwrap().contains(SUM));
        assert_eq!(list[2]["dest-filename"], "config.toml");
    }

    #[test]
    fn a_lock_the_generator_cannot_vendor_is_refused() {
        let git = "\n[[package]]\nname = \"x\"\nversion = \"0.1.0\"\nsource = \"git+https://example.com/x#abc\"\n";
        assert!(sources(&lock(git)).unwrap_err().contains("x 0.1.0: source git+"));
        let bad = format!("\n[[package]]\nname = \"../y\"\nversion = \"1.0.0\"\nsource = \"{CRATES_IO}\"\nchecksum = \"{SUM}\"\n");
        assert!(sources(&lock(&bad)).unwrap_err().contains("unexpected characters"));
        let unsummed = format!("\n[[package]]\nname = \"z\"\nversion = \"1.0.0\"\nsource = \"{CRATES_IO}\"\n");
        assert!(sources(&lock(&unsummed)).unwrap_err().contains("z 1.0.0: no SHA-256"));
    }

    #[test]
    fn the_flathub_manifest_pins_the_tag_and_keeps_the_rest() {
        let m = "env:\n  DALICA_BUILD_SHA: ''\n  DALICA_BUILD_DATE: ''\nsources:\n  # BEGIN local source (x)\n  - type: dir\n    path: ../../..\n  # END local source\n  - cargo-sources.json\n";
        let out = flathub_manifest(m, "v1.0.0", "abc123", "2026-10-20").unwrap();
        assert_eq!(
            out,
            "env:\n  DALICA_BUILD_SHA: 'abc123'\n  DALICA_BUILD_DATE: '2026-10-20'\nsources:\n  - type: git\n    url: https://github.com/pkkid/dalica.git\n    tag: v1.0.0\n    commit: abc123\n  - cargo-sources.json\n"
        );
        assert!(flathub_manifest("sources:\n  - type: dir\n", "v1", "a", "d").is_err());
    }
}
