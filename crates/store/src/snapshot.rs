//! The index snapshot, `index.snap`: the whole model at one op number, as a single checksummed
//! record (`<crc32> <json>\n`), written atomically. Loading it applies one batch op to an empty
//! library, so a snapshot can only hold what ops can build.
//!
//! Sketches (derived data, `data-model.md` §4) are a `sketches` list of `[photo id, base64]` pairs,
//! left out when there are none. A snapshot written before sketches existed reads as one without
//! them, and a reader that does not know the key ignores it, so the version stays 1.
//!
//! M6's fields follow the same rule: `photoInfo` (`[photo id, info]` pairs: caption, tags, hidden,
//! recipe, redo list, date), `folderInfo` (`[folder id, info]` pairs: description, date, place,
//! hidden) and `synced` (`[photo id, hashes]` pairs, derived data that only the index keeps), each
//! left out when empty. Loading puts them back with the same ops the journal records.
//!
//! M8's fields too: `saved` (`[photo id, record]` pairs: the saved records read from the
//! originals records, index-only like the synced hashes), `savedAs` (`[photo id, copy]` pairs)
//! and `projects` (`[folder id, kind]` pairs), each left out when empty.

use std::path::Path;

use dalica_core::{FolderId, PhotoId};
use dalica_library::{Folder, FolderInfo, Library, Op, Photo, PhotoInfo, Project, Saved, SavedAs, Sketch, Synced};
use serde::{Deserialize, Serialize};

use crate::journal::{decode, encode, write_whole};
use crate::{Error, Result};

/// The snapshot's file name in the library directory.
pub const FILE: &str = "index.snap";
/// The format version this crate writes and reads.
pub const VERSION: u64 = 1;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    snapshot: u64,
    /// Ops `1..=seq` are in it.
    seq: u64,
    folders: Vec<Folder>,
    photos: Vec<Photo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sketches: Vec<(PhotoId, Sketch)>,
    #[serde(rename = "photoInfo", default, skip_serializing_if = "Vec::is_empty")]
    photo_info: Vec<(PhotoId, PhotoInfo)>,
    #[serde(rename = "folderInfo", default, skip_serializing_if = "Vec::is_empty")]
    folder_info: Vec<(FolderId, FolderInfo)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    synced: Vec<(PhotoId, Synced)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    saved: Vec<(PhotoId, Saved)>,
    #[serde(rename = "savedAs", default, skip_serializing_if = "Vec::is_empty")]
    saved_as: Vec<(PhotoId, SavedAs)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    projects: Vec<(FolderId, Project)>,
}

/// Writes the model of `library` as of op `seq`.
pub fn write(path: &Path, library: &Library, seq: u64) -> Result<()> {
    let snap = Snapshot {
        snapshot: VERSION,
        seq,
        folders: library.folders().cloned().collect(),
        photos: library.photos().cloned().collect(),
        sketches: library.sketches().map(|(id, s)| (id, *s)).collect(),
        photo_info: library.infos().map(|(id, i)| (id, i.clone())).collect(),
        folder_info: library.folder_infos().map(|(id, i)| (id, i.clone())).collect(),
        synced: library.synced_all().map(|(id, s)| (id, *s)).collect(),
        saved: library.saved_all().map(|(id, s)| (id, s.clone())).collect(),
        saved_as: library.saved_as_all().map(|(id, s)| (id, s.clone())).collect(),
        projects: library.projects().collect(),
    };
    let json = serde_json::to_vec(&snap).map_err(|e| Error::Json(e.to_string()))?;
    write_whole(path, &encode(&json))
}

/// The library and op number a snapshot holds, or why it cannot be used.
pub fn read(bytes: &[u8]) -> std::result::Result<(Library, u64), String> {
    let line = bytes.strip_suffix(b"\n").ok_or("the snapshot is cut off")?;
    let json = decode(line)?;
    let snap: Snapshot = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    if snap.snapshot != VERSION {
        return Err(format!("snapshot version {} (this Dalica reads {VERSION})", snap.snapshot));
    }
    let mut library = Library::new();
    let mut ops: Vec<Op> = snap.folders.into_iter().map(|folder| Op::AddFolder { folder }).collect();
    ops.push(Op::AddPhotos { photos: snap.photos });
    if !snap.sketches.is_empty() {
        ops.push(Op::SetSketches { sketches: snap.sketches.into_iter().map(|(id, s)| (id, Some(s))).collect() });
    }
    if !snap.photo_info.is_empty() {
        ops.push(Op::SetPhotoInfo { photos: snap.photo_info });
    }
    if !snap.folder_info.is_empty() {
        ops.push(Op::SetFolderInfo { folders: snap.folder_info });
    }
    if !snap.synced.is_empty() {
        ops.push(Op::SetSynced { synced: snap.synced.into_iter().map(|(id, s)| (id, Some(s))).collect() });
    }
    if !snap.saved.is_empty() {
        ops.push(Op::SetSaved { saved: snap.saved.into_iter().map(|(id, s)| (id, Some(s))).collect() });
    }
    if !snap.saved_as.is_empty() {
        ops.push(Op::SetSavedAs { saved_as: snap.saved_as.into_iter().map(|(id, s)| (id, Some(s))).collect() });
    }
    if !snap.projects.is_empty() {
        ops.push(Op::SetProjects { projects: snap.projects.into_iter().map(|(id, p)| (id, Some(p))).collect() });
    }
    library.apply(Op::Batch { ops }).map_err(|e| format!("inconsistent snapshot: {e}"))?;
    Ok((library, snap.seq))
}
