//! Scenes written in every format Dalica reads (`docs/formats.md` §1), for decoder tests.
//!
//! - [`tiff()`]: our own writer: 8/16-bit and float; grey, RGB, CMYK, palette; uncompressed, LZW,
//!   Deflate or PackBits strips; either byte order; the Orientation tag; an embedded thumbnail IFD;
//!   an ICC profile.
//! - [`psd()`]: our own writer of a flattened Photoshop document: raw or RLE; 8/16-bit grey, RGB,
//!   CMYK, indexed, and 32-bit float grey and RGB; thumbnail and ICC resources.
//! - [`webp()`] (lossless, alpha, EXIF and ICC chunks), [`gif()`] (one or several frames),
//!   [`bmp()`], [`jpeg_icc()`] (JPEG with an APP2 profile): the pinned `image` crate's encoders.
//! - [`jxl()`]: lossless JPEG XL through `zune-jpegxl` (safe, pure Rust).
//!
//! No encoder we use writes a CMYK JPEG or a lossy WebP; tests of those rely on hand-built
//! bytes and the corpus. Every writer is deterministic: the same scene and options give the same
//! bytes. [`samples`] gives the exact sample values a file stores, so decoder tests can compare.

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::exif::{self, Exif};
use crate::orient;
use crate::render::{Rgb8, Scene};

pub mod codecs;
pub mod pack;
pub mod psd;
pub mod tiff;

pub use codecs::{Webp, bmp, gif, jpeg_icc, jxl, webp};
pub use psd::{Psd, psd};
pub use tiff::{Compression, Tiff, tiff};

/// Why a test file could not be written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Scene(#[from] crate::Error),
    #[error("{format} cannot store {what}")]
    Unsupported { format: &'static str, what: String },
    #[error("{format} holds at most {max} pixels on a side, not {width} × {height}")]
    TooLarge { format: &'static str, max: u32, width: u32, height: u32 },
    #[error("{format} encoding failed: {message}")]
    Encode { format: &'static str, message: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// How the samples describe colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colour {
    /// One channel (Rec. 709 luma of the scene).
    Grey,
    Rgb,
    /// Ink amounts, 0 = no ink (a naive conversion; TIFF `InkSet` 1).
    Cmyk,
    /// 8-bit indices into [`palette`].
    Palette,
}

impl Colour {
    pub const ALL: [Colour; 4] = [Colour::Grey, Colour::Rgb, Colour::Cmyk, Colour::Palette];

    /// Samples per pixel.
    pub fn channels(self) -> usize {
        match self {
            Colour::Grey | Colour::Palette => 1,
            Colour::Rgb => 3,
            Colour::Cmyk => 4,
        }
    }
}

/// Bits and kind of each sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Depth {
    Eight,
    /// `v × 256` plus a position pattern in the low byte, so a decoder that drops or swaps the low
    /// byte shows up.
    Sixteen,
    /// 32-bit IEEE float, 0.0–1.0.
    Float,
}

impl Depth {
    pub const ALL: [Depth; 3] = [Depth::Eight, Depth::Sixteen, Depth::Float];

    pub fn bytes(self) -> usize {
        match self {
            Depth::Eight => 1,
            Depth::Sixteen => 2,
            Depth::Float => 4,
        }
    }
}

/// Sample values, interleaved per pixel, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum SampleData {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

/// The exact samples a file stores.
#[derive(Clone, Debug, PartialEq)]
pub struct Samples {
    pub width: u32,
    pub height: u32,
    pub colour: Colour,
    pub depth: Depth,
    pub data: SampleData,
}

/// The fixed 256-entry palette: a 6 × 6 × 6 cube (index `36 r + 6 g + b`, levels `0, 51, … 255`),
/// then black.
pub fn palette() -> [[u8; 3]; 256] {
    let mut p = [[0u8; 3]; 256];
    for (i, slot) in p.iter_mut().enumerate().take(216) {
        let i = i as u8;
        *slot = [i / 36 * 51, i / 6 % 6 * 51, i % 6 * 51];
    }
    p
}

/// The palette index nearest to a colour (each channel rounded to the cube's level).
pub fn palette_index([r, g, b]: [u8; 3]) -> u8 {
    let level = |v: u8| ((u16::from(v) * 5 + 127) / 255) as u8;
    level(r) * 36 + level(g) * 6 + level(b)
}

/// `img` with every pixel replaced by its palette colour (what palette files show).
pub fn quantized(img: &Rgb8) -> Rgb8 {
    let p = palette();
    let data = img.data.as_chunks::<3>().0.iter().flat_map(|c| p.get(usize::from(palette_index(*c))).copied().unwrap_or_default()).collect();
    Rgb8 { width: img.width, height: img.height, data }
}

/// A soft alpha mask: opaque in the middle, fading to 48 at the corners, never fully transparent
/// (so lossless encoders keep every colour).
pub fn alpha(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (u64::from(width.max(1)), u64::from(height.max(1)));
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            // Squared distance from the centre, 0 in the middle and 2·10⁶ at the corners.
            let dx = (2 * x + 1).abs_diff(w) * 1000 / w;
            let dy = (2 * y + 1).abs_diff(h) * 1000 / h;
            let d = dx * dx + dy * dy;
            out.push((255 - d.min(2_000_000) * 207 / 2_000_000) as u8);
        }
    }
    out
}

fn luma([r, g, b]: [u32; 3], max: u32) -> u32 {
    ((r * 2126 + g * 7152 + b * 722 + 5000) / 10_000).min(max)
}

/// Naive CMYK ink amounts: K from the brightest channel, then C, M, Y relative to it.
fn cmyk([r, g, b]: [u32; 3], max: u32) -> [u32; 4] {
    let top = r.max(g).max(b);
    if top == 0 {
        return [0, 0, 0, max];
    }
    let ink = |v: u32| ((u64::from(top - v) * u64::from(max) + u64::from(top) / 2) / u64::from(top)) as u32;
    [ink(r), ink(g), ink(b), max - top]
}

/// The samples a file of `colour` and `depth` stores for the (already oriented) pixels `img`.
pub fn samples(img: &Rgb8, colour: Colour, depth: Depth) -> Result<Samples> {
    let unsupported = |what: &str| Error::Unsupported { format: "samples", what: what.to_owned() };
    if colour == Colour::Palette && depth != Depth::Eight {
        return Err(unsupported("palette indices deeper than 8 bits"));
    }
    if colour == Colour::Cmyk && depth == Depth::Float {
        return Err(unsupported("float CMYK"));
    }
    let width = img.width as usize;
    // Each RGB triple at 16 bits: the 8-bit value in the high byte, a position pattern in the low.
    let rgb16 = img.data.as_chunks::<3>().0.iter().enumerate().map(|(i, c)| {
        let (x, y) = (i % width.max(1), i / width.max(1));
        let low = |k: usize| ((x * 7 + y * 13 + k * 85) & 0xff) as u32;
        [0, 1, 2].map(|k| u32::from(c.get(k).copied().unwrap_or(0)) * 256 + low(k))
    });
    let data = match (colour, depth) {
        (Colour::Palette, _) => SampleData::U8(img.data.as_chunks::<3>().0.iter().map(|c| palette_index(*c)).collect()),
        (Colour::Rgb, Depth::Eight) => SampleData::U8(img.data.clone()),
        (Colour::Grey, Depth::Eight) => SampleData::U8(img.data.as_chunks::<3>().0.iter().map(|c| luma(c.map(u32::from), 255) as u8).collect()),
        (Colour::Cmyk, Depth::Eight) => {
            SampleData::U8(img.data.as_chunks::<3>().0.iter().flat_map(|c| cmyk(c.map(u32::from), 255).map(|v| v as u8)).collect())
        }
        (Colour::Rgb, Depth::Sixteen) => SampleData::U16(rgb16.flat_map(|c| c.map(|v| v as u16)).collect()),
        (Colour::Grey, Depth::Sixteen) => SampleData::U16(rgb16.map(|c| luma(c, 65_535) as u16).collect()),
        (Colour::Cmyk, Depth::Sixteen) => SampleData::U16(rgb16.flat_map(|c| cmyk(c, 65_535).map(|v| v as u16)).collect()),
        (Colour::Rgb, Depth::Float) => SampleData::F32(rgb16.flat_map(|c| c.map(|v| v as f32 / 65_535.0)).collect()),
        (Colour::Grey, Depth::Float) => SampleData::F32(rgb16.map(|c| luma(c, 65_535) as f32 / 65_535.0).collect()),
        (Colour::Cmyk, Depth::Float) => return Err(unsupported("float CMYK")),
    };
    Ok(Samples { width: img.width, height: img.height, colour, depth, data })
}

/// The pixels stored for `exif`'s orientation and the EXIF TIFF block (with its thumbnail), or the
/// upright pixels and no block.
pub(crate) fn stored(scene: &Scene, exif: Option<&Exif>) -> Result<(Rgb8, Option<Vec<u8>>)> {
    let upright = scene.render()?;
    let Some(exif) = exif else { return Ok((upright, None)) };
    let stored = orient::store(&upright, exif.orientation)?;
    let thumb = match exif.thumbnail {
        Some(long_side) => Some(encode_jpeg(&stored.downscale(long_side)?, 70, None, None)?),
        None => None,
    };
    let block = exif::tiff(exif, stored.width, stored.height, thumb.as_deref())?;
    Ok((stored, Some(block)))
}

/// A baseline JPEG of `img`, with an optional EXIF block (APP1) and ICC profile (APP2).
pub(crate) fn encode_jpeg(img: &Rgb8, quality: u8, exif: Option<&[u8]>, icc: Option<&[u8]>) -> Result<Vec<u8>> {
    let err = |e: &dyn std::fmt::Display| Error::Encode { format: "JPEG", message: e.to_string() };
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    if let Some(block) = exif {
        enc.set_exif_metadata(block.to_vec()).map_err(|e| err(&e))?;
    }
    if let Some(profile) = icc {
        enc.set_icc_profile(profile.to_vec()).map_err(|e| err(&e))?;
    }
    enc.write_image(&img.data, img.width, img.height, ExtendedColorType::Rgb8).map_err(|e| err(&e))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_is_a_cube_and_indices_round_to_it() {
        let p = palette();
        assert_eq!(p[0], [0, 0, 0]);
        assert_eq!(p[215], [255, 255, 255]);
        assert_eq!(p[36 + 12 + 3], [51, 102, 153]);
        assert_eq!(palette_index([50, 103, 140]), 36 + 12 + 3);
        assert_eq!(palette_index([255, 0, 26]), 180 + 1);
    }

    #[test]
    fn samples_cover_every_supported_layout() {
        let img = Rgb8 { width: 2, height: 1, data: vec![255, 255, 255, 200, 100, 0] };
        let grey = samples(&img, Colour::Grey, Depth::Eight).unwrap();
        assert_eq!(grey.data, SampleData::U8(vec![255, 114]));
        let cmyk = samples(&img, Colour::Cmyk, Depth::Eight).unwrap();
        assert_eq!(cmyk.data, SampleData::U8(vec![0, 0, 0, 0, 0, 128, 255, 55]));
        let SampleData::U16(rgb16) = samples(&img, Colour::Rgb, Depth::Sixteen).unwrap().data else { panic!() };
        assert_eq!(rgb16.len(), 6);
        assert!(rgb16.iter().zip(&img.data).all(|(v, b)| v >> 8 == u16::from(*b)));
        assert!(rgb16.iter().any(|v| v & 0xff != v >> 8), "the low byte differs from the high byte");
        let SampleData::F32(f) = samples(&img, Colour::Rgb, Depth::Float).unwrap().data else { panic!() };
        assert!(f.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(samples(&img, Colour::Palette, Depth::Sixteen).is_err());
        assert!(samples(&img, Colour::Cmyk, Depth::Float).is_err());
        // Black has full black ink and nothing else.
        let black = Rgb8 { width: 1, height: 1, data: vec![0, 0, 0] };
        assert_eq!(samples(&black, Colour::Cmyk, Depth::Eight).unwrap().data, SampleData::U8(vec![0, 0, 0, 255]));
    }

    #[test]
    fn the_alpha_mask_is_opaque_in_the_middle_and_never_clear() {
        let a = alpha(9, 7);
        assert_eq!(a.len(), 63);
        assert_eq!(a[3 * 9 + 4], 255);
        assert!(a.iter().all(|&v| v >= 48));
        assert!(a[0] < 120, "{}", a[0]);
    }
}
