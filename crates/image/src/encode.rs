//! Encoders for the files Dalica writes (`formats.md` §1, `data-model.md` §6): a baseline JPEG
//! encoder of our own ([`jpeg`], [`jpeg_with_threads`]) and PNG through the `png` crate ([`png`]).
//! Both are pure: pixels and metadata in, the file's bytes out; nothing touches the disk.
//!
//! Adapted from LightCraft (MIT OR Apache-2.0), `crates/codecs/src/encode.rs` (`EncodeImage`,
//! `EncodeMeta`, `jpeg_app_segments`, `xmp_segment`, `encode_png`) at 25ef65c8. Changes: our own
//! error type instead of panicking slices, exact sample counts, 16-bit input to JPEG, no print
//! density, every JPEG through our own encoder (no `jpeg-encoder` dependency, no 4:2:2), and
//! metadata size limits for PNG too.
//!
//! - [`Image`]: the pixels to write, 8 or 16 bits, grey, grey + alpha, RGB or RGBA, straight
//!   alpha, sRGB (or whatever the ICC profile in [`Metadata`] says).
//! - [`Metadata`]: the EXIF block (TIFF-structured, starting at `II`/`MM`), the XMP packet and the
//!   ICC profile, written as JPEG APP1/APP1/APP2 segments or PNG `eXIf`/`iTXt`/`iCCP` chunks.
//! - JPEG drops alpha (callers flatten first) and reduces 16-bit samples to 8 bits.

mod app;
pub(crate) mod jpeg;

pub use app::{EXIF_MAX as JPEG_EXIF_MAX, ICC_MAX as JPEG_ICC_MAX, XMP_MAX as JPEG_XMP_MAX};

use crate::Rgba8;
use crate::decode::{ColorModel, Decoded, Samples};

/// Why an image could not be encoded. Every input is checked; nothing here panics.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("the image has no pixels ({width} × {height})")]
    Empty { width: u32, height: u32 },
    #[error("{width} × {height} is larger than a {format} file holds (at most {max} per side)")]
    TooLarge { format: &'static str, width: u32, height: u32, max: u32 },
    #[error("{have} samples for {width} × {height} × {channels} (expected {want})")]
    Samples { width: u32, height: u32, channels: usize, have: usize, want: u64 },
    #[error("JPEG quality {0} is outside 1–100")]
    Quality(u8),
    #[error("the {what} is {len} bytes; a {format} file holds at most {max}")]
    Segment { format: &'static str, what: &'static str, len: usize, max: usize },
    #[error("the EXIF block does not start with a TIFF header (II*\\0 or MM\\0*)")]
    ExifHeader,
    #[error("{0} cannot be encoded")]
    Unsupported(&'static str),
    #[error("PNG encoder: {0}")]
    Png(String),
    #[error("an encoder thread failed")]
    Thread,
}

/// The channels of each pixel, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channels {
    Grey,
    GreyAlpha,
    Rgb,
    Rgba,
}

impl Channels {
    /// Samples per pixel.
    pub fn count(self) -> usize {
        match self {
            Channels::Grey => 1,
            Channels::GreyAlpha => 2,
            Channels::Rgb => 3,
            Channels::Rgba => 4,
        }
    }

    /// Whether the colour is a single grey channel.
    pub fn is_grey(self) -> bool {
        matches!(self, Channels::Grey | Channels::GreyAlpha)
    }
}

/// Interleaved samples, rows top to bottom, no padding.
#[derive(Clone, Copy, Debug)]
pub enum Pixels<'a> {
    U8(&'a [u8]),
    U16(&'a [u16]),
}

impl Pixels<'_> {
    fn len(&self) -> usize {
        match self {
            Pixels::U8(s) => s.len(),
            Pixels::U16(s) => s.len(),
        }
    }
}

/// An image to encode: borrowed pixels and their layout.
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub channels: Channels,
    pub pixels: Pixels<'a>,
}

impl<'a> Image<'a> {
    /// 8-bit RGBA pixels (a render's output).
    pub fn rgba8(img: &'a Rgba8) -> Image<'a> {
        Image { width: img.width, height: img.height, channels: Channels::Rgba, pixels: Pixels::U8(&img.data) }
    }

    /// Decoded samples as they came from the file (8 or 16 bits, grey or RGB, with or without
    /// alpha). CMYK and float samples go through the colour stage first.
    pub fn from_decoded(d: &'a Decoded) -> Result<Image<'a>, EncodeError> {
        let channels = match (d.model, d.alpha) {
            (ColorModel::Gray, false) => Channels::Grey,
            (ColorModel::Gray, true) => Channels::GreyAlpha,
            (ColorModel::Rgb, false) => Channels::Rgb,
            (ColorModel::Rgb, true) => Channels::Rgba,
            (ColorModel::Cmyk, _) => return Err(EncodeError::Unsupported("CMYK")),
        };
        let pixels = match &d.samples {
            Samples::U8(s) => Pixels::U8(s),
            Samples::U16(s) => Pixels::U16(s),
            Samples::F32(_) => return Err(EncodeError::Unsupported("floating-point samples")),
        };
        Ok(Image { width: d.width, height: d.height, channels, pixels })
    }

    /// Checks the size against `max` per side and that the samples fill exactly
    /// `width × height × channels`.
    fn check(&self, format: &'static str, max: u32) -> Result<(), EncodeError> {
        let (width, height) = (self.width, self.height);
        if width == 0 || height == 0 {
            return Err(EncodeError::Empty { width, height });
        }
        if width > max || height > max {
            return Err(EncodeError::TooLarge { format, width, height, max });
        }
        let channels = self.channels.count();
        let want = u64::from(width).checked_mul(u64::from(height)).and_then(|p| p.checked_mul(channels as u64)).unwrap_or(u64::MAX);
        let have = self.pixels.len();
        if have as u64 != want {
            return Err(EncodeError::Samples { width, height, channels, have, want });
        }
        Ok(())
    }
}

/// Metadata written into the file. Empty blocks are left out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Metadata<'a> {
    /// The EXIF block, TIFF-structured (starting at `II*\0` or `MM\0*`, without `Exif\0\0`).
    pub exif: Option<&'a [u8]>,
    /// The XMP packet.
    pub xmp: Option<&'a str>,
    /// The ICC profile.
    pub icc: Option<&'a [u8]>,
}

impl<'a> Metadata<'a> {
    fn exif(&self) -> Result<Option<&'a [u8]>, EncodeError> {
        match self.exif.filter(|e| !e.is_empty()) {
            Some(e) if !(e.starts_with(b"II*\0") || e.starts_with(b"MM\0*")) => Err(EncodeError::ExifHeader),
            e => Ok(e),
        }
    }

    fn xmp(&self) -> Option<&'a str> {
        self.xmp.filter(|x| !x.is_empty())
    }

    fn icc(&self) -> Option<&'a [u8]> {
        self.icc.filter(|i| !i.is_empty())
    }
}

/// JPEG chroma subsampling. The encoder writes 4:4:4 and 4:2:0; a source at 4:2:2 (or any other
/// layout) is written as 4:4:4, never with less chroma (`jpeg_quality::Sampling::for_jpeg`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Subsampling {
    /// Full-resolution chroma.
    #[default]
    S444,
    /// Chroma halved in both directions.
    S420,
}

/// The largest side a baseline JPEG frame header holds.
pub const JPEG_MAX_SIDE: u32 = u16::MAX as u32;
/// The largest side a PNG header holds (2³¹ − 1).
pub const PNG_MAX_SIDE: u32 = 0x7FFF_FFFF;

/// A baseline JPEG at `quality` (1–100, the usual scaling of the T.81 Annex K tables), encoded on
/// the caller's thread. Grey images give a one-component JPEG; alpha is ignored.
pub fn jpeg(image: &Image<'_>, quality: u8, subsampling: Subsampling, segments: &Metadata<'_>) -> Result<Vec<u8>, EncodeError> {
    jpeg_with_threads(image, quality, subsampling, segments, 1)
}

/// [`jpeg`] with its restart-interval bands encoded on up to `threads` scoped threads (the caller's
/// thread counts as one; 0 and 1 both mean the caller's thread only). The bytes are the same for
/// every thread count.
pub fn jpeg_with_threads(
    image: &Image<'_>,
    quality: u8,
    subsampling: Subsampling,
    segments: &Metadata<'_>,
    threads: usize,
) -> Result<Vec<u8>, EncodeError> {
    image.check("JPEG", JPEG_MAX_SIDE)?;
    if !(1..=100).contains(&quality) {
        return Err(EncodeError::Quality(quality));
    }
    let app = app::jpeg_app_segments(segments)?;
    jpeg::encode(image, quality, subsampling, &app, threads.max(1))
}

/// A PNG of `image` at its own bit depth (8 or 16) and channels, with the metadata in `eXIf`,
/// `iTXt` (`XML:com.adobe.xmp`, uncompressed) and `iCCP` chunks.
pub fn png(image: &Image<'_>, chunks: &Metadata<'_>) -> Result<Vec<u8>, EncodeError> {
    image.check("PNG", PNG_MAX_SIDE)?;
    let chunk_max = PNG_MAX_SIDE as usize;
    let too_big = |what: &'static str, len: usize| (len > chunk_max).then_some(EncodeError::Segment { format: "PNG", what, len, max: chunk_max });
    let exif = chunks.exif()?;
    let (xmp, icc) = (chunks.xmp(), chunks.icc());
    for (what, len) in
        [("EXIF block", exif.map_or(0, <[u8]>::len)), ("XMP packet", xmp.map_or(0, str::len)), ("ICC profile", icc.map_or(0, <[u8]>::len))]
    {
        if let Some(e) = too_big(what, len) {
            return Err(e);
        }
    }
    let (bytes, depth): (std::borrow::Cow<'_, [u8]>, png::BitDepth) = match image.pixels {
        Pixels::U8(s) => (s.into(), png::BitDepth::Eight),
        Pixels::U16(s) => (s.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>().into(), png::BitDepth::Sixteen),
    };
    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = match image.channels {
        Channels::Grey => png::ColorType::Grayscale,
        Channels::GreyAlpha => png::ColorType::GrayscaleAlpha,
        Channels::Rgb => png::ColorType::Rgb,
        Channels::Rgba => png::ColorType::Rgba,
    };
    info.bit_depth = depth;
    info.icc_profile = icc.map(|b| b.into());
    info.exif_metadata = exif.map(|b| b.into());
    let e = |e: png::EncodingError| EncodeError::Png(e.to_string());
    let mut out = Vec::new();
    let mut enc = png::Encoder::with_info(&mut out, info).map_err(e)?;
    // As LightCraft: `fdeflate`'s fast mode, many times faster than `Balanced` on photos.
    enc.set_compression(png::Compression::Fast);
    if let Some(xmp) = xmp {
        enc.add_itxt_chunk("XML:com.adobe.xmp".into(), xmp.into()).map_err(e)?;
    }
    let mut w = enc.write_header().map_err(e)?;
    w.write_image_data(&bytes).map_err(e)?;
    w.finish().map_err(e)?;
    Ok(out)
}
