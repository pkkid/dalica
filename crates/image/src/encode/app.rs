//! The APP segments a JPEG carries: JFIF, EXIF, XMP and the ICC profile in chunks.
//!
//! Adapted from LightCraft (MIT OR Apache-2.0), `crates/codecs/src/encode.rs` (`jpeg_app_segments`,
//! `xmp_segment`) at 25ef65c8. Changes: no print density (JFIF says 1:1 aspect, no units), errors
//! name the block and its size.

use super::{EncodeError, Metadata};

/// The XMP namespace that starts an APP1 XMP payload (XMP part 3, §1.1.3).
const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// The largest segment payload: the 16-bit length counts itself.
const SEGMENT_MAX: usize = 65_533;
/// The largest EXIF block: one APP1 segment after `Exif\0\0`.
pub const EXIF_MAX: usize = SEGMENT_MAX - 6;
/// The largest XMP packet: one APP1 segment after the namespace (extended XMP is not written).
pub const XMP_MAX: usize = SEGMENT_MAX - XMP_NS.len();
/// The ICC bytes in one APP2 chunk (after `ICC_PROFILE\0`, the chunk number and the count).
const ICC_CHUNK: usize = SEGMENT_MAX - 14;
/// The largest ICC profile: 255 chunks (ICC.1 Annex B.4).
pub const ICC_MAX: usize = ICC_CHUNK * 255;

fn too_big(what: &'static str, len: usize, max: usize) -> EncodeError {
    EncodeError::Segment { format: "JPEG", what, len, max }
}

/// The APP segments (marker byte, payload) for `meta`, in the order they are written: JFIF, EXIF,
/// XMP, then the ICC profile in chunks.
pub(super) fn jpeg_app_segments(meta: &Metadata<'_>) -> Result<Vec<(u8, Vec<u8>)>, EncodeError> {
    // JFIF 1.02, aspect ratio 1:1 (no units), no JFIF thumbnail.
    let mut segs: Vec<(u8, Vec<u8>)> = vec![(0xE0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0".to_vec())];
    if let Some(exif) = meta.exif()? {
        if exif.len() > EXIF_MAX {
            return Err(too_big("EXIF block", exif.len(), EXIF_MAX));
        }
        segs.push((0xE1, [&b"Exif\0\0"[..], exif].concat()));
    }
    if let Some(xmp) = meta.xmp() {
        if xmp.len() > XMP_MAX {
            return Err(too_big("XMP packet", xmp.len(), XMP_MAX));
        }
        segs.push((0xE1, [XMP_NS, xmp.as_bytes()].concat()));
    }
    if let Some(icc) = meta.icc() {
        if icc.len() > ICC_MAX {
            return Err(too_big("ICC profile", icc.len(), ICC_MAX));
        }
        let chunks: Vec<&[u8]> = icc.chunks(ICC_CHUNK).collect();
        let count = u8::try_from(chunks.len()).map_err(|_| too_big("ICC profile", icc.len(), ICC_MAX))?;
        for (number, chunk) in (1..=count).zip(&chunks) {
            segs.push((0xE2, [&b"ICC_PROFILE\0"[..], &[number, count], chunk].concat()));
        }
    }
    Ok(segs)
}
