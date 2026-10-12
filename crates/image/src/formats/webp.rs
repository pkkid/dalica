//! WebP (the RIFF-based WebP container): our own chunk walk for the header; pixels through
//! `image-webp`.
//!
//! - **Header:** the canvas size (from `VP8 `, `VP8L` or `VP8X`), orientation from the `EXIF`
//!   chunk (with or without an `Exif\0\0` prefix), and the `ICCP` chunk's location. The walk also
//!   checks that the first frame's bitstream declares the size its container says (the decoder
//!   allocates for the bitstream's own size before comparing), and counts animation frames up to
//!   [`MAX_FRAMES`].
//! - **Pixels:** lossy (`VP8 `, with an `ALPH` chunk for alpha) and lossless (`VP8L`); an animation
//!   shows its first frame composited on the canvas. 8-bit RGB or RGBA, with the `ICCP` profile
//!   attached for the colour stage. `image-webp` allocates its own working buffers (the YUV planes,
//!   a lossless frame, an animation's canvas); a conservative estimate of them is charged to the
//!   [`Budget`] before it runs.

use std::io::{Cursor, ErrorKind};
use std::ops::Range;

use image_webp::{DecodingError, WebPDecoder};

use crate::decode::{ColorModel, Decoded, Samples};
use crate::limits::{Budget, Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes WebP files.
pub const DECODES: bool = true;

/// The tile text for a WebP the decoder refuses.
pub const REASON: &str = "This kind of WebP isn't supported";

/// Most frames an animated WebP may hold (`security.md` §4.2); more is refused as damaged.
pub const MAX_FRAMES: usize = 10_000;

/// The `VP8X` animation flag (WebP container specification, "Extended file format").
const FLAG_ANIMATION: u8 = 0x02;

/// What the chunk walk found.
#[derive(Debug, Default)]
struct Layout {
    width: u32,
    height: u32,
    animated: bool,
    /// The first frame is lossy (`VP8 `).
    lossy: bool,
    /// The `EXIF` chunk's data.
    exif: Option<Range<usize>>,
    /// The `ICCP` chunk's data.
    icc: Option<Range<usize>>,
    frames: usize,
}

fn le24(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(3)) {
        Some(&[a, b2, c]) => Ok(u32::from_le_bytes([a, b2, c, 0])),
        _ => Err(Error::Truncated),
    }
}

fn le32(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(4)) {
        Some(&[a, b2, c, d]) => Ok(u32::from_le_bytes([a, b2, c, d])),
        _ => Err(Error::Truncated),
    }
}

/// One chunk: its FourCC, the range of its data in the file, and where the next chunk starts.
struct Chunk {
    kind: [u8; 4],
    data: Range<usize>,
    next: usize,
}

/// The chunk at `pos`, whose data must end by `end`.
fn chunk(b: &[u8], pos: usize, end: usize) -> Result<Chunk> {
    let kind = match b.get(pos..pos.saturating_add(4)) {
        Some(&[a, b2, c, d]) => [a, b2, c, d],
        _ => return Err(Error::Truncated),
    };
    let size = le32(b, pos.saturating_add(4))? as usize;
    let start = pos.checked_add(8).ok_or(Error::OutOfRange)?;
    let data_end = start.checked_add(size).ok_or(Error::OutOfRange)?;
    if data_end > end {
        return Err(Error::Truncated);
    }
    // Chunks are padded to an even size; the pad byte may be missing on the last one.
    let next = data_end.checked_add(size & 1).ok_or(Error::OutOfRange)?;
    Ok(Chunk { kind, data: start..data_end, next })
}

/// The size a lossy key frame declares (RFC 6386 §9.1: frame tag, start code, 14-bit sizes).
fn vp8_size(d: &[u8]) -> Result<(u32, u32)> {
    let Some(&[tag, _, _, s0, s1, s2, w0, w1, h0, h1]) = d.get(..10) else {
        return Err(Error::Truncated);
    };
    if tag & 1 != 0 {
        return Err(Error::Malformed("WebP: the VP8 frame is not a key frame".into()));
    }
    if [s0, s1, s2] != [0x9d, 0x01, 0x2a] {
        return Err(Error::Malformed("WebP: no VP8 start code".into()));
    }
    let (w, h) = (u32::from(u16::from_le_bytes([w0, w1]) & 0x3fff), u32::from(u16::from_le_bytes([h0, h1]) & 0x3fff));
    if w == 0 || h == 0 {
        return Err(Error::Malformed(format!("WebP: VP8 frame {w} × {h}")));
    }
    Ok((w, h))
}

/// The size a lossless bitstream declares (signature 0x2f, then 14-bit sizes minus one).
fn vp8l_size(d: &[u8]) -> Result<(u32, u32)> {
    let Some(&[sig, a, b, c, e]) = d.get(..5) else {
        return Err(Error::Truncated);
    };
    let bits = u32::from_le_bytes([a, b, c, e]);
    if sig != 0x2f || bits >> 29 != 0 {
        return Err(Error::Malformed("WebP: not a version 0 VP8L bitstream".into()));
    }
    Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
}

/// The size of the image bitstream in `c` (`VP8 ` or `VP8L`) and whether it is lossy, if `c` is one.
fn bitstream_size(b: &[u8], c: &Chunk) -> Result<Option<(u32, u32, bool)>> {
    match &c.kind {
        b"VP8 " => vp8_size(b.get(c.data.clone()).ok_or(Error::Truncated)?).map(|(w, h)| Some((w, h, true))),
        b"VP8L" => vp8l_size(b.get(c.data.clone()).ok_or(Error::Truncated)?).map(|(w, h)| Some((w, h, false))),
        _ => Ok(None),
    }
}

/// Walks the chunks of a WebP file.
fn layout(b: &[u8], limits: &Limits) -> Result<Layout> {
    if b.len() < 12 {
        return Err(Error::Truncated);
    }
    if b.get(..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WEBP") {
        return Err(Error::Malformed("not a RIFF WebP file".into()));
    }
    // Chunks end where the RIFF size says, or where the bytes do.
    let end = (le32(b, 4)? as usize).saturating_add(8).min(b.len());
    let first = chunk(b, 12, end)?;
    match bitstream_size(b, &first)? {
        Some((width, height, lossy)) => Ok(Layout { width, height, lossy, ..Layout::default() }),
        None if &first.kind == b"VP8X" => extended(b, &first, end, limits),
        None => Err(Error::Malformed(format!("WebP: the first chunk is {:?}", String::from_utf8_lossy(&first.kind)))),
    }
}

/// The extended format: the `VP8X` canvas, then every chunk after it.
fn extended(b: &[u8], vp8x: &Chunk, end: usize, limits: &Limits) -> Result<Layout> {
    let d = b.get(vp8x.data.clone()).ok_or(Error::Truncated)?;
    let flags = *d.first().ok_or(Error::Truncated)?;
    let (width, height) = (le24(d, 4)? + 1, le24(d, 7)? + 1);
    if u64::from(width) * u64::from(height) > u64::from(u32::MAX) {
        return Err(Error::Malformed(format!("WebP canvas {width} × {height}")));
    }
    let mut out = Layout { width, height, animated: flags & FLAG_ANIMATION != 0, ..Layout::default() };
    let mut first_frame = false;
    let mut work = Work::new(limits.max_segments, "WebP chunks");
    let mut pos = vp8x.next;
    // Fewer than 8 bytes left is trailing padding, not a chunk.
    while end.saturating_sub(pos) >= 8 {
        let c = chunk(b, pos, end)?;
        match &c.kind {
            b"ANMF" => {
                out.frames += 1;
                if out.frames > MAX_FRAMES {
                    return Err(Error::TooMany("WebP frames"));
                }
                if out.frames == 1 && out.animated {
                    out.lossy = animation_frame(b, &c, width, height)?;
                    first_frame = true;
                }
            }
            b"EXIF" => {
                work.step()?;
                out.exif.get_or_insert(c.data.clone());
            }
            b"ICCP" => {
                work.step()?;
                out.icc.get_or_insert(c.data.clone());
            }
            _ => {
                work.step()?;
                if !out.animated
                    && !first_frame
                    && let Some((w, h, lossy)) = bitstream_size(b, &c)?
                {
                    if (w, h) != (width, height) {
                        return Err(Error::Malformed(format!("WebP: a {w} × {h} bitstream on a {width} × {height} canvas")));
                    }
                    out.lossy = lossy;
                    first_frame = true;
                }
            }
        }
        pos = c.next;
    }
    if !first_frame {
        // The image data has not been seen: the file is cut short (or holds none).
        return Err(Error::Truncated);
    }
    Ok(out)
}

/// Checks an animation frame: its rectangle inside the canvas, and its bitstream (after an
/// optional `ALPH` chunk) declaring the frame's size. Returns whether it is lossy.
fn animation_frame(b: &[u8], anmf: &Chunk, width: u32, height: u32) -> Result<bool> {
    let d = b.get(anmf.data.clone()).ok_or(Error::Truncated)?;
    let (x, y) = (u64::from(le24(d, 0)?) * 2, u64::from(le24(d, 3)?) * 2);
    let (fw, fh) = (le24(d, 6)? + 1, le24(d, 9)? + 1);
    if x + u64::from(fw) > u64::from(width) || y + u64::from(fh) > u64::from(height) {
        return Err(Error::Malformed(format!("WebP: a {fw} × {fh} frame at {x}, {y} outside the {width} × {height} canvas")));
    }
    let mut pos = anmf.data.start.checked_add(16).ok_or(Error::OutOfRange)?;
    // At most an `ALPH` chunk, then the bitstream.
    for _ in 0..2 {
        let c = chunk(b, pos, anmf.data.end)?;
        if let Some((w, h, lossy)) = bitstream_size(b, &c)? {
            if (w, h) != (fw, fh) {
                return Err(Error::Malformed(format!("WebP: a {w} × {h} bitstream in a {fw} × {fh} frame")));
            }
            return Ok(lossy);
        }
        pos = c.next;
    }
    Err(Error::Malformed("WebP: an animation frame without a bitstream".into()))
}

/// The canvas size and the `EXIF` chunk (orientation, a thumbnail in IFD1).
pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let layout = layout(bytes, limits)?;
    let exif = layout.exif.and_then(|r| {
        let data = bytes.get(r.clone())?;
        // The specification stores the TIFF block bare; some writers keep the JPEG APP1 prefix.
        match data.strip_prefix(b"Exif\0\0") {
            Some(tiff) => Some((tiff, r.start + 6)),
            None => Some((data, r.start)),
        }
    });
    let header = Header {
        format: Format::WebP,
        width: layout.width,
        height: layout.height,
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: None,
    };
    Ok(header.with_exif(exif, limits))
}

/// The thumbnail in the `EXIF` chunk's IFD1, if there is one.
pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let layout = layout(bytes, limits)?;
    let (width, height) = (layout.width, layout.height);
    if (width, height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("WebP canvas {width} × {height}, header says {} × {}", header.width, header.height)));
    }
    let mut d = WebPDecoder::new(Cursor::new(bytes)).map_err(|e| webp_err(e, width, height))?;
    if d.dimensions() != (width, height) {
        let (w, h) = d.dimensions();
        return Err(Error::Malformed(format!("WebP: the decoder sees {w} × {h}, the chunks say {width} × {height}")));
    }
    let alpha = d.has_alpha();
    let len = limits.check_size(width, height, if alpha { 4 } else { 3 })?;
    // The decoder's own buffers, per pixel of the canvas padded to whole 16 × 16 macroblocks:
    // lossy, the YUV planes (1.5) and an alpha plane (1), rounded up; lossless, an RGBA frame; an
    // animation, the frame, the RGBA canvas and an alpha plane.
    let padded = u64::from(width).div_ceil(16) * 16 * (u64::from(height).div_ceil(16) * 16);
    let per_pixel = if layout.animated {
        9
    } else if layout.lossy {
        3
    } else {
        4
    };
    budget.charge(padded.saturating_mul(per_pixel))?;
    // Bounds the metadata chunks the decoder reads for itself.
    d.set_memory_limit(limits.max_icc);
    if d.output_buffer_size() != Some(len) {
        return Err(Error::Malformed(format!("WebP output of {:?} bytes, expected {len}", d.output_buffer_size())));
    }
    let mut buf = budget.vec(len, 0u8)?;
    d.read_image(&mut buf).map_err(|e| webp_err(e, width, height))?;
    let icc = layout.icc.and_then(|r| bytes.get(r)).filter(|p| p.len() <= limits.max_icc).map(<[u8]>::to_vec);
    Ok(Decoded { width, height, model: ColorModel::Rgb, alpha, samples: Samples::U8(buf), icc, orientation: header.orientation })
}

fn webp_err(e: DecodingError, width: u32, height: u32) -> Error {
    match e {
        DecodingError::IoError(io) if io.kind() == ErrorKind::UnexpectedEof => Error::Truncated,
        DecodingError::MemoryLimitExceeded | DecodingError::ImageTooLarge => Error::TooLarge { width, height },
        DecodingError::UnsupportedFeature(_) => Error::Unsupported(Format::WebP),
        other => Error::Malformed(format!("WebP: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A RIFF file of the given chunks, each padded to an even size.
    fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for (kind, data) in chunks {
            body.extend(*kind);
            body.extend((data.len() as u32).to_le_bytes());
            body.extend(*data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        [b"RIFF".as_slice(), &(body.len() as u32).to_le_bytes(), &body].concat()
    }

    /// A `VP8X` chunk's data for a `w × h` canvas.
    fn vp8x(flags: u8, w: u32, h: u32) -> Vec<u8> {
        let mut d = vec![flags, 0, 0, 0];
        d.extend(&(w - 1).to_le_bytes()[..3]);
        d.extend(&(h - 1).to_le_bytes()[..3]);
        d
    }

    /// The start of a `VP8L` bitstream declaring `w × h`.
    fn vp8l(w: u32, h: u32) -> Vec<u8> {
        let bits = (w - 1) | ((h - 1) << 14);
        [&[0x2f][..], &bits.to_le_bytes(), &[0; 8]].concat()
    }

    /// An `ANMF` chunk's data: a `w × h` frame at 0, 0 holding a `VP8L` bitstream.
    fn anmf(w: u32, h: u32) -> Vec<u8> {
        let mut d = vec![0u8; 6];
        d.extend(&(w - 1).to_le_bytes()[..3]);
        d.extend(&(h - 1).to_le_bytes()[..3]);
        d.extend([100, 0, 0, 0]);
        let bits = vp8l(w, h);
        d.extend(b"VP8L");
        d.extend((bits.len() as u32).to_le_bytes());
        d.extend(&bits);
        d
    }

    #[test]
    fn simple_lossless_and_lossy_sizes() {
        let l = Limits::default();
        let h = header(&riff(&[(b"VP8L", &vp8l(300, 7))]), &l).unwrap();
        assert_eq!((h.width, h.height, h.orientation.get()), (300, 7, 1));
        let vp8 = [0x10, 0, 0, 0x9d, 0x01, 0x2a, 0x40, 0x00, 0x30, 0xc0, 0, 0];
        let lay = layout(&riff(&[(b"VP8 ", &vp8)]), &l).unwrap();
        // The top two bits of each size are a scale, not part of the size.
        assert_eq!((lay.width, lay.height, lay.lossy), (64, 48, true));
        let mut inter = vp8;
        inter[0] = 0x11;
        assert!(matches!(layout(&riff(&[(b"VP8 ", &inter)]), &l), Err(Error::Malformed(_))));
    }

    #[test]
    fn a_bitstream_that_disagrees_with_its_canvas_is_refused() {
        let l = Limits::default();
        let ok = riff(&[(b"VP8X", &vp8x(0, 16, 16)), (b"VP8L", &vp8l(16, 16))]);
        assert_eq!(layout(&ok, &l).unwrap().width, 16);
        let big = riff(&[(b"VP8X", &vp8x(0, 16, 16)), (b"VP8L", &vp8l(16_000, 16_000))]);
        assert!(matches!(layout(&big, &l), Err(Error::Malformed(_))));
        // Without any bitstream, the file is cut short.
        assert_eq!(layout(&riff(&[(b"VP8X", &vp8x(0, 16, 16))]), &l).unwrap_err(), Error::Truncated);
        // A canvas past 2³² pixels breaks the specification.
        let huge = riff(&[(b"VP8X", &vp8x(0, 1 << 24, 1 << 24)), (b"VP8L", &vp8l(16, 16))]);
        assert!(matches!(layout(&huge, &l), Err(Error::Malformed(_))));
    }

    #[test]
    fn frames_are_counted_up_to_the_limit() {
        let l = Limits::default();
        let frame = anmf(16, 16);
        let head = vp8x(FLAG_ANIMATION, 16, 16);
        let mut chunks: Vec<(&[u8; 4], &[u8])> = vec![(b"VP8X", &head), (b"ANIM", &[0; 6])];
        chunks.extend(std::iter::repeat_n((b"ANMF", frame.as_slice()), MAX_FRAMES));
        let lay = layout(&riff(&chunks), &l).unwrap();
        assert_eq!((lay.frames, lay.animated, lay.lossy), (MAX_FRAMES, true, false));
        chunks.push((b"ANMF", &frame));
        assert_eq!(layout(&riff(&chunks), &l).unwrap_err(), Error::TooMany("WebP frames"));
        // A first frame reaching past the canvas, or whose bitstream is larger than the frame, is
        // refused before decoding.
        let mut outside = frame.clone();
        outside[0] = 1; // x = 2
        assert!(matches!(layout(&riff(&[(b"VP8X", &head), (b"ANMF", &outside)]), &l), Err(Error::Malformed(_))));
        let mut lying = frame.clone();
        lying.splice(24.., vp8l(16_000, 16_000));
        assert!(matches!(layout(&riff(&[(b"VP8X", &head), (b"ANMF", &lying)]), &l), Err(Error::Malformed(_))));
    }

    #[test]
    fn chunks_beyond_the_segment_limit_are_refused() {
        let l = Limits { max_segments: 3, ..Limits::default() };
        let head = vp8x(0, 16, 16);
        let bits = vp8l(16, 16);
        let mut chunks: Vec<(&[u8; 4], &[u8])> = vec![(b"VP8X", &head), (b"XMP ", b"x"), (b"VP8L", &bits), (b"XMP ", b"x")];
        assert!(layout(&riff(&chunks), &l).is_ok());
        chunks.push((b"XMP ", b"y"));
        assert_eq!(layout(&riff(&chunks), &l).unwrap_err(), Error::TooMany("WebP chunks"));
    }
}
