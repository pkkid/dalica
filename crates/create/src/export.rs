//! Export (`creating.md` §2): a tray, a folder or an album rendered into a new folder.
//!
//! 1. **Plan** ([`plan()`]): the options checked, the export folder made under the location with a
//!    free name (`Lisbon`, else `Lisbon-001`, …), each photo's new stem (its own, sanitized,
//!    `001-` numbered in tray order when asked), each video planned as "Export videos as" asks
//!    ([`VideoAs`]).
//! 2. **Export each item** ([`export_one`]), on whichever thread the caller likes: a photo's source read
//!    under [`MAX_FILE`], rendered through the one output pipeline ([`crate::output::render`]:
//!    the recipe, the size, the quality, the watermark hook, metadata with GPS removed when
//!    asked), then written as `<stem>.jpg` (`.png` for a source with transparency) through
//!    `fs::create::new_file`, which never replaces a file: two photos with one stem get `beach.jpg`
//!    and `beach-001.jpg`. A source that cannot be read as a picture is skipped with its reason.
//!    - **A video as its first frame**: its poster frame (`FrameSource::poster`, the frame
//!      M15.20's thumbnails show), decoded no larger than the export's long side, losslessly into
//!      PNG in memory, then through the same pipeline as a photo (its recipe's rotations, the
//!      size, the quality, the watermark, the owned fields), so it is written as a JPEG. A clip
//!      whose frames cannot be decoded (its codec, a damaged stream) is skipped with the reason.
//!    - **A video as a full video**: the file copied as it is, never re-encoded and never
//!      resized, under the planned stem with its own extension (`fs::transfer::copy_file`: a
//!      temporary file, `fsync`, a byte-for-byte check, then a name that is never taken, `-001`
//!      on a clash). It streams, so a clip of any size is copied without being held in memory.
//! 3. **Cancel** is asked before the source is read, before it is rendered and before the new
//!    file is written. Nothing is written until the whole file is in memory, and the file is
//!    written to a temporary name and given its own in one step, so a cancel (or a crash) never
//!    leaves part of a file under an export name: only complete files remain.
//!
//! [`run`] does the whole export on the calling thread (the CLI and tests); the engine runs the
//! items on its file-job runner instead.

mod options;
mod plan;

use std::path::PathBuf;

pub use self::options::{DEFAULT_SIDE, ExportOptions, MAX_SIDE, MIN_SIDE, QualityPreset, SIDE_STOPS, Size};
pub use self::plan::{
    Destination, Item, Kind, Make, NO_FIRST_FRAME, NUMBER_DIGITS, PHOTO_FALLBACK, Plan, Planned, Skipped, VideoAs, copy_extension, names, names_as,
    plan, plan_as,
};

use dalica_core::names::candidates;
use dalica_image::encode::{self, Image, Metadata};
use dalica_image::formats::video::frames::{FrameOptions, FrameSource};

use crate::output::{self, Report};

/// The largest source file an export reads.
pub const MAX_FILE: u64 = 1 << 30;

/// Why an export could not start.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The options are out of range.
    #[error("{0}")]
    Options(String),
    /// The location or the export folder could not be made.
    #[error("cannot make the export folder: {0}")]
    Folder(String),
}

/// A file an export wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// The item's place in the tray.
    pub index: usize,
    pub source: PathBuf,
    /// The new file.
    pub path: PathBuf,
    /// Its name (the planned one, or `-001` … when that was taken).
    pub name: String,
    pub report: Report,
}

/// A video an export copied as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    /// The item's place in the tray.
    pub index: usize,
    pub source: PathBuf,
    /// The new file.
    pub path: PathBuf,
    /// Its name (the planned stem and the clip's own extension, or `-001` … when that was taken).
    pub name: String,
}

/// What became of one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Written(Written),
    /// A video copied as it is.
    Copied(Copied),
    /// The source could not be read as a picture (undecodable, too large); the reason is shown.
    Skipped(String),
    /// Something else went wrong (the file is gone or unreadable, the new file could not be
    /// written); the reason is shown.
    Failed(String),
    /// Cancel came first; nothing was written for this item.
    Cancelled,
}

/// Exports `item` into `destination`'s folder; `cancelled` is asked between the steps (see the
/// module docs). Never panics and never writes part of a file.
pub fn export_one(item: &Planned, destination: &Destination, cancelled: &dyn Fn() -> bool) -> Outcome {
    if cancelled() {
        return Outcome::Cancelled;
    }
    match item.make {
        Make::Picture => {}
        Make::FirstFrame => return first_frame(item, destination, cancelled),
        Make::Copy => return copy(item, destination),
    }
    let bytes = match dalica_fs::read(&item.source, MAX_FILE) {
        Ok(Some(read)) => read.bytes,
        Ok(None) => return Outcome::Failed(format!("{} is gone", item.source.display())),
        Err(e @ dalica_fs::Error::TooLarge { .. }) => return Outcome::Skipped(e.to_string()),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    if cancelled() {
        return Outcome::Cancelled;
    }
    let spec = output::Spec { changes: item.changes.clone(), ..destination.spec.clone() };
    let out = match output::render(&bytes, &item.recipe, &spec) {
        Ok(out) => out,
        Err(e @ output::Error::Decode(_)) => return Outcome::Skipped(e.reason()),
        Err(e) => return Outcome::Failed(e.reason()),
    };
    drop(bytes);
    if cancelled() {
        return Outcome::Cancelled;
    }
    write(item, destination, &out.bytes, out.report)
}

/// A video's poster frame as a picture (see the module docs).
fn first_frame(item: &Planned, destination: &Destination, cancelled: &dyn Fn() -> bool) -> Outcome {
    let options = FrameOptions { max_side: destination.spec.long_side.map(|side| side.max(1)), threads: 0 };
    let frame = match FrameSource::open(&item.source, &destination.spec.limits, options).and_then(|mut src| src.poster()) {
        Ok(frame) => frame,
        Err(e) => return Outcome::Skipped(e.reason()),
    };
    if cancelled() {
        return Outcome::Cancelled;
    }
    let png = match encode::png(&Image::rgba8(&frame.image), &Metadata { exif: None, xmp: None, icc: None }) {
        Ok(png) => png,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    drop(frame);
    let spec = output::Spec { changes: item.changes.clone(), ..destination.spec.clone() };
    let out = match output::render(&png, &item.recipe, &spec) {
        Ok(out) => out,
        Err(e @ output::Error::Decode(_)) => return Outcome::Skipped(e.reason()),
        Err(e) => return Outcome::Failed(e.reason()),
    };
    drop(png);
    if cancelled() {
        return Outcome::Cancelled;
    }
    write(item, destination, &out.bytes, out.report)
}

/// Writes `bytes` as the item's new file under a name never taken.
fn write(item: &Planned, destination: &Destination, bytes: &[u8], report: Report) -> Outcome {
    let name = format!("{}.{}", item.stem, report.format.extension());
    match dalica_fs::create::new_file(&destination.folder, &name, bytes) {
        Ok(created) => Outcome::Written(Written {
            index: item.index,
            source: item.source.clone(),
            path: destination.folder.join(&created.name),
            name: created.name,
            report,
        }),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// A video copied as it is, under the planned stem with its own extension, never replacing a file.
fn copy(item: &Planned, destination: &Destination) -> Outcome {
    let (Some(folder), Some(file)) = (item.source.parent(), item.source.file_name().and_then(|n| n.to_str())) else {
        return Outcome::Failed(format!("{} is not a file", item.source.display()));
    };
    let expect = match dalica_fs::identity(&item.source) {
        Ok(Some(identity)) => identity,
        Ok(None) => return Outcome::Failed(format!("{} is gone", item.source.display())),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let name = match copy_extension(&item.source) {
        Some(ext) => format!("{}.{ext}", item.stem),
        None => item.stem.clone(),
    };
    for candidate in candidates(&name) {
        match dalica_fs::transfer::copy_file(folder, file, expect, &destination.folder, &candidate) {
            Ok(dalica_fs::transfer::Outcome::Done(_)) => {
                return Outcome::Copied(Copied {
                    index: item.index,
                    source: item.source.clone(),
                    path: destination.folder.join(&candidate),
                    name: candidate,
                });
            }
            Ok(dalica_fs::transfer::Outcome::Exists) => {}
            Err(e) => return Outcome::Failed(e.to_string()),
        }
    }
    Outcome::Failed(format!("no free name for {name}"))
}

/// What a whole export did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub written: Vec<Written>,
    /// The videos copied as they are.
    pub copied: Vec<Copied>,
    /// The plan's skipped items, then those whose source was not a picture, by tray place.
    pub skipped: Vec<Skipped>,
    pub failed: Vec<Skipped>,
    /// Cancel stopped the export before every item was done.
    pub cancelled: bool,
}

/// Exports every item of `plan` in order on the calling thread, stopping at the first cancel.
pub fn run(plan: &Plan, cancelled: &dyn Fn() -> bool) -> Summary {
    let mut summary = Summary { skipped: plan.skipped.clone(), ..Summary::default() };
    for item in &plan.items {
        let left = |reason| Skipped { index: item.index, path: item.source.clone(), reason };
        match export_one(item, &plan.destination, cancelled) {
            Outcome::Written(w) => summary.written.push(w),
            Outcome::Copied(c) => summary.copied.push(c),
            Outcome::Skipped(reason) => summary.skipped.push(left(reason)),
            Outcome::Failed(reason) => summary.failed.push(left(reason)),
            Outcome::Cancelled => {
                summary.cancelled = true;
                break;
            }
        }
    }
    summary.skipped.sort_by_key(|s| s.index);
    summary
}
