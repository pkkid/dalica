//! Ops: the only way the model changes (`architecture.md` §4). Applying an op returns its inverse;
//! applying the inverse restores the model exactly.
//!
//! [`Op::SetSketches`] is the one op on derived data: it bumps the thumbnail revision instead of
//! the library revision, touches no folder, and is journaled but never put on an undo stack
//! ([`Op::is_derived`]). Removing a photo drops its sketch; the inverse puts it back.

use std::collections::{BTreeSet, HashSet};

use dalica_core::{FolderId, PhotoId};
use serde::{Deserialize, Serialize};

use crate::model::{Folder, Library, Photo, Sketch};
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
    /// Several ops as one: all or nothing, one revision.
    Batch {
        ops: Vec<Op>,
    },
}

impl Op {
    /// Whether the op changes only derived data (sketches): [`Op::SetSketches`], or a non-empty
    /// batch of nothing else. Such an op bumps [`Library::thumbnail_revision`] but not
    /// [`Library::revision`] or any folder revision, and its inverse is derived too. It is
    /// journaled like any op but never goes on an undo stack: undo is for what the user did.
    pub fn is_derived(&self) -> bool {
        match self {
            Op::SetSketches { .. } => true,
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
        if derived || fx.sketches {
            self.thumbnail_revision += 1;
        }
        if !derived {
            self.revision += 1;
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
                let sketches = self.remove_sketches(&ids);
                let photos: Vec<Photo> = ids.iter().filter_map(|pid| self.remove_photo(*pid)).collect();
                self.by_path.remove(&folder.path);
                self.model.folders.remove(&id);
                touched.insert(id);
                fx.sketches |= !sketches.is_empty();
                let mut ops = vec![Op::AddFolder { folder }];
                if !photos.is_empty() {
                    ops.push(Op::AddPhotos { photos });
                }
                if !sketches.is_empty() {
                    ops.push(Op::SetSketches { sketches });
                }
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
                let sketches = self.remove_sketches(&ids);
                let photos: Vec<Photo> = ids.iter().filter_map(|id| self.remove_photo(*id)).collect();
                touched.extend(photos.iter().map(|p| p.folder));
                let add = Op::AddPhotos { photos };
                if sketches.is_empty() {
                    return Ok(add);
                }
                fx.sketches = true;
                Ok(Op::Batch { ops: vec![add, Op::SetSketches { sketches }] })
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
                undo.reverse();
                Ok(Op::SetSketches { sketches: undo })
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

    /// Takes the sketches of `ids` out of the model, as the entries that put them back.
    fn remove_sketches(&mut self, ids: &[PhotoId]) -> Vec<(PhotoId, Option<Sketch>)> {
        ids.iter().filter_map(|id| Some((*id, Some(self.model.sketches.remove(id)?)))).collect()
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
