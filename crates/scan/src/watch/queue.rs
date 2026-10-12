//! The dirty-folder queue: debounced, coalesced and bounded (`data-model.md` §7).
//!
//! Both backends push what they notice here; the caller takes the folders whose changes have
//! settled. A folder is queued once however many events touch it, a queued subtree absorbs every
//! folder below it, and when more folders are queued than the cap the deepest ones are folded
//! into their parents (as subtrees) until the queue fits again. Nothing is ever dropped: a
//! coalesced entry always covers at least what it replaced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::Timestamp;

/// Most folders queued at once (`QueueOptions::cap`).
pub const QUEUE_CAP: usize = 4096;
/// How long a folder must stay quiet before it is handed out.
pub const DEBOUNCE: Duration = Duration::from_millis(500);
/// A folder that keeps changing is handed out this long after its first change anyway.
pub const MAX_DELAY: Duration = Duration::from_secs(5);

/// A folder whose content changed on disk: the hint that it needs verifying.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirtyFolder {
    pub path: PathBuf,
    /// Everything below it may have changed too (a folder appeared, was renamed in, or the
    /// backend lost track); otherwise only its own entries did.
    pub subtree: bool,
}

impl DirtyFolder {
    /// Whether verifying this entry covers changes in `folder`.
    pub fn covers(&self, folder: &Path) -> bool {
        self.path == folder || (self.subtree && folder.starts_with(&self.path))
    }
}

/// The queue's bounds and timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueOptions {
    /// Most folders queued at once (at least one per watched root).
    pub cap: usize,
    pub debounce: Duration,
    pub max_delay: Duration,
}

impl Default for QueueOptions {
    fn default() -> Self {
        QueueOptions { cap: QUEUE_CAP, debounce: DEBOUNCE, max_delay: MAX_DELAY }
    }
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    subtree: bool,
    first: Timestamp,
    last: Timestamp,
}

/// Dirty folders waiting for their changes to settle.
#[derive(Clone, Debug)]
pub struct DirtyQueue {
    roots: Vec<PathBuf>,
    options: QueueOptions,
    pending: BTreeMap<PathBuf, Pending>,
    events: u64,
    folds: u64,
}

impl DirtyQueue {
    /// A queue for folders under `roots`; anything pushed outside them is ignored.
    pub fn new(roots: &[PathBuf], options: QueueOptions) -> DirtyQueue {
        let cap = options.cap.max(roots.len()).max(1);
        DirtyQueue { roots: roots.to_vec(), options: QueueOptions { cap, ..options }, pending: BTreeMap::new(), events: 0, folds: 0 }
    }

    /// Queues `path` (only its own entries, or with `subtree` everything below it too), changed at
    /// `now`.
    pub fn push(&mut self, path: &Path, subtree: bool, now: Timestamp) {
        self.events = self.events.saturating_add(1);
        if !self.roots.iter().any(|r| path.starts_with(r)) {
            return;
        }
        self.merge(path, Pending { subtree, first: now, last: now });
        self.fit();
    }

    /// Takes the folders that have been quiet for the debounce time, or waited the longest delay,
    /// in path order.
    pub fn take_ready(&mut self, now: Timestamp) -> Vec<DirtyFolder> {
        let QueueOptions { debounce, max_delay, .. } = self.options;
        let ready: Vec<PathBuf> =
            self.pending.iter().filter(|(_, p)| now.since(p.last) >= debounce || now.since(p.first) >= max_delay).map(|(k, _)| k.clone()).collect();
        ready.into_iter().filter_map(|path| self.pending.remove(&path).map(|p| DirtyFolder { path, subtree: p.subtree })).collect()
    }

    /// Takes every queued folder, settled or not, in path order.
    pub fn take_all(&mut self) -> Vec<DirtyFolder> {
        std::mem::take(&mut self.pending).into_iter().map(|(path, p)| DirtyFolder { path, subtree: p.subtree }).collect()
    }

    /// Folders queued.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Events pushed since the queue was made.
    pub fn events(&self) -> u64 {
        self.events
    }

    /// How many times the queue folded its deepest folders into their parents to stay in its cap.
    pub fn folds(&self) -> u64 {
        self.folds
    }

    pub fn cap(&self) -> usize {
        self.options.cap
    }

    /// Adds `path` to the queue, merged with what is there.
    fn merge(&mut self, path: &Path, new: Pending) {
        // Already covered by a queued subtree above it (or queued itself)?
        let hit = path.ancestors().find(|a| self.pending.get(*a).is_some_and(|p| *a == path || p.subtree)).map(Path::to_path_buf);
        let absorbs = match hit {
            Some(at) => {
                let Some(p) = self.pending.get_mut(&at) else { return };
                p.first = p.first.min(new.first);
                p.last = p.last.max(new.last);
                let grows = at == path && new.subtree && !p.subtree;
                p.subtree |= at == path && new.subtree;
                grows
            }
            None => {
                self.pending.insert(path.to_path_buf(), new);
                new.subtree
            }
        };
        if absorbs {
            self.absorb_below(path);
        }
    }

    /// Folds every queued folder strictly below `path` into it.
    fn absorb_below(&mut self, path: &Path) {
        let below: Vec<PathBuf> = self
            .pending
            .range::<Path, _>((std::ops::Bound::Excluded(path), std::ops::Bound::Unbounded))
            .take_while(|(k, _)| k.starts_with(path))
            .map(|(k, _)| k.clone())
            .collect();
        let mut first = None::<Timestamp>;
        let mut last = None::<Timestamp>;
        for k in below {
            if let Some(p) = self.pending.remove(&k) {
                first = Some(first.map_or(p.first, |f| f.min(p.first)));
                last = Some(last.map_or(p.last, |l| l.max(p.last)));
            }
        }
        if let Some(p) = self.pending.get_mut(path) {
            if let Some(f) = first {
                p.first = p.first.min(f);
            }
            if let Some(l) = last {
                p.last = p.last.max(l);
            }
        }
    }

    /// Watches one more root.
    pub(crate) fn add_root(&mut self, root: &Path) {
        if !self.is_root(root) {
            self.roots.push(root.to_path_buf());
            self.options.cap = self.options.cap.max(self.roots.len());
        }
    }

    fn is_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|r| r == path)
    }

    /// Folds the deepest queued folders into their parents (as subtrees) until the queue fits.
    fn fit(&mut self) {
        while self.pending.len() > self.options.cap {
            let deepest = self.pending.keys().filter(|k| !self.is_root(k)).map(|k| k.components().count()).max();
            let Some(depth) = deepest else { return };
            let at: Vec<PathBuf> = self.pending.keys().filter(|k| !self.is_root(k) && k.components().count() == depth).cloned().collect();
            for k in at {
                let Some(p) = self.pending.remove(&k) else { continue };
                match k.parent() {
                    Some(parent) if self.roots.iter().any(|r| parent.starts_with(r)) => self.merge(parent, Pending { subtree: true, ..p }),
                    // Not below a root (cannot happen: pushes outside the roots are ignored): keep it.
                    _ => {
                        self.pending.insert(k, p);
                        return;
                    }
                }
            }
            self.folds = self.folds.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: i64) -> Timestamp {
        Timestamp::from_millis(ms)
    }

    #[test]
    fn a_folder_is_queued_once_and_handed_out_when_quiet() {
        let root = PathBuf::from("/r");
        let mut q = DirtyQueue::new(std::slice::from_ref(&root), QueueOptions::default());
        q.push(&root.join("a"), false, t(0));
        q.push(&root.join("a"), false, t(400));
        assert_eq!(q.len(), 1);
        assert!(q.take_ready(t(800)).is_empty(), "still settling");
        assert_eq!(q.take_ready(t(900)), [DirtyFolder { path: root.join("a"), subtree: false }]);
        assert!(q.is_empty());
    }

    #[test]
    fn a_busy_folder_is_handed_out_after_the_longest_delay() {
        let root = PathBuf::from("/r");
        let mut q = DirtyQueue::new(std::slice::from_ref(&root), QueueOptions::default());
        for i in 0..20 {
            q.push(&root.join("a"), false, t(i * 300));
        }
        assert_eq!(q.take_ready(t(5_000)).len(), 1);
    }

    #[test]
    fn a_subtree_absorbs_what_is_below_it() {
        let root = PathBuf::from("/r");
        let mut q = DirtyQueue::new(std::slice::from_ref(&root), QueueOptions::default());
        q.push(&root.join("a/b"), false, t(0));
        q.push(&root.join("a/c/d"), false, t(0));
        q.push(&root.join("ab"), false, t(0));
        q.push(&root.join("a"), true, t(0));
        q.push(&root.join("a/e"), false, t(0));
        let all = q.take_all();
        assert_eq!(all, [DirtyFolder { path: root.join("a"), subtree: true }, DirtyFolder { path: root.join("ab"), subtree: false }]);
    }

    #[test]
    fn outside_the_roots_is_ignored() {
        let mut q = DirtyQueue::new(&[PathBuf::from("/r")], QueueOptions::default());
        q.push(Path::new("/elsewhere/a"), true, t(0));
        assert!(q.is_empty());
        assert_eq!(q.events(), 1);
    }
}
