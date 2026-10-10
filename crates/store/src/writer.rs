//! The sidecar writer (`data-model.md` §3, `architecture.md` §4). A folder whose user data
//! changed is marked dirty; it is written when it is due — at most once per second per folder on
//! the session's clock — or at once by [`SidecarWriter::flush`].
//!
//! Each write re-reads the sidecar, merges the model's values into it (keeping every other key, so
//! other programs' edits survive), skips the write if nothing changed, and replaces the file
//! atomically through `dalica-fs` with a check that it did not change since it was read. A sidecar
//! that cannot be parsed is kept aside as `.dalica.json.corrupt-<time>` and its folder becomes
//! read-only: nothing is written there until the user resets it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use dalica_core::{DateTime, FolderId, Timestamp};
use dalica_fs::Expect;
use dalica_library::Library;
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::mapping::{self, FolderState};

/// The shortest time between two writes of one folder's sidecar.
pub const DEBOUNCE: Duration = Duration::from_secs(1);

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
    /// The write failed (offline volume, a concurrent change, a full disk…); the folder stays dirty.
    Failed(String),
}

/// Writes one folder's sidecar now. Any thread but the UI thread may call it.
pub fn write_folder(state: &FolderState, now: Timestamp) -> Outcome {
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
}

impl SidecarWriter {
    pub fn new() -> SidecarWriter {
        SidecarWriter::default()
    }

    /// Notes that `folder`'s user data changed.
    pub fn mark_dirty(&mut self, folder: FolderId) {
        self.dirty.insert(folder);
    }

    pub fn is_dirty(&self, folder: FolderId) -> bool {
        self.dirty.contains(&folder)
    }

    pub fn has_pending(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Folders whose sidecar was damaged: nothing is written there.
    pub fn read_only(&self) -> impl Iterator<Item = FolderId> + '_ {
        self.read_only.iter().copied()
    }

    /// Lets writes into `folder` happen again (after the user reset or restored its sidecar).
    pub fn clear_read_only(&mut self, folder: FolderId) {
        self.read_only.remove(&folder);
    }

    /// Writes every dirty folder whose last write is at least [`DEBOUNCE`] old.
    pub fn write_due(&mut self, library: &Library, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        self.write_due_with(|f| mapping::folder_state(library, f), now)
    }

    /// [`Self::write_due`] for a writer that has no `Library` (a background thread fed with
    /// [`FolderState`]s): `state_of` gives a folder's current state, `None` if it left the model.
    pub fn write_due_with(&mut self, state_of: impl Fn(FolderId) -> Option<FolderState>, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        let due: Vec<FolderId> = self.dirty.iter().copied().filter(|f| self.last_write.get(f).is_none_or(|t| now.since(*t) >= DEBOUNCE)).collect();
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
            let outcome = if self.read_only.contains(&f) {
                Outcome::ReadOnly
            } else {
                match state_of(f) {
                    // The folder left the model: nothing to write.
                    None => Outcome::Unchanged,
                    Some(state) => write_folder(&state, now),
                }
            };
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
