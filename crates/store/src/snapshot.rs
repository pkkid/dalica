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
//!
//! M9's fields too: `folderSorts` (`[folder id, sort]` pairs), `albums` (`[album id, info]`
//! pairs), `members` (`[album id, photo id, position key]` triples), `covers` (`[album id, photo
//! id]` pairs), and the index-only `facts` (`[photo id, facts]`), `digests` (`[photo id, digest]`),
//! `firstIndexed` and `lastChanged` (`[photo id, time in milliseconds]`), each left out when empty.
//!
//! M13's too: `people` (`[person id, person]` pairs: name, nickname, thumbnail face), `faces`
//! (`[photo id, faces]` pairs) and `geo` (`[photo id, geotag]` pairs: `{"at": [lat, lon]}` or
//! `"removed"`), each left out when empty. Loading puts the people back before the faces that
//! name them.
//!
//! M14's too: `albumLegacy` (`[album id, legacy id]` pairs), `personLegacy` (`[person id, legacy
//! id]` pairs), and the derived facts `folderSidecars` (`[folder id, sidecar in effect]` pairs)
//! and `legacyOriginals` (`[photo id, original]` pairs), each left out when empty. Loading puts
//! them back after the albums and people they name.

use std::path::Path;

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId, PosKey, Timestamp};
use dalica_library::{
    AlbumInfo, ContentDigest, Face, Facts, Folder, FolderInfo, FolderSidecar, Geo, LegacyOriginal, Library, Op, Person, Photo, PhotoInfo, PhotoSort,
    Project, Saved, SavedAs, Sketch, Synced,
};
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
    #[serde(rename = "folderSorts", default, skip_serializing_if = "Vec::is_empty")]
    folder_sorts: Vec<(FolderId, PhotoSort)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    albums: Vec<(AlbumId, AlbumInfo)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    members: Vec<(AlbumId, PhotoId, PosKey)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    covers: Vec<(AlbumId, PhotoId)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    facts: Vec<(PhotoId, Facts)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    digests: Vec<(PhotoId, ContentDigest)>,
    #[serde(rename = "firstIndexed", default, skip_serializing_if = "Vec::is_empty")]
    first_indexed: Vec<(PhotoId, Timestamp)>,
    #[serde(rename = "lastChanged", default, skip_serializing_if = "Vec::is_empty")]
    last_changed: Vec<(PhotoId, Timestamp)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    people: Vec<(PersonId, Person)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    faces: Vec<(PhotoId, Vec<Face>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    geo: Vec<(PhotoId, Geo)>,
    #[serde(rename = "albumLegacy", default, skip_serializing_if = "Vec::is_empty")]
    album_legacy: Vec<(AlbumId, String)>,
    #[serde(rename = "personLegacy", default, skip_serializing_if = "Vec::is_empty")]
    person_legacy: Vec<(PersonId, String)>,
    #[serde(rename = "folderSidecars", default, skip_serializing_if = "Vec::is_empty")]
    folder_sidecars: Vec<(FolderId, FolderSidecar)>,
    #[serde(rename = "legacyOriginals", default, skip_serializing_if = "Vec::is_empty")]
    legacy_originals: Vec<(PhotoId, LegacyOriginal)>,
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
        folder_sorts: library.folder_sorts().collect(),
        albums: library.albums().map(|(id, i)| (id.clone(), i.clone())).collect(),
        members: library.memberships().flat_map(|(a, m)| m.iter().map(move |(p, k)| (a.clone(), *p, k.clone()))).collect(),
        covers: library.albums().filter_map(|(id, _)| Some((id.clone(), library.cover_set(id)?))).collect(),
        facts: library.facts_all().map(|(id, f)| (id, f.clone())).collect(),
        digests: library.digests().map(|(id, d)| (id, d.clone())).collect(),
        first_indexed: library.first_indexed_all().collect(),
        last_changed: library.last_changed_all().collect(),
        people: library.people().map(|(id, p)| (id.clone(), p.clone())).collect(),
        faces: library.faces_all().map(|(id, f)| (id, f.to_vec())).collect(),
        geo: library.geo_all().collect(),
        album_legacy: library.album_legacies().map(|(id, l)| (id.clone(), l.to_string())).collect(),
        person_legacy: library.person_legacies().map(|(id, l)| (id.clone(), l.to_string())).collect(),
        folder_sidecars: library.folder_sidecars().collect(),
        legacy_originals: library.legacy_originals().map(|(id, o)| (id, o.clone())).collect(),
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
    if !snap.folder_sorts.is_empty() {
        ops.push(Op::SetFolderSorts { sorts: snap.folder_sorts });
    }
    if !snap.albums.is_empty() {
        ops.push(Op::CreateAlbums { albums: snap.albums });
    }
    if !snap.members.is_empty() {
        ops.push(Op::AddMembers { members: snap.members });
    }
    if !snap.covers.is_empty() {
        ops.push(Op::SetCovers { covers: snap.covers.into_iter().map(|(a, p)| (a, Some(p))).collect() });
    }
    if !snap.facts.is_empty() {
        ops.push(Op::SetFacts { facts: snap.facts });
    }
    if !snap.digests.is_empty() {
        ops.push(Op::SetDigests { digests: snap.digests.into_iter().map(|(id, d)| (id, Some(d))).collect() });
    }
    if !snap.first_indexed.is_empty() {
        ops.push(Op::SetFirstIndexed { indexed: snap.first_indexed.into_iter().map(|(id, t)| (id, Some(t))).collect() });
    }
    if !snap.last_changed.is_empty() {
        ops.push(Op::SetLastChanged { changed: snap.last_changed.into_iter().map(|(id, t)| (id, Some(t))).collect() });
    }
    // People before the faces that name them (a person's thumbnail is not checked on adding).
    ops.extend(snap.people.into_iter().map(|(id, person)| Op::AddPerson { id, person }));
    if !snap.faces.is_empty() {
        ops.push(Op::SetFaces { faces: snap.faces });
    }
    if !snap.geo.is_empty() {
        ops.push(Op::SetGeo { geo: snap.geo.into_iter().map(|(id, g)| (id, Some(g))).collect() });
    }
    // M14: after the albums and people they name.
    if !snap.album_legacy.is_empty() || !snap.person_legacy.is_empty() {
        ops.push(Op::SetLegacyIds {
            albums: snap.album_legacy.into_iter().map(|(id, l)| (id, Some(l))).collect(),
            people: snap.person_legacy.into_iter().map(|(id, l)| (id, Some(l))).collect(),
        });
    }
    if !snap.folder_sidecars.is_empty() {
        ops.push(Op::SetFolderSidecars { folders: snap.folder_sidecars });
    }
    if !snap.legacy_originals.is_empty() {
        ops.push(Op::SetLegacyOriginals { originals: snap.legacy_originals.into_iter().map(|(id, o)| (id, Some(o))).collect() });
    }
    library.apply(Op::Batch { ops }).map_err(|e| format!("inconsistent snapshot: {e}"))?;
    Ok((library, snap.seq))
}
