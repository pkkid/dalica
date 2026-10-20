//! Videos in the Export dialog and the Collage room (`roadmap.md` M15.28; `creating.md` §2–§3),
//! headless: the "Export videos as" row is live with a video among what is exported and disabled
//! without one; Full video sends `videos: "fullVideo"` and the clip is copied as it is; in the
//! Collage room the clip's cell carries its duration pill. The clip is H.264 made here (every
//! macroblock I_PCM, ITU-T H.264 §7.3.5) in an MP4 of the ISO/IEC 14496-12 boxes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_ui::dialogs::export::{FIRST_FRAME_ID, FULL_VIDEO_ID, LOCATION_ID};
use dalica_ui::panels::collage::clips::duration_id;
use dalica_ui::state::Videos;
use dalica_ui::{App, Driver};
use serde_json::json;

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
const CLIP_FRAMES: usize = 4;
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

/// An MP4 of [`CLIP_FRAMES`] frames at 25 fps whose video sample entry is `code` (`avc1`: decodable).
fn clip(code: &[u8; 4]) -> Vec<u8> {
    let (s, p) = (sps(), pps());
    let samples: Vec<Vec<u8>> = (0..CLIP_FRAMES)
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
    stts.extend(be(CLIP_FRAMES as u32));
    stts.extend(be(40));
    let mut stsz = be(0).to_vec();
    stsz.extend(be(CLIP_FRAMES as u32));
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
    let duration = CLIP_FRAMES as u32 * 40;
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

// --- The window -----------------------------------------------------------------------------------

struct Fx {
    dir: tempfile::TempDir,
    d: Driver,
    /// a.jpg, b.jpg, clip.mp4.
    ids: [PhotoId; 3],
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let trip = dir.path().join("Photos").join("Trip");
    dalica_fs::create_dir_all(&trip).unwrap();
    for (name, seed) in [("a.jpg", 1), ("b.jpg", 2)] {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 160, 120), 85, None).unwrap();
        dalica_fs::replace(&trip.join(name), &jpeg, Expect::Absent).unwrap();
    }
    dalica_fs::replace(&trip.join("clip.mp4"), &clip(b"avc1"), Expect::Absent).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(dir.path().join("Pictures")));
    s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
    s.wait_idle();
    let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name}")).id;
    let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4")];
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, d, ids }
}

fn enabled(d: &Driver, id: &str) -> bool {
    try_widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).enabled
}

fn open_export(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
    d.run("output.export", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
}

fn close_dialog(d: &mut Driver) {
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
}

fn files(folder: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn the_videos_row_is_live_with_a_video_held_and_full_video_copies_the_clip() {
    let mut fx = fixture();
    let [a, b, clip_id] = fx.ids;

    // Photos only: the row is disabled.
    open_export(&mut fx.d, &[a, b]);
    assert!(!enabled(&fx.d, FIRST_FRAME_ID) && !enabled(&fx.d, FULL_VIDEO_ID), "no video, no row");
    close_dialog(&mut fx.d);

    // With the clip: live, First frame by default; Full video picked.
    open_export(&mut fx.d, &[a, clip_id, b]);
    assert!(enabled(&fx.d, FIRST_FRAME_ID) && enabled(&fx.d, FULL_VIDEO_ID), "a video held: the row is live");
    assert!(widget(&fx.d, FIRST_FRAME_ID).active && !widget(&fx.d, FULL_VIDEO_ID).active);
    shot(&fx.d, "export-videos-row-1400x900");
    fx.d.click(FULL_VIDEO_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().export.as_ref().unwrap().videos, Videos::FullVideo);
    assert!(widget(&fx.d, FULL_VIDEO_ID).active);
    let out: PathBuf = fx.dir.path().join("Out");
    fx.d.click(LOCATION_ID).unwrap();
    fx.d.key("Ctrl+A").unwrap();
    fx.d.type_text(out.to_str().unwrap()).unwrap();
    fx.d.key("Tab").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("button:dialog.export").unwrap();
    fx.d.settle(FRAMES).unwrap();
    let s = fx.d.app_mut().session_mut().unwrap();
    assert!(s.wait_file_jobs(Duration::from_secs(120)));
    let folder = std::fs::read_dir(&out).unwrap().next().unwrap().unwrap().path();
    assert_eq!(files(&folder), [".dalica.json", "a.jpg", "b.jpg", "clip.mp4"]);
    assert_eq!(std::fs::read(folder.join("clip.mp4")).unwrap(), std::fs::read(fx.dir.path().join("Photos").join("Trip").join("clip.mp4")).unwrap());
}

#[test]
fn a_clip_in_the_collage_room_carries_its_duration_pill() {
    let mut fx = fixture();
    let [a, b, clip_id] = fx.ids;
    fx.d.run("select.set", &json!({ "ids": [a, clip_id, b] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("action:collage").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.app_mut().session_mut().unwrap().wait_idle();
    fx.d.click("tab:collage/clips").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(widget(&fx.d, "tab:collage/clips").label, "Clips (3)");
    // 4 frames at 25 fps: 0.16 s, shown as a second at least.
    assert_eq!(widget(&fx.d, &duration_id(clip_id)).label, "0:01");
    assert!(try_widget(&fx.d, &duration_id(a)).is_none(), "a photo has no pill");
    shot(&fx.d, "collage-video-clips-1400x900");
}
