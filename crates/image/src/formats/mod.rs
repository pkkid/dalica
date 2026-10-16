//! One module per container format, so the tasks that fill them never share a file.
//!
//! Every format module has the same three entry points and two constants:
//!
//! - `header(bytes, &Limits) -> Result<Header>`: size, orientation, EXIF and the embedded
//!   thumbnail's location, without decoding pixels (what scanning reads).
//! - `decode(bytes, &Header, &mut Budget, &Limits) -> Result<Decoded>`: the full-resolution pixels
//!   as stored (before orientation) with the embedded ICC profile, every buffer taken from the
//!   [`Budget`]. `header` is the module's own header of the same bytes, already checked against
//!   the limits. Third-party decoders run inside the caller's `catch_unwind` guard.
//! - `embedded_thumbnail(bytes, &Limits) -> Result<Option<Range<usize>>>`: the byte range of a
//!   smaller picture the file carries (EXIF thumbnail, thumbnail IFD, PSD thumbnail resource), as
//!   a file Dalica decodes, for the thumbnail fast path.
//! - `DECODES`: whether this build decodes the format ([`Format::supported`]).
//! - `REASON`: the tile text when the module answers [`Error::Unsupported`] (`formats.md` §1).
//!
//! [`hint`] adds what the user can do about it, under the tile text, where there is something
//! (HEIC without the `heif` feature).
//!
//! Stubs answer `Unsupported` from all three until their task fills them: TIFF (M5.6), WebP, GIF
//! and BMP (M5.7), JPEG XL and PSD (M5.8), TIFF-structured raws (M5.9), the other raw containers
//! and HEIF (M5.10). AVIF is recognised and never decoded, so it has no module.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::sniff::Format;
use crate::{Error, Header, Result};

pub mod bmff;
pub mod bmp;
pub mod gif;
pub mod heif;
pub mod heif_items;
pub mod jpeg;
pub mod jxl;
pub mod png;
pub mod psd;
pub mod raw;
pub mod tiff;
pub mod webp;

/// A format module's `decode`.
pub type DecodeFn = fn(&[u8], &Header, &mut Budget, &Limits) -> Result<Decoded>;

/// The tile text for AVIF, which no build decodes (`formats.md` §1).
pub const AVIF_REASON: &str = "AVIF files can't be displayed";

/// The header of `bytes` read as `format` (no sniffing).
pub fn header(format: Format, bytes: &[u8], limits: &Limits) -> Result<Header> {
    match format {
        Format::Jpeg => jpeg::header(bytes, limits),
        Format::Png => png::header(bytes, limits),
        Format::Tiff => tiff::header(bytes, limits),
        Format::WebP => webp::header(bytes, limits),
        Format::Gif => gif::header(bytes, limits),
        Format::Bmp => bmp::header(bytes, limits),
        Format::Heif => heif::header(bytes, limits),
        Format::Avif => Err(Error::Unsupported(Format::Avif)),
        Format::JpegXl => jxl::header(bytes, limits),
        Format::Psd => psd::header(bytes, limits),
        Format::Raw(kind) => raw::header(kind, bytes, limits),
    }
}

/// The `decode` function of `format`'s module.
pub fn decoder(format: Format) -> DecodeFn {
    match format {
        Format::Jpeg => jpeg::decode,
        Format::Png => png::decode,
        Format::Tiff => tiff::decode,
        Format::WebP => webp::decode,
        Format::Gif => gif::decode,
        Format::Bmp => bmp::decode,
        Format::Heif => heif::decode,
        Format::Avif => |_, _, _, _| Err(Error::Unsupported(Format::Avif)),
        Format::JpegXl => jxl::decode,
        Format::Psd => psd::decode,
        Format::Raw(kind) => raw::decoder(kind),
    }
}

/// The embedded thumbnail of `bytes` read as `format`, as a byte range of `bytes`.
pub fn embedded_thumbnail(format: Format, bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    match format {
        Format::Jpeg => jpeg::embedded_thumbnail(bytes, limits),
        Format::Png => png::embedded_thumbnail(bytes, limits),
        Format::Tiff => tiff::embedded_thumbnail(bytes, limits),
        Format::WebP => webp::embedded_thumbnail(bytes, limits),
        Format::Gif => gif::embedded_thumbnail(bytes, limits),
        Format::Bmp => bmp::embedded_thumbnail(bytes, limits),
        Format::Heif => heif::embedded_thumbnail(bytes, limits),
        Format::Avif => Ok(None),
        Format::JpegXl => jxl::embedded_thumbnail(bytes, limits),
        Format::Psd => psd::embedded_thumbnail(bytes, limits),
        Format::Raw(kind) => raw::embedded_thumbnail(kind, bytes, limits),
    }
}

/// Whether this build decodes `format`.
pub fn decodes(format: Format) -> bool {
    match format {
        Format::Jpeg => jpeg::DECODES,
        Format::Png => png::DECODES,
        Format::Tiff => tiff::DECODES,
        Format::WebP => webp::DECODES,
        Format::Gif => gif::DECODES,
        Format::Bmp => bmp::DECODES,
        Format::Heif => heif::DECODES,
        Format::Avif => false,
        Format::JpegXl => jxl::DECODES,
        Format::Psd => psd::DECODES,
        Format::Raw(kind) => raw::decodes(kind),
    }
}

/// The tile text for a file of `format` that this build does not show.
pub fn reason(format: Format) -> &'static str {
    match format {
        Format::Jpeg => jpeg::REASON,
        Format::Png => png::REASON,
        Format::Tiff => tiff::REASON,
        Format::WebP => webp::REASON,
        Format::Gif => gif::REASON,
        Format::Bmp => bmp::REASON,
        Format::Heif => heif::REASON,
        Format::Avif => AVIF_REASON,
        Format::JpegXl => jxl::REASON,
        Format::Psd => psd::REASON,
        Format::Raw(kind) => raw::reason(kind),
    }
}

/// What the user can do about a file of `format` this build does not show, drawn under its tile
/// text: only HEIC without the `heif` feature has one (`formats.md` §3).
pub fn hint(format: Format) -> Option<&'static str> {
    match format {
        Format::Heif if !heif::DECODES => Some(heif::HINT),
        _ => None,
    }
}

/// The hint of the format whose tile text is `reason` ([`reason`], [`hint`]): what the UI holds
/// of a failure is its text.
pub fn hint_for(reason: &str) -> Option<&'static str> {
    Format::ALL.into_iter().find(|&f| self::reason(f) == reason).and_then(hint)
}

/// A big-endian `u16` at `at`, or `Err(Truncated)`.
pub(crate) fn u16_be(b: &[u8], at: usize) -> Result<u16> {
    match b.get(at..at.saturating_add(2)) {
        Some(&[hi, lo]) => Ok(u16::from_be_bytes([hi, lo])),
        _ => Err(Error::Truncated),
    }
}

/// A big-endian `u32` at `at`, or `Err(Truncated)`.
pub(crate) fn u32_be(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(4)) {
        Some(&[a, b2, c, d]) => Ok(u32::from_be_bytes([a, b2, c, d])),
        _ => Err(Error::Truncated),
    }
}
