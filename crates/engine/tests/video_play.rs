//! The edit view's video player and a video's outputs (`roadmap.md` M15.25; `editing.md` §11,
//! `creating.md` §4), on the silent `Sound` and a virtual clock: playing a second of its clock shows
//! the frames due within one frame, and a jump drops the frames in between rather than showing
//! them late; play respects the trim; Set Start and Set End write a trim that survives a restart;
//! steps and seeks land on their frames; a decoder slower than the clip (a stand-in on the virtual
//! clock) shows about as many frames as it decodes, moving forward, never held, the picture caught
//! up with the clock at each key frame; Save Frame at 1.5 s writes a JPEG of the frame's size into
//! the Video Frames project, dated the clip's date plus the frame's time; Export Clip copies the
//! trimmed part beside the clip; a clip of a codec Dalica cannot play opens with "This video can't
//! be played (MPEG-4 Part 2)" and its playback commands refuse with it and write nothing while its
//! trim and Export Clip work; a clip whose sound Dalica cannot decode plays muted and refuses the
//! volume with the reason; the owner thread makes no `fs` call. The clips are H.264 made here with
//! every macroblock I_PCM (ITU-T H.264 §7.3.5), muxed by hand from ISO/IEC 14496-12.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dalica_core::{Clock, PhotoId, VirtualClock};
use dalica_engine::sound::{Format, Silent};
use dalica_engine::{CmdError, JobState, Session};
use dalica_fs::Expect;
use dalica_image::Rgba8;
use dalica_image::formats::video::frames::{Frame, FrameError};
use dalica_library::Project;
use serde_json::{Value, json};

use crate::common::{exists, jpeg, open_with, read, virtual_clock};

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

// --- The library -------------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    sound: Arc<Silent>,
    s: Session,
}

impl Lib {
    /// A library watching `Photos`, whose `Trip` holds a photo and three clips: `clip.mp4` (H.264
    /// and AAC), `old.mov` (MPEG-4 Part 2) and `muted.mp4` (AC-3 sound); `Pictures` is the user's
    /// Pictures folder. The sound output is a silent one on the session's virtual clock.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Photos").join("Trip");
        dalica_fs::create_dir_all(&trip).unwrap();
        dalica_fs::replace(&trip.join("a.jpg"), &jpeg(1), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("clip.mp4"), &clip(b"avc1", b"mp4a"), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("old.mov"), &clip(b"mp4v", b"mp4a"), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("muted.mp4"), &clip(b"avc1", b"ac-3"), Expect::Absent).unwrap();
        let (clock, s) = Lib::session(dir.path());
        let sound = Arc::new(Silent::new(clock.clone()));
        let mut lib = Lib { dir, clock, sound, s };
        lib.s.set_sound(lib.sound.clone());
        lib.s.execute("folders.add", &json!({ "path": lib.dir.path().join("Photos") })).unwrap();
        lib.s.wait_idle();
        lib
    }

    fn session(dir: &Path) -> (Arc<VirtualClock>, Session) {
        let clock = virtual_clock();
        let mut s = open_with(&dir.join("library"), clock.clone());
        s.set_pictures_dir(Some(dir.join("Pictures")));
        (clock, s)
    }

    fn trip(&self) -> PathBuf {
        self.dir.path().join("Photos").join("Trip")
    }

    fn frames_dir(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Dalica").join("Video Frames")
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn state(&mut self) -> Value {
        self.run("video.state", json!({}))
    }

    /// Polls `video.state` (real time) until `done` holds.
    fn until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let v = self.state();
            if done(&v) {
                return v;
            }
            assert!(start.elapsed() < PATIENCE, "{what}: {v}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Opens `name` in the player and waits until its worker has it.
    fn open(&mut self, name: &str) -> Value {
        let id = self.id(name);
        self.run("video.open", json!({ "id": id }));
        self.until("opened", |v| v["ready"] == true && v["frame"].is_object())
    }

    /// Advances the virtual clock by `ms` and waits until the position follows it to `want`.
    fn advance_to(&mut self, ms: u64, want: u64) -> Value {
        self.clock.advance(Duration::from_millis(ms));
        self.until(&format!("the position at {want}"), |v| v["positionMs"] == want)
    }

    /// Waits until the frame shown is the one due at the position: shown at or before it, its
    /// successor not yet.
    fn frame_due(&mut self) -> Value {
        self.until("the frame due", |v| {
            let (Some(pos), Some(t)) = (v["positionMs"].as_u64(), v["frame"]["timeMs"].as_u64()) else { return false };
            t <= pos && pos - t < FRAME_MS
        })
    }
}

fn disabled(r: Result<Value, CmdError>) -> String {
    match r {
        Err(CmdError::Disabled(why)) => why,
        other => panic!("expected disabled, got {other:?}"),
    }
}

/// Runs `f` with the owner thread marked: it must make no `fs` call.
fn on_owner<T>(f: impl FnOnce() -> T) -> T {
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let out = f();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    out
}

// --- The tests -----------------------------------------------------------------------------------------

#[test]
fn playing_a_second_of_the_sound_clock_shows_the_frames_due_within_one_frame() {
    let mut lib = Lib::new();
    on_owner(|| {
        let v = lib.open("clip.mp4");
        assert_eq!((v["playable"].clone(), v["sound"].clone(), v["frameCount"].clone()), (json!(true), json!("plays"), json!(FRAMES)));
        assert_eq!((v["positionMs"].clone(), v["frame"]["index"].clone(), v["durationMs"].clone()), (json!(0), json!(0), json!(2000)));
        let v = lib.run("video.play", json!({}));
        assert_eq!(v["playing"], true);
        assert_eq!(lib.sound.format(), Some(Format { rate: 48_000, channels: 1 }), "the sound opened at the clip's format");
        // A frame at a time: each shows when it is due.
        for k in 1..=10u64 {
            lib.advance_to(FRAME_MS, k * FRAME_MS);
            let v = lib.frame_due();
            assert_eq!(v["frame"]["index"], k, "{v}");
        }
        // The rest of the second at once: the frame due shows, those in between are dropped.
        let v = lib.advance_to(600, 1000);
        let v2 = lib.frame_due();
        assert_eq!((v["playing"].clone(), v2["frame"]["index"].clone()), (json!(true), json!(25)));
        assert!(v2["dropped"].as_u64().unwrap() > 0, "frames dropped rather than shown late: {v2}");
        assert!(lib.sound.samples().len() >= 48_000, "a second of the clip's sound was queued");
        // Pause holds the position and closes the sound; play goes on from there.
        let v = lib.run("video.pause", json!({}));
        assert_eq!((v["playing"].clone(), v["positionMs"].clone()), (json!(false), json!(1000)));
        assert_eq!(lib.sound.format(), None);
        lib.clock.advance(Duration::from_millis(500));
        assert_eq!(lib.state()["positionMs"], 1000);
        lib.run("video.play", json!({}));
        lib.advance_to(200, 1200);
        assert_eq!(lib.frame_due()["frame"]["index"], 30);
        // Play stops at the clip's end.
        lib.advance_to(5_000, 2000);
        let v = lib.until("stopped at the end", |v| v["playing"] == false);
        assert_eq!(v["positionMs"], 2000);
    });
}

#[test]
fn play_respects_the_trim_and_the_trim_survives_a_restart() {
    let mut lib = Lib::new();
    let clip = lib.id("clip.mp4");
    on_owner(|| {
        lib.open("clip.mp4");
        // Set Start at the position, Set End at a time: one step of the video's history each.
        lib.run("video.seek", json!({ "ms": 500 }));
        lib.until("seeked", |v| v["pending"] == false && v["frame"]["index"] == 12);
        let v = lib.run("video.setStart", json!({}));
        assert_eq!(v["trim"], json!({ "startMs": 500, "endMs": 2000 }));
        let v = lib.run("video.setEnd", json!({ "ms": 1500 }));
        assert_eq!(v["trim"], json!({ "startMs": 500, "endMs": 1500 }));
        let refused = disabled(lib.s.execute("video.setEnd", &json!({ "ms": 400 })));
        assert_eq!(refused, "Set End: the start must come before the end");
        // Play from outside the trim starts at its start and stops at its end.
        lib.run("video.seek", json!({ "ms": 1800 }));
        lib.until("seeked", |v| v["pending"] == false);
        let v = lib.run("video.play", json!({}));
        assert_eq!(v["positionMs"], 500);
        lib.advance_to(400, 900);
        assert_eq!(lib.frame_due()["frame"]["index"], 22);
        lib.advance_to(2_000, 1500);
        let v = lib.until("stopped at the trim's end", |v| v["playing"] == false);
        assert_eq!(v["positionMs"], 1500);
    });
    assert_eq!(lib.s.library().info(clip).edits, ["trim(500,1500)"], "one entry, replaced in place");
    let h = lib.run("photo.history.state", json!({ "id": clip }));
    assert_eq!(h["undoLabel"], "Undo Trim");
    lib.run("photo.history.undo", json!({ "id": clip }));
    assert_eq!(lib.s.library().info(clip).edits, ["trim(500,2000)"], "Set End undone");
    lib.run("photo.history.redo", json!({ "id": clip }));
    // After a restart.
    let Lib { dir, s, .. } = lib;
    drop(s);
    let (_, mut s) = Lib::session(dir.path());
    let v = s.execute("video.inspect", &json!({ "id": clip })).unwrap();
    assert_eq!(v["trim"], json!({ "startMs": 500, "endMs": 1500 }));
    assert_eq!((v["playable"].clone(), v["reason"].clone()), (json!(true), Value::Null));
}

#[test]
fn steps_and_seeks_land_on_their_frames() {
    let mut lib = Lib::new();
    on_owner(|| {
        lib.open("clip.mp4");
        lib.run("video.step", json!({ "frames": 3 }));
        let v = lib.until("stepped", |v| v["pending"] == false);
        assert_eq!((v["frame"]["index"].clone(), v["positionMs"].clone()), (json!(3), json!(120)));
        lib.run("video.step", json!({ "frames": -2 }));
        let v = lib.until("stepped back", |v| v["pending"] == false);
        assert_eq!((v["frame"]["index"].clone(), v["positionMs"].clone()), (json!(1), json!(40)));
        lib.run("video.step", json!({ "frames": -100 }));
        assert_eq!(lib.until("at the first", |v| v["pending"] == false)["frame"]["index"], 0);
        lib.run("video.seek", json!({ "ms": 1_234 }));
        let v = lib.until("seeked", |v| v["pending"] == false);
        assert_eq!((v["frame"]["index"].clone(), v["positionMs"].clone()), (json!(30), json!(1_234)));
        lib.run("video.seek", json!({ "ms": 99_999 }));
        let v = lib.until("seeked past the end", |v| v["pending"] == false);
        assert_eq!((v["frame"]["index"].clone(), v["positionMs"].clone()), (json!(FRAMES - 1), json!(2000)));
    });
    for bad in [json!({ "frames": 0 }), json!({ "frames": 100_001 }), json!({}), json!({ "frames": "1" })] {
        assert!(matches!(lib.s.execute("video.step", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
    assert!(matches!(lib.s.execute("video.volume", &json!({ "volume": 101 })), Err(CmdError::BadParams(_))));
}

#[test]
fn save_frame_at_one_and_a_half_seconds_writes_a_jpeg_of_the_frames_size_into_video_frames() {
    let mut lib = Lib::new();
    let clip = lib.id("clip.mp4");
    lib.run("photo.history.apply", json!({ "id": clip, "op": "rotate(90)" }));
    let entry = on_owner(|| {
        lib.open("clip.mp4");
        lib.run("video.seek", json!({ "ms": 1500 }));
        lib.until("seeked", |v| v["pending"] == false && v["frame"]["index"] == 37);
        let reply = lib.run("video.saveFrame", json!({}));
        assert_eq!(
            (reply["name"].clone(), reply["timeMs"].clone(), reply["pending"].clone()),
            (json!("clip 00-00-01.jpg"), json!(1480), json!(true))
        );
        let job = reply["job"].as_u64().unwrap();
        lib.s.wait_job(job, PATIENCE).unwrap()
    });
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    let result = entry.result.unwrap();
    let path = lib.frames_dir().join("clip 00-00-01.jpg");
    assert_eq!(result["path"], json!(path));
    assert_eq!((result["width"].clone(), result["height"].clone()), (json!(96), json!(160)), "the frame's size, turned as the video is");
    let header = dalica_image::read_header(&read(&path), &dalica_image::Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (96, 160));
    // In the library, in the Video Frames project, dated the clip's date plus the frame's time.
    lib.s.wait_idle();
    let photo = lib.id("clip 00-00-01.jpg");
    let lib_ref = lib.s.library();
    let folder = lib_ref.photo(photo).unwrap().folder;
    assert_eq!(lib_ref.folder(folder).unwrap().path, lib.frames_dir());
    assert_eq!(lib_ref.project(folder), Some(Project::VideoFrames));
    let clip_date = lib_ref.effective_date(clip).expect("the clip has a date");
    assert_eq!(lib_ref.effective_date(photo), dalica_engine::video_output::frame_date(clip_date, 1480));
    // Again: a free name, never over the first.
    let reply = lib.run("video.saveFrame", json!({}));
    let again = lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(again.result.unwrap()["path"], json!(lib.frames_dir().join("clip 00-00-01-001.jpg")));
    assert!(exists(&path));
}

#[test]
fn export_clip_copies_the_trimmed_part_beside_the_clip_and_never_over_a_file() {
    let mut lib = Lib::new();
    let source = read(&lib.trip().join("clip.mp4"));
    let out = lib.dir.path().join("Out");
    dalica_fs::create_dir_all(&out).unwrap();
    let entry = on_owner(|| {
        lib.open("clip.mp4");
        lib.run("video.setStart", json!({ "ms": 500 }));
        lib.run("video.setEnd", json!({ "ms": 1500 }));
        let reply = lib.run("video.exportClip", json!({}));
        assert_eq!((reply["startMs"].clone(), reply["endMs"].clone(), reply["path"].clone()), (json!(500), json!(1500), Value::Null));
        lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap()
    });
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    let result = entry.result.unwrap();
    let path = lib.trip().join("clip trimmed.mp4");
    assert_eq!((result["path"].clone(), result["startMs"].clone(), result["endMs"].clone()), (json!(path), json!(400), json!(1520)));
    assert_eq!(entry.label, "Exported clip trimmed.mp4 (0:00.4 to 0:01.5)");
    let info = dalica_image::formats::video::probe(&read(&path), &dalica_image::Limits::default()).unwrap();
    assert_eq!(info.duration_ms, Some(1120));
    assert_eq!(read(&lib.trip().join("clip.mp4")), source, "the clip is only read");
    // To a path: exactly there, and never over a file.
    let to = out.join("part.mp4");
    let reply = lib.run("video.exportClip", json!({ "path": to }));
    let done = lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!((done.state, done.result.unwrap()["path"].clone()), (JobState::Done, json!(to)));
    let before = read(&to);
    let reply = lib.run("video.exportClip", json!({ "path": to }));
    let again = lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(again.state, JobState::Failed, "{again:?}");
    assert_eq!(read(&to), before);
    for bad in [json!({ "path": "relative.mp4" }), json!({ "path": out.join("..").join("x.mp4") }), json!({ "path": lib.trip().join("clip.mp4") })] {
        assert!(matches!(lib.s.execute("video.exportClip", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
}

#[test]
fn a_clip_of_a_codec_dalica_cannot_play_says_so_and_its_playback_commands_refuse_and_write_nothing() {
    let mut lib = Lib::new();
    let old = lib.id("old.mov");
    let reason = "This video can't be played (MPEG-4 Part 2)";
    let v = lib.run("video.inspect", json!({ "id": old }));
    assert_eq!((v["playable"].clone(), v["reason"].clone()), (json!(false), json!(reason)));
    let entry = on_owner(|| {
        let v = lib.run("video.open", json!({ "id": old }));
        assert_eq!((v["playable"].clone(), v["reason"].clone(), v["pending"].clone()), (json!(false), json!(reason), json!(false)));
        for (id, p) in [
            ("video.play", json!({})),
            ("video.pause", json!({})),
            ("video.seek", json!({ "ms": 500 })),
            ("video.step", json!({ "frames": 1 })),
            ("video.saveFrame", json!({})),
        ] {
            assert_eq!(disabled(lib.s.execute(id, &p)), reason, "{id}");
        }
        assert_eq!(lib.sound.opens(), [], "no sound opened");
        // Its trim and Export Clip work on the container.
        lib.run("video.setStart", json!({ "ms": 500 }));
        let v = lib.run("video.setEnd", json!({ "ms": 1500 }));
        assert_eq!(v["trim"], json!({ "startMs": 500, "endMs": 1500 }));
        let reply = lib.run("video.exportClip", json!({}));
        lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap()
    });
    assert!(!exists(&lib.frames_dir()), "Save Frame wrote nothing");
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    let path = lib.trip().join("old trimmed.mov");
    let info = dalica_image::formats::video::probe(&read(&path), &dalica_image::Limits::default()).unwrap();
    assert_eq!((info.video_codec.as_deref(), info.duration_ms), (Some("mp4v"), Some(1120)));
}

#[test]
fn a_clip_whose_sound_dalica_cannot_decode_plays_muted_and_refuses_the_volume() {
    let mut lib = Lib::new();
    on_owner(|| {
        let v = lib.open("muted.mp4");
        let why = "This clip plays without sound (AC-3 sound can't be played)";
        assert_eq!((v["sound"].clone(), v["muted"].clone(), v["playable"].clone()), (json!("muted"), json!(why), json!(true)));
        assert_eq!(disabled(lib.s.execute("video.volume", &json!({ "volume": 50 }))), why);
        lib.run("video.play", json!({}));
        assert_eq!(lib.sound.format(), Some(Format { rate: 48_000, channels: 1 }), "silence keeps the clock");
        lib.advance_to(400, 400);
        assert_eq!(lib.frame_due()["frame"]["index"], 10);
        assert!(lib.sound.samples().iter().all(|s| *s == 0.0));
        // The volume of a clip with sound is kept in the settings.
        let clip = lib.id("clip.mp4");
        lib.run("video.open", json!({ "id": clip }));
        assert_eq!(lib.run("video.volume", json!({ "volume": 35 }))["volume"], 35);
    });
    assert_eq!(lib.s.settings().video().volume, 35);
    let v = lib.run("video.close", json!({}));
    assert_eq!(v["open"], false);
    assert_eq!(disabled(lib.s.execute("video.play", &json!({}))), "No video is open.");
}

// --- A decoder slower than the clip --------------------------------------------------------------------

/// A stand-in decoder on the session's virtual clock: `SLOW_FRAMES` frames `SLOW_FRAME_MS` apart,
/// a key frame every `SLOW_KEYS`, each frame taking `SLOW_DECODE_MS` of the clock to decode (from
/// the key frame before it, or on from the frame decoded last when no key frame lies between),
/// once `paced` is set.
struct Slow {
    clock: Arc<VirtualClock>,
    paced: Arc<AtomicBool>,
    /// The clock's time (ms) a decode under way ends at; 0 when none is.
    busy: Arc<AtomicU64>,
    /// The frame the decoder stands at, and the one returned last.
    at: Option<usize>,
    last: Option<usize>,
}

const SLOW_FRAMES: usize = 100;
const SLOW_FRAME_MS: u64 = 20;
const SLOW_KEYS: usize = 25;
const SLOW_DECODE_MS: u64 = 50;

impl Slow {
    fn now_ms(&self) -> u64 {
        self.clock.now().as_millis() as u64
    }

    /// Spends `frames` decodes of the clock.
    fn spend(&self, frames: usize) {
        if !self.paced.load(Ordering::SeqCst) {
            return;
        }
        let until = self.now_ms() + frames as u64 * SLOW_DECODE_MS;
        self.busy.store(until, Ordering::SeqCst);
        let start = Instant::now();
        while self.now_ms() < until && start.elapsed() < PATIENCE {
            std::thread::sleep(Duration::from_micros(100));
        }
        self.busy.store(0, Ordering::SeqCst);
    }
}

impl dalica_engine::video::PlayFrames for Slow {
    fn frame_count(&self) -> usize {
        SLOW_FRAMES
    }
    fn time_of(&self, index: usize) -> Option<u64> {
        (index < SLOW_FRAMES).then_some(index as u64 * SLOW_FRAME_MS)
    }
    fn index_at(&self, ms: u64) -> usize {
        ((ms / SLOW_FRAME_MS) as usize).min(SLOW_FRAMES - 1)
    }
    fn key_after(&self, index: usize) -> Option<usize> {
        Some(index.next_multiple_of(SLOW_KEYS)).filter(|k| *k < SLOW_FRAMES)
    }
    fn frame(&mut self, index: usize) -> Result<Frame, FrameError> {
        let key = index / SLOW_KEYS * SLOW_KEYS;
        let from = match self.at {
            Some(at) if at < index && at >= key => at + 1,
            _ => key,
        };
        self.spend(index + 1 - from);
        (self.at, self.last) = (Some(index), Some(index));
        Ok(Frame { image: Rgba8 { width: 2, height: 2, data: vec![0; 16] }, index, time_ms: index as u64 * SLOW_FRAME_MS })
    }
    fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let index = self.last.map_or(0, |i| i + 1);
        if index >= SLOW_FRAMES { Ok(None) } else { self.frame(index).map(Some) }
    }
    fn current_index(&self) -> Option<usize> {
        self.last
    }
    fn decode_ms(&self) -> Option<u64> {
        Some(SLOW_DECODE_MS)
    }
}

#[test]
fn a_decoder_slower_than_the_clip_shows_frames_at_its_own_rate_and_keeps_up_with_the_clock() {
    let mut lib = Lib::new();
    let (paced, busy) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
    let (clock, p, b) = (lib.clock.clone(), paced.clone(), busy.clone());
    lib.s.set_video_frames(Some(Arc::new(move |_: &Path| {
        let slow = Slow { clock: clock.clone(), paced: p.clone(), busy: b.clone(), at: None, last: None };
        Ok(Box::new(slow) as Box<dyn dalica_engine::video::PlayFrames>)
    })));
    on_owner(|| {
        lib.open("clip.mp4");
        paced.store(true, Ordering::SeqCst);
        lib.run("video.play", json!({}));
        // The owner's frames, 10 ms of the clock apart, each once the worker has done all it can
        // by then (decoding a frame due later, or idle).
        let now = |lib: &Lib| lib.clock.now().as_millis() as u64;
        let (mut shown, mut last, mut changed_at) = (Vec::<(u64, u64)>::new(), None, 0u64);
        let (mut longest_hold, mut most_behind) = (0u64, 0u64);
        for tick in 1..=180u64 {
            lib.clock.advance(Duration::from_millis(10));
            let start = Instant::now();
            while busy.load(Ordering::SeqCst) <= now(&lib) && start.elapsed() < Duration::from_millis(20) {
                std::thread::sleep(Duration::from_micros(200));
            }
            let pos = lib.s.video_position().unwrap();
            // The sound starts once play's first frame is decoded (one decode of the clock).
            assert!(
                pos <= tick * 10 && pos + SLOW_DECODE_MS + 10 >= tick * 10,
                "the position follows the clock, the sound never held back: {pos} at {}",
                tick * 10
            );
            let f = lib.s.video_frame().unwrap();
            if Some(f.index) != last {
                assert!(last.is_none_or(|l| f.index > l), "the frames move forward: {last:?} then {}", f.index);
                shown.push((pos, f.index as u64));
                longest_hold = longest_hold.max(pos - changed_at);
                (last, changed_at) = (Some(f.index), pos);
            }
            if pos > 300 {
                most_behind = most_behind.max(pos.saturating_sub(f.time_ms));
            }
        }
        // 1.8 s at 50 ms a frame is 36 decodes: about as many shown, never the 2 or 3 of a worker
        // that drops every frame whose successor is due.
        let want = 1800 / SLOW_DECODE_MS;
        assert!(shown.len() as u64 >= want * 7 / 10 && shown.len() as u64 <= want + 10, "{} frames shown: {shown:?}", shown.len());
        let gop = SLOW_KEYS as u64 * SLOW_FRAME_MS;
        assert!(most_behind <= gop, "the picture catches up at each key frame: {most_behind} ms behind at most; {shown:?}");
        assert!(longest_hold <= 3 * SLOW_DECODE_MS, "the picture never stalls: held {longest_hold} ms at most; {shown:?}");
        let v = lib.state();
        assert_eq!(v["playing"], true, "{v}");
    });
}
