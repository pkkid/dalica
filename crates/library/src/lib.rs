//! dalica-library: The in-memory model (folders, photos, albums, people, tags, captions, stars, hidden, recipes, places), `Op`s with inverses, revisions, queries and the search grammar.
//!
//! Layer L2; see `docs/architecture.md` §2 and §4. Pure: no disk, no threads, no clock. The model
//! changes only through [`Library::apply`], which takes an [`Op`], applies all of it or none of it,
//! bumps the revision once and returns the op that undoes it. The undo stack, the journal and
//! replay are built on that.
//!
//! M1 holds folders and photos (name, size, modification time, size in pixels, capture date,
//! star); M6 adds what the user says about them ([`PhotoInfo`]: caption, tags, hidden, recipe and
//! redo list, a date of the user's; [`FolderInfo`]: description, date, place, hidden), the
//! last-synced metadata hashes ([`Synced`], derived like sketches) and the tag index
//! ([`tags`]). Ids are assigned by the caller through [`Library::next_folder_id`] /
//! [`Library::next_photo_id`] and carried inside ops, so replaying a journal gives the same ids.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod model;
mod op;
mod query;
pub mod sync;
pub mod tags;
mod tree;

pub use model::{
    Folder, FolderInfo, Library, MAX_DIGEST, MAX_FILE_NAME, MAX_SKETCH, Model, Photo, PhotoInfo, Project, Recipe, SaveState, Saved, SavedAs, Sketch,
    Synced,
};
pub use op::Op;
pub use tags::{
    MAX_CAPTION, MAX_DESCRIPTION, MAX_OPERATION, MAX_OPERATIONS, MAX_PLACE, MAX_TAG, MAX_TAGS, PartialCount, TagCount, TagIndex, clean_caption,
    clean_description, clean_place, clean_tag, clean_tags,
};
pub use tree::{FolderRow, FolderSort, Group, GroupRows, Row, SMALL_PICTURE_PX, SectionId, Tree, TreeMode, TreeOptions};

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
    /// Undo Save or Revert of a photo with no finished save (`data-model.md` §6).
    #[error("photo {0} has no finished save")]
    NotSaved(PhotoId),
    /// Text past its cap (`security.md` §4.4). The text itself is never in the message.
    #[error("a {what} longer than {max} bytes")]
    TooLong { what: &'static str, max: usize },
    #[error("more than {max} {what}")]
    TooMany { what: &'static str, max: usize },
    #[error("a {what} that is not clean: {why}")]
    BadText { what: &'static str, why: &'static str },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
