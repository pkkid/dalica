//! Incremental verification: scanning again only the dirty folders (`data-model.md` §7).
//!
//! Each dirty folder becomes a [`Scope`]. A folder whose own entries changed is scanned again
//! without the subfolders the library already knows (they report their own changes); a new
//! subfolder, or one the library holds no photos from, is scanned whole. A dirty subtree is scanned
//! whole. A dirty folder that is gone is verified through the nearest folder above it that still
//! exists (never above its watched root), so what vanished is seen as gone. Scopes inside another
//! scope are dropped. [`diff_rescan`](crate::diff_rescan) then changes only what the scopes that
//! were walked to the end cover: a folder under a root that cannot be walked is never removed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dalica_fs::{Entry, Limits};
use dalica_library::Library;

use super::queue::DirtyFolder;
use crate::walk::{Failure, Options, Scan, scan_with};

/// What one incremental verification covers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    /// The folder scanned again, with everything below it except `kept`.
    pub folder: PathBuf,
    /// Subfolders left as they are.
    pub kept: Vec<PathBuf>,
}

impl Scope {
    /// Whether the scope covers `path` (a folder or a file).
    pub fn covers(&self, path: &Path) -> bool {
        path.starts_with(&self.folder) && !self.kept.iter().any(|k| path.starts_with(k))
    }
}

/// What an incremental verification found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rescan {
    /// The folders found inside the verified scopes, sorted by path; `walked` lists the verified
    /// scopes' folders, `failures` every failure in effect (those given back outside the verified
    /// scopes kept as they were).
    pub scan: Scan,
    /// The scopes walked to the end: only these may remove anything.
    pub verified: Vec<Scope>,
}

impl Rescan {
    /// Whether a verified scope covers `path`.
    pub fn covers(&self, path: &Path) -> bool {
        self.verified.iter().any(|s| s.covers(path))
    }
}

/// The folders `library` knows, for [`plan`] and [`rescan`] (copy them before leaving the thread
/// that owns the library).
pub fn known_folders(library: &Library) -> BTreeSet<PathBuf> {
    library.folders().map(|f| f.path.clone()).collect()
}

/// The scopes that verify `dirty`: inside `roots`, nested scopes dropped, in path order. Lists the
/// dirty folders (to find their subfolders): call it off the UI thread.
pub fn plan(known: &BTreeSet<PathBuf>, roots: &[PathBuf], dirty: &[DirtyFolder], options: &Options) -> Vec<Scope> {
    let mut wanted: Vec<(PathBuf, bool)> = Vec::new();
    for d in dirty {
        let Some(root) = roots.iter().filter(|r| d.path.starts_with(r)).max_by_key(|r| r.components().count()) else { continue };
        // The nearest folder that still exists, never above the root.
        let mut folder = d.path.as_path();
        let mut subtree = d.subtree;
        while folder != root.as_path() && !folder.is_dir() {
            match folder.parent() {
                Some(p) => folder = p,
                None => break,
            }
            subtree = false;
        }
        wanted.push((folder.to_path_buf(), subtree));
    }
    // One entry per folder; a subtree wins over the folder alone.
    wanted.sort();
    wanted.dedup_by(|b, a| {
        if a.0 == b.0 {
            a.1 |= b.1;
            true
        } else {
            false
        }
    });
    let mut scopes: Vec<Scope> = wanted
        .into_iter()
        .map(|(folder, subtree)| {
            let kept = if subtree { Vec::new() } else { known_subfolders(known, &folder, options) };
            Scope { folder, kept }
        })
        .collect();
    scopes.sort();
    // Drop scopes another one covers whole.
    let all = scopes.clone();
    scopes.retain(|s| !all.iter().any(|o| o.folder != s.folder && o.covers(&s.folder)));
    scopes
}

/// The subfolders of `folder` that hold folders the library knows (found by listing it as a scan
/// would: skip rules and excluded subtrees applied). A folder that cannot be listed has none: its
/// scan fails and covers nothing.
fn known_subfolders(known: &BTreeSet<PathBuf>, folder: &Path, options: &Options) -> Vec<PathBuf> {
    let limits = Limits { max_depth: 1, ..Limits::default() };
    let skip = &options.skip;
    let excluded = &options.excluded;
    let Ok(listing) = dalica_fs::walk(folder, limits, |e: &Entry| skip.skips(e) || (e.is_dir && excluded.iter().any(|x| e.path.starts_with(x))))
    else {
        return Vec::new();
    };
    listing
        .entries
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.path)
        .filter(|sub| {
            known.range::<Path, _>((std::ops::Bound::Included(sub.as_path()), std::ops::Bound::Unbounded)).next().is_some_and(|k| k.starts_with(sub))
        })
        .collect()
}

/// Verifies `dirty` under `roots`: [`plan`]s the scopes and scans each (with `options`, its kept
/// subfolders added to the excluded ones). Stops early when `options.cancel` is set; the scopes
/// not walked to the end are left out of [`Rescan::verified`].
pub fn rescan(known: &BTreeSet<PathBuf>, roots: &[PathBuf], dirty: &[DirtyFolder], options: &Options) -> Rescan {
    let scopes = plan(known, roots, dirty, options);
    let mut out = Rescan::default();
    let mut found: Vec<Failure> = Vec::new();
    for scope in scopes {
        let mut opts = options.clone();
        opts.excluded.extend(scope.kept.iter().cloned());
        let s = scan_with(std::slice::from_ref(&scope.folder), &opts);
        out.scan.folders.extend(s.folders);
        out.scan.warnings.extend(s.warnings);
        found.extend(s.failures);
        if s.walked.contains(&scope.folder) {
            out.scan.walked.push(scope.folder.clone());
            out.verified.push(scope);
        }
        if s.cancelled {
            out.scan.cancelled = true;
            break;
        }
    }
    out.scan.folders.sort_by(|a, b| a.path.cmp(&b.path));
    out.scan.folders.dedup_by(|b, a| a.path == b.path);
    // Failures: the scans' own, then those given back that no verified scope looked at again.
    let seen: BTreeSet<PathBuf> = found.iter().map(|f| f.path.clone()).collect();
    found.extend(options.failures.iter().filter(|f| !seen.contains(&f.path) && !out.covers(&f.path)).cloned());
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|b, a| a.path == b.path);
    out.scan.failures = found;
    out
}
