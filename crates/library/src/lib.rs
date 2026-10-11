//! dalica-library: The in-memory model (folders, photos, albums, people, tags, captions, stars, hidden, recipes, places), `Op`s with inverses, revisions, queries and the search grammar.
//!
//! Layer L2; see `docs/architecture.md` §2 and §4. Pure: no disk, no threads, no clock. The model
//! changes only through [`Library::apply`], which takes an [`Op`], applies all of it or none of it,
//! bumps the revision once and returns the op that undoes it. The undo stack, the journal and
//! replay are built on that.
//!
//! M1 holds folders and photos (name, size, modification time, size in pixels, capture date,
//! star). Ids are assigned by the caller through [`Library::next_folder_id`] /
//! [`Library::next_photo_id`] and carried inside ops, so replaying a journal gives the same ids.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod model;
mod op;
mod query;

pub use model::{Folder, Library, MAX_SKETCH, Model, Photo, Sketch};
pub use op::Op;

use dalica_core::{FolderId, PhotoId};

/// Why an op was refused. A refused op changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("no folder {0}")]
    UnknownFolder(FolderId),
    #[error("no photo {0}")]
    UnknownPhoto(PhotoId),
    #[error("folder {0} exists already")]
    FolderExists(FolderId),
    #[error("a folder at that path exists already ({0})")]
    FolderPathExists(FolderId),
    #[error("photo {0} exists already")]
    PhotoExists(PhotoId),
    #[error("folder {folder} already has a photo named {name:?}")]
    NameTaken { folder: FolderId, name: String },
    #[error("an update cannot move photo {0} to another folder")]
    FolderChange(PhotoId),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
