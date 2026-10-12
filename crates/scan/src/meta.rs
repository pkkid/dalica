//! The metadata a scan imports from the photo files (`xmp-interop.md` §5, §7).
//!
//! - **Reading, on the scan's thread.** When a walk finds a photo the model does not know at its
//!   size and modification time ([`Known`]; [`ReadMetadata`] in the scan's options), it reads the
//!   file's owned fields through `dalica-meta`, in the same pass as the header and the folder
//!   sidecar: a JPEG only up to its image data, a PNG, WebP or TIFF whole (at most [`MAX_FILE`]),
//!   a RAW through its `.xmp` sidecar whatever the write option says (`xmp_sidecar::Sidecars` over
//!   the folder's names), else the XMP embedded in a TIFF-structured RAW such as a DNG. Each read
//!   runs inside `catch_unwind`; a file whose metadata cannot be read brings none
//!   ([`FoundPhoto::file`] stays `None`) and the scan goes on.
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
//! - **The sync op** ([`sync_parts`]): the diff's last op for the photos it read,
//!   `Batch[Batch(imports), Batch(first-scan fills), SetSynced]`. A caller applying it outside a
//!   command makes its first part one undoable step ([`undo_of_import`]); the rest is not undone
//!   (the synced hashes stay those of the file, so undoing an import writes Dalica's old value
//!   back rather than importing again).
//!
//! The model holds no place per photo yet: a place is read and its hash synced, never imported and
//! never reported as a conflict.

use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{DateTime, PhotoId, Timestamp};
use dalica_image::{Format, RawKind};
use dalica_library::sync::{self, Decision, Field, Values};
use dalica_library::{Library, Op, Photo, PhotoInfo, Synced, clean_caption, clean_tags};
use dalica_meta::xmp_sidecar::{self, Sidecars};
use dalica_meta::{DateTaken, OwnedFields};

use crate::walk::FoundPhoto;

/// The largest photo file whose metadata a scan reads whole (PNG, WebP, TIFF, TIFF-structured
/// RAW); a JPEG is read only up to its image data, whatever its size. The same cap as
/// `photo.metadata`'s reader.
pub const MAX_FILE: u64 = 1 << 30;

/// The first part of a JPEG read for its metadata; doubled until the metadata segments are in.
const JPEG_FIRST: u64 = 256 * 1024;

/// What a file says about the fields Dalica owns, cleaned as the library stores them (caption and
/// tags through `clean_caption` / `clean_tags`, a rating of 5 as a star, the date to the second).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileValues(pub Values);

/// The place is a `dalica_meta::Place`: always finite, so equality is an equivalence.
impl Eq for FileValues {}

impl FileValues {
    /// A file's owned fields in the library's terms.
    pub fn of(owned: &OwnedFields) -> FileValues {
        FileValues(Values {
            caption: owned.caption.as_deref().map(clean_caption).unwrap_or_default(),
            tags: clean_tags(owned.tags.iter().map(String::as_str)),
            star: owned.starred(),
            place: owned.place.map(|p| (p.latitude(), p.longitude())),
            date: owned.date_taken.as_ref().map(DateTaken::date_time),
        })
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
        let paths: HashMap<_, &Path> = library.folders().map(|f| (f.id, f.path.as_path())).collect();
        let mut out = Known::default();
        for p in library.photos() {
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

/// Reads the owned fields of the photos in `photos` (one folder, `dir`) that `known` does not
/// know, into [`FoundPhoto::file`]. `names` are every file name of the folder (for RAW sidecars).
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
        if known.knows(dir, &p.name, p.size, p.mtime) {
            continue;
        }
        if cancelled() {
            return false;
        }
        let path = dir.join(&p.name);
        let sidecar = sidecars.as_ref().and_then(|s| s.get(&p.name)).and_then(|n| n.existing.as_deref()).map(|n| dir.join(n));
        let format = p.format;
        p.file = catch_unwind(AssertUnwindSafe(|| read_file(&path, format, sidecar.as_deref(), limits))).ok().flatten();
    }
    true
}

/// One photo's owned fields; `None` when they cannot be read (or its format holds none Dalica
/// reads).
fn read_file(path: &Path, format: Format, sidecar: Option<&Path>, limits: &dalica_meta::Limits) -> Option<FileValues> {
    match format {
        Format::Jpeg => {
            let bytes = prefix(path, JPEG_FIRST, MAX_FILE)?;
            dalica_meta::read(&bytes, limits).ok().map(|m| FileValues::of(&m.owned))
        }
        Format::Png | Format::WebP | Format::Tiff => {
            let bytes = dalica_fs::read(path, MAX_FILE).ok()??.bytes;
            dalica_meta::read(&bytes, limits).ok().map(|m| FileValues::of(&m.owned))
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
            Some(FileValues::of(&read.metadata.owned))
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

/// Dalica's values for photo `p` as the diff leaves it: its caption, tags and star, and its
/// effective date with `header_date` as the file's (the date the scan read).
fn ours(library: &Library, p: &Photo, header_date: Option<DateTime>) -> Values {
    let info = library.info(p.id);
    Values { caption: info.caption.clone(), tags: info.tags.clone(), star: p.star, place: None, date: info.date.or(header_date) }
}

/// Dalica's values for photo `id` now (caption, tags, star, effective date; no place), or `None`
/// for an unknown photo. What the engine compares the hashes of a sync op with.
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
}

impl Sync {
    /// A photo new to the model, read: its fill (if any) rides with its `AddPhotos`; here only its
    /// synced hashes.
    pub(crate) fn new_photo(&mut self, id: PhotoId, file: &FileValues) {
        self.synced.push((id, Some(file.0.hashes())));
    }

    /// A photo the model knows (`photo`, under its id), read again: its file now says `file` and
    /// its header date is `header_date`.
    pub(crate) fn known_photo(&mut self, library: &Library, photo: &Photo, file: &FileValues, header_date: Option<DateTime>) {
        let id = photo.id;
        let synced = library.synced(id);
        let v = &file.0;
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
                    // No place per photo in the model yet.
                    Field::Place => {}
                }
            }
        }
        let hashes = v.hashes();
        if synced != Some(&hashes) {
            self.synced.push((id, Some(hashes)));
        }
    }

    /// The sync op, or `None` when there is nothing to change.
    pub(crate) fn op(self) -> Option<Op> {
        let Sync { captions, tags, stars, dates, fills, fill_stars, synced } = self;
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
        if imports.is_empty() && first.is_empty() && synced.is_empty() {
            return None;
        }
        Some(Op::Batch { ops: vec![Op::Batch { ops: imports }, Op::Batch { ops: first }, Op::SetSynced { synced }] })
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

/// `op`'s parts if it is a diff's sync op (`Batch[Batch(imports), Batch(fills), SetSynced]`; no
/// other op the diff makes has this shape).
pub fn sync_parts(op: &Op) -> Option<SyncParts<'_>> {
    match op {
        Op::Batch { ops } => match ops.as_slice() {
            [Op::Batch { ops: imports }, Op::Batch { .. }, Op::SetSynced { synced }] => Some(SyncParts { imports, synced }),
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
            [Op::SetSynced { .. }, Op::Batch { .. }, Op::Batch { ops: undo }] if !undo.is_empty() => Some(Op::Batch { ops: undo.clone() }),
            _ => None,
        },
        _ => None,
    }
}

/// The fields of photo `id` a sync op's hashes (`file`, the file's side) report as changed both in
/// another application and in Dalica since the last sync (`Decision::Conflict`), judged on the
/// model before the op is applied. The place is never reported (the model has none yet).
pub fn conflicts(library: &Library, id: PhotoId, file: &Synced) -> Vec<Field> {
    let Some(ours) = ours_of(library, id) else { return Vec::new() };
    let last = library.synced(id);
    Field::ALL
        .into_iter()
        .filter(|f| *f != Field::Place)
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
                Op::SetSynced { synced: vec![(id, None)] },
                Op::Batch { ops: vec![] },
                Op::Batch { ops: vec![Op::SetCaptions { captions: vec![(id, "old".into())] }] },
            ],
        };
        assert_eq!(undo_of_import(&inverse), Some(Op::Batch { ops: vec![Op::SetCaptions { captions: vec![(id, "old".into())] }] }));
        let nothing = Op::Batch { ops: vec![Op::SetSynced { synced: vec![] }, Op::Batch { ops: vec![] }, Op::Batch { ops: vec![] }] };
        assert_eq!(undo_of_import(&nothing), None);
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
        let file = FileValues(Values { caption: "Tram".into(), tags: vec!["a".into()], star: true, place: Some((1.0, 2.0)), date: Some(date) });
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
