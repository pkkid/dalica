//! What the P0 format rows (`parity.md` `DATA-FMT-JPEG`, `-PNG`, `-TIFF`) had left unmeasured,
//! measured on procedural files from `dalica-scenes`:
//!
//! - JPEG: 12-bit and arithmetic-coded frames (our own DC-only writer, `formats::jpeg_dc`) answer
//!   "This kind of JPEG isn't supported", not "Damaged file"; thumbnails are full decodes fitted
//!   (`zune-jpeg` 0.5 has no DCT-domain scaling), identical to fitting the full decode.
//! - PNG: every colour type at every bit depth (grey 1–16, palette 1–8, RGB, grey + alpha and
//!   RGBA 8 and 16), with and without `tRNS`, plain and Adam7-interlaced (our own writer,
//!   `formats::png`), decode exactly to the RGBA the file describes.
//! - TIFF: `decode_thumbnail` takes the thumbnail-IFD fast path when a reduced-resolution page
//!   covers the target, and decodes the page otherwise.

use crate::common::rgba;

use dalica_image::formats::{jpeg as jpeg_format, tiff as tiff_format};
use dalica_image::{DecodeOptions, Error, Format, Limits, Rgba8, decode, decode_thumbnail, read_header, resize};
use dalica_scenes::formats::{Coding, Compression, JpegDc, Png, PngColour, Tiff, jpeg_dc, jpeg_dc_reference, png_image, png_layout, tiff};
use dalica_scenes::{Scene, jpeg};

// ---- JPEG --------------------------------------------------------------------------------------

#[test]
fn jpeg_12_bit_and_arithmetic_frames_answer_their_message() {
    let scene = Scene::new(40, 64, 48);
    let l = Limits::default();
    // The control: the writer's 8-bit Huffman frame decodes to its block means.
    let eight = jpeg_dc(&scene, &JpegDc::default()).unwrap();
    let img = decode(&eight, &l).unwrap();
    let reference = rgba(&jpeg_dc_reference(&scene, &JpegDc::default()).unwrap());
    let worst = img.data.iter().zip(&reference.data).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(worst <= Some(2), "8-bit control: {worst:?}");
    for opts in [
        JpegDc { precision: 12, ..JpegDc::default() },
        JpegDc { precision: 12, grey: true, ..JpegDc::default() },
        JpegDc { coding: Coding::Arithmetic, ..JpegDc::default() },
        JpegDc { precision: 12, coding: Coding::Arithmetic, grey: true },
    ] {
        let bytes = jpeg_dc(&scene, &opts).unwrap();
        // Listed with its size like any JPEG; only the pixels are refused, with the reason.
        let h = read_header(&bytes, &l).unwrap();
        assert_eq!((h.format, h.width, h.height), (Format::Jpeg, 64, 48), "{opts:?}");
        for result in [decode(&bytes, &l), decode_thumbnail(&bytes, 32, &l)] {
            let err = result.unwrap_err();
            assert_eq!(err, Error::Unsupported(Format::Jpeg), "{opts:?}");
            assert_eq!(err.reason(), jpeg_format::REASON, "{opts:?}");
        }
    }
}

/// No DCT-domain scaling: a JPEG thumbnail is the full decode fitted, to the byte, at every scale
/// a DCT-scaled decode would have used (1/2, 1/4, 1/8) and between them.
#[test]
fn jpeg_thumbnails_are_the_full_decode_fitted() {
    let bytes = jpeg(&Scene::new(41, 640, 480), 90, None).unwrap();
    let l = Limits::default();
    let full = decode(&bytes, &l).unwrap();
    for target in [320, 160, 80, 75] {
        let t = decode_thumbnail(&bytes, target, &l).unwrap();
        assert_eq!(t, resize::fit(&full, target).unwrap(), "{target} px");
    }
}

// ---- PNG ---------------------------------------------------------------------------------------

#[test]
fn png_every_colour_type_depth_transparency_and_interlacing_decodes_exactly() {
    let l = Limits::default();
    let mut measured = 0;
    // 37 × 23: every Adam7 pass partial; 5 × 3: some passes empty.
    for scene in [Scene::new(42, 37, 23), Scene::new(43, 5, 3)] {
        for colour in PngColour::ALL {
            for &depth in colour.depths() {
                for trns in [false, true].into_iter().filter(|&t| !t || colour.takes_trns()) {
                    for interlaced in [false, true] {
                        let opts = Png { colour, depth, interlaced, trns };
                        let expected = png_image(&scene, &opts).unwrap();
                        let img = decode(&png_layout(&scene, &opts).unwrap(), &l).unwrap();
                        assert_eq!(
                            img,
                            Rgba8 { width: scene.width, height: scene.height, data: expected.rgba8() },
                            "{opts:?} {}×{}",
                            scene.width,
                            scene.height
                        );
                        measured += 1;
                    }
                }
            }
        }
    }
    // 15 colour type and depth pairs, 11 of them also with `tRNS`; plain and interlaced; two sizes.
    assert_eq!(measured, 104);
}

// ---- TIFF --------------------------------------------------------------------------------------

fn u32_le(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

/// The value field of `tag`'s entry in the little-endian IFD at `ifd`.
fn value_at(b: &[u8], ifd: usize, tag: u16) -> usize {
    let n = u16::from_le_bytes([b[ifd], b[ifd + 1]]) as usize;
    let e = (0..n).map(|k| ifd + 2 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == tag).unwrap();
    e + 8
}

#[test]
fn tiff_thumbnails_take_the_thumbnail_page_when_it_covers_the_target() {
    let l = Limits::default();
    let on = DecodeOptions::default();
    let intact = tiff(&Scene::new(44, 480, 320), &Tiff { thumbnail: Some(64), compression: Compression::Lzw, ..Tiff::default() }).unwrap();
    // The main page's strips (between IFD0 and IFD1) wiped: only the thumbnail page is intact.
    let mut bytes = intact.clone();
    let ifd0 = u32_le(&bytes, 4);
    let ifd1 = u32_le(&bytes, ifd0 + 2 + 12 * u16::from_le_bytes([bytes[ifd0], bytes[ifd0 + 1]]) as usize);
    let strips = u32_le(&bytes, u32_le(&bytes, value_at(&bytes, ifd0, 273)));
    bytes[strips..ifd1].fill(0);
    let full = decode(&intact, &l).unwrap();
    let wiped = decode(&bytes, &l).ok();
    assert_ne!(wiped.as_ref(), Some(&full), "the wiped page no longer decodes to the picture");
    for target in [64, 48, 20] {
        // The thumbnail page (the only intact picture in the file), not the wiped page fitted.
        let t = decode_thumbnail(&bytes, target, &l).unwrap();
        assert_eq!(Some(&t), tiff_format::thumbnail(&bytes, target, &on, &l).unwrap().as_ref(), "{target} px");
        assert_ne!(Some(&t), wiped.as_ref().map(|w| resize::fit(w, target).unwrap()).as_ref(), "{target} px");
        assert_eq!(t, decode_thumbnail(&intact, target, &l).unwrap(), "{target} px");
    }
    // Past the thumbnail page's size the page itself is decoded and fitted.
    assert_eq!(decode_thumbnail(&intact, 65, &l).unwrap(), resize::fit(&full, 65).unwrap());
    assert_ne!(decode_thumbnail(&bytes, 65, &l).ok(), Some(resize::fit(&full, 65).unwrap()));
}
