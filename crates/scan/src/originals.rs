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
//!
//! The same read also looks at the folder's **legacy originals folder** (M14.14, `data-model.md`
//! §9; its name is `dalica_sidecar::legacy::originals_dir_name`): the originals the previous
//! organizer kept for the photos it saved, and its sidecar there holding their recipes. Each
//! section of that sidecar naming a kept original beside a photo of the folder gives the photo a
//! [`LegacyOriginal`]: the original's size and `md5:` digest, and the section's edits mapped to
//! Dalica's operations (`dalica_edit::legacy`, unknown ones kept as `legacy:` entries). An original
//! whose bytes equal the photo's is not given (the photo already is its original). Nothing in
//! the legacy folder is ever written, renamed or set aside (legacy files are read-only), nothing
//! is reached through a link, and a sidecar that cannot be read leaves the model's legacy facts as
//! they are. The facts go into [`Op::SetLegacyOriginals`] ([`legacy_changes`], [`legacy_ops`]),
//! derived like the saved records.

use std::path::{Path, PathBuf};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_edit::Rect;
use dalica_edit::legacy::{self as edits, Legacy as LegacyEdits};
use dalica_library::{LegacyOriginal, Library, MAX_DIGEST, MAX_FILE_NAME, MAX_OPERATION, MAX_OPERATIONS, Op, Project, SaveState, Saved, SavedAs};
use dalica_sidecar::legacy::{LegacyFile, LegacyLimits, LegacySidecar, originals_dir_name, sidecar_names};
use dalica_sidecar::originals::{self, Entry, Originals as Record, State};
use dalica_sidecar::{Field, Limits, Sidecar, plain_name};

/// The largest kept original or photo read whole for its digest (the thumbnailer's and Save's
/// cap, 1 GiB); a larger one is not offered.
pub const MAX_LEGACY_FILE: u64 = 1 << 30;

/// What a folder's originals record gave the scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Originals {
    /// No `.dalica-originals` folder, or no record in it: no photo of the folder is saved.
    Absent,
    /// The record, read: (file name, record) for every usable entry in name order, and one
    /// sentence per entry or value not used (it stays in the file). With no record but a legacy
    /// originals folder, `saved` is empty (as [`Originals::Absent`] says) and `legacy` holds what
    /// that folder gave; the notes then name what it could not give.
    Read { saved: Vec<(String, Saved)>, notes: Vec<String>, legacy: Legacy },
    /// The record could not be read (a link in place of `.dalica-originals` or of the record, a
    /// file too large, an I/O error): the model keeps what it has.
    Unreadable(String),
    /// The record was damaged: kept aside as `.dalica.json.corrupt-<time>` beside it (`None` when
    /// even that failed), and the model keeps what it has.
    SetAside { why: String, kept: Option<PathBuf> },
}

/// What a folder's legacy originals folder gave the scan (`data-model.md` §9).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Legacy {
    /// No legacy originals folder (or the option to read legacy data is off): no photo of the
    /// folder has a legacy original.
    #[default]
    Absent,
    /// The originals offered, (file name, fact) in name order.
    Read(Vec<(String, LegacyOriginal)>),
    /// The folder or its sidecar could not be read (a link, a file in its place, a sidecar past
    /// the reader's limits, an I/O error): the model keeps what it has.
    Unreadable,
}

impl Originals {
    /// What the legacy originals folder gave ([`Legacy::Absent`] without one; when the record
    /// itself could not be read, [`Legacy::Unreadable`]: nothing is changed).
    pub fn legacy(&self) -> &Legacy {
        match self {
            Originals::Absent => &Legacy::Absent,
            Originals::Read { legacy, .. } => legacy,
            Originals::Unreadable(_) | Originals::SetAside { .. } => &Legacy::Unreadable,
        }
    }

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

/// Reads `folder`'s originals record and its legacy originals folder. Call it on the scan's
/// worker, never on the UI thread.
pub fn read(folder: &Path, now: Timestamp) -> Originals {
    read_with(folder, now, true)
}

/// [`read`], reading the legacy originals folder only when `legacy` (the option to read the
/// previous organizer's data, `files.readLegacyData`).
pub fn read_with(folder: &Path, now: Timestamp, legacy: bool) -> Originals {
    let mut notes = Vec::new();
    let found = if legacy { read_legacy(folder, &mut notes) } else { Legacy::Absent };
    match read_record(folder, now) {
        Originals::Absent if found == Legacy::Absent && notes.is_empty() => Originals::Absent,
        Originals::Absent => Originals::Read { saved: Vec::new(), notes, legacy: found },
        Originals::Read { saved, notes: mut own, .. } => {
            own.append(&mut notes);
            Originals::Read { saved, notes: own, legacy: found }
        }
        other => other,
    }
}

/// Dalica's own originals record (M8.9).
fn read_record(folder: &Path, now: Timestamp) -> Originals {
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
    Originals::Read { saved, notes, legacy: Legacy::Absent }
}

/// The legacy originals folder of `folder`: the originals its sidecar names that are offered.
/// Sentences for what it could not give go to `notes`.
fn read_legacy(folder: &Path, notes: &mut Vec<String>) -> Legacy {
    let name = originals_dir_name();
    let dir = folder.join(&name);
    match dalica_fs::stat(&dir) {
        Ok(None) => return Legacy::Absent,
        Ok(Some(s)) if s.is_file => {
            notes.push(format!("{name} is a file, not a folder: not read"));
            return Legacy::Unreadable;
        }
        Ok(Some(_)) => {}
        Err(e) => {
            notes.push(format!("{name}: {e}: not read"));
            return Legacy::Unreadable;
        }
    }
    let limits = LegacyLimits::default();
    let mut bytes = None;
    for file in sidecar_names() {
        match dalica_fs::read(&dir.join(&file), limits.max_bytes as u64) {
            Ok(None) => {}
            Ok(Some(read)) => {
                bytes = Some(read.bytes);
                break;
            }
            Err(e) => {
                notes.push(format!("{name}: {e}: not read"));
                return Legacy::Unreadable;
            }
        }
    }
    // Without its sidecar no kept file is known to be an original of a saved photo.
    let Some(bytes) = bytes else { return Legacy::Read(Vec::new()) };
    let sidecar = match LegacySidecar::parse(&bytes, &limits) {
        Ok(s) => s,
        Err(e) => {
            notes.push(format!("{name}: {e}: not read"));
            return Legacy::Unreadable;
        }
    };
    let mut out: Vec<(String, LegacyOriginal)> = Vec::new();
    for file in sidecar.file_names() {
        let Some(data) = sidecar.file(file) else { continue };
        match offered(folder, &dir, file, &data) {
            Ok(Some(original)) => out.push((file.to_string(), original)),
            Ok(None) => {}
            Err(why) => notes.push(format!("{name}/{file}: {why}: not offered")),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    Legacy::Read(out)
}

/// The legacy original `name` kept in `dir` for the photo of that name in `folder`: `None` when
/// either file is missing or their bytes are the same; `Err` with the reason when it cannot be
/// used (a link, too large, a recipe past the caps).
fn offered(folder: &Path, dir: &Path, name: &str, data: &LegacyFile) -> Result<Option<LegacyOriginal>, String> {
    if !plain_name(name) || name.len() > MAX_FILE_NAME {
        return Err("not a plain file name".into());
    }
    let photo = match dalica_fs::stat(&folder.join(name)).map_err(|e| e.to_string())? {
        Some(s) if s.is_file => s,
        _ => return Ok(None),
    };
    let Some(kept) = dalica_fs::read(&dir.join(name), MAX_LEGACY_FILE).map_err(|e| e.to_string())? else { return Ok(None) };
    let size = kept.bytes.len() as u64;
    let digest = md5_digest(&kept.bytes);
    if photo.identity.size() == size {
        let now = dalica_fs::read(&folder.join(name), MAX_LEGACY_FILE).map_err(|e| e.to_string())?;
        if now.is_some_and(|now| md5_digest(&now.bytes) == digest) {
            return Ok(None);
        }
    }
    let orientation = dalica_image::read_header(&kept.bytes, &dalica_image::Limits::default()).map(|h| h.orientation).unwrap_or_default();
    let recipe = recipe(data, orientation)?;
    Ok(Some(LegacyOriginal { size, digest, recipe }))
}

/// The recipe a section of the legacy originals folder's sidecar holds (the edits the saved file
/// holds), as Dalica's operations.
fn recipe(data: &LegacyFile, orientation: dalica_image::Orientation) -> Result<Vec<String>, String> {
    let legacy = LegacyEdits {
        filters: data.edits.clone(),
        redo: Vec::new(),
        quarters: data.rotation.unwrap_or(0),
        flipped: data.flip.is_some_and(|f| f != 0),
        crop: data.crop.map(|[x0, y0, x1, y1]| Rect { x0, y0, x1, y1 }),
        text: data.text.as_ref().map(|t| t.raw.text.clone()),
        text_active: data.text_active.map(|on| if on { "1" } else { "0" }.to_string()),
        orientation,
    };
    let recipe = edits::convert(&legacy).map_err(|e| e.to_string())?.edits_text();
    if recipe.len() > MAX_OPERATIONS || recipe.iter().any(|op| op.len() > MAX_OPERATION) {
        return Err(format!("the saved recipe has more than {MAX_OPERATIONS} operations or one longer than {MAX_OPERATION} bytes"));
    }
    Ok(recipe)
}

/// `md5:` and 32 lowercase hex digits, as Save's records and Find Duplicates make them.
fn md5_digest(bytes: &[u8]) -> String {
    format!("md5:{:x}", md5::compute(bytes))
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

/// The op that makes the legacy originals of `folder`'s photos match what the scan read
/// ([`legacy_changes`]); `None` when nothing changes.
pub fn legacy_ops(library: &Library, folder: FolderId, found: &Originals) -> Option<Op> {
    let ids: Vec<(&str, PhotoId)> = library.photos_in(folder).into_iter().map(|p| (p.name.as_str(), p.id)).collect();
    let changes = legacy_changes(library, &ids, found);
    (!changes.is_empty()).then_some(Op::SetLegacyOriginals { originals: changes })
}

/// The legacy-original changes for the photos `ids` (file name, id): a photo the legacy folder
/// offers an original for takes it, any other loses its own when the folder was read or is
/// absent. Empty when it could not be read (never remove what was not seen).
pub fn legacy_changes(library: &Library, ids: &[(&str, PhotoId)], found: &Originals) -> Vec<(PhotoId, Option<LegacyOriginal>)> {
    let offered: &[(String, LegacyOriginal)] = match found.legacy() {
        Legacy::Read(offered) => offered,
        Legacy::Absent => &[],
        Legacy::Unreadable => return Vec::new(),
    };
    let by_name: std::collections::HashMap<&str, &LegacyOriginal> = offered.iter().map(|(n, o)| (n.as_str(), o)).collect();
    let mut out = Vec::new();
    for &(name, id) in ids {
        let read = by_name.get(name).copied();
        if read != library.legacy_original(id) {
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

/// The project mark `sidecar` gives its folder: `"exported"` or `"collages"`, or `None` (absent, unknown or of the
/// wrong type; kept in the file).
pub fn project(sidecar: &Sidecar) -> Option<Project> {
    match sidecar.project() {
        Field::Value(s) if s == "exported" => Some(Project::Exported),
        Field::Value(s) if s == "collages" => Some(Project::Collages),
        _ => None,
    }
}
