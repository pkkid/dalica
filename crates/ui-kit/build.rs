//! Optional build input: with `CRAFT_FONTS_DIR=<storytold/craft-fonts checkout>`, embeds every face
//! of its `fonts/manifest.txt` as `CRAFT_FONTS` (`src/fonts.rs`); unset, `CRAFT_FONTS` is empty and
//! the UI draws with Inter alone (`docs/assets.md` §4). craft-fonts is never a Cargo dependency, and
//! this script only reads a local directory (no network). `CRAFT_FONTS_REQUIRED=1` (release builds)
//! turns a missing directory, manifest, face or required UI face into a build error naming it.
//!
//! Adapted from LightCraft `crates/engine/build.rs` (revision d999e382); MIT OR Apache-2.0. Changed
//! for Dalica: no wasm32 subset (Dalica has no web build), no warning when `CRAFT_FONTS_DIR` is
//! unset (a developer build with Inter alone is the normal case), the licence fields carried for
//! the packages, and the logic split into [`generate`] so `tests/fonts.rs` can test it.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The UI faces a release must carry: family, style and the script it is required for. Thai is the
/// second language of 1.0; Japanese and Chinese file names are common in any language.
pub const REQUIRED_FACES: &[(&str, &str, &str)] =
    &[("BIZ UDPGothic", "Regular", "Jpan"), ("Noto Sans CJK SC", "Regular", "Hans"), ("Noto Sans Thai", "Regular", "Thai")];

/// At most this many manifest lines are read (the pinned manifest has 7).
const MAX_FACES: usize = 64;

#[allow(dead_code)]
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let required = std::env::var_os("CRAFT_FONTS_REQUIRED").is_some_and(|v| !v.is_empty() && v != "0");
    let dir = std::env::var_os("CRAFT_FONTS_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).map(resolve);
    let out = generate(dir.as_deref(), required);
    for path in &out.watched {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    for warning in &out.warnings {
        println!("cargo::warning={warning}");
    }
    for error in &out.errors {
        println!("cargo::error={error}");
    }
    let file = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&file, &out.source) {
        println!("cargo::error=writing {}: {e}", file.display());
    }
}

/// What [`generate`] made: the Rust source of `CRAFT_FONTS`, the files to watch, and the warnings
/// and errors to print.
#[derive(Debug, Default)]
pub struct Generated {
    pub source: String,
    pub embedded: usize,
    pub watched: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// One manifest line: `family | style | file | scripts | licence | licence file | sha256 | source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub family: String,
    pub style: String,
    pub file: String,
    pub scripts: Vec<String>,
    pub licence: String,
    pub licence_file: String,
}

/// The `CRAFT_FONTS` source for the checkout at `dir` (none: empty). A broken input is a warning,
/// or with `required` an error naming the directory and what is missing; either way the source is
/// valid Rust.
pub fn generate(dir: Option<&Path>, required: bool) -> Generated {
    let mut out = Generated::default();
    let mut entries = String::new();
    match dir {
        None if required => {
            out.errors.push("CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is missing: set it to a storytold/craft-fonts checkout".into())
        }
        None => {}
        Some(dir) => match embed(dir, required, &mut out.watched) {
            Ok((text, count)) => {
                entries = text;
                out.embedded = count;
            }
            Err(e) if required => out.errors.push(format!("CRAFT_FONTS_DIR={}: {e}", dir.display())),
            Err(e) => out.warnings.push(format!(
                "building without craft-fonts: CRAFT_FONTS_DIR={}: {e}; Chinese, Japanese and Thai text will show boxes",
                dir.display()
            )),
        },
    }
    out.source = format!("pub static CRAFT_FONTS: &[CraftFont] = &[\n{entries}];\n");
    out
}

/// Parses a manifest's text: comments and blank lines skipped, a short line an error.
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    if lines.len() > MAX_FACES {
        return Err(format!("{} faces in fonts/manifest.txt, more than {MAX_FACES}", lines.len()));
    }
    lines
        .into_iter()
        .map(|line| {
            let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
            let [family, style, file, scripts, licence, licence_file, ..] = f.as_slice() else {
                return Err(format!("malformed manifest line: {line}"));
            };
            let scripts: Vec<String> = scripts.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            if family.is_empty() || style.is_empty() || file.is_empty() || scripts.is_empty() {
                return Err(format!("malformed manifest line: {line}"));
            }
            Ok(Entry {
                family: (*family).into(),
                style: (*style).into(),
                file: (*file).into(),
                scripts,
                licence: (*licence).into(),
                licence_file: (*licence_file).into(),
            })
        })
        .collect()
}

/// The first required UI face `entries` lack, as a message naming it.
pub fn missing_required(entries: &[Entry]) -> Option<String> {
    REQUIRED_FACES.iter().find_map(|(family, style, script)| {
        let present = entries.iter().any(|e| e.family == *family && e.style == *style && e.scripts.iter().any(|s| s == script));
        (!present).then(|| format!("the required UI face {family} {style} ({script}) is missing from fonts/manifest.txt"))
    })
}

/// One `CraftFont { .. }` initialiser per manifest line, and how many.
fn embed(dir: &Path, required: bool, watched: &mut Vec<PathBuf>) -> Result<(String, usize), String> {
    if !dir.is_dir() {
        return Err("not a directory".into());
    }
    let manifest = dir.join("fonts").join("manifest.txt");
    watched.push(manifest.clone());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let entries = parse_manifest(&text)?;
    if required && let Some(missing) = missing_required(&entries) {
        return Err(missing);
    }
    let mut out = String::new();
    for e in &entries {
        let path = dir.join(&e.file).canonicalize().map_err(|err| format!("{}: {err}", e.file))?;
        watched.push(path.clone());
        let scripts: Vec<String> = e.scripts.iter().map(|s| format!("{s:?}")).collect();
        let _ = writeln!(
            out,
            "    CraftFont {{ family: {:?}, style: {:?}, scripts: &[{}], licence: {:?}, licence_file: {:?}, bytes: include_bytes!({:?}) }},",
            e.family,
            e.style,
            scripts.join(", "),
            e.licence,
            e.licence_file,
            path.display().to_string(),
        );
    }
    Ok((out, entries.len()))
}

/// Resolves a relative `CRAFT_FONTS_DIR` against both roots a caller might mean: a build script runs
/// in the package root (`crates/ui-kit`), while the `../craft-fonts` the docs use is relative to the
/// workspace root.
fn resolve(dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        return dir;
    }
    let package = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let workspace = package.parent().and_then(Path::parent).unwrap_or(package.as_path());
    let (first, second) = (workspace.join(&dir), package.join(&dir));
    if first.is_dir() || !second.is_dir() { first } else { second }
}
