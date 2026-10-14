//! Rebuilding user data from the folders (`data-model.md` §10.1). The index is a cache: after it is
//! lost, a scan rebuilds the folders and photos (crate `scan`), and [`import_sidecars`] reads each
//! folder's `.dalica.json` to give back what the user set: per photo its star, caption, tags,
//! hidden flag, recipe, redo list, date and the copy a Save wrote beside it (`savedAs`), per folder
//! its description, date, place, hidden flag and project mark; and each folder's originals record
//! (`.dalica-originals/.dalica.json`, [`read_originals`]) to give back the photos' saved records.
//! Nothing a sidecar or a record holds is lost by deleting the index. The synced metadata hashes
//! are index-only (`mapping::SYNCED_FIELDS`) and start empty after a rebuild.
//!
//! M9 (`data-model.md` §3.2, §4): each folder's photo sort, and the albums. [`import_library`]
//! reads `albums.json` in the library directory first ([`crate::albums`]): its albums are created
//! or given its definitions and covers. An album the model and `albums.json` do not have but whose
//! name a folder sidecar's top-level `"albums"` carries is created under that name (dated by its
//! newest photo; description, place and cover empty), so a lost `albums.json` loses no album.
//! Memberships with their position keys come from every folder sidecar; a membership of an album
//! named nowhere is kept in its sidecar, reported and not used. A folder whose sidecar is damaged
//! or cannot be read takes its photos' memberships from `albums.json`'s copy of the order.
//! [`import_sidecars`] is the same without `albums.json`. Search facts, digests and the times are
//! index-only and start empty after a rebuild.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{AlbumInfo, Library, Op, PhotoSort, Project, Saved, SavedAs};
use dalica_sidecar::originals::{self, Originals};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::albums::{AlbumsRead, AlbumsReading};
use crate::mapping::{self, OriginalsReading};
use crate::writer::{self, Outcome};

/// What the folders' sidecars say, as ops on the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// Ops that make the model's user data match the sidecars and the originals records (empty if
    /// it already does): at most one `SetStars`, one `SetPhotoInfo`, one `SetFolderInfo`, one
    /// `SetSavedAs`, one `SetProjects` and one `SetSaved` (derived: no undo step), then M9's: at
    /// most one `SetFolderSorts`, `CreateAlbums`, `SetAlbumInfo`, `RemoveMembers`, `SetMemberKeys`,
    /// `AddMembers` and `SetCovers`, in that order.
    pub ops: Vec<Op>,
    /// Folders whose sidecar was damaged: kept aside, folder read-only.
    pub read_only: Vec<FolderId>,
    /// Folders whose sidecar or originals record could not be read, held values of the wrong type
    /// or values past the caps, and why (once per value).
    pub problems: Vec<(FolderId, String)>,
    /// What `albums.json` could not give, and memberships not used, once each (M9).
    pub library_problems: Vec<String>,
}

/// [`import_sidecars`] with `albums.json` in the library directory `dir` read first (M9): its
/// albums' definitions and covers, and its copy of the order for folders whose sidecar cannot be
/// read. A damaged `albums.json` is set aside and reported, and the albums come back from the
/// folders.
pub fn import_library(library: &Library, dir: &Path, now: Timestamp) -> Import {
    let read = crate::albums::read(dir, now);
    let mut problems = Vec::new();
    let albums = match read {
        AlbumsRead::Absent => None,
        AlbumsRead::Read(r) => {
            problems.extend(r.notes.iter().cloned());
            Some(r)
        }
        AlbumsRead::Unreadable(why) => {
            problems.push(format!("{}: {why}", crate::albums::path(dir).display()));
            None
        }
        AlbumsRead::SetAside { why, kept } => {
            let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "could not be kept aside".into());
            problems.push(format!("{}: {why}; {kept}", crate::albums::path(dir).display()));
            None
        }
    };
    let mut out = import(library, albums.as_ref(), now);
    problems.append(&mut out.library_problems);
    out.library_problems = problems;
    out
}

/// Reads the sidecar and the originals record of every folder in `library` and returns the ops
/// that apply their user data. A value of the wrong type leaves the model's own value as it is; a
/// folder without a sidecar keeps the model's word, and one whose originals record cannot be read
/// keeps its saved records. Any thread but the UI thread may call it.
pub fn import_sidecars(library: &Library, now: Timestamp) -> Import {
    import(library, None, now)
}

fn import(library: &Library, albums: Option<&AlbumsReading>, now: Timestamp) -> Import {
    let limits = Limits::default();
    let mut out = Import::default();
    let mut stars = Vec::new();
    let mut infos = Vec::new();
    let mut folders = Vec::new();
    let mut saved_as: Vec<(PhotoId, Option<SavedAs>)> = Vec::new();
    let mut projects: Vec<(FolderId, Option<Project>)> = Vec::new();
    let mut saved: Vec<(PhotoId, Option<Saved>)> = Vec::new();
    let mut sorts: Vec<(FolderId, PhotoSort)> = Vec::new();
    // M9: names found in the folders (the first folder's wins), each photo's memberships as its
    // sidecar says them, and the folders whose sidecar could not be read.
    let mut names: BTreeMap<AlbumId, String> = BTreeMap::new();
    let mut read_members: Vec<(PhotoId, BTreeMap<AlbumId, PosKey>)> = Vec::new();
    let mut unread: BTreeSet<FolderId> = BTreeSet::new();
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
                    unread.insert(folder.id);
                    continue;
                }
            },
            Err(e) => {
                out.problems.push((folder.id, e.to_string()));
                unread.insert(folder.id);
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
        let own_sort = library.folder_sort(folder.id);
        let sort = reading.photo_sort(own_sort);
        if sort != own_sort {
            sorts.push((folder.id, sort));
        }
        let (found, notes) = mapping::read_album_names(&sidecar);
        out.problems.extend(notes.into_iter().map(|n| (folder.id, n)));
        if let mapping::Reading::Usable(found) = found {
            for (id, name) in found {
                names.entry(id).or_insert(name);
            }
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
            if read.albums != own.albums {
                read_members.push((photo.id, read.albums));
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
    if !sorts.is_empty() {
        out.ops.push(Op::SetFolderSorts { sorts });
    }
    album_ops(library, albums, &names, read_members, &unread, &mut out);
    out
}

/// The album ops of an import (M9): the albums to create or redefine, the memberships the
/// sidecars (or, for unread folders, `albums.json`'s order) say, and the covers.
fn album_ops(
    library: &Library,
    albums: Option<&AlbumsReading>,
    names: &BTreeMap<AlbumId, String>,
    read_members: Vec<(PhotoId, BTreeMap<AlbumId, PosKey>)>,
    unread: &BTreeSet<FolderId>,
    out: &mut Import,
) {
    let empty = BTreeMap::new();
    let defs = albums.map_or(&empty, |a| &a.albums);
    // Albums: albums.json's definitions, then the names the folders carry.
    let mut create: Vec<(AlbumId, AlbumInfo)> = Vec::new();
    let mut redefine: Vec<(AlbumId, AlbumInfo)> = Vec::new();
    for (id, def) in defs {
        match library.album(id) {
            None => create.push((id.clone(), def.info.clone())),
            Some(own) if *own != def.info => redefine.push((id.clone(), def.info.clone())),
            Some(_) => {}
        }
    }
    for (id, name) in names {
        if library.album(id).is_none() && !defs.contains_key(id) {
            create.push((id.clone(), AlbumInfo::named(name.clone())));
        }
    }
    let known = |id: &AlbumId| library.album(id).is_some() || defs.contains_key(id) || names.contains_key(id);

    // Memberships: each photo's as its sidecar says, for known albums.
    let mut unnamed: BTreeSet<AlbumId> = BTreeSet::new();
    let mut target: Vec<(PhotoId, BTreeMap<AlbumId, PosKey>)> = Vec::with_capacity(read_members.len());
    for (photo, mut members) in read_members {
        members.retain(|a, _| {
            let k = known(a);
            if !k {
                unnamed.insert(a.clone());
            }
            k
        });
        target.push((photo, members));
    }
    for a in unnamed {
        out.library_problems.push(format!(
            "album {a} has members in the folders but no name in {} or in any folder: its memberships are kept in the sidecars, not used",
            crate::albums::FILE
        ));
    }
    // Folders whose sidecar could not be read: albums.json's copy of the order adds memberships.
    let mut extra: Vec<(AlbumId, PhotoId, PosKey)> = Vec::new();
    let mut seen: BTreeSet<(&AlbumId, PhotoId)> = BTreeSet::new();
    if !unread.is_empty() {
        let mut by_path: HashMap<&Path, FolderId> = HashMap::new();
        for f in library.folders().filter(|f| unread.contains(&f.id)) {
            by_path.insert(f.path.as_path(), f.id);
        }
        for (album, def) in defs {
            for (path, key) in &def.order {
                let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { continue };
                let Some(&folder) = by_path.get(parent) else { continue };
                let Some(photo) = library.photo_by_name(folder, name) else { continue };
                if library.member_key(album, photo.id).is_none() && seen.insert((album, photo.id)) {
                    extra.push((album.clone(), photo.id, key.clone()));
                }
            }
        }
    }
    let mut remove = Vec::new();
    let mut rekey = Vec::new();
    let mut add = Vec::new();
    for (photo, members) in &target {
        for (album, _) in library.albums_of(*photo) {
            if !members.contains_key(album) {
                remove.push((album.clone(), *photo));
            }
        }
        for (album, key) in members {
            match library.member_key(album, *photo) {
                None => add.push((album.clone(), *photo, key.clone())),
                Some(own) if own != key => rekey.push((album.clone(), *photo, key.clone())),
                Some(_) => {}
            }
        }
    }
    add.extend(extra);
    let member_after = |album: &AlbumId, photo: PhotoId| {
        add.iter().any(|(a, p, _)| a == album && *p == photo)
            || (library.member_key(album, photo).is_some() && !remove.iter().any(|(a, p)| a == album && *p == photo))
    };
    // Covers: albums.json's, by path, when the photo is a member.
    let mut covers = Vec::new();
    for (album, def) in defs {
        let cover = def.cover.as_deref().and_then(|path| {
            let folder = library.folder_by_path(path.parent()?)?;
            let photo = library.photo_by_name(folder, path.file_name()?.to_str()?)?;
            Some(photo.id)
        });
        let cover = cover.filter(|&p| member_after(album, p));
        if def.cover.is_some() && cover.is_none() {
            out.library_problems.push(format!("album {album}'s cover is not one of its photos: it shows its first photo"));
        }
        let own = library.cover_set(album).filter(|&p| member_after(album, p));
        if cover != own {
            covers.push((album.clone(), cover));
        }
    }
    if !create.is_empty() {
        out.ops.push(Op::CreateAlbums { albums: create });
    }
    if !redefine.is_empty() {
        out.ops.push(Op::SetAlbumInfo { albums: redefine });
    }
    if !remove.is_empty() {
        out.ops.push(Op::RemoveMembers { members: remove });
    }
    if !rekey.is_empty() {
        out.ops.push(Op::SetMemberKeys { keys: rekey });
    }
    if !add.is_empty() {
        out.ops.push(Op::AddMembers { members: add });
    }
    if !covers.is_empty() {
        out.ops.push(Op::SetCovers { covers });
    }
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
