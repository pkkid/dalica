//! Options ▸ File Types and Options ▸ Export (`menus-and-shortcuts.md` §5, `design-system.md`
//! §11.3), headless and by automation id, on a temp library of one folder (a JPEG and two PNGs, one
//! of them starred):
//!
//! - PNG turned off and applied hides the PNGs (one rescan, "Rescanning your folders…" while it
//!   runs); turned on again it shows them with their stars;
//! - the HEIC/HEIF row is enabled exactly when the build reads HEIC, else disabled with its reason;
//! - a size applied on Export is the next Export dialog's default; Custom enables its slider;
//! - Browse… with the scripted folder picker fills the location, and a cancel changes nothing;
//! - both tabs match their fingerprints at 1400 × 900 and fit 900 × 600.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_sizes, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::{JobKind, Session};
use dalica_ui::dialogs::options::{export, file_types};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const DIALOG: &str = "dialog:options";
const APPLY: &str = "button:dialog.apply";
const CANCEL: &str = "button:dialog.cancel";
const PNG: &str = "check:option.fileTypes.png";
const HEIF: &str = "check:option.fileTypes.heif";
const SIZE_FIELD: &str = "slider:option.export.size/value";
const CUSTOM: &str = "slider:option.export.customQuality";
const QUALITY: &str = "choice:option.export.quality";
const LOCATION: &str = "field:option.export.location";
const PATIENCE: Duration = Duration::from_secs(300);

struct Fx {
    dir: tempfile::TempDir,
    picker: ScriptedPicker,
    d: Driver,
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// "Harbour" with a.jpg, b.png (starred) and c.png, in a 1400 × 900 window with a scripted picker.
fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Photos").join("Harbour");
    put(&folder.join("a.jpg"), &dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, 96, 64), 80, None).unwrap());
    for (seed, name) in [(2, "b.png"), (3, "c.png")] {
        put(&folder.join(name), &dalica_scenes::png(&dalica_scenes::Scene::new(seed, 96, 64)).unwrap());
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(dir.path().join("Pictures")));
    s.wait_idle();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let b = s.library().photos().find(|p| p.name == "b.png").unwrap().id;
    s.execute("photo.star.set", &json!({ "ids": [b], "on": true })).unwrap();
    let picker = ScriptedPicker::new();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(picker.clone())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, picker, d }
}

fn open(d: &mut Driver, tab: &str) {
    d.run("app.preferences", &json!({ "tab": tab })).unwrap();
    d.settle(FRAMES).unwrap();
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

fn by_name(d: &Driver) -> BTreeMap<String, PhotoId> {
    d.app().session().unwrap().library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

fn has(d: &Driver, id: &str) -> bool {
    try_widget(d, id).is_some()
}

/// Whether photo `id` shows starred: its copy in Starred Photos, with the star badge, and the star
/// in the library.
fn starred(d: &Driver, id: PhotoId) -> bool {
    let star = d.app().session().unwrap().library().photo(id).is_some_and(|p| p.star);
    star && has(d, &format!("thumb:{id}/in/starred/star"))
}

/// Clicks Apply and steps until the scan it started has ended; returns whether the rescan line
/// showed. In every frame the line shows exactly while that scan runs (the session polls before
/// the dialog draws, so what the jobs say after a frame is what the frame drew).
fn apply_and_rescan(d: &mut Driver) -> bool {
    let scans = |d: &Driver| d.app().session().unwrap().jobs().into_iter().filter(|j| j.kind == JobKind::Scan).collect::<Vec<_>>();
    let before = scans(d).len();
    d.click(APPLY).unwrap();
    let started = scans(d);
    assert_eq!(started.len(), before + 1, "one rescan");
    let job = started.iter().map(|j| j.job).max().unwrap();
    let mut seen = false;
    let deadline = Instant::now() + PATIENCE;
    loop {
        let running = scans(d).iter().any(|j| j.job == job && !j.state.is_finished());
        let line = has(d, file_types::RESCAN_ID);
        assert_eq!(line, running, "the line shows while the rescan runs");
        if line {
            assert_eq!(widget(d, file_types::RESCAN_ID).label, file_types::RESCANNING);
        }
        seen |= line;
        if !running {
            break;
        }
        assert!(Instant::now() < deadline, "the rescan never ended");
        std::thread::sleep(Duration::from_millis(1));
        d.step();
    }
    d.settle(FRAMES).unwrap();
    seen
}

#[test]
fn png_turned_off_hides_the_pngs_and_turned_on_shows_them_with_their_stars() {
    let mut fx = fixture();
    let d = &mut fx.d;
    let ids = by_name(d);
    assert!(starred(d, ids["b.png"]), "b.png is starred");
    open(d, "fileTypes");
    assert!(widget(d, PNG).active, "PNG is on");
    click(d, PNG);
    assert!(widget(d, APPLY).enabled);
    apply_and_rescan(d);
    assert!(d.state().options.as_ref().is_some_and(|o| o.values.is_empty() && o.error.is_none()));
    click(d, CANCEL);
    assert_eq!(by_name(d).keys().collect::<Vec<_>>(), ["a.jpg"]);
    assert!(has(d, &format!("thumb:{}", ids["a.jpg"])));
    assert!(!has(d, &format!("thumb:{}", ids["b.png"])) && !has(d, &format!("thumb:{}", ids["c.png"])), "the PNGs are hidden");
    assert!(!starred(d, ids["b.png"]), "and gone from Starred Photos");
    // Nothing is deleted: the files are where they were.
    let folder = fx.dir.path().join("Photos").join("Harbour");
    assert!(folder.join("b.png").exists() && folder.join("c.png").exists());

    open(d, "fileTypes");
    assert!(!widget(d, PNG).active, "PNG is off");
    click(d, PNG);
    apply_and_rescan(d);
    click(d, CANCEL);
    let ids = by_name(d);
    assert_eq!(ids.keys().collect::<Vec<_>>(), ["a.jpg", "b.png", "c.png"]);
    assert!(has(d, &format!("thumb:{}", ids["b.png"])) && starred(d, ids["b.png"]), "b.png is back with its star");
    assert!(has(d, &format!("thumb:{}", ids["c.png"])) && !starred(d, ids["c.png"]));
}

#[test]
fn the_heic_row_follows_the_build() {
    let mut fx = fixture();
    let d = &mut fx.d;
    open(d, "fileTypes");
    let spec = dalica_engine::options::find("fileTypes.heif").unwrap();
    assert_eq!(spec.available(), dalica_image::Format::Heif.supported(), "the catalogue says what the decoders can");
    assert_eq!(widget(d, HEIF).enabled, spec.available());
    let row = file_types::rows(d.app(), dalica_engine::options::Tab::FileTypes).into_iter().find(|o| o.row.key == "fileTypes.heif").unwrap();
    if spec.available() {
        assert_eq!(row.row.reason, None);
    } else {
        assert_eq!(row.row.reason.as_deref(), Some("HEIC support isn't included in this build"));
    }
}

#[test]
fn a_size_applied_here_is_the_next_export_dialogs_default() {
    let mut fx = fixture();
    let d = &mut fx.d;
    open(d, "export");
    assert!(!widget(d, CUSTOM).enabled, "Custom quality waits for Custom");
    click(d, QUALITY);
    click(d, &format!("{QUALITY}/custom"));
    assert!(widget(d, CUSTOM).enabled);
    click(d, QUALITY);
    click(d, &format!("{QUALITY}/automatic"));
    // Typed, then Tab (Return would choose OK).
    d.click(SIZE_FIELD).unwrap();
    d.key("Ctrl+A").unwrap();
    d.type_text("2048").unwrap();
    d.key("Tab").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().options.as_ref().unwrap().values.get("export.size"), Some(&json!(2048)));
    click(d, APPLY);
    click(d, CANCEL);
    let a = by_name(d)["a.jpg"];
    d.run("select.set", &json!({ "ids": [a] })).unwrap();
    d.run("output.export", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().export.as_ref().unwrap().resize, "2048", "the Export dialog starts from it");
}

/// Clicks Browse… and waits until a frame has run the picker's answer.
fn browse(d: &mut Driver) {
    d.click(export::BROWSE_ID).unwrap();
    let deadline = Instant::now() + PATIENCE;
    while d.app().folder_request().is_some() {
        assert!(Instant::now() < deadline, "the picker never answered");
        std::thread::sleep(Duration::from_millis(1));
        d.step();
    }
    d.settle(FRAMES).unwrap();
}

#[test]
fn browse_fills_the_location_and_a_cancel_changes_nothing() {
    let mut fx = fixture();
    let d = &mut fx.d;
    open(d, "export");
    assert!(widget(d, export::BROWSE_ID).enabled);
    // Cancelled (nothing queued): nothing changes.
    browse(d);
    assert!(d.state().options.as_ref().unwrap().values.is_empty());
    assert!(!widget(d, APPLY).enabled);
    // A folder chosen fills the location, in the draft until Apply.
    let roots = d.app().session().unwrap().roots().to_vec();
    let chosen: PathBuf = fx.dir.path().join("Out").join("Exports");
    fx.picker.push_folder(&chosen);
    browse(d);
    assert_eq!(d.app().session().unwrap().roots(), roots.as_slice(), "no watched folder added");
    shot(d, "dialog-options-export-location-1400x900");
    let text = chosen.to_str().unwrap();
    assert_eq!(d.state().options.as_ref().unwrap().values.get("export.location"), Some(&json!(text)));
    assert!(has(d, DIALOG) && !widget(d, LOCATION).enabled, "the dialog stays open; the location is shown, not typed in");
    click(d, APPLY);
    let v = d.run("options.list", &json!({ "tab": "export" })).unwrap();
    let saved: &Value = &v["options"].as_array().unwrap().iter().find(|o| o["key"] == "export.location").unwrap()["value"];
    assert_eq!(saved, &json!(text));
}

#[test]
fn both_tabs_match_their_fingerprints_and_fit_the_small_window() {
    let mut fx = fixture();
    let d = &mut fx.d;
    open(d, "fileTypes");
    check_sizes(d, DIALOG, "dialog-options-file-types");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    shot(d, "dialog-options-file-types-900x600");
    d.resize(LARGE.0, LARGE.1);
    d.settle(FRAMES).unwrap();
    click(d, "tab:options.export");
    check_sizes(d, DIALOG, "dialog-options-export");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    shot(d, "dialog-options-export-900x600");
}
