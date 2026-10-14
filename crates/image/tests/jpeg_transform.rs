//! Lossless JPEG rotation and flips (`jpeg_transform`, `data-model.md` §6): each of the eight
//! transforms on scenes JPEGs in every sampling decodes to the transformed samples of the source
//! within 1 level and reads back the transformed coefficients exactly; files and sizes the
//! transform cannot handle answer "not lossless here"; damaged input never panics.
//!
//! Sources: 4:4:4 and grey files from the `image` crate's encoder (through `dalica-scenes`), and
//! 4:4:4, 4:2:2, 4:2:0 and grey files from a small forward-DCT encoder below whose coefficients go
//! through `jpeg_transform::encode` (the `image` encoder writes 4:4:4 only).

use std::f32::consts::PI;
use std::time::Instant;

use dalica_image::jpeg_transform::{self, Coefficients, Component, NotLossless, Refusal};
use dalica_image::{Limits, Orientation, Rgba8, decode, orient, read_header};
use dalica_scenes::{Exif, Scene, jpeg};
use proptest::prelude::*;

/// The natural-order index of each zig-zag position (T.81 Figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sampling {
    Grey,
    S444,
    S422,
    S420,
}

impl Sampling {
    const ALL: [Sampling; 4] = [Sampling::Grey, Sampling::S444, Sampling::S422, Sampling::S420];

    /// The luma component's sampling factors.
    fn luma(self) -> (u8, u8) {
        match self {
            Sampling::Grey | Sampling::S444 => (1, 1),
            Sampling::S422 => (2, 1),
            Sampling::S420 => (2, 2),
        }
    }
}

fn o(v: u8) -> Orientation {
    Orientation::new(v).unwrap()
}

/// The quantisation table `t` in natural order: coarser with frequency, coarser for chroma, and
/// not symmetric (as real tables are not), so a transpose that forgot the tables would show.
fn quant(t: u8) -> [u16; 64] {
    std::array::from_fn(|i| {
        let (u, v) = ((i % 8) as u16, (i / 8) as u16);
        if t == 0 { 2 + 2 * u + v } else { 4 + 3 * u + 2 * v }
    })
}

/// DQT segments for tables 0 and 1 (T.81 §B.2.4.1), values in zig-zag order.
fn dqt() -> Vec<u8> {
    let mut seg = vec![0xff, 0xdb, 0, 132];
    for t in 0..2u8 {
        seg.push(t);
        let q = quant(t);
        seg.extend(ZIGZAG.iter().map(|&n| q[n] as u8));
    }
    seg
}

/// Quantised 8 × 8 forward DCT (T.81 §A.3.3) of level-shifted samples.
fn fdct(samples: &[f32; 64], q: &[u16; 64]) -> [i16; 64] {
    let c = |k: usize| if k == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
    std::array::from_fn(|i| {
        let (u, v) = (i % 8, i / 8);
        let mut sum = 0.0;
        for y in 0..8 {
            for x in 0..8 {
                sum += (samples[y * 8 + x] - 128.0)
                    * ((2 * x + 1) as f32 * u as f32 * PI / 16.0).cos()
                    * ((2 * y + 1) as f32 * v as f32 * PI / 16.0).cos();
            }
        }
        (0.25 * c(u) * c(v) * sum / f32::from(q[i])).round() as i16
    })
}

/// A JPEG of `scene` with `sampling`, coded by our own forward DCT and `jpeg_transform::encode`.
fn scene_jpeg(scene: &Scene, sampling: Sampling, restart: u16) -> Vec<u8> {
    let img = scene.render().unwrap();
    let (w, h) = (img.width, img.height);
    let px = |x: u32, y: u32| {
        let at = ((y.min(h - 1) * w + x.min(w - 1)) * 3) as usize;
        let [r, g, b] = [img.data[at], img.data[at + 1], img.data[at + 2]].map(f32::from);
        [0.299 * r + 0.587 * g + 0.114 * b, -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0, 0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0]
    };
    let (hmax, vmax) = sampling.luma();
    let channels: &[(u8, u8, u8, usize)] =
        if sampling == Sampling::Grey { &[(1, 1, 1, 0)] } else { &[(1, hmax, vmax, 0), (2, 1, 1, 1), (3, 1, 1, 2)] };
    let multi = channels.len() > 1;
    let mut components = Vec::new();
    for &(id, ch, cv, channel) in channels {
        let (bw, bh) = if multi {
            (w.div_ceil(8 * u32::from(hmax)) * u32::from(ch), h.div_ceil(8 * u32::from(vmax)) * u32::from(cv))
        } else {
            (w.div_ceil(8), h.div_ceil(8))
        };
        let (sx, sy) = (u32::from(hmax / ch), u32::from(vmax / cv));
        let tq = u8::from(channel > 0);
        let mut blocks = Vec::new();
        for by in 0..bh {
            for bx in 0..bw {
                let samples: [f32; 64] = std::array::from_fn(|i| {
                    let (x, y) = (bx * 8 + (i % 8) as u32, by * 8 + (i / 8) as u32);
                    let mut sum = 0.0;
                    for j in 0..sy {
                        for k in 0..sx {
                            sum += px(x * sx + k, y * sy + j)[channel];
                        }
                    }
                    sum / (sx * sy) as f32
                });
                blocks.push(fdct(&samples, &quant(tq)));
            }
        }
        components.push(Component { id, h: ch, v: cv, tq, blocks_wide: bw, blocks_high: bh, blocks });
    }
    jpeg_transform::encode(&Coefficients { width: w, height: h, components }, &dqt(), restart).unwrap()
}

/// A grey JPEG of `scene` through the `image` crate's encoder.
fn image_grey_jpeg(scene: &Scene) -> Vec<u8> {
    use image::ImageEncoder;
    let img = scene.render().unwrap();
    let grey: Vec<u8> = img.data.chunks(3).map(|p| ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).write_image(&grey, img.width, img.height, image::ExtendedColorType::L8).unwrap();
    out
}

fn decoded(bytes: &[u8]) -> Rgba8 {
    decode(bytes, &Limits::default()).unwrap()
}

/// The decoder's samples before colour conversion (Y, Cb, Cr upsampled, or grey), packed as RGBA
/// so `orient::apply` turns them.
fn decoded_ycc(bytes: &[u8]) -> Rgba8 {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let mut d = zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(bytes), DecoderOptions::default());
    d.decode_headers().unwrap();
    let grey = d.input_colorspace() == Some(ColorSpace::Luma);
    let out = if grey { ColorSpace::Luma } else { ColorSpace::YCbCr };
    let mut d = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(bytes),
        DecoderOptions::default().jpeg_set_out_colorspace(out),
    );
    let samples = d.decode().unwrap();
    let info = d.info().unwrap();
    let data =
        if grey { samples.iter().flat_map(|&y| [y, y, y, 255]).collect() } else { samples.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    Rgba8 { width: u32::from(info.width), height: u32::from(info.height), data }
}

/// The largest per-channel difference between two images of the same size.
fn max_diff(a: &Rgba8, b: &Rgba8) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.data.iter().zip(&b.data).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// Every source the transforms are checked on: (name, bytes), at MCU-multiple sizes.
fn sources() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for s in Sampling::ALL {
        out.push((format!("ours-{s:?}"), scene_jpeg(&Scene::new(3, 96, 64), s, 0)));
    }
    out.push(("ours-S420-restart".into(), scene_jpeg(&Scene::new(4, 64, 96), Sampling::S420, 3)));
    out.push(("image-S444".into(), jpeg(&Scene::new(5, 96, 64), 90, None).unwrap()));
    out.push(("image-grey".into(), image_grey_jpeg(&Scene::new(6, 64, 96))));
    out
}

#[test]
fn the_test_encoder_writes_what_the_scene_shows() {
    let scene = Scene::new(3, 96, 64);
    let img = scene.render().unwrap();
    for s in [Sampling::S444, Sampling::S422, Sampling::S420] {
        let d = decoded(&scene_jpeg(&scene, s, 0));
        let mean = d.data.chunks(4).zip(img.data.chunks(3)).map(|(a, b)| (0..3).map(|i| u32::from(a[i].abs_diff(b[i]))).sum::<u32>()).sum::<u32>()
            as f64
            / (96.0 * 64.0 * 3.0);
        assert!(mean < 6.0, "{s:?}: mean difference {mean}");
    }
}

#[test]
fn each_transform_turns_pixels_and_coefficients() {
    let limits = Limits::default();
    for (name, src) in sources() {
        let before = jpeg_transform::read(&src, &limits).unwrap();
        let pixels = decoded(&src);
        for v in 1..=8 {
            let out = jpeg_transform::transform(&src, o(v), &limits).unwrap_or_else(|e| panic!("{name} {v}: {e}"));
            let h = read_header(&out, &limits).unwrap();
            assert_eq!((h.width, h.height), o(v).display_size(pixels.width, pixels.height), "{name} {v}");
            // The transformed coefficients, exactly.
            assert_eq!(jpeg_transform::read(&out, &limits).unwrap(), before.transformed(o(v)).unwrap(), "{name} {v}");
            // The transformed samples within 1 level (the decoder's inverse DCT rounds its row and
            // column passes differently once they swap); mirrors decode exactly.
            let samples = decoded_ycc(&src);
            let diff = max_diff(&decoded_ycc(&out), &orient::apply(samples, o(v)).unwrap());
            assert!(diff <= 1, "{name} orientation {v}: samples differ by {diff}");
            let diff = max_diff(&decoded(&out), &orient::apply(pixels.clone(), o(v)).unwrap());
            assert!(diff <= if o(v).swaps() { 3 } else { 0 }, "{name} orientation {v}: pixels differ by {diff}");
        }
    }
}

#[test]
fn four_quarter_turns_give_the_original_coefficients() {
    let limits = Limits::default();
    for (name, src) in sources() {
        let mut bytes = src.clone();
        for _ in 0..4 {
            bytes = jpeg_transform::transform(&bytes, o(6), &limits).unwrap();
        }
        assert_eq!(jpeg_transform::read(&bytes, &limits).unwrap(), jpeg_transform::read(&src, &limits).unwrap(), "{name}");
        // Two half turns and two mirrors too.
        for v in [2, 3, 4, 5, 7] {
            let twice = jpeg_transform::transform(&jpeg_transform::transform(&src, o(v), &limits).unwrap(), o(v), &limits).unwrap();
            assert_eq!(jpeg_transform::read(&twice, &limits).unwrap(), jpeg_transform::read(&src, &limits).unwrap(), "{name} {v} twice");
        }
    }
}

#[test]
fn other_segments_are_copied_byte_for_byte() {
    let limits = Limits::default();
    let exif = Exif { orientation: 6, thumbnail: Some(32), ..Exif::default() };
    let src = jpeg(&Scene::new(8, 64, 48), 85, Some(&exif)).unwrap();
    // The segments before the scan, walked from SOI: the first one with `marker`.
    let app = |b: &[u8], marker: u8| -> Vec<u8> {
        let mut at = 2;
        while b[at + 1] != 0xda {
            let len = usize::from(u16::from_be_bytes([b[at + 2], b[at + 3]]));
            if b[at + 1] == marker {
                return b[at..at + 2 + len].to_vec();
            }
            at += 2 + len;
        }
        panic!("no segment {marker:02X}");
    };
    let flipped = jpeg_transform::transform(&src, o(2), &limits).unwrap();
    let out = jpeg_transform::transform(&src, o(6), &limits).unwrap();
    for (marker, what) in [(0xe0, "JFIF"), (0xe1, "EXIF")] {
        assert_eq!(app(&out, marker), app(&src, marker), "{what}");
        assert_eq!(app(&flipped, marker), app(&src, marker), "{what}");
    }
    // Quantisation tables are copied by a mirror and transposed by a quarter turn (the encoder's
    // tables are not symmetric); turning back restores them.
    assert_eq!(app(&flipped, 0xdb), app(&src, 0xdb));
    assert_ne!(app(&out, 0xdb), app(&src, 0xdb));
    assert_eq!(app(&jpeg_transform::transform(&out, o(8), &limits).unwrap(), 0xdb), app(&src, 0xdb));
    // The orientation tag is the caller's to update: still 6 here.
    assert_eq!(read_header(&out, &limits).unwrap().orientation.get(), 6);
    // The restart interval is kept.
    let restarting = scene_jpeg(&Scene::new(9, 64, 64), Sampling::S422, 2);
    let out = jpeg_transform::transform(&restarting, o(8), &limits).unwrap();
    assert_eq!(app(&out, 0xdd), [0xff, 0xdd, 0, 4, 0, 2]);
    assert!(out.windows(2).any(|w| w == [0xff, 0xd3]));
}

#[test]
fn partial_mcus_move_only_to_trailing_edges() {
    let limits = Limits::default();
    // 4:2:0 at 70 × 48: the right edge is a partial MCU column, the bottom edge whole.
    let src = scene_jpeg(&Scene::new(10, 70, 48), Sampling::S420, 0);
    let pixels = decoded(&src);
    for v in 1..=8 {
        match jpeg_transform::transform(&src, o(v), &limits) {
            Ok(out) => {
                assert!([1, 4, 5, 6].contains(&v), "orientation {v} moved a partial MCU");
                let diff = max_diff(&decoded_ycc(&out), &orient::apply(decoded_ycc(&src), o(v)).unwrap());
                assert!(diff <= 1, "orientation {v}: samples differ by {diff}");
                assert_eq!(decoded(&out).width, o(v).display_size(pixels.width, pixels.height).0);
            }
            Err(e) => {
                assert_eq!(e, Refusal::NotLossless(NotLossless::PartialMcu), "orientation {v}");
                assert!([2, 3, 7, 8].contains(&v));
            }
        }
    }
    // Grey files move in 8-pixel blocks: 72 × 40 turns every way.
    let grey = scene_jpeg(&Scene::new(11, 72, 40), Sampling::Grey, 0);
    assert!((1..=8).all(|v| jpeg_transform::transform(&grey, o(v), &limits).is_ok()));
    let ragged = scene_jpeg(&Scene::new(11, 72, 41), Sampling::Grey, 0);
    assert_eq!(jpeg_transform::transform(&ragged, o(4), &limits), Err(Refusal::NotLossless(NotLossless::PartialMcu)));
}

/// The bytes of `src` with its frame header's marker and precision replaced.
fn with_frame(src: &[u8], marker: u8, precision: u8) -> Vec<u8> {
    let mut b = src.to_vec();
    let at = b.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    b[at + 1] = marker;
    b[at + 4] = precision;
    b
}

#[test]
fn other_processes_are_not_lossless_here() {
    let limits = Limits::default();
    let src = scene_jpeg(&Scene::new(12, 32, 32), Sampling::S420, 0);
    let refused = |b: &[u8]| match jpeg_transform::transform(b, o(6), &limits) {
        Err(Refusal::NotLossless(n)) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!(refused(&with_frame(&src, 0xc2, 8)), NotLossless::Progressive);
    assert_eq!(refused(&with_frame(&src, 0xc9, 8)), NotLossless::Arithmetic);
    assert_eq!(refused(&with_frame(&src, 0xc3, 8)), NotLossless::LosslessCoding);
    assert_eq!(refused(&with_frame(&src, 0xc5, 8)), NotLossless::Hierarchical);
    assert_eq!(refused(&with_frame(&src, 0xc1, 12)), NotLossless::Precision(12));
    // Extended sequential with 8-bit samples is transformed.
    assert!(jpeg_transform::transform(&with_frame(&src, 0xc1, 8), o(6), &limits).is_ok());
    // A second image after the first.
    let mut two = src.clone();
    two.extend_from_slice(&src);
    assert_eq!(refused(&two), NotLossless::TrailingData);
    // Not a JPEG at all is damaged, not "not lossless".
    assert!(matches!(jpeg_transform::transform(b"\x89PNG\r\n\x1a\n", o(6), &limits), Err(Refusal::Damaged(_))));
    assert!(matches!(jpeg_transform::read(&[], &limits), Err(Refusal::Damaged(_))));
}

#[test]
fn the_decode_limits_apply() {
    let src = scene_jpeg(&Scene::new(13, 64, 64), Sampling::S444, 0);
    let tight = Limits { max_alloc: 64 * 64 * 2, ..Limits::default() };
    assert!(matches!(jpeg_transform::transform(&src, o(6), &tight), Err(Refusal::Damaged(dalica_image::Error::TooLarge { .. }))));
    let small = Limits { max_side: 32, ..Limits::default() };
    assert!(matches!(jpeg_transform::read(&src, &small), Err(Refusal::Damaged(dalica_image::Error::TooLarge { .. }))));
}

#[test]
fn inconsistent_coefficients_are_refused() {
    let c = jpeg_transform::read(&scene_jpeg(&Scene::new(14, 32, 16), Sampling::S422, 0), &Limits::default()).unwrap();
    let mut short = c.clone();
    short.components[1].blocks.pop();
    assert!(jpeg_transform::encode(&short, &dqt(), 0).is_err());
    assert!(short.transformed(o(6)).is_err());
    let mut wide = c.clone();
    wide.components[0].blocks_wide += 1;
    assert!(jpeg_transform::encode(&wide, &dqt(), 0).is_err());
    let mut hot = c;
    hot.components[0].blocks[0][1] = 2000;
    assert!(jpeg_transform::encode(&hot, &dqt(), 0).is_err());
}

/// Every prefix of real files: `Ok` only for a file whose scan is complete (a missing EOI is
/// accepted), and then the same file as from the whole input; never a panic.
#[test]
fn truncation_at_every_offset_never_panics() {
    let limits = Limits::default();
    let exif = Exif { orientation: 3, thumbnail: Some(16), ..Exif::default() };
    for src in [scene_jpeg(&Scene::new(15, 32, 32), Sampling::S420, 2), jpeg(&Scene::new(16, 24, 16), 80, Some(&exif)).unwrap()] {
        let full = jpeg_transform::transform(&src, o(6), &limits).unwrap();
        for len in 0..src.len() {
            if let Ok(out) = jpeg_transform::transform(&src[..len], o(6), &limits) {
                assert_eq!(out, full, "prefix {len}");
                assert!(len >= src.len() - 2, "prefix {len} of {}", src.len());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Corruption anywhere in a real file never panics; whatever comes out decodes.
    #[test]
    fn corrupted_files_never_panic(v in 1u8..=8, flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = scene_jpeg(&Scene::new(17, 32, 32), Sampling::S420, 1);
        for (at, x) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= x;
        }
        if let Ok(out) = jpeg_transform::transform(&bytes, o(v), &Limits::default()) {
            prop_assert!(jpeg_transform::read(&out, &Limits::default()).is_ok());
        }
    }

    #[test]
    fn random_bytes_never_panic(v in 1u8..=8, tail in proptest::collection::vec(any::<u8>(), 0..512)) {
        let bytes = [b"\xff\xd8\xff".to_vec(), tail].concat();
        let _ = jpeg_transform::transform(&bytes, o(v), &Limits::default());
        let _ = jpeg_transform::read(&bytes, &Limits::default());
    }
}

#[test]
fn time_for_24_megapixels() {
    let limits = Limits::default();
    let src = jpeg(&Scene::new(24, 6000, 4000), 90, None).unwrap();
    let t = Instant::now();
    let out = jpeg_transform::transform(&src, o(6), &limits).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("24 MP 4:4:4 ({:.1} MB) quarter turn: {ms:.0} ms, {:.1} MB out", src.len() as f64 / 1e6, out.len() as f64 / 1e6);
    let h = read_header(&out, &limits).unwrap();
    assert_eq!((h.width, h.height), (4000, 6000));
}
