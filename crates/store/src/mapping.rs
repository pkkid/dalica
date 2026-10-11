//! Model ↔ sidecar mapping (`data-model.md` §2): which photo fields live in the folder's
//! `.dalica.json` and which are read from the file itself. Every field of `Photo` is listed in
//! [`PHOTO_FIELDS`]; a test destructures `Photo` exhaustively, so a new field does not compile
//! until it is placed here (nothing is silently dropped). Per-photo data the model keeps beside
//! `Photo` is listed in [`PHOTO_DATA`].

use std::path::PathBuf;

use dalica_core::FolderId;
use dalica_library::Library;
use dalica_sidecar::{Field, Sidecar, WrongType};

/// Where a photo field's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Kept by the index only (ids, and derived data such as the sketch, which is made again
    /// after a rebuild and never reaches a sidecar).
    Index,
    /// Read from the file system or the file's headers on every scan.
    File,
    /// User data: written to and read from the folder sidecar.
    Sidecar(&'static str),
}

/// Every field of [`dalica_library::Photo`] and where it lives.
pub const PHOTO_FIELDS: [(&str, Source); 9] = [
    ("id", Source::Index),
    ("folder", Source::Index),
    ("name", Source::File),
    ("size", Source::File),
    ("mtime", Source::File),
    ("width", Source::File),
    ("height", Source::File),
    ("date", Source::File),
    ("star", Source::Sidecar("star")),
];

/// Per-photo data the model keeps beside [`dalica_library::Photo`], and where it lives.
pub const PHOTO_DATA: [(&str, Source); 1] = [("sketch", Source::Index)];

/// What a folder's sidecar should say, taken from the model: enough to write it on any thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderState {
    pub id: FolderId,
    pub path: PathBuf,
    /// (file name, star) for every photo in the folder.
    pub photos: Vec<(String, bool)>,
}

/// The sidecar-relevant state of `folder`, or `None` if it is not in the model.
pub fn folder_state(library: &Library, folder: FolderId) -> Option<FolderState> {
    let f = library.folder(folder)?;
    let photos = library.photos_in(folder).into_iter().map(|p| (p.name.clone(), p.star)).collect();
    Some(FolderState { id: folder, path: f.path.clone(), photos })
}

/// Writes the model's values into `sidecar`, keeping everything else. Places that hold a value of
/// the wrong type are left alone and returned.
pub fn apply(state: &FolderState, sidecar: &mut Sidecar) -> Vec<WrongType> {
    let mut refused = Vec::new();
    for (name, star) in &state.photos {
        // Clearing a star that is not there would only add an empty `photos` object.
        if !star && sidecar.star(name) == Field::Absent {
            continue;
        }
        if let Err(e) = sidecar.set_star(name, *star) {
            refused.push(e);
        }
    }
    refused
}

/// The user data a sidecar holds for a photo named `name`: today, its star (`None` when the
/// sidecar says nothing usable).
pub fn star_of(sidecar: &Sidecar, name: &str) -> Option<bool> {
    match sidecar.star(name) {
        Field::Value(on) => Some(on),
        Field::Absent => Some(false),
        Field::WrongType(_) => None,
    }
}
