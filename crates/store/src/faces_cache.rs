//! The embeddings cache (`faces.md` §4–§6, `data-model.md` §4): what the face scan worked out
//! per face (its embedding and the detector's five landmarks) and which photos the detector has
//! searched, one file per recogniser beside the index, so switching back to a model does not start
//! over and one model's results are never mixed into another's. Derived data: never in a sidecar,
//! rebuilt by scanning again, deleted with its model.
//!
//! - **One file per model id**: `faces-<model id>.cache` in the library directory ([`path`]). Its
//!   header names the model by id and SHA-256 and gives the embedding dimension; a file whose
//!   header names another model (the id's file replaced by another version) or another dimension,
//!   or that is damaged, is never read: it is started again, empty ([`Opened::Replaced`]).
//! - **Appended**: [`FacesCache::append`] writes a batch of [`Record`]s at the end through
//!   `dalica-fs`'s [`Appender`] (synced; a failed append leaves the file as it was) and then
//!   applies them in memory, so memory and file agree. Each record is framed with its length and
//!   a CRC-32; reading stops at the first record that is cut off or fails its checksum, and that
//!   torn tail (a crash while appending) is cut off before the next append, so it costs only
//!   itself ([`Opened::TornTail`]). A whole record that does not hold what its kind says (a
//!   rectangle that is empty, a number that is not finite, an embedding of another dimension) is
//!   skipped. [`FacesCache::rewrite`] writes the live records afresh (atomic replace), dropping
//!   what later records replaced.
//! - **Keyed by photo and file**: a record names the photo by its index id together with the
//!   file's size and modification time ([`PhotoStamp`]); a lookup with another size or time finds
//!   nothing, so a file changed on disk is searched again, and an index rebuilt with new photo ids
//!   never reads another photo's faces. A record for a photo with another stamp starts that photo
//!   afresh. Faces are keyed by their rectangle (stored frame, six decimals).
//! - **Deleted with its model** ([`remove`]): only the file of that model id, and only a regular
//!   file of the cache's own name shape; [`list`] names the model ids that have one.
//!
//! Layout (little-endian): the header is `"DLFC"`, version `1`, the tag's length (`u16`), the tag
//! (`<model id>@<sha256 hex>`), the dimension (`u32`) and the CRC-32 of those bytes. Each record is
//! its payload's length (`u32`), the payload and the payload's CRC-32. A payload is a kind byte
//! and its fields: `1` a face (photo id `u64`, size `u64`, modification time `i64` milliseconds,
//! the rectangle as four `f64`, a landmarks flag byte, ten `f32` landmark coordinates when it is
//! `1`, then `dim` `f32`), `2` a photo searched (id, size, time), `3` a photo forgotten (id: Find
//! Faces Again, or its faces are to be made again).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dalica_core::{PhotoId, Rect01, Timestamp};
use dalica_fs::{Appender, Expect};

use crate::crc32;

/// The file's first bytes.
pub const MAGIC: &[u8; 4] = b"DLFC";
/// The format version this crate writes and reads.
pub const VERSION: u8 = 1;
/// The largest cache file read or grown.
pub const MAX_FILE: u64 = 1 << 30;
/// The largest embedding dimension.
pub const MAX_DIM: u32 = 4096;
/// The start of a cache file's name.
pub const PREFIX: &str = "faces-";
/// The end of a cache file's name.
pub const SUFFIX: &str = ".cache";
/// The longest model id (the `people.model` setting's rule).
pub const MAX_MODEL_ID: usize = 64;

const KIND_FACE: u8 = 1;
const KIND_SEARCHED: u8 = 2;
const KIND_FORGET: u8 = 3;
/// A photo's stamp: id, size and time.
const STAMP_BYTES: usize = 24;
/// A face record without its embedding: kind, stamp, rectangle, flag, landmarks.
const FACE_FIXED: usize = 1 + STAMP_BYTES + 32 + 1 + 40;

/// Why the cache could not do what was asked.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The model id or SHA-256 cannot name a cache (`faces.md` §3: ids are 1–64 lowercase letters,
    /// digits, `.`, `_` or `-`, starting with a letter or digit; a SHA-256 is 64 lowercase hex
    /// digits).
    #[error("not a model a cache can be kept for: {0}")]
    BadModel(String),
    /// A record that cannot be written (nothing was written).
    #[error("not a face record the cache can hold: {0}")]
    BadRecord(&'static str),
    /// The file would grow past [`MAX_FILE`] (nothing was written).
    #[error("the face cache is full ({MAX_FILE} bytes)")]
    Full,
    #[error(transparent)]
    Fs(#[from] dalica_fs::Error),
}

pub type Result<T, E = CacheError> = std::result::Result<T, E>;

/// The recogniser a cache belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model<'a> {
    /// The model id (`people.model`; names the file).
    pub id: &'a str,
    /// The model file's SHA-256, 64 lowercase hex digits (in the header).
    pub sha256: &'a str,
}

impl Model<'_> {
    fn check(&self) -> Result<()> {
        check_id(self.id)?;
        let hex = self.sha256.len() == 64 && self.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if hex { Ok(()) } else { Err(CacheError::BadModel("the SHA-256 is not 64 lowercase hex digits".into())) }
    }

    fn tag(&self) -> String {
        format!("{}@{}", self.id, self.sha256)
    }
}

/// Whether `id` may name a model's cache.
fn check_id(id: &str) -> Result<()> {
    let ok_char = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-');
    let first = id.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if first && id.len() <= MAX_MODEL_ID && id.bytes().all(ok_char) {
        Ok(())
    } else {
        Err(CacheError::BadModel("the model id is not 1 to 64 lowercase letters, digits, '.', '_' or '-'".into()))
    }
}

/// The cache file of model `id` in the library directory `dir`.
pub fn path(dir: &Path, id: &str) -> Result<PathBuf> {
    check_id(id)?;
    Ok(dir.join(format!("{PREFIX}{id}{SUFFIX}")))
}

/// Whether `name` has the shape of a cache file's name.
fn shaped(name: &str) -> bool {
    name.strip_prefix(PREFIX).and_then(|n| n.strip_suffix(SUFFIX)).is_some_and(|id| check_id(id).is_ok())
}

/// Deletes model `id`'s cache in `dir`, if there is one (`Ok(false)` when there is none). Only a
/// regular file of the cache's own name shape is removed.
pub fn remove(dir: &Path, id: &str) -> Result<bool> {
    Ok(dalica_fs::pack::remove_shaped(&path(dir, id)?, shaped)?)
}

/// The model ids with a cache in `dir`, sorted.
pub fn list(dir: &Path) -> Result<Vec<String>> {
    let files = dalica_fs::pack::list_shaped(dir, shaped)?;
    Ok(files.into_iter().filter_map(|f| Some(f.name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?.to_string())).collect())
}

/// A photo as the cache knows it: its index id and its file's size and modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhotoStamp {
    pub id: PhotoId,
    pub size: u64,
    pub mtime: Timestamp,
}

impl PhotoStamp {
    /// The stamp of a photo of the model.
    pub fn of(photo: &dalica_library::Photo) -> PhotoStamp {
        PhotoStamp { id: photo.id, size: photo.size, mtime: photo.mtime }
    }
}

/// The detector's five landmarks (eyes, nose, mouth corners), each `[x, y]` normalised on the
/// photo in the stored frame.
pub type Landmarks = [[f32; 2]; 5];

/// What the scan worked out for one face.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedFace {
    pub rect: Rect01,
    pub landmarks: Option<Landmarks>,
    /// Unit length, the model's dimension, every value finite.
    pub embedding: Vec<f32>,
}

/// What the cache holds for one photo.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedPhoto {
    pub stamp: PhotoStamp,
    /// The detector has searched this photo (so a face the user removed is not found again).
    pub searched: bool,
    /// One per rectangle, in the order they came.
    pub faces: Vec<CachedFace>,
}

/// One change to the cache.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    /// A face's embedding and landmarks (replacing one with the same rectangle).
    Face { photo: PhotoStamp, face: CachedFace },
    /// The detector searched the photo.
    Searched { photo: PhotoStamp },
    /// Forget everything about the photo (Find Faces Again).
    Forget { photo: PhotoId },
}

/// What [`FacesCache::open`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// There was no cache for the model: a new one was started.
    New,
    /// The cache was read whole; `skipped` whole records held nothing usable.
    Loaded { records: u64, skipped: u64 },
    /// The cache ended in a torn or damaged record: the records before it were kept and the
    /// `dropped` bytes after them cut off.
    TornTail { records: u64, skipped: u64, dropped: u64 },
    /// The file was another model's, of another dimension or damaged: started again, empty.
    Replaced { why: String },
}

/// One model's open embeddings cache.
#[derive(Debug)]
pub struct FacesCache {
    path: PathBuf,
    dim: u32,
    photos: HashMap<PhotoId, CachedPhoto>,
    faces: usize,
    appender: Appender,
}

fn header(model: &Model, dim: u32) -> Vec<u8> {
    let tag = model.tag();
    let mut h = Vec::with_capacity(15 + tag.len());
    h.extend_from_slice(MAGIC);
    h.push(VERSION);
    // A tag is at most 64 + 1 + 64 bytes (checked).
    h.extend_from_slice(&u16::try_from(tag.len()).unwrap_or(u16::MAX).to_le_bytes());
    h.extend_from_slice(tag.as_bytes());
    h.extend_from_slice(&dim.to_le_bytes());
    let crc = crc32(&h);
    h.extend_from_slice(&crc.to_le_bytes());
    h
}

/// A little-endian reader over a slice that never reads past its end.
struct Cursor<'a> {
    bytes: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.bytes.split_at_checked(n)?;
        self.bytes = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.array()?)).filter(|v| v.is_finite())
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.array()?)).filter(|v| v.is_finite())
    }

    fn stamp(&mut self) -> Option<PhotoStamp> {
        let id = PhotoId::new(self.u64()?);
        let size = self.u64()?;
        let mtime = Timestamp::from_millis(i64::from_le_bytes(self.array()?));
        Some(PhotoStamp { id, size, mtime })
    }
}

fn put_stamp(out: &mut Vec<u8>, s: &PhotoStamp) {
    out.extend_from_slice(&s.id.get().to_le_bytes());
    out.extend_from_slice(&s.size.to_le_bytes());
    out.extend_from_slice(&s.mtime.as_millis().to_le_bytes());
}

/// The payload of `record` (kind and fields), checked against the cache's dimension.
fn encode(record: &Record, dim: u32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match record {
        Record::Face { photo, face } => {
            if u32::try_from(face.embedding.len()).ok() != Some(dim) {
                return Err(CacheError::BadRecord("the embedding is not of the model's dimension"));
            }
            let landmarks = face.landmarks.iter().flatten().flatten();
            if !face.embedding.iter().chain(landmarks.clone()).all(|v| v.is_finite()) {
                return Err(CacheError::BadRecord("a number that is not finite"));
            }
            out.reserve(FACE_FIXED + face.embedding.len() * 4);
            out.push(KIND_FACE);
            put_stamp(&mut out, photo);
            for v in [face.rect.x0(), face.rect.y0(), face.rect.x1(), face.rect.y1()] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.push(u8::from(face.landmarks.is_some()));
            let zero = [[0.0f32; 2]; 5];
            for v in face.landmarks.as_ref().unwrap_or(&zero).iter().flatten() {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in &face.embedding {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Record::Searched { photo } => {
            out.push(KIND_SEARCHED);
            put_stamp(&mut out, photo);
        }
        Record::Forget { photo } => {
            out.push(KIND_FORGET);
            out.extend_from_slice(&photo.get().to_le_bytes());
        }
    }
    Ok(out)
}

/// The record a payload holds, or `None` when it does not hold what its kind says.
fn decode(payload: &[u8], dim: u32) -> Option<Record> {
    let mut c = Cursor { bytes: payload };
    let record = match c.u8()? {
        KIND_FACE => {
            let photo = c.stamp()?;
            let rect = Rect01::new(c.f64()?, c.f64()?, c.f64()?, c.f64()?).ok()?;
            let flag = c.u8()?;
            let mut points = [[0.0f32; 2]; 5];
            for p in &mut points {
                *p = [c.f32()?, c.f32()?];
            }
            let landmarks = match flag {
                0 => None,
                1 => Some(points),
                _ => return None,
            };
            let n = usize::try_from(dim).ok()?;
            let mut embedding = Vec::with_capacity(n);
            for _ in 0..n {
                embedding.push(c.f32()?);
            }
            Record::Face { photo, face: CachedFace { rect, landmarks, embedding } }
        }
        KIND_SEARCHED => Record::Searched { photo: c.stamp()? },
        KIND_FORGET => Record::Forget { photo: PhotoId::new(c.u64()?) },
        _ => return None,
    };
    c.bytes.is_empty().then_some(record)
}

/// `payload` framed: its length, itself, its CRC-32.
fn frame(payload: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&crc32(payload).to_le_bytes());
}

/// The longest payload a cache of dimension `dim` holds.
fn max_payload(dim: u32) -> usize {
    FACE_FIXED + usize::try_from(dim).unwrap_or(0) * 4
}

/// The records `body` (the bytes after the header) holds up to the first torn or damaged one:
/// (records, whole records skipped, the length of the good part).
fn records(body: &[u8], dim: u32) -> (Vec<Record>, u64, usize) {
    let (mut out, mut skipped, mut good) = (Vec::new(), 0u64, 0usize);
    let mut c = Cursor { bytes: body };
    while let Some(len) = c.u32().and_then(|l| usize::try_from(l).ok()) {
        if len == 0 || len > max_payload(dim) {
            break;
        }
        let (Some(payload), Some(crc)) = (c.take(len), c.u32()) else { break };
        if crc32(payload) != crc {
            break;
        }
        good = good.saturating_add(len).saturating_add(8);
        match decode(payload, dim) {
            Some(r) => out.push(r),
            None => skipped += 1,
        }
    }
    (out, skipped, good)
}

impl FacesCache {
    /// Opens `model`'s cache in the library directory `dir` for embeddings of `dim` values, reading
    /// what it holds; a file of another model or dimension, or a damaged one, is started again
    /// (atomic replace over the version read), and a torn tail is cut off. Any thread but the UI
    /// thread may call it.
    pub fn open(dir: &Path, model: &Model, dim: u32) -> Result<(FacesCache, Opened)> {
        model.check()?;
        if dim == 0 || dim > MAX_DIM {
            return Err(CacheError::BadModel(format!("an embedding dimension of {dim} (1 to {MAX_DIM})")));
        }
        let path = path(dir, model.id)?;
        let head = header(model, dim);
        let read = dalica_fs::read(&path, MAX_FILE)?;
        let (opened, records) = match &read {
            None => {
                dalica_fs::replace(&path, &head, Expect::Absent)?;
                (Opened::New, Vec::new())
            }
            Some(r) => match r.bytes.strip_prefix(head.as_slice()) {
                Some(body) => {
                    let (records, skipped, good) = records(body, dim);
                    let count = u64::try_from(records.len()).unwrap_or(u64::MAX);
                    let dropped = body.len().saturating_sub(good);
                    if dropped == 0 {
                        (Opened::Loaded { records: count, skipped }, records)
                    } else {
                        // Cut the torn tail so the next record is not appended after it.
                        let keep = u64::try_from(head.len().saturating_add(good)).unwrap_or(u64::MAX);
                        dalica_fs::pack::PackFile::open(&path)?.truncate(keep)?;
                        (Opened::TornTail { records: count, skipped, dropped: u64::try_from(dropped).unwrap_or(u64::MAX) }, records)
                    }
                }
                None => {
                    let why = why_not(&r.bytes, model, dim);
                    dalica_fs::replace(&path, &head, Expect::Unchanged(r.identity))?;
                    (Opened::Replaced { why }, Vec::new())
                }
            },
        };
        let appender = Appender::open(&path, MAX_FILE)?;
        let mut cache = FacesCache { path, dim, photos: HashMap::new(), faces: 0, appender };
        for r in records {
            cache.apply(r);
        }
        Ok((cache, opened))
    }

    /// Appends `records` (one synced write) and then applies them, in order. A record that cannot
    /// be written (an embedding of another dimension, a number that is not finite) refuses the
    /// whole batch, and so does a cache that would grow past [`MAX_FILE`]; on any error nothing is
    /// applied and the file holds what it held.
    pub fn append(&mut self, records: &[Record]) -> Result<()> {
        let mut bytes = Vec::new();
        for r in records {
            frame(&encode(r, self.dim)?, &mut bytes);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        match self.appender.append(&bytes) {
            Err(dalica_fs::Error::CapExceeded { .. }) => return Err(CacheError::Full),
            other => other?,
        }
        for r in records {
            self.apply(r.clone());
        }
        Ok(())
    }

    fn apply(&mut self, record: Record) {
        let fresh = |stamp: PhotoStamp| CachedPhoto { stamp, searched: false, faces: Vec::new() };
        match record {
            Record::Face { photo, face } => {
                let entry = self.photos.entry(photo.id).or_insert_with(|| fresh(photo));
                if entry.stamp != photo {
                    self.faces = self.faces.saturating_sub(entry.faces.len());
                    *entry = fresh(photo);
                }
                match entry.faces.iter_mut().find(|f| f.rect == face.rect) {
                    Some(slot) => *slot = face,
                    None => {
                        entry.faces.push(face);
                        self.faces += 1;
                    }
                }
            }
            Record::Searched { photo } => {
                let entry = self.photos.entry(photo.id).or_insert_with(|| fresh(photo));
                if entry.stamp != photo {
                    self.faces = self.faces.saturating_sub(entry.faces.len());
                    *entry = fresh(photo);
                }
                entry.searched = true;
            }
            Record::Forget { photo } => {
                if let Some(gone) = self.photos.remove(&photo) {
                    self.faces = self.faces.saturating_sub(gone.faces.len());
                }
            }
        }
    }

    /// What the cache holds for the photo, when its file is still the one stamped.
    pub fn photo(&self, stamp: &PhotoStamp) -> Option<&CachedPhoto> {
        self.photos.get(&stamp.id).filter(|p| p.stamp == *stamp)
    }

    /// Whether the detector has searched the photo (its file as stamped).
    pub fn searched(&self, stamp: &PhotoStamp) -> bool {
        self.photo(stamp).is_some_and(|p| p.searched)
    }

    /// The cached face at `rect` of the photo (its file as stamped).
    pub fn face(&self, stamp: &PhotoStamp, rect: &Rect01) -> Option<&CachedFace> {
        self.photo(stamp)?.faces.iter().find(|f| f.rect == *rect)
    }

    /// Every photo the cache holds something for (any stamp), in no particular order.
    pub fn photos(&self) -> impl Iterator<Item = &CachedPhoto> {
        self.photos.values()
    }

    /// How many faces the cache holds.
    pub fn faces(&self) -> usize {
        self.faces
    }

    pub fn dim(&self) -> u32 {
        self.dim
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes the cache afresh with only what it holds now (atomic replace over the file as this
    /// cache wrote it), dropping the records later ones replaced or forgot.
    pub fn rewrite(&mut self, model: &Model) -> Result<()> {
        model.check()?;
        let mut bytes = header(model, self.dim);
        let mut ids: Vec<&PhotoId> = self.photos.keys().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(p) = self.photos.get(id) else { continue };
            for face in &p.faces {
                frame(&encode(&Record::Face { photo: p.stamp, face: face.clone() }, self.dim)?, &mut bytes);
            }
            if p.searched {
                frame(&encode(&Record::Searched { photo: p.stamp }, self.dim)?, &mut bytes);
            }
        }
        let identity = dalica_fs::identity(&self.path)?;
        dalica_fs::replace(&self.path, &bytes, identity.map_or(Expect::Absent, Expect::Unchanged))?;
        self.appender = Appender::open(&self.path, MAX_FILE)?;
        Ok(())
    }
}

/// Why a file that does not start with `model`'s header cannot be read.
fn why_not(bytes: &[u8], model: &Model, dim: u32) -> String {
    let Some(rest) = bytes.strip_prefix(MAGIC.as_slice()) else { return "not a face cache".into() };
    let mut c = Cursor { bytes: rest };
    if c.u8() != Some(VERSION) {
        return "a face cache of another version".into();
    }
    let tag = c.array::<2>().map(u16::from_le_bytes).and_then(|n| c.take(usize::from(n)));
    if tag != Some(model.tag().as_bytes()) {
        return "the face cache of another model".into();
    }
    if c.u32() != Some(dim) {
        return "a face cache of another embedding dimension".into();
    }
    "a damaged face cache".into()
}
