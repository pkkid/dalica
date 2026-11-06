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
//! - [`png_layout()`]: our own PNG writer for every colour type and bit depth (1–16), palettes,
//!   `tRNS` and Adam7 interlacing; [`png_image`] gives its exact samples and the expected RGBA.
//! - [`jpeg_dc()`]: our own DC-only JPEG writer for 12-bit (SOF1) and arithmetic-coded (SOF9)
//!   frames, with [`jpeg_dc_reference`] (the block means a decoder shows).
//! - [`panorama()`]: a large RGB TIFF (8- or 16-bit, strips or tiles, uncompressed or Deflate, its
//!   IFD before or after the pixels) written one strip or tile at a time, never held whole, so
//!   tests and measurements can make 100–300 MP files; [`Panorama::sample`] gives every stored value.
//!
//! No encoder we use writes a CMYK JPEG or a lossy WebP; tests of those rely on hand-built
//! bytes and the corpus. Every writer is deterministic: the same scene and options give the same
//! bytes. [`samples`] gives the exact sample values a file stores, so decoder tests can compare.

use std::io::{Seek, SeekFrom, Write};

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::exif::{self, Exif};
use crate::orient;
use crate::render::{Rgb8, Scene};

pub mod codecs;
pub mod jpeg_dc;
pub mod pack;
pub mod png;
pub mod psd;
pub mod tiff;

pub use codecs::{Webp, bmp, gif, jpeg_icc, jxl, webp};
pub use jpeg_dc::{Coding, JpegDc, jpeg_dc, jpeg_dc_reference};
pub use png::{Png, PngColour, PngImage, png_image, png_layout};
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

/// A large procedural panorama TIFF ([`panorama()`]): RGB, little-endian, one page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panorama {
    pub width: u32,
    pub height: u32,
    /// [`Depth::Eight`] or [`Depth::Sixteen`].
    pub depth: Depth,
    /// Square tiles of this side (a multiple of 16); `None`: strips of [`Panorama::rows_per_strip`].
    pub tile: Option<u32>,
    pub rows_per_strip: u32,
    /// Deflate (Compression 8) with the horizontal differencing predictor (Predictor 2) instead of
    /// uncompressed chunks.
    pub deflate: bool,
    /// The IFD after the pixel data, as many stitchers write it, instead of right after the header.
    pub ifd_at_end: bool,
}

impl Default for Panorama {
    fn default() -> Panorama {
        Panorama { width: 16_384, height: 4_097, depth: Depth::Eight, tile: None, rows_per_strip: 16, deflate: true, ifd_at_end: true }
    }
}

impl Panorama {
    /// The largest sample value.
    pub fn max(&self) -> u32 {
        if self.depth == Depth::Sixteen { 65_535 } else { 255 }
    }

    /// The stored sample of channel `c` (0–2) at (`x`, `y`): red rises from left to right, green
    /// from top to bottom, blue is a tent peaking in the middle column. Every channel is linear
    /// across any cell that does not straddle the middle, so an area average over a cell equals its
    /// value at the cell's centre within rounding.
    pub fn sample(&self, x: u32, y: u32, c: usize) -> u32 {
        let max = u64::from(self.max());
        let ramp = |v: u32, len: u32| {
            let span = u64::from(len.saturating_sub(1).max(1));
            ((u64::from(v.min(len.saturating_sub(1))) * max + span / 2) / span) as u32
        };
        match c {
            0 => ramp(x, self.width),
            1 => ramp(y, self.height),
            _ => {
                let mid = self.width / 2;
                let from_mid = x.abs_diff(mid);
                self.max() - ramp(from_mid, mid.saturating_add(1))
            }
        }
    }
}

/// Writes `p` to `out` one strip or tile at a time (memory: one chunk and the offset tables) and
/// returns the file's length. Tiles at the right and bottom edges are padded as TIFF 6.0 requires.
pub fn panorama(p: &Panorama, out: &mut (impl Write + Seek)) -> Result<u64> {
    let unsupported = |what: &str| Error::Unsupported { format: "panorama", what: what.to_owned() };
    if p.depth == Depth::Float {
        return Err(unsupported("float samples"));
    }
    if p.width == 0 || p.height == 0 || p.rows_per_strip == 0 || p.tile.is_some_and(|t| t == 0 || t % 16 != 0) {
        return Err(unsupported("an empty size, strip or tile"));
    }
    let io = |e: std::io::Error| Error::Encode { format: "TIFF", message: e.to_string() };
    let bytes = p.depth.bytes();
    // Each chunk: its first column and row, and its stored width and height (tiles padded).
    let chunks: Vec<(u32, u32, u32, u32)> = match p.tile {
        Some(t) => (0..p.height.div_ceil(t)).flat_map(|ty| (0..p.width.div_ceil(t)).map(move |tx| (tx * t, ty * t, t, t))).collect(),
        None => (0..p.height.div_ceil(p.rows_per_strip))
            .map(|k| {
                let y = k * p.rows_per_strip;
                (0, y, p.width, p.rows_per_strip.min(p.height - y))
            })
            .collect(),
    };
    let n = u32::try_from(chunks.len()).map_err(|_| unsupported("more than 2³² chunks"))?;
    let bits = if p.depth == Depth::Sixteen { 16u16 } else { 8 };
    // (tag, type, count, inline value or None for "points at `values`").
    let (offsets_tag, counts_tag) = if p.tile.is_some() { (324u16, 325u16) } else { (273, 279) };
    let mut entries: Vec<(u16, u16, u32, Option<u32>)> = vec![
        (256, 4, 1, Some(p.width)),
        (257, 4, 1, Some(p.height)),
        (258, 3, 3, None),
        (259, 3, 1, Some(if p.deflate { 8 } else { 1 })),
        (262, 3, 1, Some(2)),
        (offsets_tag, 4, n, if n == 1 { Some(0) } else { None }),
        (277, 3, 1, Some(3)),
        (counts_tag, 4, n, if n == 1 { Some(0) } else { None }),
        (284, 3, 1, Some(1)),
    ];
    if p.deflate {
        entries.push((317, 3, 1, Some(2)));
    }
    match p.tile {
        Some(t) => entries.extend([(322, 4, 1, Some(t)), (323, 4, 1, Some(t))]),
        None => entries.push((278, 4, 1, Some(p.rows_per_strip))),
    }
    entries.sort_by_key(|e| e.0);
    let ifd_len = 2 + 12 * entries.len() as u64 + 4;
    let tables = if n == 1 { 0 } else { 8 * u64::from(n) };
    let ifd_block = ifd_len + 6 + tables;
    let start = out.stream_position().map_err(io)?;
    let at = |v: u64| start + v;
    let ifd_at = if p.ifd_at_end { None } else { Some(8u64) };
    out.write_all(b"II*\0").map_err(io)?;
    out.write_all(&[0; 4]).map_err(io)?;
    if ifd_at.is_some() {
        out.write_all(&vec![0; ifd_block as usize]).map_err(io)?;
    }
    let mut offsets = Vec::with_capacity(chunks.len());
    let mut counts = Vec::with_capacity(chunks.len());
    let mut raw = Vec::new();
    for &(x0, y0, w, h) in &chunks {
        raw.clear();
        for y in y0..y0 + h {
            let mut prev = [0u32; 3];
            for x in x0..x0 + w {
                for (c, last) in prev.iter_mut().enumerate() {
                    let v = if x < p.width && y < p.height { p.sample(x, y, c) } else { 0 };
                    // Predictor 2: each sample stored as its difference from the one to its left.
                    let stored = if p.deflate { v.wrapping_sub(*last) } else { v };
                    *last = v;
                    match bytes {
                        1 => raw.push(stored as u8),
                        _ => raw.extend((stored as u16).to_le_bytes()),
                    }
                }
            }
        }
        let data = if p.deflate { pack::deflate(&raw) } else { std::mem::take(&mut raw) };
        let here = out.stream_position().map_err(io)? - start;
        offsets.push(u32::try_from(here).map_err(|_| unsupported("a file past 4 GiB"))?);
        counts.push(data.len() as u32);
        out.write_all(&data).map_err(io)?;
        if data.len() % 2 == 1 {
            out.write_all(&[0]).map_err(io)?;
        }
    }
    let end = out.stream_position().map_err(io)? - start;
    let ifd = ifd_at.unwrap_or(end);
    let ifd32 = u32::try_from(ifd).map_err(|_| unsupported("a file past 4 GiB"))?;
    let mut block = Vec::with_capacity(ifd_block as usize);
    let values = ifd + ifd_len;
    let (bits_at, offsets_at, counts_at) = (values, values + 6, values + 6 + 4 * u64::from(n));
    block.extend((entries.len() as u16).to_le_bytes());
    for &(tag, kind, count, inline) in &entries {
        block.extend(tag.to_le_bytes());
        block.extend(kind.to_le_bytes());
        block.extend(count.to_le_bytes());
        let value = match (tag, inline) {
            (258, _) => bits_at as u32,
            (t, Some(_)) if t == offsets_tag => offsets.first().copied().unwrap_or(0),
            (t, Some(_)) if t == counts_tag => counts.first().copied().unwrap_or(0),
            (_, Some(v)) if kind == 3 => v & 0xffff,
            (_, Some(v)) => v,
            (t, None) if t == offsets_tag => offsets_at as u32,
            (_, None) => counts_at as u32,
        };
        block.extend(value.to_le_bytes());
    }
    block.extend(0u32.to_le_bytes());
    for _ in 0..3 {
        block.extend(bits.to_le_bytes());
    }
    if n > 1 {
        block.extend(offsets.iter().flat_map(|o| o.to_le_bytes()));
        block.extend(counts.iter().flat_map(|c| c.to_le_bytes()));
    }
    out.seek(SeekFrom::Start(at(ifd))).map_err(io)?;
    out.write_all(&block).map_err(io)?;
    out.seek(SeekFrom::Start(at(4))).map_err(io)?;
    out.write_all(&ifd32.to_le_bytes()).map_err(io)?;
    let len = ifd.max(end) + if ifd_at.is_none() { block.len() as u64 } else { 0 };
    out.seek(SeekFrom::Start(at(len))).map_err(io)?;
    Ok(len)
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

    #[test]
    fn a_panorama_is_read_back_by_an_independent_decoder() {
        for (tile, ifd_at_end, deflate, depth) in [
            (None, true, true, Depth::Eight),
            (Some(16), false, false, Depth::Sixteen),
            (None, false, false, Depth::Eight),
            (Some(32), true, true, Depth::Eight),
        ] {
            let p = Panorama { width: 70, height: 37, depth, tile, rows_per_strip: 5, deflate, ifd_at_end };
            let mut out = std::io::Cursor::new(Vec::new());
            let len = panorama(&p, &mut out).unwrap();
            let bytes = out.into_inner();
            assert_eq!(len, bytes.len() as u64);
            let mut d = ::tiff::decoder::Decoder::new(std::io::Cursor::new(&bytes)).unwrap();
            assert_eq!(d.dimensions().unwrap(), (70, 37));
            let want: Vec<u32> = (0..37).flat_map(|y| (0..70).flat_map(move |x| (0..3).map(move |c| p.sample(x, y, c)))).collect();
            let got: Vec<u32> = match d.read_image().unwrap() {
                ::tiff::decoder::DecodingResult::U8(v) => v.into_iter().map(u32::from).collect(),
                ::tiff::decoder::DecodingResult::U16(v) => v.into_iter().map(u32::from).collect(),
                _ => panic!("another sample type"),
            };
            assert_eq!(got, want, "{p:?}");
        }
        assert_eq!(Panorama::default().sample(16_383, 0, 0), 255);
        assert_eq!(Panorama::default().sample(8_192, 9, 2), 255);
        assert_eq!(Panorama::default().sample(0, 4_096, 1), 255);
    }
}
