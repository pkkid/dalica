//! The listing worker (`organizing.md` §3 items 1–3): one thread per listing runs `dalica-scan`'s
//! [`list_source`] over the source and decides, file by file, whether each picture is a duplicate.
//!
//! A candidate is a **duplicate** when a library photo, or an earlier candidate of the same listing,
//! has its size and MD5 digest and its bytes compare equal in full (so a digest collision is never a
//! duplicate). The library's side is the [`Plan`] the owner made from the library alone when the
//! source was opened: its photos by recorded size, each with its digest when the index holds one
//! that still fits the file. A photo of a candidate's size without one is read here (through `fs`,
//! at most [`MAX_FILE`] bytes) and its digest goes back to the owner, which keeps it in the index as
//! M9's Find Duplicates does (derived data, no undo step).
//!
//! Everything the worker finds goes into the listing's [`Shared`] state under a short lock; the
//! owner reads it there when a command asks and never waits on the disk. The thumbnails go into the
//! listing's memory-only cache ([`super::thumbs`]), never into the thumbnail cache on disk.
//!
//! A camera's listing ([`run_camera`]) runs on the same kind of thread over the camera's PTP
//! session instead of a folder walk, into the same shared state.

use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dalica_core::DateTime;
use dalica_core::{PhotoId, Timestamp};
use dalica_image::sniff::VideoKind;
use dalica_image::{Format, RawKind};
use dalica_library::ContentDigest;
use dalica_scan::import::{Candidate, ListEvent, ListOptions, ListProgress, MAX_FILES, THUMBNAIL, TimeSource, list_source};
use dalica_scan::ptp::codes::format::ASSOCIATION;
use dalica_scan::ptp::{self, ObjectKind, PtpError};
use dalica_scan::{FileType, FileTypes, Warning};

use super::thumbs::ThumbCache;
use crate::camera::{self, CameraObject, Link};

/// The largest library file read to make its digest (as Find Duplicates).
pub const MAX_FILE: u64 = crate::duplicates::MAX_FILE;

/// Most warnings a listing keeps (the rest are counted).
pub const MAX_WARNINGS: usize = 256;

/// Most bytes compared between two files (`dalica-scan`'s most bytes read of one file).
const MAX_COMPARE: u64 = dalica_scan::import::MAX_READ;

/// Bytes compared at a time.
const CHUNK: usize = 64 * 1024;

/// A library photo a candidate may duplicate.
#[derive(Clone, Debug)]
pub(crate) struct LibraryFile {
    pub id: PhotoId,
    pub path: PathBuf,
    /// Its digest: from the index when it still fits the file, else made here when first needed.
    pub digest: Option<String>,
    /// A digest was tried here (made or not): never read twice.
    pub tried: bool,
}

/// The library's photos by recorded size.
pub(crate) type Plan = HashMap<u64, Vec<LibraryFile>>;

/// What a duplicate repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateOf {
    /// A photo of the library.
    Photo(PhotoId),
    /// An earlier candidate of the listing (its id).
    Candidate(u64),
}

/// One picture of the source as the listing keeps it (its thumbnail is in the cache).
#[derive(Clone, Debug)]
pub(crate) struct Found {
    pub candidate: Candidate,
    pub duplicate: Option<DuplicateOf>,
    /// The object on the camera, for a camera's listing.
    pub camera: Option<CameraObject>,
}

/// Where a listing is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListState {
    /// Walking the source or reading its files.
    #[default]
    Loading,
    /// Every file was looked at.
    Done,
    /// Stopped before the end (the listing was dropped).
    Cancelled,
    /// The worker stopped unexpectedly.
    Failed,
}

impl ListState {
    pub fn id(self) -> &'static str {
        match self {
            ListState::Loading => "loading",
            ListState::Done => "done",
            ListState::Cancelled => "cancelled",
            ListState::Failed => "failed",
        }
    }
}

/// What the worker has found so far, read by the owner under a short lock.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub state: ListState,
    /// The walk ended ([`ListEvent::Walked`]).
    pub walked: bool,
    pub progress: ListProgress,
    /// Batches of pictures received.
    pub batches: u64,
    /// The pictures, in listing order (a picture's id is its place here).
    pub found: Vec<Found>,
    /// The source holds more than the listing looked at.
    pub truncated: bool,
    /// The first [`MAX_WARNINGS`] warnings, and how many there were.
    pub warnings: Vec<Warning>,
    pub warning_count: u64,
    /// Library digests made here that the owner has not kept yet.
    pub digests: Vec<(PhotoId, ContentDigest)>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The listing's thread body: lists `root` into `shared` and `thumbs` until done or `options.cancel`
/// is set.
pub(crate) fn run(root: PathBuf, options: ListOptions, plan: Plan, shared: Arc<Mutex<Shared>>, thumbs: Arc<Mutex<ThumbCache>>) {
    let cancel = options.cancel.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut dups = Duplicates { plan, seen: HashMap::new(), cancel: cancel.clone() };
        list_source(&root, &options, |event| {
            handle(event, &mut dups, &shared, &thumbs);
            true
        })
    }));
    let mut s = lock(&shared);
    match outcome {
        Ok(listing) => {
            s.state = if listing.cancelled { ListState::Cancelled } else { ListState::Done };
            s.truncated = listing.truncated;
            s.progress = listing.progress;
        }
        Err(_) => {
            log::error!("the import listing of {} stopped unexpectedly", root.display());
            s.state = ListState::Failed;
        }
    }
}

fn handle(event: ListEvent, dups: &mut Duplicates, shared: &Mutex<Shared>, thumbs: &Mutex<ThumbCache>) {
    match event {
        ListEvent::Walked(p) => {
            let mut s = lock(shared);
            s.walked = true;
            s.progress = p;
        }
        ListEvent::Batch(batch) => {
            let mut found = Vec::with_capacity(batch.len());
            let mut digests = Vec::new();
            for mut c in batch {
                let id = c.index as u64;
                if let Some(t) = c.thumbnail.take() {
                    lock(thumbs).insert(id, Arc::new(t));
                }
                let duplicate = dups.check(&c, &mut digests);
                found.push(Found { candidate: c, duplicate, camera: None });
            }
            let mut s = lock(shared);
            s.found.extend(found);
            s.digests.extend(digests);
            s.batches = s.batches.saturating_add(1);
        }
        ListEvent::Progress(p) => lock(shared).progress = p,
        ListEvent::Warning(w) => {
            let mut s = lock(shared);
            s.warning_count = s.warning_count.saturating_add(1);
            if s.warnings.len() < MAX_WARNINGS {
                s.warnings.push(w);
            }
        }
    }
}

/// The duplicate check's memory: the library's plan, and the candidates seen by size and digest.
struct Duplicates {
    plan: Plan,
    /// The candidates that duplicate nothing, by size and digest: what a later one is compared with.
    seen: HashMap<(u64, String), Vec<(u64, PathBuf)>>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Duplicates {
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// What `c` duplicates: a library photo first, else an earlier candidate. Library digests made
    /// on the way are added to `digests`.
    fn check(&mut self, c: &Candidate, digests: &mut Vec<(PhotoId, ContentDigest)>) -> Option<DuplicateOf> {
        if self.cancelled() {
            return None;
        }
        if let Some(files) = self.plan.get_mut(&c.size) {
            for f in files.iter_mut() {
                if f.digest.is_none() && !f.tried {
                    f.tried = true;
                    if let Some(record) = digest_of(&f.path) {
                        f.digest = Some(record.digest.clone());
                        digests.push((f.id, record));
                    }
                }
                if f.digest.as_deref() == Some(c.digest.as_str()) && same_bytes(&c.path, &f.path) {
                    return Some(DuplicateOf::Photo(f.id));
                }
            }
        }
        let firsts = self.seen.entry((c.size, c.digest.clone())).or_default();
        if let Some((id, _)) = firsts.iter().find(|(_, path)| same_bytes(&c.path, path)) {
            return Some(DuplicateOf::Candidate(*id));
        }
        firsts.push((c.index as u64, c.path.clone()));
        None
    }
}

/// The digest record of the library file at `path` as it is now, or `None` when it cannot be read,
/// is not a regular file, is larger than [`MAX_FILE`] or changed while it was read.
fn digest_of(path: &Path) -> Option<ContentDigest> {
    let stat = dalica_fs::stat(path).ok()??;
    if !stat.is_file || stat.identity.size() > MAX_FILE {
        return None;
    }
    let read = dalica_fs::read(path, MAX_FILE).ok()??;
    (read.identity == stat.identity).then(|| ContentDigest {
        size: stat.identity.size(),
        mtime: Timestamp::from_system_time(stat.times.modified),
        digest: crate::duplicates::md5_digest(&read.bytes),
    })
}

/// Whether the files at `a` and `b` hold the same bytes, compared in full a chunk at a time (never
/// more than [`MAX_COMPARE`] bytes; a file that goes on past that compares unequal). A file that
/// cannot be read compares unequal.
pub(crate) fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut fa, mut fb) = (fa.take(MAX_COMPARE.saturating_add(1)), fb.take(MAX_COMPARE.saturating_add(1)));
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut total: u64 = 0;
    loop {
        let (Ok(na), Ok(nb)) = (fill(&mut fa, &mut ba), fill(&mut fb, &mut bb)) else { return false };
        if na != nb || ba.get(..na) != bb.get(..nb) {
            return false;
        }
        if na == 0 {
            return true;
        }
        total = total.saturating_add(na as u64);
        if total > MAX_COMPARE {
            return false;
        }
    }
}

/// Reads into `buf` until it is full or the file ends; the bytes read.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let Some(rest) = buf.get_mut(n..) else { break };
        match r.read(rest) {
            Ok(0) => break,
            Ok(k) => n = n.saturating_add(k),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

// --- A camera's listing ----------------------------------------------------------------------------

/// What a camera's listing reads.
pub(crate) struct CameraSource {
    pub link: Arc<Link>,
    /// The storage listed (`None`: every storage).
    pub storage: Option<u32>,
    pub types: FileTypes,
    /// The source's path: the candidates' paths are below it (they name no file on disk).
    pub root: PathBuf,
    pub cancel: Arc<AtomicBool>,
}

impl CameraSource {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// A camera listing's thread body (`organizing.md` §3, M15.19): every object of the storage
/// described, pictures (and videos, with the type on) listed one by one with the camera's own
/// thumbnail; then the objects whose bytes the duplicate check needs (a library photo, or another
/// object, of the same size), and those without a thumbnail of the camera's, are read (at most
/// [`camera::READ_CAP`] each) to make their digest and thumbnail. A camera lost on the way ends the
/// listing with its reason as a warning. Each listed object is kept in the thumbnail cache, so a
/// thumbnail dropped later is made again from the device ([`super::thumbs::remake_camera`]).
pub(crate) fn run_camera(source: CameraSource, plan: Plan, shared: Arc<Mutex<Shared>>, thumbs: Arc<Mutex<ThumbCache>>) {
    lock(&thumbs).set_camera(Arc::clone(&source.link));
    let outcome = catch_unwind(AssertUnwindSafe(|| list_camera(&source, plan, &shared, &thumbs)));
    if matches!(&outcome, Ok(Err(e)) if camera::lost(e)) {
        lock(&thumbs).camera_gone();
    }
    let mut s = lock(&shared);
    match outcome {
        Ok(Ok(true)) => s.state = ListState::Done,
        Ok(Ok(false)) => s.state = ListState::Cancelled,
        Ok(Err(e)) => {
            push_warning(&mut s, Warning { path: source.root.clone(), reason: camera::lost_reason(&e) });
            s.state = if s.walked { ListState::Failed } else { ListState::Done };
        }
        Err(_) => {
            log::error!("the import listing of a camera stopped unexpectedly");
            s.state = ListState::Failed;
        }
    }
}

fn push_warning(s: &mut Shared, w: Warning) {
    s.warning_count = s.warning_count.saturating_add(1);
    if s.warnings.len() < MAX_WARNINGS {
        s.warnings.push(w);
    }
}

/// The File Types switch of a picture named `name` (by its extension; `None`: always listed).
fn file_type(name: &str) -> Option<FileType> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => FileType::Jpeg,
        "png" => FileType::Png,
        "gif" => FileType::Gif,
        "bmp" => FileType::Bmp,
        "tif" | "tiff" => FileType::Tiff,
        "webp" => FileType::WebP,
        "heic" | "heif" => FileType::Heif,
        "avif" => FileType::Avif,
        "jxl" => FileType::JpegXl,
        "psd" => FileType::Psd,
        _ => FileType::Raw,
    })
}

/// The format a camera object's name says (its bytes are not read to list it).
fn format_of(name: &str, video: bool) -> Format {
    if video {
        return Format::Video(VideoKind::Mp4);
    }
    match file_type(name) {
        Some(FileType::Png) => Format::Png,
        Some(FileType::Gif) => Format::Gif,
        Some(FileType::Bmp) => Format::Bmp,
        Some(FileType::Tiff) => Format::Tiff,
        Some(FileType::WebP) => Format::WebP,
        Some(FileType::Heif) => Format::Heif,
        Some(FileType::Avif) => Format::Avif,
        Some(FileType::JpegXl) => Format::JpegXl,
        Some(FileType::Psd) => Format::Psd,
        Some(FileType::Raw) => Format::Raw(RawKind::TiffBased),
        _ => Format::Jpeg,
    }
}

/// Lists the camera into `shared`; `Ok(false)` when cancelled.
fn list_camera(src: &CameraSource, mut plan: Plan, shared: &Mutex<Shared>, thumbs: &Mutex<ThumbCache>) -> Result<bool, PtpError> {
    let storages = match src.storage {
        Some(id) => vec![id],
        None => src.link.with(|s| s.storage_ids())?,
    };
    let mut handles: Vec<u32> = Vec::new();
    let mut truncated = false;
    for st in storages {
        if st == 0 || st == u32::MAX {
            continue;
        }
        if src.cancelled() {
            return Ok(false);
        }
        match src.link.with(|s| s.object_handles(st)) {
            Ok(h) => handles.extend(h),
            Err(e) if !camera::lost(&e) => {
                push_warning(&mut lock(shared), Warning { path: src.root.clone(), reason: format!("storage {st:#x}: {e}") })
            }
            Err(e) => return Err(e),
        }
        if handles.len() > MAX_FILES {
            handles.truncate(MAX_FILES);
            truncated = true;
            break;
        }
    }
    {
        let mut s = lock(shared);
        s.walked = true;
        s.truncated = truncated;
        s.progress = ListProgress { files: handles.len(), done: 0, listed: 0 };
    }
    let mut folders: HashMap<u32, String> = HashMap::new();
    for (n, handle) in handles.iter().copied().enumerate() {
        if src.cancelled() {
            return Ok(false);
        }
        let listed = list_object(src, handle, &mut folders, shared, thumbs);
        let mut s = lock(shared);
        s.progress.done = n.saturating_add(1);
        s.progress.listed = s.found.len();
        match listed {
            Ok(()) => {}
            Err(e) if !camera::lost(&e) => push_warning(&mut s, Warning { path: src.root.clone(), reason: format!("object {handle:#x}: {e}") }),
            Err(e) => return Err(e),
        }
    }
    if !check_camera(src, &mut plan, shared, thumbs)? {
        return Ok(false);
    }
    Ok(true)
}

/// Describes the object `handle` and, when it is a picture (or a video, with the type on), adds
/// it to the listing with the camera's thumbnail.
fn list_object(
    src: &CameraSource,
    handle: u32,
    folders: &mut HashMap<u32, String>,
    shared: &Mutex<Shared>,
    thumbs: &Mutex<ThumbCache>,
) -> Result<(), PtpError> {
    let info = src.link.with(|s| s.object_info(handle))?;
    if info.format == ASSOCIATION {
        folders.insert(handle, dalica_core::names::sanitize(&info.filename, ""));
        return Ok(());
    }
    let Some(object) = CameraObject::of(handle, &info) else {
        push_warning(&mut lock(shared), Warning { path: src.root.join(&info.filename), reason: "larger than 4 GiB: not imported".into() });
        return Ok(());
    };
    let kind = ptp::Object {
        handle,
        storage: info.storage,
        parent: info.parent,
        format: info.format,
        name: object.name.clone(),
        renamed: object.name != info.filename,
        size: Some(object.size),
        captured: object.captured,
        modified: object.modified,
        width: info.width,
        height: info.height,
        has_thumb: info.thumb_size > 0 && info.thumb_format != 0,
    };
    let video = match kind.kind() {
        ObjectKind::Picture if file_type(&object.name).is_none_or(|t| src.types.allows(t)) => false,
        ObjectKind::Video if src.types.allows(FileType::Video) => true,
        _ => return Ok(()),
    };
    let folder = match info.parent {
        0 | u32::MAX => String::new(),
        p => match folders.get(&p) {
            Some(name) => name.clone(),
            None => {
                let name = match src.link.with(|s| s.object_info(p)) {
                    Ok(i) if i.format == ASSOCIATION => dalica_core::names::sanitize(&i.filename, ""),
                    Ok(_) | Err(PtpError::Device { .. }) => String::new(),
                    Err(e) => return Err(e),
                };
                folders.insert(p, name.clone());
                name
            }
        },
    };
    let thumbnail = if kind.has_thumb {
        match src.link.with(|s| s.thumb(handle)) {
            Ok(bytes) => camera::thumbnail_of(&bytes, THUMBNAIL),
            Err(e) if camera::lost(&e) => return Err(e),
            Err(_) => None,
        }
    } else {
        None
    };
    // An object the camera gives no date is filed under the epoch, as a file without a time.
    let Some(captured) = object.date().or_else(|| DateTime::from_unix_secs(0)) else { return Ok(()) };
    let modified = object.modified.or(object.captured).and_then(camera::system_time).map_or(Timestamp::EPOCH, Timestamp::from_system_time);
    let mut s = lock(shared);
    let index = s.found.len();
    let candidate = Candidate {
        index,
        path: src.root.join(&folder).join(&object.name),
        relative: PathBuf::from(&folder),
        exact_name: !kind.renamed,
        name: object.name.clone(),
        size: object.size,
        modified,
        captured,
        time_source: if object.captured.is_some() { TimeSource::Metadata } else { TimeSource::File },
        format: format_of(&object.name, video),
        width: info.width,
        height: info.height,
        digest: String::new(),
        thumbnail: None,
        problem: None,
    };
    {
        let mut cache = lock(thumbs);
        cache.add_camera_object(index as u64, object.clone(), kind.has_thumb);
        if let Some(t) = thumbnail {
            cache.insert(index as u64, Arc::new(t));
        }
    }
    s.found.push(Found { candidate, duplicate: None, camera: Some(object) });
    s.batches = s.batches.saturating_add(1);
    Ok(())
}

/// The duplicate check and the missing thumbnails of a camera's listing (see [`run_camera`]);
/// `Ok(false)` when cancelled.
fn check_camera(src: &CameraSource, plan: &mut Plan, shared: &Mutex<Shared>, thumbs: &Mutex<ThumbCache>) -> Result<bool, PtpError> {
    let objects: Vec<(u64, CameraObject)> = lock(shared).found.iter().filter_map(|f| Some((f.candidate.index as u64, f.camera.clone()?))).collect();
    let mut sizes: HashMap<u64, usize> = HashMap::new();
    for (_, o) in &objects {
        *sizes.entry(o.size).or_default() += 1;
    }
    let mut seen: HashMap<(u64, String), Vec<(u64, CameraObject)>> = HashMap::new();
    for (id, object) in objects {
        if src.cancelled() {
            return Ok(false);
        }
        let check = plan.contains_key(&object.size) || sizes.get(&object.size).is_some_and(|n| *n > 1);
        let thumb = !lock(thumbs).has(id);
        if !(check || thumb) || object.size > camera::READ_CAP {
            continue;
        }
        let bytes = match camera::read_bytes(&src.link, &object) {
            Ok(b) => b,
            Err(e) if camera::lost(&e) => return Err(e),
            Err(e) => {
                push_warning(&mut lock(shared), Warning { path: src.root.join(&object.name), reason: e.to_string() });
                continue;
            }
        };
        if thumb && let Some(t) = camera::thumbnail_of(&bytes, THUMBNAIL) {
            lock(thumbs).insert(id, Arc::new(t));
        }
        if !check {
            continue;
        }
        let digest = camera::digest_of(&bytes);
        let mut digests = Vec::new();
        let mut duplicate = None;
        if let Some(files) = plan.get_mut(&object.size) {
            for f in files.iter_mut() {
                if f.digest.is_none() && !f.tried {
                    f.tried = true;
                    if let Some(record) = digest_of(&f.path) {
                        f.digest = Some(record.digest.clone());
                        digests.push((f.id, record));
                    }
                }
                if f.digest.as_deref() == Some(digest.as_str()) && holds(&f.path, &bytes) {
                    duplicate = Some(DuplicateOf::Photo(f.id));
                    break;
                }
            }
        }
        if duplicate.is_none() {
            let firsts = seen.get(&(object.size, digest.clone())).cloned().unwrap_or_default();
            for (other, earlier) in firsts {
                match camera::read_bytes(&src.link, &earlier) {
                    Ok(b) if b == bytes => {
                        duplicate = Some(DuplicateOf::Candidate(other));
                        break;
                    }
                    Ok(_) => {}
                    Err(e) if camera::lost(&e) => return Err(e),
                    Err(_) => {}
                }
            }
            if duplicate.is_none() {
                seen.entry((object.size, digest.clone())).or_default().push((id, object.clone()));
            }
        }
        let mut s = lock(shared);
        if let Some(f) = usize::try_from(id).ok().and_then(|i| s.found.get_mut(i)) {
            f.candidate.digest = digest;
            f.duplicate = duplicate;
        }
        s.digests.extend(digests);
    }
    Ok(true)
}

/// Whether the library file at `path` holds exactly `bytes`.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    let limit = u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(1);
    matches!(dalica_fs::read(path, limit), Ok(Some(r)) if r.bytes == bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_compare_equal_only_when_every_byte_matches() {
        let dir = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..(CHUNK * 2 + 7)).map(|i| (i % 251) as u8).collect();
        let mut other = big.clone();
        *other.last_mut().unwrap() ^= 1;
        let (a, b, c, d) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"), dir.path().join("d"));
        for (path, bytes) in [(&a, &big[..]), (&b, &big[..]), (&c, &other[..]), (&d, &big[..CHUNK])] {
            dalica_fs::replace(path, bytes, dalica_fs::Expect::Absent).unwrap();
        }
        assert!(same_bytes(&a, &b));
        assert!(!same_bytes(&a, &c), "the last byte differs");
        assert!(!same_bytes(&a, &d), "one is shorter");
        assert!(!same_bytes(&a, &dir.path().join("missing")));
    }
}
