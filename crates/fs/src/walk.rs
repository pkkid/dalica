//! Bounded directory walks (`data-model.md` §7, `security.md` §5.6): depth 32, 1 000 000 entries,
//! directory links followed with loop detection, caller-supplied skip rules, entries sorted by name.
//!
//! A walk never fails part-way: anything it cannot read or must stop at becomes a [`Warning`] and
//! the walk goes on with the rest. Only an unreadable root is an error. Entries come in depth-first
//! pre-order (a folder, then its contents), children sorted by their names' bytes, so the same tree
//! always walks the same way.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use dalica_core::Timestamp;

use crate::{Result, guard, io_err};

/// Deepest level listed (the root's children are depth 1).
pub const MAX_DEPTH: usize = 32;
/// Most entries one walk returns.
pub const MAX_ENTRIES: usize = 1_000_000;

/// Limits and controls of one walk.
#[derive(Clone, Copy, Debug)]
pub struct Limits<'a> {
    pub max_depth: usize,
    pub max_entries: usize,
    /// Checked between folders; when set, the walk stops with [`Warning::Cancelled`].
    pub cancel: Option<&'a AtomicBool>,
}

impl Default for Limits<'_> {
    fn default() -> Self {
        Limits { max_depth: MAX_DEPTH, max_entries: MAX_ENTRIES, cancel: None }
    }
}

/// One file or folder found by a walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// 1 for the root's children.
    pub depth: usize,
    pub is_dir: bool,
    /// Reached through a symbolic link (or junction); `is_dir`, `size` and `modified` describe its target.
    pub is_link: bool,
    pub size: u64,
    pub modified: Timestamp,
}

impl Entry {
    /// The last path component, lossily as text (for skip rules and display only).
    pub fn name(&self) -> std::borrow::Cow<'_, str> {
        self.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    }
}

/// Why part of a tree was not listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// This folder is at the depth limit; its contents were not listed.
    TooDeep(PathBuf),
    /// The entry limit was reached; the walk stopped.
    TooManyEntries,
    /// This folder link leads to a folder already walked (a loop or a second way in); not entered.
    Loop(PathBuf),
    /// This folder or entry could not be read.
    Unreadable { path: PathBuf, reason: String },
    /// The cancel flag was set; the walk stopped.
    Cancelled,
}

/// What a walk found.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    pub entries: Vec<Entry>,
    pub warnings: Vec<Warning>,
}

/// Walks `root`. `skip(entry)` returning true leaves the entry out, and for a folder, everything in
/// it. The root itself is not an entry.
pub fn walk(root: &Path, limits: Limits<'_>, mut skip: impl FnMut(&Entry) -> bool) -> Result<Walk> {
    guard::check()?;
    let root_meta = fs::metadata(root).map_err(io_err("walk", root))?;
    if !root_meta.is_dir() {
        return Err(io_err("walk", root)(std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a folder")));
    }
    let mut out = Walk::default();
    let mut seen: HashSet<DirKey> = HashSet::new();
    seen.insert(dir_key(root, &root_meta));
    // Folders still to list, popped from the end: (path, depth of its children).
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 1)];
    // Folder links wait until the real tree is walked, so a real folder always wins over a link
    // to it, and a link is listed only when it is followed.
    let mut links: Vec<Entry> = Vec::new();
    'walk: loop {
        while let Some((dir, depth)) = stack.pop() {
            if limits.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                out.warnings.push(Warning::Cancelled);
                break 'walk;
            }
            let mut children = match fs::read_dir(&dir) {
                Ok(rd) => rd
                    .filter_map(|e| match e {
                        Ok(e) => Some(e.path()),
                        Err(e) => {
                            out.warnings.push(Warning::Unreadable { path: dir.clone(), reason: e.to_string() });
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(e) => {
                    out.warnings.push(Warning::Unreadable { path: dir.clone(), reason: e.to_string() });
                    continue;
                }
            };
            children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            let mut subdirs = Vec::new();
            for path in children {
                if out.entries.len() >= limits.max_entries {
                    out.warnings.push(Warning::TooManyEntries);
                    break 'walk;
                }
                let Some(entry) = describe(&path, depth, &mut out.warnings) else { continue };
                if skip(&entry) {
                    continue;
                }
                if entry.is_dir && entry.is_link {
                    links.push(entry);
                    continue;
                }
                if entry.is_dir {
                    match enter(&entry, &mut seen, limits, &mut out.warnings) {
                        Decision::Enter => subdirs.push(path),
                        Decision::ListOnly => {}
                        // A second way into a folder already walked (bind mount, hard-linked folder).
                        Decision::Leave => continue,
                    }
                }
                out.entries.push(entry);
            }
            // Reverse, so the first child folder is popped (walked) first.
            stack.extend(subdirs.into_iter().rev().map(|p| (p, depth + 1)));
        }
        if links.is_empty() {
            break;
        }
        links.sort_by(|a, b| a.path.components().cmp(b.path.components()));
        for link in std::mem::take(&mut links) {
            if out.entries.len() >= limits.max_entries {
                out.warnings.push(Warning::TooManyEntries);
                break 'walk;
            }
            match enter(&link, &mut seen, limits, &mut out.warnings) {
                Decision::Enter => {
                    stack.push((link.path.clone(), link.depth + 1));
                    out.entries.push(link);
                }
                Decision::ListOnly => out.entries.push(link),
                Decision::Leave => {}
            }
        }
        // Walk the followed links' folders, first link first; they may hold more links, so the
        // outer loop goes round again.
        stack.reverse();
    }
    // Entries were collected a folder at a time; put each folder's contents right after it.
    out.entries = preorder(out.entries);
    Ok(out)
}

/// What to do with a folder found by a walk.
enum Decision {
    /// List it and walk into it.
    Enter,
    /// List it, but not its contents (depth limit, or it cannot be read).
    ListOnly,
    /// Leave it out: it leads to a folder already walked.
    Leave,
}

/// Decides about the folder `entry`, marking it seen, with a warning for anything but [`Decision::Enter`].
fn enter(entry: &Entry, seen: &mut HashSet<DirKey>, limits: Limits<'_>, warnings: &mut Vec<Warning>) -> Decision {
    match fs::metadata(&entry.path) {
        Ok(m) if !seen.insert(dir_key(&entry.path, &m)) => {
            warnings.push(Warning::Loop(entry.path.clone()));
            Decision::Leave
        }
        Ok(_) if entry.depth >= limits.max_depth => {
            warnings.push(Warning::TooDeep(entry.path.clone()));
            Decision::ListOnly
        }
        Ok(_) => Decision::Enter,
        Err(e) => {
            warnings.push(Warning::Unreadable { path: entry.path.clone(), reason: e.to_string() });
            Decision::ListOnly
        }
    }
}

/// Sorts entries into depth-first pre-order: each folder followed by its contents, siblings by name.
fn preorder(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by(|a, b| a.path.components().cmp(b.path.components()));
    entries
}

/// An entry for `path`, following a link to describe its target; `None` (with a warning) if unreadable.
fn describe(path: &Path, depth: usize, warnings: &mut Vec<Warning>) -> Option<Entry> {
    let link = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(Warning::Unreadable { path: path.to_path_buf(), reason: e.to_string() });
            return None;
        }
    };
    let is_link = link.file_type().is_symlink();
    let meta = if is_link {
        match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                // A dangling link: reported, not listed.
                warnings.push(Warning::Unreadable { path: path.to_path_buf(), reason: e.to_string() });
                return None;
            }
        }
    } else {
        link
    };
    Some(Entry {
        path: path.to_path_buf(),
        depth,
        is_dir: meta.is_dir(),
        is_link,
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: modified(&meta),
    })
}

fn modified(meta: &Metadata) -> Timestamp {
    meta.modified().map(Timestamp::from_system_time).unwrap_or_default()
}

/// What makes two paths the same folder: (device, inode) on Unix; the canonical path elsewhere,
/// since stable Rust has no file index on Windows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DirKey {
    #[cfg_attr(not(unix), allow(dead_code))]
    Inode(u64, u64),
    #[cfg_attr(unix, allow(dead_code))]
    Path(PathBuf),
}

#[cfg(unix)]
fn dir_key(_path: &Path, meta: &Metadata) -> DirKey {
    use std::os::unix::fs::MetadataExt;
    DirKey::Inode(meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn dir_key(path: &Path, _meta: &Metadata) -> DirKey {
    DirKey::Path(fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}
