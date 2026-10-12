//! The polling backend: pure Rust, works everywhere, used where the system's events cannot be
//! had (a network share, a watch limit reached, a backend that failed).
//!
//! It keeps a fingerprint of every folder under the roots: the names, kinds, sizes and
//! modification times of what the folder lists (skip rules and excluded subtrees applied, as a
//! scan would). Each step lists the next few folders in path order; a full pass over all of them
//! is a sweep. The number per step is sized to the library, so a sweep takes about
//! [`PollOptions::sweep`] whatever the library's size, within the per-step bounds. A changed
//! fingerprint makes the folder dirty: an added, removed or renamed entry changes it, and so does a
//! file rewritten in place (its size or time). A new folder is dirty with its subtree. A folder
//! that cannot be listed (an offline volume) is left alone until it can be again, and is then dirty
//! with its subtree.
//!
//! Time is the caller's: [`PollingWatcher::poll`] takes `now`, so tests drive it with virtual time.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::Timestamp;
use dalica_fs::{Entry, Limits};

use super::filter::Filter;
use super::queue::{DirtyFolder, DirtyQueue, QueueOptions};
use super::{Backend, Watcher};

/// How fast the polling backend sweeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollOptions {
    /// The time one sweep over every folder should take.
    pub sweep: Duration,
    /// Time between two steps.
    pub step: Duration,
    /// Fewest folders listed per step.
    pub min_per_step: usize,
    /// Most folders listed per step: a bigger library sweeps more slowly instead.
    pub max_per_step: usize,
}

impl Default for PollOptions {
    fn default() -> Self {
        PollOptions { sweep: Duration::from_secs(30), step: Duration::from_secs(1), min_per_step: 32, max_per_step: 2000 }
    }
}

/// Most folders the backend tracks; beyond, new folders are left to the scheduled verification.
pub const MAX_FOLDERS: usize = dalica_fs::walk::MAX_ENTRIES;

/// What the backend knows about one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    /// Found, not listed yet.
    Unvisited,
    Listed(u64),
    /// Could not be listed last time.
    Unreachable,
}

/// Watches roots by listing their folders in sweeps.
#[derive(Debug)]
pub struct PollingWatcher {
    filter: Filter,
    options: PollOptions,
    queue: DirtyQueue,
    folders: BTreeMap<PathBuf, Seen>,
    /// The last folder listed in the current sweep.
    cursor: Option<PathBuf>,
    next_step: Option<Timestamp>,
    sweeps: u64,
    listed: u64,
}

impl PollingWatcher {
    pub(crate) fn new(filter: Filter, queue: QueueOptions, options: PollOptions) -> PollingWatcher {
        let queue = DirtyQueue::new(&filter.roots, queue);
        let folders = filter.roots.iter().map(|r| (r.clone(), Seen::Unvisited)).collect();
        PollingWatcher { filter, options, queue, folders, cursor: None, next_step: None, sweeps: 0, listed: 0 }
    }

    /// Full sweeps done. The first one learns the tree and reports nothing.
    pub fn sweeps(&self) -> u64 {
        self.sweeps
    }

    /// Folders listed so far.
    pub fn listed(&self) -> u64 {
        self.listed
    }

    /// Folders tracked.
    pub fn folders(&self) -> usize {
        self.folders.len()
    }

    /// Folders listed per step: enough for one sweep in [`PollOptions::sweep`], within the bounds.
    pub fn per_step(&self) -> usize {
        let steps = (self.options.sweep.as_millis() / self.options.step.as_millis().max(1)).max(1);
        let n = u128::try_from(self.folders.len()).unwrap_or(u128::MAX).div_ceil(steps);
        usize::try_from(n).unwrap_or(usize::MAX).clamp(self.options.min_per_step.max(1), self.options.max_per_step.max(1))
    }

    /// Adds a root (one the native backend gave up on); it is dirty with its subtree, since
    /// whatever happened while nobody watched is unknown.
    pub(crate) fn add_root(&mut self, root: &Path, now: Timestamp) {
        if !self.filter.roots.iter().any(|r| r == root) {
            self.filter.roots.push(root.to_path_buf());
            self.queue.add_root(root);
        }
        self.folders.entry(root.to_path_buf()).or_insert(Seen::Unvisited);
        self.queue.push(root, true, now);
    }

    /// Lists the next folders of the sweep if a step is due, then hands out what has settled.
    fn tick(&mut self, now: Timestamp) -> Vec<DirtyFolder> {
        if self.next_step.is_none_or(|t| now >= t) {
            self.next_step = Some(now.saturating_add(self.options.step));
            self.step(now);
        }
        self.queue.take_ready(now)
    }

    /// Lists the next [`per_step`](Self::per_step) folders.
    fn step(&mut self, now: Timestamp) {
        let mut budget = self.per_step();
        while budget > 0 {
            let next = match &self.cursor {
                None => self.folders.keys().next().cloned(),
                Some(c) => {
                    self.folders.range::<Path, _>((std::ops::Bound::Excluded(c.as_path()), std::ops::Bound::Unbounded)).next().map(|(k, _)| k.clone())
                }
            };
            let Some(folder) = next else {
                // The end of a sweep.
                self.cursor = None;
                self.sweeps = self.sweeps.saturating_add(1);
                return;
            };
            self.visit(&folder, now);
            self.cursor = Some(folder);
            budget -= 1;
        }
    }

    fn visit(&mut self, folder: &Path, now: Timestamp) {
        let Some(&seen) = self.folders.get(folder) else { return };
        let learning = self.sweeps == 0;
        self.listed = self.listed.saturating_add(1);
        let filter = &self.filter;
        let limits = Limits { max_depth: 1, ..Limits::default() };
        let listing =
            dalica_fs::walk(folder, limits, |e: &Entry| filter.skip.skips(e) || (e.is_dir && filter.excluded.iter().any(|x| e.path.starts_with(x))));
        let walk = match listing {
            Ok(w) => w,
            Err(_) => {
                self.folders.insert(folder.to_path_buf(), Seen::Unreachable);
                return;
            }
        };
        let print = fingerprint(&walk.entries);
        match seen {
            Seen::Listed(old) if old != print => self.queue.push(folder, false, now),
            Seen::Unreachable if !learning => self.queue.push(folder, true, now),
            _ => {}
        }
        self.folders.insert(folder.to_path_buf(), Seen::Listed(print));
        // Subfolders: new ones are tracked (and dirty after the first sweep); gone ones forgotten
        // with everything below them (this folder's changed fingerprint already covers them).
        let depth_ok = self
            .filter
            .root_of(folder)
            .is_some_and(|r| folder.components().count().saturating_sub(r.components().count()) < dalica_fs::walk::MAX_DEPTH);
        let subdirs: Vec<&Path> = walk.entries.iter().filter(|e| e.is_dir && !e.is_link).map(|e| e.path.as_path()).collect();
        for sub in &subdirs {
            if !depth_ok || self.folders.contains_key(*sub) || self.folders.len() >= MAX_FOLDERS {
                continue;
            }
            self.folders.insert(sub.to_path_buf(), Seen::Unvisited);
            if !learning {
                self.queue.push(sub, true, now);
            }
        }
        let known: Vec<PathBuf> = self
            .folders
            .range::<Path, _>((std::ops::Bound::Excluded(folder), std::ops::Bound::Unbounded))
            .take_while(|(k, _)| k.starts_with(folder))
            .filter(|(k, _)| k.parent() == Some(folder))
            .map(|(k, _)| k.clone())
            .collect();
        for k in known {
            if !subdirs.contains(&k.as_path()) && !self.filter.roots.contains(&k) {
                self.forget(&k);
            }
        }
    }

    /// Forgets `folder` and everything tracked below it.
    fn forget(&mut self, folder: &Path) {
        let below: Vec<PathBuf> = self
            .folders
            .range::<Path, _>((std::ops::Bound::Included(folder), std::ops::Bound::Unbounded))
            .take_while(|(k, _)| k.starts_with(folder))
            .map(|(k, _)| k.clone())
            .collect();
        for k in below {
            if !self.filter.roots.contains(&k) {
                self.folders.remove(&k);
            }
        }
    }
}

impl Watcher for PollingWatcher {
    fn poll(&mut self, now: Timestamp) -> Vec<DirtyFolder> {
        self.tick(now)
    }

    fn roots(&self) -> Vec<(PathBuf, Backend)> {
        self.filter.roots.iter().map(|r| (r.clone(), Backend::Polling)).collect()
    }
}

/// A folder listing's fingerprint: every kept entry's name and kind, and each file's size and
/// time, in name order. A subfolder's own time is left out: what changes inside it is its own
/// fingerprint's business, so a new photo in a subfolder does not make its parent dirty too.
fn fingerprint(entries: &[Entry]) -> u64 {
    let mut h = DefaultHasher::new();
    entries.len().hash(&mut h);
    for e in entries {
        e.path.file_name().hash(&mut h);
        (e.is_dir, e.is_file, e.is_link).hash(&mut h);
        if !e.is_dir {
            (e.size, e.modified).hash(&mut h);
        }
    }
    h.finish()
}
