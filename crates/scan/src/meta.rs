//! The metadata a scan imports from the photo files (`xmp-interop.md` §5, §7).
//!
//! - **Reading, in the same pass as the header.** When a walk finds a photo the model does not
//!   know at its size and modification time ([`Known`]; [`ReadMetadata`] in the scan's options),
//!   it reads the file's owned fields through `dalica-meta`. A JPEG, PNG, WebP or TIFF is read on
//!   the reading thread, with its header and from the bytes the header read already holds
//!   ([`crate::walk::HeaderBytesReader`]): a JPEG's metadata segments come before its image data,
//!   so it is read again (only up to its image data) only when they do not fit; a PNG, WebP or
//!   TIFF is read whole (at most [`MAX_FILE`]) when the header's bytes are not the whole file.
//!   A RAW is read on the scan's thread once the folder's headers are in, through its `.xmp`
//!   sidecar whatever the write option says (`xmp_sidecar::Sidecars` over the folder's names),
//!   else the XMP embedded in a TIFF-structured RAW such as a DNG. Every read runs inside
//!   `catch_unwind`; a file whose metadata cannot be read brings none ([`FoundPhoto::file`] stays
//!   `None`) and the scan goes on.
//! - **Deciding, in the diff** (a pure function of the model and what was read, on the owner
//!   thread with no file access). Per photo and field the rules of `dalica_library::sync`:
//!   - a photo Dalica has nothing for (no caption, tags, star, date or any other field, and never
//!     synced: no entry of its own in the folder sidecar) imports every value its file has, a
//!     rating of 5 as a star, with no undo step: a new photo in the same batch as the
//!     `AddPhotos` that brings it, a known one in the sync op below;
//!   - a value changed in another application since the last sync while Dalica's did not is
//!     imported (`Import`);
//!   - both changed (`Conflict`): Dalica's value is kept; the engine notes it for Properties when
//!     it applies the sync op ([`sync_parts`] gives it the hashes it needs);
//!   - the last-synced hashes become those of what was read (`Op::SetSynced`, index-only).
//!   - the search facts read (camera, video, GPS location: [`crate::facts`]) become the photo's
//!     (`Op::SetFacts`, index-only): a new photo's when it has any, a known one's when they changed
//!     (cleared when its file says nothing any more).
//! - **The sync op** ([`sync_parts`]): the diff's last op for the photos it read,
//!   `Batch[Batch(imports), Batch(first-scan fills), SetSynced, SetFacts]`. A caller applying it
//!   outside a command makes its first part one undoable step ([`undo_of_import`]); the rest is not
//!   undone (the synced hashes stay those of the file, so undoing an import writes Dalica's old
//!   value back rather than importing again; the facts are derived data).
//!
//! **The place** (M13.14, `organizing.md` §7) is the model's word on where the photo was taken
//! (`dalica_library::Geo`): a point, or the location the user removed. A file's GPS comes into the
//! model as follows, always with no undo step (the place a file holds is the photo's while the
//! user has said nothing else, so bringing it in changes nothing the user sees):
//! - a photo the model has no word on takes its file's GPS as its point (a new photo's in the
//!   engine's add batch, after its sidecar's word, `scanning.rs`; a known one's in the sync op's
//!   fills), so a library indexed before geotags were read gets them at the next scan:
//!   [`Known::of_placed`] leaves out the photos whose file held GPS when last read and that the
//!   model has no word on, so their files are read again;
//! - a point the file changed in another application while Dalica's did not follows the file (the
//!   word dropped when the file lost its GPS);
//! - a location the user removed (`"geo": null`) is never brought back, whatever the file says;
//! - changed on both sides, Dalica's point stays and [`conflicts`] names the place.
//!
//! **Faces** (M13.19, `xmp-interop.md` §6): the same read brings the file's MWG face regions
//! ([`FileValues::faces`]: boxes in the stored frame, each with the name the file gives it, if
//! any). The diff leaves them to its caller, the engine, which turns them into faces and people
//! (a name matched to a person ignoring case, else a new person) in the op that adds the photo and,
//! for a photo the model knows whose file changed, in the op that updates it: the faces of the file
//! that overlap none of the photo's (`dalica_store::metawrite::new_faces`), with no undo step.

use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::geo::LatLon;
use dalica_core::{DateTime, PhotoId, Timestamp};
use dalica_image::{Format, RawKind};
use dalica_library::sync::{self, Decision, Field, Values};
use dalica_library::{Facts, Geo, Library, Op, Photo, PhotoInfo, Synced, clean_caption, clean_tags};
use dalica_meta::fields::FaceRegion;
use dalica_meta::xmp_sidecar::{self, Sidecars};
use dalica_meta::{DateTaken, Metadata, OwnedFields};

use crate::walk::FoundPhoto;

/// The largest photo file whose metadata a scan reads whole (PNG, WebP, TIFF, TIFF-structured
/// RAW); a JPEG is read only up to its image data, whatever its size. The same cap as
/// `photo.metadata`'s reader.
pub const MAX_FILE: u64 = 1 << 30;

/// The first part of a JPEG read for its metadata; doubled until the metadata segments are in.
const JPEG_FIRST: u64 = 256 * 1024;

/// What a file says: the fields Dalica owns, cleaned as the library stores them (caption and tags
/// through `clean_caption` / `clean_tags`, a rating of 5 as a star, the date to the second), its
/// search facts ([`crate::facts`]: camera, video, GPS location), derived data the diff sets
/// without an undo step, and its face regions (M13.19, in file order; empty for none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileValues(pub Values, pub Facts, pub Vec<FaceRegion>);

/// The place is a `dalica_meta::Place` and a face's box a `Rect01`: always finite, so equality is
/// an equivalence.
impl Eq for FileValues {}

impl FileValues {
    /// A file's owned fields in the library's terms, with the facts they tell (a GPS location);
    /// the camera needs the EXIF summary: [`FileValues::read`].
    pub fn of(owned: &OwnedFields) -> FileValues {
        let values = Values {
            caption: owned.caption.as_deref().map(clean_caption).unwrap_or_default(),
            tags: clean_tags(owned.tags.iter().map(String::as_str)),
            star: owned.starred(),
            place: owned.place.map(|p| (p.latitude(), p.longitude())),
            date: owned.date_taken.as_ref().map(DateTaken::date_time),
        };
        FileValues(values, Facts { geotagged: owned.place.is_some(), ..Facts::default() }, owned.faces.clone())
    }

    /// The file's face regions: boxes in the stored frame (`data-model.md` §3.3) with the name the
    /// file gives each, if any (`xmp-interop.md` §6).
    pub fn faces(&self) -> &[FaceRegion] {
        &self.2
    }

    /// What a file of `format` whose metadata reads as `metadata` says: its owned fields and every
    /// search fact.
    pub fn read(format: Format, metadata: &Metadata) -> FileValues {
        let FileValues(values, _, faces) = FileValues::of(&metadata.owned);
        FileValues(values, crate::facts::of(format, metadata), faces)
    }
}

/// The photo files the model knows, by folder and name, at the size and modification time it
/// knows them: a scan reads the metadata of the others only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Known {
    folders: HashMap<PathBuf, HashMap<String, (u64, Timestamp)>>,
    photos: usize,
}

impl Known {
    /// Every photo of `library` (one pass over its folders and photos).
    pub fn of(library: &Library) -> Known {
        Known::of_photos(library, |_| true)
    }

    /// Every photo of `library` but the ones whose file held GPS when last read and that the model
    /// has no word on the place of (a library indexed before geotags were read into the model):
    /// their files are read again, so their place comes in (the engine's scans, M13.14).
    pub fn of_placed(library: &Library) -> Known {
        Known::of_photos(library, |p| library.geo(p.id).is_some() || !library.facts(p.id).geotagged)
    }

    fn of_photos(library: &Library, keep: impl Fn(&Photo) -> bool) -> Known {
        let paths: HashMap<_, &Path> = library.folders().map(|f| (f.id, f.path.as_path())).collect();
        let mut out = Known::default();
        for p in library.photos().filter(|p| keep(p)) {
            if let Some(path) = paths.get(&p.folder) {
                out.folders.entry(path.to_path_buf()).or_default().insert(p.name.clone(), (p.size, p.mtime));
                out.photos = out.photos.saturating_add(1);
            }
        }
        out
    }

    /// Whether the model knows `folder/name` at this size and time.
    pub fn knows(&self, folder: &Path, name: &str, size: u64, mtime: Timestamp) -> bool {
        self.folders.get(folder).and_then(|f| f.get(name)) == Some(&(size, mtime))
    }

    /// How many photos it knows.
    pub fn len(&self) -> usize {
        self.photos
    }

    pub fn is_empty(&self) -> bool {
        self.photos == 0
    }
}

/// Whether a scan reads the owned metadata of the photos it finds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ReadMetadata {
    /// Never (the default: a scan only lists).
    #[default]
    Off,
    /// The photos `Known` does not know at their size and modification time: new and changed
    /// files. An empty `Known` reads every photo (a rebuild).
    NewOrChanged(Arc<Known>),
}

impl ReadMetadata {
    /// Every photo found.
    pub fn all() -> ReadMetadata {
        ReadMetadata::NewOrChanged(Arc::new(Known::default()))
    }
}

/// Reads the owned fields of the RAWs in `photos` (one folder, `dir`) that `known` does not know,
/// into [`FoundPhoto::file`] (the other formats are read with their headers:
/// [`read_with_header`]). `names` are every file name of the folder (for the `.xmp` sidecars).
/// Returns `false` when `cancelled` stopped it before the end.
pub(crate) fn read_folder(
    dir: &Path,
    names: &[String],
    photos: &mut [FoundPhoto],
    known: &Known,
    limits: &dalica_meta::Limits,
    cancelled: &dyn Fn() -> bool,
) -> bool {
    let raws: Vec<&str> = photos.iter().filter(|p| matches!(p.format, Format::Raw(_))).map(|p| p.name.as_str()).collect();
    let sidecars = (!raws.is_empty()).then(|| Sidecars::new(names, |n| raws.contains(&n)));
    for p in photos.iter_mut() {
        if !matches!(p.format, Format::Raw(_)) || known.knows(dir, &p.name, p.size, p.mtime) {
            continue;
        }
        if cancelled() {
            return false;
        }
        let path = dir.join(&p.name);
        let sidecar = sidecars.as_ref().and_then(|s| s.get(&p.name)).and_then(|n| n.existing.as_deref()).map(|n| dir.join(n));
        let format = p.format;
        // The camera the header's EXIF gave on the reading thread ([`read_with_header`]), for the
        // containers `dalica-meta` does not read.
        let header = p.file.take().map(|f| f.1).unwrap_or_default();
        p.file = catch_unwind(AssertUnwindSafe(|| read_file(&path, format, sidecar.as_deref(), limits)))
            .ok()
            .flatten()
            .map(|FileValues(values, facts, faces)| FileValues(values, crate::facts::with_camera_of(facts, &header), faces));
    }
    true
}

/// A photo's header and, for a JPEG, PNG, WebP or TIFF, its owned fields and search facts, on the
/// reading thread (inside its `catch_unwind` and time budget). With `bytes` the header comes with
/// the bytes it read and the fields are taken from them (read from the file again only when they
/// are not enough); without, the header comes from `header` and the fields from the file. `size`
/// is the file's size as listed. A RAW's fields are left to [`read_folder`]; here it brings only
/// the camera of its header's EXIF (facts alone, which [`read_folder`] completes), so a container
/// `dalica-meta` does not read (CR3, RAF …) still has its camera.
pub(crate) fn read_with_header(
    path: &Path,
    format: Format,
    size: u64,
    header: &crate::walk::HeaderReader,
    bytes: Option<&crate::walk::HeaderBytesReader>,
    limits: &dalica_image::Limits,
) -> (dalica_image::Result<dalica_image::Header>, Option<FileValues>) {
    let meta_limits = dalica_meta::Limits::default();
    if let Format::Raw(_) = format {
        let h = header(path, limits);
        let facts = h.as_ref().ok().map(crate::facts::of_header).filter(|f| !f.is_empty());
        return (h, facts.map(|f| FileValues(Values::default(), f, Vec::new())));
    }
    if !matches!(format, Format::Jpeg | Format::Png | Format::WebP | Format::Tiff) {
        return (header(path, limits), None);
    }
    match bytes {
        Some(read) => match read(path, limits) {
            Ok((h, bytes)) => (Ok(h), from_bytes(path, format, size, &bytes, &meta_limits)),
            Err(e) => (Err(e), None),
        },
        None => {
            let h = header(path, limits);
            let file = if h.is_ok() { read_file(path, format, None, &meta_limits) } else { None };
            (h, file)
        }
    }
}

/// The owned fields from `bytes`, the start of the file the header read: a JPEG's whenever its
/// metadata segments are all in them, any other format's when they are the whole file (`size`);
/// otherwise read from the file.
fn from_bytes(path: &Path, format: Format, size: u64, bytes: &[u8], limits: &dalica_meta::Limits) -> Option<FileValues> {
    let whole = u64::try_from(bytes.len()).is_ok_and(|n| n >= size);
    match format {
        Format::Jpeg => match dalica_meta::container::scan(bytes, limits) {
            Ok(scan) => Some(FileValues::read(format, &dalica_meta::read_scan(&scan, limits))),
            Err(dalica_meta::Error::Truncated) if !whole => read_file(path, format, None, limits),
            Err(_) => None,
        },
        _ if whole => dalica_meta::read(bytes, limits).ok().map(|m| FileValues::read(format, &m)),
        _ => read_file(path, format, None, limits),
    }
}

/// One photo's owned fields; `None` when they cannot be read (or its format holds none Dalica
/// reads).
fn read_file(path: &Path, format: Format, sidecar: Option<&Path>, limits: &dalica_meta::Limits) -> Option<FileValues> {
    match format {
        Format::Jpeg => {
            let bytes = prefix(path, JPEG_FIRST, MAX_FILE)?;
            dalica_meta::read(&bytes, limits).ok().map(|m| FileValues::read(format, &m))
        }
        Format::Png | Format::WebP | Format::Tiff => {
            let bytes = dalica_fs::read(path, MAX_FILE).ok()??.bytes;
            dalica_meta::read(&bytes, limits).ok().map(|m| FileValues::read(format, &m))
        }
        Format::Raw(kind) => {
            let max_xmp = u64::try_from(limits.max_xmp).unwrap_or(u64::MAX);
            let sidecar = match sidecar {
                // One that cannot be read now is read on the next scan that finds the RAW changed.
                Some(p) => Some(dalica_fs::read(p, max_xmp).ok()??.bytes),
                None => None,
            };
            // Only TIFF-structured RAWs (DNG …) hold metadata `dalica-meta` reads.
            let raw = match kind {
                RawKind::TiffBased => Some(dalica_fs::read(path, MAX_FILE).ok()??.bytes),
                _ => None,
            };
            let read = xmp_sidecar::read(raw.as_deref(), sidecar.as_deref(), limits);
            Some(FileValues::read(format, &read.metadata))
        }
        _ => None,
    }
}

/// The start of a JPEG, `first` bytes and then twice as many each time until its metadata
/// segments are complete (the container walk stops at the image data), at most `max`.
fn prefix(path: &Path, first: u64, max: u64) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len().min(max);
    let mut file = file.take(max);
    let mut bytes = Vec::new();
    // Enough to tell the container from its magic number.
    let mut want = first.max(64);
    loop {
        let have = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let target = want.min(size);
        if target > have {
            (&mut file).take(target.saturating_sub(have)).read_to_end(&mut bytes).ok()?;
        }
        let complete = u64::try_from(bytes.len()).unwrap_or(u64::MAX) >= size;
        match dalica_meta::container::scan(&bytes, &dalica_meta::Limits::default()) {
            Err(dalica_meta::Error::Truncated) if !complete => want = want.saturating_mul(2),
            _ => return Some(bytes),
        }
    }
}

/// Dalica's values for photo `p` as the diff leaves it: its caption, tags and star, its effective
/// date with `header_date` as the file's (the date the scan read), and the model's point (none
/// for a location removed, or no word).
fn ours(library: &Library, p: &Photo, header_date: Option<DateTime>) -> Values {
    let info = library.info(p.id);
    let place = library.geotag(p.id).map(|g| (g.lat(), g.lon()));
    Values { caption: info.caption.clone(), tags: info.tags.clone(), star: p.star, place, date: info.date.or(header_date) }
}

/// The point a file's place `(lat, lon)` is in the model, at the six decimals a geotag is stored
/// with (`None` off the globe, which a place read from a file never is).
pub fn point_of(place: (f64, f64)) -> Option<LatLon> {
    LatLon::new(place.0, place.1).ok().map(|p| p.round6())
}

/// What the model's word on photo `id`'s place becomes now that its file says `file` (the sync
/// rules of the module doc), or `None` to leave it: no word takes the file's GPS; a point follows
/// the file when only the file changed since the last sync (`synced`, the hashes before this read);
/// a location removed stays removed.
fn place_follows(library: &Library, p: &Photo, file: &Values, synced: Option<&Synced>) -> Option<Option<Geo>> {
    match library.geo(p.id) {
        None => file.place.and_then(point_of).map(|g| Some(Geo::At(g))),
        Some(Geo::Removed) => None,
        Some(Geo::At(_)) => {
            let ours = ours(library, p, p.date);
            if sync::decide_field(Field::Place, file, &ours, synced, false) != Decision::Import {
                return None;
            }
            Some(file.place.and_then(point_of).map(Geo::At))
        }
    }
}

/// Dalica's values for photo `id` now (caption, tags, star, effective date, the model's point), or
/// `None` for an unknown photo. What the engine compares the hashes of a sync op with.
pub fn ours_of(library: &Library, id: PhotoId) -> Option<Values> {
    let p = library.photo(id)?;
    Some(ours(library, p, p.date))
}

/// The user's date that makes a photo whose file says `header` show `date`: none when they agree.
fn user_date(date: Option<DateTime>, header: Option<DateTime>) -> Option<DateTime> {
    if date == header { None } else { date }
}

/// A photo Dalica has nothing for takes every value its file has (rule 1, the first scan): its
/// star and its info, from `info` (what its sidecar said; nothing, or this is not a first scan).
/// Returns `None` when Dalica has something for it.
pub(crate) fn first_fill(star: bool, info: &PhotoInfo, file: &FileValues, header_date: Option<DateTime>) -> Option<(bool, PhotoInfo)> {
    if star || !info.is_empty() {
        return None;
    }
    let v = &file.0;
    Some((v.star, PhotoInfo { caption: v.caption.clone(), tags: v.tags.clone(), date: user_date(v.date, header_date), ..PhotoInfo::default() }))
}

/// What the diff makes of the photos it read: the sync op's parts.
#[derive(Default)]
pub(crate) struct Sync {
    captions: Vec<(PhotoId, String)>,
    tags: Vec<(PhotoId, Vec<String>)>,
    stars: Vec<(PhotoId, bool)>,
    dates: Vec<(PhotoId, Option<DateTime>)>,
    fills: Vec<(PhotoId, PhotoInfo)>,
    fill_stars: Vec<(PhotoId, bool)>,
    synced: Vec<(PhotoId, Option<Synced>)>,
    facts: Vec<(PhotoId, Facts)>,
    /// Places following the files ([`place_follows`]), with no undo step.
    geo: Vec<(PhotoId, Option<Geo>)>,
}

impl Sync {
    /// A photo new to the model, read: its fill (if any) rides with its `AddPhotos`; here only its
    /// synced hashes and its search facts.
    pub(crate) fn new_photo(&mut self, id: PhotoId, file: &FileValues) {
        self.synced.push((id, Some(file.0.hashes())));
        if !file.1.is_empty() {
            self.facts.push((id, file.1.clone()));
        }
    }

    /// A photo the model knows (`photo`, under its id), read again: its file now says `file` and
    /// its header date is `header_date`.
    pub(crate) fn known_photo(&mut self, library: &Library, photo: &Photo, file: &FileValues, header_date: Option<DateTime>) {
        let id = photo.id;
        let synced = library.synced(id);
        let v = &file.0;
        if let Some(geo) = place_follows(library, photo, v, synced) {
            self.geo.push((id, geo));
        }
        if synced.is_none()
            && let Some((star, info)) = first_fill(photo.star, library.info(id), file, header_date)
        {
            if !info.is_empty() {
                self.fills.push((id, info));
            }
            if star {
                self.fill_stars.push((id, true));
            }
        } else {
            let ours = ours(library, photo, header_date);
            for (field, decision) in sync::reconcile(v, &ours, synced, false) {
                if decision != Decision::Import {
                    continue;
                }
                match field {
                    Field::Caption => self.captions.push((id, v.caption.clone())),
                    Field::Tags => self.tags.push((id, v.tags.clone())),
                    Field::Rating => self.stars.push((id, v.star)),
                    Field::Date => self.dates.push((id, user_date(v.date, header_date))),
                    // With no undo step: above.
                    Field::Place => {}
                }
            }
        }
        let hashes = v.hashes();
        if synced != Some(&hashes) {
            self.synced.push((id, Some(hashes)));
        }
        if library.facts(id) != &file.1 {
            self.facts.push((id, file.1.clone()));
        }
    }

    /// The sync op, or `None` when there is nothing to change.
    pub(crate) fn op(self) -> Option<Op> {
        let Sync { captions, tags, stars, dates, fills, fill_stars, synced, facts, geo } = self;
        let mut imports = Vec::new();
        if !captions.is_empty() {
            imports.push(Op::SetCaptions { captions });
        }
        if !tags.is_empty() {
            imports.push(Op::SetTags { tags });
        }
        if !stars.is_empty() {
            imports.push(Op::SetStars { stars });
        }
        if !dates.is_empty() {
            imports.push(Op::SetDates { dates });
        }
        let mut first = Vec::new();
        if !fills.is_empty() {
            first.push(Op::SetPhotoInfo { photos: fills });
        }
        if !fill_stars.is_empty() {
            first.push(Op::SetStars { stars: fill_stars });
        }
        if !geo.is_empty() {
            first.push(Op::SetGeo { geo });
        }
        if imports.is_empty() && first.is_empty() && synced.is_empty() && facts.is_empty() {
            return None;
        }
        Some(Op::Batch { ops: vec![Op::Batch { ops: imports }, Op::Batch { ops: first }, Op::SetSynced { synced }, Op::SetFacts { facts }] })
    }
}

/// A sync op's parts, as a caller applying it needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncParts<'a> {
    /// The values changed in another application, imported: one undoable step when not empty.
    pub imports: &'a [Op],
    /// The photos read and the hashes of what their files say (the file's side of each decision).
    pub synced: &'a [(PhotoId, Option<Synced>)],
}

/// `op`'s parts if it is a diff's sync op (`Batch[Batch(imports), Batch(fills), SetSynced,
/// SetFacts]`; no other op the diff makes has this shape).
pub fn sync_parts(op: &Op) -> Option<SyncParts<'_>> {
    match op {
        Op::Batch { ops } => match ops.as_slice() {
            [Op::Batch { ops: imports }, Op::Batch { .. }, Op::SetSynced { synced }, Op::SetFacts { .. }] => Some(SyncParts { imports, synced }),
            _ => None,
        },
        _ => None,
    }
}

/// The undo step of an applied sync op: given the inverse the library gave back for it, the op
/// that undoes its imports alone, or `None` when it imported nothing.
pub fn undo_of_import(inverse: &Op) -> Option<Op> {
    match inverse {
        // The inverse of a batch lists its parts' inverses in reverse order.
        Op::Batch { ops } => match ops.as_slice() {
            [Op::SetFacts { .. }, Op::SetSynced { .. }, Op::Batch { .. }, Op::Batch { ops: undo }] if !undo.is_empty() => {
                Some(Op::Batch { ops: undo.clone() })
            }
            _ => None,
        },
        _ => None,
    }
}

/// The fields of photo `id` a sync op's hashes (`file`, the file's side) report as changed both in
/// another application and in Dalica since the last sync (`Decision::Conflict`), judged on the
/// model before the op is applied. The place only when the model has a word on it (without one
/// the file's GPS is the photo's).
pub fn conflicts(library: &Library, id: PhotoId, file: &Synced) -> Vec<Field> {
    let Some(ours) = ours_of(library, id) else { return Vec::new() };
    let last = library.synced(id);
    let word = library.geo(id).is_some();
    Field::ALL
        .into_iter()
        .filter(|f| *f != Field::Place || word)
        .filter(|f| file.get(*f).is_some_and(|hash| sync::decide(hash, ours.hash(*f), last.and_then(|s| s.get(*f)), false) == Decision::Conflict))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jpeg_prefix_grows_until_its_metadata_is_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        let scene = dalica_scenes::Scene::new(1, 64, 48);
        let plain = dalica_scenes::jpeg(&scene, 85, None).unwrap();
        let changes = dalica_meta::Changes { caption: Some(Some("x".repeat(30_000))), ..dalica_meta::Changes::default() };
        let bytes = dalica_meta::write_owned(&plain, &changes, &dalica_meta::WriteOptions::default()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // From one byte (64, the least): doubled until the walk reaches the image data.
        let got = prefix(&path, 1, MAX_FILE).unwrap();
        assert!(got.len() >= 30_000 && got.len() <= bytes.len());
        assert_eq!(got.as_slice(), bytes.get(..got.len()).unwrap());
        let m = dalica_meta::read(&got, &dalica_meta::Limits::default()).unwrap();
        assert_eq!(m.owned.caption.as_deref().map(str::len), Some(30_000));
        // Capped: a prefix never passes `max`.
        assert!(prefix(&path, 1, 100).unwrap().len() <= 100);
        assert_eq!(prefix(&dir.path().join("missing.jpg"), 1, MAX_FILE), None);
    }

    #[test]
    fn known_files_match_by_folder_name_size_and_time() {
        use dalica_core::FolderId;
        use dalica_library::Folder;
        let mut lib = Library::new();
        let folder = Folder { id: FolderId::new(1), path: PathBuf::from("/p/a") };
        lib.apply(Op::AddFolder { folder }).unwrap();
        let t = Timestamp::from_millis(5);
        let photo = Photo {
            id: PhotoId::new(1),
            folder: FolderId::new(1),
            name: "x.jpg".into(),
            size: 10,
            mtime: t,
            width: 1,
            height: 1,
            date: None,
            star: false,
        };
        lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
        let k = Known::of(&lib);
        assert_eq!(k.len(), 1);
        assert!(k.knows(Path::new("/p/a"), "x.jpg", 10, t));
        assert!(!k.knows(Path::new("/p/a"), "x.jpg", 11, t));
        assert!(!k.knows(Path::new("/p/a"), "x.jpg", 10, Timestamp::from_millis(6)));
        assert!(!k.knows(Path::new("/p/b"), "x.jpg", 10, t));
        assert!(Known::default().is_empty());
    }

    #[test]
    fn sync_ops_are_told_apart_and_their_undo_is_the_import_alone() {
        let id = PhotoId::new(1);
        let mut s = Sync::default();
        s.captions.push((id, "new".into()));
        s.synced.push((id, Some(Synced::default())));
        let op = s.op().unwrap();
        let parts = sync_parts(&op).unwrap();
        assert_eq!(parts.imports, &[Op::SetCaptions { captions: vec![(id, "new".into())] }][..]);
        let inverse = Op::Batch {
            ops: vec![
                Op::SetFacts { facts: vec![] },
                Op::SetSynced { synced: vec![(id, None)] },
                Op::Batch { ops: vec![] },
                Op::Batch { ops: vec![Op::SetCaptions { captions: vec![(id, "old".into())] }] },
            ],
        };
        assert_eq!(undo_of_import(&inverse), Some(Op::Batch { ops: vec![Op::SetCaptions { captions: vec![(id, "old".into())] }] }));
        let nothing = Op::Batch {
            ops: vec![Op::SetFacts { facts: vec![] }, Op::SetSynced { synced: vec![] }, Op::Batch { ops: vec![] }, Op::Batch { ops: vec![] }],
        };
        assert_eq!(undo_of_import(&nothing), None);
        // Facts alone make a sync op too.
        let mut s = Sync::default();
        s.facts.push((id, Facts { model: "X100V".into(), ..Facts::default() }));
        assert!(sync_parts(&s.op().unwrap()).is_some_and(|p| p.imports.is_empty() && p.synced.is_empty()));
        // Other shapes are not sync ops.
        assert_eq!(sync_parts(&Op::SetSynced { synced: vec![] }), None);
        assert_eq!(sync_parts(&Op::Batch { ops: vec![Op::AddPhotos { photos: vec![] }, Op::SetPhotoInfo { photos: vec![] }] }), None);
        assert_eq!(undo_of_import(&Op::SetStars { stars: vec![] }), None);
        assert!(Sync::default().op().is_none());
    }

    #[test]
    fn a_first_fill_takes_every_value_and_only_for_a_photo_dalica_has_nothing_for() {
        let date: DateTime = "2026-06-04T19:42:10".parse().unwrap();
        let other: DateTime = "2020-01-01T00:00:00".parse().unwrap();
        let values = Values { caption: "Tram".into(), tags: vec!["a".into()], star: true, place: Some((1.0, 2.0)), date: Some(date) };
        let file = FileValues(values, Facts::default(), Vec::new());
        let (star, info) = first_fill(false, &PhotoInfo::default(), &file, Some(date)).unwrap();
        assert!(star);
        assert_eq!(
            (info.caption.as_str(), info.tags.as_slice(), info.date),
            ("Tram", &["a".to_owned()][..], None),
            "the header's date is not a user date"
        );
        assert_eq!(first_fill(false, &PhotoInfo::default(), &file, Some(other)).unwrap().1.date, Some(date));
        assert_eq!(first_fill(true, &PhotoInfo::default(), &file, None), None);
        let hidden = PhotoInfo { hidden: true, ..PhotoInfo::default() };
        assert_eq!(first_fill(false, &hidden, &file, None), None);
    }
}
