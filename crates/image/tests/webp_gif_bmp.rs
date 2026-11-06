//! WebP, GIF and BMP (`formats.md` §1) against `dalica-scenes` files, checked exactly where the
//! format is lossless and by fingerprints (`fingerprints/{webp,gif,bmp}-*.json`, `architecture.md`
//! §11) for every case.
//!
//! Every input is built here or by `dalica_scenes::formats`: lossless WebP (with alpha, EXIF and
//! ICCP chunks), GIF (one or several frames) and BMP (RGB, grey, palette) from its writers; an
//! animated WebP from two of its lossless bitstreams; a lossy WebP from a hand-built VP8 key frame
//! (no lossy encoder exists in pure Rust: its bool-coded partitions are all zero bytes, which
//! decode as every flag off, every macroblock DC-predicted and every coefficient block empty, a
//! flat grey; RFC 6386 §7 and §13), checked against the `image` crate's own decoder; a 32-bit
//! top-down `BITMAPV5HEADER` bitmap with an alpha mask and an embedded profile.
//!
//! Tolerance: 1.0 per fingerprint value (cell mean or deviation, 0–255). The decoders are integer
//! code and the scenes bit-identical everywhere, so the decodes are exact; the slack covers the
//! colour stage's profile path.

use crate::common::{assert_fingerprint, cases, rgba};
use std::time::Instant;

use dalica_image::color::{self, ColorPath};
use dalica_image::formats;
use dalica_image::resize;
use dalica_image::{
    Budget, ColorModel, DecodeOptions, Decoded, Error, Format, Limits, Orientation, Rgba8, Samples, decode, decode_thumbnail, decode_thumbnail_with,
    decode_with, read_header, sniff,
};
use dalica_scenes::formats::{Colour, Depth, SampleData, Webp, alpha, bmp, gif, quantized, samples, webp};
use dalica_scenes::icc::{self, Space, Version};
use dalica_scenes::{ByteOrder, Exif, Rgb8, Scene};
use proptest::prelude::*;

const TOL: f64 = 1.0;
const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

/// RGBA of an RGB image and an alpha plane.
fn with_alpha(img: &Rgb8, a: &[u8]) -> Rgba8 {
    let data = img.data.as_chunks::<3>().0.iter().zip(a).flat_map(|(&[r, g, b], &a)| [r, g, b, a]).collect();
    Rgba8 { width: img.width, height: img.height, data }
}

/// What the colour stage makes of `img` tagged with `icc` (the reference for profile tests).
fn through_profile(img: &Rgba8, icc: &[u8], alpha: bool) -> Rgba8 {
    let channels = if alpha { 4 } else { 3 };
    let samples = img.data.as_chunks::<4>().0.iter().flat_map(|px| px[..channels].to_vec()).collect();
    let d = Decoded {
        width: img.width,
        height: img.height,
        model: ColorModel::Rgb,
        alpha,
        samples: Samples::U8(samples),
        icc: Some(icc.to_vec()),
        orientation: Orientation::UPRIGHT,
    };
    let l = Limits::default();
    let converted = color::to_srgb8(d, &ON, &mut Budget::new(&l, img.width, img.height), &l).unwrap();
    assert_eq!(converted.path, ColorPath::Profile);
    converted.image
}

fn alpha_of(img: &Rgba8) -> Vec<u8> {
    img.data.iter().skip(3).step_by(4).copied().collect()
}

// ---- Hand-built files --------------------------------------------------------------------------

/// A RIFF WebP of the given chunks, each padded to an even size.
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

/// The chunks of a RIFF WebP: (FourCC, data).
fn chunks_of(file: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 12;
    while pos + 8 <= file.len() {
        let kind: [u8; 4] = file[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(file[pos + 4..pos + 8].try_into().unwrap()) as usize;
        out.push((kind, file[pos + 8..pos + 8 + size].to_vec()));
        pos += 8 + size + (size & 1);
    }
    out
}

fn u24(v: u32) -> [u8; 3] {
    let [a, b, c, _] = v.to_le_bytes();
    [a, b, c]
}

fn vp8x(flags: u8, w: u32, h: u32) -> Vec<u8> {
    [&[flags, 0, 0, 0][..], &u24(w - 1), &u24(h - 1)].concat()
}

/// A lossy key frame of `w × h` whose two partitions are all zero bytes (see the file comment).
fn vp8_flat(w: u16, h: u16) -> Vec<u8> {
    let first = 1024u32;
    let tag = (1 << 4) | (first << 5); // key frame, version 0, shown, first partition size
    let mut d = u24(tag).to_vec();
    d.extend([0x9d, 0x01, 0x2a]);
    d.extend(w.to_le_bytes());
    d.extend(h.to_le_bytes());
    d.extend(vec![0u8; first as usize + 4096]);
    d
}

/// A lossy WebP of `w × h`, with `alpha` stored raw in an `ALPH` chunk if given.
fn lossy_webp(w: u16, h: u16, alpha: Option<&[u8]>) -> Vec<u8> {
    let frame = vp8_flat(w, h);
    match alpha {
        None => riff(&[(b"VP8 ", &frame)]),
        Some(a) => {
            let alph = [&[0u8][..], a].concat(); // no compression, no filter
            riff(&[(b"VP8X", &vp8x(0x10, u32::from(w), u32::from(h))), (b"ALPH", &alph), (b"VP8 ", &frame)])
        }
    }
}

// ---- A lossy picture with content ----------------------------------------------------------------
//
// The flat frame above has no coefficients. This writer codes a key frame of its own (RFC 6386):
// the frame header with an RFC §7.3 bool encoder, then nothing but zero bits in the first
// partition (which decode as every macroblock B_PRED with DC-predicted subblocks, and no token
// probability updates), and in the DCT partitions a DC coefficient for every luma subblock and
// every macroblock's first U and V blocks. The token probabilities it needs are the RFC's
// defaults (§13.5) for those planes and bands.

/// RFC 6386 §7.3's boolean encoder.
struct BoolWriter {
    out: Vec<u8>,
    range: u32,
    bottom: u32,
    bit_count: i32,
}

impl BoolWriter {
    fn new() -> BoolWriter {
        BoolWriter { out: Vec::new(), range: 255, bottom: 0, bit_count: 24 }
    }

    fn put(&mut self, prob: u8, bit: bool) {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        if bit {
            self.bottom = self.bottom.wrapping_add(split);
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bottom & (1 << 31) != 0 {
                for b in self.out.iter_mut().rev() {
                    *b = b.wrapping_add(1);
                    if *b != 0 {
                        break;
                    }
                }
            }
            self.bottom <<= 1;
            self.bit_count -= 1;
            if self.bit_count == 0 {
                self.out.push((self.bottom >> 24) as u8);
                self.bottom &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }

    fn literal(&mut self, bits: u32, v: u32) {
        for i in (0..bits).rev() {
            self.put(128, (v >> i) & 1 == 1);
        }
    }

    /// The value `leaf` of `tree` (RFC form), from entry `at`.
    fn tree(&mut self, tree: &[i8], probs: &[u8], at: usize, leaf: i8) {
        fn path(tree: &[i8], at: usize, leaf: i8, bits: &mut Vec<(usize, bool)>) -> bool {
            for bit in [false, true] {
                let next = tree[at + usize::from(bit)];
                bits.push((at, bit));
                if (next <= 0 && -next == leaf) || (next > 0 && path(tree, next as usize, leaf, bits)) {
                    return true;
                }
                bits.pop();
            }
            false
        }
        let mut bits = Vec::new();
        assert!(path(tree, at, leaf, &mut bits));
        for (i, bit) in bits {
            self.put(probs[i >> 1], bit);
        }
    }

    /// Flushes: the code then ends exactly at the interval's bottom, so the zero bytes that follow
    /// read as 0 at any probability.
    fn finish(mut self) -> Vec<u8> {
        for _ in 0..32 {
            self.put(128, false);
        }
        self.out
    }
}

const TOKEN_TREE: [i8; 22] = [-11, 2, -0, 4, -1, 6, 8, 12, -2, 10, -3, -4, 14, 16, -5, -6, 18, 20, -7, -8, -9, -10];
/// RFC 6386 §13.5's default token probabilities, bands 0 and 1, contexts 0 to 2: luma in a
/// B_PRED macroblock (plane type 3) and chroma (plane type 2).
const LUMA_PROBS: [[[u8; 11]; 3]; 2] = [
    [
        [202, 24, 213, 235, 186, 191, 220, 160, 240, 175, 255],
        [126, 38, 182, 232, 169, 184, 228, 174, 255, 187, 128],
        [61, 46, 138, 219, 151, 178, 240, 170, 255, 216, 128],
    ],
    [
        [1, 112, 230, 250, 199, 191, 247, 159, 255, 255, 128],
        [166, 109, 228, 252, 211, 215, 255, 174, 128, 128, 128],
        [39, 77, 162, 232, 172, 180, 245, 178, 255, 255, 128],
    ],
];
const CHROMA_PROBS: [[[u8; 11]; 3]; 2] = [
    [
        [253, 9, 248, 251, 207, 208, 255, 192, 128, 128, 128],
        [175, 13, 224, 243, 193, 185, 249, 198, 255, 255, 128],
        [73, 17, 171, 221, 161, 179, 236, 167, 255, 234, 128],
    ],
    [
        [1, 95, 247, 253, 212, 183, 255, 255, 128, 128, 128],
        [239, 90, 244, 250, 211, 209, 255, 255, 128, 128, 128],
        [155, 77, 195, 248, 188, 195, 255, 255, 128, 128, 128],
    ],
];
/// The extra bits of tokens CAT1 to CAT6 (§13.2) and their bases.
const CAT_PROBS: [&[u8]; 6] = [
    &[159],
    &[165, 145],
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
    &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
];
const CAT_BASE: [i32; 6] = [5, 7, 11, 19, 35, 67];

/// A block holding only `dc` at context `ctx`; whether it is non-zero (the next blocks' context).
fn put_dc(w: &mut BoolWriter, probs: &[[[u8; 11]; 3]; 2], ctx: usize, dc: i32) -> bool {
    if dc == 0 {
        w.tree(&TOKEN_TREE, &probs[0][ctx], 0, 11);
        return false;
    }
    let a = dc.abs();
    if a <= 4 {
        w.tree(&TOKEN_TREE, &probs[0][ctx], 0, a as i8);
    } else {
        let cat = CAT_BASE.iter().rposition(|&b| a >= b).unwrap();
        w.tree(&TOKEN_TREE, &probs[0][ctx], 0, 5 + cat as i8);
        let extra = a - CAT_BASE[cat];
        let n = CAT_PROBS[cat].len();
        for (i, &p) in CAT_PROBS[cat].iter().enumerate() {
            w.put(p, (extra >> (n - 1 - i)) & 1 == 1);
        }
    }
    w.put(128, dc < 0);
    w.tree(&TOKEN_TREE, &probs[1][if a == 1 { 1 } else { 2 }], 0, 11);
    true
}

/// A lossy key frame of `w × h`, its loop filter at `filter` (0: off), whose luma subblock
/// `b` of macroblock `mbx`, `mby` gets DC level `luma(mbx, mby, b)` and whose first U and V
/// blocks get `chroma(mbx, mby)`, at quantizer index 40, in two DCT partitions.
fn vp8_picture(w: u16, h: u16, filter: u32, luma: impl Fn(usize, usize, usize) -> i32, chroma: impl Fn(usize, usize) -> (i32, i32)) -> Vec<u8> {
    let (mbw, mbh) = (usize::from(w).div_ceil(16), usize::from(h).div_ceil(16));
    let mut head = BoolWriter::new();
    head.literal(2, 0); // colour space, clamping
    head.put(128, false); // no segments
    head.put(128, false); // the normal filter
    head.literal(6, filter);
    head.literal(3, 0); // sharpness
    head.put(128, false); // no filter adjustments
    head.literal(2, 1); // two DCT partitions
    head.literal(7, 40); // quantizer index
    head.literal(5, 0); // no quantizer deltas
    head.literal(1, 0); // refresh entropy probabilities
    let mut first = head.finish();
    // Zero bits for the token updates and every macroblock header: about 4 bits each.
    first.resize(first.len() + mbw * mbh * 5 / 8 + 1024, 0);
    assert!(first.len() < 1 << 19, "the first partition outgrows its 19-bit size");
    let mut parts = [BoolWriter::new(), BoolWriter::new()];
    // Non-zero flags above (per macroblock column: 4 luma, 2 U, 2 V) and to the left.
    let mut top = vec![[false; 8]; mbw];
    for mby in 0..mbh {
        let p = &mut parts[mby % 2];
        let mut left = [false; 8];
        for (mbx, top) in top.iter_mut().enumerate() {
            for b in 0..16 {
                let (x, y) = (b % 4, b / 4);
                let ctx = usize::from(top[x]) + usize::from(left[y]);
                let nz = put_dc(p, &LUMA_PROBS, ctx, luma(mbx, mby, b));
                top[x] = nz;
                left[y] = nz;
            }
            let (u, v) = chroma(mbx, mby);
            for (plane, dc) in [(4usize, u), (6, v)] {
                for b in 0..4 {
                    let (x, y) = (b % 2, b / 2);
                    let ctx = usize::from(top[plane + x]) + usize::from(left[plane + y]);
                    let nz = put_dc(p, &CHROMA_PROBS, ctx, if b == 0 { dc } else { 0 });
                    top[plane + x] = nz;
                    left[plane + y] = nz;
                }
            }
        }
    }
    let [p0, p1] = parts.map(|p| {
        let mut d = p.finish();
        d.resize(d.len() + 64, 0);
        d
    });
    let tag = (1 << 4) | ((first.len() as u32) << 5);
    let mut d = u24(tag).to_vec();
    d.extend([0x9d, 0x01, 0x2a]);
    d.extend(w.to_le_bytes());
    d.extend(h.to_le_bytes());
    d.extend(&first);
    d.extend(u24(p0.len() as u32));
    d.extend(&p0);
    d.extend(&p1);
    d
}

/// A lossy WebP with content: macroblocks of different DC levels, textured subblocks, colour.
fn lossy_picture(w: u16, h: u16) -> Vec<u8> {
    let frame = vp8_picture(
        w,
        h,
        20,
        |x, y, b| if b == 0 { ((x / 2 + y / 3) % 5) as i32 * 6 - 12 } else { ((x * 3 + y * 5 + b * 7) % 5) as i32 - 2 },
        |x, y| (((x / 3 + y) % 5) as i32 * 4 - 8, ((x + y / 2) % 3) as i32 * 4 - 4),
    );
    riff(&[(b"VP8 ", &frame)])
}

/// The PSNR of `a` against `b` (same size, RGB), in dB.
fn psnr(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (mut sum, mut n) = (0f64, 0f64);
    for (pa, pb) in a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0) {
        for c in 0..3 {
            sum += (f64::from(pa[c]) - f64::from(pb[c])).powi(2);
            n += 1.0;
        }
    }
    if sum == 0.0 { f64::INFINITY } else { 10.0 * (255f64 * 255.0 / (sum / n)).log10() }
}

/// `small` stretched to `w × h`, each pixel the one its position falls in.
fn stretched(small: &Rgba8, w: u32, h: u32) -> Rgba8 {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * small.width / w, y * small.height / h);
            data.extend_from_slice(&small.data[((sy * small.width + sx) * 4) as usize..][..4]);
        }
    }
    Rgba8 { width: w, height: h, data }
}

/// `img` reduced to 1/`s` by exact means of `s × s` blocks (fewer at the edges).
fn box_reduced(img: &Rgba8, s: u32) -> Rgba8 {
    let (w, h) = (img.width.div_ceil(s), img.height.div_ceil(s));
    let mut data = Vec::new();
    for by in 0..h {
        for bx in 0..w {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in by * s..((by + 1) * s).min(img.height) {
                for x in bx * s..((bx + 1) * s).min(img.width) {
                    let px = &img.data[((y * img.width + x) * 4) as usize..][..4];
                    for c in 0..4 {
                        sum[c] += u32::from(px[c]);
                    }
                    n += 1;
                }
            }
            data.extend(sum.map(|v| ((v + n / 2) / n) as u8));
        }
    }
    Rgba8 { width: w, height: h, data }
}

/// An animation of the scenes `seeds`, each a lossless frame covering the canvas, alpha-blended
/// onto it or not.
fn animated_webp(scene: &Scene, seeds: &[u64], blend: bool) -> Vec<u8> {
    let frames: Vec<Vec<u8>> = seeds
        .iter()
        .map(|&seed| {
            let file = webp(&Scene { seed, ..*scene }, &Webp::default()).unwrap();
            let (kind, bits) = chunks_of(&file).remove(0);
            assert_eq!(&kind, b"VP8L");
            let mut d = [[0u8; 6].as_slice(), &u24(scene.width - 1), &u24(scene.height - 1), &u24(100), &[if blend { 0 } else { 0x02 }]].concat();
            d.extend(b"VP8L");
            d.extend((bits.len() as u32).to_le_bytes());
            d.extend(&bits);
            if bits.len() % 2 == 1 {
                d.push(0);
            }
            d
        })
        .collect();
    let head = vp8x(0x02, scene.width, scene.height);
    let mut chunks: Vec<(&[u8; 4], &[u8])> = vec![(b"VP8X", &head), (b"ANIM", &[0, 0, 0, 0, 0, 0])];
    chunks.extend(frames.iter().map(|f| (b"ANMF", f.as_slice())));
    riff(&chunks)
}

/// A top-down 32-bit `BITMAPV5HEADER` bitmap (BI_BITFIELDS with an alpha mask) of `img` and
/// `alpha`, with `icc` embedded after the pixels.
fn bmp_v5(img: &Rgb8, a: &[u8], icc: &[u8]) -> Vec<u8> {
    let (w, h) = (img.width, img.height);
    let pixels: Vec<u8> = img.data.as_chunks::<3>().0.iter().zip(a).flat_map(|(&[r, g, b], &a)| [b, g, r, a]).collect();
    let mut v5 = Vec::new();
    v5.extend(124u32.to_le_bytes());
    v5.extend((w as i32).to_le_bytes());
    v5.extend((-(h as i32)).to_le_bytes());
    v5.extend(1u16.to_le_bytes());
    v5.extend(32u16.to_le_bytes());
    v5.extend(3u32.to_le_bytes()); // BI_BITFIELDS
    v5.extend((pixels.len() as u32).to_le_bytes());
    v5.extend([0; 16]); // resolution, colours used and important
    for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
        v5.extend(mask.to_le_bytes());
    }
    v5.extend(0x4d42_4544u32.to_le_bytes()); // 'MBED'
    v5.extend([0; 36 + 12]); // endpoints, gammas
    v5.extend(4u32.to_le_bytes()); // LCS_GM_IMAGES
    v5.extend((124 + pixels.len() as u32).to_le_bytes()); // profile, from the DIB header's start
    v5.extend((icc.len() as u32).to_le_bytes());
    v5.extend([0; 4]);
    assert_eq!(v5.len(), 124);
    let total = 14 + v5.len() + pixels.len() + icc.len();
    [b"BM".as_slice(), &(total as u32).to_le_bytes(), &[0; 4], &138u32.to_le_bytes(), &v5, &pixels, icc].concat()
}

// ---- WebP --------------------------------------------------------------------------------------

#[test]
fn lossless_webp_decodes_exactly() {
    let scene = Scene::new(31, 160, 120);
    let upright = scene.render().unwrap();
    let file = webp(&scene, &Webp::default()).unwrap();
    assert_eq!(sniff(&file), Some(Format::WebP));
    let h = read_header(&file, &Limits::default()).unwrap();
    assert_eq!((h.format, h.width, h.height, h.orientation.get(), h.thumbnail), (Format::WebP, 160, 120, 1, None));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, rgba(&upright));

    // With alpha: the colours untouched, the alpha plane exact.
    let file = webp(&scene, &Webp { alpha: true, ..Webp::default() }).unwrap();
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, with_alpha(&upright, &alpha(160, 120)));
}

#[test]
fn lossy_webp_decodes_like_the_reference_decoder() {
    let (w, h) = (72u16, 40u16);
    let reference = |file: &[u8]| image::load_from_memory_with_format(file, image::ImageFormat::WebP).unwrap().to_rgba8();
    let file = lossy_webp(w, h, None);
    let header = read_header(&file, &Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (72, 40));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img.data, reference(&file).into_raw());
    // Grey, opaque and nearly flat: DC prediction from the frame's edge values (127 above, 129 to
    // the left) settles within a level or two of mid-grey.
    for px in img.data.as_chunks::<4>().0 {
        assert!(px[0] == px[1] && px[1] == px[2] && (125..=135).contains(&px[0]) && px[3] == 255, "{px:?}");
    }
    assert_fingerprint("webp-lossy", &img, TOL);

    // Lossy with an alpha chunk.
    let a = alpha(u32::from(w), u32::from(h));
    let file = lossy_webp(w, h, Some(&a));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img.data, reference(&file).into_raw());
    assert_eq!(alpha_of(&img), a);
    assert_fingerprint("webp-lossy-alpha", &img, TOL);
}

/// A lossy WebP's thumbnail is decoded scaled (M16.19): exactly the full decode's pixels reduced
/// by 1/2, 1/4 or 1/8 and then fitted, so against the full decode it loses no more than fitting
/// the full decode does. The bound: measured against the full decode itself (each thumbnail
/// stretched back to full size), the scaled thumbnail's PSNR is within 1 dB of the full decode
/// fitted.
#[test]
fn a_lossy_thumbnail_is_decoded_scaled() {
    let l = Limits::default();
    let file = lossy_picture(1000, 700);
    let full = decode(&file, &l).unwrap();
    // Content: many levels, not a flat frame.
    let mut levels: Vec<u8> = full.data.iter().step_by(4).copied().collect();
    levels.sort_unstable();
    levels.dedup();
    assert!(levels.len() > 40, "{} levels of red", levels.len());
    for (long_side, s) in [(256, 2), (200, 4), (120, 8)] {
        let thumb = decode_thumbnail(&file, long_side, &l).unwrap();
        let fitted = resize::fit(&full, long_side).unwrap();
        assert_eq!((thumb.width, thumb.height), (fitted.width, fitted.height));
        // The decoder's rows are the full decode's, bit for bit.
        assert_eq!(thumb, resize::fit(&box_reduced(&full, s), long_side).unwrap(), "1/{s}");
        let own = psnr(&stretched(&fitted, full.width, full.height), &full);
        let scaled = psnr(&stretched(&thumb, full.width, full.height), &full);
        assert!(scaled >= own - 1.0, "1/{s}: {scaled:.2} dB against the fit's {own:.2} dB");
    }
    // An odd size without the loop filter, then with a profile and orientation.
    let frame = vp8_picture(333, 201, 0, |x, y, b| ((x + y + b) % 3) as i32 - 1, |_, _| (3, -3));
    let file = riff(&[(b"VP8 ", &frame)]);
    let full = decode(&file, &l).unwrap();
    assert_eq!(decode_thumbnail(&file, 64, &l).unwrap(), resize::fit(&box_reduced(&full, 4), 64).unwrap());
    let p3 = icc::rgb(Space::DisplayP3, Version::V4);
    let with_exif = webp(&Scene::new(1, 333, 201), &Webp { exif: Some(Exif { orientation: 6, ..Exif::default() }), ..Webp::default() }).unwrap();
    let (_, exif) = chunks_of(&with_exif).into_iter().find(|(k, _)| k == b"EXIF").unwrap();
    let tagged = riff(&[(b"VP8X", &vp8x(0x20 | 0x08, 333, 201)), (b"ICCP", &p3), (b"VP8 ", &frame), (b"EXIF", &exif)]);
    let t = decode_thumbnail(&tagged, 64, &l).unwrap();
    assert_eq!((t.width, t.height), (39, 64), "turned upright");
    // The reduced picture goes through the profile, then is fitted and turned.
    let expected = through_profile(&box_reduced(&full, 4), &p3, false);
    let expected = dalica_image::orient::apply(resize::fit(&expected, 64).unwrap(), Orientation::new(6).unwrap()).unwrap();
    assert_eq!(t, expected);
    assert_ne!(t, decode_thumbnail_with(&tagged, 64, &OFF, &l).unwrap(), "the profile changed nothing");
}

/// A lossy picture past the 64 MP that a thumbnail used to stop at gets its thumbnail, holding a
/// small part of the frame: with an allocation limit of 1/16 of what its full decode charges,
/// the full decode is refused and the thumbnail is made.
#[test]
fn a_huge_lossy_webp_gets_its_thumbnail_on_a_small_budget() {
    let (w, h) = (16_000u16, 12_000u16);
    let frame = vp8_picture(w, h, 0, |_, _, _| 0, |_, _| (0, 0));
    let file = riff(&[(b"VP8 ", &frame)]);
    let (w, h) = (u64::from(w), u64::from(h));
    // The full decode: the decoder's planes (charged as 3 bytes a padded pixel), its RGB output
    // and the RGBA picture.
    let full_peak = w.div_ceil(16) * 16 * h.div_ceil(16) * 16 * 3 + w * h * 3 + w * h * 4;
    let tight = Limits { max_alloc: full_peak / 16, ..Limits::default() };
    assert!(matches!(decode(&file, &tight), Err(Error::TooLarge { .. })));
    let t = decode_thumbnail(&file, 256, &tight).unwrap();
    assert_eq!((t.width, t.height), (256, 192));
    // A flat frame: mid-grey everywhere (as the small flat frame decodes).
    assert!(t.data.as_chunks::<4>().0.iter().all(|px| px[0] == px[1] && (120..=136).contains(&px[0]) && px[3] == 255));
}

/// Cut or damaged rows give the damaged-file error, never a panic; lossless, alpha and animated
/// files keep the full decode's thumbnail.
#[test]
fn damaged_lossy_rows_and_the_other_kinds() {
    let l = Limits::default();
    let file = lossy_picture(400, 300);
    let header_end = 12 + 8 + 10;
    // Inside the second DCT partition (before its zero padding), the first, the frame header.
    for cut in [file.len() - 200, file.len() / 2, file.len() / 3, header_end + 100] {
        let mut cut_file = file[..cut].to_vec();
        let body = (cut_file.len() - 8) as u32;
        cut_file[4..8].copy_from_slice(&body.to_le_bytes());
        let len = (cut_file.len() - 20) as u32;
        cut_file[16..20].copy_from_slice(&len.to_le_bytes());
        let err = decode_thumbnail(&cut_file, 128, &l).unwrap_err();
        assert_eq!(err.reason(), "Damaged file", "cut at {cut}: {err:?}");
        assert!(!matches!(err, Error::Panic(_)), "cut at {cut}");
    }
    for at in (header_end..file.len()).step_by(97) {
        let mut bad = file.clone();
        bad[at] ^= 0x5a;
        match decode_thumbnail(&bad, 128, &l) {
            Ok(t) => assert_eq!((t.width, t.height), (128, 96)),
            Err(e) => assert!(e.reason() == "Damaged file" && !matches!(e, Error::Panic(_)), "byte {at}: {e:?}"),
        }
    }
    let scene = Scene::new(12, 600, 400);
    for (name, file) in [
        ("lossless", webp(&scene, &Webp::default()).unwrap()),
        ("lossy alpha", lossy_webp(600, 400, Some(&alpha(600, 400)))),
        ("animated", animated_webp(&scene, &[12, 13], false)),
    ] {
        let full = decode(&file, &l).unwrap();
        assert_eq!(decode_thumbnail(&file, 128, &l).unwrap(), resize::fit(&full, 128).unwrap(), "{name}");
    }
}

#[test]
fn webp_orientation_comes_from_its_exif_chunk() {
    let scene = Scene::new(37, 120, 80);
    let reference = rgba(&scene.render().unwrap());
    for o in 1..=8 {
        let exif =
            Exif { orientation: o, thumbnail: Some(48), byte_order: if o % 2 == 0 { ByteOrder::Big } else { ByteOrder::Little }, ..Exif::default() };
        let file = webp(&scene, &Webp { exif: Some(exif), ..Webp::default() }).unwrap();
        let h = read_header(&file, &Limits::default()).unwrap();
        assert_eq!(h.orientation.get(), o);
        assert_eq!((h.width, h.height), if o >= 5 { (80, 120) } else { (120, 80) }, "orientation {o}");
        assert_eq!(h.display_size(), (120, 80));
        assert!(h.exif_error.is_none(), "{:?}", h.exif_error);
        // The EXIF chunk's IFD1 thumbnail is the fast path.
        let thumb = formats::embedded_thumbnail(Format::WebP, &file, &Limits::default()).unwrap().unwrap();
        let th = read_header(&file[thumb], &Limits::default()).unwrap();
        assert_eq!(th.format, Format::Jpeg);
        assert_eq!(th.width.max(th.height), 48);
        let img = decode(&file, &Limits::default()).unwrap();
        assert_eq!(img, reference, "orientation {o}");
        let t = decode_thumbnail(&file, 60, &Limits::default()).unwrap();
        assert_eq!((t.width, t.height), (60, 40), "orientation {o}");
    }
    // A writer that kept the `Exif\0\0` prefix of the JPEG segment is read too.
    let exif = Exif { orientation: 6, ..Exif::default() };
    let file = webp(&scene, &Webp { exif: Some(exif), ..Webp::default() }).unwrap();
    let mut chunks = chunks_of(&file);
    for (kind, data) in &mut chunks {
        if kind == b"EXIF" {
            data.splice(0..0, *b"Exif\0\0");
        }
    }
    let refs: Vec<(&[u8; 4], &[u8])> = chunks.iter().map(|(k, d)| (k, d.as_slice())).collect();
    let prefixed = riff(&refs);
    assert_eq!(read_header(&prefixed, &Limits::default()).unwrap().orientation.get(), 6);
    assert_eq!(decode(&prefixed, &Limits::default()).unwrap(), reference);
}

#[test]
fn a_webp_profile_goes_through_the_colour_stage() {
    let scene = Scene::new(41, 128, 96);
    let upright = scene.render().unwrap();
    let p3 = icc::rgb(Space::DisplayP3, Version::V4);
    let file = webp(&scene, &Webp { icc: Some(p3.clone()), ..Webp::default() }).unwrap();
    let managed = decode_with(&file, &ON, &Limits::default()).unwrap();
    assert_eq!(managed, through_profile(&rgba(&upright), &p3, false));
    assert_ne!(managed, rgba(&upright), "the profile changed nothing");
    assert_fingerprint("webp-icc-p3", &managed, TOL);
    // Colour management off: the stored values as they are.
    assert_eq!(decode_with(&file, &OFF, &Limits::default()).unwrap(), rgba(&upright));
    // With alpha too (the extended format carries the profile beside the alpha).
    let a = alpha(128, 96);
    let file = webp(&scene, &Webp { icc: Some(p3.clone()), alpha: true, ..Webp::default() }).unwrap();
    assert_eq!(decode(&file, &Limits::default()).unwrap(), through_profile(&with_alpha(&upright, &a), &p3, true));
}

#[test]
fn an_animated_webp_shows_frame_one() {
    let scene = Scene::new(50, 96, 64);
    let file = animated_webp(&scene, &[50, 51, 52], false);
    let h = read_header(&file, &Limits::default()).unwrap();
    assert_eq!((h.format, h.width, h.height), (Format::WebP, 96, 64));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, rgba(&scene.render().unwrap()));
    assert_ne!(img, rgba(&Scene { seed: 51, ..scene }.render().unwrap()));
    assert_fingerprint("webp-animated", &img, TOL);
    // Alpha-blended onto the empty canvas, an opaque frame comes out within one level (the
    // decoder's integer blend rounds).
    let blended = decode(&animated_webp(&scene, &[50, 51, 52], true), &Limits::default()).unwrap();
    assert!(blended.data.iter().zip(&img.data).all(|(a, b)| a.abs_diff(*b) <= 1));
}

// ---- GIF ---------------------------------------------------------------------------------------

/// Where the first image descriptor of a `gif()` file starts (its writer puts one after the
/// screen, the global table if any, and the extensions).
fn first_descriptor(file: &[u8]) -> usize {
    let mut pos = 13;
    if file[10] & 0x80 != 0 {
        pos += 3 << ((file[10] & 7) + 1);
    }
    while file[pos] == 0x21 {
        pos += 2;
        while file[pos] != 0 {
            pos += 1 + usize::from(file[pos]);
        }
        pos += 1;
    }
    assert_eq!(file[pos], 0x2c);
    pos
}

#[test]
fn gif_decodes_its_palette_colours_exactly() {
    let scene = Scene::new(60, 150, 100);
    let file = gif(&scene, 1).unwrap();
    assert_eq!(sniff(&file), Some(Format::Gif));
    let h = read_header(&file, &Limits::default()).unwrap();
    assert_eq!((h.format, h.width, h.height, h.orientation.get(), h.exif), (Format::Gif, 150, 100, 1, None));
    assert_eq!(formats::embedded_thumbnail(Format::Gif, &file, &Limits::default()), Ok(None));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, rgba(&quantized(&scene.render().unwrap())));
    assert_fingerprint("gif", &img, TOL);
    let t = decode_thumbnail(&file, 75, &Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (75, 50));
}

#[test]
fn an_animated_gif_shows_frame_one() {
    let scene = Scene::new(70, 120, 90);
    let file = gif(&scene, 5).unwrap();
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, rgba(&quantized(&scene.render().unwrap())));
    assert_ne!(img, rgba(&quantized(&Scene { seed: 71, ..scene }.render().unwrap())));
    assert_fingerprint("gif-animated", &img, TOL);
}

#[test]
fn a_gif_frame_smaller_than_its_screen_is_placed_on_it() {
    let scene = Scene::new(80, 40, 30);
    let mut file = gif(&scene, 1).unwrap();
    // Grow the screen to 50 × 36 and move the frame to (6, 4): the rest is transparent.
    file[6..10].copy_from_slice(&[50, 0, 36, 0]);
    let d = first_descriptor(&file);
    file[d + 1..d + 5].copy_from_slice(&[6, 0, 4, 0]);
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!((img.width, img.height), (50, 36));
    let frame = rgba(&quantized(&scene.render().unwrap()));
    for y in 0..36usize {
        for x in 0..50usize {
            let px = &img.data[(y * 50 + x) * 4..][..4];
            if (6..46).contains(&x) && (4..34).contains(&y) {
                assert_eq!(px, &frame.data[((y - 4) * 40 + x - 6) * 4..][..4], "{x}, {y}");
            } else {
                assert_eq!(px[3], 0, "{x}, {y} is not transparent");
            }
        }
    }
}

// ---- BMP ---------------------------------------------------------------------------------------

#[test]
fn bmp_decodes_exactly_in_each_layout() {
    let scene = Scene::new(90, 101, 67); // odd width: rows are padded
    let upright = scene.render().unwrap();
    let SampleData::U8(grey) = samples(&upright, Colour::Grey, Depth::Eight).unwrap().data else { unreachable!() };
    let grey_rgb = Rgb8 { width: 101, height: 67, data: grey.iter().flat_map(|&v| [v, v, v]).collect() };
    let indexed = quantized(&upright);
    for (colour, expected) in [(Colour::Rgb, &upright), (Colour::Grey, &grey_rgb), (Colour::Palette, &indexed)] {
        let file = bmp(&scene, colour).unwrap();
        assert_eq!(sniff(&file), Some(Format::Bmp));
        let h = read_header(&file, &Limits::default()).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation.get()), (Format::Bmp, 101, 67, 1));
        assert_eq!(formats::embedded_thumbnail(Format::Bmp, &file, &Limits::default()), Ok(None));
        let img = decode(&file, &Limits::default()).unwrap();
        assert_eq!(img, rgba(expected), "{colour:?}");
    }
}

#[test]
fn a_v5_bitmap_with_alpha_and_a_profile() {
    let scene = Scene::new(95, 64, 48);
    let upright = scene.render().unwrap();
    let a = alpha(64, 48);
    let p3 = icc::rgb(Space::DisplayP3, Version::V2);
    let file = bmp_v5(&upright, &a, &p3);
    assert_eq!(sniff(&file), Some(Format::Bmp));
    let img = decode(&file, &Limits::default()).unwrap();
    assert_eq!(img, through_profile(&with_alpha(&upright, &a), &p3, true));
    assert_eq!(alpha_of(&img), a);
    assert_fingerprint("bmp-v5-alpha-p3", &img, TOL);
    assert_eq!(decode_with(&file, &OFF, &Limits::default()).unwrap(), with_alpha(&upright, &a));
    // A profile pointer past the end of the file is no profile.
    let mut lost = file.clone();
    lost[14 + 112..14 + 116].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&lost, &Limits::default()).unwrap(), with_alpha(&upright, &a));
}

#[test]
fn unsupported_bmp_compressions_answer_their_reason() {
    let mut file = bmp(&Scene::new(3, 8, 8), Colour::Rgb).unwrap();
    file[14 + 16..14 + 20].copy_from_slice(&4u32.to_le_bytes()); // BI_JPEG
    let err = decode(&file, &Limits::default()).unwrap_err();
    assert_eq!(err, Error::Unsupported(Format::Bmp));
    assert_eq!(err.reason(), "This kind of BMP isn't supported");
}

// ---- Limits and hostile input ------------------------------------------------------------------

/// A BMP whose header claims `w × h` (24-bit), with no pixels.
fn bmp_claiming(w: i32, h: i32) -> Vec<u8> {
    let mut d = 40u32.to_le_bytes().to_vec();
    d.extend(w.to_le_bytes());
    d.extend(h.to_le_bytes());
    d.extend([1, 0, 24, 0]);
    d.extend([0; 24]);
    [b"BM".as_slice(), &[0; 8], &54u32.to_le_bytes(), &d].concat()
}

/// A GIF whose screen and first frame claim the given sizes, with no pixel data.
fn gif_claiming(screen: (u16, u16), frame: (u16, u16)) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend(screen.0.to_le_bytes());
    b.extend(screen.1.to_le_bytes());
    b.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255, 0x2c, 0, 0, 0, 0]);
    b.extend(frame.0.to_le_bytes());
    b.extend(frame.1.to_le_bytes());
    b.extend([0, 2, 2, 0x4c, 0x01, 0, 0x3b]);
    b
}

/// Each size is refused from the header alone. Allocating for it would abort the test process
/// (or take seconds), so passing quickly shows nothing was allocated.
#[test]
fn oversized_headers_are_refused_before_allocating() {
    let start = Instant::now();
    let l = Limits::default();
    let too_large = |bytes: &[u8], what: &str| {
        assert!(matches!(decode(bytes, &l), Err(Error::TooLarge { .. })), "{what}: {:?}", decode(bytes, &l));
        assert!(matches!(decode_thumbnail(bytes, 256, &l), Err(Error::TooLarge { .. })), "{what}");
    };
    // WebP: a 16 383² lossless bitstream (1 GiB of RGBA, plus the decoder's own frame).
    let bits = [&[0x2f][..], &(0x3ffe | (0x3ffe << 14) | (1 << 28) as u32).to_le_bytes(), &[0; 16]].concat();
    too_large(&riff(&[(b"VP8L", &bits)]), "VP8L 16383²");
    // The decoder's own buffers are charged before it runs: a lossy 4096² frame fits a budget of
    // 5 bytes per pixel for the 8-bit output, not with its YUV planes as well.
    let tight = Limits { max_alloc: 5 * 4096 * 4096, ..l };
    let lossy = lossy_webp(4096, 4096, None);
    assert_eq!(decode(&lossy, &tight), Err(Error::TooLarge { width: 4096, height: 4096 }));
    // An animation canvas wider than any edge Dalica decodes, around a small frame.
    let anim = animated_webp(&Scene::new(1, 16, 16), &[1], true);
    let mut chunks = chunks_of(&anim);
    chunks[0].1 = vp8x(0x02, 100_000, 16);
    let refs: Vec<(&[u8; 4], &[u8])> = chunks.iter().map(|(k, d)| (k, d.as_slice())).collect();
    too_large(&riff(&refs), "WebP canvas 100000 × 16");
    // GIF: a 65 535² screen; a small screen whose first frame claims 65 535².
    too_large(&gif_claiming((65_535, 65_535), (1, 1)), "GIF screen");
    too_large(&gif_claiming((16, 16), (65_535, 65_535)), "GIF frame");
    // BMP: past the edge limit, and past the allocation budget.
    too_large(&bmp_claiming(100_000, 10), "BMP 100000 wide");
    too_large(&bmp_claiming(30_000, -30_000), "BMP 30000²");
    assert!(start.elapsed().as_millis() < 500, "{:?}", start.elapsed());
    // A VP8X canvas past 2³² pixels, or a bitstream larger than its canvas, is damaged.
    let mut chunks = chunks_of(&anim);
    chunks[0].1 = vp8x(0x02, 1 << 24, 1 << 24);
    let refs: Vec<(&[u8; 4], &[u8])> = chunks.iter().map(|(k, d)| (k, d.as_slice())).collect();
    assert_eq!(read_header(&riff(&refs), &l).unwrap_err().reason(), "Damaged file");
}

/// Every test file of these formats.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    let scene = Scene::new(7, 40, 28);
    let exif = Exif { orientation: 6, thumbnail: Some(16), ..Exif::default() };
    let upright = scene.render().unwrap();
    vec![
        ("webp lossless", webp(&scene, &Webp::default()).unwrap()),
        ("webp alpha exif icc", webp(&scene, &Webp { alpha: true, exif: Some(exif), icc: Some(icc::rgb(Space::AdobeRgb, Version::V2)) }).unwrap()),
        ("webp lossy", lossy_webp(40, 28, None)),
        ("webp lossy alpha", lossy_webp(40, 28, Some(&alpha(40, 28)))),
        ("webp lossy picture", lossy_picture(72, 56)),
        ("webp animated", animated_webp(&scene, &[7, 8], true)),
        ("gif", gif(&scene, 1).unwrap()),
        ("gif animated", gif(&scene, 3).unwrap()),
        ("bmp rgb", bmp(&scene, Colour::Rgb).unwrap()),
        ("bmp palette", bmp(&scene, Colour::Palette).unwrap()),
        ("bmp v5", bmp_v5(&upright, &alpha(40, 28), &icc::rgb(Space::DisplayP3, Version::V4))),
    ]
}

/// Every prefix of every file: `Ok` with the full size, or `Err`, never a panic, and never a
/// caught panic inside a third-party decoder either.
#[test]
fn truncation_at_every_offset_never_panics() {
    let l = Limits::default();
    for (name, bytes) in files() {
        let full = read_header(&bytes, &l).unwrap();
        decode(&bytes, &l).unwrap();
        for len in 0..bytes.len() {
            let cut = &bytes[..len];
            if let Ok(h) = read_header(cut, &l) {
                assert_eq!((h.width, h.height), (full.width, full.height), "{name}: prefix {len}");
            }
            let format = full.format;
            let results =
                [decode(cut, &l).map(|_| ()), decode_thumbnail(cut, 32, &l).map(|_| ()), formats::embedded_thumbnail(format, cut, &l).map(|_| ())];
            for result in results {
                assert!(!matches!(result, Err(Error::Panic(_))), "{name}: prefix {len} panicked in the decoder");
            }
        }
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    /// Bytes changed anywhere in a real file, then cut anywhere: never a panic.
    #[test]
    fn corrupted_files_never_panic(which in 0usize..11, cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let (_, mut bytes) = files().swap_remove(which);
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1));
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&bytes, &l);
        let _ = decode(&bytes, &l);
        let _ = decode_thumbnail(&bytes, 32, &l);
    }
}
