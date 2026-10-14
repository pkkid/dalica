//! Save of a format Dalica cannot write in place (`data-model.md` §6, `formats.md` §1): a PNG with
//! transparency, TIFF, WebP, GIF, BMP, HEIF, JPEG XL, PSD and raws. The recipe is rendered through
//! the one output pipeline ([`crate::output::render`], `Purpose::Beside`: JPEG at Match quality,
//! transparency over white, a fresh EXIF thumbnail, the source's metadata carried) and written as
//! `<stem>.jpg` beside the photo, or the first free `<stem>-001.jpg`, `-002`, … through
//! `fs::create::new_file`, which never replaces a file. The source and the originals record are
//! untouched: the new JPEG is a new, unedited photo, and the source records it as its `savedAs`
//! (the name and the recipe it holds), so Undo Save and Revert do not apply to it.

use dalica_core::names::split_ext;
use dalica_edit::Recipe;
use dalica_image::Format;

use super::{Options, Photo, Result};
use crate::output::{self, Purpose, Report, Spec};

/// What a Save beside the photo wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beside {
    /// The new JPEG's name in the photo's folder.
    pub name: String,
    /// The recipe it was rendered with (the photo's `savedAs`).
    pub recipe: Vec<String>,
    pub report: Report,
}

/// The name a JPEG beside the photo `name` asks for first: its stem and `.jpg`.
pub fn jpeg_name(name: &str) -> String {
    format!("{}.jpg", split_ext(name).0)
}

/// Writes the JPEG beside `photo`, whose format cannot be written in place (`_format`, as the
/// pipeline answered), and leaves the photo as it is.
pub(super) fn save(photo: &Photo, recipe: &Recipe, options: &Options, _format: Format) -> Result<Beside> {
    let spec = Spec { thumbnail: true, write: options.write.clone(), limits: options.limits, threads: options.threads, ..Spec::new(Purpose::Beside) };
    let out = output::render(&photo.bytes, recipe, &spec)?;
    let created = dalica_fs::create::new_file(&photo.folder, &jpeg_name(&photo.name), &out.bytes)?;
    Ok(Beside { name: created.name, recipe: recipe.print(), report: out.report })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_stem_with_jpg() {
        assert_eq!(jpeg_name("IMG_0001.CR2"), "IMG_0001.jpg");
        assert_eq!(jpeg_name("scan.final.tiff"), "scan.final.jpg");
        assert_eq!(jpeg_name("noext"), "noext.jpg");
        assert_eq!(jpeg_name(".hidden.png"), ".hidden.jpg");
    }
}
