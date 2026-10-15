//! Our own DC-only sequential JPEG writer (ITU-T T.81), for the frames no encoder we use writes:
//! 12-bit samples (SOF1, extended sequential) and arithmetic coding (SOF9). Every 8 × 8 block is
//! flat (its mean), so the only coefficient is DC and the quantisation table is all ones: a
//! decoder shows exactly the block means (`jpeg_dc_reference`).
//!
//! - Huffman coding: one DC table for the difference categories 0–15 (enough for 12-bit
//!   differences) and one AC table holding EOB, shared by every component.
//! - Arithmetic coding: the same frame and scan headers under SOF9 with a DAC segment (DC
//!   conditioning L = 0, U = 1; AC Kx = 5). No arithmetic encoder is written here: the scan holds
//!   zero bytes, which an arithmetic decoder reads as some image, not this one. The file is for
//!   what a decoder answers to the coding process, not for its pixels.
//!
//! Grey frames have one component; colour frames three (JFIF YCbCr, no subsampling).

use crate::formats::{Colour, Depth, Error, Result, SampleData, samples};
use crate::render::{Rgb8, Scene};

/// How the scan is entropy-coded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Coding {
    Huffman,
    Arithmetic,
}

/// What the JPEG holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JpegDc {
    /// Sample precision: 8 or 12 bits.
    pub precision: u8,
    pub coding: Coding,
    /// One component (the scene's luma) instead of three.
    pub grey: bool,
}

impl Default for JpegDc {
    fn default() -> JpegDc {
        JpegDc { precision: 8, coding: Coding::Huffman, grey: false }
    }
}

impl JpegDc {
    /// The SOF marker code: extended sequential DCT, Huffman (C1) or arithmetic (C9).
    pub fn sof(&self) -> u8 {
        match self.coding {
            Coding::Huffman => 0xc1,
            Coding::Arithmetic => 0xc9,
        }
    }
}

/// Each block's component levels at the frame's precision, in raster order.
struct Blocks {
    across: u32,
    down: u32,
    /// One entry per block: Y, or Y, Cb, Cr.
    levels: Vec<[i32; 3]>,
}

/// The scene's samples at `precision` bits: grey (luma) or RGB, interleaved.
fn levels(img: &Rgb8, grey: bool, precision: u8) -> Result<Vec<u32>> {
    let colour = if grey { Colour::Grey } else { Colour::Rgb };
    Ok(match samples(img, colour, if precision > 8 { Depth::Sixteen } else { Depth::Eight })?.data {
        SampleData::U8(v) => v.into_iter().map(u32::from).collect(),
        SampleData::U16(v) => v.into_iter().map(|s| u32::from(s) >> (16 - precision)).collect(),
        SampleData::F32(_) => Vec::new(),
    })
}

fn blocks(img: &Rgb8, opts: &JpegDc) -> Result<Blocks> {
    if !matches!(opts.precision, 8 | 12) {
        return Err(Error::Unsupported { format: "JPEG", what: format!("{}-bit samples", opts.precision) });
    }
    let (w, h) = (img.width, img.height);
    let ch = if opts.grey { 1 } else { 3 };
    let src = levels(img, opts.grey, opts.precision)?;
    let (across, down) = (w.div_ceil(8), h.div_ceil(8));
    let top = f64::from((1u32 << opts.precision) - 1);
    let half = f64::from(1u32 << (opts.precision - 1));
    let mut out = Vec::with_capacity((across * down) as usize);
    for by in 0..down {
        for bx in 0..across {
            let mut sum = [0u64; 3];
            let mut n = 0u64;
            for y in by * 8..(by * 8 + 8).min(h) {
                for x in bx * 8..(bx * 8 + 8).min(w) {
                    let i = (y as usize * w as usize + x as usize) * ch;
                    for (c, s) in sum.iter_mut().enumerate().take(ch) {
                        *s += u64::from(src.get(i + c).copied().unwrap_or(0));
                    }
                    n += 1;
                }
            }
            let mean = sum.map(|s| (s + n / 2) / n.max(1));
            let level = if opts.grey {
                [mean[0] as i32, 0, 0]
            } else {
                let [r, g, b] = mean.map(|v| v as f64);
                let q = |v: f64| v.round().clamp(0.0, top) as i32;
                [
                    q(0.299 * r + 0.587 * g + 0.114 * b),
                    q(-0.168_736 * r - 0.331_264 * g + 0.5 * b + half),
                    q(0.5 * r - 0.418_688 * g - 0.081_312 * b + half),
                ]
            };
            out.push(level);
        }
    }
    Ok(Blocks { across, down, levels: out })
}

/// What a decoder shows for `jpeg_dc(scene, opts)` at 8 bits per sample: each block's mean (for
/// colour, its YCbCr levels converted back with the JFIF equations), scaled from the frame's
/// precision to 8 bits.
pub fn jpeg_dc_reference(scene: &Scene, opts: &JpegDc) -> Result<Rgb8> {
    let img = scene.render()?;
    let b = blocks(&img, opts)?;
    let shift = f64::from(1u32 << (opts.precision - 8));
    let half = f64::from(1u32 << (opts.precision - 1));
    let to8 = |v: f64| (v / shift).round().clamp(0.0, 255.0) as u8;
    let mut out = Rgb8::new(img.width, img.height)?;
    for y in 0..img.height {
        for x in 0..img.width {
            let [l0, l1, l2] = b.levels.get(((y / 8) * b.across + x / 8) as usize).copied().unwrap_or_default().map(f64::from);
            let rgb = if opts.grey {
                [to8(l0); 3]
            } else {
                let (cb, cr) = (l1 - half, l2 - half);
                [to8(l0 + 1.402 * cr), to8(l0 - 0.344_136 * cb - 0.714_136 * cr), to8(l0 + 1.772 * cb)]
            };
            let i = (y as usize * img.width as usize + x as usize) * 3;
            if let Some(px) = out.data.get_mut(i..i + 3) {
                px.copy_from_slice(&rgb);
            }
        }
    }
    Ok(out)
}

/// Bits written most significant first, with a 0x00 stuffed after every 0xFF byte.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, width: u32) {
        for k in (0..width).rev() {
            self.acc = (self.acc << 1) | ((value >> k) & 1);
            self.n += 1;
            if self.n == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                if byte == 0xff {
                    self.out.push(0);
                }
                (self.acc, self.n) = (0, 0);
            }
        }
    }

    /// Pads the last byte with one bits (T.81 §F.1.2.3).
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.put(0xff, 8 - self.n);
        }
        self.out
    }
}

/// The DC table: categories 0–14 as 4-bit codes 0000–1110, category 15 as 11110.
const DC_BITS: [u8; 16] = [0, 0, 0, 15, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// The AC table: EOB as `0`, ZRL (unused) as `10`.
const AC_BITS: [u8; 16] = [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const AC_VALUES: [u8; 2] = [0x00, 0xf0];

fn dc_code(category: u32) -> (u32, u32) {
    if category < 15 { (category, 4) } else { (0b11110, 5) }
}

fn segment(out: &mut Vec<u8>, marker: u8, data: &[u8]) {
    out.extend([0xff, marker]);
    out.extend(((data.len() + 2) as u16).to_be_bytes());
    out.extend(data);
}

/// A DC-only JPEG of `scene` (at least 1 × 1, at most 65 535 on a side).
pub fn jpeg_dc(scene: &Scene, opts: &JpegDc) -> Result<Vec<u8>> {
    let img = scene.render()?;
    let (w, h) = (img.width, img.height);
    if w > 65_535 || h > 65_535 {
        return Err(Error::TooLarge { format: "JPEG", max: 65_535, width: w, height: h });
    }
    let b = blocks(&img, opts)?;
    let comps: u8 = if opts.grey { 1 } else { 3 };
    let mut out = vec![0xff, 0xd8];
    if !opts.grey {
        segment(&mut out, 0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    }
    let mut dqt = vec![0u8];
    dqt.extend([1u8; 64]);
    segment(&mut out, 0xdb, &dqt);
    let mut sof = vec![opts.precision];
    sof.extend((h as u16).to_be_bytes());
    sof.extend((w as u16).to_be_bytes());
    sof.push(comps);
    for c in 1..=comps {
        sof.extend([c, 0x11, 0]);
    }
    segment(&mut out, opts.sof(), &sof);
    match opts.coding {
        Coding::Huffman => {
            let mut dht = vec![0x00];
            dht.extend(DC_BITS);
            dht.extend(0u8..16);
            dht.push(0x10);
            dht.extend(AC_BITS);
            dht.extend(AC_VALUES);
            segment(&mut out, 0xc4, &dht);
        }
        Coding::Arithmetic => segment(&mut out, 0xcc, &[0x00, 0x10, 0x10, 0x05]),
    }
    let mut sos = vec![comps];
    for c in 1..=comps {
        sos.extend([c, 0x00]);
    }
    sos.extend([0, 63, 0]);
    segment(&mut out, 0xda, &sos);
    match opts.coding {
        Coding::Huffman => {
            let shift = 1i32 << (opts.precision - 1);
            let mut pred = [0i32; 3];
            let mut bits = Bits::default();
            for level in &b.levels {
                for (c, p) in pred.iter_mut().enumerate().take(usize::from(comps)) {
                    let dc = 8 * (level.get(c).copied().unwrap_or(0) - shift);
                    let diff = dc - *p;
                    *p = dc;
                    let category = 32 - diff.unsigned_abs().leading_zeros();
                    let (code, len) = dc_code(category);
                    bits.put(code, len);
                    if category > 0 {
                        let extra = if diff >= 0 { diff } else { diff + (1 << category) - 1 };
                        bits.put(extra as u32, category);
                    }
                    bits.put(0, 1); // EOB
                }
            }
            out.extend(bits.finish());
        }
        Coding::Arithmetic => out.resize(out.len() + (b.across * b.down) as usize, 0),
    }
    out.extend([0xff, 0xd9]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap()
    }

    /// The 8-bit frames read back through `image` (an independent decoder): grey exactly the block
    /// means, colour within the decoder's YCbCr rounding.
    #[test]
    fn eight_bit_frames_decode_to_their_block_means() {
        let scene = Scene::new(12, 45, 29);
        let grey = JpegDc { grey: true, ..JpegDc::default() };
        let img = decode(&jpeg_dc(&scene, &grey).unwrap()).to_luma8();
        let reference = jpeg_dc_reference(&scene, &grey).unwrap();
        assert_eq!(img.into_raw(), reference.data.iter().step_by(3).copied().collect::<Vec<_>>());
        let colour = JpegDc::default();
        let img = decode(&jpeg_dc(&scene, &colour).unwrap()).to_rgb8();
        let reference = jpeg_dc_reference(&scene, &colour).unwrap();
        let worst = img.as_raw().iter().zip(&reference.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 2, "{worst}");
    }

    /// 12-bit and arithmetic frames carry their SOF marker and precision; the 12-bit scan spends
    /// more bits on its larger differences.
    #[test]
    fn twelve_bit_and_arithmetic_frames_are_marked() {
        let scene = Scene::new(12, 45, 29);
        for (opts, marker) in [
            (JpegDc { precision: 12, ..JpegDc::default() }, 0xc1),
            (JpegDc { coding: Coding::Arithmetic, ..JpegDc::default() }, 0xc9),
            (JpegDc { precision: 12, coding: Coding::Arithmetic, grey: true }, 0xc9),
        ] {
            let bytes = jpeg_dc(&scene, &opts).unwrap();
            let sof = bytes.windows(2).position(|m| m == [0xff, marker]).unwrap();
            assert_eq!(bytes[sof + 4], opts.precision, "{opts:?}");
            assert_eq!(&bytes[sof + 5..sof + 9], &[0, 29, 0, 45], "{opts:?}");
            assert_eq!(bytes.windows(2).any(|m| m == [0xff, 0xcc]), opts.coding == Coding::Arithmetic, "{opts:?}");
            assert!(bytes.ends_with(&[0xff, 0xd9]));
        }
        let eight = jpeg_dc(&scene, &JpegDc::default()).unwrap();
        let twelve = jpeg_dc(&scene, &JpegDc { precision: 12, ..JpegDc::default() }).unwrap();
        assert!(twelve.len() > eight.len());
        assert!(jpeg_dc(&scene, &JpegDc { precision: 10, ..JpegDc::default() }).is_err());
    }
}
