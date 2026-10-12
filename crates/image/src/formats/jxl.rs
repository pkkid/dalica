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
//!   everywhere else.
//! - **Limits:** the declared size is checked by the caller before this module runs. Every buffer
//!   `jxl-oxide` allocates for the image goes through its allocation tracker, whose limit is what
//!   the decode's [`Budget`] has left; running out is "too large", never an abort. The decoder runs
//!   on the calling thread (no `rayon` pool: decodes already run on the thumbnail workers). Its
//!   panics are caught: the decode's by the caller's guard, the header's here.

use std::ops::Range;

use jxl_oxide::color::{ColourEncoding, ColourSpace, TransferFunction, WhitePoint};
use jxl_oxide::image::BitDepth;
use jxl_oxide::{AllocTracker, InitializeResult, JxlImage, PixelFormat};

use crate::decode::{ColorModel, Decoded, Samples, guard};
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build decodes JPEG XL files.
pub const DECODES: bool = true;

/// The tile text for a JPEG XL file the decoder refuses (CMYK images).
pub const REASON: &str = "This kind of JPEG XL file isn't supported";

/// The first prefix fed to the decoder to read the header.
const FIRST_CHUNK: usize = 16 * 1024;
/// What the decoder may allocate through its tracker while reading only the header.
const HEADER_ALLOC: usize = 64 * 1024 * 1024;

/// Reads the image header (and the ICC profile and preview frame header that follow it) from the
/// shortest prefix of `b` that holds it.
fn open_header(b: &[u8], limits: &Limits) -> Result<JxlImage> {
    let mut uninit = JxlImage::builder().alloc_tracker(AllocTracker::with_limit(HEADER_ALLOC)).build_uninit();
    let cap = usize::try_from(limits.max_header_bytes).unwrap_or(usize::MAX).min(b.len());
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

pub fn decode(b: &[u8], header: &Header, budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    let (width, height) = (header.width, header.height);
    // Everything the budget has left is lent to the decoder's tracker for the image's buffers;
    // what it does not use is given back once the samples are copied out.
    let lent = budget.left();
    budget.charge(lent)?;
    let tracker = AllocTracker::with_limit(usize::try_from(lent).unwrap_or(usize::MAX));
    // The decoder and its buffers are gone when `render` returns; the samples it made stay.
    let result = render(b, width, height, &tracker, budget);
    budget.refund(lent);
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
    let icc = profile(&image);
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
/// curve and a D65 white), which the colour stage takes untagged pixels to be.
fn profile(image: &JxlImage) -> Option<Vec<u8>> {
    match &image.image_header().metadata.colour_encoding {
        ColourEncoding::Enum(e) if e.is_srgb() => None,
        ColourEncoding::Enum(e)
            if e.colour_space == ColourSpace::Grey && matches!(e.white_point, WhitePoint::D65) && matches!(e.tf, TransferFunction::Srgb) =>
        {
            None
        }
        _ => Some(image.rendered_icc()),
    }
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
