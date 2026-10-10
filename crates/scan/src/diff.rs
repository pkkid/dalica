//! The verification pass: the ops that make the model match a [`Scan`].

use std::collections::{BTreeMap, HashMap};

use dalica_core::PhotoId;
use dalica_library::{Folder, Library, Op, Photo};

use crate::walk::{FoundPhoto, Scan};

/// The most photos in one `AddPhotos` / `UpdatePhotos` / `RemovePhotos` op.
pub const BATCH: usize = 500;

/// The ops that bring `library` in line with `scan`, in order: new folders, then per folder
/// removed, renamed or changed, and added photos, then folders gone from disk. Ids for new folders
/// and photos are taken from `library`. Applying the ops in order makes the model match the scan;
/// diffing again then gives nothing.
pub fn diff(library: &mut Library, scan: &Scan) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut removes = Vec::new();
    let mut updates = Vec::new();
    let mut adds = Vec::new();
    for found in &scan.folders {
        let folder = match library.folder_by_path(&found.path) {
            Some(id) => id,
            None => {
                let id = library.next_folder_id();
                ops.push(Op::AddFolder { folder: Folder { id, path: found.path.clone() } });
                id
            }
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
        for (id, f) in renamed {
            if let Some(old) = library.photo(id) {
                updates.push(photo(old.id, folder, f, old.star));
            }
        }
        for f in &found.photos {
            if let Some(old) = known.get(f.name.as_str())
                && (old.size, old.mtime, old.width, old.height, old.date) != (f.size, f.mtime, f.width, f.height, f.date)
            {
                updates.push(photo(old.id, folder, f, old.star));
            }
        }
        for f in new {
            let id = library.next_photo_id();
            adds.push(photo(id, folder, f, f.star));
        }
    }
    // Folders under a walked root that hold no photos any more.
    let found: BTreeMap<_, _> = scan.folders.iter().map(|f| (f.path.as_path(), ())).collect();
    let mut gone_folders = Vec::new();
    for folder in library.folders() {
        let under_walked = scan.walked.iter().any(|root| folder.path.starts_with(root));
        if under_walked && !found.contains_key(folder.path.as_path()) {
            gone_folders.push(folder.id);
        }
    }
    // Removals first, so a rename's new name is free; then updates (renames, changes); then adds.
    for chunk in removes.chunks(BATCH) {
        ops.push(Op::RemovePhotos { ids: chunk.to_vec() });
    }
    for chunk in updates.chunks(BATCH) {
        ops.push(Op::UpdatePhotos { photos: chunk.to_vec() });
    }
    for chunk in adds.chunks(BATCH) {
        ops.push(Op::AddPhotos { photos: chunk.to_vec() });
    }
    ops.extend(gone_folders.into_iter().map(|id| Op::RemoveFolder { id }));
    ops
}

fn photo(id: PhotoId, folder: dalica_core::FolderId, f: &FoundPhoto, star: bool) -> Photo {
    Photo { id, folder, name: f.name.clone(), size: f.size, mtime: f.mtime, width: f.width, height: f.height, date: f.date, star }
}
