//! A damaged sidecar and Reset folder data (`data-model.md` §3): `folder.resetData`, and the list
//! of folders whose sidecar was damaged.
//!
//! - **Found.** The I/O worker's sidecar writer keeps an unreadable `.dalica.json` aside as
//!   `.dalica.json.corrupt-<time>` and writes nothing more into its folder (`dalica-store`'s
//!   `SidecarWriter`); a carry that meets one does the same. The worker notes each such folder in
//!   the list [`Damage`], shared with the session, which copies it at each [`Session::poll`] and
//!   [`Session::flush`]: [`Session::damaged_folders`] and [`Session::damaged_folder`] read that copy,
//!   in memory (the light table's caution banner asks every frame).
//! - **Kept across a restart.** The worker writes the list to [`FILE`] in the library directory
//!   whenever it changes, and the session reads it at open: after a restart nothing is written into
//!   a damaged folder (no sidecar write, no carried entry) until the user chooses, though the
//!   writer itself forgets its read-only folders. The list is keyed by the folder's path, so an
//!   index rebuilt from the folders keeps it. A damaged list file is kept aside as
//!   `.corrupt-<time>` and read as empty (a notice says so).
//! - **`folder.resetData {folder, confirm}`** (Files, no history) asks first: without
//!   `"confirm": true` it refuses with [`RESET_QUESTION`] (`needs_confirm` over automation). For a
//!   damaged folder it reads the folder's sidecar again on the I/O worker, in order with the
//!   writes: a readable one there now (the user restored it by hand) gives the model its values,
//!   as a rebuild reads them, and the folder's state is lifted with nothing written by the reset;
//!   none there, the state is lifted and the writer writes a new sidecar from the model (what was
//!   set while the folder was damaged included); an unreadable one there (a second damage) is kept
//!   aside under a name of its own first, never replacing the first copy, then the new sidecar is
//!   written. The `.corrupt-<time>` copies are never read, renamed, overwritten or deleted. The
//!   reply names them: `{folder, kept: [path], restored, written}`. Refused: an unknown folder
//!   (`not_found`), a folder that is not damaged or not there (`disabled`), hostile params.
//! - **Photo files too** (M10.57): the metadata write-back (`metadata.rs`) holds the writes of the
//!   photos in a listed folder (captions, tags, dates into their XMP/IPTC) and sends them once the
//!   folder is taken off the list, which [`Session::sync_damage`] tells it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::mpsc;

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_fs::Expect;
use dalica_library::{Library, Op};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};
use dalica_store::FolderState;
use dalica_store::mapping;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::{Msg, Session};

/// The list of damaged folders in the library directory: `{"version": 1, "folders": [{"path",
/// "kept": [path]}]}`.
pub const FILE: &str = "damaged-folders.json";
/// The largest list file read.
const CAP: u64 = 8 << 20;
/// The most folders the list holds, and the most kept copies it names per folder.
const MAX_FOLDERS: usize = 100_000;
const MAX_KEPT: usize = 64;

/// The question Reset Folder Data asks, its title and its default button.
pub const RESET_TITLE: &str = "Reset Folder Data";
pub const RESET_QUESTION: &str = "Reset this folder's data? Dalica writes a new data file from what it knows about these photos. The damaged file stays beside the photos, untouched.";
pub const RESET_BUTTON: &str = "Reset";

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "folder.resetData",
        label: RESET_TITLE,
        menu: "",
        keys: &[],
        params: r#"{"folder":FolderId, "confirm"?:bool}: for a folder whose data file (.dalica.json) was damaged and kept aside as .dalica.json.corrupt-<time>: reads the folder's data file again; one restored by hand is read into the library and nothing is written, else a new one is written from the library (an unreadable one found there is kept aside under its own name first). The kept copies are never touched. Asks first: needs "confirm": true. Replies {folder, kept:[path], restored, written}"#,
        effect: Effect::Files,
        history: History::None,
        enabled: always,
        run: reset,
    }]
}

/// The folders whose sidecar was damaged, by path, each with the copies kept aside; shared by the
/// session and the I/O worker. [`Damage::revision`] counts the changes.
#[derive(Debug, Default)]
pub(crate) struct Damage {
    folders: BTreeMap<PathBuf, Vec<PathBuf>>,
    revision: u64,
}

impl Damage {
    pub(crate) fn contains(&self, folder: &Path) -> bool {
        self.folders.contains_key(folder)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<PathBuf, Vec<PathBuf>> {
        self.folders.clone()
    }

    /// Notes that `folder`'s sidecar was damaged (and kept aside as `kept`, when that worked).
    /// Returns whether the list changed.
    pub(crate) fn note(&mut self, folder: &Path, kept: Option<&Path>) -> bool {
        if !self.folders.contains_key(folder) && self.folders.len() >= MAX_FOLDERS {
            log::warn!("{FILE}: more than {MAX_FOLDERS} damaged folders; {} is not listed", folder.display());
            return false;
        }
        let list = self.folders.entry(folder.to_path_buf()).or_default();
        let before = list.len();
        if let Some(k) = kept.filter(|k| !list.iter().any(|x| x == k))
            && list.len() < MAX_KEPT
        {
            list.push(k.to_path_buf());
        }
        let changed = before == 0 || list.len() != before;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }

    /// Takes `folder` off the list; returns the copies it had kept, `None` if it was not listed.
    pub(crate) fn lift(&mut self, folder: &Path) -> Option<Vec<PathBuf>> {
        let kept = self.folders.remove(folder)?;
        self.revision = self.revision.wrapping_add(1);
        Some(kept)
    }

    fn json(&self) -> Vec<u8> {
        let folders: Vec<Value> = self
            .folders
            .iter()
            .filter_map(|(path, kept)| {
                let kept: Vec<&str> = kept.iter().filter_map(|k| k.to_str()).collect();
                Some(json!({ "path": path.to_str()?, "kept": kept }))
            })
            .collect();
        let mut bytes = serde_json::to_vec_pretty(&json!({ "version": 1, "folders": folders })).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

#[derive(Deserialize)]
struct FileEntry {
    path: PathBuf,
    #[serde(default)]
    kept: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct ListFile {
    version: u32,
    folders: Vec<FileEntry>,
}

/// `corrupt-<time>Z`, the tag of a copy kept aside at `now`.
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
    format!("corrupt-{stamp}Z")
}

/// Reads the list in the library directory `dir` (at open, off the UI thread). A damaged one is
/// kept aside (`.corrupt-<time>`) and read as empty; the notice says so.
pub(crate) fn load(dir: &Path, now: Timestamp) -> (Damage, Option<String>) {
    let path = dir.join(FILE);
    let bytes = match dalica_fs::read(&path, CAP) {
        Ok(Some(read)) => read.bytes,
        Ok(None) => return (Damage::default(), None),
        Err(dalica_fs::Error::TooLarge { .. }) => Vec::new(),
        Err(e) => return (Damage::default(), Some(format!("{FILE}: {e}"))),
    };
    match serde_json::from_slice::<ListFile>(&bytes) {
        Ok(list) if list.version == 1 && list.folders.len() <= MAX_FOLDERS => {
            let mut damage = Damage::default();
            for entry in list.folders {
                let kept = damage.folders.entry(entry.path).or_default();
                for k in entry.kept.into_iter().take(MAX_KEPT) {
                    if !kept.contains(&k) {
                        kept.push(k);
                    }
                }
            }
            (damage, None)
        }
        _ => {
            let kept = dalica_fs::set_aside(&path, &corrupt_tag(now))
                .map(|k| format!("kept as {}", k.display()))
                .unwrap_or_else(|e| format!("could not be kept aside: {e}"));
            (
                Damage::default(),
                Some(format!("{FILE} could not be read ({kept}); folders with a damaged data file are found again when Dalica next writes there")),
            )
        }
    }
}

/// Writes the list to the library directory `dir` (on the I/O worker), replacing the file it
/// finds there.
pub(crate) fn save(dir: &Path, bytes: &[u8]) -> Result<(), String> {
    let path = dir.join(FILE);
    let expect = dalica_fs::identity(&path).map_err(|e| e.to_string())?.map_or(Expect::Absent, Expect::Unchanged);
    dalica_fs::replace(&path, bytes, expect).map(|_| ()).map_err(|e| format!("{FILE}: {e}"))
}

/// The bytes of the list as it stands.
pub(crate) fn list_bytes(damage: &Damage) -> Vec<u8> {
    damage.json()
}

/// What reading a damaged folder's sidecar again found (on the I/O worker).
pub(crate) enum Found {
    /// A readable sidecar: the user restored one.
    Restored(Box<Sidecar>),
    /// No sidecar.
    Absent,
    /// An unreadable one, kept aside under this name.
    SetAside(PathBuf),
    /// The folder is not there, or the file could not be read or kept aside: nothing changed.
    Failed(String),
}

/// Reads `folder`'s sidecar again: a readable one is returned, an unreadable one kept aside as
/// `.dalica.json.corrupt-<time>` under a name no other copy has (on the I/O worker).
pub(crate) fn look(folder: &Path, now: Timestamp) -> Found {
    match dalica_fs::identity(folder) {
        Ok(Some(_)) => {}
        Ok(None) => return Found::Failed(format!("{} is not there (is its volume offline?)", folder.display())),
        Err(e) => return Found::Failed(e.to_string()),
    }
    let path = folder.join(FILE_NAME);
    let limits = Limits::default();
    let aside = |path: &Path| match dalica_fs::set_aside(path, &corrupt_tag(now)) {
        Ok(kept) => Found::SetAside(kept),
        Err(e) => Found::Failed(e.to_string()),
    };
    match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => Found::Absent,
        Ok(Some(read)) => match Sidecar::parse(&read.bytes, &limits) {
            Ok(s) => Found::Restored(Box::new(s)),
            Err(_) => aside(&path),
        },
        Err(dalica_fs::Error::TooLarge { .. }) => aside(&path),
        Err(e) => Found::Failed(e.to_string()),
    }
}

/// What the I/O worker did for a reset.
pub(crate) enum Reset {
    /// The folder's sidecar reads now: its values, and the copies kept aside.
    Restored { sidecar: Box<Sidecar>, kept: Vec<PathBuf> },
    /// A new sidecar was written from the model (`written`: false when the write is still to land,
    /// with why), and the copies kept aside.
    Rewritten { kept: Vec<PathBuf>, written: Result<(), String> },
}

/// What the I/O worker answers a reset with.
pub(crate) type ResetReply = mpsc::Sender<Result<Reset, String>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    folder: FolderId,
    #[serde(default)]
    confirm: bool,
}

fn reset(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Params { folder, confirm } = params(p)?;
    let path = s.library.folder(folder).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {folder}")))?;
    s.sync_damage();
    if !s.damaged.contains_key(&path) {
        return Err(CmdError::Disabled(format!("The data file of {} is not damaged: there is nothing to reset.", path.display())));
    }
    if !confirm {
        return Err(CmdError::Disabled(format!("{RESET_TITLE}: {RESET_QUESTION} Send \"confirm\": true to go ahead.")));
    }
    let state = mapping::folder_state(&s.library, folder).ok_or_else(|| CmdError::NotFound(format!("no folder {folder}")))?;
    let answer = s.reset_on_worker(folder, state)?;
    let (kept, restored, written) = match answer {
        Reset::Restored { sidecar, kept } => {
            for op in restored_ops(&s.library, folder, &sidecar) {
                s.apply_from_files(op)?;
            }
            (kept, true, false)
        }
        Reset::Rewritten { kept, written } => {
            if let Err(e) = &written {
                s.notices.push(format!("{}: the new data file is not written yet: {e}", path.display()));
            }
            (kept, false, written.is_ok())
        }
    };
    s.sync_damage();
    let kept: Vec<String> = kept.iter().map(|k| k.display().to_string()).collect();
    Ok(json!({ "folder": folder, "kept": kept, "restored": restored, "written": written }))
}

/// The ops that give the model what `sidecar` says about `folder` and its photos, as a rebuild
/// reads it (`dalica-store`'s `rebuild`): the photos' stars, info, copies saved beside, album
/// memberships of the albums the library has (others stay in the file) and last-changed times,
/// and the folder's info, project mark and photo sort. Empty when the model already says it.
fn restored_ops(library: &Library, folder: FolderId, sidecar: &Sidecar) -> Vec<Op> {
    let mut ops = Vec::new();
    let reading = mapping::read_folder(sidecar);
    let own_project = library.project(folder);
    let project = reading.project.clone().or(own_project);
    let own_sort = library.folder_sort(folder);
    let sort = reading.photo_sort(own_sort);
    let own_info = library.folder_info(folder).clone();
    let info = reading.merged(&own_info);
    let (mut stars, mut infos, mut saved_as, mut changed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut add, mut remove, mut rekey) = (Vec::new(), Vec::new(), Vec::new());
    for photo in library.photos_in(folder) {
        let own = mapping::photo_state(library, photo);
        let mut read = mapping::read_photo(sidecar, &photo.name).merged(&own);
        read.albums.retain(|a, _| library.album(a).is_some());
        if read.star != own.star {
            stars.push((photo.id, read.star));
        }
        if read.info != own.info {
            infos.push((photo.id, read.info));
        }
        if read.saved_as != own.saved_as {
            saved_as.push((photo.id, read.saved_as));
        }
        if read.changed != own.changed {
            changed.push((photo.id, read.changed));
        }
        members(photo.id, &own.albums, &read.albums, &mut add, &mut remove, &mut rekey);
    }
    if !stars.is_empty() {
        ops.push(Op::SetStars { stars });
    }
    if !infos.is_empty() {
        ops.push(Op::SetPhotoInfo { photos: infos });
    }
    if info != own_info {
        ops.push(Op::SetFolderInfo { folders: vec![(folder, info)] });
    }
    if !saved_as.is_empty() {
        ops.push(Op::SetSavedAs { saved_as });
    }
    if project != own_project {
        ops.push(Op::SetProjects { projects: vec![(folder, project)] });
    }
    if sort != own_sort {
        ops.push(Op::SetFolderSorts { sorts: vec![(folder, sort)] });
    }
    if !remove.is_empty() {
        ops.push(Op::RemoveMembers { members: remove });
    }
    if !rekey.is_empty() {
        ops.push(Op::SetMemberKeys { keys: rekey });
    }
    if !add.is_empty() {
        ops.push(Op::AddMembers { members: add });
    }
    if !changed.is_empty() {
        ops.push(Op::SetLastChanged { changed });
    }
    ops
}

type Members = BTreeMap<dalica_core::AlbumId, dalica_core::PosKey>;

/// The membership changes that take `photo` from `own` to `read`.
fn members(
    photo: PhotoId,
    own: &Members,
    read: &Members,
    add: &mut Vec<(dalica_core::AlbumId, PhotoId, dalica_core::PosKey)>,
    remove: &mut Vec<(dalica_core::AlbumId, PhotoId)>,
    rekey: &mut Vec<(dalica_core::AlbumId, PhotoId, dalica_core::PosKey)>,
) {
    for album in own.keys().filter(|a| !read.contains_key(*a)) {
        remove.push((album.clone(), photo));
    }
    for (album, key) in read {
        match own.get(album) {
            None => add.push((album.clone(), photo, key.clone())),
            Some(k) if k != key => rekey.push((album.clone(), photo, key.clone())),
            Some(_) => {}
        }
    }
}

impl Session {
    /// The folders whose sidecar was damaged and kept aside, in path order (see the module's
    /// notes): nothing is written into them until the user resets them or restores the file. A
    /// listed path the library has no folder for is left out.
    pub fn damaged_folders(&self) -> Vec<FolderId> {
        self.damaged.keys().filter_map(|p| self.library.folder_by_path(p)).collect()
    }

    /// The copies of `folder`'s damaged sidecar kept aside (possibly none, when keeping one aside
    /// failed), or `None` when its sidecar is not damaged. In memory: cheap to ask every frame.
    pub fn damaged_folder(&self, folder: FolderId) -> Option<&[PathBuf]> {
        let path = &self.library.folder(folder)?.path;
        self.damaged.get(path).map(Vec::as_slice)
    }

    /// Copies the I/O worker's list of damaged folders when it changed (a lock, no disk), and tells
    /// the metadata write-back (M10.57): the photo writes held for a folder taken off the list are
    /// sent then.
    pub(crate) fn sync_damage(&mut self) {
        let lifted: Vec<PathBuf> = {
            let shared = self.damage.lock().unwrap_or_else(PoisonError::into_inner);
            if shared.revision() == self.damage_seen {
                return;
            }
            self.damage_seen = shared.revision();
            let now = shared.snapshot();
            let lifted = self.damaged.keys().filter(|p| !now.contains_key(*p)).cloned().collect();
            self.damaged = now;
            lifted
        };
        self.metadata_damage_changed(&lifted);
    }

    /// Has the I/O worker reset `folder` (with `state`, the model's word for it) and waits for
    /// its answer.
    fn reset_on_worker(&mut self, folder: FolderId, state: FolderState) -> Result<Reset, CmdError> {
        let (reply, answer) = mpsc::channel();
        self.send(Msg::Reset { folder, state: Box::new(state), reply });
        match answer.recv() {
            Ok(Ok(reset)) => Ok(reset),
            Ok(Err(e)) => Err(CmdError::Disabled(format!("Reset Folder Data: {e}"))),
            Err(_) => Err(CmdError::Internal("the I/O worker did not answer".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list round-trips through its file; a damaged file is kept aside and read as empty.
    #[test]
    fn the_list_round_trips_and_a_damaged_one_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Damage::default();
        assert!(d.note(Path::new("/p/Lisbon"), Some(Path::new("/p/Lisbon/.dalica.json.corrupt-1"))));
        assert!(!d.note(Path::new("/p/Lisbon"), Some(Path::new("/p/Lisbon/.dalica.json.corrupt-1"))), "already listed");
        assert!(d.note(Path::new("/p/Porto"), None));
        save(dir.path(), &list_bytes(&d)).unwrap();
        let (read, note) = load(dir.path(), Timestamp::EPOCH);
        assert_eq!((read.snapshot(), note), (d.snapshot(), None));
        std::fs::write(dir.path().join(FILE), b"{broken").unwrap();
        let (read, note) = load(dir.path(), Timestamp::EPOCH);
        assert!(read.snapshot().is_empty() && note.is_some_and(|n| n.contains("kept as")));
        assert!(dir.path().join(format!("{FILE}.corrupt-19700101T000000Z")).exists());
    }
}
