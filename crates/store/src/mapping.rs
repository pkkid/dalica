//! Model ↔ sidecar mapping (`data-model.md` §2–§3): which model fields live in the folder's
//! `.dalica.json`, which are read from the file itself and which only the index keeps. Every field
//! of `Photo`, `PhotoInfo`, `Synced`, `Folder` and `FolderInfo` is listed here; a test destructures
//! each struct exhaustively, so a new field does not compile until it is placed (nothing is
//! silently dropped). Per-photo data the model keeps beside `Photo` is listed in [`PHOTO_DATA`].
//!
//! **Reading** a key gives a [`Reading`]: the value the model takes from the sidecar (the empty
//! value when the key is absent), or [`Reading::Unusable`] when the key holds a value of the wrong
//! type, or a recipe past the library's caps (cutting a recipe would change it). An unusable value
//! is kept in the file and reported once, and the model keeps its own. Text past its cap is cut
//! for the model (and reported) while the file keeps it whole; a tag list is cleaned the way the
//! library cleans tags (`clean_tags`).
//!
//! **Writing** ([`apply`]) sets a key only where the model's value differs from what the sidecar
//! reads as, so a file that already says what the model says is left byte for byte, including
//! spellings the model would not choose (a tag list it cleaned, an EXIF-style date). An unusable
//! value is written over only when the model holds a value of its own for that field (the user
//! set one); otherwise it is kept.
//!
//! **M8's fields** follow the same rules: a photo's copy saved beside it (`savedAs`, from
//! [`dalica_library::SavedAs`]) and a folder's project mark (`"folder": {"project": "exported"}`)
//! live in the folder sidecar ([`PHOTO_DATA_M8`], [`FOLDER_DATA_M8`]); a `savedAs` whose name is
//! not one plain path component, or a project kind Dalica does not know, is unusable (kept,
//! reported). A photo's saved record ([`dalica_library::Saved`], [`SAVED_FIELDS`]) lives in the
//! originals record `.dalica-originals/.dalica.json` ([`read_originals`], [`originals_entry`]).
//!
//! **M9's fields** follow them too. A photo's album memberships with their position keys
//! (`photos.<name>.albums`, [`read_photo`] → [`PhotoReading::albums`]), the folder's photo sort
//! (`"folder": {"sort", "reverse"}`, [`read_folder`] → [`FolderReading::photo_sort`]) and the
//! names of the albums with members in the folder (the top-level `"albums"`,
//! [`read_album_names`]) live in the folder sidecar ([`PHOTO_DATA_M9`], [`FOLDER_DATA_M9`]); the
//! album definitions and covers live in `albums.json` ([`ALBUM_INFO_FIELDS`], [`LIBRARY_DATA_M9`],
//! [`crate::albums`]). The model is the truth for the memberships of its own albums and for the
//! folder's album names; a membership of an album the model does not have (deleted here, or never
//! in this library) is kept as it is, and an entry whose id is not an album id, or whose key or
//! name cannot be used, is kept verbatim and reported, never used and never written over. A sort the model cannot hold (`manual`, `modified`, anything else) is kept
//! and reported. Search facts, content digests and the first-indexed times are index-only
//! ([`FACTS_FIELDS`], [`CONTENT_DIGEST_FIELDS`]). These are the rules a scan reading a folder
//! sidecar follows too.
//!
//! **M10**: a photo's last-changed time (when Dalica last changed what is said about it, which
//! Recently Updated reads) lives in its entry as `changed` (a UTC time with milliseconds,
//! [`dalica_sidecar::utc_text`]), so a rebuild keeps it ([`PHOTO_DATA_M9`]'s `lastChanged`,
//! [`PhotoState::changed`]). A sidecar without it says no change was recorded; a value that is
//! not such a time is unusable (kept, reported).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use dalica_core::{AlbumId, Date, DateTime, FolderId, PosKey, Timestamp};
use dalica_library::{
    FolderInfo, Library, MAX_CAPTION, MAX_DESCRIPTION, MAX_DIGEST, MAX_FILE_NAME, MAX_OPERATION, MAX_OPERATIONS, MAX_PLACE, PhotoInfo, PhotoSort,
    Project, SaveState, Saved, SavedAs, SortBy,
};
use dalica_sidecar::originals::{self, Entry, Originals, State};
use dalica_sidecar::{Field, FolderValue, PhotoValue, Sidecar, WrongType, plain_name};

/// Where a model field's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Kept by the index only (ids, and derived data such as the sketch and the synced hashes,
    /// which are made again after a rebuild and never reach a sidecar).
    Index,
    /// Read from the file system or the file's headers on every scan.
    File,
    /// User data: written to and read from the photo's entry in the folder sidecar
    /// (`photos.<name>.<key>`).
    Sidecar(&'static str),
    /// User data: written to and read from the sidecar's `"folder"` object (`folder.<key>`).
    FolderSidecar(&'static str),
    /// A struct whose fields have a table of their own (named here).
    Fields(&'static str),
    /// Dalica's own record of a Save: written to and read from the photo's entry in the folder's
    /// originals record (`.dalica-originals/.dalica.json`, `originals.<name>.<key>`).
    Originals(&'static str),
    /// Written to and read from the folder sidecar's top level (`<key>`, M9: the album names).
    TopSidecar(&'static str),
    /// Written to and read from the album's entry in `albums.json` in the library directory
    /// (`albums.<id>.<key>`, M9).
    Albums(&'static str),
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
pub const PHOTO_DATA: [(&str, Source); 3] =
    [("sketch", Source::Index), ("info", Source::Fields("PHOTO_INFO_FIELDS")), ("synced", Source::Fields("SYNCED_FIELDS"))];

/// Every field of [`dalica_library::PhotoInfo`] (what the user said about a photo).
pub const PHOTO_INFO_FIELDS: [(&str, Source); 6] = [
    ("caption", Source::Sidecar("caption")),
    ("tags", Source::Sidecar("tags")),
    ("hidden", Source::Sidecar("hidden")),
    ("edits", Source::Sidecar("edits")),
    ("redo", Source::Sidecar("redo")),
    ("date", Source::Sidecar("date")),
];

/// Every field of [`dalica_library::Synced`]: the last-synced metadata hashes, index-only derived
/// data (`xmp-interop.md` §5) that never reaches a sidecar and starts empty after a rebuild.
pub const SYNCED_FIELDS: [(&str, Source); 5] =
    [("caption", Source::Index), ("tags", Source::Index), ("rating", Source::Index), ("place", Source::Index), ("date", Source::Index)];

/// Every field of [`dalica_library::Folder`].
pub const FOLDER_FIELDS: [(&str, Source); 2] = [("id", Source::Index), ("path", Source::File)];

/// Folder data the model keeps beside [`dalica_library::Folder`].
pub const FOLDER_DATA: [(&str, Source); 1] = [("info", Source::Fields("FOLDER_INFO_FIELDS"))];

/// Per-photo data M8 adds beside [`dalica_library::Photo`], and where it lives.
pub const PHOTO_DATA_M8: [(&str, Source); 2] = [("saved", Source::Fields("SAVED_FIELDS")), ("savedAs", Source::Fields("SAVED_AS_FIELDS"))];

/// Every field of [`dalica_library::Saved`]: each under its own key in the photo's entry of the
/// originals record. The model reads it at scan and rebuild (no undo step, no revision bump).
pub const SAVED_FIELDS: [(&str, Source); 8] = [
    ("recipe", Source::Originals("recipe")),
    ("size", Source::Originals("size")),
    ("mtime", Source::Originals("mtime")),
    ("width", Source::Originals("width")),
    ("height", Source::Originals("height")),
    ("digest", Source::Originals("digest")),
    ("state", Source::Originals("state")),
    ("time", Source::Originals("time")),
];

/// Every field of [`dalica_library::SavedAs`]: one object under the photo's `savedAs` key.
pub const SAVED_AS_FIELDS: [(&str, Source); 2] = [("name", Source::Sidecar("savedAs")), ("recipe", Source::Sidecar("savedAs"))];

/// Folder data M8 adds beside [`dalica_library::Folder`]: the project mark, in `"folder"`.
pub const FOLDER_DATA_M8: [(&str, Source); 1] = [("project", Source::FolderSidecar("project"))];

/// Per-photo data M9 adds beside [`dalica_library::Photo`], and where it lives: the album
/// memberships (album id → position key, `photos.<name>.albums`; the order is also copied into
/// `albums.json`), the index-only search facts, content digest and first-indexed time, and the
/// last-changed time (`photos.<name>.changed`; in the sidecar since M10).
pub const PHOTO_DATA_M9: [(&str, Source); 5] = [
    ("albums", Source::Sidecar("albums")),
    ("facts", Source::Fields("FACTS_FIELDS")),
    ("digest", Source::Fields("CONTENT_DIGEST_FIELDS")),
    ("firstIndexed", Source::Index),
    ("lastChanged", Source::Sidecar("changed")),
];

/// Every field of [`dalica_library::Facts`]: read from the file on scan, kept by the index only.
pub const FACTS_FIELDS: [(&str, Source); 4] =
    [("make", Source::Index), ("model", Source::Index), ("video", Source::Index), ("geotagged", Source::Index)];

/// Every field of [`dalica_library::ContentDigest`]: made from the file, kept by the index only.
pub const CONTENT_DIGEST_FIELDS: [(&str, Source); 3] = [("size", Source::Index), ("mtime", Source::Index), ("digest", Source::Index)];

/// Folder data M9 adds beside [`dalica_library::Folder`]: its photo sort, and the names of the
/// albums with members in it (mirrored from [`ALBUM_INFO_FIELDS`]'s `name`).
pub const FOLDER_DATA_M9: [(&str, Source); 2] = [("sort", Source::Fields("PHOTO_SORT_FIELDS")), ("albumNames", Source::TopSidecar("albums"))];

/// Every field of [`dalica_library::PhotoSort`] as a folder holds it, in `"folder"`.
pub const PHOTO_SORT_FIELDS: [(&str, Source); 2] = [("by", Source::FolderSidecar("sort")), ("reverse", Source::FolderSidecar("reverse"))];

/// Library data M9 adds: the user albums, their memberships and their covers.
pub const LIBRARY_DATA_M9: [(&str, Source); 3] =
    [("albums", Source::Fields("ALBUM_INFO_FIELDS")), ("members", Source::Sidecar("albums")), ("covers", Source::Albums("cover"))];

/// Every field of [`dalica_library::AlbumInfo`] (Album Properties), each under its own key of the
/// album's entry in `albums.json`; the name is also mirrored in each member folder's sidecar.
pub const ALBUM_INFO_FIELDS: [(&str, Source); 5] = [
    ("name", Source::Albums("name")),
    ("description", Source::Albums("description")),
    ("date", Source::Albums("date")),
    ("useNewest", Source::Albums("useNewest")),
    ("place", Source::Albums("place")),
];

/// Every field of [`dalica_library::FolderInfo`] (Folder Properties, Hide Folder).
pub const FOLDER_INFO_FIELDS: [(&str, Source); 4] = [
    ("description", Source::FolderSidecar("description")),
    ("date", Source::FolderSidecar("date")),
    ("place", Source::FolderSidecar("place")),
    ("hidden", Source::FolderSidecar("hidden")),
];

/// The sidecar-relevant state of one photo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhotoState {
    pub star: bool,
    pub info: PhotoInfo,
    /// The copy a Save wrote beside the photo (M8).
    pub saved_as: Option<SavedAs>,
    /// The albums the photo belongs to, with its position key in each (M9).
    pub albums: BTreeMap<AlbumId, PosKey>,
    /// When Dalica last changed what is said about the photo (M10; `None`: no change recorded).
    pub changed: Option<Timestamp>,
}

/// What a folder's sidecar should say, taken from the model: enough to write it on any thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderState {
    pub id: FolderId,
    pub path: PathBuf,
    /// What the user said about the folder.
    pub folder: FolderInfo,
    /// (file name, state) for every photo in the folder.
    pub photos: Vec<(String, PhotoState)>,
    /// The folder's project mark (M8).
    pub project: Option<Project>,
    /// The folder's photo sort (M9; the default, by date, is written as no key).
    pub sort: PhotoSort,
    /// The name of every album with a member in the folder (M9).
    pub album_names: BTreeMap<AlbumId, String>,
    /// Every album the model has (M9): a photo's memberships of other albums are kept as they
    /// are in the file (an album deleted here, or one this library never had).
    pub known_albums: BTreeSet<AlbumId>,
}

impl Default for FolderState {
    /// A folder with nothing to say (id 0, an empty path): fill in what the caller has.
    fn default() -> FolderState {
        FolderState {
            id: FolderId::new(0),
            path: PathBuf::new(),
            folder: FolderInfo::default(),
            photos: Vec::new(),
            project: None,
            sort: PhotoSort::default(),
            album_names: BTreeMap::new(),
            known_albums: BTreeSet::new(),
        }
    }
}

/// The sidecar-relevant state of `folder`, or `None` if it is not in the model.
pub fn folder_state(library: &Library, folder: FolderId) -> Option<FolderState> {
    let f = library.folder(folder)?;
    let photos: Vec<(String, PhotoState)> = library.photos_in(folder).into_iter().map(|p| (p.name.clone(), photo_state(library, p))).collect();
    let album_names = photos.iter().flat_map(|(_, p)| p.albums.keys()).filter_map(|id| Some((id.clone(), library.album(id)?.name.clone()))).collect();
    Some(FolderState {
        id: folder,
        path: f.path.clone(),
        folder: library.folder_info(folder).clone(),
        photos,
        project: library.project(folder),
        sort: library.folder_sort(folder),
        album_names,
        known_albums: library.albums().map(|(id, _)| id.clone()).collect(),
    })
}

/// The sidecar-relevant state of `photo`.
pub fn photo_state(library: &Library, photo: &dalica_library::Photo) -> PhotoState {
    PhotoState {
        star: photo.star,
        info: library.info(photo.id).clone(),
        saved_as: library.saved_as(photo.id).cloned(),
        albums: library.albums_of(photo.id).into_iter().map(|(a, k)| (a.clone(), k.clone())).collect(),
        changed: library.last_changed(photo.id),
    }
}

/// What one sidecar key gives the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading<T> {
    /// The model's value for the field (the empty value when the key is absent).
    Usable(T),
    /// A value of the wrong type, or a recipe past the caps: kept in the file, not used.
    Unusable,
}

impl<T> Reading<T> {
    /// The value, or `own` when the reading is unusable.
    pub fn or(self, own: T) -> T {
        match self {
            Reading::Usable(v) => v,
            Reading::Unusable => own,
        }
    }

    /// The value, or `own()` when the reading is unusable.
    pub fn or_else(self, own: impl FnOnce() -> T) -> T {
        match self {
            Reading::Usable(v) => v,
            Reading::Unusable => own(),
        }
    }
}

/// Whether the model's `value` must be written over a key that reads as `reading`.
fn differs<T: PartialEq + Default>(reading: &Reading<T>, value: &T) -> bool {
    match reading {
        Reading::Usable(v) => v != value,
        Reading::Unusable => *value != T::default(),
    }
}

fn plain<T: Default>(f: Field<T>) -> Reading<T> {
    match f {
        Field::Absent => Reading::Usable(T::default()),
        Field::Value(v) => Reading::Usable(v),
        Field::WrongType(_) => Reading::Unusable,
    }
}

fn dated<T>(f: Field<T>) -> Reading<Option<T>> {
    match f {
        Field::Absent => Reading::Usable(None),
        Field::Value(v) => Reading::Usable(Some(v)),
        Field::WrongType(_) => Reading::Unusable,
    }
}

/// Text cut by `clean` when it is longer than `max` bytes (noted under `what`).
fn capped(f: Field<String>, max: usize, clean: fn(&str) -> String, what: &str, notes: &mut Vec<String>) -> Reading<String> {
    match plain(f) {
        Reading::Usable(s) if s.len() > max => {
            notes.push(format!("the {what} is longer than {max} bytes: shown cut, kept whole in the file"));
            Reading::Usable(clean(&s))
        }
        other => other,
    }
}

/// A recipe or redo list: unusable past the library's caps.
fn operations(f: Field<Vec<String>>, what: &str, notes: &mut Vec<String>) -> Reading<Vec<String>> {
    match plain(f) {
        Reading::Usable(l) if l.len() > MAX_OPERATIONS || l.iter().any(|op| op.len() > MAX_OPERATION) => {
            notes.push(format!(
                "the {what} has more than {MAX_OPERATIONS} operations or one longer than {MAX_OPERATION} bytes: kept in the file, not used"
            ));
            Reading::Unusable
        }
        other => other,
    }
}

/// What a sidecar says about one photo, key by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotoReading {
    pub star: Reading<bool>,
    pub caption: Reading<String>,
    pub tags: Reading<Vec<String>>,
    pub hidden: Reading<bool>,
    pub edits: Reading<Vec<String>>,
    pub redo: Reading<Vec<String>>,
    pub date: Reading<Option<DateTime>>,
    pub saved_as: Reading<Option<SavedAs>>,
    /// The photo's usable memberships (M9): entries Dalica cannot use are left out (they stay in
    /// the file and are noted); `albums` of the wrong type is unusable.
    pub albums: Reading<BTreeMap<AlbumId, PosKey>>,
    /// When Dalica last changed the photo (M10); a value that is not a UTC time is unusable.
    pub changed: Reading<Option<Timestamp>>,
    /// Values past a cap, one sentence each (values of the wrong type are
    /// [`Sidecar::problems`]).
    pub notes: Vec<String>,
}

impl PhotoReading {
    /// The state the model takes from the sidecar, keeping `own`'s value wherever the reading is
    /// unusable.
    pub fn merged(self, own: &PhotoState) -> PhotoState {
        let o = &own.info;
        PhotoState {
            star: self.star.or(own.star),
            info: PhotoInfo {
                caption: self.caption.or_else(|| o.caption.clone()),
                tags: self.tags.or_else(|| o.tags.clone()),
                hidden: self.hidden.or(o.hidden),
                edits: self.edits.or_else(|| o.edits.clone()),
                redo: self.redo.or_else(|| o.redo.clone()),
                date: self.date.or(o.date),
            },
            saved_as: self.saved_as.or_else(|| own.saved_as.clone()),
            albums: self.albums.or_else(|| own.albums.clone()),
            changed: self.changed.or(own.changed),
        }
    }
}

/// What a sidecar says about its folder, key by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderReading {
    pub description: Reading<String>,
    pub date: Reading<Option<Date>>,
    pub place: Reading<String>,
    pub hidden: Reading<bool>,
    /// The project mark (M8; [`FolderReading::merged`] leaves it out: see [`FolderReading::project`]).
    pub project: Reading<Option<Project>>,
    /// What the folder's photos sort by (M9; [`FolderReading::photo_sort`]).
    pub sort: Reading<SortBy>,
    /// Whether that sort is reversed (M9).
    pub reverse: Reading<bool>,
    /// Values past a cap, one sentence each.
    pub notes: Vec<String>,
}

impl FolderReading {
    /// The folder info the model takes from the sidecar, keeping `own`'s value wherever the
    /// reading is unusable.
    pub fn merged(self, own: &FolderInfo) -> FolderInfo {
        FolderInfo {
            description: self.description.or_else(|| own.description.clone()),
            date: self.date.or(own.date),
            place: self.place.or_else(|| own.place.clone()),
            hidden: self.hidden.or(own.hidden),
        }
    }

    /// The photo sort the model takes from the sidecar, keeping `own`'s parts wherever the
    /// reading is unusable.
    pub fn photo_sort(&self, own: PhotoSort) -> PhotoSort {
        PhotoSort { by: self.sort.clone().or(own.by), reverse: self.reverse.clone().or(own.reverse) }
    }
}

/// Reads photo `name`'s entry.
pub fn read_photo(sidecar: &Sidecar, name: &str) -> PhotoReading {
    let f = sidecar.photo(name);
    let mut notes = Vec::new();
    let tags = match plain(f.tags) {
        Reading::Usable(l) => Reading::Usable(dalica_library::clean_tags(l.iter().map(String::as_str))),
        Reading::Unusable => Reading::Unusable,
    };
    PhotoReading {
        star: plain(f.star),
        caption: capped(f.caption, MAX_CAPTION, dalica_library::clean_caption, "caption", &mut notes),
        tags,
        hidden: plain(f.hidden),
        edits: operations(f.edits, "recipe", &mut notes),
        redo: operations(f.redo, "redo list", &mut notes),
        date: dated(f.date),
        saved_as: saved_as(sidecar.saved_as(name), &mut notes),
        albums: memberships(sidecar.photo_albums(name), &mut notes),
        changed: dated(f.changed),
        notes,
    }
}

/// A photo's usable memberships: the entries with an album id and a position key.
fn memberships(f: Field<dalica_sidecar::PhotoAlbums>, notes: &mut Vec<String>) -> Reading<BTreeMap<AlbumId, PosKey>> {
    match f {
        Field::Absent => Reading::Usable(BTreeMap::new()),
        Field::Value(v) => {
            for id in &v.bad {
                notes.push(format!("the album entry {id:?} is not an album id with a position key: kept in the file, not used"));
            }
            Reading::Usable(v.members)
        }
        Field::WrongType(_) => Reading::Unusable,
    }
}

/// The spelling of a photo sort in the folder sidecar's `sort`; the default (by date) is written
/// as no key.
pub fn sort_text(by: SortBy) -> Option<&'static str> {
    match by {
        SortBy::Date => None,
        SortBy::Name => Some("name"),
        SortBy::Size => Some("size"),
    }
}

/// A folder `sort` the model can hold: `date` (or none), `name`, `size`.
fn sort_by(f: Field<String>, notes: &mut Vec<String>) -> Reading<SortBy> {
    match f {
        Field::Absent => Reading::Usable(SortBy::Date),
        Field::Value(s) => match s.as_str() {
            "date" => Reading::Usable(SortBy::Date),
            "name" => Reading::Usable(SortBy::Name),
            "size" => Reading::Usable(SortBy::Size),
            _ => {
                notes.push(format!("the folder's sort {s:?} is not one Dalica can sort by: kept in the file, not used"));
                Reading::Unusable
            }
        },
        Field::WrongType(_) => Reading::Unusable,
    }
}

/// What the folder sidecar's top-level `"albums"` says (M9): the name of each album with a usable
/// id and name, cleaned the way the library cleans album names, with one sentence per name that
/// was cut or is nothing once cleaned (not used), and one per entry Dalica cannot use (left out,
/// kept in the file); an `"albums"` of the wrong type is unusable.
pub fn read_album_names(sidecar: &Sidecar) -> (Reading<BTreeMap<AlbumId, String>>, Vec<String>) {
    let mut notes = Vec::new();
    let names = match sidecar.album_names() {
        Field::Absent => Reading::Usable(BTreeMap::new()),
        Field::WrongType(_) => Reading::Unusable,
        Field::Value(v) => {
            for id in &v.bad {
                notes.push(format!("the folder's album entry {id:?} is not an album id with a name: kept in the file, not used"));
            }
            let mut out = BTreeMap::new();
            for (id, raw) in v.names {
                match dalica_library::clean_album_name(&raw) {
                    Some(name) => {
                        if raw.len() > dalica_library::MAX_ALBUM_NAME {
                            notes.push(format!(
                                "album {id}'s name is longer than {} bytes: shown cut, kept whole in the file",
                                dalica_library::MAX_ALBUM_NAME
                            ));
                        }
                        out.insert(id, name);
                    }
                    None => notes.push(format!("album {id}'s name is empty once cleaned: kept in the file, not used")),
                }
            }
            Reading::Usable(out)
        }
    };
    (names, notes)
}

/// A `savedAs` the model can take: a plain file name within the cap and a recipe within the caps.
fn saved_as(f: Field<dalica_sidecar::SavedAs>, notes: &mut Vec<String>) -> Reading<Option<SavedAs>> {
    match f {
        Field::Absent => Reading::Usable(None),
        Field::WrongType(_) => Reading::Unusable,
        Field::Value(v) if !plain_name(&v.name) || v.name.len() > MAX_FILE_NAME => {
            notes.push("the copy saved beside it is not named by a plain file name: kept in the file, not used".into());
            Reading::Unusable
        }
        Field::Value(v) => match operations(Field::Value(v.recipe), "copy's recipe", notes) {
            Reading::Usable(recipe) => Reading::Usable(Some(SavedAs { name: v.name, recipe })),
            Reading::Unusable => Reading::Unusable,
        },
    }
}

/// The spelling of a project kind in the sidecar.
pub fn project_text(project: Project) -> &'static str {
    match project {
        Project::Exported => "exported",
    }
}

/// A project mark the model can take.
fn project(f: Field<String>, notes: &mut Vec<String>) -> Reading<Option<Project>> {
    match f {
        Field::Absent => Reading::Usable(None),
        Field::Value(s) if s == project_text(Project::Exported) => Reading::Usable(Some(Project::Exported)),
        Field::Value(s) => {
            notes.push(format!("the folder's project kind {s:?} is not one Dalica knows: kept in the file, not used"));
            Reading::Unusable
        }
        Field::WrongType(_) => Reading::Unusable,
    }
}

/// Reads the sidecar's `"folder"` object.
pub fn read_folder(sidecar: &Sidecar) -> FolderReading {
    let f = sidecar.folder();
    let mut notes = Vec::new();
    FolderReading {
        description: capped(f.description, MAX_DESCRIPTION, dalica_library::clean_description, "folder description", &mut notes),
        date: dated(f.date),
        place: capped(f.place, MAX_PLACE, dalica_library::clean_place, "folder place", &mut notes),
        hidden: plain(f.hidden),
        project: project(sidecar.project(), &mut notes),
        sort: sort_by(sidecar.folder_sort(), &mut notes),
        reverse: plain(sidecar.folder_reverse()),
        notes,
    }
}

/// Writes the model's values into `sidecar`, keeping everything else: only the keys whose value
/// differs from what the sidecar reads as are set or cleared. Places that hold a value of the
/// wrong type where a value must go (a `photos` or photo entry that is not an object, a
/// `"folder"` that is not one) are left alone and returned, one per place.
pub fn apply(state: &FolderState, sidecar: &mut Sidecar) -> Vec<WrongType> {
    let mut refused = Vec::new();
    let r = read_folder(sidecar);
    let info = &state.folder;
    let mut folder = Vec::new();
    if differs(&r.description, &info.description) {
        folder.push(FolderValue::Description(info.description.clone()));
    }
    if differs(&r.date, &info.date) {
        folder.push(FolderValue::Date(info.date));
    }
    if differs(&r.place, &info.place) {
        folder.push(FolderValue::Place(info.place.clone()));
    }
    if differs(&r.hidden, &info.hidden) {
        folder.push(FolderValue::Hidden(info.hidden));
    }
    let mut folder_refused = folder.iter().find_map(|v| sidecar.set_folder(v).err());
    if folder_refused.is_none() && differs(&r.project, &state.project) {
        folder_refused = sidecar.set_project(state.project.map(project_text)).err();
    }
    if folder_refused.is_none() && differs(&r.sort, &state.sort.by) {
        folder_refused = sidecar.set_folder_sort(sort_text(state.sort.by)).err();
    }
    if folder_refused.is_none() && differs(&r.reverse, &state.sort.reverse) {
        folder_refused = sidecar.set_folder_reverse(state.sort.reverse).err();
    }
    if differs(&read_album_names(sidecar).0, &state.album_names) {
        sidecar.set_album_names(&state.album_names);
    }
    if let Some(e) = folder_refused {
        refused.push(e);
    }
    for (name, photo) in &state.photos {
        let r = read_photo(sidecar, name);
        let i = &photo.info;
        let mut values = Vec::new();
        if differs(&r.star, &photo.star) {
            values.push(PhotoValue::Star(photo.star));
        }
        if differs(&r.caption, &i.caption) {
            values.push(PhotoValue::Caption(i.caption.clone()));
        }
        if differs(&r.tags, &i.tags) {
            values.push(PhotoValue::Tags(i.tags.clone()));
        }
        if differs(&r.hidden, &i.hidden) {
            values.push(PhotoValue::Hidden(i.hidden));
        }
        if differs(&r.edits, &i.edits) {
            values.push(PhotoValue::Edits(i.edits.clone()));
        }
        if differs(&r.redo, &i.redo) {
            values.push(PhotoValue::Redo(i.redo.clone()));
        }
        if differs(&r.date, &i.date) {
            values.push(PhotoValue::Date(i.date));
        }
        if differs(&r.changed, &photo.changed) {
            values.push(PhotoValue::Changed(photo.changed));
        }
        // Every key of one photo goes into the same entry: its first refusal says it all.
        let mut photo_refused = values.iter().find_map(|v| sidecar.set_photo(name, v).err());
        if photo_refused.is_none() && differs(&r.saved_as, &photo.saved_as) {
            let copy = photo.saved_as.as_ref().map(|c| dalica_sidecar::SavedAs { name: c.name.clone(), recipe: c.recipe.clone() });
            photo_refused = sidecar.set_saved_as(name, copy.as_ref()).err();
        }
        // Only memberships of the model's albums are the model's to write.
        let known = |a: &AlbumId| state.known_albums.contains(a);
        let ours = match r.albums {
            Reading::Usable(m) => Reading::Usable(m.into_iter().filter(|(a, _)| known(a)).collect()),
            Reading::Unusable => Reading::Unusable,
        };
        let members: BTreeMap<AlbumId, PosKey> = photo.albums.iter().filter(|(a, _)| known(a)).map(|(a, k)| (a.clone(), k.clone())).collect();
        if photo_refused.is_none() && differs(&ours, &members) {
            photo_refused = sidecar.set_photo_albums(name, &members, known).err();
        }
        if let Some(e) = photo_refused {
            refused.push(e);
        }
    }
    refused
}

/// The star a sidecar holds for a photo named `name` (`None` when the value is of the wrong type).
pub fn star_of(sidecar: &Sidecar, name: &str) -> Option<bool> {
    match plain(sidecar.star(name)) {
        Reading::Usable(on) => Some(on),
        Reading::Unusable => None,
    }
}

/// What a folder's originals record gives the model: the saved records it may use, by photo file
/// name, and why the others are not used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OriginalsReading {
    /// (file name, record) for every entry with a plain name and values within the caps, in name
    /// order.
    pub saved: Vec<(String, Saved)>,
    /// One sentence per entry or value not used (it stays in the file).
    pub notes: Vec<String>,
}

/// Reads an originals record ([`Originals`]): entries whose name is not one plain path component
/// or that hold a value of the wrong type are kept in the file and reported, never used; so is
/// one with a digest or a recipe past the library's caps.
pub fn read_originals(record: &Originals) -> OriginalsReading {
    let mut out = OriginalsReading::default();
    for p in record.problems() {
        out.notes.push(format!("{}{} should be {}", originals::DIR_NAME, p.pointer, p.expected));
    }
    for (name, entry) in record.entries() {
        if entry.digest.len() > MAX_DIGEST {
            out.notes.push(format!("{name}: the digest is longer than {MAX_DIGEST} bytes: kept in the file, not used"));
            continue;
        }
        let mut notes = Vec::new();
        match operations(Field::Value(entry.recipe.clone()), "saved recipe", &mut notes) {
            Reading::Usable(_) => out.saved.push((name, saved_of(entry))),
            Reading::Unusable => out.notes.extend(notes.into_iter().map(|n| format!("{name}: {n}"))),
        }
    }
    out
}

/// The model's record for an originals record entry (no caps checked: see [`read_originals`]).
pub fn saved_of(entry: Entry) -> Saved {
    Saved {
        recipe: entry.recipe,
        size: entry.size,
        mtime: Timestamp::from_millis(entry.mtime),
        width: entry.width,
        height: entry.height,
        digest: entry.digest,
        state: match entry.state {
            State::Pending => SaveState::Pending,
            State::Done => SaveState::Done,
        },
        time: entry.time.map(Timestamp::from_millis),
    }
}

/// The originals record entry for a model record: what a Save writes ([`Originals::set`]).
pub fn originals_entry(saved: &Saved) -> Entry {
    Entry {
        recipe: saved.recipe.clone(),
        size: saved.size,
        mtime: saved.mtime.as_millis(),
        width: saved.width,
        height: saved.height,
        digest: saved.digest.clone(),
        state: match saved.state {
            SaveState::Pending => State::Pending,
            SaveState::Done => State::Done,
        },
        time: saved.time.map(Timestamp::as_millis),
    }
}
