//! PSD and PSB composites (`formats.md` §1) from the `scenes` writer: every depth and colour model,
//! raw and PackBits, checked exactly against the samples the writer stored; PSB built here from a
//! PSD by widening its
//! length fields; the thumbnail and ICC resources; the limits; and damaged or hostile documents,
//! which are refused without a panic.

use crate::common::{assert_close, assert_fingerprint, cases, distance, rgba, to8_16};
use std::time::Instant;

use dalica_image::{DecodeOptions, Error, Format, Limits, Rgba8, decode, decode_as, decode_thumbnail, decode_with, formats, read_header, sniff};
use dalica_scenes::formats::{Colour, Depth, Psd, SampleData, psd, quantized, samples};
use dalica_scenes::icc::{self, Space, Version};
use dalica_scenes::{Rgb8, Scene};
use moxcms::{ColorProfile, Layout};
use proptest::prelude::*;

const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

fn to8_f32(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// What a document of `colour` and `depth` must decode to without a profile, computed from the
/// samples the writer stored: grey spread to RGB, 16-bit and float rounded to 8 bits, CMYK ink
/// converted naively (R = (1 − C)(1 − K)), palette indices looked up.
fn expected(img: &Rgb8, colour: Colour, depth: Depth) -> Rgba8 {
    if colour == Colour::Palette {
        return rgba(&quantized(img));
    }
    let s = samples(img, colour, depth).unwrap();
    let v8: Vec<u8> = match s.data {
        SampleData::U8(v) => v,
        SampleData::U16(v) => v.into_iter().map(to8_16).collect(),
        SampleData::F32(v) => v.into_iter().map(to8_f32).collect(),
    };
    let ink = |v: u8| u32::from(255 - v);
    let data = match colour {
        Colour::Grey => v8.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        Colour::Rgb => v8.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        Colour::Cmyk => v8
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[c, m, y, k]| [c, m, y].map(|v| ((ink(v) * ink(k) + 127) / 255) as u8).into_iter().chain([255]))
            .collect(),
        Colour::Palette => unreachable!(),
    };
    Rgba8 { width: img.width, height: img.height, data }
}

const LAYOUTS: [(Colour, Depth, &str); 9] = [
    (Colour::Grey, Depth::Eight, "grey8"),
    (Colour::Grey, Depth::Sixteen, "grey16"),
    (Colour::Grey, Depth::Float, "grey32"),
    (Colour::Rgb, Depth::Eight, "rgb8"),
    (Colour::Rgb, Depth::Sixteen, "rgb16"),
    (Colour::Rgb, Depth::Float, "rgb32"),
    (Colour::Cmyk, Depth::Eight, "cmyk8"),
    (Colour::Cmyk, Depth::Sixteen, "cmyk16"),
    (Colour::Palette, Depth::Eight, "indexed8"),
];

fn doc(scene: &Scene, colour: Colour, depth: Depth, rle: bool) -> Vec<u8> {
    psd(scene, &Psd { colour, depth, rle, ..Psd::default() }).unwrap()
}

// ---- Layout of the writer's documents ----------------------------------------------------------

fn be16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([b[at], b[at + 1]]))
}

fn be32(b: &[u8], at: usize) -> usize {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// Offsets of a PSD's sections: (colour mode data length field, resources length field, layer
/// section length field, compression field).
fn sections(b: &[u8]) -> (usize, usize, usize, usize) {
    let resources = 26 + 4 + be32(b, 26);
    let layers = resources + 4 + be32(b, resources);
    let image = layers + 4 + be32(b, layers);
    (26, resources, layers, image)
}

/// The same document as a PSB: version 2, an 8-byte layer section length, 32-bit RLE row counts.
fn to_psb(b: &[u8]) -> Vec<u8> {
    let (_, _, layers, image) = sections(b);
    assert_eq!(be32(b, layers), 0, "the writer's layer section is empty");
    let mut out = b[..layers].to_vec();
    out[4..6].copy_from_slice(&2u16.to_be_bytes());
    out.extend([0u8; 8]);
    out.extend(&b[image..image + 2]);
    if be16(b, image) == 1 {
        let rows = be16(b, 12) * be32(b, 14);
        for r in 0..rows {
            out.extend((be16(b, image + 2 + 2 * r) as u32).to_be_bytes());
        }
        out.extend(&b[image + 2 + 2 * rows..]);
    } else {
        out.extend(&b[image + 2..]);
    }
    out
}

fn set16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn set32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

// ---- Depths and colour models -------------------------------------------------------------------

#[test]
fn every_depth_and_colour_model_decodes_exactly() {
    let scene = Scene::new(41, 96, 64);
    let img = scene.render().unwrap();
    for (colour, depth, name) in LAYOUTS {
        let raw = doc(&scene, colour, depth, false);
        let packed = doc(&scene, colour, depth, true);
        assert_eq!(sniff(&raw), Some(Format::Psd));
        let h = read_header(&raw, &Limits::default()).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation.get(), h.thumbnail.clone()), (Format::Psd, 96, 64, 1, None), "{name}");
        let a = decode(&raw, &Limits::default()).unwrap();
        let b = decode(&packed, &Limits::default()).unwrap();
        assert!(a == b, "{name}: raw and PackBits differ");
        assert!(a == expected(&img, colour, depth), "{name}: not the stored samples");
        assert_eq!(decode_as(Format::Psd, &packed, &ON, &Limits::default()).unwrap(), b, "{name}");
    }
    // 8-bit RGB is the scene itself.
    assert_eq!(decode(&doc(&scene, Colour::Rgb, Depth::Eight, true), &Limits::default()).unwrap(), rgba(&img));
}

#[test]
fn psb_documents_decode_like_their_psd() {
    let scene = Scene::new(42, 70, 50);
    for (colour, depth, rle) in [(Colour::Rgb, Depth::Eight, true), (Colour::Cmyk, Depth::Sixteen, false), (Colour::Grey, Depth::Float, true)] {
        let psd = doc(&scene, colour, depth, rle);
        let psb = to_psb(&psd);
        assert_eq!(be16(&psb, 4), 2);
        assert_eq!(read_header(&psb, &Limits::default()).unwrap().width, 70);
        assert_eq!(decode(&psb, &Limits::default()).unwrap(), decode(&psd, &Limits::default()).unwrap(), "{colour:?} {depth:?} rle {rle}");
    }
}

/// Bitmap (1 bit, 1 = black) and duotone (a grey composite) documents.
#[test]
fn bitmap_and_duotone_documents() {
    // 10 × 3 bitmap: each row is two bytes, the last six bits padding.
    let rows: [[u8; 2]; 3] = [[0b1010_0000, 0b1100_0000], [0xff, 0xff], [0, 0]];
    let mut b = b"8BPS\0\x01\0\0\0\0\0\0\0\x01".to_vec();
    b.extend(3u32.to_be_bytes());
    b.extend(10u32.to_be_bytes());
    b.extend([0, 1, 0, 0]); // 1 bit, bitmap mode
    b.extend([0u8; 12]); // empty colour mode data, resources, layers
    b.extend([0, 0]); // raw
    rows.iter().for_each(|r| b.extend(r));
    let img = decode(&b, &Limits::default()).unwrap();
    let grey: Vec<u8> = img.data.chunks(4).map(|p| p[0]).collect();
    let (k, w) = (0u8, 255u8);
    assert_eq!(grey[..10], [k, w, k, w, w, w, w, w, k, k]);
    assert!(grey[10..20].iter().all(|&v| v == 0) && grey[20..].iter().all(|&v| v == 255));

    let scene = Scene::new(43, 40, 30);
    let mut duotone = doc(&scene, Colour::Grey, Depth::Eight, true);
    set16(&mut duotone, 24, 8);
    assert_eq!(decode(&duotone, &Limits::default()).unwrap(), expected(&scene.render().unwrap(), Colour::Grey, Depth::Eight));
}

/// Channels after the colour channels (an alpha channel, a saved selection) are not shown.
#[test]
fn extra_channels_are_left_out() {
    let scene = Scene::new(44, 96, 20);
    let reference = decode(&doc(&scene, Colour::Rgb, Depth::Eight, false), &Limits::default()).unwrap();
    // Raw: a fourth plane after the three.
    let mut raw = doc(&scene, Colour::Rgb, Depth::Eight, false);
    set16(&mut raw, 12, 4);
    raw.extend(vec![7u8; 96 * 20]);
    assert_eq!(decode(&raw, &Limits::default()).unwrap(), reference);
    // PackBits: twenty more row counts in the table, and their rows (one literal run each).
    let rle = doc(&scene, Colour::Rgb, Depth::Eight, true);
    let (_, _, _, image) = sections(&rle);
    let table_end = image + 2 + 3 * 20 * 2;
    let mut four = rle[..table_end].to_vec();
    set16(&mut four, 12, 4);
    (0..20).for_each(|_| four.extend(97u16.to_be_bytes()));
    four.extend(&rle[table_end..]);
    (0..20).for_each(|_| four.extend([95u8].iter().chain(&[9u8; 96])));
    assert_eq!(decode(&four, &Limits::default()).unwrap(), reference);
}

// ---- Resources ----------------------------------------------------------------------------------

/// `img`'s 8-bit samples taken as being in `icc`'s space, converted to sRGB by `moxcms` directly.
fn from_space(img: &Rgb8, icc: &[u8], layout: Layout) -> Rgba8 {
    let profile = ColorProfile::new_from_slice(icc).unwrap();
    let t = profile.create_transform_8bit(layout, &ColorProfile::new_srgb(), Layout::Rgba, dalica_image::color::transform_options(8)).unwrap();
    let src: Vec<u8> =
        if layout == Layout::Gray { expected(img, Colour::Grey, Depth::Eight).data.chunks(4).map(|p| p[0]).collect() } else { img.data.clone() };
    let mut out = vec![0u8; img.width as usize * img.height as usize * 4];
    t.transform(&src, &mut out).unwrap();
    Rgba8 { width: img.width, height: img.height, data: out }
}

#[test]
fn the_icc_resource_goes_through_the_colour_stage() {
    let scene = Scene::new(45, 120, 80);
    let img = scene.render().unwrap();
    let adobe = icc::rgb(Space::AdobeRgb, Version::V4);
    let tagged = psd(&scene, &Psd { icc: Some(adobe.clone()), rle: true, ..Psd::default() }).unwrap();
    let on = decode_with(&tagged, &ON, &Limits::default()).unwrap();
    assert_close("psd-adobe-vs-moxcms", &on, &from_space(&img, &adobe, Layout::Rgb), 0.5);
    assert_fingerprint("psd-rgb8-adobe", &on, 1.0);
    // Without colour management the samples are taken as sRGB: the scene itself, visibly different.
    let off = decode_with(&tagged, &OFF, &Limits::default()).unwrap();
    assert_eq!(off, rgba(&img));
    assert!(distance(&on, &off) > 2.0, "{}", distance(&on, &off));
    // A grey profile on a 16-bit grey document.
    let grey = icc::grey(Version::V2);
    let g = psd(&scene, &Psd { colour: Colour::Grey, depth: Depth::Sixteen, icc: Some(grey.clone()), ..Psd::default() }).unwrap();
    assert_close("psd-grey16-icc", &decode(&g, &Limits::default()).unwrap(), &from_space(&img, &grey, Layout::Gray), 1.5);
    // A profile over the size limit is not copied; the document shows as sRGB.
    let small = Limits { max_icc: adobe.len() - 1, ..Limits::default() };
    assert_eq!(decode_with(&tagged, &ON, &small).unwrap(), rgba(&img));
}

#[test]
fn the_thumbnail_resource_serves_a_small_target() {
    let scene = Scene::new(46, 480, 320);
    let bytes = psd(&scene, &Psd { thumbnail: Some(160), icc: Some(icc::rgb(Space::Srgb, Version::V2)), ..Psd::default() }).unwrap();
    let l = Limits::default();
    let range = formats::embedded_thumbnail(Format::Psd, &bytes, &l).unwrap().expect("a thumbnail resource");
    assert_eq!(read_header(&bytes, &l).unwrap().thumbnail, Some(range.clone()));
    let thumb = &bytes[range];
    assert_eq!(sniff(thumb), Some(Format::Jpeg));
    let small = decode_thumbnail(thumb, 96, &l).unwrap();
    assert_eq!((small.width, small.height), (96, 64));
    // The writer's thumbnail is the scene at 160 pixels, stored as JPEG (quality 80).
    let reference = dalica_image::resize::fit(&rgba(&scene.render().unwrap().downscale(160).unwrap()), 96).unwrap();
    assert_close("psd-thumbnail-resource", &small, &reference, 3.0);
    // Past 64 MP, a thumbnail comes from the resource rather than the composite.
    let mut huge = bytes.clone();
    set32(&mut huge, 14, 9000);
    set32(&mut huge, 18, 9000);
    let t = decode_thumbnail(&huge, 100, &l).unwrap();
    assert_eq!(t.width, 100);
    assert!((66..=67).contains(&t.height), "{}", t.height);
    // A document without one has none.
    assert_eq!(formats::embedded_thumbnail(Format::Psd, &doc(&scene, Colour::Rgb, Depth::Eight, false), &l), Ok(None));
    // A resource that is not "JPEG RGB" (format 0, raw) is not offered.
    let mut raw_format = bytes.clone();
    let at = raw_format.windows(6).position(|w| w == b"8BIM\x04\x0c").unwrap() + 12;
    set32(&mut raw_format, at, 0);
    assert_eq!(formats::embedded_thumbnail(Format::Psd, &raw_format, &l), Ok(None));
    // Nor one over the thumbnail size limit.
    let tight = Limits { max_thumbnail: 100, ..l };
    assert_eq!(formats::embedded_thumbnail(Format::Psd, &bytes, &tight), Ok(None));
}

// ---- Limits and refusals ------------------------------------------------------------------------

/// The size is refused from the header alone. Allocating for it would abort the test process, so
/// passing at all shows nothing was allocated.
#[test]
fn a_declared_size_past_the_limits_is_refused_before_allocating() {
    let start = Instant::now();
    let small = doc(&Scene::new(47, 8, 8), Colour::Rgb, Depth::Sixteen, true);
    for (psb, w, h) in [(false, 30_000, 30_000), (true, 300_000, 300_000), (true, 1, 300_000), (true, 70_000, 2)] {
        let mut b = if psb { to_psb(&small) } else { small.clone() };
        set32(&mut b, 14, h);
        set32(&mut b, 18, w);
        assert_eq!(read_header(&b, &Limits::default()).map(|h| (h.width, h.height)), Ok((w, h)));
        assert!(matches!(decode(&b, &Limits::default()), Err(Error::TooLarge { .. })), "{w} × {h}");
        assert!(matches!(decode_thumbnail(&b, 256, &Limits::default()), Err(Error::TooLarge { .. })), "{w} × {h}");
    }
    // Past the format's own maximum, the header is damaged.
    let mut b = small.clone();
    set32(&mut b, 18, 30_001);
    assert!(matches!(read_header(&b, &Limits::default()), Err(Error::Malformed(_))));
    assert!(start.elapsed().as_millis() < 500, "{:?}", start.elapsed());
    // The 16-bit samples are charged with the 8-bit output: a budget with room for the output
    // alone is "too large".
    let roomy = Limits { max_alloc: 8 * 8 * (6 + 4) + 64, ..Limits::default() };
    assert!(decode(&small, &roomy).is_ok());
    let tight = Limits { max_alloc: 8 * 8 * 4 + 16, ..Limits::default() };
    assert!(matches!(decode(&small, &tight), Err(Error::TooLarge { .. })));
}

#[test]
fn zip_lab_and_multichannel_documents_are_listed_but_not_decoded() {
    let scene = Scene::new(48, 40, 30);
    let bytes = psd(&scene, &Psd { thumbnail: Some(20), ..Psd::default() }).unwrap();
    let (_, _, _, image) = sections(&bytes);
    let mut cases = Vec::new();
    for compression in [2, 3] {
        let mut zip = bytes.clone();
        set16(&mut zip, image, compression);
        cases.push(zip);
    }
    for mode in [9, 7] {
        let mut lab = bytes.clone();
        set16(&mut lab, 24, mode);
        cases.push(lab);
    }
    for b in cases {
        let h = read_header(&b, &Limits::default()).unwrap();
        assert_eq!((h.width, h.height), (40, 30));
        assert!(h.thumbnail.is_some());
        let err = decode(&b, &Limits::default()).unwrap_err();
        assert_eq!(err, Error::Unsupported(Format::Psd));
        assert_eq!(err.reason(), "This kind of PSD isn't supported");
        assert_eq!(err.reason(), formats::reason(Format::Psd));
    }
    assert!(Format::Psd.supported() && formats::decodes(Format::Psd));
}

#[test]
fn hostile_documents_are_refused() {
    let scene = Scene::new(49, 24, 16);
    let good =
        psd(&scene, &Psd { rle: true, thumbnail: Some(8), icc: Some(icc::grey(Version::V4)), colour: Colour::Grey, ..Psd::default() }).unwrap();
    let (cm, resources, layers, image) = sections(&good);
    assert!(decode(&good, &Limits::default()).is_ok());
    let damaged = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut b = good.clone();
        edit(&mut b);
        decode(&b, &Limits::default()).unwrap_err()
    };
    let malformed = |e: Error| assert_eq!(e.reason(), "Damaged file", "{e:?}");
    malformed(damaged(&|b| set16(b, 4, 3))); // version 3
    malformed(damaged(&|b| set16(b, 12, 0))); // no channels
    malformed(damaged(&|b| set16(b, 12, 57))); // too many
    malformed(damaged(&|b| set16(b, 22, 12))); // 12 bits
    malformed(damaged(&|b| set16(b, 24, 5))); // no such mode
    malformed(damaged(&|b| set16(b, 24, 3))); // RGB with one channel
    malformed(damaged(&|b| set32(b, 14, 0))); // no rows
    malformed(damaged(&|b| set32(b, cm, u32::MAX))); // colour mode data past the end
    malformed(damaged(&|b| set32(b, resources, u32::MAX - 1))); // resources past the end
    malformed(damaged(&|b| set32(b, resources + 4 + 8, u32::MAX))); // a resource past its section
    malformed(damaged(&|b| set32(b, layers, u32::MAX))); // layer section past the end
    malformed(damaged(&|b| set16(b, image, 4))); // no such compression
    malformed(damaged(&|b| set16(b, image + 2, u16::MAX))); // a row count past the end
    malformed(damaged(&|b| set16(b, image + 2, 1))); // a short row
    malformed(damaged(&|b| b.truncate(b.len() - 1)));
    // An indexed document without its palette.
    let mut rgb = doc(&scene, Colour::Grey, Depth::Eight, false);
    set16(&mut rgb, 24, 2);
    assert!(matches!(decode(&rgb, &Limits::default()), Err(Error::Malformed(_))));
    // A resource section of tiny blocks stops at the step limit.
    let mut many = b"8BPS\0\x01\0\0\0\0\0\0\0\x01\0\0\0\x01\0\0\0\x01\0\x08\0\x01\0\0\0\0".to_vec();
    let blocks = 5000;
    many.extend((blocks * 12u32).to_be_bytes());
    (0..blocks).for_each(|_| many.extend(b"8BIM\x03\xe8\0\0\0\0\0\0"));
    many.extend([0, 0, 0, 0, 0, 0, 128]);
    assert_eq!(read_header(&many, &Limits::default()), Err(Error::TooMany("PSD image resources")));
    assert!(read_header(&many, &Limits { max_segments: 6000, ..Limits::default() }).is_ok());
}

/// Every prefix of real documents: `Ok` with the right size, or `Err`, never a panic.
#[test]
fn truncation_at_every_offset_never_panics() {
    let scene = Scene::new(50, 20, 12);
    for (colour, depth, rle) in [(Colour::Rgb, Depth::Eight, true), (Colour::Cmyk, Depth::Sixteen, false), (Colour::Palette, Depth::Eight, true)] {
        let full = psd(&scene, &Psd { colour, depth, rle, thumbnail: Some(8), icc: Some(icc::rgb(Space::DisplayP3, Version::V2)) }).unwrap();
        for bytes in [full.clone(), to_psb(&full)] {
            for len in 0..bytes.len() {
                let cut = &bytes[..len];
                if let Ok(h) = read_header(cut, &Limits::default()) {
                    assert_eq!((h.width, h.height), (20, 12), "prefix {len}");
                }
                let _ = formats::embedded_thumbnail(Format::Psd, cut, &Limits::default());
                assert!(decode(cut, &Limits::default()).is_err(), "prefix {len} of {colour:?}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    /// Random bytes behind a plausible header never panic.
    #[test]
    fn random_bytes_never_panic(version in 1u16..=2, channels in 0u16..=6, mode in 0u16..=10, depth in prop::sample::select(vec![1u16, 8, 16, 32, 7]), w in 0u32..40, h in 0u32..40, tail in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut b = b"8BPS".to_vec();
        b.extend(version.to_be_bytes());
        b.extend([0u8; 6]);
        b.extend(channels.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend(depth.to_be_bytes());
        b.extend(mode.to_be_bytes());
        b.extend(tail);
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&b, &l);
        let _ = decode(&b, &l);
        let _ = decode_thumbnail(&b, 16, &l);
        let _ = formats::embedded_thumbnail(Format::Psd, &b, &l);
    }
}
