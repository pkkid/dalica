//! TIFF (TIFF 6.0, written from the public specification; `formats.md` §1): the first
//! full-resolution page of the file.
//!
//! - **Header:** our own bounded IFD walk ([`crate::ifd`]) finds the page: the first IFD in IFD0's
//!   chain that is not marked reduced-resolution (`NewSubfileType` bit 0, or the old
//!   `SubfileType` 2). Its size and `Orientation` tag, and the file's EXIF fields (the TIFF file is
//!   itself the EXIF structure), are what scanning reads.
//! - **Decode:** the page's layout is read and checked against the limits with our own reader
//!   before the pinned `tiff` decoder sees any strip; then `tiff` decodes the strips or tiles
//!   (uncompressed, LZW, Deflate, PackBits, JPEG; predictors) into buffers taken from the
//!   [`Budget`]. Every layout goes straight to [`Decoded`]: 1–16-bit and 32-bit integers and 16-,
//!   32- and 64-bit floats; grey (either polarity), RGB, CMYK ink and palette; chunky or planar;
//!   an alpha channel from `ExtraSamples` (premultiplied alpha divided out). `tiff` 0.11 refuses
//!   palettes, so a palette page's `PhotometricInterpretation` is read as grey through a patched
//!   view of the file and the indices are looked up in its `ColorMap` here. JPEG-compressed YCbCr
//!   is converted to RGB here; other YCbCr, the Lab encodings and everything else answer
//!   [`Error::Unsupported`]. The page's ICC profile tag goes to the colour stage.
//! - **Thumbnail fast path:** a reduced-resolution page (a thumbnail IFD) that covers the target
//!   is decoded instead of the page ([`thumbnail`]); an EXIF IFD1 JPEG thumbnail is the
//!   [`embedded_thumbnail`] range.
//!
//! BigTIFF (`II+`/`MM+`) is recognised and answers `Unsupported`.

use std::borrow::Cow;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::ops::Range;

use tiff::decoder::{Decoder, DecodingBuffer};

use crate::decode::{ColorModel, DecodeOptions, Decoded, Samples, guard};
use crate::ifd::{Entry, Ifd, Tiff, Visited, types};
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Orientation, Result, Rgba8, color, orient, resize};

/// Whether this build decodes TIFF files.
pub const DECODES: bool = true;

/// The tile text for a TIFF whose layout Dalica does not decode (Lab, plain YCbCr, BigTIFF, …).
pub const REASON: &str = "This kind of TIFF isn't supported";

const TAG_NEW_SUBFILE_TYPE: u16 = 254;
const TAG_SUBFILE_TYPE: u16 = 255;
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_ORIENTATION: u16 = 274;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_COLOR_MAP: u16 = 320;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_INK_SET: u16 = 332;
const TAG_NUMBER_OF_INKS: u16 = 334;
const TAG_EXTRA_SAMPLES: u16 = 338;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_ICC_PROFILE: u16 = 34_675;

const COMPRESSION_OLD_JPEG: u32 = 6;
const COMPRESSION_JPEG: u32 = 7;

/// Most strips or tiles in one page: enough for 16 × 16 tiles over 2³⁰ pixels.
pub const MAX_CHUNKS: u64 = 1 << 22;
/// What the `tiff` decoder holds per strip or tile while it reads the page's tags: each offset and
/// byte count as a tag value (24 bytes) and then as a `u64`, charged to the budget.
const BYTES_PER_CHUNK: u64 = 64;
/// Memory the `tiff` decoder uses besides its per-chunk numbers and buffers (other tag values,
/// decompressor state).
const DECODER_OVERHEAD: u64 = 1 << 20;

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let t = open(bytes)?;
    // A value past the end of what we hold may lie in the part of the file not read yet.
    let page = first_page(&t, limits).map_err(prefix)?;
    let plain = Header {
        format: Format::Tiff,
        width: page.width,
        height: page.height,
        orientation: page.orientation,
        exif: None,
        exif_error: None,
        thumbnail: None,
    };
    let mut header = plain.with_exif(Some((bytes, 0)), limits);
    if header.exif_error == Some(Error::OutOfRange) && holds_a_prefix(&t, &page.ifd, limits) {
        return Err(Error::Truncated);
    }
    header.orientation = page.orientation;
    Ok(header)
}

/// The EXIF IFD1 JPEG thumbnail, if the file carries one (thumbnail pages are not files of their
/// own: [`thumbnail`] decodes them).
pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let t = open(bytes)?;
    let page = first_page(&t, limits)?;
    if (page.width, page.height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("page of {} × {}, header says {} × {}", page.width, page.height, header.width, header.height)));
    }
    let mut decoded = decode_page(bytes, &t, &page, budget, limits)?;
    decoded.orientation = header.orientation;
    Ok(decoded)
}

/// The thumbnail fast path: the smallest reduced-resolution page whose longer side covers
/// `long_side`, decoded (in its own panic guard and budget), converted to sRGB, fitted and turned
/// upright with the main page's orientation. `Ok(None)` when no thumbnail page covers the target;
/// a thumbnail page that fails to decode is an `Err`, and the caller decodes the page instead.
pub fn thumbnail(bytes: &[u8], long_side: u32, options: &DecodeOptions, limits: &Limits) -> Result<Option<Rgba8>> {
    let t = open(bytes)?;
    let main = first_page(&t, limits)?;
    let Some(small) = thumbnail_page(&t, &main, long_side, limits) else { return Ok(None) };
    limits.check_size(small.width, small.height, 4)?;
    let mut budget = Budget::new(limits, small.width, small.height);
    let decoded = guard(Format::Tiff, || decode_page(bytes, &t, &small, &mut budget, limits))?;
    let stored = color::to_srgb8(decoded, options, &mut budget, limits)?.image;
    let fitted = resize::fit(&stored, long_side)?;
    orient::apply(fitted, main.orientation).map(Some)
}

// ---- Pages ----------------------------------------------------------------------------------

/// One IFD of the chain, with its position for the `tiff` decoder.
#[derive(Clone, Debug)]
struct Page {
    /// Position in IFD0's chain (0 for IFD0).
    index: usize,
    ifd: Ifd,
    width: u32,
    height: u32,
    orientation: Orientation,
    reduced: bool,
}

fn open(bytes: &[u8]) -> Result<Tiff<'_>> {
    if bytes.starts_with(b"II+\0") || bytes.starts_with(b"MM\0+") {
        return Err(Error::Unsupported(Format::Tiff));
    }
    Tiff::new(bytes)
}

fn page(t: &Tiff, index: usize, ifd: Ifd) -> Result<Page> {
    let size = |tag| ifd.get(tag).and_then(|e| t.uint(e)).filter(|&v| v > 0);
    let (Some(width), Some(height)) = (size(TAG_IMAGE_WIDTH), size(TAG_IMAGE_LENGTH)) else {
        return Err(Error::Malformed("a TIFF page without a size".into()));
    };
    let orientation =
        ifd.get(TAG_ORIENTATION).and_then(|e| t.uint(e)).and_then(|o| u8::try_from(o).ok()).and_then(Orientation::new).unwrap_or_default();
    let reduced = ifd.get(TAG_NEW_SUBFILE_TYPE).and_then(|e| t.uint(e)).is_some_and(|v| v & 1 == 1)
        || ifd.get(TAG_SUBFILE_TYPE).and_then(|e| t.uint(e)) == Some(2);
    Ok(Page { index, ifd, width, height, orientation, reduced })
}

/// Walks IFD0's chain, handing each page to `visit` until it returns `true`. The walk stops at the
/// end of the chain, or with an error at the IFD cap, a loop or an unreadable IFD.
fn walk(t: &Tiff, limits: &Limits, mut visit: impl FnMut(Page) -> bool) -> Result<()> {
    let mut visited = Visited::default();
    let mut offset = t.first_ifd()?;
    let mut index = 0usize;
    loop {
        let ifd = t.ifd(offset, &mut visited, limits)?;
        let next = ifd.next;
        if visit(page(t, index, ifd)?) || next == 0 {
            return Ok(());
        }
        offset = next;
        index = index.checked_add(1).ok_or(Error::TooMany("IFDs"))?;
    }
}

/// The page Dalica shows: the first full-resolution IFD of the chain; failing that, the first
/// thumbnail page before trouble or the end of the chain.
fn first_page(t: &Tiff, limits: &Limits) -> Result<Page> {
    let (mut first, mut full) = (None, None);
    let walked = walk(t, limits, |p| {
        if !p.reduced {
            full = Some(p);
            return true;
        }
        first.get_or_insert(p);
        false
    });
    match (full, first, walked) {
        (Some(p), _, _) => Ok(p),
        // The full page may lie in the part of the file not read yet.
        (None, _, Err(Error::OutOfRange)) => Err(Error::OutOfRange),
        (None, Some(p), _) => Ok(p),
        (None, None, Err(e)) => Err(e),
        (None, None, Ok(())) => Err(Error::Malformed("a TIFF without pages".into())),
    }
}

/// The smallest reduced-resolution page (other than `main`) whose longer side is at least
/// `long_side`. Trouble further down the chain ends the search; it never fails the decode.
fn thumbnail_page(t: &Tiff, main: &Page, long_side: u32, limits: &Limits) -> Option<Page> {
    let mut best: Option<Page> = None;
    let _ = walk(t, limits, |p| {
        let side = p.width.max(p.height);
        let smaller = best.as_ref().is_none_or(|b| side < b.width.max(b.height));
        if p.reduced && p.index != main.index && side >= long_side && smaller {
            best = Some(p);
        }
        false
    });
    best
}

/// `OutOfRange` becomes `Truncated`: while reading a header from the start of a file, a value past
/// the end may only mean that more of the file is needed.
fn prefix(e: Error) -> Error {
    if e == Error::OutOfRange { Error::Truncated } else { e }
}

/// Whether `t` is surely the start of a longer file (or a damaged one): the page's strips or
/// tiles end past it or cannot be read, and the header reader could still read more.
fn holds_a_prefix(t: &Tiff, ifd: &Ifd, limits: &Limits) -> bool {
    if t.len() as u64 >= limits.max_header_bytes {
        return false;
    }
    match chunk_tags(ifd) {
        Some((offsets, counts)) => data_end(t, offsets, counts).is_none_or(|end| end > t.len() as u64),
        None => false,
    }
}

/// The page's chunk offset and byte count entries: tiles if it has them, else strips.
fn chunk_tags(ifd: &Ifd) -> Option<(&Entry, &Entry)> {
    match (ifd.get(TAG_TILE_OFFSETS), ifd.get(TAG_TILE_BYTE_COUNTS)) {
        (Some(o), Some(c)) => Some((o, c)),
        _ => Some((ifd.get(TAG_STRIP_OFFSETS)?, ifd.get(TAG_STRIP_BYTE_COUNTS)?)),
    }
}

/// The end of the last chunk, or `None` if the entries cannot be read.
fn data_end(t: &Tiff, offsets: &Entry, counts: &Entry) -> Option<u64> {
    let (o, c) = (Values::read(t, offsets).ok()?, Values::read(t, counts).ok()?);
    o.iter().zip(c.iter()).map(|(o, c)| u64::from(o) + u64::from(c)).max()
}

// ---- Values ---------------------------------------------------------------------------------

/// The SHORT or LONG values of one entry, in the file's byte order.
struct Values<'a> {
    bytes: Cow<'a, [u8]>,
    size: usize,
    little: bool,
}

impl<'a> Values<'a> {
    fn read(t: &Tiff<'a>, e: &Entry) -> Result<Values<'a>> {
        let size = match e.kind {
            types::SHORT => 2,
            types::LONG => 4,
            kind => return Err(Error::Malformed(format!("TIFF tag {} has type {kind}", e.tag))),
        };
        let bytes = t.value(e)?.ok_or(Error::Malformed(format!("TIFF tag {} has no value", e.tag)))?;
        Ok(Values { bytes, size, little: little_endian(t) })
    }

    fn len(&self) -> usize {
        self.bytes.len() / self.size
    }

    fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.bytes.chunks_exact(self.size).map(|c| match (c, self.little) {
            (&[a, b], true) => u32::from(u16::from_le_bytes([a, b])),
            (&[a, b], false) => u32::from(u16::from_be_bytes([a, b])),
            (&[a, b, c, d], true) => u32::from_le_bytes([a, b, c, d]),
            (&[a, b, c, d], false) => u32::from_be_bytes([a, b, c, d]),
            _ => 0,
        })
    }

    /// The one value every sample shares (`BitsPerSample`, `SampleFormat`); mixed values are a
    /// layout Dalica does not decode.
    fn uniform(&self) -> Result<Option<u32>> {
        let mut values = self.iter();
        let first = values.next();
        if values.any(|v| Some(v) != first) {
            return Err(Error::Unsupported(Format::Tiff));
        }
        Ok(first)
    }
}

fn little_endian(t: &Tiff) -> bool {
    t.bytes(0, 2).ok() == Some(b"II".as_slice())
}

// ---- Layout ---------------------------------------------------------------------------------

/// How a page's samples relate to colour.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Gray,
    Rgb,
    Cmyk,
    /// Indices into these colours.
    Palette(Vec<[u8; 3]>),
    /// JPEG-compressed YCbCr (JFIF full range), turned into RGB here.
    YCbCr,
}

/// What a page's IFD says about its pixels, read with our own reader and checked before the
/// `tiff` decoder is created.
#[derive(Clone, Debug)]
struct Layout {
    width: u32,
    height: u32,
    kind: Kind,
    /// Colour samples per pixel as stored (1 for palette and grey).
    colour: usize,
    /// Samples per pixel, extra samples included.
    samples: usize,
    /// The first extra sample is alpha; `true` if premultiplied ("associated").
    alpha: Option<bool>,
    bits: u32,
    float: bool,
    planar: bool,
    jpeg: bool,
    chunks: u64,
    largest_chunk: u64,
    /// The bytes that relabel a palette page as grey for the `tiff` decoder: file offset, bytes.
    patch: Option<(u64, [u8; 4])>,
    icc: Option<Vec<u8>>,
}

impl Layout {
    fn read(t: &Tiff, page: &Page, limits: &Limits) -> Result<Layout> {
        let ifd = &page.ifd;
        let unsupported = || Error::Unsupported(Format::Tiff);
        let uint = |tag| ifd.get(tag).and_then(|e| t.uint(e));
        let photometric = uint(TAG_PHOTOMETRIC).ok_or(Error::Malformed("a TIFF page without PhotometricInterpretation".into()))?;
        let compression = uint(TAG_COMPRESSION).unwrap_or(1);
        if compression == COMPRESSION_OLD_JPEG {
            return Err(unsupported());
        }
        let jpeg = compression == COMPRESSION_JPEG;
        let samples = uint(TAG_SAMPLES_PER_PIXEL).unwrap_or(1) as usize;
        let read_uniform = |tag, default| -> Result<u32> {
            let Some(e) = ifd.get(tag) else { return Ok(default) };
            let v = Values::read(t, e)?;
            if v.len() != 1 && v.len() != samples {
                return Err(Error::Malformed(format!("TIFF tag {tag} has {} values for {samples} samples", v.len())));
            }
            Ok(v.uniform()?.unwrap_or(default))
        };
        let bits = read_uniform(TAG_BITS_PER_SAMPLE, 1)?;
        let float = match read_uniform(TAG_SAMPLE_FORMAT, 1)? {
            1 => false,
            3 => true,
            _ => return Err(unsupported()),
        };
        let (kind, colour) = match photometric {
            0 | 1 => (Kind::Gray, 1),
            2 => (Kind::Rgb, 3),
            3 => (Kind::Palette(Vec::new()), 1),
            5 => {
                if uint(TAG_INK_SET).unwrap_or(1) != 1 || uint(TAG_NUMBER_OF_INKS).unwrap_or(4) != 4 {
                    return Err(unsupported());
                }
                (Kind::Cmyk, 4)
            }
            6 if jpeg && bits == 8 && !float => (Kind::YCbCr, 3),
            // Plain YCbCr, the Lab encodings, masks, LogLuv, CFA and linear raw data.
            _ => return Err(unsupported()),
        };
        if samples < colour || samples > 64 {
            return Err(Error::Malformed(format!("{samples} samples per pixel for photometric {photometric}")));
        }
        let palette = matches!(kind, Kind::Palette(_));
        let supported = match (float, bits) {
            (false, 1 | 2 | 4) => samples == 1 && matches!(kind, Kind::Gray | Kind::Palette(_)),
            (false, 8) => !palette || samples == 1,
            (false, 16 | 32) => !palette,
            (true, 16 | 32 | 64) => matches!(kind, Kind::Gray | Kind::Rgb),
            _ => false,
        };
        if !supported {
            return Err(unsupported());
        }
        let planar = match uint(TAG_PLANAR_CONFIGURATION).unwrap_or(1) {
            1 => false,
            2 => samples > 1,
            other => return Err(Error::Malformed(format!("PlanarConfiguration {other}"))),
        };
        let alpha = match ifd.get(TAG_EXTRA_SAMPLES) {
            Some(e) if samples > colour => match Values::read(t, e)?.iter().next() {
                Some(1) => Some(true),
                Some(2) => Some(false),
                _ => None,
            },
            _ => None,
        };
        let (kind, patch) = match kind {
            Kind::Palette(_) => (Kind::Palette(color_map(t, ifd, bits)?), Some(grey_label(t, ifd)?)),
            kind => (kind, None),
        };
        let (chunks, largest_chunk) = chunks(t, ifd)?;
        let icc = match ifd.get(TAG_ICC_PROFILE) {
            Some(e) if e.count as usize > limits.max_icc => {
                log::warn!("a TIFF colour profile of {} bytes is over the limit; shown as sRGB", e.count);
                None
            }
            Some(e) if matches!(e.kind, types::UNDEFINED | types::BYTE) => t.value(e)?.map(Cow::into_owned),
            _ => None,
        };
        Ok(Layout {
            width: page.width,
            height: page.height,
            kind,
            colour,
            samples,
            alpha,
            bits,
            float,
            planar,
            jpeg,
            chunks,
            largest_chunk,
            patch,
            icc,
        })
    }

    /// Bytes the `tiff` decoder writes for the whole page.
    fn raw_bytes(&self) -> Option<u64> {
        let (w, h) = (u64::from(self.width), u64::from(self.height));
        let bits = u64::from(self.bits);
        if self.bits < 8 {
            // One sample per pixel, every row starting on a byte.
            return w.checked_mul(bits)?.div_ceil(8).checked_mul(h);
        }
        w.checked_mul(h)?.checked_mul(self.samples as u64)?.checked_mul(bits / 8)
    }

    /// Channels of the decoded image: colour (palette and YCbCr as RGB) plus alpha.
    fn out_channels(&self) -> usize {
        let colour = match self.kind {
            Kind::Palette(_) | Kind::YCbCr => 3,
            _ => self.colour,
        };
        colour + usize::from(self.alpha.is_some())
    }

    /// Bytes per decoded sample.
    fn out_sample_bytes(&self) -> u64 {
        match (self.float, self.bits) {
            (true, _) => 4,
            (false, 16 | 32) => 2,
            _ => 1,
        }
    }
}

/// The number of strips or tiles of a page and the largest one's byte count, after checking the
/// count against [`MAX_CHUNKS`] and both lists against the file.
fn chunks(t: &Tiff, ifd: &Ifd) -> Result<(u64, u64)> {
    let (offsets, counts) = chunk_tags(ifd).ok_or(Error::Malformed("a TIFF page without strips or tiles".into()))?;
    let n = u64::from(offsets.count);
    if n > MAX_CHUNKS || u64::from(counts.count) > MAX_CHUNKS {
        return Err(Error::TooMany("TIFF strips or tiles"));
    }
    Values::read(t, offsets)?;
    let largest = Values::read(t, counts)?.iter().max().map_or(0, u64::from);
    Ok((n, largest))
}

/// The colour map of a palette page: 2^bits reds, then greens, then blues, 16 bits each.
fn color_map(t: &Tiff, ifd: &Ifd, bits: u32) -> Result<Vec<[u8; 3]>> {
    let e = ifd.get(TAG_COLOR_MAP).ok_or(Error::Malformed("a palette TIFF without a ColorMap".into()))?;
    let n = 1usize << bits.min(8);
    if e.count as usize != 3 * n {
        return Err(Error::Malformed(format!("a ColorMap of {} values for {bits}-bit indices", e.count)));
    }
    let map: Vec<u32> = Values::read(t, e)?.iter().collect();
    let to8 = |v: u32| ((v * 255 + 32_767) / 65_535) as u8;
    Ok((0..n).map(|i| [0, n, 2 * n].map(|plane| to8(map.get(plane + i).copied().unwrap_or(0)))).collect())
}

/// Where the page's `PhotometricInterpretation` value lies in the file and the bytes that make it
/// 1 (grey), so the `tiff` decoder reads a palette page's indices.
fn grey_label(t: &Tiff, ifd: &Ifd) -> Result<(u64, [u8; 4])> {
    let k = ifd.entries.iter().position(|e| e.tag == TAG_PHOTOMETRIC).ok_or(Error::Malformed("no PhotometricInterpretation".into()))?;
    let at = u64::from(ifd.offset) + 2 + 12 * k as u64 + 8;
    let value = match (ifd.entries.get(k).map(|e| e.kind), little_endian(t)) {
        (Some(types::SHORT | types::LONG), true) => [1, 0, 0, 0],
        (Some(types::SHORT), false) => [0, 1, 0, 0],
        (Some(types::LONG), false) => [0, 0, 0, 1],
        _ => return Err(Error::Malformed("PhotometricInterpretation is not a number".into())),
    };
    Ok((at, value))
}

// ---- Decoding -------------------------------------------------------------------------------

/// The file's bytes as the `tiff` decoder reads them, with an optional 4-byte patch laid over.
struct Patched<'a> {
    inner: Cursor<&'a [u8]>,
    patch: Option<(u64, [u8; 4])>,
}

impl Read for Patched<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.inner.position();
        let n = self.inner.read(buf)?;
        if let Some((at, bytes)) = self.patch {
            for (k, b) in bytes.iter().enumerate() {
                let Some(pos) = (at + k as u64).checked_sub(start) else { continue };
                if let Some(slot) = usize::try_from(pos).ok().filter(|&p| p < n).and_then(|p| buf.get_mut(p)) {
                    *slot = *b;
                }
            }
        }
        Ok(n)
    }
}

impl Seek for Patched<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.inner.seek(to)
    }
}

fn tiff_err(width: u32, height: u32) -> impl Fn(tiff::TiffError) -> Error {
    move |e| match e {
        tiff::TiffError::UnsupportedError(_) => Error::Unsupported(Format::Tiff),
        tiff::TiffError::LimitsExceeded => Error::TooLarge { width, height },
        tiff::TiffError::IoError(e) if e.kind() == io::ErrorKind::UnexpectedEof => Error::Truncated,
        other => Error::Malformed(format!("TIFF: {other}")),
    }
}

/// Samples as the `tiff` decoder writes them (in the machine's byte order).
enum Raw {
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    /// IEEE half-precision bit patterns.
    F16(Vec<u16>),
    F32(Vec<f32>),
    /// IEEE double-precision bit patterns.
    F64(Vec<u64>),
}

fn decode_page(bytes: &[u8], t: &Tiff, page: &Page, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let l = Layout::read(t, page, limits)?;
    let (width, height) = (l.width, l.height);
    let too_large = || Error::TooLarge { width, height };
    // The limits, from the IFD alone: the decoded samples, the raw buffer, and the decoder's own
    // memory (its tag values, the largest compressed chunk, and for JPEG a decoded copy). We read
    // into our own buffers, so `tiff`'s decoding-buffer limit only bounds its tag value lists.
    limits.check_size(width, height, l.out_channels() as u64 * l.out_sample_bytes())?;
    let raw_len = l.raw_bytes().filter(|&b| b <= limits.max_alloc).ok_or_else(too_large)?;
    let compressed = l.largest_chunk.min(bytes.len() as u64);
    // The `tiff` decoder reads IFD0's strip lists under its default limits before it moves to
    // the page: they are checked and charged here too.
    let first = match page.index {
        0 => 0,
        _ => chunks(t, &t.ifd(t.first_ifd()?, &mut Visited::default(), limits)?)?.0,
    };
    let values = l.chunks.saturating_mul(BYTES_PER_CHUNK).saturating_add(DECODER_OVERHEAD);
    let own = values.saturating_add(first.saturating_mul(BYTES_PER_CHUNK));
    let own = own.saturating_add(compressed).saturating_add(if l.jpeg { raw_len } else { 0 });
    budget.charge(own)?;
    let mut tl = tiff::decoder::Limits::default();
    tl.decoding_buffer_size = usize::try_from(values).map_err(|_| too_large())?;
    tl.intermediate_buffer_size = usize::try_from(compressed).map_err(|_| too_large())?;
    tl.ifd_value_size = usize::try_from(values).map_err(|_| too_large())?;
    let err = tiff_err(width, height);
    let mut decoder = Decoder::new(Patched { inner: Cursor::new(bytes), patch: l.patch }).map_err(&err)?.with_limits(tl);
    decoder.seek_to_image(page.index).map_err(&err)?;
    if decoder.dimensions().map_err(&err)? != (width, height) {
        return Err(Error::Malformed("the TIFF decoder found another page size".into()));
    }
    let raw_len = usize::try_from(raw_len).map_err(|_| too_large())?;
    let raw = match (l.float, l.bits) {
        (false, 1..=8) => {
            let mut v = budget.vec(raw_len, 0u8)?;
            decoder.read_image_bytes(&mut v).map_err(&err)?;
            Raw::U8(v)
        }
        (false, 16) => {
            let mut v = budget.vec(raw_len / 2, 0u16)?;
            decoder.read_image_bytes(DecodingBuffer::U16(&mut v).as_bytes_mut()).map_err(&err)?;
            Raw::U16(v)
        }
        (false, 32) => {
            let mut v = budget.vec(raw_len / 4, 0u32)?;
            decoder.read_image_bytes(DecodingBuffer::U32(&mut v).as_bytes_mut()).map_err(&err)?;
            Raw::U32(v)
        }
        (true, 16) => {
            let mut v = budget.vec(raw_len / 2, 0u16)?;
            decoder.read_image_bytes(DecodingBuffer::U16(&mut v).as_bytes_mut()).map_err(&err)?;
            Raw::F16(v)
        }
        (true, 32) => {
            let mut v = budget.vec(raw_len / 4, 0f32)?;
            decoder.read_image_bytes(DecodingBuffer::F32(&mut v).as_bytes_mut()).map_err(&err)?;
            Raw::F32(v)
        }
        (true, 64) => {
            let mut v = budget.vec(raw_len / 8, 0u64)?;
            decoder.read_image_bytes(DecodingBuffer::U64(&mut v).as_bytes_mut()).map_err(&err)?;
            Raw::F64(v)
        }
        _ => return Err(Error::Unsupported(Format::Tiff)),
    };
    drop(decoder);
    budget.refund(own);
    let samples = finish(raw, &l, budget)?;
    let model = match l.kind {
        Kind::Gray => ColorModel::Gray,
        Kind::Cmyk => ColorModel::Cmyk,
        Kind::Rgb | Kind::Palette(_) | Kind::YCbCr => ColorModel::Rgb,
    };
    Ok(Decoded { width, height, model, alpha: l.alpha.is_some(), samples, icc: l.icc, orientation: page.orientation })
}

/// Raw samples → the decoded samples: low bit depths unpacked, planes interleaved, extra samples
/// dropped, 32-bit integers narrowed, half and double floats converted, palettes and YCbCr turned
/// into RGB, premultiplied alpha divided out.
fn finish(raw: Raw, l: &Layout, budget: &mut Budget) -> Result<Samples> {
    let pixels = l.width as usize * l.height as usize;
    // The channels kept, as indices into each stored pixel.
    let keep: Vec<usize> = (0..l.colour).chain(l.alpha.map(|_| l.colour)).collect();
    let samples = match raw {
        Raw::U8(v) => {
            let v = if l.bits < 8 { unpack(&v, l, pixels, budget)? } else { arrange(v, l, pixels, &keep, budget)? };
            Samples::U8(match &l.kind {
                Kind::Palette(map) => expand(&v, map, budget)?,
                Kind::YCbCr => ycbcr_to_rgb(v, keep.len()),
                _ => v,
            })
        }
        Raw::U16(v) => Samples::U16(arrange(v, l, pixels, &keep, budget)?),
        Raw::U32(v) => {
            let v = arrange(v, l, pixels, &keep, budget)?;
            let mut out = budget.vec(v.len(), 0u16)?;
            for (o, s) in out.iter_mut().zip(&v) {
                *o = (s >> 16) as u16;
            }
            Samples::U16(out)
        }
        Raw::F16(v) => {
            let v = arrange(v, l, pixels, &keep, budget)?;
            let mut out = budget.vec(v.len(), 0f32)?;
            for (o, s) in out.iter_mut().zip(&v) {
                *o = f16_to_f32(*s);
            }
            Samples::F32(out)
        }
        Raw::F32(v) => Samples::F32(arrange(v, l, pixels, &keep, budget)?),
        Raw::F64(v) => {
            let v = arrange(v, l, pixels, &keep, budget)?;
            let mut out = budget.vec(v.len(), 0f32)?;
            for (o, s) in out.iter_mut().zip(&v) {
                *o = f64::from_bits(*s) as f32;
            }
            Samples::F32(out)
        }
    };
    let ch = keep.len();
    Ok(match (l.alpha, samples) {
        (Some(true), Samples::U8(mut v)) => {
            unpremultiply(&mut v, ch, |c, a| if a == 0 { 0 } else { ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8 });
            Samples::U8(v)
        }
        (Some(true), Samples::U16(mut v)) => {
            unpremultiply(&mut v, ch, |c, a| if a == 0 { 0 } else { ((u64::from(c) * 65_535 + u64::from(a) / 2) / u64::from(a)).min(65_535) as u16 });
            Samples::U16(v)
        }
        (Some(true), Samples::F32(mut v)) => {
            unpremultiply(&mut v, ch, |c, a| if a > 0.0 { c / a } else { 0.0 });
            Samples::F32(v)
        }
        (_, samples) => samples,
    })
}

/// Planes interleaved (planar pages), then only the `keep` channels of each pixel, in place.
fn arrange<T: Copy + Default>(v: Vec<T>, l: &Layout, pixels: usize, keep: &[usize], budget: &mut Budget) -> Result<Vec<T>> {
    let n = l.samples;
    if Some(v.len()) != pixels.checked_mul(n) {
        return Err(Error::Malformed(format!("{} TIFF samples for {pixels} pixels of {n}", v.len())));
    }
    let mut v = if l.planar {
        let mut out = budget.vec(v.len(), T::default())?;
        for (plane, src) in v.chunks_exact(pixels.max(1)).enumerate() {
            for (dst, &s) in out.iter_mut().skip(plane).step_by(n).zip(src) {
                *dst = s;
            }
        }
        out
    } else {
        v
    };
    if keep.len() < n {
        // Pixel i's kept channels go to i·k: never past a sample still to be read.
        let k = keep.len();
        for i in 0..pixels {
            for (j, &c) in keep.iter().enumerate() {
                if let Some(s) = v.get(i * n + c).copied()
                    && let Some(d) = v.get_mut(i * k + j)
                {
                    *d = s;
                }
            }
        }
        v.truncate(pixels * k);
    }
    Ok(v)
}

/// 1-, 2- or 4-bit samples (one per pixel, rows starting on a byte) to one byte each: grey scaled
/// to 0–255, palette indices as they are.
fn unpack(packed: &[u8], l: &Layout, pixels: usize, budget: &mut Budget) -> Result<Vec<u8>> {
    let bits = l.bits as usize;
    let width = l.width as usize;
    let row_bytes = (width * bits).div_ceil(8);
    let max = (1u32 << bits) - 1;
    let scale = !matches!(l.kind, Kind::Palette(_));
    let mut out = budget.vec(pixels, 0u8)?;
    for (row, dst) in packed.chunks(row_bytes.max(1)).zip(out.chunks_mut(width.max(1))) {
        for (x, d) in dst.iter_mut().enumerate() {
            let bit = x * bits;
            let byte = u32::from(row.get(bit / 8).copied().unwrap_or(0));
            let v = (byte >> (8 - bits - bit % 8)) & max;
            *d = if scale { (v * 255 / max) as u8 } else { v as u8 };
        }
    }
    Ok(out)
}

/// Palette indices to RGB.
fn expand(indices: &[u8], map: &[[u8; 3]], budget: &mut Budget) -> Result<Vec<u8>> {
    let mut out = budget.vec(indices.len() * 3, 0u8)?;
    for (o, &i) in out.as_chunks_mut::<3>().0.iter_mut().zip(indices) {
        *o = map.get(usize::from(i)).copied().unwrap_or_default();
    }
    Ok(out)
}

/// JFIF YCbCr (full range) to RGB in place, in 16.16 fixed point so every machine agrees.
fn ycbcr_to_rgb(mut v: Vec<u8>, channels: usize) -> Vec<u8> {
    for px in v.chunks_exact_mut(channels.max(1)) {
        if let [y, cb, cr, ..] = px {
            let (l, b, r) = (i32::from(*y) << 16, i32::from(*cb) - 128, i32::from(*cr) - 128);
            let c = |v: i32| ((v + 32_768) >> 16).clamp(0, 255) as u8;
            [*y, *cb, *cr] = [c(l + 91_881 * r), c(l - 22_554 * b - 46_802 * r), c(l + 116_130 * b)];
        }
    }
    v
}

/// Divides each colour sample by the pixel's alpha (its last channel).
fn unpremultiply<T: Copy>(v: &mut [T], channels: usize, div: impl Fn(T, T) -> T) {
    for px in v.chunks_exact_mut(channels.max(1)) {
        if let Some((&mut a, colour)) = px.split_last_mut() {
            for c in colour {
                *c = div(*c, a);
            }
        }
    }
}

/// An IEEE 754 half-precision value to single precision (exact).
fn f16_to_f32(h: u16) -> f32 {
    let negative = h >> 15 == 1;
    let exp = u32::from((h >> 10) & 0x1f);
    let frac = u32::from(h & 0x3ff);
    let magnitude = match (exp, frac) {
        // Zero and subnormals: frac × 2⁻²⁴.
        (0, f) => f as f32 * f32::from_bits(0x3380_0000),
        (0x1f, f) => f32::from_bits(0x7f80_0000 | (f << 13)),
        (e, f) => f32::from_bits(((e + 112) << 23) | (f << 13)),
    };
    if negative { -magnitude } else { magnitude }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_widen_exactly() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x8001), -(2f32.powi(-24)));
        assert_eq!(f16_to_f32(0x7bff), 65_504.0);
        assert!(f16_to_f32(0x7c00).is_infinite() && f16_to_f32(0x7e00).is_nan());
    }

    #[test]
    fn ycbcr_greys_and_primaries() {
        assert_eq!(ycbcr_to_rgb(vec![128, 128, 128, 0, 128, 128, 255, 128, 128], 3), vec![128, 128, 128, 0, 0, 0, 255, 255, 255]);
        // Pure red in JFIF YCbCr is (76, 85, 255); the alpha after it is left alone.
        assert_eq!(ycbcr_to_rgb(vec![76, 85, 255, 9], 4), vec![254, 0, 0, 9]);
    }

    #[test]
    fn the_patch_lies_over_whatever_is_read() {
        let data = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut p = Patched { inner: Cursor::new(&data[..]), patch: Some((3, [30, 40, 50, 60])) };
        let mut buf = [0u8; 4];
        p.seek(SeekFrom::Start(1)).unwrap();
        assert_eq!(p.read(&mut buf).unwrap(), 4);
        assert_eq!(buf, [1, 2, 30, 40]);
        assert_eq!(p.read(&mut buf).unwrap(), 4);
        assert_eq!(buf, [50, 60, 7, 8]);
        assert_eq!(p.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], 9);
    }

    #[test]
    fn packed_samples_unpack_per_row() {
        let l = |bits, kind| Layout {
            width: 3,
            height: 2,
            kind,
            colour: 1,
            samples: 1,
            alpha: None,
            bits,
            float: false,
            planar: false,
            jpeg: false,
            chunks: 1,
            largest_chunk: 0,
            patch: None,
            icc: None,
        };
        let mut b = Budget::new(&Limits::default(), 3, 2);
        // 1-bit grey: rows 101 and 010, each padded to a byte.
        assert_eq!(unpack(&[0b1010_0000, 0b0100_0000], &l(1, Kind::Gray), 6, &mut b).unwrap(), [255, 0, 255, 0, 255, 0]);
        // 4-bit palette indices: kept as indices.
        assert_eq!(unpack(&[0x12, 0x30, 0xf0, 0x10], &l(4, Kind::Palette(vec![])), 6, &mut b).unwrap(), [1, 2, 3, 15, 0, 1]);
        // 2-bit grey scaled to 0–255.
        assert_eq!(unpack(&[0b0001_1011, 0], &l(2, Kind::Gray), 6, &mut b).unwrap(), [0, 85, 170, 0, 0, 0]);
    }
}
