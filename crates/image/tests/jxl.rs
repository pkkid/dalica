//! JPEG XL through `jxl-oxide` (`formats.md` §1): lossless files written by `scenes` (grey and RGB,
//! 8 and 16 bits, alpha at 8) decode to exactly the samples they store; the box container reads
//! like a bare
//! codestream; the eight orientations and a Display P3 colour encoding, written into the image
//! header here, come out upright and converted by the colour stage; declared sizes past the limits
//! and decoders running out of their allocation are refused; damaged files never panic.
//!
//! Embedded ICC profiles (M6.41): an ICC stream written here (ISO/IEC 18181-1 §E.4: the profile's
//! header as residuals against the decoder's prediction, the rest copied, entropy-coded with ANS
//! over a flat 256-symbol distribution) decodes like the enumerated Display P3 file; a profile or
//! stream declared past `max_icc` is refused before the decoder reserves it, and an accepted one
//! is charged to the decode's budget.
//!
//! Orientation and colour encoding are image header fields `zune-jpegxl` always leaves at their
//! defaults, so the tests rewrite its header (ISO/IEC 18181-1 §A.3–A.6: SizeHeader and
//! ImageMetadata, then padding to a byte, after which the frame starts unchanged).

use crate::common::{assert_close, assert_fingerprint, cases, distance, rgba, to8_16};
use std::sync::LazyLock;

use allocation_counter::AllocationInfo;

use dalica_image::{
    Budget, DecodeOptions, Error, Format, Header, Limits, Orientation, Rgba8, decode, decode_as, decode_thumbnail, decode_with, formats, read_header,
    sniff,
};
use dalica_scenes::formats::{Colour, Depth, SampleData, alpha, jxl, samples};
use dalica_scenes::{Rgb8, Scene, orient};
use moxcms::{ColorProfile, Layout};
use proptest::prelude::*;
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

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

    /// A `U64` (selector 3: twelve bits, then bytes behind continuation bits, the last four bits
    /// at shift 60).
    fn u64(&mut self, v: u64) {
        match v {
            0 => self.put(2, 0),
            1..=16 => {
                self.put(2, 1);
                self.put(4, v - 1);
            }
            17..=272 => {
                self.put(2, 2);
                self.put(8, v - 17);
            }
            _ => {
                self.put(2, 3);
                self.put(12, v & 0xfff);
                let (mut rest, mut shift) = (v >> 12, 12);
                while rest != 0 {
                    self.put(1, 1);
                    if shift == 60 {
                        self.put(4, rest);
                        return;
                    }
                    self.put(8, rest & 0xff);
                    rest >>= 8;
                    shift += 8;
                }
                self.put(1, 0);
            }
        }
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
    header_bits(m, false).bytes
}

/// The image header's bits; with `icc`, the colour encoding asks for an embedded profile (whose
/// stream the caller appends before the frame).
fn header_bits(m: Meta, icc: bool) -> Bits {
    header_bits_with(m, |b| {
        if icc {
            b.put(1, 0); // colour encoding not all default
            b.put(1, 1); // an ICC profile
            b.enumerated(u64::from(m.grey)); // RGB 0, grey 1
        } else if m.grey || m.p3 {
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
    })
}

/// The image header's bits with the colour encoding `colour` writes.
fn header_bits_with(m: Meta, colour: impl FnOnce(&mut Bits)) -> Bits {
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
    colour(&mut b);
    if m.orientation != 0 {
        b.put(1, 1); // tone mapping all default
    }
    b.put(2, 0); // no extensions
    b.put(1, 1); // default transform data
    b
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

// ---- Writing an embedded ICC profile -----------------------------------------------------------

/// A base-128 varint, low bits first, as the ICC stream stores its sizes and commands.
fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// The decoder's prediction of the profile header's byte `i` from the residuals before it
/// (ISO/IEC 18181-1 §E.4.3).
fn predicted(i: usize, size: u32, r: &[u8]) -> u8 {
    match i {
        0..=3 => size.to_be_bytes()[i],
        8 => 4,
        12..=23 => b"mntrRGB XYZ "[i - 12],
        36..=39 => b"acsp"[i - 36],
        41 | 42 if r[40] == b'A' => b'P',
        43 if r[40] == b'A' => b'L',
        41 if r[40] == b'M' => b'S',
        42 if r[40] == b'M' => b'F',
        43 if r[40] == b'M' => b'T',
        42 if r[40] == b'S' && r[41] == b'G' => b'I',
        43 if r[40] == b'S' && r[41] == b'G' => b' ',
        42 if r[40] == b'S' && r[41] == b'U' => b'N',
        43 if r[40] == b'S' && r[41] == b'U' => b'W',
        70 => 246,
        71 => 214,
        73 => 1,
        78 => 211,
        79 => 45,
        80..=83 => r[4 + i - 80],
        _ => 0,
    }
}

/// The ICC stream of `profile` before entropy coding: its size, the commands (no tag table, then
/// "copy the rest"), the header as residuals against the prediction, the rest as it is.
fn icc_stream(profile: &[u8]) -> Vec<u8> {
    assert!(profile.len() > 128);
    let commands = [&[0u8, 1][..], &varint(profile.len() as u64 - 128)].concat();
    let mut header = Vec::new();
    for (i, &p) in profile[..128].iter().enumerate() {
        let r = p.wrapping_sub(predicted(i, profile.len() as u32, &header));
        header.push(r);
    }
    [varint(profile.len() as u64), varint(commands.len() as u64), commands, header, profile[128..].to_vec()].concat()
}

/// The ICC stream as the codestream stores it: its declared length, then `symbols` entropy-coded
/// (no LZ77, every context in one cluster, ANS with a flat distribution of 256 literal tokens).
/// The decoder checks the ANS state only once it has read `declared` symbols, so a test may
/// declare more than it writes.
fn put_icc(b: &mut Bits, declared: u64, symbols: &[u8]) {
    b.u64(declared);
    b.put(1, 0); // no LZ77
    b.put(1, 1); // a simple context map,
    b.put(2, 0); // zero bits per context: every context in cluster 0
    b.put(1, 0); // ANS, not prefix codes
    b.put(2, 3); // log alphabet size 5 + 3
    b.put(4, 8); // split exponent 8: every token is its own value
    b.put(1, 0); // not a one- or two-symbol distribution;
    b.put(1, 1); // flat, over
    b.put(1, 1); // U8: 2^7 + 127 + 1 = 256 symbols
    b.put(3, 7);
    b.put(7, 127);
    // rANS with 12-bit precision, 32-bit state and 16-bit words: each symbol has frequency 16 at
    // 16 × symbol. Encoded backwards from the state the decoder ends on.
    let mut state: u64 = 0x13_0000;
    let mut words = Vec::new();
    for &s in symbols.iter().rev() {
        if state >= 1 << 24 {
            words.push(state & 0xffff);
            state >>= 16;
        }
        state = ((state / 16) << 12) + 16 * u64::from(s) + state % 16;
    }
    b.put(32, state);
    for &w in words.iter().rev() {
        b.put(16, w);
    }
}

/// A lossless codestream of `pixels` with an embedded ICC stream: `declared` symbols, of which
/// `symbols` are written.
fn codestream_icc(pixels: &[u8], m: Meta, declared: u64, symbols: &[u8]) -> Vec<u8> {
    let plain = codestream(pixels, Meta { orientation: 0, p3: false, ..m });
    let frame = &plain[image_header(Meta { orientation: 0, p3: false, ..m }).len()..];
    let mut b = header_bits(m, true);
    put_icc(&mut b, declared, symbols);
    [b.bytes, frame.to_vec()].concat()
}

/// A JPEG XL container with the codestream split into `jxlp` boxes of `size` bytes.
fn container_parts(codestream: &[u8], size: usize) -> Vec<u8> {
    let boxed = |kind: &[u8; 4], payload: &[u8]| [&(8 + payload.len() as u32).to_be_bytes()[..], kind, payload].concat();
    let chunks: Vec<&[u8]> = codestream.chunks(size).collect();
    let mut out = [boxed(b"JXL ", b"\r\n\x87\n"), boxed(b"ftyp", b"jxl \0\0\0\0jxl ")].concat();
    for (i, chunk) in chunks.iter().enumerate() {
        let index = i as u32 | if i + 1 == chunks.len() { 0x8000_0000 } else { 0 };
        out.extend(boxed(b"jxlp", &[&index.to_be_bytes()[..], chunk].concat()));
    }
    out
}

/// A JPEG XL container: the signature box, `ftyp`, and the codestream in one `jxlc` box.
fn container(codestream: &[u8]) -> Vec<u8> {
    let boxed = |kind: &[u8; 4], payload: &[u8]| [&(8 + payload.len() as u32).to_be_bytes()[..], kind, payload].concat();
    [boxed(b"JXL ", b"\r\n\x87\n"), boxed(b"ftyp", b"jxl \0\0\0\0jxl "), boxed(b"jxlc", codestream)].concat()
}

// ---- Tests --------------------------------------------------------------------------------------

#[test]
fn scenes_files_decode_exactly() {
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

/// What `run` allocated on this thread (`allocation-counter`'s global allocator counts each
/// thread's allocations apart, so tests running beside it do not count).
fn allocations<T>(run: impl FnOnce() -> T) -> (T, AllocationInfo) {
    let mut out = None;
    let info = allocation_counter::measure(|| out = Some(run()));
    (out.expect("the closure ran"), info)
}

/// The most bytes a refusal may hold at once: the header's few small structures, nothing sized by
/// the image (the smallest declared below, 70 000 × 2, would take 560 KB of samples).
const REFUSAL_PEAK: u64 = 64 << 10;

/// A declared size past the limits is refused from the header alone: the decode and the
/// thumbnail allocate exactly what reading the header does, so no decoder ran and nothing was
/// reserved for the image (counted, not timed, so a loaded machine cannot fail it).
#[test]
fn a_declared_size_past_the_limits_is_refused_before_allocating() {
    let small = Scene::new(65, 16, 16).render().unwrap();
    let real = codestream(&small.data, Meta { width: 16, height: 16, grey: false, bits: 8, alpha: false, orientation: 0, p3: false });
    let plain = image_header(Meta { width: 16, height: 16, grey: false, bits: 8, alpha: false, orientation: 0, p3: false });
    // Past the pixel cap, past the allocation limit, past the side limit. (Larger sizes are refused
    // by the decoder itself when it reads the frame header: at most 65 536 groups.)
    for (w, h) in [(40_000, 40_000), (30_000, 30_000), (2, 1_000_000), (70_000, 2)] {
        let bytes =
            [image_header(Meta { width: w, height: h, grey: false, bits: 8, alpha: false, orientation: 0, p3: false }), real[plain.len()..].to_vec()]
                .concat();
        let (header, read) = allocations(|| read_header(&bytes, &Limits::default()));
        assert_eq!(header.map(|h| (h.width, h.height)), Ok((w, h)));
        assert!(read.bytes_max < REFUSAL_PEAK, "{w} × {h}: the header held {read:?}");
        let (decoded, refused) = allocations(|| decode(&bytes, &Limits::default()));
        assert!(matches!(decoded, Err(Error::TooLarge { .. })), "{w} × {h}");
        assert_eq!((refused.count_total, refused.bytes_total), (read.count_total, read.bytes_total), "{w} × {h}: the decode allocated {refused:?}");
        let (thumb, refused) = allocations(|| decode_thumbnail(&bytes, 64, &Limits::default()));
        assert!(matches!(thumb, Err(Error::TooLarge { .. })), "{w} × {h}");
        assert_eq!(
            (refused.count_total, refused.bytes_total),
            (read.count_total, read.bytes_total),
            "{w} × {h}: the thumbnail allocated {refused:?}"
        );
    }
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

/// The Display P3 pixels of `a_display_p3_image_converts_to_srgb`, the size of a test image.
fn p3_pixels(seed: u64, width: u32, height: u32) -> (Rgb8, Vec<u8>) {
    let img = Scene::new(seed, width, height).render().unwrap();
    let to_p3 = ColorProfile::new_srgb()
        .create_transform_8bit(Layout::Rgb, &ColorProfile::new_display_p3(), Layout::Rgb, dalica_image::color::transform_options(8))
        .unwrap();
    let mut p3 = vec![0u8; img.data.len()];
    to_p3.transform(&img.data, &mut p3).unwrap();
    (img, p3)
}

const RGB8: Meta = Meta { width: 120, height: 80, grey: false, bits: 8, alpha: false, orientation: 0, p3: false };

/// The smallest `max_alloc` with which `bytes` decode.
fn least_alloc(bytes: &[u8]) -> u64 {
    let (mut fails, mut works) = (0u64, 64 << 20);
    assert!(decode(bytes, &Limits { max_alloc: works, ..Limits::default() }).is_ok());
    while works - fails > 1 {
        let mid = (fails + works) / 2;
        match decode(bytes, &Limits { max_alloc: mid, ..Limits::default() }) {
            Ok(_) => works = mid,
            Err(Error::TooLarge { .. }) => fails = mid,
            Err(e) => panic!("{e:?} at {mid}"),
        }
    }
    works
}

/// A file with an embedded Display P3 profile: the profile reaches the colour stage and the pixels
/// match the enumerated P3 file's fingerprint, in a bare codestream, one `jxlc` box or many
/// `jxlp` boxes; the profile is charged to the budget; `max_icc` is checked at its boundary.
#[test]
fn an_embedded_profile_decodes_and_is_charged_to_the_budget() {
    let (img, p3) = p3_pixels(63, 120, 80);
    let profile = ColorProfile::new_display_p3().encode().unwrap();
    let stream = icc_stream(&profile);
    let bytes = codestream_icc(&p3, RGB8, stream.len() as u64, &stream);
    for file in [bytes.clone(), container(&bytes), container_parts(&bytes, 7)] {
        assert_eq!(sniff(&file), Some(Format::JpegXl));
        let h = read_header(&file, &Limits::default()).unwrap();
        assert_eq!((h.width, h.height), (120, 80));
        let on = decode_with(&file, &ON, &Limits::default()).unwrap();
        assert_close("jxl-icc-vs-source", &on, &rgba(&img), 1.5);
        assert_fingerprint("jxl-rgb8-p3", &on, 1.0);
        let off = decode_with(&file, &OFF, &Limits::default()).unwrap();
        assert_eq!(off.data, p3.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect::<Vec<u8>>());
    }
    // The decoder hands the embedded profile itself to the colour stage.
    let header = read_header(&bytes, &Limits::default()).unwrap();
    let mut budget = Budget::new(&Limits::default(), 120, 80);
    let decoded = formats::jxl::decode(&bytes, &header, &mut budget, &Limits::default()).unwrap();
    assert_eq!(decoded.icc.as_deref(), Some(&profile[..]));
    // Charged: the stream and two copies of the profile, on top of what the same pixels need with
    // an enumerated encoding.
    let enumerated = codestream(&p3, Meta { p3: true, ..RGB8 });
    let reserved = (stream.len() + 2 * profile.len()) as u64;
    let (with_icc, without) = (least_alloc(&bytes), least_alloc(&enumerated));
    assert_eq!(with_icc, without + reserved, "{with_icc} bytes with the profile, {without} without");
    // `max_icc` at its boundary: the profile's own length passes, a byte less refuses it.
    let at = Limits { max_icc: profile.len(), ..Limits::default() };
    assert!(decode(&bytes, &at).is_ok());
    let under = Limits { max_icc: profile.len() - 1, ..Limits::default() };
    assert_eq!(read_header(&bytes, &under), Err(Error::TooLarge { width: 0, height: 0 }));
    assert_eq!(
        formats::jxl::decode(&bytes, &header, &mut Budget::new(&under, 120, 80), &under).unwrap_err(),
        Error::TooLarge { width: 0, height: 0 }
    );
    // Every prefix: the right size or an error, never a panic.
    for len in 0..bytes.len() {
        if let Ok(h) = read_header(&bytes[..len], &Limits::default()) {
            assert_eq!((h.width, h.height), (120, 80), "prefix {len}");
        }
    }
}

/// A small file declaring a 200 MiB profile, or a 200 MiB stream, is refused as too large from
/// the declared sizes, before the decoder reserves either: from the header, the decode and the
/// thumbnail, in a bare codestream and in containers. Directly against a budget (with `max_icc`
/// raised past it), the charge fails and nothing is left charged; had the decoder run, it would
/// have reserved the size and then found the file damaged or cut short instead.
#[test]
fn a_declared_profile_past_the_limit_is_refused_before_the_decoder_reserves_it() {
    let pixels = Scene::new(70, 16, 16).render().unwrap().data;
    let m = Meta { width: 16, height: 16, ..RGB8 };
    const BIG: u64 = 200 << 20;
    // The profile's length is 200 MiB; the stream holds 140 bytes (its sizes, then zeros).
    let mut sizes = [varint(BIG), varint(0)].concat();
    sizes.resize(140, 0);
    let big_profile = codestream_icc(&pixels, m, sizes.len() as u64, &sizes);
    // The stream's length is 200 MiB; nothing of it follows.
    let big_stream = codestream_icc(&pixels, m, BIG, &[]);
    let raised = Limits { max_icc: 256 << 20, max_alloc: 128 << 20, ..Limits::default() };
    for (name, bytes) in [("profile", big_profile), ("stream", big_stream)] {
        assert!(bytes.len() < 4096, "{name}: {} bytes", bytes.len());
        for file in [bytes.clone(), container(&bytes), container_parts(&bytes, 5)] {
            let too_large = Error::TooLarge { width: 0, height: 0 };
            // Each refusal holds a few KiB at most: nothing sized by the 200 MiB declared.
            let refused = |(result, info): (Result<(), Error>, AllocationInfo), what: &str| {
                assert_eq!(result, Err(too_large.clone()), "{name}: {what}");
                assert!(info.bytes_max < REFUSAL_PEAK, "{name}: {what} held {info:?}");
            };
            refused(allocations(|| read_header(&file, &Limits::default()).map(drop)), "the header");
            refused(allocations(|| decode(&file, &Limits::default()).map(drop)), "the decode");
            refused(allocations(|| decode_thumbnail(&file, 8, &Limits::default()).map(drop)), "the thumbnail");
            refused(allocations(|| formats::embedded_thumbnail(Format::JpegXl, &file, &Limits::default()).map(drop)), "the embedded thumbnail");
            assert_eq!(too_large.reason(), "Too large to display");
            // With `max_icc` raised: the header's 64 MiB allowance refuses it, and so does the
            // decode's budget.
            refused(allocations(|| read_header(&file, &raised).map(drop)), "the header, raised");
            let header = Header {
                format: Format::JpegXl,
                width: 16,
                height: 16,
                orientation: Orientation::UPRIGHT,
                exif: None,
                exif_error: None,
                thumbnail: None,
            };
            let mut budget = Budget::new(&raised, 16, 16);
            let (result, info) = allocations(|| formats::jxl::decode(&file, &header, &mut budget, &raised).map(drop));
            assert_eq!(result, Err(Error::TooLarge { width: 16, height: 16 }), "{name}");
            assert!(info.bytes_max < REFUSAL_PEAK, "{name}: the decode, raised, held {info:?}");
            assert_eq!(budget.left(), 128 << 20, "{name}");
        }
    }
    // The stream's limit is `max_icc` plus the 64 KiB the decoder itself allows beyond the
    // profile: at it the sizes are read on (and this file ends there), past it refused at once.
    let small = Limits { max_icc: 1000, ..Limits::default() };
    let at = codestream_icc(&pixels, m, 1000 + 65_536, &[]);
    assert_ne!(read_header(&at, &small), Err(Error::TooLarge { width: 0, height: 0 }));
    let past = codestream_icc(&pixels, m, 1000 + 65_537, &[]);
    let (result, info) = allocations(|| read_header(&past, &small).map(drop));
    assert_eq!(result, Err(Error::TooLarge { width: 0, height: 0 }));
    assert!(info.bytes_max < REFUSAL_PEAK, "{info:?}");
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

/// Enumerated colour encodings no profile describes (the decoder's profile builder panics on
/// them) are refused as unsupported, never a caught panic: the XYB and unknown colour spaces, an
/// unknown transfer function, a gamma of 0. The first is the fuzzer's find (M10.R), kept as it was.
#[test]
fn colour_encodings_no_profile_describes_are_unsupported() {
    let img = Scene::new(70, 16, 12).render().unwrap();
    let m = Meta { width: 16, height: 12, ..RGB8 };
    let frame = codestream(&img.data, m)[image_header(m).len()..].to_vec();
    // The colour space (RGB 0, XYB 2, unknown 3), then (XYB has none) a D65 white and sRGB
    // primaries, then the curve: a gamma flag and a 24-bit gamma, or an enum (unknown 2, sRGB 13).
    type Encoding = fn(&mut Bits);
    let encodings: [(&str, Encoding); 4] = [
        ("XYB", |b| {
            b.enumerated(2);
            b.put(1, 0);
            b.enumerated(13);
        }),
        ("unknown space", |b| {
            b.enumerated(3);
            b.enumerated(1);
            b.enumerated(1);
            b.put(1, 0);
            b.enumerated(13);
        }),
        ("unknown curve", |b| {
            b.enumerated(0);
            b.enumerated(1);
            b.enumerated(1);
            b.put(1, 0);
            b.enumerated(2);
        }),
        ("gamma 0", |b| {
            b.enumerated(0);
            b.enumerated(1);
            b.enumerated(1);
            b.put(1, 1);
            b.put(24, 0);
        }),
    ];
    for (name, encoding) in encodings {
        let header = header_bits_with(m, |b| {
            b.put(1, 0); // colour encoding not all default
            b.put(1, 0); // no ICC profile
            encoding(b);
            b.enumerated(1); // relative colorimetric
        });
        let bytes = [header.bytes, frame.clone()].concat();
        assert_eq!(read_header(&bytes, &Limits::default()).map(|h| (h.width, h.height)), Ok((16, 12)), "{name}");
        for options in [ON, OFF] {
            assert_eq!(decode_as(Format::JpegXl, &bytes, &options, &Limits::default()).map(drop), Err(Error::Unsupported(Format::JpegXl)), "{name}");
        }
    }
    #[rustfmt::skip]
    let found: &[u8] = &[
        0xff, 0x0a, 0x6b, 0x20, 0x20, 0x54, 0x01, 0x20, 0x20, 0x20, 0x01, 0x00, 0x00, 0x01, 0x20, 0x20,
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0xba,
        0xba, 0xba, 0x68, 0xc0, 0x92, 0x6b, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        0x20, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x0a, 0x7a,
        0x00, 0x7a, 0x0a, 0x14, 0x4e, 0x00, 0x06,
    ];
    for options in [ON, OFF] {
        assert_eq!(decode_as(Format::JpegXl, found, &options, &Limits::default()).map(drop), Err(Error::Unsupported(Format::JpegXl)));
    }
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

/// Every layout and a file with an embedded profile, as a bare codestream and in a container
/// (encoded once: the encoder is slow in debug builds).
static SMALL_FILES: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    let scene = Scene::new(69, 20, 14);
    let stream = icc_stream(&ColorProfile::new_display_p3().encode().unwrap());
    let icc = codestream_icc(&p3_pixels(69, 20, 14).1, Meta { width: 20, height: 14, ..RGB8 }, stream.len() as u64, &stream);
    LAYOUTS
        .iter()
        .map(|&(colour, depth, with_alpha, _)| jxl(&scene, colour, depth, with_alpha).unwrap())
        .chain([icc])
        .flat_map(|b| [container(&b), b])
        .collect()
});

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    /// Corruption and truncation anywhere in real files never panic.
    #[test]
    fn corrupted_files_never_panic(file in 0..2 * LAYOUTS.len() + 2, cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6)) {
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
}
