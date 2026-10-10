//! Encoding scenes as JPEG (with an optional EXIF block; the `image` crate's baseline encoder) and
//! PNG (`png`). Both encoders are deterministic, so a scene always gives the same file bytes.

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::exif::{self, Exif};
use crate::render::{Rgb8, Scene};
use crate::{Error, Result, orient};

/// Quality of embedded EXIF thumbnails.
const THUMB_QUALITY: u8 = 70;

/// A baseline JPEG of `scene` at `quality` (1–100). With `exif`, the pixels are stored for its
/// orientation (so they display upright) and the EXIF block goes in APP1.
pub fn jpeg(scene: &Scene, quality: u8, exif: Option<&Exif>) -> Result<Vec<u8>> {
    let upright = scene.render()?;
    let Some(exif) = exif else { return encode_jpeg(&upright, quality, None) };
    let stored = orient::store(&upright, exif.orientation)?;
    let thumb = match exif.thumbnail {
        Some(long_side) => Some(encode_jpeg(&stored.downscale(long_side)?, THUMB_QUALITY, None)?),
        None => None,
    };
    let block = exif::tiff(exif, stored.width, stored.height, thumb.as_deref())?;
    encode_jpeg(&stored, quality, Some(&block))
}

/// A PNG of `scene` (8-bit RGB, upright; PNG has no orientation here).
pub fn png(scene: &Scene) -> Result<Vec<u8>> {
    let img = scene.render()?;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width, img.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| Error::Png(e.to_string()))?;
    writer.write_image_data(&img.data).map_err(|e| Error::Png(e.to_string()))?;
    writer.finish().map_err(|e| Error::Png(e.to_string()))?;
    Ok(out)
}

fn encode_jpeg(img: &Rgb8, quality: u8, exif_tiff: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    if let Some(block) = exif_tiff {
        enc.set_exif_metadata(block.to_vec()).map_err(|e| Error::Jpeg(e.to_string()))?;
    }
    enc.write_image(&img.data, img.width, img.height, ExtendedColorType::Rgb8).map_err(|e| Error::Jpeg(e.to_string()))?;
    Ok(out)
}
