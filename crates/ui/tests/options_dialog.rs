//! Options ▸ General and the dialog's OK / Cancel / Apply (`design-system.md` §11.3,
//! `menus-and-shortcuts.md` §5 General), headless and by automation id: Apply's state follows the
//! draft (a change put back disables it again); an applied change survives Cancel, an unapplied one
//! does not; a planted write failure shows its error in the dialog and keeps the draft; Large font
//! previews at once and reverts on Cancel; every option of the tab that can be changed reaches
//! `options.set` under its key; options whose feature is still to come are drawn disabled with
//! their reason; "Save imported pictures in" (its own fingerprint, scrolled to) shows the Pictures folder as its hint, and a folder
//! chosen with its Browse… and applied is stored in `import.folder` and is where the Import room's
//! "Import to:" starts; the tab matches its fingerprint at 1400 × 900 (the Windows and Linux layout; on
//! macOS, which leaves out Use system title bar, its rows are checked instead) and fits 900 × 600.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_sizes, check_small_and_back, new_driver, shot, try_widget, widget};
use dalica_engine::Session;
use dalica_engine::keymap::Platform;
use dalica_ui::dialogs::options::{ERROR_ID, general};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const DIALOG: &str = "dialog:options";
const APPLY: &str = "button:dialog.apply";
const CANCEL: &str = "button:dialog.cancel";
const TOOLTIPS: &str = "check:option.interface.tooltips";
const ANIMATIONS: &str = "check:option.interface.animations";
const FONT: &str = "choice:option.interface.fontSize";
const IMPORT_FOLDER: &str = "field:option.import.folder";
const IMPORT_BROWSE: &str = "button:option.import.folder/browse";
/// The Pictures folder the session knows: the same text on every machine (the fingerprint shows
/// it as the import folder's hint).
const PICTURES: &str = "/home/ana/Pictures";

struct Fx {
    dir: tempfile::TempDir,
    picker: ScriptedPicker,
    d: Driver,
}

/// An empty library in a 1400 × 900 window, Options open on General.
fn open() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(PathBuf::from(PICTURES)));
    let picker = ScriptedPicker::new();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(picker.clone())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    Fx { dir, picker, d }
}

/// Clicks `id` (a Browse…) and steps frames until the picker's answer has been run.
fn browse(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while d.app().folder_request().is_some() {
        assert!(Instant::now() < deadline, "the picker never answered");
        std::thread::sleep(Duration::from_millis(1));
        d.step();
    }
    d.settle(FRAMES).unwrap();
}

fn enabled(d: &Driver, id: &str) -> bool {
    widget(d, id).enabled
}

/// The saved value of option `key`, as `options.list` gives it.
fn saved(d: &mut Driver, key: &str) -> Value {
    let v = d.run("options.list", &json!({ "tab": "general" })).unwrap();
    v["options"].as_array().unwrap().iter().find(|o| o["key"] == key).unwrap_or_else(|| panic!("no option {key}"))["value"].clone()
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Brings widget `id` into the tab's view (the body scrolls when the dialog is shorter than it).
fn reveal(d: &mut Driver, id: &str) {
    for _ in 0..40 {
        let body = widget(d, DIALOG).rect;
        let (top, bottom) = (body[1] + 80.0, body[1] + body[3] - 60.0);
        // A row scrolled out of view is not drawn: further down.
        let dy = match try_widget(d, id).map(|w| w.rect) {
            Some(r) if r[1] >= top && r[1] + r[3] <= bottom => return,
            Some(r) if r[1] < top => 60.0,
            _ => -60.0,
        };
        d.scroll(Some(DIALOG), egui::vec2(0.0, dy)).unwrap();
        d.settle(FRAMES).unwrap();
    }
    panic!("{id} never came into view");
}

#[test]
fn apply_follows_the_draft_and_cancel_keeps_only_what_was_applied() {
    let mut fx = open();
    let d = &mut fx.d;
    assert!(!enabled(d, APPLY), "nothing changed");
    click(d, TOOLTIPS);
    assert!(enabled(d, APPLY), "a change enables Apply");
    assert!(!widget(d, TOOLTIPS).active);
    click(d, TOOLTIPS);
    assert!(!enabled(d, APPLY), "putting the saved value back disables it again");
    assert_eq!(d.state().options.as_ref().unwrap().values.len(), 0);
    // Applied, then cancelled: the applied value stays.
    click(d, TOOLTIPS);
    click(d, APPLY);
    assert!(try_widget(d, DIALOG).is_some(), "Apply keeps the dialog open");
    assert!(!enabled(d, APPLY), "the draft equals the saved values again");
    assert_eq!(saved(d, "interface.tooltips"), json!(false));
    // Changed but not applied, then cancelled: the old value stays.
    click(d, ANIMATIONS);
    click(d, CANCEL);
    assert!(d.state().options.is_none() && try_widget(d, DIALOG).is_none());
    assert_eq!(saved(d, "interface.tooltips"), json!(false), "the applied change survives Cancel");
    assert_eq!(saved(d, "interface.animations"), json!(true), "the unapplied one does not");
    // Show tooltips off: egui waits for ever before a tooltip.
    assert_eq!(d.ctx().global_style().interaction.tooltip_delay, f32::INFINITY);
}

#[test]
fn a_failed_apply_shows_its_error_and_keeps_the_draft() {
    let mut fx = open();
    let file = fx.dir.path().join("library").join(dalica_engine::settings::FILE);
    if !file.exists() {
        std::fs::write(&file, "{}\n").unwrap();
    }
    let mut perms = std::fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&file, perms.clone()).unwrap();
    let d = &mut fx.d;
    click(d, TOOLTIPS);
    click(d, APPLY);
    let o = d.state().options.clone().unwrap();
    let error = o.error.clone().expect("the error is kept in the draft");
    assert!(error.contains("could not be saved"), "{error}");
    assert_eq!(o.values.get("interface.tooltips"), Some(&json!(false)), "the draft is kept");
    assert!(enabled(d, APPLY), "still something to apply");
    assert_eq!(saved(d, "interface.tooltips"), json!(true), "nothing changed");
    assert_eq!(widget(d, ERROR_ID).label, error, "the error is on screen");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&file, perms).unwrap();
}

#[test]
fn large_font_previews_at_once_and_reverts_on_cancel() {
    let mut fx = open();
    let d = &mut fx.d;
    assert_eq!(d.app().tokens().body, 13.0);
    reveal(d, FONT);
    click(d, FONT);
    click(d, &format!("{FONT}/large"));
    let body = d.app().tokens().body;
    assert!((body - 17.0).abs() < 1e-3, "previewed: {body}");
    assert_eq!(dalica_ui_kit::Tokens::get(d.ctx()).body, body, "the context's tokens too");
    assert!(enabled(d, APPLY));
    assert_eq!(saved(d, "interface.fontSize"), json!("small"), "not saved before Apply");
    d.key("Esc").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().options.is_none());
    assert_eq!(d.app().tokens().body, 13.0, "Cancel reverts the preview");
    assert_eq!(saved(d, "interface.fontSize"), json!("small"));
}

#[test]
fn every_option_of_the_tab_reaches_options_set_with_its_key() {
    let mut fx = open();
    let rows = general::rows(fx.d.app_mut());
    let d = &mut fx.d;
    let mut wanted = serde_json::Map::new();
    for row in &rows {
        let id = general::control_id(&row.kind, &row.key);
        reveal(d, &id);
        if let Some(reason) = &row.reason {
            assert!(!enabled(d, &id), "{}: disabled ({reason})", row.key);
            continue;
        }
        let new = match row.kind.as_str() {
            "bool" => {
                click(d, &id);
                json!(!row.saved.as_bool().unwrap())
            }
            // A folder as its Browse…'s picker puts it (Browse… itself: the test below; by now the
            // largest UI scale is previewed, which leaves no room beside the path).
            "folder" => {
                let path = fx.dir.path().join("Imports");
                d.run(general::FOLDER_COMMAND, &json!({ "key": row.key, "path": path })).unwrap();
                d.settle(FRAMES).unwrap();
                json!(path)
            }
            _ => {
                let (value, _) = row.choices.iter().rev().find(|(v, _)| *v != row.saved).unwrap_or_else(|| panic!("{}: no other choice", row.key));
                click(d, &id);
                click(d, &general::item_id(&row.key, value));
                value.clone()
            }
        };
        wanted.insert(row.key.clone(), new);
    }
    assert_eq!(&d.state().options.as_ref().unwrap().values, &wanted, "the draft holds every change by key");
    let changed: Vec<&str> = rows.iter().filter(|r| r.reason.is_none()).map(|r| r.key.as_str()).collect();
    assert!(changed.len() >= 10 && changed.contains(&"interface.uiScale") && changed.contains(&"files.writeDates"), "{changed:?}");
    // Apply from wherever the body is scrolled to.
    click(d, APPLY);
    assert_eq!(d.state().options.as_ref().unwrap().error, None);
    for (key, value) in &wanted {
        assert_eq!(&saved(d, key), value, "{key}");
    }
    // The tab's disabled rows say why.
    for key in ["interface.language"] {
        let row = rows.iter().find(|r| r.key == key).unwrap();
        assert!(row.reason.as_deref().is_some_and(|r| r.starts_with("Arrives with")), "{key}: {:?}", row.reason);
    }
}

/// The option only Windows and Linux show (`menus-and-shortcuts.md` §5 General).
const TITLE_BAR: &str = "check:option.interface.systemTitleBar";

#[test]
fn the_general_tab_matches_its_fingerprint_and_fits_the_small_window() {
    let mut fx = open();
    if Platform::current() == Platform::Mac {
        // The fingerprint is the Windows and Linux layout. macOS leaves out the options of the
        // other platforms (Use system title bar), so there the tab is checked by its rows (every
        // other option drawn, that one never) and by fit, not against a baseline of another layout.
        let rows = general::rows(fx.d.app_mut());
        let mut keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert!(!keys.contains(&"interface.systemTitleBar"), "{keys:?}");
        let listed = fx.d.run("options.list", &json!({ "tab": "general" })).unwrap();
        let mut wanted: Vec<&str> =
            listed["options"].as_array().unwrap().iter().map(|o| o["key"].as_str().unwrap()).filter(|k| *k != "interface.systemTitleBar").collect();
        keys.sort_unstable();
        wanted.sort_unstable();
        assert_eq!(keys, wanted, "every other option of the tab is listed");
        for row in &rows {
            reveal(&mut fx.d, &general::control_id(&row.kind, &row.key));
            assert!(try_widget(&fx.d, TITLE_BAR).is_none(), "{TITLE_BAR} drawn on macOS");
        }
        check_small_and_back(&mut fx.d, DIALOG);
    } else {
        check_sizes(&mut fx.d, DIALOG, "dialog-options-general");
        assert!(try_widget(&fx.d, TITLE_BAR).is_some(), "Windows and Linux show Use system title bar");
        // Scrolled to the Import group: the folder's hint (the Pictures folder) and a live Browse….
        reveal(&mut fx.d, IMPORT_BROWSE);
        fx.d.scroll(Some(DIALOG), egui::vec2(0.0, -2000.0)).unwrap();
        fx.d.settle(FRAMES).unwrap();
        check_region(&fx.d, DIALOG, "dialog-options-general-import-1400x900");
    }
    // For the record (window shots, no fingerprints): the small window, and the Large preview.
    fx.d.resize(SMALL.0, SMALL.1);
    fx.d.settle(FRAMES).unwrap();
    shot(&fx.d, "dialog-options-general-900x600");
    fx.d.resize(LARGE.0, LARGE.1);
    reveal(&mut fx.d, FONT);
    click(&mut fx.d, FONT);
    click(&mut fx.d, &format!("{FONT}/large"));
    shot(&fx.d, "dialog-options-general-large-1400x900");
}

/// A 192 × 128 JPEG of scene `seed`.
fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 192, 128), 85, None).unwrap()
}

/// The last component of `path`, as "Import to:" shows a folder.
fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn the_import_folder_chosen_with_browse_is_stored_and_import_to_starts_there() {
    let mut fx = open();
    let chosen = fx.dir.path().join("Camera Imports");
    let scans = fx.dir.path().join("Scans");
    std::fs::create_dir_all(&scans).unwrap();
    std::fs::write(scans.join("a.jpg"), jpeg(3)).unwrap();
    let d = &mut fx.d;
    // Nothing chosen: the field is empty and shows the Pictures folder as its hint; Browse… works.
    // The path first: its text sits lower in the row than Browse…'s top, so a row the dialog's
    // body clips at its foot (as macOS, without Use system title bar, scrolls it) can show Browse…
    // and not yet the path; once the path shows, Browse… (the whole row's height) does too.
    reveal(d, IMPORT_FOLDER);
    reveal(d, IMPORT_BROWSE);
    assert_eq!(saved(d, "import.folder"), Value::Null);
    assert_eq!(widget(d, IMPORT_FOLDER).label, Path::new(PICTURES).display().to_string(), "the Pictures folder as the hint");
    assert!(enabled(d, IMPORT_BROWSE), "Browse… is live");
    let request = general::browse_request("import.folder", &Value::Null, Some(Path::new(PICTURES)));
    assert_eq!((request.command, request.params_with(&chosen)), (general::FOLDER_COMMAND, json!({ "key": "import.folder", "path": chosen })));
    // Browse… with the picker answering: the folder goes into the draft and Apply stores it.
    fx.picker.push_folder(&chosen);
    browse(d, IMPORT_BROWSE);
    assert_eq!(d.state().options.as_ref().unwrap().values.get("import.folder"), Some(&json!(chosen)));
    assert!(enabled(d, APPLY));
    click(d, APPLY);
    assert_eq!(d.state().options.as_ref().unwrap().error, None);
    assert_eq!(saved(d, "import.folder"), json!(chosen), "stored in import.folder");
    assert_eq!(d.app().session().unwrap().import_folder(), Some(chosen.clone()));
    // A cancelled picker changes nothing.
    browse(d, IMPORT_BROWSE);
    assert!(!enabled(d, APPLY), "a cancel leaves the draft as it was");
    click(d, CANCEL);
    // The Import room on a folder: "Import to:" starts from the folder stored.
    fx.picker.push_folder(&scans);
    d.key("Ctrl+M").unwrap();
    d.settle(FRAMES).unwrap();
    click(d, "choice:import.source");
    click(d, "choice:import.source/folder");
    let deadline = Instant::now() + Duration::from_secs(60);
    while try_widget(d, "choice:import.destination").is_none() {
        assert!(Instant::now() < deadline, "the destination strip never showed");
        std::thread::sleep(Duration::from_millis(2));
        d.step();
    }
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(d, "choice:import.destination").label, name(&chosen));
}

#[test]
fn the_duplicates_checkbox_stores_import_detect_duplicates() {
    let mut fx = open();
    let d = &mut fx.d;
    let id = "check:option.import.detectDuplicates";
    reveal(d, id);
    assert!(enabled(d, id), "live since M11.1");
    assert_eq!(saved(d, "import.detectDuplicates"), json!(true));
    click(d, id);
    click(d, APPLY);
    assert_eq!(saved(d, "import.detectDuplicates"), json!(false));
}
