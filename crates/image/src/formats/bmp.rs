//! BMP (Windows and OS/2 bitmaps): our own read of the file and DIB headers for the size and a
//! `BITMAPV5HEADER`'s embedded ICC profile; pixels through the pinned `image` decoder: 1–32-bit,
//! palettes, RLE4/RLE8, bit fields (with alpha when a mask says so), bottom-up and top-down.
//! JPEG- or PNG-compressed and CMYK bitmaps answer [`Error::Unsupported`]. A linked profile (a
//! file name inside the bitmap) is never followed.

use std::io::{Cursor, ErrorKind};
use std::ops::Range;

use image::codecs::bmp::BmpDecoder;
use image::{ColorType, ImageDecoder, ImageError};

use crate::decode::{ColorModel, Decoded, Samples};
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes BMP files.
pub const DECODES: bool = true;

/// The tile text for a BMP the decoder refuses.
pub const REASON: &str = "This kind of BMP isn't supported";

/// Where the DIB header starts (after the 14-byte file header).
const DIB: usize = 14;
/// `bV5CSType` of an embedded profile: `'MBED'`.
const PROFILE_EMBEDDED: u32 = 0x4d42_4544;
/// Bytes of a `BITMAPV5HEADER`.
const V5_SIZE: u32 = 124;

/// What the headers say.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dib {
    width: u32,
    height: u32,
    /// An embedded profile's range in the file (`BITMAPV5HEADER` only).
    icc: Option<Range<usize>>,
}

fn le16(b: &[u8], at: usize) -> Result<u16> {
    match b.get(at..at.saturating_add(2)) {
        Some(&[lo, hi]) => Ok(u16::from_le_bytes([lo, hi])),
        _ => Err(Error::Truncated),
    }
}

fn le32(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(4)) {
        Some(&[a, b2, c, d]) => Ok(u32::from_le_bytes([a, b2, c, d])),
        _ => Err(Error::Truncated),
    }
}

fn dib(b: &[u8]) -> Result<Dib> {
    if b.get(..2) != Some(b"BM") {
        return Err(if b.len() < 2 { Error::Truncated } else { Error::Malformed("not a BMP".into()) });
    }
    let size = le32(b, DIB)?;
    let (width, height) = match size {
        // BITMAPCOREHEADER (OS/2 1.x): unsigned 16-bit sizes, always bottom-up.
        12 => (u32::from(le16(b, DIB + 4)?), u32::from(le16(b, DIB + 6)?)),
        // BITMAPINFOHEADER and its successors (and OS/2 2.x): signed 32-bit sizes, a negative
        // height meaning top-down rows.
        40.. => {
            let w = le32(b, DIB + 4)? as i32;
            let h = le32(b, DIB + 8)? as i32;
            if w <= 0 || h == 0 || h == i32::MIN {
                return Err(Error::Malformed(format!("BMP size {w} × {h}")));
            }
            (w.unsigned_abs(), h.unsigned_abs())
        }
        _ => return Err(Error::Malformed(format!("BMP header of {size} bytes"))),
    };
    if width == 0 || height == 0 {
        return Err(Error::Malformed(format!("BMP size {width} × {height}")));
    }
    let mut icc = None;
    if size >= V5_SIZE && le32(b, DIB + 56)? == PROFILE_EMBEDDED {
        // bV5ProfileData counts from the start of the DIB header. A range that cannot exist is
        // no profile, not a damaged bitmap.
        let at = (le32(b, DIB + 112)? as usize).checked_add(DIB);
        let len = le32(b, DIB + 116)? as usize;
        icc = at.and_then(|at| Some(at..at.checked_add(len)?));
    }
    Ok(Dib { width, height, icc })
}

/// The bitmap's size; BMP has no orientation or EXIF.
pub fn header(bytes: &[u8], _limits: &Limits) -> Result<Header> {
    let d = dib(bytes)?;
    Ok(Header {
        format: Format::Bmp,
        width: d.width,
        height: d.height,
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: None,
    })
}

/// BMP carries no embedded thumbnail.
pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let info = dib(bytes)?;
    let (width, height) = (info.width, info.height);
    if (width, height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("BMP {width} × {height}, header says {} × {}", header.width, header.height)));
    }
    let err = |e| image_err(e, width, height);
    let mut d = BmpDecoder::new(Cursor::new(bytes)).map_err(err)?;
    let mut own = image::Limits::default();
    own.max_image_width = Some(limits.max_side);
    own.max_image_height = Some(limits.max_side);
    own.max_alloc = Some(budget.left());
    d.set_limits(own).map_err(err)?;
    if d.dimensions() != (width, height) {
        let (w, h) = d.dimensions();
        return Err(Error::Malformed(format!("BMP: the decoder sees {w} × {h}, the header says {width} × {height}")));
    }
    let alpha = match d.color_type() {
        ColorType::Rgb8 => false,
        ColorType::Rgba8 => true,
        other => return Err(Error::Malformed(format!("BMP decoded as {other:?}"))),
    };
    let len = limits.check_size(width, height, if alpha { 4 } else { 3 })?;
    // The decoder's own buffer: one row of the file (at most 4 bytes per pixel).
    budget.charge(u64::from(width) * 4)?;
    let mut buf = budget.vec(len, 0u8)?;
    d.read_image(&mut buf).map_err(err)?;
    let icc = info.icc.and_then(|r| bytes.get(r)).filter(|p| !p.is_empty() && p.len() <= limits.max_icc).map(<[u8]>::to_vec);
    Ok(Decoded { width, height, model: ColorModel::Rgb, alpha, samples: Samples::U8(buf), icc, orientation: Orientation::UPRIGHT })
}

fn image_err(e: ImageError, width: u32, height: u32) -> Error {
    match e {
        ImageError::Limits(_) => Error::TooLarge { width, height },
        ImageError::Unsupported(_) => Error::Unsupported(Format::Bmp),
        ImageError::IoError(io) if io.kind() == ErrorKind::UnexpectedEof => Error::Truncated,
        other => Error::Malformed(format!("BMP: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dib_header: &[u8]) -> Vec<u8> {
        [b"BM".as_slice(), &[0; 8], &54u32.to_le_bytes(), dib_header].concat()
    }

    fn info(w: i32, h: i32) -> Vec<u8> {
        let mut d = 40u32.to_le_bytes().to_vec();
        d.extend(w.to_le_bytes());
        d.extend(h.to_le_bytes());
        d.extend([1, 0, 24, 0]);
        d.extend([0; 24]);
        d
    }

    #[test]
    fn sizes_from_each_header_kind() {
        assert_eq!(dib(&file(&info(30, 20))).unwrap(), Dib { width: 30, height: 20, icc: None });
        // Top-down rows.
        assert_eq!(dib(&file(&info(30, -20))).unwrap().height, 20);
        let core = [&12u32.to_le_bytes()[..], &[0x10, 0x27, 0x05, 0x00, 1, 0, 24, 0]].concat();
        assert_eq!(dib(&file(&core)).unwrap(), Dib { width: 10_000, height: 5, icc: None });
        for (w, h) in [(0, 5), (-3, 5), (3, 0), (3, i32::MIN)] {
            assert!(matches!(dib(&file(&info(w, h))), Err(Error::Malformed(_))), "{w} × {h}");
        }
        let odd = [&20u32.to_le_bytes()[..], &[0; 16]].concat();
        assert!(matches!(dib(&file(&odd)), Err(Error::Malformed(_))));
        assert_eq!(dib(&file(&info(30, 20))[..20]).unwrap_err(), Error::Truncated);
    }

    #[test]
    fn a_v5_profile_range_is_checked_arithmetic() {
        let mut v5 = info(4, 4);
        v5[..4].copy_from_slice(&V5_SIZE.to_le_bytes());
        v5.resize(124, 0);
        v5[56..60].copy_from_slice(&PROFILE_EMBEDDED.to_le_bytes());
        v5[112..116].copy_from_slice(&200u32.to_le_bytes());
        v5[116..120].copy_from_slice(&10u32.to_le_bytes());
        assert_eq!(dib(&file(&v5)).unwrap().icc, Some(214..224));
        v5[116..120].copy_from_slice(&u32::MAX.to_le_bytes());
        // Wherever it points, the range is computed without overflow and read with `get`.
        assert!(dib(&file(&v5)).is_ok());
    }
}
