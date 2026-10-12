//! Watching the roots for changes (`data-model.md` §7): events are hints, verification is the
//! truth.
//!
//! A [`Watcher`] reports dirty folders: debounced, coalesced, from a bounded queue
//! ([`DirtyQueue`]). [`Watching::start`] watches each root with the system's events through
//! `notify` ([`NotifyWatcher`]) and falls back, root by root, to the pure-Rust
//! [`PollingWatcher`] where that fails: a network share, a watch limit reached, a root the native
//! backend lost. The caller verifies what is reported with [`rescan`] and [`diff_rescan`]
//! (incremental: only the dirty folders), and runs a full verification ([`scan_with`] and
//! [`diff`]) when its [`Schedule`] says so, which catches anything the events missed.
//!
//! Everything here may touch the disk (the polling backend lists folders, starting the native
//! backend registers every folder of a root): use it from a worker thread, never the UI thread.
//!
//! [`diff_rescan`]: crate::diff_rescan
//! [`diff`]: crate::diff
//! [`scan_with`]: crate::scan_with

mod filter;
mod native;
mod poll;
mod queue;
mod verify;

use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::Timestamp;

pub use native::NotifyWatcher;
pub use poll::{MAX_FOLDERS, PollOptions, PollingWatcher};
pub use queue::{DEBOUNCE, DirtyFolder, DirtyQueue, MAX_DELAY, QUEUE_CAP, QueueOptions};
pub use verify::{Rescan, Scope, known_folders, plan, rescan};

use crate::skip::SkipRules;
use crate::volumes::{VolumeKind, volumes};
use filter::Filter;

/// How a root is watched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// The system's change events (`notify`).
    Native,
    /// Listing folders in sweeps.
    Polling,
}

/// Which backend to try first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Prefer {
    /// The system's events where they work, polling elsewhere.
    #[default]
    Native,
    /// Polling only.
    Polling,
}

/// How to watch.
#[derive(Clone, Debug)]
pub struct WatchOptions {
    pub prefer: Prefer,
    /// As the scan's ([`Options::skip`](crate::Options::skip)): changes in what scans skip are
    /// not reported.
    pub skip: SkipRules,
    /// As the scan's ([`Options::excluded`](crate::Options::excluded)).
    pub excluded: Vec<PathBuf>,
    pub queue: QueueOptions,
    pub poll: PollOptions,
    /// Whether roots on network shares are polled (their events are unreliable). Tests turn it off.
    pub poll_network: bool,
}

impl Default for WatchOptions {
    fn default() -> Self {
        WatchOptions {
            prefer: Prefer::Native,
            skip: SkipRules::system(),
            excluded: Vec::new(),
            queue: QueueOptions::default(),
            poll: PollOptions::default(),
            poll_network: true,
        }
    }
}

/// Something that reports dirty folders.
pub trait Watcher: Send {
    /// The folders that changed and have settled (each at most once, in path order); nothing when
    /// nothing did. `now` is the caller's clock: debouncing, and the polling backend's sweeps, run
    /// on it. The polling backend lists folders here: call it from a worker thread.
    fn poll(&mut self, now: Timestamp) -> Vec<DirtyFolder>;
    /// Every watched root and how it is watched.
    fn roots(&self) -> Vec<(PathBuf, Backend)>;
}

/// Why a root is polled instead of watched natively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fallback {
    pub root: PathBuf,
    pub reason: String,
}

/// The roots watched natively where possible, polled elsewhere.
#[derive(Debug)]
pub struct Watching {
    native: Option<NotifyWatcher>,
    polling: PollingWatcher,
    fallbacks: Vec<Fallback>,
}

impl Watching {
    /// Starts watching `roots`. Never fails: a root the native backend cannot watch is polled,
    /// with the reason in [`fallbacks`](Self::fallbacks).
    pub fn start(roots: &[PathBuf], options: WatchOptions) -> Watching {
        let filter = Filter { roots: Vec::new(), skip: options.skip.clone(), excluded: options.excluded.clone() };
        let mut polled = Vec::new();
        let mut fallbacks = Vec::new();
        let mut native = match options.prefer {
            Prefer::Polling => None,
            Prefer::Native => match NotifyWatcher::new(filter.clone(), options.queue) {
                Ok(w) => Some(w),
                Err(reason) => {
                    fallbacks.extend(roots.iter().map(|r| Fallback { root: r.clone(), reason: format!("no change events: {reason}") }));
                    None
                }
            },
        };
        let network: Vec<PathBuf> = if options.poll_network && native.is_some() {
            volumes().into_iter().filter(|v| v.kind == VolumeKind::Network).map(|v| v.path).collect()
        } else {
            Vec::new()
        };
        for root in roots {
            match native.as_mut() {
                None => polled.push(root.clone()),
                Some(_) if on_network(root, &network) => {
                    fallbacks.push(Fallback { root: root.clone(), reason: "a network share: its change events are unreliable".into() });
                    polled.push(root.clone());
                }
                Some(w) => {
                    if let Err(reason) = w.watch(root) {
                        fallbacks.push(Fallback { root: root.clone(), reason });
                        polled.push(root.clone());
                    }
                }
            }
        }
        let polling = PollingWatcher::new(Filter { roots: polled, ..filter }, options.queue, options.poll);
        Watching { native, polling, fallbacks }
    }

    /// Roots polled instead of watched natively, and why.
    pub fn fallbacks(&self) -> &[Fallback] {
        &self.fallbacks
    }

    pub fn native(&self) -> Option<&NotifyWatcher> {
        self.native.as_ref()
    }

    pub fn polling(&self) -> &PollingWatcher {
        &self.polling
    }
}

impl Watcher for Watching {
    fn poll(&mut self, now: Timestamp) -> Vec<DirtyFolder> {
        let mut out = Vec::new();
        if let Some(native) = self.native.as_mut() {
            for root in native.take_lost() {
                self.fallbacks.push(Fallback { root: root.clone(), reason: "the change events stopped for this folder".into() });
                self.polling.add_root(&root, now);
            }
            out.extend(native.poll(now));
        }
        out.extend(self.polling.poll(now));
        out.sort();
        out.dedup();
        out
    }

    fn roots(&self) -> Vec<(PathBuf, Backend)> {
        let mut out: Vec<(PathBuf, Backend)> = self.native.as_ref().map(Watcher::roots).unwrap_or_default();
        out.extend(self.polling.roots());
        out
    }
}

fn on_network(root: &Path, network: &[PathBuf]) -> bool {
    network.iter().any(|n| root.starts_with(n))
}

/// How often the whole library is verified, whatever the watchers say.
pub const FULL_VERIFICATION: Duration = Duration::from_secs(6 * 60 * 60);

/// When the next full verification is due: one at start, then every `every`, and at once after
/// the watchers are restarted (call [`Schedule::now`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    every: Duration,
    next: Option<Timestamp>,
}

impl Schedule {
    /// A schedule whose first verification is due at once.
    pub fn new(every: Duration) -> Schedule {
        Schedule { every, next: None }
    }

    /// Whether a full verification is due at `now`.
    pub fn due(&self, now: Timestamp) -> bool {
        self.next.is_none_or(|t| now >= t)
    }

    /// A full verification finished at `now`: the next is due `every` later.
    pub fn done(&mut self, now: Timestamp) {
        self.next = Some(now.saturating_add(self.every));
    }

    /// Makes a full verification due at once.
    pub fn now(&mut self) {
        self.next = None;
    }

    /// When the next one is due; `None`: now.
    pub fn next(&self) -> Option<Timestamp> {
        self.next
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule::new(FULL_VERIFICATION)
    }
}
