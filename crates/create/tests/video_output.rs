//! A video's outputs (`roadmap.md` M15.25; `editing.md` §11, `creating.md` §4): Export Clip cuts
//! the trimmed part out of the container without re-encoding (MP4 with H.264 and AAC, the same
//! clip with a codec Dalica cannot decode, and Matroska), starting at the key frame at or before
//! the start, and what it writes probes (`image::formats::video::probe`) and decodes at the length
//! it says; hostile cuts are refused; Save Frame as Picture's JPEG is the frame's size with the
//! video's rotation, at quality 92, dated. The clips are made here: H.264 with every macroblock
//! I_PCM (ITU-T H.264 §7.3.5) and silent AAC-LC packets, muxed by hand from ISO/IEC 14496-12 and
//! RFC 9559.

use std::path::Path;

use dalica_create::video::{self, ClipError, FRAME_QUALITY};
use dalica_fs::{Expect, TempDir};
use dalica_image::formats::video::frames::{FrameOptions, FrameSource};
use dalica_image::formats::video::sound::SoundSource;
use dalica_image::formats::video::{playability, probe};
use dalica_image::{Limits, Rgba8};
use dalica_meta::DateTaken;

// --- An H.264 writer: every macroblock I_PCM, CAVLC, Baseline, POC type 2 -------------------------

#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                (self.acc, self.n) = (0, 0);
            }
        }
        self
    }
    fn ue(&mut self, v: u64) -> &mut Bits {
        let v1 = v + 1;
        let len = 64 - v1.leading_zeros();
        self.u(len - 1, 0).u(len, v1)
    }
    fn se(&mut self, v: i64) -> &mut Bits {
        self.ue(if v > 0 { (2 * v - 1) as u64 } else { (-2 * v) as u64 })
    }
    fn align(&mut self) -> &mut Bits {
        while self.n != 0 {
            self.u(1, 0);
        }
        self
    }
    fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        self.align();
        self.out
    }
}

fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![(ref_idc << 5) | kind];
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    out
}

fn sps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 30).ue(0);
    b.ue(0).ue(2).ue(1).u(1, 0);
    b.ue(MBS.0 as u64 - 1).ue(MBS.1 as u64 - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0);
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 8, &b.trailing())
}

/// Frame `k` as one I slice of I_PCM macroblocks, length-prefixed: grey `grey(k)`.
fn frame(k: usize) -> Vec<u8> {
    let idr = k.is_multiple_of(GOP);
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, (k % GOP) as u64);
    if idr {
        b.ue((k / GOP % 2) as u64).u(1, 0).u(1, 0);
    } else {
        b.u(1, 0);
    }
    b.se(0).ue(1);
    for _ in 0..MBS.0 * MBS.1 {
        b.ue(25).align();
        for _ in 0..256 {
            b.u(8, u64::from(grey(k)));
        }
        for _ in 0..128 {
            b.u(8, 128);
        }
    }
    let s = nal(if idr { 3 } else { 2 }, if idr { 5 } else { 1 }, &b.trailing());
    let mut out = (s.len() as u32).to_be_bytes().to_vec();
    out.extend(s);
    out
}

const FPS: usize = 25;
const GOP: usize = 10;
const MBS: (usize, usize) = (10, 6);
const FRAMES: usize = 50;
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

fn grey(k: usize) -> u8 {
    40 + 3 * k as u8
}

/// The frame number of a decoded picture, from its grey.
fn frame_of(img: &Rgba8) -> usize {
    let g = f64::from(img.data[((img.height as usize / 2) * img.width as usize + img.width as usize / 2) * 4]);
    (0..FRAMES).min_by_key(|&k| ((f64::from(grey(k)) - 16.0) / 219.0 * 255.0 - g).abs() as u64).unwrap()
}

// --- An MP4 writer ---------------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0, 0, 0, 0][..], payload].concat())
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

struct Track {
    id: u32,
    handler: [u8; 4],
    entry: Vec<u8>,
    timescale: u32,
    durations: Vec<u32>,
    syncs: Option<Vec<u32>>,
    size: (u32, u32),
    samples: Vec<Vec<u8>>,
}

fn trak(t: &Track, at: &mut u32) -> Vec<u8> {
    let duration: u32 = t.durations.iter().sum();
    let mut tkhd = vec![0u8; 8];
    tkhd.extend(be(t.id));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be(duration * 1000 / t.timescale));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(t.size.0 << 16));
    tkhd.extend(be(t.size.1 << 16));
    let mut mdhd = vec![0u8; 8];
    mdhd.extend(be(t.timescale));
    mdhd.extend(be(duration));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(&t.entry);
    let mut stts = be(t.durations.len() as u32).to_vec();
    for d in &t.durations {
        stts.extend(be(1));
        stts.extend(be(*d));
    }
    let mut stsz = be(0).to_vec();
    stsz.extend(be(t.samples.len() as u32));
    let mut stco = be(t.samples.len() as u32).to_vec();
    for s in &t.samples {
        stsz.extend(be(s.len() as u32));
        stco.extend(be(*at));
        *at += s.len() as u32;
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let mut stbl = [full(b"stsd", &stsd), full(b"stts", &stts), full(b"stsc", &stsc), full(b"stsz", &stsz), full(b"stco", &stco)].concat();
    if let Some(syncs) = &t.syncs {
        let mut stss = be(syncs.len() as u32).to_vec();
        syncs.iter().for_each(|s| stss.extend(be(*s)));
        stbl.extend(full(b"stss", &stss));
    }
    let mdia = bx(b"mdia", &[full(b"mdhd", &mdhd), full(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    bx(b"trak", &[full(b"tkhd", &tkhd), mdia].concat())
}

fn mp4(tracks: &[Track]) -> Vec<u8> {
    let mut out = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let data: Vec<u8> = tracks.iter().flat_map(|t| t.samples.concat()).collect();
    let mut at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &data));
    let traks: Vec<u8> = tracks.iter().flat_map(|t| trak(t, &mut at)).collect();
    let mut mvhd = vec![0u8; 8];
    mvhd.extend(be(1000));
    mvhd.extend(be((FRAMES * 1000 / FPS) as u32));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    // The movie's place, as phones write it: copied into the clip.
    let udta = bx(b"udta", &bx(b"\xa9xyz", &[&[0, 18, 0x15, 0xc7][..], b"+38.7139-009.1394/"].concat()));
    out.extend(bx(b"moov", &[full(b"mvhd", &mvhd), traks, udta].concat()));
    out
}

fn avc1(code: &[u8; 4]) -> Vec<u8> {
    let (sps, pps) = (sps(), pps());
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend(((MBS.0 * 16) as u16).to_be_bytes());
    p.extend(((MBS.1 * 16) as u16).to_be_bytes());
    p.extend(be(0x48_0000));
    p.extend(be(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    p.extend(bx(b"avcC", &avcc));
    bx(code, &p)
}

fn mp4a() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(b"mp4a", &p)
}

fn packets() -> usize {
    (FRAMES * 48_000 / FPS).div_ceil(1024)
}

/// Two seconds of 160 × 96 at 25 frames a second, a key frame every 10, with silent AAC; the
/// video's sample entry `code` (`avc1`, or a codec Dalica cannot decode). A clip of another codec
/// marks no sync samples (every one is).
fn clip(code: &[u8; 4]) -> Vec<u8> {
    let syncs = (code == b"avc1").then(|| (0..FRAMES as u32).filter(|k| (*k as usize).is_multiple_of(GOP)).map(|k| k + 1).collect());
    let video = Track {
        id: 1,
        handler: *b"vide",
        entry: avc1(code),
        timescale: 1000,
        durations: vec![(1000 / FPS) as u32; FRAMES],
        syncs,
        size: ((MBS.0 * 16) as u32, (MBS.1 * 16) as u32),
        samples: (0..FRAMES).map(frame).collect(),
    };
    let sound = Track {
        id: 2,
        handler: *b"soun",
        entry: mp4a(),
        timescale: 48_000,
        durations: vec![1024; packets()],
        syncs: None,
        size: (0, 0),
        samples: vec![SILENT_AAC.to_vec(); packets()],
    };
    mp4(&[video, sound])
}

// --- A Matroska writer -------------------------------------------------------------------------------

fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
    v.push(0x01);
    v.extend(&(data.len() as u64).to_be_bytes()[1..]);
    v.extend_from_slice(data);
    v
}

fn el_uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// The same clip in Matroska: clusters of one second, simple blocks, the sound in its own blocks.
fn mkv() -> Vec<u8> {
    let (sps, pps) = (sps(), pps());
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    let video = [el_uint(0xb0, (MBS.0 * 16) as u64), el_uint(0xba, (MBS.1 * 16) as u64)].concat();
    let mut tracks = el(0xae, &[el_uint(0xd7, 1), el_uint(0x83, 1), el(0x86, b"V_MPEG4/ISO/AVC"), el(0x63a2, &avcc), el(0xe0, &video)].concat());
    tracks.extend(el(0xae, &[el_uint(0xd7, 2), el_uint(0x83, 2), el(0x86, b"A_AAC"), el(0x63a2, &[0x11, 0x88])].concat()));
    let info = [el_uint(0x2a_d7b1, 1_000_000), el(0x4489, &2000f64.to_be_bytes()), el(0x4d80, b"dalica-test")].concat();
    let mut segment = [el(0x1549_a966, &info), el(0x1654_ae6b, &tracks)].concat();
    let block = |track: u8, rel: i16, key: bool, data: &[u8]| {
        let mut b = vec![0x80 | track];
        b.extend(rel.to_be_bytes());
        b.push(if key { 0x80 } else { 0 });
        b.extend_from_slice(data);
        el(0xa3, &b)
    };
    let mut packet = 0;
    for second in 0..FRAMES.div_ceil(FPS) {
        let mut cluster = el_uint(0xe7, second as u64 * 1000);
        for k in second * FPS..((second + 1) * FPS).min(FRAMES) {
            cluster.extend(block(1, ((k - second * FPS) * 40) as i16, k.is_multiple_of(GOP), &frame(k)));
            while packet < packets() && packet * 1024 * 1000 / 48_000 < (k + 1) * 40 {
                let t = (packet * 1024 * 1000 / 48_000) as i64 - second as i64 * 1000;
                cluster.extend(block(2, t as i16, true, &SILENT_AAC));
                packet += 1;
            }
        }
        segment.extend(el(0x1f43_b675, &cluster));
    }
    let header = el(0x1a45_dfa3, &[el(0x4282, b"matroska"), el_uint(0x4287, 4)].concat());
    [header, el(0x1853_8067, &segment)].concat()
}

// --- The tests --------------------------------------------------------------------------------------

fn file(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    dalica_fs::replace(&path, bytes, Expect::Absent).unwrap();
    path
}

fn cut(path: &Path, start: u64, end: u64) -> video::Cut {
    video::cut(path, start, end).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The cut decodes from its first frame, the key frame at or before the start, to the last
/// frame shown before the end, and its sound lasts as long.
fn decodes_as_cut(bytes: &[u8], first: usize, last: usize) {
    let mut src = FrameSource::from_bytes(bytes.to_vec(), &Limits::default(), FrameOptions::default()).unwrap();
    assert_eq!(src.frame_count(), last - first + 1);
    let f = src.frame(0).unwrap();
    assert_eq!((f.time_ms, frame_of(&f.image)), (0, first));
    let f = src.frame(last - first).unwrap();
    assert_eq!(frame_of(&f.image), last);
    let mut sound = SoundSource::from_bytes(bytes.to_vec(), &Limits::default()).unwrap();
    let samples = sound.read_all(48_000 * 4).unwrap();
    let ms = samples.len() as u64 * 1000 / 48_000;
    let want = ((last - first + 1) * 1000 / FPS) as u64;
    assert!(ms.abs_diff(want) <= 2 * 1024 * 1000 / 48_000, "sound of {ms} ms for {want} ms of frames");
}

#[test]
fn export_clip_of_half_a_second_to_one_and_a_half_copies_about_one_second_from_the_key_frame_before() {
    let dir = TempDir::new("dalica-clip").unwrap();
    let source = clip(b"avc1");
    let path = file(&dir, "clip.mp4", &source);
    let c = cut(&path, 500, 1500);
    // The key frame at or before 500 ms is frame 10 (400 ms); the last frame before 1500 ms is 37.
    assert_eq!((c.start_ms, c.end_ms, c.extension), (400, 1520, "mp4"));
    let info = probe(&c.bytes, &Limits::default()).unwrap();
    assert_eq!(info.duration_ms, Some(c.length_ms()), "the clip says how long it is");
    assert!(c.length_ms().abs_diff(1000) <= (GOP * 1000 / FPS) as u64, "about one second: {}", c.length_ms());
    assert_eq!((info.size, info.video_codec.as_deref(), info.has_sound), (Some((160, 96)), Some("avc1"), true));
    assert_eq!(info.location.map(|l| (l.lat(), l.lon())), Some((38.7139, -9.1394)), "the movie's place is copied");
    assert!(playability(&info).video.is_ok());
    decodes_as_cut(&c.bytes, 10, 37);
    assert!(c.bytes.len() < source.len() * 2 / 3, "only the cut's samples: {} of {}", c.bytes.len(), source.len());
    // From the start the cut is exact; the whole clip copies whole.
    let c = cut(&path, 0, 2000);
    assert_eq!((c.start_ms, c.end_ms), (0, 2000));
    decodes_as_cut(&c.bytes, 0, FRAMES - 1);
    // The source was only read.
    assert_eq!(std::fs::read(&path).unwrap(), source);
}

#[test]
fn export_clip_cuts_a_clip_of_a_codec_dalica_cannot_play_alike() {
    let dir = TempDir::new("dalica-clip").unwrap();
    let path = file(&dir, "clip.mov", &clip(b"mp4v"));
    let before = probe(&std::fs::read(&path).unwrap(), &Limits::default()).unwrap();
    assert!(playability(&before).video.is_err(), "MPEG-4 Part 2 does not play");
    let c = cut(&path, 500, 1500);
    // Every sample is a sync sample: the cut starts at the frame shown at 500 ms (480 ms).
    assert_eq!((c.start_ms, c.end_ms), (480, 1520));
    let info = probe(&c.bytes, &Limits::default()).unwrap();
    assert_eq!(info.duration_ms, Some(1040));
    assert_eq!((info.video_codec.as_deref(), info.has_sound, info.size), (Some("mp4v"), true, Some((160, 96))));
}

#[test]
fn export_clip_cuts_matroska_clusters_and_their_blocks() {
    let dir = TempDir::new("dalica-clip").unwrap();
    let path = file(&dir, "clip.mkv", &mkv());
    let c = cut(&path, 500, 1500);
    assert_eq!((c.start_ms, c.end_ms, c.extension), (400, 1520, "mkv"));
    let info = probe(&c.bytes, &Limits::default()).unwrap();
    assert_eq!(info.duration_ms, Some(1120));
    assert_eq!((info.video_codec.as_deref(), info.has_sound), (Some("V_MPEG4/ISO/AVC"), true));
    decodes_as_cut(&c.bytes, 10, 37);
}

#[test]
fn export_clip_refuses_hostile_cuts_and_files() {
    let dir = TempDir::new("dalica-clip").unwrap();
    let path = file(&dir, "clip.mp4", &clip(b"avc1"));
    assert_eq!(video::cut(&path, 900, 900), Err(ClipError::Empty { start_ms: 900, end_ms: 900 }));
    assert_eq!(video::cut(&path, 1500, 500), Err(ClipError::Empty { start_ms: 1500, end_ms: 500 }));
    assert!(matches!(video::cut(&path, 5_000, 9_000), Err(ClipError::Empty { .. })), "past the clip's end");
    assert!(matches!(video::cut(&dir.path().join("missing.mp4"), 0, 10), Err(ClipError::Read(_))));
    let jpeg = file(&dir, "photo.jpg", &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0]);
    assert!(matches!(video::cut(&jpeg, 0, 10), Err(ClipError::Unsupported(_))));
    // Cut short, or with its tables and blocks overwritten: refused or cut, never a panic.
    let whole = clip(b"avc1");
    let short = file(&dir, "short.mp4", &whole[..whole.len() - 300]);
    assert!(matches!(video::cut(&short, 0, 1000), Err(ClipError::Damaged(_))), "the movie box is cut");
    let m = mkv();
    let short = file(&dir, "short.mkv", &m[..m.len() / 2]);
    assert!(video::cut(&short, 0, 1000).is_err());
    let mut seed = 0x5eed_u64;
    for (i, base) in [whole, m].into_iter().enumerate() {
        for k in 0..16 {
            let mut bytes = base.clone();
            for _ in 0..8 {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let at = (seed >> 33) as usize % bytes.len();
                bytes[at] = (seed >> 13) as u8;
            }
            let path = file(&dir, &format!("bad-{i}-{k}.{}", if i == 0 { "mp4" } else { "mkv" }), &bytes);
            let _ = video::cut(&path, 300, 1300);
        }
    }
}

#[test]
fn save_frame_writes_the_frame_upright_with_the_videos_rotation_as_a_dated_jpeg_at_92() {
    let frame = Rgba8 { width: 160, height: 96, data: [200u8, 120, 40, 255].repeat(160 * 96) };
    let taken = DateTaken::parse_iso("2026-10-19T14:02:05").unwrap();
    let edits = vec!["rotate(90)".to_string(), "trim(500,1500)".to_string()];
    let jpeg = video::frame_picture(&frame, &edits, Some(taken.clone())).unwrap();
    let header = dalica_image::read_header(&jpeg, &Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (96, 160), "turned as the video is");
    assert_eq!(dalica_image::jpeg_quality::read(&jpeg).map(|s| s.quality), Some(FRAME_QUALITY));
    let meta = dalica_meta::read(&jpeg, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.owned.date_taken, Some(taken));
    let plain = video::frame_picture(&frame, &[], None).unwrap();
    let header = dalica_image::read_header(&plain, &Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (160, 96), "the frame's size");
    assert_eq!(video::frame_name("IMG_0042", 1_500), "IMG_0042 00-00-01.jpg");
}
