//! Videos in Export (`roadmap.md` M15.28; `creating.md` §2): an export of two photos and a
//! playable clip as first frame writes three JPEGs (the clip's is its poster frame, at the export's
//! size), as full video two JPEGs and the clip byte for byte; with a clip of a codec this build
//! does not decode added, first frame skips and counts it with the reason and full video copies
//! it. The clips are made here: H.264 with every macroblock I_PCM (ITU-T H.264 §7.3.5) in an MP4
//! of the ISO/IEC 14496-12 boxes; the same clip under a ProRes sample entry for the failed codec.

use crate::common;

use std::path::{Path, PathBuf};

use common::*;
use dalica_create::export::{self, ExportOptions, Item, Kind, Make, Outcome, QualityPreset, Size, VideoAs};
use dalica_edit::Recipe;
use dalica_meta::Changes;

const FALLBACK: &str = "Export 2026-10-20";

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

/// The clip's size in macroblocks (256 × 192) and frames; frames before the picture lights up.
const MBS: (usize, usize) = (16, 12);
const FRAMES: usize = 4;
const BLACK: usize = 2;

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

/// The luma of frame `k` at (`x`, `_y`): black frames first; then the poster frame, light on the
/// left half and mid grey on the right.
fn luma(k: usize, x: usize, _y: usize) -> u8 {
    if k < BLACK {
        16
    } else if x < 128 {
        220
    } else {
        120
    }
}

/// Picture `k` as one I slice of I_PCM macroblocks, chroma neutral.
fn slice(k: usize) -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, k as u64 % 16);
    if k == 0 {
        b.ue(0).u(1, 0).u(1, 0);
    } else {
        b.u(1, 0);
    }
    b.se(0).ue(1);
    for my in 0..MBS.1 {
        for mx in 0..MBS.0 {
            b.ue(25).align();
            for y in 0..16 {
                for x in 0..16 {
                    b.u(8, u64::from(luma(k, mx * 16 + x, my * 16 + y)));
                }
            }
            for _ in 0..128 {
                b.u(8, 128);
            }
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

/// An MP4 of [`FRAMES`] frames at 25 fps whose video sample entry is `code` (`avc1`: decodable).
fn clip(code: &[u8; 4]) -> Vec<u8> {
    let (s, p) = (sps(), pps());
    let samples: Vec<Vec<u8>> = (0..FRAMES)
        .map(|k| {
            let n = slice(k);
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
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((s.len() as u16).to_be_bytes());
    avcc.extend(&s);
    avcc.push(1);
    avcc.extend((p.len() as u16).to_be_bytes());
    avcc.extend(&p);
    entry.extend(bx(b"avcC", &avcc));
    let ftyp = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
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
    let stbl = [
        full_box(b"stsd", &[&be(1)[..], &bx(code, &entry)].concat()),
        full_box(b"stts", &stts),
        full_box(b"stsc", &[be(1), be(1), be(1), be(1)].concat()),
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
    [ftyp, bx(b"mdat", &samples.concat()), bx(b"moov", &[full_box(b"mvhd", &mvhd), trak].concat())].concat()
}

// --- The export -----------------------------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
    src: PathBuf,
    out: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    let out = dir.path().join("Exports");
    Fixture { src, out, _dir: dir }
}

impl Fixture {
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.src.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Two photos and a playable clip, in tray order: `a.jpg`, `clip.mp4`, `b.jpg`.
    fn tray(&self) -> Vec<Item> {
        let a = self.put("a.jpg", &jpeg(1, 320, 240, 90, 1));
        let clip = self.put("clip.mp4", &clip(b"avc1"));
        let b = self.put("b.jpg", &jpeg(2, 240, 320, 90, 1));
        vec![item(&a, Kind::Photo), item(&clip, Kind::PlayableVideo), item(&b, Kind::Photo)]
    }
}

fn item(path: &Path, kind: Kind) -> Item {
    Item { path: path.to_path_buf(), kind, recipe: Recipe::default(), changes: Changes::default() }
}

fn listing(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

fn run(f: &Fixture, folder: &str, items: Vec<Item>, videos: VideoAs) -> (export::Plan, export::Summary) {
    let options = ExportOptions { size: Size::LongSide(128), quality: QualityPreset::Normal, ..ExportOptions::default() };
    let plan = export::plan_as(&f.out, folder, FALLBACK, items, &options, videos).unwrap();
    let summary = export::run(&plan, &|| false);
    (plan, summary)
}

#[test]
fn as_first_frame_a_clip_is_its_poster_frame_as_a_jpeg() {
    let f = fixture();
    let (plan, s) = run(&f, "First", f.tray(), VideoAs::FirstFrame);
    assert_eq!(plan.items.iter().map(|p| p.make).collect::<Vec<_>>(), [Make::Picture, Make::FirstFrame, Make::Picture]);
    assert!(s.skipped.is_empty() && s.failed.is_empty() && s.copied.is_empty(), "{s:?}");
    assert_eq!(listing(&plan.destination.folder), ["a.jpg", "b.jpg", "clip.jpg"]);
    let frame = decode(&read(plan.destination.folder.join("clip.jpg")));
    // 256 × 192 fitted into 128: never enlarged, the clip's ratio kept.
    assert_eq!((frame.width, frame.height), (128, 96));
    // The poster is the first lit frame: light on the left, grey on the right (never the black
    // frames before it).
    let px = |x: u32, y: u32| frame.data[((y * frame.width + x) * 4 + 1) as usize];
    assert!(px(20, 48) > 190 && (90..150).contains(&px(108, 48)), "{} {}", px(20, 48), px(108, 48));
    let clip = s.written.iter().find(|w| w.index == 1).unwrap();
    assert_eq!(clip.report.format, dalica_create::output::OutFormat::Jpeg);
}

#[test]
fn as_full_video_a_clip_is_copied_byte_for_byte() {
    let f = fixture();
    let (plan, s) = run(&f, "Full", f.tray(), VideoAs::FullVideo);
    assert_eq!(listing(&plan.destination.folder), ["a.jpg", "b.jpg", "clip.mp4"]);
    assert_eq!(s.written.len(), 2);
    assert_eq!(s.copied.len(), 1);
    assert_eq!(read(plan.destination.folder.join("clip.mp4")), read(f.src.join("clip.mp4")), "byte for byte");
    // A clip whose name is taken in the export folder gets `-001`, never replacing the file.
    let again = export::export_one(&plan.items[1], &plan.destination, &|| false);
    let Outcome::Copied(c) = again else { panic!("{again:?}") };
    assert_eq!(c.name, "clip-001.mp4");
    assert_eq!(read(plan.destination.folder.join("clip.mp4")), read(f.src.join("clip.mp4")));
}

#[test]
fn a_clip_of_a_failed_codec_is_skipped_as_first_frame_and_copied_as_full_video() {
    let f = fixture();
    let mut tray = f.tray();
    let prores = f.put("prores.mov", &clip(b"apcn"));
    tray.push(item(&prores, Kind::Video));
    // The caller did not know the codec failed: the poster cannot be made, and the clip is
    // skipped with the decoder's reason.
    let unknown = f.put("unknown.mov", &clip(b"apcn"));
    tray.push(item(&unknown, Kind::PlayableVideo));

    let (plan, s) = run(&f, "First", tray.clone(), VideoAs::FirstFrame);
    assert_eq!(listing(&plan.destination.folder), ["a.jpg", "b.jpg", "clip.jpg"]);
    assert_eq!(s.skipped.iter().map(|k| k.index).collect::<Vec<_>>(), [3, 4], "both counted");
    assert_eq!(s.skipped[0].reason, export::NO_FIRST_FRAME);
    assert!(s.skipped[1].reason.contains("can't be played") && s.skipped[1].reason.contains("ProRes"), "{}", s.skipped[1].reason);

    let (plan, s) = run(&f, "Full", tray, VideoAs::FullVideo);
    assert!(s.skipped.is_empty() && s.failed.is_empty(), "{s:?}");
    assert_eq!(listing(&plan.destination.folder), ["a.jpg", "b.jpg", "clip.mp4", "prores.mov", "unknown.mov"]);
    assert_eq!(read(plan.destination.folder.join("prores.mov")), read(&prores));
}
