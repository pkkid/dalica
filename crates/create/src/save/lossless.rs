//! Lossless Save of a JPEG whose recipe only rotates and flips (`data-model.md` §6,
//! `formats.md` §1), through the same journaled steps as a rendered Save ([`super::journal`]).
//!
//! The net transform is the photo's EXIF orientation followed by the recipe's rotations and flips
//! (for a photo saved before: the kept original's orientation and the whole recipe, so a later
//! rotate-only Save starts again from the original's own coefficients). Then:
//!
//! 1. **DCT-domain transform** (`image::jpeg_transform`, M8.8): the file turned in its quantised
//!    coefficients with its own quantisation tables, the orientation set to 1 (`meta`'s
//!    `set_orientation`, M8.7: only IFD0's value and XMP's `tiff:Orientation` change), and the
//!    EXIF thumbnail, which the transform copies as it was, replaced by the same thumbnail turned
//!    the same way and re-encoded into the bytes it occupied (the rest of its span zeros after its
//!    end-of-image marker), so every offset in the EXIF block stays. Every other segment is
//!    unchanged.
//! 2. **Orientation fallback**, when the transform answers "not lossless here" (a partial MCU on
//!    an edge the transform moves, progressive, arithmetic-coded or 12-bit files, data after the
//!    end of the image), when the turned thumbnail cannot be read or does not fit its old span,
//!    and when the net transform is the identity: the pixels stay byte for byte and the
//!    orientation tag becomes the composed transform (an EXIF block added when there is none).
//!    The thumbnail stays right, since readers turn it by the same tag.
//!
//! Either way the library's owned fields are then merged under the write-back options (`meta`'s
//! `write_owned`, which verifies its own write), and the file goes to [`super::journal::commit`]:
//! the entry journaled, the original kept in `.dalica-originals/`, the file verified against the
//! pixel size and orientation computed here.

use dalica_edit::{Op, Recipe};
use dalica_image::encode::{self, Channels, Image, Metadata, Subsampling};
use dalica_image::jpeg_transform::{self, Refusal};
use dalica_image::{Format, Limits, Orientation, jpeg_quality, orient};
use dalica_meta::Changes;
use dalica_meta::carry::set_orientation;

use super::journal::{self, Base, NewFile};
use super::{Options, Photo, Result, Saved};
use crate::output::{self, OutFormat, Report, THUMBNAIL_QUALITY};

/// The JPEG qualities tried, in order, for a turned EXIF thumbnail until one fits its old span.
const THUMBNAIL_QUALITIES: [u8; 5] = [THUMBNAIL_QUALITY, 65, 50, 35, 20];

/// Whether `recipe` holds only rotations and flips (and at least one): the recipes a JPEG can
/// save without re-encoding.
pub fn applies(recipe: &Recipe) -> bool {
    !recipe.is_empty() && recipe.ops().iter().all(|op| matches!(op, Op::Rotate(_) | Op::FlipH | Op::FlipV))
}

/// How a lossless Save changed the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The coefficients were turned and the orientation set to 1.
    Transformed,
    /// The pixels stayed and the orientation tag holds the composed transform.
    Orientation,
}

/// A lossless Save's new file, before the journal commits it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turned {
    pub bytes: Vec<u8>,
    pub method: Method,
    /// The EXIF orientation the new file has.
    pub orientation: Orientation,
    /// Its size as displayed.
    pub width: u32,
    pub height: u32,
}

/// The new file of a lossless Save of the JPEG `source` with the rotations and flips of `recipe`
/// and the owned fields `changes` (see the module docs). Pure: nothing touches the disk.
///
/// `Ok(None)` when the recipe is not rotate-only or the source is not a JPEG: the Save renders.
/// A damaged or oversized file, and metadata `meta` cannot rewrite safely, are `Err`.
pub fn turn(source: &[u8], recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Option<Turned>> {
    if !applies(recipe) {
        return Ok(None);
    }
    let header = dalica_image::read_header(source, &options.limits).map_err(output::Error::from)?;
    if header.format != Format::Jpeg {
        return Ok(None);
    }
    let net = recipe.display(header.orientation).to_exif();
    let (width, height) = net.display_size(header.width, header.height);
    let meta_limits = &options.write.limits;
    // The tag is written only when it changes, so a file already reading as wanted keeps its bytes.
    let tagged = |bytes: &[u8], o: Orientation| -> Result<Vec<u8>> {
        if o == header.orientation {
            return Ok(bytes.to_vec());
        }
        set_orientation(bytes, u16::from(o.get()), meta_limits).map_err(|e| output::Error::from(e).into())
    };

    let transformed = if net == Orientation::UPRIGHT {
        None
    } else {
        match jpeg_transform::transform(source, net, &options.limits) {
            Ok(bytes) => refresh_thumbnail(tagged(&bytes, Orientation::UPRIGHT)?, net, &options.limits),
            Err(Refusal::NotLossless(_)) => None,
            Err(Refusal::Damaged(e)) => return Err(output::Error::Decode(e).into()),
        }
    };
    let (bytes, method, orientation) = match transformed {
        Some(bytes) => (bytes, Method::Transformed, Orientation::UPRIGHT),
        None => (tagged(source, net)?, Method::Orientation, net),
    };
    let bytes = dalica_meta::write_owned(&bytes, changes, &options.write).map_err(output::Error::from)?;
    Ok(Some(Turned { bytes, method, orientation, width, height }))
}

/// Saves `photo` losslessly when its format and the whole recipe (the recorded one followed by
/// `recipe`) allow; `None` sends it to the rendered Save.
pub(super) fn save(photo: &Photo, recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Option<Saved>> {
    if photo.header.format != Format::Jpeg || !applies(recipe) {
        return Ok(None);
    }
    let base = Base::read(photo, options)?;
    let mut whole = base.recipe().to_vec();
    whole.extend(recipe.print());
    // Too many operations, or a recorded operation that is not a rotation or flip: the rendered
    // Save reports or renders it.
    let Ok(whole) = Recipe::parse(&whole) else { return Ok(None) };
    let Some(turned) = turn(base.source(photo), &whole, changes, options)? else { return Ok(None) };

    let report = report(&turned);
    let new =
        NewFile { bytes: &turned.bytes, width: turned.width, height: turned.height, orientation: turned.orientation, changes, write: &options.write };
    let (entry, identity, first) = journal::commit(photo, base, whole.print(), &new, options)?;
    Ok(Some(Saved { entry, identity, report, first }))
}

/// `upright` (a turned JPEG with orientation 1) with its EXIF thumbnail turned by `o` too, written
/// into the bytes the old one occupied. `None` when the thumbnail cannot be read or turned, or
/// does not fit at any of [`THUMBNAIL_QUALITIES`]: the caller then keeps the pixels and sets the
/// tag. A file without an EXIF thumbnail is returned as it is.
fn refresh_thumbnail(mut upright: Vec<u8>, o: Orientation, limits: &Limits) -> Option<Vec<u8>> {
    let header = dalica_image::read_header(&upright, limits).ok()?;
    if header.exif.as_ref().is_some_and(|e| e.strip_thumbnail.is_some()) {
        // An uncompressed thumbnail is not rewritten here.
        return None;
    }
    let Some(span) = header.thumbnail.clone() else { return Some(upright) };
    let old = upright.get(span.clone())?;
    let small = orient::apply(dalica_image::decode(old, limits).ok()?, o).ok()?;
    let image = Image::rgba8(&small);
    let new = THUMBNAIL_QUALITIES
        .iter()
        .filter_map(|&q| encode::jpeg(&image, q, Subsampling::S420, &Metadata::default()).ok())
        .find(|bytes| bytes.len() <= span.len())?;
    let slot = upright.get_mut(span)?;
    let (head, rest) = slot.split_at_mut(new.len());
    head.copy_from_slice(&new);
    rest.fill(0);
    Some(upright)
}

/// What a lossless Save wrote: the source's own quality and subsampling, no new profile.
fn report(turned: &Turned) -> Report {
    let settings = jpeg_quality::read(&turned.bytes);
    let grey = settings.is_some_and(|s| s.sampling == jpeg_quality::Sampling::Grey);
    let thumbnail = dalica_image::read_header(&turned.bytes, &Limits::default()).is_ok_and(|h| h.thumbnail.is_some());
    Report {
        format: OutFormat::Jpeg,
        width: turned.width,
        height: turned.height,
        bits: 8,
        channels: if grey { Channels::Grey } else { Channels::Rgb },
        quality: settings.map(|s| s.quality),
        subsampling: settings.filter(|_| !grey).map(|s| s.sampling.for_jpeg()),
        source_quality: settings.map(|s| s.quality),
        icc: false,
        thumbnail,
        source_metadata: true,
        dropped: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rotations_and_flips_apply() {
        let parse = |ops: &[&str]| Recipe::parse(ops).unwrap_or_default();
        assert!(applies(&parse(&["rotate(90)", "flip_h()"])));
        assert!(!applies(&parse(&[])));
        assert!(!applies(&parse(&["rotate(90)", "autofix()"])));
    }
}
