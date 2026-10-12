//! PNG (ISO/IEC 15948): our own chunk walk for the header and the `eXIf` block; pixels through
//! `png` with palette, low bit depths and `tRNS` expanded, alpha always present, 16-bit samples
//! kept, and the `iCCP` profile attached for the colour stage. `gAMA`/`cHRM` without `iCCP` are
//! not interpreted: such files are taken as sRGB, like untagged ones.

use std::io::Cursor;
use std::ops::Range;

use super::u32_be;
use crate::decode::{ColorModel, Decoded, Samples};
use crate::limits::{Budget, Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes PNG files.
pub const DECODES: bool = true;

/// The tile text for a PNG the decoder refuses.
pub const REASON: &str = "This kind of PNG isn't supported";

/// The IHDR chunk and an `eXIf` chunk before the image data (PNG 1.2, Extensions 1.5).
pub fn header(b: &[u8], limits: &Limits) -> Result<Header> {
    let mut pos = 8;
    let mut size = None;
    let mut exif_block = None;
    let mut work = Work::new(limits.max_segments, "PNG chunks");
    while work.step().is_ok() {
        let len = u32_be(b, pos)? as usize;
        let kind = b.get(pos + 4..pos + 8).ok_or(Error::Truncated)?;
        let data_end = (pos + 8).checked_add(len).ok_or(Error::OutOfRange)?;
        if size.is_none() && kind != b"IHDR" {
            return Err(Error::Malformed("the first chunk is not IHDR".into()));
        }
        match kind {
            b"IHDR" => {
                let data = b.get(pos + 8..data_end).ok_or(Error::Truncated)?;
                let (w, h) = (u32_be(data, 0)?, u32_be(data, 4)?);
                if len != 13 || w == 0 || h == 0 || w > i32::MAX as u32 || h > i32::MAX as u32 {
                    return Err(Error::Malformed(format!("IHDR {w} × {h}, {len} bytes")));
                }
                size = Some((w, h));
            }
            b"eXIf" => {
                let data = b.get(pos + 8..data_end).ok_or(Error::Truncated)?;
                exif_block = Some((data, pos + 8));
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        pos = data_end.checked_add(4).ok_or(Error::OutOfRange)?;
    }
    let (width, height) = size.ok_or(Error::Truncated)?;
    let header = Header { format: Format::Png, width, height, orientation: Orientation::UPRIGHT, exif: None, exif_error: None, thumbnail: None };
    Ok(header.with_exif(exif_block, limits))
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    // The `png` crate charges its own allocations (chunk buffers, the inflated `iCCP`, one row) to
    // this limit; text chunks are skipped (metadata is read elsewhere), so twice the profile limit
    // plus room for a row is plenty and keeps a compressed `iCCP` bomb small.
    let own = (limits.max_icc as u64).saturating_mul(2).saturating_add(16 << 20).min(budget.left());
    budget.charge(own)?;
    let mut d = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: usize::try_from(own).unwrap_or(usize::MAX) });
    d.set_ignore_text_chunk(true);
    // Palettes, low bit depths and `tRNS` expanded, an opaque alpha added where there is none (so
    // an untagged 8-bit PNG is already the RGBA the display wants); 16-bit samples kept.
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = d.read_info().map_err(png_err)?;
    let (width, height) = (reader.info().width, reader.info().height);
    if (width, height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("decoded {width} × {height}, header says {} × {}", header.width, header.height)));
    }
    let icc = reader.info().icc_profile.as_ref().map(|p| p.to_vec());
    let (color, depth) = reader.output_color_type();
    let (model, alpha) = match color {
        png::ColorType::Grayscale => (ColorModel::Gray, false),
        png::ColorType::GrayscaleAlpha => (ColorModel::Gray, true),
        png::ColorType::Rgb => (ColorModel::Rgb, false),
        png::ColorType::Rgba => (ColorModel::Rgb, true),
        png::ColorType::Indexed => return Err(Error::Malformed("PNG palette was not expanded".into())),
    };
    let channels = model.channels() + usize::from(alpha);
    let sixteen = depth == png::BitDepth::Sixteen;
    let len = limits.check_size(width, height, channels as u64 * if sixteen { 2 } else { 1 })?;
    let size = reader.output_buffer_size().ok_or(Error::TooLarge { width, height })?;
    if size != len {
        return Err(Error::Malformed(format!("PNG output of {size} bytes, expected {len}")));
    }
    let mut buf = budget.vec(len, 0u8)?;
    reader.next_frame(&mut buf).map_err(png_err)?;
    let samples = if sixteen {
        let mut wide = budget.vec(len / 2, 0u16)?;
        for (w, &[hi, lo]) in wide.iter_mut().zip(buf.as_chunks::<2>().0) {
            *w = u16::from_be_bytes([hi, lo]);
        }
        drop(buf);
        budget.refund(len as u64);
        Samples::U16(wide)
    } else {
        Samples::U8(buf)
    };
    Ok(Decoded { width, height, model, alpha, samples, icc, orientation: header.orientation })
}

fn png_err(e: png::DecodingError) -> Error {
    match e {
        png::DecodingError::LimitsExceeded => Error::TooLarge { width: 0, height: 0 },
        other => Error::Malformed(format!("PNG: {other}")),
    }
}
