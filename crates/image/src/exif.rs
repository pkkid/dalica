//! Reading the EXIF fields Dalica needs from a TIFF structure (an EXIF APP1 payload, a PNG `eXIf`
//! chunk, a TIFF file or a TIFF-structured raw): orientation, dates, camera make and model, the
//! colour space, and the embedded thumbnail of IFD1, whether it is a JPEG or uncompressed TIFF
//! strips. Strict: a loop, an offset outside the block or a count past the limits anywhere on the
//! way is an error, except in the parts read since M5.9, which never make a readable block fail:
//! the Interoperability IFD (it only refines the colour space) and strip thumbnails (a broken one
//! is left out).
//!
//! Provenance: written for Dalica from the public EXIF 2.32 specification (IFD0, the EXIF and
//! Interoperability IFDs, the IFD1 thumbnail as JPEG or as uncompressed strips, §4.5.4 and §4.6)
//! and the TIFF 6.0 specification (strip layout, §3–§6); DCF 2.0 for the "R03" option file
//! (Adobe RGB) colour space. No other reader's code was consulted.

use std::ops::Range;

use dalica_core::DateTime;

use crate::color;
use crate::decode::{ColorModel, DecodeOptions, Decoded, Samples};
use crate::ifd::{Ifd, Tiff, Visited, tags};
use crate::limits::{Budget, Limits};
use crate::{Error, Orientation, Result, Rgba8};

pub const TAG_MAKE: u16 = 0x010f;
pub const TAG_MODEL: u16 = 0x0110;
pub const TAG_ORIENTATION: u16 = 0x0112;
pub const TAG_EXIF_IFD: u16 = tags::EXIF_IFD;
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
pub const TAG_DATE_TIME_DIGITIZED: u16 = 0x9004;
pub const TAG_COLOR_SPACE: u16 = 0xa001;
pub const TAG_INTEROP_INDEX: u16 = 0x0001;
pub const TAG_THUMB_OFFSET: u16 = 0x0201;
pub const TAG_THUMB_LENGTH: u16 = 0x0202;
pub const TAG_IMAGE_WIDTH: u16 = 0x0100;
pub const TAG_IMAGE_LENGTH: u16 = 0x0101;
pub const TAG_BITS_PER_SAMPLE: u16 = 0x0102;
pub const TAG_COMPRESSION: u16 = 0x0103;
pub const TAG_PHOTOMETRIC: u16 = 0x0106;
pub const TAG_STRIP_OFFSETS: u16 = 0x0111;
pub const TAG_SAMPLES_PER_PIXEL: u16 = 0x0115;
pub const TAG_STRIP_BYTE_COUNTS: u16 = 0x0117;
pub const TAG_PLANAR_CONFIGURATION: u16 = 0x011c;

/// The colour space EXIF declares for the image (EXIF 2.32 §4.6.5 ColorSpace; DCF 2.0 option
/// files are "uncalibrated" with the interoperability index "R03").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    AdobeRgb,
    /// Uncalibrated, without a known index.
    Uncalibrated,
}

/// An uncompressed thumbnail stored as TIFF strips (EXIF 2.32 §4.5.4): 8-bit chunky RGB or grey.
/// The strips are byte ranges of the TIFF block it was read from, holding exactly
/// `width × height × channels` bytes in order (a last strip's padding left out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripThumbnail {
    pub width: u32,
    pub height: u32,
    /// [`ColorModel::Rgb`] or [`ColorModel::Gray`].
    pub model: ColorModel,
    pub strips: Vec<Range<usize>>,
}

impl StripThumbnail {
    /// The same thumbnail with its ranges moved by `at` (the block's position in a file);
    /// `None` if a range would overflow.
    pub fn offset_by(&self, at: usize) -> Option<StripThumbnail> {
        let strips = self.strips.iter().map(|r| Some(r.start.checked_add(at)?..r.end.checked_add(at)?)).collect::<Option<Vec<_>>>()?;
        Some(StripThumbnail { strips, ..self.clone() })
    }
}

/// The EXIF fields of one image. Absent or unreadable fields are `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exif {
    /// 1–8; values outside that range are dropped.
    pub orientation: Option<u8>,
    pub date_time_original: Option<DateTime>,
    pub date_time_digitized: Option<DateTime>,
    pub make: Option<String>,
    pub model: Option<String>,
    /// The ColorSpace tag of the EXIF IFD, refined by the interoperability index.
    pub color_space: Option<ColorSpace>,
    /// The embedded JPEG thumbnail, as a byte range of the TIFF block.
    pub thumbnail: Option<Range<usize>>,
    /// The embedded thumbnail when it is stored as uncompressed strips instead: ranges of the
    /// TIFF block here, and of the file in [`crate::Header::exif`] (the thumbnail fast path of
    /// `decode_thumbnail` reads them from there).
    pub strip_thumbnail: Option<StripThumbnail>,
}

/// Reads [`Exif`] from a TIFF block.
pub fn read(block: &[u8], limits: &Limits) -> Result<Exif> {
    read_tiff(&Tiff::new(block)?, limits)
}

/// Reads [`Exif`] from a TIFF structure already opened (a raw's variant header included).
pub fn read_tiff(tiff: &Tiff, limits: &Limits) -> Result<Exif> {
    let mut visited = Visited::default();
    let ifd0 = tiff.ifd(tiff.first_ifd()?, &mut visited, limits)?;
    let mut out = Exif {
        orientation: ifd0.get(TAG_ORIENTATION).and_then(|e| tiff.uint(e)).and_then(|o| u8::try_from(o).ok()).filter(|o| (1..=8).contains(o)),
        make: ifd0.get(TAG_MAKE).and_then(|e| tiff.ascii(e, limits.max_string)).filter(|s| !s.trim().is_empty()),
        model: ifd0.get(TAG_MODEL).and_then(|e| tiff.ascii(e, limits.max_string)).filter(|s| !s.trim().is_empty()),
        ..Exif::default()
    };
    if let Some(ptr) = ifd0.get(TAG_EXIF_IFD).and_then(|e| tiff.uint(e)) {
        if limits.max_ifd_depth < 2 {
            return Err(Error::TooMany("IFD depth"));
        }
        let sub = tiff.ifd(ptr, &mut visited, limits)?;
        let date = |tag| sub.get(tag).and_then(|e| tiff.ascii(e, 64)).and_then(|s| s.trim().parse::<DateTime>().ok());
        out.date_time_original = date(TAG_DATE_TIME_ORIGINAL);
        out.date_time_digitized = date(TAG_DATE_TIME_DIGITIZED);
        out.color_space = color_space(tiff, &sub, &mut visited, limits);
    }
    if ifd0.next != 0 {
        let ifd1 = tiff.ifd(ifd0.next, &mut visited, limits)?;
        let offset = ifd1.get(TAG_THUMB_OFFSET).and_then(|e| tiff.uint(e));
        let length = ifd1.get(TAG_THUMB_LENGTH).and_then(|e| tiff.uint(e));
        if let (Some(offset), Some(length)) = (offset, length) {
            if u64::from(length) > limits.max_thumbnail {
                return Err(Error::TooMany("thumbnail bytes"));
            }
            let bytes = tiff.bytes(u64::from(offset), u64::from(length))?;
            // Only a JPEG counts here; strips are read below.
            if bytes.starts_with(b"\xff\xd8") {
                let start = offset as usize;
                out.thumbnail = Some(start..start + length as usize);
            }
        } else {
            out.strip_thumbnail = strip_image(tiff, &ifd1, limits).unwrap_or_else(|e| {
                log::debug!("EXIF: the strip thumbnail is left out: {e}");
                None
            });
        }
        // Further IFDs are not needed, but a loop or bad offset among them is still an error.
        let mut next = ifd1.next;
        while next != 0 {
            next = tiff.ifd(next, &mut visited, limits)?.next;
        }
    }
    Ok(out)
}

/// The colour space of the EXIF IFD `sub`. The Interoperability IFD is only looked at for an
/// uncalibrated image, and trouble there leaves the space uncalibrated.
fn color_space(tiff: &Tiff, sub: &Ifd, visited: &mut Visited, limits: &Limits) -> Option<ColorSpace> {
    let space = match sub.get(TAG_COLOR_SPACE).and_then(|e| tiff.uint(e))? {
        1 => ColorSpace::Srgb,
        // Not in the EXIF standard, but written by some cameras for Adobe RGB.
        2 => ColorSpace::AdobeRgb,
        0xffff => ColorSpace::Uncalibrated,
        _ => return None,
    };
    if space != ColorSpace::Uncalibrated || limits.max_ifd_depth < 3 {
        return Some(space);
    }
    let index = sub
        .get(tags::INTEROP_IFD)
        .and_then(|e| tiff.uint(e))
        .and_then(|ptr| tiff.ifd(ptr, visited, limits).ok())
        .and_then(|interop| interop.get(TAG_INTEROP_INDEX).and_then(|e| tiff.ascii(e, 8)));
    Some(if index.as_deref() == Some("R03") { ColorSpace::AdobeRgb } else { space })
}

/// An IFD's image as a [`StripThumbnail`], if it is uncompressed 8-bit chunky RGB or grey
/// (BlackIsZero) in strips; `Ok(None)` for any other layout. Strips outside the data, too few
/// bytes for the declared size, or a size past the thumbnail limit are errors.
pub fn strip_image(tiff: &Tiff, ifd: &Ifd, limits: &Limits) -> Result<Option<StripThumbnail>> {
    let uint = |tag| ifd.get(tag).and_then(|e| tiff.uint(e));
    let (Some(offsets), Some(counts)) = (ifd.get(TAG_STRIP_OFFSETS), ifd.get(TAG_STRIP_BYTE_COUNTS)) else { return Ok(None) };
    let (Some(width), Some(height)) = (uint(TAG_IMAGE_WIDTH), uint(TAG_IMAGE_LENGTH)) else { return Ok(None) };
    let spp = uint(TAG_SAMPLES_PER_PIXEL).unwrap_or(1);
    let model = match (uint(TAG_PHOTOMETRIC), spp) {
        (Some(1), 1) => ColorModel::Gray,
        (Some(2), 3) => ColorModel::Rgb,
        _ => return Ok(None),
    };
    let bits = match ifd.get(TAG_BITS_PER_SAMPLE) {
        Some(e) => tiff.uints(e, 8, "bits per sample")?,
        None => return Ok(None),
    };
    let compression = uint(TAG_COMPRESSION).unwrap_or(1);
    let planar = uint(TAG_PLANAR_CONFIGURATION).unwrap_or(1);
    if compression != 1 || planar != 1 || bits.is_empty() || bits.iter().any(|&b| b != 8) {
        return Ok(None);
    }
    let need = limits.check_size(width, height, u64::from(spp))?;
    if need as u64 > limits.max_thumbnail {
        return Err(Error::TooMany("thumbnail bytes"));
    }
    let offsets = tiff.uints(offsets, limits.max_ifd_entries, "strips")?;
    let counts = tiff.uints(counts, limits.max_ifd_entries, "strips")?;
    if offsets.len() != counts.len() {
        return Err(Error::Malformed(format!("{} strip offsets, {} byte counts", offsets.len(), counts.len())));
    }
    let mut strips = Vec::new();
    let mut left = need;
    for (&at, &len) in offsets.iter().zip(&counts) {
        if left == 0 {
            break;
        }
        let take = (len as usize).min(left);
        tiff.bytes(u64::from(at), take as u64)?;
        // `bytes` succeeded, so the range fits in `usize`.
        strips.push(at as usize..at as usize + take);
        left -= take;
    }
    if left > 0 {
        return Err(Error::Malformed(format!("the strips hold {} of {need} thumbnail bytes", need - left)));
    }
    Ok(Some(StripThumbnail { width, height, model, strips }))
}

/// The pixels of a strip thumbnail of `file` (whose ranges it holds), as stored, every buffer
/// charged to `budget`.
pub fn decode_strip(file: &[u8], thumb: &StripThumbnail, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let len = limits.check_size(thumb.width, thumb.height, thumb.model.channels() as u64)?;
    budget.charge(len as u64)?;
    let mut data = Vec::new();
    data.try_reserve_exact(len).map_err(|_| Error::TooLarge { width: thumb.width, height: thumb.height })?;
    for r in &thumb.strips {
        data.extend_from_slice(file.get(r.clone()).ok_or(Error::Truncated)?);
        if data.len() > len {
            return Err(Error::Malformed("the strips hold more than the thumbnail".into()));
        }
    }
    if data.len() != len {
        return Err(Error::Truncated);
    }
    Ok(Decoded {
        width: thumb.width,
        height: thumb.height,
        model: thumb.model,
        alpha: false,
        samples: Samples::U8(data),
        icc: None,
        orientation: Orientation::UPRIGHT,
    })
}

/// A strip thumbnail of `file` as 8-bit sRGB RGBA, in its stored orientation (the fast path when
/// the camera stored its thumbnail uncompressed).
pub fn strip_thumbnail_rgba(file: &[u8], thumb: &StripThumbnail, options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    let mut budget = Budget::new(limits, thumb.width, thumb.height);
    let decoded = decode_strip(file, thumb, &mut budget, limits)?;
    Ok(color::to_srgb8(decoded, options, &mut budget, limits)?.image)
}

#[cfg(test)]
mod tests {
    use dalica_scenes::exif::{ByteOrder, Exif as SceneExif, tiff as scene_tiff};

    use super::*;

    fn block(o: u8, order: ByteOrder, thumb: Option<&[u8]>) -> Vec<u8> {
        let exif = SceneExif {
            orientation: o,
            date_time_original: "2026-06-04T19:42:10".parse().ok(),
            make: Some("Dalica".into()),
            model: Some("Scenes 1".into()),
            thumbnail: None,
            byte_order: order,
        };
        scene_tiff(&exif, 64, 48, thumb).unwrap()
    }

    #[test]
    fn reads_every_field_in_both_byte_orders() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            for o in 1..=8 {
                let b = block(o, order, Some(b"\xff\xd8thumb\xff\xd9"));
                let e = read(&b, &Limits::default()).unwrap();
                assert_eq!(e.orientation, Some(o));
                assert_eq!(e.date_time_original.unwrap().to_string(), "2026-06-04T19:42:10");
                assert_eq!((e.make.as_deref(), e.model.as_deref()), (Some("Dalica"), Some("Scenes 1")));
                assert_eq!(&b[e.thumbnail.unwrap()], b"\xff\xd8thumb\xff\xd9");
            }
        }
    }

    /// Patches the u32 at `at` (little-endian blocks only).
    fn patch(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Offset of IFD0's next-IFD pointer, and of the EXIF IFD pointer's value, in a little-endian block.
    fn pointers(b: &[u8]) -> (usize, usize) {
        let n = u16::from_le_bytes([b[8], b[9]]) as usize;
        let exif_entry = (0..n).map(|k| 10 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_EXIF_IFD).unwrap();
        (10 + 12 * n, exif_entry + 8)
    }

    #[test]
    fn looping_ifds_are_an_error() {
        let base = block(6, ByteOrder::Little, Some(b"\xff\xd8x\xff\xd9"));
        let (next_at, exif_at) = pointers(&base);
        // IFD0's next pointer leads back to IFD0.
        let mut b = base.clone();
        patch(&mut b, next_at, 8);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
        // The EXIF IFD pointer leads back to IFD0.
        let mut b = base.clone();
        patch(&mut b, exif_at, 8);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
        // IFD1's next pointer leads to IFD1 itself.
        let mut b = base.clone();
        let ifd1 = u32::from_le_bytes(b[next_at..next_at + 4].try_into().unwrap()) as usize;
        let n1 = u16::from_le_bytes([b[ifd1], b[ifd1 + 1]]) as usize;
        patch(&mut b, ifd1 + 2 + 12 * n1, ifd1 as u32);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
    }

    #[test]
    fn out_of_range_offsets_are_an_error() {
        let base = block(3, ByteOrder::Little, Some(b"\xff\xd8x\xff\xd9"));
        let (next_at, exif_at) = pointers(&base);
        for (at, v) in [(4, u32::MAX), (4, base.len() as u32), (4, 2), (next_at, 0xffff_fff0), (exif_at, base.len() as u32 + 100)] {
            let mut b = base.clone();
            patch(&mut b, at, v);
            assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)), "pointer at {at} = {v}");
        }
        // A thumbnail whose length runs past the block.
        let mut b = base.clone();
        let ifd1 = u32::from_le_bytes(b[next_at..next_at + 4].try_into().unwrap()) as usize;
        let n1 = u16::from_le_bytes([b[ifd1], b[ifd1 + 1]]) as usize;
        let len_entry = (0..n1).map(|k| ifd1 + 2 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_THUMB_LENGTH).unwrap();
        patch(&mut b, len_entry + 8, 1_000_000);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)));
        // An entry count that cannot fit.
        let mut b = base.clone();
        b[8..10].copy_from_slice(&5000u16.to_le_bytes());
        assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)));
        b[8..10].copy_from_slice(&9000u16.to_le_bytes());
        assert!(matches!(read(&b, &Limits::default()), Err(Error::TooMany(_))));
    }

    #[test]
    fn caps_and_odd_values() {
        let b = block(8, ByteOrder::Big, None);
        assert!(matches!(read(&b, &Limits { max_ifds: 1, ..Limits::default() }), Err(Error::TooMany(_))));
        assert!(matches!(read(&b, &Limits { max_ifd_depth: 1, ..Limits::default() }), Err(Error::TooMany(_))));
        assert!(read(b"II*\0", &Limits::default()).is_err());
        assert!(read(b"XX*\0\x08\0\0\0", &Limits::default()).is_err());
        // An orientation of 0 or 9 is dropped, not an error.
        for bad in [0u16, 9, 300] {
            let mut b = block(1, ByteOrder::Little, None);
            let n = u16::from_le_bytes([b[8], b[9]]) as usize;
            let e = (0..n).map(|k| 10 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_ORIENTATION).unwrap();
            b[e + 8..e + 10].copy_from_slice(&bad.to_le_bytes());
            assert_eq!(read(&b, &Limits::default()).unwrap().orientation, None);
        }
    }
}
