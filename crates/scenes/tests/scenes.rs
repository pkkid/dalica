//! `dalica-scenes` against an independent decoder (the `image` crate): determinism, all eight
//! orientations, the EXIF fields, the embedded thumbnail and the demo library
//! (`architecture.md` §11: "determinism (same seed → same bytes) and that its EXIF block decodes").

use std::io::Cursor;
use std::time::Instant;

use dalica_core::DateTime;
use dalica_scenes::exif::{TAG_DATE_TIME_ORIGINAL, TAG_EXIF_IFD, TAG_MAKE, TAG_ORIENTATION, TAG_THUMB_LENGTH, TAG_THUMB_OFFSET};
use dalica_scenes::{ByteOrder, Exif, Kind, Rgb8, Scene, demo_library, jpeg, orient, png};
use image::{ImageDecoder, ImageReader};

/// Width, height, EXIF orientation, raw EXIF and RGB pixels as the `image` crate decodes them.
fn decode(bytes: &[u8]) -> (u32, u32, u8, Option<Vec<u8>>, Vec<u8>) {
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().unwrap();
    let mut decoder = reader.into_decoder().unwrap();
    let orientation = decoder.orientation().unwrap().to_exif();
    let exif = decoder.exif_metadata().unwrap();
    let img = image::DynamicImage::from_decoder(decoder).unwrap().to_rgb8();
    (img.width(), img.height(), orientation, exif, img.into_raw())
}

/// Mean absolute difference per channel between two same-sized images.
fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / a.len() as f64
}

/// IFD entries `(tag, value-or-offset)` of the IFD at `at`, and the next IFD's offset.
fn ifd(t: &[u8], at: usize) -> (Vec<(u16, u32)>, u32) {
    let le = &t[..2] == b"II";
    let u16_at = |i: usize| if le { u16::from_le_bytes([t[i], t[i + 1]]) } else { u16::from_be_bytes([t[i], t[i + 1]]) };
    let u32_at = |i: usize| {
        let b = [t[i], t[i + 1], t[i + 2], t[i + 3]];
        if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
    };
    let n = u16_at(at) as usize;
    let entries = (0..n)
        .map(|k| at + 2 + 12 * k)
        .map(|e| (u16_at(e), if u16_at(e + 2) == 3 && u32_at(e + 4) == 1 { u32::from(u16_at(e + 8)) } else { u32_at(e + 8) }))
        .collect();
    (entries, u32_at(at + 2 + 12 * n))
}

fn value(entries: &[(u16, u32)], tag: u16) -> u32 {
    entries.iter().find(|e| e.0 == tag).unwrap().1
}

#[test]
fn the_same_seed_gives_identical_bytes() {
    let exif = Exif { orientation: 6, thumbnail: Some(160), date_time_original: "2026-06-04T19:42:10".parse().ok(), ..Exif::default() };
    for kind in Kind::ALL {
        let s = Scene::new(42, 300, 200).with_kind(kind);
        assert_eq!(jpeg(&s, 90, Some(&exif)).unwrap(), jpeg(&s, 90, Some(&exif)).unwrap());
        assert_eq!(png(&s).unwrap(), png(&s).unwrap());
        assert_ne!(png(&s).unwrap(), png(&Scene { seed: 43, ..s }).unwrap());
    }
    let a = demo_library(2, 24, 7).unwrap();
    assert_eq!(a, demo_library(2, 24, 7).unwrap());
    assert_ne!(a, demo_library(2, 24, 8).unwrap());
}

#[test]
fn all_eight_orientations_are_written_and_display_upright() {
    let scene = Scene::new(9, 240, 160);
    let upright = scene.render().unwrap();
    for o in 1..=8u8 {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let bytes = jpeg(&scene, 95, Some(&Exif { orientation: o, byte_order: order, ..Exif::default() })).unwrap();
            let (w, h, read_o, _, pixels) = decode(&bytes);
            assert_eq!(read_o, o, "{order:?}");
            assert_eq!((w, h), if o >= 5 { (160, 240) } else { (240, 160) });
            // Applying the orientation to what the decoder returns gives the upright scene back.
            let shown = orient::display(&Rgb8 { width: w, height: h, data: pixels }, o).unwrap();
            let diff = mean_diff(&shown.data, &upright.data);
            assert!(diff < 3.0, "orientation {o}: mean difference {diff}");
        }
    }
}

#[test]
fn png_decodes_to_the_exact_pixels() {
    let scene = Scene::new(5, 123, 77);
    let (w, h, o, exif, pixels) = decode(&png(&scene).unwrap());
    assert_eq!((w, h, o, exif), (123, 77, 1, None));
    assert_eq!(pixels, scene.render().unwrap().data);
}

#[test]
fn the_exif_block_carries_date_camera_and_a_decodable_thumbnail() {
    let date: DateTime = "2026-06-04T19:42:10".parse().unwrap();
    let exif = Exif {
        orientation: 8,
        date_time_original: Some(date),
        make: Some("Dalica".into()),
        model: Some("Scenes 1".into()),
        thumbnail: Some(160),
        byte_order: ByteOrder::Big,
    };
    let bytes = jpeg(&Scene::new(3, 400, 300), 85, Some(&exif)).unwrap();
    let (_, _, _, raw, _) = decode(&bytes);
    let t = raw.expect("an EXIF block");
    let (ifd0, next) = ifd(&t, 8);
    assert_eq!(value(&ifd0, TAG_ORIENTATION), 8);
    let make = value(&ifd0, TAG_MAKE) as usize;
    assert_eq!(&t[make..make + 7], b"Dalica\0");
    let (exif_ifd, _) = ifd(&t, value(&ifd0, TAG_EXIF_IFD) as usize);
    let at = value(&exif_ifd, TAG_DATE_TIME_ORIGINAL) as usize;
    assert_eq!(&t[at..at + 19], b"2026:06:04 19:42:10");
    let (ifd1, _) = ifd(&t, next as usize);
    let (off, len) = (value(&ifd1, TAG_THUMB_OFFSET) as usize, value(&ifd1, TAG_THUMB_LENGTH) as usize);
    let (tw, th, _, _, _) = decode(&t[off..off + len]);
    // Stored like the main image (orientation 8 swaps the sides): 300 × 400 → 120 × 160.
    assert_eq!((tw, th), (120, 160));
}

#[test]
fn the_demo_library_is_spread_over_folders_with_dates_and_orientations() {
    let start = Instant::now();
    let files = demo_library(3, 200, 1).unwrap();
    let elapsed = start.elapsed();
    println!("demo_library(3, 200): {} files, {} KiB, {elapsed:?}", files.len(), files.iter().map(|f| f.bytes.len()).sum::<usize>() / 1024);
    assert_eq!(files.len(), 200);
    let folders: std::collections::BTreeSet<_> = files.iter().map(|f| f.path.parent().unwrap().to_owned()).collect();
    assert_eq!(folders.len(), 3, "{folders:?}");
    assert!(folders.iter().next().unwrap().to_string_lossy().starts_with("2026-06-04 "));
    for (i, f) in files.iter().enumerate() {
        assert_eq!(f.orientation, 1 + (i % 8) as u8);
        assert_eq!(f.path.file_name().unwrap().to_string_lossy(), format!("IMG_{:04}.jpg", i + 1));
        assert_eq!((f.width < f.height), i % 5 == 4);
    }
    assert!(files.windows(2).all(|w| w[0].date_time_original < w[1].date_time_original));
    for o in 1..=8 {
        assert!(files.iter().any(|f| f.orientation == o));
    }
    // Every file decodes, with its orientation, at its stored size.
    for f in files.iter().step_by(7) {
        let (w, h, o, _, _) = decode(&f.bytes);
        assert_eq!(o, f.orientation);
        assert_eq!((w, h), if o >= 5 { (f.height, f.width) } else { (f.width, f.height) });
    }
    assert!(demo_library(0, 0, 1).unwrap().is_empty());
    assert_eq!(demo_library(10, 2, 1).unwrap().len(), 2);
    assert!(demo_library(1, dalica_scenes::demo::MAX_PHOTOS + 1, 1).is_err());
}

/// FNV-1a, 64-bit.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The same scene renders the same pixels and file bytes on every platform and in every build
/// profile (CI runs this on Linux, Windows and macOS, in debug). A change here changes every test
/// photo: only on purpose, with the reason in the commit.
#[test]
fn golden_hashes_hold_everywhere() {
    let got: Vec<(Kind, u64, u64)> = Kind::ALL
        .iter()
        .map(|&k| {
            let s = Scene::new(2026, 96, 64).with_kind(k);
            (k, fnv(&s.render().unwrap().data), fnv(&jpeg(&s, 90, Some(&Exif { orientation: 6, thumbnail: Some(32), ..Exif::default() })).unwrap()))
        })
        .collect();
    assert_eq!(got, GOLDEN);
}

/// (kind, pixels, JPEG bytes), the same in debug, optimised-dev and release builds (2026-10-10).
const GOLDEN: [(Kind, u64, u64); 3] = [
    (Kind::Gradient, 0x878b_31ee_05e3_aebe, 0x58e3_4fbb_68f2_3e39),
    (Kind::Horizon, 0x4441_da02_02ea_7917, 0x9754_ad58_68e1_d463),
    (Kind::Blobs, 0xa995_69c0_77fb_2dc8, 0x685f_6475_5955_c36d),
];
