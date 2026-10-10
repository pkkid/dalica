//! Decoding against `dalica-scenes` sources, checked by fingerprints (`architecture.md` §11):
//! per-channel mean and standard deviation over an 8 × 8 grid, with a tolerance. On a mismatch the
//! test writes actual and expected PNGs under `target/test-output/image/` to look at.

use std::path::PathBuf;
use std::time::Instant;

use dalica_image::{Error, Limits, Rgba8, decode, decode_thumbnail, read_header};
use dalica_scenes::{ByteOrder, Exif, Scene, jpeg, png};

/// (mean, standard deviation) of R, G and B in each cell of an 8 × 8 grid, and the size.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    size: (u32, u32),
    cells: Vec<[(f64, f64); 3]>,
}

fn fingerprint(img: &Rgba8) -> Fingerprint {
    let mut cells = Vec::with_capacity(64);
    for gy in 0..8u64 {
        for gx in 0..8u64 {
            let (x0, x1) = ((gx * u64::from(img.width) / 8) as u32, ((gx + 1) * u64::from(img.width) / 8) as u32);
            let (y0, y1) = ((gy * u64::from(img.height) / 8) as u32, ((gy + 1) * u64::from(img.height) / 8) as u32);
            let mut stats = [(0.0, 0.0); 3];
            for (c, stat) in stats.iter_mut().enumerate() {
                let values: Vec<f64> = (y0..y1.max(y0 + 1))
                    .flat_map(|y| (x0..x1.max(x0 + 1)).map(move |x| (x, y)))
                    .filter_map(|(x, y)| img.data.get(((y * img.width + x) * 4) as usize + c).map(|&v| f64::from(v)))
                    .collect();
                let n = values.len().max(1) as f64;
                let mean = values.iter().sum::<f64>() / n;
                let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
                *stat = (mean, var.sqrt());
            }
            cells.push(stats);
        }
    }
    Fingerprint { size: (img.width, img.height), cells }
}

/// Fails (writing actual/expected PNGs) unless the fingerprints agree within `tol` per value.
fn assert_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let (a, e) = (fingerprint(actual), fingerprint(expected));
    let worst =
        a.cells.iter().zip(&e.cells).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p.0 - q.0).abs().max((p.1 - q.1).abs()))).fold(0.0, f64::max);
    if a.size != e.size || worst > tol {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, img) in [("actual", actual), ("expected", expected)] {
            let file = std::fs::File::create(dir.join(format!("{case}-{name}.png"))).unwrap();
            let mut enc = ::png::Encoder::new(file, img.width, img.height);
            enc.set_color(::png::ColorType::Rgba);
            enc.write_header().unwrap().write_image_data(&img.data).unwrap();
        }
        panic!("{case}: sizes {:?} vs {:?}, worst cell difference {worst:.2} > {tol} (PNGs in {})", a.size, e.size, dir.display());
    }
}

fn rgba(img: &dalica_scenes::Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
}

fn camera(scene: &Scene, o: u8) -> Vec<u8> {
    let exif = Exif { orientation: o, thumbnail: Some(160), byte_order: if o > 4 { ByteOrder::Big } else { ByteOrder::Little }, ..Exif::default() };
    jpeg(scene, 95, Some(&exif)).unwrap()
}

#[test]
fn each_orientation_decodes_upright() {
    let scene = Scene::new(17, 240, 160);
    let source = rgba(&scene.render().unwrap());
    let reference = decode(&camera(&scene, 1), &Limits::default()).unwrap();
    assert_close("orientation-1-vs-source", &reference, &source, 3.0);
    for o in 2..=8 {
        let img = decode(&camera(&scene, o), &Limits::default()).unwrap();
        assert_eq!((img.width, img.height), (240, 160), "orientation {o}");
        assert_close(&format!("orientation-{o}"), &img, &reference, 3.0);
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
        assert_close(&format!("thumbnail-{o}"), &t, &expected, 3.0);
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
fn truncated_files_decode_or_fail_but_never_panic() {
    for bytes in [camera(&Scene::new(3, 96, 64), 6), png(&Scene::new(3, 96, 64)).unwrap()] {
        for len in (0..bytes.len()).step_by(7) {
            let _ = decode(&bytes[..len], &Limits::default());
            let _ = decode_thumbnail(&bytes[..len], 32, &Limits::default());
        }
    }
}

#[test]
fn a_256_px_thumbnail_of_a_24_mp_scene() {
    let bytes = jpeg(&Scene::new(24, 6000, 4000), 90, None).unwrap();
    let start = Instant::now();
    let t = decode_thumbnail(&bytes, 256, &Limits::default()).unwrap();
    let elapsed = start.elapsed();
    println!("256 px thumbnail of a 6000 x 4000 JPEG ({} KiB): {elapsed:?}", bytes.len() / 1024);
    assert_eq!((t.width, t.height), (256, 171));
    // Generous: catches a pathological slowdown, not ordinary noise.
    assert!(elapsed.as_secs() < 10, "{elapsed:?}");
}
