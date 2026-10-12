//! M5 part 3, *Formats and colour*: every format of `formats.md` §1 decoded from files `scenes`
//! writes, compared with the scene they were made from, at each depth and colour model the
//! container stores and with each colour profile; orientation 1–8 in every container that carries
//! one; and malformed input (every truncation of a file's head, cuts through the rest, and seeded
//! byte damage) through every entry point of `dalica-image`, never panicking.
//!
//! The crate suites check each decoder against its own fingerprints; this checks them all against
//! one independent truth, the scene's pixels, through the public entry points scanning and the
//! thumbnail ladder use (`sniff`, `read_header`, `decode_with`, `decode_thumbnail_with`,
//! `formats::embedded_thumbnail`). Files are built in memory; nothing is written to disk.

use std::panic::{AssertUnwindSafe, catch_unwind};

use dalica_image::{DecodeOptions, Format, Limits, RawKind, Rgba8, decode_thumbnail_with, decode_with, formats, read_header, sniff};
use dalica_scenes::formats::{self as sf, Colour, Compression, Depth, Psd, SampleData, Tiff, Webp};
use dalica_scenes::icc::{self, Space, Version};
use dalica_scenes::{ByteOrder, Exif, Rgb8, Scene};

pub const ON: DecodeOptions = DecodeOptions { color_managed: true };
pub const OFF: DecodeOptions = DecodeOptions { color_managed: false };

/// The size of the test scenes: not square, so a swapped orientation shows.
const W: u32 = 64;
const H: u32 = 40;

// ---------------------------------------------------------------------------------------------
// Files of every format, built in memory
// ---------------------------------------------------------------------------------------------

pub fn scene(seed: u64) -> Scene {
    Scene::new(seed, W, H)
}

/// A PNG of `samples` (8- or 16-bit grey or RGB), with an optional `eXIf` chunk: `scenes` writes
/// 8-bit RGB only, so 16-bit and EXIF-carrying PNGs are encoded here with the `png` crate.
pub fn png_of(s: &sf::Samples, exif: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, s.width, s.height);
        enc.set_color(match s.colour {
            Colour::Grey => png::ColorType::Grayscale,
            _ => png::ColorType::Rgb,
        });
        let bytes: Vec<u8> = match &s.data {
            SampleData::U8(v) => {
                enc.set_depth(png::BitDepth::Eight);
                v.clone()
            }
            SampleData::U16(v) => {
                enc.set_depth(png::BitDepth::Sixteen);
                v.iter().flat_map(|x| x.to_be_bytes()).collect()
            }
            SampleData::F32(_) => panic!("PNG has no float samples"),
        };
        let mut w = enc.write_header().unwrap();
        if let Some(block) = exif {
            w.write_chunk(png::chunk::ChunkType(*b"eXIf"), block).unwrap();
        }
        w.write_image_data(&bytes).unwrap();
    }
    out
}

/// A Fujifilm RAF container around `jpeg` (`formats.md` §2): the 16-byte signature, a version,
/// and at byte 84 the big-endian (offset, length) of the embedded JPEG.
pub fn raf(jpeg: &[u8]) -> Vec<u8> {
    const AT: usize = 160;
    let mut out = vec![0u8; AT];
    out[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    out[16..20].copy_from_slice(b"0201");
    out[28..36].copy_from_slice(b"ACCEPT05");
    out[84..88].copy_from_slice(&(AT as u32).to_be_bytes());
    out[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
    out.extend_from_slice(jpeg);
    out
}

/// An ISOBMFF file of an `ftyp` box with `brand` as its major and compatible brand, then an
/// empty `meta` box: what a HEIC or AVIF file starts with, enough to be recognised.
pub fn bmff(brand: &[u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(24u32.to_be_bytes());
    out.extend(b"ftyp");
    out.extend(brand);
    out.extend(0u32.to_be_bytes());
    out.extend(b"mif1");
    out.extend(brand);
    out.extend(12u32.to_be_bytes());
    out.extend(b"meta");
    out.extend(0u32.to_be_bytes());
    out
}

/// A JPEG cut right after its APP segments: the SOI and EXIF are there, the frame header and
/// the picture are not. Listed, and shown as "Damaged file".
pub fn broken_jpeg(seed: u64) -> Vec<u8> {
    let full = dalica_scenes::jpeg(&scene(seed), 90, Some(&Exif::default())).unwrap();
    // Walk the markers: SOI, then segments; stop before the first that is not APPn.
    let mut at = 2;
    while let (Some(&0xff), Some(&m)) = (full.get(at), full.get(at + 1)) {
        if !(0xe0..=0xef).contains(&m) {
            break;
        }
        let len = u16::from_be_bytes([full[at + 2], full[at + 3]]) as usize;
        at += 2 + len;
    }
    full[..at].to_vec()
}

/// One file to decode and what it should show.
pub struct Case {
    pub name: String,
    pub format: Format,
    pub bytes: Vec<u8>,
    /// The upright pixels it should decode to.
    pub want: Rgb8,
    /// Largest mean and largest single difference per channel (levels of 255) from `want`.
    pub mean: f64,
    pub max: u32,
}

/// `img` as grey: Rec. 709 luma, as `scenes` stores grey samples.
fn grey_of(img: &Rgb8) -> Rgb8 {
    let SampleData::U8(g) = sf::samples(img, Colour::Grey, Depth::Eight).unwrap().data else { unreachable!() };
    Rgb8 { width: img.width, height: img.height, data: g.iter().flat_map(|&v| [v, v, v]).collect() }
}

/// What a file of `colour` made from `img` should look like.
fn want_for(img: &Rgb8, colour: Colour) -> Rgb8 {
    match colour {
        Colour::Rgb | Colour::Cmyk => img.clone(),
        Colour::Grey => grey_of(img),
        Colour::Palette => sf::quantized(img),
    }
}

fn depth_name(d: Depth) -> &'static str {
    match d {
        Depth::Eight => "8",
        Depth::Sixteen => "16",
        Depth::Float => "float",
    }
}

fn colour_name(c: Colour) -> &'static str {
    match c {
        Colour::Grey => "grey",
        Colour::Rgb => "rgb",
        Colour::Cmyk => "cmyk",
        Colour::Palette => "palette",
    }
}

/// The tolerance for a lossless file of `colour` and `depth`: 16-bit and float samples round to
/// 8 bits; naive CMYK rounds twice; everything else is exact.
fn lossless(colour: Colour, depth: Depth) -> (f64, u32) {
    match (colour, depth) {
        (Colour::Cmyk, _) => (1.0, 3),
        (_, Depth::Eight) => (0.0, 0),
        _ => (0.6, 1),
    }
}

/// Every format at every depth and colour model its container stores (`formats.md` §1).
pub fn depth_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut seed = 500;
    let mut next = || {
        seed += 1;
        scene(seed)
    };
    let case = |name: String, format: Format, bytes: Vec<u8>, want: Rgb8, (mean, max): (f64, u32)| Case { name, format, bytes, want, mean, max };

    // JPEG: baseline, an EXIF thumbnail, both byte orders (lossy).
    for (order, tag) in [(ByteOrder::Little, "II"), (ByteOrder::Big, "MM")] {
        let s = next();
        let exif = Exif { thumbnail: Some(32), byte_order: order, ..Exif::default() };
        cases.push(case(
            format!("jpeg q92 exif {tag}"),
            Format::Jpeg,
            dalica_scenes::jpeg(&s, 92, Some(&exif)).unwrap(),
            s.render().unwrap(),
            (3.0, 48),
        ));
    }
    // PNG: 8-bit RGB from `scenes`; 8/16-bit grey and 16-bit RGB built here.
    let s = next();
    cases.push(case("png rgb 8".into(), Format::Png, dalica_scenes::png(&s).unwrap(), s.render().unwrap(), (0.0, 0)));
    for (colour, depth) in [(Colour::Grey, Depth::Eight), (Colour::Grey, Depth::Sixteen), (Colour::Rgb, Depth::Sixteen)] {
        let s = next();
        let img = s.render().unwrap();
        let bytes = png_of(&sf::samples(&img, colour, depth).unwrap(), None);
        cases.push(case(
            format!("png {} {}", colour_name(colour), depth_name(depth)),
            Format::Png,
            bytes,
            want_for(&img, colour),
            lossless(colour, depth),
        ));
    }
    // TIFF: every colour model × depth × compression, both byte orders.
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            for compression in [Compression::None, Compression::Lzw, Compression::Deflate, Compression::PackBits] {
                let s = next();
                let byte_order = if s.seed % 2 == 0 { ByteOrder::Little } else { ByteOrder::Big };
                let Ok(bytes) = sf::tiff(&s, &Tiff { colour, depth, compression, byte_order, rows_per_strip: Some(7), ..Tiff::default() }) else {
                    continue; // a layout TIFF cannot store (palette deeper than 8 bits, float CMYK)
                };
                let img = s.render().unwrap();
                let name = format!("tiff {} {} {compression:?}", colour_name(colour), depth_name(depth));
                cases.push(case(name, Format::Tiff, bytes, want_for(&img, colour), lossless(colour, depth)));
            }
        }
    }
    // WebP: lossless, with and without alpha.
    for alpha in [false, true] {
        let s = next();
        let bytes = sf::webp(&s, &Webp { alpha, ..Webp::default() }).unwrap();
        cases.push(case(format!("webp lossless alpha={alpha}"), Format::WebP, bytes, s.render().unwrap(), (0.0, 0)));
    }
    // GIF: one frame, and the first of three.
    for frames in [1, 3] {
        let s = next();
        let bytes = sf::gif(&s, frames).unwrap();
        cases.push(case(format!("gif {frames} frame(s)"), Format::Gif, bytes, sf::quantized(&s.render().unwrap()), (0.0, 0)));
    }
    // BMP: RGB, grey, indexed.
    for colour in [Colour::Rgb, Colour::Grey, Colour::Palette] {
        let s = next();
        let img = s.render().unwrap();
        cases.push(case(format!("bmp {}", colour_name(colour)), Format::Bmp, sf::bmp(&s, colour).unwrap(), want_for(&img, colour), (0.0, 0)));
    }
    // JPEG XL: grey and RGB at 8 and 16 bits; RGB with alpha.
    for (colour, depth, alpha) in [
        (Colour::Grey, Depth::Eight, false),
        (Colour::Grey, Depth::Sixteen, false),
        (Colour::Rgb, Depth::Eight, false),
        (Colour::Rgb, Depth::Sixteen, false),
        (Colour::Rgb, Depth::Eight, true),
    ] {
        let s = next();
        let img = s.render().unwrap();
        let bytes = sf::jxl(&s, colour, depth, alpha).unwrap();
        let name = format!("jxl {} {} alpha={alpha}", colour_name(colour), depth_name(depth));
        cases.push(case(name, Format::JpegXl, bytes, want_for(&img, colour), lossless(colour, depth)));
    }
    // PSD: every colour model × depth, raw and RLE, one with a thumbnail resource.
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            for rle in [false, true] {
                let s = next();
                let thumbnail = (colour == Colour::Rgb && rle).then_some(24);
                let Ok(bytes) = sf::psd(&s, &Psd { colour, depth, rle, thumbnail, icc: None }) else {
                    continue; // a layout PSD cannot store
                };
                let img = s.render().unwrap();
                let name = format!("psd {} {} rle={rle}", colour_name(colour), depth_name(depth));
                cases.push(case(name, Format::Psd, bytes, want_for(&img, colour), lossless(colour, depth)));
            }
        }
    }
    // RAW: a RAF around a camera-style JPEG (the preview is what is shown).
    let s = next();
    let jpeg = dalica_scenes::jpeg(&s, 92, Some(&Exif { thumbnail: Some(32), ..Exif::default() })).unwrap();
    cases.push(case("raf preview".into(), Format::Raw(RawKind::Raf), raf(&jpeg), s.render().unwrap(), (3.0, 48)));
    cases
}

// ---------------------------------------------------------------------------------------------
// Comparing a decode with its scene
// ---------------------------------------------------------------------------------------------

/// Mean and largest difference per RGB channel between `got` and `want` (alpha ignored).
pub fn diff(got: &Rgba8, want: &Rgb8) -> (f64, u32) {
    assert_eq!((got.width, got.height), (want.width, want.height), "size");
    let (mut sum, mut max, mut n) = (0u64, 0u32, 0u64);
    for (g, w) in got.data.as_chunks::<4>().0.iter().zip(want.data.as_chunks::<3>().0) {
        for k in 0..3 {
            let d = u32::from(g[k].abs_diff(w[k]));
            sum += u64::from(d);
            max = max.max(d);
            n += 1;
        }
    }
    (sum as f64 / n.max(1) as f64, max)
}

/// Mean difference per RGB channel between two decodes of the same size.
fn mean_diff(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (sum, n) = a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0)
        .fold((0u64, 0u64), |(s, n), (x, y)| (s + (0..3).map(|k| u64::from(x[k].abs_diff(y[k]))).sum::<u64>(), n + 3));
    sum as f64 / n.max(1) as f64
}

/// Mean chroma (largest minus smallest channel) of a decode.
fn chroma(img: &Rgba8) -> f64 {
    let (sum, n) = img.data.as_chunks::<4>().0.iter().fold((0u64, 0u64), |(s, n), p| {
        let (hi, lo) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        (s + u64::from(hi - lo), n + 1)
    });
    sum as f64 / n.max(1) as f64
}

/// Decodes every case and compares it with its scene; returns one line per case.
pub fn check_depths() -> Vec<String> {
    let limits = Limits::default();
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for c in depth_cases() {
        assert_eq!(sniff(&c.bytes), Some(c.format), "{}: sniffed", c.name);
        let header = read_header(&c.bytes, &limits).unwrap_or_else(|e| panic!("{}: header: {e}", c.name));
        assert_eq!((header.format, header.display_size()), (c.format, (W, H)), "{}: header", c.name);
        match decode_with(&c.bytes, &ON, &limits) {
            Ok(img) => {
                let (mean, max) = diff(&img, &c.want);
                let ok = mean <= c.mean && max <= c.max;
                lines.push(format!("{:<32} mean {mean:.2} (≤ {}), max {max} (≤ {})", c.name, c.mean, c.max));
                if !ok {
                    failures.push(format!("{}: mean {mean:.2} (≤ {}), max {max} (≤ {})", c.name, c.mean, c.max));
                }
            }
            Err(e) => failures.push(format!("{}: {e} ({})", c.name, e.reason())),
        }
        // The thumbnail path of the ladder: a 32-pixel long side, the same picture.
        let t = decode_thumbnail_with(&c.bytes, 32, &ON, &limits).unwrap_or_else(|e| panic!("{}: thumbnail: {e}", c.name));
        assert_eq!(t.width.max(t.height), 32, "{}: thumbnail size", c.name);
    }
    assert!(failures.is_empty(), "decodes that do not match their scene:\n  {}", failures.join("\n  "));
    lines
}

/// The colour profiles, in every container that embeds one (JPEG APP2, TIFF tag, WebP ICCP, PSD
/// resource): an sRGB profile (v2 and v4) decodes like no profile; a wide-gamut one (Display P3,
/// Adobe RGB, ProPhoto, Rec. 2020) converts, making the same stored values more saturated in
/// sRGB, and with colour management off it is ignored (the same as untagged).
pub fn check_profiles() -> Vec<String> {
    let limits = Limits::default();
    let s = scene(77);
    type Writer = fn(&Scene, Option<Vec<u8>>) -> Vec<u8>;
    let containers: [(&str, Writer); 4] = [
        ("jpeg", |s, icc| match icc {
            Some(p) => sf::jpeg_icc(s, 95, None, &p).unwrap(),
            None => dalica_scenes::jpeg(s, 95, None).unwrap(),
        }),
        ("tiff", |s, icc| sf::tiff(s, &Tiff { icc, ..Tiff::default() }).unwrap()),
        ("webp", |s, icc| sf::webp(s, &Webp { icc, ..Webp::default() }).unwrap()),
        ("psd", |s, icc| sf::psd(s, &Psd { icc, ..Psd::default() }).unwrap()),
    ];
    let mut lines = Vec::new();
    for (name, write) in containers {
        let plain = decode_with(&write(&s, None), &ON, &limits).unwrap();
        for version in [Version::V2, Version::V4] {
            let tagged = decode_with(&write(&s, Some(icc::rgb(Space::Srgb, version))), &ON, &limits).unwrap();
            let d = mean_diff(&tagged, &plain);
            assert!(d <= 1.0, "{name}: sRGB {version:?} profile changes the picture by {d:.2}");
            lines.push(format!("{name:<5} sRGB {version:?}: {d:.2} from untagged"));
        }
        for (space, version) in
            [(Space::DisplayP3, Version::V4), (Space::AdobeRgb, Version::V2), (Space::ProPhoto, Version::V4), (Space::Rec2020, Version::V4)]
        {
            let bytes = write(&s, Some(icc::rgb(space, version)));
            let on = decode_with(&bytes, &ON, &limits).unwrap();
            let off = decode_with(&bytes, &OFF, &limits).unwrap();
            assert_eq!(off, decode_with(&write(&s, None), &OFF, &limits).unwrap(), "{name} {space:?}: colour management off ignores the profile");
            let d = mean_diff(&on, &off);
            let (c_on, c_off) = (chroma(&on), chroma(&off));
            assert!(d >= 1.0, "{name} {space:?}: the profile changed the picture by only {d:.2}");
            assert!(c_on > c_off, "{name} {space:?}: a wider gamut than sRGB gives more saturated sRGB ({c_on:.1} against {c_off:.1})");
            lines.push(format!("{name:<5} {space:?} {version:?}: {d:.2} from unmanaged, chroma {c_off:.1} → {c_on:.1}"));
        }
    }
    // A grey profile on a grey TIFF decodes like none.
    let grey = |icc| sf::tiff(&s, &Tiff { colour: Colour::Grey, icc, ..Tiff::default() }).unwrap();
    let d = mean_diff(&decode_with(&grey(Some(icc::grey(Version::V4))), &ON, &limits).unwrap(), &decode_with(&grey(None), &ON, &limits).unwrap());
    assert!(d <= 1.0, "a grey profile changes a grey TIFF by {d:.2}");
    lines.push(format!("tiff  grey v4: {d:.2} from untagged"));
    lines
}

// ---------------------------------------------------------------------------------------------
// Orientation 1–8 per container
// ---------------------------------------------------------------------------------------------

/// A container's name, its format, the file, and the tolerance (mean, max) of its decode.
type Oriented = (&'static str, Format, Vec<u8>, (f64, u32));

/// Orientation `o` in every container that carries one: the stored pixels are the scene turned
/// for `o`, and the decode must be the scene upright again. JPEG (EXIF APP1), PNG (`eXIf`), TIFF
/// (the Orientation tag), WebP (its EXIF chunk) and a RAW container (RAF, through its preview's
/// EXIF). JPEG XL applies its own orientation in the decoder (`jxl-oxide`), PSD has none, and
/// HEIF's `irot`/`imir` are tested with the `heif` feature in `dalica-image`.
pub fn check_orientations() -> Vec<String> {
    let limits = Limits::default();
    let mut lines = Vec::new();
    for o in 1..=8u8 {
        let s = scene(900 + u64::from(o));
        let upright = s.render().unwrap();
        let exif = Exif { orientation: o, thumbnail: Some(24), ..Exif::default() };
        let stored = dalica_scenes::orient::store(&upright, o).unwrap();
        let block = dalica_scenes::exif::tiff(&Exif { thumbnail: None, ..exif.clone() }, stored.width, stored.height, None).unwrap();
        let jpeg = dalica_scenes::jpeg(&s, 95, Some(&exif)).unwrap();
        let files: Vec<Oriented> = vec![
            ("jpeg", Format::Jpeg, jpeg.clone(), (3.0, 48)),
            ("png", Format::Png, png_of(&sf::samples(&stored, Colour::Rgb, Depth::Eight).unwrap(), Some(&block)), (0.0, 0)),
            ("tiff", Format::Tiff, sf::tiff(&s, &Tiff { orientation: o, ..Tiff::default() }).unwrap(), (0.0, 0)),
            ("webp", Format::WebP, sf::webp(&s, &Webp { exif: Some(exif.clone()), ..Webp::default() }).unwrap(), (0.0, 0)),
            ("raf", Format::Raw(RawKind::Raf), raf(&jpeg), (3.0, 48)),
        ];
        let mut row = format!("orientation {o}:");
        for (name, format, bytes, (mean_max, max_max)) in files {
            let h = read_header(&bytes, &limits).unwrap_or_else(|e| panic!("{name} {o}: {e}"));
            assert_eq!(h.format, format, "{name} {o}");
            assert_eq!(h.orientation.get(), o, "{name} {o}: the header's orientation");
            assert_eq!(h.display_size(), (W, H), "{name} {o}: the displayed size");
            let img = decode_with(&bytes, &ON, &limits).unwrap_or_else(|e| panic!("{name} {o}: {e}"));
            let (mean, max) = diff(&img, &upright);
            assert!(mean <= mean_max && max <= max_max, "{name} orientation {o}: not upright (mean {mean:.2}, max {max})");
            let t = decode_thumbnail_with(&bytes, 32, &ON, &limits).unwrap();
            assert!(t.width > t.height, "{name} {o}: the thumbnail is upright too ({} × {})", t.width, t.height);
            row.push_str(&format!(" {name} {mean:.2}"));
        }
        lines.push(row);
    }
    lines
}

// ---------------------------------------------------------------------------------------------
// Malformed input
// ---------------------------------------------------------------------------------------------

/// SplitMix64: a seeded generator, the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One file of each kind for the damage tests.
pub fn damage_seeds() -> Vec<(String, Vec<u8>)> {
    let s = scene(1234);
    let img = s.render().unwrap();
    let exif = Exif { orientation: 6, thumbnail: Some(24), ..Exif::default() };
    let jpeg = dalica_scenes::jpeg(&s, 85, Some(&exif)).unwrap();
    vec![
        ("jpeg exif thumbnail".into(), jpeg.clone()),
        ("jpeg icc".into(), sf::jpeg_icc(&s, 85, None, &icc::rgb(Space::DisplayP3, Version::V4)).unwrap()),
        ("png".into(), dalica_scenes::png(&s).unwrap()),
        (
            "png 16 exif".into(),
            png_of(&sf::samples(&img, Colour::Rgb, Depth::Sixteen).unwrap(), Some(&dalica_scenes::exif::tiff(&exif, W, H, None).unwrap())),
        ),
        (
            "tiff lzw thumbnail icc".into(),
            sf::tiff(
                &s,
                &Tiff { compression: Compression::Lzw, thumbnail: Some(16), icc: Some(icc::rgb(Space::AdobeRgb, Version::V2)), ..Tiff::default() },
            )
            .unwrap(),
        ),
        (
            "tiff cmyk 16 deflate".into(),
            sf::tiff(&s, &Tiff { colour: Colour::Cmyk, depth: Depth::Sixteen, compression: Compression::Deflate, ..Tiff::default() }).unwrap(),
        ),
        (
            "webp alpha exif icc".into(),
            sf::webp(&s, &Webp { alpha: true, exif: Some(exif.clone()), icc: Some(icc::rgb(Space::Srgb, Version::V2)) }).unwrap(),
        ),
        ("gif 3 frames".into(), sf::gif(&s, 3).unwrap()),
        ("bmp palette".into(), sf::bmp(&s, Colour::Palette).unwrap()),
        ("jxl rgb 16".into(), sf::jxl(&s, Colour::Rgb, Depth::Sixteen, false).unwrap()),
        (
            "psd rle thumbnail icc".into(),
            sf::psd(&s, &Psd { rle: true, thumbnail: Some(16), icc: Some(icc::rgb(Space::ProPhoto, Version::V4)), ..Psd::default() }).unwrap(),
        ),
        ("raf".into(), raf(&jpeg)),
        ("heic".into(), bmff(b"heic")),
        ("avif".into(), bmff(b"avif")),
    ]
}

/// Every entry point on `bytes`; `Err` if one of them panicked.
fn every_entry_point(bytes: &[u8], limits: &Limits) -> Result<(), String> {
    let run = |what: &str, f: &mut dyn FnMut()| catch_unwind(AssertUnwindSafe(f)).map_err(|_| what.to_string());
    run("sniff", &mut || {
        let _ = sniff(bytes);
    })?;
    run("read_header", &mut || {
        if let Ok(h) = read_header(bytes, limits) {
            let _ = h.thumbnail(bytes);
        }
    })?;
    run("decode", &mut || {
        let _ = decode_with(bytes, &ON, limits);
    })?;
    run("decode_thumbnail", &mut || {
        let _ = decode_thumbnail_with(bytes, 48, &OFF, limits);
    })?;
    run("embedded_thumbnail", &mut || {
        if let Some(f) = sniff(bytes) {
            let _ = formats::embedded_thumbnail(f, bytes, limits);
        }
    })?;
    Ok(())
}

/// What the damage pass did.
pub struct Damage {
    pub files: usize,
    pub inputs: usize,
    pub decoded: usize,
    pub refused: std::collections::BTreeMap<String, usize>,
}

/// Every truncation of the first 1 024 bytes and 64 more cuts through the rest, then 96 seeded
/// damages (bytes flipped, runs zeroed or set to 0xff, a length-sized word made huge, a chunk
/// repeated) of each file, through every entry point: none may panic, and the refusals are
/// counted by their tile text.
pub fn check_damage() -> Damage {
    let limits = Limits::default();
    let mut rng = Rng(0x05_ac_ce_97);
    let mut out = Damage { files: 0, inputs: 0, decoded: 0, refused: Default::default() };
    let mut panics = Vec::new();
    for (name, file) in damage_seeds() {
        out.files += 1;
        let mut inputs: Vec<Vec<u8>> = Vec::new();
        let head = file.len().min(1024);
        inputs.extend((0..head).map(|n| file[..n].to_vec()));
        inputs.extend((1..=64).map(|k| file[..head + (file.len() - head) * k / 65].to_vec()));
        for _ in 0..96 {
            let mut b = file.clone();
            match rng.below(5) {
                0 => {
                    for _ in 0..1 + rng.below(8) {
                        let i = rng.below(b.len());
                        b[i] ^= 1 << rng.below(8);
                    }
                }
                1 => {
                    let at = rng.below(b.len());
                    let end = (at + 1 + rng.below(64)).min(b.len());
                    b[at..end].fill(0);
                }
                2 => {
                    let at = rng.below(b.len());
                    let end = (at + 1 + rng.below(16)).min(b.len());
                    b[at..end].fill(0xff);
                }
                3 => {
                    let at = rng.below(b.len().saturating_sub(4));
                    b[at..at + 4].copy_from_slice(&[0x7f, 0xff, 0xff, (rng.next() & 0xff) as u8]);
                }
                _ => {
                    let at = rng.below(b.len());
                    let end = (at + 1 + rng.below(256)).min(b.len());
                    let chunk = b[at..end].to_vec();
                    b.splice(at..at, chunk);
                }
            }
            inputs.push(b);
        }
        for input in &inputs {
            out.inputs += 1;
            if let Err(what) = every_entry_point(input, &limits) {
                panics.push(format!("{name}, {} bytes: {what} panicked", input.len()));
                continue;
            }
            match decode_with(input, &ON, &limits) {
                Ok(_) => out.decoded += 1,
                Err(e) => *out.refused.entry(e.reason()).or_default() += 1,
            }
        }
    }
    assert!(panics.is_empty(), "{} inputs panicked:\n  {}", panics.len(), panics.join("\n  "));
    out
}

// ---------------------------------------------------------------------------------------------
// The mixed-format folder of the live app's snapshot
// ---------------------------------------------------------------------------------------------

/// A file of the mixed-format folder: its name, bytes, and the reason its tile gives when it
/// cannot be shown (`None`: it shows its picture).
pub struct Mixed {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub reason: Option<&'static str>,
}

/// One file of every format of `formats.md` §1, in the order the light table shows them (each
/// gets a file time a minute after the last): JPEG (with an EXIF thumbnail, and one turned by its
/// orientation), 16-bit PNG, TIFF (8-bit LZW with a thumbnail page; 16-bit CMYK Deflate), WebP
/// with alpha, an animated GIF, an indexed BMP, 16-bit JPEG XL, a 16-bit RLE PSD with its
/// thumbnail, a RAW (RAF), a Display P3 JPEG, then the three that show a "!" with their reason:
/// a HEIC (no `heif` feature in this build), an AVIF (never decoded) and a damaged JPEG.
pub fn mixed_folder() -> Vec<Mixed> {
    let s = |seed: u64| Scene::new(seed, 480, 320);
    let exif = |orientation: u8| Exif { orientation, thumbnail: Some(160), ..Exif::default() };
    let heif = if formats::decodes(Format::Heif) { "Damaged file" } else { formats::heif::REASON };
    let shown = |name, bytes| Mixed { name, bytes, reason: None };
    vec![
        shown("Harbour.jpg", dalica_scenes::jpeg(&s(11), 90, Some(&exif(1))).unwrap()),
        shown("Tower.jpg", dalica_scenes::jpeg(&Scene::new(12, 320, 480), 90, Some(&exif(6))).unwrap()),
        shown("Garden.png", png_of(&sf::samples(&s(13).render().unwrap(), Colour::Rgb, Depth::Sixteen).unwrap(), None)),
        shown("Bridge.tif", sf::tiff(&s(14), &Tiff { compression: Compression::Lzw, thumbnail: Some(160), ..Tiff::default() }).unwrap()),
        shown(
            "Print.tif",
            sf::tiff(&s(15), &Tiff { colour: Colour::Cmyk, depth: Depth::Sixteen, compression: Compression::Deflate, ..Tiff::default() }).unwrap(),
        ),
        shown("Sky.webp", sf::webp(&s(16), &Webp { alpha: true, ..Webp::default() }).unwrap()),
        shown("Loop.gif", sf::gif(&s(17), 3).unwrap()),
        shown("Paint.bmp", sf::bmp(&s(18), Colour::Palette).unwrap()),
        shown("Dunes.jxl", sf::jxl(&s(19), Colour::Rgb, Depth::Sixteen, false).unwrap()),
        shown("Layers.psd", sf::psd(&s(20), &Psd { depth: Depth::Sixteen, rle: true, thumbnail: Some(160), ..Psd::default() }).unwrap()),
        shown("Camera.raf", raf(&dalica_scenes::jpeg(&s(21), 90, Some(&exif(1))).unwrap())),
        shown("Wide.jpg", sf::jpeg_icc(&s(22), 90, None, &icc::rgb(Space::DisplayP3, Version::V4)).unwrap()),
        Mixed { name: "Phone.heic", bytes: bmff(b"heic"), reason: Some(heif) },
        Mixed { name: "Drone.avif", bytes: bmff(b"avif"), reason: Some(formats::AVIF_REASON) },
        Mixed { name: "Broken.jpg", bytes: broken_jpeg(23), reason: Some("Damaged file") },
    ]
}

/// The mixed-format folder through `dalica-image` itself: each file sniffs as its format, and
/// decodes (and makes a thumbnail) or answers its tile's reason.
pub fn check_mixed() -> Vec<String> {
    let limits = Limits::default();
    let mut out = Vec::new();
    for f in mixed_folder() {
        let got = decode_with(&f.bytes, &ON, &limits);
        match (f.reason, got) {
            (None, Ok(img)) => {
                let t = decode_thumbnail_with(&f.bytes, 128, &ON, &limits).unwrap_or_else(|e| panic!("{}: thumbnail: {e}", f.name));
                assert_eq!(t.width.max(t.height), 128, "{}", f.name);
                out.push(format!("{} {} × {}", f.name, img.width, img.height));
            }
            (Some(reason), Err(e)) => {
                assert_eq!(e.reason(), reason, "{}", f.name);
                out.push(format!("{} \"{reason}\"", f.name));
            }
            (want, got) => panic!("{}: want {want:?}, got {:?}", f.name, got.map(|i| (i.width, i.height))),
        }
    }
    out
}
