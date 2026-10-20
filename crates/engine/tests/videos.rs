//! Videos in the session (`roadmap.md` M15.16; `editing.md` §11, `viewing.md` §4, `creating.md`
//! §2–§3), decision-neutral: in a library of two photos and a clip (an MP4 built here from the
//! documented ISO/IEC 14496-12 boxes, with no frames Dalica decodes), `video.inspect` reports the
//! clip's facts, that Dalica cannot play it yet and its trim; `photo.inspect` and `library.query`
//! carry each photo's kind and duration; an export of the folder writes two files and counts one
//! video; the slideshow plays two and counts one; a collage of the three holds two clips and says
//! why; Batch Edit, an effect, Crop and Save on the clip are refused with the reason and nothing is
//! written; its thumbnail is the video tile (no decode, no error); the viewer lists it as a video
//! and never decodes it; the face scan never reads it; the owner thread made no `fs` call.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, CmdError, JobState, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::{jpeg_of, read};

const PATIENCE: Duration = Duration::from_secs(300);

// --- An MP4 from its boxes ------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

/// Version 0 `mvhd`/`mdhd` times.
fn times(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0; 4];
    for v in [0, 0, timescale, duration] {
        p.extend(v.to_be_bytes());
    }
    p
}

fn matrix() -> Vec<u8> {
    IDENTITY.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0; 8];
    p.extend(handler);
    p.extend([0; 13]);
    bx(b"hdlr", &p)
}

/// 2.5 s of 640 × 360 ProRes (a codec no build decodes, so the clip stays a tile under M15.20) at
/// 30 frames a second, with an AAC sound track: the facts only (the `mdat` holds no frames).
fn clip() -> Vec<u8> {
    let mut mvhd = times(1000, 2_500);
    mvhd.extend(0x1_0000u32.to_be_bytes());
    mvhd.extend(0x100u16.to_be_bytes());
    mvhd.extend([0; 10]);
    mvhd.extend(matrix());
    mvhd.extend([0; 24]);
    mvhd.extend(3u32.to_be_bytes());
    let track = |id: u32, handler: &[u8; 4], code: &[u8; 4], (w, h): (u16, u16), timescale: u32, duration: u32, samples: u32| {
        let mut tkhd = vec![0, 0, 0, 7];
        tkhd.extend([0; 8]);
        tkhd.extend(id.to_be_bytes());
        tkhd.extend([0; 4 + 4 + 8 + 8]);
        tkhd.extend(matrix());
        tkhd.extend((u32::from(w) << 16).to_be_bytes());
        tkhd.extend((u32::from(h) << 16).to_be_bytes());
        let mut entry = vec![0; 6];
        entry.extend(1u16.to_be_bytes());
        entry.extend([0; 16]);
        entry.extend(w.to_be_bytes());
        entry.extend(h.to_be_bytes());
        entry.extend([0; 50]);
        let mut stsd = vec![0; 4];
        stsd.extend(1u32.to_be_bytes());
        stsd.extend(bx(code, &entry));
        let mut stsz = vec![0; 4];
        stsz.extend(1000u32.to_be_bytes());
        stsz.extend(samples.to_be_bytes());
        let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stsz", &stsz)].concat());
        let mut mdhd = times(timescale, duration);
        mdhd.extend([0; 4]);
        let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), hdlr(handler), bx(b"minf", &stbl)].concat());
        bx(b"trak", &[bx(b"tkhd", &tkhd), mdia].concat())
    };
    let moov = bx(
        b"moov",
        &[bx(b"mvhd", &mvhd), track(1, b"vide", b"apcn", (640, 360), 30_000, 75_000, 75), track(2, b"soun", b"mp4a", (0, 0), 48_000, 120_000, 118)]
            .concat(),
    );
    let mut ftyp = b"mp42".to_vec();
    ftyp.extend(0x200u32.to_be_bytes());
    ftyp.extend(b"mp42isom");
    [bx(b"ftyp", &ftyp), bx(b"mdat", &[0x5a; 256]), moov].concat()
}

// --- The library ----------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    /// a.jpg, b.jpg, clip.mp4.
    ids: [PhotoId; 3],
    /// The paths the thumbnail scans read, in order.
    thumbs_read: Arc<Mutex<Vec<PathBuf>>>,
}

impl Lib {
    /// A library watching `Photos`, whose `Trip` holds two photos and a clip; `Pictures` is the
    /// user's Pictures folder.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Photos").join("Trip");
        dalica_fs::create_dir_all(&trip).unwrap();
        dalica_fs::replace(&trip.join("a.jpg"), &jpeg_of(1, 160, 120), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("b.jpg"), &jpeg_of(2, 120, 160), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("clip.mp4"), &clip(), Expect::Absent).unwrap();
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        let thumbs_read = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&thumbs_read);
        s.set_thumbnail_scan_hook(Some(Arc::new(move |path: &Path| seen.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_path_buf()))));
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id;
        let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4")];
        Lib { dir, s, ids, thumbs_read }
    }

    fn trip(&self) -> PathBuf {
        self.dir.path().join("Photos").join("Trip")
    }

    fn clip(&self) -> PhotoId {
        self.ids[2]
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }
}

fn disabled(r: Result<Value, CmdError>) -> String {
    match r {
        Err(CmdError::Disabled(why)) => why,
        other => panic!("expected disabled, got {other:?}"),
    }
}

// --- The tests --------------------------------------------------------------------------------------

#[test]
fn video_inspect_reports_the_facts_and_that_dalica_cannot_play_it_yet() {
    let mut lib = Lib::new();
    let clip = lib.clip();
    let v = lib.run("video.inspect", json!({ "id": clip }));
    assert_eq!(v["id"], json!(clip));
    assert_eq!(v["kind"], "video");
    assert_eq!(v["durationMs"], 2_500);
    assert_eq!((v["width"].clone(), v["height"].clone(), v["rotation"].clone()), (json!(640), json!(360), json!(0)));
    assert_eq!(v["displayed"], json!({ "width": 640, "height": 360 }));
    assert_eq!((v["videoCodec"].clone(), v["soundCodec"].clone(), v["sound"].clone()), (json!("apcn"), json!("mp4a"), json!(true)));
    assert_eq!(v["frameRateMilli"], 30_000);
    assert_eq!(v["playable"], false, "no decoder yet: decision-neutral");
    assert_eq!(v["reason"], "This video can't be played");
    assert_eq!(v["trim"], Value::Null, "no trim in its recipe");
    assert_eq!(v["path"], json!(lib.trip().join("clip.mp4")));

    // A still is not a video; an unknown id is not found.
    let why = disabled(lib.s.execute("video.inspect", &json!({ "id": lib.ids[0] })));
    assert_eq!(why, "a.jpg is not a video.");
    assert!(matches!(lib.s.execute("video.inspect", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
}

#[test]
fn photo_inspect_and_library_query_carry_the_kind_and_duration() {
    let mut lib = Lib::new();
    let [a, _, clip] = lib.ids;
    let v = lib.run("photo.inspect", json!({ "ids": [a, clip] }));
    assert_eq!((v[0]["kind"].clone(), v[0]["durationMs"].clone()), (json!("still"), Value::Null));
    assert_eq!((v[1]["kind"].clone(), v[1]["durationMs"].clone()), (json!("video"), json!(2_500)));
    let q = lib.run("library.query", json!({}));
    let kinds: Vec<(String, Value, Value)> = q["photos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap().to_string(), p["kind"].clone(), p["durationMs"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("a.jpg".to_string(), json!("still"), Value::Null),
            ("b.jpg".to_string(), json!("still"), Value::Null),
            ("clip.mp4".to_string(), json!("video"), json!(2_500))
        ]
    );
}

#[test]
fn an_export_writes_the_photos_and_counts_the_video_and_the_slideshow_leaves_it_out() {
    let mut lib = Lib::new();
    let ids = lib.ids;
    let folder = lib.s.library().photo(ids[0]).unwrap().folder;
    let section = format!("folder:{folder}");
    let location = lib.dir.path().join("Pictures").join("Out");
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let reply = lib.run("export.run", json!({ "section": section, "dir": location, "folderName": "Trip", "size": 100 }));
    let job = reply["job"].as_u64().unwrap();
    let done = lib.s.wait_job(job, PATIENCE).unwrap();
    let show = lib.run("slideshow.list", json!({ "section": section }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!((reply["total"].clone(), reply["videos"].clone()), (json!(2), json!(1)), "{reply}");
    assert_eq!(done.state, JobState::Done, "{done:?}");
    let written = done.result.unwrap()["written"].clone();
    assert_eq!(written, json!([location.join("Trip").join("a.jpg"), location.join("Trip").join("b.jpg")]));
    assert_eq!(show["photos"], json!([ids[0], ids[1]]));
    assert_eq!(show["videos"], 1);
}

#[test]
fn a_collage_of_the_three_holds_the_two_photos_and_says_why() {
    let mut lib = Lib::new();
    let ids = lib.ids;
    let opened = lib.run("create.collage", json!({ "ids": ids, "seed": "5eed" }));
    assert_eq!((opened["clips"].clone(), opened["videos"].clone()), (json!(2), json!(1)), "{opened}");
    assert_eq!(opened["notice"], "1 video left out: videos can't go in a collage yet.");
    let added = lib.run("collage.clips.add", json!({ "ids": [ids[2]] }));
    assert_eq!((added["added"].clone(), added["videos"].clone()), (json!(0), json!(1)), "{added}");
    assert_eq!(added["notice"], "1 video left out: videos can't go in a collage yet.");
    lib.run("collage.close", json!({ "confirm": true }));
    // The clip alone: nothing to open, and why.
    let why = disabled(lib.s.execute("create.collage", &json!({ "ids": [ids[2]] })));
    assert_eq!(why, "Videos can't go in a collage yet.");
}

#[test]
fn batch_edit_an_effect_crop_and_save_refuse_the_clip_and_nothing_is_written() {
    let mut lib = Lib::new();
    let [a, b, clip] = lib.ids;
    let path = lib.trip().join("clip.mp4");
    let bytes = read(&path);
    let refusal = "A video takes no photo edits.";

    // Batch Edit: alone refused, beside photos skipped with the reason.
    assert_eq!(disabled(lib.s.execute("edit.apply", &json!({ "ids": [clip], "op": "autofix" }))), refusal);
    let v = lib.run("edit.apply", json!({ "ids": [a, b, clip], "op": "autofix" }));
    assert_eq!(v["ids"], json!([a, b]));
    assert_eq!(v["skipped"], json!([{ "id": clip, "reason": refusal }]));
    // An effect in the edit view.
    assert_eq!(disabled(lib.s.execute("photo.history.apply", &json!({ "id": clip, "op": "bw()" }))), refusal);
    // Tuning, Auto Tuning and a recipe set over automation.
    assert_eq!(disabled(lib.s.execute("photo.history.tuning", &json!({ "id": clip, "session": 1, "fill": 0.3 }))), refusal);
    assert_eq!(disabled(lib.s.execute("photo.tuning.auto", &json!({ "id": clip, "what": "lighting" }))), refusal);
    assert_eq!(disabled(lib.s.execute("edit.recipe.set", &json!({ "ids": [clip], "recipe": "rotate(90)|sepia()" }))), refusal);
    let before = lib.s.library().info(a).edits.clone();
    assert_eq!(disabled(lib.s.execute("edit.recipe.set", &json!({ "ids": [a, clip], "recipe": "sepia()" }))), refusal);
    assert_eq!(lib.s.library().info(a).edits, before, "all or nothing: the photo beside it unchanged");
    // Crop and straighten.
    let crop = disabled(lib.s.execute("photo.crop.apply", &json!({ "id": clip, "rect": [0.1, 0.1, 0.9, 0.9] })));
    assert_eq!(crop, "A video can't be cropped.");
    let straighten = disabled(lib.s.execute("photo.straighten.apply", &json!({ "id": clip, "degrees": 2.0 })));
    assert_eq!(straighten, "A video can't be cropped.");
    assert!(lib.s.library().info(clip).edits.is_empty(), "no edit reached the clip");
    // A rotation is a video's edit (`editing.md` §11); Save refuses to write it into the clip.
    lib.run("photo.history.apply", json!({ "id": clip, "op": "rotate(90)" }));
    assert_eq!(lib.s.library().info(clip).edits, ["rotate(90)"]);
    lib.run("edit.recipe.set", json!({ "ids": [clip], "recipe": "rotate(90)|trim(0,1000)" }));
    lib.run("edit.recipe.set", json!({ "ids": [clip], "recipe": "rotate(90)" }));
    let save = disabled(lib.s.execute("photo.save", &json!({ "ids": [clip], "confirm": true })));
    assert_eq!(save, "This video can't be played, so Dalica cannot save its edits");
    lib.s.wait_idle();
    assert!(!lib.s.file_jobs_running());
    assert_eq!(read(&path), bytes, "the clip is untouched");
    let mut names = crate::common::names(&lib.trip());
    names.retain(|n| n != ".dalica.json");
    assert_eq!(names, ["a.jpg", "b.jpg", "clip.mp4"], "nothing written beside");
}

#[test]
fn the_clip_has_the_video_tile_and_is_never_decoded_for_a_thumbnail() {
    let mut lib = Lib::new();
    let clip = lib.clip();
    for _ in 0..3 {
        assert_eq!(lib.s.thumbnail(clip, 256), Thumb::NotScanned, "the video tile");
        lib.s.prefetch_thumbnails(&lib.ids, 0..3, 0.0, 256);
        lib.s.wait_idle();
    }
    assert_eq!(lib.s.thumbnail_failure(clip), None, "never the error badge");
    let refreshed = lib.run("section.refreshThumbs", json!({ "ids": [clip] }));
    assert_eq!(refreshed["photos"], 2, "Refresh Thumbnails remakes the photos, not the clip: {refreshed}");
    lib.s.wait_idle();
    assert_eq!(lib.s.thumbnail(clip, 256), Thumb::NotScanned);
    assert_eq!(lib.s.thumbnail_failure(clip), None);
    for id in &lib.ids[..2] {
        assert!(matches!(lib.s.thumbnail(*id, 128), Thumb::Image { .. }), "the photos have theirs");
    }
    let read = lib.thumbs_read.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert!(!read.is_empty() && read.iter().all(|p| !p.ends_with("clip.mp4")), "{read:?}");
}

#[test]
fn the_viewer_lists_the_clip_as_a_video_and_never_decodes_it() {
    let mut lib = Lib::new();
    let a = lib.trip().join("a.jpg");
    let clip = lib.trip().join("clip.mp4");
    let list = loop {
        let v = lib.run("viewer.list", json!({ "path": a }));
        if v.get("pending").is_none() {
            break v;
        }
        lib.s.wait_viewer_images(Duration::from_millis(20));
    };
    let photos = list["photos"].as_array().unwrap();
    let kinds: Vec<(Value, Value)> = photos.iter().map(|p| (p["name"].clone(), p["kind"].clone())).collect();
    assert_eq!(kinds, [(json!("a.jpg"), json!("still")), (json!("b.jpg"), json!("still")), (json!("clip.mp4"), json!("video"))]);
    assert_eq!(photos[2]["durationMs"], 2_500);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let pixels = lib.s.viewer_image(&clip);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!(pixels, dalica_engine::viewer::Pixels::Failed("This video can't be played".into()), "the video tile, without a decode");
    assert_eq!(lib.s.viewer_images_pending(), 0, "no decode asked for");
}

#[test]
fn the_face_scan_reads_the_photos_and_never_the_clip() {
    let mut lib = Lib::new();
    let models = crate::face_scan::models(lib.dir.path(), false);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    crate::face_scan::scan(&mut lib.s, &models);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    let stat = |s: &mut Session, key: &str| crate::face_scan::stat(s, key);
    assert_eq!((stat(&mut lib.s, "started"), stat(&mut lib.s, "read"), stat(&mut lib.s, "failed")), (2, 2, 0), "the two photos, never the clip");
    assert!(lib.s.library().faces(lib.clip()).is_empty());
}
