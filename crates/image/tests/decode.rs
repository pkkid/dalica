//! Decoding against `dalica-scenes` sources, checked by fingerprints (`architecture.md` §11):
//! per-channel mean and standard deviation over an 8 × 8 grid, with a tolerance. On a mismatch the
//! test writes actual and expected PNGs under `target/test-output/image/` to look at.

use crate::common::{assert_cells_close, cases, rgba};
use std::time::Instant;

use dalica_image::color::load_profile;
use dalica_image::formats;
use dalica_image::{ColorModel, DecodeOptions, Error, Format, Limits, RawKind, decode, decode_as, decode_thumbnail, decode_with, read_header, sniff};
use dalica_scenes::{ByteOrder, Exif, Scene, jpeg, png};
use proptest::prelude::*;

fn camera(scene: &Scene, o: u8) -> Vec<u8> {
    let exif = Exif { orientation: o, thumbnail: Some(160), byte_order: if o > 4 { ByteOrder::Big } else { ByteOrder::Little }, ..Exif::default() };
    jpeg(scene, 95, Some(&exif)).unwrap()
}

#[test]
fn each_orientation_decodes_upright() {
    let scene = Scene::new(17, 240, 160);
    let source = rgba(&scene.render().unwrap());
    let reference = decode(&camera(&scene, 1), &Limits::default()).unwrap();
    assert_cells_close("orientation-1-vs-source", &reference, &source, 3.0);
    for o in 2..=8 {
        let img = decode(&camera(&scene, o), &Limits::default()).unwrap();
        assert_eq!((img.width, img.height), (240, 160), "orientation {o}");
        assert_cells_close(&format!("orientation-{o}"), &img, &reference, 3.0);
    }
}

#[test]
fn thumbnails_are_upright_and_fit() {
    let scene = Scene::new(21, 480, 320);
    let full = rgba(&scene.render().unwrap());
    for o in [1, 6, 8, 3] {
        let t = decode_thumbnail(&camera(&scene, o), 120, &Limits::default()).unwrap();
        assert_eq!((t.width, t.height), (120, 80), "orientation {o}");
        let expected = dalica_image::resize::fit(&full, 120).unwrap();
        assert_cells_close(&format!("thumbnail-{o}"), &t, &expected, 3.0);
    }
}

#[test]
fn png_decodes_exactly_in_any_layout() {
    let scene = Scene::new(8, 37, 23);
    assert_eq!(decode(&png(&scene).unwrap(), &Limits::default()).unwrap(), rgba(&scene.render().unwrap()));
    // 16-bit grey with alpha: normalised to 8-bit RGBA.
    let mut bytes = Vec::new();
    let mut enc = ::png::Encoder::new(&mut bytes, 2, 1);
    enc.set_color(::png::ColorType::GrayscaleAlpha);
    enc.set_depth(::png::BitDepth::Sixteen);
    enc.write_header().unwrap().write_image_data(&[0xff, 0xff, 0x80, 0x00, 0x00, 0x00, 0xff, 0xff]).unwrap();
    let img = decode(&bytes, &Limits::default()).unwrap();
    assert_eq!(img.data, [255, 255, 255, 128, 0, 0, 0, 255]);
}

/// A PNG whose IHDR claims `w × h`, followed by nothing useful.
fn png_claiming(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]); // RGBA8, then a CRC the header reader does not need
    b.extend(b"\0\0\0\0IEND\xae\x42\x60\x82");
    b
}

/// The size is refused from the header alone. Allocating for it (40 GB) would abort the test
/// process, so passing at all shows nothing was allocated.
#[test]
fn a_header_bomb_is_refused_before_allocating() {
    let start = Instant::now();
    for (w, h) in [(100_000, 100_000), (65_535, 65_535), (1, 2_000_000_000)] {
        let bytes = png_claiming(w, h);
        assert!(matches!(decode(&bytes, &Limits::default()), Err(Error::TooLarge { .. })), "{w}x{h}");
        assert!(matches!(decode_thumbnail(&bytes, 256, &Limits::default()), Err(Error::TooLarge { .. })), "{w}x{h}");
    }
    // A JPEG frame header can claim at most 65 535 × 65 535 = 4.3 Gpx: also refused.
    let mut jpg = jpeg(&Scene::new(1, 16, 16), 80, None).unwrap();
    let sof = jpg.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    jpg[sof + 5..sof + 9].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    assert!(matches!(decode(&jpg, &Limits::default()), Err(Error::TooLarge { .. })));
    assert!(start.elapsed().as_millis() < 500, "{:?}", start.elapsed());
}

#[test]
fn past_64_megapixels_a_thumbnail_comes_from_the_embedded_copy() {
    let scene = Scene::new(5, 400, 300);
    let mut bytes = camera(&scene, 1);
    // Make the frame header claim 9000 × 9000 (81 MP): the embedded 160 × 120 thumbnail is used.
    let sof = bytes.windows(2).rposition(|w| w == [0xff, 0xc0]).unwrap();
    bytes[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().width, 9000);
    let t = decode_thumbnail(&bytes, 100, &Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (100, 75));
    // Without an embedded thumbnail, it is refused.
    let mut plain = jpeg(&scene, 90, None).unwrap();
    let sof = plain.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    plain[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
    assert!(matches!(decode_thumbnail(&plain, 100, &Limits::default()), Err(Error::TooLarge { .. })));
}

#[test]
fn a_256_px_thumbnail_of_a_large_jpeg() {
    // 1.5 MP; speed is the release benches'.
    let bytes = jpeg(&Scene::new(24, 1500, 1000), 90, None).unwrap();
    let t = decode_thumbnail(&bytes, 256, &Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (256, 171));
}

/// A little-endian TIFF whose IFD0 holds one SHORT/LONG entry.
fn tiff_with(tag: u16, kind: u16, value: u32) -> Vec<u8> {
    let mut b = b"II*\0\x08\0\0\0".to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(tag.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.extend(value.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b
}

/// The start of a file of every format Dalica recognises but does not decode yet, padded past
/// the sniffing length.
fn stub_files() -> Vec<(Format, Vec<u8>)> {
    let pad = |head: &[u8]| [head, &[0u8; 64]].concat();
    vec![
        (Format::Tiff, tiff_with(0x0106, 3, 2)),
        (Format::WebP, pad(b"RIFF\x24\0\0\0WEBPVP8 ")),
        (Format::Gif, pad(b"GIF89a\x01\0\x01\0")),
        (Format::Bmp, pad(b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0")),
        (Format::JpegXl, pad(b"\xff\x0a\xfa")),
        (Format::JpegXl, pad(b"\0\0\0\x0cJXL \r\n\x87\n")),
        (Format::Psd, pad(b"8BPS\0\x01")),
        (Format::Heif, pad(b"\0\0\0\x18ftypheic\0\0\0\0")),
        (Format::Avif, pad(b"\0\0\0\x1cftypavif\0\0\0\0")),
        (Format::Raw(RawKind::TiffBased), tiff_with(0xc612, 1, 0x0001_0401)),
        (Format::Raw(RawKind::TiffBased), pad(b"II*\0\x10\0\0\0CR\x02\0")),
        (Format::Raw(RawKind::TiffBased), pad(b"IIRO\x08\0\0\0")),
        (Format::Raw(RawKind::TiffBased), pad(b"IIU\0\x08\0\0\0")),
        (Format::Raw(RawKind::Cr3), pad(b"\0\0\0\x18ftypcrx \0\0\0\x01")),
        (Format::Raw(RawKind::Raf), pad(b"FUJIFILMCCD-RAW 0201")),
        (Format::Raw(RawKind::Crw), pad(b"II\x1a\0\0\0HEAPCCDR")),
        (Format::Raw(RawKind::Mrw), pad(b"\0MRM\0\0\0\x08")),
        (Format::Raw(RawKind::X3f), pad(b"FOVb\0\0\x04\0")),
    ]
}

/// Every format without a decoder yet is recognised by content and answers `Unsupported` with
/// its tile text on every path; one that has gained a decoder refuses these few bytes as damaged.
#[test]
fn every_stub_format_sniffs_and_answers_its_reason() {
    let l = Limits::default();
    let files = stub_files();
    for f in Format::ALL {
        assert!(files.iter().any(|(g, _)| *g == f) || f.supported(), "{f:?} has no test file");
    }
    for (format, bytes) in files {
        assert_eq!(sniff(&bytes), Some(format), "{format:?}");
        let results = [
            read_header(&bytes, &l).map(|_| ()),
            decode(&bytes, &l).map(|_| ()),
            decode_thumbnail(&bytes, 128, &l).map(|_| ()),
            decode_as(format, &bytes, &DecodeOptions::default(), &l).map(|_| ()),
        ];
        for result in results {
            let err = result.unwrap_err();
            if format.supported() {
                assert_eq!(err.reason(), "Damaged file", "{format:?}: {err:?}");
            } else {
                assert_eq!(err, Error::Unsupported(format));
                assert_eq!(err.reason(), formats::reason(format));
                assert!(!err.reason().is_empty());
            }
        }
        if !format.supported() && format != Format::Avif {
            assert_eq!(formats::embedded_thumbnail(format, &bytes, &l), Err(Error::Unsupported(format)));
        }
    }
    // The texts `formats.md` §1 gives.
    assert_eq!(Error::Unsupported(Format::Avif).reason(), "AVIF files can't be displayed");
    if !Format::Heif.supported() {
        assert_eq!(Error::Unsupported(Format::Heif).reason(), "HEIC support isn't included in this build");
    }
    assert_eq!(Error::TooLarge { width: 70_000, height: 500 }.reason(), "Too large to display (70000 × 500)");
    assert_eq!(Error::Truncated.reason(), "Damaged file");
    assert_eq!(Error::Panic(Format::Jpeg).reason(), "Damaged file");
}

/// A file sniffed as one format and decoded as another is refused, never misread.
#[test]
fn decoding_as_the_wrong_format_is_refused() {
    let png_bytes = png(&Scene::new(9, 24, 16)).unwrap();
    let jpg = jpeg(&Scene::new(9, 24, 16), 80, None).unwrap();
    assert!(decode_as(Format::Jpeg, &png_bytes, &DecodeOptions::default(), &Limits::default()).is_err());
    assert!(decode_as(Format::Png, &jpg, &DecodeOptions::default(), &Limits::default()).is_err());
    assert_eq!(decode_as(Format::Png, &png_bytes, &DecodeOptions::default(), &Limits::default()), decode(&png_bytes, &Limits::default()));
}

/// A magic number of every format and container (and the variants the format tests used).
const HEADS: &[&[u8]] = &[
    b"\xff\xd8\xff",
    b"\x89PNG\r\n\x1a\n",
    b"II*\0",
    b"MM\0*",
    b"II*\0\x08\0\0\0",
    b"MM\0*\0\0\0\x08",
    b"II*\0\x10\0\0\0CR\x02\0",
    b"IIRO",
    b"IIRO\x08\0\0\0",
    b"MMOR\0\0\0\x08",
    b"IIU\0\x08\0\0\0",
    b"RIFF\0\0\0\0WEBP",
    b"RIFF\0\0\0\0WEBPVP8L",
    b"RIFF\0\0\0\0WEBPVP8X",
    b"RIFF\0\0\0\0WEBPVP8 ",
    b"GIF89a",
    b"GIF87a",
    b"BM\0\0\0\0\0\0\0\0\x36",
    b"BM\0\0\0\0\0\0\0\0\x36\0\0\0",
    b"\xff\x0a",
    b"\0\0\0\x0cJXL \r\n\x87\n",
    b"8BPS",
    b"\0\0\0\x18ftypheic",
    b"\0\0\0\x18ftypheic\0\0\0\0mif1heic",
    b"\0\0\0\x18ftypmif1\0\0\0\0mif1avif",
    b"\0\0\0\x18ftypmsf1\0\0\0\0msf1hevc",
    b"\0\0\0\x18ftypcrx ",
    b"\0\0\0\x18ftypcrx \0\0\0\x01",
    b"FUJIFILMCCD-RAW ",
    b"FUJIFILMCCD-RAW 0201",
    b"II\x1a\0\0\0HEAPCCDR",
    b"MM\0\0\0\x1aHEAPCCDR",
    b"\0MRM",
    b"\0MRM\0\0\0\x40",
    b"FOVb",
    b"FOVb\0\0\x02\0",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(2048)))]

    /// Random bytes behind every magic number, through every entry point and every format
    /// module: `Ok` or `Err`, never a panic that escapes (a third-party decoder's caught panic is
    /// an `Err` like any other). The one random-bytes test of the crate: each format's own tests
    /// sweep truncations and corrupt real files (2048 cases in full mode, the sum of the
    /// per-format random-bytes tests it replaced).
    #[test]
    fn random_bytes_never_panic_on_any_path(head in 0..HEADS.len(), tail in proptest::collection::vec(any::<u8>(), 0..2048), managed in any::<bool>()) {
        let bytes = [HEADS[head], &tail].concat();
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let options = DecodeOptions { color_managed: managed };
        let _ = sniff(&bytes);
        let _ = read_header(&bytes, &l);
        let _ = decode_with(&bytes, &options, &l);
        let _ = decode_thumbnail(&bytes, 64, &l);
        let _ = dalica_image::exif::read(&bytes, &l);
        let _ = formats::tiff::thumbnail(&bytes, 8, &options, &l);
        for f in Format::ALL {
            let _ = decode_as(f, &bytes, &options, &l);
            let _ = formats::embedded_thumbnail(f, &bytes, &l);
        }
        for model in [ColorModel::Gray, ColorModel::Rgb, ColorModel::Cmyk] {
            let _ = load_profile(&bytes, model, &l);
        }
    }
}
