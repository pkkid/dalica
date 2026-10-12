//! Rebuilding user data from the folders (`data-model.md` §10.1). The index is a cache: after it is
//! lost, a scan rebuilds the folders and photos (crate `scan`), and [`import_sidecars`] reads each
//! folder's `.dalica.json` to give back what the user set: per photo its star, caption, tags,
//! hidden flag, recipe, redo list and date, per folder its description, date, place and hidden
//! flag. Nothing a sidecar holds is lost by deleting the index. The synced metadata hashes are
//! index-only (`mapping::SYNCED_FIELDS`) and start empty after a rebuild.

use dalica_core::{FolderId, Timestamp};
use dalica_library::{Library, Op};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::mapping::{self, PhotoState};
use crate::writer::{self, Outcome};

/// What the folders' sidecars say, as ops on the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// Ops that make the model's user data match the sidecars (empty if it already does): at most
    /// one `SetStars`, one `SetPhotoInfo` and one `SetFolderInfo`, in that order.
    pub ops: Vec<Op>,
    /// Folders whose sidecar was damaged: kept aside, folder read-only.
    pub read_only: Vec<FolderId>,
    /// Folders whose sidecar could not be read, held values of the wrong type or values past the
    /// caps, and why (once per value).
    pub problems: Vec<(FolderId, String)>,
}

/// Reads the sidecar of every folder in `library` and returns the ops that apply its user data. A
/// value of the wrong type leaves the model's own value as it is. Any thread but the UI thread may
/// call it.
pub fn import_sidecars(library: &Library, now: Timestamp) -> Import {
    let limits = Limits::default();
    let mut out = Import::default();
    let mut stars = Vec::new();
    let mut infos = Vec::new();
    let mut folders = Vec::new();
    for folder in library.folders() {
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
                    continue;
                }
            },
            Err(e) => {
                out.problems.push((folder.id, e.to_string()));
                continue;
            }
        };
        for p in sidecar.problems() {
            out.problems.push((folder.id, format!("{} should be {}", p.pointer, p.expected)));
        }
        let reading = mapping::read_folder(&sidecar);
        out.problems.extend(reading.notes.iter().map(|n| (folder.id, n.clone())));
        let own = library.folder_info(folder.id);
        let info = reading.merged(own);
        if info != *own {
            folders.push((folder.id, info));
        }
        for photo in library.photos_in(folder.id) {
            let reading = mapping::read_photo(&sidecar, &photo.name);
            out.problems.extend(reading.notes.iter().map(|n| (folder.id, format!("{}: {n}", photo.name))));
            let own = PhotoState { star: photo.star, info: library.info(photo.id).clone() };
            let read = reading.merged(&own);
            if read.star != own.star {
                stars.push((photo.id, read.star));
            }
            if read.info != own.info {
                infos.push((photo.id, read.info));
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
    out
}
