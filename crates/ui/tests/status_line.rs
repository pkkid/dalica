//! The status line (M4.17, `ui-layout.md` §6.1), driven headlessly at 1400 × 900 and 900 × 600 over
//! a library whose photos have known names, dates, pixel sizes and file sizes, so every case is
//! checked against the literal §6.1 string: nothing in the tray ("No selection"); one photo; several
//! photos, from one day or several; a section selected as a unit; held photos with a selection from
//! another folder (the tray's contents, not just the selection). A long file name is middle-elided
//! where the line is too narrow and shown whole where it is not; the elision order (the edit view's
//! folder prefix first, then the name's middle) and the edit view's "(12 of 48)" are checked as
//! formats for M7; a transient message replaces the text for 4 s of virtual time.
//!
//! An export's progress (M10.53, `creating.md` §2): while an export job runs (held part-way by a
//! gate, no sleep), the line reads "Exporting 1 of 4" in place of "No selection" and ends with it
//! with a photo selected, in the library and in the edit view, and loses it when the job ends; with
//! a scan running too, both show at 1400 and the scan's field is dropped first at 900.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, new_driver};
use dalica_core::{Date, DateTime, PhotoId};
use dalica_engine::{Done, FileItem, FileJob, JobId, JobKind, Session};
use dalica_scenes::{Exif, Scene};
use dalica_ui::status_text::{self, Line};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::json;

/// Every photo file is padded to this many bytes ("100.0 KB").
const FILE_BYTES: usize = 100_000;
/// The window sizes of the done-when.
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A file name long enough to be elided at 900 wide and short enough to fit at 1400.
const LONG: &str = "the_garden_in_june_with_every_rose_in_full_bloom_and_the_bees_busy_among_them_all_afternoon_long_before_the_storm_came.jpg";

/// The library: (folder, file name, day of June 2026, hour, minute).
const PHOTOS: [(&str, &str, u8, u8, u8); 7] = [
    ("Lisbon Weekend", "IMG_0361.jpg", 4, 19, 42),
    ("Lisbon Weekend", "IMG_0362.jpg", 5, 10, 15),
    ("Lisbon Weekend", "IMG_0363.jpg", 6, 9, 0),
    ("Lisbon Weekend", "IMG_0364.jpg", 6, 17, 30),
    ("Garden", "IMG_0401.jpg", 10, 8, 5),
    ("Garden", "IMG_0402.jpg", 10, 8, 6),
    ("Garden", LONG, 11, 14, 0),
];

/// `jpeg` padded to exactly `total` bytes with comment segments placed after its APPn segments
/// (so the EXIF block stays where readers look for it); the image is unchanged.
fn pad_jpeg(jpeg: &[u8], total: usize) -> Vec<u8> {
    let mut at = 2;
    while jpeg[at] == 0xFF && (0xE0..=0xEF).contains(&jpeg[at + 1]) {
        at += 2 + usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
    }
    let mut left = total.checked_sub(jpeg.len()).expect("the photo is smaller than the padded size");
    let mut out = jpeg[..at].to_vec();
    while left > 0 {
        assert!(left >= 4, "a comment segment needs 4 bytes");
        let mut body = (left - 4).min(65_533);
        if (1..4).contains(&(left - 4 - body)) {
            body -= 4;
        }
        out.extend_from_slice(&[0xFF, 0xFE]);
        out.extend_from_slice(&u16::try_from(body + 2).unwrap().to_be_bytes());
        out.resize(out.len() + body, b' ');
        left -= 4 + body;
    }
    out.extend_from_slice(&jpeg[at..]);
    assert_eq!(out.len(), total);
    out
}

/// The library in `root`, scanned: every photo 400 × 300, `FILE_BYTES` long, dated as `PHOTOS` says.
fn session(root: &Path) -> Session {
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut folders = Vec::new();
    for (i, (folder, name, day, hour, minute)) in PHOTOS.into_iter().enumerate() {
        let date = DateTime::new(Date::new(2026, 6, day).unwrap(), hour, minute, 0).unwrap();
        let exif = Exif { date_time_original: Some(date), ..Exif::default() };
        let jpeg = dalica_scenes::jpeg(&Scene::new(40 + i as u64, 400, 300), 80, Some(&exif)).unwrap();
        let dir = root.join("photos").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), pad_jpeg(&jpeg, FILE_BYTES)).unwrap();
        if !folders.contains(&dir) {
            folders.push(dir);
        }
    }
    for dir in &folders {
        s.execute("folders.add", &json!({ "path": dir })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(root: &Path, (w, h): (u32, u32)) -> Driver {
    let mut d = new_driver(App::with_picker(Some(session(root)), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// Photo ids by file name.
fn ids(d: &Driver) -> BTreeMap<String, PhotoId> {
    let library = d.app().session().unwrap().library();
    let ids: BTreeMap<String, PhotoId> = library.photos().map(|p| (p.name.clone(), p.id)).collect();
    assert_eq!(ids.len(), PHOTOS.len(), "every photo scanned");
    ids
}

/// The status line's text as drawn, checked against the `text:status` label and its rect.
fn status(d: &Driver) -> String {
    let w = d.widgets().into_iter().find(|w| w.id == "text:status").expect("the status line registers its text");
    assert_eq!(w.label, d.state().status_line, "the label drawn is the text `ui.state` reports");
    let (width, _, _) = d.size();
    assert!(w.rect[2] <= width as f32 - 2.0 * dalica_ui::panels::status::PAD, "the text fits the line: {:?}", w.rect);
    w.label
}

fn select(d: &mut Driver, names: &[&str]) {
    let ids = ids(d);
    let ids: Vec<u64> = names.iter().map(|n| ids[*n].get()).collect();
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
}

/// The done-when: each §6.1 case reads as the spec writes it.
#[test]
fn every_case_reads_as_the_spec_writes_it() {
    let size = LARGE;
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), size);
    let ids = ids(&d);
    let case = |c: &str| format!("{size:?}: {c}");
    assert_eq!(status(&d), "No selection", "{}", case("nothing"));

    // One photo, clicked: name, date taken, pixel size, file size.
    d.click(&format!("thumb:{}", ids["IMG_0361.jpg"].get())).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "IMG_0361.jpg     June 4, 2026 7:42 PM     400 × 300     100.0 KB", "{}", case("one photo"));

    // Several photos: count, date range, size on disk; one day shows that day alone.
    select(&mut d, &["IMG_0361.jpg", "IMG_0362.jpg", "IMG_0363.jpg"]);
    assert_eq!(status(&d), "3 pictures     June 4, 2026 to June 6, 2026     300.0 KB on disk", "{}", case("several"));
    select(&mut d, &["IMG_0363.jpg", "IMG_0364.jpg"]);
    assert_eq!(status(&d), "2 pictures     June 6, 2026     200.0 KB on disk", "{}", case("one day"));

    // A section selected as a unit from the tree: the tray's token, the section's pictures.
    let lisbon = d.app().session().unwrap().library().photo(ids["IMG_0361.jpg"]).unwrap().folder;
    d.click(&format!("tree:folder:{}", lisbon.get())).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.widgets().iter().any(|w| w.id == "tray:token"), "{}", case("the section is the tray's token"));
    assert_eq!(status(&d), "4 pictures     June 4, 2026 to June 6, 2026     400.0 KB on disk", "{}", case("a section"));

    // The tray's contents, not just the selection: two held photos and two selected elsewhere.
    d.run("select.none", &json!({})).unwrap();
    let held = [ids["IMG_0361.jpg"].get(), ids["IMG_0362.jpg"].get()];
    d.run("tray.hold", &json!({ "ids": held })).unwrap();
    select(&mut d, &["IMG_0401.jpg", "IMG_0402.jpg"]);
    assert_eq!(status(&d), "4 pictures     June 4, 2026 to June 10, 2026     400.0 KB on disk", "{}", case("held + selected"));
    d.run("select.none", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "2 pictures     June 4, 2026 to June 5, 2026     200.0 KB on disk", "{}", case("held only"));
    d.run("tray.clear", &json!({ "confirm": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "No selection", "{}", case("the tray emptied"));
}

/// A frame grows the size the line gives (M12.27): a Border of 5 % of the short side on every side
/// makes the 400 × 300 photo 430 × 330, turned by a rotation after it; a crop is not counted, as
/// before frames.
#[test]
fn a_framed_photo_reads_at_its_grown_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    let id = ids(&d)["IMG_0361.jpg"];
    select(&mut d, &["IMG_0361.jpg"]);
    let line = |size: &str| format!("IMG_0361.jpg     June 4, 2026 7:42 PM     {size}     100.0 KB");
    assert_eq!(status(&d), line("400 × 300"));
    for (op, size) in [("border(#000000,50,#ffffff,0,0,0)", "430 × 330"), ("rotate(90)", "330 × 430"), ("crop(0.25,0,0.75,1)", "330 × 430")] {
        d.app_mut().session_mut().unwrap().execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(status(&d), line(size), "after {op}");
    }
}

/// The done-when: a long file name is shown whole where it fits (1400) and middle-elided where it
/// does not (900), the other fields kept whole.
#[test]
fn a_long_name_loses_its_middle_only_when_the_line_is_too_narrow() {
    let rest = "     June 11, 2026 2:00 PM     400 × 300     100.0 KB";
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    select(&mut d, &[LONG]);
    for size in SIZES {
        d.resize(size.0, size.1);
        d.settle(FRAMES).unwrap();
        let text = status(&d);
        if size.0 == 1400 {
            assert_eq!(text, format!("{LONG}{rest}"), "1400: the whole name fits");
        } else {
            let name = text.strip_suffix(rest).unwrap_or_else(|| panic!("900: the other fields stay whole: {text}"));
            let (head, tail) = name.split_once('…').unwrap_or_else(|| panic!("900: the name is cut in its middle: {name}"));
            assert!(LONG.starts_with(head) && LONG.ends_with(tail), "900: the name's two ends: {name}");
            assert!(head.len() >= 10 && tail.ends_with(".jpg"), "900: as much as fits is kept: {name}");
            assert!(head.chars().count().abs_diff(tail.chars().count()) <= 1, "900: cut in the middle: {name}");
        }
    }
}

/// The done-when's elision order, on the edit view's line (a tested format for M7): the folder
/// prefix goes first, then the name's middle; "(12 of 48)" stays.
#[test]
fn the_edit_view_line_drops_the_prefix_before_eliding_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let d = driver(dir.path(), SMALL);
    let library = d.app().session().unwrap().library();
    let photo = library.photo(ids(&d)["IMG_0361.jpg"]).unwrap();
    let line = status_text::edit("Lisbon Weekend", photo, 12, 48);
    let whole = "Lisbon Weekend › IMG_0361.jpg     June 4, 2026 7:42 PM     400 × 300     100.0 KB     (12 of 48)";
    assert_eq!(line.text(), whole);
    // Measured one unit per character.
    let w = |s: &str| s.chars().count() as f32;
    assert_eq!(line.fit(w(whole), w), whole);
    let bare = "IMG_0361.jpg     June 4, 2026 7:42 PM     400 × 300     100.0 KB     (12 of 48)";
    assert_eq!(line.fit(w(whole) - 1.0, w), bare, "the prefix first");
    assert_eq!(line.fit(w(bare) - 4.0, w), "IMG_…jpg     June 4, 2026 7:42 PM     400 × 300     100.0 KB     (12 of 48)", "then the name");
    assert_eq!(Line::plain("No selection").fit(1e9, w), "No selection");
}

/// The done-when: a transient message (here why an action was refused) replaces the text for 4 s
/// of virtual time, then the tray's text is back; a successful action clears it at once.
#[test]
fn a_transient_message_shows_for_four_seconds() {
    let size = LARGE;
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), size);
    select(&mut d, &["IMG_0361.jpg", "IMG_0362.jpg"]);
    let text = "2 pictures     June 4, 2026 to June 5, 2026     200.0 KB on disk";
    assert_eq!(status(&d), text);
    assert!(d.app_mut().run("photo.star.toggle", &json!({ "ids": [999_999] })).is_err());
    d.step();
    let message = status(&d);
    assert!(!message.is_empty() && message != text, "{size:?}: the reason is shown: {message}");
    d.wait(3_900).unwrap();
    assert_eq!(status(&d), message, "{size:?}: still shown at 3.9 s");
    d.wait(200).unwrap();
    assert_eq!(status(&d), text, "{size:?}: gone after 4 s");

    assert!(d.app_mut().run("photo.star.toggle", &json!({ "ids": [999_999] })).is_err());
    d.step();
    assert_ne!(status(&d), text);
    d.run("select.set", &json!({ "ids": [ids(&d)["IMG_0361.jpg"].get()] })).unwrap();
    d.step();
    assert!(status(&d).starts_with("IMG_0361.jpg     "), "{size:?}: an action that succeeds clears the message");
}

/// Open once the test lets the held items of a job (or the held files of a scan) go.
type Gate = Arc<(Mutex<bool>, Condvar)>;

fn new_gate() -> Gate {
    Arc::new((Mutex::new(false), Condvar::new()))
}

fn open(gate: &Gate) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

/// Waits (at most 60 s) until `gate` is open.
fn pass(gate: &Gate) {
    let (lock, cv) = &**gate;
    let _ = cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(60), |open| !*open).unwrap();
}

/// One item of the test export job.
struct Item(std::path::PathBuf);

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.0
    }
}

/// Starts an export job of 4 items that writes nothing: the first is done at once, the others
/// wait for `gate`; returns once the first is done, so the job reads "1 of 4".
fn held_export(d: &mut Driver, gate: &Gate) -> JobId {
    let held = Arc::clone(gate);
    let items: Vec<Item> = (1..=4).map(|i| Item(format!("IMG_{i:04}.jpg").into())).collect();
    let job = FileJob::new(JobKind::Export, "Exporting", items, move |item: Item| {
        if item.0 != Path::new("IMG_0001.jpg") {
            pass(&held);
        }
        Ok(Done { changes: Vec::new(), value: () })
    });
    let id = d.app_mut().session_mut().unwrap().start_file_job(job);
    d.step_until(|app| app.session().unwrap().job(id).unwrap().progress.done == 1, FRAMES).expect("one item done");
    d.wait(100).unwrap();
    id
}

/// The done-when: while the export runs the line reads "Exporting 1 of 4" with nothing selected,
/// ends with it with a photo selected, in the library and in the edit view, and loses it once the
/// job ends.
#[test]
fn an_export_shows_its_progress_until_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    let gate = new_gate();
    let id = held_export(&mut d, &gate);
    assert_eq!(status(&d), "Exporting 1 of 4", "in place of No selection");

    select(&mut d, &["IMG_0361.jpg"]);
    let one = "IMG_0361.jpg     June 4, 2026 7:42 PM     400 × 300     100.0 KB";
    assert_eq!(status(&d), format!("{one}     Exporting 1 of 4"), "the last field");

    let photo = ids(&d)["IMG_0361.jpg"].get();
    d.run("view.edit", &json!({ "id": photo })).unwrap();
    d.wait(100).unwrap();
    let text = status(&d);
    assert!(text.starts_with("Lisbon Weekend › IMG_0361.jpg     ") && text.ends_with("(1 of 4)     Exporting 1 of 4"), "the edit view: {text}");

    open(&gate);
    d.step_until(|app| app.session().unwrap().job(id).is_some_and(|e| e.state.is_finished()), FRAMES).expect("the job ends");
    d.wait(100).unwrap();
    let text = status(&d);
    assert!(text.starts_with("Lisbon Weekend › IMG_0361.jpg     ") && text.ends_with("(1 of 4)"), "gone in the edit view: {text}");
    d.run("view.library", &json!({})).unwrap();
    d.run("select.none", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "No selection", "gone in the library");
}

/// The tag of the scan-and-export test.
const TAG: &str = "lisbon-weekend-in-june-with-the-trams-and-the-sea-and-the-hills";

/// The done-when: a scan and an export together both show where there is room (1400), and at 900
/// the scan's field is dropped first, the export's kept.
#[test]
fn with_a_scan_too_the_scan_field_goes_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    // A folder still to add, whose scan waits for its own gate on every file.
    let porto = dir.path().join("photos").join("Porto");
    std::fs::create_dir_all(&porto).unwrap();
    let jpeg = dalica_scenes::jpeg(&Scene::new(7, 96, 64), 80, None).unwrap();
    std::fs::write(porto.join("IMG_0901.jpg"), jpeg).unwrap();
    let files = new_gate();
    let (held, inner) = (Arc::clone(&files), dalica_scan::default_reader());
    let reader: dalica_scan::HeaderReader = Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
        pass(&held);
        inner(path, limits)
    });
    d.app_mut().session_mut().unwrap().set_scan_reader(Some(reader));

    // A tag long enough that at 900 the selection's fields and the export's fit and the scan's
    // does not (about 835 and 990 points of the 884 there are; all 990 fit at 1400).
    let picked: Vec<u64> = ["IMG_0361.jpg", "IMG_0362.jpg", "IMG_0363.jpg"].iter().map(|n| ids(&d)[*n].get()).collect();
    d.run("photo.tags.add", &json!({ "ids": picked, "tags": TAG })).unwrap();

    let items = new_gate();
    let id = held_export(&mut d, &items);
    // The scan starts after the export (one running when a file job starts is put off).
    d.run("folders.add", &json!({ "path": porto })).unwrap();
    d.step_until(|app| app.session().unwrap().scan_progress().running, FRAMES).expect("the scan runs");
    // Selected without settling: the scan keeps the library busy.
    d.run("select.set", &json!({ "ids": picked })).unwrap();
    d.wait(100).unwrap();
    let several = format!("3 pictures     June 4, 2026 to June 6, 2026     300.0 KB on disk     Tags: {TAG} (3)");
    let export = format!("{several}     Exporting 1 of 4");
    assert_eq!(status(&d), format!("{export}     Scanning: 0 pictures found"), "both at 1400, the export's first");

    d.resize(SMALL.0, SMALL.1);
    d.wait(100).unwrap();
    assert_eq!(status(&d), export, "900: the scan's field dropped first");

    open(&items);
    open(&files);
    d.step_until(|app| app.session().unwrap().job(id).is_some_and(|e| e.state.is_finished()), FRAMES).expect("the export ends");
    d.step_until(|app| !app.session().unwrap().busy(), FRAMES).expect("the scan ends");
    d.wait(100).unwrap();
    assert_eq!(status(&d), several, "both gone");
}

/// The Collage room in front: the line describes the room's selection, a picture selected on the
/// canvas reading as that photo and two as two pictures, not "No selection" for the library's
/// tray, which the room's selection does not touch (M14.R's screenshots); back on the Library tab
/// it describes the tray again.
#[test]
fn the_collage_room_describes_its_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    let all = ids(&d);
    let three: Vec<u64> = ["IMG_0361.jpg", "IMG_0362.jpg", "IMG_0363.jpg"].iter().map(|n| all[*n].get()).collect();
    assert_eq!(d.run("collage.open", &json!({ "ids": three })).unwrap(), json!({ "room": "collage" }));
    d.run("collageCanvas.selectNone", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "No selection", "nothing selected in the room");
    let c = d.app_mut().session_mut().unwrap().execute("collage.inspect", &json!({})).unwrap();
    let placed: Vec<u64> = c["placements"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let names: BTreeMap<u64, String> = ids(&d).into_iter().map(|(n, id)| (id.get(), n)).collect();
    d.run("collageCanvas.select", &json!({ "items": [0] })).unwrap();
    d.settle(FRAMES).unwrap();
    crate::common::shot(&d, "status-collage-one-1400x900");
    let text = status(&d);
    assert!(text.starts_with(&format!("{}     June ", names[&placed[0]])) && text.ends_with("400 × 300     100.0 KB"), "one picture: {text}");
    d.run("collageCanvas.select", &json!({ "items": [0, 1] })).unwrap();
    d.settle(FRAMES).unwrap();
    let text = status(&d);
    assert!(text.starts_with("2 pictures     ") && text.ends_with("200.0 KB on disk"), "two pictures: {text}");
    d.run("room.select", &json!({ "room": "library" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&d), "No selection", "the library's tray, empty");
}
