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
//! dropped. A folder under a root the engine found offline ([`SidecarWriter::hold`]) waits the same
//! way even if a folder is there: the empty folder an unmounted volume leaves at its mount point
//! must never get a sidecar (M5.17).
//!
//! **Guarded roots** (M6.43): the engine's online checks run every 10 s, so a volume unmounted
//! between two of them is not held yet. Right before each write, a folder under a guarded root
//! ([`SidecarWriter::guard`]) has its root checked ([`RootCheck`]: the engine's look at the root's
//! device and the library's files there); a root that fails is treated like a missing folder: the
//! write fails, nothing is written, the folder waits and is looked at again (and checked again)
//! when it is marked dirty or after [`WAIT_RETRY`]. Each root is checked at most once per pass.
//!
//! **Carrying entries** (M9.9, `data-model.md` §8): [`SidecarWriter::carry`] moves a photo's entry
//! with its file ([`crate::carry`]) in order with the writes: the two folders' pending debounced
//! changes are written first, then the entries are carried, then the destination is written again
//! from the model so the model's values stand over the carried keys. A star made just before a
//! move, still waiting for its write, therefore lands in the destination.
//!
//! **Legacy-only folders** (M14, `data-model.md` §9): the first write of a folder whose state says
//! [`FolderSidecar::Legacy`] is [`crate::legacy::first_write`]: a whole new `.dalica.json`, checked
//! by reading it back, never written over a `.dalica.json` that appeared since the scan. The writer
//! remembers the folders it converted (their later writes are ordinary ones, whatever the state
//! still says until the engine records the fact `both`) and reports each first write once
//! ([`SidecarWriter::take_legacy`]): [`LegacyWrite::Converted`], or [`LegacyWrite::Appeared`] (the
//! write is reported failed, the folder is no longer dirty, and the caller reads it again). A carry
//! into or out of a legacy-only folder writes that folder's first `.dalica.json` before the carry.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{DateTime, FolderId, Timestamp};
use dalica_fs::Expect;
use dalica_library::{FolderSidecar, Library};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};
use serde_json::Value;

use crate::carry::{self, Carried, Carry, CarryError};
use crate::legacy::{self, FirstWrite, LegacyWrite};
use crate::mapping::{self, FolderState};

/// The shortest time between two writes of one folder's sidecar.
pub const DEBOUNCE: Duration = Duration::from_secs(1);
/// How long a folder that was not there waits before the writer looks again on its own. Marking
/// it dirty again ends the wait.
pub const WAIT_RETRY: Duration = Duration::from_secs(10);

/// Says, right before a write under a guarded root, whether that root still is the volume the
/// library knows (`true`) or not (an empty mount point, another volume in its place: `false`).
/// Runs on the writer's thread and may read the disk; one that panics counts as `false`.
pub type RootCheck = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// A guarded root and its check.
#[derive(Clone)]
struct Guard {
    root: PathBuf,
    check: RootCheck,
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guard").field("root", &self.root).finish_non_exhaustive()
    }
}

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

/// What [`SidecarWriter::carry`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryReport {
    /// The carry itself.
    pub carried: Result<Carried, CarryError>,
    /// The sidecar writes made on the way: the folders' pending changes before the carry, the
    /// destination's model values after it.
    pub writes: BTreeMap<FolderId, Outcome>,
}

/// Writes one folder's sidecar now. Any thread but the UI thread may call it. A folder that is not
/// there is never created: the write fails. A legacy-only folder ([`FolderSidecar::Legacy`]) gets
/// its first `.dalica.json` ([`crate::legacy::first_write`]); one that has a `.dalica.json`
/// already fails, nothing written.
pub fn write_folder(state: &FolderState, now: Timestamp) -> Outcome {
    match attempt(state, now, true) {
        Attempt::Done(outcome) | Attempt::Legacy(outcome, _) => outcome,
        Attempt::Missing => missing(state),
    }
}

/// What one write found.
enum Attempt {
    Done(Outcome),
    /// The folder is not there (an offline volume, or renamed or removed): nothing was written.
    Missing,
    /// A legacy-only folder's first write: what it did, and what that means for the folder.
    Legacy(Outcome, LegacyWrite),
}

fn missing(state: &FolderState) -> Outcome {
    Outcome::Failed(format!("{}: the folder is not there (is its volume offline?); the write waits for it", state.path.display()))
}

fn not_verified(state: &FolderState, root: &Path) -> Outcome {
    Outcome::Failed(format!(
        "{}: {} is not the volume the library knows (unmounted, or another one in its place); the write waits for it",
        state.path.display(),
        root.display()
    ))
}

/// One write of `state`; `first` when a legacy-only folder's state still means its first write
/// (the writer has not converted it yet).
fn attempt(state: &FolderState, now: Timestamp, first: bool) -> Attempt {
    // Checked first, so a missing folder is told apart from other failures and never created.
    match dalica_fs::identity(&state.path) {
        Ok(Some(_)) => {}
        Ok(None) => return Attempt::Missing,
        Err(e) => return Attempt::Done(Outcome::Failed(e.to_string())),
    }
    if first && state.sidecar == FolderSidecar::Legacy {
        return match legacy::first_write(state, now) {
            FirstWrite::Converted => Attempt::Legacy(Outcome::Written, LegacyWrite::Converted),
            FirstWrite::Appeared => Attempt::Legacy(
                Outcome::Failed(format!(
                    "{}: a {FILE_NAME} appeared since the folder was read: nothing was written; the folder is read again",
                    state.path.display()
                )),
                LegacyWrite::Appeared,
            ),
            FirstWrite::Nothing => Attempt::Done(Outcome::Unchanged),
            FirstWrite::Failed(why) => Attempt::Done(Outcome::Failed(why)),
        };
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
    /// Roots found offline: nothing under them is written.
    held: Vec<PathBuf>,
    /// Roots checked right before each write under them.
    guards: Vec<Guard>,
    /// Legacy-only folders whose first `.dalica.json` this writer wrote (M14): written as ordinary
    /// folders from then on.
    converted: BTreeSet<FolderId>,
    /// First writes of legacy-only folders not yet taken by [`SidecarWriter::take_legacy`].
    legacy: BTreeMap<FolderId, LegacyWrite>,
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

    /// The roots found offline (replacing the earlier list): folders under them are written as if
    /// they were not there (they wait, and a flush reports them unsaved). When the list changes, the
    /// waiting folders are looked at again at their next due write.
    pub fn hold(&mut self, roots: Vec<PathBuf>) {
        if roots != self.held {
            self.held = roots;
            self.waiting.clear();
        }
    }

    /// The roots held (found offline).
    pub fn held(&self) -> &[PathBuf] {
        &self.held
    }

    /// The roots to check right before each write under them, with their checks (replacing the
    /// earlier list). A folder under a root whose check fails is written as if it were not there:
    /// the write fails and the folder waits (looked at again when marked dirty or after
    /// [`WAIT_RETRY`], and checked again then).
    pub fn guard(&mut self, roots: Vec<(PathBuf, RootCheck)>) {
        self.guards = roots.into_iter().map(|(root, check)| Guard { root, check }).collect();
    }

    /// The guarded roots.
    pub fn guarded(&self) -> impl Iterator<Item = &Path> + '_ {
        self.guards.iter().map(|g| g.root.as_path())
    }

    /// The first guarded root above `path` whose check fails (each root checked at most once per
    /// pass: `verdicts` remembers the answers by index).
    fn failed_root(&self, path: &Path, verdicts: &mut HashMap<usize, bool>) -> Option<PathBuf> {
        for (i, g) in self.guards.iter().enumerate() {
            if !path.starts_with(&g.root) {
                continue;
            }
            let ok = *verdicts.entry(i).or_insert_with(|| catch_unwind(AssertUnwindSafe(|| (g.check)(&g.root))).unwrap_or(false));
            if !ok {
                return Some(g.root.clone());
            }
        }
        None
    }

    /// What the first writes of legacy-only folders did since the last call, by folder (M14): a
    /// [`LegacyWrite::Converted`] folder's fact is now `both`; an [`LegacyWrite::Appeared`] one is
    /// to be read again.
    pub fn take_legacy(&mut self) -> BTreeMap<FolderId, LegacyWrite> {
        std::mem::take(&mut self.legacy)
    }

    /// Whether this writer wrote legacy-only folder `folder`'s first `.dalica.json`.
    pub fn converted(&self, folder: FolderId) -> bool {
        self.converted.contains(&folder)
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
        self.write(&state_of, &due, now, false)
    }

    /// Writes every dirty folder now, whatever the debounce says.
    pub fn flush(&mut self, library: &Library, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        self.flush_with(|f| mapping::folder_state(library, f), now)
    }

    /// [`Self::flush`] fed with folder states (see [`Self::write_due_with`]).
    pub fn flush_with(&mut self, state_of: impl Fn(FolderId) -> Option<FolderState>, now: Timestamp) -> BTreeMap<FolderId, Outcome> {
        let all: Vec<FolderId> = self.dirty.iter().copied().collect();
        self.write(&state_of, &all, now, true)
    }

    /// Carries the sidecar entries of `carry` from folder `from` to folder `to` (the same folder for
    /// a rename) in order with this writer's writes (`data-model.md` §8, [`crate::carry`]): first
    /// the pending writes of both folders, from `state_of`; then the entries; then, when anything
    /// was carried, the destination is written from `state_of` again, so the model's values (a
    /// star still in the debounce, a cleared caption) stand over the carried keys while the keys
    /// the model does not know arrive as they were. Call it after the model holds the photos in
    /// their new folder (their states sent), so the source's next write no longer names them.
    ///
    /// Refused with nothing carried: a read-only folder (an earlier damaged sidecar), a folder
    /// under a held root or whose guarded root fails its check, a folder that is not there. A
    /// damaged sidecar met on the way is kept aside and its folder becomes read-only, as a write
    /// would make it.
    pub fn carry(
        &mut self,
        carry: &Carry,
        from: FolderId,
        to: FolderId,
        state_of: impl Fn(FolderId) -> Option<FolderState>,
        now: Timestamp,
    ) -> CarryReport {
        let folders: Vec<(FolderId, &Path)> = if from == to { vec![(from, &carry.from)] } else { vec![(from, &carry.from), (to, &carry.to)] };
        let mut writes = BTreeMap::new();
        let mut verdicts = HashMap::new();
        for &(f, path) in &folders {
            let refused = if self.read_only.contains(&f) {
                Some(CarryError::ReadOnly(path.to_path_buf()))
            } else if self.held.iter().any(|r| path.starts_with(r)) || self.failed_root(path, &mut verdicts).is_some() {
                Some(CarryError::Missing(path.to_path_buf()))
            } else {
                None
            };
            if let Some(e) = refused {
                return CarryReport { carried: Err(e), writes };
            }
        }
        // A legacy-only folder gets its whole first `.dalica.json` before entries are carried into
        // or out of it, so the carry never starts a file holding only the carried entries.
        let first: Vec<FolderId> = folders
            .iter()
            .map(|(f, _)| *f)
            .filter(|f| !self.converted.contains(f) && state_of(*f).is_some_and(|s| s.sidecar == FolderSidecar::Legacy))
            .collect();
        self.dirty.extend(first.iter().copied());
        let pending: Vec<FolderId> = folders.iter().map(|(f, _)| *f).filter(|f| self.dirty.contains(f)).collect();
        writes.extend(self.write(&state_of, &pending, now, true));
        for f in first {
            match writes.get(&f) {
                // Converted, or nothing to say yet: what is there from here on is Dalica's own.
                Some(Outcome::Written | Outcome::Unchanged) => {
                    self.converted.insert(f);
                }
                Some(Outcome::Failed(why)) => return CarryReport { carried: Err(CarryError::Fs(why.clone())), writes },
                _ => {}
            }
        }
        // A pending write that found a damaged sidecar kept it aside: nothing goes there now.
        if let Some(&(_, path)) = folders.iter().find(|(f, _)| self.read_only.contains(f)) {
            return CarryReport { carried: Err(CarryError::ReadOnly(path.to_path_buf())), writes };
        }
        let carried = carry::entries(carry, now);
        if let Err(CarryError::SetAside { path, .. }) = &carried {
            for &(f, folder) in &folders {
                if path.parent() == Some(folder) {
                    self.read_only.insert(f);
                    self.dirty.remove(&f);
                }
            }
        }
        if matches!(&carried, Ok(c) if c.changed()) || matches!(&carried, Err(CarryError::SourceKept { .. })) {
            self.mark_dirty(to);
            writes.extend(self.write(&state_of, &[to], now, true));
        }
        CarryReport { carried, writes }
    }

    /// Writes `folders`. A folder under a held root waits; it is reported (as not there) only by
    /// a flush (`report_held`), not at every due write: the hold already says why it waits.
    fn write(
        &mut self,
        state_of: &impl Fn(FolderId) -> Option<FolderState>,
        folders: &[FolderId],
        now: Timestamp,
        report_held: bool,
    ) -> BTreeMap<FolderId, Outcome> {
        let mut out = BTreeMap::new();
        let mut verdicts = HashMap::new();
        for &f in folders {
            let (outcome, gone) = if self.read_only.contains(&f) {
                (Outcome::ReadOnly, false)
            } else {
                match state_of(f) {
                    // The folder left the model: nothing to write.
                    None => (Outcome::Unchanged, false),
                    Some(state) if self.held.iter().any(|r| state.path.starts_with(r)) => {
                        if !report_held {
                            self.waiting.insert(f, now);
                            continue;
                        }
                        (missing(&state), true)
                    }
                    Some(state) => match self.failed_root(&state.path, &mut verdicts) {
                        // Checked right before the write: not the library's volume (M6.43).
                        Some(root) => (not_verified(&state, &root), true),
                        None => match attempt(&state, now, !self.converted.contains(&f)) {
                            Attempt::Done(outcome) => (outcome, false),
                            Attempt::Missing => (missing(&state), true),
                            Attempt::Legacy(outcome, what) => {
                                self.legacy.insert(f, what);
                                match what {
                                    LegacyWrite::Converted => {
                                        self.converted.insert(f);
                                    }
                                    // Not retried: the file there wins, and the folder is read again.
                                    LegacyWrite::Appeared => {
                                        self.dirty.remove(&f);
                                        self.waiting.remove(&f);
                                        out.insert(f, outcome);
                                        continue;
                                    }
                                }
                                (outcome, false)
                            }
                        },
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

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;
    use dalica_library::{Folder, Op, Photo};

    use super::*;

    /// A folder under a held root (an empty mount point) is never written, though it is there:
    /// due writes pass it over, a flush reports it as not there, and it is written once released.
    #[test]
    fn a_held_root_gets_no_sidecar_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("CARD");
        std::fs::create_dir(&root).unwrap();
        let mut lib = Library::new();
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.clone() } }).unwrap();
        let id = PhotoId::new(1);
        let photo = Photo { id, folder: f, name: "a.jpg".into(), size: 1, mtime: Timestamp::EPOCH, width: 1, height: 1, date: None, star: true };
        lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
        let mut w = SidecarWriter::new();
        w.hold(vec![root.clone()]);
        w.mark_dirty(f);
        assert!(w.write_due(&lib, Timestamp::EPOCH).is_empty(), "passed over quietly");
        assert!(matches!(&w.flush(&lib, Timestamp::EPOCH)[&f], Outcome::Failed(m) if m.contains("not there")));
        assert!(w.is_dirty(f) && !root.join(FILE_NAME).exists());
        assert_eq!(w.held(), std::slice::from_ref(&root));
        w.hold(Vec::new());
        assert_eq!(w.write_due(&lib, Timestamp::EPOCH)[&f], Outcome::Written);
        assert!(root.join(FILE_NAME).exists());
    }
}
