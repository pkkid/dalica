//! The sidecar writer (`data-model.md` §3, `architecture.md` §4). A folder whose user data
//! changed is marked dirty; it is written when it is due — at most once per second per folder on
//! the session's clock — or at once by [`SidecarWriter::flush`].
//!
//! Each write re-reads the sidecar, merges the model's values into it (keeping every other key, so
//! other programs' edits survive), skips the write if nothing changed, and replaces the file
//! atomically through `dalica-fs` with a check that it did not change since it was read. A sidecar
//! that cannot be parsed is kept aside as `.dalica.json.corrupt-<time>` and its folder becomes
//! read-only: nothing is written there until the user resets it.
//!
//! **Stale entries** (`data-model.md` §7): every write also sweeps the entries of photos whose file
//! is gone ([`crate::stale`]): marked in `dalica.missing` when first found gone, pruned by the
//! first write [`crate::stale::PRUNE_AFTER`] (30 days) later, unmarked if the file comes back.
//!
//! **Offline folders** (`data-model.md` §7, `security.md` §5.9): a folder that is not there — its
//! volume is unplugged or its share unreachable, or it was renamed or removed — is never created
//! to write into. The write fails ([`Outcome::Failed`], so the edit is reported unsaved, as
//! `control-protocol.md` §5.8 says), nothing is written, the folder stays dirty and waits: it is
//! tried again when it is marked dirty again (the engine does that when the folder's root comes
//! back) or after [`WAIT_RETRY`], not every second. A folder that left the model meanwhile is
//! dropped.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::{DateTime, FolderId, Timestamp};
use dalica_fs::Expect;
use dalica_library::Library;
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};
use serde_json::Value;

use crate::mapping::{self, FolderState};

/// The shortest time between two writes of one folder's sidecar.
pub const DEBOUNCE: Duration = Duration::from_secs(1);
/// How long a folder that was not there waits before the writer looks again on its own. Marking
/// it dirty again ends the wait.
pub const WAIT_RETRY: Duration = Duration::from_secs(10);

/// What happened to one folder's sidecar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file was replaced (or created).
    Written,
    /// The file already said this; nothing was written.
    Unchanged,
    /// The file could not be parsed: kept aside under this name, the folder is now read-only.
    SetAside(PathBuf),
    /// The folder is read-only after an earlier damaged sidecar; nothing was written.
    ReadOnly,
    /// The write failed (an offline volume, a concurrent change, a full disk…); the folder stays
    /// dirty. A folder that is not there is never created.
    Failed(String),
}

/// Writes one folder's sidecar now. Any thread but the UI thread may call it. A folder that is not
/// there is never created: the write fails.
pub fn write_folder(state: &FolderState, now: Timestamp) -> Outcome {
    match attempt(state, now) {
        Attempt::Done(outcome) => outcome,
        Attempt::Missing => missing(state),
    }
}

/// What one write found.
enum Attempt {
    Done(Outcome),
    /// The folder is not there (an offline volume, or renamed or removed): nothing was written.
    Missing,
}

fn missing(state: &FolderState) -> Outcome {
    Outcome::Failed(format!("{}: the folder is not there (is its volume offline?); the write waits for it", state.path.display()))
}

fn attempt(state: &FolderState, now: Timestamp) -> Attempt {
    // Checked first, so a missing folder is told apart from other failures and never created.
    match dalica_fs::identity(&state.path) {
        Ok(Some(_)) => {}
        Ok(None) => return Attempt::Missing,
        Err(e) => return Attempt::Done(Outcome::Failed(e.to_string())),
    }
    Attempt::Done(write_present(state, now))
}

fn write_present(state: &FolderState, now: Timestamp) -> Outcome {
    let path = state.path.join(FILE_NAME);
    let limits = Limits::default();
    let (mut sidecar, before, expect) = match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => (Sidecar::new(), None, Expect::Absent),
        Ok(Some(read)) => match Sidecar::parse(&read.bytes, &limits) {
            Ok(s) => (s, Some(read.bytes), Expect::Unchanged(read.identity)),
            Err(_) => return set_aside(&path, now),
        },
        Err(dalica_fs::Error::TooLarge { .. }) => return set_aside(&path, now),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    // Places holding a wrong-type value are kept as they are; the rest is written.
    let _refused = mapping::apply(state, &mut sidecar);
    let sidecar = sweep_stale(state, sidecar, now, &limits);
    let bytes = sidecar.print();
    if before.as_deref() == Some(bytes.as_slice()) {
        return Outcome::Unchanged;
    }
    // A folder that never had a sidecar and has nothing to say gets none (created lazily).
    if before.is_none() && sidecar == Sidecar::new() {
        return Outcome::Unchanged;
    }
    match dalica_fs::replace(&path, &bytes, expect) {
        Ok(_) => Outcome::Written,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// The sidecar with its stale entries marked, kept or pruned ([`crate::stale::sweep`]). Most
/// folders have no entry the model lacks and no marks: they are returned as they are without a
/// second parse. Anything that does not round-trip is left unswept (never a reason to fail).
fn sweep_stale(state: &FolderState, sidecar: Sidecar, now: Timestamp, limits: &Limits) -> Sidecar {
    let model: BTreeSet<&str> = state.photos.iter().map(|(n, _)| n.as_str()).collect();
    let orphans = sidecar.photo_names().any(|n| !model.contains(n));
    let printed = sidecar.print();
    let key = format!("\"{}\"", crate::stale::MISSING_KEY);
    let marks = printed.windows(key.len()).any(|w| w == key.as_bytes());
    if !orphans && !marks {
        return sidecar;
    }
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(&printed) else { return sidecar };
    if !crate::stale::wanted(&root, &model) {
        return sidecar;
    }
    let swept = crate::stale::sweep(&mut root, &model, |name| present(&state.path, name), now);
    if !swept.changed() {
        return sidecar;
    }
    match serde_json::to_vec(&Value::Object(root)).ok().and_then(|b| Sidecar::parse(&b, limits).ok()) {
        Some(swept) => swept,
        None => sidecar,
    }
}

/// Whether `folder` holds an entry named `name`: `true` unless it is known to be absent.
fn present(folder: &Path, name: &str) -> bool {
    !matches!(dalica_fs::identity(&folder.join(name)), Ok(None))
}

/// Keeps a damaged sidecar aside as `.dalica.json.corrupt-<time>`.
pub(crate) fn set_aside(path: &std::path::Path, now: Timestamp) -> Outcome {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
    match dalica_fs::set_aside(path, &format!("corrupt-{stamp}Z")) {
        Ok(kept) => Outcome::SetAside(kept),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// Which folders need writing, and when they were last written.
#[derive(Debug, Default)]
pub struct SidecarWriter {
    dirty: BTreeSet<FolderId>,
    last_write: HashMap<FolderId, Timestamp>,
    read_only: BTreeSet<FolderId>,
    /// Dirty folders that were not there, and when the writer last looked.
    waiting: HashMap<FolderId, Timestamp>,
}

impl SidecarWriter {
    pub fn new() -> SidecarWriter {
        SidecarWriter::default()
    }

    /// Notes that `folder`'s user data changed (or that its root came back): it is written when
    /// due. A folder waiting for its root is looked at again at its next due write.
    pub fn mark_dirty(&mut self, folder: FolderId) {
        self.dirty.insert(folder);
        self.waiting.remove(&folder);
    }

    pub fn is_dirty(&self, folder: FolderId) -> bool {
        self.dirty.contains(&folder)
    }

    pub fn has_pending(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// How many folders have a sidecar write still to make (waiting ones included).
    pub fn pending(&self) -> usize {
        self.dirty.len()
    }

    /// Dirty folders whose last write found them missing (waiting for their root), in id order.
    pub fn waiting(&self) -> impl Iterator<Item = FolderId> + '_ {
        self.dirty.iter().copied().filter(|f| self.waiting.contains_key(f))
    }

    /// Folders whose sidecar was damaged: nothing is written there.
    pub fn read_only(&self) -> impl Iterator<Item = FolderId> + '_ {
        self.read_only.iter().copied()
    }

    /// Lets writes into `folder` happen again (after the user reset or restored its sidecar).
    pub fn clear_read_only(&mut self, folder: FolderId) {
        self.read_only.remove(&folder);
    }

    /// Writes every dirty folder whose last write is at least [`DEBOUNCE`] old (a folder waiting
    /// for its root: whose last look is at least [`WAIT_RETRY`] old).
    pub fn write_due(&mut self, library: &Library, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        self.write_due_with(|f| mapping::folder_state(library, f), now)
    }

    /// [`Self::write_due`] for a writer that has no `Library` (a background thread fed with
    /// [`FolderState`]s): `state_of` gives a folder's current state, `None` if it left the model.
    pub fn write_due_with(&mut self, state_of: impl Fn(FolderId) -> Option<FolderState>, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        let due: Vec<FolderId> = self
            .dirty
            .iter()
            .copied()
            .filter(|f| match self.waiting.get(f) {
                Some(t) => now.since(*t) >= WAIT_RETRY,
                None => self.last_write.get(f).is_none_or(|t| now.since(*t) >= DEBOUNCE),
            })
            .collect();
        self.write(&state_of, &due, now)
    }

    /// Writes every dirty folder now, whatever the debounce says.
    pub fn flush(&mut self, library: &Library, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        self.flush_with(|f| mapping::folder_state(library, f), now)
    }

    /// [`Self::flush`] fed with folder states (see [`Self::write_due_with`]).
    pub fn flush_with(&mut self, state_of: impl Fn(FolderId) -> Option<FolderState>, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        let all: Vec<FolderId> = self.dirty.iter().copied().collect();
        self.write(&state_of, &all, now)
    }

    fn write(&mut self, state_of: &impl Fn(FolderId) -> Option<FolderState>, folders: &[FolderId], now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        let mut out = BTreeMap::new();
        for &f in folders {
            let (outcome, gone) = if self.read_only.contains(&f) {
                (Outcome::ReadOnly, false)
            } else {
                match state_of(f) {
                    // The folder left the model: nothing to write.
                    None => (Outcome::Unchanged, false),
                    Some(state) => match attempt(&state, now) {
                        Attempt::Done(outcome) => (outcome, false),
                        Attempt::Missing => (missing(&state), true),
                    },
                }
            };
            if gone {
                // Stays dirty; looked at again when marked dirty or after the wait.
                self.waiting.insert(f, now);
                out.insert(f, outcome);
                continue;
            }
            self.waiting.remove(&f);
            match &outcome {
                Outcome::Written => {
                    self.last_write.insert(f, now);
                    self.dirty.remove(&f);
                }
                Outcome::Unchanged | Outcome::ReadOnly => {
                    self.dirty.remove(&f);
                }
                Outcome::SetAside(_) => {
                    self.read_only.insert(f);
                    self.dirty.remove(&f);
                }
                Outcome::Failed(_) => {
                    // Stays dirty; the next due write retries, no sooner than the debounce.
                    self.last_write.insert(f, now);
                }
            }
            out.insert(f, outcome);
        }
        out
    }
}
