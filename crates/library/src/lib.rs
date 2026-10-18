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
//!
//! M9 adds albums ([`AlbumInfo`], memberships with position keys, covers; the keys of adds, drops
//! and Sort Album By in [`albums`]), each photo's search facts ([`Facts`]), content digest
//! ([`ContentDigest`]), first-indexed and last-changed times (all index-only, like sketches), each
//! folder's photo sort ([`PhotoSort`]), moves and renames of photos and folders as ops with exact
//! inverses, the automatic albums (Starred Photos, Recently Updated) and the Albums group of the
//! tree with its sections and a section filter for search mode ([`AlbumTree`], [`Section`],
//! [`SectionFilter`]).
//!
//! M13 adds each photo's faces ([`Face`]: a rectangle and who it is) and geotag ([`Geo`]), the
//! people ([`Person`]) with their ops, and the queries over them: person albums, Unnamed faces,
//! counts, the geotagged photos (`people.rs`, `query.rs`); search reads `person:` and the Faces
//! filter.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod albums;
mod model;
mod op;
mod people;
mod query;
pub mod search;
pub mod sync;
pub mod tags;
mod tree;

pub use albums::{Anchor, album_id_from, clean_album_name};
pub use model::{
    AlbumInfo, ContentDigest, Facts, Folder, FolderInfo, Library, MAX_ALBUM_NAME, MAX_CAMERA, MAX_DIGEST, MAX_FILE_NAME, MAX_SKETCH, Model, Photo,
    PhotoInfo, PhotoSort, Project, Recipe, SaveState, Saved, SavedAs, Sketch, SortBy, Synced,
};
pub use op::Op;
pub use people::{
    Face, FaceKey, FacePerson, FaceRef, Geo, MAX_FACES, MAX_PERSON_ID_DRAWS, MAX_PERSON_NAME, PeopleSort, Person, clean_person_name, person_id_from,
};
pub use query::AlbumStats;
pub use tags::{
    MAX_CAPTION, MAX_DESCRIPTION, MAX_OPERATION, MAX_OPERATIONS, MAX_PLACE, MAX_TAG, MAX_TAGS, PartialCount, TagCount, TagIndex, clean_caption,
    clean_description, clean_place, clean_tag, clean_tags,
};
pub use tree::{
    AlbumRef, AlbumRow, AlbumTree, FolderRow, FolderSort, Group, GroupRows, RECENT_DAYS, Row, SMALL_PICTURE_PX, Section, SectionFilter, SectionId,
    Tree, TreeMode, TreeOptions, section_photos,
};

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId};

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
    #[error("no album {0}")]
    UnknownAlbum(AlbumId),
    #[error("album {0} exists already")]
    AlbumExists(AlbumId),
    #[error("photo {photo} is in album {album} already")]
    AlreadyMember { album: AlbumId, photo: PhotoId },
    #[error("photo {photo} is not in album {album}")]
    NotMember { album: AlbumId, photo: PhotoId },
    /// A folder moved inside itself, or onto a folder it is inside (`data-model.md` §8).
    #[error("folder {0} cannot move inside itself or onto a folder it is in")]
    FolderIntoItself(FolderId),
    /// People (M13): named by id only, never by name (`faces.md` §7).
    #[error("no person {0}")]
    UnknownPerson(PersonId),
    #[error("person {0} exists already")]
    PersonExists(PersonId),
    /// Two people's names may not be equal ignoring case; the id is the person holding it.
    #[error("person {0} has that name already")]
    PersonNameTaken(PersonId),
    #[error("person {0} cannot merge into itself")]
    MergeIntoItself(PersonId),
    #[error("no face of person {person} on photo {photo}")]
    NotPersonsFace { person: PersonId, photo: PhotoId },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
