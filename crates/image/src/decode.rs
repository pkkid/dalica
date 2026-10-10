//! Decoding JPEG and PNG to upright 8-bit RGBA (`zune-jpeg`, `png`). Colour profiles are not yet
//! applied: pixels are taken as sRGB (ICC arrives in M5).
//!
//! The header is read and its size checked against [`Limits`] before any decoder is created, so a
//! file declaring a huge image is refused without allocating for it. For thumbnails, `zune-jpeg`
//! has no DCT scaling, so [`decode_thumbnail`] decodes in full and resizes (`resize`), up to a
//! 64 MP source; past that it uses the embedded EXIF thumbnail or refuses.

use std::io::Cursor;

use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::{Error, Format, Header, Limits, Result, Rgba8, orient, read_header, resize};

/// The largest source [`decode_thumbnail`] decodes in full to make a thumbnail.
pub const THUMBNAIL_SOURCE_MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Decodes `bytes` at full size, upright.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8> {
    let header = read_header(bytes, limits)?;
    let stored = decode_stored(bytes, &header, limits)?;
    orient::apply(stored, header.orientation)
}

/// Decodes `bytes` to fit in `long_side` pixels on the longer side (never enlarged), upright.
pub fn decode_thumbnail(bytes: &[u8], long_side: u32, limits: &Limits) -> Result<Rgba8> {
    let header = read_header(bytes, limits)?;
    let pixels = u64::from(header.width) * u64::from(header.height);
    let stored = if pixels > THUMBNAIL_SOURCE_MAX_PIXELS {
        // Too big to decode just for a thumbnail: the camera's own small copy, if there is one.
        let thumb = header.thumbnail(bytes).ok_or(Error::TooLarge { width: header.width, height: header.height })?;
        let small = read_header(thumb, limits)?;
        decode_stored(thumb, &small, limits)?
    } else {
        decode_stored(bytes, &header, limits)?
    };
    let fitted = resize::fit(&stored, long_side)?;
    orient::apply(fitted, header.orientation)
}

/// Decodes the stored pixels (before orientation) after checking the declared size.
fn decode_stored(bytes: &[u8], header: &Header, limits: &Limits) -> Result<Rgba8> {
    limits.check_size(header.width, header.height, 4)?;
    let img = match header.format {
        Format::Jpeg => decode_jpeg(bytes, limits)?,
        Format::Png => decode_png(bytes, limits)?,
        other => return Err(Error::Unsupported(other)),
    };
    if (img.width, img.height) != (header.width, header.height) {
        return Err(Error::Malformed(format!("decoded {} × {}, header says {} × {}", img.width, img.height, header.width, header.height)));
    }
    Ok(img)
}

fn decode_jpeg(bytes: &[u8], limits: &Limits) -> Result<Rgba8> {
    let side = limits.max_side as usize;
    let options = DecoderOptions::default().set_max_width(side).set_max_height(side).jpeg_set_out_colorspace(ColorSpace::RGBA).set_strict_mode(false);
    let mut d = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    d.decode_headers().map_err(|e| Error::Malformed(format!("JPEG: {e:?}")))?;
    let info = d.info().ok_or(Error::Truncated)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let len = limits.check_size(width, height, 4)?;
    let mut data = vec![0u8; len];
    d.decode_into(&mut data).map_err(|e| Error::Malformed(format!("JPEG: {e:?}")))?;
    Ok(Rgba8 { width, height, data })
}

fn decode_png(bytes: &[u8], limits: &Limits) -> Result<Rgba8> {
    let budget = usize::try_from(limits.max_alloc).unwrap_or(usize::MAX);
    let mut d = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: budget });
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = d.read_info().map_err(png_err)?;
    let (width, height) = (reader.info().width, reader.info().height);
    limits.check_size(width, height, 4)?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or(Error::TooLarge { width, height })?];
    let frame = reader.next_frame(&mut buf).map_err(png_err)?;
    let pixels = buf.get(..frame.buffer_size()).ok_or(Error::Truncated)?;
    let data: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.as_chunks::<2>().0.iter().flat_map(|&[v, a]| [v, v, v, a]).collect(),
        png::ColorType::Indexed => return Err(Error::Malformed("PNG palette was not expanded".into())),
    };
    Ok(Rgba8 { width, height, data })
}

fn png_err(e: png::DecodingError) -> Error {
    match e {
        png::DecodingError::LimitsExceeded => Error::TooLarge { width: 0, height: 0 },
        other => Error::Malformed(format!("PNG: {other}")),
    }
}
