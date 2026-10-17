//! Listing an import source (`organizing.md` §3 items 1–2, `security.md` §5.9): a card, a drive or
//! a folder, read as an untrusted tree.
//!
//! [`list_source`] walks the source through `dalica-fs` (depth 32, links followed with loop
//! detection, FIFOs and devices never opened; at most [`MAX_FILES`] files), recognises each
//! regular file by its content under the File Types setting and reads each picture once, start to
//! end: its MD5 digest is computed while it is read, its capture time comes from its metadata
//! (`dalica-meta`, then the EXIF of its header, else the file's own time), and a thumbnail of at
//! most [`THUMBNAIL`] pixels is made from the embedded one or a scaled decode. The size a file or a
//! header reports is never trusted for an allocation: bytes are read in chunks and kept only up to
//! [`MAX_KEPT`]. Pictures are handed out in batches with progress; the cancel flag is checked
//! between files. [`groups`] splits the pictures where capture times are more than two hours
//! apart.
//!
//! Everything here reads the disk: run it on a worker, never the UI thread. Nothing here writes.

use std::ffi::OsStr;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::{Entry, Limits};
use dalica_image::{Format, Header, Rgba8};

use crate::skip::{FileTypes, SkipRules};
use crate::walk::Warning;

/// Most files a listing looks at.
pub const MAX_FILES: usize = 100_000;

/// The longer side of a candidate's thumbnail, at most.
pub const THUMBNAIL: u32 = 256;

/// Capture times further apart than this start a new group.
pub const GROUP_GAP: Duration = Duration::from_secs(2 * 60 * 60);

/// Most candidates in one batch.
pub const BATCH: usize = 16;

/// A batch is handed out once it has waited this long, full or not.
pub const BATCH_WAIT: Duration = Duration::from_millis(200);

/// Most bytes of one file kept in memory for its header and thumbnail; the digest goes on past
/// them.
pub const MAX_KEPT: u64 = 256 * 1024 * 1024;

/// Most bytes of one file read: a file that goes on past this (a hostile device) is reported.
pub const MAX_READ: u64 = 16 * 1024 * 1024 * 1024;

/// 9999-12-31T23:59:59 in seconds since 1970: the last file time a date can hold.
const LAST_SECOND: i64 = 253_402_300_799;

/// The warning a cancelled listing ends with.
const CANCELLED: &str = "Cancelled: the listing stopped before the end";

/// Bytes read at a time.
const CHUNK: usize = 64 * 1024;

/// The prefix of a digest, as `dalica-engine`'s duplicates and Save's records write them.
pub const DIGEST_PREFIX: &str = "md5:";

/// How to list a source. Owned and `Send`, so a caller can move it to a worker.
#[derive(Clone, Debug)]
pub struct ListOptions {
    /// Options → File Types.
    pub types: FileTypes,
    pub skip: SkipRules,
    /// Checked between files (and by the walk between folders).
    pub cancel: Option<Arc<AtomicBool>>,
    /// Most files looked at ([`MAX_FILES`]).
    pub max_files: usize,
    /// Deepest folder level walked (`dalica-fs`'s 32).
    pub max_depth: usize,
    /// The thumbnail's longer side ([`THUMBNAIL`]).
    pub thumbnail: u32,
    /// Most candidates in a batch ([`BATCH`]).
    pub batch: usize,
    pub limits: dalica_image::Limits,
    pub meta_limits: dalica_meta::Limits,
}

impl Default for ListOptions {
    fn default() -> ListOptions {
        ListOptions {
            types: FileTypes::all(),
            skip: SkipRules::system(),
            cancel: None,
            max_files: MAX_FILES,
            max_depth: dalica_fs::walk::MAX_DEPTH,
            thumbnail: THUMBNAIL,
            batch: BATCH,
            limits: dalica_image::Limits::default(),
            meta_limits: dalica_meta::Limits::default(),
        }
    }
}

/// Where a candidate's capture time came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeSource {
    /// The file's metadata (EXIF, XMP or IPTC).
    Metadata,
    /// The file's modification time (UTC): the file records no date.
    File,
}

/// A picture an import can copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Its place in the listing (0, 1, 2, … in the order handed out).
    pub index: usize,
    /// The file, exactly as the source names it (the only form ever opened).
    pub path: PathBuf,
    /// Its folder below the source (empty for the source itself), as the source names it.
    pub relative: PathBuf,
    /// The file name to show: a name that is not valid Unicode in its lossy form, control and
    /// bidirectional characters replaced by U+FFFD.
    pub name: String,
    /// The name is valid Unicode and needed no replacement ([`Candidate::name`] is exact).
    pub exact_name: bool,
    /// The bytes read (never the size the source reported).
    pub size: u64,
    pub modified: Timestamp,
    pub captured: DateTime,
    pub time_source: TimeSource,
    /// What its content says, whatever its name says.
    pub format: Format,
    /// As displayed (after orientation); 0 × 0 when the header could not be read.
    pub width: u32,
    pub height: u32,
    /// `md5:` and 32 lowercase hex digits, of every byte read.
    pub digest: String,
    /// At most [`ListOptions::thumbnail`] on its longer side, upright; `None` when none could be
    /// made ([`Candidate::problem`] says why).
    pub thumbnail: Option<Rgba8>,
    /// Why the header or the thumbnail could not be read (the picture is still listed).
    pub problem: Option<String>,
}

/// How far a listing has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListProgress {
    /// Regular files the walk found (at most [`ListOptions::max_files`]).
    pub files: usize,
    /// Files looked at.
    pub done: usize,
    /// Pictures listed.
    pub listed: usize,
}

/// What a listing hands out as it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListEvent {
    /// The walk ended: the files to look at ("Loading 0 of 340 files…").
    Walked(ListProgress),
    /// Pictures, in listing order.
    Batch(Vec<Candidate>),
    /// After every batch, and at the end.
    Progress(ListProgress),
    /// Something the listing left out or could not read; it went on without it.
    Warning(Warning),
}

/// A run of pictures whose capture times are at most [`GROUP_GAP`] apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub start: DateTime,
    pub end: DateTime,
    /// The members' indices into the times given to [`groups`] (for a listing: the candidates'
    /// [`Candidate::index`]), in capture order.
    pub members: Vec<usize>,
}

/// How a listing ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub progress: ListProgress,
    /// The pictures' groups, in capture order.
    pub groups: Vec<Group>,
    /// The cancel flag stopped it, or the receiver went away.
    pub cancelled: bool,
    /// The source holds more than the listing looked at (the file cap or the walk's entry cap).
    pub truncated: bool,
    /// Warnings handed out.
    pub warnings: usize,
}

/// What [`list_all`] gathers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gathered {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<Warning>,
    pub listing: Listing,
}

/// [`list_source`], gathered.
pub fn list_all(root: &Path, options: &ListOptions) -> Gathered {
    let mut out = Gathered::default();
    out.listing = list_source(root, options, |event| {
        match event {
            ListEvent::Batch(b) => out.candidates.extend(b),
            ListEvent::Warning(w) => out.warnings.push(w),
            ListEvent::Walked(_) | ListEvent::Progress(_) => {}
        }
        true
    });
    out
}

/// Lists the source `root`, handing each [`ListEvent`] to `emit` as it happens. `emit` returning
/// `false` (a channel whose receiver is gone) stops the listing like the cancel flag. A source
/// that cannot be walked at all gives one warning and nothing else.
pub fn list_source(root: &Path, options: &ListOptions, mut emit: impl FnMut(ListEvent) -> bool) -> Listing {
    let mut l = Lister { options, emit: &mut emit, listing: Listing::default(), stopped: false, times: Vec::new() };
    l.run(root);
    let mut listing = l.listing;
    listing.groups = groups(&l.times);
    listing.cancelled = l.stopped;
    listing
}

struct Lister<'a> {
    options: &'a ListOptions,
    emit: &'a mut dyn FnMut(ListEvent) -> bool,
    listing: Listing,
    stopped: bool,
    times: Vec<DateTime>,
}

impl Lister<'_> {
    fn cancelled(&self) -> bool {
        self.options.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
    }

    fn send(&mut self, event: ListEvent) {
        if !self.stopped && !(self.emit)(event) {
            self.stopped = true;
        }
    }

    fn warn(&mut self, path: &Path, reason: impl Into<String>) {
        self.listing.warnings = self.listing.warnings.saturating_add(1);
        self.send(ListEvent::Warning(Warning { path: path.to_path_buf(), reason: reason.into() }));
    }

    fn run(&mut self, root: &Path) {
        let Some(files) = self.walk(root) else { return };
        self.listing.progress.files = files.len();
        let progress = self.listing.progress;
        self.send(ListEvent::Walked(progress));
        let mut batch: Vec<Candidate> = Vec::new();
        let mut since = Instant::now();
        for entry in files {
            if self.stopped {
                return;
            }
            if self.cancelled() {
                // The pictures read so far go out; nothing more is read.
                self.flush(&mut batch);
                self.warn(root, CANCELLED);
                self.stopped = true;
                return;
            }
            let candidate = self.look_at(root, &entry);
            self.listing.progress.done = self.listing.progress.done.saturating_add(1);
            if let Some(c) = candidate {
                self.times.push(c.captured);
                self.listing.progress.listed = self.listing.progress.listed.saturating_add(1);
                batch.push(c);
            }
            if !batch.is_empty() && (batch.len() >= self.options.batch.max(1) || since.elapsed() >= BATCH_WAIT) {
                self.flush(&mut batch);
                since = Instant::now();
            }
        }
        if self.stopped {
            return;
        }
        self.flush(&mut batch);
        let progress = self.listing.progress;
        self.send(ListEvent::Progress(progress));
    }

    fn flush(&mut self, batch: &mut Vec<Candidate>) {
        if batch.is_empty() {
            return;
        }
        self.send(ListEvent::Batch(std::mem::take(batch)));
        let progress = self.listing.progress;
        self.send(ListEvent::Progress(progress));
    }

    /// The regular files of the source in walk order, at most `max_files`; what the walk left out
    /// is warned. `None`: the source could not be walked.
    fn walk(&mut self, root: &Path) -> Option<Vec<Entry>> {
        let cancel = self.options.cancel.clone();
        let max_files = self.options.max_files;
        // Folders count against the walk's entries too: room for as many as files.
        let limits = Limits { max_depth: self.options.max_depth, max_entries: max_files.saturating_mul(2).max(1), cancel: cancel.as_deref() };
        let skip = &self.options.skip;
        let walk = match dalica_fs::walk(root, limits, |e: &Entry| skip.skips(e)) {
            Ok(w) => w,
            Err(e) => {
                self.warn(root, e.to_string());
                return None;
            }
        };
        let mut cancelled = false;
        for w in walk.warnings {
            match w {
                dalica_fs::Warning::TooDeep(p) => self.warn(&p, format!("deeper than {} folders: not listed", self.options.max_depth)),
                dalica_fs::Warning::TooManyEntries => {
                    self.listing.truncated = true;
                    self.warn(root, format!("more than {} entries: the listing stopped there", limits.max_entries));
                }
                dalica_fs::Warning::Loop(p) => self.warn(&p, "a link to a folder already listed: not followed"),
                dalica_fs::Warning::Unreadable { path, reason } => self.warn(&path, reason),
                dalica_fs::Warning::Cancelled => cancelled = true,
            }
        }
        if cancelled {
            self.warn(root, CANCELLED);
            self.stopped = true;
        }
        if self.stopped {
            return None;
        }
        let mut files = Vec::new();
        for e in walk.entries {
            if e.is_dir {
                continue;
            }
            if !e.is_file {
                // A FIFO, socket or device: opening one can block forever.
                self.warn(&e.path, "not a regular file: not read");
                continue;
            }
            if files.len() >= max_files {
                self.listing.truncated = true;
                self.warn(root, format!("more than {max_files} files: the rest are not listed"));
                break;
            }
            files.push(e);
        }
        Some(files)
    }

    /// Reads one file: a candidate when its content is a picture of a type that is on.
    fn look_at(&mut self, root: &Path, entry: &Entry) -> Option<Candidate> {
        let read = match read_file(&entry.path, self.options.types) {
            Ok(Some(r)) => r,
            Ok(None) => return None,
            Err(reason) => {
                self.warn(&entry.path, reason);
                return None;
            }
        };
        let (name, exact_name) = display_name(entry.path.file_name().unwrap_or_default());
        let relative = entry.path.parent().and_then(|p| p.strip_prefix(root).ok()).map(Path::to_path_buf).unwrap_or_default();
        let index = self.listing.progress.listed;
        // Clamped to the years 1970–9999, which always make a date.
        let file_time = DateTime::from_unix_secs(entry.modified.as_secs().clamp(0, LAST_SECOND));
        let FileRead { format, bytes, size, digest } = read;
        let options = self.options;
        // The readers guard their own decoders; this guard catches anything else on hostile input.
        let looked = catch_unwind(AssertUnwindSafe(|| look(format, &bytes, options)))
            .unwrap_or_else(|_| Looked { problem: Some("reading the picture panicked".into()), ..Looked::default() });
        let (captured, time_source) = match (looked.captured, file_time) {
            (Some(t), _) => (t, TimeSource::Metadata),
            (None, Some(t)) => (t, TimeSource::File),
            (None, None) => {
                self.warn(&entry.path, "no usable time: not listed");
                return None;
            }
        };
        if let Some(p) = &looked.problem {
            self.warn(&entry.path, p.clone());
        }
        Some(Candidate {
            index,
            path: entry.path.clone(),
            relative,
            name,
            exact_name,
            size,
            modified: entry.modified,
            captured,
            time_source,
            format,
            width: looked.width,
            height: looked.height,
            digest,
            thumbnail: looked.thumbnail,
            problem: looked.problem,
        })
    }
}

/// What reading a picture's bytes gave.
#[derive(Default)]
struct Looked {
    width: u32,
    height: u32,
    captured: Option<DateTime>,
    thumbnail: Option<Rgba8>,
    problem: Option<String>,
}

/// The header, capture time and thumbnail of a picture held in `bytes`.
fn look(format: Format, bytes: &[u8], options: &ListOptions) -> Looked {
    let header = match dalica_image::read_header(bytes, &options.limits) {
        Ok(h) => h,
        Err(dalica_image::Error::Unsupported(_)) => return Looked::default(),
        Err(e) => return Looked { problem: Some(e.to_string()), ..Looked::default() },
    };
    let (width, height) = header.display_size();
    let captured = capture_time(format, bytes, &header, &options.meta_limits);
    let (thumbnail, problem) = match thumbnail(bytes, &header, options.thumbnail, &options.limits) {
        Ok(t) => (Some(t), None),
        Err(dalica_image::Error::Unsupported(_)) => (None, None),
        Err(e) => (None, Some(e.to_string())),
    };
    Looked { width, height, captured, thumbnail, problem }
}

/// When the picture was taken, by its metadata: the owned date `dalica-meta` resolves (EXIF, XMP,
/// IPTC) for the formats it reads, else the EXIF date of the header (the RAW containers).
fn capture_time(format: Format, bytes: &[u8], header: &Header, limits: &dalica_meta::Limits) -> Option<DateTime> {
    let meta = match format {
        Format::Jpeg | Format::Png | Format::WebP | Format::Tiff => dalica_meta::read(bytes, limits).ok(),
        _ => None,
    };
    meta.and_then(|m| m.owned.date_taken.map(|d| d.date_time())).or_else(|| header.exif.as_ref().and_then(|e| e.date_time_original))
}

/// A thumbnail of at most `long_side` pixels, upright: from the embedded one when it decodes, else
/// a scaled decode.
fn thumbnail(bytes: &[u8], header: &Header, long_side: u32, limits: &dalica_image::Limits) -> dalica_image::Result<Rgba8> {
    if let Some(embedded) = header.thumbnail(bytes)
        && let Ok(small) = dalica_image::decode_thumbnail(embedded, long_side, limits)
        && let Ok(upright) = dalica_image::orient::apply(small, header.orientation)
    {
        return Ok(upright);
    }
    dalica_image::decode_thumbnail(bytes, long_side, limits)
}

/// One file read start to end.
struct FileRead {
    format: Format,
    /// The first [`MAX_KEPT`] bytes.
    bytes: Vec<u8>,
    /// Every byte read.
    size: u64,
    digest: String,
}

/// Reads the file at `path`: `None` when its content is no picture, or a type that is off (only
/// its first bytes are read); else every byte through the digest, the first [`MAX_KEPT`] kept.
/// The size the file system reports is never used.
fn read_file(path: &Path, types: FileTypes) -> Result<Option<FileRead>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open the file: {e}"))?;
    let mut file = file.take(MAX_READ.saturating_add(1));
    let mut bytes: Vec<u8> = Vec::new();
    let sniff = u64::try_from(dalica_image::sniff::SNIFF_LEN).unwrap_or(u64::MAX);
    (&mut file).take(sniff).read_to_end(&mut bytes).map_err(|e| format!("cannot read the file: {e}"))?;
    let Some(format) = dalica_image::sniff(&bytes) else { return Ok(None) };
    if !types.allows_format(format) {
        return Ok(None);
    }
    let mut context = md5::Context::new();
    context.consume(&bytes);
    let mut size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("cannot read the file: {e}")),
        };
        let part = chunk.get(..n).unwrap_or_default();
        context.consume(part);
        size = size.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        let room = usize::try_from(MAX_KEPT.saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX))).unwrap_or(usize::MAX);
        bytes.extend_from_slice(part.get(..n.min(room)).unwrap_or_default());
    }
    if size > MAX_READ {
        return Err(format!("more than {MAX_READ} bytes: not read to the end"));
    }
    Ok(Some(FileRead { format, bytes, size, digest: format!("{DIGEST_PREFIX}{:x}", context.finalize()) }))
}

/// A file name to show, and whether it is exact: the lossy form of a name that is not valid
/// Unicode, with control and bidirectional characters replaced by U+FFFD.
fn display_name(name: &OsStr) -> (String, bool) {
    let mut exact = name.to_str().is_some();
    let shown: String = name
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_control() || dalica_core::names::is_bidi_control(c) {
                exact = false;
                char::REPLACEMENT_CHARACTER
            } else {
                c
            }
        })
        .collect();
    (shown, exact)
}

/// The groups of `times`: sorted by time (ties in the order given), split where two neighbours are
/// more than [`GROUP_GAP`] apart.
pub fn groups(times: &[DateTime]) -> Vec<Group> {
    let mut order: Vec<usize> = (0..times.len()).collect();
    order.sort_by_key(|&i| (times.get(i).copied(), i));
    let gap = i64::try_from(GROUP_GAP.as_secs()).unwrap_or(i64::MAX);
    let mut out: Vec<Group> = Vec::new();
    let mut last: Option<i64> = None;
    for i in order {
        let Some(&t) = times.get(i) else { continue };
        let secs = seconds(t);
        match out.last_mut() {
            Some(g) if last.is_some_and(|l| secs.saturating_sub(l) <= gap) => {
                g.end = t;
                g.members.push(i);
            }
            _ => out.push(Group { start: t, end: t, members: vec![i] }),
        }
        last = Some(secs);
    }
    out
}

/// Seconds from 1970-01-01T00:00:00 to `t`, read as UTC (only differences are used).
fn seconds(t: DateTime) -> i64 {
    let d = t.date();
    let (y, m, day) = (i64::from(d.year()), i64::from(d.month()), i64::from(d.day()));
    // Howard Hinnant's `days_from_civil`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_agree_with_unix_times() {
        for secs in [0i64, 86_399, 951_782_400, 1_780_000_000, -86_400 * 365] {
            let t = DateTime::from_unix_secs(secs).unwrap();
            assert_eq!(seconds(t), secs, "{t}");
        }
    }
}
