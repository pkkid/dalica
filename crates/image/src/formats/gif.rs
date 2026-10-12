//! GIF (GIF87a and GIF89a): our own block walk for the header (the logical screen and the first
//! image descriptor); the first frame's pixels through the pinned `image` decoder (`gif`
//! underneath), placed on the logical screen, transparent where the frame does not reach.
//!
//! Only the first frame is ever decoded, so an animation's frame count costs nothing: neither the
//! walk nor the decoder looks past frame one. The screen and the first frame's rectangle are both
//! checked against the limits and charged to the [`Budget`] before the decoder runs. An ICC
//! profile in an `ICCRGBG1` application extension before the first frame is attached.

use std::io::{Cursor, ErrorKind};
use std::ops::Range;

use image::codecs::gif::GifDecoder;
use image::{ImageDecoder, ImageError};

use crate::decode::{ColorModel, Decoded, Samples};
use crate::limits::{Budget, Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes GIF files.
pub const DECODES: bool = true;

/// The tile text for a GIF the decoder refuses.
pub const REASON: &str = "This kind of GIF isn't supported";

/// The logical screen and where the first frame lies on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Screen {
    width: u32,
    height: u32,
    /// The first image descriptor: left, top, width, height.
    frame: [u32; 4],
}

fn le16(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(2)) {
        Some(&[lo, hi]) => Ok(u32::from(u16::from_le_bytes([lo, hi]))),
        _ => Err(Error::Truncated),
    }
}

/// Skips the data sub-blocks starting at `pos` (each a length byte, then that many bytes, ended
/// by a zero length). Every step moves forward, so the loop ends with the data.
fn skip_sub_blocks(b: &[u8], mut pos: usize) -> Result<usize> {
    loop {
        let n = usize::from(*b.get(pos).ok_or(Error::Truncated)?);
        pos = pos.checked_add(1 + n).ok_or(Error::OutOfRange)?;
        if n == 0 {
            return Ok(pos);
        }
    }
}

/// The logical screen descriptor, then blocks up to the first image descriptor.
fn screen(b: &[u8], limits: &Limits) -> Result<Screen> {
    match b.get(..6) {
        Some(b"GIF87a" | b"GIF89a") => {}
        Some(_) => return Err(Error::Malformed("not a GIF".into())),
        None => return Err(Error::Truncated),
    }
    let (width, height) = (le16(b, 6)?, le16(b, 8)?);
    let packed = *b.get(10).ok_or(Error::Truncated)?;
    if width == 0 || height == 0 {
        return Err(Error::Malformed(format!("GIF screen {width} × {height}")));
    }
    // The global colour table: 3 × 2^(n + 1) bytes.
    let mut pos = 13usize;
    if packed & 0x80 != 0 {
        pos += 3 << ((packed & 7) + 1);
    }
    let mut work = Work::new(limits.max_segments, "GIF blocks");
    loop {
        work.step()?;
        match *b.get(pos).ok_or(Error::Truncated)? {
            // Extension: introducer, label, data sub-blocks.
            0x21 => pos = skip_sub_blocks(b, pos.checked_add(2).ok_or(Error::OutOfRange)?)?,
            0x2c => {
                let frame = [le16(b, pos + 1)?, le16(b, pos + 3)?, le16(b, pos + 5)?, le16(b, pos + 7)?];
                b.get(pos + 9).ok_or(Error::Truncated)?;
                return Ok(Screen { width, height, frame });
            }
            0x3b => return Err(Error::Malformed("a GIF without an image".into())),
            other => return Err(Error::Malformed(format!("GIF block {other:#04x}"))),
        }
    }
}

/// The logical screen's size; GIF has no orientation or EXIF.
pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let s = screen(bytes, limits)?;
    Ok(Header {
        format: Format::Gif,
        width: s.width,
        height: s.height,
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: None,
    })
}

/// GIF carries no embedded thumbnail.
pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let s = screen(bytes, limits)?;
    let (width, height) = (s.width, s.height);
    if (width, height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("GIF screen {width} × {height}, header says {} × {}", header.width, header.height)));
    }
    let len = limits.check_size(width, height, 4)?;
    // A frame that does not span the screen's width inside it is decoded into a buffer of its own
    // first (`image`'s GIF decoder), so its declared size is checked and charged too.
    let [left, top, fw, fh] = s.frame;
    if !(left == 0 && fw == width && top + fh <= height) {
        let frame = limits.check_size(fw, fh, 4)?;
        budget.charge(frame as u64)?;
    }
    let too_large = |e| image_err(e, width, height);
    let mut d = GifDecoder::new(Cursor::new(bytes)).map_err(too_large)?;
    let mut own = image::Limits::default();
    own.max_image_width = Some(limits.max_side);
    own.max_image_height = Some(limits.max_side);
    own.max_alloc = Some(budget.left());
    d.set_limits(own).map_err(too_large)?;
    if d.dimensions() != (width, height) {
        let (w, h) = d.dimensions();
        return Err(Error::Malformed(format!("GIF: the decoder sees {w} × {h}, the header says {width} × {height}")));
    }
    let icc = d.icc_profile().ok().flatten().filter(|p| p.len() <= limits.max_icc);
    let mut buf = budget.vec(len, 0u8)?;
    d.read_image(&mut buf).map_err(too_large)?;
    Ok(Decoded { width, height, model: ColorModel::Rgb, alpha: true, samples: Samples::U8(buf), icc, orientation: Orientation::UPRIGHT })
}

fn image_err(e: ImageError, width: u32, height: u32) -> Error {
    match e {
        ImageError::Limits(_) => Error::TooLarge { width, height },
        ImageError::Unsupported(_) => Error::Unsupported(Format::Gif),
        ImageError::IoError(io) if io.kind() == ErrorKind::UnexpectedEof => Error::Truncated,
        other => Error::Malformed(format!("GIF: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF89a header for a `w × h` screen with a 2-colour global table, then `blocks`.
    fn gif(w: u16, h: u16, blocks: &[u8]) -> Vec<u8> {
        let mut b = b"GIF89a".to_vec();
        b.extend(w.to_le_bytes());
        b.extend(h.to_le_bytes());
        b.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        b.extend(blocks);
        b
    }

    #[test]
    fn the_walk_skips_extensions_to_the_first_frame() {
        let l = Limits::default();
        let descriptor = [0x2c, 1, 0, 2, 0, 3, 0, 4, 0, 0];
        let comment = [0x21, 0xfe, 3, b'a', b'b', b'c', 2, b'd', b'e', 0];
        let s = screen(&gif(10, 20, &[&comment[..], &descriptor].concat()), &l).unwrap();
        assert_eq!(s, Screen { width: 10, height: 20, frame: [1, 2, 3, 4] });
        // Cut inside the comment, or with no image before the trailer.
        assert_eq!(screen(&gif(10, 20, &comment[..5]), &l).unwrap_err(), Error::Truncated);
        assert!(matches!(screen(&gif(10, 20, &[0x3b]), &l), Err(Error::Malformed(_))));
        assert!(matches!(screen(&gif(0, 20, &descriptor), &l), Err(Error::Malformed(_))));
    }

    #[test]
    fn extensions_beyond_the_block_limit_are_refused() {
        let l = Limits { max_segments: 3, ..Limits::default() };
        let empty = [0x21, 0xfe, 0];
        let descriptor = [0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        assert!(screen(&gif(1, 1, &[&empty[..], &empty, &descriptor].concat()), &l).is_ok());
        let three = [&empty[..], &empty, &empty, &descriptor].concat();
        assert_eq!(screen(&gif(1, 1, &three), &l).unwrap_err(), Error::TooMany("GIF blocks"));
    }
}
