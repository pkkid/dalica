//! HEIF measured on a wider set (`formats.md` §3): the `heif-wide` corpus set, made locally by
//! `cargo xtask corpus make heif-wide` from `dalica-scenes` pictures through libheif's `heif-enc`
//! (8, 10 and 12 bits; 4:2:0, 4:2:2 and 4:4:4; alpha; grids; `irot` and `imir`; ICC profiles;
//! and the layouts Dalica refuses). It stays in `corpus/heif-wide/` (gitignored) until the
//! maintainer decides whether to publish it, so the measurement skips with a message when the set
//! is absent.
//!
//! Run it with the decoder: `cargo test -p dalica-image --features heif --test all heif_wide --
//! --nocapture` (add `--release` for representative times). Every file is decoded; the table
//! gives its answer (the size it decoded to, or the tile text it shows), its error against the
//! upright source picture and against libheif's own decode (`heif-dec`, when it was installed),
//! and the decode time. Each row must give the answer [`VARIANTS`] expects, a decoded row must
//! stay above its PSNR floor, and no file may panic. Without the feature every file must answer
//! with a tile text. The table is also written to `target/test-output/image/heif-wide.md`.
//!
//! [`sources_and_recipes`] doubles as the set's generator: with `DALICA_HEIF_WIDE_MAKE=<dir>` it
//! writes the source PNGs and `recipes.tsv` (the `heif-enc` arguments of every file) there, for
//! the xtask to encode. [`VARIANTS`] is the one list of files, arguments and expectations.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_image::{Error, Limits, Rgba8, decode, formats};
use dalica_scenes::{Exif, Kind, Rgb8, Scene, exif, icc, orient};

/// The set's directory name under `corpus/`.
const SET: &str = "heif-wide";

/// Where the xtask asks for the sources (`cargo xtask corpus make heif-wide`).
const MAKE_ENV: &str = "DALICA_HEIF_WIDE_MAKE";

/// How a source picture is stored in its PNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Rgb8,
    Rgb16,
    Rgba8,
    Rgba16,
    Grey8,
}

/// A source PNG: a scene, stored in `layout`, its pixels turned for `orientation` (1–8, as EXIF),
/// optionally tagged Display P3 (an ICC profile) or carrying an EXIF block with the orientation.
struct Source {
    name: &'static str,
    kind: Kind,
    seed: u64,
    width: u32,
    height: u32,
    layout: Layout,
    orientation: u8,
    p3: bool,
    exif: bool,
}

const fn src(name: &'static str, kind: Kind, seed: u64, size: (u32, u32), layout: Layout) -> Source {
    Source { name, kind, seed, width: size.0, height: size.1, layout, orientation: 1, p3: false, exif: false }
}

const fn turned(name: &'static str, orientation: u8) -> Source {
    Source { orientation, ..src(name, Kind::Horizon, 7, A, Layout::Rgb8) }
}

const fn p3(s: Source) -> Source {
    Source { p3: true, ..s }
}

/// The main picture (a phone photo's shape), an odd-sized one and a large one for grids.
const A: (u32, u32) = (640, 480);
const ODD: (u32, u32) = (641, 479);
const BIG: (u32, u32) = (1280, 960);

const SOURCES: &[Source] = &[
    src("a8", Kind::Horizon, 7, A, Layout::Rgb8),
    src("a16", Kind::Horizon, 7, A, Layout::Rgb16),
    src("a8-alpha", Kind::Horizon, 7, A, Layout::Rgba8),
    src("a16-alpha", Kind::Horizon, 7, A, Layout::Rgba16),
    src("a8-grey", Kind::Horizon, 7, A, Layout::Grey8),
    p3(src("a8-p3", Kind::Horizon, 7, A, Layout::Rgb8)),
    p3(src("a16-p3", Kind::Horizon, 7, A, Layout::Rgb16)),
    src("odd8", Kind::Blobs, 11, ODD, Layout::Rgb8),
    src("big8", Kind::Blobs, 3, BIG, Layout::Rgb8),
    src("big16", Kind::Blobs, 3, BIG, Layout::Rgb16),
    src("big8-alpha", Kind::Blobs, 3, BIG, Layout::Rgba8),
    turned("a8-o2", 2),
    turned("a8-o3", 3),
    turned("a8-o4", 4),
    turned("a8-o5", 5),
    turned("a8-o6", 6),
    turned("a8-o7", 7),
    turned("a8-o8", 8),
    Source { exif: true, ..turned("a8-o6-exif", 6) },
    src("seq/frame-1", Kind::Horizon, 7, A, Layout::Rgb8),
    src("seq/frame-2", Kind::Gradient, 5, A, Layout::Rgb8),
];

/// What a file must answer.
#[derive(Clone, Copy, Debug)]
enum Expect {
    /// Decodes to its reference (size and pixels), at least this many dB from it.
    Decodes(f64),
    /// Refused with this tile text.
    Refused(&'static str),
}

/// One file of the set.
struct Variant {
    /// The file in `corpus/heif-wide/`; its stem names libheif's decode `libheif/<stem>.png`.
    file: &'static str,
    /// The source PNGs given to `heif-enc` (several for a sequence).
    inputs: &'static [&'static str],
    /// The source showing what the file should look like when shown.
    reference: &'static str,
    args: &'static [&'static str],
    what: &'static str,
    expect: Expect,
}

const HEIC_UNSUPPORTED: &str = "This kind of HEIC file can't be displayed";
/// What a file answers when sniffing does not take it for HEIF (its brands; `sniff.rs`).
const NOT_SNIFFED: &str = "Not a picture Dalica can read";

const fn v(file: &'static str, inputs: &'static [&'static str], args: &'static [&'static str], what: &'static str, expect: Expect) -> Variant {
    let reference = match inputs {
        [first, ..] => first,
        [] => "",
    };
    Variant { file, inputs, reference, args, what, expect }
}

const fn shown_as(mut variant: Variant, reference: &'static str) -> Variant {
    variant.reference = reference;
    variant
}

/// Every file, its `heif-enc` arguments and what it must answer. The PSNR floors sit a little
/// under the measured values (`formats.md` §3); like every floor they only rise.
const VARIANTS: &[Variant] = &[
    v("q90-420-8bit.heic", &["a8"], &["-q", "90"], "8-bit 4:2:0, quality 90", Expect::Decodes(47.0)),
    v("q50-420-8bit.heic", &["a8"], &["-q", "50"], "8-bit 4:2:0, quality 50 (heif-enc's default)", Expect::Decodes(46.0)),
    v("q10-420-8bit.heic", &["a8"], &["-q", "10"], "8-bit 4:2:0, quality 10", Expect::Decodes(39.0)),
    v("q90-420-10bit.heic", &["a16"], &["-q", "90", "-b", "10"], "10-bit 4:2:0 (Main 10)", Expect::Decodes(48.0)),
    v("q90-420-12bit.heic", &["a16"], &["-q", "90", "-b", "12"], "12-bit 4:2:0 (RExt)", Expect::Decodes(48.0)),
    v("q90-422-8bit.heic", &["a8"], &["-q", "90", "-p", "chroma=422"], "8-bit 4:2:2", Expect::Refused(HEIC_UNSUPPORTED)),
    v("q90-444-8bit.heic", &["a8"], &["-q", "90", "-p", "chroma=444"], "8-bit 4:4:4", Expect::Refused(HEIC_UNSUPPORTED)),
    v("q90-422-10bit.heic", &["a16"], &["-q", "90", "-b", "10", "-p", "chroma=422"], "10-bit 4:2:2", Expect::Refused(HEIC_UNSUPPORTED)),
    v("q90-444-10bit.heic", &["a16"], &["-q", "90", "-b", "10", "-p", "chroma=444"], "10-bit 4:4:4", Expect::Refused(HEIC_UNSUPPORTED)),
    v("lossless-rgb.heic", &["a8"], &["-L"], "lossless: RGB 4:4:4, identity matrix", Expect::Refused(HEIC_UNSUPPORTED)),
    v("alpha-8bit.heic", &["a8-alpha"], &["-q", "90"], "8-bit with an alpha auxiliary image", Expect::Decodes(47.0)),
    v("alpha-10bit.heic", &["a16-alpha"], &["-q", "90", "-b", "10"], "10-bit with a 10-bit alpha image", Expect::Decodes(48.0)),
    v("grey-8bit.heic", &["a8-grey"], &["-q", "90"], "monochrome (4:0:0)", Expect::Decodes(70.0)),
    v("odd-size.heic", &["odd8"], &["-q", "90"], "641 × 479, odd sides with 4:2:0", Expect::Decodes(49.0)),
    v("grid-512.heic", &["big8"], &["-q", "90", "--cut-tiles", "512"], "1280 × 960 as a 3 × 2 grid of 512 px tiles", Expect::Decodes(51.0)),
    v("grid-256-10bit.heic", &["big16"], &["-q", "90", "-b", "10", "--cut-tiles", "256"], "10-bit 5 × 4 grid of 256 px tiles", Expect::Decodes(55.0)),
    v("grid-alpha.heic", &["big8-alpha"], &["-q", "90", "--cut-tiles", "512"], "a grid with a grid of alpha tiles", Expect::Decodes(51.0)),
    v(
        "grid-444.heic",
        &["big8"],
        &["-q", "90", "-p", "chroma=444", "--cut-tiles", "512"],
        "a grid of 4:4:4 tiles",
        Expect::Refused(HEIC_UNSUPPORTED),
    ),
    shown_as(v("irot-90.heic", &["a8-o6"], &["-q", "90", "--rotate-cw", "90"], "irot: shown turned 90° clockwise", Expect::Decodes(46.0)), "a8"),
    shown_as(v("irot-180.heic", &["a8-o3"], &["-q", "90", "--rotate-cw", "180"], "irot: shown turned 180°", Expect::Decodes(47.0)), "a8"),
    shown_as(v("irot-270.heic", &["a8-o8"], &["-q", "90", "--rotate-cw", "270"], "irot: shown turned 270° clockwise", Expect::Decodes(46.0)), "a8"),
    shown_as(v("imir-h.heic", &["a8-o2"], &["-q", "90", "--flip-h"], "imir: shown flipped left to right", Expect::Decodes(47.0)), "a8"),
    shown_as(v("imir-v.heic", &["a8-o4"], &["-q", "90", "--flip-v"], "imir: shown flipped top to bottom", Expect::Decodes(47.0)), "a8"),
    shown_as(
        v(
            "irot-imir-5.heic",
            &["a8-o5"],
            &["-q", "90", "--rotate-cw", "90", "--flip-h"],
            "irot then imir (EXIF 5, transpose)",
            Expect::Decodes(46.0),
        ),
        "a8",
    ),
    shown_as(
        v(
            "irot-imir-7.heic",
            &["a8-o7"],
            &["-q", "90", "--rotate-cw", "270", "--flip-h"],
            "irot then imir (EXIF 7, transverse)",
            Expect::Decodes(46.0),
        ),
        "a8",
    ),
    shown_as(
        v(
            "exif-orientation-6.heic",
            &["a8-o6-exif"],
            &["-q", "90"],
            "EXIF orientation 6 and no irot: shown as stored, as libheif does",
            Expect::Decodes(46.0),
        ),
        "a8-o6",
    ),
    v("icc-p3.heic", &["a8-p3"], &["-q", "90"], "a Display P3 ICC profile (colr prof)", Expect::Decodes(45.0)),
    v("icc-p3-10bit.heic", &["a16-p3"], &["-q", "90", "-b", "10"], "10-bit with a Display P3 ICC profile", Expect::Decodes(45.0)),
    v("icc-and-nclx.heic", &["a8-p3"], &["-q", "90", "--enable-two-colr-boxes"], "both colr boxes: ICC and nclx", Expect::Decodes(45.0)),
    v(
        "nclx-709-limited.heic",
        &["a8"],
        &["-q", "90", "--matrix_coefficients", "1", "--full_range_flag", "0"],
        "nclx: BT.709 matrix, limited range",
        Expect::Decodes(46.0),
    ),
    v("thumbnail.heic", &["a8"], &["-q", "90", "-t", "160"], "with a 160 px HEVC thumbnail item", Expect::Decodes(47.0)),
    v(
        "sequence.heic",
        &["seq/frame-1", "seq/frame-2"],
        &["-q", "90", "-S"],
        "a two-frame sequence (moov) beside its still primary item",
        Expect::Decodes(47.0),
    ),
    v("mini.heic", &["a8"], &["-q", "90", "--mini"], "the compact `mini` box layout (brand mif3): not sniffed as HEIF", Expect::Refused(NOT_SNIFFED)),
    v("uncompressed.heic", &["a8"], &["-U"], "an uncompressed (unci) item", Expect::Refused(HEIC_UNSUPPORTED)),
    v("jpeg-coded.heic", &["a8"], &["-q", "90", "--jpeg"], "a JPEG-coded item (brand jpeg): not sniffed as HEIF", Expect::Refused(NOT_SNIFFED)),
    v("av1.avif", &["a8"], &["-q", "90", "-A"], "AV1 (AVIF)", Expect::Refused("AVIF files can't be displayed")),
];

/// `corpus/heif-wide/` in the workspace (this crate is `crates/image`).
fn set_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("corpus").join(SET)
}

fn stem(file: &str) -> &str {
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}

// ---------------------------------------------------------------------------------------------
// The generator

/// The upright picture of `s`, turned for its orientation.
fn stored(s: &Source) -> Rgb8 {
    let upright = Scene::new(s.seed, s.width, s.height).with_kind(s.kind).render().unwrap();
    orient::store(&upright, s.orientation).unwrap()
}

fn png_of(s: &Source) -> Vec<u8> {
    let img = stored(s);
    let rgb = img.data.as_chunks::<3>().0;
    let alpha = || dalica_scenes::formats::alpha(img.width, img.height);
    let wide = |v: u8| (u16::from(v) * 257).to_be_bytes();
    let (color, depth, data): (png::ColorType, png::BitDepth, Vec<u8>) = match s.layout {
        Layout::Rgb8 => (png::ColorType::Rgb, png::BitDepth::Eight, img.data.clone()),
        Layout::Rgb16 => (png::ColorType::Rgb, png::BitDepth::Sixteen, img.data.iter().flat_map(|&v| wide(v)).collect()),
        Layout::Rgba8 => (png::ColorType::Rgba, png::BitDepth::Eight, rgb.iter().zip(alpha()).flat_map(|(&[r, g, b], a)| [r, g, b, a]).collect()),
        Layout::Rgba16 => (
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            rgb.iter().zip(alpha()).flat_map(|(&p, a)| p.into_iter().chain([a]).flat_map(wide)).collect(),
        ),
        Layout::Grey8 => (
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            rgb.iter().map(|&[r, g, b]| ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000) as u8).collect(),
        ),
    };
    let mut info = png::Info::with_size(img.width, img.height);
    info.color_type = color;
    info.bit_depth = depth;
    if s.p3 {
        info.icc_profile = Some(Cow::Owned(icc::rgb(icc::Space::DisplayP3, icc::Version::V4)));
    }
    if s.exif {
        let block = exif::tiff(&Exif { orientation: s.orientation, ..Exif::default() }, img.width, img.height, None).unwrap();
        info.exif_metadata = Some(Cow::Owned(block));
    }
    let mut out = Vec::new();
    let mut w = png::Encoder::with_info(&mut out, info).unwrap().write_header().unwrap();
    w.write_image_data(&data).unwrap();
    w.finish().unwrap();
    out
}

/// `recipes.tsv`: one line per file, `<file>\t<inputs>\t<args>` (inputs and args separated by
/// spaces, inputs relative to the set), read by `cargo xtask corpus make heif-wide`.
fn recipes() -> String {
    let mut out = String::from("# file\tinputs\theif-enc arguments (written by dalica-image tests/heif_wide.rs)\n");
    for v in VARIANTS {
        let inputs: Vec<String> = v.inputs.iter().map(|i| format!("sources/{i}.png")).collect();
        let _ = writeln!(out, "{}\t{}\t{}", v.file, inputs.join(" "), v.args.join(" "));
    }
    out
}

/// The list is consistent; with `DALICA_HEIF_WIDE_MAKE=<dir>`, also writes the sources and
/// `recipes.tsv` there.
#[test]
fn sources_and_recipes() {
    let source = |name: &str| SOURCES.iter().find(|s| s.name == name);
    for (i, v) in VARIANTS.iter().enumerate() {
        assert!(VARIANTS.iter().skip(i + 1).all(|w| stem(w.file) != stem(v.file)), "{} is listed twice", v.file);
        let safe = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b)) && !s.contains("..");
        assert!(safe(v.file) && !v.file.contains('/') && v.args.iter().all(|a| safe(a) || a.contains('=')), "{}", v.file);
        assert!(!v.inputs.is_empty() && v.inputs.iter().all(|i| source(i).is_some()), "{}: unknown input", v.file);
        let reference = source(v.reference).unwrap_or_else(|| panic!("{}: unknown reference", v.file));
        assert!(!reference.exif, "{}: a reference is shown as stored, without EXIF", v.file);
    }
    let Some(out) = std::env::var_os(MAKE_ENV).map(PathBuf::from) else { return };
    for s in SOURCES {
        let path = out.join("sources").join(format!("{}.png", s.name));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, png_of(s)).unwrap();
    }
    std::fs::write(out.join("recipes.tsv"), recipes()).unwrap();
    println!("heif_wide: {} sources and recipes.tsv written to {}", SOURCES.len(), out.display());
}

// ---------------------------------------------------------------------------------------------
// The measurement

/// PSNR in dB over all four channels, and the largest difference; `None` when the sizes differ.
fn psnr(a: &Rgba8, b: &Rgba8) -> Option<(f64, u8)> {
    if (a.width, a.height) != (b.width, b.height) {
        return None;
    }
    let (mut sum, mut max) = (0u64, 0u8);
    for (x, y) in a.data.iter().zip(&b.data) {
        let d = x.abs_diff(*y);
        sum += u64::from(d) * u64::from(d);
        max = max.max(d);
    }
    let mse = sum as f64 / a.data.len().max(1) as f64;
    Some((if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0 * 255.0 / mse).log10() }, max))
}

fn decode_file(path: &Path) -> Option<Rgba8> {
    std::fs::read(path).ok().and_then(|b| decode(&b, &Limits::default()).ok())
}

fn db(m: Option<(f64, u8)>) -> String {
    match m {
        Some((p, max)) if p.is_infinite() => format!("exact (max {max})"),
        Some((p, max)) => format!("{p:.1} dB (max {max})"),
        None => "size differs".into(),
    }
}

/// One row of the table, and the failure it shows, if any.
fn measure(dir: &Path, v: &Variant) -> (String, Option<String>) {
    let path = dir.join(v.file);
    let Ok(bytes) = std::fs::read(&path) else {
        return (format!("| `{}` | {} | — | missing | | | |", v.file, v.what), Some(format!("{} is missing: remake the set", v.file)));
    };
    let start = Instant::now();
    let result = decode(&bytes, &Limits::default());
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let kb = bytes.len() as f64 / 1024.0;
    let reference = decode_file(&dir.join("sources").join(format!("{}.png", v.reference)));
    let libheif = dir.join("libheif").join(format!("{}.png", stem(v.file)));
    let libheif = if libheif.exists() { decode_file(&libheif).map(Some) } else { Some(None) };
    let (answer, vs_source, vs_libheif, failure) = match (&result, formats::heif::DECODES) {
        (Err(Error::Panic(_)), _) => ("**panicked**".to_string(), String::new(), String::new(), Some(format!("{} panicked", v.file))),
        (Err(e), false) => (e.reason(), String::new(), String::new(), None),
        (Ok(_), false) => ("decoded".into(), String::new(), String::new(), Some(format!("{} decoded without the heif feature", v.file))),
        (Ok(img), true) => {
            let src = reference.as_ref().and_then(|r| psnr(img, r));
            let lib = match &libheif {
                Some(Some(l)) => db(psnr(img, l)),
                Some(None) => "libheif failed".into(),
                None => "—".into(),
            };
            let failure = match (v.expect, src) {
                (Expect::Decodes(floor), Some((p, _))) if p >= floor => None,
                (Expect::Decodes(floor), _) => {
                    let out = crate::common::out_dir().join(format!("heif-wide-{}-actual.png", stem(v.file)));
                    let _ = std::fs::write(&out, dalica_scenes::fingerprint::png_rgba(img.width, img.height, &img.data));
                    Some(format!("{}: {} from the source, floor {floor} dB (decoded picture: {})", v.file, db(src), out.display()))
                }
                (Expect::Refused(text), _) => Some(format!("{}: decoded, expected {text:?}", v.file)),
            };
            (format!("{} × {}", img.width, img.height), db(src), lib, failure)
        }
        (Err(e), true) => {
            let lib = match &libheif {
                Some(Some(_)) => "libheif decodes it".into(),
                Some(None) => "libheif failed".into(),
                None => "—".into(),
            };
            let failure = match v.expect {
                Expect::Refused(text) if e.reason() == text => None,
                _ => Some(format!("{}: answered {e:?} ({}), expected {:?}", v.file, e.reason(), v.expect)),
            };
            (format!("refused: \"{}\"", e.reason()), String::new(), lib, failure)
        }
    };
    (format!("| `{}` | {} | {kb:.0} KB | {answer} | {vs_source} | {vs_libheif} | {ms:.1} |", v.file, v.what), failure)
}

/// Every file of the local set gives the answer [`VARIANTS`] expects; none panics.
#[test]
fn measures_every_file() {
    let dir = set_dir();
    if !dir.join("recipes.tsv").is_file() {
        println!("heif_wide: skipped: {} is absent (make it with `cargo xtask corpus make heif-wide`)", dir.display());
        return;
    }
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let mut table = format!(
        "heif feature: {}; {profile} build\n\n| File | What | Size | Answer | Against the source | Against libheif | ms |\n|---|---|---|---|---|---|---|\n",
        if formats::heif::DECODES { "on" } else { "off" }
    );
    let mut failures = Vec::new();
    for v in VARIANTS {
        let (row, failure) = measure(&dir, v);
        table.push_str(&row);
        table.push('\n');
        failures.extend(failure);
    }
    println!("{table}");
    let _ = std::fs::write(crate::common::out_dir().join("heif-wide.md"), &table);
    assert!(failures.is_empty(), "heif_wide:\n  {}", failures.join("\n  "));
}
