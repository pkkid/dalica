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

use std::collections::{BTreeSet, HashSet};

use dalica_core::{DateTime, FolderId, PhotoId};
use serde::{Deserialize, Serialize};

use crate::model::{Folder, FolderInfo, Library, Photo, PhotoInfo, Project, Recipe, Saved, SavedAs, Sketch, Synced};
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
    /// Several ops as one: all or nothing, one revision.
    Batch {
        ops: Vec<Op>,
    },
}

impl Op {
    /// Whether the op changes only derived data (sketches, synced hashes, saved records):
    /// [`Op::SetSketches`], [`Op::SetSynced`], [`Op::SetSaved`], or a non-empty batch of nothing
    /// else. Such an op bumps neither [`Library::revision`] nor any folder revision (one with
    /// sketches bumps [`Library::thumbnail_revision`]), and its inverse is derived too. It is
    /// journaled like any op but never goes on an undo stack: undo is for what the user did.
    pub fn is_derived(&self) -> bool {
        match self {
            Op::SetSketches { .. } | Op::SetSynced { .. } | Op::SetSaved { .. } => true,
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
    /// records, copies saved beside and sketches, and returns the ops that put them back, in
    /// order: [`Op::AddPhotos`] first, then only what there was.
    fn remove_photos(&mut self, ids: &[PhotoId], fx: &mut Effects) -> Vec<Op> {
        let sketches: Vec<(PhotoId, Option<Sketch>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.sketches.remove(id)?)))).collect();
        let synced: Vec<(PhotoId, Option<Synced>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.synced.remove(id)?)))).collect();
        let saved: Vec<(PhotoId, Option<Saved>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.saved.remove(id)?)))).collect();
        let saved_as: Vec<(PhotoId, Option<SavedAs>)> = ids.iter().filter_map(|id| Some((*id, Some(self.model.saved_as.remove(id)?)))).collect();
        let infos: Vec<(PhotoId, PhotoInfo)> = ids.iter().filter_map(|id| Some((*id, self.put_info(*id, PhotoInfo::default())?))).collect();
        let photos: Vec<Photo> = ids.iter().filter_map(|id| self.remove_photo(*id)).collect();
        fx.touched.extend(photos.iter().map(|p| p.folder));
        let mut ops = Vec::with_capacity(6);
        ops.push(Op::AddPhotos { photos });
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
