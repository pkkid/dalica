//! TIFF decoding (`formats.md` §1) against `dalica-scenes` TIFFs at every depth, colour model,
//! compression and byte order, checked by fingerprints (`fingerprints/tiff-*.json`,
//! `architecture.md` §11; `DALICA_BLESS=1` rewrites them) and by an independent reference built
//! from the scene. Layouts `scenes` does not write (JPEG strips, tiles, planar samples, low bit
//! depths, alpha, half and double floats, multi-page files, IFDs at the end of the file) are built
//! here by a small writer of our own, from the TIFF 6.0 specification.
//!
//! Tolerances, per fingerprint value (cell mean or deviation, 0–255): 0.5 against the stored
//! fingerprints (lossless files decode identically everywhere; the colour stage's 16-bit and
//! float paths are the same scalar code on every target), 1.5 against the scene for 16-bit, float
//! and CMYK layouts (8-bit rounding of the stored samples), 2.0 for JPEG strips against the JPEG
//! decoded on its own (two YCbCr conversions).

use std::path::PathBuf;
use std::time::Instant;

use dalica_image::color::{naive_cmyk, transform_options};
use dalica_image::formats::{self, tiff as tiff_format};
use dalica_image::{
    DecodeOptions, Error, Format, Limits, Rgba8, decode, decode_as, decode_thumbnail, decode_with, read_header, read_header_file, resize, sniff,
};
use dalica_scenes::formats::{Colour, Compression, Depth, SampleData, Tiff, quantized, samples, tiff};
use dalica_scenes::icc::{self, Space, Version};
use dalica_scenes::{ByteOrder, Kind, Scene, fingerprint, jpeg};
use proptest::prelude::*;

const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn distance(a: &Rgba8, b: &Rgba8) -> f64 {
    fingerprint::distance(&fingerprint::fingerprint(a.width, a.height, &a.data), &fingerprint::fingerprint(b.width, b.height, &b.data))
}

/// Fails (writing actual/expected PNGs) unless the fingerprints agree within `tol` per value.
fn assert_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let d = distance(actual, expected);
    if d > tol {
        let dir = out_dir();
        std::fs::write(dir.join(format!("{case}-actual.png")), fingerprint::png_rgba(actual.width, actual.height, &actual.data)).unwrap();
        std::fs::write(dir.join(format!("{case}-expected.png")), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
        panic!("{case}: {d:.2} away from its reference, tolerance {tol} (PNGs in {})", dir.display());
    }
}

/// Compares `img` with `fingerprints/<name>.json` (`DALICA_BLESS=1` stores it instead), writing
/// the decoded image as `<name>.png` to look at.
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

fn rgba(img: &dalica_scenes::Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
}

/// Every layout `scenes` writes.
fn layouts() -> Vec<(Colour, Depth)> {
    let mut out = Vec::new();
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            if !(colour == Colour::Palette && depth != Depth::Eight) && !(colour == Colour::Cmyk && depth == Depth::Float) {
                out.push((colour, depth));
            }
        }
    }
    out
}

fn layout_name(colour: Colour, depth: Depth) -> String {
    let c = match colour {
        Colour::Grey => "grey",
        Colour::Rgb => "rgb",
        Colour::Cmyk => "cmyk",
        Colour::Palette => "palette",
    };
    let d = match depth {
        Depth::Eight => "8",
        Depth::Sixteen => "16",
        Depth::Float => "float",
    };
    format!("tiff-{c}-{d}")
}

/// What the scene should look like decoded from a file of this layout, from the scene alone:
/// grey as its 8-bit luma, CMYK through the naive conversion of its 8-bit inks, palette quantised.
fn reference(scene: &Scene, colour: Colour) -> Rgba8 {
    let upright = scene.render().unwrap();
    let (w, h) = (upright.width, upright.height);
    let data = match colour {
        Colour::Rgb => return rgba(&upright),
        Colour::Palette => return rgba(&quantized(&upright)),
        Colour::Grey => match samples(&upright, Colour::Grey, Depth::Eight).unwrap().data {
            SampleData::U8(v) => v.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            _ => unreachable!(),
        },
        Colour::Cmyk => match samples(&upright, Colour::Cmyk, Depth::Eight).unwrap().data {
            SampleData::U8(v) => v
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&ink| {
                    let [r, g, b] = naive_cmyk(ink);
                    [r, g, b, 255]
                })
                .collect(),
            _ => unreachable!(),
        },
    };
    Rgba8 { width: w, height: h, data }
}

fn file(scene: &Scene, opts: Tiff) -> Vec<u8> {
    tiff(scene, &opts).unwrap()
}

#[test]
fn every_depth_colour_model_compression_and_byte_order_matches_its_fingerprint() {
    let scene = Scene::new(31, 96, 64).with_kind(Kind::Blobs);
    for (colour, depth) in layouts() {
        let name = layout_name(colour, depth);
        let mut first: Option<Rgba8> = None;
        for compression in Compression::ALL {
            for byte_order in [ByteOrder::Little, ByteOrder::Big] {
                // 7 rows per strip: several strips and a short last one.
                let bytes = file(&scene, Tiff { colour, depth, compression, byte_order, rows_per_strip: Some(7), ..Tiff::default() });
                assert_eq!(sniff(&bytes), Some(Format::Tiff));
                let img = decode(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("{name} {compression:?} {byte_order:?}: {e:?}"));
                match &first {
                    None => {
                        assert_fingerprint(&name, &img, 0.5);
                        let exact = matches!((colour, depth), (Colour::Rgb | Colour::Palette, Depth::Eight));
                        if exact {
                            assert_eq!(img, reference(&scene, colour), "{name}");
                        }
                        assert_close(&format!("{name}-vs-scene"), &img, &reference(&scene, colour), 1.5);
                        first = Some(img);
                    }
                    Some(f) => assert_eq!(&img, f, "{name} {compression:?} {byte_order:?} differs from uncompressed little-endian"),
                }
            }
        }
    }
}

#[test]
fn orientations_one_to_eight_decode_upright() {
    let scene = Scene::new(12, 90, 60);
    for (depth, compression) in [(Depth::Eight, Compression::None), (Depth::Sixteen, Compression::Lzw)] {
        let upright = decode(&file(&scene, Tiff { depth, compression, ..Tiff::default() }), &Limits::default()).unwrap();
        for o in 1..=8 {
            let bytes = file(
                &scene,
                Tiff {
                    depth,
                    compression,
                    orientation: o,
                    byte_order: if o % 2 == 0 { ByteOrder::Big } else { ByteOrder::Little },
                    ..Tiff::default()
                },
            );
            let h = read_header(&bytes, &Limits::default()).unwrap();
            assert_eq!((h.format, h.orientation.get(), h.display_size()), (Format::Tiff, o, (90, 60)), "orientation {o}");
            assert_eq!((h.width, h.height), if o >= 5 { (60, 90) } else { (90, 60) });
            // The file is its own EXIF structure: IFD0's Orientation is read there too.
            assert_eq!(h.exif.as_ref().and_then(|e| e.orientation), Some(o));
            // Lossless: every orientation shows the same pixels; exactly at 8 bits, and within
            // a level at 16 (the low bytes `scenes` writes follow the stored position).
            let img = decode(&bytes, &Limits::default()).unwrap();
            if depth == Depth::Eight {
                assert_eq!(img, upright, "orientation {o}");
            } else {
                assert_close(&format!("tiff-orientation-{o}-16"), &img, &upright, 1.0);
            }
        }
    }
}

/// The reference conversion: `moxcms` with the options the colour stage uses, 8-bit RGB to RGBA.
fn moxcms_rgb8(profile: &[u8], img: &Rgba8) -> Rgba8 {
    let src = moxcms::ColorProfile::new_from_slice(profile).unwrap();
    let t = src.create_transform_8bit(moxcms::Layout::Rgba, &moxcms::ColorProfile::new_srgb(), moxcms::Layout::Rgba, transform_options(8)).unwrap();
    let mut data = vec![0u8; img.data.len()];
    t.transform(&img.data, &mut data).unwrap();
    Rgba8 { width: img.width, height: img.height, data }
}

#[test]
fn a_tagged_profile_converts() {
    let scene = Scene::new(44, 96, 64);
    let untagged = rgba(&scene.render().unwrap());
    for version in Version::ALL {
        let profile = icc::rgb(Space::AdobeRgb, version);
        let bytes = file(&scene, Tiff { icc: Some(profile.clone()), compression: Compression::Deflate, ..Tiff::default() });
        let on = decode_with(&bytes, &ON, &Limits::default()).unwrap();
        assert_eq!(on, moxcms_rgb8(&profile, &untagged), "Adobe RGB {version:?}");
        assert!(distance(&on, &untagged) > 5.0, "the profile changed nothing");
        assert_eq!(decode_with(&bytes, &OFF, &Limits::default()).unwrap(), untagged);
    }
    // 16-bit samples through the floating-point path, with a v2 ProPhoto profile.
    let pro = icc::rgb(Space::ProPhoto, Version::V2);
    let bytes = file(&scene, Tiff { depth: Depth::Sixteen, icc: Some(pro.clone()), byte_order: ByteOrder::Big, ..Tiff::default() });
    let on = decode_with(&bytes, &ON, &Limits::default()).unwrap();
    assert_fingerprint("tiff-rgb-16-prophoto", &on, 0.5);
    assert_close("tiff-rgb-16-prophoto-vs-8", &on, &moxcms_rgb8(&pro, &untagged), 1.5);
    assert!(distance(&on, &untagged) > 5.0);
    // A grey profile on grey samples; an RGB profile on grey samples is unusable and ignored.
    let grey = file(&scene, Tiff { colour: Colour::Grey, icc: Some(icc::grey(Version::V4)), ..Tiff::default() });
    assert_close("tiff-grey-tagged", &decode(&grey, &Limits::default()).unwrap(), &reference(&scene, Colour::Grey), 1.5);
    let wrong = file(&scene, Tiff { colour: Colour::Grey, icc: Some(icc::rgb(Space::ProPhoto, Version::V4)), ..Tiff::default() });
    assert_eq!(decode(&wrong, &Limits::default()).unwrap(), decode_with(&wrong, &OFF, &Limits::default()).unwrap());
    // A damaged profile, and one over the profile limit, decode as sRGB.
    let damaged = file(&scene, Tiff { icc: Some(profile_cut(&pro)), ..Tiff::default() });
    assert_eq!(decode(&damaged, &Limits::default()).unwrap(), untagged);
    let tagged = file(&scene, Tiff { icc: Some(pro.clone()), ..Tiff::default() });
    let small = Limits { max_icc: pro.len() - 1, ..Limits::default() };
    assert_eq!(decode(&tagged, &small).unwrap(), untagged);
}

fn profile_cut(p: &[u8]) -> Vec<u8> {
    p[..p.len() / 2].to_vec()
}

// ---- Structure helpers (little-endian files only) --------------------------------------------

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// The position of `tag`'s entry in the IFD at `ifd`.
fn entry(b: &[u8], ifd: usize, tag: u16) -> usize {
    let n = u16_at(b, ifd) as usize;
    (0..n).map(|k| ifd + 2 + 12 * k).find(|&e| u16_at(b, e) == tag).unwrap_or_else(|| panic!("no tag {tag}"))
}

fn next_ifd(b: &[u8], ifd: usize) -> usize {
    u32_at(b, ifd + 2 + 12 * u16_at(b, ifd) as usize) as usize
}

fn set_inline(b: &mut [u8], ifd: usize, tag: u16, v: u32) {
    let e = entry(b, ifd, tag);
    b[e + 8..e + 12].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn the_thumbnail_ifd_serves_a_small_target() {
    let scene = Scene::new(7, 480, 320);
    let limits = Limits::default();
    let mut bytes = file(&scene, Tiff { thumbnail: Some(64), orientation: 6, compression: Compression::Lzw, ..Tiff::default() });
    let full = decode(&bytes, &limits).unwrap();
    assert_eq!((full.width, full.height), (480, 320));
    // Wipe the main page's strips: a full decode is now garbage, the thumbnail page is not.
    let ifd1 = next_ifd(&bytes, 8);
    let strips = u32_at(&bytes, u32_at(&bytes, entry(&bytes, 8, 273) + 8) as usize) as usize;
    bytes[strips..ifd1].fill(0);
    let t = tiff_format::thumbnail(&bytes, 48, &ON, &limits).unwrap().expect("a thumbnail page covers 48 px");
    assert_eq!((t.width, t.height), (48, 32));
    // The same two steps as the file: reduced to 64 px (as `scenes` writes the thumbnail page),
    // then fitted to 48.
    let expected = resize::fit(&resize::fit(&full, 64).unwrap(), 48).unwrap();
    assert_close("tiff-thumbnail-ifd", &t, &expected, 4.0);
    assert!(decode(&bytes, &limits).is_err() || distance(&decode(&bytes, &limits).unwrap(), &full) > 20.0);
    // Exactly the thumbnail's size still covers; one more pixel does not.
    assert!(tiff_format::thumbnail(&bytes, 64, &ON, &limits).unwrap().is_some());
    assert_eq!(tiff_format::thumbnail(&bytes, 65, &ON, &limits).unwrap(), None);
    // A file without a thumbnail page has no fast path.
    let plain = file(&scene, Tiff::default());
    assert_eq!(tiff_format::thumbnail(&plain, 32, &ON, &limits).unwrap(), None);
    // The page shown is still the full one, and the thumbnail page is not an EXIF JPEG thumbnail.
    let h = read_header(&bytes, &limits).unwrap();
    assert_eq!((h.width, h.height, h.thumbnail.clone()), (320, 480, None));
    assert_eq!(formats::embedded_thumbnail(Format::Tiff, &bytes, &limits), Ok(None));
}

#[test]
fn multi_page_files_show_the_first_full_resolution_page() {
    let scene = Scene::new(9, 80, 60);
    let base = file(&scene, Tiff { thumbnail: Some(40), ..Tiff::default() });
    let ifd1 = next_ifd(&base, 8);
    // IFD1 marked as a second full page: page one is still IFD0.
    let mut two_pages = base.clone();
    set_inline(&mut two_pages, ifd1, 254, 0);
    let img = decode(&two_pages, &Limits::default()).unwrap();
    assert_eq!(img, decode(&base, &Limits::default()).unwrap());
    // IFD0 marked as a reduced-resolution copy: the first full page is IFD1 (40 × 30).
    let mut thumb_first = base.clone();
    set_inline(&mut thumb_first, 8, 254, 1);
    set_inline(&mut thumb_first, ifd1, 254, 0);
    let h = read_header(&thumb_first, &Limits::default()).unwrap();
    assert_eq!((h.width, h.height), (40, 30));
    assert_eq!(decode(&thumb_first, &Limits::default()).unwrap().width, 40);
    // Read from the start of the file, before the full page arrives: more is needed, not the
    // thumbnail's size.
    assert_eq!(read_header(&thumb_first[..ifd1 + 4], &Limits::default()), Err(Error::Truncated));
    // The `tiff` decoder parses IFD0 on its way to the page, so IFD0's strip list is checked too.
    let mut bad_first = thumb_first.clone();
    let e = entry(&bad_first, 8, 273);
    bad_first[e + 4..e + 8].copy_from_slice(&(tiff_format::MAX_CHUNKS as u32 + 1).to_le_bytes());
    assert_eq!(decode(&bad_first, &Limits::default()), Err(Error::TooMany("TIFF strips or tiles")));
    // Only reduced pages: the first of them is shown.
    let mut all_reduced = base.clone();
    set_inline(&mut all_reduced, 8, 254, 1);
    assert_eq!(read_header(&all_reduced, &Limits::default()).unwrap().width, 80);
}

// ---- A small TIFF writer for layouts `scenes` does not write ----------------------------------

#[derive(Clone)]
enum V {
    S(Vec<u16>),
    L(Vec<u32>),
}

/// The value standing for "the offset of data chunk k" in a LONG entry.
const DATA: u32 = 0xdead_0000;

/// A little-endian TIFF: header, the data chunks, then one IFD (with its long values) at the end.
/// LONG values `DATA + k` become the offset of `chunks[k]`.
fn build(entries: &[(u16, V)], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut b = b"II*\0\0\0\0\0".to_vec();
    let mut offsets = Vec::new();
    for c in chunks {
        offsets.push(b.len() as u32);
        b.extend(c);
        if b.len() % 2 == 1 {
            b.push(0);
        }
    }
    let ifd = b.len();
    b[4..8].copy_from_slice(&(ifd as u32).to_le_bytes());
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(t, _)| *t);
    let mut values_at = ifd + 2 + 12 * sorted.len() + 4;
    let mut table = (sorted.len() as u16).to_le_bytes().to_vec();
    let mut values = Vec::new();
    for (tag, v) in &sorted {
        let (kind, count, bytes): (u16, usize, Vec<u8>) = match v {
            V::S(s) => (3, s.len(), s.iter().flat_map(|x| x.to_le_bytes()).collect()),
            V::L(l) => (
                4,
                l.len(),
                l.iter().map(|&x| if x & 0xffff_0000 == DATA { offsets[(x - DATA) as usize] } else { x }).flat_map(|x| x.to_le_bytes()).collect(),
            ),
        };
        table.extend(tag.to_le_bytes());
        table.extend(kind.to_le_bytes());
        table.extend((count as u32).to_le_bytes());
        if bytes.len() <= 4 {
            let mut inline = bytes.clone();
            inline.resize(4, 0);
            table.extend(inline);
        } else {
            table.extend((values_at as u32).to_le_bytes());
            values_at += bytes.len().next_multiple_of(2);
            values.extend(&bytes);
            if bytes.len() % 2 == 1 {
                values.push(0);
            }
        }
    }
    table.extend(0u32.to_le_bytes());
    b.extend(table);
    b.extend(values);
    b
}

/// The common tags of a strip page: size, bits, samples, photometric, one strip per chunk.
fn strip_page(w: u32, h: u32, bits: u16, spp: u16, photometric: u16, chunks: &[Vec<u8>], rows: u32) -> Vec<(u16, V)> {
    vec![
        (256, V::L(vec![w])),
        (257, V::L(vec![h])),
        (258, V::S(vec![bits; spp as usize])),
        (259, V::S(vec![1])),
        (262, V::S(vec![photometric])),
        (273, V::L((0..chunks.len() as u32).map(|k| DATA + k).collect())),
        (277, V::S(vec![spp])),
        (278, V::L(vec![rows])),
        (279, V::L(chunks.iter().map(|c| c.len() as u32).collect())),
    ]
}

fn with(mut entries: Vec<(u16, V)>, extra: &[(u16, V)]) -> Vec<(u16, V)> {
    for (tag, v) in extra {
        entries.retain(|(t, _)| t != tag);
        entries.push((*tag, v.clone()));
    }
    entries
}

fn pixels(img: &Rgba8) -> Vec<[u8; 4]> {
    img.data.as_chunks::<4>().0.to_vec()
}

#[test]
fn low_bit_depths_palettes_planes_alpha_and_wide_samples() {
    let l = Limits::default();
    // 1-bit WhiteIsZero (a fax-style page): 1 is black.
    let chunk = vec![vec![0b1010_0000, 0b0101_0000]];
    let bytes = build(&strip_page(4, 2, 1, 1, 0, &chunk, 2), &chunk);
    let g = |v: u8| [v, v, v, 255];
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [g(0), g(255), g(0), g(255), g(255), g(0), g(255), g(0)]);
    // 4-bit palette: indices into a 16-entry map (16-bit values).
    let map: Vec<u16> = (0..3).flat_map(|c| (0..16u16).map(move |i| (i * 16 + c * 5) * 257)).collect();
    let chunk = vec![vec![0x0f, 0x3a]];
    let bytes = build(&with(strip_page(4, 1, 4, 1, 3, &chunk, 1), &[(320, V::S(map))]), &chunk);
    let col = |i: u8| [i * 16, i * 16 + 5, i * 16 + 10, 255];
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [col(0), col(15), col(3), col(10)]);
    // Planar RGB, one strip per plane.
    let planes = vec![vec![10, 20, 30], vec![40, 50, 60], vec![70, 80, 90]];
    let bytes = build(&with(strip_page(3, 1, 8, 3, 2, &planes, 1), &[(284, V::S(vec![2]))]), &planes);
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [[10, 40, 70, 255], [20, 50, 80, 255], [30, 60, 90, 255]]);
    // Premultiplied (associated) alpha is divided out; straight alpha is kept as it is.
    let chunk = vec![vec![50, 100, 0, 128, 0, 0, 0, 0]];
    let assoc = build(&with(strip_page(2, 1, 8, 4, 2, &chunk, 1), &[(338, V::S(vec![1]))]), &chunk);
    assert_eq!(pixels(&decode(&assoc, &l).unwrap()), [[100, 199, 0, 128], [0, 0, 0, 0]]);
    let straight = build(&with(strip_page(2, 1, 8, 4, 2, &chunk, 1), &[(338, V::S(vec![2]))]), &chunk);
    assert_eq!(pixels(&decode(&straight, &l).unwrap()), [[50, 100, 0, 128], [0, 0, 0, 0]]);
    // An unspecified extra sample is dropped.
    let unspecified = build(&with(strip_page(2, 1, 8, 4, 2, &chunk, 1), &[(338, V::S(vec![0]))]), &chunk);
    assert_eq!(pixels(&decode(&unspecified, &l).unwrap()), [[50, 100, 0, 255], [0, 0, 0, 255]]);
    // Grey with alpha.
    let chunk = vec![vec![200, 255, 100, 50]];
    let ga = build(&with(strip_page(2, 1, 8, 2, 1, &chunk, 1), &[(338, V::S(vec![2]))]), &chunk);
    assert_eq!(pixels(&decode(&ga, &l).unwrap()), [[200, 200, 200, 255], [100, 100, 100, 50]]);
    // 16-bit (half) floats, 32-bit integers and 64-bit floats, grey.
    let half = vec![[0x00, 0x00, 0x00, 0x38, 0x00, 0x3c].to_vec()]; // 0, 0.5, 1
    let bytes = build(&with(strip_page(3, 1, 16, 1, 1, &half, 1), &[(339, V::S(vec![3]))]), &half);
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [g(0), g(128), g(255)]);
    let wide = vec![[0u32, 0x8000_0000, u32::MAX].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()];
    let bytes = build(&strip_page(3, 1, 32, 1, 1, &wide, 1), &wide);
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [g(0), g(128), g(255)]);
    let double = vec![[0.0f64, 0.25, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()];
    let bytes = build(&with(strip_page(3, 1, 64, 1, 1, &double, 1), &[(339, V::S(vec![3]))]), &double);
    assert_eq!(pixels(&decode(&bytes, &l).unwrap()), [g(0), g(64), g(255)]);
}

#[test]
fn tiled_pages_decode() {
    // 20 × 20 grey in 16 × 16 tiles: four tiles, three of them padded.
    let value = |x: u32, y: u32| ((x * 7 + y * 11) % 251) as u8;
    let mut tiles = Vec::new();
    for ty in 0..2 {
        for tx in 0..2 {
            tiles.push((0..16 * 16).map(|i| value(tx * 16 + i % 16, ty * 16 + i / 16)).collect::<Vec<u8>>());
        }
    }
    let entries = vec![
        (256, V::L(vec![20])),
        (257, V::L(vec![20])),
        (258, V::S(vec![8])),
        (259, V::S(vec![1])),
        (262, V::S(vec![1])),
        (277, V::S(vec![1])),
        (322, V::S(vec![16])),
        (323, V::S(vec![16])),
        (324, V::L((0..4).map(|k| DATA + k).collect())),
        (325, V::L(vec![256; 4])),
    ];
    let img = decode(&build(&entries, &tiles), &Limits::default()).unwrap();
    assert_eq!((img.width, img.height), (20, 20));
    for y in 0..20 {
        for x in 0..20 {
            let v = value(x, y);
            assert_eq!(img.data[((y * 20 + x) * 4) as usize..][..4], [v, v, v, 255], "({x}, {y})");
        }
    }
}

/// A TIFF whose one strip is a whole baseline JPEG (Compression 7, YCbCr), as cameras and
/// scanners write them.
fn jpeg_tiff(scene: &Scene) -> (Vec<u8>, Vec<u8>) {
    let jpg = jpeg(scene, 95, None).unwrap();
    let (w, h) = (scene.width, scene.height);
    let chunk = vec![jpg.clone()];
    let entries = with(strip_page(w, h, 8, 3, 6, &chunk, h), &[(259, V::S(vec![7])), (530, V::S(vec![2, 2]))]);
    (build(&entries, &chunk), jpg)
}

#[test]
fn jpeg_compressed_strips_decode() {
    let scene = Scene::new(55, 96, 64);
    let (bytes, jpg) = jpeg_tiff(&scene);
    let img = decode(&bytes, &Limits::default()).unwrap();
    assert_fingerprint("tiff-jpeg-ycbcr", &img, 0.5);
    assert_close("tiff-jpeg-vs-jpeg", &img, &decode(&jpg, &Limits::default()).unwrap(), 2.0);
    assert_close("tiff-jpeg-vs-scene", &img, &rgba(&scene.render().unwrap()), 3.0);
}

#[test]
fn lab_plain_ycbcr_and_other_layouts_are_unsupported() {
    let scene = Scene::new(3, 24, 16);
    let base = file(&scene, Tiff::default());
    let l = Limits::default();
    let refused = |bytes: &[u8], what: &str| {
        let err = decode(bytes, &l).unwrap_err();
        assert_eq!(err, Error::Unsupported(Format::Tiff), "{what}");
        assert_eq!(err.reason(), "This kind of TIFF isn't supported");
        assert_eq!(decode_as(Format::Tiff, bytes, &ON, &l).unwrap_err(), Error::Unsupported(Format::Tiff), "{what}");
    };
    for (tag, value, what) in [
        (262, 8, "CIE Lab"),
        (262, 9, "ICC Lab"),
        (262, 10, "ITU Lab"),
        (262, 6, "uncompressed YCbCr"),
        (262, 4, "transparency mask"),
        (259, 6, "old-style JPEG"),
        (259, 50_000, "Zstandard"),
        (339, 2, "signed integers"),
    ] {
        let mut b = base.clone();
        set_inline(&mut b, 8, tag, value);
        refused(&b, what);
    }
    // CMYK with another ink set.
    let mut cmyk = file(&scene, Tiff { colour: Colour::Cmyk, ..Tiff::default() });
    set_inline(&mut cmyk, 8, 332, 2);
    refused(&cmyk, "ink set 2");
    // BigTIFF is recognised, and refused as a layout.
    let big = [b"II+\0\x08\0\0\0".as_slice(), &[0u8; 64]].concat();
    assert_eq!(sniff(&big), Some(Format::Tiff));
    assert_eq!(read_header(&big, &l), Err(Error::Unsupported(Format::Tiff)));
    // Mixed bits per sample.
    let chunk = vec![vec![0u8; 8]];
    let mixed = build(&with(strip_page(2, 1, 8, 3, 2, &chunk, 1), &[(258, V::S(vec![8, 16, 8]))]), &chunk);
    refused(&mixed, "mixed bits");
}

/// A TIFF claiming `w × h` with one tiny strip.
fn claiming(w: u32, h: u32, bits: u16, spp: u16) -> Vec<u8> {
    let chunk = vec![vec![0u8; 16]];
    build(&strip_page(w, h, bits, spp, if spp == 1 { 1 } else { 2 }, &chunk, h), &chunk)
}

/// Sizes and counts are refused from the IFD alone. Allocating for them (up to 25 GB) would
/// abort the test process, so passing at all shows nothing was allocated.
#[test]
fn a_header_bomb_is_refused_before_allocating() {
    let start = Instant::now();
    let l = Limits::default();
    for (w, h, bits, spp) in [(65_535, 65_535, 16, 3), (60_000, 60_000, 8, 1), (30_000, 30_000, 8, 4), (1, 2_000_000_000, 8, 1), (100_000, 10, 8, 1)]
    {
        let bytes = claiming(w, h, bits, spp);
        assert!(matches!(decode(&bytes, &l), Err(Error::TooLarge { .. })), "{w}x{h}");
        assert!(matches!(decode_thumbnail(&bytes, 256, &l), Err(Error::TooLarge { .. })), "{w}x{h}");
    }
    // Within the pixel limits but past a small allocation limit.
    let real = file(&Scene::new(2, 200, 100), Tiff { depth: Depth::Sixteen, ..Tiff::default() });
    assert!(decode(&real, &l).is_ok());
    assert!(matches!(decode(&real, &Limits { max_alloc: 200 * 100 * 6, ..l }), Err(Error::TooLarge { .. })));
    // A strip count past the cap, with its arrays nowhere near the file.
    let mut many = claiming(64, 64, 8, 1);
    let ifd = u32_at(&many, 4) as usize;
    for tag in [273, 279] {
        let e = entry(&many, ifd, tag);
        many[e + 4..e + 8].copy_from_slice(&(tiff_format::MAX_CHUNKS as u32 + 1).to_le_bytes());
        many[e + 8..e + 12].copy_from_slice(&0x4000_0000u32.to_le_bytes());
    }
    assert_eq!(decode(&many, &l), Err(Error::TooMany("TIFF strips or tiles")));
    // An IFD claiming 60 000 entries, and one whose chain loops.
    let mut entries = claiming(4, 4, 8, 1);
    let ifd = u32_at(&entries, 4) as usize;
    entries[ifd..ifd + 2].copy_from_slice(&60_000u16.to_le_bytes());
    assert!(matches!(decode(&entries, &l), Err(Error::TooMany(_))));
    let mut looping = file(&Scene::new(2, 20, 10), Tiff { thumbnail: Some(8), ..Tiff::default() });
    let ifd1 = next_ifd(&looping, 8);
    let n1 = u16_at(&looping, ifd1) as usize;
    looping[ifd1 + 2 + 12 * n1..][..4].copy_from_slice(&(ifd1 as u32).to_le_bytes());
    assert!(decode(&looping, &l).is_ok(), "page one does not need the rest of the chain");
    // The thumbnail page sits in the loop: `tiff` refuses to read it, and the caller decodes the page.
    assert!(tiff_format::thumbnail(&looping, 4, &ON, &l).is_err());
    assert!(start.elapsed().as_millis() < 1000, "{:?}", start.elapsed());
}

#[test]
fn a_header_at_the_end_of_a_long_file_is_read_by_reading_more() {
    // 600 KiB of strips before the IFD: the header reader starts with 256 KiB and reads on.
    let (w, h) = (640u32, 320u32);
    let strip: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
    let chunk = vec![strip];
    let bytes = build(&with(strip_page(w, h, 8, 3, 2, &chunk, h), &[(274, V::S(vec![8]))]), &chunk);
    assert!(bytes.len() > 512 * 1024);
    let dir = std::env::temp_dir().join(format!("dalica-image-tiff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scan.tif");
    std::fs::write(&path, &bytes).unwrap();
    let result = read_header_file(&path, &Limits::default());
    std::fs::remove_dir_all(&dir).unwrap();
    let (header, read) = result.unwrap();
    assert_eq!((header.width, header.height, header.orientation.get()), (640, 320, 8));
    assert_eq!(read.len(), bytes.len());
    // The same file cut short: damaged, not a wrong size.
    assert_eq!(read_header(&bytes[..bytes.len() - 20], &Limits::default()).map(|h| h.width), Err(Error::Truncated));
}

/// Small files in every container shape this module reads.
fn small_files() -> Vec<(String, Vec<u8>)> {
    let s = Scene::new(4, 24, 16);
    let mut out = vec![
        (
            "rgb lzw thumb".to_string(),
            file(&s, Tiff { compression: Compression::Lzw, thumbnail: Some(8), rows_per_strip: Some(5), ..Tiff::default() }),
        ),
        (
            "palette packbits".into(),
            file(&s, Tiff { colour: Colour::Palette, compression: Compression::PackBits, rows_per_strip: Some(5), ..Tiff::default() }),
        ),
        (
            "grey16 deflate mm icc".into(),
            file(
                &s,
                Tiff {
                    colour: Colour::Grey,
                    depth: Depth::Sixteen,
                    compression: Compression::Deflate,
                    byte_order: ByteOrder::Big,
                    icc: Some(icc::grey(Version::V2)),
                    ..Tiff::default()
                },
            ),
        ),
        ("cmyk".into(), file(&s, Tiff { colour: Colour::Cmyk, orientation: 5, ..Tiff::default() })),
        ("float".into(), file(&s, Tiff { depth: Depth::Float, ..Tiff::default() })),
    ];
    out.push(("jpeg".into(), jpeg_tiff(&Scene::new(4, 24, 16)).0));
    out
}

/// Every prefix of real files: `Ok` with the right size or `Err`, never a panic, not even one
/// the guard catches.
#[test]
fn truncation_at_every_offset_never_panics() {
    let l = Limits::default();
    for (what, bytes) in small_files() {
        let full = decode(&bytes, &l).unwrap();
        for len in 0..bytes.len() {
            let cut = &bytes[..len];
            for result in [decode(cut, &l), decode_thumbnail(cut, 8, &l), decode_as(Format::Tiff, cut, &OFF, &l)] {
                match result {
                    Ok(img) => assert!(img.width == full.width || img.width <= 8, "{what} prefix {len}"),
                    Err(e) => assert!(!matches!(e, Error::Panic(_)), "{what} prefix {len}: a decoder panicked"),
                }
            }
            if let Ok(h) = read_header(cut, &l) {
                assert_eq!(h.display_size(), (full.width, full.height), "{what} prefix {len}");
            }
            let _ = formats::embedded_thumbnail(Format::Tiff, cut, &l);
            let _ = tiff_format::thumbnail(cut, 8, &ON, &l);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Corruption anywhere in a real file, then truncation: `Ok` or `Err`, never a panic that
    /// escapes (a caught panic in the third-party decoder is an `Err` like any other).
    #[test]
    fn corrupted_files_never_panic(which in 0usize..6, cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let files = small_files();
        let mut bytes = files[which].1.clone();
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1).max(bytes.len() / 2));
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&bytes, &l);
        let _ = decode_with(&bytes, &ON, &l);
        let _ = decode_thumbnail(&bytes, 8, &l);
        let _ = tiff_format::thumbnail(&bytes, 8, &ON, &l);
        let _ = formats::embedded_thumbnail(Format::Tiff, &bytes, &l);
    }

    #[test]
    fn random_bytes_never_panic(head in prop_oneof![Just(b"II*\0\x08\0\0\0".to_vec()), Just(b"MM\0*\0\0\0\x08".to_vec())], tail in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let bytes = [head, tail].concat();
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = read_header(&bytes, &l);
        let _ = decode_as(Format::Tiff, &bytes, &OFF, &l);
        let _ = tiff_format::thumbnail(&bytes, 8, &ON, &l);
    }
}
