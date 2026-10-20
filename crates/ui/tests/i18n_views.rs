//! The edit view, the rooms, the slideshow and the viewer through the catalogs (M15.33,
//! `localization.md` → Checking a translation), headless in the pseudo-locale `qps`: in the edit
//! view on each tab, with each tool open, each effect's panel, A│B compare and the face box drawn
//! by hand; on a clip (the player's bar, its readout trimmed, Save Frame and Export Clip) and on a
//! clip Dalica cannot play; in the Import room on a camera (its source menu open, a picture
//! previewed) and in the Collage room (both tabs, a picture's menu); in the slideshow with its bar
//! and its Transition drop-up; in the viewer handed a file by another launch (its More menu open);
//! and in the quick preview, every text a frame draws is bracketed except user data (folder, file,
//! album and camera names, captions, tags, paths), the keyboard shortcuts written in tooltips (key
//! names, as the keymap writes them) and numbers. At 1400 × 900 and 900 × 600 no label is cut at
//! the side of the area it is drawn in. `DALICA_SHOTS=<dir>` writes the window in the
//! pseudo-locale there (`i18n-views-*.png`). The clips are H.264 with every macroblock I_PCM, made
//! here (the writer of `video_player.rs`).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, demo_open, new_driver, shot};
use dalica_core::PhotoId;
use dalica_engine::Key;
use dalica_engine::camera::Opener;
use dalica_engine::sound::Silent;
use dalica_scan::devices::{Camera, CameraAccess, CameraStorage};
use dalica_scan::ptp::codes::format;
use dalica_scan::ptp::responder::MemoryCamera;
use dalica_scan::ptp::{Limits, PtpError, Transport, TransportError};
use dalica_ui::headless::{Click, KeyState};
use dalica_ui::i18n::pseudo;
use dalica_ui::panels::edit::video::bar;
use dalica_ui::state::View;
use dalica_ui::{App, Driver};
use egui::epaint::{ClippedShape, Shape};
use egui::{PointerButton, Rect};
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

// --- The window ----------------------------------------------------------------------------------

/// A camera with three pictures, each with a thumbnail of the camera's own.
fn camera() -> (MemoryCamera, u32) {
    let mut cam = MemoryCamera::new("Canon", "Canon EOS R6");
    let storage = cam.add_storage("SD1", 64 << 30);
    let dcim = cam.add_folder(storage, 0, "DCIM");
    let folder = cam.add_folder(storage, dcim, "100CANON");
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 1, 192 + 8 * i as u32, 128), 85, None).unwrap();
        let thumb = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 1, 160, 120), 80, None).unwrap();
        let h = cam.add_file(storage, folder, &format!("IMG_{:04}.JPG", i + 1), &format!("20260604T08{i:02}00"), bytes);
        let o = cam.object_mut(h).unwrap();
        o.info.thumb_format = format::EXIF_JPEG;
        o.info.thumb_size = thumb.len() as u32;
        o.thumb = thumb;
    }
    (cam, storage)
}

/// The demo library in the pseudo-locale with `Trip` (a clip it plays, `clip.mp4`, and one of a
/// codec it cannot, `old.mov`) added, a camera connected (and one another program holds), the
/// silent sound.
fn pseudo_library(dir: &std::path::Path, (w, h): (u32, u32)) -> Driver {
    let clock = Arc::new(dalica_core::SystemClock);
    let mut s = demo_open(dir, clock.clone());
    let trip = dir.join("Trip");
    std::fs::create_dir_all(&trip).unwrap();
    std::fs::write(trip.join("clip.mp4"), clip(b"avc1", b"mp4a")).unwrap();
    std::fs::write(trip.join("old.mov"), clip(b"mp4v", b"mp4a")).unwrap();
    s.execute("folders.add", &json!({ "path": trip })).unwrap();
    s.wait_idle();
    s.set_sound(Arc::new(Silent::new(clock)));
    s.set_pictures_dir(Some(dir.join("Pictures")));
    let (cam, storage) = camera();
    let (transport, _handle) = cam.connect();
    let slot = Mutex::new(Some(transport));
    let opener: Opener = Arc::new(move |_id: &str| {
        let t = slot.lock().unwrap().take().ok_or(PtpError::Transport(TransportError::Disconnected))?;
        dalica_scan::ptp::Session::open(Box::new(t) as Box<dyn Transport>, Limits::default())
    });
    let canon = Camera {
        id: "usb-1-4".into(),
        vendor: "Canon".into(),
        model: "Canon EOS R6".into(),
        serial: String::new(),
        storages: vec![CameraStorage { id: storage, name: "SD1".into(), capacity: 64 << 30, free: 32 << 30 }],
        access: CameraAccess::Open,
    };
    let nikon = Camera {
        id: "usb-1-5".into(),
        vendor: "Nikon".into(),
        model: "Z6".into(),
        serial: String::new(),
        storages: Vec::new(),
        access: CameraAccess::Held("busy".into()),
    };
    let mounts = dir.join("mounts");
    std::fs::create_dir_all(&mounts).unwrap();
    s.set_import_device_roots(Some(std::env::join_paths([&mounts]).unwrap()), Duration::from_millis(50));
    s.set_camera_backend(Some((Box::new(move || vec![canon.clone(), nikon.clone()]), opener)), Duration::from_millis(50));
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.run("options.set", &json!({ "values": { "interface.language": "qps" } })).unwrap();
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    d
}

/// The texts no catalog may translate: what the library holds (names, captions, tags, paths), the
/// cameras' names and their pictures' names, and the handed file's folder.
fn user_data(d: &Driver, pictures: Option<&std::path::Path>) -> BTreeSet<String> {
    let lib = d.app().session().unwrap().library();
    let mut out = BTreeSet::new();
    for f in lib.folders() {
        out.insert(f.path.display().to_string());
        out.extend(f.path.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    for p in lib.photos() {
        out.insert(p.name.clone());
        let info = lib.info(p.id);
        out.insert(info.caption.clone());
        out.extend(info.tags.iter().cloned());
    }
    for (_, a) in lib.albums() {
        out.insert(a.name.clone());
    }
    for (_, p) in lib.people() {
        out.insert(p.name.clone());
    }
    // The cameras, their storage and pictures, and what the system says of the one held; the demo
    // photos' camera as their metadata names it (EXIF Make and Model).
    out.extend(["Canon EOS R6", "Nikon Z6", "SD1", "IMG_0001.JPG", "IMG_0002.JPG", "IMG_0003.JPG", "busy", "Dalica Scenes 1"].map(String::from));
    // The Pictures folder the Import room offers.
    if let Some(p) = pictures {
        out.insert(p.display().to_string());
        out.extend(p.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// A text that is user data: one of `data`, user data with a count after it ("Lisbon (20)"), or
/// user data cut short in its middle or at its end to fit ("2026-06… Garden").
fn is_user_data(text: &str, data: &BTreeSet<String>) -> bool {
    let bare = text.trim();
    let counted = bare.rsplit_once(" (").filter(|(_, n)| n.trim_end_matches(')').chars().all(|c| c.is_ascii_digit())).map(|(name, _)| name);
    let elided = bare
        .split_once('…')
        .is_some_and(|(head, tail)| data.iter().any(|d| d.starts_with(head) && d.ends_with(tail) && d.len() > bare.len() - '…'.len_utf8()));
    data.contains(bare) || counted.is_some_and(|n| data.contains(n)) || elided
}

/// A keyboard shortcut as a tooltip or a menu row writes it ("Ctrl+Shift+R", "F11", "Esc").
fn is_shortcut(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && !text.contains(' ') && (Key::parse(text).is_ok() || Key::parse(&text.replace("Cmd+", "Ctrl+")).is_ok())
}

/// The texts the next frame draws that went through no catalog and are not user data, each with
/// `place`.
fn untranslated(d: &mut Driver, place: &str, data: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    d.step();
    let drawn = d.drawn_texts().to_vec();
    if !drawn.iter().any(|t| pseudo::is_marked(t)) {
        out.insert(format!("{place}: nothing bracketed in {drawn:?}"));
    }
    for t in pseudo::untranslated(drawn.iter().map(String::as_str)) {
        if !is_user_data(t, data) && !is_shortcut(t) {
            out.insert(format!("{place}: {t:?}"));
        }
    }
}

/// Every text of `shapes` with the rect it is drawn in and its clip.
fn texts(shapes: &[ClippedShape]) -> Vec<(String, Rect, Rect)> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            Shape::Text(t) if !t.galley.text().trim().is_empty() => out.push((t.galley.text().to_owned(), t.visual_bounding_rect(), clip)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, s.clip_rect, &mut out);
    }
    out
}

/// The shapes of one more frame of the app in `d`'s context, run outside the driver (whose step
/// keeps none) at the time of the frame before, so the driver's clock stays ahead of egui's.
fn shapes(d: &mut Driver) -> Vec<ClippedShape> {
    let ctx = d.ctx().clone();
    let (w, h, native) = d.size();
    let mut input = dalica_ui_kit::raw_input(w, h, native);
    let ppp = native * ctx.zoom_factor();
    input.screen_rect = Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32 / ppp, h as f32 / ppp)));
    input.time = Some(ctx.input(|i| i.time));
    let mut out = ctx.run_ui(input, |ui| d.app_mut().frame(ui));
    out.textures_delta.clear();
    out.shapes
}

/// The labels the next frame cuts at the side of the area they are drawn in, or past the window.
fn clipped(d: &mut Driver, place: &str, out: &mut BTreeSet<String>) {
    let shapes = shapes(d);
    let screen = d.ctx().content_rect();
    for (text, r, clip) in texts(&shapes) {
        if !r.intersect(clip).is_positive() {
            continue; // scrolled out of sight
        }
        if r.left() < clip.left() - 0.5 || r.right() > clip.right() + 0.5 || r.right() > screen.right() + 0.5 {
            out.insert(format!("{place}: {text:?} cut at the side: {r:?} in {clip:?}"));
        }
    }
}

/// The controls of the bars along the bottom (the slideshow's, the player's) whose rect leaves the
/// window, each with `place`.
fn off_screen(d: &Driver, place: &str, prefixes: &[&str], out: &mut BTreeSet<String>) {
    let screen = d.ctx().content_rect();
    for w in d.widgets() {
        let r = Rect::from_min_size(egui::pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]));
        if prefixes.iter().any(|p| w.id.starts_with(p)) && !screen.expand(0.5).contains_rect(r) {
            out.insert(format!("{place}: {} out of the window: {r:?}", w.id));
        }
    }
}

/// Steps frames until `done` holds; at most a minute.
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&mut Driver) -> bool) {
    let start = Instant::now();
    while !done(d) {
        assert!(start.elapsed() < Duration::from_secs(60), "never: {what}");
        d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn photo(d: &Driver, name: &str) -> PhotoId {
    d.app().session().unwrap().library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id
}

fn player(d: &mut Driver) -> Value {
    d.app_mut().session_mut().unwrap().execute("video.state", &json!({})).unwrap()
}

/// What the views draw untranslated and what they clip at `size`, through each state the task
/// names; the window painted as `i18n-views-<place>-<size>.png` after each.
fn walk(size: (u32, u32)) -> (BTreeSet<String>, BTreeSet<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut d = pseudo_library(dir.path(), size);
    let data = user_data(&d, Some(&dir.path().join("Pictures")));
    let (mut text, mut cut) = (BTreeSet::new(), BTreeSet::new());
    let tag = format!("{}x{}", size.0, size.1);
    let look = |d: &mut Driver, place: &str, text: &mut BTreeSet<String>, cut: &mut BTreeSet<String>| {
        d.settle(FRAMES).unwrap();
        untranslated(d, place, &data, text);
        clipped(d, place, cut);
        shot(d, &format!("i18n-views-{}-{tag}", place.replace([' ', '/', ':', ','], "-")));
    };

    // The edit view on a photo: each tab, each tool, each effect's panel.
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    look(&mut d, "edit basic", &mut text, &mut cut);
    for tab in ["tuning", "effects", "effects2", "effects3"] {
        d.run("edit.tab", &json!({ "tab": tab })).unwrap();
        look(&mut d, &format!("edit {tab}"), &mut text, &mut cut);
    }
    for (tab, n) in [("effects", 3u8), ("effects2", 4), ("effects3", 5)] {
        d.run("edit.tab", &json!({ "tab": tab })).unwrap();
        for (row, effect) in dalica_edit::catalog::effects(n) {
            if effect.one_click {
                continue;
            }
            d.run("edit.tool.open", &json!({ "tool": "effect", "name": row.name })).unwrap();
            look(&mut d, &format!("effect {}", row.name), &mut text, &mut cut);
            // A choice's words in its menu (Pixelate's blend mode).
            let choices: Vec<String> =
                d.widgets().into_iter().filter(|w| w.id.starts_with("choice:effect.") && !w.id.contains('/')).map(|w| w.id).collect();
            for c in choices {
                d.click(&c).unwrap();
                look(&mut d, &format!("effect {} {c}", row.name), &mut text, &mut cut);
                d.key("Escape").unwrap();
            }
            d.run("edit.tool.cancel", &json!({})).unwrap();
        }
    }
    d.run("edit.tab", &json!({ "tab": "basic" })).unwrap();
    for tool in ["crop", "straighten", "retouch"] {
        d.run("edit.tool.open", &json!({ "tool": tool })).unwrap();
        look(&mut d, &format!("tool {tool}"), &mut text, &mut cut);
        if tool == "crop" {
            // The sizes and ratios in their menu.
            d.click("choice:crop.ratio").unwrap();
            look(&mut d, "tool crop ratios", &mut text, &mut cut);
            d.key("Escape").unwrap();
        }
        d.run("edit.tool.cancel", &json!({})).unwrap();
    }
    // An edit made, so the history names a step; then A│B compare.
    d.run("edit.autoFix", &json!({})).unwrap();
    look(&mut d, "edit fixed", &mut text, &mut cut);
    d.run("edit.compare", &json!({ "mode": "two" })).unwrap();
    look(&mut d, "compare", &mut text, &mut cut);
    d.run("edit.compare", &json!({ "mode": "one" })).unwrap();
    d.run("faces.addManually", &json!({ "on": true })).unwrap();
    look(&mut d, "face box", &mut text, &mut cut);
    d.run("faces.addManually", &json!({ "on": false })).unwrap();

    // A clip it plays, trimmed with Set End; a clip it cannot play.
    let clip = photo(&d, "clip.mp4");
    d.run("select.set", &json!({ "ids": [clip.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    until(&mut d, "the clip's first frame", |d| {
        let v = player(d);
        v["ready"] == true && v["frame"].is_object()
    });
    look(&mut d, "clip", &mut text, &mut cut);
    d.run("player.seek", &json!({ "ms": 1500 })).unwrap();
    until(&mut d, "at 1.5 s", |d| player(d)["pending"] == false);
    d.click(bar::SET_END_ID).unwrap();
    look(&mut d, "clip trimmed", &mut text, &mut cut);
    off_screen(&d, "clip trimmed", &["button:video.", "slider:video.", "toggle:video."], &mut cut);
    let old = photo(&d, "old.mov");
    d.run("select.set", &json!({ "ids": [old.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    until(&mut d, "the clip it cannot play", |d| player(d)["playable"] == false);
    look(&mut d, "clip unplayable", &mut text, &mut cut);
    d.run("view.library", &json!({})).unwrap();

    // The slideshow with its bar, and its Transition drop-up.
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.slideshow", &json!({})).unwrap();
    d.step();
    untranslated(&mut d, "slideshow", &data, &mut text);
    clipped(&mut d, "slideshow", &mut cut);
    off_screen(&d, "slideshow", &["button:slideshow.", "stepper:slideshow.", "check:slideshow.", "slider:slideshow."], &mut cut);
    shot(&d, &format!("i18n-views-slideshow-{tag}"));
    d.click(dalica_ui::panels::slideshow::bar::TRANSITION_ID).unwrap();
    d.step();
    d.step();
    untranslated(&mut d, "slideshow transition", &data, &mut text);
    clipped(&mut d, "slideshow transition", &mut cut);
    d.key("Escape").unwrap();
    d.run("slideshow.exit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();

    // The Collage room on four photos: Settings, Clips, a picture's menu.
    let ids: Vec<u64> = d.app().session().unwrap().library().photos().take(4).map(|p| p.id.get()).collect();
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.run("create.collage", &json!({ "ids": ids })).unwrap();
    look(&mut d, "collage", &mut text, &mut cut);
    for menu in [dalica_ui::panels::collage::settings::STYLE_ID, dalica_ui::panels::collage::settings::FORMAT_ID] {
        d.click(menu).unwrap();
        look(&mut d, &format!("collage {menu}"), &mut text, &mut cut);
        d.key("Escape").unwrap();
    }
    d.click_with(&dalica_ui::panels::collage::canvas::item_id(0), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    assert!(d.widgets().iter().any(|w| w.id.starts_with("menuitem:")), "the picture's menu is open");
    look(&mut d, "collage picture menu", &mut text, &mut cut);
    d.key("Escape").unwrap();
    d.run("collage.tab", &json!({ "tab": "clips" })).unwrap();
    look(&mut d, "collage clips", &mut text, &mut cut);

    // The Import room on the camera: its source menu, a picture previewed.
    d.run("import.open", &json!({})).unwrap();
    until(&mut d, "the camera listed", |d| {
        let l = d.app_mut().session_mut().unwrap().execute("import.list", &json!({})).unwrap();
        l["state"] == "done" && l["candidates"].as_array().is_some_and(|c| c.len() == 3)
    });
    look(&mut d, "import", &mut text, &mut cut);
    d.click("choice:import.source").unwrap();
    look(&mut d, "import source", &mut text, &mut cut);
    d.key("Escape").unwrap();
    d.click("choice:import.destination").unwrap();
    look(&mut d, "import destination", &mut text, &mut cut);
    d.key("Escape").unwrap();
    d.click("choice:import.folderName").unwrap();
    look(&mut d, "import folder name", &mut text, &mut cut);
    d.key("Escape").unwrap();
    let cell = d.widgets().into_iter().find(|w| w.id.strip_prefix("candidate:").is_some_and(|n| n.parse::<u64>().is_ok())).unwrap().id;
    d.click_with(&cell, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    assert!(d.widgets().iter().any(|w| w.id.starts_with("menuitem:import.")), "the tray's menu is open");
    look(&mut d, "import menu", &mut text, &mut cut);
    d.key("Escape").unwrap();
    d.run("room.close", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();

    // The quick preview over a thumbnail.
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:") && !w.id[6..].contains('/')).unwrap().id;
    d.hover(&cell, 0).unwrap();
    d.key_state("Ctrl+Alt+Space", KeyState::Down).unwrap();
    look(&mut d, "peek", &mut text, &mut cut);
    d.key_state("Ctrl+Alt+Space", KeyState::Up).unwrap();
    d.settle(FRAMES).unwrap();

    // The viewer, handed a file by another launch, its More menu open.
    let file = d.app().session().unwrap().library().folders().next().map(|f| f.path.clone()).unwrap();
    let file = std::fs::read_dir(&file).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "jpg")).unwrap();
    d.app_mut().session_mut().unwrap().hand_viewer(&file).unwrap();
    // What the app's receiver does with a handed file (`apps/dalica/src/instance/receive.rs`).
    let state = d.app_mut().state_mut();
    state.full_screen = true;
    state.view = View::Viewer;
    state.viewer = Some(dalica_ui::state::ViewerState::default());
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().view, View::Viewer);
    look(&mut d, "viewer", &mut text, &mut cut);
    d.click(dalica_ui::panels::viewer::bar::MORE_ID).unwrap();
    look(&mut d, "viewer more", &mut text, &mut cut);
    d.key("Escape").unwrap();
    (text, cut)
}

#[test]
fn every_text_of_the_views_goes_through_the_catalogs_and_no_label_clips() {
    for size in [LARGE, SMALL] {
        let (text, cut) = walk(size);
        assert!(text.is_empty(), "{size:?}: {} texts drawn without a catalog:\n{}", text.len(), text.into_iter().collect::<Vec<_>>().join("\n"));
        assert!(cut.is_empty(), "{size:?}: {} labels clipped or out of the window:\n{}", cut.len(), cut.into_iter().collect::<Vec<_>>().join("\n"));
    }
}
