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

use std::path::PathBuf;

use dalica_core::{Date, DateTime, FolderId};
use dalica_library::{FolderInfo, Library, MAX_CAPTION, MAX_DESCRIPTION, MAX_OPERATION, MAX_OPERATIONS, MAX_PLACE, PhotoInfo};
use dalica_sidecar::{Field, FolderValue, PhotoValue, Sidecar, WrongType};

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
}

/// The sidecar-relevant state of `folder`, or `None` if it is not in the model.
pub fn folder_state(library: &Library, folder: FolderId) -> Option<FolderState> {
    let f = library.folder(folder)?;
    let photos =
        library.photos_in(folder).into_iter().map(|p| (p.name.clone(), PhotoState { star: p.star, info: library.info(p.id).clone() })).collect();
    Some(FolderState { id: folder, path: f.path.clone(), folder: library.folder_info(folder).clone(), photos })
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
        notes,
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
    if let Some(e) = folder.iter().find_map(|v| sidecar.set_folder(v).err()) {
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
        // Every key of one photo goes into the same entry: its first refusal says it all.
        if let Some(e) = values.iter().find_map(|v| sidecar.set_photo(name, v).err()) {
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
