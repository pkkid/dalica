//! The M5 test-file writers against independent readers: every writer is deterministic, every file
//! sniffs as its format and reads back through the `image` crate's own decoders (its `tiff`
//! backend directly where `image` refuses a layout, `jxl-oxide` for JPEG XL), PSD and ICC through
//! a structural check, and every profile parses with `moxcms`.

use std::io::Cursor;

use dalica_scenes::formats::{self, Colour, Compression, Depth, Psd, SampleData, Samples, Tiff, Webp};
use dalica_scenes::icc::{self, Space, Version};
use dalica_scenes::{ByteOrder, Exif, Kind, Rgb8, Scene, orient};
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader};

fn scene() -> Scene {
    Scene::new(11, 96, 64).with_kind(Kind::Blobs)
}

/// FNV-1a, 64-bit.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn decoder(bytes: &[u8]) -> Box<dyn ImageDecoder + '_> {
    Box::new(ImageReader::new(Cursor::new(bytes)).with_guessed_format().unwrap().into_decoder().unwrap())
}

/// Every TIFF layout the writer supports.
fn tiff_layouts() -> Vec<(Colour, Depth)> {
    let mut v = Vec::new();
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            let ok = match colour {
                Colour::Palette => depth == Depth::Eight,
                Colour::Cmyk => depth != Depth::Float,
                Colour::Grey | Colour::Rgb => true,
            };
            if ok {
                v.push((colour, depth));
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Determinism

#[test]
fn every_writer_gives_the_same_bytes_for_the_same_seed() {
    let s = scene();
    let other = Scene { seed: 12, ..s };
    let exif = Exif { orientation: 6, thumbnail: Some(32), ..Exif::default() };
    let profile = icc::rgb(Space::DisplayP3, Version::V4);
    type Writer = Box<dyn Fn(&Scene) -> Vec<u8>>;
    let mut writers: Vec<(String, Writer)> = Vec::new();
    for (colour, depth) in tiff_layouts() {
        for compression in Compression::ALL {
            let opts = Tiff { colour, depth, compression, orientation: 3, thumbnail: Some(24), icc: Some(profile.clone()), ..Tiff::default() };
            writers.push((format!("TIFF {colour:?} {depth:?} {compression:?}"), Box::new(move |s| formats::tiff(s, &opts).unwrap())));
        }
    }
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            if (colour == Colour::Palette && depth != Depth::Eight) || (colour == Colour::Cmyk && depth == Depth::Float) {
                continue;
            }
            for rle in [false, true] {
                let opts = Psd { colour, depth, rle, thumbnail: Some(32), icc: Some(profile.clone()) };
                writers.push((format!("PSD {colour:?} {depth:?} rle {rle}"), Box::new(move |s| formats::psd(s, &opts).unwrap())));
            }
        }
    }
    let webp = Webp { alpha: true, exif: Some(exif.clone()), icc: Some(profile.clone()) };
    writers.push(("WebP".into(), Box::new(move |s| formats::webp(s, &webp).unwrap())));
    writers.push(("GIF".into(), Box::new(|s| formats::gif(s, 3).unwrap())));
    for colour in [Colour::Rgb, Colour::Grey, Colour::Palette] {
        writers.push((format!("BMP {colour:?}"), Box::new(move |s| formats::bmp(s, colour).unwrap())));
    }
    let p = profile.clone();
    writers.push(("JPEG ICC".into(), Box::new(move |s| formats::jpeg_icc(s, 90, Some(&exif), &p).unwrap())));
    for (colour, depth, alpha) in [(Colour::Rgb, Depth::Sixteen, false), (Colour::Grey, Depth::Eight, true)] {
        writers.push((format!("JXL {colour:?} {depth:?}"), Box::new(move |s| formats::jxl(s, colour, depth, alpha).unwrap())));
    }
    for (name, write) in &writers {
        let a = write(&s);
        assert_eq!(a, write(&s), "{name}");
        assert_ne!(a, write(&other), "{name}: another seed, other bytes");
    }
    for space in Space::ALL {
        for version in Version::ALL {
            assert_eq!(icc::rgb(space, version), icc::rgb(space, version));
        }
    }
}

/// Our own writers (TIFF, PSD, ICC) give the same bytes on every platform and in every build
/// profile (CI runs this on Linux, Windows and macOS). A change here changes every such test file:
/// only on purpose, with the reason in the commit.
#[test]
fn golden_hashes_of_our_own_writers_hold_everywhere() {
    let s = Scene::new(2026, 48, 32).with_kind(Kind::Horizon);
    let got = [
        fnv(&formats::tiff(
            &s,
            &Tiff { colour: Colour::Rgb, depth: Depth::Float, compression: Compression::Lzw, orientation: 6, thumbnail: Some(16), ..Tiff::default() },
        )
        .unwrap()),
        fnv(&formats::tiff(
            &s,
            &Tiff { colour: Colour::Cmyk, depth: Depth::Sixteen, compression: Compression::PackBits, byte_order: ByteOrder::Big, ..Tiff::default() },
        )
        .unwrap()),
        fnv(&formats::tiff(&s, &Tiff { colour: Colour::Palette, compression: Compression::Deflate, ..Tiff::default() }).unwrap()),
        fnv(&formats::psd(&s, &Psd { colour: Colour::Grey, depth: Depth::Sixteen, rle: true, ..Psd::default() }).unwrap()),
        fnv(&icc::rgb(Space::Srgb, Version::V2)),
        fnv(&icc::rgb(Space::Rec2020, Version::V4)),
        fnv(&icc::rgb(Space::ProPhoto, Version::V2)),
        fnv(&icc::grey(Version::V4)),
    ];
    assert_eq!(got, GOLDEN, "{got:#x?}");
}

/// TIFF RGB float LZW orientation 6 with a thumbnail; TIFF CMYK 16-bit PackBits big-endian; TIFF
/// palette Deflate; PSD grey 16-bit RLE; ICC sRGB v2, Rec. 2020 v4, ProPhoto v2, grey v4
/// (2026-10-11, debug and release).
const GOLDEN: [u64; 8] = [
    0x8761_c335_b6de_d20f,
    0x6521_0ab6_86ba_0a34,
    0x4745_8b58_9dac_86ff,
    0x379f_4e4d_cfe9_838b,
    0x2f45_a783_9a9a_e9a0,
    0x5f9d_ce57_8f5f_cf34,
    0xe78d_f965_b5ca_5583,
    0x3e74_7aed_d24a_41e6,
];

// ---------------------------------------------------------------------------------------------
// TIFF

/// Decodes the first IFD with the `tiff` crate: raw samples, as stored.
fn tiff_samples(bytes: &[u8]) -> (u32, u32, tiff::decoder::DecodingResult) {
    let mut d = tiff::decoder::Decoder::new(Cursor::new(bytes)).unwrap();
    let (w, h) = d.dimensions().unwrap();
    (w, h, d.read_image().unwrap())
}

/// A palette TIFF relabelled as 8-bit grey (Photometric 3 → 1), so that the `tiff` crate, which
/// refuses palette images, decompresses its indices; the colour map is checked on its own.
fn palette_as_grey(bytes: &[u8]) -> Vec<u8> {
    let le = &bytes[..2] == b"II";
    let u16_at = |i: usize| if le { u16::from_le_bytes([bytes[i], bytes[i + 1]]) } else { u16::from_be_bytes([bytes[i], bytes[i + 1]]) };
    let u32_at = |i: usize| {
        let b = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
        if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
    };
    let ifd = u32_at(4) as usize;
    let entry = (0..u16_at(ifd) as usize).map(|k| ifd + 2 + 12 * k).find(|&e| u16_at(e) == 262).unwrap();
    assert_eq!(u16_at(entry + 8), 3, "Photometric: palette");
    let mut out = bytes.to_vec();
    out[entry + 8..entry + 10].copy_from_slice(&if le { 1u16.to_le_bytes() } else { 1u16.to_be_bytes() });
    out
}

fn same_samples(got: &tiff::decoder::DecodingResult, want: &Samples) -> bool {
    use tiff::decoder::DecodingResult as D;
    match (got, &want.data) {
        (D::U8(a), SampleData::U8(b)) => a == b,
        (D::U16(a), SampleData::U16(b)) => a == b,
        (D::F32(a), SampleData::F32(b)) => a.iter().map(|v| v.to_bits()).eq(b.iter().map(|v| v.to_bits())),
        _ => false,
    }
}

#[test]
fn tiff_every_layout_compression_and_byte_order_reads_back_exactly() {
    let s = scene();
    let upright = s.render().unwrap();
    for (colour, depth) in tiff_layouts() {
        let want = formats::samples(&upright, colour, depth).unwrap();
        for compression in Compression::ALL {
            for byte_order in [ByteOrder::Little, ByteOrder::Big] {
                let name = format!("{colour:?} {depth:?} {compression:?} {byte_order:?}");
                let opts = Tiff { colour, depth, compression, byte_order, rows_per_strip: Some(7), ..Tiff::default() };
                let bytes = formats::tiff(&s, &opts).unwrap();
                assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Tiff, "{name}");
                assert_eq!(&bytes[..2], if byte_order == ByteOrder::Little { b"II" } else { b"MM" });
                let (w, h, got) = tiff_samples(&if colour == Colour::Palette { palette_as_grey(&bytes) } else { bytes.clone() });
                assert_eq!((w, h), (96, 64), "{name}");
                assert!(same_samples(&got, &want), "{name}: samples differ");

                // `image` reads every layout but palette and grey float, which it refuses.
                let decoded = ImageReader::new(Cursor::new(&bytes)).with_guessed_format().unwrap().decode();
                let refused = colour == Colour::Palette || (colour == Colour::Grey && depth == Depth::Float);
                assert_eq!(decoded.is_err(), refused, "{name}: {:?}", decoded.as_ref().err());
                let Ok(img) = decoded else { continue };
                let rgb = img.to_rgb8().into_raw();
                let tolerance = match colour {
                    // Naive CMYK → RGB truncates; floats and 16-bit round to 8 bits.
                    Colour::Cmyk => 2,
                    _ => 1,
                };
                let expect = if colour == Colour::Grey {
                    let SampleData::U8(g) = formats::samples(&upright, Colour::Grey, Depth::Eight).unwrap().data else { unreachable!() };
                    g.iter().flat_map(|&v| [v, v, v]).collect()
                } else {
                    upright.data.clone()
                };
                let worst = rgb.iter().zip(&expect).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                assert!(worst <= tolerance, "{name}: worst difference {worst}");
            }
        }
    }
}

#[test]
fn tiff_palette_has_its_colour_map() {
    let bytes =
        palette_as_grey(&formats::tiff(&scene(), &Tiff { colour: Colour::Palette, compression: Compression::Lzw, ..Tiff::default() }).unwrap());
    let mut d = tiff::decoder::Decoder::new(Cursor::new(&bytes)).unwrap();
    let map = d.get_tag_u16_vec(tiff::tags::Tag::ColorMap).unwrap();
    assert_eq!(map.len(), 768);
    let p = formats::palette();
    assert!((0..256).all(|i| [map[i], map[256 + i], map[512 + i]] == p[i].map(|v| u16::from(v) * 257)));
    let tiff::decoder::DecodingResult::U8(indices) = d.read_image().unwrap() else { panic!("8-bit indices") };
    let shown: Vec<u8> = indices.iter().flat_map(|&i| p[usize::from(i)]).collect();
    assert_eq!(shown, formats::quantized(&scene().render().unwrap()).data);
}

#[test]
fn tiff_orientation_thumbnail_ifd_and_icc_profile() {
    let s = scene();
    let upright = s.render().unwrap();
    let profile = icc::rgb(Space::AdobeRgb, Version::V2);
    for o in 1..=8u8 {
        let opts = Tiff { orientation: o, thumbnail: Some(40), icc: Some(profile.clone()), compression: Compression::Deflate, ..Tiff::default() };
        let bytes = formats::tiff(&s, &opts).unwrap();
        let mut dec = decoder(&bytes);
        assert_eq!(dec.orientation().unwrap().to_exif(), o);
        assert_eq!(dec.icc_profile().unwrap().as_deref(), Some(&profile[..]), "orientation {o}");
        let img = image::DynamicImage::from_decoder(dec).unwrap().to_rgb8();
        let stored = Rgb8 { width: img.width(), height: img.height(), data: img.into_raw() };
        assert_eq!(orient::display(&stored, o).unwrap(), upright, "orientation {o} shows the scene upright");

        // IFD1: the thumbnail, stored the same way, marked as a reduced-resolution image.
        let mut d = tiff::decoder::Decoder::new(Cursor::new(&bytes)).unwrap();
        assert!(d.more_images());
        d.next_image().unwrap();
        assert_eq!(d.get_tag_u32(tiff::tags::Tag::NewSubfileType).unwrap(), 1);
        let (tw, th) = d.dimensions().unwrap();
        assert_eq!(tw.max(th), 40);
        let tiff::decoder::DecodingResult::U8(thumb) = d.read_image().unwrap() else { panic!("8-bit thumbnail") };
        assert_eq!(thumb, stored.downscale(40).unwrap().data);
        assert!(!d.more_images());
    }
}

#[test]
fn tiff_one_large_lzw_strip_survives_table_resets() {
    let s = Scene::new(4, 400, 300);
    let bytes = formats::tiff(&s, &Tiff { compression: Compression::Lzw, rows_per_strip: Some(300), ..Tiff::default() }).unwrap();
    let (_, _, got) = tiff_samples(&bytes);
    assert!(same_samples(&got, &formats::samples(&s.render().unwrap(), Colour::Rgb, Depth::Eight).unwrap()));
}

// ---------------------------------------------------------------------------------------------
// WebP, GIF, BMP, JPEG

#[test]
fn webp_lossless_with_alpha_exif_and_icc() {
    let s = scene();
    let upright = s.render().unwrap();
    let profile = icc::rgb(Space::Srgb, Version::V4);
    for o in [1u8, 6, 7] {
        let exif = Exif { orientation: o, thumbnail: Some(24), make: Some("Dalica".into()), ..Exif::default() };
        for alpha in [false, true] {
            let opts = Webp { alpha, exif: Some(exif.clone()), icc: Some(profile.clone()) };
            let bytes = formats::webp(&s, &opts).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::WebP);
            let mut dec = decoder(&bytes);
            assert_eq!(dec.icc_profile().unwrap().as_deref(), Some(&profile[..]));
            let raw = dec.exif_metadata().unwrap().expect("an EXIF chunk");
            assert_eq!(&raw[..4], b"II*\0");
            assert_eq!(dec.orientation().unwrap().to_exif(), o);
            let img = image::DynamicImage::from_decoder(dec).unwrap().to_rgba8();
            let (w, h) = img.dimensions();
            let px = img.into_raw();
            let stored = Rgb8 { width: w, height: h, data: px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect() };
            assert_eq!(orient::display(&stored, o).unwrap(), upright, "lossless, orientation {o}");
            let a: Vec<u8> = px.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
            assert_eq!(a, if alpha { formats::alpha(w, h) } else { vec![255; a.len()] });
        }
    }
    let plain = formats::webp(&s, &Webp::default()).unwrap();
    let mut dec = decoder(&plain);
    assert_eq!((dec.icc_profile().unwrap(), dec.exif_metadata().unwrap()), (None, None));
}

#[test]
fn gif_one_and_several_frames_keep_every_palette_colour() {
    let s = scene();
    for frames in [1u16, 4] {
        let bytes = formats::gif(&s, frames).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Gif);
        let dec = image::codecs::gif::GifDecoder::new(Cursor::new(&bytes)).unwrap();
        let got = dec.into_frames().collect_frames().unwrap();
        assert_eq!(got.len(), usize::from(frames));
        for (k, f) in got.iter().enumerate() {
            let want = formats::quantized(&Scene { seed: s.seed + k as u64, ..s }.render().unwrap());
            let rgb: Vec<u8> = f.buffer().pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
            assert_eq!(rgb, want.data, "frame {k}");
            assert_eq!(f.delay().numer_denom_ms(), (100, 1));
        }
    }
    assert!(formats::gif(&s, 0).is_err());
    assert!(formats::gif(&s, formats::codecs::MAX_FRAMES + 1).is_err());
}

#[test]
fn bmp_rgb_grey_and_indexed_read_back_exactly() {
    let s = scene();
    let upright = s.render().unwrap();
    let SampleData::U8(grey) = formats::samples(&upright, Colour::Grey, Depth::Eight).unwrap().data else { unreachable!() };
    for (colour, want) in [
        (Colour::Rgb, upright.data.clone()),
        (Colour::Grey, grey.iter().flat_map(|&v| [v, v, v]).collect()),
        (Colour::Palette, formats::quantized(&upright).data),
    ] {
        let bytes = formats::bmp(&s, colour).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Bmp);
        let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(img.into_raw(), want, "{colour:?}");
    }
    assert!(formats::bmp(&s, Colour::Cmyk).is_err());
}

#[test]
fn jpeg_carries_its_icc_profile_in_app2_even_across_segments() {
    let s = scene();
    let exif = Exif { orientation: 5, ..Exif::default() };
    // A real profile, and a 150 000-byte blob that needs three APP2 segments.
    let big: Vec<u8> = (0..150_000u32).map(|i| (i * 7 % 251) as u8).collect();
    for profile in [icc::rgb(Space::ProPhoto, Version::V4), big] {
        let bytes = formats::jpeg_icc(&s, 92, Some(&exif), &profile).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
        let segments = bytes.windows(12).filter(|w| w == b"ICC_PROFILE\0").count();
        assert_eq!(segments, profile.len().div_ceil(65_519));
        let mut dec = decoder(&bytes);
        assert_eq!(dec.icc_profile().unwrap(), Some(profile.clone()));
        assert_eq!(dec.orientation().unwrap().to_exif(), 5);
    }
}

// ---------------------------------------------------------------------------------------------
// PSD: a structural reader

struct PsdFile {
    channels: usize,
    height: usize,
    width: usize,
    depth: usize,
    mode: u16,
    colour_data: Vec<u8>,
    resources: Vec<(u16, Vec<u8>)>,
    /// One plane per channel, decompressed.
    planes: Vec<Vec<u8>>,
}

fn unpackbits(mut p: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some((&h, rest)) = p.split_first() {
        let h = h as i8;
        if h >= 0 {
            let n = h as usize + 1;
            out.extend(&rest[..n]);
            p = &rest[n..];
        } else if h != -128 {
            out.extend(std::iter::repeat_n(rest[0], (1 - h as isize) as usize));
            p = &rest[1..];
        } else {
            p = rest;
        }
    }
    out
}

fn read_psd(b: &[u8]) -> PsdFile {
    let u16_at = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    assert_eq!(&b[..4], b"8BPS");
    assert_eq!(u16_at(4), 1, "version 1");
    assert_eq!(&b[6..12], [0; 6]);
    let (channels, height, width, depth, mode) = (u16_at(12) as usize, u32_at(14), u32_at(18), u16_at(22) as usize, u16_at(24));
    let mut at = 26;
    let colour_len = u32_at(at);
    let colour_data = b[at + 4..at + 4 + colour_len].to_vec();
    at += 4 + colour_len;
    let res_len = u32_at(at);
    let res_end = at + 4 + res_len;
    let mut r = at + 4;
    let mut resources = Vec::new();
    while r < res_end {
        assert_eq!(&b[r..r + 4], b"8BIM");
        let id = u16_at(r + 4);
        let name_len = b[r + 6] as usize;
        let name_total = (1 + name_len).next_multiple_of(2);
        let size_at = r + 6 + name_total;
        let size = u32_at(size_at);
        resources.push((id, b[size_at + 4..size_at + 4 + size].to_vec()));
        r = size_at + 4 + size.next_multiple_of(2);
    }
    assert_eq!(r, res_end);
    at = res_end;
    let layers_len = u32_at(at);
    at += 4 + layers_len;
    let compression = u16_at(at);
    at += 2;
    let row_bytes = width * depth / 8;
    let planes = match compression {
        0 => {
            assert_eq!(b.len() - at, channels * height * row_bytes, "raw composite size");
            b[at..].chunks(height * row_bytes).map(<[u8]>::to_vec).collect()
        }
        1 => {
            let counts: Vec<usize> = (0..channels * height).map(|k| u16_at(at + 2 * k) as usize).collect();
            let mut p = at + 2 * channels * height;
            let mut planes = vec![Vec::new(); channels];
            for (k, n) in counts.iter().enumerate() {
                let row = unpackbits(&b[p..p + n]);
                assert_eq!(row.len(), row_bytes);
                planes[k / height].extend(row);
                p += n;
            }
            assert_eq!(p, b.len());
            planes
        }
        c => panic!("compression {c}"),
    };
    PsdFile { channels, height, width, depth, mode, colour_data, resources, planes }
}

#[test]
fn psd_every_mode_depth_and_compression_reads_back_structurally() {
    let s = scene();
    let upright = s.render().unwrap();
    let profile = icc::rgb(Space::Srgb, Version::V2);
    for colour in Colour::ALL {
        for depth in Depth::ALL {
            if (colour == Colour::Palette && depth != Depth::Eight) || (colour == Colour::Cmyk && depth == Depth::Float) {
                assert!(formats::psd(&s, &Psd { colour, depth, ..Psd::default() }).is_err());
                continue;
            }
            let want = formats::samples(&upright, colour, depth).unwrap();
            let mut raw_planes = None;
            for rle in [false, true] {
                let name = format!("{colour:?} {depth:?} rle {rle}");
                let opts = Psd { colour, depth, rle, thumbnail: Some(48), icc: Some(profile.clone()) };
                let f = read_psd(&formats::psd(&s, &opts).unwrap());
                assert_eq!((f.width, f.height, f.channels, f.depth), (96, 64, colour.channels(), depth.bytes() * 8), "{name}");
                assert_eq!(f.mode, [1, 3, 4, 2][Colour::ALL.iter().position(|c| *c == colour).unwrap()], "{name}");
                // Interleave the planes back and compare with the samples (CMYK stored inverted).
                let n = f.width * f.height;
                let got: Vec<u32> = (0..n * f.channels)
                    .map(|i| {
                        let (px, c) = (i / f.channels, i % f.channels);
                        let plane = &f.planes[c];
                        let (v, max) = match depth {
                            Depth::Eight => (u32::from(plane[px]), 255),
                            Depth::Sixteen => (u32::from(u16::from_be_bytes([plane[2 * px], plane[2 * px + 1]])), 65_535),
                            Depth::Float => (u32::from_be_bytes([plane[4 * px], plane[4 * px + 1], plane[4 * px + 2], plane[4 * px + 3]]), 0),
                        };
                        if colour == Colour::Cmyk { max - v } else { v }
                    })
                    .collect();
                let expect: Vec<u32> = match &want.data {
                    SampleData::U8(v) => v.iter().map(|&x| u32::from(x)).collect(),
                    SampleData::U16(v) => v.iter().map(|&x| u32::from(x)).collect(),
                    SampleData::F32(v) => v.iter().map(|x| x.to_bits()).collect(),
                };
                assert_eq!(got, expect, "{name}");
                match &raw_planes {
                    None => raw_planes = Some(f.planes.clone()),
                    Some(p) => assert_eq!(&f.planes, p, "{name}: RLE and raw hold the same composite"),
                }
                // Indexed: the palette in the colour mode data, 256 reds, greens, blues.
                if colour == Colour::Palette {
                    let p = formats::palette();
                    assert_eq!(f.colour_data, (0..3).flat_map(|c| p.iter().map(move |rgb| rgb[c])).collect::<Vec<u8>>());
                } else {
                    assert!(f.colour_data.is_empty());
                }
                // Resources: the JPEG thumbnail (1036) and the ICC profile (1039).
                let thumb = &f.resources.iter().find(|r| r.0 == formats::psd::RESOURCE_THUMBNAIL).expect("a thumbnail").1;
                let tw = u32::from_be_bytes([thumb[4], thumb[5], thumb[6], thumb[7]]);
                let th = u32::from_be_bytes([thumb[8], thumb[9], thumb[10], thumb[11]]);
                assert_eq!((tw, th), (48, 32));
                assert_eq!(u32::from_be_bytes([thumb[0], thumb[1], thumb[2], thumb[3]]), 1, "JFIF");
                let jpeg = image::load_from_memory_with_format(&thumb[28..], ImageFormat::Jpeg).unwrap();
                assert_eq!((jpeg.width(), jpeg.height()), (48, 32));
                let icc_res = &f.resources.iter().find(|r| r.0 == formats::psd::RESOURCE_ICC).expect("a profile").1;
                assert_eq!(icc_res, &profile);
            }
        }
    }
    // Without resources the section is empty.
    let f = read_psd(&formats::psd(&s, &Psd::default()).unwrap());
    assert!(f.resources.is_empty());
}

// ---------------------------------------------------------------------------------------------
// ICC

fn moxcms_builtin(space: Space) -> moxcms::ColorProfile {
    match space {
        Space::Srgb => moxcms::ColorProfile::new_srgb(),
        Space::DisplayP3 => moxcms::ColorProfile::new_display_p3(),
        Space::AdobeRgb => moxcms::ColorProfile::new_adobe_rgb(),
        Space::ProPhoto => moxcms::ColorProfile::new_pro_photo_rgb(),
        Space::Rec2020 => moxcms::ColorProfile::new_bt2020(),
    }
}

/// `pixels` (RGB) converted from `from` to sRGB by `moxcms`.
fn to_srgb(from: &moxcms::ColorProfile, pixels: &[u8]) -> Vec<u8> {
    let srgb = moxcms::ColorProfile::new_srgb();
    let t = from.create_transform_8bit(moxcms::Layout::Rgb, &srgb, moxcms::Layout::Rgb, moxcms::TransformOptions::default()).unwrap();
    let mut out = vec![0; pixels.len()];
    t.transform(pixels, &mut out).unwrap();
    out
}

#[test]
fn icc_profiles_parse_with_moxcms_and_match_its_own_definitions() {
    // Primaries, white, mid-tones and dark tones, through every curve segment.
    let probe: Vec<u8> =
        [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128], [200, 120, 40], [10, 6, 3], [30, 200, 220]].concat();
    for space in Space::ALL {
        let builtin = moxcms_builtin(space);
        let reference = to_srgb(&builtin, &probe);
        for version in Version::ALL {
            let bytes = icc::rgb(space, version);
            assert_eq!(&bytes[36..40], b"acsp");
            let p = moxcms::ColorProfile::new_from_slice(&bytes).unwrap_or_else(|e| panic!("{space:?} {version:?}: {e:?}"));
            assert_eq!(p.color_space, moxcms::DataColorSpace::Rgb);
            let want = if version == Version::V2 { moxcms::ProfileVersion::V2_1 } else { moxcms::ProfileVersion::V4_3 };
            assert_eq!(p.version(), want, "{space:?} {version:?}");
            for (ours, theirs) in
                [(p.red_colorant, builtin.red_colorant), (p.green_colorant, builtin.green_colorant), (p.blue_colorant, builtin.blue_colorant)]
            {
                for (a, b) in [(ours.x, theirs.x), (ours.y, theirs.y), (ours.z, theirs.z)] {
                    assert!((a - b).abs() < 2e-3, "{space:?} {version:?}: colorant {a} vs {b}");
                }
            }
            assert!(p.red_trc.is_some() && p.green_trc.is_some() && p.blue_trc.is_some());
            let ours = to_srgb(&p, &probe);
            let worst = ours.iter().zip(&reference).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 2, "{space:?} {version:?}: converts {ours:?}, moxcms's own {reference:?}");
        }
    }
}

#[test]
fn icc_grey_profiles_parse_and_carry_the_srgb_curve() {
    for version in Version::ALL {
        let p = moxcms::ColorProfile::new_from_slice(&icc::grey(version)).unwrap();
        assert_eq!(p.color_space, moxcms::DataColorSpace::Gray);
        assert!(p.gray_trc.is_some());
        let srgb = moxcms::ColorProfile::new_srgb();
        let t = p.create_transform_8bit(moxcms::Layout::Gray, &srgb, moxcms::Layout::Rgb, moxcms::TransformOptions::default()).unwrap();
        let mut out = vec![0; 9];
        t.transform(&[0, 128, 255], &mut out).unwrap();
        for (v, px) in [0u8, 128, 255].iter().zip(out.chunks(3)) {
            assert!(px.iter().all(|c| c.abs_diff(*v) <= 2), "{version:?}: grey {v} → {px:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// JPEG XL

#[test]
fn jxl_lossless_reads_back_through_jxl_oxide() {
    let s = scene();
    let upright = s.render().unwrap();
    for colour in [Colour::Grey, Colour::Rgb] {
        for depth in [Depth::Eight, Depth::Sixteen] {
            for alpha in [false, true] {
                let name = format!("{colour:?} {depth:?} alpha {alpha}");
                if depth == Depth::Sixteen && alpha {
                    assert!(formats::jxl(&s, colour, depth, alpha).is_err(), "{name}: refused (the encoder mislabels 16-bit alpha)");
                    continue;
                }
                let bytes = formats::jxl(&s, colour, depth, alpha).unwrap();
                assert_eq!(&bytes[..2], [0xff, 0x0a], "{name}: a bare codestream");
                let img = jxl_oxide::JxlImage::builder().read(Cursor::new(&bytes)).unwrap();
                assert_eq!((img.width(), img.height()), (96, 64), "{name}");
                let fb = img.render_frame(0).unwrap().image_all_channels();
                let channels = colour.channels() + usize::from(alpha);
                assert_eq!(fb.channels(), channels, "{name}");
                let max = if depth == Depth::Eight { 255.0 } else { 65_535.0 };
                let got: Vec<u32> = fb.buf().iter().map(|v| (v * max + 0.5) as u32).collect();
                let want = formats::samples(&upright, colour, depth).unwrap();
                let a = formats::alpha(96, 64);
                let base: Vec<u32> = match &want.data {
                    SampleData::U8(v) => v.iter().map(|&x| u32::from(x)).collect(),
                    SampleData::U16(v) => v.iter().map(|&x| u32::from(x)).collect(),
                    SampleData::F32(_) => unreachable!(),
                };
                let expect: Vec<u32> = if alpha {
                    base.chunks(colour.channels()).zip(&a).flat_map(|(px, &al)| px.iter().copied().chain([u32::from(al)])).collect()
                } else {
                    base
                };
                let first = got.iter().zip(&expect).position(|(a, b)| a != b);
                assert_eq!(
                    first,
                    None,
                    "{name}: first difference at sample {first:?} of {}: {:?} vs {:?}",
                    got.len(),
                    first.map(|i| &got[i.saturating_sub(4)..i + 6]),
                    first.map(|i| &expect[i.saturating_sub(4)..i + 6])
                );
                assert_eq!(got.len(), expect.len(), "{name}");
            }
        }
    }
    assert!(formats::jxl(&Scene::new(1, 1, 8), Colour::Rgb, Depth::Eight, false).is_err());
    assert!(formats::jxl(&s, Colour::Cmyk, Depth::Eight, false).is_err());
    assert!(formats::jxl(&s, Colour::Rgb, Depth::Float, false).is_err());
}

// ---------------------------------------------------------------------------------------------
// Edges

#[test]
fn tiny_and_odd_sizes_write_and_read_back() {
    for (w, h) in [(1, 1), (2, 3), (17, 1), (1, 9), (5, 5)] {
        let s = Scene::new(3, w, h);
        let upright = s.render().unwrap();
        for compression in Compression::ALL {
            let bytes = formats::tiff(&s, &Tiff { compression, thumbnail: Some(1), ..Tiff::default() }).unwrap();
            let (tw, th, got) = tiff_samples(&bytes);
            assert_eq!((tw, th), (w, h));
            assert!(same_samples(&got, &formats::samples(&upright, Colour::Rgb, Depth::Eight).unwrap()), "{w}x{h} {compression:?}");
        }
        for rle in [false, true] {
            let f = read_psd(&formats::psd(&s, &Psd { rle, thumbnail: Some(4), ..Psd::default() }).unwrap());
            assert_eq!((f.width, f.height), (w as usize, h as usize));
        }
        let webp = formats::webp(&s, &Webp { alpha: true, ..Webp::default() }).unwrap();
        assert_eq!(image::load_from_memory(&webp).unwrap().to_rgb8().into_raw(), upright.data, "{w}x{h} WebP");
        let gif = formats::gif(&s, 2).unwrap();
        assert_eq!(image::load_from_memory(&gif).unwrap().to_rgb8().into_raw(), formats::quantized(&upright).data, "{w}x{h} GIF");
        let bmp = formats::bmp(&s, Colour::Rgb).unwrap();
        assert_eq!(image::load_from_memory(&bmp).unwrap().to_rgb8().into_raw(), upright.data, "{w}x{h} BMP");
        let jpeg = formats::jpeg_icc(&s, 90, None, &icc::grey(Version::V2)).unwrap();
        assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), w);
        let jxl = formats::jxl(&s, Colour::Rgb, Depth::Eight, false);
        assert_eq!(jxl.is_ok(), w >= 2 && h >= 2, "{w}x{h} JPEG XL");
    }
}

#[test]
fn sizes_formats_cannot_hold_are_refused_without_a_panic() {
    // WebP holds at most 16 384 pixels on a side; GIF's frames count is bounded; scenes over the
    // limits fail before any encoder runs.
    assert!(formats::webp(&Scene::new(1, 16_385, 1), &Webp::default()).is_err());
    assert!(formats::psd(&Scene::new(1, 1, 30_001), &Psd::default()).is_err());
    for bad in [Scene::new(1, 0, 4), Scene::new(1, 4, 0)] {
        assert!(formats::tiff(&bad, &Tiff::default()).is_err());
        assert!(formats::psd(&bad, &Psd::default()).is_err());
        assert!(formats::webp(&bad, &Webp::default()).is_err());
        assert!(formats::gif(&bad, 1).is_err());
        assert!(formats::bmp(&bad, Colour::Rgb).is_err());
        assert!(formats::jpeg_icc(&bad, 90, None, &[]).is_err());
        assert!(formats::jxl(&bad, Colour::Rgb, Depth::Eight, false).is_err());
    }
    let bad_exif = Exif { orientation: 0, ..Exif::default() };
    assert!(formats::webp(&scene(), &Webp { exif: Some(bad_exif.clone()), ..Webp::default() }).is_err());
    assert!(formats::jpeg_icc(&scene(), 90, Some(&bad_exif), &[]).is_err());
}
