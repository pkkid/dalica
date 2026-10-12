//! Our own writer of a flattened Photoshop document (PSD, version 1), from Adobe's public file
//! format specification: the header, colour mode data (the palette of indexed images), image
//! resources (1036 thumbnail, 1039 ICC profile), an empty layer and mask section, and the merged
//! composite, planar, raw or PackBits-compressed per row. Everything is big-endian. As Photoshop
//! does, CMYK samples are stored inverted (255 or 65 535 means no ink); 32-bit documents hold IEEE
//! floats (grey and RGB only).

use crate::formats::{Colour, Depth, Error, Result, SampleData, encode_jpeg, pack, palette, samples};
use crate::render::Scene;

/// The largest width or height of a PSD (larger documents are PSB).
pub const MAX_SIDE: u32 = 30_000;

pub const RESOURCE_THUMBNAIL: u16 = 1036;
pub const RESOURCE_ICC: u16 = 1039;

/// What the document holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Psd {
    /// Grey, RGB, CMYK or indexed (`Palette`).
    pub colour: Colour,
    /// 8 or 16 bits, or 32-bit float for grey and RGB (indexed: 8).
    pub depth: Depth,
    /// PackBits rows (compression 1) instead of raw samples (0).
    pub rle: bool,
    /// A JPEG thumbnail resource this many pixels on its longer side.
    pub thumbnail: Option<u32>,
    pub icc: Option<Vec<u8>>,
}

impl Default for Psd {
    fn default() -> Psd {
        Psd { colour: Colour::Rgb, depth: Depth::Eight, rle: false, thumbnail: None, icc: None }
    }
}

fn resource(out: &mut Vec<u8>, id: u16, data: &[u8]) -> Result<()> {
    out.extend(*b"8BIM");
    out.extend(id.to_be_bytes());
    out.extend([0, 0]); // an empty Pascal name, padded to even
    out.extend(len32(data.len())?.to_be_bytes());
    out.extend(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    Ok(())
}

fn len32(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| Error::Encode { format: "PSD", message: "a section passes 4 GiB".into() })
}

/// A PSD of `scene` as `opts` describes (upright; PSD has no orientation).
pub fn psd(scene: &Scene, opts: &Psd) -> Result<Vec<u8>> {
    if scene.width > MAX_SIDE || scene.height > MAX_SIDE {
        return Err(Error::TooLarge { format: "PSD", max: MAX_SIDE, width: scene.width, height: scene.height });
    }
    let img = scene.render()?;
    let s = samples(&img, opts.colour, opts.depth)?;
    let channels = opts.colour.channels();
    let (width, height) = (s.width as usize, s.height as usize);

    let mut out = Vec::new();
    out.extend(*b"8BPS");
    out.extend(1u16.to_be_bytes());
    out.extend([0; 6]);
    out.extend((channels as u16).to_be_bytes());
    out.extend(s.height.to_be_bytes());
    out.extend(s.width.to_be_bytes());
    out.extend(((opts.depth.bytes() * 8) as u16).to_be_bytes());
    let mode: u16 = match opts.colour {
        Colour::Grey => 1,
        Colour::Palette => 2,
        Colour::Rgb => 3,
        Colour::Cmyk => 4,
    };
    out.extend(mode.to_be_bytes());

    // Colour mode data: 256 reds, 256 greens, 256 blues for indexed images.
    if opts.colour == Colour::Palette {
        let p = palette();
        out.extend(768u32.to_be_bytes());
        (0..3).for_each(|c| out.extend(p.iter().map(|rgb| rgb[c])));
    } else {
        out.extend(0u32.to_be_bytes());
    }

    let mut resources = Vec::new();
    if let Some(long_side) = opts.thumbnail {
        let small = img.downscale(long_side)?;
        let jpeg = encode_jpeg(&small, 80, None, None)?;
        let width_bytes = (small.width * 24).div_ceil(32) * 4;
        let mut d = Vec::with_capacity(28 + jpeg.len());
        d.extend(1u32.to_be_bytes()); // kJpegRGB
        d.extend(small.width.to_be_bytes());
        d.extend(small.height.to_be_bytes());
        d.extend(width_bytes.to_be_bytes());
        d.extend(width_bytes.saturating_mul(small.height).to_be_bytes());
        d.extend(len32(jpeg.len())?.to_be_bytes());
        d.extend(24u16.to_be_bytes());
        d.extend(1u16.to_be_bytes());
        d.extend(jpeg);
        resource(&mut resources, RESOURCE_THUMBNAIL, &d)?;
    }
    if let Some(icc) = &opts.icc {
        resource(&mut resources, RESOURCE_ICC, icc)?;
    }
    out.extend(len32(resources.len())?.to_be_bytes());
    out.extend(resources);
    out.extend(0u32.to_be_bytes()); // no layer and mask information

    // The composite, one plane per channel, big-endian samples.
    let row_bytes = width * opts.depth.bytes();
    let invert = opts.colour == Colour::Cmyk;
    let mut planes: Vec<Vec<u8>> = vec![Vec::with_capacity(row_bytes * height); channels];
    match &s.data {
        SampleData::U8(v) => {
            for (i, &b) in v.iter().enumerate() {
                if let Some(p) = planes.get_mut(i % channels) {
                    p.push(if invert { 255 - b } else { b });
                }
            }
        }
        SampleData::U16(v) => {
            for (i, &b) in v.iter().enumerate() {
                if let Some(p) = planes.get_mut(i % channels) {
                    p.extend((if invert { 65_535 - b } else { b }).to_be_bytes());
                }
            }
        }
        SampleData::F32(v) => {
            for (i, &f) in v.iter().enumerate() {
                if let Some(p) = planes.get_mut(i % channels) {
                    p.extend(f.to_bits().to_be_bytes());
                }
            }
        }
    }
    if opts.rle {
        out.extend(1u16.to_be_bytes());
        let rows: Vec<Vec<u8>> = planes.iter().flat_map(|p| p.chunks(row_bytes.max(1)).map(pack::packbits)).collect();
        for r in &rows {
            // Rows of 8- and 16-bit documents always fit (30 000 × 2 bytes pack to at most 60 469).
            let n = u16::try_from(r.len()).map_err(|_| Error::Encode { format: "PSD", message: "a packed row passes 65 535 bytes".into() })?;
            out.extend(n.to_be_bytes());
        }
        rows.iter().for_each(|r| out.extend(r));
    } else {
        out.extend(0u16.to_be_bytes());
        planes.iter().for_each(|p| out.extend(p));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_describes_the_document() {
        let p = psd(&Scene::new(3, 20, 10), &Psd { colour: Colour::Cmyk, depth: Depth::Sixteen, ..Psd::default() }).unwrap();
        assert_eq!(&p[..6], b"8BPS\0\x01");
        assert_eq!(&p[12..14], [0, 4], "channels");
        assert_eq!(&p[14..22], [0, 0, 0, 10, 0, 0, 0, 20], "height then width");
        assert_eq!(&p[22..26], [0, 16, 0, 4], "16-bit CMYK");
        // Raw: header 26, three empty sections (4 each), compression 2, then 4 planes of 20 × 10 × 2.
        assert_eq!(p.len(), 26 + 12 + 2 + 4 * 400);
    }

    #[test]
    fn oversized_documents_and_unsupported_layouts_are_refused() {
        assert!(matches!(psd(&Scene::new(1, 30_001, 2), &Psd::default()), Err(Error::TooLarge { .. })));
        assert!(matches!(
            psd(&Scene::new(1, 4, 4), &Psd { colour: Colour::Cmyk, depth: Depth::Float, ..Psd::default() }),
            Err(Error::Unsupported { .. })
        ));
        // RLE row byte counts are 16-bit: a 30 000-pixel float row cannot be counted.
        assert!(psd(&Scene::new(1, 30_000, 1), &Psd { depth: Depth::Float, rle: true, ..Psd::default() }).is_err());
        assert!(psd(&Scene::new(1, 4, 4), &Psd { colour: Colour::Palette, depth: Depth::Sixteen, ..Psd::default() }).is_err());
    }
}
