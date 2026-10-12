//! The native backend: the system's change events through `notify` 8 (inotify on Linux, FSEvents
//! on macOS, `ReadDirectoryChangesW` on Windows).
//!
//! Events are hints. Each one only marks folders dirty: the folder holding the changed entry, and
//! a folder that appeared or was renamed in with its subtree. A "rescan" notice or an error marks
//! the whole root. The verification pass decides what really changed. Events arrive on `notify`'s
//! thread and go straight into the bounded queue; [`NotifyWatcher::poll`] stamps them with the
//! caller's time, so debouncing runs on the same clock as everything else.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use dalica_core::Timestamp;
use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::filter::Filter;
use super::queue::{DirtyFolder, DirtyQueue, QueueOptions};
use super::{Backend, Watcher};

/// What `notify`'s thread shares with the caller.
#[derive(Debug)]
struct Shared {
    queue: DirtyQueue,
    filter: Filter,
    /// `(as given, as the system reports it)` for every root: event paths are mapped back.
    roots: Vec<(PathBuf, PathBuf)>,
    /// The time of the last poll: events between two polls are stamped with it.
    now: Timestamp,
    /// Roots this backend lost (deleted, unmounted, out of watches): the caller polls them instead.
    lost: Vec<PathBuf>,
    errors: u64,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    match m.lock() {
        Ok(g) => g,
        // A panic on notify's thread while it held the lock; the queue is still consistent.
        Err(p) => p.into_inner(),
    }
}

/// Watches roots through the system's events.
pub struct NotifyWatcher {
    watcher: RecommendedWatcher,
    shared: Arc<Mutex<Shared>>,
    watched: Vec<PathBuf>,
}

impl std::fmt::Debug for NotifyWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyWatcher").field("watched", &self.watched).finish_non_exhaustive()
    }
}

impl NotifyWatcher {
    /// A watcher with no roots yet (`filter`'s roots are ignored; [`watch`](Self::watch) adds
    /// them), or why the system's events cannot be had.
    pub(crate) fn new(filter: Filter, queue: QueueOptions) -> Result<NotifyWatcher, String> {
        let shared = Arc::new(Mutex::new(Shared {
            queue: DirtyQueue::new(&[], queue),
            filter: Filter { roots: Vec::new(), ..filter },
            roots: Vec::new(),
            now: Timestamp::EPOCH,
            lost: Vec::new(),
            errors: 0,
        }));
        let handler = Arc::clone(&shared);
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let mut s = lock(&handler);
            match event {
                Ok(e) => s.event(&e),
                Err(e) => s.error(&e),
            }
        })
        .map_err(|e| e.to_string())?;
        Ok(NotifyWatcher { watcher, shared, watched: Vec::new() })
    }

    /// Starts watching `root` and everything below it.
    pub(crate) fn watch(&mut self, root: &Path) -> Result<(), String> {
        // Register the root first, so its first events are not dropped as foreign.
        let reported = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        {
            let mut s = lock(&self.shared);
            s.filter.roots.push(root.to_path_buf());
            s.roots.push((root.to_path_buf(), reported));
            s.queue.add_root(root);
        }
        match self.watcher.watch(root, RecursiveMode::Recursive) {
            Ok(()) => {
                self.watched.push(root.to_path_buf());
                Ok(())
            }
            Err(e) => {
                let mut s = lock(&self.shared);
                s.filter.roots.retain(|r| r != root);
                s.roots.retain(|(r, _)| r != root);
                Err(e.to_string())
            }
        }
    }

    /// Roots this backend lost since the last call; they are no longer watched here.
    pub(crate) fn take_lost(&mut self) -> Vec<PathBuf> {
        let lost = std::mem::take(&mut lock(&self.shared).lost);
        for root in &lost {
            let _ = self.watcher.unwatch(root);
            self.watched.retain(|r| r != root);
            let mut s = lock(&self.shared);
            s.filter.roots.retain(|r| r != root);
            s.roots.retain(|(r, _)| r != root);
        }
        lost
    }

    /// Errors `notify` reported so far.
    pub fn errors(&self) -> u64 {
        lock(&self.shared).errors
    }
}

impl Watcher for NotifyWatcher {
    fn poll(&mut self, now: Timestamp) -> Vec<DirtyFolder> {
        let mut s = lock(&self.shared);
        s.now = now;
        s.queue.take_ready(now)
    }

    fn roots(&self) -> Vec<(PathBuf, Backend)> {
        self.watched.iter().map(|r| (r.clone(), Backend::Native)).collect()
    }
}

impl Shared {
    /// `path` as under the root it was given as (the system may report `/private/var/…` for
    /// `/var/…`, or a long `\\?\` path).
    fn map(&self, path: &Path) -> Option<PathBuf> {
        self.roots.iter().find_map(|(given, reported)| {
            if path.starts_with(given) {
                Some(path.to_path_buf())
            } else {
                path.strip_prefix(reported).ok().map(|rest| if rest.as_os_str().is_empty() { given.clone() } else { given.join(rest) })
            }
        })
    }

    fn event(&mut self, e: &Event) {
        let now = self.now;
        if matches!(e.kind, EventKind::Access(_)) {
            return;
        }
        let paths: Vec<PathBuf> = e.paths.iter().filter_map(|p| self.map(p)).collect();
        if e.need_rescan() {
            // The events lapsed: everything under the roots concerned (all, if none is named).
            let roots: Vec<PathBuf> = if paths.is_empty() {
                self.filter.roots.clone()
            } else {
                paths.iter().filter_map(|p| self.filter.root_of(p).map(Path::to_path_buf)).collect()
            };
            for r in roots {
                self.queue.push(&r, true, now);
            }
            return;
        }
        for path in paths {
            let is_dir = path.is_dir();
            let Some(root) = self.filter.accepts(&path, is_dir).map(Path::to_path_buf) else { continue };
            if path == root {
                if !is_dir {
                    // The root itself went away: hand it to the polling backend.
                    if !self.lost.contains(&root) {
                        self.lost.push(root);
                    }
                } else {
                    self.queue.push(&root, !matches!(e.kind, EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))), now);
                }
                continue;
            }
            if is_dir {
                match e.kind {
                    // Its own entries changed (its time or attributes).
                    EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
                        self.queue.push(&path, false, now);
                        continue;
                    }
                    // A folder appeared, was renamed or moved in: everything in it is news.
                    _ => self.queue.push(&path, true, now),
                }
            }
            if let Some(parent) = path.parent() {
                self.queue.push(parent, false, now);
            }
        }
    }

    fn error(&mut self, e: &notify::Error) {
        self.errors = self.errors.saturating_add(1);
        let now = self.now;
        let paths: Vec<PathBuf> = e.paths.iter().filter_map(|p| self.map(p)).collect();
        let mut roots: Vec<PathBuf> = paths.iter().filter_map(|p| self.filter.root_of(p).map(Path::to_path_buf)).collect();
        if roots.is_empty() {
            roots = self.filter.roots.clone();
        }
        let out_of_watches = matches!(e.kind, notify::ErrorKind::MaxFilesWatch);
        for r in roots {
            self.queue.push(&r, true, now);
            if out_of_watches && !self.lost.contains(&r) {
                self.lost.push(r);
            }
        }
    }
}
