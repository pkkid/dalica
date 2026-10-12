//! Synthetic libraries for tests, benches and milestone acceptance (`development.md` §6,
//! `architecture.md` §12): `N` photos in `M` folders, all decided by a seed, so the same [`Spec`]
//! always gives the same library — the same ids, paths, dates, sizes, stars and sketches.
//!
//! - **Index only** ([`library`], [`write_index`], [`open`]): the model is built in memory and
//!   written as an index snapshot through `store`, then opened with [`Session::open`]. No photo
//!   files exist: 100 000 photos are written in about 0.2 s and opened in about 0.1 s (release;
//!   about 1.3 s and 0.9 s in a debug build). Every photo has a real sketch from the
//!   thumbnailer's encoder, made from a small `scenes` picture at the photo's aspect: one of
//!   [`PICTURES`] pictures per aspect, each encoded once.
//! - **Scanned** ([`write_photos`], [`open_scanned`]): the same plan written as small `scenes`
//!   JPEGs (at most [`MAX_SCANNED_PHOTOS`], 100 000: the scanned 100 k pass of the
//!   `first_thumbnails` bench) and added as a watched folder, for tests that need files on disk
//!   (thumbnails, the ladder). Ids then follow the scan, and sizes are the files'.
//!   [`write_photos_with`] can also embed an EXIF thumbnail in each, as cameras do.
//!
//! **The plan.** Folders are spread over [`YEARS`] years ending on 2026-06-04, oldest folder
//! first, each later than the one before; folder sizes are uneven (most hold a few dozen photos,
//! some events far more), and every folder holds at least one photo. Inside a folder photos are
//! taken seconds to minutes apart and named like a camera's (`IMG_0001.jpg`, counting through the
//! whole library, so names are unique). Aspects are mixed: 3:2 and 4:3 both ways, 16:9, square and
//! 3:1 panoramas. One photo in 50 has no capture date (it sorts by its file time), one in 30 is
//! starred. Paths are `<dir>/photos/<year>/<date> <place>`, and `<dir>/photos` is the watched root.
//!
//! **Ids** follow the demo library's rule (`control-protocol.md` §5.4): folders are numbered from 1
//! in tree order (newest first) and photos from 1 in light-table order (each folder oldest first),
//! so a test can name the photos it needs without a query.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{Clock, DateTime, FolderId, PhotoId, Timestamp};
use dalica_fs::Expect;
use dalica_library::{Folder, Library, Op, Photo, Sketch};
use dalica_scenes::Scene;
use serde_json::json;

use crate::session::FOLDERS_FILE;
use crate::{CacheDir, CmdError, OpenError, Options, Session, ThumbOptions};

/// The most photos an index-only library may hold.
pub const MAX_PHOTOS: usize = 1_000_000;
/// The most folders a library may hold.
pub const MAX_FOLDERS: usize = 100_000;
/// The most photos [`write_photos`] writes as files.
pub const MAX_SCANNED_PHOTOS: usize = 100_000;
/// The smallest and largest long side of the files [`write_photos`] writes.
pub const FILE_SIDE: (u32, u32) = (16, 2048);
/// The long side the scanned case usually uses: the 256 ladder entry is still a real reduction.
pub const DEFAULT_FILE_SIDE: u32 = 320;
/// The years the folders are spread over.
pub const YEARS: i64 = 10;
/// The seed of the presets.
pub const SEED: u64 = 2026;
/// Distinct pictures per aspect that photos are drawn from.
pub const PICTURES: u64 = 128;

/// The long side of the picture each sketch is made from: enough for the sketch's 32-cell average
/// and an aspect within a few per cent.
const SKETCH_SIDE: u32 = 48;
/// 2026-06-04T09:00:00 UTC: the newest folder starts here.
const END: i64 = 1_780_563_600;
const SPAN: i64 = YEARS * 365 * 86_400 + (YEARS / 4) * 86_400;
/// Folder names after the date (our own list).
const PLACES: [&str; 16] = [
    "Harbour", "Hills", "Garden", "Market", "Coast", "Forest", "River", "Old Town", "Lake", "Bridge", "Station", "Orchard", "Dunes", "Valley",
    "Square", "Pier",
];
/// Displayed sizes, each with its share of photos in 1/100.
const ASPECTS: [((u32, u32), u64); 7] =
    [((6000, 4000), 45), ((4000, 6000), 20), ((4032, 3024), 15), ((3024, 4032), 8), ((3840, 2160), 6), ((3000, 3000), 4), ((9000, 3000), 2)];
/// The marker [`write_photos`] leaves when every file is written.
const DONE: &str = ".dalica-synthetic-complete";

/// What to generate: `photos` photos in `folders` folders, decided by `seed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Spec {
    pub photos: usize,
    pub folders: usize,
    pub seed: u64,
}

impl Spec {
    /// The light-table scale target: 100 000 photos in 1 000 folders.
    pub const PHOTOS_100K: Spec = Spec { photos: 100_000, folders: 1_000, seed: SEED };
    /// The library-tree scale target: 5 000 folders (of 20 photos on average).
    pub const FOLDERS_5K: Spec = Spec { photos: 100_000, folders: 5_000, seed: SEED };

    pub const fn new(photos: usize, folders: usize, seed: u64) -> Spec {
        Spec { photos, folders, seed }
    }

    /// `Err` when the counts are out of range: at most [`MAX_PHOTOS`] photos and [`MAX_FOLDERS`]
    /// folders, at least one folder when there are photos, and no more folders than photos.
    pub fn check(&self) -> Result<(), SyntheticError> {
        let bad = |why: String| Err(SyntheticError::Spec(why));
        if self.photos > MAX_PHOTOS {
            return bad(format!("{} photos is more than the {MAX_PHOTOS} a synthetic library may hold", self.photos));
        }
        if self.folders > MAX_FOLDERS {
            return bad(format!("{} folders is more than the {MAX_FOLDERS} a synthetic library may hold", self.folders));
        }
        if self.folders > self.photos {
            return bad(format!("{} folders cannot each hold one of {} photos", self.folders, self.photos));
        }
        if self.photos > 0 && self.folders == 0 {
            return bad(format!("{} photos need at least one folder", self.photos));
        }
        Ok(())
    }
}

/// Why a synthetic library could not be made.
#[derive(Debug, thiserror::Error)]
pub enum SyntheticError {
    /// The [`Spec`] (or a file size) is out of range.
    #[error("{0}")]
    Spec(String),
    #[error("{} already holds a library or photos; synthetic libraries are made in empty folders", .0.display())]
    Exists(PathBuf),
    #[error("cannot make a picture: {0}")]
    Scenes(String),
    #[error("cannot make a sketch: {0}")]
    Sketch(String),
    #[error("the library refused the plan: {0}")]
    Model(String),
    #[error(transparent)]
    Fs(#[from] dalica_fs::Error),
    #[error(transparent)]
    Store(#[from] dalica_store::Error),
    #[error(transparent)]
    Open(#[from] OpenError),
    #[error("cannot add the photo folder: {0}")]
    Command(#[from] CmdError),
}

/// The watched root of a synthetic library made in `dir`.
pub fn photos_dir(dir: &Path) -> PathBuf {
    dir.join("photos")
}

/// The library directory (index, `folders.json`, `settings.json`) inside `dir`.
pub fn library_dir(dir: &Path) -> PathBuf {
    dir.join("library")
}

/// One photo of the plan.
struct Planned {
    photo: Photo,
    /// The seed of its `scenes` picture.
    scene: u64,
}

/// The whole plan, folders in tree order (newest first), each with its photos oldest first.
struct Plan {
    folders: Vec<(Folder, Vec<Planned>)>,
}

/// The library of `spec`, in memory, every photo sketched. Its folders live under `root`.
/// Pure apart from CPU time.
pub fn library_at(root: &Path, spec: &Spec) -> Result<Library, SyntheticError> {
    let plan = plan(root, spec)?;
    let mut sketches = Vec::with_capacity(spec.photos);
    let mut made: HashMap<(u64, u32, u32), Sketch> = HashMap::new();
    for (_, photos) in &plan.folders {
        for p in photos {
            let key = (p.scene, p.photo.width, p.photo.height);
            let sketch = match made.get(&key) {
                Some(s) => *s,
                None => {
                    let s = sketch_of(p.scene, p.photo.width, p.photo.height)?;
                    made.insert(key, s);
                    s
                }
            };
            sketches.push((p.photo.id, Some(sketch)));
        }
    }
    let mut ops: Vec<Op> = Vec::with_capacity(plan.folders.len() + 2);
    let mut photos = Vec::with_capacity(spec.photos);
    for (folder, planned) in plan.folders {
        ops.push(Op::AddFolder { folder });
        photos.extend(planned.into_iter().map(|p| p.photo));
    }
    ops.push(Op::AddPhotos { photos });
    if !sketches.is_empty() {
        ops.push(Op::SetSketches { sketches });
    }
    let mut library = Library::new();
    library.apply(Op::Batch { ops }).map_err(|e| SyntheticError::Model(e.to_string()))?;
    Ok(library)
}

/// The library of `spec` made in `dir` ([`library_at`] under [`photos_dir`]), in memory.
pub fn library(dir: &Path, spec: &Spec) -> Result<Library, SyntheticError> {
    library_at(&photos_dir(dir), spec)
}

/// Writes the library of `spec` into `dir`'s library directory: the index snapshot, and
/// `folders.json` naming [`photos_dir`] as the watched root (which holds no files: a rescan or
/// rebuild would empty the library). `dir`'s library directory must not hold a library yet.
/// Off the UI thread.
pub fn write_index(dir: &Path, spec: &Spec) -> Result<(), SyntheticError> {
    spec.check()?;
    let lib_dir = library_dir(dir);
    for name in [dalica_store::snapshot::FILE, dalica_store::journal::FILE, FOLDERS_FILE] {
        if dalica_fs::identity(&lib_dir.join(name))?.is_some() {
            return Err(SyntheticError::Exists(lib_dir));
        }
    }
    let lib = library(dir, spec)?;
    dalica_fs::create_dir_all(&lib_dir)?;
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &lib, 0)?;
    let roots = json!({ "version": 1, "folders": [{ "path": photos_dir(dir) }] });
    let bytes = serde_json::to_vec_pretty(&roots).map_err(|e| SyntheticError::Model(e.to_string()))?;
    dalica_fs::replace(&lib_dir.join(FOLDERS_FILE), &bytes, Expect::Absent)?;
    Ok(())
}

/// [`write_index`], then [`Session::open_with`] on it: the index-only synthetic library, ready to
/// use. Thumbnails are off ([`CacheDir::Off`]): there are no files to make them from, so every
/// cell draws its sketch and no photo is marked as failed. Off the UI thread.
pub fn open(dir: &Path, spec: &Spec, clock: Arc<dyn Clock>) -> Result<Session, SyntheticError> {
    write_index(dir, spec)?;
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    Ok(Session::open_with(&library_dir(dir), clock, &options)?)
}

/// Writes the plan of `spec` as JPEG files under [`photos_dir`]: `long_side` pixels on the longer
/// side at each photo's aspect, with its capture date in EXIF (none for undated photos), upright.
/// At most [`MAX_SCANNED_PHOTOS`] photos; the photo folder must be new. Returns the folders in
/// tree order. Off the UI thread.
pub fn write_photos(dir: &Path, spec: &Spec, long_side: u32) -> Result<Vec<PathBuf>, SyntheticError> {
    write_photos_with(dir, spec, long_side, None)
}

/// [`write_photos`], each file with an EXIF thumbnail `thumbnail` pixels on its long side (as a
/// camera writes one; `None`: none). A thumbnail longer than the photo is refused.
pub fn write_photos_with(dir: &Path, spec: &Spec, long_side: u32, thumbnail: Option<u32>) -> Result<Vec<PathBuf>, SyntheticError> {
    spec.check()?;
    if let Some(t) = thumbnail.filter(|t| *t == 0 || *t > long_side) {
        return Err(SyntheticError::Spec(format!("an embedded thumbnail of {t} px does not fit a {long_side} px photo")));
    }
    if spec.photos > MAX_SCANNED_PHOTOS {
        return Err(SyntheticError::Spec(format!("{} photos is more than the {MAX_SCANNED_PHOTOS} written as files", spec.photos)));
    }
    if !(FILE_SIDE.0..=FILE_SIDE.1).contains(&long_side) {
        return Err(SyntheticError::Spec(format!("a long side of {long_side} px is outside {}..={} px", FILE_SIDE.0, FILE_SIDE.1)));
    }
    let root = photos_dir(dir);
    if dalica_fs::identity(&root)?.is_some() {
        return Err(SyntheticError::Exists(root));
    }
    let plan = plan(&root, spec)?;
    let mut folders = Vec::with_capacity(plan.folders.len());
    let mut files = Vec::with_capacity(spec.photos);
    for (folder, photos) in plan.folders {
        dalica_fs::create_dir_all(&folder.path)?;
        files.extend(photos.into_iter().map(|p| (folder.path.join(&p.photo.name), p)));
        folders.push(folder.path);
    }
    // Rendering, encoding and the synced writes dominate: spread them over a few threads. Each
    // file is decided by the plan alone, so the order they are written in does not matter.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    let per_thread = files.len().div_ceil(threads).max(1);
    let results: Vec<Result<(), SyntheticError>> = std::thread::scope(|scope| {
        let workers: Vec<_> = files.chunks(per_thread).map(|chunk| scope.spawn(move || write_files(chunk, long_side, thumbnail))).collect();
        workers.into_iter().map(|w| w.join().unwrap_or_else(|_| Err(SyntheticError::Scenes("a writer thread panicked".into())))).collect()
    });
    results.into_iter().collect::<Result<(), _>>()?;
    dalica_fs::replace(&root.join(DONE), b"", Expect::Absent)?;
    Ok(folders)
}

/// Writes each planned photo as a JPEG `long_side` pixels on its longer side, with an embedded
/// thumbnail `thumbnail` pixels long when asked.
fn write_files(files: &[(PathBuf, Planned)], long_side: u32, thumbnail: Option<u32>) -> Result<(), SyntheticError> {
    for (path, p) in files {
        let (w, h) = fit(p.photo.width, p.photo.height, long_side);
        let exif = dalica_scenes::Exif { date_time_original: p.photo.date, thumbnail, ..Default::default() };
        let bytes = dalica_scenes::jpeg(&Scene::new(p.scene, w, h), 85, Some(&exif)).map_err(|e| SyntheticError::Scenes(e.to_string()))?;
        dalica_fs::replace(path, &bytes, Expect::Absent)?;
    }
    Ok(())
}

/// [`write_photos`], then a new library in `dir` that watches [`photos_dir`], opened and scanned
/// (`folders.add`, then [`Session::wait_idle`]). Off the UI thread.
pub fn open_scanned(dir: &Path, spec: &Spec, long_side: u32, clock: Arc<dyn Clock>) -> Result<Session, SyntheticError> {
    let lib_dir = library_dir(dir);
    if dalica_fs::identity(&lib_dir)?.is_some() {
        return Err(SyntheticError::Exists(lib_dir));
    }
    write_photos(dir, spec, long_side)?;
    let mut session = Session::open(&lib_dir, clock)?;
    session.execute("folders.add", &json!({ "path": photos_dir(dir) }))?;
    session.wait_idle();
    Ok(session)
}

/// The plan of `spec` with folders under `root`.
fn plan(root: &Path, spec: &Spec) -> Result<Plan, SyntheticError> {
    spec.check()?;
    let (n, m) = (spec.photos, spec.folders);
    let overflow = || SyntheticError::Spec("the plan does not fit in the date range".into());
    // Uneven folder sizes: each folder holds one photo plus its share of the rest by weight.
    let weights: Vec<u64> = (0..m)
        .map(|f| {
            let mut r = Rng::new(spec.seed, 1, f as u64);
            let w = 1 + r.below(16);
            if r.below(20) == 0 { w * 8 } else { w }
        })
        .collect();
    let total: u64 = weights.iter().sum::<u64>().max(1);
    let extra = (n - m) as u64;
    let mut sizes = Vec::with_capacity(m);
    let (mut cum, mut given) = (0u64, 0u64);
    for w in &weights {
        cum += w;
        let upto = extra * cum / total;
        sizes.push(1 + (upto - given) as usize);
        given = upto;
    }

    // Chronological pass (oldest folder first): dates, names, the plan's photos.
    let slot = SPAN / (m.max(1) as i64);
    let start = END - SPAN + slot;
    let mut names = HashSet::new();
    let mut chrono: Vec<(PathBuf, Vec<Planned>)> = Vec::with_capacity(m);
    let mut counter = 0u64;
    for (f, size) in sizes.iter().enumerate() {
        let mut r = Rng::new(spec.seed, 2, f as u64);
        // Strictly increasing: each folder starts in the first half of its own slot.
        let mut t = start + f as i64 * slot + r.below((slot / 2).max(1) as u64) as i64;
        let first = DateTime::from_unix_secs(t).ok_or_else(overflow)?;
        let place = PLACES.get(r.below(PLACES.len() as u64) as usize).copied().unwrap_or("Photos");
        let mut name = format!("{} {place}", first.date());
        let mut k = 2;
        while !names.insert(name.clone()) {
            name = format!("{} {place} {k}", first.date());
            k += 1;
        }
        let path = root.join(first.date().year().to_string()).join(&name);
        let mut photos = Vec::with_capacity(*size);
        for i in 0..*size {
            counter += 1;
            let mut r = Rng::new(spec.seed, 3, counter);
            if i > 0 {
                t += 5 + r.below(900) as i64;
            }
            let (width, height) = pick_aspect(r.below(100));
            let dated = r.below(50) != 0;
            let date = if dated { Some(DateTime::from_unix_secs(t).ok_or_else(overflow)?) } else { None };
            // A dated photo was copied off the card up to a month later; an undated one sorts by
            // its file time, which is when it was taken.
            let copied = if dated { t + r.below(30 * 86_400) as i64 } else { t };
            let pixels = u64::from(width) * u64::from(height);
            let photo = Photo {
                id: PhotoId::new(0),
                folder: FolderId::new(0),
                name: format!("IMG_{counter:04}.jpg"),
                size: pixels / 4 + r.below(pixels / 8),
                mtime: Timestamp::from_millis(copied.checked_mul(1000).ok_or_else(overflow)?),
                width,
                height,
                date,
                star: r.below(30) == 0,
            };
            let picture = r.below(PICTURES);
            photos.push(Planned { photo, scene: Rng::new(spec.seed, 4, picture).next() });
        }
        chrono.push((path, photos));
    }

    // Ids in tree order (newest folder first) and light-table order (oldest photo first).
    let mut folders = Vec::with_capacity(m);
    let mut next_photo = 0u64;
    for (n, (path, mut photos)) in (1u64..).zip(chrono.into_iter().rev()) {
        let id = FolderId::new(n);
        for p in &mut photos {
            next_photo += 1;
            p.photo.id = PhotoId::new(next_photo);
            p.photo.folder = id;
        }
        folders.push((Folder { id, path }, photos));
    }
    Ok(Plan { folders })
}

/// The displayed size for a draw in `0..100`.
fn pick_aspect(draw: u64) -> (u32, u32) {
    let mut acc = 0;
    for (size, share) in ASPECTS {
        acc += share;
        if draw < acc {
            return size;
        }
    }
    ASPECTS[0].0
}

/// `width × height` scaled so its longer side is `long_side` (each side at least 1).
fn fit(width: u32, height: u32, long_side: u32) -> (u32, u32) {
    let long = u64::from(width.max(height).max(1));
    let side = |v: u32| ((u64::from(v) * u64::from(long_side) + long / 2) / long).clamp(1, u64::from(long_side)) as u32;
    (side(width), side(height))
}

/// The sketch of scene `seed` at the aspect of `width × height`.
fn sketch_of(seed: u64, width: u32, height: u32) -> Result<Sketch, SyntheticError> {
    let (w, h) = fit(width, height, SKETCH_SIDE);
    let rgb = Scene::new(seed, w, h).render().map_err(|e| SyntheticError::Scenes(e.to_string()))?;
    let data = rgb.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
    let rgba = dalica_thumbs::Rgba8 { width: rgb.width, height: rgb.height, data };
    let bytes = dalica_thumbs::sketch::encode(&rgba).map_err(|e| SyntheticError::Sketch(e.to_string()))?;
    Sketch::new(&bytes).ok_or_else(|| SyntheticError::Sketch(format!("{} bytes do not fit a sketch", bytes.len())))
}

/// SplitMix64, one stream per (seed, purpose, index), so each folder and photo is decided on its
/// own and the plan does not depend on the order it is walked in.
struct Rng(u64);

impl Rng {
    fn new(seed: u64, purpose: u64, index: u64) -> Rng {
        let mut r = Rng(seed ^ purpose.wrapping_mul(0xa076_1d64_78bd_642f));
        let mix = r.next() ^ index.wrapping_mul(0xe703_7ed1_a0b4_28db);
        Rng(mix)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (0 when `n` is 0).
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Thumb;

    #[test]
    fn photos_can_carry_an_embedded_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let spec = Spec::new(3, 1, SEED);
        assert!(matches!(write_photos_with(dir.path(), &spec, 64, Some(65)), Err(SyntheticError::Spec(_))));
        assert!(matches!(write_photos_with(dir.path(), &spec, 64, Some(0)), Err(SyntheticError::Spec(_))));
        let folders = write_photos_with(dir.path(), &spec, 320, Some(160)).unwrap();
        let file = std::fs::read_dir(&folders[0]).unwrap().next().unwrap().unwrap().path();
        let bytes = std::fs::read(file).unwrap();
        let header = dalica_image::read_header(&bytes, &dalica_image::Limits::default()).unwrap();
        let thumb = dalica_image::read_header(header.thumbnail(&bytes).unwrap(), &dalica_image::Limits::default()).unwrap();
        assert_eq!(thumb.width.max(thumb.height), 160);
    }

    /// With no files to make thumbnails from, the index-only library runs with thumbnails off:
    /// every photo answers with its sketch, nothing is queued and nothing is marked as failed.
    #[test]
    fn an_index_only_library_answers_sketches_and_fails_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), &Spec::new(300, 7, SEED), Arc::new(dalica_core::SystemClock)).unwrap();
        assert_eq!(s.thumbnail_cache_dir(), None);
        let ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
        let order = ids.clone();
        s.prefetch_thumbnails(&order, 0..50, 500.0, 256);
        for id in ids {
            assert!(matches!(s.thumbnail(id, 256), Thumb::Sketch(_)), "{id}");
        }
        s.wait_idle();
        let stats = s.thumbnail_stats();
        assert_eq!((stats.failed, stats.jobs, stats.decodes), (0, 0, 0), "{stats:?}");
        assert!(s.library().photos().all(|p| s.thumbnail_failure(p.id).is_none()));
    }
}
