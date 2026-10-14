//! The model's data: folders, photos, what the user said about them (M6: captions, tags, hidden,
//! recipes, dates, folder info), their sketches and synced hashes, what Save and Export leave
//! (M8: saved records, copies saved beside, project marks), and the [`Library`] that owns them
//! with its revisions and its tag index.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::OnceLock;

use dalica_core::{Date, DateTime, FolderId, PhotoId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::tags::TagIndex;

/// A folder of photos, by its absolute path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub path: PathBuf,
}

/// One photo file and what Dalica knows about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub id: PhotoId,
    pub folder: FolderId,
    /// The file name inside its folder.
    pub name: String,
    /// Bytes on disk.
    pub size: u64,
    /// File modification time.
    pub mtime: Timestamp,
    /// Size as displayed (after EXIF orientation); 0 × 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// Capture date (EXIF DateTimeOriginal), if the file has one.
    pub date: Option<DateTime>,
    pub star: bool,
}

/// What the user has said about one photo (`data-model.md` §2–§3), kept in its folder's sidecar.
/// Held beside [`Photo`] (which is what the file says), so a rescan that updates a photo never
/// touches it. The default (no caption, no tags, shown, no recipe, no date of the user's) is not
/// stored: a photo whose info returns to the default leaves the map, so equal models compare equal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhotoInfo {
    /// At most [`MAX_CAPTION`](crate::MAX_CAPTION) bytes; empty for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub caption: String,
    /// Clean tags ([`clean_tag`](crate::clean_tag)), no two equal ignoring case, in the order
    /// added; at most [`MAX_TAGS`](crate::MAX_TAGS).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Hidden with Picture ▸ Hide.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// The edit recipe: operation strings in the order applied (`data-model.md` §3.1), opaque here
    /// (`edit` parses them).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<String>,
    /// Operations undone in the edit view that Redo can bring back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub redo: Vec<String>,
    /// A date taken set by the user (Adjust Date and Time). The file's own date stays in
    /// [`Photo::date`]; the effective date is this one, else the file's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime>,
}

impl PhotoInfo {
    /// The default info, as a constant (for references to "nothing said").
    pub const EMPTY: PhotoInfo =
        PhotoInfo { caption: String::new(), tags: Vec::new(), hidden: false, edits: Vec::new(), redo: Vec::new(), date: None };

    pub fn is_empty(&self) -> bool {
        *self == PhotoInfo::EMPTY
    }

    /// The recipe and redo list together.
    pub fn recipe(&self) -> Recipe {
        Recipe { edits: self.edits.clone(), redo: self.redo.clone() }
    }
}

/// A photo's edit recipe and redo list, as [`Op::SetRecipes`](crate::Op::SetRecipes) sets them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Recipe {
    pub edits: Vec<String>,
    pub redo: Vec<String>,
}

/// What the user has said about one folder (Folder Properties, Hide Folder), kept in the sidecar's
/// `"folder"` object. Like [`PhotoInfo`], the default is not stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FolderInfo {
    /// At most [`MAX_DESCRIPTION`](crate::MAX_DESCRIPTION) bytes; empty for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The folder's date when the user set one; `None` uses the oldest photo's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<Date>,
    /// At most [`MAX_PLACE`](crate::MAX_PLACE) bytes; empty for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub place: String,
    /// Hidden with Hide Folder: the folder leaves Folders for Hidden Folders.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl FolderInfo {
    pub const EMPTY: FolderInfo = FolderInfo { description: String::new(), date: None, place: String::new(), hidden: false };

    pub fn is_empty(&self) -> bool {
        *self == FolderInfo::EMPTY
    }
}

/// The last-synced hashes of a photo's owned metadata fields (`xmp-interop.md` §5): a hash of each
/// field's value as last read from or written to the file, `None` when never synced. Index-only
/// derived data like the [`Sketch`]: never in a sidecar, never on an undo stack, no library
/// revision bump. The hash function is the engine's; the library only stores the numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Synced {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<u64>,
}

/// The most bytes of a [`Saved::digest`] (`security.md` §4.4: input-derived text is capped).
pub const MAX_DIGEST: usize = 256;
/// The most bytes of a [`SavedAs::name`]: a file name, with room for any platform's.
pub const MAX_FILE_NAME: usize = 1024;

/// Where a Save stands (`data-model.md` §6): its record is written as pending before anything
/// moves and marked done last, so a pending record found at open or scan is a Save to repair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveState {
    #[default]
    Pending,
    Done,
}

/// A photo's saved record (`data-model.md` §6): the photo was saved in place and its original is
/// kept in the folder's `.dalica-originals/`. Read from `.dalica-originals/.dalica.json`, so it is
/// index-only like the [`Sketch`]: never in the folder sidecar, and
/// [`Op::SetSaved`](crate::Op::SetSaved) alone is derived (no undo step, no revision bump), so
/// loading records moves no cache. A Save, Undo Save or Revert sets or clears it in one batch with
/// the recipe ([`Library::save_op`], [`Library::undo_save_op`], [`Library::revert_op`]), which is
/// not derived.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    /// The whole recipe the saved file holds (operation strings, opaque here): a second Save
    /// renders this followed by the new edits from the kept original and records them all.
    pub recipe: Vec<String>,
    /// The kept original's bytes on disk.
    pub size: u64,
    /// The kept original's modification time.
    pub mtime: Timestamp,
    /// The kept original's size in pixels (as displayed); 0 × 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// The kept original's digest, as text (opaque here: the writer's), at most [`MAX_DIGEST`]
    /// bytes.
    pub digest: String,
    pub state: SaveState,
    /// When the Save was made, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<Timestamp>,
}

impl Saved {
    /// Whether the Save finished, so Undo Save and Revert apply.
    pub fn is_done(&self) -> bool {
        self.state == SaveState::Done
    }

    pub(crate) fn check(&self) -> crate::Result<()> {
        crate::tags::check_list(&self.recipe)?;
        if self.digest.len() > MAX_DIGEST {
            return Err(crate::Error::TooLong { what: "digest", max: MAX_DIGEST });
        }
        Ok(())
    }
}

/// The JPEG a Save wrote beside a photo Dalica cannot write in place (`data-model.md` §6): a new,
/// unedited photo of its own. The source keeps its recipe and this record, so it counts as unsaved
/// again only once its recipe differs from the one the copy holds. Kept in the folder sidecar
/// (`savedAs`), so its op is an ordinary, undoable one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedAs {
    /// The copy's file name in the same folder: not empty, at most [`MAX_FILE_NAME`] bytes.
    pub name: String,
    /// The recipe the copy was rendered with.
    pub recipe: Vec<String>,
}

impl SavedAs {
    pub(crate) fn check(&self) -> crate::Result<()> {
        if self.name.is_empty() {
            return Err(crate::Error::BadText { what: "file name", why: "empty" });
        }
        if self.name.len() > MAX_FILE_NAME {
            return Err(crate::Error::TooLong { what: "file name", max: MAX_FILE_NAME });
        }
        crate::tags::check_list(&self.recipe)
    }
}

/// What kind of project folder a folder is (`organizing.md` §10, `creating.md` §2): the tree lists
/// marked folders in Projects wherever they are. Kept in the folder sidecar
/// (`"project": "exported"`), so the mark survives moves and rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Project {
    /// A folder an export made.
    Exported,
}

/// The most bytes a [`Sketch`] holds.
pub const MAX_SKETCH: usize = 32;

/// A photo's colour sketch (`architecture.md` §6.5): a tiny blurred preview, 1 to [`MAX_SKETCH`]
/// opaque bytes made by the thumbnailer. The library only stores it: it is derived data, kept in
/// the index (journal and snapshot) and never in a sidecar. Held inline (no allocation), so every
/// photo's sketch can stay in memory. In JSON it is one base64 string (standard alphabet, no
/// padding); anything longer than [`MAX_SKETCH`] bytes, empty or not canonical base64 is refused.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sketch {
    len: u8,
    /// `bytes[len..]` is always zero, so the derived equality compares only the sketch.
    bytes: [u8; MAX_SKETCH],
}

impl Sketch {
    /// A sketch of `bytes`, or `None` when they are empty or longer than [`MAX_SKETCH`].
    pub fn new(bytes: &[u8]) -> Option<Sketch> {
        let len = u8::try_from(bytes.len()).ok().filter(|&n| n > 0 && usize::from(n) <= MAX_SKETCH)?;
        let mut out = Sketch { len, bytes: [0; MAX_SKETCH] };
        out.bytes.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(out)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or_default()
    }

    /// The base64 text of the sketch (standard alphabet, no padding).
    pub fn to_base64(&self) -> String {
        base64::encode(self.as_bytes())
    }

    /// The sketch a base64 text holds, or why it holds none.
    pub fn from_base64(text: &str) -> Result<Sketch, String> {
        let bytes = base64::decode(text, MAX_SKETCH)?;
        Sketch::new(&bytes).ok_or_else(|| format!("a sketch holds 1 to {MAX_SKETCH} bytes, not {}", bytes.len()))
    }
}

impl std::fmt::Debug for Sketch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sketch({})", self.to_base64())
    }
}

impl Serialize for Sketch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base64())
    }
}

impl<'de> Deserialize<'de> for Sketch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Sketch, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Sketch;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "a base64 sketch of 1 to {MAX_SKETCH} bytes")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Sketch, E> {
                Sketch::from_base64(v).map_err(E::custom)
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

/// Base64 with the standard alphabet and no padding. Decoding is strict (canonical text only), so
/// every sketch has exactly one spelling and decoding then encoding gives the text back.
mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub(super) fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for k in 0..=chunk.len() {
                let six = (n >> (18 - 6 * k)) & 63;
                out.push(char::from(ALPHABET.get(six as usize).copied().unwrap_or(b'A')));
            }
        }
        out
    }

    fn value(c: u8) -> Option<u32> {
        ALPHABET.iter().position(|&a| a == c).map(|v| v as u32)
    }

    /// The bytes `text` encodes, refusing more than `max` of them before decoding anything.
    pub(super) fn decode(text: &str, max: usize) -> Result<Vec<u8>, String> {
        let text = text.as_bytes();
        if text.len() > max.div_ceil(3) * 4 {
            return Err(format!("a sketch of more than {max} bytes"));
        }
        let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
        for chunk in text.chunks(4) {
            if chunk.len() == 1 {
                return Err("base64 text of a wrong length".into());
            }
            let mut n = 0u32;
            for (k, &c) in chunk.iter().enumerate() {
                n |= value(c).ok_or_else(|| format!("{:?} is not a base64 character", char::from(c)))? << (18 - 6 * k);
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            let whole = chunk.len() - 1;
            // Bits past the last whole byte must be zero, or two texts would give the same bytes.
            if bytes.get(whole..).is_some_and(|rest| rest.iter().any(|&b| b != 0)) {
                return Err("base64 text with stray bits".into());
            }
            out.extend_from_slice(bytes.get(..whole).unwrap_or_default());
        }
        Ok(out)
    }
}

/// The data that ops change: everything except counters and indexes. Two libraries with equal
/// models show the same photos with the same stars, info, sketches, synced hashes, saved records,
/// copies saved beside and project marks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
    pub(crate) folders: BTreeMap<FolderId, Folder>,
    pub(crate) photos: BTreeMap<PhotoId, Photo>,
    /// What the user said about photos: only photos in `photos`, and never an empty info.
    pub(crate) photo_info: BTreeMap<PhotoId, PhotoInfo>,
    /// What the user said about folders: only folders in `folders`, never an empty info.
    pub(crate) folder_info: BTreeMap<FolderId, FolderInfo>,
    /// Derived data beside the photos: only photos in `photos` have one.
    pub(crate) sketches: BTreeMap<PhotoId, Sketch>,
    /// Derived data: only photos in `photos`, never all `None`.
    pub(crate) synced: BTreeMap<PhotoId, Synced>,
    /// Saved records (M8; index-only, from `.dalica-originals/.dalica.json`): only photos in
    /// `photos`.
    pub(crate) saved: BTreeMap<PhotoId, Saved>,
    /// Copies saved beside photos (M8; folder sidecar): only photos in `photos`.
    pub(crate) saved_as: BTreeMap<PhotoId, SavedAs>,
    /// Project marks of folders (M8; folder sidecar): only folders in `folders`.
    pub(crate) projects: BTreeMap<FolderId, Project>,
}

/// The library: the model, its revision counters and the next free ids.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub(crate) model: Model,
    /// (folder, file name) → photo.
    pub(crate) by_name: HashMap<(FolderId, String), PhotoId>,
    /// Absolute path → folder.
    pub(crate) by_path: HashMap<PathBuf, FolderId>,
    pub(crate) revision: u64,
    pub(crate) thumbnail_revision: u64,
    pub(crate) folder_revisions: HashMap<FolderId, u64>,
    pub(crate) next_folder: u64,
    pub(crate) next_photo: u64,
    /// Every tag in use, kept in step with `model.photo_info` by every op.
    pub(crate) tags: TagIndex,
    /// Each folder's count of photos with unsaved edits, worked out on first use after each
    /// revision (cleared by every op that bumps the revision; a derived op cannot change it).
    pub(crate) unsaved: OnceLock<HashMap<FolderId, usize>>,
}

static EMPTY_PHOTO_INFO: PhotoInfo = PhotoInfo::EMPTY;
static EMPTY_FOLDER_INFO: FolderInfo = FolderInfo::EMPTY;

impl Library {
    pub fn new() -> Library {
        Library::default()
    }

    /// The model, for comparisons and snapshots.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Bumped once by every applied op.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Bumped by every op that changes a sketch. Ops that change only sketches leave
    /// [`revision`](Library::revision) and the folder revisions alone, so the caches keyed on them
    /// (tree, layout) survive the thumbnailer filling sketches in.
    pub fn thumbnail_revision(&self) -> u64 {
        self.thumbnail_revision
    }

    /// The sketch of photo `id`, if it has one.
    pub fn sketch(&self, id: PhotoId) -> Option<&Sketch> {
        self.model.sketches.get(&id)
    }

    /// Every sketch, by photo id.
    pub fn sketches(&self) -> impl Iterator<Item = (PhotoId, &Sketch)> {
        self.model.sketches.iter().map(|(id, s)| (*id, s))
    }

    /// How many photos have a sketch.
    pub fn sketch_count(&self) -> usize {
        self.model.sketches.len()
    }

    /// What the user said about photo `id` ([`PhotoInfo::EMPTY`] for nothing, or an unknown id).
    pub fn info(&self, id: PhotoId) -> &PhotoInfo {
        self.model.photo_info.get(&id).unwrap_or(&EMPTY_PHOTO_INFO)
    }

    /// Every photo with something said about it, by id.
    pub fn infos(&self) -> impl Iterator<Item = (PhotoId, &PhotoInfo)> {
        self.model.photo_info.iter().map(|(id, i)| (*id, i))
    }

    /// Photo `id`'s caption ("" for none).
    pub fn caption(&self, id: PhotoId) -> &str {
        &self.info(id).caption
    }

    /// Photo `id`'s tags, in the order added.
    pub fn tags_of(&self, id: PhotoId) -> &[String] {
        &self.info(id).tags
    }

    /// Whether photo `id` is hidden.
    pub fn is_hidden(&self, id: PhotoId) -> bool {
        self.info(id).hidden
    }

    /// Photo `id`'s effective date taken: the user's date, else the file's.
    pub fn effective_date(&self, id: PhotoId) -> Option<DateTime> {
        self.info(id).date.or_else(|| self.model.photos.get(&id).and_then(|p| p.date))
    }

    /// The date `photo` sorts by: its effective date, else its file's modification time (UTC).
    pub fn sort_date(&self, photo: &Photo) -> Option<DateTime> {
        self.info(photo.id).date.or(photo.date).or_else(|| DateTime::from_unix_secs(photo.mtime.as_secs()))
    }

    /// What the user said about folder `id` ([`FolderInfo::EMPTY`] for nothing or an unknown id).
    pub fn folder_info(&self, id: FolderId) -> &FolderInfo {
        self.model.folder_info.get(&id).unwrap_or(&EMPTY_FOLDER_INFO)
    }

    /// Every folder with something said about it, by id.
    pub fn folder_infos(&self) -> impl Iterator<Item = (FolderId, &FolderInfo)> {
        self.model.folder_info.iter().map(|(id, i)| (*id, i))
    }

    /// Whether folder `id` is hidden (Hide Folder).
    pub fn is_folder_hidden(&self, id: FolderId) -> bool {
        self.folder_info(id).hidden
    }

    /// The last-synced hashes of photo `id`, if any field was ever synced.
    pub fn synced(&self, id: PhotoId) -> Option<&Synced> {
        self.model.synced.get(&id)
    }

    /// Every photo's synced hashes, by photo id.
    pub fn synced_all(&self) -> impl Iterator<Item = (PhotoId, &Synced)> {
        self.model.synced.iter().map(|(id, s)| (*id, s))
    }

    /// Photo `id`'s saved record, if it was saved in place (pending or done).
    pub fn saved(&self, id: PhotoId) -> Option<&Saved> {
        self.model.saved.get(&id)
    }

    /// Every saved record, by photo id.
    pub fn saved_all(&self) -> impl Iterator<Item = (PhotoId, &Saved)> {
        self.model.saved.iter().map(|(id, s)| (*id, s))
    }

    /// The copy saved beside photo `id`, if any.
    pub fn saved_as(&self, id: PhotoId) -> Option<&SavedAs> {
        self.model.saved_as.get(&id)
    }

    /// Every copy saved beside a photo, by photo id.
    pub fn saved_as_all(&self) -> impl Iterator<Item = (PhotoId, &SavedAs)> {
        self.model.saved_as.iter().map(|(id, s)| (*id, s))
    }

    /// Folder `id`'s project mark, if it has one.
    pub fn project(&self, id: FolderId) -> Option<Project> {
        self.model.projects.get(&id).copied()
    }

    /// Every folder with a project mark, by folder id.
    pub fn projects(&self) -> impl Iterator<Item = (FolderId, Project)> {
        self.model.projects.iter().map(|(id, p)| (*id, *p))
    }

    /// The tag index: every tag in use with its count.
    pub fn tag_index(&self) -> &TagIndex {
        &self.tags
    }

    /// The library revision at which something in `folder` (the folder or its photos) last changed;
    /// 0 if never.
    pub fn folder_revision(&self, folder: FolderId) -> u64 {
        self.folder_revisions.get(&folder).copied().unwrap_or(0)
    }

    /// A folder id no folder has had in this library.
    pub fn next_folder_id(&mut self) -> FolderId {
        let max = self.model.folders.keys().next_back().map_or(0, |id| id.get());
        self.next_folder = self.next_folder.max(max).saturating_add(1);
        FolderId::new(self.next_folder)
    }

    /// A photo id no photo has had in this library.
    pub fn next_photo_id(&mut self) -> PhotoId {
        let max = self.model.photos.keys().next_back().map_or(0, |id| id.get());
        self.next_photo = self.next_photo.max(max).saturating_add(1);
        PhotoId::new(self.next_photo)
    }

    pub(crate) fn touch(&mut self, folder: FolderId) {
        self.folder_revisions.insert(folder, self.revision);
    }
}
