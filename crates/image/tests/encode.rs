//! The encoders (`formats.md` §1, `data-model.md` §6): JPEGs written by our baseline encoder decode
//! through our own decoder (and the `image` crate's, independently) within a stated error per
//! quality, shrink as quality falls, and are the same bytes on one thread or many; EXIF, XMP and
//! ICC segments read back and oversized ones are refused; PNGs keep their bit depth, channels,
//! alpha and metadata chunks exactly. Fingerprints of decoded encodes live in
//! `fingerprints/encode-*.json` (`DALICA_BLESS=1` rewrites them).
//!
//! Some cases adapted from LightCraft (MIT OR Apache-2.0), `crates/codecs/src/jpeg_par.rs` tests
//! (`decodes_with_independent_decoders_at_odd_sizes`, `grayscale_and_quality_monotone`) at
//! 25ef65c8.
//!
//! Stated errors: PSNR against the source pixels (all three channels, 8-bit) on a 640 × 424
//! scenes photo, decoded by our own decoder. Measured at M8.6 (4:4:4 / 4:2:0): q100 52.2 / 50.2 dB,
//! q95 51.7 / 49.8, q90 50.8 / 48.9, q75 47.8 / 46.9, q50 44.6 / 43.2; the floors in
//! `QUALITY_FLOORS` are those values less about 1 dB, for decoders' rounding on other CPUs.

use crate::common::{assert_fingerprint, cases};

use dalica_image::encode::{self, Channels, EncodeError, Image, Metadata, Pixels, Subsampling};
use dalica_image::{Budget, ColorModel, DecodeOptions, Format, Limits, Rgba8, Samples, decode_with, formats, read_header};
use dalica_scenes::{Exif as SceneExif, Kind, Scene};
use proptest::prelude::*;

const OFF: DecodeOptions = DecodeOptions { color_managed: false };

/// (quality, subsampling, the lowest PSNR in dB its decode may have).
const QUALITY_FLOORS: [(u8, Subsampling, f64); 10] = [
    (100, Subsampling::S444, 51.0),
    (95, Subsampling::S444, 50.5),
    (90, Subsampling::S444, 49.5),
    (75, Subsampling::S444, 46.5),
    (50, Subsampling::S444, 43.5),
    (100, Subsampling::S420, 49.0),
    (95, Subsampling::S420, 48.5),
    (90, Subsampling::S420, 48.0),
    (75, Subsampling::S420, 45.5),
    (50, Subsampling::S420, 42.0),
];

/// A scenes photo as RGB bytes.
fn photo(seed: u64, w: u32, h: u32, kind: Kind) -> Vec<u8> {
    Scene::new(seed, w, h).with_kind(kind).render().unwrap().data
}

fn rgb_image(data: &[u8], w: u32, h: u32) -> Image<'_> {
    Image { width: w, height: h, channels: Channels::Rgb, pixels: Pixels::U8(data) }
}

fn rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect()
}

/// PSNR in dB over the colour channels of two RGBA buffers.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let (mut sum, mut n) = (0.0, 0.0);
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        for c in 0..3 {
            sum += (f64::from(pa[c]) - f64::from(pb[c])).powi(2);
            n += 1.0;
        }
    }
    10.0 * (255.0f64.powi(2) / (sum / n).max(1e-9)).log10()
}

fn decode(bytes: &[u8]) -> Rgba8 {
    decode_with(bytes, &OFF, &Limits::default()).unwrap()
}

/// The segments of a JPEG before its first scan: (marker, payload).
fn segments(b: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut i = 2;
    while b[i] == 0xFF && b[i + 1] != 0xDA {
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        out.push((b[i + 1], &b[i + 4..i + 2 + len]));
        i += 2 + len;
    }
    out
}

// ---- JPEG: error, size and decoders ------------------------------------------------------------

#[test]
fn jpeg_error_per_quality_and_sizes_fall_with_quality() {
    let (w, h) = (640, 424);
    let src = photo(11, w, h, Kind::Blobs);
    let reference = rgba(&src);
    for sub in [Subsampling::S444, Subsampling::S420] {
        let mut last: Option<(u8, usize)> = None;
        for &(q, _, floor) in QUALITY_FLOORS.iter().filter(|f| f.1 == sub) {
            let bytes = encode::jpeg(&rgb_image(&src, w, h), q, sub, &Metadata::default()).unwrap();
            let p = psnr(&decode(&bytes).data, &reference);
            println!("{sub:?} q{q}: {} bytes, PSNR {p:.2} dB (floor {floor})", bytes.len());
            assert!(p >= floor, "{sub:?} q{q}: PSNR {p:.2} dB is under its floor {floor}");
            if let Some((lq, len)) = last {
                assert!(bytes.len() < len, "{sub:?}: q{q} is {} bytes, not smaller than q{lq} at {len}", bytes.len());
            }
            last = Some((q, bytes.len()));
        }
    }
    // 4:2:0 is smaller than 4:4:4 at the same quality.
    let s444 = encode::jpeg(&rgb_image(&src, w, h), 85, Subsampling::S444, &Metadata::default()).unwrap();
    let s420 = encode::jpeg(&rgb_image(&src, w, h), 85, Subsampling::S420, &Metadata::default()).unwrap();
    assert!(s420.len() < s444.len());
}

#[test]
fn jpeg_decodes_through_an_independent_decoder_at_odd_sizes() {
    for (w, h, sub) in [
        (1, 1, Subsampling::S444),
        (1, 1, Subsampling::S420),
        (37, 23, Subsampling::S420),
        (17, 9, Subsampling::S444),
        (640, 1031, Subsampling::S420),
        (803, 600, Subsampling::S444),
    ] {
        let src = photo(u64::from(w * h), w, h, Kind::Horizon);
        let bytes = encode::jpeg(&rgb_image(&src, w, h), 92, sub, &Metadata::default()).unwrap();
        let ours = decode(&bytes);
        assert_eq!((ours.width, ours.height), (w, h));
        let theirs = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap().to_rgba8();
        assert_eq!(theirs.dimensions(), (w, h));
        if w * h > 64 {
            assert!(psnr(&ours.data, &rgba(&src)) > 30.0, "{w}×{h} {sub:?}: ours {:.2}", psnr(&ours.data, &rgba(&src)));
            assert!(psnr(theirs.as_raw(), &rgba(&src)) > 30.0, "{w}×{h} {sub:?}: theirs {:.2}", psnr(theirs.as_raw(), &rgba(&src)));
        }
        let header = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((header.format, header.width, header.height), (Format::Jpeg, w, h));
    }
}

#[test]
fn grey_rgba_and_sixteen_bit_inputs() {
    let (w, h) = (300, 200);
    // Grey: a one-component JPEG that decodes grey; quality monotone.
    let grey: Vec<u8> = (0..w * h).map(|i| ((i % w) * 255 / w) as u8 ^ ((i / w) as u8 & 0x10)).collect();
    let img = Image { width: w, height: h, channels: Channels::Grey, pixels: Pixels::U8(&grey) };
    let lo = encode::jpeg(&img, 30, Subsampling::S420, &Metadata::default()).unwrap();
    let hi = encode::jpeg(&img, 95, Subsampling::S420, &Metadata::default()).unwrap();
    assert!(lo.len() < hi.len());
    let header = read_header(&hi, &Limits::default()).unwrap();
    let decoded = formats::jpeg::decode(&hi, &header, &mut Budget::new(&Limits::default(), w, h), &Limits::default()).unwrap();
    assert_eq!((decoded.model, decoded.alpha), (ColorModel::Gray, false));
    let grey_rgba: Vec<u8> = grey.iter().flat_map(|&v| [v, v, v, 255]).collect();
    assert!(psnr(&decode(&hi).data, &grey_rgba) > 40.0);
    // Grey + alpha: the alpha is ignored.
    let ga: Vec<u8> = grey.iter().flat_map(|&v| [v, 7]).collect();
    let img = Image { width: w, height: h, channels: Channels::GreyAlpha, pixels: Pixels::U8(&ga) };
    assert_eq!(encode::jpeg(&img, 95, Subsampling::S420, &Metadata::default()).unwrap(), hi);

    // RGBA: the alpha is ignored, the bytes are those of the RGB image.
    let src = photo(3, w, h, Kind::Gradient);
    let with_alpha: Vec<u8> = src.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 0]).collect();
    let rgba_img = Image { width: w, height: h, channels: Channels::Rgba, pixels: Pixels::U8(&with_alpha) };
    let a = encode::jpeg(&rgba_img, 88, Subsampling::S444, &Metadata::default()).unwrap();
    let b = encode::jpeg(&rgb_image(&src, w, h), 88, Subsampling::S444, &Metadata::default()).unwrap();
    assert_eq!(a, b);
    // `Image::rgba8` takes a render's output as it is.
    let render = Rgba8 { width: w, height: h, data: with_alpha.clone() };
    assert_eq!(encode::jpeg(&Image::rgba8(&render), 88, Subsampling::S444, &Metadata::default()).unwrap(), b);

    // 16-bit: scaled to 8 bits, the same picture within rounding.
    let wide: Vec<u16> = src.iter().map(|&v| u16::from(v) * 257).collect();
    let img16 = Image { width: w, height: h, channels: Channels::Rgb, pixels: Pixels::U16(&wide) };
    let c = encode::jpeg(&img16, 88, Subsampling::S444, &Metadata::default()).unwrap();
    assert!(psnr(&decode(&c).data, &decode(&b).data) > 50.0);
}

#[test]
fn parallel_and_single_threaded_output_are_the_same() {
    for (w, h, sub) in [(1500, 1100, Subsampling::S420), (1203, 997, Subsampling::S444), (64, 48, Subsampling::S420), (5, 3, Subsampling::S444)] {
        let src = photo(5, w, h, Kind::Blobs);
        let img = rgb_image(&src, w, h);
        let one = encode::jpeg(&img, 90, sub, &Metadata::default()).unwrap();
        for threads in [0, 2, 3, 8, 64] {
            let many = encode::jpeg_with_threads(&img, 90, sub, &Metadata::default(), threads).unwrap();
            assert!(many == one, "{w}×{h} {sub:?}: {threads} threads wrote different bytes");
        }
        // Large images are cut into restart-interval bands; their decode equals the
        // single-threaded one by construction (same bytes), and decodes cleanly.
        assert_eq!(decode(&one).width, w);
        if h > 1000 {
            assert!(segments(&one).iter().any(|(m, _)| *m == 0xDD), "{w}×{h}: no restart interval");
        }
    }
}

// ---- JPEG: metadata segments ------------------------------------------------------------------

#[test]
fn exif_xmp_and_icc_segments_read_back() {
    let (w, h) = (96, 64);
    let src = photo(8, w, h, Kind::Horizon);
    let exif = dalica_scenes::exif::tiff(&SceneExif { orientation: 6, make: Some("Dalica".into()), ..SceneExif::default() }, w, h, None).unwrap();
    let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"/></x:xmpmeta>"#;
    // A real sRGB profile, and a large stand-in that needs several APP2 chunks.
    let srgb = moxcms::ColorProfile::new_srgb().encode().unwrap();
    let big: Vec<u8> = (0..200_000u32).map(|i| (i * 31 % 251) as u8).collect();
    for icc in [&srgb, &big] {
        let meta = Metadata { exif: Some(&exif), xmp: Some(xmp), icc: Some(icc) };
        let bytes = encode::jpeg(&rgb_image(&src, w, h), 90, Subsampling::S420, &meta).unwrap();
        let segs = segments(&bytes);
        // JFIF first, then EXIF, XMP and the profile's chunks.
        let markers: Vec<u8> = segs.iter().map(|s| s.0).filter(|m| (0xE0..=0xEF).contains(m)).collect();
        assert_eq!(&markers[..3], &[0xE0, 0xE1, 0xE1]);
        assert_eq!(segs[0].1.get(..5), Some(&b"JFIF\0"[..]));
        // EXIF: our own header reader sees it.
        let header = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!(header.orientation.get(), 6);
        assert_eq!(header.exif.as_ref().unwrap().make.as_deref(), Some("Dalica"));
        assert_eq!(segs[1].1, [&b"Exif\0\0"[..], &exif].concat());
        // XMP: the namespace, then the packet as given.
        assert_eq!(segs[2].1, [&b"http://ns.adobe.com/xap/1.0/\0"[..], xmp.as_bytes()].concat());
        // ICC: our decoder reassembles the chunks.
        let decoded = formats::jpeg::decode(&bytes, &header, &mut Budget::new(&Limits::default(), w, h), &Limits::default()).unwrap();
        assert_eq!(decoded.icc.as_deref(), Some(&icc[..]));
        let chunks = segs.iter().filter(|s| s.0 == 0xE2).count();
        assert_eq!(chunks, icc.len().div_ceil(65_519));
    }
    // A tagged sRGB file decodes the same with colour management on.
    let meta = Metadata { icc: Some(&srgb), ..Metadata::default() };
    let tagged = encode::jpeg(&rgb_image(&src, w, h), 90, Subsampling::S444, &meta).unwrap();
    let on = decode_with(&tagged, &DecodeOptions { color_managed: true }, &Limits::default()).unwrap();
    assert!(psnr(&on.data, &decode(&tagged).data) > 45.0);
    // Empty blocks are left out.
    let empty = Metadata { exif: Some(&[]), xmp: Some(""), icc: Some(&[]) };
    let bytes = encode::jpeg(&rgb_image(&src, w, h), 90, Subsampling::S444, &empty).unwrap();
    assert_eq!(segments(&bytes).iter().filter(|s| (0xE1..=0xE2).contains(&s.0)).count(), 0);
}

#[test]
fn oversized_or_wrong_segments_are_err() {
    let (w, h) = (16, 16);
    let src = photo(2, w, h, Kind::Gradient);
    let img = rgb_image(&src, w, h);
    let tiff = |len: usize| -> Vec<u8> { [&b"II*\0\x08\0\0\0"[..], &vec![0; len - 8]].concat() };
    // EXIF: exactly one APP1 segment fits; one byte more does not.
    let fits = tiff(encode::JPEG_EXIF_MAX);
    assert!(encode::jpeg(&img, 80, Subsampling::S444, &Metadata { exif: Some(&fits), ..Metadata::default() }).is_ok());
    let over = tiff(encode::JPEG_EXIF_MAX + 1);
    let err = encode::jpeg(&img, 80, Subsampling::S444, &Metadata { exif: Some(&over), ..Metadata::default() }).unwrap_err();
    assert!(matches!(err, EncodeError::Segment { what: "EXIF block", len: 65_528, max: 65_527, .. }), "{err:?}");
    // EXIF must be TIFF-structured (no `Exif\0\0` prefix).
    let prefixed = [&b"Exif\0\0"[..], &tiff(64)].concat();
    assert_eq!(encode::jpeg(&img, 80, Subsampling::S444, &Metadata { exif: Some(&prefixed), ..Metadata::default() }), Err(EncodeError::ExifHeader));
    assert_eq!(encode::png(&img, &Metadata { exif: Some(&prefixed), ..Metadata::default() }), Err(EncodeError::ExifHeader));
    // XMP: no extended XMP.
    let xmp = "x".repeat(encode::JPEG_XMP_MAX + 1);
    assert!(matches!(
        encode::jpeg(&img, 80, Subsampling::S444, &Metadata { xmp: Some(&xmp), ..Metadata::default() }),
        Err(EncodeError::Segment { what: "XMP packet", .. })
    ));
    let xmp = "x".repeat(encode::JPEG_XMP_MAX);
    assert!(encode::jpeg(&img, 80, Subsampling::S444, &Metadata { xmp: Some(&xmp), ..Metadata::default() }).is_ok());
    // ICC: at most 255 chunks.
    let icc = vec![1u8; encode::JPEG_ICC_MAX + 1];
    assert!(matches!(
        encode::jpeg(&img, 80, Subsampling::S444, &Metadata { icc: Some(&icc), ..Metadata::default() }),
        Err(EncodeError::Segment { what: "ICC profile", .. })
    ));
}

#[test]
fn bad_images_and_qualities_are_err() {
    let px = [0u8; 12];
    let ok = Image { width: 2, height: 2, channels: Channels::Rgb, pixels: Pixels::U8(&px) };
    let none = Metadata::default();
    assert!(encode::jpeg(&ok, 1, Subsampling::S444, &none).is_ok());
    assert!(encode::jpeg(&ok, 100, Subsampling::S444, &none).is_ok());
    assert_eq!(encode::jpeg(&ok, 0, Subsampling::S444, &none), Err(EncodeError::Quality(0)));
    assert_eq!(encode::jpeg(&ok, 101, Subsampling::S444, &none), Err(EncodeError::Quality(101)));
    for (w, h, len) in [(0, 2, 0), (2, 0, 0), (2, 2, 11), (2, 2, 13), (u32::MAX, u32::MAX, 12)] {
        let px = vec![0u8; len];
        let img = Image { width: w, height: h, channels: Channels::Rgb, pixels: Pixels::U8(&px) };
        assert!(encode::jpeg(&img, 80, Subsampling::S420, &none).is_err(), "{w}×{h} with {len}");
        assert!(encode::png(&img, &none).is_err(), "{w}×{h} with {len}");
    }
    // A JPEG side is at most 65,535 (checked before the samples are).
    let img = Image { width: 65_536, height: 1, channels: Channels::Grey, pixels: Pixels::U8(&[]) };
    assert!(matches!(encode::jpeg(&img, 80, Subsampling::S444, &none), Err(EncodeError::TooLarge { max: 65_535, .. })));
    // CMYK and float decodes are not encodable as they are.
    let cmyk = dalica_image::Decoded {
        width: 1,
        height: 1,
        model: ColorModel::Cmyk,
        alpha: false,
        samples: Samples::U8(vec![0; 4]),
        icc: None,
        orientation: Default::default(),
    };
    assert!(matches!(Image::from_decoded(&cmyk), Err(EncodeError::Unsupported(_))));
    let float = dalica_image::Decoded { model: ColorModel::Rgb, samples: Samples::F32(vec![0.0; 3]), ..cmyk.clone() };
    assert!(matches!(Image::from_decoded(&float), Err(EncodeError::Unsupported(_))));
}

// ---- PNG -------------------------------------------------------------------------------------

/// What the `png` crate reads back: colour type, depth, samples, EXIF, ICC and iTXt chunks.
type PngRead = (png::ColorType, png::BitDepth, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>, Vec<(String, String)>);

/// Decodes `bytes` with the `png` crate (independent of our decoder): its colour type, depth,
/// samples (16-bit big-endian as written) and info.
fn png_read(bytes: &[u8]) -> PngRead {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    let info = reader.info();
    let text = info.utf8_text.iter().map(|t| (t.keyword.clone(), t.get_text().unwrap())).collect();
    (info.color_type, info.bit_depth, buf, info.exif_metadata.as_ref().map(|e| e.to_vec()), info.icc_profile.as_ref().map(|e| e.to_vec()), text)
}

#[test]
fn png_keeps_depth_channels_and_alpha_exactly() {
    let (w, h) = (45u32, 31u32);
    for channels in [Channels::Grey, Channels::GreyAlpha, Channels::Rgb, Channels::Rgba] {
        let n = (w * h) as usize * channels.count();
        let eight: Vec<u8> = (0..n).map(|i| (i * 7 % 256) as u8).collect();
        // Every 16-bit value distinct in its low byte, so 8-bit truncation would show.
        let sixteen: Vec<u16> = (0..n).map(|i| (i as u32 * 40_503 % 65_536) as u16).collect();
        let want_type = match channels {
            Channels::Grey => png::ColorType::Grayscale,
            Channels::GreyAlpha => png::ColorType::GrayscaleAlpha,
            Channels::Rgb => png::ColorType::Rgb,
            Channels::Rgba => png::ColorType::Rgba,
        };
        let bytes = encode::png(&Image { width: w, height: h, channels, pixels: Pixels::U8(&eight) }, &Metadata::default()).unwrap();
        let (ct, depth, buf, ..) = png_read(&bytes);
        assert_eq!((ct, depth, &buf), (want_type, png::BitDepth::Eight, &eight), "{channels:?} 8-bit");

        let bytes = encode::png(&Image { width: w, height: h, channels, pixels: Pixels::U16(&sixteen) }, &Metadata::default()).unwrap();
        let (ct, depth, buf, ..) = png_read(&bytes);
        let back: Vec<u16> = buf.as_chunks::<2>().0.iter().map(|&[hi, lo]| u16::from_be_bytes([hi, lo])).collect();
        assert_eq!((ct, depth), (want_type, png::BitDepth::Sixteen), "{channels:?} 16-bit");
        assert_eq!(back, sixteen, "{channels:?} 16-bit samples");
        // Our own decoder keeps the 16 bits (and adds an opaque alpha where there is none).
        let header = read_header(&bytes, &Limits::default()).unwrap();
        let d = formats::png::decode(&bytes, &header, &mut Budget::new(&Limits::default(), w, h), &Limits::default()).unwrap();
        let Samples::U16(ours) = &d.samples else { panic!("{channels:?}: decoded as {:?}", d.samples.len()) };
        let k = channels.count();
        let stride = d.channels();
        assert!(d.alpha);
        for (px, want) in ours.chunks_exact(stride).zip(sixteen.chunks_exact(k)) {
            assert_eq!(&px[..k], want);
            if k % 2 == 1 {
                assert_eq!(px[k], u16::MAX);
            }
        }
        // Round trip through `Image::from_decoded`: the same file.
        if matches!(channels, Channels::GreyAlpha | Channels::Rgba) {
            assert_eq!(encode::png(&Image::from_decoded(&d).unwrap(), &Metadata::default()).unwrap(), bytes);
        }
    }
}

#[test]
fn png_metadata_chunks_read_back() {
    let (w, h) = (24, 16);
    let src = photo(4, w, h, Kind::Blobs);
    let exif = dalica_scenes::exif::tiff(&SceneExif { orientation: 8, ..SceneExif::default() }, w, h, None).unwrap();
    let xmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">Ünïcödé</x:xmpmeta>";
    let icc = moxcms::ColorProfile::new_srgb().encode().unwrap();
    let meta = Metadata { exif: Some(&exif), xmp: Some(xmp), icc: Some(&icc) };
    let bytes = encode::png(&rgb_image(&src, w, h), &meta).unwrap();
    let (_, _, buf, got_exif, got_icc, text) = png_read(&bytes);
    assert_eq!(buf, src);
    assert_eq!(got_exif.as_deref(), Some(&exif[..]));
    assert_eq!(got_icc.as_deref(), Some(&icc[..]));
    assert_eq!(text, vec![("XML:com.adobe.xmp".to_string(), xmp.to_string())]);
    // The iTXt chunk is uncompressed (XMP part 3 §1.1.5).
    let at = bytes.windows(4).position(|c| c == b"iTXt").unwrap();
    assert_eq!(&bytes[at + 4..at + 4 + 19], b"XML:com.adobe.xmp\0\0");
    // Our header reader sees the EXIF orientation.
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().orientation.get(), 8);
}

// ---- Fingerprints and timing -------------------------------------------------------------------

#[test]
fn encoded_scenes_match_their_fingerprints() {
    let (w, h) = (320, 212);
    for (name, kind, q, sub, grey) in [
        ("encode-jpeg-q90-444", Kind::Horizon, 90, Subsampling::S444, false),
        ("encode-jpeg-q75-420", Kind::Blobs, 75, Subsampling::S420, false),
        ("encode-jpeg-q85-grey", Kind::Gradient, 85, Subsampling::S444, true),
    ] {
        let src = photo(21, w, h, kind);
        let bytes = if grey {
            let g: Vec<u8> =
                src.as_chunks::<3>().0.iter().map(|&[r, g, b]| ((u32::from(r) * 77 + u32::from(g) * 150 + u32::from(b) * 29) >> 8) as u8).collect();
            encode::jpeg(&Image { width: w, height: h, channels: Channels::Grey, pixels: Pixels::U8(&g) }, q, sub, &Metadata::default()).unwrap()
        } else {
            encode::jpeg(&rgb_image(&src, w, h), q, sub, &Metadata::default()).unwrap()
        };
        assert_fingerprint(name, &decode(&bytes), 1.0);
    }
    let src = photo(22, w, h, Kind::Horizon);
    let bytes = encode::png(&rgb_image(&src, w, h), &Metadata::default()).unwrap();
    assert_fingerprint("encode-png-rgb8", &decode(&bytes), 0.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(64)))]

    /// Any size, layout, depth, quality and subsampling encodes without a panic and decodes to
    /// that size.
    #[test]
    fn any_small_image_encodes_and_decodes(w in 1u32..40, h in 1u32..40, layout in 0usize..4, wide in any::<bool>(), q in 1u8..=100, s420 in any::<bool>(), seed in any::<u8>()) {
        let channels = [Channels::Grey, Channels::GreyAlpha, Channels::Rgb, Channels::Rgba][layout];
        let n = (w * h) as usize * channels.count();
        let eight: Vec<u8> = (0..n).map(|i| (i as u8).wrapping_mul(seed)).collect();
        let sixteen: Vec<u16> = eight.iter().map(|&v| u16::from(v) * 257).collect();
        let pixels = if wide { Pixels::U16(&sixteen) } else { Pixels::U8(&eight) };
        let img = Image { width: w, height: h, channels, pixels };
        let sub = if s420 { Subsampling::S420 } else { Subsampling::S444 };
        let jpeg = encode::jpeg(&img, q, sub, &Metadata::default()).unwrap();
        let d = decode(&jpeg);
        prop_assert_eq!((d.width, d.height), (w, h));
        let png = encode::png(&img, &Metadata::default()).unwrap();
        let d = decode(&png);
        prop_assert_eq!((d.width, d.height), (w, h));
    }
}
