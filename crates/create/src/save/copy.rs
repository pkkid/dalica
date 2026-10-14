//! Save a Copy and Save As (`data-model.md` §6): the photo rendered with its edits through the one
//! output pipeline ([`crate::output::render`]: Match quality, a fresh EXIF thumbnail, the source's
//! metadata carried and every owned field written, stars as ratings too) into a new file that
//! never replaces one; the photo, its edits and its originals record untouched.
//!
//! - **Save a Copy** ([`Destination::Numbered`]): `<stem>-001.jpg` in the photo's folder, or the
//!   next free number (`-002`, …).
//! - **Save As** ([`Destination::Path`]): the chosen path in the chosen format (JPEG or PNG). An
//!   existing file there is refused before anything is rendered; the write itself goes through
//!   `fs::create::new_file`, which never replaces a file, so a file that appears at that name in
//!   the meantime gets the next free name instead ([`Copied::path`] says which).
//!
//! The copy renders the photo's file as it is with the edits given (for a saved photo, the saved
//! file and its unsaved edits).

use std::io;
use std::path::{Path, PathBuf};

use dalica_core::names::{MAX_NUMBER, numbered, split_ext};
use dalica_edit::Recipe;
use dalica_meta::{Changes, WriteOptions};

use super::{Error, MAX_FILE, Options, Result, backup_name_ok, check_recipe};
use crate::output::{self, Choice, OutFormat, Purpose, Report, Spec};

/// Where a copy goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Save a Copy: `<stem>-001.jpg`, the next free number, beside the photo.
    Numbered,
    /// Save As: this path, never replaced, in this format. A relative path is taken from the
    /// photo's folder.
    Path { path: PathBuf, format: OutFormat },
}

/// What a copy wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub path: PathBuf,
    pub report: Report,
}

/// Writes a copy of the photo `name` in `folder` with `recipe` applied and the owned fields
/// `changes` written into it (see the module docs).
pub fn copy(folder: &Path, name: &str, recipe: &Recipe, changes: &Changes, to: &Destination, options: &Options) -> Result<Copied> {
    check_recipe(recipe)?;
    if !backup_name_ok(name) {
        return Err(Error::Name(name.to_string()));
    }
    let (dir, file_name, format) = match to {
        Destination::Numbered => (folder.to_path_buf(), first_free_number(folder, name)?, Choice::For(Purpose::Copy)),
        Destination::Path { path, format } => {
            let path = folder.join(path);
            let file_name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| Error::Name(path.display().to_string()))?.to_string();
            let dir = path.parent().map_or_else(|| folder.to_path_buf(), Path::to_path_buf);
            if dalica_fs::stat(&path)?.is_some() {
                return Err(exists(&path));
            }
            (dir, file_name, Choice::Exactly(*format))
        }
    };

    let path = folder.join(name);
    let source = dalica_fs::read(&path, MAX_FILE)?.ok_or_else(|| Error::Missing(path.clone()))?;
    let spec = Spec {
        format,
        thumbnail: true,
        changes: changes.clone(),
        // A copy is a new file: every owned field goes in, whatever the write-back options say.
        write: WriteOptions { captions_tags_places: true, stars_as_ratings: true, dates: true, ..options.write.clone() },
        limits: options.limits,
        threads: options.threads,
        ..Spec::new(Purpose::Copy)
    };
    let out = output::render(&source.bytes, recipe, &spec)?;
    let created = dalica_fs::create::new_file(&dir, &file_name, &out.bytes)?;
    Ok(Copied { path: dir.join(created.name), report: out.report })
}

/// `<stem>-001.jpg` for the photo `name`, or the first number whose name is free in `folder`.
fn first_free_number(folder: &Path, name: &str) -> Result<String> {
    let jpeg = format!("{}.jpg", split_ext(name).0);
    for n in 1..=MAX_NUMBER {
        let candidate = numbered(&jpeg, n);
        if dalica_fs::stat(&folder.join(&candidate))?.is_none() {
            return Ok(candidate);
        }
    }
    Err(Error::Fs(dalica_fs::Error::Io {
        op: "create",
        path: folder.join(jpeg),
        source: io::Error::new(io::ErrorKind::AlreadyExists, "every numbered name is taken"),
    }))
}

/// Save As refuses a path that already holds something.
fn exists(path: &Path) -> Error {
    Error::Fs(dalica_fs::Error::Io {
        op: "create",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::AlreadyExists, "a file of that name already exists; choose another name"),
    })
}
