//! M8's data as a scan finds it (`data-model.md` §6, §7): a folder's originals record
//! (`.dalica-originals/.dalica.json`) read on the scan's worker and turned into the saved records
//! of M8.1 ([`read`], [`ops`]), and the `savedAs` and project mark a folder sidecar holds, as a
//! photo or folder new to the model takes them ([`saved_as`], [`project`]).
//!
//! The walk still skips `.dalica-originals` (the kept originals are never photos of the library);
//! only the record inside it is read, through `dalica-fs`, never through a link. A record entry is
//! used only when its name is one plain path component (`security.md` §5.4) naming a photo of the
//! folder and its values are within the library's caps; everything else stays in the file. A
//! damaged record is set aside as `.dalica.json.corrupt-<time>` and reported, so a later Save
//! starts a new one without writing over it. The ops are [`Op::SetSaved`], derived data: no undo
//! step and no revision bump, so loading records moves no cache.

use std::path::{Path, PathBuf};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::{Library, MAX_DIGEST, MAX_FILE_NAME, MAX_OPERATION, MAX_OPERATIONS, Op, Project, SaveState, Saved, SavedAs};
use dalica_sidecar::originals::{self, Entry, Originals as Record, State};
use dalica_sidecar::{Field, Limits, Sidecar, plain_name};

/// What a folder's originals record gave the scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Originals {
    /// No `.dalica-originals` folder, or no record in it: no photo of the folder is saved.
    Absent,
    /// The record, read: (file name, record) for every usable entry in name order, and one
    /// sentence per entry or value not used (it stays in the file).
    Read { saved: Vec<(String, Saved)>, notes: Vec<String> },
    /// The record could not be read (a link in place of `.dalica-originals` or of the record, a
    /// file too large, an I/O error): the model keeps what it has.
    Unreadable(String),
    /// The record was damaged: kept aside as `.dalica.json.corrupt-<time>` beside it (`None` when
    /// even that failed), and the model keeps what it has.
    SetAside { why: String, kept: Option<PathBuf> },
}

impl Originals {
    /// The names whose Save is still pending: a Save interrupted before it was marked done, for
    /// the repair to look at (`data-model.md` §6).
    pub fn pending(&self) -> Vec<&str> {
        match self {
            Originals::Read { saved, .. } => saved.iter().filter(|(_, s)| s.state == SaveState::Pending).map(|(n, _)| n.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

/// The originals record's path in `folder`.
pub fn record_path(folder: &Path) -> PathBuf {
    folder.join(originals::DIR_NAME).join(originals::FILE_NAME)
}

/// Reads `folder`'s originals record. Call it on the scan's worker, never on the UI thread.
pub fn read(folder: &Path, now: Timestamp) -> Originals {
    let dir = folder.join(originals::DIR_NAME);
    match dalica_fs::stat(&dir) {
        Ok(None) => return Originals::Absent,
        Ok(Some(s)) if s.is_file => return Originals::Unreadable(format!("{} is a file, not a folder", dir.display())),
        Ok(Some(_)) => {}
        Err(e) => return Originals::Unreadable(e.to_string()),
    }
    let path = record_path(folder);
    let limits = Limits::default();
    let bytes = match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => return Originals::Absent,
        Ok(Some(read)) => read.bytes,
        Err(e) => return Originals::Unreadable(e.to_string()),
    };
    let record = match Record::parse(&bytes, &limits) {
        Ok(r) => r,
        Err(e) => {
            let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            return Originals::SetAside { why: e.to_string(), kept: dalica_fs::set_aside(&path, &format!("corrupt-{stamp}Z")).ok() };
        }
    };
    let mut notes: Vec<String> =
        record.problems().into_iter().map(|p| format!("{}{} should be {}", originals::DIR_NAME, p.pointer, p.expected)).collect();
    let mut saved = Vec::new();
    for (name, entry) in record.entries() {
        match usable(entry) {
            Ok(s) => saved.push((name, s)),
            Err(why) => notes.push(format!("{name}: {why}: kept in the file, not used")),
        }
    }
    Originals::Read { saved, notes }
}

/// The model's record for an entry within the library's caps.
fn usable(entry: Entry) -> Result<Saved, String> {
    if entry.digest.len() > MAX_DIGEST {
        return Err(format!("the digest is longer than {MAX_DIGEST} bytes"));
    }
    if entry.recipe.len() > MAX_OPERATIONS || entry.recipe.iter().any(|op| op.len() > MAX_OPERATION) {
        return Err(format!("the saved recipe has more than {MAX_OPERATIONS} operations or one longer than {MAX_OPERATION} bytes"));
    }
    Ok(Saved {
        recipe: entry.recipe,
        size: entry.size,
        mtime: Timestamp::from_millis(entry.mtime),
        width: entry.width,
        height: entry.height,
        digest: entry.digest,
        state: match entry.state {
            State::Pending => SaveState::Pending,
            State::Done => SaveState::Done,
        },
        time: entry.time.map(Timestamp::from_millis),
    })
}

/// The op that makes the saved records of `folder`'s photos (as the model holds them now) match
/// what the scan read: a photo named in the record takes its entry, a photo not named loses its
/// record when the record was read or is absent. `None` when nothing changes, or when the record
/// could not be read (never remove what was not seen).
pub fn ops(library: &Library, folder: FolderId, found: &Originals) -> Option<Op> {
    let ids: Vec<(&str, PhotoId)> = library.photos_in(folder).into_iter().map(|p| (p.name.as_str(), p.id)).collect();
    let changes = changes(library, &ids, found);
    (!changes.is_empty()).then_some(Op::SetSaved { saved: changes })
}

/// The saved-record changes for the photos `ids` (file name, id; a photo the model does not hold
/// yet has no record): what [`ops`] puts in its op, and what the scan's diff gives for each folder
/// it read. Empty when the record could not be read.
pub fn changes(library: &Library, ids: &[(&str, PhotoId)], found: &Originals) -> Vec<(PhotoId, Option<Saved>)> {
    let saved: &[(String, Saved)] = match found {
        Originals::Read { saved, .. } => saved,
        Originals::Absent => &[],
        Originals::Unreadable(_) | Originals::SetAside { .. } => return Vec::new(),
    };
    let by_name: std::collections::HashMap<&str, &Saved> = saved.iter().map(|(n, s)| (n.as_str(), s)).collect();
    let mut out = Vec::new();
    for &(name, id) in ids {
        let read = by_name.get(name).copied();
        if read != library.saved(id) {
            out.push((id, read.cloned()));
        }
    }
    out
}

/// The copy saved beside photo `name` that `sidecar` names, as a photo new to the model takes it
/// (the rules of `dalica-store`'s `mapping::read_photo`): `None` when there is none, or when it is
/// not usable (a value of the wrong type, a name that is not one plain path component, a recipe
/// past the caps; kept in the file).
pub fn saved_as(sidecar: &Sidecar, name: &str) -> Option<SavedAs> {
    match sidecar.saved_as(name) {
        Field::Value(v)
            if plain_name(&v.name)
                && v.name.len() <= MAX_FILE_NAME
                && v.recipe.len() <= MAX_OPERATIONS
                && v.recipe.iter().all(|op| op.len() <= MAX_OPERATION) =>
        {
            Some(SavedAs { name: v.name, recipe: v.recipe })
        }
        _ => None,
    }
}

/// The project mark `sidecar` gives its folder: `"exported"`, or `None` (absent, unknown or of the
/// wrong type; kept in the file).
pub fn project(sidecar: &Sidecar) -> Option<Project> {
    match sidecar.project() {
        Field::Value(s) if s == "exported" => Some(Project::Exported),
        _ => None,
    }
}
