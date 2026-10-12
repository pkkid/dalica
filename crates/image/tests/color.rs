//! The colour stage end to end (`formats.md` §4): `scenes` photos stored in Display P3, Adobe RGB
//! and ProPhoto, tagged with v2 and v4 profiles in JPEG (APP2, several chunks) and PNG (`iCCP`, 8
//! and 16 bits), decode to their sRGB reference; with colour management off they do not. Damaged
//! and oversized profiles decode as sRGB. 16-bit PNG and CMYK/YCCK JPEG match stored fingerprints
//! (`fingerprints/color-*.json`, `architecture.md` §11) and an independent reference.
//!
//! Every input is built here: the v4 profiles by `moxcms` from the published primaries, the v2
//! ones by a small writer of our own (ICC.1:2001-04 layout), the CMYK JPEGs by a DC-only baseline
//! encoder of our own (T.81 §F.1.2: every 8 × 8 block is flat).
//!
//! Tolerances, per fingerprint value (cell mean or deviation, 0–255): 3.0 for JPEG (the encoder's
//! loss, as in `decode.rs`), 1.5 for PNG (8-bit quantisation in the wide space).

use std::borrow::Cow;
use std::path::PathBuf;

use dalica_image::color;
use dalica_image::{DecodeOptions, Error, Limits, Rgba8, decode_with};
use dalica_scenes::{Scene, fingerprint};
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};
use moxcms::{ColorProfile, Layout};

const JPEG_TOL: f64 = 3.0;
const PNG_TOL: f64 = 1.5;
const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rgba(img: &dalica_scenes::Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
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

/// Compares `img` with `fingerprints/<name>.json` (`DALICA_BLESS=1` stores it instead).
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

// ---- Profiles ----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Space {
    DisplayP3,
    AdobeRgb,
    ProPhoto,
}

impl Space {
    fn moxcms(self) -> ColorProfile {
        let mut p = match self {
            Space::DisplayP3 => ColorProfile::new_display_p3(),
            Space::AdobeRgb => ColorProfile::new_adobe_rgb(),
            Space::ProPhoto => ColorProfile::new_pro_photo_rgb(),
        };
        // The tone curves, not coding-independent code points, carry the transfer function.
        p.cicp = None;
        p
    }

    /// A v4 profile, written by `moxcms`.
    fn v4(self) -> Vec<u8> {
        let bytes = self.moxcms().encode().unwrap();
        assert_eq!(bytes[8], 4, "{self:?} is not v4");
        bytes
    }

    /// A v2 matrix/TRC profile of our own writing, with `table` entries per tone curve for
    /// Display P3 (sRGB's curve) and a plain gamma for the others.
    fn v2(self, table: usize) -> Vec<u8> {
        let p = self.moxcms();
        let trc = match self {
            Space::DisplayP3 => curv_table(table, srgb_to_linear),
            Space::AdobeRgb => curv_gamma(563.0 / 256.0),
            Space::ProPhoto => curv_gamma(1.8),
        };
        let xyz = |c: moxcms::Xyzd| xyz_tag(c.x, c.y, c.z);
        let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
            (b"rXYZ", xyz(p.red_colorant)),
            (b"gXYZ", xyz(p.green_colorant)),
            (b"bXYZ", xyz(p.blue_colorant)),
        ];
        // Separate (identical) curves, so a long table makes a profile that needs several APP2 chunks.
        for sig in [b"rTRC", b"gTRC", b"bTRC"] {
            tags.push((sig, trc.clone()));
        }
        let bytes = icc_v2(&tags);
        assert_eq!(bytes[8], 2);
        bytes
    }
}

fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    [b"XYZ \0\0\0\0".as_slice(), &s15(x), &s15(y), &s15(z)].concat()
}

fn curv_gamma(g: f64) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend(1u32.to_be_bytes());
    t.extend(((g * 256.0).round() as u16).to_be_bytes());
    t.extend([0, 0]);
    t
}

fn curv_table(n: usize, f: impl Fn(f64) -> f64) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend((n as u32).to_be_bytes());
    for i in 0..n {
        t.extend(((f(i as f64 / (n - 1) as f64) * 65535.0).round() as u16).to_be_bytes());
    }
    while !t.len().is_multiple_of(4) {
        t.push(0);
    }
    t
}

/// An ICC v2.1 display RGB profile with an XYZ PCS holding `tags` (ICC.1:2001-04 §6).
fn icc_v2(tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 128 + 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    for (sig, body) in tags {
        table.extend(*sig);
        table.extend(((table_end + data.len()) as u32).to_be_bytes());
        table.extend((body.len() as u32).to_be_bytes());
        data.extend(body);
    }
    let mut h = vec![0u8; 128];
    h[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
    h[12..16].copy_from_slice(b"mntr");
    h[16..20].copy_from_slice(b"RGB ");
    h[20..24].copy_from_slice(b"XYZ ");
    h[36..40].copy_from_slice(b"acsp");
    h[68..80].copy_from_slice(&[s15(0.9642), s15(1.0), s15(0.8249)].concat());
    let mut icc = [h, table, data].concat();
    let len = icc.len() as u32;
    icc[0..4].copy_from_slice(&len.to_be_bytes());
    icc
}

// ---- Tagged files ------------------------------------------------------------------------------

/// The scene's sRGB pixels expressed in the space of `icc`, at 16 bits.
fn to_space(img: &dalica_scenes::Rgb8, icc: &[u8]) -> Vec<u16> {
    let target = ColorProfile::new_from_slice(icc).unwrap();
    let t = ColorProfile::new_srgb().create_transform_16bit(Layout::Rgb, &target, Layout::Rgb, color::transform_options(16)).unwrap();
    let src: Vec<u16> = img.data.iter().map(|&v| u16::from(v) * 257).collect();
    let mut dst = vec![0u16; src.len()];
    t.transform(&src, &mut dst).unwrap();
    dst
}

fn to8(v: &[u16]) -> Vec<u8> {
    v.iter().map(|&v| ((u32::from(v) * 255 + 32_767) / 65_535) as u8).collect()
}

fn jpeg_rgb(width: u32, height: u32, rgb: &[u8], icc: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 95);
    if let Some(icc) = icc {
        enc.set_icc_profile(icc).unwrap();
    }
    enc.write_image(rgb, width, height, ExtendedColorType::Rgb8).unwrap();
    out
}

fn png_with(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8], icc: Option<Vec<u8>>) -> Vec<u8> {
    let mut info = png::Info::with_size(width, height);
    info.color_type = color;
    info.bit_depth = depth;
    info.icc_profile = icc.map(Cow::Owned);
    let mut out = Vec::new();
    let mut w = png::Encoder::with_info(&mut out, info).unwrap().write_header().unwrap();
    w.write_image_data(data).unwrap();
    w.finish().unwrap();
    out
}

fn png16(width: u32, height: u32, color: png::ColorType, samples: &[u16], icc: Option<Vec<u8>>) -> Vec<u8> {
    let bytes: Vec<u8> = samples.iter().flat_map(|v| v.to_be_bytes()).collect();
    png_with(width, height, color, png::BitDepth::Sixteen, &bytes, icc)
}

/// How many APP2 ICC chunks a JPEG carries.
fn icc_chunks(jpeg: &[u8]) -> usize {
    jpeg.windows(16).filter(|w| w[0] == 0xff && w[1] == 0xe2 && &w[4..] == b"ICC_PROFILE\0").count()
}

#[test]
fn tagged_photos_match_their_srgb_reference_and_differ_without_colour_management() {
    let scene = Scene::new(31, 192, 128);
    let img = scene.render().unwrap();
    let reference = rgba(&img);
    let mut report = Vec::new();
    for space in [Space::DisplayP3, Space::AdobeRgb, Space::ProPhoto] {
        // A long P3 table makes a 144 KB profile: three APP2 chunks to reassemble.
        for (version, icc) in [("v2", space.v2(24_000)), ("v4", space.v4())] {
            let wide = to_space(&img, &icc);
            let files = [
                ("jpeg", jpeg_rgb(img.width, img.height, &to8(&wide), Some(icc.clone())), JPEG_TOL),
                ("png8", png_with(img.width, img.height, png::ColorType::Rgb, png::BitDepth::Eight, &to8(&wide), Some(icc.clone())), PNG_TOL),
                ("png16", png16(img.width, img.height, png::ColorType::Rgb, &wide, Some(icc.clone())), PNG_TOL),
            ];
            for (container, bytes, tol) in files {
                let case = format!("{space:?}-{version}-{container}");
                if container == "jpeg" {
                    let chunks = icc_chunks(&bytes);
                    assert_eq!(chunks, icc.len().div_ceil(65_519), "{case}: APP2 chunks");
                    assert!(matches!((space, version), (Space::DisplayP3, "v2")) == (chunks > 1), "{case}: {chunks} chunks");
                }
                let managed = decode_with(&bytes, &ON, &Limits::default()).unwrap();
                assert_close(&case, &managed, &reference, tol);
                let ignored = decode_with(&bytes, &OFF, &Limits::default()).unwrap();
                let (on, off) = (distance(&managed, &reference), distance(&ignored, &reference));
                assert!(off > tol, "{case}: without colour management it still matches ({off:.2} ≤ {tol})");
                report.push(format!("{case}: {on:.2} managed, {off:.2} unmanaged"));
            }
        }
    }
    println!("{}", report.join("\n"));
}

/// A space, its linear → linear-sRGB matrix and its decoding curve.
type Case = (Space, [[f64; 3]; 3], fn(f64) -> f64);

fn mat_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// Single colours against the published RGB → XYZ matrices, computed independently of `moxcms`:
/// Display P3, Adobe RGB and ProPhoto values decode, at 8 and at 16 bits, to the sRGB values the
/// matrices give. ProPhoto → sRGB has a coefficient over 2 (red), which a fixed-point path with a
/// ±2 range gets wrong: this case is what keeps every architecture on the same arithmetic.
#[test]
fn known_colours_convert_like_the_published_matrices() {
    // Linear Display P3 → linear sRGB and linear Adobe RGB (1998) → linear sRGB, both D65.
    let p3 = [[1.224_940_2, -0.224_940_4, 0.0], [-0.042_056_9, 1.042_057_1, 0.0], [-0.019_637_6, -0.078_636_1, 1.098_273_5]];
    let adobe = [[1.398_355, -0.398_355, 0.0], [0.0, 1.0, 0.0], [0.0, -0.042_930, 1.042_930]];
    // ProPhoto (D50) → XYZ, Bradford D50 → D65, XYZ → linear sRGB.
    let prophoto_to_xyz = [[0.797_674_9, 0.135_191_7, 0.031_353_4], [0.288_040_2, 0.711_874_1, 0.000_085_7], [0.0, 0.0, 0.825_210_0]];
    let bradford = [[0.955_576_6, -0.023_039_3, 0.063_163_6], [-0.028_289_5, 1.009_941_6, 0.021_007_7], [0.012_298_2, -0.020_483_0, 1.329_909_8]];
    let xyz_to_srgb = [[3.240_454_2, -1.537_138_5, -0.498_531_4], [-0.969_266_0, 1.876_010_8, 0.041_556_0], [0.055_643_4, -0.204_025_9, 1.057_225_2]];
    let prophoto = mat_mul(xyz_to_srgb, mat_mul(bradford, prophoto_to_xyz));
    assert!(prophoto[0][0] > 2.0, "{prophoto:?}");
    let cases: [Case; 3] = [
        (Space::DisplayP3, p3, srgb_to_linear),
        (Space::AdobeRgb, adobe, |v: f64| v.powf(563.0 / 256.0)),
        (Space::ProPhoto, prophoto, |v: f64| v.powf(1.8)),
    ];
    for (space, m, to_linear) in cases {
        for (version, icc) in [("v2", space.v2(4096)), ("v4", space.v4())] {
            for px in [[204u8, 77, 51], [40, 160, 90], [128, 128, 128], [230, 200, 20], [10, 30, 200], [150, 90, 80]] {
                let lin = px.map(|v| to_linear(f64::from(v) / 255.0));
                let want: Vec<u8> =
                    m.iter().map(|row| (linear_to_srgb(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]) * 255.0).round() as u8).collect();
                let eight = png_with(8, 8, png::ColorType::Rgb, png::BitDepth::Eight, &px.repeat(64), Some(icc.clone()));
                let wide: Vec<u16> = px.map(|v| u16::from(v) * 257).repeat(64);
                let sixteen = png16(8, 8, png::ColorType::Rgb, &wide, Some(icc.clone()));
                for (depth, bytes) in [(8, eight), (16, sixteen)] {
                    let got = decode_with(&bytes, &ON, &Limits::default()).unwrap();
                    for c in 0..3 {
                        assert!(
                            got.data[c].abs_diff(want[c]) <= 2,
                            "{space:?} {version} {depth}-bit {px:?}: got {:?}, want {want:?}",
                            &got.data[..3]
                        );
                    }
                }
            }
        }
    }
}

// ---- Unusable profiles -------------------------------------------------------------------------

/// Inserts `segments` (whole APP segments) right after SOI.
fn with_segments(jpeg: &[u8], segments: &[Vec<u8>]) -> Vec<u8> {
    [&jpeg[..2], &segments.concat(), &jpeg[2..]].concat()
}

fn app2_icc(n: u8, count: u8, data: &[u8]) -> Vec<u8> {
    let mut s = vec![0xff, 0xe2];
    s.extend(((2 + 14 + data.len()) as u16).to_be_bytes());
    s.extend(b"ICC_PROFILE\0");
    s.extend([n, count]);
    s.extend(data);
    s
}

#[test]
fn damaged_or_oversized_profiles_decode_as_srgb() {
    let img = Scene::new(32, 96, 64).render().unwrap();
    let wide = to8(&to_space(&img, &Space::ProPhoto.v4()));
    let srgb_view = |bytes: &[u8]| decode_with(bytes, &OFF, &Limits::default()).unwrap();
    let plain_jpeg = jpeg_rgb(img.width, img.height, &wide, None);
    let good = Space::ProPhoto.v4();
    let mut too_many_tags = good.clone();
    too_many_tags[128..132].copy_from_slice(&5000u32.to_be_bytes());
    let mut bad_offsets = good.clone();
    bad_offsets[136..140].copy_from_slice(&0x7fff_ff00u32.to_be_bytes());
    let mut truncated = good.clone();
    truncated.truncate(good.len() / 2);
    let jpegs = [
        ("garbage profile", with_segments(&plain_jpeg, &[app2_icc(1, 1, &[0x5a; 3000])])),
        ("too many tags", with_segments(&plain_jpeg, &[app2_icc(1, 1, &too_many_tags)])),
        ("tag outside the profile", with_segments(&plain_jpeg, &[app2_icc(1, 1, &bad_offsets)])),
        ("truncated profile", with_segments(&plain_jpeg, &[app2_icc(1, 1, &truncated)])),
        ("missing chunk", with_segments(&plain_jpeg, &[app2_icc(1, 2, &good)])),
        ("repeated chunk", with_segments(&plain_jpeg, &[app2_icc(1, 2, &good[..300]), app2_icc(1, 2, &good[300..])])),
        ("chunk zero", with_segments(&plain_jpeg, &[app2_icc(0, 1, &good)])),
    ];
    let expected = srgb_view(&plain_jpeg);
    for (case, bytes) in jpegs {
        assert_eq!(decode_with(&bytes, &ON, &Limits::default()).unwrap(), expected, "{case}");
    }
    // PNG: a 17 MiB profile (over the 16 MiB limit; `iCCP` compresses it to almost nothing) and
    // an RGB profile on a grey image.
    let mut huge = good.clone();
    huge.resize(17 << 20, 0);
    let len = huge.len() as u32;
    huge[0..4].copy_from_slice(&len.to_be_bytes());
    let pngs = [
        ("oversized profile", png_with(img.width, img.height, png::ColorType::Rgb, png::BitDepth::Eight, &wide, Some(huge))),
        ("garbage profile", png_with(img.width, img.height, png::ColorType::Rgb, png::BitDepth::Eight, &wide, Some(vec![0xa5; 500]))),
        (
            "RGB profile on grey",
            png_with(
                img.width,
                img.height,
                png::ColorType::Grayscale,
                png::BitDepth::Eight,
                &wide[..(img.width * img.height) as usize],
                Some(good.clone()),
            ),
        ),
    ];
    for (case, bytes) in pngs {
        assert!(bytes.len() < 1 << 20, "{case}: {} bytes", bytes.len());
        assert_eq!(decode_with(&bytes, &ON, &Limits::default()).unwrap(), srgb_view(&bytes), "{case}");
    }
    // The same 17 MiB profile is fine under a larger limit (it is only padded), so the size is
    // what refused it.
    let big = Limits { max_icc: 18 << 20, ..Limits::default() };
    let mut padded = good.clone();
    padded.resize(17 << 20, 0);
    let len = padded.len() as u32;
    padded[0..4].copy_from_slice(&len.to_be_bytes());
    let bytes = png_with(img.width, img.height, png::ColorType::Rgb, png::BitDepth::Eight, &wide, Some(padded));
    assert_ne!(decode_with(&bytes, &ON, &big).unwrap(), srgb_view(&bytes));
}

// ---- 16-bit PNG --------------------------------------------------------------------------------

#[test]
fn sixteen_bit_pngs_match_their_fingerprints() {
    let img = Scene::new(33, 160, 120).render().unwrap();
    let reference = rgba(&img);
    // Untagged RGB: 8-bit values spread over 16 bits with a low-order ramp the decode rounds away.
    let samples: Vec<u16> = img.data.iter().enumerate().map(|(i, &v)| (i32::from(v) * 257 + (i % 97) as i32 - 48).clamp(0, 65_535) as u16).collect();
    let rgb = decode_with(&png16(img.width, img.height, png::ColorType::Rgb, &samples, None), &ON, &Limits::default()).unwrap();
    assert_close("png16-rgb", &rgb, &reference, 0.5);
    assert_fingerprint("color-png16-rgb", &rgb, 0.5);
    // ProPhoto-tagged, at 16 bits: the stage converts with 16-bit precision.
    let icc = Space::ProPhoto.v4();
    let tagged = decode_with(&png16(img.width, img.height, png::ColorType::Rgb, &to_space(&img, &icc), Some(icc)), &ON, &Limits::default()).unwrap();
    assert_close("png16-prophoto", &tagged, &reference, 1.0);
    assert_fingerprint("color-png16-prophoto", &tagged, 1.0);
    // Grey with alpha at 16 bits.
    let grey: Vec<u16> = (0..img.width * img.height).flat_map(|i| [(i * 37 % 65_536) as u16, (65_535 - i * 11 % 65_536) as u16]).collect();
    let ga = decode_with(&png16(img.width, img.height, png::ColorType::GrayscaleAlpha, &grey, None), &ON, &Limits::default()).unwrap();
    for (px, s) in ga.data.chunks(4).zip(grey.chunks(2)) {
        let v = ((u32::from(s[0]) * 255 + 32_767) / 65_535) as u8;
        assert_eq!(px, [v, v, v, ((u32::from(s[1]) * 255 + 32_767) / 65_535) as u8]);
    }
}

// ---- CMYK JPEG ---------------------------------------------------------------------------------

/// A baseline JPEG with four components, no subsampling, and flat 8 × 8 blocks (DC coefficients
/// only, quantiser 1): each block decodes to exactly the stored value `block(bx, by)` per
/// component. `adobe` adds an APP14 segment with that colour transform (0 CMYK, 2 YCCK).
fn flat_jpeg4(width: u32, height: u32, adobe: Option<u8>, block: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut out = vec![0xff, 0xd8];
    let mut segment = |marker: u8, body: &[u8]| {
        out.extend([0xff, marker]);
        out.extend(((body.len() + 2) as u16).to_be_bytes());
        out.extend(body);
    };
    if let Some(t) = adobe {
        segment(0xee, &[b"Adobe".as_slice(), &100u16.to_be_bytes(), &[0, 0, 0, 0, t]].concat());
    }
    segment(0xdb, &[[0u8].as_slice(), &[1u8; 64]].concat());
    let mut sof = vec![8];
    sof.extend((height as u16).to_be_bytes());
    sof.extend((width as u16).to_be_bytes());
    sof.push(4);
    for id in 1..=4 {
        sof.extend([id, 0x11, 0]);
    }
    segment(0xc0, &sof);
    // DC table 0: the typical luminance DC table of T.81 Annex K.3; AC table 0: only EOB, code "0".
    let dc_bits = [0u8, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    segment(0xc4, &[[0x00u8].as_slice(), &dc_bits, &(0..12).collect::<Vec<u8>>()].concat());
    let ac_bits = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    segment(0xc4, &[[0x10u8].as_slice(), &ac_bits, &[0x00]].concat());
    segment(0xda, &[4, 1, 0, 2, 0, 3, 0, 4, 0, 0, 63, 0]);
    // Canonical DC codes (T.81 Annex C).
    let mut codes = Vec::new();
    let mut code = 0u32;
    for (len, &n) in dc_bits.iter().enumerate() {
        for _ in 0..n {
            codes.push((code, len as u32 + 1));
            code += 1;
        }
        code <<= 1;
    }
    let mut bits = BitWriter::default();
    let mut pred = [0i32; 4];
    for by in 0..height.div_ceil(8) {
        for bx in 0..width.div_ceil(8) {
            let values = block(bx, by);
            for c in 0..4 {
                let dc = (i32::from(values[c]) - 128) * 8;
                let diff = dc - pred[c];
                pred[c] = dc;
                let cat = 32 - diff.unsigned_abs().leading_zeros();
                let (code, len) = codes[cat as usize];
                bits.put(code, len);
                if cat > 0 {
                    let v = if diff < 0 { diff + (1 << cat) - 1 } else { diff };
                    bits.put(v as u32, cat);
                }
                bits.put(0, 1); // EOB
            }
        }
    }
    out.extend(bits.finish());
    out.extend([0xff, 0xd9]);
    out
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, len: u32) {
        for i in (0..len).rev() {
            self.acc = (self.acc << 1) | ((value >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.byte();
            }
        }
    }

    fn byte(&mut self) {
        let b = self.acc as u8;
        self.out.push(b);
        if b == 0xff {
            self.out.push(0); // byte stuffing
        }
        self.acc = 0;
        self.n = 0;
    }

    fn finish(mut self) -> Vec<u8> {
        while self.n != 0 {
            self.put(1, 1); // pad with ones
        }
        self.out
    }
}

/// The scene averaged over 8 × 8 blocks: what a DC-only JPEG of it shows.
fn block_means(img: &dalica_scenes::Rgb8) -> Vec<[u8; 3]> {
    let (bw, bh) = (img.width / 8, img.height / 8);
    let mut means = Vec::new();
    for by in 0..bh {
        for bx in 0..bw {
            let mut sum = [0u32; 3];
            for y in by * 8..by * 8 + 8 {
                for x in bx * 8..bx * 8 + 8 {
                    let p = img.get(x, y).unwrap();
                    for c in 0..3 {
                        sum[c] += u32::from(p[c]);
                    }
                }
            }
            means.push(sum.map(|s| ((s + 32) / 64) as u8));
        }
    }
    means
}

/// Ink amounts (0 = none) for an RGB colour, with full grey-component replacement.
fn ink([r, g, b]: [u8; 3]) -> [u8; 4] {
    let k = 255 - r.max(g).max(b);
    if k == 255 {
        return [0, 0, 0, 255];
    }
    let ink = |v: u8| ((1.0 - f64::from(v) / f64::from(255 - k)) * 255.0).round().clamp(0.0, 255.0) as u8;
    [ink(r), ink(g), ink(b), k]
}

fn rgb_to_ycc([r, g, b]: [u8; 3]) -> [u8; 3] {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    [y, cb, cr].map(|v| v.round().clamp(0.0, 255.0) as u8)
}

#[test]
fn cmyk_and_ycck_jpegs_match_their_fingerprints() {
    let img = Scene::new(34, 160, 120).render().unwrap();
    let means = block_means(&img);
    let bw = img.width / 8;
    let mut reference = Rgba8 { width: img.width, height: img.height, data: Vec::new() };
    for y in 0..img.height {
        for x in 0..img.width {
            let [r, g, b] = means[(y / 8 * bw + x / 8) as usize];
            reference.data.extend([r, g, b, 255]);
        }
    }
    let at = |bx: u32, by: u32| means[(by * bw + bx) as usize];
    // Adobe CMYK, stored inverted (255 = no ink).
    let cmyk = flat_jpeg4(img.width, img.height, Some(0), |bx, by| ink(at(bx, by)).map(|v| 255 - v));
    // Adobe YCCK: inverted C, M, Y coded as YCbCr, then inverted K.
    let ycck = flat_jpeg4(img.width, img.height, Some(2), |bx, by| {
        let [c, m, y, k] = ink(at(bx, by)).map(|v| 255 - v);
        let [yy, cb, cr] = rgb_to_ycc([c, m, y]);
        [yy, cb, cr, k]
    });
    // No Adobe segment: plain ink amounts.
    let plain = flat_jpeg4(img.width, img.height, None, |bx, by| ink(at(bx, by)));
    for (name, bytes) in [("color-cmyk-jpeg", cmyk), ("color-ycck-jpeg", ycck), ("color-cmyk-plain-jpeg", plain)] {
        let header = dalica_image::read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((header.width, header.height), (img.width, img.height));
        let got = decode_with(&bytes, &ON, &Limits::default()).unwrap();
        assert_close(name, &got, &reference, 2.0);
        if name != "color-cmyk-plain-jpeg" {
            assert_fingerprint(name, &got, 1.0);
        }
        // Without a profile, colour management changes nothing for CMYK.
        assert_eq!(decode_with(&bytes, &OFF, &Limits::default()).unwrap(), got, "{name}");
    }
}

/// Truncated tagged and CMYK files decode or fail, never panic; a refused one says why.
#[test]
fn truncated_tagged_files_never_panic() {
    let img = Scene::new(35, 64, 48).render().unwrap();
    let icc = Space::DisplayP3.v2(1024);
    let wide = to_space(&img, &icc);
    let files = [
        (jpeg_rgb(img.width, img.height, &to8(&wide), Some(icc.clone())), 1),
        (png16(img.width, img.height, png::ColorType::Rgb, &wide, Some(icc)), 7),
        (flat_jpeg4(64, 48, Some(2), |bx, by| [(bx * 30) as u8, (by * 40) as u8, 128, 60]), 1),
    ];
    for (bytes, step) in files {
        for len in (0..bytes.len()).step_by(step) {
            if let Err(e) = decode_with(&bytes[..len], &ON, &Limits::default()) {
                assert!(!e.reason().is_empty());
                assert!(!matches!(e, Error::Panic(_)), "a decoder panicked at {len}: {e:?}");
            }
        }
    }
}
