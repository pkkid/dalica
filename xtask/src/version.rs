//! `cargo xtask version [set X.Y.Z[-pre]]`: the workspace version, its single source of truth
//! (`docs/releasing.md` → Cutting a release).
//!
//! Adapted from PhotoCraft `xtask/src/version.rs` (revision bb17dd55); MIT OR Apache-2.0.
//!
//! The version lives in `[workspace.package] version` in the root `Cargo.toml`; every crate
//! inherits it with `version.workspace = true`. `set` rewrites that one line and the `version` line
//! of every workspace member in `Cargo.lock` (the `[[package]]` entries without a `source`), and
//! nothing else, then asks cargo whether the lock file is still current (`cargo metadata --locked
//! --offline`); if it is not, both files are put back as they were.

use std::path::Path;
use std::process::{Command, Stdio};

const USAGE: &str = "usage: cargo xtask version [set X.Y.Z[-pre]]";

/// The `version = "…"` value inside `[workspace.package]`.
pub fn read(manifest: &str) -> Result<String, String> {
    let (_, line) = find_line(manifest)?;
    let v = line.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_string()).unwrap_or_default();
    if v.is_empty() {
        return Err("version: empty [workspace.package] version".into());
    }
    Ok(v)
}

/// `line` with its value replaced by `"new"`, keeping its indentation and line ending.
fn version_line(line: &str, new: &str) -> String {
    let body = line.trim_end_matches(['\r', '\n']);
    let eol = line.get(body.len()..).unwrap_or("");
    let indent: String = body.chars().take_while(|c| c.is_whitespace()).collect();
    format!("{indent}version = \"{new}\"{eol}")
}

/// `manifest` with the workspace version replaced; every other byte unchanged.
pub fn replace(manifest: &str, new: &str) -> Result<String, String> {
    validate(new)?;
    let (idx, _) = find_line(manifest)?;
    Ok(manifest.split_inclusive('\n').enumerate().map(|(i, line)| if i == idx { version_line(line, new) } else { line.to_string() }).collect())
}

/// `lock` with the `version` of every workspace member that is at `old` set to `new`: the
/// `[[package]]` entries without a `source` line. Every other byte unchanged. Returns the text and
/// how many entries changed.
pub fn replace_in_lock(lock: &str, old: &str, new: &str) -> Result<(String, usize), String> {
    validate(new)?;
    let lines: Vec<&str> = lock.split_inclusive('\n').collect();
    // Each `[[package]]` block: [start, end) line indices.
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.trim_end() == "[[package]]").map(|(i, _)| i).collect();
    let mut targets = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(lines.len());
        let block = lines.get(start..end).unwrap_or_default();
        let key = |l: &&str, name: &str| l.trim_start().strip_prefix(name).is_some_and(|r| r.trim_start().starts_with('='));
        if block.iter().any(|l| key(l, "source")) {
            continue;
        }
        let want = format!("\"{old}\"");
        if let Some(i) = block.iter().position(|l| key(l, "version") && l.split_once('=').is_some_and(|(_, v)| v.trim() == want)) {
            targets.push(start + i);
        }
    }
    let text = lines.iter().enumerate().map(|(i, line)| if targets.contains(&i) { version_line(line, new) } else { (*line).to_string() }).collect();
    Ok((text, targets.len()))
}

/// A semantic version without build metadata: `MAJOR.MINOR.PATCH` and an optional pre-release
/// (`-rc.1`): dot-separated identifiers of ASCII letters, digits and hyphens, numeric ones without
/// leading zeros.
pub fn validate(v: &str) -> Result<(), String> {
    let bad = || Err(format!("version: `{v}` is not a semantic version like 1.2.3 or 1.2.3-rc.1"));
    let numeric = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0'));
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    if nums.len() != 3 || !nums.iter().all(|n| numeric(n)) || nums.iter().any(|n| n.len() > 9) {
        return bad();
    }
    if let Some(p) = pre {
        let ident_ok = |id: &str| {
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && (!id.bytes().all(|b| b.is_ascii_digit()) || numeric(id))
        };
        if p.is_empty() || !p.split('.').all(ident_ok) {
            return bad();
        }
    }
    Ok(())
}

/// Index and text of the `version` line in `[workspace.package]`.
fn find_line(manifest: &str) -> Result<(usize, &str), String> {
    let mut in_section = false;
    for (i, line) in manifest.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[workspace.package]";
            continue;
        }
        if in_section
            && let Some(rest) = t.strip_prefix("version")
            && rest.trim_start().starts_with('=')
        {
            return Ok((i, line));
        }
    }
    Err("version: no `version = \"…\"` in [workspace.package] of the root Cargo.toml".into())
}

/// Writes `text` to `path` through a temporary file beside it and a rename, so a failure never
/// leaves a half-written manifest (one broken manifest breaks every build in the workspace).
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.xtask-version.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("version: cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("version: cannot replace {}: {e}", path.display())
    })
}

/// What `set` changed.
#[derive(Debug, PartialEq)]
pub struct Changed {
    pub old: String,
    pub members: usize,
}

/// Sets the version in `root`'s `Cargo.toml` and `Cargo.lock` (when there is one), touching only
/// their version lines. `check` is asked afterwards; if it fails, both files are restored.
pub fn set_in(root: &Path, new: &str, check: impl FnOnce(&Path) -> Result<(), String>) -> Result<Changed, String> {
    validate(new)?;
    let manifest_path = root.join("Cargo.toml");
    let lock_path = root.join("Cargo.lock");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|e| format!("version: cannot read {}: {e}", manifest_path.display()))?;
    let lock = match std::fs::read_to_string(&lock_path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("version: cannot read {}: {e}", lock_path.display())),
    };
    let old = read(&manifest)?;
    let new_manifest = replace(&manifest, new)?;
    let new_lock = lock.as_deref().map(|l| replace_in_lock(l, &old, new)).transpose()?;
    if let (Some((_, 0)), true) = (&new_lock, old != new) {
        return Err(format!("version: Cargo.lock has no workspace member at {old}; run `cargo update --workspace` first"));
    }
    write_atomic(&manifest_path, &new_manifest)?;
    if let Some((text, _)) = &new_lock
        && let Err(e) = write_atomic(&lock_path, text)
    {
        let _ = write_atomic(&manifest_path, &manifest);
        return Err(e);
    }
    if let Err(e) = check(root) {
        let restored = write_atomic(&manifest_path, &manifest).and_then(|()| lock.as_deref().map_or(Ok(()), |l| write_atomic(&lock_path, l)));
        let note = match restored {
            Ok(()) => "Cargo.toml and Cargo.lock were restored".to_string(),
            Err(r) => format!("restoring them failed too: {r}"),
        };
        return Err(format!("{e}\nversion: {note}"));
    }
    Ok(Changed { old, members: new_lock.map_or(0, |(_, n)| n) })
}

/// Asks cargo whether `root`'s lock file is current, without the network.
fn cargo_check(root: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("version: cannot run cargo metadata: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("version: cargo metadata --locked refused the new lock file:\n{}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    match args {
        [] => {
            let path = root.join("Cargo.toml");
            let text = std::fs::read_to_string(&path).map_err(|e| format!("version: cannot read {}: {e}", path.display()))?;
            println!("{}", read(&text)?);
            Ok(())
        }
        [set, new] if set == "set" => {
            let changed = set_in(root, new, cargo_check)?;
            println!("version: {} -> {new} (Cargo.toml and {} workspace members in Cargo.lock)", changed.old, changed.members);
            Ok(())
        }
        [h] if h == "-h" || h == "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace.dependencies]\nfoo = { version = \"1\" }\n";

    #[test]
    fn reads_the_workspace_version() {
        assert_eq!(read(MANIFEST).unwrap(), "0.1.0");
        assert!(read("[package]\nversion = \"1.0.0\"\n").is_err());
        assert!(read("[workspace.package]\nversion = \"\"\n").is_err());
        assert!(read("").is_err());
    }

    #[test]
    fn replaces_only_the_workspace_version() {
        let out = replace(MANIFEST, "1.2.3-rc.1").unwrap();
        assert_eq!(read(&out).unwrap(), "1.2.3-rc.1");
        assert_eq!(out, MANIFEST.replace("version = \"0.1.0\"", "version = \"1.2.3-rc.1\""));
        let crlf = MANIFEST.replace('\n', "\r\n");
        assert_eq!(replace(&crlf, "2.0.0").unwrap(), crlf.replace("\"0.1.0\"", "\"2.0.0\""));
    }

    #[test]
    fn validates_versions() {
        for ok in ["0.0.0", "0.1.0", "1.20.300", "1.0.0-rc.1", "1.0.0-alpha", "1.0.0-x-y.2", "1.0.0-0", "1.0.0-0a"] {
            assert!(validate(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "1",
            "1.0",
            "1.0.0.0",
            "v1.0.0",
            "01.0.0",
            "1.00.0",
            "1.0.0-",
            "1.0.0-a..b",
            "1.0.0-rc.01",
            "1.0.0+meta",
            "1.a.0",
            " 1.0.0",
            "1.0.0 ",
            "1.0.0-é",
            "1.0.0\"",
            "1.0.0\n",
            "-1.0.0",
            "1.0.-1",
            "9999999999.0.0",
        ] {
            assert!(validate(bad).is_err(), "{bad:?}");
            assert!(replace(MANIFEST, bad).is_err(), "{bad:?}");
        }
    }

    /// The real `Cargo.toml` and `Cargo.lock`, copied: `set 0.1.0-rc.1` changes only version lines,
    /// every workspace member's in the lock, and reads back; a bad version changes nothing.
    #[test]
    fn set_on_a_copied_workspace_rewrites_only_the_version_lines() {
        let root = crate::workspace_root();
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
        let old = read(&manifest).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), &manifest).unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), &lock).unwrap();

        for bad in ["0.1", "v0.1.0", "0.1.0+b", "latest"] {
            assert!(set_in(dir.path(), bad, |_| panic!("not checked")).is_err(), "{bad}");
        }
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(), manifest);
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(), lock);

        let changed = set_in(dir.path(), "0.1.0-rc.1", |_| Ok(())).unwrap();
        let members = lock.matches("[[package]]").count() - lock.matches("\nsource = ").count();
        assert_eq!(changed, Changed { old: old.clone(), members });
        assert!(members >= 20, "{members} workspace members");

        let new_manifest = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        let new_lock = std::fs::read_to_string(dir.path().join("Cargo.lock")).unwrap();
        assert_eq!(read(&new_manifest).unwrap(), "0.1.0-rc.1");
        for (before, after, want) in [(&manifest, &new_manifest, 1), (&lock, &new_lock, members)] {
            let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
            assert_eq!(b.len(), a.len());
            let diff: Vec<(&str, &str)> = b.iter().zip(&a).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
            assert_eq!(diff.len(), want);
            for (x, y) in diff {
                assert_eq!((x, y), (format!("version = \"{old}\"").as_str(), "version = \"0.1.0-rc.1\""));
            }
        }
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        // Setting it back gives the original bytes.
        set_in(dir.path(), &old, |_| Ok(())).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(), manifest);
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(), lock);
    }

    #[test]
    fn a_failed_check_restores_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"foo\"\nversion = \"0.1.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
        std::fs::write(dir.path().join("Cargo.toml"), MANIFEST).unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), lock).unwrap();
        let e = set_in(dir.path(), "0.2.0", |_| Err("stale lock".into())).unwrap_err();
        assert!(e.contains("stale lock") && e.contains("restored"), "{e}");
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(), MANIFEST);
        assert_eq!(std::fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(), lock);
        // The registry package at the same version is not a member and keeps its version.
        let (out, n) = replace_in_lock(lock, "0.1.0", "0.2.0").unwrap();
        assert_eq!(n, 1);
        assert_eq!(out, lock.replacen("version = \"0.1.0\"", "version = \"0.2.0\"", 1));
    }

    #[test]
    fn a_workspace_without_a_lock_or_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_in(dir.path(), "1.0.0", |_| Ok(())).is_err(), "no Cargo.toml");
        std::fs::write(dir.path().join("Cargo.toml"), MANIFEST).unwrap();
        assert_eq!(set_in(dir.path(), "1.0.0", |_| Ok(())).unwrap(), Changed { old: "0.1.0".into(), members: 0 });
        assert!(!dir.path().join("Cargo.lock").exists());
        // A lock without the members at the old version is refused before anything is written.
        std::fs::write(dir.path().join("Cargo.lock"), "version = 4\n").unwrap();
        assert!(set_in(dir.path(), "2.0.0", |_| Ok(())).is_err());
        assert_eq!(read(&std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap()).unwrap(), "1.0.0");
    }

    #[test]
    fn the_command_line() {
        let root = crate::workspace_root();
        assert!(run(&root, &[]).is_ok());
        for bad in [&["set"][..], &["set", "1.0"], &["frob"], &["set", "1.0.0", "x"]] {
            let args: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            assert!(run(&root, &args).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_real_manifest_has_a_valid_version() {
        let text = std::fs::read_to_string(crate::workspace_root().join("Cargo.toml")).unwrap();
        validate(&read(&text).unwrap()).unwrap();
    }
}
