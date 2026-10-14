//! The one output pipeline (`architecture.md` §6, `data-model.md` §6): an empty recipe at Match
//! quality re-encodes a q75 JPEG at its own quality; a rotated recipe gives orientation 1 and the
//! rotated size; a resize hits the long side exactly and never enlarges; PNG keeps alpha and 16
//! bits; EXIF, XMP and IPTC are carried with the owned fields; GPS goes when asked; the decoded
//! output equals `edit::render` (exactly for PNG, by fingerprint for JPEG); undecodable sources,
//! hostile sizes and bad requests are `Err`, never a panic; and the pipeline never touches the
//! disk.

mod common;

use common::*;
use dalica_create::output::{Choice, Error, OutFormat, Purpose, Quality, Spec, render};
use dalica_image::encode::{Channels, Subsampling};
use dalica_image::{Format, Limits, Samples, jpeg_quality, read_header};
use dalica_meta::{Changes, Place};

fn spec(purpose: Purpose) -> Spec {
    Spec { threads: 2, ..Spec::new(purpose) }
}

fn meta(bytes: &[u8]) -> dalica_meta::Metadata {
    dalica_meta::read(bytes, &dalica_meta::Limits::default()).unwrap()
}

/// How far a re-encode at the same quality may stray from its source, decoded: the PSNR floor
/// (dB) and the largest mean absolute difference per sample (levels of 255).
const REENCODE_PSNR: f64 = 40.0;
const REENCODE_MEAN: f64 = 1.0;

#[test]
fn an_empty_recipe_at_match_reencodes_a_q75_jpeg_at_75() {
    let source = jpeg(7, 640, 480, 75, 1);
    let estimate = jpeg_quality::quality(&source).unwrap();
    assert!(estimate.abs_diff(75) <= 2, "the source estimates at {estimate}");
    let out = render(&source, &recipe(&[]), &spec(Purpose::InPlace)).unwrap();
    let r = &out.report;
    assert_eq!((r.format, r.width, r.height, r.bits), (OutFormat::Jpeg, 640, 480, 8));
    assert_eq!(r.source_quality, Some(estimate));
    assert_eq!(r.quality, Some(estimate), "Match writes the source's estimate");
    let sampling = jpeg_quality::subsampling(&source).unwrap();
    assert_eq!(r.subsampling, Some(sampling.for_jpeg()), "the source's chroma subsampling");
    let written = jpeg_quality::read(&out.bytes).unwrap();
    assert!(written.quality.abs_diff(75) <= 2, "the new file estimates at {}", written.quality);
    assert_eq!(written.sampling, sampling);

    let (before, after) = (decode(&source), decode(&out.bytes));
    let (p, (_, mean)) = (psnr(&before, &after), difference(&before, &after));
    println!("q75 re-encode: {p:.1} dB, mean difference {mean:.2} levels");
    assert!(p >= REENCODE_PSNR && mean <= REENCODE_MEAN, "re-encode error {p:.1} dB, mean {mean:.2}");
}

#[test]
fn a_4_2_0_source_stays_4_2_0_and_4_2_2_gets_more_chroma_never_less() {
    let img = pixels(7, 320, 240);
    let source = dalica_image::encode::jpeg(&dalica_image::encode::Image::rgba8(&img), 75, Subsampling::S420, &Default::default()).unwrap();
    let out = render(&source, &recipe(&[]), &spec(Purpose::InPlace)).unwrap();
    assert_eq!(out.report.subsampling, Some(Subsampling::S420));
    assert_eq!(jpeg_quality::subsampling(&out.bytes), Some(jpeg_quality::Sampling::S420));
    assert!(out.report.quality.unwrap().abs_diff(75) <= 2);
    assert_eq!(jpeg_quality::Sampling::S422.for_jpeg(), Subsampling::S444);
}

#[test]
fn a_fixed_quality_and_a_non_jpeg_source_at_match() {
    let source = jpeg(8, 320, 240, 75, 1);
    let out = render(&source, &recipe(&[]), &Spec { quality: Quality::Fixed(95), ..spec(Purpose::Export) }).unwrap();
    assert_eq!(out.report.quality, Some(95));
    assert!(jpeg_quality::quality(&out.bytes).unwrap().abs_diff(95) <= 2);

    let png = png(8, 320, 240);
    let out = render(&png, &recipe(&[]), &spec(Purpose::Beside)).unwrap();
    assert_eq!(out.report.format, OutFormat::Jpeg);
    assert_eq!(out.report.source_quality, None);
    assert_eq!(out.report.quality, Some(90), "no estimate: 90");
    assert_eq!(out.report.subsampling, Some(Subsampling::S444), "a PNG has full chroma, kept");
}

#[test]
fn a_rotated_recipe_gives_orientation_1_and_the_rotated_size() {
    // A 300 × 200 picture stored turned, with EXIF orientation 6, then a quarter turn: 200 × 300.
    let source = jpeg(9, 300, 200, 90, 6);
    let stored = read_header(&source, &Limits::default()).unwrap();
    assert_eq!((stored.width, stored.height, stored.orientation.get()), (200, 300, 6));
    let r = recipe(&["rotate(90)"]);
    let shown = r.display_size(stored.orientation, stored.width, stored.height);
    assert_eq!(shown, (200, 300));
    let out = render(&source, &r, &spec(Purpose::InPlace)).unwrap();
    assert_eq!((out.report.width, out.report.height), shown);
    let header = read_header(&out.bytes, &Limits::default()).unwrap();
    assert_eq!((header.width, header.height, header.orientation.get()), (200, 300, 1));
    assert_eq!(meta(&out.bytes).summary.orientation, Some(1));
    // The pixels are the turned picture.
    let (max, mean) = difference(&decode(&out.bytes), &oracle(&source, &r));
    assert!(mean < 2.0 && max < 64, "the saved pixels differ from the render: mean {mean:.2}, max {max}");
}

#[test]
fn a_resize_hits_the_long_side_exactly_and_never_enlarges() {
    for (w, h, side, want) in [
        (300, 200, 120, (120, 80)),
        (200, 300, 120, (80, 120)),
        (301, 199, 100, (100, 66)),
        (300, 200, 300, (300, 200)),
        (300, 200, 4000, (300, 200)),
    ] {
        let source = jpeg(10, w, h, 85, 1);
        let out = render(&source, &recipe(&[]), &Spec { long_side: Some(side), ..spec(Purpose::Export) }).unwrap();
        assert_eq!((out.report.width, out.report.height), want, "{w} × {h} to {side}");
        let header = read_header(&out.bytes, &Limits::default()).unwrap();
        assert_eq!((header.width, header.height), want);
    }
    // A crop that leaves the picture smaller than the long side is not enlarged either.
    let source = jpeg(10, 300, 200, 85, 1);
    let out = render(&source, &recipe(&["crop(0,0,0.5,0.5)"]), &Spec { long_side: Some(200), ..spec(Purpose::Export) }).unwrap();
    assert_eq!((out.report.width, out.report.height), (150, 100));
}

#[test]
fn a_png_with_alpha_stays_png_with_alpha() {
    let img = with_alpha(11, 96, 64);
    let source = png_rgba(&img);
    let out = render(&source, &recipe(&[]), &spec(Purpose::Export)).unwrap();
    assert_eq!((out.report.format, out.report.channels, out.report.bits), (OutFormat::Png, Channels::Rgba, 8));
    assert_eq!(dalica_image::sniff(&out.bytes), Some(Format::Png));
    assert_eq!(decode(&out.bytes), img, "lossless, alpha and all");
    // Saving it in place is not possible: it goes beside as a JPEG, flattened over white.
    assert!(matches!(render(&source, &recipe(&[]), &spec(Purpose::InPlace)), Err(Error::NotInPlace(Format::Png))));
    let beside = render(&source, &recipe(&[]), &spec(Purpose::Beside)).unwrap();
    assert_eq!(beside.report.format, OutFormat::Jpeg);
    let left = decode(&beside.bytes);
    let corner = &left.data[..3];
    assert!(corner.iter().all(|&v| v >= 245), "a transparent pixel flattens to white, got {corner:?}");
}

#[test]
fn a_16_bit_png_stays_16_bit() {
    let img = pixels(12, 80, 60);
    // In place, rotated: the 16-bit samples move pixel for pixel, low bytes included.
    let source = png16(&img, false, None);
    let r = recipe(&["rotate(90)", "flip_h()"]);
    let out = render(&source, &r, &spec(Purpose::InPlace)).unwrap();
    assert_eq!((out.report.format, out.report.bits, out.report.width, out.report.height), (OutFormat::Png, 16, 60, 80));
    assert_eq!(out.report.channels, Channels::Rgb, "no alpha without transparency");
    let (src, dst) = (decode_stored(&source), decode_stored(&out.bytes));
    let (Samples::U16(src), Samples::U16(dst)) = (&src.samples, &dst.samples) else { panic!("not 16-bit") };
    let t = r.display(dalica_image::Orientation::default());
    for y in 0..60u32 {
        for x in 0..80u32 {
            let (nx, ny) = t.map_pixel(x, y, 80, 60);
            // Both decode with the opaque alpha the PNG decoder adds.
            let (a, b) = (((y * 80 + x) * 4) as usize, ((ny * 60 + nx) * 4) as usize);
            assert_eq!(src[a..a + 4], dst[b..b + 4], "pixel {x},{y}");
        }
    }
    // With an alpha channel and an edit that changes pixels: still 16 bits and alpha (rendered at
    // 8 bits and widened), equal to the render.
    let source = png16(&with_alpha(12, 80, 60), true, None);
    let r = recipe(&["tuning(0.3,0,0,0.2,none)"]);
    let out = render(&source, &r, &spec(Purpose::Export)).unwrap();
    assert_eq!((out.report.format, out.report.bits, out.report.channels), (OutFormat::Png, 16, Channels::Rgba));
    assert_eq!(decode(&out.bytes), oracle(&source, &r));
    // A JPEG target takes 8 bits.
    let out = render(&png16(&img, false, None), &recipe(&[]), &spec(Purpose::Copy)).unwrap();
    assert_eq!((out.report.format, out.report.bits), (OutFormat::Jpeg, 8));
}

#[test]
fn a_grey_source_stays_grey_while_its_pixels_stay_neutral() {
    let img = pixels(13, 64, 48);
    let grey: Vec<u8> = img.data.as_chunks::<4>().0.iter().map(|p| p[1]).collect();
    let image = dalica_image::encode::Image { width: 64, height: 48, channels: Channels::Grey, pixels: dalica_image::encode::Pixels::U8(&grey) };
    let source = dalica_image::encode::jpeg(&image, 90, Subsampling::S444, &Default::default()).unwrap();
    let out = render(&source, &recipe(&["rotate(180)"]), &spec(Purpose::InPlace)).unwrap();
    assert_eq!((out.report.channels, out.report.subsampling), (Channels::Grey, None));
    // A warming tune adds colour: RGB.
    let out = render(&source, &recipe(&["tuning(0,0,0,0.8,none)"]), &spec(Purpose::InPlace)).unwrap();
    assert_eq!(out.report.channels, Channels::Rgb);
}

/// A JPEG with the source metadata a camera and Dalica's write-back leave: EXIF (orientation 6,
/// make, model, GPS), XMP and IPTC with a caption and tags.
fn described(place: bool) -> Vec<u8> {
    let source = jpeg(14, 300, 200, 90, 6);
    let changes = Changes {
        caption: Some(Some("Harbour at dusk".into())),
        tags: Some(vec!["boats".into(), "evening".into()]),
        place: place.then(|| Place::new(38.708, -9.136)),
        ..Changes::default()
    };
    dalica_meta::write_owned(&source, &changes, &dalica_meta::WriteOptions::default()).unwrap()
}

#[test]
fn exif_xmp_and_iptc_are_carried_with_the_owned_fields() {
    let source = described(true);
    let before = meta(&source);
    assert!(before.sources.xmp.caption.is_some() && before.sources.exif.place.is_some());
    let owned = Changes { tags: Some(vec!["boats".into(), "evening".into(), "exported".into()]), star: Some(true), ..Changes::default() };
    let write = dalica_meta::WriteOptions { stars_as_ratings: true, ..dalica_meta::WriteOptions::default() };
    let s = Spec { changes: owned, write, thumbnail: true, ..spec(Purpose::Copy) };
    let out = render(&source, &recipe(&["rotate(270)"]), &s).unwrap();
    let after = meta(&out.bytes);
    assert!(after.problems.is_empty(), "{:?}", after.problems);
    assert!(out.report.source_metadata);
    assert_eq!(after.summary.make.as_deref(), Some("Dalicam"));
    assert_eq!(after.summary.model.as_deref(), Some("Model 7"));
    assert_eq!(after.summary.orientation, Some(1));
    assert_eq!(after.owned.caption.as_deref(), Some("Harbour at dusk"));
    assert_eq!(after.sources.xmp.caption, before.sources.xmp.caption, "XMP carried");
    assert!(before.sources.iptc.caption.is_some(), "the source has IPTC");
    assert_eq!(after.sources.iptc.caption, before.sources.iptc.caption, "IPTC carried");
    assert_eq!(after.owned.tags, ["boats", "evening", "exported"], "the owned fields merged");
    assert!(after.owned.starred());
    assert_eq!(after.owned.place, before.owned.place, "GPS kept unless asked");
    // A fresh thumbnail of the new pixels, upright.
    let header = read_header(&out.bytes, &Limits::default()).unwrap();
    assert!(out.report.thumbnail);
    let thumb = read_header(header.thumbnail(&out.bytes).unwrap(), &Limits::default()).unwrap();
    assert_eq!((thumb.width, thumb.height), (107, 160), "the thumbnail of a 200 × 300 picture");
}

#[test]
fn gps_is_removed_when_asked_and_place_names_stay() {
    let source = described(true);
    let out = render(&source, &recipe(&[]), &Spec { remove_location: true, ..spec(Purpose::Export) }).unwrap();
    let after = meta(&out.bytes);
    assert_eq!(after.owned.place, None);
    assert_eq!(after.sources.exif.place, None);
    assert_eq!(after.sources.xmp.place, None);
    assert_eq!(after.owned.caption.as_deref(), Some("Harbour at dusk"));
    // A place passed as an owned field is not written back either.
    let s = Spec { remove_location: true, changes: Changes { place: Some(Place::new(1.0, 2.0)), ..Changes::default() }, ..spec(Purpose::Export) };
    assert_eq!(meta(&render(&source, &recipe(&[]), &s).unwrap().bytes).owned.place, None);
    // An export without a thumbnail has none.
    let header = read_header(&out.bytes, &Limits::default()).unwrap();
    assert!(header.thumbnail.is_none() && !out.report.thumbnail);
}

#[test]
fn a_source_whose_metadata_dalica_does_not_read_still_gets_the_facts() {
    let gif = dalica_scenes::formats::gif(&scene(15, 64, 48), 1).unwrap();
    let s = Spec { changes: Changes { caption: Some(Some("From a GIF".into())), ..Changes::default() }, ..spec(Purpose::Beside) };
    let out = render(&gif, &recipe(&[]), &s).unwrap();
    assert!(!out.report.source_metadata);
    let after = meta(&out.bytes);
    assert_eq!(after.summary.orientation, Some(1));
    assert_eq!(after.owned.caption.as_deref(), Some("From a GIF"));
}

#[test]
fn an_srgb_profile_only_when_the_source_had_one() {
    use dalica_scenes::icc::{Space, Version, rgb};
    let tagged = dalica_scenes::formats::jpeg_icc(&scene(16, 64, 48), 90, None, &rgb(Space::DisplayP3, Version::V4)).unwrap();
    let out = render(&tagged, &recipe(&[]), &spec(Purpose::InPlace)).unwrap();
    assert!(out.report.icc);
    let profile = icc_of(&out.bytes).expect("an embedded profile");
    assert_ne!(profile, rgb(Space::DisplayP3, Version::V4), "not the source's profile");
    // The embedded profile is sRGB: applying it changes (almost) nothing.
    let managed = decode(&out.bytes);
    let ignored = dalica_image::decode_with(&out.bytes, &dalica_image::DecodeOptions { color_managed: false }, &Limits::default()).unwrap();
    let (max, _) = difference(&managed, &ignored);
    assert!(max <= 2, "the embedded profile is not sRGB: {max} levels apart");
    // The pixels are in sRGB: the decode of the new file equals the colour-managed source.
    let (max, mean) = difference(&decode(&out.bytes), &decode(&tagged));
    assert!(mean < 2.5 && max < 64, "mean {mean:.2}, max {max}");

    let untagged = jpeg(16, 64, 48, 90, 1);
    let out = render(&untagged, &recipe(&[]), &spec(Purpose::InPlace)).unwrap();
    assert!(!out.report.icc && icc_of(&out.bytes).is_none());
    // A tagged 16-bit PNG goes through the colour stage (8 bits widened) and stays 16-bit.
    let deep = png16(&pixels(16, 40, 30), false, Some(&rgb(Space::AdobeRgb, Version::V4)));
    let out = render(&deep, &recipe(&[]), &spec(Purpose::InPlace)).unwrap();
    assert_eq!((out.report.bits, out.report.icc), (16, true));
    assert_eq!(decode(&out.bytes), decode(&deep));
}

/// The ICC profile of a JPEG (APP2 `ICC_PROFILE` chunks, one here) or PNG (`iCCP`), as the
/// decoder sees it.
fn icc_of(bytes: &[u8]) -> Option<Vec<u8>> {
    decode_stored(bytes).icc
}

#[test]
fn the_decoded_output_equals_edit_render() {
    let r = recipe(&["crop(0.05,0.1,0.95,0.85)", "tuning(0.35,0.1,0.2,0.3,none)", "rotate(90)"]);
    // PNG is lossless: exactly the render.
    let source = png(17, 320, 240);
    let out = render(&source, &r, &Spec { format: Choice::Exactly(OutFormat::Png), ..spec(Purpose::Copy) }).unwrap();
    let expected = oracle(&source, &r);
    let got = decode(&out.bytes);
    assert_eq!(got, expected, "a PNG output decodes to exactly the render");
    check_fingerprint("output-png-crop-tune-rotate", &got, 0.5);

    // JPEG: within the encoder's error of the render, and its own fingerprint.
    let source = jpeg(17, 320, 240, 92, 8);
    let out = render(&source, &r, &spec(Purpose::InPlace)).unwrap();
    let expected = oracle(&source, &r);
    let got = decode(&out.bytes);
    let p = psnr(&got, &expected);
    println!("JPEG output vs render: {p:.1} dB");
    assert!(p >= REENCODE_PSNR, "{p:.1} dB");
    check_fingerprint("output-jpeg-crop-tune-rotate", &got, 1.0);

    // Resized: the render fitted by `image::resize`.
    let out =
        render(&png(17, 320, 240), &r, &Spec { long_side: Some(100), format: Choice::Exactly(OutFormat::Png), ..spec(Purpose::Export) }).unwrap();
    let fitted = dalica_image::resize::fit(&oracle(&png(17, 320, 240), &r), 100).unwrap();
    assert_eq!(decode(&out.bytes), fitted);
}

#[test]
fn an_undecodable_source_and_hostile_sizes_are_err() {
    let any = recipe(&[]);
    let s = spec(Purpose::Export);
    assert!(matches!(render(b"", &any, &s), Err(Error::Decode(_))));
    assert!(matches!(render(b"not a picture at all, just some words", &any, &s), Err(Error::Decode(_))));
    // Truncated anywhere: an `Err` or a picture, never a panic.
    let source = jpeg(18, 64, 48, 80, 6);
    for cut in (0..source.len()).step_by(37) {
        let _ = render(&source[..cut], &any, &s);
    }
    // A PNG header declaring 70 000 × 70 000 pixels is refused before any pixel buffer.
    let mut huge = png(18, 8, 8);
    huge[16..20].copy_from_slice(&70_000u32.to_be_bytes());
    huge[20..24].copy_from_slice(&70_000u32.to_be_bytes());
    let crc = crc32(&huge[12..29]);
    huge[29..33].copy_from_slice(&crc.to_be_bytes());
    assert!(matches!(render(&huge, &any, &s), Err(Error::Decode(dalica_image::Error::TooLarge { .. }))));
    // Tight limits refuse an ordinary photo.
    let small = Spec { limits: Limits { max_pixels: 100, ..Limits::default() }, ..spec(Purpose::Export) };
    assert!(render(&jpeg(18, 64, 48, 80, 1), &any, &small).is_err());
    // Bad requests.
    let source = jpeg(18, 64, 48, 80, 1);
    for bad in [
        Spec { long_side: Some(0), ..spec(Purpose::Export) },
        Spec { quality: Quality::Fixed(0), ..spec(Purpose::Export) },
        Spec { quality: Quality::Fixed(101), ..spec(Purpose::Export) },
    ] {
        assert!(matches!(render(&source, &any, &bad), Err(Error::Spec(_))), "{bad:?}");
    }
    // A TIFF cannot be saved in place.
    let tiff = dalica_scenes::formats::tiff(&scene(18, 32, 24), &dalica_scenes::formats::Tiff::default()).unwrap();
    let err = render(&tiff, &any, &spec(Purpose::InPlace)).unwrap_err();
    assert_eq!(err, Error::NotInPlace(Format::Tiff));
    assert_eq!(err.reason(), "TIFF files cannot be saved in place");
    assert_eq!(render(b"junk", &any, &s).unwrap_err().reason(), "Damaged file");
}

/// The CRC-32 of a PNG chunk's type and data (ISO 3309).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[test]
fn nothing_is_written_to_disk() {
    // The pipeline's signature is bytes in, bytes out; its sources never name a file call or
    // `fs` (the `layers` gate refuses file writes outside `fs` too). `create` uses `fs` only in
    // `export` and `save`, which write what the pipeline gives them.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![dir.join("output.rs"), dir.join("watermark.rs")];
    files.extend(std::fs::read_dir(dir.join("output")).unwrap().map(|e| e.unwrap().path()));
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for call in ["std::fs", "File::", "OpenOptions", "dalica_fs", "fs::", "create_dir", "write_all"] {
            assert!(!text.contains(call), "{} names {call}", file.display());
        }
    }
}

#[test]
fn a_24_mp_save_is_timed() {
    // A 6000 × 4000 JPEG through the whole pipeline: an empty recipe at Match (Save) and an
    // export to 2048 pixels; `--release` gives the numbers worth comparing.
    let source = jpeg(19, 6000, 4000, 90, 1);
    for (name, s) in [("save", spec(Purpose::InPlace)), ("export 2048", Spec { long_side: Some(2048), ..spec(Purpose::Export) })] {
        let s = Spec { threads: 4, thumbnail: name == "save", ..s };
        let t = std::time::Instant::now();
        let out = render(&source, &recipe(&[]), &s).unwrap();
        println!(
            "24 MP {name}: {} × {} in {:.0} ms, {} bytes",
            out.report.width,
            out.report.height,
            t.elapsed().as_secs_f64() * 1000.0,
            out.bytes.len()
        );
    }
}
