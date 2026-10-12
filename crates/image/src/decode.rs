//! Decoding: container → [`Decoded`] → colour stage → upright 8-bit sRGB [`Rgba8`].
//!
//! The format module (`formats/`) turns the file into a [`Decoded`] value: the stored pixels at
//! their own depth (8/16-bit integer or float) and colour model (grey, RGB, CMYK; palettes
//! expanded), an optional alpha channel, the embedded ICC profile and the orientation. The colour
//! stage ([`crate::color`]) converts that to 8-bit sRGB, and [`orient`] turns it upright.
//!
//! The header is read and its size checked against [`Limits`] before any decoder is created, so a
//! file declaring a huge image is refused without allocating for it; every pixel buffer after that
//! is charged to one [`Budget`] per decode. Third-party decoders run inside `catch_unwind`: a panic
//! in one is a damaged file ([`Error::Panic`]), never a crash. For thumbnails, `zune-jpeg` has no
//! DCT scaling, so [`decode_thumbnail`] decodes in full and resizes (`resize`), up to a 64 MP
//! source; past that it uses the embedded thumbnail or refuses.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::color;
use crate::formats::{self, DecodeFn};
use crate::limits::Budget;
use crate::{Error, Format, Header, Limits, Orientation, Result, Rgba8, orient, read_header, resize};

/// The largest source [`decode_thumbnail`] decodes in full to make a thumbnail.
pub const THUMBNAIL_SOURCE_MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// How to decode, besides the limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeOptions {
    /// Apply embedded ICC profiles (View → Use Color Management, on by default). Off: every file
    /// is taken as sRGB, and CMYK is converted naively.
    pub color_managed: bool,
}

impl Default for DecodeOptions {
    fn default() -> DecodeOptions {
        DecodeOptions { color_managed: true }
    }
}

/// The colour channels of a [`Decoded`] image, before any alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorModel {
    Gray,
    Rgb,
    /// Ink amounts, 0 = no ink (the ICC convention), whatever the file stored.
    Cmyk,
}

impl ColorModel {
    /// Colour channels, without alpha.
    pub fn channels(self) -> usize {
        match self {
            ColorModel::Gray => 1,
            ColorModel::Rgb => 3,
            ColorModel::Cmyk => 4,
        }
    }
}

/// Interleaved samples, rows top to bottom, no padding.
#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
    /// Nominally 0.0–1.0; values outside are clamped, non-finite ones read as 0.
    F32(Vec<f32>),
}

impl Samples {
    pub fn len(&self) -> usize {
        match self {
            Samples::U8(v) => v.len(),
            Samples::U16(v) => v.len(),
            Samples::F32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An image between its container and the display: the stored pixels (before orientation), as
/// the file holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub model: ColorModel,
    /// A straight (not premultiplied) alpha channel after the colour channels.
    pub alpha: bool,
    pub samples: Samples,
    /// The embedded ICC profile, if the file has one.
    pub icc: Option<Vec<u8>>,
    pub orientation: Orientation,
}

impl Decoded {
    /// Samples per pixel.
    pub fn channels(&self) -> usize {
        self.model.channels() + usize::from(self.alpha)
    }

    /// Checks that the samples fill exactly `width × height × channels`.
    pub fn check(&self) -> Result<()> {
        let want = (self.width as usize).checked_mul(self.height as usize).and_then(|p| p.checked_mul(self.channels()));
        if want != Some(self.samples.len()) || self.width == 0 || self.height == 0 {
            return Err(Error::Malformed(format!("{} samples for {} × {} × {}", self.samples.len(), self.width, self.height, self.channels())));
        }
        Ok(())
    }
}

/// Decodes `bytes` at full size, upright, with colour management on.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8> {
    decode_with(bytes, &DecodeOptions::default(), limits)
}

/// Decodes `bytes` at full size, upright.
pub fn decode_with(bytes: &[u8], options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    let header = read_header(bytes, limits)?;
    let stored = decode_stored(bytes, &header, options, limits)?;
    orient::apply(stored, header.orientation)
}

/// Decodes `bytes` as `format`, without sniffing (the fuzz targets' entry, and a check that
/// sniffing and explicit decoding agree).
pub fn decode_as(format: Format, bytes: &[u8], options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    let header = formats::header(format, bytes, limits)?;
    let stored = decode_stored(bytes, &header, options, limits)?;
    orient::apply(stored, header.orientation)
}

/// Decodes `bytes` to fit in `long_side` pixels on the longer side (never enlarged), upright,
/// with colour management on.
pub fn decode_thumbnail(bytes: &[u8], long_side: u32, limits: &Limits) -> Result<Rgba8> {
    decode_thumbnail_with(bytes, long_side, &DecodeOptions::default(), limits)
}

/// Decodes `bytes` to fit in `long_side` pixels on the longer side (never enlarged), upright.
pub fn decode_thumbnail_with(bytes: &[u8], long_side: u32, options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    let header = read_header(bytes, limits)?;
    // A TIFF's reduced-resolution page covering the target, when it has one (M5.6); a page that
    // fails to decode falls back to the paths below.
    if header.format == Format::Tiff
        && let Ok(Some(t)) = formats::tiff::thumbnail(bytes, long_side, options, limits)
    {
        return Ok(t);
    }
    let pixels = u64::from(header.width) * u64::from(header.height);
    let stored = if pixels > THUMBNAIL_SOURCE_MAX_PIXELS {
        // Too big to decode just for a thumbnail: the camera's own small copy, if there is one,
        // a JPEG or uncompressed strips.
        if let Some(thumb) = header.thumbnail(bytes) {
            let small = read_header(thumb, limits)?;
            decode_stored(thumb, &small, options, limits)?
        } else if let Some(strip) = header.exif.as_ref().and_then(|e| e.strip_thumbnail.as_ref()) {
            crate::exif::strip_thumbnail_rgba(bytes, strip, options, limits)?
        } else {
            return Err(Error::TooLarge { width: header.width, height: header.height });
        }
    } else {
        decode_stored(bytes, &header, options, limits)?
    };
    let fitted = resize::fit(&stored, long_side)?;
    orient::apply(fitted, header.orientation)
}

/// Decodes the stored pixels (before orientation) after checking the declared size.
fn decode_stored(bytes: &[u8], header: &Header, options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    run(formats::decoder(header.format), bytes, header, options, limits)
}

/// Checks the declared size, runs `decoder` inside the panic guard, and hands its [`Decoded`]
/// value to the colour stage, all under one [`Budget`].
fn run(decoder: DecodeFn, bytes: &[u8], header: &Header, options: &DecodeOptions, limits: &Limits) -> Result<Rgba8> {
    limits.check_size(header.width, header.height, 4)?;
    let mut budget = Budget::new(limits, header.width, header.height);
    let decoded = guard(header.format, || decoder(bytes, header, &mut budget, limits))?;
    decoded.check()?;
    if (decoded.width, decoded.height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("decoded {} × {}, header says {} × {}", decoded.width, decoded.height, header.width, header.height)));
    }
    Ok(color::to_srgb8(decoded, options, &mut budget, limits)?.image)
}

/// Runs `f`, turning a panic inside it (a third-party decoder failing on hostile data) into
/// [`Error::Panic`] ("Damaged file").
pub fn guard<T>(format: Format, f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(Error::Panic(format)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planted_panic(_: &[u8], _: &Header, _: &mut Budget, _: &Limits) -> Result<Decoded> {
        panic!("a decoder bug, planted by the test");
    }

    fn header(width: u32, height: u32) -> Header {
        Header { format: Format::Png, width, height, orientation: Orientation::UPRIGHT, exif: None, exif_error: None, thumbnail: None }
    }

    /// A panic inside a decoder is a damaged file, with the "Damaged file" tile text.
    #[test]
    fn a_planted_panic_inside_a_decoder_is_a_damaged_file() {
        let err = run(planted_panic, b"", &header(4, 4), &DecodeOptions::default(), &Limits::default()).unwrap_err();
        assert_eq!(err, Error::Panic(Format::Png));
        assert_eq!(err.reason(), "Damaged file");
    }

    /// A decoder that returns fewer samples than its size says is refused, not read past.
    #[test]
    fn a_short_decode_is_refused() {
        fn short(_: &[u8], _: &Header, _: &mut Budget, _: &Limits) -> Result<Decoded> {
            Ok(Decoded {
                width: 4,
                height: 4,
                model: ColorModel::Rgb,
                alpha: false,
                samples: Samples::U8(vec![0; 47]),
                icc: None,
                orientation: Orientation::UPRIGHT,
            })
        }
        let err = run(short, b"", &header(4, 4), &DecodeOptions::default(), &Limits::default()).unwrap_err();
        assert!(matches!(err, Error::Malformed(_)), "{err:?}");
    }

    /// The decode charges its buffers: with room for the 8-bit output but not for 16-bit samples
    /// as well, a decode is "too large" instead of allocating past the limit.
    #[test]
    fn decoded_samples_and_output_share_one_budget() {
        fn wide(_: &[u8], h: &Header, budget: &mut Budget, _: &Limits) -> Result<Decoded> {
            let samples = budget.vec(16 * 3, 0u16)?;
            Ok(Decoded {
                width: h.width,
                height: h.height,
                model: ColorModel::Rgb,
                alpha: false,
                samples: Samples::U16(samples),
                icc: None,
                orientation: Orientation::UPRIGHT,
            })
        }
        let roomy = Limits { max_alloc: 96 + 64, ..Limits::default() };
        assert!(run(wide, b"", &header(4, 4), &DecodeOptions::default(), &roomy).is_ok());
        let tight = Limits { max_alloc: 96 + 63, ..Limits::default() };
        assert_eq!(run(wide, b"", &header(4, 4), &DecodeOptions::default(), &tight), Err(Error::TooLarge { width: 4, height: 4 }));
    }
}
