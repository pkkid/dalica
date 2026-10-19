//! Videos (`formats.md` §1): MP4, M4V, QuickTime and Matroska files built here from the documented
//! layouts (ISO/IEC 14496-12 boxes, QuickTime atoms, RFC 8794 EBML and RFC 9559 Matroska
//! elements) are recognised by content and probed for their facts; still-image brands never
//! sniff as videos; cut and hostile containers are refused or bounded, never a panic.

use crate::common::cases;

use dalica_image::formats::{self, video};
use dalica_image::sniff::VideoKind;
use dalica_image::{DecodeOptions, Error, Format, Limits, RawKind, decode, decode_as, decode_thumbnail, read_header, sniff};
use proptest::prelude::*;

// ---------------------------------------------------------------------------------------------
// ISO BMFF writer

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let mut p = major.to_vec();
    p.extend(0x200u32.to_be_bytes());
    for c in compatible {
        p.extend(*c);
    }
    bx(b"ftyp", &p)
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];
/// A quarter turn clockwise (x' = −y, y' = x).
const QUARTER: [i32; 9] = [0, 0x1_0000, 0, -0x1_0000, 0, 0, 0, 0, 0x4000_0000];

fn matrix(m: [i32; 9]) -> Vec<u8> {
    m.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// `mvhd` or `mdhd` times: version 0 (32-bit) or 1 (64-bit).
fn times(v1: bool, created: u64, timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = vec![u8::from(v1), 0, 0, 0];
    if v1 {
        p.extend(created.to_be_bytes());
        p.extend(created.to_be_bytes());
        p.extend(timescale.to_be_bytes());
        p.extend(duration.to_be_bytes());
    } else {
        p.extend(u32::try_from(created).unwrap().to_be_bytes());
        p.extend(u32::try_from(created).unwrap().to_be_bytes());
        p.extend(timescale.to_be_bytes());
        p.extend(u32::try_from(duration).unwrap_or(u32::MAX).to_be_bytes());
    }
    p
}

fn mvhd(v1: bool, created: u64, timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = times(v1, created, timescale, duration);
    p.extend(0x1_0000u32.to_be_bytes()); // rate
    p.extend(0x100u16.to_be_bytes()); // volume
    p.extend([0; 10]);
    p.extend(matrix(IDENTITY));
    p.extend([0; 24]);
    p.extend(3u32.to_be_bytes()); // next track id
    bx(b"mvhd", &p)
}

fn tkhd(v1: bool, m: [i32; 9], width: u32, height: u32) -> Vec<u8> {
    let mut p = vec![u8::from(v1), 0, 0, 7];
    let n = if v1 { 8 } else { 4 };
    p.extend(vec![0; 2 * n]); // creation, modification
    p.extend(1u32.to_be_bytes()); // track id
    p.extend([0; 4]);
    p.extend(vec![0; n]); // duration
    p.extend([0; 8]);
    p.extend([0; 8]); // layer, group, volume, reserved
    p.extend(matrix(m));
    p.extend((width << 16).to_be_bytes());
    p.extend((height << 16).to_be_bytes());
    bx(b"tkhd", &p)
}

fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0; 8];
    p.extend(handler);
    p.extend([0; 12]);
    p.push(0);
    bx(b"hdlr", &p)
}

/// A sample description with one entry; a visual entry holds the frame size.
fn stsd(code: &[u8; 4], visual: Option<(u16, u16)>) -> Vec<u8> {
    let mut entry = vec![0; 6];
    entry.extend(1u16.to_be_bytes());
    if let Some((w, h)) = visual {
        entry.extend([0; 16]);
        entry.extend(w.to_be_bytes());
        entry.extend(h.to_be_bytes());
        entry.extend([0; 50]);
    } else {
        entry.extend([0; 20]);
    }
    let mut p = vec![0, 0, 0, 0];
    p.extend(1u32.to_be_bytes());
    p.extend(bx(code, &entry));
    bx(b"stsd", &p)
}

/// A sample size box of `count` samples of one size (no table).
fn stsz(count: u32) -> Vec<u8> {
    let mut p = vec![0; 4];
    p.extend(1000u32.to_be_bytes());
    p.extend(count.to_be_bytes());
    bx(b"stsz", &p)
}

struct Track {
    handler: &'static [u8; 4],
    code: &'static [u8; 4],
    matrix: [i32; 9],
    size: (u32, u32),
    timescale: u32,
    duration: u64,
    samples: u32,
}

fn trak(v1: bool, t: &Track) -> Vec<u8> {
    let visual = (t.handler == b"vide").then(|| (u16::try_from(t.size.0).unwrap(), u16::try_from(t.size.1).unwrap()));
    let stbl = bx(b"stbl", &[stsd(t.code, visual), stsz(t.samples)].concat());
    let minf = bx(b"minf", &stbl);
    let mut mdhd = times(v1, 0, t.timescale, t.duration);
    mdhd.extend([0; 4]);
    let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), hdlr(t.handler), minf].concat());
    bx(b"trak", &[tkhd(v1, t.matrix, t.size.0, t.size.1), mdia].concat())
}

/// QuickTime user data text: a length, a language, the text.
fn xyz(text: &str) -> Vec<u8> {
    let mut p = u16::try_from(text.len()).unwrap().to_be_bytes().to_vec();
    p.extend(0x15c7u16.to_be_bytes());
    p.extend(text.as_bytes());
    bx(b"udta", &bx(b"\xa9xyz", &p))
}

/// QuickTime metadata (`meta` without version and flags): an unrelated key, then the location.
fn location_keys(text: &str) -> Vec<u8> {
    let key = |name: &[u8]| {
        let mut k = u32::try_from(name.len() + 8).unwrap().to_be_bytes().to_vec();
        k.extend(b"mdta");
        k.extend(name);
        k
    };
    let mut keys = vec![0; 4];
    keys.extend(2u32.to_be_bytes());
    keys.extend(key(b"com.apple.quicktime.make"));
    keys.extend(key(b"com.apple.quicktime.location.ISO6709"));
    let value = |kind: u32, v: &[u8]| {
        let mut d = 1u32.to_be_bytes().to_vec();
        d.extend(0u32.to_be_bytes());
        d.extend(v);
        bx(&kind.to_be_bytes(), &bx(b"data", &d))
    };
    let ilst = bx(b"ilst", &[value(1, b"Dalica"), value(2, text.as_bytes())].concat());
    bx(b"meta", &[hdlr(b"mdta"), bx(b"keys", &keys), ilst].concat())
}

const MP4_CREATED: u64 = 3_863_446_930; // 2026-06-04T19:42:10 UTC, from 1904
const MOV_CREATED: u64 = 3_849_409_800; // 2025-12-24T08:30:00 UTC, from 1904

fn video_track(code: &'static [u8; 4], m: [i32; 9], size: (u32, u32), timescale: u32, duration: u64, samples: u32) -> Track {
    Track { handler: b"vide", code, matrix: m, size, timescale, duration, samples }
}

fn sound_track(code: &'static [u8; 4], duration: u64) -> Track {
    Track { handler: b"soun", code, matrix: IDENTITY, size: (0, 0), timescale: 48_000, duration, samples: 1 }
}

/// An MP4 from a phone: `ftyp`, the media data, then the movie (version 0 headers).
fn mp4() -> Vec<u8> {
    // 12.345 s at 30000/1001 frames per second: 370 frames of 1001 ticks.
    let moov = bx(
        b"moov",
        &[
            mvhd(false, MP4_CREATED, 1000, 12_345),
            trak(false, &video_track(b"avc1", IDENTITY, (1920, 1080), 30_000, 370_370, 370)),
            trak(false, &sound_track(b"mp4a", 592_560)),
            xyz("+37.7749-122.4194+010.000/"),
        ]
        .concat(),
    );
    [ftyp(b"mp42", &[b"mp42", b"isom"]), bx(b"mdat", &[0x5a; 300]), moov].concat()
}

/// An M4V (version 1 headers, 64-bit times), without sound.
fn m4v() -> Vec<u8> {
    let moov = bx(
        b"moov",
        &[mvhd(true, MP4_CREATED, 600, 3_000), trak(true, &video_track(b"hvc1", IDENTITY, (1280, 720), 600, 3_000, 125)), xyz("-33.8688+151.2093/")]
            .concat(),
    );
    [ftyp(b"M4V ", &[b"M4V ", b"M4A ", b"mp42", b"isom"]), bx(b"mdat", &[1; 64]), moov].concat()
}

/// A QuickTime movie from a phone held upright: the frames stored landscape and turned a quarter
/// by the track's matrix; the location in the QuickTime metadata keys; the movie before the data.
fn mov() -> Vec<u8> {
    let moov = bx(
        b"moov",
        &[
            mvhd(false, MOV_CREATED, 600, 4_320),
            trak(false, &video_track(b"hvc1", QUARTER, (1920, 1080), 600, 4_320, 216)),
            trak(false, &sound_track(b"lpcm", 345_680)),
            location_keys("+38.7139-009.1394+050.000/"),
        ]
        .concat(),
    );
    [ftyp(b"qt  ", &[b"qt  "]), bx(b"wide", &[]), moov, bx(b"mdat", &[2; 128])].concat()
}

// ---------------------------------------------------------------------------------------------
// EBML / Matroska writer

/// An EBML data size, in its shortest form.
fn size(n: usize) -> Vec<u8> {
    let n = u64::try_from(n).unwrap();
    let len = (1..=8).find(|&l| n < (1u64 << (7 * l)) - 1).unwrap();
    let marked = n | (1u64 << (7 * len));
    marked.to_be_bytes()[8 - len as usize..].to_vec()
}

fn id_bytes(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    let skip = b.iter().position(|&x| x != 0).unwrap();
    b[skip..].to_vec()
}

fn el(id: u32, data: &[u8]) -> Vec<u8> {
    [id_bytes(id), size(data.len()), data.to_vec()].concat()
}

/// An element of unknown size (eight bytes, every value bit set).
fn el_unknown(id: u32, data: &[u8]) -> Vec<u8> {
    [id_bytes(id), vec![0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], data.to_vec()].concat()
}

fn uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

fn float(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

fn string(id: u32, s: &str) -> Vec<u8> {
    el(id, s.as_bytes())
}

const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114d_9b74;
const INFO: u32 = 0x1549_a966;
const TRACKS: u32 = 0x1654_ae6b;
const TAGS: u32 = 0x1254_c367;
const CLUSTER: u32 = 0x1f43_b675;
const VOID: u32 = 0xec;

fn ebml_header(doc: &str) -> Vec<u8> {
    el(
        0x1a45_dfa3,
        &[uint(0x4286, 1), uint(0x42f7, 1), uint(0x42f2, 4), uint(0x42f3, 8), string(0x4282, doc), uint(0x4287, 4), uint(0x4285, 2)].concat(),
    )
}

fn mkv_info() -> Vec<u8> {
    // 123,456 ticks of 0.1 ms: 12,345.6 ms. DateUTC: 2026-06-04T19:42:10 UTC, from 2001.
    el(
        INFO,
        &[uint(0x2a_d7b1, 100_000), float(0x4489, 123_456.0), el(0x4461, &802_294_930_000_000_000i64.to_be_bytes()), string(0x4d80, "Dalica tests")]
            .concat(),
    )
}

fn mkv_tracks() -> Vec<u8> {
    el(TRACKS, &mkv_track_entries())
}

/// A 4K HEVC video track, turned by its projection's roll, and an Opus sound track.
fn mkv_track_entries() -> Vec<u8> {
    let projection = el(0x7670, &[uint(0x7671, 0), float(0x7675, 90.0)].concat());
    let video = el(0xe0, &[uint(0xb0, 3840), uint(0xba, 2160), projection].concat());
    let v = el(0xae, &[uint(0xd7, 1), uint(0x83, 1), string(0x86, "V_MPEGH/ISO/HEVC"), uint(0x23_e383, 33_366_666), video].concat());
    let a = el(0xae, &[uint(0xd7, 2), uint(0x83, 2), string(0x86, "A_OPUS"), el(0xe1, &float(0xb5, 48_000.0))].concat());
    [v, a].concat()
}

fn mkv_tags() -> Vec<u8> {
    let simple = |name: &str, value: &str| el(0x67c8, &[string(0x45a3, name), string(0x4487, value)].concat());
    let tag = el(0x7373, &[el(0x63c0, &uint(0x68ca, 50)), simple("ENCODER", "Dalica"), simple("RECORDING_LOCATION", "+38.7139-009.1394/")].concat());
    el(TAGS, &tag)
}

/// A seek head pointing at `targets` (id, position in the segment's data), positions in 8 bytes.
fn seek_head(targets: &[(u32, usize)]) -> Vec<u8> {
    let seeks: Vec<u8> = targets.iter().flat_map(|&(id, pos)| el(0x4dbb, &[el(0x53ab, &id_bytes(id)), uint(0x53ac, pos as u64)].concat())).collect();
    el(SEEK_HEAD, &seeks)
}

/// A Matroska file as muxers write it: a seek head, the info and tracks, two clusters, then the
/// tags, reached through the seek head.
fn mkv_with(segment_unknown: bool, cluster_unknown: bool) -> Vec<u8> {
    let (info, tracks, tags) = (mkv_info(), mkv_tracks(), mkv_tags());
    let cluster = el(CLUSTER, &[uint(0xe7, 0), el(0xa3, &[0x81, 0, 0, 0x80, 1, 2, 3, 4])].concat());
    let clusters = if cluster_unknown {
        // An unknown-size cluster runs to the end: the tags must come first then.
        [cluster.clone(), el_unknown(CLUSTER, &[uint(0xe7, 1), el(0xa3, &[0x81, 0, 0, 0x80, 5, 6])].concat())].concat()
    } else {
        [cluster.clone(), cluster].concat()
    };
    let head_len = seek_head(&[(INFO, 0), (TRACKS, 0), (TAGS, 0)]).len();
    let info_at = head_len;
    let tracks_at = info_at + info.len();
    let (body, tags_at) = if cluster_unknown {
        let tags_at = tracks_at + tracks.len();
        ([info.clone(), tracks.clone(), tags.clone(), clusters].concat(), tags_at)
    } else {
        let tags_at = tracks_at + tracks.len() + clusters.len();
        ([info.clone(), tracks.clone(), clusters, tags.clone()].concat(), tags_at)
    };
    let data = [seek_head(&[(INFO, info_at), (TRACKS, tracks_at), (TAGS, tags_at)]), body].concat();
    let segment = if segment_unknown { el_unknown(SEGMENT, &data) } else { el(SEGMENT, &data) };
    [ebml_header("matroska"), segment].concat()
}

fn mkv() -> Vec<u8> {
    mkv_with(false, false)
}

// ---------------------------------------------------------------------------------------------
// Probing

fn probe(bytes: &[u8]) -> Result<video::VideoInfo, Error> {
    video::probe(bytes, &Limits::default())
}

fn place(info: &video::VideoInfo) -> Option<(f64, f64)> {
    info.location.map(|p| ((p.lat() * 1e4).round() / 1e4, (p.lon() * 1e4).round() / 1e4))
}

/// An MP4, an M4V, a MOV turned a quarter by its matrix and an MKV each report their duration to
/// the millisecond, size, rotation, date, codecs, rate, sound and location.
#[test]
fn every_kind_reports_its_facts() {
    let mp4 = probe(&mp4()).unwrap();
    assert_eq!(sniff(&self::mp4()), Some(Format::Video(VideoKind::Mp4)));
    assert_eq!((mp4.kind, mp4.duration_ms, mp4.size, mp4.rotation), (VideoKind::Mp4, Some(12_345), Some((1920, 1080)), 0));
    assert_eq!(mp4.created.unwrap().to_string(), "2026-06-04T19:42:10");
    assert_eq!((mp4.video_codec.as_deref(), mp4.sound_codec.as_deref(), mp4.has_sound), (Some("avc1"), Some("mp4a"), true));
    assert_eq!(mp4.frame_rate, Some(29.97));
    assert_eq!(place(&mp4), Some((37.7749, -122.4194)));

    let m4v = probe(&m4v()).unwrap();
    assert_eq!(sniff(&self::m4v()), Some(Format::Video(VideoKind::M4v)), "M4A among the compatible brands does not make it sound-only");
    assert_eq!((m4v.kind, m4v.duration_ms, m4v.size, m4v.rotation), (VideoKind::M4v, Some(5_000), Some((1280, 720)), 0));
    assert_eq!(m4v.created.unwrap().to_string(), "2026-06-04T19:42:10");
    assert_eq!((m4v.video_codec.as_deref(), m4v.sound_codec.as_deref(), m4v.has_sound, m4v.frame_rate), (Some("hvc1"), None, false, Some(25.0)));
    assert_eq!(place(&m4v), Some((-33.8688, 151.2093)));

    let mov = probe(&mov()).unwrap();
    assert_eq!(sniff(&self::mov()), Some(Format::Video(VideoKind::QuickTime)));
    assert_eq!((mov.kind, mov.duration_ms, mov.size, mov.rotation), (VideoKind::QuickTime, Some(7_200), Some((1920, 1080)), 1));
    assert_eq!(mov.display_size(), Some((1080, 1920)), "turned a quarter, shown upright");
    assert_eq!(mov.created.unwrap().to_string(), "2025-12-24T08:30:00");
    assert_eq!((mov.video_codec.as_deref(), mov.sound_codec.as_deref(), mov.has_sound), (Some("hvc1"), Some("lpcm"), true));
    assert_eq!(mov.frame_rate, Some(30.0));
    assert_eq!(place(&mov), Some((38.7139, -9.1394)));

    let mkv = probe(&mkv()).unwrap();
    assert_eq!(sniff(&self::mkv()), Some(Format::Video(VideoKind::Matroska)));
    assert_eq!((mkv.kind, mkv.duration_ms, mkv.size, mkv.rotation), (VideoKind::Matroska, Some(12_346), Some((3840, 2160)), 3));
    assert_eq!(mkv.created.unwrap().to_string(), "2026-06-04T19:42:10");
    assert_eq!((mkv.video_codec.as_deref(), mkv.sound_codec.as_deref(), mkv.has_sound), (Some("V_MPEGH/ISO/HEVC"), Some("A_OPUS"), true));
    assert_eq!(mkv.frame_rate, Some(29.97));
    assert_eq!(place(&mkv), Some((38.7139, -9.1394)), "the tags after the clusters, through the seek head");
}

/// Older QuickTime movies have no `ftyp`: their first box says what they are.
#[test]
fn a_movie_without_ftyp_is_quicktime() {
    let moov = bx(b"moov", &[mvhd(false, 0, 600, 600), trak(false, &video_track(b"jpeg", IDENTITY, (640, 480), 600, 600, 15))].concat());
    for bytes in [[moov.clone(), bx(b"mdat", &[0; 40])].concat(), [bx(b"mdat", &[0; 40]), moov].concat()] {
        assert_eq!(sniff(&bytes), Some(Format::Video(VideoKind::QuickTime)));
        let info = probe(&bytes).unwrap();
        assert_eq!(
            (info.duration_ms, info.size, info.created, info.video_codec.as_deref(), info.frame_rate),
            (Some(1000), Some((640, 480)), None, Some("jpeg"), Some(15.0))
        );
    }
}

/// HEIC, AVIF and CR3 files are never videos, whatever ISO brands they also name; nor is a
/// sound-only MP4.
#[test]
fn still_images_and_sound_are_never_videos() {
    let pad = |b: Vec<u8>| [b, bx(b"mdat", &[0; 32])].concat();
    let cases: [(Vec<u8>, Option<Format>); 6] = [
        (pad(ftyp(b"heic", &[b"mif1", b"heic", b"iso8"])), Some(Format::Heif)),
        (pad(ftyp(b"mif1", &[b"mif1", b"heic", b"isom"])), Some(Format::Heif)),
        (pad(ftyp(b"avif", &[b"avif", b"mif1", b"miaf", b"iso8"])), Some(Format::Avif)),
        (pad(ftyp(b"crx ", &[b"crx ", b"isom"])), Some(Format::Raw(RawKind::Cr3))),
        (pad(ftyp(b"isom", &[b"isom", b"avif"])), None),
        (pad(ftyp(b"M4A ", &[b"M4A ", b"mp42", b"isom"])), None),
    ];
    for (bytes, want) in cases {
        assert_eq!(sniff(&bytes), want, "{:?}", &bytes[8..12]);
        assert!(video::recognise(&bytes).is_none());
        assert_eq!(probe(&bytes).unwrap_err(), Error::Unknown);
    }
    // A WebM (DocType `webm`) or another EBML document is not listed.
    let webm = [ebml_header("webm"), el(SEGMENT, &mkv_info())].concat();
    assert_eq!(sniff(&webm), None);
}

/// The first bytes the scanner reads may cut a Matroska DocType: four or more of its bytes decide.
#[test]
fn matroska_is_recognised_from_the_first_bytes() {
    let whole = mkv();
    let doc = whole.windows(8).position(|w| w == b"matroska").unwrap();
    for len in 0..whole.len().min(64) {
        let want = (len >= doc + 4).then_some(Format::Video(VideoKind::Matroska));
        assert_eq!(sniff(&whole[..len]), want, "{len} bytes");
    }
    // As muxers write it (one-byte values), the DocType fits in the 32 bytes a scan reads.
    assert_eq!(sniff(&HEADS[4][..32]), Some(Format::Video(VideoKind::Matroska)));
}

/// Every video kind is recognised by content and answers "This video can't be played" on every
/// image path until frames are decoded.
#[test]
fn every_image_path_answers_the_video_reason() {
    let l = Limits::default();
    for (bytes, kind) in [(mp4(), VideoKind::Mp4), (m4v(), VideoKind::M4v), (mov(), VideoKind::QuickTime), (mkv(), VideoKind::Matroska)] {
        let format = Format::Video(kind);
        assert!(!format.supported() && VideoKind::ALL.contains(&kind));
        assert_eq!(format.name(), kind.name());
        for result in [
            read_header(&bytes, &l).map(|_| ()),
            decode(&bytes, &l).map(|_| ()),
            decode_thumbnail(&bytes, 128, &l).map(|_| ()),
            decode_as(format, &bytes, &DecodeOptions::default(), &l).map(|_| ()),
        ] {
            let err = result.unwrap_err();
            assert_eq!((err.clone(), err.reason()), (Error::Unsupported(format), "This video can't be played".to_string()));
        }
        assert_eq!(formats::embedded_thumbnail(format, &bytes, &l), Ok(None));
        assert_eq!(formats::hint(format), None);
    }
}

/// A file cut anywhere (every box and element boundary included) is truncated, never a panic:
/// a movie box not yet complete is `Truncated`; a Matroska file is `Truncated` until its info and
/// tracks are in, then reads as far as its bytes go.
#[test]
fn a_cut_file_is_truncated() {
    for (name, bytes) in [("mp4", mp4()), ("m4v", m4v()), ("mov", mov())] {
        let full = probe(&bytes).unwrap();
        for len in 0..bytes.len() {
            match probe(&bytes[..len]) {
                Ok(info) => assert_eq!(info, full, "{name} cut at {len}: only a cut after the movie box reads"),
                Err(e) => assert_eq!(e, Error::Truncated, "{name} cut at {len}"),
            }
        }
    }
    // The MP4's movie box is last: every cut loses it.
    let mp4 = mp4();
    assert!((0..mp4.len()).all(|len| probe(&mp4[..len]) == Err(Error::Truncated)));

    let mkv = mkv();
    let full = probe(&mkv).unwrap();
    let tracks = mkv_tracks();
    let tracks_end = mkv.windows(tracks.len()).position(|w| w == tracks).unwrap() + tracks.len();
    for len in 0..mkv.len() {
        match probe(&mkv[..len]) {
            Ok(info) => {
                assert!(len >= tracks_end, "mkv cut at {len}, before its tracks end at {tracks_end}");
                assert_eq!(info.location, None, "the tags lie past {len}");
                assert_eq!(video::VideoInfo { location: full.location, ..info }, full, "mkv cut at {len}");
            }
            Err(e) => {
                assert_eq!(e, Error::Truncated, "mkv cut at {len}");
                assert!(len < tracks_end, "mkv cut at {len}");
            }
        }
    }
}

/// Nesting past 12 levels, 4,097 sibling boxes or elements, an EBML size past its parent and
/// elements of unknown size are refused or bounded.
#[test]
fn hostile_structures_are_refused_or_bounded() {
    // 4,097 boxes beside the movie's tracks: refused at the cap.
    let crowd = bx(b"moov", &[mvhd(false, 0, 600, 600), bx(b"free", &[]).repeat(4097)].concat());
    let bytes = [ftyp(b"isom", &[b"isom"]), crowd].concat();
    assert_eq!(probe(&bytes), Err(Error::TooMany("boxes in one level")));
    let fine = bx(b"moov", &[mvhd(false, 0, 600, 600), bx(b"free", &[]).repeat(4000)].concat());
    assert_eq!(probe(&[ftyp(b"isom", &[b"isom"]), fine].concat()).unwrap().duration_ms, Some(1000));

    // Twenty levels of boxes inside the user data and a track: never followed, the rest reads.
    let mut deep = bx(b"leaf", b"");
    for _ in 0..20 {
        deep = bx(b"udta", &deep);
    }
    let mut deep_trak = bx(b"leaf", b"");
    for _ in 0..20 {
        deep_trak = bx(b"trak", &deep_trak);
    }
    let moov = bx(b"moov", &[mvhd(false, 0, 600, 1200), deep, deep_trak].concat());
    let info = probe(&[ftyp(b"mp41", &[b"mp41"]), moov].concat()).unwrap();
    assert_eq!((info.duration_ms, info.size, info.location), (Some(2000), None, None));

    // A box running past its parent inside the bytes: damaged.
    let mut bad = bx(b"moov", &mvhd(false, 0, 600, 600));
    bad[11] = 0xff; // mvhd's size, now past the movie box
    bad.extend([0; 300]);
    assert!(matches!(probe(&[ftyp(b"isom", &[b"isom"]), bad].concat()), Err(Error::Malformed(_))));

    // Matroska: 4,097 elements in the segment's head.
    let crowd = [ebml_header("matroska"), el(SEGMENT, &[mkv_info(), el(VOID, &[]).repeat(4097), mkv_tracks()].concat())].concat();
    assert_eq!(probe(&crowd), Err(Error::TooMany("EBML elements in one level")));

    // An element whose size runs past its parent (and not past the bytes): damaged.
    let info = mkv_info();
    let mut past = [ebml_header("matroska"), el(SEGMENT, &info), el(VOID, &[0; 400])].concat();
    let at = past.windows(info.len()).position(|w| w == info).unwrap();
    assert_eq!(past[at + 4], 0x80 | u8::try_from(info.len() - 5).unwrap(), "a one-byte size");
    past[at + 4] = 0xfe; // 126 bytes: past the segment, inside the void after it
    assert!(matches!(probe(&past), Err(Error::Malformed(_))), "{:?}", probe(&past));

    // Unknown sizes: a segment and a cluster running to the end are bounded by it.
    let want = probe(&mkv()).unwrap();
    for (segment, cluster) in [(true, false), (false, true), (true, true)] {
        let bytes = mkv_with(segment, cluster);
        assert_eq!(probe(&bytes), Ok(want.clone()), "unknown-size segment {segment}, cluster {cluster}");
    }
    // An unknown size where no parent bounds it but the bytes: the tracks running to the end.
    let open = [ebml_header("matroska"), el_unknown(SEGMENT, &[mkv_info(), el_unknown(TRACKS, &mkv_track_entries())].concat())].concat();
    let info = probe(&open).unwrap();
    assert_eq!((info.duration_ms, info.size), (Some(12_346), Some((3840, 2160))));
}

/// From a file, only the boxes the facts need are read; the result is the in-memory probe's.
#[test]
fn a_file_is_probed_without_reading_its_media() {
    let dir = std::env::temp_dir().join(format!("dalica-image-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cases = vec![("clip.mp4", mp4()), ("clip.mov", mov()), ("clip.m4v", m4v()), ("clip.mkv", mkv())];
    // An MP4 whose movie box follows 3 MiB of media data, and one after a 64-bit media data size.
    let moov_of = |b: &[u8]| b[b.windows(4).position(|w| w == b"moov").unwrap() - 4..].to_vec();
    let mp4_moov = moov_of(&mp4());
    cases.push(("long.mp4", [ftyp(b"mp42", &[b"mp42", b"isom"]), bx(b"mdat", &vec![7; 3 << 20]), mp4_moov.clone()].concat()));
    let mut large = 1u32.to_be_bytes().to_vec();
    large.extend(b"mdat");
    large.extend((16u64 + 64).to_be_bytes());
    large.extend([0; 64]);
    cases.push(("large.mp4", [ftyp(b"mp42", &[b"mp42", b"isom"]), large, mp4_moov].concat()));
    for (name, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(video::probe_file(&path, &Limits::default()), probe(&bytes), "{name}");
    }
    // A movie box past the header limit is refused, not read.
    let tight = Limits { max_header_bytes: 256, ..Limits::default() };
    assert_eq!(video::probe_file(&dir.join("long.mp4"), &tight), Err(Error::TooMany("bytes in a movie box")));
    std::fs::remove_dir_all(&dir).unwrap();
}

const HEADS: [&[u8]; 5] = [
    b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom",
    b"\0\0\0\x14ftypqt  \0\0\0\0qt  ",
    b"\0\0\0\x14ftypM4V \0\0\0\0M4V ",
    b"\0\0\0\x08wide",
    b"\x1a\x45\xdf\xa3\xa3\x42\x86\x81\x01\x42\xf7\x81\x01\x42\xf2\x81\x04\x42\xf3\x81\x08\x42\x82\x88matroska\x42\x87\x81\x04\x42\x85\x81\x02",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    /// Random bytes after a video header never panic, through the probe and every image path.
    #[test]
    fn random_bytes_after_a_video_header_never_panic(head in 0..HEADS.len(), tail in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let bytes = [HEADS[head], &tail].concat();
        let l = Limits::default();
        let _ = sniff(&bytes);
        let _ = probe(&bytes);
        let _ = read_header(&bytes, &l);
        let _ = decode(&bytes, &l);
        for kind in VideoKind::ALL {
            let _ = video::probe_as(kind, &bytes, &l);
        }
    }

    /// Bytes flipped anywhere in a real container never panic.
    #[test]
    fn corrupted_containers_never_panic(which in 0..4usize, flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = [mp4(), m4v(), mov(), mkv()][which].clone();
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        let _ = probe(&bytes);
    }
}
