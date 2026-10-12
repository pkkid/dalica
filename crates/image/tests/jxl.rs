//! JPEG XL through `jxl-oxide` (`formats.md` §1): lossless files written by `scenes` (grey and RGB,
//! 8 and 16 bits, alpha at 8) decode to exactly the samples they store and match their fingerprints
//! (`fingerprints/jxl-*.json`, `architecture.md` §11); the box container reads like a bare
//! codestream; the eight orientations and a Display P3 colour encoding, written into the image
//! header here, come out upright and converted by the colour stage; declared sizes past the limits
//! and decoders running out of their allocation are refused; damaged files never panic.
//!
//! Orientation and colour encoding are image header fields `zune-jpegxl` always leaves at their
//! defaults, so the tests rewrite its header (ISO/IEC 18181-1 §A.3–A.6: SizeHeader and
//! ImageMetadata, then padding to a byte, after which the frame starts unchanged).

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;

use dalica_image::{DecodeOptions, Error, Format, Limits, Rgba8, decode, decode_as, decode_thumbnail, decode_with, formats, read_header, sniff};
use dalica_scenes::formats::{Colour, Depth, SampleData, alpha, jxl, samples};
use dalica_scenes::{Rgb8, Scene, fingerprint, orient};
use moxcms::{ColorProfile, Layout};
use proptest::prelude::*;
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
}

fn distance(a: &Rgba8, b: &Rgba8) -> f64 {
    fingerprint::distance(&fingerprint::fingerprint(a.width, a.height, &a.data), &fingerprint::fingerprint(b.width, b.height, &b.data))
}

/// Fails (writing actual/expected PNGs) unless the fingerprints agree within `tol` per value.
fn assert_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let d = distance(actual, expected);
    if d > tol || (actual.width, actual.height) != (expected.width, expected.height) {
        let dir = out_dir();
        std::fs::write(dir.join(format!("{case}-actual.png")), fingerprint::png_rgba(actual.width, actual.height, &actual.data)).unwrap();
        std::fs::write(dir.join(format!("{case}-expected.png")), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
        panic!("{case}: {d:.2} away from its reference, tolerance {tol} (PNGs in {})", dir.display());
    }
}

/// Compares `img` with `fingerprints/<name>.json` (`DALICA_BLESS=1` stores it instead).
fn assert_fingerprint(name: &str, img: &Rgba8, tol: f64) {
    let dir = out_dir();
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.data)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(img.width, img.height, &img.data, &expected, tol) {
        std::fs::write(dir.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {} (images in {})", m.message, dir.display());
    }
}

fn to8_16(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32_767) / 65_535) as u8
}

/// What a lossless file of `colour` and `depth` must decode to: its stored samples, grey spread to
/// RGB, 16 bits rounded to 8, and the `scenes` alpha mask when it has one.
fn expected(img: &Rgb8, colour: Colour, depth: Depth, with_alpha: bool) -> Rgba8 {
    let s = samples(img, colour, depth).unwrap();
    let v8: Vec<u8> = match s.data {
        SampleData::U8(v) => v,
        SampleData::U16(v) => v.into_iter().map(to8_16).collect(),
        SampleData::F32(_) => unreachable!(),
    };
    let a = if with_alpha { alpha(img.width, img.height) } else { vec![255; (img.width * img.height) as usize] };
    let data = match colour {
        Colour::Grey => v8.iter().zip(&a).flat_map(|(&g, &a)| [g, g, g, a]).collect(),
        _ => v8.as_chunks::<3>().0.iter().zip(&a).flat_map(|(&[r, g, b], &a)| [r, g, b, a]).collect(),
    };
    Rgba8 { width: img.width, height: img.height, data }
}

const LAYOUTS: [(Colour, Depth, bool, &str); 6] = [
    (Colour::Grey, Depth::Eight, false, "grey8"),
    (Colour::Grey, Depth::Eight, true, "greya8"),
    (Colour::Grey, Depth::Sixteen, false, "grey16"),
    (Colour::Rgb, Depth::Eight, false, "rgb8"),
    (Colour::Rgb, Depth::Eight, true, "rgba8"),
    (Colour::Rgb, Depth::Sixteen, false, "rgb16"),
];

// ---- Rewriting the image header ----------------------------------------------------------------

/// Bits in JPEG XL order: each field least significant bit first, fields packed from the low bit
/// of each byte.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: u32,
}

impl Bits {
    fn put(&mut self, n: u32, v: u64) {
        for i in 0..n {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = ((v >> i) & 1) as u8;
            *self.bytes.last_mut().unwrap() |= bit << (self.used % 8);
            self.used += 1;
        }
    }

    /// A size as `U32(1 + u(9), 1 + u(13), 1 + u(18), 1 + u(30))`.
    fn size(&mut self, v: u32) {
        let v = u64::from(v - 1);
        let (sel, bits) = [(0, 9), (1, 13), (2, 18), (3, 30)].into_iter().find(|&(_, b)| v < 1 << b).unwrap();
        self.put(2, sel);
        self.put(bits, v);
    }

    /// An enum value as `U32(0, 1, 2 + u(4), 18 + u(6))`.
    fn enumerated(&mut self, v: u64) {
        match v {
            0 | 1 => self.put(2, v),
            2..=17 => {
                self.put(2, 2);
                self.put(4, v - 2);
            }
            _ => {
                self.put(2, 3);
                self.put(6, v - 18);
            }
        }
    }
}

/// The image header fields the tests set.
#[derive(Clone, Copy)]
struct Meta {
    width: u32,
    height: u32,
    grey: bool,
    bits: u32,
    alpha: bool,
    /// 1–8; 0 leaves the extra fields out, as `zune-jpegxl` does.
    orientation: u8,
    /// Display P3 primaries instead of sRGB's (RGB only).
    p3: bool,
}

/// The signature, SizeHeader and ImageMetadata of a lossless modular image, padded to a byte.
fn image_header(m: Meta) -> Vec<u8> {
    let mut b = Bits::default();
    b.put(16, 0x0aff);
    b.put(1, 0); // not small
    b.size(m.height);
    b.put(3, 0); // no ratio: the width follows
    b.size(m.width);
    b.put(1, 0); // not all default
    b.put(1, u64::from(m.orientation != 0)); // extra fields
    if m.orientation != 0 {
        b.put(3, u64::from(m.orientation - 1));
        b.put(3, 0); // no intrinsic size, preview or animation
    }
    b.put(1, 0); // integer samples
    if m.bits == 8 {
        b.put(2, 0);
    } else {
        b.put(2, 3);
        b.put(6, u64::from(m.bits - 1));
    }
    b.put(1, u64::from(m.bits <= 14)); // 16-bit buffers suffice
    if m.alpha {
        b.put(2, 1); // one extra channel,
        b.put(1, 1); // an 8-bit alpha channel (all default)
    } else {
        b.put(2, 0);
    }
    b.put(1, 0); // not XYB
    if m.grey || m.p3 {
        b.put(1, 0); // colour encoding not all default
        b.put(1, 0); // no ICC profile
        b.enumerated(u64::from(m.grey)); // RGB 0, grey 1
        b.enumerated(1); // D65
        if !m.grey {
            b.enumerated(if m.p3 { 11 } else { 1 }); // P3 or sRGB primaries
        }
        b.put(1, 0); // no gamma: a transfer function
        b.enumerated(13); // sRGB
        b.enumerated(1); // relative colorimetric
    } else {
        b.put(1, 1); // sRGB
    }
    if m.orientation != 0 {
        b.put(1, 1); // tone mapping all default
    }
    b.put(2, 0); // no extensions
    b.put(1, 1); // default transform data
    b.bytes
}

/// A lossless codestream of `pixels` (interleaved, native-endian 16-bit) from `zune-jpegxl`, with
/// its image header replaced by `m`'s.
fn codestream(pixels: &[u8], m: Meta) -> Vec<u8> {
    let space = match (m.grey, m.alpha) {
        (true, false) => ColorSpace::Luma,
        (true, true) => ColorSpace::LumaA,
        (false, false) => ColorSpace::RGB,
        (false, true) => ColorSpace::RGBA,
    };
    let depth = if m.bits == 8 { BitDepth::Eight } else { BitDepth::Sixteen };
    let mut out = Vec::new();
    JxlSimpleEncoder::new(pixels, EncoderOptions::new(m.width as usize, m.height as usize, space, depth)).encode(&mut out).unwrap();
    // Our writer reproduces the encoder's own header exactly, which shows where its frame starts.
    let plain = image_header(Meta { orientation: 0, p3: false, ..m });
    assert_eq!(out[..plain.len()], plain[..], "the encoder's image header");
    [image_header(m), out[plain.len()..].to_vec()].concat()
}

/// A JPEG XL container: the signature box, `ftyp`, and the codestream in one `jxlc` box.
fn container(codestream: &[u8]) -> Vec<u8> {
    let boxed = |kind: &[u8; 4], payload: &[u8]| [&(8 + payload.len() as u32).to_be_bytes()[..], kind, payload].concat();
    [boxed(b"JXL ", b"\r\n\x87\n"), boxed(b"ftyp", b"jxl \0\0\0\0jxl "), boxed(b"jxlc", codestream)].concat()
}

// ---- Tests --------------------------------------------------------------------------------------

#[test]
fn scenes_files_decode_exactly_and_match_their_fingerprints() {
    let scene = Scene::new(61, 96, 64);
    let img = scene.render().unwrap();
    for (colour, depth, with_alpha, name) in LAYOUTS {
        let bytes = jxl(&scene, colour, depth, with_alpha).unwrap();
        assert_eq!(sniff(&bytes), Some(Format::JpegXl));
        let h = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation.get(), h.thumbnail.clone()), (Format::JpegXl, 96, 64, 1, None), "{name}");
        assert_eq!(formats::embedded_thumbnail(Format::JpegXl, &bytes, &Limits::default()), Ok(None));
        let decoded = decode(&bytes, &Limits::default()).unwrap();
        assert!(decoded == expected(&img, colour, depth, with_alpha), "{name}: not the stored samples");
        assert_eq!(decode_as(Format::JpegXl, &bytes, &ON, &Limits::default()).unwrap(), decoded, "{name}");
        // Untagged sRGB: colour management changes nothing.
        assert_eq!(decode_with(&bytes, &OFF, &Limits::default()).unwrap(), decoded, "{name}");
        assert_fingerprint(&format!("jxl-{name}"), &decoded, 0.5);
        // In a box container, the same pixels.
        let boxed = container(&bytes);
        assert_eq!(sniff(&boxed), Some(Format::JpegXl));
        assert_eq!(decode(&boxed, &Limits::default()).unwrap(), decoded, "{name} in a container");
    }
    let t = decode_thumbnail(&jxl(&scene, Colour::Rgb, Depth::Eight, false).unwrap(), 48, &Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (48, 32));
}

#[test]
fn every_orientation_decodes_upright() {
    let upright = Scene::new(62, 60, 40).render().unwrap();
    for o in 1..=8u8 {
        let stored = orient::store(&upright, o).unwrap();
        let m = Meta { width: stored.width, height: stored.height, grey: false, bits: 8, alpha: false, orientation: o, p3: false };
        let bytes = codestream(&stored.data, m);
        let h = read_header(&bytes, &Limits::default()).unwrap();
        // The decoder turns the pixels upright, so the header gives the upright size.
        assert_eq!((h.width, h.height, h.orientation.get(), h.display_size()), (60, 40, 1, (60, 40)), "orientation {o}");
        assert!(decode(&bytes, &Limits::default()).unwrap() == rgba(&upright), "orientation {o}");
        assert!(decode(&container(&bytes), &Limits::default()).unwrap() == rgba(&upright), "orientation {o} in a container");
    }
}

/// A Display P3 image: the profile the decoder describes it with goes through the colour stage.
#[test]
fn a_display_p3_image_converts_to_srgb() {
    let img = Scene::new(63, 120, 80).render().unwrap();
    let to_p3 = ColorProfile::new_srgb()
        .create_transform_8bit(Layout::Rgb, &ColorProfile::new_display_p3(), Layout::Rgb, dalica_image::color::transform_options(8))
        .unwrap();
    let mut p3 = vec![0u8; img.data.len()];
    to_p3.transform(&img.data, &mut p3).unwrap();
    let m = Meta { width: 120, height: 80, grey: false, bits: 8, alpha: false, orientation: 0, p3: true };
    let bytes = codestream(&p3, m);
    let on = decode_with(&bytes, &ON, &Limits::default()).unwrap();
    assert_close("jxl-p3-vs-source", &on, &rgba(&img), 1.5);
    assert_fingerprint("jxl-rgb8-p3", &on, 1.0);
    // Without colour management, the P3 samples as they are.
    let off = decode_with(&bytes, &OFF, &Limits::default()).unwrap();
    assert_eq!(off.data, p3.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect::<Vec<u8>>());
    assert!(distance(&on, &off) > 2.0, "{}", distance(&on, &off));
    // An explicitly grey sRGB encoding is untagged grey (no profile to apply).
    let grey = Scene::new(64, 40, 30);
    let bytes = jxl(&grey, Colour::Grey, Depth::Eight, false).unwrap();
    assert_eq!(decode_with(&bytes, &ON, &Limits::default()), decode_with(&bytes, &OFF, &Limits::default()));
}

/// A declared size past the limits is refused from the header alone; allocating for it would
/// abort the test process, so passing at all shows nothing was allocated.
#[test]
fn a_declared_size_past_the_limits_is_refused_before_allocating() {
    let start = Instant::now();
    let small = Scene::new(65, 16, 16).render().unwrap();
    let real = codestream(&small.data, Meta { width: 16, height: 16, grey: false, bits: 8, alpha: false, orientation: 0, p3: false });
    let plain = image_header(Meta { width: 16, height: 16, grey: false, bits: 8, alpha: false, orientation: 0, p3: false });
    // Past the pixel cap, past the allocation limit, past the side limit. (Larger sizes are refused
    // by the decoder itself when it reads the frame header: at most 65 536 groups.)
    for (w, h) in [(40_000, 40_000), (30_000, 30_000), (2, 1_000_000), (70_000, 2)] {
        let bytes =
            [image_header(Meta { width: w, height: h, grey: false, bits: 8, alpha: false, orientation: 0, p3: false }), real[plain.len()..].to_vec()]
                .concat();
        assert_eq!(read_header(&bytes, &Limits::default()).map(|h| (h.width, h.height)), Ok((w, h)));
        assert!(matches!(decode(&bytes, &Limits::default()), Err(Error::TooLarge { .. })), "{w} × {h}");
        assert!(matches!(decode_thumbnail(&bytes, 64, &Limits::default()), Err(Error::TooLarge { .. })), "{w} × {h}");
    }
    assert!(start.elapsed().as_millis() < 500, "{:?}", start.elapsed());
}

/// The decoder's own buffers are charged to the decode's allocation limit: with room for the
/// output but not for them, the decode is "too large", not an abort.
#[test]
fn the_decoder_runs_inside_the_allocation_limit() {
    let scene = Scene::new(66, 256, 256);
    let bytes = jxl(&scene, Colour::Rgb, Depth::Sixteen, false).unwrap();
    let pixels = 256 * 256;
    assert!(decode(&bytes, &Limits { max_alloc: pixels * 64, ..Limits::default() }).is_ok());
    let tight = Limits { max_alloc: pixels * 4, ..Limits::default() };
    assert_eq!(decode(&bytes, &tight), Err(Error::TooLarge { width: 256, height: 256 }));
    assert_eq!(Error::TooLarge { width: 256, height: 256 }.reason(), "Too large to display (256 × 256)");
}

#[test]
fn damaged_files_are_refused_and_the_reason_text() {
    let scene = Scene::new(67, 32, 24);
    let bytes = jxl(&scene, Colour::Rgb, Depth::Eight, true).unwrap();
    // The frame cut short: the header reads, the decode fails as damaged.
    let cut = &bytes[..bytes.len() - 8];
    assert_eq!(read_header(cut, &Limits::default()).map(|h| (h.width, h.height)), Ok((32, 24)));
    assert_eq!(decode(cut, &Limits::default()).unwrap_err().reason(), "Damaged file");
    // Only the signature: truncated.
    assert_eq!(read_header(&bytes[..2], &Limits::default()), Err(Error::Truncated));
    // A header longer than the header limit is truncated.
    assert_eq!(read_header(&bytes, &Limits { max_header_bytes: 3, ..Limits::default() }), Err(Error::Truncated));
    assert!(Format::JpegXl.supported() && formats::decodes(Format::JpegXl));
    assert_eq!(Error::Unsupported(Format::JpegXl).reason(), "This kind of JPEG XL file isn't supported");
}

/// Every prefix of real files (codestream and container): `Ok` with the right size, or `Err`,
/// never a panic.
#[test]
fn truncation_at_every_offset_never_panics() {
    let scene = Scene::new(68, 24, 16);
    let bytes = jxl(&scene, Colour::Rgb, Depth::Eight, true).unwrap();
    for file in [bytes.clone(), container(&bytes)] {
        for len in 0..file.len() {
            if let Ok(h) = read_header(&file[..len], &Limits::default()) {
                assert_eq!((h.width, h.height), (24, 16), "prefix {len}");
            }
            if let Ok(img) = decode(&file[..len], &Limits::default()) {
                assert_eq!((img.width, img.height), (24, 16), "prefix {len}");
            }
        }
    }
}

/// Every layout, as a bare codestream and in a container (encoded once: the encoder is slow in
/// debug builds).
static SMALL_FILES: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    let scene = Scene::new(69, 20, 14);
    LAYOUTS.iter().map(|&(colour, depth, with_alpha, _)| jxl(&scene, colour, depth, with_alpha).unwrap()).flat_map(|b| [container(&b), b]).collect()
});

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Corruption and truncation anywhere in real files never panic.
    #[test]
    fn corrupted_files_never_panic(file in 0..2 * LAYOUTS.len(), cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6)) {
        let mut bytes = SMALL_FILES[file].clone();
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1));
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&bytes, &l);
        let _ = decode(&bytes, &l);
        let _ = decode_thumbnail(&bytes, 8, &l);
        let _ = decode_as(Format::JpegXl, &bytes, &OFF, &l);
        let _ = formats::embedded_thumbnail(Format::JpegXl, &bytes, &l);
    }

    /// Random bytes behind both signatures never panic.
    #[test]
    fn random_bytes_never_panic(boxed in any::<bool>(), tail in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let head: &[u8] = if boxed { b"\0\0\0\x0cJXL \r\n\x87\n" } else { b"\xff\x0a" };
        let bytes = [head, &tail].concat();
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&bytes, &l);
        let _ = decode(&bytes, &l);
        let _ = decode_thumbnail(&bytes, 16, &l);
    }
}
