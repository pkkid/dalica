//! The verification pass: the ops that make the model match a [`Scan`], or the part of the model
//! an incremental [`Rescan`] covers.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo, PhotoInfo, Saved, SavedAs};

use crate::meta::{Sync, first_fill};
use crate::walk::{FoundFolder, FoundPhoto, Scan};
use crate::watch::Rescan;

/// The most photos in one `AddPhotos` / `UpdatePhotos` / `RemovePhotos` op.
pub const BATCH: usize = 500;

/// The ops that bring `library` in line with `scan`, in order: new and renamed folders, then per
/// folder removed, renamed or changed, and added photos, then folders gone from disk. Ids for new
/// folders and photos are taken from `library`. A new folder or photo brings what its sidecar says
/// (its star, and its [`FolderInfo`](dalica_library::FolderInfo) or [`PhotoInfo`]) in the same op as the one that adds it
/// (`AddFolder` or `AddPhotos` batched with `SetFolderInfo` or `SetPhotoInfo`), so no sidecar
/// write can see it without them; for folders and photos already known, the model's word stands.
/// A new photo's copy saved beside it comes in its `AddPhotos` batch (`SetSavedAs`) and a new
/// folder's project mark in its `AddFolder` batch (`SetProjects`). The saved records are the
/// originals record's word for every photo of a folder the scan read (known or new): one
/// derived `SetSaved` after the adds sets or clears them ([`crate::originals`]); a record that
/// could not be read changes none.
/// Photos whose files were read ([`FoundPhoto::file`]) follow `xmp-interop.md` §5 ([`crate::meta`]):
/// a new photo Dalica has nothing for brings every value its file has in its `AddPhotos` batch, and
/// a last sync op ([`crate::meta::sync_parts`]) imports what changed in another application and
/// sets the synced hashes. Applying the ops in order makes the model match the
/// scan; diffing again then gives nothing. Folders are removed (or seen as renamed) only under the
/// roots in `scan.walked`.
pub fn diff(library: &mut Library, scan: &Scan) -> Vec<Op> {
    let walked = &scan.walked;
    diff_in(library, &scan.folders, &|path: &Path| walked.iter().any(|root| path.starts_with(root)))
}

/// [`diff`] for an incremental verification: only what the verified scopes cover changes, so the
/// result equals the full diff restricted to them.
pub fn diff_rescan(library: &mut Library, rescan: &Rescan) -> Vec<Op> {
    diff_in(library, &rescan.scan.folders, &|path: &Path| rescan.covers(path))
}

/// [`diff`] for one folder a streaming scan found at a path the model does not know, diffed once
/// the walk has ended: `gone` are the folders the model holds where the walk looked and did not
/// find. When `found` holds most of the photos of one of them (as [`diff`] judges a rename), it is
/// that folder renamed or moved on disk, and it keeps its id, its photos' ids and their stars;
/// else it is a new folder. Unlike [`diff`], the folders of `gone` it does not pair with stay in
/// the model (the scan removes those at its end, after every such folder had its turn).
pub fn diff_found(library: &mut Library, found: &FoundFolder, gone: &[FolderId]) -> Vec<Op> {
    let mut gone = gone.to_vec();
    gone.sort();
    gone.dedup();
    diff_with(library, std::slice::from_ref(found), gone, false)
}

/// A photo's (size, modification time): what renames are recognised by.
type Key = (u64, Timestamp);

fn diff_in(library: &mut Library, folders: &[FoundFolder], covered: &dyn Fn(&Path) -> bool) -> Vec<Op> {
    // Folders known but gone from disk where the scan looked.
    let found: BTreeMap<&Path, ()> = folders.iter().map(|f| (f.path.as_path(), ())).collect();
    let mut gone: Vec<FolderId> = library.folders().filter(|f| covered(&f.path) && !found.contains_key(f.path.as_path())).map(|f| f.id).collect();
    gone.sort();
    diff_with(library, folders, gone, true)
}

/// The diff of `folders` against the model, `gone` (sorted) the known folders not found where the
/// scan looked: each may pair with a new folder as its rename; those that do not are removed when
/// `remove_gone`.
fn diff_with(library: &mut Library, folders: &[FoundFolder], mut gone: Vec<FolderId>, remove_gone: bool) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut removes = Vec::new();
    let mut updates = Vec::new();
    let mut adds: Vec<(Photo, PhotoInfo, Option<SavedAs>)> = Vec::new();
    let mut saved: Vec<(PhotoId, Option<Saved>)> = Vec::new();
    let mut sync = Sync::default();
    let mut renames = folder_renames(library, folders, &gone);
    gone.retain(|id| !renames.values().any(|old| old == id));
    for found in folders {
        let folder = match library.folder_by_path(&found.path) {
            Some(id) => id,
            None => match renames.remove(found.path.as_path()) {
                Some(old) => {
                    ops.push(move_folder(library, old, found));
                    old
                }
                None => {
                    let id = library.next_folder_id();
                    let mut batch = vec![Op::AddFolder { folder: Folder { id, path: found.path.clone() } }];
                    if !found.info.is_empty() {
                        batch.push(Op::SetFolderInfo { folders: vec![(id, found.info.clone())] });
                    }
                    if let Some(project) = found.project {
                        batch.push(Op::SetProjects { projects: vec![(id, Some(project))] });
                    }
                    ops.push(if batch.len() == 1 { batch.swap_remove(0) } else { Op::Batch { ops: batch } });
                    id
                }
            },
        };
        let known: HashMap<&str, &Photo> = library.photos_in(folder).into_iter().map(|p| (p.name.as_str(), p)).collect();
        let on_disk: HashMap<&str, &FoundPhoto> = found.photos.iter().map(|p| (p.name.as_str(), p)).collect();
        let mut gone: Vec<&Photo> = known.values().filter(|p| !on_disk.contains_key(p.name.as_str())).copied().collect();
        gone.sort_by_key(|p| p.id);
        let mut new: Vec<&FoundPhoto> = found.photos.iter().filter(|f| !known.contains_key(f.name.as_str())).collect();
        // Renames: a gone photo and a new one with the same size and modification time.
        let mut renamed: Vec<(PhotoId, &FoundPhoto)> = Vec::new();
        gone.retain(|old| match new.iter().position(|n| n.size == old.size && n.mtime == old.mtime) {
            Some(i) => {
                renamed.push((old.id, new.remove(i)));
                false
            }
            None => true,
        });
        removes.extend(gone.iter().map(|p| p.id));
        // Each photo's id once the ops are applied, for its saved record.
        let mut ids: Vec<(&str, PhotoId)> = Vec::with_capacity(found.photos.len());
        for (id, f) in renamed {
            ids.push((f.name.as_str(), id));
            if let Some(old) = library.photo(id) {
                updates.push(photo(old.id, folder, f, old.star));
                if let Some(file) = &f.file {
                    sync.known_photo(library, old, file, f.date);
                }
            }
        }
        for f in &found.photos {
            if let Some(old) = known.get(f.name.as_str()) {
                ids.push((f.name.as_str(), old.id));
                if (old.size, old.mtime, old.width, old.height, old.date) != (f.size, f.mtime, f.width, f.height, f.date) {
                    updates.push(photo(old.id, folder, f, old.star));
                }
                if let Some(file) = &f.file {
                    sync.known_photo(library, old, file, f.date);
                }
            }
        }
        for f in new {
            let id = library.next_photo_id();
            let (star, info) = match &f.file {
                Some(file) => {
                    sync.new_photo(id, file);
                    first_fill(f.star, &f.info, file, f.date).unwrap_or_else(|| (f.star, f.info.clone()))
                }
                None => (f.star, f.info.clone()),
            };
            ids.push((f.name.as_str(), id));
            adds.push((photo(id, folder, f, star), info, f.saved_as.clone()));
        }
        saved.extend(crate::originals::changes(library, &ids, &found.originals));
    }
    // Removals first, so a rename's new name is free; then updates (renames, changes); then adds.
    for chunk in removes.chunks(BATCH) {
        ops.push(Op::RemovePhotos { ids: chunk.to_vec() });
    }
    for chunk in updates.chunks(BATCH) {
        ops.push(Op::UpdatePhotos { photos: chunk.to_vec() });
    }
    for chunk in adds.chunks(BATCH) {
        ops.push(add_photos(chunk));
    }
    for chunk in saved.chunks(BATCH) {
        ops.push(Op::SetSaved { saved: chunk.to_vec() });
    }
    // What the files said, once every photo read is in the model.
    ops.extend(sync.op());
    if remove_gone {
        ops.extend(gone.into_iter().map(|id| Op::RemoveFolder { id }));
    }
    ops
}

/// Folders renamed or moved on disk: a folder gone from where the scan looked, paired with a new
/// folder holding most of the same files (by size and modification time, like photo renames). A
/// gone folder pairs with the new folder sharing the most of its photos, when that is more than
/// half of them; each pairs at most once, new folders in path order. Returns new path → old folder.
fn folder_renames<'a>(library: &Library, folders: &'a [FoundFolder], gone: &[FolderId]) -> HashMap<&'a Path, FolderId> {
    let mut out = HashMap::new();
    if gone.is_empty() {
        return out;
    }
    // (size, time) → the gone folders holding such photos, with how many.
    let mut index: HashMap<Key, Vec<(FolderId, usize)>> = HashMap::new();
    let mut sizes: HashMap<FolderId, usize> = HashMap::new();
    for &id in gone {
        let photos = library.photos_in(id);
        sizes.insert(id, photos.len());
        for (key, n) in counts(photos.iter().map(|p| (p.size, p.mtime))) {
            index.entry(key).or_default().push((id, n));
        }
    }
    let mut taken: Vec<FolderId> = Vec::new();
    for found in folders {
        if library.folder_by_path(&found.path).is_some() {
            continue;
        }
        let mut overlap: BTreeMap<FolderId, usize> = BTreeMap::new();
        for (key, n) in counts(found.photos.iter().map(|p| (p.size, p.mtime))) {
            for &(id, m) in index.get(&key).map(Vec::as_slice).unwrap_or_default() {
                let e = overlap.entry(id).or_default();
                *e = e.saturating_add(n.min(m));
            }
        }
        // The most shared photos; ties go to the same folder name, then the oldest folder.
        let name = found.path.file_name();
        let best = overlap
            .into_iter()
            .filter(|(id, n)| !taken.contains(id) && n.saturating_mul(2) > sizes.get(id).copied().unwrap_or(usize::MAX))
            .max_by(|(a, n), (b, m)| {
                let same = |id: &FolderId| library.folder(*id).is_some_and(|f| f.path.file_name() == name);
                n.cmp(m).then(same(a).cmp(&same(b))).then(b.cmp(a))
            });
        if let Some((id, _)) = best {
            taken.push(id);
            out.insert(found.path.as_path(), id);
        }
    }
    out
}

/// How many of each key.
fn counts(keys: impl Iterator<Item = Key>) -> HashMap<Key, usize> {
    let mut out: HashMap<Key, usize> = HashMap::new();
    for k in keys {
        let e = out.entry(k).or_default();
        *e = e.saturating_add(1);
    }
    out
}

/// One op that moves folder `old` to `found.path` keeping its id, its photos' ids, stars and
/// sketches: the folder is taken out and put back at its new path, in one batch.
fn move_folder(library: &Library, old: FolderId, found: &FoundFolder) -> Op {
    let photos: Vec<Photo> = library.photos_in(old).into_iter().cloned().collect();
    let sketches: Vec<_> = photos.iter().filter_map(|p| library.sketch(p.id).map(|s| (p.id, Some(*s)))).collect();
    let mut ops = vec![Op::RemoveFolder { id: old }, Op::AddFolder { folder: Folder { id: old, path: found.path.clone() } }];
    if !photos.is_empty() {
        ops.push(Op::AddPhotos { photos });
    }
    if !sketches.is_empty() {
        ops.push(Op::SetSketches { sketches });
    }
    Op::Batch { ops }
}

/// One `AddPhotos` op, batched with the `SetPhotoInfo` and `SetSavedAs` of those that bring
/// something from their sidecar.
fn add_photos(chunk: &[(Photo, PhotoInfo, Option<SavedAs>)]) -> Op {
    let add = Op::AddPhotos { photos: chunk.iter().map(|(p, ..)| p.clone()).collect() };
    let infos: Vec<(PhotoId, PhotoInfo)> = chunk.iter().filter(|(_, i, _)| !i.is_empty()).map(|(p, i, _)| (p.id, i.clone())).collect();
    let copies: Vec<(PhotoId, Option<SavedAs>)> = chunk.iter().filter_map(|(p, _, c)| Some((p.id, Some(c.clone()?)))).collect();
    let mut ops = vec![add];
    if !infos.is_empty() {
        ops.push(Op::SetPhotoInfo { photos: infos });
    }
    if !copies.is_empty() {
        ops.push(Op::SetSavedAs { saved_as: copies });
    }
    if ops.len() == 1 { ops.swap_remove(0) } else { Op::Batch { ops } }
}

fn photo(id: PhotoId, folder: FolderId, f: &FoundPhoto, star: bool) -> Photo {
    Photo { id, folder, name: f.name.clone(), size: f.size, mtime: f.mtime, width: f.width, height: f.height, date: f.date, star }
}
