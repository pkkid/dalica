//! Videos in the live app (`m15.rs` part 1; `roadmap.md` M15 → Video's *Done when*), driven over
//! the control channel of the real `dalica --headless` on a new library of two folders:
//!
//! - `Clips`: two painted photos and three H.264 clips of 2 s with sound (`clip.mp4`, `clip.mov`,
//!   `clip.mkv`), each lit from its own grey so its poster tells which it is;
//! - `Old`: `old.mov`, whose video is MPEG-4 Part 2 (a codec Dalica has no decoder for),
//!   `hevc.mp4` (HEVC, left out of a build without the `hevc` feature), and `cut.mp4`, a hostile
//!   container (its `moov` claims 2 GiB and the file ends after 64 bytes of
//!   it).
//!
//! It checks: the three clips list as videos of 2 s, the photos as stills; the hostile one is
//! listed with no duration and the reason it cannot play, never a panic; each clip's cell shows its
//! poster (the first frame that is not black, by its grey); the Videos filter leaves only the
//! videos; Export of photos and clips writes each playable clip's poster frame as a JPEG and counts
//! the clip it cannot play, skipped with the reason; a slideshow counts the clip and plays it to
//! its end (2 s against a display time of 1 s); in the edit view the player plays, pauses, steps a
//! frame with `.`, trims at the playhead (Set End, Set Start) with the readout saying so, and Save
//! Frame as Picture writes the frame shown (by its grey) into `Pictures/Dalica/Video Frames`, which
//! the library lists; the clip it cannot play shows "This video can't be played" with Open With,
//! Play greyed; after a restart the trim is still there. Fingerprints at 1400 × 900: the library
//! with the video cells (`m15-1400x900-library-videos`) and the player trimmed
//! (`m15-1400x900-player`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::{self, click, idle, key, run, widget};
use crate::clips::{self, BLACK, DURATION_MS, SIZE};
use crate::common::{self, COLOURS, LARGE, PATIENCE, steady, steady_shot, until};
use crate::live::{self, Client, Launch};

/// The greys the three playable clips are lit from.
const MP4: u8 = 40;
const MOV: u8 = 60;
const MKV: u8 = 80;
/// What Dalica says of `old.mov`.
const PART2: &str = "This video can't be played (MPEG-4 Part 2)";
/// What a build without the `hevc` feature says of an HEVC clip.
const HEVC: &str = "This video can't be played (HEVC support isn't included in this build)";

/// The library's photos by name: (id, `photo.inspect`).
fn by_name(c: &mut Client) -> std::collections::BTreeMap<String, (u64, Value)> {
    let ids: Vec<Value> = c.photos().into_iter().map(|p| p["id"].clone()).collect();
    let r = c.exec("photo.inspect", json!({ "ids": ids }));
    r.as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap().to_string(), (p["id"].as_u64().unwrap(), p.clone()))).collect()
}

/// The id of the photo named `name`.
fn id(c: &mut Client, name: &str) -> u64 {
    by_name(c).get(name).unwrap_or_else(|| panic!("no {name} in the library")).0
}

/// The grey at the middle of an image (away from the white top-left macroblock).
fn middle(img: &dalica_image::Rgba8) -> u8 {
    let at = ((img.height as usize / 2) * img.width as usize + img.width as usize * 3 / 4) * 4;
    img.data[at]
}

/// The grey a whole-window shot shows at the middle of the widget `id`'s rect, and whether its red,
/// green and blue are one grey there.
fn grey_at(shot: &live::Shot, rect: [f64; 4]) -> (u8, bool) {
    let (x, y) = ((rect[0] + rect[2] * 0.7) as u32, (rect[1] + rect[3] / 2.0) as u32);
    let i = ((y * shot.width + x) * 4) as usize;
    let px = &shot.rgba[i..i + 3];
    (px[1], px.iter().all(|&v| (i32::from(v) - i32::from(px[1])).abs() <= 2))
}

/// `video.state` once `done` holds of it, asked between frames without waiting for the app to
/// settle (a playing clip keeps it drawing).
fn player_until(c: &mut Client, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + PATIENCE;
    loop {
        let v = c.exec("video.state", json!({}));
        if done(&v) {
            return v;
        }
        assert!(Instant::now() < end, "{what}: still not so after {PATIENCE:?}: {v}");
        c.ok("ui.wait", json!({ "ms": 20 }));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Starts the app on `home`'s library with automation writing inside `home` (the Pictures folder,
/// Export's folder, the screenshots).
fn start(exe: &Path, home: &Path) -> (live::App, Client) {
    live::start_with(exe, home, Launch::default().arg("--automation-write-root").arg(home))
}

pub fn videos(exe: &Path, home: &Path) {
    let clips_dir = home.join("Clips");
    let old = home.join("Old");
    for d in [&clips_dir, &old] {
        std::fs::create_dir_all(d).unwrap();
    }
    app::put(&clips_dir.join("a.jpg"), &common::painted_jpeg(320, 240, COLOURS[0]), 1);
    app::put(&clips_dir.join("b.jpg"), &common::painted_jpeg(320, 240, COLOURS[2]), 2);
    app::put(&clips_dir.join("clip.mp4"), &clips::mp4(MP4), 3);
    app::put(&clips_dir.join("clip.mov"), &clips::mov(MOV), 4);
    app::put(&clips_dir.join("clip.mkv"), &clips::mkv(MKV), 5);
    app::put(&old.join("old.mov"), &clips::mpeg4_part2(), 6);
    app::put(&old.join("cut.mp4"), &clips::hostile(), 7);
    app::put(&old.join("hevc.mp4"), &clips::hevc(), 8);
    let originals: Vec<(PathBuf, Vec<u8>)> = [&clips_dir, &old]
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap().flatten().map(|e| e.path()))
        .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
        .collect();

    let (app_, mut c) = start(exe, home);
    for d in [&clips_dir, &old] {
        run(&mut c, "folders.add", json!({ "path": d }));
    }
    idle(&mut c);

    // --- Listed: three clips of 2 s and two photos; the hostile container listed, never a panic.
    let listed = by_name(&mut c);
    for name in ["clip.mp4", "clip.mov", "clip.mkv", "old.mov"] {
        let p = &listed[name].1;
        assert_eq!((&p["kind"], &p["durationMs"]), (&json!("video"), &json!(DURATION_MS)), "{name}: {p}");
        assert_eq!((&p["width"], &p["height"]), (&json!(SIZE.0), &json!(SIZE.1)), "{name}: {p}");
    }
    for name in ["a.jpg", "b.jpg"] {
        assert_eq!((&listed[name].1["kind"], &listed[name].1["durationMs"]), (&json!("still"), &Value::Null), "{name}");
    }
    let cut = &listed["cut.mp4"];
    assert_eq!((&cut.1["kind"], &cut.1["durationMs"]), (&json!("video"), &Value::Null), "the hostile container: {}", cut.1);
    let inspected = c.exec("video.inspect", json!({ "id": cut.0 }));
    assert_eq!(inspected["playable"], false, "{inspected}");
    assert!(inspected["reason"].as_str().is_some_and(|r| r.starts_with("This video can't be played")), "{inspected}");
    for (name, codec) in [("clip.mp4", "avc1"), ("clip.mov", "avc1"), ("clip.mkv", "V_MPEG4/ISO/AVC")] {
        let v = c.exec("video.inspect", json!({ "id": listed[name].0 }));
        assert_eq!((&v["playable"], &v["videoCodec"], &v["soundPlays"]), (&json!(true), &json!(codec), &json!("plays")), "{name}: {v}");
    }
    let v = c.exec("video.inspect", json!({ "id": listed["old.mov"].0 }));
    assert_eq!((&v["playable"], &v["reason"]), (&json!(false), &json!(PART2)), "{v}");
    // HEVC: decoded in a build with the `hevc` feature (its suite in `m15_checks.rs`); this build
    // leaves it out and says so.
    let v = c.exec("video.inspect", json!({ "id": listed["hevc.mp4"].0 }));
    assert_eq!((&v["playable"], &v["reason"]), (&json!(false), &json!(HEVC)), "{v}");

    // --- The cells: each clip's poster, the first frame that is not black.
    let shot = steady(&mut c, home, "m15-library-videos-probe");
    for (name, base) in [("clip.mp4", MP4), ("clip.mov", MOV), ("clip.mkv", MKV)] {
        let cell = app::rect_of(&mut c, &format!("thumb:{}", listed[name].0)).unwrap_or_else(|| panic!("{name}'s cell is drawn"));
        let (grey, flat) = grey_at(&shot, cell);
        let poster = clips::shown(clips::luma(base, BLACK));
        assert!(flat && (i32::from(grey) - i32::from(poster)).abs() <= 4, "{name}'s cell shows its poster (grey {poster}), not {grey}");
    }
    steady_shot(&mut c, home, "m15-1400x900-library-videos", &[]);

    // --- The Videos filter leaves only the videos.
    click(&mut c, "button:filter.videos");
    let found = c.exec("library.query", json!({ "section": "album:search", "limit": 100 }));
    let kinds: Vec<&str> = found["photos"].as_array().unwrap().iter().map(|p| p["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["video"; 6], "{found}");
    click(&mut c, "button:filter.videos");

    // --- Export: each playable clip as its poster frame, the one Dalica cannot play counted and
    // skipped with the reason.
    let out = home.join("Out");
    let ids: Vec<u64> = ["a.jpg", "clip.mp4", "clip.mov", "clip.mkv", "old.mov"].iter().map(|n| listed[*n].0).collect();
    let reply = c.ok("engine.execute", json!({ "command": "export.run", "params": { "ids": ids, "dir": out, "folderName": "Clips" } }));
    assert_eq!(reply["videos"], 1, "the clip left out is counted: {reply}");
    let job = if reply["pending"] == true { app::wait_job(&mut c, &reply["job"]) } else { reply.clone() };
    let result = &job["result"];
    assert_eq!(job["state"], "done", "{job}");
    let skipped = result["skipped"].as_array().unwrap();
    assert!(skipped.len() == 1 && skipped[0]["file"].as_str().unwrap().ends_with("old.mov") && skipped[0]["reason"] == PART2, "{job}");
    let written: Vec<PathBuf> = result["written"].as_array().unwrap().iter().map(|p| PathBuf::from(p.as_str().unwrap())).collect();
    assert_eq!(written.len(), 4, "{job}");
    let mut greys: Vec<u8> = written
        .iter()
        .filter(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with("clip")))
        .map(|p| {
            let img = dalica_image::decode(&std::fs::read(p).unwrap(), &dalica_image::Limits::default()).unwrap();
            assert_eq!((img.width, img.height), SIZE, "{}", p.display());
            middle(&img)
        })
        .collect();
    greys.sort_unstable();
    let mut posters: Vec<u8> = [MP4, MOV, MKV].iter().map(|b| clips::shown(clips::luma(*b, BLACK))).collect();
    posters.sort_unstable();
    assert_eq!(greys.len(), 3, "{written:?}");
    for (g, p) in greys.iter().zip(&posters) {
        assert!((i32::from(*g) - i32::from(*p)).abs() <= 3, "each clip exported as its poster frame: {greys:?} against {posters:?}");
    }

    // --- The slideshow counts the clip and plays it to its end (2 s; a photo shows for 1 s).
    c.exec("options.set", json!({ "values": { "slideshow.displayTime": 1, "slideshow.transition": "cut" } }));
    let (mp4, a) = (listed["clip.mp4"].0, listed["a.jpg"].0);
    let started = Instant::now();
    let show = c.exec("view.slideshow", json!({ "ids": [mp4, a], "start": mp4 }));
    assert_eq!((&show["photos"], &show["videos"], &show["start"]), (&json!(2), &json!(1), &json!(0)), "{show}");
    let index = |c: &mut Client| c.state()["slideshow"]["index"].as_u64();
    let end = Instant::now() + PATIENCE;
    while index(&mut c) == Some(0) {
        assert!(Instant::now() < end, "the slideshow never left the clip");
        c.ok("ui.wait", json!({ "ms": 20 }));
        std::thread::sleep(Duration::from_millis(20));
    }
    let held = started.elapsed();
    assert_eq!(index(&mut c), Some(1), "the photo after the clip");
    assert!(held >= Duration::from_millis(DURATION_MS - 200), "the clip played to its end, not for the display time: {held:?}");
    key(&mut c, "Escape");
    assert_eq!(c.state()["view"], "library");

    // --- The player in the edit view.
    c.exec("select.set", json!({ "ids": [mp4] }));
    key(&mut c, "Enter");
    assert_eq!((c.state()["view"].clone(), c.state()["edit"]["photo"].clone()), (json!("edit"), json!(mp4)));
    assert!(widget(&mut c, "region:video").is_some(), "the player is shown");
    player_until(&mut c, "the player has the clip", |v| v["ready"] == true && v["id"] == mp4);
    c.ok("ui.clickWidget", json!({ "id": "button:video.toggle" }));
    player_until(&mut c, "the clip plays on", |v| v["playing"] == true && v["positionMs"].as_u64().is_some_and(|ms| ms > 0));
    c.ok("ui.clickWidget", json!({ "id": "button:video.toggle" }));
    player_until(&mut c, "paused", |v| v["playing"] == false);
    c.exec("player.seek", json!({ "ms": 1000 }));
    player_until(&mut c, "the frame at 1 s", |v| v["pending"] == false && v["frame"]["index"] == 25);
    key(&mut c, ".");
    let v = player_until(&mut c, "a frame on", |v| v["pending"] == false && v["frame"]["index"] == 26);
    assert_eq!(v["frame"]["timeMs"], 1040, "{v}");
    assert_eq!(c.exec("player.setEnd", json!({}))["trim"], json!({ "startMs": 0, "endMs": 1040 }));
    c.exec("player.seek", json!({ "ms": 400 }));
    player_until(&mut c, "the frame at 0.4 s", |v| v["pending"] == false && v["frame"]["index"] == 10);
    assert_eq!(c.exec("player.setStart", json!({}))["trim"], json!({ "startMs": 400, "endMs": 1040 }));
    idle(&mut c);
    assert!(app::label(&mut c, "slider:video.position/readout").contains("(trimmed to"), "the readout says the clip is trimmed");
    steady_shot(&mut c, home, "m15-1400x900-player", &[]);

    // Save Frame as Picture: the frame shown, into Video Frames, which the library lists.
    c.exec("player.seek", json!({ "ms": 1000 }));
    player_until(&mut c, "the frame at 1 s again", |v| v["pending"] == false && v["frame"]["index"] == 25);
    click(&mut c, "button:video.saveFrame");
    let frames = home.join("Pictures").join("Dalica").join("Video Frames");
    let saved = frames.join("clip 00-00-01.jpg");
    until(&mut c, "the frame saved", |_| saved.is_file());
    idle(&mut c);
    let img = dalica_image::decode(&std::fs::read(&saved).unwrap(), &dalica_image::Limits::default()).unwrap();
    assert_eq!((img.width, img.height), SIZE);
    assert_eq!(clips::frame_of(MP4, middle(&img)), Some(25), "the frame shown was saved (grey {})", middle(&img));
    until(&mut c, "Video Frames is in the library", |c| by_name(c).get("clip 00-00-01.jpg").is_some_and(|p| p.1["kind"] == "still"));
    key(&mut c, "Escape");

    // --- A clip Dalica cannot play: the tile, the reason and Open With; Play greyed.
    c.exec("select.set", json!({ "ids": [listed["old.mov"].0] }));
    key(&mut c, "Enter");
    assert_eq!(app::label(&mut c, "text:video.cannotPlay"), "This video can't be played");
    let open_with = widget(&mut c, "button:video.openWith").expect("Open With beside the reason");
    assert_eq!((&open_with["label"], &open_with["enabled"]), (&json!("Open With"), &json!(true)), "{open_with}");
    assert_eq!(widget(&mut c, "button:video.toggle").map(|w| w["enabled"].clone()), Some(json!(false)), "Play greyed");
    key(&mut c, "Escape");

    // --- The trim survives a restart; the clips were only read.
    live::quit(app_, c);
    let (app_, mut c) = start(exe, home);
    idle(&mut c);
    let mp4 = id(&mut c, "clip.mp4");
    let v = c.exec("video.inspect", json!({ "id": mp4 }));
    assert_eq!(v["trim"], json!({ "startMs": 400, "endMs": 1040 }), "{v}");
    live::quit(app_, c);
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}: only read", path.display());
    }
    let _ = LARGE;
}
