//! Videos listed on scan (M15.11, `formats.md` §1): MP4, MOV and MKV files built here from the
//! documented layouts (ISO/IEC 14496-12 boxes, QuickTime atoms, RFC 8794 EBML and RFC 9559
//! Matroska elements) are listed as videos with their container's facts; the video type turned off
//! and on again; a cut container listed with its reason; a legacy-only folder's star on a video.

use std::path::Path;

use dalica_core::DateTime;
use dalica_image::Format;
use dalica_image::sniff::VideoKind;
use dalica_library::{Library, VideoFacts};
use dalica_scan::{FileType, FileTypes, FoundPhoto, Options, ReadMetadata, Scan, diff, scan_with};
use dalica_scenes::{Scene, jpeg};
use dalica_sidecar::legacy::sidecar_names;

use crate::common::{apply_all, put};

// ---------------------------------------------------------------------------------------------
// ISO BMFF

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
/// A quarter turn clockwise.
const QUARTER: [i32; 9] = [0, 0x1_0000, 0, -0x1_0000, 0, 0, 0, 0, 0x4000_0000];

fn matrix(m: [i32; 9]) -> Vec<u8> {
    m.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// Version 0 `mvhd`/`mdhd` times.
fn times(created: u32, timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0; 4];
    for v in [created, created, timescale, duration] {
        p.extend(v.to_be_bytes());
    }
    p
}

fn mvhd(created: u32, timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = times(created, timescale, duration);
    p.extend(0x1_0000u32.to_be_bytes());
    p.extend(0x100u16.to_be_bytes());
    p.extend([0; 10]);
    p.extend(matrix(IDENTITY));
    p.extend([0; 24]);
    p.extend(2u32.to_be_bytes());
    bx(b"mvhd", &p)
}

fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0; 8];
    p.extend(handler);
    p.extend([0; 13]);
    bx(b"hdlr", &p)
}

/// One video track of `code` frames stored at `w` × `h`, turned by `m`, `samples` frames over
/// `duration` ticks of `timescale`.
fn video_trak(code: &[u8; 4], m: [i32; 9], (w, h): (u16, u16), timescale: u32, duration: u32, samples: u32) -> Vec<u8> {
    let mut tkhd = vec![0, 0, 0, 7];
    tkhd.extend([0; 8]);
    tkhd.extend(1u32.to_be_bytes());
    tkhd.extend([0; 4 + 4 + 8 + 8]);
    tkhd.extend(matrix(m));
    tkhd.extend((u32::from(w) << 16).to_be_bytes());
    tkhd.extend((u32::from(h) << 16).to_be_bytes());
    let mut entry = vec![0; 6];
    entry.extend(1u16.to_be_bytes());
    entry.extend([0; 16]);
    entry.extend(w.to_be_bytes());
    entry.extend(h.to_be_bytes());
    entry.extend([0; 50]);
    let mut stsd = vec![0; 4];
    stsd.extend(1u32.to_be_bytes());
    stsd.extend(bx(code, &entry));
    let mut stsz = vec![0; 4];
    stsz.extend(1000u32.to_be_bytes());
    stsz.extend(samples.to_be_bytes());
    let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stsz", &stsz)].concat());
    let mut mdhd = times(0, timescale, duration);
    mdhd.extend([0; 4]);
    let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), hdlr(b"vide"), bx(b"minf", &stbl)].concat());
    bx(b"trak", &[bx(b"tkhd", &tkhd), mdia].concat())
}

/// QuickTime user data: the recording location as ISO 6709 text.
fn xyz(text: &str) -> Vec<u8> {
    let mut p = u16::try_from(text.len()).unwrap().to_be_bytes().to_vec();
    p.extend(0x15c7u16.to_be_bytes());
    p.extend(text.as_bytes());
    bx(b"udta", &bx(b"\xa9xyz", &p))
}

/// 2026-06-04T19:42:10 UTC, in seconds from 1904.
const MP4_CREATED: u32 = 3_863_446_930;
/// 2025-12-24T08:30:00 UTC, in seconds from 1904.
const MOV_CREATED: u32 = 3_849_409_800;

/// A phone's MP4: 12.345 s of 1920 × 1080 AVC at 30000/1001 frames a second, with a location.
fn mp4() -> Vec<u8> {
    let moov = bx(
        b"moov",
        &[mvhd(MP4_CREATED, 1000, 12_345), video_trak(b"avc1", IDENTITY, (1920, 1080), 30_000, 370_370, 370), xyz("+37.7749-122.4194/")].concat(),
    );
    [ftyp(b"mp42", &[b"mp42", b"isom"]), bx(b"mdat", &[0x5a; 300]), moov].concat()
}

/// A QuickTime movie filmed upright: stored landscape, turned a quarter by the track matrix.
fn mov() -> Vec<u8> {
    let moov = bx(b"moov", &[mvhd(MOV_CREATED, 600, 4_320), video_trak(b"hvc1", QUARTER, (1920, 1080), 600, 4_320, 216)].concat());
    [ftyp(b"qt  ", &[b"qt  "]), moov, bx(b"mdat", &[2; 128])].concat()
}

// ---------------------------------------------------------------------------------------------
// EBML / Matroska

fn ebml_size(n: usize) -> Vec<u8> {
    let n = u64::try_from(n).unwrap();
    let len = (1..=8).find(|&l| n < (1u64 << (7 * l)) - 1).unwrap();
    (n | (1u64 << (7 * len))).to_be_bytes()[8 - len as usize..].to_vec()
}

fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let b = id.to_be_bytes();
    let skip = b.iter().position(|&x| x != 0).unwrap();
    [b[skip..].to_vec(), ebml_size(data.len()), data.to_vec()].concat()
}

fn uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

fn float(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// A Matroska file: 2.5 s of 1280 × 720 VP9 with Opus sound, recorded 2026-06-04T19:42:10 UTC.
fn mkv() -> Vec<u8> {
    let header = el(0x1a45_dfa3, &[uint(0x4286, 1), el(0x4282, b"matroska"), uint(0x4287, 4), uint(0x4285, 2)].concat());
    let info = el(0x1549_a966, &[uint(0x2a_d7b1, 1_000_000), float(0x4489, 2_500.0), el(0x4461, &802_294_930_000_000_000i64.to_be_bytes())].concat());
    let video = el(0xe0, &[uint(0xb0, 1280), uint(0xba, 720)].concat());
    let v = el(0xae, &[uint(0xd7, 1), uint(0x83, 1), el(0x86, b"V_VP9"), video].concat());
    let a = el(0xae, &[uint(0xd7, 2), uint(0x83, 2), el(0x86, b"A_OPUS")].concat());
    let tracks = el(0x1654_ae6b, &[v, a].concat());
    let cluster = el(0x1f43_b675, &uint(0xe7, 0));
    [header, el(0x1853_8067, &[info, tracks, cluster].concat())].concat()
}

// ---------------------------------------------------------------------------------------------

fn photo(seed: u64) -> Vec<u8> {
    jpeg(&Scene::new(seed, 32, 24), 80, None).unwrap()
}

fn scan(root: &Path, options: &Options) -> Scan {
    scan_with(&[root.to_path_buf()], options)
}

fn listed(scan: &Scan) -> Vec<&FoundPhoto> {
    scan.folders.iter().flat_map(|f| &f.photos).collect()
}

fn found<'a>(scan: &'a Scan, name: &str) -> &'a FoundPhoto {
    listed(scan).into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed"))
}

fn dt((y, mo, d): (u16, u8, u8), (h, mi, s): (u8, u8, u8)) -> DateTime {
    DateTime::new(dalica_core::Date::new(y, mo, d).unwrap(), h, mi, s).unwrap()
}

fn without_videos() -> Options {
    Options { types: FileTypes::all().with(FileType::Video, false), ..Options::default() }
}

#[test]
fn a_folder_lists_its_videos_with_their_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Trip");
    put(&dir.join("clip.mp4"), &mp4());
    put(&dir.join("upright.mov"), &mov());
    put(&dir.join("talk.mkv"), &mkv());
    put(&dir.join("a.jpg"), &photo(1));
    let s = scan(tmp.path(), &Options { metadata: ReadMetadata::all(), ..Options::default() });
    assert!(s.failures.is_empty(), "{:?}", s.failures);
    assert_eq!(listed(&s).len(), 4);
    assert_eq!(listed(&s).iter().filter(|p| p.video.is_some()).count(), 3);
    assert_eq!(found(&s, "a.jpg").video, None);

    let clip = found(&s, "clip.mp4");
    assert_eq!(clip.format, Format::Video(VideoKind::Mp4), "a video, not a RAW");
    assert_eq!((clip.width, clip.height, clip.date), (1920, 1080, Some(dt((2026, 6, 4), (19, 42, 10)))));
    let facts = clip.video.as_ref().unwrap();
    assert_eq!((facts.duration_ms, facts.video_codec.as_str(), facts.frame_rate_milli), (12_345, "avc1", 29_970));
    let file = clip.file.as_ref().expect("its metadata read with the probe");
    assert_eq!(file.0.place, Some((37.7749, -122.4194)), "the location, for the geotag");
    assert!(file.1.video && file.1.geotagged);

    let upright = found(&s, "upright.mov");
    assert_eq!((upright.width, upright.height), (1080, 1920), "the rotation applied to the cell");
    let facts = upright.video.as_ref().unwrap();
    assert_eq!((facts.duration_ms, facts.width, facts.height, facts.rotation), (7_200, 1920, 1080, 1));
    assert_eq!(upright.date, Some(dt((2025, 12, 24), (8, 30, 0))));

    let talk = found(&s, "talk.mkv");
    assert_eq!(talk.format, Format::Video(VideoKind::Matroska));
    let facts = talk.video.as_ref().unwrap();
    assert_eq!((facts.duration_ms, facts.video_codec.as_str(), facts.sound_codec.as_str(), facts.sound), (2_500, "V_VP9", "A_OPUS", true));
    assert_eq!((talk.width, talk.height, talk.date), (1280, 720, Some(dt((2026, 6, 4), (19, 42, 10)))));

    // The diff gives the model the facts; the location comes in as a geotag as a photo's GPS does
    // (the engine's add batch for a new photo; a read of a known one here); then nothing changes.
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    let id = |lib: &Library, name: &str| lib.photos().find(|p| p.name == name).unwrap().id;
    assert_eq!(lib.video_facts(id(&lib, "clip.mp4")), clip.video.as_ref());
    assert!(lib.is_video(id(&lib, "talk.mkv")) && !lib.is_video(id(&lib, "a.jpg")));
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    let at = lib.geotag(id(&lib, "clip.mp4")).unwrap();
    assert_eq!((at.lat(), at.lon()), (37.7749, -122.4194));
    let again = diff(&mut lib, &s);
    assert!(again.is_empty(), "{again:?}");

    // Without metadata reading, the facts still come; what the file says does not.
    let plain = scan(tmp.path(), &Options::default());
    assert_eq!(found(&plain, "clip.mp4").video, clip.video);
    assert!(found(&plain, "clip.mp4").file.is_none());
}

#[test]
fn turning_the_video_type_off_and_on_keeps_what_the_sidecar_says() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Trip");
    put(&dir.join("clip.mp4"), &mp4());
    put(&dir.join("a.jpg"), &photo(1));
    let sidecar = br#"{ "dalica": { "version": 1 }, "photos": { "clip.mp4": { "star": true, "caption": "Tram 28", "tags": ["Lisbon"] } } }"#;
    put(&dir.join(".dalica.json"), sidecar);
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(tmp.path(), &Options::default()));
    apply_all(&mut lib, ops);
    assert_eq!(lib.photos().count(), 2);

    let off = scan(tmp.path(), &without_videos());
    assert_eq!(listed(&off).iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a.jpg"]);
    let ops = diff(&mut lib, &off);
    apply_all(&mut lib, ops);
    assert_eq!(lib.photos().count(), 1);
    assert_eq!(std::fs::read(dir.join(".dalica.json")).unwrap(), sidecar, "the videos' sidecar entries stay");

    let ops = diff(&mut lib, &scan(tmp.path(), &Options::default()));
    apply_all(&mut lib, ops);
    let clip = lib.photos().find(|p| p.name == "clip.mp4").unwrap().clone();
    assert!(clip.star && lib.is_video(clip.id));
    assert_eq!((lib.info(clip.id).caption.as_str(), lib.info(clip.id).tags.clone()), ("Tram 28", vec!["Lisbon".to_string()]));
    assert_eq!(lib.video_facts(clip.id).map(|v| v.duration_ms), Some(12_345));
}

#[test]
fn a_cut_or_hostile_container_is_listed_with_its_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Cut");
    let whole = mp4();
    // The movie box cut short: the facts are not all there.
    put(&dir.join("cut.mp4"), &whole[..whole.len() - 40]);
    // A movie box claiming more bytes than the file holds.
    let mut hostile = [ftyp(b"isom", &[b"isom"]), bx(b"moov", &[0; 16])].concat();
    let at = hostile.len() - 24;
    hostile[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    put(&dir.join("hostile.mov"), &hostile);
    let s = scan(tmp.path(), &Options::default());
    for name in ["cut.mp4", "hostile.mov"] {
        let p = found(&s, name);
        assert_eq!(p.video, Some(VideoFacts::default()), "{name}: a video with nothing known");
        assert_eq!((p.width, p.height, p.date), (0, 0, None));
        let failure = s.failures.iter().find(|f| f.path == dir.join(name)).unwrap_or_else(|| panic!("{name}: a failure"));
        assert!(!failure.reason.is_empty() && matches!(failure.format, Some(Format::Video(_))), "{failure:?}");
    }
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    assert!(lib.photos().all(|p| lib.is_video(p.id)));
}

#[test]
fn a_legacy_only_folders_star_on_a_video_shows_on_its_cell() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Old");
    put(&dir.join("clip.mp4"), &mp4());
    put(&dir.join("a.jpg"), &photo(1));
    let legacy = ["[encoding]", "utf8=1", "[clip.mp4]", "star=yes", "caption=Old tram"].join("\r\n") + "\r\n";
    put(&dir.join(&sidecar_names()[0]), legacy.as_bytes());
    let s = scan(tmp.path(), &Options::default());
    let clip = found(&s, "clip.mp4");
    assert!(clip.star && clip.video.is_some());
    assert_eq!(clip.info.caption, "Old tram");
    assert!(!found(&s, "a.jpg").star);
}
