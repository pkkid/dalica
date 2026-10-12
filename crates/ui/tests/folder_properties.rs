//! Folder descriptions and Folder Properties (M6.26, `ui-layout.md` §5.2, `organizing.md` §10),
//! driven headlessly by widget id on a library of two folders of dated photos:
//!
//! - a click on a header's description edits it inline (`field:section.description`): Enter
//!   commits it through `folder.info.set` as one undo step, Esc closes it and changes nothing, and
//!   the description is still there after a restart;
//! - a double-click on a section title opens Folder Properties for that folder: its draft starts
//!   from the library, Cancel changes nothing, OK sets the date and place (and description) in one
//!   undo step, which undo takes back; OK is disabled while the date does not read;
//! - the header's date line shows the folder's date and " · place";
//! - every widget has an id, and the header and the dialog match their fingerprints at 1400 × 900
//!   and 900 × 600 (`tests/fingerprints/region-header-info-<w>x<h>.json`,
//!   `dialog-folder-properties-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones, renders go to
//!   `target/test-output/ui/`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{Date, DateTime, FolderId};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::dialogs::folder_properties::{self, BROWSE_ID, DATE_ID, DESCRIPTION_ID, MUSIC_ID, NAME_ID, PLACE_ID, USE_OLDEST_ID};
use dalica_ui::headless::{Click, DriverError};
use dalica_ui::panels::light_table::header::{DESCRIPTION_FIELD, PLACEHOLDER};
use dalica_ui::state::{DescriptionEdit, DialogKind, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry;
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A JPEG of `seed` taken on `day` at 10:00 (EXIF DateTimeOriginal).
fn put_jpeg(path: &Path, seed: u64, (y, m, d): (u16, u8, u8)) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let taken = DateTime::new(Date::new(y, m, d).unwrap(), 10, 0, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, Some(&exif)).unwrap();
    std::fs::write(path, jpeg).unwrap();
}

fn open_session(library: &Path) -> Session {
    let mut s = Session::open(library, Arc::new(dalica_core::SystemClock)).unwrap();
    s.wait_idle();
    s
}

struct Fx {
    dir: tempfile::TempDir,
    /// `Lisbon Weekend` (three photos, the oldest taken on 2026-06-04) and `Porto` (two).
    lisbon: FolderId,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    for (i, day) in [(2026, 6, 5), (2026, 6, 4), (2026, 6, 6)].into_iter().enumerate() {
        put_jpeg(&photos.join("Lisbon Weekend").join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    for i in 0..2 {
        put_jpeg(&photos.join("Porto").join(format!("IMG_{i:04}.jpg")), 10 + i, (2026, 5, 1));
    }
    let mut s = open_session(&dir.path().join("library"));
    for f in ["Lisbon Weekend", "Porto"] {
        s.execute("folders.add", &json!({ "path": photos.join(f) })).unwrap();
    }
    s.wait_idle();
    let lisbon = s.library().folder_by_path(&photos.join("Lisbon Weekend")).unwrap();
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, d }
}

/// The library under `fx`'s directory opened again in a new window: a restart.
fn restart(fx: Fx) -> Fx {
    let Fx { dir, lisbon, d } = fx;
    let size = d.size();
    drop(d.into_app());
    let s = open_session(&dir.path().join("library"));
    let mut d = Driver::new(App::new(Some(s)), size.0, size.1, size.2);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, d }
}

fn widget(d: &Driver, id: &str) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == id)
}

fn has(d: &Driver, id: &str) -> bool {
    widget(d, id).is_some()
}

fn part(folder: FolderId, p: &str) -> String {
    format!("section:folder:{folder}/{p}")
}

fn info(d: &Driver, folder: FolderId) -> dalica_library::FolderInfo {
    d.app().session().unwrap().library().folder_info(folder).clone()
}

fn undo_steps(d: &Driver) -> (usize, Option<&'static str>) {
    let u = d.app().session().unwrap().undo_stack();
    (u.len(), u.undo_label())
}

/// The header's date line and description as last drawn.
fn shown(d: &Driver, folder: FolderId) -> (String, String) {
    let (date, description) = d.app().panels().unwrap().light_table.headers.shown(Section::Folder(folder)).expect("the header was drawn");
    (date.to_string(), description.to_string())
}

fn fields(d: &Driver) -> serde_json::Map<String, Value> {
    let top = d.state().dialogs.last().expect("a dialog is open");
    assert_eq!(top.kind, DialogKind::FolderProperties);
    top.fields.clone()
}

/// Replaces the text of the focused field: select all, type.
fn retype(d: &mut Driver, id: &str, text: &str) {
    d.click(id).unwrap();
    d.key("Ctrl+A").unwrap();
    d.type_text(text).unwrap();
}

/// Checks the last frame: no unregistered interactive widget, unique ids in the grammar, every
/// rect on screen.
fn check_ids(d: &Driver) {
    let (w, h, _) = d.size();
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for wi in d.widgets() {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.clone()), "{w}×{h}: `{}` is registered twice", wi.id);
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{w}×{h}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes the whole window to the test output as `<name>.png` (and to `$DALICA_SHOTS` when set).
fn shot(d: &Driver, name: &str) {
    let s = d.screenshot().unwrap();
    let png = fingerprint::png_rgba(s.width, s.height, &s.rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
}

/// Checks the region widget `id` covers in the last frame against `tests/fingerprints/<name>.json`,
/// and writes it and the whole window to the test output.
fn check_region(d: &Driver, id: &str, name: &str) {
    shot(d, &format!("{name}-window"));
    let s = d.screenshot().unwrap();
    let r = widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&s.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// The inline editor by widget id: a click opens it focused on the description, typing fills the
/// draft, Enter commits it as one undo step, Esc changes nothing; the description survives a
/// restart.
#[test]
fn an_inline_description_by_id_survives_a_restart() {
    let mut fx = fixture((1400, 900));
    let (folder, section) = (fx.lisbon, Section::Folder(fx.lisbon));
    let row = widget(&fx.d, &part(folder, "description")).unwrap();
    assert_eq!(row.label, PLACEHOLDER);
    assert!(row.enabled, "a click edits the description");
    assert!(!has(&fx.d, DESCRIPTION_FIELD));

    // A click opens the editor, focused, on the (empty) description.
    fx.d.click(&part(folder, "description")).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, Some(DescriptionEdit { section, draft: String::new() }));
    let field = widget(&fx.d, DESCRIPTION_FIELD).expect("the editor is open");
    assert!(field.active && fx.d.ctx().egui_wants_keyboard_input(), "it has the focus");
    assert!(!has(&fx.d, &part(folder, "description")), "the field takes the description's place");
    check_ids(&fx.d);
    fx.d.type_text("Trams and tiles 🚋").unwrap();
    assert_eq!(fx.d.state().description_edit.as_ref().map(|e| e.draft.as_str()), Some("Trams and tiles 🚋"));
    assert_eq!(info(&fx.d, folder).description, "", "nothing is applied before Enter");
    shot(&fx.d, "header-editing-1400x900");
    fx.d.key("Enter").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, None, "Enter closes the editor");
    assert_eq!(info(&fx.d, folder).description, "Trams and tiles 🚋");
    assert_eq!(undo_steps(&fx.d), (1, Some("Folder Properties")), "one undo step");
    assert_eq!(widget(&fx.d, &part(folder, "description")).unwrap().label, "Trams and tiles 🚋");
    assert_eq!(shown(&fx.d, folder).1, "Trams and tiles 🚋");

    // Esc: the editor closes and nothing changes.
    fx.d.click(&part(folder, "description")).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit.as_ref().map(|e| e.draft.as_str()), Some("Trams and tiles 🚋"), "it opens on the description");
    fx.d.type_text(" and more").unwrap();
    fx.d.key("Esc").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, None);
    assert_eq!(info(&fx.d, folder).description, "Trams and tiles 🚋");
    assert_eq!(undo_steps(&fx.d).0, 1, "Esc makes no step");

    // Opened through ui.set (as an agent would), the editor takes the focus too; a click away commits.
    fx.d.app_mut().state_mut().description_edit = Some(DescriptionEdit { section, draft: "Tiles".into() });
    fx.d.settle(FRAMES).unwrap();
    assert!(widget(&fx.d, DESCRIPTION_FIELD).unwrap().active);
    let porto_title = fx.d.widgets().into_iter().find(|w| w.id.ends_with("/title") && !w.id.contains(&format!(":{folder}/"))).unwrap();
    fx.d.click(&porto_title.id).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!((fx.d.state().description_edit.as_ref(), info(&fx.d, folder).description.as_str()), (None, "Tiles"), "a click away commits");
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(info(&fx.d, folder).description, "Trams and tiles 🚋", "undo takes it back");

    // A restart: the description is read back and shown.
    let fx = restart(fx);
    assert_eq!(info(&fx.d, folder).description, "Trams and tiles 🚋");
    assert_eq!(widget(&fx.d, &part(folder, "description")).unwrap().label, "Trams and tiles 🚋");
}

/// A double-click on the title opens Folder Properties for that folder; Cancel changes nothing;
/// OK sets the date, place and description in one undo step, and the header shows them.
#[test]
fn folder_properties_ok_sets_date_and_place_in_one_undo_step_and_cancel_changes_nothing() {
    let mut fx = fixture((1400, 900));
    let folder = fx.lisbon;
    let (date, _) = shown(&fx.d, folder);
    assert_eq!(date, "Thursday, June 4, 2026", "the oldest photo's date, no place");

    let open = |fx: &mut Fx| {
        // Clear of the last click, so the two clicks read as a double-click, not a triple.
        fx.d.wait(700).unwrap();
        fx.d.click_with(&part(folder, "title"), &Click { count: 2, ..Click::default() }).unwrap();
        fx.d.settle(FRAMES).unwrap();
        assert!(has(&fx.d, "dialog:folderProperties"), "a double-click on the title opens Folder Properties");
    };
    open(&mut fx);
    let f = fields(&fx.d);
    assert_eq!(
        Value::Object(f),
        json!({ "folder": folder, "name": "Lisbon Weekend", "date": "2026-06-04", "useOldest": true, "place": "", "description": "" })
    );
    check_ids(&fx.d);
    for (id, enabled) in
        [(NAME_ID, false), (DATE_ID, false), (USE_OLDEST_ID, true), (PLACE_ID, true), (DESCRIPTION_ID, true), (MUSIC_ID, false), (BROWSE_ID, false)]
    {
        assert_eq!(widget(&fx.d, id).unwrap_or_else(|| panic!("no {id}")).enabled, enabled, "{id}");
    }
    assert!(matches!(fx.d.click(BROWSE_ID), Err(DriverError::Disabled(_))), "Music for slideshows waits for slideshows");

    // Cancel: whatever was typed, nothing changes.
    retype(&mut fx.d, PLACE_ID, "Somewhere");
    assert_eq!(fields(&fx.d)["place"], "Somewhere");
    fx.d.click("button:dialog.cancel").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert!(info(&fx.d, folder).is_empty() && undo_steps(&fx.d).0 == 0, "Cancel changes nothing");

    // OK: a date of our own, a place and a description, in one step.
    open(&mut fx);
    fx.d.click(USE_OLDEST_ID).unwrap();
    assert_eq!(fields(&fx.d)["useOldest"], false);
    assert!(widget(&fx.d, DATE_ID).unwrap().enabled, "the box off, the date is typed");
    retype(&mut fx.d, DATE_ID, "2026-13-01");
    assert!(!widget(&fx.d, "button:dialog.ok").unwrap().enabled, "OK waits for a date that reads");
    retype(&mut fx.d, DATE_ID, "2026-06-01");
    assert!(widget(&fx.d, "button:dialog.ok").unwrap().enabled);
    retype(&mut fx.d, PLACE_ID, "Lisbon, Portugal");
    retype(&mut fx.d, DESCRIPTION_ID, "A weekend of trams");
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty(), "OK closes the dialog");
    let i = info(&fx.d, folder);
    assert_eq!((i.date, i.place.as_str(), i.description.as_str()), (Some(Date::new(2026, 6, 1).unwrap()), "Lisbon, Portugal", "A weekend of trams"));
    assert_eq!(undo_steps(&fx.d), (1, Some("Folder Properties")), "one undo step");
    // The header shows them.
    assert_eq!(shown(&fx.d, folder), ("Monday, June 1, 2026 · Lisbon, Portugal".to_string(), "A weekend of trams".to_string()));
    assert_eq!(widget(&fx.d, &part(folder, "description")).unwrap().label, "A weekend of trams");

    // Opened again, it starts from what was set; "use the oldest" goes back to the photos' date.
    open(&mut fx);
    assert_eq!(
        Value::Object(fields(&fx.d)),
        json!({ "folder": folder, "name": "Lisbon Weekend", "date": "2026-06-01", "useOldest": false, "place": "Lisbon, Portugal", "description": "A weekend of trams" })
    );
    fx.d.click(USE_OLDEST_ID).unwrap();
    assert_eq!((fields(&fx.d)["useOldest"].clone(), fields(&fx.d)["date"].clone()), (json!(true), json!("2026-06-04")));
    assert!(!widget(&fx.d, DATE_ID).unwrap().enabled);
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(info(&fx.d, folder).date, None, "back to the oldest photo's date");
    assert_eq!(shown(&fx.d, folder).0, "Thursday, June 4, 2026 · Lisbon, Portugal");
    assert_eq!(undo_steps(&fx.d).0, 2);

    // Undo takes each step back whole.
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(info(&fx.d, folder).is_empty(), "{:?}", info(&fx.d, folder));
    assert_eq!(shown(&fx.d, folder), ("Thursday, June 4, 2026".to_string(), String::new()));
}

/// A description longer than an inline editor keeps opens Folder Properties instead, so nothing is
/// cut; a folder.info.set that fails leaves the reason on the status line.
#[test]
fn a_description_too_long_to_edit_inline_opens_folder_properties() {
    let mut fx = fixture((1400, 900));
    let folder = fx.lisbon;
    let long = "trams ".repeat(1000);
    fx.d.run("folder.info.set", &json!({ "id": folder, "description": long })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click(&part(folder, "description")).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, None);
    assert_eq!(fields(&fx.d)["description"], long.trim(), "Folder Properties holds the whole description");
    assert_eq!(folder_properties::chosen_date(&fields(&fx.d)), Ok(None));
}

/// Every widget has an id with the editor and the dialog open, and the header (date · place and a
/// description) and the dialog match their fingerprints at both window sizes.
#[test]
fn the_header_and_the_dialog_match_their_fingerprints_at_both_sizes() {
    for size in SIZES {
        let (w, h) = size;
        let mut fx = fixture(size);
        let folder = fx.lisbon;
        fx.d.run("folder.info.set", &json!({ "id": folder, "description": "A weekend of trams and tiles", "place": "Lisbon, Portugal" })).unwrap();
        fx.d.app_mut().state_mut().folder = Some(folder);
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d);
        check_region(&fx.d, &format!("section:folder:{folder}"), &format!("region-header-info-{w}x{h}"));

        fx.d.click(&part(folder, "description")).unwrap();
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d);
        fx.d.key("Esc").unwrap();

        fx.d.run("section.properties", &json!({ "folder": folder })).unwrap();
        // The draft as a user leaves it before OK: the box off, a date of their own.
        let top = fx.d.app_mut().state_mut().dialogs.last_mut().unwrap();
        top.fields.insert("useOldest".into(), json!(false));
        top.fields.insert("date".into(), json!("2026-06-01"));
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d);
        check_region(&fx.d, "dialog:folderProperties", &format!("dialog-folder-properties-{w}x{h}"));
    }
}
