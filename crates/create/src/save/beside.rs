//! Save of a format Dalica cannot write in place (`data-model.md` §6, `formats.md` §1): a JPEG
//! `name.jpg` (or the next free name) written beside the photo through `fs::create`, the source
//! untouched; it answers the new name and the recipe the copy holds (the photo's `savedAs`).
//! Built by M8.14; until then such a Save is refused with the reason, nothing written.

use dalica_edit::Recipe;
use dalica_image::Format;

use super::{Error, Options, Photo, Result};
use crate::output::{self, Report};

/// What a Save beside the photo wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beside {
    /// The new JPEG's name in the photo's folder.
    pub name: String,
    /// The recipe it was rendered with (the photo's `savedAs`).
    pub recipe: Vec<String>,
    pub report: Report,
}

/// Writes the JPEG beside `photo`, whose `format` cannot be written in place (M8.14).
pub(super) fn save(photo: &Photo, recipe: &Recipe, options: &Options, format: Format) -> Result<Beside> {
    let _ = (photo, recipe, options);
    Err(Error::Output(output::Error::NotInPlace(format)))
}
