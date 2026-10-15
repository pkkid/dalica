//! Our own PNG writer (ISO/IEC 15948), for the layouts the `png` crate's encoder does not write:
//! every colour type at every bit depth it allows (grey 1, 2, 4, 8 and 16 bits; palette 1, 2, 4
//! and 8; RGB, grey + alpha and RGBA 8 and 16), `tRNS` transparency (an alpha per palette entry, or
//! one transparent grey or RGB value), and Adam7 interlacing. Rows cycle through the five filter
//! types (None, Sub, Up, Average, Paeth) so a decoder's unfiltering is exercised too; the image data
//! is zlib through `fdeflate`, split over IDAT chunks of at most 8 KiB.
//!
//! [`png_image`] gives the exact samples a file stores (and the palette and `tRNS` values), and
//! [`PngImage::rgba8`] what a decoder should show, so decoder tests compare exactly.

use crate::formats::{Colour, Depth, Error, Result, SampleData, alpha, pack, palette, samples};
use crate::render::Scene;

/// The colour type of a PNG (IHDR byte 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PngColour {
    Grey,
    Rgb,
    Palette,
    GreyAlpha,
    Rgba,
}

impl PngColour {
    pub const ALL: [PngColour; 5] = [PngColour::Grey, PngColour::Rgb, PngColour::Palette, PngColour::GreyAlpha, PngColour::Rgba];

    /// The IHDR colour type code.
    pub fn code(self) -> u8 {
        match self {
            PngColour::Grey => 0,
            PngColour::Rgb => 2,
            PngColour::Palette => 3,
            PngColour::GreyAlpha => 4,
            PngColour::Rgba => 6,
        }
    }

    /// Samples per pixel.
    pub fn channels(self) -> usize {
        match self {
            PngColour::Grey | PngColour::Palette => 1,
            PngColour::GreyAlpha => 2,
            PngColour::Rgb => 3,
            PngColour::Rgba => 4,
        }
    }

    /// The bit depths PNG allows for this colour type (Table 11.1).
    pub fn depths(self) -> &'static [u8] {
        match self {
            PngColour::Grey => &[1, 2, 4, 8, 16],
            PngColour::Palette => &[1, 2, 4, 8],
            PngColour::Rgb | PngColour::GreyAlpha | PngColour::Rgba => &[8, 16],
        }
    }

    /// Whether a `tRNS` chunk is allowed (types without an alpha channel).
    pub fn takes_trns(self) -> bool {
        matches!(self, PngColour::Grey | PngColour::Rgb | PngColour::Palette)
    }
}

/// What the PNG holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Png {
    pub colour: PngColour,
    /// Bits per sample: one of [`PngColour::depths`].
    pub depth: u8,
    /// Adam7 interlacing.
    pub interlaced: bool,
    /// A `tRNS` chunk: palette entries with falling alpha (entry 0 fully transparent), or the
    /// first pixel's grey or RGB value made transparent. Only for types without alpha.
    pub trns: bool,
}

impl Default for Png {
    fn default() -> Png {
        Png { colour: PngColour::Rgb, depth: 8, interlaced: false, trns: false }
    }
}

/// The exact contents of a PNG: samples interleaved per pixel, rows top to bottom, each below
/// `2^depth` (palette indices for [`PngColour::Palette`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngImage {
    pub width: u32,
    pub height: u32,
    pub colour: PngColour,
    pub depth: u8,
    pub samples: Vec<u16>,
    /// The PLTE entries (palette images only).
    pub palette: Vec<[u8; 3]>,
    /// The `tRNS` values: an alpha per palette entry, or the transparent grey value, or the
    /// transparent RGB triple.
    pub trns: Option<Vec<u16>>,
}

impl PngImage {
    /// What a decoder shows: 8-bit RGBA with straight alpha. Low depths scale to 0–255
    /// (`v × 255 / (2^depth − 1)`, exact), 16-bit samples round to nearest (`(v × 255 + 32767) /
    /// 65535`), palette entries look up their colour and `tRNS` alpha, and a pixel equal to the
    /// `tRNS` value is transparent.
    pub fn rgba8(&self) -> Vec<u8> {
        let max = (1u32 << self.depth) - 1;
        let to8 = |v: u16| -> u8 { if self.depth == 16 { ((u32::from(v) * 255 + 32_767) / 65_535) as u8 } else { (u32::from(v) * 255 / max) as u8 } };
        let ch = self.colour.channels();
        let mut out = Vec::with_capacity(self.samples.len() / ch * 4);
        for px in self.samples.chunks_exact(ch) {
            let keyed = self.trns.as_deref().is_some_and(|key| self.colour != PngColour::Palette && key == px);
            let rgba = match (self.colour, px) {
                (PngColour::Grey, &[g]) => [to8(g), to8(g), to8(g), if keyed { 0 } else { 255 }],
                (PngColour::Rgb, &[r, g, b]) => [to8(r), to8(g), to8(b), if keyed { 0 } else { 255 }],
                (PngColour::GreyAlpha, &[g, a]) => [to8(g), to8(g), to8(g), to8(a)],
                (PngColour::Rgba, &[r, g, b, a]) => [to8(r), to8(g), to8(b), to8(a)],
                (PngColour::Palette, &[i]) => {
                    let [r, g, b] = self.palette.get(usize::from(i)).copied().unwrap_or_default();
                    let a = self.trns.as_ref().and_then(|t| t.get(usize::from(i))).map_or(255, |&a| a as u8);
                    [r, g, b, a]
                }
                _ => [0; 4],
            };
            out.extend(rgba);
        }
        out
    }
}

/// The samples a PNG of `opts` stores for `scene` (upright). Grey is the scene's luma, its top
/// bits at low depths; 16-bit samples carry `samples`' position pattern in the low byte; alpha is
/// [`alpha`]'s mask; 8-bit palettes are the fixed [`palette`], smaller ones `2^depth` colours of
/// it indexed by the luma's top bits.
pub fn png_image(scene: &Scene, opts: &Png) -> Result<PngImage> {
    let unsupported = |what: String| Error::Unsupported { format: "PNG", what };
    if !opts.colour.depths().contains(&opts.depth) {
        return Err(unsupported(format!("{}-bit {:?} samples", opts.depth, opts.colour)));
    }
    if opts.trns && !opts.colour.takes_trns() {
        return Err(unsupported(format!("a tRNS chunk with {:?} samples", opts.colour)));
    }
    let img = scene.render()?;
    let (w, h) = (img.width, img.height);
    let wide = opts.depth == 16;
    let colour_samples = |colour: Colour| -> Result<Vec<u16>> {
        Ok(match samples(&img, colour, if wide { Depth::Sixteen } else { Depth::Eight })?.data {
            SampleData::U8(v) => v.into_iter().map(|s| u16::from(s) >> (8 - opts.depth.min(8))).collect(),
            SampleData::U16(v) => v,
            SampleData::F32(_) => Vec::new(),
        })
    };
    let mask: Vec<u16> =
        alpha(w, h).into_iter().enumerate().map(|(i, a)| if wide { u16::from(a) * 256 + ((i * 29) & 0xff) as u16 } else { u16::from(a) }).collect();
    let with_alpha =
        |colour: Vec<u16>, ch: usize| -> Vec<u16> { colour.chunks_exact(ch).zip(&mask).flat_map(|(px, &a)| px.iter().copied().chain([a])).collect() };
    let mut pal = Vec::new();
    let samples = match opts.colour {
        PngColour::Grey => colour_samples(Colour::Grey)?,
        PngColour::Rgb => colour_samples(Colour::Rgb)?,
        PngColour::GreyAlpha => with_alpha(colour_samples(Colour::Grey)?, 1),
        PngColour::Rgba => with_alpha(colour_samples(Colour::Rgb)?, 3),
        PngColour::Palette if opts.depth == 8 => {
            pal = palette().to_vec();
            colour_samples(Colour::Palette)?
        }
        PngColour::Palette => {
            let n = 1usize << opts.depth;
            let cube = palette();
            pal = (0..n).map(|i| cube.get(i * 215 / (n - 1)).copied().unwrap_or_default()).collect();
            colour_samples(Colour::Grey)?
        }
    };
    let trns = match (opts.trns, opts.colour) {
        (false, _) => None,
        // Entry 0 clear, then falling alpha, every fourth entry opaque.
        (true, PngColour::Palette) => Some(
            (0..pal.len())
                .map(|i| {
                    if i == 0 {
                        0
                    } else if i % 4 == 0 {
                        255
                    } else {
                        (255 - i * 37 % 200) as u16
                    }
                })
                .collect(),
        ),
        (true, colour) => samples.get(..colour.channels()).map(<[u16]>::to_vec),
    };
    Ok(PngImage { width: w, height: h, colour: opts.colour, depth: opts.depth, samples, palette: pal, trns })
}

/// A PNG of `scene` as `opts` says.
pub fn png_layout(scene: &Scene, opts: &Png) -> Result<Vec<u8>> {
    write(&png_image(scene, opts)?, opts.interlaced)
}

/// The Adam7 passes: (first column, first row, column step, row step).
const ADAM7: [(u32, u32, u32, u32); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

/// The file bytes of `img`, interlaced or not.
pub fn write(img: &PngImage, interlaced: bool) -> Result<Vec<u8>> {
    let ch = img.colour.channels();
    let want = (img.width as usize).checked_mul(img.height as usize).and_then(|p| p.checked_mul(ch));
    if want != Some(img.samples.len()) || img.width == 0 || img.height == 0 || !img.colour.depths().contains(&img.depth) {
        return Err(Error::Unsupported { format: "PNG", what: format!("{} samples for {} × {} × {ch}", img.samples.len(), img.width, img.height) });
    }
    let passes: &[(u32, u32, u32, u32)] = if interlaced { &ADAM7 } else { &[(0, 0, 1, 1)] };
    let mut raw = Vec::new();
    for &(x0, y0, dx, dy) in passes {
        let cols: Vec<u32> = (x0..img.width).step_by(dx as usize).collect();
        let rows: Vec<u32> = (y0..img.height).step_by(dy as usize).collect();
        if cols.is_empty() || rows.is_empty() {
            continue;
        }
        let mut prior: Vec<u8> = Vec::new();
        for (k, &y) in rows.iter().enumerate() {
            let line: Vec<u16> = cols
                .iter()
                .flat_map(|&x| (0..ch).map(move |c| (y as usize * img.width as usize + x as usize) * ch + c))
                .map(|i| img.samples.get(i).copied().unwrap_or(0))
                .collect();
            let row = pack_row(&line, img.depth);
            if prior.is_empty() {
                prior = vec![0; row.len()];
            }
            let kind = (k % 5) as u8;
            raw.push(kind);
            raw.extend(filter(kind, &row, &prior, (ch * usize::from(img.depth)).div_ceil(8)));
            prior = row;
        }
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(img.width.to_be_bytes());
    ihdr.extend(img.height.to_be_bytes());
    ihdr.extend([img.depth, img.colour.code(), 0, 0, u8::from(interlaced)]);
    chunk(&mut out, b"IHDR", &ihdr);
    if img.colour == PngColour::Palette {
        chunk(&mut out, b"PLTE", &img.palette.concat());
    }
    if let Some(t) = &img.trns {
        let data: Vec<u8> =
            if img.colour == PngColour::Palette { t.iter().map(|&a| a as u8).collect() } else { t.iter().flat_map(|v| v.to_be_bytes()).collect() };
        chunk(&mut out, b"tRNS", &data);
    }
    for part in pack::deflate(&raw).chunks(8192) {
        chunk(&mut out, b"IDAT", part);
    }
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// One row of samples packed at `depth` bits, most significant first, 16-bit big-endian; the last
/// byte of a low-depth row is padded with zeros.
fn pack_row(line: &[u16], depth: u8) -> Vec<u8> {
    match depth {
        16 => line.iter().flat_map(|v| v.to_be_bytes()).collect(),
        8 => line.iter().map(|&v| v as u8).collect(),
        _ => {
            let per = usize::from(8 / depth);
            line.chunks(per)
                .map(|group| group.iter().enumerate().fold(0u8, |acc, (i, &v)| acc | ((v as u8 & ((1 << depth) - 1)) << (8 - depth * (i as u8 + 1)))))
                .collect()
        }
    }
}

/// `row` filtered with filter `kind` (PNG §9.2) against the `prior` row; `bpp` bytes per complete
/// pixel (at least 1).
fn filter(kind: u8, row: &[u8], prior: &[u8], bpp: usize) -> Vec<u8> {
    let at = |v: &[u8], i: usize| -> i16 { v.get(i).copied().map_or(0, i16::from) };
    (0..row.len())
        .map(|i| {
            let (x, b) = (at(row, i), at(prior, i));
            let a = i.checked_sub(bpp).map_or(0, |j| at(row, j));
            let c = i.checked_sub(bpp).map_or(0, |j| at(prior, j));
            let predicted = match kind {
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => paeth(a, b, c),
                _ => 0,
            };
            (x - predicted) as u8
        })
        .collect()
}

fn paeth(a: i16, b: i16, c: i16) -> i16 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Appends a chunk: length, type, data, CRC-32 of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(kind.iter().chain(data)).to_be_bytes());
}

/// CRC-32 (ISO 3309, reflected polynomial 0xEDB88320), bit by bit: test files are small.
fn crc32<'a>(bytes: impl Iterator<Item = &'a u8>) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every layout, read back by the `png` crate without transformations: the same header,
    /// palette, `tRNS` and packed rows (interlaced files de-interlaced by the reader).
    #[test]
    fn every_layout_reads_back_exactly() {
        let scene = Scene::new(3, 21, 13);
        for colour in PngColour::ALL {
            for &depth in colour.depths() {
                for trns in [false, true] {
                    if trns && !colour.takes_trns() {
                        assert!(png_image(&scene, &Png { colour, depth, interlaced: false, trns }).is_err());
                        continue;
                    }
                    let img = png_image(&scene, &Png { colour, depth, interlaced: false, trns }).unwrap();
                    let packed: Vec<u8> = img.samples.chunks(colour.channels() * img.width as usize).flat_map(|r| pack_row(r, depth)).collect();
                    for interlaced in [false, true] {
                        let case = format!("{colour:?} {depth} trns {trns} interlaced {interlaced}");
                        let bytes = write(&img, interlaced).unwrap();
                        let mut d = ::png::Decoder::new(std::io::Cursor::new(&bytes));
                        d.set_transformations(::png::Transformations::IDENTITY);
                        let mut r = d.read_info().unwrap();
                        let info = r.info();
                        assert_eq!(
                            (info.width, info.height, info.bit_depth as u8, info.color_type as u8, info.interlaced),
                            (21, 13, depth, colour.code(), interlaced),
                            "{case}"
                        );
                        let pal: Option<Vec<u8>> = info.palette.as_ref().map(|p| p.to_vec());
                        assert_eq!(pal, (colour == PngColour::Palette).then(|| img.palette.concat()), "{case}");
                        assert_eq!(info.trns.is_some(), trns, "{case}");
                        let mut buf = vec![0; r.output_buffer_size().unwrap()];
                        r.next_frame(&mut buf).unwrap();
                        assert_eq!(buf, packed, "{case}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_reference_scales_and_keys_like_the_specification() {
        let img =
            PngImage { width: 3, height: 1, colour: PngColour::Grey, depth: 2, samples: vec![0, 1, 3], palette: Vec::new(), trns: Some(vec![1]) };
        assert_eq!(img.rgba8(), [0, 0, 0, 255, 85, 85, 85, 0, 255, 255, 255, 255]);
        let wide =
            PngImage { width: 1, height: 1, colour: PngColour::GreyAlpha, depth: 16, samples: vec![0x8000, 0x0100], palette: Vec::new(), trns: None };
        assert_eq!(wide.rgba8(), [128, 128, 128, 1]);
        let pal = PngImage {
            width: 2,
            height: 1,
            colour: PngColour::Palette,
            depth: 1,
            samples: vec![1, 0],
            palette: vec![[1, 2, 3], [4, 5, 6]],
            trns: Some(vec![9]),
        };
        assert_eq!(pal.rgba8(), [4, 5, 6, 255, 1, 2, 3, 9]);
        // A 3-pixel row at 2 bits: 01 11 10, padded with 00 → 0x78.
        assert_eq!(pack_row(&[1, 3, 2], 2), [0x78]);
        assert_eq!(crc32(b"IEND".iter()), 0xae42_6082);
    }
}
