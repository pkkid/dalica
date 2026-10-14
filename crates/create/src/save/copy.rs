//! Save a Copy and Save As (`data-model.md` §6): `<stem>-001.jpg` (the next free number) in the
//! photo's folder, or a chosen path that is never replaced in a chosen format (JPEG or PNG); the
//! photo and its edits untouched. Built by M8.14.

use std::path::{Path, PathBuf};

use dalica_edit::Recipe;
use dalica_meta::Changes;

use super::{Error, Options, Result};
use crate::output::{OutFormat, Report};

/// Where a copy goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Save a Copy: `<stem>-001.jpg`, the next free number, beside the photo.
    Numbered,
    /// Save As: this path, never replaced, in this format.
    Path { path: PathBuf, format: OutFormat },
}

/// What a copy wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub path: PathBuf,
    pub report: Report,
}

/// Writes a copy of the photo `name` in `folder` with `recipe` applied (M8.14).
pub fn copy(folder: &Path, name: &str, recipe: &Recipe, changes: &Changes, to: &Destination, options: &Options) -> Result<Copied> {
    let _ = (folder, name, recipe, changes, to, options);
    Err(Error::NotYet("Save a Copy"))
}
