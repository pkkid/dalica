//! Formats written through ready-made encoders: WebP (lossless), GIF and BMP from the pinned
//! `image` crate, JPEG with an ICC profile from `image`'s JPEG encoder, and lossless JPEG XL from
//! `zune-jpegxl`. All of them are deterministic.

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::webp::WebPEncoder;
use image::{Delay, ExtendedColorType, Frame, ImageEncoder, RgbaImage};
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

use crate::exif::Exif;
use crate::formats::{Colour, Depth, Error, Result, SampleData, alpha, encode_jpeg, palette, palette_index, quantized, samples, stored};
use crate::render::Scene;

/// The most frames [`gif`] writes.
pub const MAX_FRAMES: u16 = 64;

fn encode_err(format: &'static str) -> impl Fn(image::ImageError) -> Error {
    move |e| Error::Encode { format, message: e.to_string() }
}

fn unsupported(format: &'static str, what: &str) -> Error {
    Error::Unsupported { format, what: what.to_owned() }
}

/// What a WebP holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Webp {
    /// An alpha channel ([`alpha`](crate::formats::alpha)).
    pub alpha: bool,
    /// An EXIF chunk; the pixels are stored for its orientation.
    pub exif: Option<Exif>,
    /// An ICCP chunk.
    pub icc: Option<Vec<u8>>,
}

/// A lossless WebP of `scene` (the only kind `image` encodes).
pub fn webp(scene: &Scene, opts: &Webp) -> Result<Vec<u8>> {
    let (img, block) = stored(scene, opts.exif.as_ref())?;
    let (data, colour) = if opts.alpha {
        let a = alpha(img.width, img.height);
        (img.data.as_chunks::<3>().0.iter().zip(a).flat_map(|(c, a)| [c[0], c[1], c[2], a]).collect(), ExtendedColorType::Rgba8)
    } else {
        (img.data.clone(), ExtendedColorType::Rgb8)
    };
    let mut out = Vec::new();
    let mut enc = WebPEncoder::new_lossless(&mut out);
    if let Some(icc) = &opts.icc {
        enc.set_icc_profile(icc.clone()).map_err(|e| Error::Encode { format: "WebP", message: e.to_string() })?;
    }
    if let Some(block) = block {
        enc.set_exif_metadata(block).map_err(|e| Error::Encode { format: "WebP", message: e.to_string() })?;
    }
    enc.write_image(&data, img.width, img.height, colour).map_err(encode_err("WebP"))?;
    Ok(out)
}

/// A GIF of `frames` frames (1 to [`MAX_FRAMES`]): frame `k` is the scene with seed `seed + k`,
/// in the fixed [`palette`] (so every colour survives exactly), shown for 100 ms, looping.
pub fn gif(scene: &Scene, frames: u16) -> Result<Vec<u8>> {
    if !(1..=MAX_FRAMES).contains(&frames) {
        return Err(unsupported("GIF", &format!("{frames} frames (1 to {MAX_FRAMES})")));
    }
    let mut list = Vec::with_capacity(usize::from(frames));
    for k in 0..frames {
        let img = quantized(&Scene { seed: scene.seed.wrapping_add(u64::from(k)), ..*scene }.render()?);
        let rgba = img.data.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect();
        let buffer = RgbaImage::from_raw(img.width, img.height, rgba).ok_or_else(|| unsupported("GIF", "a frame of the wrong size"))?;
        list.push(Frame::from_parts(buffer, 0, 0, Delay::from_numer_denom_ms(100, 1)));
    }
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        if frames > 1 {
            enc.set_repeat(Repeat::Infinite).map_err(encode_err("GIF"))?;
        }
        enc.encode_frames(list).map_err(encode_err("GIF"))?;
    }
    Ok(out)
}

/// A BMP of `scene`: 24-bit RGB, 8-bit grey, or 8-bit indexed in the fixed [`palette`].
pub fn bmp(scene: &Scene, colour: Colour) -> Result<Vec<u8>> {
    let img = scene.render()?;
    let mut out = Vec::new();
    let mut enc = BmpEncoder::new(&mut out);
    match colour {
        Colour::Rgb => enc.encode(&img.data, img.width, img.height, ExtendedColorType::Rgb8),
        Colour::Grey => {
            let SampleData::U8(grey) = samples(&img, Colour::Grey, Depth::Eight)?.data else {
                return Err(unsupported("BMP", "grey samples"));
            };
            enc.encode(&grey, img.width, img.height, ExtendedColorType::L8)
        }
        Colour::Palette => {
            let indices: Vec<u8> = img.data.as_chunks::<3>().0.iter().map(|c| palette_index(*c)).collect();
            enc.encode_with_palette(&indices, img.width, img.height, ExtendedColorType::L8, Some(&palette()))
        }
        Colour::Cmyk => return Err(unsupported("BMP", "CMYK")),
    }
    .map_err(encode_err("BMP"))?;
    Ok(out)
}

/// A baseline JPEG of `scene` with `icc` in APP2 (split over several segments when large) and an
/// optional EXIF block, as [`crate::jpeg`] writes it.
pub fn jpeg_icc(scene: &Scene, quality: u8, exif: Option<&Exif>, icc: &[u8]) -> Result<Vec<u8>> {
    let (img, block) = stored(scene, exif)?;
    encode_jpeg(&img, quality, block.as_deref(), Some(icc))
}

/// A lossless JPEG XL codestream of `scene`: grey or RGB, 8 or 16 bits; 8-bit files may have an
/// alpha channel ([`alpha`](crate::formats::alpha)). At least 2 × 2 pixels.
pub fn jxl(scene: &Scene, colour: Colour, depth: Depth, with_alpha: bool) -> Result<Vec<u8>> {
    if scene.width < 2 || scene.height < 2 {
        return Err(unsupported("JPEG XL", "images under 2 × 2 pixels (the encoder's minimum)"));
    }
    let img = scene.render()?;
    let s = samples(&img, colour, depth)?;
    let a = if with_alpha { Some(alpha(img.width, img.height)) } else { None };
    let space = match (colour, with_alpha) {
        (Colour::Grey, false) => ColorSpace::Luma,
        (Colour::Grey, true) => ColorSpace::LumaA,
        (Colour::Rgb, false) => ColorSpace::RGB,
        (Colour::Rgb, true) => ColorSpace::RGBA,
        (Colour::Cmyk | Colour::Palette, _) => return Err(unsupported("JPEG XL", "CMYK or palette samples")),
    };
    let channels = colour.channels();
    let (bytes, bit_depth) = match (&s.data, &a) {
        (SampleData::U8(v), None) => (v.clone(), BitDepth::Eight),
        (SampleData::U8(v), Some(a)) => {
            (v.chunks_exact(channels).zip(a).flat_map(|(px, &al)| px.iter().copied().chain([al])).collect(), BitDepth::Eight)
        }
        (SampleData::U16(v), None) => (v.iter().flat_map(|x| x.to_ne_bytes()).collect(), BitDepth::Sixteen),
        // `zune-jpegxl` 0.5 declares every alpha channel 8-bit and loses high 16-bit samples next
        // to one, so 16-bit files have no alpha.
        (SampleData::U16(_), Some(_)) => return Err(unsupported("JPEG XL", "16-bit samples with alpha")),
        (SampleData::F32(_), _) => return Err(unsupported("JPEG XL", "float samples")),
    };
    let options = EncoderOptions::new(img.width as usize, img.height as usize, space, bit_depth);
    let mut out = Vec::new();
    JxlSimpleEncoder::new(&bytes, options).encode(&mut out).map_err(|e| Error::Encode { format: "JPEG XL", message: format!("{e:?}") })?;
    Ok(out)
}
