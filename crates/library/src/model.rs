//! The model's data: folders, photos and their sketches, and the [`Library`] that owns them with
//! its revisions.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use serde::{Deserialize, Serialize};

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

/// The data that ops change: everything except counters. Two libraries with equal models show the
/// same photos with the same stars and the same sketches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
    pub(crate) folders: BTreeMap<FolderId, Folder>,
    pub(crate) photos: BTreeMap<PhotoId, Photo>,
    /// Derived data beside the photos: only photos in `photos` have one.
    pub(crate) sketches: BTreeMap<PhotoId, Sketch>,
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
}

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
