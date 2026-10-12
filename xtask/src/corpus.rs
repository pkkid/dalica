//! `cargo xtask corpus` and `cargo xtask test-corpus` (`docs/development.md` §11): the test
//! corpora. Pins live in [`crate::corpus_pins`]; each set's files are listed with their sha256 in
//! `xtask/corpus/<set>.sha256` (`<sha256>  <path>` per line, sorted by path). Files land in
//! `corpus/<set>/` (gitignored, never committed).
//!
//! - `cargo xtask corpus`: each set's directory, source, pin and state (present, missing, stale,
//!   not published yet). No network.
//! - `cargo xtask corpus --all` (or `corpus <set>…`): fetches every published set that is missing
//!   or stale. A download is checked against its pin (an archive's or a file's sha256) before it is
//!   used and every file against the manifest before it replaces the set; a verified copy is left
//!   alone, so a restored CI cache is only re-hashed.
//! - `--local`: our own sets are copied from a clone of the corpus repository (`../dalica-corpus`
//!   or `$DALICA_CORPUS_REPO`) instead of downloaded.
//! - `--update-manifest <set>…`: fetches the set from its pins (or the clone) and rewrites its
//!   manifest, after a pin moved.
//! - `cargo xtask test-corpus [-p <crate>]… [-- <test args>]`: fetches what is missing or stale,
//!   then runs each corpus crate's `tests/corpus.rs` with `--release --features corpus`. A set that
//!   is not published yet is reported and skipped, never counted as passed.
//!
//! Network is used only by these two commands, never by `ci`, builds or the ordinary tests.
//! Downloads go through `curl` and archives are unpacked with `tar` (both ship with Linux, macOS and
//! Windows 10 and later), with a generic User-Agent.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::corpus_pins::{self, PinnedFile, Set, Source};

/// Generic User-Agent: no personal details in requests (`security.md` §8.2).
pub const USER_AGENT: &str = "dalica-corpus-fetch";
/// The largest single download accepted (`curl --max-filesize`).
pub const MAX_DOWNLOAD: u64 = 512 << 20;
/// Bounds on walking a set's directory.
const MAX_FILES: usize = 100_000;
const MAX_DEPTH: usize = 16;

/// The crates with corpus tests (`tests/corpus.rs` behind their `corpus` feature) and the extra
/// features their corpus tests need.
pub const CORPUS_CRATES: &[(&str, &[&str])] = &[("dalica-image", &["heif"])];

/// `(sha256, path)` entries of a manifest, sorted by path.
pub type Manifest = Vec<(String, String)>;

/// Downloads `url` to `dest` (the real one runs `curl`; tests pass their own).
pub type Fetch<'a> = &'a dyn Fn(&str, &Path) -> Result<(), String>;

// ---------------------------------------------------------------------------------------------
// SHA-256, streaming (FIPS 180-4). `names::sha256` hashes short words from memory; corpus files
// run to tens of megabytes, so this one reads in blocks and never copies the input.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// An incremental SHA-256.
pub struct Sha256 {
    h: [u32; 8],
    block: [u8; 64],
    filled: usize,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            block: [0; 64],
            filled: 0,
            len: 0,
        }
    }
}

impl Sha256 {
    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.filled).min(data.len());
            let (head, rest) = data.split_at(take);
            if let Some(dst) = self.block.get_mut(self.filled..self.filled + take) {
                dst.copy_from_slice(head);
            }
            self.filled += take;
            data = rest;
            if self.filled == 64 {
                let block = self.block;
                self.compress(&block);
                self.filled = 0;
            }
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (slot, word) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = self.h;
        for (k, wi) in K.iter().zip(w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(s0.wrapping_add(maj));
        }
        for (x, y) in self.h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }

    /// The lower-case hex digest.
    pub fn hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        self.h.iter().map(|v| format!("{v:08x}")).collect()
    }
}

/// The hex sha256 of a file, read in 1 MiB blocks.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::default();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            return Ok(hasher.hex());
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
}

// ---------------------------------------------------------------------------------------------
// Manifests

/// Whether `path` is a safe relative path inside a set: `/`-separated, no empty, `.` or `..`
/// component, nothing Windows refuses in a name.
pub fn safe_rel_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && !c.ends_with(['.', ' '])
                && !c.chars().any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        })
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Parses a manifest; every line must be `<64 hex>  <safe relative path>`, each path once.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let mut out = Manifest::new();
    for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let bad = || format!("line {}: expected `<sha256>  <relative path>`, got {line:?}", i + 1);
        let (hash, path) = line.split_once("  ").ok_or_else(bad)?;
        if !is_hash(hash) || !safe_rel_path(path) {
            return Err(bad());
        }
        out.push((hash.to_string(), path.to_string()));
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    if let Some(w) = out.windows(2).find(|w| matches!(w, [a, b] if a.1 == b.1)) {
        return Err(format!("{} is listed twice", w.first().map_or("", |e| e.1.as_str())));
    }
    Ok(out)
}

pub fn format_manifest(entries: &Manifest) -> String {
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| a.1.cmp(&b.1));
    sorted.iter().map(|(h, p)| format!("{h}  {p}\n")).collect()
}

pub fn manifest_path(root: &Path, set: &Set) -> PathBuf {
    root.join("xtask").join("corpus").join(format!("{}.sha256", set.name))
}

/// The set's manifest, `None` when it has none yet.
pub fn load_manifest(root: &Path, set: &Set) -> Result<Option<Manifest>, String> {
    let path = manifest_path(root, set);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_manifest(&text).map(Some).map_err(|e| format!("xtask/corpus/{}.sha256: {e}", set.name)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_manifest(root: &Path, set: &Set, entries: &Manifest) -> Result<(), String> {
    let path = manifest_path(root, set);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, format_manifest(entries)).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("corpus: {}: wrote {} entries to xtask/corpus/{}.sha256", set.name, entries.len(), set.name);
    Ok(())
}

/// The manifest a [`Source::Files`] set must have: its pins.
pub fn files_manifest(files: &[PinnedFile]) -> Manifest {
    let mut m: Manifest = files.iter().map(|f| (f.sha256.to_string(), f.name.to_string())).collect();
    m.sort_by(|a, b| a.1.cmp(&b.1));
    m
}

// ---------------------------------------------------------------------------------------------
// State

/// What `cargo xtask corpus` says about a set.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// Our own repository has no pinned commit for it yet.
    NotPublished,
    /// Pinned, but `xtask/corpus/<set>.sha256` does not exist (run `--update-manifest`).
    NoManifest,
    /// Every manifest entry is present and matches.
    Present { files: usize, bytes: u64 },
    /// Some entries are absent (all of them when the directory is).
    Missing { missing: usize, total: usize },
    /// Entries whose bytes differ from the manifest (and how many are absent).
    Stale { differ: Vec<String>, missing: usize, total: usize },
}

impl State {
    pub fn label(&self) -> String {
        match self {
            State::NotPublished => "not published yet".into(),
            State::NoManifest => "no manifest".into(),
            State::Present { files, bytes } => format!("present ({files} files, {:.1} MB)", *bytes as f64 / 1e6),
            State::Missing { missing, total } if missing == total => format!("missing ({total} files pinned)"),
            State::Missing { missing, total } => format!("missing {missing} of {total} files"),
            State::Stale { differ, missing, total } => {
                let extra = if *missing > 0 { format!(", {missing} missing") } else { String::new() };
                format!("stale ({} of {total} files differ from the manifest{extra}: {})", differ.len(), differ.join(", "))
            }
        }
    }
}

/// Whether the set can be fetched at all (its pin exists).
pub fn published(set: &Set) -> bool {
    !matches!(set.source, Source::Repo { commit: None, .. })
}

/// Checks the files of `dir` against `manifest`.
pub fn check_dir(dir: &Path, manifest: &Manifest) -> State {
    let (mut missing, mut differ, mut bytes) = (0, Vec::new(), 0u64);
    for (hash, rel) in manifest {
        let path = dir.join(rel);
        match (std::fs::symlink_metadata(&path), hash_file(&path)) {
            (Ok(meta), Ok(got)) if meta.is_file() && got == *hash => bytes = bytes.saturating_add(meta.len()),
            (Ok(meta), Ok(_)) if meta.is_file() => differ.push(rel.clone()),
            (Ok(_), _) => differ.push(rel.clone()),
            (Err(_), _) => missing += 1,
        }
    }
    let total = manifest.len();
    if !differ.is_empty() {
        State::Stale { differ, missing, total }
    } else if missing > 0 {
        State::Missing { missing, total }
    } else {
        State::Present { files: total, bytes }
    }
}

pub fn set_dir(root: &Path, set: &Set) -> PathBuf {
    root.join("corpus").join(set.name)
}

pub fn state(root: &Path, set: &Set) -> Result<State, String> {
    if !published(set) {
        return Ok(State::NotPublished);
    }
    match load_manifest(root, set)? {
        None => Ok(State::NoManifest),
        Some(m) => Ok(check_dir(&set_dir(root, set), &m)),
    }
}

// ---------------------------------------------------------------------------------------------
// Fetching

/// `curl` with a generic User-Agent, retries and a size cap; fails on HTTP errors.
pub fn curl(url: &str, dest: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--max-filesize", &MAX_DOWNLOAD.to_string(), "-A", USER_AGENT, "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("curl {url} failed ({status})")) }
}

fn untar(archive: &Path, into: &Path) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let status = Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(into).status().map_err(|e| format!("cannot run tar: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("tar -xzf {} failed ({status})", archive.display())) }
}

/// Every regular file under `dir` as `/`-separated relative paths, sorted; symbolic links, `.git`
/// and `SOURCES.md` at the top are left out. Bounded in depth and count.
pub fn list_files(dir: &Path) -> Result<Vec<String>, String> {
    fn walk(base: &Path, dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("{}: nested deeper than {MAX_DEPTH}", dir.display()));
        }
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry.file_type().map_err(|e| format!("{}: {e}", entry.path().display()))?;
            let rel = format!("{prefix}{name}");
            if name == ".git" || (prefix.is_empty() && name == "SOURCES.md") || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                walk(base, &entry.path(), &format!("{rel}/"), depth + 1, out)?;
            } else if kind.is_file() {
                if out.len() >= MAX_FILES {
                    return Err(format!("{}: more than {MAX_FILES} files", base.display()));
                }
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, "", 0, &mut out)?;
    out.sort();
    Ok(out)
}

/// The manifest of everything under `dir` (for `--update-manifest`).
fn manifest_of(dir: &Path) -> Result<Manifest, String> {
    let mut out = Manifest::new();
    for rel in list_files(dir)? {
        if !safe_rel_path(&rel) {
            return Err(format!("{rel}: not a portable file name; rename it upstream"));
        }
        out.push((hash_file(&dir.join(&rel))?, rel));
    }
    Ok(out)
}

fn sources_md(set: &Set, manifest: &Manifest, pin: &str) -> String {
    let mut text = format!(
        "# Test corpus `{name}`\n\n{what}.\n\n{terms}\n\nPin: {pin}. Every file is checked against `xtask/corpus/{name}.sha256` \
         ({n} files) by `cargo xtask corpus`. Gitignored; never commit these files or excerpts of them (`docs/development.md` §11).\n",
        name = set.name,
        what = set.what,
        terms = set.terms,
        n = manifest.len(),
    );
    if let Source::Files(files) = &set.source {
        text.push_str("\n| File | What | Source |\n|---|---|---|\n");
        for f in *files {
            text.push_str(&format!("| `{}` | {} | <{}> |\n", f.name, f.what, f.url));
        }
    }
    text
}

fn pin_text(set: &Set) -> String {
    match &set.source {
        Source::Archive { url, sha256 } => format!("{url} (sha256 {sha256})"),
        Source::Files(files) => format!("{} files from {}, each by URL and sha256 in xtask/src/corpus_pins.rs", files.len(), host(files)),
        Source::Repo { repo, commit: Some(c), subdir } => format!("`{subdir}/` of https://github.com/{repo} at {c}"),
        Source::Repo { repo, commit: None, subdir } => format!("`{subdir}/` of https://github.com/{repo}, no commit pinned yet"),
    }
}

fn host(files: &[PinnedFile]) -> String {
    files.first().and_then(|f| f.url.split('/').nth(2)).unwrap_or("?").to_string()
}

/// A staging directory `corpus/.<set>-staging` (our own naming shape), empty.
fn staging(root: &Path, set: &Set) -> Result<PathBuf, String> {
    let dir = root.join("corpus").join(format!(".{}-staging", set.name));
    remove_dir(&dir)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn remove_dir(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

/// Checks `files` (a directory holding a candidate copy of the set) against `manifest`, then
/// makes it `corpus/<set>/` with its `SOURCES.md`.
fn install(root: &Path, set: &Set, files: &Path, manifest: &Manifest, how: &str) -> Result<(), String> {
    match check_dir(files, manifest) {
        State::Present { files: n, bytes } => {
            let dest = set_dir(root, set);
            remove_dir(&dest)?;
            std::fs::rename(files, &dest).map_err(|e| format!("{} → {}: {e}", files.display(), dest.display()))?;
            std::fs::write(dest.join("SOURCES.md"), sources_md(set, manifest, &pin_text(set))).map_err(|e| format!("SOURCES.md: {e}"))?;
            println!("corpus: {}: {n} files ({:.1} MB) verified and installed in corpus/{} ({how})", set.name, bytes as f64 / 1e6, set.name);
            Ok(())
        }
        other => {
            Err(format!("corpus: {}: the {how} does not match xtask/corpus/{}.sha256: {}; nothing was installed", set.name, set.name, other.label()))
        }
    }
}

/// Where a clone of our corpus repository lives: `$DALICA_CORPUS_REPO`, else `../dalica-corpus`.
pub fn local_clone(root: &Path) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("DALICA_CORPUS_REPO").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let sibling = root.parent().map(|p| p.join("dalica-corpus"));
    sibling
        .filter(|s| s.is_dir())
        .ok_or_else(|| "no local clone of dalica-corpus: expected ../dalica-corpus next to this checkout, or set DALICA_CORPUS_REPO".into())
}

/// How one set is fetched.
pub struct Plan<'a> {
    pub update_manifest: bool,
    /// Copy our own sets from this clone instead of downloading them.
    pub local: Option<&'a Path>,
    pub fetch: Fetch<'a>,
}

/// Fetches `set` unless its verified copy is already present; with `update_manifest`, refetches
/// it and rewrites its manifest first. Returns the state it ends in.
pub fn fetch_set(root: &Path, set: &Set, plan: &Plan) -> Result<State, String> {
    let manifest = load_manifest(root, set)?;
    let from_clone = plan.local.filter(|_| matches!(set.source, Source::Repo { .. }));
    if !published(set) && from_clone.is_none() {
        println!("corpus: {}: not published yet ({}); skipped", set.name, pin_text(set));
        return Ok(State::NotPublished);
    }
    if let Source::Files(files) = &set.source {
        return fetch_files(root, set, files, manifest.as_ref(), plan);
    }
    if !plan.update_manifest
        && from_clone.is_none()
        && let Some(m) = &manifest
        && let State::Present { files, bytes } = check_dir(&set_dir(root, set), m)
    {
        println!("corpus: {}: {files} files ({:.1} MB) present and verified", set.name, bytes as f64 / 1e6);
        return Ok(State::Present { files, bytes });
    }
    let manifest = match manifest {
        // `--update-manifest` writes a new one from what it fetches.
        _ if plan.update_manifest => None,
        Some(m) => Some(m),
        None => {
            let n = set.name;
            return Err(format!("corpus: {n}: xtask/corpus/{n}.sha256 is missing; run `cargo xtask corpus --update-manifest {n}`"));
        }
    };
    fetch_unpacked(root, set, manifest, from_clone, plan)
}

/// Archive and repository sets: download (or copy) into staging, check, install.
fn fetch_unpacked(root: &Path, set: &Set, manifest: Option<Manifest>, clone: Option<&Path>, plan: &Plan) -> Result<State, String> {
    let stage = staging(root, set)?;
    let (files, how) = match (&set.source, clone) {
        (Source::Repo { subdir, commit, .. }, Some(clone)) => {
            warn_clone_head(clone, *commit, set);
            let src = clone.join(subdir);
            let files = stage.join("files");
            copy_tree(&src, &files)?;
            (files, format!("copy of {}", src.display()))
        }
        (Source::Archive { url, sha256 }, _) => {
            let archive = stage.join("archive.tgz");
            (plan.fetch)(url, &archive)?;
            verify_download(&archive, sha256, url)?;
            let files = stage.join("files");
            untar(&archive, &files)?;
            (files, format!("download of {url}"))
        }
        (Source::Repo { repo, commit: Some(commit), subdir }, None) => {
            let url = format!("https://codeload.github.com/{repo}/tar.gz/{commit}");
            let archive = stage.join("archive.tgz");
            (plan.fetch)(&url, &archive)?;
            let unpacked = stage.join("unpacked");
            untar(&archive, &unpacked)?;
            let name = repo.rsplit('/').next().unwrap_or(repo);
            (unpacked.join(format!("{name}-{commit}")).join(subdir), format!("download of {repo}@{commit}"))
        }
        (Source::Repo { commit: None, .. }, None) | (Source::Files(_), _) => return Err(format!("corpus: {}: nothing to fetch", set.name)),
    };
    let manifest = match manifest {
        Some(m) => m,
        None => {
            let m = manifest_of(&files)?;
            write_manifest(root, set, &m)?;
            m
        }
    };
    install(root, set, &files, &manifest, &how)?;
    remove_dir(&stage)?;
    Ok(check_dir(&set_dir(root, set), &manifest))
}

/// Single-file sets: each file not already verified is downloaded, checked against its pin and
/// moved into place. The manifest must list exactly the pins.
fn fetch_files(root: &Path, set: &Set, files: &[PinnedFile], manifest: Option<&Manifest>, plan: &Plan) -> Result<State, String> {
    let pinned = files_manifest(files);
    if let Some(bad) = files.iter().find(|f| !safe_rel_path(f.name) || !is_hash(f.sha256)) {
        return Err(format!("corpus: {}: bad pin for {:?}", set.name, bad.name));
    }
    if plan.update_manifest {
        write_manifest(root, set, &pinned)?;
    } else if manifest != Some(&pinned) {
        return Err(format!(
            "corpus: {}: xtask/corpus/{}.sha256 does not list exactly the pins of corpus_pins.rs; run `cargo xtask corpus --update-manifest {}`",
            set.name, set.name, set.name
        ));
    }
    let dest = set_dir(root, set);
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut stage: Option<PathBuf> = None;
    let (mut fetched, mut kept) = (0, 0);
    for f in files {
        let path = dest.join(f.name);
        if hash_file(&path).is_ok_and(|h| h == f.sha256) {
            kept += 1;
            continue;
        }
        let dir = match &stage {
            Some(d) => d.clone(),
            None => {
                let d = staging(root, set)?;
                stage = Some(d.clone());
                d
            }
        };
        let tmp = dir.join(f.name);
        (plan.fetch)(f.url, &tmp)?;
        verify_download(&tmp, f.sha256, f.url)?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{} → {}: {e}", tmp.display(), path.display()))?;
        fetched += 1;
    }
    if let Some(d) = &stage {
        remove_dir(d)?;
    }
    std::fs::write(dest.join("SOURCES.md"), sources_md(set, &pinned, &pin_text(set))).map_err(|e| format!("SOURCES.md: {e}"))?;
    let state = check_dir(&dest, &pinned);
    println!("corpus: {}: {fetched} downloaded and verified, {kept} already present; {}", set.name, state.label());
    Ok(state)
}

/// A download whose sha256 is not its pin is deleted and refused.
pub fn verify_download(path: &Path, want: &str, url: &str) -> Result<(), String> {
    let got = hash_file(path)?;
    if got == want {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err(format!("corpus: {url}: the download's sha256 is {got}, the pin says {want}; refused (nothing installed)"))
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("{} is not a directory", src.display()));
    }
    for rel in list_files(src)? {
        let to = dest.join(&rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::copy(src.join(&rel), &to).map_err(|e| format!("{rel}: {e}"))?;
    }
    Ok(())
}

fn warn_clone_head(clone: &Path, commit: Option<&str>, set: &Set) {
    let head = Command::new("git").arg("-C").arg(clone).args(["rev-parse", "HEAD"]).output().ok().filter(|o| o.status.success());
    let head = head.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match (head, commit) {
        (Some(h), Some(c)) if h == c => println!("corpus: {}: {} is at the pinned commit", set.name, clone.display()),
        (Some(h), _) => eprintln!(
            "corpus: warning: {}: {} is at {h}, not the pinned commit {}; the tests see unpinned files",
            set.name,
            clone.display(),
            commit.unwrap_or("(none yet)")
        ),
        (None, _) => eprintln!("corpus: warning: {}: cannot read the git HEAD of {}", set.name, clone.display()),
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub all: bool,
    pub local: bool,
    pub update_manifest: bool,
    pub sets: Vec<String>,
}

pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    for a in args {
        match a.as_str() {
            "--all" => out.all = true,
            "--local" => out.local = true,
            "--update-manifest" => out.update_manifest = true,
            flag if flag.starts_with('-') => return Err(format!("corpus: unknown option `{flag}`")),
            name if corpus_pins::SETS.iter().any(|s| s.name == name) => out.sets.push(name.to_string()),
            other => return Err(format!("corpus: no set `{other}` (sets: {})", set_names())),
        }
    }
    if out.update_manifest && out.sets.is_empty() && !out.all {
        return Err("corpus: --update-manifest needs the sets to rewrite (or --all)".into());
    }
    Ok(out)
}

fn set_names() -> String {
    corpus_pins::SETS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
}

/// `cargo xtask corpus [--all | <set>…] [--local] [--update-manifest]`
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let args = parse_args(args)?;
    if !args.all && args.sets.is_empty() {
        return status(root);
    }
    let clone = if args.local { Some(local_clone(root)?) } else { None };
    let plan = Plan { update_manifest: args.update_manifest, local: clone.as_deref(), fetch: &curl };
    let mut problems = Vec::new();
    for set in corpus_pins::SETS.iter().filter(|s| args.all || args.sets.iter().any(|n| n == s.name)) {
        match fetch_set(root, set, &plan) {
            Ok(State::Present { .. } | State::NotPublished) => {}
            Ok(other) => problems.push(format!("{}: {}", set.name, other.label())),
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(format!("corpus: {} set(s) failed:\n  {}", problems.len(), problems.join("\n  "))) }
}

fn status(root: &Path) -> Result<(), String> {
    println!("Test corpora live in {} (gitignored, never committed); pins in xtask/src/corpus_pins.rs.\n", root.join("corpus").display());
    for set in corpus_pins::SETS {
        let state = state(root, set)?;
        println!("  corpus/{:<10} {}", format!("{}/", set.name), state.label());
        println!("  {:<17} {}", "", set.what);
        println!("  {:<17} {}", "", pin_text(set));
        if let Source::Files(files) = &set.source {
            let mut by_ext: BTreeMap<String, usize> = BTreeMap::new();
            for f in *files {
                *by_ext.entry(f.name.rsplit('.').next().unwrap_or("").to_ascii_uppercase()).or_default() += 1;
            }
            let kinds: Vec<String> = by_ext.iter().map(|(k, n)| format!("{k} {n}")).collect();
            println!("  {:<17} {}", "", kinds.join(", "));
        }
        println!();
    }
    println!(
        "cargo xtask corpus --all           fetch every published set that is missing or stale (sha256-verified)\n\
         cargo xtask corpus <set>…          the same for some sets\n\
         cargo xtask corpus --local …       our own sets from ../dalica-corpus (or $DALICA_CORPUS_REPO)\n\
         cargo xtask corpus --update-manifest <set>…   refetch from the pins and rewrite the manifests\n\
         cargo xtask test-corpus [-p image] fetch, then run the corpus tests (--release --features corpus)"
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct TestArgs {
    pub crates: Vec<String>,
    pub local: bool,
    pub passthrough: Vec<String>,
}

pub fn parse_test_args(args: &[String]) -> Result<TestArgs, String> {
    let (ours, passthrough) = match args.iter().position(|a| a == "--") {
        Some(i) => (args.get(..i).unwrap_or_default(), args.get(i + 1..).unwrap_or_default()),
        None => (args, &[][..]),
    };
    let mut crates = Vec::new();
    let mut local = false;
    let mut it = ours.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-p" | "--package" => {
                let name = it.next().ok_or("test-corpus: -p needs a crate name")?;
                let full = if name.starts_with("dalica-") { name.clone() } else { format!("dalica-{name}") };
                if !CORPUS_CRATES.iter().any(|(c, _)| *c == full) {
                    let known: Vec<&str> = CORPUS_CRATES.iter().map(|(c, _)| *c).collect();
                    return Err(format!("test-corpus: {full} has no corpus tests (crates with them: {})", known.join(", ")));
                }
                if !crates.contains(&full) {
                    crates.push(full);
                }
            }
            "--local" => local = true,
            other => return Err(format!("test-corpus: unknown argument `{other}`")),
        }
    }
    if crates.is_empty() {
        crates = CORPUS_CRATES.iter().map(|(c, _)| (*c).to_string()).collect();
    }
    Ok(TestArgs { crates, local, passthrough: passthrough.to_vec() })
}

/// `cargo xtask test-corpus [-p <crate>]… [--local] [-- <test args>]`
pub fn test(root: &Path, args: &[String]) -> Result<(), String> {
    let args = parse_test_args(args)?;
    let clone = if args.local { Some(local_clone(root)?) } else { None };
    let plan = Plan { update_manifest: false, local: clone.as_deref(), fetch: &curl };
    let mut skipped = Vec::new();
    for set in corpus_pins::SETS.iter().filter(|s| s.used_by.iter().any(|c| args.crates.iter().any(|a| a == c))) {
        match fetch_set(root, set, &plan)? {
            State::Present { .. } => {}
            State::NotPublished => skipped.push(set.name),
            other => return Err(format!("test-corpus: {}: {}", set.name, other.label())),
        }
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(["test", "--release"]);
    let mut features = Vec::new();
    for (krate, extra) in CORPUS_CRATES.iter().filter(|(c, _)| args.crates.iter().any(|a| a == c)) {
        cmd.args(["-p", krate]);
        features.push(format!("{krate}/corpus"));
        features.extend(extra.iter().map(|f| format!("{krate}/{f}")));
    }
    // One test at a time, so each set's table prints in one piece.
    cmd.args(["--features", &features.join(","), "--test", "corpus", "--", "--nocapture"]);
    if !args.passthrough.iter().any(|a| a.starts_with("--test-threads")) {
        cmd.arg("--test-threads=1");
    }
    cmd.args(&args.passthrough);
    println!("test-corpus: cargo test --release -p {} --features {} --test corpus", args.crates.join(" -p "), features.join(","));
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err("test-corpus: the corpus tests failed".into());
    }
    if skipped.is_empty() {
        println!("test-corpus: ok");
    } else {
        println!(
            "test-corpus: ok for the published sets; SKIPPED, not passed: {} (not published yet in {}; no floors exist for them)",
            skipped.join(", "),
            corpus_pins::DALICA_CORPUS_REPO
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn streaming_sha256_matches_the_one_shot_one() {
        assert_eq!(Sha256::default().hex(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let data: Vec<u8> = (0..5000u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
        for len in [0, 1, 3, 55, 56, 63, 64, 65, 119, 128, 1000, 5000] {
            let slice = &data[..len];
            let mut h = Sha256::default();
            for chunk in slice.chunks(7) {
                h.update(chunk);
            }
            assert_eq!(h.hex(), crate::names::sha256_hex(slice), "length {len}");
        }
    }

    #[test]
    fn manifests_refuse_unsafe_lines() {
        let h = "a".repeat(64);
        assert_eq!(
            parse_manifest(&format!("{h}  b.png\n{h}  a/c.png\n\n")).unwrap(),
            vec![(h.clone(), "a/c.png".into()), (h.clone(), "b.png".into())]
        );
        for bad in ["../x", "/abs", "a//b", "a\\b", "c:x", "dot.", "", "./x"] {
            assert!(parse_manifest(&format!("{h}  {bad}")).is_err(), "{bad:?}");
        }
        assert!(parse_manifest(&format!("{}  x", "A".repeat(64))).is_err());
        assert!(parse_manifest("abc  x").is_err());
        assert!(parse_manifest(&format!("{h} x")).is_err());
        assert!(parse_manifest(&format!("{h}  x\n{h}  x")).is_err());
        let m = parse_manifest(&format!("{h}  b\n{h}  a")).unwrap();
        assert_eq!(parse_manifest(&format_manifest(&m)).unwrap(), m);
    }

    #[test]
    fn the_committed_pins_and_manifests_agree() {
        let root = crate::workspace_root();
        for set in corpus_pins::SETS {
            let manifest = load_manifest(&root, set).unwrap();
            match &set.source {
                Source::Files(files) => assert_eq!(manifest, Some(files_manifest(files)), "{}", set.name),
                Source::Archive { sha256, url } => {
                    assert!(is_hash(sha256) && url.starts_with("http"), "{}", set.name);
                    assert!(manifest.is_some_and(|m| !m.is_empty()), "{} has no manifest", set.name);
                }
                Source::Repo { commit, .. } => {
                    assert_eq!(commit.is_some(), manifest.is_some(), "{}: a published set has a manifest, an unpublished one none", set.name);
                    assert!(commit.is_none_or(|c| c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit())), "{}", set.name);
                }
            }
        }
        let names: Vec<&str> = corpus_pins::RAW_FILES.iter().map(|f| f.name).collect();
        assert!(names.iter().all(|n| safe_rel_path(n)), "{names:?}");
        assert!(corpus_pins::RAW_FILES.iter().all(|f| f.url.starts_with("https://raw.pixls.us/") && is_hash(f.sha256)));
        // Two per raw kind Dalica reads.
        for ext in ["CR2", "CR3", "CRW", "DNG", "NEF", "ARW", "PEF", "ORF", "RW2", "SRW", "RAF", "MRW", "X3F"] {
            assert_eq!(names.iter().filter(|n| n.ends_with(&format!(".{ext}"))).count(), 2, "{ext}");
        }
    }

    #[test]
    fn unpublished_sets_are_said_so_and_never_fetched() {
        let tmp = tempfile::tempdir().unwrap();
        let set = Set { name: "mine", what: "w", terms: "t", used_by: &[], source: Source::Repo { repo: "o/r", commit: None, subdir: "s" } };
        assert_eq!(state(tmp.path(), &set).unwrap(), State::NotPublished);
        assert_eq!(State::NotPublished.label(), "not published yet");
        let never = |url: &str, _: &Path| -> Result<(), String> { panic!("fetched {url}") };
        let plan = Plan { update_manifest: false, local: None, fetch: &never };
        assert_eq!(fetch_set(tmp.path(), &set, &plan).unwrap(), State::NotPublished);
        assert!(!tmp.path().join("corpus").exists());
    }

    /// A fake upstream: URL → bytes.
    fn serve<'a>(files: &'a [(&'a str, Vec<u8>)], calls: &'a std::cell::Cell<usize>) -> impl Fn(&str, &Path) -> Result<(), String> + 'a {
        move |url: &str, dest: &Path| {
            calls.set(calls.get() + 1);
            let (_, bytes) = files.iter().find(|(u, _)| *u == url).ok_or(format!("404 {url}"))?;
            std::fs::write(dest, bytes).map_err(|e| e.to_string())
        }
    }

    fn hex_of(bytes: &[u8]) -> String {
        let mut h = Sha256::default();
        h.update(bytes);
        h.hex()
    }

    fn file_set(pins: &'static [PinnedFile]) -> Set {
        Set { name: "files", what: "w", terms: "t", used_by: &[], source: Source::Files(pins) }
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    #[test]
    fn single_files_are_verified_planted_stale_files_are_replaced_and_altered_downloads_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let (a, b) = (b"first raw".to_vec(), b"second raw".to_vec());
        let pins: &'static [PinnedFile] = Box::leak(Box::new([
            PinnedFile { name: "A.CR2", url: "https://example.test/a", sha256: leak(hex_of(&a)), what: "a" },
            PinnedFile { name: "B.NEF", url: "https://example.test/b", sha256: leak(hex_of(&b)), what: "b" },
        ]));
        let set = file_set(pins);
        let calls = std::cell::Cell::new(0);
        let upstream = [("https://example.test/a", a.clone()), ("https://example.test/b", b.clone())];
        let fetch = serve(&upstream, &calls);

        // No manifest yet: refused until --update-manifest writes it from the pins.
        let plan = Plan { update_manifest: false, local: None, fetch: &fetch };
        assert!(fetch_set(root, &set, &plan).unwrap_err().contains("--update-manifest files"));
        assert_eq!(state(root, &set).unwrap(), State::NoManifest);
        let update = Plan { update_manifest: true, local: None, fetch: &fetch };
        assert!(matches!(fetch_set(root, &set, &update).unwrap(), State::Present { files: 2, .. }));
        assert_eq!(calls.get(), 2);
        assert_eq!(load_manifest(root, &set).unwrap(), Some(files_manifest(pins)));
        assert!(set_dir(root, &set).join("SOURCES.md").is_file());

        // Verified copies are left alone.
        assert!(matches!(fetch_set(root, &set, &plan).unwrap(), State::Present { files: 2, .. }));
        assert_eq!(calls.get(), 2);

        // A planted stale file fails verification, and the next fetch replaces only it.
        std::fs::write(set_dir(root, &set).join("B.NEF"), b"altered").unwrap();
        assert_eq!(state(root, &set).unwrap(), State::Stale { differ: vec!["B.NEF".into()], missing: 0, total: 2 });
        assert!(state(root, &set).unwrap().label().starts_with("stale (1 of 2 files differ"));
        assert!(matches!(fetch_set(root, &set, &plan).unwrap(), State::Present { files: 2, .. }));
        assert_eq!(calls.get(), 3);
        assert_eq!(std::fs::read(set_dir(root, &set).join("B.NEF")).unwrap(), b);

        // An upstream that now serves different bytes is refused; the stale file stays out.
        std::fs::remove_file(set_dir(root, &set).join("A.CR2")).unwrap();
        assert_eq!(state(root, &set).unwrap(), State::Missing { missing: 1, total: 2 });
        let tampered = [("https://example.test/a", b"tampered".to_vec()), ("https://example.test/b", b.clone())];
        let bad_fetch = serve(&tampered, &calls);
        let bad = Plan { update_manifest: false, local: None, fetch: &bad_fetch };
        let err = fetch_set(root, &set, &bad).unwrap_err();
        assert!(err.contains("the pin says") && err.contains("refused"), "{err}");
        assert!(!set_dir(root, &set).join("A.CR2").exists());
        assert!(!root.join("corpus").join(".files-staging").join("A.CR2").exists());

        // A manifest that drifted from the pins is an error, not a silent pass.
        std::fs::write(manifest_path(root, &set), format!("{}  A.CR2\n", hex_of(&a))).unwrap();
        assert!(fetch_set(root, &set, &plan).unwrap_err().contains("does not list exactly the pins"));
    }

    #[test]
    fn an_altered_archive_is_refused_before_it_is_unpacked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let h = "0".repeat(64);
        std::fs::create_dir_all(root.join("xtask").join("corpus")).unwrap();
        std::fs::write(root.join("xtask").join("corpus").join("arch.sha256"), format!("{h}  x.png\n")).unwrap();
        let set =
            Set { name: "arch", what: "w", terms: "t", used_by: &[], source: Source::Archive { url: "http://example.test/a.tgz", sha256: leak(h) } };
        assert_eq!(state(root, &set).unwrap(), State::Missing { missing: 1, total: 1 });
        let calls = std::cell::Cell::new(0);
        let upstream = [("http://example.test/a.tgz", b"not the pinned archive".to_vec())];
        let fetch = serve(&upstream, &calls);
        let err = fetch_set(root, &set, &Plan { update_manifest: false, local: None, fetch: &fetch }).unwrap_err();
        assert!(err.contains("refused"), "{err}");
        assert!(!set_dir(root, &set).exists());
    }

    #[test]
    fn local_copies_are_checked_against_the_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("checkout");
        let clone = tmp.path().join("clone");
        std::fs::create_dir_all(clone.join("photos").join("phones")).unwrap();
        std::fs::write(clone.join("photos").join("one.jpg"), b"jpeg one").unwrap();
        std::fs::write(clone.join("photos").join("phones").join("two.jpg"), b"jpeg two").unwrap();
        let set = Set { name: "photos", what: "w", terms: "t", used_by: &[], source: Source::Repo { repo: "o/r", commit: None, subdir: "photos" } };
        let never = |url: &str, _: &Path| -> Result<(), String> { Err(format!("network used: {url}")) };
        // --local --update-manifest writes the manifest from the clone (before anything is pinned).
        let update = Plan { update_manifest: true, local: Some(&clone), fetch: &never };
        assert!(matches!(fetch_set(&root, &set, &update).unwrap(), State::Present { files: 2, .. }));
        let m = load_manifest(&root, &set).unwrap().unwrap();
        assert_eq!(m.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(), ["one.jpg", "phones/two.jpg"]);
        // A clone that changed since no longer matches: nothing is installed over the good copy.
        std::fs::write(clone.join("photos").join("one.jpg"), b"changed").unwrap();
        let plan = Plan { update_manifest: false, local: Some(&clone), fetch: &never };
        assert!(fetch_set(&root, &set, &plan).unwrap_err().contains("does not match"));
        assert!(matches!(check_dir(&set_dir(&root, &set), &m), State::Present { files: 2, .. }));
        // Without --local the unpinned set is only reported.
        let remote = Plan { update_manifest: false, local: None, fetch: &never };
        assert_eq!(fetch_set(&root, &set, &remote).unwrap(), State::NotPublished);
    }

    #[test]
    fn arguments() {
        assert_eq!(parse_args(&strings(&["--all", "--local"])).unwrap(), Args { all: true, local: true, update_manifest: false, sets: vec![] });
        assert_eq!(parse_args(&strings(&["--update-manifest", "raw"])).unwrap().sets, ["raw"]);
        assert!(parse_args(&strings(&["--update-manifest"])).is_err());
        assert!(parse_args(&strings(&["nope"])).is_err());
        assert!(parse_args(&strings(&["--fast"])).is_err());
        let t = parse_test_args(&strings(&["-p", "image", "--", "--test-threads=1"])).unwrap();
        assert_eq!(t, TestArgs { crates: vec!["dalica-image".into()], local: false, passthrough: vec!["--test-threads=1".into()] });
        assert_eq!(parse_test_args(&[]).unwrap().crates, ["dalica-image"]);
        assert!(parse_test_args(&strings(&["-p", "meta"])).unwrap_err().contains("has no corpus tests"));
        assert!(parse_test_args(&strings(&["-p"])).is_err());
    }

    #[test]
    fn walking_skips_links_git_and_sources() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join(".git").join("HEAD"), b"x").unwrap();
        std::fs::write(tmp.path().join("SOURCES.md"), b"x").unwrap();
        std::fs::write(tmp.path().join("sub").join("SOURCES.md"), b"x").unwrap();
        std::fs::write(tmp.path().join("a.png"), b"x").unwrap();
        assert_eq!(list_files(tmp.path()).unwrap(), ["a.png", "sub/SOURCES.md"]);
    }
}
