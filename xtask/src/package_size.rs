//! `cargo xtask package-size [--dist DIR] [--json]`: the release packages' size budget (M16.27,
//! `docs/releasing.md` → Package sizes). Each package may be at most half its 1.0.0 size, the
//! size 1.0.0 grew to by embedding every craft-fonts face whole in both binaries.
//!
//! Without `--dist` (Linux; the review's `cargo xtask budgets`): builds the release tarball as the
//! release does (`packaging/linux/package.sh --formats tar`, with `CRAFT_FONTS_REQUIRED=1`, so
//! `CRAFT_FONTS_DIR` must name a craft-fonts checkout: a package without the fonts says nothing),
//! measures it, and checks that `dalica` carries the fallback faces and `dalica-cli` none. With
//! `--dist DIR`: measures the packages already in `DIR` (a release run's `.tar.gz`, `.msi` and
//! `.dmg`). `--json` adds the report line `cargo xtask budgets` judges ([`judge`]).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const USAGE: &str = "usage: cargo xtask package-size [--dist DIR] [--json]";

/// The packages measured, by the end of their file name, with their 1.0.0 size in bytes (the
/// v1.0.0 release's assets, 2026-11-05).
pub const PACKAGES_1_0_0: &[(&str, u64)] = &[
    ("-linux-x86_64.tar.gz", 97_695_476),
    ("-linux-aarch64.tar.gz", 93_975_093),
    ("-windows-x64.msi", 81_707_008),
    ("-macos-universal.dmg", 100_592_249),
];

/// A string only the embedded Noto Sans CJK SC face holds (its CFF font name): in `dalica`, never
/// in `dalica-cli`.
pub const FONT_MARKER: &[u8] = b"NotoSansCJKsc-Regular";

/// A package's budget: half its 1.0.0 size, by its file name; `None` for a file that is not a
/// measured package.
pub fn budget(name: &str) -> Option<u64> {
    PACKAGES_1_0_0.iter().find(|(end, _)| name.starts_with("dalica-") && name.ends_with(end)).map(|(_, size)| size / 2)
}

/// Whether `haystack` holds `needle`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

/// The report of `packages` (`(file name, bytes)`) and, when the binaries were checked, whether
/// `dalica` and `dalica-cli` hold the fonts: one row per package.
pub fn report(packages: &[(String, u64)], fonts: Option<(bool, bool)>) -> Value {
    let rows: Vec<Value> =
        packages.iter().filter_map(|(name, bytes)| budget(name).map(|b| json!({ "name": name, "bytes": bytes, "budget": b }))).collect();
    let mut out = json!({ "mode": "release", "rows": rows });
    if let (Some((app, cli)), Some(map)) = (fonts, out.as_object_mut()) {
        map.insert("app_fonts".into(), json!(app));
        map.insert("cli_fonts".into(), json!(cli));
    }
    out
}

/// The verdict on a [`report`] line in `stdout` (the budgets' judge; a planted slowdown means
/// nothing here): every package within its budget, and the fonts in the app only.
pub fn judge(stdout: &str, _plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("package-size printed no JSON report with `rows`")?;
    let rows = report.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
    if rows.is_empty() {
        return Err("package-size measured no package".into());
    }
    let mut parts = Vec::new();
    let mut over = Vec::new();
    for row in &rows {
        let name = row.get("name").and_then(Value::as_str).unwrap_or("?");
        let (Some(bytes), Some(b)) = (row.get("bytes").and_then(Value::as_u64), row.get("budget").and_then(Value::as_u64)) else {
            return Err(format!("package-size: `{name}` without `bytes` or `budget`"));
        };
        let line = format!("{name} {} (budget {})", mb(bytes), mb(b));
        if bytes > b {
            over.push(line);
        } else {
            parts.push(line);
        }
    }
    if report.get("app_fonts").and_then(Value::as_bool) == Some(false) {
        over.push("dalica holds no fallback faces: built without CRAFT_FONTS_DIR?".into());
    }
    if report.get("cli_fonts").and_then(Value::as_bool) == Some(true) {
        over.push("dalica-cli holds the fallback faces".into());
    }
    if over.is_empty() { Ok(parts.join("; ")) } else { Err(over.join("; ")) }
}

/// The measured packages in `dist`, sorted by name.
fn packages_in(dist: &Path) -> Result<Vec<(String, u64)>, String> {
    let entries = std::fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if budget(&name).is_some() {
            let bytes = entry.metadata().map_err(|e| format!("{name}: {e}"))?.len();
            out.push((name, bytes));
        }
    }
    out.sort();
    Ok(out)
}

/// Builds the Linux tarball into `<target>/package-size` and returns that folder and whether the
/// release `dalica` and `dalica-cli` hold the fonts.
fn build_tarball(root: &Path) -> Result<(PathBuf, (bool, bool)), String> {
    if !cfg!(target_os = "linux") {
        return Err("package-size builds the Linux tarball; on Windows and macOS pass --dist with built packages".into());
    }
    if std::env::var_os("CRAFT_FONTS_DIR").is_none_or(|v| v.is_empty()) {
        return Err("package-size: set CRAFT_FONTS_DIR to a craft-fonts checkout at the release pin (docs/releasing.md): a package without the fonts says nothing".into());
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()).map_or_else(|| root.join("target"), PathBuf::from);
    let dist = target.join("package-size");
    std::fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    for (name, _) in packages_in(&dist)? {
        std::fs::remove_file(dist.join(&name)).map_err(|e| format!("{name}: {e}"))?;
    }
    println!("==> package-size: packaging/linux/package.sh --formats tar into {}", dist.display());
    let status = Command::new(root.join("packaging/linux/package.sh"))
        .args(["--formats", "tar"])
        .current_dir(root)
        .env("DIST", &dist)
        .env("CRAFT_FONTS_REQUIRED", "1")
        .env("DALICA_INSTALL_TOOLS", "0")
        .stdout(std::process::Stdio::inherit())
        .status()
        .map_err(|e| format!("cannot run packaging/linux/package.sh: {e}"))?;
    if !status.success() {
        return Err(format!("packaging/linux/package.sh failed ({status})"));
    }
    let holds = |name: &str| -> Result<bool, String> {
        let path = target.join("release").join(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(contains(&bytes, FONT_MARKER))
    };
    Ok((dist, (holds("dalica")?, holds("dalica-cli")?)))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut dist: Option<PathBuf> = None;
    let mut json_out = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dist" => dist = Some(it.next().map(PathBuf::from).ok_or(USAGE)?),
            "--json" => json_out = true,
            _ => return Err(USAGE.into()),
        }
    }
    let (dist, fonts) = match dist {
        Some(d) => (d, None),
        None => {
            let (d, f) = build_tarball(root)?;
            (d, Some(f))
        }
    };
    let packages = packages_in(&dist)?;
    if packages.is_empty() {
        return Err(format!("package-size: no package in {}", dist.display()));
    }
    for (name, bytes) in &packages {
        let b = budget(name).unwrap_or_default();
        let verdict = if *bytes <= b { "ok" } else { "OVER" };
        println!("  {verdict}: {name} {} ({bytes} bytes), budget {} (half of 1.0.0's)", mb(*bytes), mb(b));
    }
    if let Some((app, cli)) = fonts {
        println!("  dalica holds the fallback faces: {app}; dalica-cli: {cli}");
    }
    let report = report(&packages, fonts);
    if json_out {
        println!("{report}");
    }
    judge(&report.to_string(), 0.0).map(|line| println!("package-size: {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_package_may_be_half_its_1_0_0_size() {
        assert_eq!(budget("dalica-1.1.0-linux-x86_64.tar.gz"), Some(48_847_738));
        assert_eq!(budget("dalica-1.1.0-windows-x64.msi"), Some(40_853_504));
        assert_eq!(budget("dalica-1.1.0-macos-universal.dmg"), Some(50_296_124));
        assert_eq!(budget("dalica-1.1.0-linux-x86_64.deb"), None);
        assert_eq!(budget("dalica-1.1.0-linux-x86_64.tar.gz.sha256"), None);
        assert_eq!(budget("other-1.1.0-linux-x86_64.tar.gz"), None);
    }

    #[test]
    fn the_judge_fails_a_package_over_budget_and_fonts_in_the_cli() {
        let ok = vec![("dalica-1.1.0-linux-x86_64.tar.gz".to_string(), 45_000_000)];
        let line = judge(&report(&ok, Some((true, false))).to_string(), 0.0).unwrap();
        assert!(line.contains("45.0 MB") && line.contains("48.8 MB"), "{line}");
        let over = vec![("dalica-1.1.0-windows-x64.msi".to_string(), 41_000_000)];
        assert!(judge(&report(&over, None).to_string(), 0.0).unwrap_err().contains("msi"));
        assert!(judge(&report(&ok, Some((true, true))).to_string(), 0.0).unwrap_err().contains("dalica-cli"));
        assert!(judge(&report(&ok, Some((false, false))).to_string(), 0.0).unwrap_err().contains("CRAFT_FONTS_DIR"));
        assert!(judge(&report(&[], None).to_string(), 0.0).is_err());
        assert!(judge("no report", 0.0).is_err());
    }

    #[test]
    fn the_font_marker_is_found_only_where_it_is() {
        let mut binary = vec![0u8; 1000];
        assert!(!contains(&binary, FONT_MARKER));
        binary.splice(500..500, FONT_MARKER.iter().copied());
        assert!(contains(&binary, FONT_MARKER));
        assert!(!contains(&binary, b""));
    }
}
