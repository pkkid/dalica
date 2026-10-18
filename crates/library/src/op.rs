//! Ops: the only way the model changes (`architecture.md` §4). Applying an op returns its inverse;
//! applying the inverse restores the model exactly.
//!
//! [`Op::SetSketches`] and [`Op::SetSynced`] are the ops on derived data: they bump no library
//! revision (a sketch bumps the thumbnail revision instead), touch no folder, and are journaled but
//! never put on an undo stack ([`Op::is_derived`]). Removing a photo drops its info, sketch and
//! synced hashes; removing a folder drops its info too; the inverse puts them back.
//!
//! The M6 ops on what the user says about photos and folders each change one field over any
//! number of photos or folders, and refuse text past the caps of [`crate::tags`]. Tag ops follow
//! the case rule there; their inverse is [`Op::SetTags`] with the lists as they were.
//!
//! M8's ops: [`Op::SetSaved`] (the saved records, index-only like synced hashes, so derived),
//! [`Op::SetSavedAs`] (copies saved beside photos) and [`Op::SetProjects`] (the Exported mark).
//! A Save, an Undo Save and a Revert are each one batch of the record and the recipe, built by
//! [`Library::save_op`], [`Library::undo_save_op`] and [`Library::revert_op`]; such a batch is not
//! derived (its recipe change bumps the revision). Removing a photo takes its record and its copy
//! saved beside; removing a folder takes its mark; the inverse puts them back.
//!
//! M9's ops: albums ([`Op::CreateAlbums`], [`Op::DeleteAlbums`] with every membership and cover in
//! the inverse, [`Op::SetAlbumInfo`], [`Op::AddMembers`] at position keys, [`Op::RemoveMembers`],
//! [`Op::SetMemberKeys`] for reorders and re-keys, [`Op::SetCovers`]), photos moved or renamed
//! keeping their id and data ([`Op::MovePhotos`]), folders renamed or moved with their subtrees
//! ([`Op::MoveFolder`]; an empty folder is an [`Op::AddFolder`]), folder photo sorts
//! ([`Op::SetFolderSorts`]), and the index-only search facts, digests, first-indexed and
//! last-changed times ([`Op::SetFacts`], [`Op::SetDigests`], [`Op::SetFirstIndexed`],
//! [`Op::SetLastChanged`]: derived). [`Library::stamped`] adds the last-changed time to the op of a
//! change, so the journal replays it and the inverse puts the old time back. A cover leaving its
//! album (removed from it, or the photo removed) is cleared in the same op, and its inverse sets it
//! again. Removing a photo takes everything said about it, memberships included.
//!
//! M13's ops (`people.rs`): faces ([`Op::SetFaces`]), geotags ([`Op::SetGeo`]) and people
//! ([`Op::AddPerson`], [`Op::RenamePerson`], [`Op::MergePeople`], [`Op::DeletePerson`],
//! [`Op::SetPersonThumbnail`]). Removing a photo takes its faces and geotag; deleting or merging
//! away a person takes them out of every face, and the inverse puts the person back first, then
//! the faces.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use dalica_core::{AlbumId, DateTime, FolderId, PersonId, PhotoId, PosKey, Timestamp};
use serde::{Deserialize, Serialize};

use crate::model::{
    AlbumInfo, ContentDigest, Facts, Folder, FolderInfo, Library, MAX_FILE_NAME, Photo, PhotoInfo, PhotoSort, Project, Recipe, Saved, SavedAs,
    Sketch, Synced,
};
use crate::people::{Face, FaceKey, Geo, Person};
use crate::tags::{self, fold};
use crate::{Error, Result};

/// One change to the model. Serialised as one journal record (`{"op": "setStars", …}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// A new folder (its id and path must be new).
    AddFolder {
        folder: Folder,
    },
    /// A folder and every photo in it.
    RemoveFolder {
        id: FolderId,
    },
    /// New photos in existing folders (new ids, names free in their folder).
    AddPhotos {
        photos: Vec<Photo>,
    },
    RemovePhotos {
        ids: Vec<PhotoId>,
    },
    /// New values for existing photos (same folder; a new name must be free).
    UpdatePhotos {
        photos: Vec<Photo>,
    },
    /// Stars, applied in order.
    SetStars {
        stars: Vec<(PhotoId, bool)>,
    },
    /// Sketches (`None` clears one), applied in order. Derived data: see [`Op::is_derived`].
    SetSketches {
        sketches: Vec<(PhotoId, Option<Sketch>)>,
    },
    /// Captions ("" clears one), applied in order; each at most [`crate::MAX_CAPTION`] bytes.
    SetCaptions {
        captions: Vec<(PhotoId, String)>,
    },
    /// `tags` added to each of `ids` that lacks them (ignoring case), appended in order. A tag
    /// already in use anywhere takes the spelling in use. Each tag must be clean
    /// ([`crate::clean_tag`]).
    AddTags {
        ids: Vec<PhotoId>,
        tags: Vec<String>,
    },
    /// `tags` (ignoring case) removed from each of `ids`.
    RemoveTags {
        ids: Vec<PhotoId>,
        tags: Vec<String>,
    },
    /// Tag `from` renamed to `to` on every photo (ignoring case). A photo that already holds `to`
    /// just loses `from` (the tags merge, keeping the spelling in use); when `to` differs from
    /// `from` in case only, every photo takes the new spelling.
    RenameTag {
        from: String,
        to: String,
    },
    /// Whole tag lists, applied in order (clean tags, no two the same ignoring case).
    SetTags {
        tags: Vec<(PhotoId, Vec<String>)>,
    },
    /// Hidden flags of photos, applied in order.
    SetHidden {
        hidden: Vec<(PhotoId, bool)>,
    },
    /// Edit recipes with their redo lists, applied in order.
    SetRecipes {
        recipes: Vec<(PhotoId, Recipe)>,
    },
    /// Dates set by the user (`None` goes back to the file's date), applied in order.
    SetDates {
        dates: Vec<(PhotoId, Option<DateTime>)>,
    },
    /// Everything the user said about photos at once (rebuilds, and the inverse of removing
    /// photos), applied in order.
    SetPhotoInfo {
        photos: Vec<(PhotoId, PhotoInfo)>,
    },
    /// Everything the user said about folders (Folder Properties), applied in order.
    SetFolderInfo {
        folders: Vec<(FolderId, FolderInfo)>,
    },
    /// Hidden flags of folders (Hide Folder), applied in order.
    SetFoldersHidden {
        hidden: Vec<(FolderId, bool)>,
    },
    /// Last-synced metadata hashes (`None` clears them), applied in order. Derived data: see
    /// [`Op::is_derived`].
    SetSynced {
        synced: Vec<(PhotoId, Option<Synced>)>,
    },
    /// Saved records (`None` clears one), applied in order. Derived data (the record lives in
    /// `.dalica-originals/.dalica.json`): see [`Op::is_derived`].
    SetSaved {
        saved: Vec<(PhotoId, Option<Saved>)>,
    },
    /// Copies saved beside photos (`None` clears one), applied in order.
    SetSavedAs {
        #[serde(rename = "savedAs")]
        saved_as: Vec<(PhotoId, Option<SavedAs>)>,
    },
    /// Project marks of folders (`None` clears one), applied in order.
    SetProjects {
        projects: Vec<(FolderId, Option<Project>)>,
    },
    /// Photos moved to another folder and/or renamed, all at once: (photo, its folder, its file
    /// name) afterwards. A photo keeps its id and everything said about it. Each name must be one
    /// plain path component and free in its folder afterwards (photos of the same op may swap
    /// names).
    MovePhotos {
        moves: Vec<(PhotoId, FolderId, String)>,
    },
    /// Folder `id` renamed or moved to `path`, with every library folder under its old path
    /// (ids kept). Refused when `path` lies inside the folder or the folder inside `path`, or when
    /// a library folder that is not moved already lies at or under `path`.
    MoveFolder {
        id: FolderId,
        path: PathBuf,
    },
    /// Photo sorts of folders (Sort Folder By; the default clears one), applied in order.
    SetFolderSorts {
        sorts: Vec<(FolderId, PhotoSort)>,
    },
    /// New user albums (new ids, no members yet).
    CreateAlbums {
        albums: Vec<(AlbumId, AlbumInfo)>,
    },
    /// User albums deleted with every membership and cover; the photos stay.
    DeleteAlbums {
        ids: Vec<AlbumId>,
    },
    /// Album Properties, applied in order.
    SetAlbumInfo {
        albums: Vec<(AlbumId, AlbumInfo)>,
    },
    /// Photos added to albums at the given position keys (each not yet a member).
    AddMembers {
        members: Vec<(AlbumId, PhotoId, PosKey)>,
    },
    /// Photos removed from albums (each a member). A cover that leaves its album is cleared, so
    /// the album shows its first photo again.
    RemoveMembers {
        members: Vec<(AlbumId, PhotoId)>,
    },
    /// New position keys for members (a reorder, Sort Album By, or a whole album re-keyed).
    SetMemberKeys {
        keys: Vec<(AlbumId, PhotoId, PosKey)>,
    },
    /// Album covers (`None` goes back to the first photo), applied in order; a cover must be a
    /// member of its album.
    SetCovers {
        covers: Vec<(AlbumId, Option<PhotoId>)>,
    },
    /// Search facts (the default clears them), applied in order. Derived data: see
    /// [`Op::is_derived`].
    SetFacts {
        facts: Vec<(PhotoId, Facts)>,
    },
    /// Content digests (`None` clears one), applied in order. Derived data.
    SetDigests {
        digests: Vec<(PhotoId, Option<ContentDigest>)>,
    },
    /// When this library first indexed photos (`None` clears one), applied in order. Derived data.
    SetFirstIndexed {
        indexed: Vec<(PhotoId, Option<Timestamp>)>,
    },
    /// When Dalica last changed what is said about photos (`None` clears one), applied in order.
    /// Derived data; [`Library::stamped`] adds it to the op of a change.
    SetLastChanged {
        changed: Vec<(PhotoId, Option<Timestamp>)>,
    },
    /// Whole face lists of photos (an empty list clears them), applied in order: at most
    /// [`crate::MAX_FACES`] each, every named face naming a person the model has.
    SetFaces {
        faces: Vec<(PhotoId, Vec<Face>)>,
    },
    /// The model's geotags (`None` forgets the model's word, so the file's GPS counts again),
    /// applied in order; points are stored at six decimals.
    SetGeo {
        geo: Vec<(PhotoId, Option<Geo>)>,
    },
    /// A new person (a new id; a clean name no other person holds, ignoring case).
    AddPerson {
        id: PersonId,
        person: Person,
    },
    /// A person's name and nickname (the name unique ignoring case; the thumbnail stays).
    RenamePerson {
        id: PersonId,
        name: String,
        nickname: String,
    },
    /// Every face of `from` becomes `into`'s, which keeps its name, and `from` goes.
    MergePeople {
        from: PersonId,
        into: PersonId,
    },
    /// A person deleted; their faces go back to Unnamed.
    DeletePerson {
        id: PersonId,
    },
    /// A person's thumbnail face (`None` goes back to their first face); it must be one of their
    /// faces.
    SetPersonThumbnail {
        id: PersonId,
        thumbnail: Option<FaceKey>,
    },
    /// Several ops as one: all or nothing, one revision.
    Batch {
        ops: Vec<Op>,
    },
}

impl Op {
    /// Whether the op changes only derived data (sketches, synced hashes, saved records, search
    /// facts, digests, first-indexed and last-changed times): [`Op::SetSketches`],
    /// [`Op::SetSynced`], [`Op::SetSaved`], [`Op::SetFacts`], [`Op::SetDigests`],
    /// [`Op::SetFirstIndexed`], [`Op::SetLastChanged`], or a non-empty batch of nothing else. Such
    /// an op bumps neither [`Library::revision`] nor any folder revision (one with sketches bumps
    /// [`Library::thumbnail_revision`]), and its inverse is derived too. It is journaled like any
    /// op but never goes on an undo stack: undo is for what the user did.
    pub fn is_derived(&self) -> bool {
        match self {
            Op::SetSketches { .. }
            | Op::SetSynced { .. }
            | Op::SetSaved { .. }
            | Op::SetFacts { .. }
            | Op::SetDigests { .. }
            | Op::SetFirstIndexed { .. }
            | Op::SetLastChanged { .. } => true,
            Op::Batch { ops } => !ops.is_empty() && ops.iter().all(Op::is_derived),
            _ => false,
        }
    }
}

/// What applying an op changed besides the model.
#[derive(Default)]
struct Effects {
    /// Folders whose content changed.
    touched: BTreeSet<FolderId>,
    /// Whether a sketch was set, cleared or removed with its photo.
    sketches: bool,
    /// Whether a [`Op::SetSketches`] ran (even an empty one).
    sketch_op: bool,
    /// Whether a first-indexed or last-changed time was set, cleared or removed with its photo
    /// (Recently Updated reads them, so its cache goes even when the op is derived).
    times: bool,
}

impl Library {
    /// Applies `op` completely or not at all and returns the op that undoes it; on error nothing
    /// changed. On success the revision goes up by one, or for a derived op
    /// ([`Op::is_derived`]) the thumbnail revision instead; any op that changes a sketch also bumps
    /// the thumbnail revision.
    pub fn apply(&mut self, op: Op) -> Result<Op> {
        let derived = op.is_derived();
        let mut fx = Effects::default();
        let inverse = self.apply_inner(op, &mut fx)?;
        if fx.sketches || (derived && fx.sketch_op) {
            self.thumbnail_revision += 1;
        }
        if !derived || fx.times {
            self.cache = crate::query::Cache::default();
        }
        if !derived {
            self.revision += 1;
            self.unsaved = std::sync::OnceLock::new();
            for folder in fx.touched {
                self.touch(folder);
            }
        }
        Ok(inverse)
    }

    fn apply_inner(&mut self, op: Op, fx: &mut Effects) -> Result<Op> {
        let touched = &mut fx.touched;
        match op {
            Op::AddFolder { folder } => {
                if self.model.folders.contains_key(&folder.id) {
                    return Err(Error::FolderExists(folder.id));
                }
                if let Some(&other) = self.by_path.get(&folder.path) {
                    return Err(Error::FolderPathExists(other));
                }
                touched.insert(folder.id);
                self.by_path.insert(folder.path.clone(), folder.id);
                let id = folder.id;
                self.model.folders.insert(id, folder);
                Ok(Op::RemoveFolder { id })
            }
            Op::RemoveFolder { id } => {
                let folder = self.model.folders.get(&id).cloned().ok_or(Error::UnknownFolder(id))?;
                let ids: Vec<PhotoId> = self.model.photos.values().filter(|p| p.folder == id).map(|p| p.id).collect();
                let mut ops = vec![Op::AddFolder { folder: folder.clone() }];
                ops.extend(self.remove_photos(&ids, fx).into_iter().filter(|op| !matches!(op, Op::AddPhotos { photos } if photos.is_empty())));
                self.by_path.remove(&folder.path);
                self.model.folders.remove(&id);
                if let Some(info) = self.model.folder_info.remove(&id) {
                    ops.insert(1, Op::SetFolderInfo { folders: vec![(id, info)] });
                }
                if let Some(project) = self.model.projects.remove(&id) {
                    ops.insert(1, Op::SetProjects { projects: vec![(id, Some(project))] });
                }
                if let Some(sort) = self.model.folder_sorts.remove(&id) {
                    ops.insert(1, Op::SetFolderSorts { sorts: vec![(id, sort)] });
                }
                fx.touched.insert(id);
                Ok(if ops.len() == 1 { ops.swap_remove(0) } else { Op::Batch { ops } })
            }
            Op::AddPhotos { photos } => {
                let mut ids = HashSet::new();
                let mut names = HashSet::new();
                for p in &photos {
                    if self.model.photos.contains_key(&p.id) || !ids.insert(p.id) {
                        return Err(Error::PhotoExists(p.id));
                    }
                    if !self.model.folders.contains_key(&p.folder) {
                        return Err(Error::UnknownFolder(p.folder));
                    }
                    if self.by_name.contains_key(&(p.folder, p.name.clone())) || !names.insert((p.folder, p.name.as_str())) {
                        return Err(Error::NameTaken { folder: p.folder, name: p.name.clone() });
                    }
                }
                let ids: Vec<PhotoId> = photos.iter().map(|p| p.id).collect();
                for p in photos {
                    touched.insert(p.folder);
                    self.insert_photo(p);
                }
                Ok(Op::RemovePhotos { ids })
            }
            Op::RemovePhotos { ids } => {
                let mut seen = HashSet::new();
                for id in &ids {
                    if !self.model.photos.contains_key(id) || !seen.insert(*id) {
                        return Err(Error::UnknownPhoto(*id));
                    }
                }
                let mut ops = self.remove_photos(&ids, fx);
                Ok(if ops.len() == 1 { ops.swap_remove(0) } else { Op::Batch { ops } })
            }
            Op::UpdatePhotos { photos } => {
                let mut seen = HashSet::new();
                for p in &photos {
                    let old = self.model.photos.get(&p.id).ok_or(Error::UnknownPhoto(p.id))?;
                    if !seen.insert(p.id) {
                        return Err(Error::PhotoExists(p.id));
                    }
                    if old.folder != p.folder {
                        return Err(Error::FolderChange(p.id));
                    }
                }
                // Names must be unique in each folder after the update.
                let updated: HashSet<PhotoId> = photos.iter().map(|p| p.id).collect();
                let mut names: HashSet<(FolderId, &str)> =
                    self.model.photos.values().filter(|p| !updated.contains(&p.id)).map(|p| (p.folder, p.name.as_str())).collect();
                for p in &photos {
                    if !names.insert((p.folder, p.name.as_str())) {
                        return Err(Error::NameTaken { folder: p.folder, name: p.name.clone() });
                    }
                }
                let old: Vec<Photo> = photos.iter().filter_map(|p| self.remove_photo(p.id)).collect();
                for p in photos {
                    touched.insert(p.folder);
                    self.insert_photo(p);
                }
                Ok(Op::UpdatePhotos { photos: old })
            }
            Op::SetStars { stars } => {
                if let Some((id, _)) = stars.iter().find(|(id, _)| !self.model.photos.contains_key(id)) {
                    return Err(Error::UnknownPhoto(*id));
                }
                let mut undo = Vec::with_capacity(stars.len());
                for (id, on) in stars {
                    if let Some(p) = self.model.photos.get_mut(&id) {
                        undo.push((id, p.star));
                        p.star = on;
                        touched.insert(p.folder);
                    }
                }
                undo.reverse();
                Ok(Op::SetStars { stars: undo })
            }
            Op::SetSketches { sketches } => {
                if let Some((id, _)) = sketches.iter().find(|(id, _)| !self.model.photos.contains_key(id)) {
                    return Err(Error::UnknownPhoto(*id));
                }
                let mut undo = Vec::with_capacity(sketches.len());
                for (id, sketch) in sketches {
                    let old = match sketch {
                        Some(s) => self.model.sketches.insert(id, s),
                        None => self.model.sketches.remove(&id),
                    };
                    undo.push((id, old));
                }
                fx.sketches |= !undo.is_empty();
                fx.sketch_op = true;
                undo.reverse();
                Ok(Op::SetSketches { sketches: undo })
            }
            Op::SetCaptions { captions } => {
                self.check_photos(captions.iter().map(|(id, _)| *id))?;
                for (_, c) in &captions {
                    tags::check_caption(c)?;
                }
                let undo = self.edit_infos(captions, touched, |info, c| std::mem::replace(&mut info.caption, c));
                Ok(Op::SetCaptions { captions: undo })
            }
            Op::SetTags { tags: lists } => {
                self.check_photos(lists.iter().map(|(id, _)| *id))?;
                for (_, list) in &lists {
                    tags::check_tags(list)?;
                }
                let undo = self.edit_infos(lists, touched, |info, list| std::mem::replace(&mut info.tags, list));
                Ok(Op::SetTags { tags: undo })
            }
            Op::AddTags { ids, tags: add } => {
                self.check_photos(ids.iter().copied())?;
                for t in &add {
                    tags::check_tag(t)?;
                }
                // One spelling per tag for the whole op: the one in use, else the one given.
                let mut seen = HashSet::new();
                let add: Vec<(String, String)> =
                    add.iter().filter(|t| seen.insert(fold(t))).map(|t| (fold(t), self.tags.spelling(t).unwrap_or(t).to_string())).collect();
                let lists = self.new_tag_lists(&ids, |old| {
                    let have: HashSet<String> = old.iter().map(|t| fold(t)).collect();
                    let mut list = old.to_vec();
                    list.extend(add.iter().filter(|(k, _)| !have.contains(k)).map(|(_, t)| t.clone()));
                    list
                })?;
                Ok(self.set_tag_lists(lists, touched))
            }
            Op::RemoveTags { ids, tags: remove } => {
                self.check_photos(ids.iter().copied())?;
                for t in &remove {
                    tags::check_tag(t)?;
                }
                let gone: HashSet<String> = remove.iter().map(|t| fold(t)).collect();
                let lists = self.new_tag_lists(&ids, |old| old.iter().filter(|t| !gone.contains(&fold(t))).cloned().collect())?;
                Ok(self.set_tag_lists(lists, touched))
            }
            Op::RenameTag { from, to } => {
                tags::check_tag(&from)?;
                tags::check_tag(&to)?;
                let (kf, kt) = (fold(&from), fold(&to));
                let respell = kf == kt;
                let target = if respell { to.clone() } else { self.tags.spelling(&to).unwrap_or(&to).to_string() };
                let ids: Vec<PhotoId> =
                    self.model.photo_info.iter().filter(|(_, i)| i.tags.iter().any(|t| fold(t) == kf)).map(|(id, _)| *id).collect();
                let lists = self.new_tag_lists(&ids, |old| {
                    let merge = !respell && old.iter().any(|t| fold(t) == kt);
                    old.iter()
                        .filter_map(|t| match fold(t) == kf {
                            false => Some(t.clone()),
                            true if merge => None,
                            true => Some(target.clone()),
                        })
                        .collect()
                })?;
                Ok(self.set_tag_lists(lists, touched))
            }
            Op::SetHidden { hidden } => {
                self.check_photos(hidden.iter().map(|(id, _)| *id))?;
                let undo = self.edit_infos(hidden, touched, |info, on| std::mem::replace(&mut info.hidden, on));
                Ok(Op::SetHidden { hidden: undo })
            }
            Op::SetRecipes { recipes } => {
                self.check_photos(recipes.iter().map(|(id, _)| *id))?;
                for (_, r) in &recipes {
                    tags::check_list(&r.edits)?;
                    tags::check_list(&r.redo)?;
                }
                let undo = self.edit_infos(recipes, touched, |info, r| {
                    let old = Recipe { edits: std::mem::take(&mut info.edits), redo: std::mem::take(&mut info.redo) };
                    info.edits = r.edits;
                    info.redo = r.redo;
                    old
                });
                Ok(Op::SetRecipes { recipes: undo })
            }
            Op::SetDates { dates } => {
                self.check_photos(dates.iter().map(|(id, _)| *id))?;
                let undo = self.edit_infos(dates, touched, |info, d| std::mem::replace(&mut info.date, d));
                Ok(Op::SetDates { dates: undo })
            }
            Op::SetPhotoInfo { photos } => {
                self.check_photos(photos.iter().map(|(id, _)| *id))?;
                for (_, info) in &photos {
                    tags::check_info(info)?;
                }
                let undo = self.edit_infos(photos, touched, std::mem::replace);
                Ok(Op::SetPhotoInfo { photos: undo })
            }
            Op::SetFolderInfo { folders } => {
                self.check_folders(folders.iter().map(|(id, _)| *id))?;
                for (_, info) in &folders {
                    tags::check_folder_info(info)?;
                }
                let undo = self.edit_folder_infos(folders, touched, std::mem::replace);
                Ok(Op::SetFolderInfo { folders: undo })
            }
            Op::SetFoldersHidden { hidden } => {
                self.check_folders(hidden.iter().map(|(id, _)| *id))?;
                let undo = self.edit_folder_infos(hidden, touched, |info, on| std::mem::replace(&mut info.hidden, on));
                Ok(Op::SetFoldersHidden { hidden: undo })
            }
            Op::SetSynced { synced } => {
                self.check_photos(synced.iter().map(|(id, _)| *id))?;
                let mut undo = Vec::with_capacity(synced.len());
                for (id, s) in synced {
                    let old = match s.filter(|s| *s != Synced::default()) {
                        Some(s) => self.model.synced.insert(id, s),
                        None => self.model.synced.remove(&id),
                    };
                    undo.push((id, old));
                }
                undo.reverse();
                Ok(Op::SetSynced { synced: undo })
            }
            Op::SetSaved { saved } => {
                self.check_photos(saved.iter().map(|(id, _)| *id))?;
                for s in saved.iter().filter_map(|(_, s)| s.as_ref()) {
                    s.check()?;
                }
                let mut undo = Vec::with_capacity(saved.len());
                for (id, s) in saved {
                    let old = match s {
                        Some(s) => self.model.saved.insert(id, s),
                        None => self.model.saved.remove(&id),
                    };
                    undo.push((id, old));
                }
                undo.reverse();
                Ok(Op::SetSaved { saved: undo })
            }
            Op::SetSavedAs { saved_as } => {
                self.check_photos(saved_as.iter().map(|(id, _)| *id))?;
                for s in saved_as.iter().filter_map(|(_, s)| s.as_ref()) {
                    s.check()?;
                }
                let mut undo = Vec::with_capacity(saved_as.len());
                for (id, s) in saved_as {
                    let Some(folder) = self.model.photos.get(&id).map(|p| p.folder) else { continue };
                    let old = match s {
                        Some(s) => self.model.saved_as.insert(id, s),
                        None => self.model.saved_as.remove(&id),
                    };
                    touched.insert(folder);
                    undo.push((id, old));
                }
                undo.reverse();
                Ok(Op::SetSavedAs { saved_as: undo })
            }
            Op::SetProjects { projects } => {
                self.check_folders(projects.iter().map(|(id, _)| *id))?;
                let mut undo = Vec::with_capacity(projects.len());
                for (id, p) in projects {
                    let old = match p {
                        Some(p) => self.model.projects.insert(id, p),
                        None => self.model.projects.remove(&id),
                    };
                    touched.insert(id);
                    undo.push((id, old));
                }
                undo.reverse();
                Ok(Op::SetProjects { projects: undo })
            }
            Op::MovePhotos { moves } => self.move_photos(moves, touched),
            Op::MoveFolder { id, path } => self.move_folder(id, path, touched),
            Op::SetFolderSorts { sorts } => {
                self.check_folders(sorts.iter().map(|(id, _)| *id))?;
                let mut undo = Vec::with_capacity(sorts.len());
                for (id, sort) in sorts {
                    let old = if sort.is_default() { self.model.folder_sorts.remove(&id) } else { self.model.folder_sorts.insert(id, sort) };
                    touched.insert(id);
                    undo.push((id, old.unwrap_or_default()));
                }
                undo.reverse();
                Ok(Op::SetFolderSorts { sorts: undo })
            }
            Op::CreateAlbums { albums } => {
                let mut seen = HashSet::with_capacity(albums.len());
                for (id, info) in &albums {
                    if self.model.albums.contains_key(id) || !seen.insert(id) {
                        return Err(Error::AlbumExists(id.clone()));
                    }
                    info.check()?;
                }
                let ids = albums.iter().map(|(id, _)| id.clone()).collect();
                self.model.albums.extend(albums);
                Ok(Op::DeleteAlbums { ids })
            }
            Op::DeleteAlbums { ids } => {
                let mut seen = HashSet::with_capacity(ids.len());
                for id in &ids {
                    if !self.model.albums.contains_key(id) || !seen.insert(id) {
                        return Err(Error::UnknownAlbum(id.clone()));
                    }
                }
                let (mut infos, mut members, mut covers) = (Vec::new(), Vec::new(), Vec::new());
                for id in ids {
                    if let Some(cover) = self.model.covers.remove(&id) {
                        covers.push((id.clone(), Some(cover)));
                    }
                    self.by_key.remove(&id);
                    for (photo, key) in self.model.members.remove(&id).unwrap_or_default() {
                        touched.extend(self.model.photos.get(&photo).map(|p| p.folder));
                        members.push((id.clone(), photo, key));
                    }
                    if let Some(info) = self.model.albums.remove(&id) {
                        infos.push((id, info));
                    }
                }
                let mut ops = vec![Op::CreateAlbums { albums: infos }];
                if !members.is_empty() {
                    ops.push(Op::AddMembers { members });
                }
                if !covers.is_empty() {
                    ops.push(Op::SetCovers { covers });
                }
                Ok(if ops.len() == 1 { ops.swap_remove(0) } else { Op::Batch { ops } })
            }
            Op::SetAlbumInfo { albums } => {
                for (id, info) in &albums {
                    if !self.model.albums.contains_key(id) {
                        return Err(Error::UnknownAlbum(id.clone()));
                    }
                    info.check()?;
                }
                let mut undo = Vec::with_capacity(albums.len());
                for (id, info) in albums {
                    // The name is mirrored in every member folder's sidecar.
                    touched.extend(self.member_folders(&id));
                    if let Some(old) = self.model.albums.insert(id.clone(), info) {
                        undo.push((id, old));
                    }
                }
                undo.reverse();
                Ok(Op::SetAlbumInfo { albums: undo })
            }
            Op::AddMembers { members } => {
                let mut seen = HashSet::with_capacity(members.len());
                for (album, photo, _) in &members {
                    if !self.model.albums.contains_key(album) {
                        return Err(Error::UnknownAlbum(album.clone()));
                    }
                    if !self.model.photos.contains_key(photo) {
                        return Err(Error::UnknownPhoto(*photo));
                    }
                    if self.member_key(album, *photo).is_some() || !seen.insert((album, *photo)) {
                        return Err(Error::AlreadyMember { album: album.clone(), photo: *photo });
                    }
                }
                let mut undo = Vec::with_capacity(members.len());
                for (album, photo, key) in members {
                    touched.extend(self.model.photos.get(&photo).map(|p| p.folder));
                    self.by_key.entry(album.clone()).or_default().insert((key.clone(), photo));
                    self.model.members.entry(album.clone()).or_default().insert(photo, key);
                    undo.push((album, photo));
                }
                undo.reverse();
                Ok(Op::RemoveMembers { members: undo })
            }
            Op::RemoveMembers { members } => {
                self.check_members(members.iter().map(|(a, p)| (a, *p)))?;
                let mut undo = Vec::with_capacity(members.len());
                let mut covers = Vec::new();
                for (album, photo) in members {
                    if self.model.covers.get(&album) == Some(&photo) {
                        self.model.covers.remove(&album);
                        covers.push((album.clone(), Some(photo)));
                    }
                    if let Some(key) = self.take_member(&album, photo) {
                        touched.extend(self.model.photos.get(&photo).map(|p| p.folder));
                        undo.push((album, photo, key));
                    }
                }
                undo.reverse();
                let add = Op::AddMembers { members: undo };
                Ok(if covers.is_empty() { add } else { Op::Batch { ops: vec![add, Op::SetCovers { covers }] } })
            }
            Op::SetMemberKeys { keys } => {
                self.check_members(keys.iter().map(|(a, p, _)| (a, *p)))?;
                let mut undo = Vec::with_capacity(keys.len());
                for (album, photo, key) in keys {
                    let old = self.model.members.get_mut(&album).and_then(|m| m.insert(photo, key.clone()));
                    if let (Some(index), Some(old)) = (self.by_key.get_mut(&album), &old) {
                        index.remove(&(old.clone(), photo));
                        index.insert((key, photo));
                    }
                    touched.extend(self.model.photos.get(&photo).map(|p| p.folder));
                    if let Some(old) = old {
                        undo.push((album, photo, old));
                    }
                }
                undo.reverse();
                Ok(Op::SetMemberKeys { keys: undo })
            }
            Op::SetCovers { covers } => {
                for (album, cover) in &covers {
                    if !self.model.albums.contains_key(album) {
                        return Err(Error::UnknownAlbum(album.clone()));
                    }
                    if let Some(photo) = cover
                        && self.member_key(album, *photo).is_none()
                    {
                        return Err(Error::NotMember { album: album.clone(), photo: *photo });
                    }
                }
                let mut undo = Vec::with_capacity(covers.len());
                for (album, cover) in covers {
                    let old = match cover {
                        Some(p) => self.model.covers.insert(album.clone(), p),
                        None => self.model.covers.remove(&album),
                    };
                    undo.push((album, old));
                }
                undo.reverse();
                Ok(Op::SetCovers { covers: undo })
            }
            Op::SetFacts { facts } => {
                self.check_photos(facts.iter().map(|(id, _)| *id))?;
                for (_, f) in &facts {
                    f.check()?;
                }
                let mut undo = Vec::with_capacity(facts.len());
                for (id, f) in facts {
                    let old = if f.is_empty() { self.model.facts.remove(&id) } else { self.model.facts.insert(id, f) };
                    undo.push((id, old.unwrap_or_default()));
                }
                undo.reverse();
                Ok(Op::SetFacts { facts: undo })
            }
            Op::SetDigests { digests } => {
                self.check_photos(digests.iter().map(|(id, _)| *id))?;
                for d in digests.iter().filter_map(|(_, d)| d.as_ref()) {
                    d.check()?;
                }
                let undo = set_all(&mut self.model.digests, digests);
                Ok(Op::SetDigests { digests: undo })
            }
            Op::SetFirstIndexed { indexed } => {
                self.check_photos(indexed.iter().map(|(id, _)| *id))?;
                fx.times = true;
                Ok(Op::SetFirstIndexed { indexed: set_all(&mut self.model.indexed, indexed) })
            }
            Op::SetLastChanged { changed } => {
                self.check_photos(changed.iter().map(|(id, _)| *id))?;
                fx.times = true;
                Ok(Op::SetLastChanged { changed: set_all(&mut self.model.changed, changed) })
            }
            Op::SetFaces { faces } => self.set_faces(faces, touched),
            Op::SetGeo { geo } => self.set_geo(geo, touched),
            Op::AddPerson { id, person } => self.add_person(id, person),
            Op::RenamePerson { id, name, nickname } => self.rename_person(id, name, nickname, touched),
            Op::MergePeople { from, into } => self.merge_people(from, into, touched),
            Op::DeletePerson { id } => self.delete_person(id, touched),
            Op::SetPersonThumbnail { id, thumbnail } => self.set_person_thumbnail(id, thumbnail),
            Op::Batch { ops } => {
                let mut inverses = Vec::with_capacity(ops.len());
                for op in ops {
                    match self.apply_inner(op, fx) {
                        Ok(inv) => inverses.push(inv),
                        Err(e) => {
                            // Undo what this batch did so far; those inverses were just made from
                            // the current model, so they apply.
                            for inv in inverses.into_iter().rev() {
                                let _ = self.apply_inner(inv, &mut Effects::default());
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                Ok(Op::Batch { ops: inverses })
            }
        }
    }

    /// Removes the photos `ids` (all known, no repeats) with their info, synced hashes, saved
    /// records, copies saved beside, sketches, search facts, digests, times, album memberships and
    /// the covers they were, faces and geotags, and returns the ops that put them back, in order:
    /// [`Op::AddPhotos`] first, then only what there was (memberships before covers).
    fn remove_photos(&mut self, ids: &[PhotoId], fx: &mut Effects) -> Vec<Op> {
        let sketches: Vec<(PhotoId, Option<Sketch>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.sketches.remove(id)?)))).collect();
        let synced: Vec<(PhotoId, Option<Synced>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.synced.remove(id)?)))).collect();
        let saved: Vec<(PhotoId, Option<Saved>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.saved.remove(id)?)))).collect();
        let saved_as: Vec<(PhotoId, Option<SavedAs>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.saved_as.remove(id)?)))).collect();
        let facts: Vec<(PhotoId, Facts)> = ids.iter().filter_map(|id| Some((*id, self.model.facts.remove(id)?))).collect();
        let digests: Vec<(PhotoId, Option<ContentDigest>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.digests.remove(id)?)))).collect();
        let indexed: Vec<(PhotoId, Option<Timestamp>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.indexed.remove(id)?)))).collect();
        let changed: Vec<(PhotoId, Option<Timestamp>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.changed.remove(id)?)))).collect();
        fx.times |= !indexed.is_empty() || !changed.is_empty();
        let faces: Vec<(PhotoId, Vec<Face>)> = ids.iter().filter_map(|id| Some((*id, self.model.faces.remove(id)?))).collect();
        let geo: Vec<(PhotoId, Option<Geo>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.geo.remove(id)?)))).collect();
        let gone: HashSet<PhotoId> = ids.iter().copied().collect();
        let mut covers = Vec::new();
        self.model.covers.retain(|album, photo| {
            let keep = !gone.contains(photo);
            if !keep {
                covers.push((album.clone(), Some(*photo)));
            }
            keep
        });
        let mut members = Vec::new();
        for (album, m) in &mut self.model.members {
            m.retain(|photo, key| {
                let keep = !gone.contains(photo);
                if !keep {
                    members.push((album.clone(), *photo, key.clone()));
                }
                keep
            });
        }
        self.model.members.retain(|_, m| !m.is_empty());
        for (album, photo, key) in &members {
            if let Some(index) = self.by_key.get_mut(album) {
                index.remove(&(key.clone(), *photo));
            }
        }
        self.by_key.retain(|_, index| !index.is_empty());
        let infos: Vec<(PhotoId, PhotoInfo)> = ids.iter().filter_map(|id| Some((*id, self.put_info(*id, PhotoInfo::default())?))).collect();
        let photos: Vec<Photo> = ids.iter().filter_map(|id| self.remove_photo(*id)).collect();
        fx.touched.extend(photos.iter().map(|p| p.folder));
        let mut ops = Vec::with_capacity(14);
        ops.push(Op::AddPhotos { photos });
        if !facts.is_empty() {
            ops.push(Op::SetFacts { facts });
        }
        if !digests.is_empty() {
            ops.push(Op::SetDigests { digests });
        }
        if !indexed.is_empty() {
            ops.push(Op::SetFirstIndexed { indexed });
        }
        if !changed.is_empty() {
            ops.push(Op::SetLastChanged { changed });
        }
        if !members.is_empty() {
            ops.push(Op::AddMembers { members });
        }
        if !covers.is_empty() {
            ops.push(Op::SetCovers { covers });
        }
        if !faces.is_empty() {
            ops.push(Op::SetFaces { faces });
        }
        if !geo.is_empty() {
            ops.push(Op::SetGeo { geo });
        }
        if !infos.is_empty() {
            ops.push(Op::SetPhotoInfo { photos: infos });
        }
        if !synced.is_empty() {
            ops.push(Op::SetSynced { synced });
        }
        if !saved.is_empty() {
            ops.push(Op::SetSaved { saved });
        }
        if !saved_as.is_empty() {
            ops.push(Op::SetSavedAs { saved_as });
        }
        if !sketches.is_empty() {
            fx.sketches = true;
            ops.push(Op::SetSketches { sketches });
        }
        ops
    }

    /// Replaces photo `id`'s info (dropping an empty one) and keeps the tag index in step; returns
    /// the old info if there was one.
    fn put_info(&mut self, id: PhotoId, info: PhotoInfo) -> Option<PhotoInfo> {
        let old = self.model.photo_info.remove(&id);
        if let Some(o) = &old {
            self.tags.remove(&o.tags);
        }
        if !info.is_empty() {
            self.tags.add(&info.tags);
            self.model.photo_info.insert(id, info);
        }
        old
    }

    fn check_photos(&self, ids: impl Iterator<Item = PhotoId>) -> Result<()> {
        for id in ids {
            if !self.model.photos.contains_key(&id) {
                return Err(Error::UnknownPhoto(id));
            }
        }
        Ok(())
    }

    fn check_folders(&self, ids: impl Iterator<Item = FolderId>) -> Result<()> {
        for id in ids {
            if !self.model.folders.contains_key(&id) {
                return Err(Error::UnknownFolder(id));
            }
        }
        Ok(())
    }

    /// Sets one field of each photo's info in order (`set` stores the new value and returns the
    /// old one); returns the entries that undo it. Every id must be known.
    fn edit_infos<T>(
        &mut self,
        items: Vec<(PhotoId, T)>,
        touched: &mut BTreeSet<FolderId>,
        set: impl Fn(&mut PhotoInfo, T) -> T,
    ) -> Vec<(PhotoId, T)> {
        let mut undo = Vec::with_capacity(items.len());
        for (id, value) in items {
            let Some(folder) = self.model.photos.get(&id).map(|p| p.folder) else { continue };
            let mut info = self.model.photo_info.get(&id).cloned().unwrap_or_default();
            let old = set(&mut info, value);
            self.put_info(id, info);
            touched.insert(folder);
            undo.push((id, old));
        }
        undo.reverse();
        undo
    }

    /// [`Library::edit_infos`] for folders. Every id must be known.
    fn edit_folder_infos<T>(
        &mut self,
        items: Vec<(FolderId, T)>,
        touched: &mut BTreeSet<FolderId>,
        set: impl Fn(&mut FolderInfo, T) -> T,
    ) -> Vec<(FolderId, T)> {
        let mut undo = Vec::with_capacity(items.len());
        for (id, value) in items {
            let mut info = self.model.folder_info.remove(&id).unwrap_or_default();
            let old = set(&mut info, value);
            if !info.is_empty() {
                self.model.folder_info.insert(id, info);
            }
            touched.insert(id);
            undo.push((id, old));
        }
        undo.reverse();
        undo
    }

    /// The new tag list of each of `ids` (repeats ignored) that `change` alters, checked against
    /// [`crate::MAX_TAGS`] before anything changes.
    fn new_tag_lists(&self, ids: &[PhotoId], change: impl Fn(&[String]) -> Vec<String>) -> Result<Vec<(PhotoId, Vec<String>)>> {
        let mut seen = HashSet::with_capacity(ids.len());
        let mut out = Vec::new();
        for &id in ids {
            if !seen.insert(id) {
                continue;
            }
            let old = self.tags_of(id);
            let list = change(old);
            if list.as_slice() != old {
                tags::check_tags(&list)?;
                out.push((id, list));
            }
        }
        Ok(out)
    }

    /// Applies checked tag lists; returns the [`Op::SetTags`] that undoes them.
    fn set_tag_lists(&mut self, lists: Vec<(PhotoId, Vec<String>)>, touched: &mut BTreeSet<FolderId>) -> Op {
        Op::SetTags { tags: self.edit_infos(lists, touched, |info, list| std::mem::replace(&mut info.tags, list)) }
    }

    fn insert_photo(&mut self, p: Photo) {
        self.by_name.insert((p.folder, p.name.clone()), p.id);
        self.model.photos.insert(p.id, p);
    }

    fn remove_photo(&mut self, id: PhotoId) -> Option<Photo> {
        let p = self.model.photos.remove(&id)?;
        self.by_name.remove(&(p.folder, p.name.clone()));
        Some(p)
    }

    /// [`Op::MovePhotos`].
    fn move_photos(&mut self, moves: Vec<(PhotoId, FolderId, String)>, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        let mut moving = HashSet::with_capacity(moves.len());
        for (id, folder, name) in &moves {
            if !self.model.photos.contains_key(id) {
                return Err(Error::UnknownPhoto(*id));
            }
            if !moving.insert(*id) {
                return Err(Error::PhotoExists(*id));
            }
            if !self.model.folders.contains_key(folder) {
                return Err(Error::UnknownFolder(*folder));
            }
            check_file_name(name)?;
        }
        // A place is free when no photo holds it, or the one holding it moves too (a swap).
        let mut targets = HashSet::with_capacity(moves.len());
        for (_, folder, name) in &moves {
            let held = self.by_name.get(&(*folder, name.clone())).is_some_and(|other| !moving.contains(other));
            if held || !targets.insert((*folder, name.as_str())) {
                return Err(Error::NameTaken { folder: *folder, name: name.clone() });
            }
        }
        let mut undo = Vec::with_capacity(moves.len());
        for (id, _, _) in &moves {
            if let Some(p) = self.model.photos.get(id) {
                self.by_name.remove(&(p.folder, p.name.clone()));
            }
        }
        for (id, folder, name) in moves {
            let Some(p) = self.model.photos.get_mut(&id) else { continue };
            touched.insert(p.folder);
            touched.insert(folder);
            let old_folder = std::mem::replace(&mut p.folder, folder);
            let old_name = std::mem::replace(&mut p.name, name.clone());
            self.by_name.insert((folder, name), id);
            undo.push((id, old_folder, old_name));
        }
        undo.reverse();
        Ok(Op::MovePhotos { moves: undo })
    }

    /// [`Op::MoveFolder`].
    fn move_folder(&mut self, id: FolderId, path: PathBuf, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        let old = self.model.folders.get(&id).map(|f| f.path.clone()).ok_or(Error::UnknownFolder(id))?;
        if path.as_os_str().is_empty() {
            return Err(Error::BadText { what: "folder path", why: "empty" });
        }
        // Into itself, or onto the path of a folder it is inside: impossible on disk, and the
        // inverse of the second would be the first.
        if path != old && (path.starts_with(&old) || old.starts_with(&path)) {
            return Err(Error::FolderIntoItself(id));
        }
        let mut moved = Vec::new();
        for f in self.model.folders.values() {
            if let Ok(rest) = f.path.strip_prefix(&old) {
                let to = if rest.as_os_str().is_empty() { path.clone() } else { path.join(rest) };
                moved.push((f.id, f.path.clone(), to));
            } else if f.path.starts_with(&path) {
                // Moving back would take this folder along: refused, so the inverse is exact.
                return Err(Error::FolderPathExists(f.id));
            }
        }
        for (_, from, _) in &moved {
            self.by_path.remove(from);
        }
        for (fid, _, to) in moved {
            if let Some(f) = self.model.folders.get_mut(&fid) {
                f.path = to.clone();
            }
            self.by_path.insert(to, fid);
            touched.insert(fid);
        }
        Ok(Op::MoveFolder { id, path: old })
    }

    /// The folders of album `id`'s members.
    fn member_folders(&self, id: &AlbumId) -> BTreeSet<FolderId> {
        let members = self.model.members.get(id).into_iter().flat_map(BTreeMap::keys);
        members.filter_map(|p| self.model.photos.get(p).map(|p| p.folder)).collect()
    }

    /// Checks that every (album, photo) pair is a membership, each once.
    fn check_members<'a>(&self, pairs: impl Iterator<Item = (&'a AlbumId, PhotoId)>) -> Result<()> {
        let mut seen = HashSet::new();
        for (album, photo) in pairs {
            if !self.model.albums.contains_key(album) {
                return Err(Error::UnknownAlbum(album.clone()));
            }
            if self.member_key(album, photo).is_none() || !seen.insert((album, photo)) {
                return Err(Error::NotMember { album: album.clone(), photo });
            }
        }
        Ok(())
    }

    /// Removes one membership (dropping an album's map once it is empty); returns its key.
    fn take_member(&mut self, album: &AlbumId, photo: PhotoId) -> Option<PosKey> {
        let m = self.model.members.get_mut(album)?;
        let key = m.remove(&photo);
        if m.is_empty() {
            self.model.members.remove(album);
        }
        if let (Some(index), Some(key)) = (self.by_key.get_mut(album), &key) {
            index.remove(&(key.clone(), photo));
            if index.is_empty() {
                self.by_key.remove(album);
            }
        }
        key
    }
}

/// Sets or clears each entry of `map` in order; returns the entries that undo it.
fn set_all<T>(map: &mut BTreeMap<PhotoId, T>, items: Vec<(PhotoId, Option<T>)>) -> Vec<(PhotoId, Option<T>)> {
    let mut undo = Vec::with_capacity(items.len());
    for (id, value) in items {
        let old = match value {
            Some(v) => map.insert(id, v),
            None => map.remove(&id),
        };
        undo.push((id, old));
    }
    undo.reverse();
    undo
}

/// Checks a file name an op gives a photo: one plain path component (`security.md` §5.4) of at
/// most [`MAX_FILE_NAME`] bytes.
fn check_file_name(name: &str) -> Result<()> {
    if name.len() > MAX_FILE_NAME {
        return Err(Error::TooLong { what: "file name", max: MAX_FILE_NAME });
    }
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(Error::BadText { what: "file name", why: "not one plain path component" });
    }
    Ok(())
}

/// The last-changed stamp (M9): which photos an op changes what is said about.
impl Library {
    /// `op` with the last-changed time of every photo whose data it changes (star, caption, tags,
    /// hidden, recipe, date, copy saved beside, album memberships and keys, moves and renames,
    /// faces, including those of a person merged away or deleted, and geotags)
    /// set to `at`, as one batch, so the journal replays the same times and the inverse puts the
    /// old ones back exactly. Photos the op removes are left out; an op that changes no photo's
    /// data (scans, folder info, derived data) comes back as it was. The engine stamps the ops of
    /// commands with its clock (Recently Updated, `organizing.md` §4.1).
    pub fn stamped(&self, op: Op, at: Timestamp) -> Op {
        let ids = self.changed_by(&op);
        if ids.is_empty() {
            return op;
        }
        let changed = Op::SetLastChanged { changed: ids.into_iter().map(|id| (id, Some(at))).collect() };
        Op::Batch { ops: vec![op, changed] }
    }

    /// The photos whose data `op` changes that exist once it applied. Walks nested batches without
    /// recursion.
    fn changed_by(&self, op: &Op) -> BTreeSet<PhotoId> {
        let (mut ids, mut added, mut removed) = (BTreeSet::new(), HashSet::new(), HashSet::new());
        let mut stack = vec![op];
        while let Some(op) = stack.pop() {
            match op {
                Op::SetStars { stars } => ids.extend(stars.iter().map(|(id, _)| *id)),
                Op::SetCaptions { captions } => ids.extend(captions.iter().map(|(id, _)| *id)),
                Op::AddTags { ids: list, .. } | Op::RemoveTags { ids: list, .. } => ids.extend(list.iter().copied()),
                Op::RenameTag { from, .. } => {
                    let from = fold(from);
                    ids.extend(self.model.photo_info.iter().filter(|(_, i)| i.tags.iter().any(|t| fold(t) == from)).map(|(id, _)| *id));
                }
                Op::SetTags { tags } => ids.extend(tags.iter().map(|(id, _)| *id)),
                Op::SetHidden { hidden } => ids.extend(hidden.iter().map(|(id, _)| *id)),
                Op::SetRecipes { recipes } => ids.extend(recipes.iter().map(|(id, _)| *id)),
                Op::SetDates { dates } => ids.extend(dates.iter().map(|(id, _)| *id)),
                Op::SetPhotoInfo { photos } => ids.extend(photos.iter().map(|(id, _)| *id)),
                Op::SetSavedAs { saved_as } => ids.extend(saved_as.iter().map(|(id, _)| *id)),
                Op::MovePhotos { moves } => ids.extend(moves.iter().map(|(id, _, _)| *id)),
                Op::AddMembers { members } | Op::SetMemberKeys { keys: members } => ids.extend(members.iter().map(|(_, id, _)| *id)),
                Op::RemoveMembers { members } => ids.extend(members.iter().map(|(_, id)| *id)),
                Op::DeleteAlbums { ids: albums } => {
                    ids.extend(albums.iter().filter_map(|a| self.model.members.get(a)).flat_map(|m| m.keys().copied()));
                }
                Op::SetFaces { faces } => ids.extend(faces.iter().map(|(id, _)| *id)),
                Op::SetGeo { geo } => ids.extend(geo.iter().map(|(id, _)| *id)),
                Op::MergePeople { from: id, .. } | Op::DeletePerson { id } => ids.extend(self.photos_of_person(id)),
                Op::AddPhotos { photos } => added.extend(photos.iter().map(|p| p.id)),
                Op::RemovePhotos { ids: list } => removed.extend(list.iter().copied()),
                Op::RemoveFolder { id } => removed.extend(self.model.photos.values().filter(|p| p.folder == *id).map(|p| p.id)),
                Op::Batch { ops } => stack.extend(ops),
                _ => {}
            }
        }
        ids.retain(|id| !removed.contains(id) && (added.contains(id) || self.model.photos.contains_key(id)));
        ids
    }

    /// The derived op that records `at` as the first-indexed time of each of `ids` that has none
    /// yet (unknown photos and repeats are left out): a scan applies it for the photos it adds.
    pub fn first_indexed_op(&self, ids: &[PhotoId], at: Timestamp) -> Op {
        let mut seen = HashSet::with_capacity(ids.len());
        let indexed = ids
            .iter()
            .filter(|id| self.model.photos.contains_key(id) && !self.model.indexed.contains_key(id) && seen.insert(**id))
            .map(|id| (*id, Some(at)))
            .collect();
        Op::SetFirstIndexed { indexed }
    }
}

/// The ops of M8's file changes (`data-model.md` §6), built from the current model so the caller
/// applies them once the files on disk say so. Each is one op (a batch where it changes two
/// things) whose inverse is exact.
impl Library {
    /// The op a finished in-place Save applies: each photo's record set as given (the whole recipe
    /// the saved file holds; [`crate::SaveState::Done`] once the file is committed) and its recipe
    /// and redo list cleared. A photo whose save was beside it takes [`Library::save_beside_op`]
    /// instead.
    pub fn save_op(&self, saves: Vec<(PhotoId, Saved)>) -> Result<Op> {
        self.check_photos(saves.iter().map(|(id, _)| *id))?;
        let recipes = saves.iter().map(|(id, _)| (*id, Recipe::default())).collect();
        let saved = saves.into_iter().map(|(id, s)| (id, Some(s))).collect();
        Ok(Op::Batch { ops: vec![Op::SetSaved { saved }, Op::SetRecipes { recipes }] })
    }

    /// The op Undo Save applies once the original is back: each photo's record cleared and the
    /// recorded recipe put back as unsaved edits in front of any edits made since the save (the
    /// redo list stays). Every photo must have a done record ([`Library::can_undo_save`]); repeats
    /// count once.
    pub fn undo_save_op(&self, ids: &[PhotoId]) -> Result<Op> {
        let ids = self.saved_photos(ids)?;
        let mut recipes = Vec::with_capacity(ids.len());
        for &id in &ids {
            let info = self.info(id);
            let mut edits = self.saved(id).map(|s| s.recipe.clone()).unwrap_or_default();
            edits.extend(info.edits.iter().cloned());
            recipes.push((id, Recipe { edits, redo: info.redo.clone() }));
        }
        let saved = ids.iter().map(|&id| (id, None)).collect();
        Ok(Op::Batch { ops: vec![Op::SetSaved { saved }, Op::SetRecipes { recipes }] })
    }

    /// The op Revert applies once the original is back: each photo's record, recipe and redo list
    /// cleared. Every photo must have a done record ([`Library::can_revert`]); repeats count once.
    pub fn revert_op(&self, ids: &[PhotoId]) -> Result<Op> {
        let ids = self.saved_photos(ids)?;
        let recipes = ids.iter().map(|&id| (id, Recipe::default())).collect();
        let saved = ids.iter().map(|&id| (id, None)).collect();
        Ok(Op::Batch { ops: vec![Op::SetSaved { saved }, Op::SetRecipes { recipes }] })
    }

    /// The op a Save beside the photo applies: the copy named `name` holds the photo's current
    /// recipe, which stays (so the photo no longer counts as unsaved until its recipe changes).
    /// The caller adds the copy itself as a new photo in the same batch.
    pub fn save_beside_op(&self, id: PhotoId, name: String) -> Result<Op> {
        self.check_photos(std::iter::once(id))?;
        let saved_as = SavedAs { name, recipe: self.info(id).edits.clone() };
        saved_as.check()?;
        Ok(Op::SetSavedAs { saved_as: vec![(id, Some(saved_as))] })
    }

    /// `ids` without repeats, each known and with a done record.
    fn saved_photos(&self, ids: &[PhotoId]) -> Result<Vec<PhotoId>> {
        let mut seen = HashSet::with_capacity(ids.len());
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            if !self.model.photos.contains_key(&id) {
                return Err(Error::UnknownPhoto(id));
            }
            if !self.saved(id).is_some_and(Saved::is_done) {
                return Err(Error::NotSaved(id));
            }
            if seen.insert(id) {
                out.push(id);
            }
        }
        Ok(out)
    }
}
