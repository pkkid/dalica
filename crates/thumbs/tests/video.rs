//! Videos' poster frames (`roadmap.md` M15.20; `docs/video-decoders.md`): an H.264 clip made here
//! (every macroblock I_PCM, ITU-T H.264 §7.3.5, in an MP4 of the ISO/IEC 14496-12 boxes) that starts
//! black has its first lit frame as its thumbnail at every ladder size, cached and made again when
//! the file changes; a clip of a codec this build does not decode (HEVC without `dalica-image`'s
//! `hevc` feature, ProRes, an unknown code) is not played: it gives no entry, only its reason.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use dalica_image::Limits;
use dalica_image::formats::video::frames::{FrameOptions, FrameSource};
use dalica_thumbs::ladder::{self, Error, Render, Source};
use dalica_thumbs::pack::{Cache, Options, Size};
use dalica_thumbs::{Rgba8, video};

const SCREEN: u32 = 2560;

// --- An H.264 clip -------------------------------------------------------------------------------

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
        let len = 64 - (v + 1).leading_zeros();
        self.u(len - 1, 0).u(len, v + 1)
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

/// A NAL unit with its header and emulation prevention.
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

/// Baseline, level 4, POC type 2, `mbs` macroblocks a side.
fn sps((w, h): (usize, usize)) -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 40).ue(0);
    b.ue(0).ue(2).ue(1).u(1, 0);
    b.ue(w as u64 - 1).ue(h as u64 - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0);
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 8, &b.trailing())
}

/// Picture `k` as one I slice of I_PCM macroblocks, every luma sample `luma`, chroma neutral.
fn slice(k: usize, (w, h): (usize, usize), luma: u8) -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, k as u64 % 16);
    if k == 0 {
        b.ue(0).u(1, 0).u(1, 0);
    } else {
        b.u(1, 0);
    }
    b.se(0).ue(1);
    for _ in 0..w * h {
        b.ue(25).align();
        for _ in 0..256 {
            b.u(8, u64::from(luma));
        }
        for _ in 0..128 {
            b.u(8, 128);
        }
    }
    nal(if k == 0 { 3 } else { 2 }, if k == 0 { 5 } else { 1 }, &b.trailing())
}

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0u8, 0, 0, 0][..], payload].concat())
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const MATRIX: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

/// The clip's macroblocks a side (640 × 368) and frames.
const MBS: (usize, usize) = (40, 23);
const FRAMES: usize = 6;
/// Frames before the picture lights up.
const BLACK: usize = 2;

/// The luma of frame `k`: black, then a grey per frame.
fn luma(k: usize) -> u8 {
    if k < BLACK { 16 } else { 120 + 20 * k as u8 }
}

/// The 8-bit sRGB grey frame `k` decodes to (limited range).
fn grey(k: usize) -> u8 {
    ((f64::from(luma(k)) - 16.0) / 219.0 * 255.0).round() as u8
}

/// An MP4 of [`FRAMES`] frames at 25 fps whose video sample entry is `code` (`avc1`: decodable).
fn clip(code: &[u8; 4]) -> Vec<u8> {
    let (s, p) = (sps(MBS), pps());
    let samples: Vec<Vec<u8>> = (0..FRAMES)
        .map(|k| {
            let n = slice(k, MBS, luma(k));
            [&be(n.len() as u32)[..], &n].concat()
        })
        .collect();
    let (w, h) = ((MBS.0 * 16) as u16, (MBS.1 * 16) as u16);
    let mut entry = vec![0u8; 6];
    entry.extend([0, 1]);
    entry.extend([0u8; 16]);
    entry.extend(w.to_be_bytes());
    entry.extend(h.to_be_bytes());
    entry.extend(be(0x48_0000));
    entry.extend(be(0x48_0000));
    entry.extend([0u8; 4]);
    entry.extend([0, 1]);
    entry.extend([0u8; 32]);
    entry.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 40, 0xff, 0xe1];
    avcc.extend((s.len() as u16).to_be_bytes());
    avcc.extend(&s);
    avcc.push(1);
    avcc.extend((p.len() as u16).to_be_bytes());
    avcc.extend(&p);
    entry.extend(bx(b"avcC", &avcc));
    let ftyp = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let data = samples.concat();
    let mut at = ftyp.len() as u32 + 8;
    let mut stco = be(samples.len() as u32).to_vec();
    for s in &samples {
        stco.extend(be(at));
        at += s.len() as u32;
    }
    let mut stts = be(1).to_vec();
    stts.extend(be(FRAMES as u32));
    stts.extend(be(40));
    let mut stsz = be(0).to_vec();
    stsz.extend(be(FRAMES as u32));
    for s in &samples {
        stsz.extend(be(s.len() as u32));
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let stbl = [
        full_box(b"stsd", &[&be(1)[..], &bx(code, &entry)].concat()),
        full_box(b"stts", &stts),
        full_box(b"stsc", &stsc),
        full_box(b"stsz", &stsz),
        full_box(b"stco", &stco),
        full_box(b"stss", &[be(1), be(1)].concat()),
    ]
    .concat();
    let duration = FRAMES as u32 * 40;
    let mut tkhd = vec![0u8; 36];
    tkhd.extend(MATRIX.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(u32::from(w) << 16));
    tkhd.extend(be(u32::from(h) << 16));
    let mdhd = [&[0u8; 8][..], &be(1000), &be(duration), &[0x55, 0xc4, 0, 0]].concat();
    let hdlr = [&[0u8; 4][..], b"vide", &[0u8; 13]].concat();
    let mdia = bx(b"mdia", &[full_box(b"mdhd", &mdhd), full_box(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    let trak = bx(b"trak", &[full_box(b"tkhd", &tkhd), mdia].concat());
    let mut mvhd = [&[0u8; 8][..], &be(1000), &be(duration), &be(0x1_0000), &[1, 0], &[0u8; 10]].concat();
    mvhd.extend(MATRIX.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(2));
    [ftyp, bx(b"mdat", &data), bx(b"moov", &[full_box(b"mvhd", &mvhd), trak].concat())].concat()
}

// --- The tests ------------------------------------------------------------------------------------

fn write(dir: &Path, name: &str, bytes: &[u8], mtime_ms: i64) -> Source {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    Source { path, file_size: bytes.len() as u64, mtime_ms }
}

fn cache(dir: &Path) -> Cache {
    Cache::open(&dir.join("thumbs"), Options::default()).unwrap()
}

/// The mean of the red channel (the picture is grey).
fn mean(img: &Rgba8) -> f64 {
    let px = img.data.chunks(4);
    let n = px.len().max(1) as f64;
    px.map(|p| f64::from(p[0])).sum::<f64>() / n
}

#[test]
fn a_playable_clip_has_its_first_lit_frame_as_thumbnail_at_every_size_cached() {
    assert!(video::plays("avc1") && video::plays("avc3") && video::plays("V_MPEG4/ISO/AVC"), "H.264 always decodes");
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "clip.mp4", &clip(b"avc1"), 1_700_000_000_000);
    let cache = cache(dir.path());
    let (render, limits, go) = (Render::default(), Limits::default(), AtomicBool::new(false));
    // The poster is the first frame that clears the luma threshold, never the black start.
    let direct = FrameSource::open(&source.path, &limits, FrameOptions::default()).unwrap().poster().unwrap();
    assert_eq!(direct.index, BLACK);
    let lit = f64::from(grey(BLACK));
    let started = std::time::Instant::now();
    let scanned = video::scan(&cache, &source, &render, true, &limits, &go).unwrap();
    let scan_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(scanned.decoded && scanned.sketch.is_some());
    let sizes: Vec<(Size, (u32, u32))> = scanned.images.iter().map(|(s, i)| (*s, (i.width, i.height))).collect();
    assert_eq!(sizes, [(Size::S128, (128, 74)), (Size::S256, (256, 147))]);
    for (_, image) in &scanned.images {
        assert!((mean(image) - lit).abs() < 3.0, "the lit frame ({lit}), not black: {}", mean(image));
    }
    // Every ladder size: the poster fitted to it, never enlarged past the clip's 640 × 368.
    for (size, want) in
        [(Size::S128, (128, 74)), (Size::S256, (256, 147)), (Size::S512, (512, 294)), (Size::S1024, (640, 368)), (Size::Screen, (640, 368))]
    {
        let fetched = video::fetch(&cache, &source, &render, size, SCREEN, &limits, &go).unwrap();
        assert_eq!((fetched.image.width, fetched.image.height), want, "{size:?}");
        assert!((mean(&fetched.image) - lit).abs() < 3.0, "{size:?}");
        assert_eq!(fetched.decoded, !matches!(size, Size::S128 | Size::S256), "{size:?}: the scan's entries are read from the cache");
        let again = video::fetch(&cache, &source, &render, size, SCREEN, &limits, &go).unwrap();
        assert!(!again.decoded, "{size:?} is cached");
        assert_eq!(again.image, fetched.image);
    }
    // Under the key a photo of the same file would have: one cache, one shape.
    assert!(cache.contains(Size::S128, source.key_for(&render)));
    let rescan = video::scan(&cache, &source, &render, false, &limits, &go).unwrap();
    assert!(!rescan.decoded, "a second scan reads nothing");
    // The file changed (a new modification time): made again under a new key.
    let changed = Source { mtime_ms: source.mtime_ms + 1000, ..source.clone() };
    assert!(!cache.contains(Size::S128, changed.key_for(&render)));
    assert!(video::scan(&cache, &changed, &render, false, &limits, &go).unwrap().decoded);
    eprintln!("poster scan (128 + 256 + sketch) of a {}-frame 640 × 368 clip: {scan_ms:.1} ms", FRAMES);
}

#[test]
fn a_clip_of_a_codec_this_build_does_not_decode_gives_no_entry_and_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path());
    let (render, limits, go) = (Render::default(), Limits::default(), AtomicBool::new(false));
    let hevc = dalica_image::formats::video::playability(&dalica_image::formats::video::VideoInfo {
        video_codec: Some("hvc1".into()),
        ..dalica_image::formats::video::probe(&clip(b"avc1"), &limits).unwrap()
    })
    .video
    .is_ok();
    assert_eq!(video::plays("hvc1"), hevc, "HEVC only with dalica-image's `hevc` feature");
    if !hevc {
        assert!(!video::plays("hvc1") && !video::plays("hev1") && !video::plays("V_MPEGH/ISO/HEVC"));
        assert_eq!(video::why_not("hvc1").as_deref(), Some("This video can't be played (HEVC support isn't included in this build)"));
    }
    // Opened anyway (the engine never does: it asks `plays` first), the frame source refuses them.
    let refused = [(b"apcn", "ProRes"), (b"zzzz", "zzzz")];
    assert!(!video::plays(""), "an unknown codec");
    for (code, name) in refused {
        let text = std::str::from_utf8(code).unwrap();
        assert!(!video::plays(text), "{name}");
        let source = write(dir.path(), &format!("{text}.mov"), &clip(code), 1_700_000_000_000);
        let why = video::scan(&cache, &source, &render, true, &limits, &go).unwrap_err();
        assert!(matches!(&why, Error::Unplayable(r) if r.starts_with("This video can't be played") && r.contains(name)), "{why:?}");
        for size in Size::ALL {
            assert!(!cache.contains(size, ladder::entry_key(source.key_for(&render), size, SCREEN)), "{name}: no entry");
        }
    }
    // A damaged clip of a playable codec: its decode error, no entry, no panic.
    let mut cut = clip(b"avc1");
    cut.truncate(cut.len() / 2);
    let source = write(dir.path(), "cut.mp4", &cut, 1_700_000_000_000);
    assert!(matches!(video::scan(&cache, &source, &render, true, &limits, &go), Err(Error::Photo(_))));
    // Cancelled before the decode.
    let source = write(dir.path(), "late.mp4", &clip(b"avc1"), 1_700_000_000_000);
    assert_eq!(video::scan(&cache, &source, &render, true, &limits, &AtomicBool::new(true)), Err(Error::Cancelled));
}
