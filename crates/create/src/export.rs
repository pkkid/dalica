//! Export (`creating.md` §2): a tray, a folder or an album rendered into a new folder.
//!
//! 1. **Plan** ([`plan()`]): the options checked, the export folder made under the location with a
//!    free name (`Lisbon`, else `Lisbon-001`, …), each photo's new stem (its own, sanitized,
//!    `001-` numbered in tray order when asked), videos listed as skipped until M15 decodes them.
//! 2. **Export each item** ([`export_one`]), on whichever thread the caller likes: the source read
//!    under [`MAX_FILE`], rendered through the one output pipeline ([`crate::output::render`]:
//!    the recipe, the size, the quality, the watermark hook, metadata with GPS removed when
//!    asked), then written as `<stem>.jpg` (`.png` for a source with transparency) through
//!    `fs::create::new_file`, which never replaces a file: two photos with one stem get `beach.jpg`
//!    and `beach-001.jpg`. A source that cannot be read as a picture is skipped with its reason.
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
pub use self::plan::{Destination, Item, Kind, NUMBER_DIGITS, PHOTO_FALLBACK, Plan, Planned, Skipped, names, plan};

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

/// What became of one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Written(Written),
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
    let name = format!("{}.{}", item.stem, out.report.format.extension());
    match dalica_fs::create::new_file(&destination.folder, &name, &out.bytes) {
        Ok(created) => Outcome::Written(Written {
            index: item.index,
            source: item.source.clone(),
            path: destination.folder.join(&created.name),
            name: created.name,
            report: out.report,
        }),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// What a whole export did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub written: Vec<Written>,
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
