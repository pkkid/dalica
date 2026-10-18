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
//! [`import_sidecars`] is the same without `albums.json`. Search facts, digests and the
//! first-indexed times are index-only and start empty after a rebuild; each photo's last-changed
//! time comes back from its entry's `changed` (M10), so Recently Updated keeps the changes.
//!
//! M13 (`data-model.md` §3–§4): faces, geotags and people. [`import_library`] reads `people.json`
//! too ([`crate::people`]): its people are created or given its names, nicknames and thumbnails. A
//! person `people.json` does not have but whose name a folder sidecar's top-level `"people"`
//! carries is created under that name (the first folder's), so a lost `people.json` loses no
//! person; one whose name another person already holds (ignoring case) is that person (a name is
//! unique ignoring case), so their faces are read as that person's. Each photo's faces and geotag
//! come from its entry; a face naming a person named nowhere is kept in its sidecar, reported and
//! not used. The rejected
//! suggestions live only in `people.json` and are not the model's; the embeddings cache is not
//! read here (`crate::faces_cache`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, DateTime, FolderId, PersonId, PhotoId, PosKey, Timestamp};
use dalica_library::tags::fold;
use dalica_library::{AlbumInfo, Face, FaceKey, Geo, Library, Op, Person, PhotoSort, Project, Saved, SavedAs};
use dalica_sidecar::PersonName;
use dalica_sidecar::originals::{self, Originals};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::albums::{AlbumsRead, AlbumsReading};
use crate::mapping::{self, OriginalsReading};
use crate::people::{PeopleRead, PeopleReading};
use crate::writer::{self, Outcome};

/// What the folders' sidecars say, as ops on the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// Ops that make the model's user data match the sidecars and the originals records (empty if
    /// it already does): at most one `SetStars`, one `SetPhotoInfo`, one `SetFolderInfo`, one
    /// `SetSavedAs`, one `SetProjects` and one `SetSaved` (derived: no undo step), then M9's: at
    /// most one `SetFolderSorts`, `CreateAlbums`, `SetAlbumInfo`, `RemoveMembers`, `SetMemberKeys`,
    /// `AddMembers` and `SetCovers`, in that order; then M13's: at most one batch of `AddPerson`
    /// and `RenamePerson`, one `SetFaces`, one `SetGeo` and one batch of `SetPersonThumbnail`; then
    /// at most one derived `SetLastChanged` (M10).
    pub ops: Vec<Op>,
    /// Folders whose sidecar was damaged: kept aside, folder read-only.
    pub read_only: Vec<FolderId>,
    /// Folders whose sidecar or originals record could not be read, held values of the wrong type
    /// or values past the caps, and why (once per value).
    pub problems: Vec<(FolderId, String)>,
    /// What `albums.json` and `people.json` could not give, and memberships, people and faces not
    /// used, once each (M9, M13).
    pub library_problems: Vec<String>,
}

/// [`import_sidecars`] with `albums.json` in the library directory `dir` read first (M9): its
/// albums' definitions and covers, and its copy of the order for folders whose sidecar cannot be
/// read. A damaged `albums.json` is set aside and reported, and the albums come back from the
/// folders.
pub fn import_library(library: &Library, dir: &Path, now: Timestamp) -> Import {
    let read = crate::albums::read(dir, now);
    let mut problems = Vec::new();
    let people = match crate::people::read(dir, now) {
        PeopleRead::Absent => None,
        PeopleRead::Read(r) => {
            problems.extend(r.notes.iter().cloned());
            Some(r)
        }
        PeopleRead::Unreadable(why) => {
            problems.push(format!("{}: {why}", crate::people::path(dir).display()));
            None
        }
        PeopleRead::SetAside { why, kept } => {
            let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "could not be kept aside".into());
            problems.push(format!("{}: {why}; {kept}", crate::people::path(dir).display()));
            None
        }
    };
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
    let mut out = import(library, albums.as_ref(), people.as_ref(), now);
    problems.append(&mut out.library_problems);
    out.library_problems = problems;
    out
}

/// Reads the sidecar and the originals record of every folder in `library` and returns the ops
/// that apply their user data. A value of the wrong type leaves the model's own value as it is; a
/// folder without a sidecar keeps the model's word, and one whose originals record cannot be read
/// keeps its saved records. Any thread but the UI thread may call it.
pub fn import_sidecars(library: &Library, now: Timestamp) -> Import {
    import(library, None, None, now)
}

fn import(library: &Library, albums: Option<&AlbumsReading>, people: Option<&PeopleReading>, now: Timestamp) -> Import {
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
    // M10: each photo's last-changed time as its sidecar says it.
    let mut changed: Vec<(PhotoId, Option<Timestamp>)> = Vec::new();
    // M13: names found in the folders (the first folder's wins), each photo's faces and geotag.
    let mut person_names: BTreeMap<PersonId, PersonName> = BTreeMap::new();
    let mut read_faces: Vec<(PhotoId, FolderId, Vec<Face>)> = Vec::new();
    let mut geo: Vec<(PhotoId, Option<Geo>)> = Vec::new();
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
        let (found, notes) = mapping::read_people(&sidecar);
        out.problems.extend(notes.into_iter().map(|n| (folder.id, n)));
        if let mapping::Reading::Usable(found) = found {
            for (id, name) in found {
                person_names.entry(id).or_insert(name);
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
            if read.changed != own.changed {
                changed.push((photo.id, read.changed));
            }
            read_faces.push((photo.id, folder.id, read.faces));
            if read.geo != own.geo {
                geo.push((photo.id, read.geo));
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
    people_ops(library, people, &person_names, read_faces, geo, &mut out);
    if !changed.is_empty() {
        out.ops.push(Op::SetLastChanged { changed });
    }
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

/// The people ops of an import (M13): the people to create or rename (from `people.json`, then
/// the names the folders carry), each photo's faces as its sidecar says them (faces naming a
/// person named nowhere left out), the geotags (`geo`, the changes found), then the thumbnails
/// `people.json` gives.
fn people_ops(
    library: &Library,
    people: Option<&PeopleReading>,
    names: &BTreeMap<PersonId, PersonName>,
    read_faces: Vec<(PhotoId, FolderId, Vec<Face>)>,
    geo: Vec<(PhotoId, Option<Geo>)>,
    out: &mut Import,
) {
    // Who each person is: people.json's word, else the first folder's.
    let mut defs: BTreeMap<&PersonId, (String, String)> = BTreeMap::new();
    if let Some(p) = people {
        for (id, d) in &p.people {
            defs.insert(id, (d.name.clone(), d.nickname.clone()));
        }
    }
    for (id, n) in names {
        defs.entry(id).or_insert_with(|| (n.name.clone(), n.nickname.clone().unwrap_or_default()));
    }
    // Names in use ignoring case, as the ops below leave them.
    let mut taken: BTreeMap<String, PersonId> = library.people().map(|(id, p)| (fold(&p.name), id.clone())).collect();
    let mut person_ops = Vec::new();
    let mut known: std::collections::BTreeSet<PersonId> = library.people().map(|(id, _)| id.clone()).collect();
    // People whose name another holds: the same person (names are unique ignoring case).
    let mut alias: BTreeMap<PersonId, PersonId> = BTreeMap::new();
    for (id, (name, nickname)) in defs {
        let holder = taken.get(&fold(&name)).filter(|h| *h != id).cloned();
        match library.person(id) {
            None if holder.is_none() => {
                taken.insert(fold(&name), id.clone());
                known.insert(id.clone());
                person_ops.push(Op::AddPerson { id: id.clone(), person: Person { name, nickname, thumbnail: None } });
            }
            Some(own) if own.name != name || own.nickname != nickname => {
                if holder.is_none() {
                    taken.remove(&fold(&own.name));
                    taken.insert(fold(&name), id.clone());
                    person_ops.push(Op::RenamePerson { id: id.clone(), name, nickname });
                } else {
                    out.library_problems.push(format!("person {id}'s name is another person's (ignoring case): kept as the library has it"));
                }
            }
            Some(_) => {}
            None => {
                if let Some(holder) = holder {
                    out.library_problems.push(format!("person {id} has the name of person {holder} (ignoring case): their faces are {holder}'s"));
                    alias.insert(id.clone(), holder);
                }
            }
        }
    }
    // Faces: each photo's, as its sidecar says, of known people.
    let mut faces: Vec<(PhotoId, Vec<Face>)> = Vec::new();
    let mut after: BTreeMap<PhotoId, Vec<Face>> = BTreeMap::new();
    let mut unnamed: BTreeMap<FolderId, usize> = BTreeMap::new();
    for (photo, folder, list) in read_faces {
        let list = list
            .into_iter()
            .map(|f| match f.person.person().and_then(|p| alias.get(p)) {
                Some(holder) => Face::new(f.rect, dalica_library::FacePerson::Person(holder.clone())),
                None => f,
            })
            .collect();
        let (list, dropped) = mapping::known_faces(list, |p| known.contains(p));
        if dropped > 0 {
            *unnamed.entry(folder).or_default() += dropped;
        }
        if list != library.faces(photo) {
            after.insert(photo, list.clone());
            faces.push((photo, list));
        }
    }
    for (folder, n) in unnamed {
        out.problems
            .push((folder, format!("{n} faces name a person named nowhere (in {} or a folder): kept in the sidecar, not used", crate::people::FILE)));
    }
    // Thumbnails: people.json's, by path and rectangle, when the face is still the person's.
    let mut thumbs = Vec::new();
    if let Some(p) = people {
        for (id, def) in &p.people {
            if !known.contains(id) {
                continue;
            }
            let key = def.thumbnail.as_ref().and_then(|(path, rect)| {
                let folder = library.folder_by_path(path.parent()?)?;
                let photo = library.photo_by_name(folder, path.file_name()?.to_str()?)?.id;
                let list = after.get(&photo).map_or_else(|| library.faces(photo), Vec::as_slice);
                list.iter().any(|f| f.rect == *rect && f.person.person() == Some(id)).then_some(FaceKey { photo, rect: *rect })
            });
            if def.thumbnail.is_some() && key.is_none() {
                out.library_problems.push(format!("person {id}'s thumbnail is not one of their faces: they show their first face"));
            }
            if key != library.person(id).and_then(|own| own.thumbnail) {
                thumbs.push(Op::SetPersonThumbnail { id: id.clone(), thumbnail: key });
            }
        }
    }
    if !person_ops.is_empty() {
        out.ops.push(Op::Batch { ops: person_ops });
    }
    if !faces.is_empty() {
        out.ops.push(Op::SetFaces { faces });
    }
    if !geo.is_empty() {
        out.ops.push(Op::SetGeo { geo });
    }
    if !thumbs.is_empty() {
        out.ops.push(Op::Batch { ops: thumbs });
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
