//! Save in place, Undo Save and Revert (`data-model.md` §6 and §10.9, `security.md` §5.3): the
//! only path that changes a photo's pixels, with its original always recoverable.
//!
//! [`in_place`] takes these steps, in order (`data-model.md` §6 lists them as the journal):
//!
//! 1. **Refuse** a recipe with an operation Dalica cannot render ([`renders`]): an unknown or
//!    legacy operation, or a known one without a renderer yet, would be dropped from the pixels.
//! 2. **Read** the photo and its identity (`fs::stat` and `fs::read`), refusing a read-only file
//!    and a file whose size or modification time is not what the caller last saw
//!    ([`Options::seen`]); read the folder's originals record. A photo saved before (a `done`
//!    entry) renders from its **kept original** (checked against the record's size and digest)
//!    with the recorded recipe followed by the new edits, so quality loss never compounds; a
//!    `pending` entry is an interrupted Save, left for the repair.
//! 3. **Render** through the one output pipeline ([`crate::output::render`], `Purpose::InPlace`)
//!    at Match quality, with a fresh EXIF thumbnail and the owned fields merged under the
//!    write-back options. Nothing on disk has changed yet.
//! 4. **Journal:** the record's entry for the photo is written `pending` (the whole recipe, the
//!    original's size, modification time, pixel size and digest, the time) through `fs`'s atomic
//!    replace, before anything moves.
//! 5. **Keep the original:** on a first Save the photo moves into `.dalica-originals/` by one
//!    rename that never replaces, after its identity is checked once more (`fs::originals`); on a
//!    later Save the kept original is checked and stays.
//! 6. **Commit** the new file at the photo's name: a first Save writes it where nothing is (the
//!    name was just freed), a later one replaces the saved file only while it is still the version
//!    read in step 2.
//! 7. **Verify** the file on disk: the bytes written, the pixel size, orientation 1 and the owned
//!    fields read back.
//! 8. **Mark** the entry `done`.
//!
//! A failure before step 5 changes nothing (the pending entry is taken back); a failure in step 6
//! or 7 of a first Save moves the original back when it can. Anything else leaves the entry
//! `pending` and returns [`Error::Incomplete`]: the repair (M8.18) rolls it forward when the new
//! file is complete and verifies, else back. A simulated crash (`fs`'s fault points) stops at once,
//! as a killed process would.
//!
//! [`undo`] (Undo Save) and [`revert`] (Revert) move the kept original back over the saved file,
//! only while the saved file is the version the caller saw and the backup is still the recorded
//! original, then drop the entry: Undo Save answers the recorded recipe to put back as unsaved
//! edits, Revert answers none.
//!
//! [`save`] is the dispatch Save calls (M8.15): a rotate-only JPEG goes lossless
//! ([`lossless`], M8.14), a format Dalica cannot write in place gets a JPEG beside it ([`beside`],
//! M8.14), everything else [`in_place`]; [`copy`] is Save a Copy and Save As (M8.14). Every
//! function here touches the disk only through `dalica-fs`; call them on a worker, never on the UI
//! thread.

pub mod beside;
pub mod copy;
mod journal;
pub mod legacy;
pub mod lossless;
mod record;
pub mod recover;
mod restore;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use dalica_core::Timestamp;
use dalica_edit::{Op, Recipe};
use dalica_fs::{Identity, originals as backup};
use dalica_image::{Format, Header, Limits};
use dalica_meta::{Changes, WriteOptions};
use dalica_sidecar::originals::{Entry, State};

use crate::output::{self, Purpose, Report, Spec};

pub use record::{DIGEST_PREFIX, digest, digest_matches};
pub use restore::{Restored, revert, undo};

/// The largest photo file Save reads whole (the thumbnailer's and the metadata reader's cap).
pub const MAX_FILE: u64 = 1 << 30;

/// Why a Save, Undo Save or Revert did nothing, or stopped part-way. Every failure is an `Err`;
/// nothing here panics.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The recipe holds an operation Dalica cannot render; saving would drop it from the pixels.
    #[error("the edit {0:?} cannot be rendered by this version of Dalica, so the photo is not saved")]
    Unrenderable(String),
    /// More operations than a recipe may hold (the recorded recipe and the new edits together).
    #[error("the recipe has too many operations to save")]
    TooManyOperations,
    /// The name is not one plain path component (`security.md` §5.4).
    #[error("{0:?} is not a plain file name")]
    Name(String),
    #[error("{} is not there", .0.display())]
    Missing(PathBuf),
    #[error("{} is not a regular file", .0.display())]
    NotAFile(PathBuf),
    #[error("{} is read-only", .0.display())]
    ReadOnly(PathBuf),
    /// The file is not the version the caller last saw ([`Options::seen`]).
    #[error("{} changed on disk since Dalica last read it", .0.display())]
    Changed(PathBuf),
    /// Undo Save or Revert of a photo without a kept original.
    #[error("{0} has no kept original to put back")]
    NotSaved(String),
    /// An earlier Save of this photo did not finish; it is repaired first (M8.18).
    #[error("an earlier Save of {0} did not finish and is repaired first")]
    Pending(String),
    /// The originals record cannot be used for this photo.
    #[error("the originals record cannot be used: {0}")]
    Record(String),
    /// The new file did not read back as written; the original was put back.
    #[error("the saved file did not verify ({0}); the original was put back")]
    Verify(String),
    /// The Save, Undo Save or Revert stopped part-way and left the record for the repair (M8.18):
    /// a Save's entry stays `pending`.
    #[error("the change to {name} stopped part-way ({reason}); it is repaired at the next scan")]
    Incomplete { name: String, reason: String },
    #[error(transparent)]
    Output(#[from] output::Error),
    #[error(transparent)]
    Fs(#[from] dalica_fs::Error),
}

impl Error {
    /// The text shown to people.
    pub fn reason(&self) -> String {
        match self {
            Error::Output(e) => e.reason(),
            other => other.to_string(),
        }
    }

    /// A simulated crash (`fs`'s fault points): the operation stopped as a killed process would.
    fn crashed(&self) -> bool {
        matches!(self, Error::Fs(dalica_fs::Error::Crashed(_)))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What the caller last knew of the photo's file (the library's size and modification time, as
/// the scan read them). A file that no longer matches is refused: Dalica never writes over a
/// version it has not seen (`security.md` §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub size: u64,
    pub mtime: Timestamp,
}

/// How to save.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The write-back options the owned fields are written under, and the metadata limits.
    pub write: WriteOptions,
    /// The decode limits the source is read under.
    pub limits: Limits,
    /// Threads the JPEG encoder may use.
    pub threads: usize,
    /// The file as the caller last saw it; `None` checks only that it does not change while
    /// Save works.
    pub seen: Option<Seen>,
    /// The time of the Save (the session's clock; this crate reads none).
    pub now: Timestamp,
}

impl Options {
    /// Save at `now` with the default write-back options and limits, without a [`Seen`] check.
    pub fn new(now: Timestamp) -> Options {
        let spec = Spec::new(Purpose::InPlace);
        Options { write: spec.write, limits: spec.limits, threads: spec.threads, seen: None, now }
    }
}

/// What a Save in place did.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// The record's entry as written (`done`): the whole recipe the saved file holds and the kept
    /// original's facts.
    pub entry: Entry,
    /// The saved file's identity.
    pub identity: Identity,
    /// What was written.
    pub report: Report,
    /// Whether this Save moved the original into the backup (its first Save).
    pub first: bool,
}

/// What [`save`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Saving {
    /// The photo's file holds the edits; its original is kept.
    InPlace(Saved),
    /// A JPEG was written beside the photo, which is untouched (M8.14).
    Beside(beside::Beside),
}

/// Whether Dalica can render the operation `text`: a known operation whose module reads its
/// parameters. Unknown and `legacy:` operations, operations without a renderer yet and parameters
/// a module refuses are not (`Library::unrenderable` takes this).
pub fn renders(text: &str) -> bool {
    matches!(Op::parse(text).typed(), Some(Ok(_)))
}

/// The first operation of `recipe` that cannot be rendered, refused by its text.
pub fn check_recipe(recipe: &Recipe) -> Result<()> {
    match recipe.ops().iter().find(|op| !matches!(op.typed(), Some(Ok(_)))) {
        Some(op) => Err(Error::Unrenderable(op.to_string())),
        None => Ok(()),
    }
}

/// Save: the photo `name` in `folder` with the new edits `recipe` (`photo.save`, M8.15). Routes
/// to the lossless rotation of a JPEG (M8.14), a JPEG beside a format Dalica cannot write in place
/// (M8.14), or [`in_place`].
///
/// The format is checked from the header before anything renders, so a Save that ends beside
/// decodes its source once (M9.30). Only a PNG needs its pixels to decide (transparency sends it
/// beside): it tries the Save in place, and a transparent one is rendered again for the JPEG.
pub fn save(folder: &Path, name: &str, recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Saving> {
    check_recipe(recipe)?;
    let photo = Photo::read(folder, name, options)?;
    if let Some(saved) = lossless::save(&photo, recipe, changes, options)? {
        return Ok(Saving::InPlace(saved));
    }
    let format = photo.header.format;
    if !may_save_in_place(format) {
        return save_beside(&photo, recipe, options, format);
    }
    match render_in_place(&photo, recipe, changes, options) {
        Err(Error::Output(output::Error::NotInPlace(format))) => save_beside(&photo, recipe, options, format),
        other => other.map(Saving::InPlace),
    }
}

/// Whether a photo of `format` can be saved in place when it has no transparency (JPEG and PNG):
/// any other format goes beside without being decoded first.
fn may_save_in_place(format: Format) -> bool {
    output::target(format, false, Purpose::InPlace).is_some()
}

/// The JPEG beside `photo` ([`beside`]: one render of the source).
fn save_beside(photo: &Photo, recipe: &Recipe, options: &Options, format: Format) -> Result<Saving> {
    count_decode();
    beside::save(photo, recipe, options, format).map(Saving::Beside)
}

thread_local! {
    /// The sources Save has decoded on this thread ([`decodes_on_this_thread`]).
    static DECODES: Cell<u64> = const { Cell::new(0) };
}

/// Counts one decode of the source: each render through the output pipeline reads it once.
fn count_decode() {
    DECODES.with(|n| n.set(n.get().saturating_add(1)));
}

/// How many times Save has decoded a photo's pixels on the calling thread (one per render
/// through the output pipeline, in place or beside). Tests read it to check that a Save decodes
/// its source once; Save runs on one thread, so a test reads its own Saves only.
#[doc(hidden)]
pub fn decodes_on_this_thread() -> u64 {
    DECODES.with(Cell::get)
}

/// Saves the photo `name` in `folder` in place with the new edits `recipe` and the library's owned
/// fields `changes`; see the module docs for the steps. A format that cannot be written in place
/// is `Err(Output(NotInPlace))`, with nothing changed.
pub fn in_place(folder: &Path, name: &str, recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Saved> {
    check_recipe(recipe)?;
    let photo = Photo::read(folder, name, options)?;
    if !may_save_in_place(photo.header.format) {
        return Err(output::Error::NotInPlace(photo.header.format).into());
    }
    render_in_place(&photo, recipe, changes, options)
}

/// Steps 2 (the record) to 8 for a photo already read.
fn render_in_place(photo: &Photo, recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Saved> {
    let base = journal::Base::read(photo, options)?;
    let mut whole = base.recipe().to_vec();
    whole.extend(recipe.print());
    let whole = Recipe::parse(&whole).map_err(|_| Error::TooManyOperations)?;
    check_recipe(&whole)?;

    let spec = Spec {
        thumbnail: true,
        changes: changes.clone(),
        write: options.write.clone(),
        limits: options.limits,
        threads: options.threads,
        ..Spec::new(Purpose::InPlace)
    };
    count_decode();
    let out = output::render(base.source(photo), &whole, &spec)?;
    let new = journal::NewFile {
        bytes: &out.bytes,
        width: out.report.width,
        height: out.report.height,
        orientation: dalica_image::Orientation::UPRIGHT,
        changes,
        write: &options.write,
    };
    let (entry, identity, first) = journal::commit(photo, base, whole.print(), &new, options)?;
    Ok(Saved { entry, identity, report: out.report, first })
}

/// The photo as Save read it.
pub(crate) struct Photo {
    pub folder: PathBuf,
    pub name: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub identity: Identity,
    /// Its modification time, as the scan reads it.
    pub mtime: Timestamp,
    pub header: Header,
}

impl Photo {
    /// Reads `folder/name` whole with its identity, refusing a name that is not one plain
    /// component, a missing or read-only file, and a file that is not the one the caller saw.
    fn read(folder: &Path, name: &str, options: &Options) -> Result<Photo> {
        if !backup_name_ok(name) {
            return Err(Error::Name(name.to_string()));
        }
        let path = folder.join(name);
        let stat = dalica_fs::stat(&path)?.ok_or_else(|| Error::Missing(path.clone()))?;
        if !stat.is_file {
            return Err(Error::NotAFile(path));
        }
        if !stat.writable {
            return Err(Error::ReadOnly(path));
        }
        let mtime = Timestamp::from_system_time(stat.times.modified);
        if options.seen.is_some_and(|seen| seen != Seen { size: stat.identity.size(), mtime }) {
            return Err(Error::Changed(path));
        }
        let read = dalica_fs::read(&path, MAX_FILE)?.ok_or_else(|| Error::Missing(path.clone()))?;
        if read.identity != stat.identity {
            return Err(Error::Changed(path));
        }
        let header = dalica_image::read_header(&read.bytes, &options.limits).map_err(output::Error::from)?;
        Ok(Photo { folder: folder.to_path_buf(), name: name.to_string(), path, bytes: read.bytes, identity: read.identity, mtime, header })
    }
}

/// A name Save may use: one plain path component that is not one of Dalica's own files.
fn backup_name_ok(name: &str) -> bool {
    dalica_fs::create::plain_name(name) && name != backup::DIR_NAME
}

/// The record entry a first Save writes for `photo` with the whole recipe `recipe`, `pending`.
fn first_entry(photo: &Photo, recipe: Vec<String>, now: Timestamp) -> Entry {
    let (width, height) = photo.header.display_size();
    Entry {
        recipe,
        size: photo.identity.size(),
        mtime: photo.mtime.as_millis(),
        width,
        height,
        digest: digest(&photo.bytes),
        state: State::Pending,
        time: Some(now.as_millis()),
    }
}
