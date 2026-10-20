//! The video player in the edit view (`roadmap.md` M15.26; `editing.md` §11, `viewing.md` §4),
//! headless and by id, on the silent `Sound` and a virtual clock, in a folder of three clips made
//! here (H.264 with every macroblock I_PCM, ITU-T H.264 §7.3.5, muxed by hand from ISO/IEC
//! 14496-12; 2 s of 160 × 96 at 25 frames a second): `clip.mp4` (H.264 and AAC), `old.mov`
//! (MPEG-4 Part 2, which Dalica cannot play) and `muted.mp4` (AC-3 sound).
//!
//! On the clip Space plays and the readout follows the clock; `.` steps one frame; dragging the
//! start handle and Set End trim and the readout says "(trimmed to …)"; Save Frame adds a photo to
//! Video Frames, and a control request's click on it with Video Frames outside the write roots is
//! refused and writes nothing; the frame time while playing stays within the edit view's budget.
//! On the clip of a failed codec Space does nothing and play, the steps and Save Frame are greyed
//! with the reason while Export Clip works; on the muted clip the volume is greyed with its
//! reason. At 900 × 600 the bar fits. Fingerprints at 1400 × 900: the video playing
//! (`video-player-playing-1400x900`) and the bar trimmed (`video-player-trimmed-1400x900`).
//! Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_engine::sound::Silent;
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::headless::{Drag, Target};
use dalica_ui::panels::edit::video::{self, bar, tab};
use dalica_ui::picker::ScriptedPicker;
use dalica_ui::state::View;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

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
const FRAMES_IN_CLIP: usize = 50;
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

/// Frame `k`'s grey: each frame a little lighter than the one before.
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
    let packets = (FRAMES_IN_CLIP * 48_000 / FPS).div_ceil(1024);
    let tracks = [
        Track {
            id: 1,
            handler: *b"vide",
            entry: avc1(video),
            timescale: 1000,
            duration: FRAME_MS as u32,
            syncs: Some((0..FRAMES_IN_CLIP as u32).filter(|k| (*k as usize).is_multiple_of(GOP)).map(|k| k + 1).collect()),
            size: ((MBS.0 * 16) as u32, (MBS.1 * 16) as u32),
            samples: (0..FRAMES_IN_CLIP).map(frame).collect(),
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
    mvhd.extend(be((FRAMES_IN_CLIP as u64 * FRAME_MS) as u32));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    out.extend(bx(b"moov", &[full(b"mvhd", &mvhd), traks].concat()));
    out
}

// --- The window -----------------------------------------------------------------------------------

/// How the window is made: with a scripted save picker, or driven by a control request whose
/// one write root is this folder of the test's directory.
enum With {
    Picker,
    Control(&'static str),
}

/// A window at 1400 × 900 on a library watching `Trip` (`clip.mp4`, `old.mov`, `muted.mp4`), the
/// session's sound the silent one on a virtual clock, `Pictures` its Pictures folder.
struct W {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    d: Driver,
    picker: ScriptedPicker,
    control: Option<(Control, Connection)>,
}

impl W {
    fn new() -> W {
        W::with(With::Picker)
    }

    fn with(with: With) -> W {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Trip");
        std::fs::create_dir_all(&trip).unwrap();
        std::fs::write(trip.join("clip.mp4"), clip(b"avc1", b"mp4a")).unwrap();
        std::fs::write(trip.join("old.mov"), clip(b"mp4v", b"mp4a")).unwrap();
        std::fs::write(trip.join("muted.mp4"), clip(b"avc1", b"ac-3")).unwrap();
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
        let mut s = Session::open(&dir.path().join("library"), clock.clone()).unwrap();
        s.set_sound(Arc::new(Silent::new(clock.clone())));
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": trip })).unwrap();
        s.wait_idle();
        let picker = ScriptedPicker::new();
        let (app, control) = match with {
            With::Picker => (App::with_picker(Some(s), Arc::new(picker.clone())), None),
            With::Control(root) => {
                let control = Control::new(Config { write_roots: vec![dir.path().join(root)], ..Config::default() });
                (App::with_picker(Some(s), control.picker()), Some((control, Connection::new())))
            }
        };
        let mut d = new_driver(app, LARGE.0, LARGE.1, 1.0);
        d.settle(FRAMES).unwrap();
        W { dir, clock, d, picker, control }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.d.app().session().unwrap().library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id
    }

    /// Opens `name` in the edit view the way a person does (selected, then Enter) and waits until
    /// the player has it: its first frame, or why it cannot play.
    fn open(&mut self, name: &str) {
        let id = self.id(name);
        self.d.run("select.set", &json!({ "ids": [id] })).unwrap();
        self.d.settle(FRAMES).unwrap();
        self.d.key("Enter").unwrap();
        self.d.settle(FRAMES).unwrap();
        assert_eq!((self.d.state().view, self.d.state().edit.photo), (View::Edit, Some(id)), "Enter opens {name}");
        self.until(&format!("{name} opened"), |v| v["playable"] == false || (v["ready"] == true && v["frame"].is_object()));
    }

    /// The engine's player now.
    fn player(&mut self) -> Value {
        self.d.app_mut().session_mut().unwrap().execute("video.state", &json!({})).unwrap()
    }

    /// Runs frames until the engine's player satisfies `done`.
    fn until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..FRAMES {
            let v = self.player();
            if done(&v) {
                self.d.step();
                return v;
            }
            self.d.step();
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("{what}: {}", self.player());
    }

    /// The scrubber's readout as drawn.
    fn readout(&self) -> String {
        widget(&self.d, bar::READOUT_ID).label
    }

    fn enabled(&self, id: &str) -> bool {
        widget(&self.d, id).enabled
    }

    /// A control request (`ui.clickWidget`, …) served on the window.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let (control, conn) = self.control.as_mut().expect("a window driven by a control request");
        serve(control, conn, &mut self.d, &Request::new(method, params)).map_err(|e| e.to_string())
    }

    fn frames_dir(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Dalica").join("Video Frames")
    }

    /// The photos the library lists in `folder`.
    fn listed_in(&self, folder: &Path) -> Vec<String> {
        let lib = self.d.app().session().unwrap().library();
        lib.photos().filter(|p| lib.folder(p.folder).is_some_and(|f| f.path == folder)).map(|p| p.name.clone()).collect()
    }
}

// --- The tests -----------------------------------------------------------------------------------------

#[test]
fn space_plays_and_the_readout_follows_the_clock_and_a_step_moves_one_frame() {
    let mut w = W::new();
    w.open("clip.mp4");
    assert!(try_widget(&w.d, video::CANNOT_PLAY_ID).is_none(), "the clip plays");
    assert!(w.enabled(bar::TOGGLE_ID) && w.enabled(tab::SAVE_FRAME_ID) && w.enabled(tab::EXPORT_CLIP_ID));
    assert_eq!(w.readout(), "00:00:00 / 00:00:02");
    w.d.key("Space").unwrap();
    w.d.step();
    assert!(w.d.state().video.playing, "Space plays");
    assert!(widget(&w.d, bar::TOGGLE_ID).active, "the toggle shows Pause");
    // The clock moves a second: the readout and the frame follow.
    w.clock.advance(Duration::from_millis(1000));
    w.until("a second played", |v| v["positionMs"] == 1000 && v["frame"]["index"] == 25);
    w.d.step();
    assert_eq!((w.d.state().video.position, w.readout()), (1000, "00:00:01 / 00:00:02".to_string()));
    w.d.key("Space").unwrap();
    w.d.step();
    assert!(!w.d.state().video.playing, "Space again pauses");
    // `.` steps one frame forward, `,` one back.
    w.d.key(".").unwrap();
    let v = w.until("a frame forward", |v| v["frame"]["index"] == 26 && v["pending"] == false);
    assert_eq!(v["positionMs"], 26 * FRAME_MS);
    w.d.key(",").unwrap();
    w.until("a frame back", |v| v["frame"]["index"] == 25 && v["pending"] == false);
    assert!(!w.d.state().video.playing);

    // Playing to its end: a frame every 40 ms of the clock; the window's frames stay within the
    // edit view's budget (the 16 ms of a slider's draft, a 60 Hz frame).
    w.d.key("Space").unwrap();
    w.d.step();
    w.d.perf_mut().reset();
    // No frame reads the disk or decodes: the frames come from the player's worker.
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let start = w.d.frames();
    // From frame 25 at 1 s: each frame shows when the clock reaches it.
    for k in 26..FRAMES_IN_CLIP as u64 {
        w.clock.advance(Duration::from_millis(FRAME_MS));
        w.until(&format!("frame {k}"), |v| v["frame"]["index"] == k);
    }
    w.clock.advance(Duration::from_millis(FRAME_MS));
    let v = w.until("stopped at the end", |v| v["playing"] == false);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "a frame made an fs call");
    assert_eq!(v["positionMs"], 2000);
    let report = w.d.perf().report();
    let uploaded: u64 = w.d.perf().samples().map(|s| s.textures_uploaded).sum();
    eprintln!(
        "the player playing: {} frames, mean {:.2} ms, p95 {:.2} ms, max {:.2} ms, {} textures uploaded, {} frames dropped",
        w.d.frames() - start,
        report.frame_ms,
        report.frame_p95_ms,
        report.max_frame_ms,
        uploaded,
        v["dropped"]
    );
    assert!(uploaded >= 20, "the frames shown were uploaded: {uploaded}");
    assert!(report.frame_p95_ms <= 16.0, "the frame time while playing: {report:?}");
    w.d.step();
    assert!(!w.d.state().video.playing, "the view state follows the end");
}

#[test]
fn dragging_the_start_handle_and_set_end_trim_and_the_readout_says_trimmed() {
    let mut w = W::new();
    let id = w.id("clip.mp4");
    w.open("clip.mp4");
    let track = widget(&w.d, bar::POSITION_ID).rect;
    let width = track[2] - 2.0 * (dalica_ui_kit::widgets::PLAYHEAD_R + 1.0);
    // A quarter of the way: half a second.
    w.d.drag(bar::TRIM_START_ID, Target::Delta(egui::vec2(width / 4.0, 0.0)), &Drag::default()).unwrap();
    w.d.settle(FRAMES).unwrap();
    let v = w.player();
    let start = v["trim"]["startMs"].as_u64().unwrap_or_else(|| panic!("trimmed: {v}"));
    assert!(start.abs_diff(500) <= FRAME_MS, "the start where it was dropped: {v}");
    assert_eq!(v["trim"]["endMs"], 2000);
    assert!(w.readout().ends_with(&format!("(trimmed to {})", video::clock(2000 - start))), "{}", w.readout());
    assert_eq!(w.d.state().video.trim, None, "no handle dragged now");
    // Set End at 1.5 s: the playhead there, then the button.
    w.d.run("player.seek", &json!({ "ms": 1500 })).unwrap();
    w.until("at 1.5 s", |v| v["pending"] == false);
    w.d.click(bar::SET_END_ID).unwrap();
    w.d.settle(FRAMES).unwrap();
    let v = w.player();
    assert_eq!(v["trim"]["endMs"], 1500, "{v}");
    assert_eq!(w.readout(), format!("00:00:01 / 00:00:02 (trimmed to {})", video::clock(1500 - start)));
    let edits = w.d.app().session().unwrap().library().info(id).edits.clone();
    assert_eq!(edits.iter().filter(|e| e.starts_with("trim(")).count(), 1, "one trim in the recipe: {edits:?}");
    assert_eq!(edits.last().map(String::as_str), Some(format!("trim({start},1500)").as_str()));
    let rect = widget(&w.d, video::REGION_ID).rect;
    let bar_rect = [rect[0], rect[1] + rect[3] - bar::HEIGHT, rect[2], bar::HEIGHT];
    check_rect(&w.d, "video-player-trimmed-1400x900", bar_rect);
    // Undo takes the trim drag and Set End back one step each.
    w.d.run("edit.undo", &json!({})).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(w.player()["trim"]["endMs"], 2000, "Set End undone");
}

#[test]
fn the_video_playing_matches_its_fingerprint() {
    let mut w = W::new();
    w.open("clip.mp4");
    w.d.click(bar::TOGGLE_ID).unwrap();
    w.clock.advance(Duration::from_millis(1000));
    w.until("a second played", |v| v["positionMs"] == 1000 && v["frame"]["index"] == 25);
    w.d.step();
    w.d.step();
    assert!(w.d.state().video.playing);
    check_rect(&w.d, "video-player-playing-1400x900", widget(&w.d, video::REGION_ID).rect);
}

#[test]
fn save_frame_adds_a_photo_to_video_frames() {
    let mut w = W::new();
    w.open("clip.mp4");
    w.d.run("player.seek", &json!({ "ms": 1000 })).unwrap();
    w.until("at 1 s", |v| v["pending"] == false && v["frame"]["index"] == 25);
    w.d.click(tab::SAVE_FRAME_ID).unwrap();
    let frames = w.frames_dir();
    for _ in 0..FRAMES {
        if !w.listed_in(&frames).is_empty() {
            break;
        }
        w.d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(w.listed_in(&frames), ["clip 00-00-01.jpg"], "the frame in Video Frames");
    assert!(frames.join("clip 00-00-01.jpg").is_file());
}

#[test]
fn a_control_click_on_save_frame_outside_the_write_roots_is_refused_and_writes_nothing() {
    // The write root is `allowed`; Pictures is outside it.
    let mut w = W::with(With::Control("allowed"));
    let allowed = w.dir.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    w.open("clip.mp4");
    let pictures = w.dir.path().join("Pictures");
    w.call("ui.clickWidget", json!({ "id": tab::SAVE_FRAME_ID })).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert!(!pictures.exists(), "nothing written outside the write roots");
    assert!(w.d.app().state().status.contains("outside every automation write root"), "refused: {}", w.d.app().state().status);
    // Inside a write root the same click saves the frame.
    w.d.app_mut().session_mut().unwrap().set_pictures_dir(Some(allowed.join("Pictures")));
    w.call("ui.clickWidget", json!({ "id": tab::SAVE_FRAME_ID })).unwrap();
    let frames = allowed.join("Pictures").join("Dalica").join("Video Frames");
    for _ in 0..FRAMES {
        if !w.listed_in(&frames).is_empty() {
            break;
        }
        w.d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(w.listed_in(&frames).len(), 1, "saved inside the write root");
}

#[test]
fn a_clip_of_a_failed_codec_greys_play_the_steps_and_save_frame_while_export_clip_works() {
    let mut w = W::new();
    w.open("old.mov");
    assert_eq!(widget(&w.d, video::CANNOT_PLAY_ID).label, "This video can't be played");
    assert!(w.enabled(video::OPEN_WITH_ID));
    let reason = "This video can't be played (MPEG-4 Part 2)";
    for id in [bar::TOGGLE_ID, tab::SAVE_FRAME_ID] {
        assert!(!w.enabled(id), "{id} greyed");
    }
    for key in ["Space", "/", "."] {
        w.d.key(key).unwrap();
        w.d.settle(FRAMES).unwrap();
        assert!(!w.d.state().video.playing, "{key} does nothing");
        assert_eq!(w.d.app().state().status, reason, "{key}: the reason on the status line");
    }
    assert_eq!(w.player()["positionMs"], 0, "the playhead never moved");
    for (id, params) in [("player.play", json!({})), ("player.step", json!({ "frames": 1 })), ("player.saveFrame", json!({}))] {
        match w.d.run(id, &params) {
            Err(e) => assert!(e.to_string().contains(reason), "{id}: {e}"),
            Ok(v) => panic!("{id} ran: {v}"),
        }
    }
    // What works on the container: the trim and Export Clip.
    assert!(w.enabled(bar::SET_END_ID) && w.enabled(tab::EXPORT_CLIP_ID) && w.enabled(tab::ROTATE_CW_ID));
    let to = w.dir.path().join("part.mov");
    w.picker.push_save(&to);
    w.d.click(tab::EXPORT_CLIP_ID).unwrap();
    for _ in 0..FRAMES {
        if to.is_file() {
            break;
        }
        w.d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(to.is_file(), "Export Clip wrote the clip");
    let asked = w.picker.last_save().expect("the save picker asked");
    assert_eq!(asked.name, "old trimmed.mov");
    assert!(!w.dir.path().join("Pictures").exists(), "no frame saved");

    // A clip whose sound Dalica cannot decode plays, its volume greyed with the reason.
    w.d.run("view.library", &json!({})).unwrap();
    w.d.settle(FRAMES).unwrap();
    w.open("muted.mp4");
    assert!(w.enabled(bar::TOGGLE_ID), "its pictures play");
    assert!(!w.enabled(bar::VOLUME_ID) && !w.enabled(&format!("{}/mute", bar::VOLUME_ID)), "the volume greyed");
    let muted = w.player()["muted"].as_str().unwrap().to_string();
    assert!(muted.contains("AC-3"), "{muted}");
    assert!(w.d.run("player.volume", &json!({ "volume": 10 })).is_err_and(|e| e.to_string().contains(&muted)));
}

#[test]
fn at_900_by_600_the_bar_fits() {
    let mut w = W::new();
    w.open("clip.mp4");
    w.d.resize(SMALL.0, SMALL.1);
    w.d.settle(FRAMES).unwrap();
    check_fits(&w.d, "the video player");
    shot(&w.d, "video-player-900x600");
    let area = widget(&w.d, video::REGION_ID).rect;
    let inside = |r: [f32; 4]| {
        r[0] >= area[0] - 0.01 && r[1] >= area[1] - 0.01 && r[0] + r[2] <= area[0] + area[2] + 0.01 && r[1] + r[3] <= area[1] + area[3] + 0.01
    };
    let ids = [
        bar::TOGGLE_ID,
        bar::POSITION_ID,
        bar::READOUT_ID,
        bar::TRIM_START_ID,
        bar::TRIM_END_ID,
        bar::SET_START_ID,
        bar::SET_END_ID,
        bar::VOLUME_ID,
        bar::FULL_SCREEN_ID,
        video::PICTURE_ID,
    ];
    let mut rects = Vec::new();
    for id in ids {
        let r = widget(&w.d, id).rect;
        assert!(inside(r), "{id} at {r:?} in {area:?}");
        rects.push((id, r));
    }
    // The bar's buttons and the scrubber do not overlap.
    let row = [bar::TOGGLE_ID, bar::POSITION_ID, bar::SET_START_ID, bar::SET_END_ID, bar::VOLUME_ID, bar::FULL_SCREEN_ID];
    let mut xs: Vec<(f32, f32, &str)> = rects.iter().filter(|(id, _)| row.contains(id)).map(|(id, r)| (r[0], r[0] + r[2], *id)).collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in xs.windows(2) {
        assert!(pair[0].1 <= pair[1].0 + 0.01, "{} overlaps {}", pair[0].2, pair[1].2);
    }
    assert!(widget(&w.d, bar::POSITION_ID).rect[2] > 200.0, "the scrubber keeps room");
    shot(&w.d, "video-player-900x600");
    w.d.resize(LARGE.0, LARGE.1);
    w.d.settle(FRAMES).unwrap();
    shot(&w.d, "video-player-1400x900");
    // Full Screen spreads the video over the window, and back.
    w.d.click(bar::FULL_SCREEN_ID).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(widget(&w.d, video::REGION_ID).rect, [0.0, 0.0, LARGE.0 as f32, LARGE.1 as f32]);
    assert!(widget(&w.d, bar::FULL_SCREEN_ID).active);
    w.d.click(bar::FULL_SCREEN_ID).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(widget(&w.d, video::REGION_ID).rect, widget(&w.d, "region:canvas").rect);
}

#[test]
fn the_players_controls_are_reached_with_tab_and_named() {
    let mut w = W::new();
    w.open("clip.mp4");
    let mut reached = Vec::new();
    for _ in 0..120 {
        w.d.key("Tab").unwrap();
        let focus = w.d.ctx().memory(|m| m.focused());
        if let Some(f) = w.d.widgets().into_iter().find(|wi| wi.egui_id.is_some() && wi.egui_id == focus) {
            reached.push(f.id.clone());
        }
    }
    for id in [bar::TOGGLE_ID, bar::POSITION_ID, bar::SET_START_ID, bar::SET_END_ID, bar::VOLUME_ID, bar::FULL_SCREEN_ID, tab::SAVE_FRAME_ID] {
        assert!(reached.iter().any(|r| r == id), "{id} reached with Tab: {reached:?}");
        assert!(!widget(&w.d, id).label.is_empty(), "{id} named");
    }
    for id in [format!("{}/start", bar::POSITION_ID), format!("{}/end", bar::POSITION_ID)] {
        assert!(reached.contains(&id), "{id} reached with Tab: {reached:?}");
    }
}
