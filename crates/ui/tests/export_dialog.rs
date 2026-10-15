//! The Export dialog (M8.20, `creating.md` §2), driven headlessly by widget id on a library of two
//! folders ("Lisbon Weekend" of three photos, "Porto" of two):
//!
//! - every entry point opens it with the right folder name: File ▸ Export Pictures to Folder…,
//!   Ctrl+Shift+S and the bottom bar's Export with the tray ("Lisbon Weekend" for photos of one
//!   folder, "Export <date>" for a mixed tray), a section header's Export (its folder's name);
//! - the size field and the slider stay in step: a drag near a stop snaps to it, a drag between
//!   stops does not, a typed 1700 holds and moves the slider, ↑ goes on to the next stop;
//! - Custom enables its slider and the dropdown reads "Custom (75)";
//! - a size of 0, 99999 or letters disables Export, as does an empty location or folder name;
//! - Export starts an export job with the dialog's values (the files are written, numbered and
//!   resized) and they are the next defaults; Cancel and Esc change nothing;
//! - every widget has an id, and the dialog matches its fingerprints at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/dialog-export-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones, renders
//!   go to `target/test-output/ui/`, and to `$DALICA_SHOTS` when set).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, check_region, new_driver, shot, try_widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{JobKind, Session};
use dalica_ui::dialogs::export::{
    BROWSE_ID, CUSTOM_ID, FOLDER_NAME_ID, LOCATION_ID, NUMBERS_ID, ORIGINAL_ID, QUALITY_ID, RESIZE_ID, SIZE_FIELD_ID, SIZE_ID, WATERMARK_ID,
    WATERMARK_TEXT_ID, quality_item_id,
};
use dalica_ui::headless::{Drag, Target};
use dalica_ui::state::{ExportDraft, Quality, Section};
use dalica_ui::{App, Driver, DriverError, Picker, ScriptedPicker};
use dalica_ui_kit::{Tokens, WidgetInfo};
use egui::pos2;
use serde_json::{Value, json};

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const PATIENCE: Duration = Duration::from_secs(300);
const EXPORT: &str = "button:dialog.export";
const DIALOG: &str = "dialog:export";
/// How long the test's folder picker takes to answer, as a person would: longer than a settled
/// frame loop, so a test that does not wait for the answer fails on any machine.
const THINKING: Duration = Duration::from_millis(150);

fn put_jpeg(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 300, 200), 85, None).unwrap();
    std::fs::write(path, jpeg).unwrap();
}

struct Fx {
    dir: tempfile::TempDir,
    lisbon: FolderId,
    /// Lisbon Weekend's three photos, then Porto's two, by name.
    ids: Vec<PhotoId>,
    d: Driver,
}

impl Fx {
    /// The export location every test types: under the temp dir, outside every watched folder.
    fn out(&self) -> PathBuf {
        self.dir.path().join("Out")
    }
}

fn fixture(size: (u32, u32)) -> Fx {
    fixture_with(size, None)
}

/// The library, in a window whose file dialogs are `picker`'s (none: no file dialog).
fn fixture_with((w, h): (u32, u32), picker: Option<Arc<dyn Picker>>) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("Photos");
    for i in 0..3 {
        put_jpeg(&photos.join("Lisbon Weekend").join(format!("IMG_{i:04}.jpg")), i);
    }
    for i in 0..2 {
        put_jpeg(&photos.join("Porto").join(format!("PRT_{i:04}.jpg")), 10 + i);
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    // The user's Pictures folder inside the temp dir, so the default location never names the
    // machine's own.
    s.set_pictures_dir(Some(dir.path().join("Pictures")));
    s.wait_idle();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    let lisbon = s.library().folder_by_path(&photos.join("Lisbon Weekend")).unwrap();
    let mut named: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    // IMG_* (Lisbon Weekend) sort before PRT_* (Porto).
    let ids = named.into_iter().map(|(_, id)| id).collect();
    let app = match picker {
        Some(p) => App::with_picker(Some(s), p),
        None => App::new(Some(s)),
    };
    let mut d = new_driver(app, w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, ids, d }
}

/// A [`ScriptedPicker`] that answers after [`THINKING`].
struct Thinking(ScriptedPicker);

impl Picker for Thinking {
    fn pick_folder(&self) -> Option<PathBuf> {
        std::thread::sleep(THINKING);
        self.0.pick_folder()
    }
}

/// Clicks Browse… and waits for the event the test checks: the picker's answer, run by a frame
/// (the app's folder request is gone). `settle` alone does not wait for a folder request, so it
/// may return while the dialog thread is still picking.
fn browse(d: &mut Driver) {
    d.click(BROWSE_ID).unwrap();
    assert!(d.app().folder_request().is_some(), "Browse… opened the picker");
    let deadline = Instant::now() + PATIENCE;
    while d.app().folder_request().is_some() {
        assert!(Instant::now() < deadline, "the picker never answered");
        std::thread::sleep(Duration::from_millis(1));
        d.step();
    }
    d.settle(FRAMES).unwrap();
}

fn enabled(d: &Driver, id: &str) -> bool {
    try_widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).enabled
}

fn draft(d: &Driver) -> ExportDraft {
    d.state().export.clone().expect("the Export dialog's draft")
}

fn select(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
}

fn cancel(d: &mut Driver) {
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(d, DIALOG).is_none() && d.state().export.is_none(), "Cancel closes it");
}

/// Replaces the text of a field: click, select all, type, Tab (Return would choose Export, the
/// dialog's default button).
fn retype(d: &mut Driver, id: &str, text: &str) {
    d.click(id).unwrap();
    d.key("Ctrl+A").unwrap();
    if text.is_empty() {
        d.key("Backspace").unwrap();
    } else {
        d.type_text(text).unwrap();
    }
    d.key("Tab").unwrap();
    d.settle(FRAMES).unwrap();
}

/// The point on the size slider's track at `value` pixels.
fn at_size(d: &Driver, value: f32) -> egui::Pos2 {
    let r = try_widget(d, SIZE_ID).unwrap().rect;
    let ring = Tokens::default().metrics.slider_ring;
    let (x0, x1) = (r[0] + ring + 1.0, r[0] + r[2] - ring - 1.0);
    pos2(x0 + (value - 100.0) / 3900.0 * (x1 - x0), r[1] + r[3] / 2.0)
}

fn drag_size(d: &mut Driver, value: f32) {
    let to = at_size(d, value);
    d.drag(SIZE_ID, Target::Point(to), &Drag::default()).unwrap();
    d.settle(FRAMES).unwrap();
}

fn defaults(d: &mut Driver, params: Value) -> Value {
    d.run("export.defaults", &params).unwrap()
}

/// Each entry point opens the dialog for what it exports, with that folder's name (or the
/// fallback "Export <date>" for photos of two folders).
#[test]
fn each_entry_point_opens_it_with_the_right_folder_name() {
    let mut fx = fixture(SIZES[0]);
    let ids = fx.ids.clone();
    // Nothing in the tray: the bottom bar's Export and the menu item wait.
    assert!(!enabled(&fx.d, "action:export"));
    assert!(matches!(fx.d.click("action:export"), Err(DriverError::Disabled(_))));

    // File ▸ Export Pictures to Folder… with two Lisbon photos in the tray.
    select(&mut fx.d, &ids[..2]);
    fx.d.click("menu:File").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("menuitem:output.export").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DIALOG).is_some(), "the menu item opens it");
    let e = draft(&fx.d);
    assert_eq!((e.ids.as_slice(), e.section, e.folder_name.as_str()), (&ids[..2], None, "Lisbon Weekend"));
    let pictures = fx.dir.path().join("Pictures").join("Dalica").join("Exports");
    assert_eq!(PathBuf::from(&e.location), pictures, "the default location");
    assert_eq!((e.original_size, e.resize.as_str(), e.quality, e.numbers, e.watermark), (false, "1600", Quality::Automatic, false, false));
    cancel(&mut fx.d);

    // Ctrl+Shift+S with a mixed tray: the fallback name.
    select(&mut fx.d, &[ids[0], ids[4]]);
    fx.d.key("Ctrl+Shift+S").unwrap();
    fx.d.settle(FRAMES).unwrap();
    let e = draft(&fx.d);
    assert_eq!(e.ids, [ids[0], ids[4]]);
    assert!(e.folder_name.starts_with("Export 20") && e.folder_name.len() == "Export 2026-06-04".len(), "{:?}", e.folder_name);
    // Esc closes it like Cancel.
    fx.d.key("Esc").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DIALOG).is_none() && fx.d.state().export.is_none(), "Esc closes it");

    // The bottom bar's Export is live with a tray, and opens the dialog for it.
    select(&mut fx.d, &ids[3..]);
    assert!(enabled(&fx.d, "action:export"));
    fx.d.click("action:export").unwrap();
    fx.d.settle(FRAMES).unwrap();
    let e = draft(&fx.d);
    assert_eq!((e.ids.as_slice(), e.folder_name.as_str()), (&ids[3..], "Porto"));
    cancel(&mut fx.d);

    // A section header's Export, nothing selected in it: the whole section.
    fx.d.run("select.set", &json!({ "ids": [] })).unwrap();
    fx.d.run("section.scrollTo", &json!({ "section": Section::Folder(fx.lisbon) })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let header = format!("section:{}/export", Section::Folder(fx.lisbon));
    fx.d.click(&header).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let e = draft(&fx.d);
    assert_eq!((e.ids.len(), e.section, e.folder_name.as_str()), (0, Some(Section::Folder(fx.lisbon)), "Lisbon Weekend"));
    cancel(&mut fx.d);
}

/// The size field and the slider stay in step; a drag snaps at the stops, a typed size never does;
/// Custom enables its slider and shows its value; bad sizes disable Export.
#[test]
fn field_and_slider_stay_in_step_and_bad_values_disable_export() {
    let mut fx = fixture(SIZES[0]);
    let ids = fx.ids.clone();
    select(&mut fx.d, &ids[..3]);
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(enabled(&fx.d, EXPORT), "the defaults are ready to export");

    // A drag near 1280 snaps to it; one between 2048 and 3840 does not.
    drag_size(&mut fx.d, 1300.0);
    assert_eq!(draft(&fx.d).resize, "1280", "snapped to the stop");
    drag_size(&mut fx.d, 2900.0);
    let between: u32 = draft(&fx.d).resize.parse().unwrap();
    assert!((2700..=3100).contains(&between) && between != 2048 && between != 3840, "{between}");

    // Typing 1700 holds 1700, and the slider stands there: ↑ goes on to the next stop, 2048.
    retype(&mut fx.d, SIZE_FIELD_ID, "1700");
    assert_eq!(draft(&fx.d).resize, "1700");
    fx.d.hover(SIZE_ID, 50).unwrap();
    fx.d.key("Up").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(draft(&fx.d).resize, "2048", "the slider stood at 1700");

    // Sizes Export does not take disable it; a good one enables it again.
    for bad in ["0", "99999", "abc", "99"] {
        retype(&mut fx.d, SIZE_FIELD_ID, bad);
        assert_eq!(draft(&fx.d).resize, bad, "kept as typed");
        assert!(!enabled(&fx.d, EXPORT), "{bad:?} disables Export");
        assert!(matches!(fx.d.click(EXPORT), Err(DriverError::Disabled(_))));
    }
    // With the original size the field does not matter (and is disabled).
    fx.d.click(ORIGINAL_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(draft(&fx.d).original_size && enabled(&fx.d, EXPORT) && !enabled(&fx.d, SIZE_FIELD_ID) && !enabled(&fx.d, SIZE_ID));
    fx.d.click(RESIZE_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(!draft(&fx.d).original_size && !enabled(&fx.d, EXPORT));
    retype(&mut fx.d, SIZE_FIELD_ID, "640");
    assert!(enabled(&fx.d, EXPORT));

    // An empty folder name or location disables it too.
    retype(&mut fx.d, FOLDER_NAME_ID, "");
    assert!(!enabled(&fx.d, EXPORT));
    retype(&mut fx.d, FOLDER_NAME_ID, "Trip");
    retype(&mut fx.d, LOCATION_ID, "");
    assert!(!enabled(&fx.d, EXPORT));
    let out = fx.out();
    retype(&mut fx.d, LOCATION_ID, out.to_str().unwrap());
    assert!(enabled(&fx.d, EXPORT));

    // Custom: its slider is enabled and the dropdown reads "Custom (75)".
    assert!(!enabled(&fx.d, CUSTOM_ID), "the custom slider waits for Custom");
    fx.d.click(QUALITY_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let items: Vec<WidgetInfo> = Quality::ALL.iter().map(|q| try_widget(&fx.d, &quality_item_id(*q)).unwrap()).collect();
    assert!(items[0].active && items.iter().skip(1).all(|i| !i.active), "Automatic is checked");
    fx.d.click(&quality_item_id(Quality::Custom)).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(draft(&fx.d).quality, Quality::Custom);
    assert!(enabled(&fx.d, CUSTOM_ID));
    assert_eq!(try_widget(&fx.d, QUALITY_ID).unwrap().label, "Custom (75)");
    fx.d.drag(CUSTOM_ID, Target::Delta(egui::vec2(-400.0, 0.0)), &Drag::default()).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!((draft(&fx.d).custom, try_widget(&fx.d, QUALITY_ID).unwrap().label.as_str()), (1, "Custom (1)"));

    // The watermark's text waits for its checkbox; on, an empty text disables Export.
    assert!(!enabled(&fx.d, WATERMARK_TEXT_ID));
    fx.d.click(WATERMARK_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(enabled(&fx.d, WATERMARK_TEXT_ID) && !enabled(&fx.d, EXPORT));
    retype(&mut fx.d, WATERMARK_TEXT_ID, "© Ana");
    assert!(enabled(&fx.d, EXPORT));

    // The videos row is disabled until videos are exported (M15).
    assert!(!enabled(&fx.d, "choice:dialog.videos/firstFrame") && !enabled(&fx.d, "choice:dialog.videos/fullVideo"));
    assert!(try_widget(&fx.d, BROWSE_ID).is_some());
}

/// Export runs `export.run` with the dialog's values: the job writes the files, numbered and
/// resized, and the values are the next defaults. Cancel before it changed nothing.
#[test]
fn export_runs_the_job_with_the_dialogs_values_and_cancel_changes_nothing() {
    let mut fx = fixture(SIZES[0]);
    let ids = fx.ids.clone();
    let out = fx.out();
    select(&mut fx.d, &ids[..3]);
    let before = defaults(&mut fx.d, json!({ "ids": &ids[..3] }));

    // Cancel after changing every control: nothing is exported or remembered.
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click(NUMBERS_ID).unwrap();
    fx.d.click(WATERMARK_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    retype(&mut fx.d, WATERMARK_TEXT_ID, "nope");
    retype(&mut fx.d, LOCATION_ID, out.to_str().unwrap());
    cancel(&mut fx.d);
    assert_eq!(defaults(&mut fx.d, json!({ "ids": &ids[..3] })), before, "Cancel remembered nothing");
    assert!(!out.exists(), "Cancel exported nothing");
    assert!(!fx.d.app().session().unwrap().jobs().iter().any(|j| j.kind == JobKind::Export), "no export started");

    // Export with numbers, 160 pixels, Custom 60 and a watermark, into the typed location.
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    retype(&mut fx.d, LOCATION_ID, out.to_str().unwrap());
    retype(&mut fx.d, FOLDER_NAME_ID, "Lisbon Picks");
    fx.d.click(NUMBERS_ID).unwrap();
    retype(&mut fx.d, SIZE_FIELD_ID, "160");
    fx.d.click(QUALITY_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click(&quality_item_id(Quality::Custom)).unwrap();
    // The custom value as a script sets it (`ui.set`); the slider's drag is checked above.
    fx.d.app_mut().state_mut().export.as_mut().unwrap().custom = 60;
    fx.d.click(WATERMARK_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    retype(&mut fx.d, WATERMARK_TEXT_ID, "© Ana Costa");
    fx.d.click(EXPORT).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DIALOG).is_none() && fx.d.state().export.is_none(), "Export closes the dialog");

    // One export job (its label reads "Exported 3 pictures" once it has ended).
    let exports: Vec<_> = fx.d.app().session().unwrap().jobs().into_iter().filter(|j| j.kind == JobKind::Export).collect();
    assert_eq!(exports.len(), 1, "one export job: {exports:?}");
    let job = exports[0].job;
    let entry = fx.d.app_mut().session_mut().unwrap().wait_job(job, PATIENCE).unwrap();
    assert!(entry.state.is_finished(), "{entry:?}");
    fx.d.settle(FRAMES).unwrap();
    let result = entry.result.unwrap();
    assert_eq!((result["written"].as_array().map(Vec::len), result["name"].as_str()), (Some(3), Some("Lisbon Picks")), "{result}");
    let folder = out.join("Lisbon Picks");
    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.retain(|n| n.ends_with(".jpg"));
    names.sort();
    assert_eq!(names, ["001-IMG_0000.jpg", "002-IMG_0001.jpg", "003-IMG_0002.jpg"], "numbered in tray order");
    let img = dalica_image::decode(&std::fs::read(folder.join(&names[0])).unwrap(), &dalica_image::Limits::default()).unwrap();
    assert_eq!((img.width, img.height), (160, 107), "resized to 160 pixels on the long side");

    // The values used are the next defaults, in the engine and in the dialog.
    let next = defaults(&mut fx.d, json!({ "ids": &ids[..3] }));
    assert_eq!(
        (
            &next["location"],
            &next["numbers"],
            &next["resize"],
            &next["size"],
            &next["quality"],
            &next["customQuality"],
            &next["watermark"],
            &next["watermarkText"]
        ),
        (&json!(out), &json!(true), &json!(true), &json!(160), &json!("custom"), &json!(60), &json!(true), &json!("© Ana Costa"))
    );
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let e = draft(&fx.d);
    assert_eq!(
        (PathBuf::from(&e.location), e.numbers, e.resize.as_str(), e.quality, e.custom, e.watermark, e.watermark_text.as_str()),
        (out.clone(), true, "160", Quality::Custom, 60, true, "© Ana Costa")
    );
    assert_eq!(try_widget(&fx.d, QUALITY_ID).unwrap().label, "Custom (60)");
    // Turning the watermark off is remembered too (`watermark: false`, never "the stored one").
    fx.d.click(WATERMARK_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click(EXPORT).unwrap();
    fx.d.settle_for(FRAMES, true);
    assert_eq!(defaults(&mut fx.d, json!({ "ids": &ids[..3] }))["watermark"], json!(false));
}

/// Browse… opens the folder picker in the location typed; the folder chosen becomes the location,
/// a cancel changes nothing. Without a picker it is disabled.
#[test]
fn browse_picks_the_export_location() {
    let mut fx = fixture(SIZES[0]);
    let ids = fx.ids.clone();
    select(&mut fx.d, &ids[..1]);
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(!enabled(&fx.d, BROWSE_ID), "no file dialog here");

    let picker = ScriptedPicker::new();
    let mut fx = fixture_with(SIZES[0], Some(Arc::new(Thinking(picker.clone()))));
    select(&mut fx.d, &ids[..1]);
    fx.d.run("output.export", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let before = draft(&fx.d);
    assert!(enabled(&fx.d, BROWSE_ID));
    // Cancelled (nothing queued): nothing changes.
    browse(&mut fx.d);
    assert_eq!(draft(&fx.d), before);
    // A folder chosen becomes the location; the dialog stays open and no folder is watched.
    let roots = fx.d.app().session().unwrap().roots().to_vec();
    let chosen = fx.out().join("Exports");
    picker.push_folder(&chosen);
    browse(&mut fx.d);
    assert_eq!(PathBuf::from(draft(&fx.d).location), chosen);
    assert!(try_widget(&fx.d, DIALOG).is_some());
    assert_eq!(fx.d.app().session().unwrap().roots(), roots.as_slice(), "no watched folder added");
}

/// Every widget has an id with the dialog open, and the dialog (three Lisbon photos, a location,
/// the watermark on) matches its fingerprints at both window sizes.
#[test]
fn the_dialog_matches_its_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let mut fx = fixture_with((w, h), Some(Arc::new(ScriptedPicker::new())));
        let ids = fx.ids.clone();
        select(&mut fx.d, &ids[..3]);
        fx.d.run("output.export", &json!({})).unwrap();
        let e = fx.d.app_mut().state_mut().export.as_mut().unwrap();
        e.location = "/home/ana/Pictures/Dalica/Exports".into();
        e.watermark = true;
        e.watermark_text = "© Ana Costa 2026".into();
        fx.d.settle(FRAMES).unwrap();
        check_region(&fx.d, DIALOG, &format!("dialog-export-{w}x{h}"));
        // The quality menu open, for the record (no fingerprint: the window shot).
        fx.d.click(QUALITY_ID).unwrap();
        fx.d.settle(FRAMES).unwrap();
        shot(&fx.d, &format!("dialog-export-quality-{w}x{h}"));
    }
}
