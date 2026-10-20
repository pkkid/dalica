//! Videos in the slideshow, headless (`roadmap.md` M15.27; `viewing.md` §1.1, §1.5): Options ▸
//! Slideshow's Sound group is live (Play music, its folder and Browse…, Play video sound) and Play
//! video sound turned on there is the setting; a show of a 2 s clip draws the clip's frames as
//! they fall due on the session's virtual clock (the silent `Sound`), then the photo after it; a
//! clip Dalica cannot play is drawn as its tile. The clips are H.264 made here with every
//! macroblock I_PCM, muxed by hand (the writer of `dalica-engine`'s `video_play.rs`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{LARGE, new_driver, widget};

/// Frames the driver settles in at most.
const SETTLE: usize = crate::common::FRAMES;
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_ui::panels::slideshow::show;
use dalica_ui::{App, Driver};
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(120);

// --- An H.264 writer: every macroblock I_PCM -------------------------------------------------------

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

const FPS: usize = 25;
const FRAME_MS: u64 = 40;
const GOP: usize = 10;
const MBS: (usize, usize) = (10, 6);
const FRAMES: usize = 50;
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

fn grey(k: usize) -> u8 {
    40 + 3 * k as u8
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

/// Frame `k`, length-prefixed: one I slice of I_PCM macroblocks of grey `grey(k)`.
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

// --- An MP4 writer -------------------------------------------------------------------------------------

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
    duration: u32,
    syncs: Option<Vec<u32>>,
    size: (u32, u32),
    samples: Vec<Vec<u8>>,
}

fn trak(t: &Track, at: &mut u32) -> Vec<u8> {
    let total = t.duration * t.samples.len() as u32;
    let mut tkhd = vec![0u8; 8];
    tkhd.extend(be(t.id));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be(total * 1000 / t.timescale));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(t.size.0 << 16));
    tkhd.extend(be(t.size.1 << 16));
    let mut mdhd = vec![0u8; 8];
    mdhd.extend(be(t.timescale));
    mdhd.extend(be(total));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(&t.entry);
    let stts = [be(1), be(t.samples.len() as u32), be(t.duration)].concat();
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

/// A sound entry: AAC-LC mono at 48 kHz (`mp4a`), or another code.
fn sound_entry(code: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(code, &p)
}

/// Two seconds of 160 × 96 at 25 frames a second (a key frame every 10) with a sound track; the
/// video's sample entry `video` and the sound's `sound`.
fn clip(video: &[u8; 4], sound: &[u8; 4]) -> Vec<u8> {
    let packets = (FRAMES * 48_000 / FPS).div_ceil(1024);
    let tracks = [
        Track {
            id: 1,
            handler: *b"vide",
            entry: avc1(video),
            timescale: 1000,
            duration: FRAME_MS as u32,
            syncs: Some((0..FRAMES as u32).filter(|k| (*k as usize).is_multiple_of(GOP)).map(|k| k + 1).collect()),
            size: ((MBS.0 * 16) as u32, (MBS.1 * 16) as u32),
            samples: (0..FRAMES).map(frame).collect(),
        },
        Track {
            id: 2,
            handler: *b"soun",
            entry: sound_entry(sound),
            timescale: 48_000,
            duration: 1024,
            syncs: None,
            size: (0, 0),
            samples: vec![SILENT_AAC.to_vec(); packets],
        },
    ];
    let mut out = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let data: Vec<u8> = tracks.iter().flat_map(|t| t.samples.concat()).collect();
    let mut at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &data));
    let traks: Vec<u8> = tracks.iter().flat_map(|t| trak(t, &mut at)).collect();
    // Recorded 2026-10-19 14:02:00 UTC (seconds since 1904), modified then too.
    let mut mvhd = [be(3_875_263_320), be(3_875_263_320)].concat();
    mvhd.extend(be(1000));
    mvhd.extend(be((FRAMES as u64 * FRAME_MS) as u32));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    out.extend(bx(b"moov", &[full(b"mvhd", &mvhd), traks].concat()));
    out
}

struct Fx {
    _dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    d: Driver,
}

/// A folder holding a photo, a playable clip and a clip of a codec Dalica cannot play, on a virtual
/// clock, in the large window.
fn setup() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    let photo = dalica_scenes::jpeg(&dalica_scenes::Scene::new(5, 320, 240), 85, None).unwrap();
    std::fs::write(folder.join("a.jpg"), photo).unwrap();
    std::fs::write(folder.join("clip.mp4"), clip(b"avc1", b"mp4a")).unwrap();
    std::fs::write(folder.join("old.mov"), clip(b"mp4v", b"mp4a")).unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_780_000_000_000)));
    let mut s = Session::open(&dir.path().join("library"), clock.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(SETTLE).unwrap();
    Fx { _dir: dir, clock, d }
}

fn id(f: &Fx, name: &str) -> PhotoId {
    f.d.app().session().unwrap().library().photos().find(|p| p.name == name).unwrap().id
}

/// Steps frames (real time) until `done` holds.
fn until(f: &mut Fx, what: &str, done: impl Fn(&Fx) -> bool) {
    let start = Instant::now();
    while !done(f) {
        assert!(start.elapsed() < PATIENCE, "{what}");
        f.d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_sound_rows_are_live_and_the_clips_frames_show_in_the_show() {
    let mut f = setup();
    f.d.run("app.preferences", &json!({ "tab": "slideshow" })).unwrap();
    f.d.settle(SETTLE).unwrap();
    for wid in ["check:option.slideshow.music", "check:option.slideshow.videoSound"] {
        assert!(widget(&f.d, wid).enabled, "{wid} is live");
    }
    // Browse… opens a folder picker where there is one (none headless).
    let can_pick = f.d.app().can_pick().is_ok();
    assert_eq!(widget(&f.d, "button:option.slideshow.musicFolder/browse").enabled, can_pick);
    f.d.click("check:option.slideshow.videoSound").unwrap();
    f.d.settle(SETTLE).unwrap();
    f.d.click("button:dialog.ok").unwrap();
    f.d.settle(SETTLE).unwrap();
    assert!(f.d.app().session().unwrap().settings().slideshow().video_sound, "Play video sound is the setting");

    let (clip, photo, old) = (id(&f, "clip.mp4"), id(&f, "a.jpg"), id(&f, "old.mov"));
    f.d.run("view.slideshow", &json!({ "ids": [clip, photo, old] })).unwrap();
    f.d.step();
    until(&mut f, "the clip's first frame drawn", |_| show::clip_shown() == Some((clip, 0)));
    // Its frames as they fall due, then the photo after it.
    for k in 1..=4usize {
        f.clock.advance(Duration::from_millis(400));
        until(&mut f, &format!("frame {}", 10 * k), |_| show::clip_shown() == Some((clip, 10 * k)));
    }
    f.clock.advance(Duration::from_millis(400));
    until(&mut f, "the photo after the clip", |f| f.d.state().slideshow.as_ref().is_some_and(|s| s.index == 1));
    assert_eq!(show::clip_shown(), None, "the photo is no clip");
    // The clip Dalica cannot play: its tile for the display time, no frames.
    f.clock.advance(Duration::from_millis(3_000));
    until(&mut f, "the clip it cannot play", |f| f.d.state().slideshow.as_ref().is_some_and(|s| s.index == 2));
    f.d.step();
    assert_eq!(show::clip_shown(), None);
    f.d.run("slideshow.exit", &json!({})).unwrap();
}
