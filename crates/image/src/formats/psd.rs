//! PSD and PSB (Photoshop documents): the merged composite only, through our own reader.
//!
//! Provenance: written from Adobe's public *Adobe Photoshop File Formats Specification* (its prose
//! description of the file header, the colour mode data, the image resources with their thumbnail
//! (1036) and ICC profile (1039) blocks, the length of the layer and mask information, the image
//! data section, and the Large Document Format's wider fields), and from Apple's public Technical
//! Note TN1023 for the PackBits run-length scheme. No other implementation's code was used.
//!
//! - **Read:** the 26-byte header (PSD, version 1, up to 30 000 pixels a side; PSB, version 2, up
//!   to 300 000), the colour mode data (the palette of indexed documents), the image resources, and
//!   the merged composite: planar, raw or PackBits rows (row byte counts are 16-bit in PSD and
//!   32-bit in PSB); 8, 16 and 32-bit (IEEE float) samples, big-endian; grey (and duotone, whose
//!   composite is grey), RGB, CMYK (stored inverted: the maximum means no ink) and indexed; 1-bit
//!   bitmap documents (1 = black).
//! - **Never read:** layers. Only the layer section's length is read, to step over it.
//! - **Refused as unsupported:** ZIP-compressed composites, Lab and multichannel documents.
//! - Extra channels after the colour channels (saved selections, spot colours) are not shown: the
//!   composite is opaque.
//!
//! Every length and count comes from the file and is checked before it is used: sections must lie
//! inside the data, the image resources are walked with a step counter, the composite's size is
//! checked against the limits before anything is allocated, and every buffer is charged to the
//! decode's [`Budget`].

use std::ops::Range;

use super::{u16_be, u32_be};
use crate::decode::{ColorModel, Decoded, Samples};
use crate::limits::{Budget, Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes PSD files.
pub const DECODES: bool = true;

/// The tile text for a PSD this build does not show (ZIP-compressed composites, Lab and
/// multichannel documents).
pub const REASON: &str = "This kind of PSD isn't supported";

/// The image resource holding a JPEG thumbnail (RGB order; the older 1033 is BGR and not used).
const RESOURCE_THUMBNAIL: u16 = 1036;
/// The image resource holding the ICC profile.
const RESOURCE_ICC: u16 = 1039;
/// The thumbnail resource's fixed header before the JPEG data.
const THUMBNAIL_HEADER: usize = 28;
/// Most channels a document may have.
const MAX_CHANNELS: u16 = 56;

/// How the document describes colour (the header's colour mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Bitmap,
    Grey,
    Indexed,
    Rgb,
    Cmyk,
    /// Lab or multichannel: listed, never decoded.
    Unsupported,
}

impl Mode {
    /// Colour channels in the composite.
    fn channels(self) -> usize {
        match self {
            Mode::Bitmap | Mode::Grey | Mode::Indexed | Mode::Unsupported => 1,
            Mode::Rgb => 3,
            Mode::Cmyk => 4,
        }
    }
}

/// What the sections before the layers say.
#[derive(Clone, Debug)]
struct Doc {
    /// PSB (version 2): wider length fields in the layer section and the RLE table.
    psb: bool,
    channels: usize,
    width: u32,
    height: u32,
    /// Bits per sample: 1, 8, 16 or 32.
    depth: u16,
    mode: Mode,
    /// The colour mode data (the palette of an indexed document).
    color_data: Range<usize>,
    /// The ICC profile resource's data.
    icc: Option<Range<usize>>,
    /// The JPEG data of the thumbnail resource.
    thumbnail: Option<Range<usize>>,
    /// Where the layer and mask information section starts.
    layers_at: usize,
}

/// `len` bytes at `at`, as a range checked to lie inside `b`.
fn span(b: &[u8], at: usize, len: u64) -> Result<Range<usize>> {
    let len = usize::try_from(len).map_err(|_| Error::OutOfRange)?;
    let end = at.checked_add(len).ok_or(Error::OutOfRange)?;
    if end > b.len() {
        return Err(Error::Truncated);
    }
    Ok(at..end)
}

/// Reads the header, the colour mode data and the image resources.
fn read_doc(b: &[u8], limits: &Limits) -> Result<Doc> {
    if b.get(..4) != Some(b"8BPS".as_slice()) {
        return Err(Error::Malformed("not a Photoshop document".into()));
    }
    let psb = match u16_be(b, 4)? {
        1 => false,
        2 => true,
        v => return Err(Error::Malformed(format!("PSD version {v}"))),
    };
    let channels = u16_be(b, 12)?;
    let (height, width) = (u32_be(b, 14)?, u32_be(b, 18)?);
    let (depth, mode) = (u16_be(b, 22)?, u16_be(b, 24)?);
    let max_side = if psb { 300_000 } else { 30_000 };
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(Error::Malformed(format!("{channels} channels")));
    }
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return Err(Error::Malformed(format!("document size {width} × {height}")));
    }
    let mode = match mode {
        0 => Mode::Bitmap,
        1 | 8 => Mode::Grey, // duotone composites are stored as grey
        2 => Mode::Indexed,
        3 => Mode::Rgb,
        4 => Mode::Cmyk,
        7 | 9 => Mode::Unsupported,
        m => return Err(Error::Malformed(format!("colour mode {m}"))),
    };
    let depth_fits = match mode {
        Mode::Bitmap => depth == 1,
        Mode::Indexed => depth == 8,
        _ => matches!(depth, 8 | 16 | 32),
    };
    if !depth_fits {
        return Err(Error::Malformed(format!("{depth} bits in colour mode {mode:?}")));
    }
    if usize::from(channels) < mode.channels() {
        return Err(Error::Malformed(format!("{channels} channels for {mode:?}")));
    }

    let color_data = span(b, 30, u64::from(u32_be(b, 26)?))?;
    let resources_len = u32_be(b, color_data.end)?;
    let resources = span(b, color_data.end + 4, u64::from(resources_len))?;
    let (mut icc, mut thumbnail) = (None, None);
    let mut at = resources.start;
    let mut work = Work::new(limits.max_segments, "PSD image resources");
    while at < resources.end {
        work.step()?;
        // A signature, an id, a Pascal name padded to an even length, a data length, and the data
        // padded to an even length.
        let block = b.get(at..resources.end).ok_or(Error::OutOfRange)?;
        let id = u16_be(block, 4)?;
        let name_len = usize::from(*block.get(6).ok_or(Error::Truncated)?);
        let size_at = 6 + (name_len + 2) / 2 * 2;
        let data = span(block, size_at + 4, u64::from(u32_be(block, size_at)?))?;
        if block.get(..4) == Some(b"8BIM".as_slice()) {
            let in_file = at + data.start..at + data.end;
            match id {
                RESOURCE_ICC => icc = Some(in_file),
                RESOURCE_THUMBNAIL => thumbnail = thumbnail_jpeg(b, in_file, limits),
                _ => {}
            }
        }
        at = at.checked_add(data.end + data.len() % 2).ok_or(Error::OutOfRange)?;
    }
    Ok(Doc { psb, channels: usize::from(channels), width, height, depth, mode, color_data, icc, thumbnail, layers_at: resources.end })
}

/// The JPEG data inside a thumbnail resource, if it holds one (format 1, "JPEG RGB") within the
/// thumbnail size limit.
fn thumbnail_jpeg(b: &[u8], data: Range<usize>, limits: &Limits) -> Option<Range<usize>> {
    let d = b.get(data.clone())?;
    if u32_be(d, 0).ok()? != 1 || d.len() <= THUMBNAIL_HEADER || (d.len() - THUMBNAIL_HEADER) as u64 > limits.max_thumbnail {
        return None;
    }
    let jpeg = data.start + THUMBNAIL_HEADER..data.end;
    (b.get(jpeg.start..jpeg.start + 2)? == [0xff, 0xd8]).then_some(jpeg)
}

/// The size (documents are stored upright, without orientation) and the thumbnail resource.
pub fn header(b: &[u8], limits: &Limits) -> Result<Header> {
    let doc = read_doc(b, limits)?;
    Ok(Header {
        format: Format::Psd,
        width: doc.width,
        height: doc.height,
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: doc.thumbnail,
    })
}

/// The JPEG of the thumbnail resource (1036), if the document has one.
pub fn embedded_thumbnail(b: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(read_doc(b, limits)?.thumbnail)
}

/// The output samples while rows are being written.
enum Out {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

pub fn decode(b: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let doc = read_doc(b, limits)?;
    if (doc.width, doc.height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("document {} × {}, header says {} × {}", doc.width, doc.height, header.width, header.height)));
    }
    let (model, out_channels) = match doc.mode {
        Mode::Grey | Mode::Bitmap => (ColorModel::Gray, 1),
        Mode::Rgb | Mode::Indexed => (ColorModel::Rgb, 3),
        Mode::Cmyk => (ColorModel::Cmyk, 4),
        Mode::Unsupported => return Err(Error::Unsupported(Format::Psd)),
    };
    // Step over the layer and mask information (never parsed): a 4-byte length, 8 in PSB.
    let layers_len = if doc.psb {
        (u64::from(u32_be(b, doc.layers_at)?) << 32) | u64::from(u32_be(b, doc.layers_at + 4)?)
    } else {
        u64::from(u32_be(b, doc.layers_at)?)
    };
    let image_at = span(b, doc.layers_at + if doc.psb { 8 } else { 4 }, layers_len)?.end;
    let rle = match u16_be(b, image_at)? {
        0 => false,
        1 => true,
        2 | 3 => return Err(Error::Unsupported(Format::Psd)), // ZIP, without or with prediction
        c => return Err(Error::Malformed(format!("composite compression {c}"))),
    };

    let (w, h) = (doc.width as usize, doc.height as usize);
    let colour = doc.mode.channels();
    let row_bytes = if doc.mode == Mode::Bitmap { w.div_ceil(8) } else { w * usize::from(doc.depth / 8) };
    let sample_bytes = usize::from(doc.depth.div_ceil(8));
    // Checked against the limits before anything is allocated.
    let len = limits.check_size(doc.width, doc.height, (out_channels * sample_bytes) as u64)? / sample_bytes;
    let palette = match doc.mode {
        Mode::Indexed => Some(
            b.get(doc.color_data.clone())
                .filter(|p| p.len() >= 768)
                .ok_or_else(|| Error::Malformed("an indexed document without a palette".into()))?,
        ),
        _ => None,
    };
    let mut out = match doc.depth {
        16 => Out::U16(budget.vec(len, 0u16)?),
        32 => Out::F32(budget.vec(len, 0f32)?),
        _ => Out::U8(budget.vec(len, 0u8)?),
    };

    let data_at = image_at + 2;
    if rle {
        // One byte count per row of every channel (extra channels too), then the packed rows.
        let count_bytes = if doc.psb { 4 } else { 2 };
        let rows = doc.channels.checked_mul(h).ok_or(Error::OutOfRange)?;
        let table = span(b, data_at, (rows as u64).saturating_mul(count_bytes as u64))?;
        let mut row = budget.vec(row_bytes, 0u8)?;
        let mut at = table.end;
        for c in 0..colour {
            for y in 0..h {
                let entry = table.start + (c * h + y) * count_bytes;
                let n = if doc.psb { u64::from(u32_be(b, entry)?) } else { u64::from(u16_be(b, entry)?) };
                let packed = span(b, at, n)?;
                unpack_bits(b.get(packed.clone()).ok_or(Error::Truncated)?, &mut row)?;
                put_row(&mut out, &doc, palette, out_channels, c, y, &row);
                at = packed.end;
            }
        }
    } else {
        let plane = row_bytes.checked_mul(h).ok_or(Error::OutOfRange)?;
        let planes = span(b, data_at, (plane as u64).saturating_mul(colour as u64))?;
        for (i, row) in b.get(planes).ok_or(Error::Truncated)?.chunks_exact(row_bytes).enumerate() {
            put_row(&mut out, &doc, palette, out_channels, i / h, i % h, row);
        }
    }
    let samples = match out {
        Out::U8(v) => Samples::U8(v),
        Out::U16(v) => Samples::U16(v),
        Out::F32(v) => Samples::F32(v),
    };
    let icc = match doc.icc.and_then(|r| b.get(r)) {
        Some(p) if p.len() <= limits.max_icc => Some(p.to_vec()),
        Some(p) => {
            log::warn!("a PSD colour profile of {} bytes is over the {} byte limit; shown as sRGB", p.len(), limits.max_icc);
            None
        }
        None => None,
    };
    Ok(Decoded { width: doc.width, height: doc.height, model, alpha: false, samples, icc, orientation: Orientation::UPRIGHT })
}

/// Writes row `y` of stored channel `c` into the interleaved output of `channels` per pixel:
/// bitmap bits become grey, palette indices RGB, CMYK is inverted back to ink amounts.
fn put_row(out: &mut Out, doc: &Doc, palette: Option<&[u8]>, channels: usize, c: usize, y: usize, row: &[u8]) {
    let w = doc.width as usize;
    let start = y * w * channels + c;
    let cmyk = doc.mode == Mode::Cmyk;
    match out {
        Out::U8(o) => {
            let Some(o) = o.get_mut(start..) else { return };
            match (doc.mode, palette) {
                (Mode::Bitmap, _) => {
                    for (x, d) in o.iter_mut().step_by(channels).take(w).enumerate() {
                        let bit = row.get(x / 8).map_or(0, |byte| (byte >> (7 - x % 8)) & 1);
                        *d = if bit == 1 { 0 } else { 255 };
                    }
                }
                (Mode::Indexed, Some(p)) => {
                    for (px, &i) in o.chunks_mut(channels).zip(row) {
                        for (k, d) in px.iter_mut().enumerate() {
                            *d = p.get(k * 256 + usize::from(i)).copied().unwrap_or(0);
                        }
                    }
                }
                _ => {
                    for (d, &v) in o.iter_mut().step_by(channels).zip(row) {
                        *d = if cmyk { 255 - v } else { v };
                    }
                }
            }
        }
        Out::U16(o) => {
            let Some(o) = o.get_mut(start..) else { return };
            for (d, &[hi, lo]) in o.iter_mut().step_by(channels).zip(row.as_chunks::<2>().0) {
                let v = u16::from_be_bytes([hi, lo]);
                *d = if cmyk { 65_535 - v } else { v };
            }
        }
        Out::F32(o) => {
            let Some(o) = o.get_mut(start..) else { return };
            for (d, &bytes) in o.iter_mut().step_by(channels).zip(row.as_chunks::<4>().0) {
                let v = f32::from_bits(u32::from_be_bytes(bytes));
                *d = if cmyk { 1.0 - v } else { v };
            }
        }
    }
}

/// Unpacks one PackBits row (TN1023) into `out`, which it must fill exactly. A header byte `n` of
/// 0 to 127 copies the next `n + 1` bytes; −1 to −127 repeats the next byte `1 − n` times; −128 is
/// skipped. Bytes left once the row is full are ignored.
fn unpack_bits(src: &[u8], out: &mut [u8]) -> Result<()> {
    let overrun = || Error::Malformed("a PackBits run passes the end of the row".into());
    let (mut i, mut o) = (0usize, 0usize);
    while o < out.len() {
        let n = *src.get(i).ok_or(Error::Truncated)? as i8;
        i += 1;
        match n {
            0..=127 => {
                let run = n as usize + 1;
                let from = src.get(i..i + run).ok_or(Error::Truncated)?;
                out.get_mut(o..o + run).ok_or_else(overrun)?.copy_from_slice(from);
                i += run;
                o += run;
            }
            -127..=-1 => {
                let run = (1 - i16::from(n)) as usize;
                let v = *src.get(i).ok_or(Error::Truncated)?;
                out.get_mut(o..o + run).ok_or_else(overrun)?.fill(v);
                i += 1;
                o += run;
            }
            -128 => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_unpacks_literals_runs_and_no_ops() {
        let mut out = [0u8; 8];
        unpack_bits(&[2, 1, 2, 3, 0x80, 0xfd, 9, 0, 7], &mut out).unwrap();
        assert_eq!(out, [1, 2, 3, 9, 9, 9, 9, 7]);
        // Bytes after a full row are ignored.
        let mut one = [0u8; 1];
        unpack_bits(&[0, 5, 0xff, 0xff], &mut one).unwrap();
        assert_eq!(one, [5]);
        assert_eq!(unpack_bits(&[], &mut []), Ok(()));
    }

    #[test]
    fn packbits_refuses_short_and_overlong_rows() {
        let mut out = [0u8; 4];
        assert_eq!(unpack_bits(&[2, 1, 2, 3], &mut out), Err(Error::Truncated));
        assert!(matches!(unpack_bits(&[5, 1, 2, 3, 4, 5, 6], &mut out), Err(Error::Malformed(_))));
        assert!(matches!(unpack_bits(&[0x81, 1], &mut out), Err(Error::Malformed(_))));
        assert_eq!(unpack_bits(&[0xfe], &mut out), Err(Error::Truncated));
        assert_eq!(unpack_bits(&[0x80, 0x80], &mut out), Err(Error::Truncated));
    }
}
