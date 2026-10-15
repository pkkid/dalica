//! JPEG (ITU T.81): our own segment walk for the header, the EXIF block, the ICC profile (APP2
//! chunks, reassembled and validated) and the Adobe APP14 colour transform; pixels through
//! `zune-jpeg`. Grey, YCbCr/RGB, CMYK and YCCK frames come out as [`Decoded`] samples with the
//! profile attached, and the colour stage turns them into sRGB.
//!
//! Colour without a profile: a colour JPEG without an embedded ICC profile whose EXIF declares
//! Adobe RGB (ColorSpace 2, or Uncalibrated with the DCF option-file interoperability index
//! "R03", [`crate::exif::ColorSpace::AdobeRgb`]) is tagged with an Adobe RGB (1998) profile built
//! from its published primaries, as the RAW previews are; anything else untagged is sRGB.
//!
//! CMYK convention: files with an Adobe APP14 segment store CMYK inverted (255 = no ink), the
//! convention of the application that introduced the marker; their samples are inverted back so a
//! [`Decoded`] CMYK sample is always the amount of ink (0 = none), as ICC profiles expect. YCCK
//! frames are YCbCr-coded inverted CMY plus K; they are converted to CMY first.

use std::ops::Range;
use std::sync::OnceLock;

use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use super::u16_be;
use crate::decode::{ColorModel, Decoded, Samples};
use crate::exif::ColorSpace as ExifSpace;
use crate::limits::{Budget, Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes JPEG files.
pub const DECODES: bool = true;

/// The tile text for a JPEG variant the decoder refuses: arithmetic coding, 12-bit samples,
/// lossless and hierarchical frames (none of which `zune-jpeg` 0.5 decodes), 2-component frames.
pub const REASON: &str = "This kind of JPEG isn't supported";

/// The APP2 identifier of an ICC profile chunk (ICC.1 Annex B.4).
const ICC_TAG: &[u8] = b"ICC_PROFILE\0";
/// An ICC chunk's header: the identifier, then the sequence number and the chunk count.
const ICC_HEADER: usize = ICC_TAG.len() + 2;

/// What the segments before the scan data say.
#[derive(Debug, Default)]
struct Segments<'a> {
    /// Frame size and component count, from the first SOF.
    frame: Option<(u16, u16, u8)>,
    /// The first SOF's marker code (the coding process) and sample precision.
    process: Option<(u8, u8)>,
    /// The EXIF TIFF block and its offset in the file.
    exif: Option<(&'a [u8], usize)>,
    /// APP2 ICC chunks: (sequence number, count, data), in file order.
    icc: Vec<(u8, u8, &'a [u8])>,
    /// The Adobe APP14 colour transform (0 = none/CMYK, 1 = YCbCr, 2 = YCCK).
    adobe: Option<u8>,
}

/// Walks the segments from SOI (T.81 §B.1): to the first SOF when `to_scan` is false (the header),
/// to the first SOS otherwise (APP segments may follow the frame header).
fn segments<'a>(b: &'a [u8], limits: &Limits, to_scan: bool) -> Result<Segments<'a>> {
    let mut seg = Segments::default();
    let mut pos = 2;
    let mut work = Work::new(limits.max_segments, "JPEG segments");
    loop {
        work.step()?;
        if b.get(pos) != Some(&0xff) {
            return Err(if pos >= b.len() { Error::Truncated } else { Error::Malformed(format!("no marker at byte {pos}")) });
        }
        // Fill bytes: any number of 0xFF before the marker code.
        while b.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *b.get(pos).ok_or(Error::Truncated)?;
        pos += 1;
        match marker {
            // Markers without a length: TEM, RSTn, SOI.
            0x01 | 0xd0..=0xd8 => continue,
            0xd9 => return Err(Error::Malformed("end of image before a frame header".into())),
            0xda if seg.frame.is_some() => return Ok(seg),
            0xda => return Err(Error::Malformed("scan data before a frame header".into())),
            _ => {}
        }
        let len = usize::from(u16_be(b, pos)?);
        if len < 2 {
            return Err(Error::Malformed(format!("segment length {len}")));
        }
        let data = b.get(pos + 2..pos + len).ok_or(Error::Truncated)?;
        match marker {
            // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC).
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                if seg.frame.is_none() {
                    let (height, width) = (u16_be(data, 1)?, u16_be(data, 3)?);
                    let components = *data.get(5).ok_or(Error::Truncated)?;
                    if width == 0 || height == 0 {
                        return Err(Error::Malformed(format!("frame size {width} × {height}")));
                    }
                    seg.frame = Some((width, height, components));
                    seg.process = Some((marker, *data.first().ok_or(Error::Truncated)?));
                }
                if !to_scan {
                    return Ok(seg);
                }
            }
            0xe1 if seg.exif.is_none() && data.starts_with(b"Exif\0\0") => {
                seg.exif = data.get(6..).map(|t| (t, pos + 2 + 6));
            }
            0xe2 if data.starts_with(ICC_TAG) => {
                if let (Some(&n), Some(&count), Some(chunk)) = (data.get(ICC_TAG.len()), data.get(ICC_TAG.len() + 1), data.get(ICC_HEADER..)) {
                    seg.icc.push((n, count, chunk));
                }
            }
            0xee if seg.adobe.is_none() && data.starts_with(b"Adobe") => seg.adobe = data.get(11).copied(),
            _ => {}
        }
        pos += len;
    }
}

/// Reassembles the APP2 chunks into one profile (ICC.1 Annex B.4): every chunk carries the same
/// count, the sequence numbers are exactly 1..=count with none missing or repeated, and the total
/// is within `max_icc`. Anything else is an unusable profile: `Err` with the reason.
pub fn reassemble_icc(chunks: &[(u8, u8, &[u8])], max_icc: usize) -> Result<Option<Vec<u8>>, String> {
    let Some(&(_, count, _)) = chunks.first() else { return Ok(None) };
    if count == 0 || chunks.len() != usize::from(count) {
        return Err(format!("{} ICC chunks, the first says {count}", chunks.len()));
    }
    let mut ordered: Vec<Option<&[u8]>> = vec![None; usize::from(count)];
    let mut total = 0usize;
    for &(n, c, data) in chunks {
        let slot = usize::from(n).checked_sub(1).and_then(|i| ordered.get_mut(i)).ok_or_else(|| format!("ICC chunk {n} of {count}"))?;
        if c != count || slot.is_some() {
            return Err(format!("ICC chunk {n} of {c} repeats or disagrees with {count}"));
        }
        total = total.saturating_add(data.len());
        if total > max_icc {
            return Err(format!("the ICC profile is over {max_icc} bytes"));
        }
        *slot = Some(data);
    }
    let mut profile = Vec::new();
    profile.try_reserve_exact(total).map_err(|e| e.to_string())?;
    for data in ordered.into_iter().flatten() {
        profile.extend_from_slice(data);
    }
    Ok(Some(profile))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let seg = segments(bytes, limits, false)?;
    let (width, height, _) = seg.frame.ok_or(Error::Truncated)?;
    let header = Header {
        format: Format::Jpeg,
        width: u32::from(width),
        height: u32::from(height),
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: None,
    };
    Ok(header.with_exif(seg.exif, limits))
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let seg = segments(bytes, limits, true)?;
    if let Some((marker, precision)) = seg.process
        && !decodable(marker, precision)
    {
        return Err(Error::Unsupported(Format::Jpeg));
    }
    // An unusable profile is not a damaged photo: it is dropped here, logged, and shown as sRGB.
    let icc = reassemble_icc(&seg.icc, limits.max_icc).unwrap_or_else(|reason| {
        log::warn!("JPEG: ignoring the embedded colour profile: {reason}");
        None
    });
    let side = limits.max_side as usize;
    let options = DecoderOptions::default().set_max_width(side).set_max_height(side).set_strict_mode(false);
    let mut probe = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    probe.decode_headers().map_err(jpeg_err)?;
    // Colour frames come out as RGBA with an opaque alpha channel, the layout the display wants:
    // an untagged JPEG then needs no copy in the colour stage.
    let (model, alpha, out) = match probe.input_colorspace().ok_or(Error::Truncated)? {
        ColorSpace::Luma => (ColorModel::Gray, false, ColorSpace::Luma),
        ColorSpace::YCbCr | ColorSpace::RGB => (ColorModel::Rgb, true, ColorSpace::RGBA),
        ColorSpace::CMYK => (ColorModel::Cmyk, false, ColorSpace::CMYK),
        ColorSpace::YCCK => (ColorModel::Cmyk, false, ColorSpace::YCCK),
        _ => return Err(Error::Unsupported(Format::Jpeg)),
    };
    let mut d = JpegDecoder::new_with_options(ZCursor::new(bytes), options.jpeg_set_out_colorspace(out));
    d.decode_headers().map_err(jpeg_err)?;
    let info = d.info().ok_or(Error::Truncated)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if (width, height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("decoded {width} × {height}, header says {} × {}", header.width, header.height)));
    }
    let len = limits.check_size(width, height, out.num_components() as u64)?;
    let mut data = budget.vec(len, 0u8)?;
    d.decode_into(&mut data).map_err(jpeg_err)?;
    if model == ColorModel::Cmyk {
        to_ink(&mut data, out == ColorSpace::YCCK, seg.adobe.is_some());
    }
    let icc = icc.or_else(|| declared_profile(header, model));
    Ok(Decoded { width, height, model, alpha, samples: Samples::U8(data), icc, orientation: header.orientation })
}

/// The profile EXIF declares for colour pixels that carry none of their own: Adobe RGB (1998)
/// when the header's EXIF says so (DCF 2.0 option files), else `None` (sRGB).
fn declared_profile(header: &Header, model: ColorModel) -> Option<Vec<u8>> {
    let adobe = header.exif.as_ref().and_then(|e| e.color_space) == Some(ExifSpace::AdobeRgb);
    (adobe && model == ColorModel::Rgb).then(adobe_rgb).flatten()
}

/// The Adobe RGB (1998) profile, encoded once by `moxcms` from its published primaries.
fn adobe_rgb() -> Option<Vec<u8>> {
    static ADOBE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    ADOBE
        .get_or_init(|| {
            moxcms::ColorProfile::new_adobe_rgb().encode().map_err(|e| log::warn!("JPEG: the Adobe RGB profile could not be built: {e:?}")).ok()
        })
        .clone()
}

/// Whether `zune-jpeg` decodes a frame of this SOF marker and sample precision: Huffman-coded
/// baseline, extended sequential or progressive DCT at 8 bits. Arithmetic coding (SOF9–11,
/// SOF13–15), lossless (SOF3), hierarchical (SOF5–7) and 12-bit frames are a kind of JPEG Dalica
/// does not decode yet ([`REASON`]), not a damaged file.
fn decodable(marker: u8, precision: u8) -> bool {
    matches!(marker, 0xc0..=0xc2) && precision == 8
}

/// Turns 4-channel JPEG samples into ink amounts: YCCK's YCbCr part back to (inverted) CMY with
/// the JFIF equations (T.871 §7), then Adobe's inversion undone.
fn to_ink(data: &mut [u8], ycck: bool, adobe: bool) {
    for px in data.as_chunks_mut::<4>().0 {
        if ycck {
            let [y, cb, cr, _] = px.map(f32::from);
            let (cb, cr) = (cb - 128.0, cr - 128.0);
            let clamp = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            px[0] = clamp(y + 1.402 * cr);
            px[1] = clamp(y - 0.344_136 * cb - 0.714_136 * cr);
            px[2] = clamp(y + 1.772 * cb);
        }
        if adobe {
            for v in px.iter_mut() {
                *v = 255 - *v;
            }
        }
    }
}

fn jpeg_err(e: zune_jpeg::errors::DecodeErrors) -> Error {
    match e {
        // A coding process the decoder names as unsupported is a kind of JPEG, not damage.
        zune_jpeg::errors::DecodeErrors::Unsupported(_) => Error::Unsupported(Format::Jpeg),
        other => Error::Malformed(format!("JPEG: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icc_chunks_are_reassembled_in_sequence_order() {
        let a: &[u8] = b"abc";
        let b: &[u8] = b"de";
        assert_eq!(reassemble_icc(&[(2, 2, b), (1, 2, a)], 100), Ok(Some(b"abcde".to_vec())));
        assert_eq!(reassemble_icc(&[], 100), Ok(None));
        // Missing, repeated, out-of-range and disagreeing chunks, and an oversized total.
        assert!(reassemble_icc(&[(1, 2, a)], 100).is_err());
        assert!(reassemble_icc(&[(1, 2, a), (1, 2, b)], 100).is_err());
        assert!(reassemble_icc(&[(1, 2, a), (3, 2, b)], 100).is_err());
        assert!(reassemble_icc(&[(0, 1, a)], 100).is_err());
        assert!(reassemble_icc(&[(1, 2, a), (2, 3, b)], 100).is_err());
        assert!(reassemble_icc(&[(1, 0, a)], 100).is_err());
        assert!(reassemble_icc(&[(1, 2, a), (2, 2, b)], 4).is_err());
        assert_eq!(reassemble_icc(&[(1, 2, a), (2, 2, b)], 5), Ok(Some(b"abcde".to_vec())));
    }

    #[test]
    fn only_8_bit_huffman_dct_frames_are_decodable() {
        for marker in [0xc0, 0xc1, 0xc2] {
            assert!(decodable(marker, 8), "{marker:#x}");
            assert!(!decodable(marker, 12), "{marker:#x} at 12 bits");
        }
        for marker in [0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf] {
            assert!(!decodable(marker, 8), "{marker:#x}");
        }
    }

    /// An EXIF block whose EXIF IFD holds ColorSpace `space`, and an interoperability index when
    /// `index` is given (little-endian TIFF, built from EXIF 2.32 §4.6).
    fn exif_color_space(space: u16, index: Option<&[u8; 4]>) -> Vec<u8> {
        let entry = |tag: u16, kind: u16, value: [u8; 4]| [&tag.to_le_bytes()[..], &kind.to_le_bytes(), &1u32.to_le_bytes(), &value].concat();
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        // IFD0 at 8: the EXIF IFD pointer, to 26.
        b.extend(1u16.to_le_bytes());
        b.extend(entry(0x8769, 4, 26u32.to_le_bytes()));
        b.extend([0; 4]);
        // The EXIF IFD at 26: ColorSpace, and the interoperability IFD pointer (to 56).
        let n: u16 = if index.is_some() { 2 } else { 1 };
        b.extend(n.to_le_bytes());
        let [lo, hi] = space.to_le_bytes();
        b.extend(entry(0xa001, 3, [lo, hi, 0, 0]));
        if let Some(index) = index {
            b.extend(entry(0xa005, 4, 56u32.to_le_bytes()));
            b.extend([0; 4]);
            // The interoperability IFD at 56: InteroperabilityIndex, four ASCII bytes inline.
            b.extend(1u16.to_le_bytes());
            b.extend([&0x0001u16.to_le_bytes()[..], &2u16.to_le_bytes(), &4u32.to_le_bytes(), index].concat());
        }
        b.extend([0; 4]);
        b
    }

    /// A colour JPEG without a profile is Adobe RGB when its EXIF says so (ColorSpace 2, or
    /// Uncalibrated with the "R03" index), and sRGB otherwise; an embedded profile always wins.
    #[test]
    fn a_jpeg_marked_adobe_rgb_without_a_profile_decodes_as_adobe_rgb() {
        use crate::encode::{Channels, Image, Metadata, Pixels, Subsampling, jpeg};
        let limits = Limits::default();
        let px: Vec<u8> = [200u8, 100, 50].repeat(64);
        let image = Image { width: 8, height: 8, channels: Channels::Rgb, pixels: Pixels::U8(&px) };
        let file = |exif: Option<&[u8]>, icc: Option<&[u8]>| jpeg(&image, 95, Subsampling::S444, &Metadata { exif, xmp: None, icc }).unwrap();
        let tagged = |bytes: &[u8]| {
            let h = header(bytes, &limits).unwrap();
            decode(bytes, &h, &mut Budget::new(&limits, h.width, h.height), &limits).unwrap().icc
        };
        let adobe = adobe_rgb().unwrap();
        let declared = exif_color_space(2, None);
        let r03 = exif_color_space(0xffff, Some(b"R03\0"));
        assert_eq!(tagged(&file(Some(&declared), None)).as_deref(), Some(&adobe[..]));
        assert_eq!(tagged(&file(Some(&r03), None)).as_deref(), Some(&adobe[..]));
        for srgb in [exif_color_space(1, None), exif_color_space(0xffff, Some(b"R98\0")), exif_color_space(0xffff, None)] {
            assert_eq!(tagged(&file(Some(&srgb), None)), None);
        }
        assert_eq!(tagged(&file(None, None)), None);
        let own = moxcms::ColorProfile::new_srgb().encode().unwrap();
        assert_eq!(tagged(&file(Some(&declared), Some(&own))), Some(own));
        // Through the colour stage: Adobe RGB's wider gamut makes the orange more saturated in sRGB.
        let shown = |bytes: &[u8]| crate::decode(bytes, &limits).unwrap().data[..3].to_vec();
        let (plain, wide) = (shown(&file(None, None)), shown(&file(Some(&declared), None)));
        assert!(wide[0] > plain[0] + 20 && wide[2] + 5 < plain[2], "{plain:?} as Adobe RGB is {wide:?}");
    }

    #[test]
    fn ink_is_the_inverse_of_adobe_samples() {
        let mut px = [255, 0, 128, 10];
        to_ink(&mut px, false, true);
        assert_eq!(px, [0, 255, 127, 245]);
        let mut plain = [1, 2, 3, 4];
        to_ink(&mut plain, false, false);
        assert_eq!(plain, [1, 2, 3, 4]);
        // YCCK grey (Y = 200, no chroma) is inverted CMY of 200 each.
        let mut ycck = [200, 128, 128, 30];
        to_ink(&mut ycck, true, true);
        assert_eq!(ycck, [55, 55, 55, 225]);
    }
}
