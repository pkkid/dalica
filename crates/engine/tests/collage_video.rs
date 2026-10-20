//! Videos in collages (`roadmap.md` M15.28; `creating.md` §3): in a library of two photos, a
//! playable clip and a clip of a codec this build does not decode (H.264 with every macroblock
//! I_PCM, ITU-T H.264 §7.3.5, in an MP4 of the ISO/IEC 14496-12 boxes; the same clip under a ProRes
//! sample entry), a collage takes the playable clip at its poster frame's ratio and leaves the
//! other out with M15.16's notice; Create Collage draws the clip's poster frame in its place, its
//! recipe holds the clip, and Edit Collage reopens it with the clip; the owner thread makes no
//! `fs` call. The same library's export (`creating.md` §2, the session's half of
//! `crates/create/tests/export_video.rs`): as first frames the playable clip's poster is a JPEG and
//! the other is skipped with its codec named; as full videos both are copied byte for byte.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_create::collage::Recipe;
use dalica_engine::{CacheDir, JobEntry, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::{jpeg_of, read};

const PATIENCE: Duration = Duration::from_secs(120);

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

// --- The library ----------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    /// a.jpg, b.jpg, clip.mp4 (plays), prores.mov (does not).
    ids: [PhotoId; 4],
}

impl Lib {
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Photos").join("Trip");
        dalica_fs::create_dir_all(&trip).unwrap();
        dalica_fs::replace(&trip.join("a.jpg"), &jpeg_of(1, 160, 120), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("b.jpg"), &jpeg_of(2, 120, 160), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("clip.mp4"), &clip(b"avc1"), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("prores.mov"), &clip(b"apcn"), Expect::Absent).unwrap();
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id;
        let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4"), id("prores.mov")];
        Lib { dir, s, ids }
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// Runs `id` and waits for its job and the scans it started.
    fn output(&mut self, id: &str, p: Value) -> JobEntry {
        let reply = self.run(id, p);
        let job = reply["job"].as_u64().unwrap_or_else(|| panic!("no job in {reply}"));
        let entry = self.s.wait_job(job, PATIENCE).unwrap();
        assert!(entry.state.is_finished(), "{entry:?}");
        self.s.wait_idle();
        entry
    }
}

/// The mean green of the page over the box `(x0, y0)–(x1, y1)` in fractions of its sides.
fn mean(img: &dalica_image::Rgba8, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let (xa, xb, ya, yb) = ((x0 * w) as usize, (x1 * w) as usize, (y0 * h) as usize, (y1 * h) as usize);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in ya..yb.max(ya + 1) {
        for x in xa..xb.max(xa + 1) {
            sum += f64::from(img.data[(y * img.width as usize + x) * 4 + 1]);
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn a_collage_places_a_playable_clips_poster_and_leaves_a_failed_codec_out() {
    let mut l = Lib::new();
    let [a, b, clip_id, prores] = l.ids;

    // The room: the playable clip is a clip like a photo; the failed codec is left out and said.
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let opened = l.run("create.collage", json!({ "ids": [a, clip_id, prores, b], "seed": "5eed" }));
    let added = l.run("collage.clips.add", json!({ "ids": [prores] }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!((opened["clips"].clone(), opened["videos"].clone()), (json!(3), json!(1)), "{opened}");
    assert_eq!(opened["notice"], "1 video left out: videos can't go in a collage yet.");
    assert_eq!((added["added"].clone(), added["videos"].clone()), (json!(0), json!(1)), "{added}");
    let doc = l.s.collage().document().cloned().unwrap();
    assert_eq!(doc.clips.iter().map(|c| c.id).collect::<Vec<_>>(), [a, clip_id, b], "in tray order");
    // Laid out at the poster frame's ratio (256 × 192).
    let aspects = l.s.collage_aspects();
    assert!((aspects[1] - 256.0 / 192.0).abs() < 1e-9, "{aspects:?}");

    // One picture on the page, so its place is known: the clip alone, in a grid without spacing.
    l.run("collage.unplace", json!({ "ids": [a, b] }));
    l.run("collage.set", json!({ "style": "grid", "shadows": false }));
    let place = l.s.collage_placements().into_iter().find(|p| p.id == clip_id).expect("the clip is placed");
    let entry = l.output("collage.create", json!({}));
    let jpeg = PathBuf::from(entry.result.unwrap()["path"].as_str().unwrap().to_string());
    let page = dalica_image::decode(&read(&jpeg), &dalica_image::Limits::default()).unwrap();
    // The poster frame (light on its left half, grey on its right), never the black first frames.
    let at = |fx: f64| {
        let shown = place.shown;
        let f = (fx - shown.x0()) / shown.width();
        let x = place.centre[0] + (f - 0.5) * place.size[0];
        let y = place.centre[1];
        let (dx, dy) = (place.size[0] * 0.05, place.size[1] * 0.1);
        mean(&page, (x - dx, y - dy, x + dx, y + dy))
    };
    let (left, right) = (at(0.2), at(0.8));
    assert!(left > 200.0 && (95.0..150.0).contains(&right), "left {left}, right {right}");

    // Its recipe holds the clip as any clip, and Edit Collage reopens it with the clip.
    let stem = jpeg.file_stem().unwrap().to_string_lossy().into_owned();
    let recipe = Recipe::parse(&read(&jpeg.with_file_name(format!("{stem}.dalica-collage")))).unwrap();
    assert!(recipe.collage.clips.iter().any(|c| c.id == clip_id && c.on_page));
    let photo = l.s.collage_made().unwrap().photo.expect("the collage is in the library");
    l.run("collage.close", json!({ "confirm": true }));
    let entry = l.output("collage.edit", json!({ "id": photo }));
    assert_eq!(entry.result.unwrap(), json!({ "opened": true, "clips": 3, "missing": [] }));
    let reopened = l.s.collage().document().cloned().unwrap();
    assert!(reopened.clips.iter().any(|c| c.id == clip_id && c.on_page), "{reopened:?}");
    l.run("collage.close", json!({ "confirm": true }));

    // The failed codec alone: nothing to open, and why.
    let why = l.s.execute("create.collage", &json!({ "ids": [prores] })).unwrap_err().to_string();
    assert!(why.contains("Videos can't go in a collage yet."), "{why}");
    let _ = &l.dir;
}

#[test]
fn an_export_takes_the_playable_clips_first_frame_or_copies_both_clips() {
    let mut l = Lib::new();
    let ids = l.ids;
    let out = l.dir.path().join("Out");
    let files = |folder: PathBuf| {
        let mut v: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    };
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let reply = l.run("export.run", json!({ "ids": ids, "dir": out, "folderName": "First", "size": 100 }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!((reply["total"].clone(), reply["videos"].clone()), (json!(3), json!(1)), "{reply}");
    let job = reply["job"].as_u64().unwrap();
    let done = l.s.wait_job(job, PATIENCE).unwrap();
    let result = done.result.unwrap();
    assert_eq!(result["skipped"][0]["reason"], "This video can't be played (ProRes)", "{result}");
    assert_eq!(done.label, "Exported 3 pictures; 1 skipped");
    assert_eq!(files(out.join("First")), [".dalica.json", "a.jpg", "b.jpg", "clip.jpg"]);
    let (header, _) = dalica_image::read_header_file(&out.join("First").join("clip.jpg"), &dalica_image::Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (100, 75), "the poster at the export's size");

    let reply = l.run("export.run", json!({ "ids": ids, "dir": out, "folderName": "Full", "size": 100, "videos": "fullVideo" }));
    assert_eq!((reply["total"].clone(), reply["videos"].clone()), (json!(4), json!(0)), "{reply}");
    let done = l.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(done.label, "Exported 2 pictures and 2 videos");
    assert_eq!(files(out.join("Full")), [".dalica.json", "a.jpg", "b.jpg", "clip.mp4", "prores.mov"]);
    let trip = l.dir.path().join("Photos").join("Trip");
    for name in ["clip.mp4", "prores.mov"] {
        assert_eq!(read(&out.join("Full").join(name)), read(&trip.join(name)), "{name} byte for byte");
    }
    let bad = l.s.execute("export.run", &json!({ "ids": ids, "dir": out, "folderName": "X", "videos": "both" }));
    assert!(bad.is_err());
    let only = l.s.execute("export.run", &json!({ "ids": [ids[3]], "dir": out, "folderName": "Y" })).unwrap_err().to_string();
    assert!(only.contains("full videos"), "{only}");
}
