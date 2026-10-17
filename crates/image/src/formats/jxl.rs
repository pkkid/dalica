//! JPEG XL (ISO/IEC 18181), bare codestreams and the box container, through `jxl-oxide`.
//!
//! - **Header:** the decoder is fed a growing prefix of the file (16 KiB, doubling) until it has
//!   read the image header, so scanning a large file reads little of it. The size is the size as
//!   displayed: the decoder applies the codestream's orientation itself, so the header and the
//!   decoded pixels are both upright.
//! - **Pixels:** the first keyframe, at 8 bits for samples of up to 8 bits, 16 for up to 16,
//!   floats otherwise; grey or RGB, with alpha when the image has an alpha channel. CMYK images
//!   (a black extra channel) answer `Unsupported`.
//! - **Colour:** pixels come out in the image's own colour encoding; unless that is sRGB, the
//!   profile describing it (the embedded ICC profile, or one the decoder builds from the enumerated
//!   encoding) goes to the colour stage (`color.rs`), so View → Use Color Management applies here as
//!   everywhere else. An enumerated encoding no profile can describe (the XYB or an unknown colour
//!   space, an unknown transfer function, a gamma of 0) answers `Unsupported` before the decoder
//!   is asked for one: it would panic building it.
//! - **Limits:** the declared size is checked by the caller before this module runs. Every buffer
//!   `jxl-oxide` allocates for the image goes through its allocation tracker, whose limit is what
//!   the decode's [`Budget`] has left; running out is "too large", never an abort. The decoder runs
//!   on the calling thread (no `rayon` pool: decodes already run on the thumbnail workers). Its
//!   panics are caught: the decode's by the caller's guard, the header's here.
//! - **Embedded ICC profile:** `jxl-oxide` reserves the encoded profile stream and the profile
//!   (each up to 256 MiB) outside its tracker, as soon as it has read their declared sizes. So
//!   before the decoder sees the file, [`declared_icc`] reads those sizes with the decoder's own
//!   parsers (the image header, the stream's `U64` length, and the first entropy-coded bytes that
//!   hold the profile's length): a profile over `Limits::max_icc`, or a stream over it plus the
//!   64 KiB slack the decoder itself allows, is "too large". A smaller one is charged: against the
//!   header's 64 MiB allowance while the header is read, and to the decode's [`Budget`] (the stream
//!   and two copies of the profile: the decoder's and the colour stage's) while it decodes.

use std::ops::Range;

use jxl_bitstream::{Bitstream, ContainerParser, ParseEvent};
use jxl_oxide::color::{ColourEncoding, ColourSpace, TransferFunction, WhitePoint};
use jxl_oxide::image::BitDepth;
use jxl_oxide::{AllocTracker, EnumColourEncoding, ImageHeader, InitializeResult, JxlImage, PixelFormat};
use jxl_oxide_common::Bundle;

use crate::decode::{ColorModel, Decoded, Samples, guard};
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes JPEG XL files.
pub const DECODES: bool = true;

/// The tile text for a JPEG XL file the decoder refuses (CMYK images, colour encodings no profile
/// describes).
pub const REASON: &str = "This kind of JPEG XL file isn't supported";

/// The first prefix fed to the decoder to read the header.
const FIRST_CHUNK: usize = 16 * 1024;
/// What the decoder may allocate while reading only the header: through its tracker, plus the
/// embedded ICC profile it reserves outside it.
const HEADER_ALLOC: u64 = 64 * 1024 * 1024;
/// How much longer than the profile its encoded stream may be: the decoder refuses a stream more
/// than this longer than the profile it declares.
const ICC_STREAM_SLACK: u64 = 64 * 1024;
/// The entropy-coded ICC stream's distributions (ISO/IEC 18181-1 §E.4: 41 contexts).
const ICC_CONTEXTS: u32 = 41;
/// The first bytes of the ICC stream, which hold the profile's length (the decoder reads as many
/// before it reserves the stream).
const ICC_SIZE_BYTES: usize = 18;

/// An embedded ICC profile as the codestream declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Icc {
    /// No embedded profile, or a codestream the decoder stops reading (damaged or cut short)
    /// before it reserves one.
    None,
    /// The encoded stream's length and the profile's (0 when the stream alone is over the limit,
    /// so the profile's length was not read).
    Declared { stream: u64, profile: u64 },
}

/// The ICC sizes `cs` (the start of a codestream) declares, or `None` when it ends before they
/// can be told: the image header, then (when the colour encoding asks for a profile) the stream's
/// length and its first entropy-coded bytes, all read by the decoder's own parsers.
fn icc_in(cs: &[u8], max_stream: u64) -> Option<Icc> {
    // Out of data: wait for more. Any other error: the decoder refuses the file at the same point.
    let stop = |eof: bool| if eof { None } else { Some(Icc::None) };
    let mut bits = Bitstream::new(cs);
    let header = match ImageHeader::parse(&mut bits, ()) {
        Ok(header) => header,
        Err(e) => return stop(e.unexpected_eof()),
    };
    if !header.metadata.colour_encoding.want_icc() {
        return Some(Icc::None);
    }
    let stream = match bits.read_u64() {
        Ok(stream) => stream,
        Err(e) => return stop(e.unexpected_eof()),
    };
    if stream > max_stream {
        return Some(Icc::Declared { stream, profile: 0 });
    }
    let mut decoder = match jxl_coding::Decoder::parse(&mut bits, ICC_CONTEXTS) {
        Ok(decoder) => decoder,
        Err(e) => return stop(e.unexpected_eof()),
    };
    if let Err(e) = decoder.begin(&mut bits) {
        return stop(e.unexpected_eof());
    }
    let n = usize::try_from(stream).unwrap_or(usize::MAX).min(ICC_SIZE_BYTES);
    let mut head = [0u8; ICC_SIZE_BYTES];
    for byte in head.iter_mut().take(n) {
        // Context 0: every byte this early in the stream (§E.4.2).
        match decoder.read_varint(&mut bits, 0).map(u8::try_from) {
            Ok(Ok(v)) => *byte = v,
            Ok(Err(_)) => return Some(Icc::None),
            Err(e) => return stop(e.unexpected_eof()),
        }
    }
    match varint(head.get(..n).unwrap_or_default()) {
        Some(profile) => Some(Icc::Declared { stream, profile }),
        None => Some(Icc::None),
    }
}

/// The profile's length at the start of the ICC stream: a little-endian base-128 varint of at
/// most nine bytes, read the way the decoder reads it. `None` when the bytes end inside it.
fn varint(bytes: &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().take(9).enumerate() {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 || i == 8 {
            return Some(value);
        }
    }
    None
}

/// The ICC sizes `b` (a bare codestream or a container) declares. Container boxes are joined only
/// as far as needed, re-read each time the joined part has doubled, so a file of many tiny boxes
/// costs linear time.
fn declared_icc(b: &[u8], limits: &Limits) -> Result<Icc> {
    let max_stream = (limits.max_icc as u64).saturating_add(ICC_STREAM_SLACK);
    let mut parser = ContainerParser::new();
    let mut first: Option<&[u8]> = None;
    let mut joined: Vec<u8> = Vec::new();
    let mut tried = 0usize;
    for event in parser.feed_bytes(b) {
        // A container error stops the decoder too, before the codestream.
        let Ok(event) = event else { break };
        let ParseEvent::Codestream(part) = event else { continue };
        if first.is_none() && joined.is_empty() {
            // The common case, one part (a bare codestream or one `jxlc` box): read in place.
            if let Some(icc) = icc_in(part, max_stream) {
                return Ok(icc);
            }
            first = Some(part);
            tried = part.len();
            continue;
        }
        for piece in [first.take().unwrap_or_default(), part] {
            joined.try_reserve(piece.len()).map_err(|_| Error::TooLarge { width: 0, height: 0 })?;
            joined.extend_from_slice(piece);
        }
        if joined.len() >= tried.saturating_mul(2) {
            tried = joined.len();
            if let Some(icc) = icc_in(&joined, max_stream) {
                return Ok(icc);
            }
        }
    }
    if joined.len() > tried
        && let Some(icc) = icc_in(&joined, max_stream)
    {
        return Ok(icc);
    }
    // Cut short before the sizes: the decoder stops there too.
    Ok(Icc::None)
}

/// The bytes `jxl-oxide` reserves outside its tracker for `b`'s embedded profile while it reads
/// the header (the stream and the profile; 0 without one). A profile over `max_icc`, or a stream
/// over it plus the decoder's slack, is too large before anything is reserved for it.
fn icc_reserve(b: &[u8], limits: &Limits) -> Result<(u64, u64)> {
    match declared_icc(b, limits)? {
        Icc::None => Ok((0, 0)),
        Icc::Declared { stream, profile }
            if stream <= (limits.max_icc as u64).saturating_add(ICC_STREAM_SLACK) && profile <= limits.max_icc as u64 =>
        {
            Ok((stream, profile))
        }
        Icc::Declared { stream, profile } => {
            log::debug!("JPEG XL ICC profile of {profile} bytes in a {stream}-byte stream: over the {}-byte limit", limits.max_icc);
            Err(Error::TooLarge { width: 0, height: 0 })
        }
    }
}

/// Reads the image header (and the ICC profile and preview frame header that follow it) from the
/// shortest prefix of `b` that holds it.
fn open_header(b: &[u8], limits: &Limits) -> Result<JxlImage> {
    let cap = usize::try_from(limits.max_header_bytes).unwrap_or(usize::MAX).min(b.len());
    // The profile comes out of the header's allowance; the tracker gets the rest.
    let (stream, profile) = icc_reserve(b.get(..cap).ok_or(Error::Truncated)?, limits)?;
    let tracked = HEADER_ALLOC.checked_sub(stream.saturating_add(profile)).ok_or(Error::TooLarge { width: 0, height: 0 })?;
    let tracker = AllocTracker::with_limit(usize::try_from(tracked).unwrap_or(usize::MAX));
    let mut uninit = JxlImage::builder().alloc_tracker(tracker).build_uninit();
    let (mut pos, mut chunk) = (0usize, FIRST_CHUNK);
    loop {
        let end = pos.saturating_add(chunk).min(cap);
        let fed = b.get(pos..end).ok_or(Error::Truncated)?;
        pos += uninit.feed_bytes(fed).map_err(|e| jxl_err(&*e, 0, 0))?.min(fed.len());
        match uninit.try_init().map_err(|e| jxl_err(&*e, 0, 0))? {
            InitializeResult::Initialized(image) => return Ok(image),
            InitializeResult::NeedMoreData(more) if end < cap => {
                uninit = more;
                chunk = chunk.saturating_mul(2);
            }
            InitializeResult::NeedMoreData(_) => return Err(Error::Truncated),
        }
    }
}

/// The size as displayed (the decoder turns the pixels upright) without decoding pixels.
pub fn header(b: &[u8], limits: &Limits) -> Result<Header> {
    let image = guard(Format::JpegXl, || open_header(b, limits))?;
    Ok(Header {
        format: Format::JpegXl,
        width: image.width(),
        height: image.height(),
        orientation: Orientation::UPRIGHT,
        exif: None,
        exif_error: None,
        thumbnail: None,
    })
}

/// JPEG XL files carry no thumbnail Dalica uses (a preview frame is JPEG XL too, decoded with the
/// image).
pub fn embedded_thumbnail(b: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    header(b, limits).map(|_| None)
}

pub fn decode(b: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let (width, height) = (header.width, header.height);
    // The embedded profile is reserved outside the tracker: the stream and the profile while the
    // header is read, then the decoder's profile and the copy handed to the colour stage.
    let (stream, profile) = icc_reserve(b, limits)?;
    let reserved = stream.saturating_add(profile.saturating_mul(2));
    budget.charge(reserved)?;
    // Everything the budget has left is lent to the decoder's tracker for the image's buffers;
    // what it does not use is given back once the samples are copied out.
    let lent = budget.left();
    budget.charge(lent)?;
    let tracker = AllocTracker::with_limit(usize::try_from(lent).unwrap_or(usize::MAX));
    // The decoder and its buffers are gone when `render` returns; the samples it made stay.
    let result = render(b, width, height, &tracker, budget);
    budget.refund(lent);
    budget.refund(reserved);
    let (decoded, samples_bytes) = result?;
    budget.charge(samples_bytes)?;
    Ok(decoded)
}

/// Decodes the first keyframe into samples taken from what the tracker has not used. Returns the
/// image and the bytes its samples hold (charged by the caller once the decoder's own buffers are
/// gone).
fn render(b: &[u8], width: u32, height: u32, tracker: &AllocTracker, budget: &mut Budget) -> Result<(Decoded, u64)> {
    let image = JxlImage::builder().alloc_tracker(tracker.clone()).read(b).map_err(|e| jxl_err(&*e, width, height))?;
    if (image.width(), image.height()) != (width, height) {
        return Err(Error::Malformed(format!("JPEG XL {} × {}, header says {width} × {height}", image.width(), image.height())));
    }
    if image.num_loaded_keyframes() == 0 {
        return Err(Error::Truncated);
    }
    let format = image.pixel_format();
    let (model, alpha) = match format {
        PixelFormat::Gray => (ColorModel::Gray, false),
        PixelFormat::Graya => (ColorModel::Gray, true),
        PixelFormat::Rgb => (ColorModel::Rgb, false),
        PixelFormat::Rgba => (ColorModel::Rgb, true),
        PixelFormat::Cmyk | PixelFormat::Cmyka => return Err(Error::Unsupported(Format::JpegXl)),
    };
    let icc = profile(&image)?;
    let bits = image.image_header().metadata.bit_depth;
    let frame = image.render_frame(0).map_err(|e| jxl_err(&*e, width, height))?;
    let mut stream = frame.stream();
    let channels = model.channels() + usize::from(alpha);
    if (stream.width(), stream.height(), stream.channels() as usize) != (width, height, channels) {
        return Err(Error::Malformed(format!("JPEG XL frame of {} × {} × {}", stream.width(), stream.height(), stream.channels())));
    }
    let len = (width as usize).checked_mul(height as usize).and_then(|p| p.checked_mul(channels)).ok_or(Error::TooLarge { width, height })?;
    // Room for the samples comes out of what the decoder has not used.
    let (samples, bytes) = match bits {
        BitDepth::IntegerSample { bits_per_sample: 0..=8 } => {
            let mut v = take(tracker, budget, len, 0u8, width, height)?;
            fill(stream.write_to_buffer(&mut v), len)?;
            (Samples::U8(v), len as u64)
        }
        BitDepth::IntegerSample { bits_per_sample: 9..=16 } => {
            let mut v = take(tracker, budget, len, 0u16, width, height)?;
            fill(stream.write_to_buffer(&mut v), len)?;
            (Samples::U16(v), len as u64 * 2)
        }
        _ => {
            let mut v = take(tracker, budget, len, 0f32, width, height)?;
            fill(stream.write_to_buffer(&mut v), len)?;
            (Samples::F32(v), len as u64 * 4)
        }
    };
    Ok((Decoded { width, height, model, alpha, samples, icc, orientation: Orientation::UPRIGHT }, bytes))
}

/// A buffer of `len` samples, taken out of the tracker's unused limit: that part of the loan goes
/// back to the budget, which allocates the buffer fallibly.
fn take<T: Clone>(tracker: &AllocTracker, budget: &mut Budget, len: usize, fill: T, width: u32, height: u32) -> Result<Vec<T>> {
    let bytes = len.checked_mul(std::mem::size_of::<T>()).ok_or(Error::TooLarge { width, height })?;
    tracker.shrink_limit(bytes).map_err(|_| Error::TooLarge { width, height })?;
    budget.refund(bytes as u64);
    match budget.vec(len, fill) {
        Ok(v) => Ok(v),
        Err(e) => {
            // Keep the loan whole, so the caller's refund is exact.
            budget.charge(bytes as u64)?;
            Err(e)
        }
    }
}

fn fill(written: usize, len: usize) -> Result<()> {
    if written != len {
        return Err(Error::Malformed(format!("JPEG XL wrote {written} samples, expected {len}")));
    }
    Ok(())
}

/// The profile of the rendered pixels, or `None` when they are sRGB (RGB or grey with the sRGB
/// curve and a D65 white), which the colour stage takes untagged pixels to be. An embedded profile
/// is handed on as it is when the pixels are stored in its space (not XYB): the decoder would
/// otherwise describe them with an encoding it guessed from the profile, which can be wrong (its
/// guess for a Display P3 profile with chromatic adaptation has other primaries). An enumerated
/// encoding the decoder cannot build a profile for is `Unsupported`.
fn profile(image: &JxlImage) -> Result<Option<Vec<u8>>> {
    let metadata = &image.image_header().metadata;
    if let (ColourEncoding::IccProfile(_), false, Some(icc)) = (&metadata.colour_encoding, metadata.xyb_encoded, image.original_icc()) {
        return Ok(Some(icc.to_vec()));
    }
    match &metadata.colour_encoding {
        ColourEncoding::Enum(e) if !describable(e) => {
            log::debug!("JPEG XL colour encoding no profile describes: {e:?}");
            Err(Error::Unsupported(Format::JpegXl))
        }
        ColourEncoding::Enum(e) if e.is_srgb() => Ok(None),
        ColourEncoding::Enum(e)
            if e.colour_space == ColourSpace::Grey && matches!(e.white_point, WhitePoint::D65) && matches!(e.tf, TransferFunction::Srgb) =>
        {
            Ok(None)
        }
        // The rendered encoding is the enumerated one above, or one the decoder parsed from the
        // embedded profile (always RGB or grey with a known curve).
        _ => Ok(Some(image.rendered_icc())),
    }
}

/// Whether `jxl-color` can build a profile for an enumerated encoding: it panics on the XYB and
/// unknown colour spaces, an unknown transfer function and a gamma of 0 (a division by it), all of
/// which a hostile header may declare.
fn describable(e: &EnumColourEncoding) -> bool {
    matches!(e.colour_space, ColourSpace::Rgb | ColourSpace::Grey)
        && !matches!(e.tf, TransferFunction::Unknown | TransferFunction::Gamma { g: 0, .. })
}

/// A `jxl-oxide` error as ours: an exhausted allocation tracker is "too large", an early end of
/// the data is truncation, anything else is damage.
fn jxl_err(e: &(dyn std::error::Error + 'static), width: u32, height: u32) -> Error {
    let mut cause = Some(e);
    while let Some(err) = cause {
        if err.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::UnexpectedEof) {
            return Error::Truncated;
        }
        // `jxl_grid::OutOfMemory`, which `jxl-oxide` does not re-export.
        if err.to_string().starts_with("failed to allocate") {
            return Error::TooLarge { width, height };
        }
        cause = err.source();
    }
    Error::Malformed(format!("JPEG XL: {e}"))
}
