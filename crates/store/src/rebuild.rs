//! Rebuilding user data from the folders (`data-model.md` §10.1). The index is a cache: after it is
//! lost, a scan rebuilds the folders and photos (crate `scan`), and [`import_sidecars`] reads each
//! folder's `.dalica.json` to give back what the user set: per photo its star, caption, tags,
//! hidden flag, recipe, redo list, date and the copy a Save wrote beside it (`savedAs`), per folder
//! its description, date, place, hidden flag and project mark; and each folder's originals record
//! (`.dalica-originals/.dalica.json`, [`read_originals`]) to give back the photos' saved records.
//! Nothing a sidecar or a record holds is lost by deleting the index. The synced metadata hashes
//! are index-only (`mapping::SYNCED_FIELDS`) and start empty after a rebuild.

use std::path::{Path, PathBuf};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::{Library, Op, Project, Saved, SavedAs};
use dalica_sidecar::originals::{self, Originals};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::mapping::{self, OriginalsReading};
use crate::writer::{self, Outcome};

/// What the folders' sidecars say, as ops on the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// Ops that make the model's user data match the sidecars and the originals records (empty if
    /// it already does): at most one `SetStars`, one `SetPhotoInfo`, one `SetFolderInfo`, one
    /// `SetSavedAs`, one `SetProjects` and one `SetSaved` (derived: no undo step), in that order.
    pub ops: Vec<Op>,
    /// Folders whose sidecar was damaged: kept aside, folder read-only.
    pub read_only: Vec<FolderId>,
    /// Folders whose sidecar or originals record could not be read, held values of the wrong type
    /// or values past the caps, and why (once per value).
    pub problems: Vec<(FolderId, String)>,
}

/// Reads the sidecar and the originals record of every folder in `library` and returns the ops
/// that apply their user data. A value of the wrong type leaves the model's own value as it is; a
/// folder without a sidecar keeps the model's word, and one whose originals record cannot be read
/// keeps its saved records. Any thread but the UI thread may call it.
pub fn import_sidecars(library: &Library, now: Timestamp) -> Import {
    let limits = Limits::default();
    let mut out = Import::default();
    let mut stars = Vec::new();
    let mut infos = Vec::new();
    let mut folders = Vec::new();
    let mut saved_as: Vec<(PhotoId, Option<SavedAs>)> = Vec::new();
    let mut projects: Vec<(FolderId, Option<Project>)> = Vec::new();
    let mut saved: Vec<(PhotoId, Option<Saved>)> = Vec::new();
    for folder in library.folders() {
        match read_originals(&folder.path, now) {
            OriginalsFile::Read(reading) => {
                out.problems.extend(reading.notes.iter().map(|n| (folder.id, n.clone())));
                saved.extend(saved_ops(library, folder.id, &reading));
            }
            OriginalsFile::Absent => saved.extend(saved_ops(library, folder.id, &OriginalsReading::default())),
            OriginalsFile::Unreadable(why) => out.problems.push((folder.id, why)),
            OriginalsFile::SetAside { why, kept } => {
                let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "could not be kept aside".into());
                out.problems.push((folder.id, format!("{}: {why}; {kept}", originals_path(&folder.path).display())));
            }
        }
        let path = folder.path.join(FILE_NAME);
        let sidecar = match dalica_fs::read(&path, limits.max_bytes as u64) {
            Ok(None) => continue,
            Ok(Some(read)) => match Sidecar::parse(&read.bytes, &limits) {
                Ok(s) => s,
                Err(e) => {
                    out.problems.push((folder.id, e.to_string()));
                    if let Outcome::SetAside(_) = writer::set_aside(&path, now) {
                        out.read_only.push(folder.id);
                    }
                    continue;
                }
            },
            Err(e) => {
                out.problems.push((folder.id, e.to_string()));
                continue;
            }
        };
        for p in sidecar.problems() {
            out.problems.push((folder.id, format!("{} should be {}", p.pointer, p.expected)));
        }
        let reading = mapping::read_folder(&sidecar);
        out.problems.extend(reading.notes.iter().map(|n| (folder.id, n.clone())));
        let own_project = library.project(folder.id);
        let project = reading.project.clone().or(own_project);
        if project != own_project {
            projects.push((folder.id, project));
        }
        let own = library.folder_info(folder.id);
        let info = reading.merged(own);
        if info != *own {
            folders.push((folder.id, info));
        }
        for photo in library.photos_in(folder.id) {
            let reading = mapping::read_photo(&sidecar, &photo.name);
            out.problems.extend(reading.notes.iter().map(|n| (folder.id, format!("{}: {n}", photo.name))));
            let own = mapping::photo_state(library, photo);
            let read = reading.merged(&own);
            if read.star != own.star {
                stars.push((photo.id, read.star));
            }
            if read.info != own.info {
                infos.push((photo.id, read.info));
            }
            if read.saved_as != own.saved_as {
                saved_as.push((photo.id, read.saved_as));
            }
        }
    }
    if !stars.is_empty() {
        out.ops.push(Op::SetStars { stars });
    }
    if !infos.is_empty() {
        out.ops.push(Op::SetPhotoInfo { photos: infos });
    }
    if !folders.is_empty() {
        out.ops.push(Op::SetFolderInfo { folders });
    }
    if !saved_as.is_empty() {
        out.ops.push(Op::SetSavedAs { saved_as });
    }
    if !projects.is_empty() {
        out.ops.push(Op::SetProjects { projects });
    }
    if !saved.is_empty() {
        out.ops.push(Op::SetSaved { saved });
    }
    out
}

/// The changes that make the saved records of `folder`'s photos match `reading`: a photo named in
/// it takes its record, a photo not named in it loses one. Records for names that are not photos
/// of the folder stay in the file and change nothing.
fn saved_ops(library: &Library, folder: FolderId, reading: &OriginalsReading) -> Vec<(PhotoId, Option<Saved>)> {
    let by_name: std::collections::HashMap<&str, &Saved> = reading.saved.iter().map(|(n, s)| (n.as_str(), s)).collect();
    let mut out = Vec::new();
    for photo in library.photos_in(folder) {
        let read = by_name.get(photo.name.as_str()).copied();
        if read != library.saved(photo.id) {
            out.push((photo.id, read.cloned()));
        }
    }
    out
}

/// What a folder's originals record gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalsFile {
    /// No `.dalica-originals` folder, or no record in it: no photo of the folder is saved.
    Absent,
    /// The record, read.
    Read(OriginalsReading),
    /// The record could not be read (a link in place of `.dalica-originals` or of the record, a
    /// file too large, an I/O error): the model keeps what it has.
    Unreadable(String),
    /// The record was damaged (not JSON, not an object, over the limits): kept aside as
    /// `.dalica.json.corrupt-<time>` beside it (`None` when even that failed), and the model keeps
    /// what it has.
    SetAside { why: String, kept: Option<PathBuf> },
}

/// The originals record's path in `folder`.
pub fn originals_path(folder: &Path) -> PathBuf {
    folder.join(originals::DIR_NAME).join(originals::FILE_NAME)
}

/// Reads `folder`'s originals record (`data-model.md` §6) through `dalica-fs`, never through a
/// link; a damaged one is set aside as `.dalica.json.corrupt-<time>` and reported, so a later Save
/// starts a new record without writing over it.
pub fn read_originals(folder: &Path, now: Timestamp) -> OriginalsFile {
    let dir = folder.join(originals::DIR_NAME);
    match dalica_fs::stat(&dir) {
        Ok(None) => return OriginalsFile::Absent,
        Ok(Some(s)) if s.is_file => return OriginalsFile::Unreadable(format!("{} is a file, not a folder", dir.display())),
        Ok(Some(_)) => {}
        Err(e) => return OriginalsFile::Unreadable(e.to_string()),
    }
    let path = originals_path(folder);
    let limits = Limits::default();
    let bytes = match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => return OriginalsFile::Absent,
        Ok(Some(read)) => read.bytes,
        Err(e) => return OriginalsFile::Unreadable(e.to_string()),
    };
    match Originals::parse(&bytes, &limits) {
        Ok(record) => OriginalsFile::Read(mapping::read_originals(&record)),
        Err(e) => {
            let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            OriginalsFile::SetAside { why: e.to_string(), kept: dalica_fs::set_aside(&path, &format!("corrupt-{stamp}Z")).ok() }
        }
    }
}
