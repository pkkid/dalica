//! The owned extract of what search reads (`organizing.md` §8.1–§8.2), and the updates that keep an
//! index in step with the library one op at a time.
//!
//! The extract owns its data, so the engine can hand it to a worker that keeps the index there; the
//! owner thread only works out an [`Update`] for each op it applies (the records the op touched,
//! read again from the library) and sends it along.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, Timestamp};

use super::grammar::{MONTHS, terms};
use crate::tags::fold;
use crate::{Library, Op, Photo, PhotoSort};

/// What search reads of one photo: its own fields, what the user said about it, its search facts
/// and its albums.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotoDoc {
    pub id: PhotoId,
    pub folder: FolderId,
    /// The file name.
    pub name: String,
    /// Bytes on disk.
    pub size: u64,
    /// Size in pixels as displayed; 0 × 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// The effective date ([`Library::effective_date`]): what the date words match.
    pub date: Option<DateTime>,
    /// The date it sorts by ([`Library::sort_date`]): what the age filter reads.
    pub sort_date: Option<DateTime>,
    /// When it was last updated ([`Library::updated_time`]), for Recently Updated.
    pub updated: Timestamp,
    pub star: bool,
    /// Hidden by the user (its folder's hidden flag is the folder's).
    pub hidden: bool,
    pub caption: String,
    pub tags: Vec<String>,
    /// Camera make and model, as read.
    pub make: String,
    pub model: String,
    pub video: bool,
    pub geotagged: bool,
    /// The user albums it is in, by id.
    pub albums: Vec<AlbumId>,
}

impl PhotoDoc {
    /// The record of `photo` in `library`, in the albums `albums`.
    fn of(library: &Library, photo: &Photo, mut albums: Vec<AlbumId>) -> PhotoDoc {
        albums.sort();
        albums.dedup();
        let info = library.info(photo.id);
        let facts = library.facts(photo.id);
        PhotoDoc {
            id: photo.id,
            folder: photo.folder,
            name: photo.name.clone(),
            size: photo.size,
            width: photo.width,
            height: photo.height,
            date: info.date.or(photo.date),
            sort_date: library.sort_date(photo),
            updated: library.updated_time(photo),
            star: photo.star,
            hidden: info.hidden,
            caption: info.caption.clone(),
            tags: info.tags.clone(),
            make: facts.make.clone(),
            model: facts.model.clone(),
            video: facts.video,
            geotagged: facts.geotagged,
            albums,
        }
    }

    /// Photo `id`'s record, or `None` when the library has no such photo.
    pub fn from_library(library: &Library, id: PhotoId) -> Option<PhotoDoc> {
        let photo = library.photo(id)?;
        Some(PhotoDoc::of(library, photo, library.albums_of(id).into_iter().map(|(a, _)| a.clone()).collect()))
    }

    /// The terms plain words find in the photo itself: file name, caption, the date words (year,
    /// `2026-06`, `2026-06-04`, the English month name) and the pixel size (`4000x3000`).
    pub(crate) fn own_terms(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = terms(&self.name).chain(terms(&self.caption)).collect();
        if let Some(d) = self.date {
            let date = d.date();
            let (y, m) = (date.year(), date.month());
            out.insert(format!("{y:04}"));
            out.insert(format!("{y:04}-{m:02}"));
            out.insert(format!("{y:04}-{m:02}-{:02}", date.day()));
            if let Some(name) = usize::from(m).checked_sub(1).and_then(|i| MONTHS.get(i)) {
                out.insert((*name).to_string());
            }
        }
        if self.width > 0 && self.height > 0 {
            out.insert(format!("{}x{}", self.width, self.height));
        }
        out
    }

    /// The terms of its tags (what `tag:` looks at).
    pub(crate) fn tag_terms(&self) -> BTreeSet<String> {
        self.tags.iter().flat_map(|t| terms(t)).collect()
    }

    /// The terms of its camera make and model (what `camera:` looks at).
    pub(crate) fn camera_terms(&self) -> BTreeSet<String> {
        terms(&self.make).chain(terms(&self.model)).collect()
    }
}

/// What search reads of one folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderDoc {
    pub id: FolderId,
    pub path: PathBuf,
    pub description: String,
    pub place: String,
    /// Hidden by the user: its photos count as hidden.
    pub hidden: bool,
    /// Its own photo sort, for the light-table order.
    pub sort: PhotoSort,
}

impl FolderDoc {
    /// Folder `id`'s record, or `None` when the library has no such folder.
    pub fn from_library(library: &Library, id: FolderId) -> Option<FolderDoc> {
        let folder = library.folder(id)?;
        let info = library.folder_info(id);
        Some(FolderDoc {
            id,
            path: folder.path.clone(),
            description: info.description.clone(),
            place: info.place.clone(),
            hidden: info.hidden,
            sort: library.folder_sort(id),
        })
    }

    /// Its name as the tree shows it: the last part of its path.
    pub fn name(&self) -> String {
        folder_name(&self.path)
    }

    /// The terms plain words find through the folder: its name, description and place.
    pub(crate) fn terms(&self) -> BTreeSet<String> {
        let name = self.name();
        terms(&name).chain(terms(&self.description)).chain(terms(&self.place)).collect()
    }
}

/// The last part of `path` (the whole path when it has none, as for a root).
pub(crate) fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// What search reads of one user album.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumDoc {
    pub id: AlbumId,
    pub name: String,
}

impl AlbumDoc {
    /// Album `id`'s record, or `None` when the library has no such album.
    pub fn from_library(library: &Library, id: &AlbumId) -> Option<AlbumDoc> {
        library.album(id).map(|info| AlbumDoc { id: id.clone(), name: info.name.clone() })
    }

    /// The terms of its name.
    pub(crate) fn terms(&self) -> BTreeSet<String> {
        terms(&self.name).collect()
    }
}

/// Everything search reads of a library, owned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extract {
    pub photos: Vec<PhotoDoc>,
    pub folders: Vec<FolderDoc>,
    pub albums: Vec<AlbumDoc>,
}

impl Extract {
    /// The extract of the whole library (one pass over the photos and one over the memberships).
    pub fn from_library(library: &Library) -> Extract {
        let mut albums_of: HashMap<PhotoId, Vec<AlbumId>> = HashMap::new();
        for (album, members) in library.memberships() {
            for id in members.keys() {
                albums_of.entry(*id).or_default().push(album.clone());
            }
        }
        Extract {
            photos: library.photos().map(|p| PhotoDoc::of(library, p, albums_of.remove(&p.id).unwrap_or_default())).collect(),
            folders: library.folders().filter_map(|f| FolderDoc::from_library(library, f.id)).collect(),
            albums: library.albums().map(|(id, info)| AlbumDoc { id: id.clone(), name: info.name.clone() }).collect(),
        }
    }
}

/// The records one applied op changed, read again from the library after it: each `None` is a
/// record that is gone. An index applies the purges first (a removed folder takes its photos, a
/// deleted album its memberships), then the folders, the albums and the photos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Update {
    /// Folders removed by the op (their photos go with them, even when the folder came back).
    pub purged_folders: Vec<FolderId>,
    /// Albums deleted by the op (every membership goes, even when the album came back).
    pub purged_albums: Vec<AlbumId>,
    pub folders: Vec<(FolderId, Option<FolderDoc>)>,
    pub albums: Vec<(AlbumId, Option<AlbumDoc>)>,
    pub photos: Vec<(PhotoId, Option<PhotoDoc>)>,
}

/// What an op touched, before reading the records again.
#[derive(Default)]
struct Touched {
    purged_folders: BTreeSet<FolderId>,
    purged_albums: BTreeSet<AlbumId>,
    folders: BTreeSet<FolderId>,
    albums: BTreeSet<AlbumId>,
    photos: BTreeSet<PhotoId>,
    /// Tags renamed to these (folded): every photo holding one now is touched.
    renamed_to: BTreeSet<String>,
    /// Folders moved here: every folder at or under one is touched.
    moved_to: Vec<PathBuf>,
}

impl Touched {
    /// Walks `op` (batches without recursion, so any nesting is safe).
    fn walk(&mut self, op: &Op) {
        let mut stack: Vec<&Op> = vec![op];
        while let Some(op) = stack.pop() {
            match op {
                Op::Batch { ops } => stack.extend(ops.iter()),
                Op::AddFolder { folder } => {
                    self.folders.insert(folder.id);
                }
                Op::RemoveFolder { id } => {
                    self.purged_folders.insert(*id);
                    self.folders.insert(*id);
                }
                Op::AddPhotos { photos } | Op::UpdatePhotos { photos } => self.photos.extend(photos.iter().map(|p| p.id)),
                Op::RemovePhotos { ids } => self.photos.extend(ids.iter().copied()),
                Op::SetStars { stars } => self.photos.extend(stars.iter().map(|(id, _)| *id)),
                Op::SetCaptions { captions } => self.photos.extend(captions.iter().map(|(id, _)| *id)),
                Op::AddTags { ids, .. } | Op::RemoveTags { ids, .. } => self.photos.extend(ids.iter().copied()),
                Op::RenameTag { to, .. } => {
                    self.renamed_to.insert(fold(to));
                }
                Op::SetTags { tags } => self.photos.extend(tags.iter().map(|(id, _)| *id)),
                Op::SetHidden { hidden } => self.photos.extend(hidden.iter().map(|(id, _)| *id)),
                Op::SetDates { dates } => self.photos.extend(dates.iter().map(|(id, _)| *id)),
                Op::SetPhotoInfo { photos } => self.photos.extend(photos.iter().map(|(id, _)| *id)),
                Op::SetFolderInfo { folders } => self.folders.extend(folders.iter().map(|(id, _)| *id)),
                Op::SetFoldersHidden { hidden } => self.folders.extend(hidden.iter().map(|(id, _)| *id)),
                Op::SetFolderSorts { sorts } => self.folders.extend(sorts.iter().map(|(id, _)| *id)),
                Op::MovePhotos { moves } => self.photos.extend(moves.iter().map(|(id, ..)| *id)),
                Op::MoveFolder { id, path } => {
                    self.folders.insert(*id);
                    self.moved_to.push(path.clone());
                }
                Op::CreateAlbums { albums } | Op::SetAlbumInfo { albums } => self.albums.extend(albums.iter().map(|(id, _)| id.clone())),
                Op::DeleteAlbums { ids } => {
                    self.purged_albums.extend(ids.iter().cloned());
                    self.albums.extend(ids.iter().cloned());
                }
                Op::AddMembers { members } => self.photos.extend(members.iter().map(|(_, id, _)| *id)),
                Op::RemoveMembers { members } => self.photos.extend(members.iter().map(|(_, id)| *id)),
                Op::SetFacts { facts } => self.photos.extend(facts.iter().map(|(id, _)| *id)),
                Op::SetFirstIndexed { indexed } => self.photos.extend(indexed.iter().map(|(id, _)| *id)),
                Op::SetLastChanged { changed } => self.photos.extend(changed.iter().map(|(id, _)| *id)),
                // Nothing search reads: recipes, sketches, synced hashes, saved records, copies saved
                // beside, project marks, member keys (album order is not search order), covers,
                // digests.
                Op::SetRecipes { .. }
                | Op::SetSketches { .. }
                | Op::SetSynced { .. }
                | Op::SetSaved { .. }
                | Op::SetSavedAs { .. }
                | Op::SetProjects { .. }
                | Op::SetMemberKeys { .. }
                | Op::SetCovers { .. }
                | Op::SetDigests { .. } => {}
            }
        }
    }
}

impl Update {
    /// The update for `op`, which `library` has just applied (or whose inverse it has just
    /// applied: an inverse is an op like any other).
    pub fn for_op(library: &Library, op: &Op) -> Update {
        let mut t = Touched::default();
        t.walk(op);
        if !t.renamed_to.is_empty() {
            for (id, info) in library.infos() {
                if info.tags.iter().any(|tag| t.renamed_to.contains(&fold(tag))) {
                    t.photos.insert(id);
                }
            }
        }
        if !t.moved_to.is_empty() {
            for f in library.folders() {
                if t.moved_to.iter().any(|to| f.path.starts_with(to)) {
                    t.folders.insert(f.id);
                }
            }
        }
        let albums_of = memberships_of(library, &t.photos);
        Update {
            purged_folders: t.purged_folders.into_iter().collect(),
            purged_albums: t.purged_albums.into_iter().collect(),
            folders: t.folders.into_iter().map(|id| (id, FolderDoc::from_library(library, id))).collect(),
            albums: t.albums.into_iter().map(|id| (id.clone(), AlbumDoc::from_library(library, &id))).collect(),
            photos: t
                .photos
                .into_iter()
                .map(|id| {
                    let doc = library.photo(id).map(|p| PhotoDoc::of(library, p, albums_of.get(&id).cloned().unwrap_or_default()));
                    (id, doc)
                })
                .collect(),
        }
    }

    /// Whether the update changes nothing.
    pub fn is_empty(&self) -> bool {
        self.purged_folders.is_empty() && self.purged_albums.is_empty() && self.folders.is_empty() && self.albums.is_empty() && self.photos.is_empty()
    }
}

/// The albums of each of `photos`, walking each album's members or the photos, whichever is fewer.
fn memberships_of(library: &Library, photos: &BTreeSet<PhotoId>) -> BTreeMap<PhotoId, Vec<AlbumId>> {
    let mut out: BTreeMap<PhotoId, Vec<AlbumId>> = BTreeMap::new();
    if photos.is_empty() {
        return out;
    }
    for (album, members) in library.memberships() {
        if members.len() < photos.len() {
            for id in members.keys().filter(|id| photos.contains(id)) {
                out.entry(*id).or_default().push(album.clone());
            }
        } else {
            for id in photos.iter().filter(|id| members.contains_key(id)) {
                out.entry(*id).or_default().push(album.clone());
            }
        }
    }
    out
}
