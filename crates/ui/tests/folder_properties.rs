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
//! - renaming in Folder Properties (M9.20) renames the folder on disk as its own undo step; the
//!   section title and the tree row follow it, undo puts the name back, a blank name greys OK and
//!   a name another library folder has is refused with nothing changed;
//! - the header and the dialog match their fingerprints at 1400 × 900 and fit 900 × 600
//!   (`tests/fingerprints/region-header-info-1400x900.json`,
//!   `dialog-folder-properties-1400x900.json`; `DALICA_BLESS=1` stores new ones, renders go to
//!   `target/test-output/ui/`).

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, put_dated_jpeg, shot, try_widget};
use dalica_core::{Date, FolderId};
use dalica_engine::Session;
use dalica_ui::dialogs::folder_properties::{self, BROWSE_ID, DATE_ID, DESCRIPTION_ID, MUSIC_ID, NAME_ID, PLACE_ID, USE_OLDEST_ID};
use dalica_ui::headless::{Click, DriverError};
use dalica_ui::panels::light_table::header::{DESCRIPTION_FIELD, PLACEHOLDER};
use dalica_ui::state::{DescriptionEdit, DialogKind, Section};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

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
        put_dated_jpeg(&photos.join("Lisbon Weekend").join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    for i in 0..2 {
        put_dated_jpeg(&photos.join("Porto").join(format!("IMG_{i:04}.jpg")), 10 + i, (2026, 5, 1));
    }
    let mut s = open_session(&dir.path().join("library"));
    for f in ["Lisbon Weekend", "Porto"] {
        s.execute("folders.add", &json!({ "path": photos.join(f) })).unwrap();
    }
    s.wait_idle();
    let lisbon = s.library().folder_by_path(&photos.join("Lisbon Weekend")).unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, d }
}

/// The library under `fx`'s directory opened again in a new window: a restart.
fn restart(fx: Fx) -> Fx {
    let Fx { dir, lisbon, d } = fx;
    let size = d.size();
    drop(d.into_app());
    let s = open_session(&dir.path().join("library"));
    let mut d = new_driver(App::new(Some(s)), size.0, size.1, size.2);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, d }
}

fn has(d: &Driver, id: &str) -> bool {
    try_widget(d, id).is_some()
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

/// The inline editor by widget id: a click opens it focused on the description, typing fills the
/// draft, Enter commits it as one undo step, Esc changes nothing; the description survives a
/// restart.
#[test]
fn an_inline_description_by_id_survives_a_restart() {
    let mut fx = fixture((1400, 900));
    let (folder, section) = (fx.lisbon, Section::Folder(fx.lisbon));
    let row = try_widget(&fx.d, &part(folder, "description")).unwrap();
    assert_eq!(row.label, PLACEHOLDER);
    assert!(row.enabled, "a click edits the description");
    assert!(!has(&fx.d, DESCRIPTION_FIELD));

    // A click opens the editor, focused, on the (empty) description.
    fx.d.click(&part(folder, "description")).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, Some(DescriptionEdit { section, draft: String::new() }));
    let field = try_widget(&fx.d, DESCRIPTION_FIELD).expect("the editor is open");
    assert!(field.active && fx.d.ctx().egui_wants_keyboard_input(), "it has the focus");
    assert!(!has(&fx.d, &part(folder, "description")), "the field takes the description's place");
    fx.d.type_text("Trams and tiles 🚋").unwrap();
    assert_eq!(fx.d.state().description_edit.as_ref().map(|e| e.draft.as_str()), Some("Trams and tiles 🚋"));
    assert_eq!(info(&fx.d, folder).description, "", "nothing is applied before Enter");
    shot(&fx.d, "header-editing-1400x900");
    fx.d.key("Enter").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().description_edit, None, "Enter closes the editor");
    assert_eq!(info(&fx.d, folder).description, "Trams and tiles 🚋");
    assert_eq!(undo_steps(&fx.d), (1, Some("Folder Properties")), "one undo step");
    assert_eq!(try_widget(&fx.d, &part(folder, "description")).unwrap().label, "Trams and tiles 🚋");
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
    assert!(try_widget(&fx.d, DESCRIPTION_FIELD).unwrap().active);
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
    assert_eq!(try_widget(&fx.d, &part(folder, "description")).unwrap().label, "Trams and tiles 🚋");
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
    for (id, enabled) in
        [(NAME_ID, true), (DATE_ID, false), (USE_OLDEST_ID, true), (PLACE_ID, true), (DESCRIPTION_ID, true), (MUSIC_ID, false), (BROWSE_ID, false)]
    {
        assert_eq!(try_widget(&fx.d, id).unwrap_or_else(|| panic!("no {id}")).enabled, enabled, "{id}");
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
    assert!(try_widget(&fx.d, DATE_ID).unwrap().enabled, "the box off, the date is typed");
    retype(&mut fx.d, DATE_ID, "2026-13-01");
    assert!(!try_widget(&fx.d, "button:dialog.ok").unwrap().enabled, "OK waits for a date that reads");
    retype(&mut fx.d, DATE_ID, "2026-06-01");
    assert!(try_widget(&fx.d, "button:dialog.ok").unwrap().enabled);
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
    assert_eq!(try_widget(&fx.d, &part(folder, "description")).unwrap().label, "A weekend of trams");

    // Opened again, it starts from what was set; "use the oldest" goes back to the photos' date.
    open(&mut fx);
    assert_eq!(
        Value::Object(fields(&fx.d)),
        json!({ "folder": folder, "name": "Lisbon Weekend", "date": "2026-06-01", "useOldest": false, "place": "Lisbon, Portugal", "description": "A weekend of trams" })
    );
    fx.d.click(USE_OLDEST_ID).unwrap();
    assert_eq!((fields(&fx.d)["useOldest"].clone(), fields(&fx.d)["date"].clone()), (json!(true), json!("2026-06-04")));
    assert!(!try_widget(&fx.d, DATE_ID).unwrap().enabled);
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

/// Runs frames until the file jobs are done and their results are in the library.
fn settle_jobs(d: &mut Driver) {
    let s = d.settle_for(FRAMES, true);
    assert!(s.settled, "still busy: {:?}", s.busy);
}

/// The Name field renames the folder on disk: OK runs `folder.rename` (its own undo step) and then
/// `folder.info.set`; the section title and the tree row follow, and undo puts the name back. A
/// blank name greys OK; a name another library folder has is refused and nothing changes.
#[test]
fn renaming_in_folder_properties_renames_on_disk_and_the_title_follows() {
    let mut fx = fixture((1400, 900));
    let folder = fx.lisbon;
    let photos = fx.dir.path().join("photos");
    let (old, new) = (photos.join("Lisbon Weekend"), photos.join("Lisbon 2026"));
    let title = |d: &Driver| try_widget(d, &part(folder, "title")).unwrap().label;
    let row = |d: &Driver| try_widget(d, &format!("tree:folder:{}", folder.get())).unwrap().label;
    assert_eq!(title(&fx.d), "Lisbon Weekend");
    let open = |fx: &mut Fx| {
        fx.d.run("section.properties", &json!({ "folder": folder })).unwrap();
        fx.d.settle(FRAMES).unwrap();
        assert!(has(&fx.d, "dialog:folderProperties"));
    };

    // A blank name greys OK; a name the library's other folder has is refused, nothing changed.
    open(&mut fx);
    retype(&mut fx.d, NAME_ID, "  ");
    assert!(!try_widget(&fx.d, "button:dialog.ok").unwrap().enabled, "OK waits for a name");
    retype(&mut fx.d, NAME_ID, "Porto");
    retype(&mut fx.d, PLACE_ID, "Lisbon, Portugal");
    fx.d.click("button:dialog.ok").unwrap();
    settle_jobs(&mut fx.d);
    assert!(has(&fx.d, "dialog:folderProperties"), "the dialog stays open with the reason");
    assert!(fx.d.state().status.contains("Porto"), "{}", fx.d.state().status);
    assert!(info(&fx.d, folder).is_empty() && undo_steps(&fx.d).0 == 0, "nothing changed");
    assert!(old.is_dir());
    fx.d.click("button:dialog.cancel").unwrap();
    fx.d.settle(FRAMES).unwrap();

    // A new name and a place: the folder is renamed on disk, then its place is set.
    open(&mut fx);
    retype(&mut fx.d, NAME_ID, "Lisbon 2026");
    assert_eq!(fields(&fx.d)["name"], "Lisbon 2026");
    retype(&mut fx.d, PLACE_ID, "Lisbon, Portugal");
    fx.d.click("button:dialog.ok").unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty(), "OK closes the dialog");
    assert!(new.is_dir() && !old.exists(), "renamed on disk");
    assert_eq!(std::fs::read_dir(&new).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "jpg")).count(), 3);
    let lib = fx.d.app().session().unwrap().library();
    assert_eq!(lib.folder(folder).unwrap().path, new, "the folder keeps its id");
    assert_eq!(info(&fx.d, folder).place, "Lisbon, Portugal");
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(title(&fx.d), "Lisbon 2026", "the section title follows");
    assert!(row(&fx.d).contains("Lisbon 2026"), "the tree row follows: {}", row(&fx.d));
    // The rename's step lands when its file job ends, after the information's.
    assert_eq!(undo_steps(&fx.d), (2, Some("Rename Folder")), "the rename and the information, one step each");

    // Undo puts the name back, then takes back the place.
    fx.d.run("edit.undo", &json!({})).unwrap();
    settle_jobs(&mut fx.d);
    assert!(old.is_dir() && !new.exists(), "undo puts the name back");
    assert_eq!(title(&fx.d), "Lisbon Weekend");
    assert_eq!(undo_steps(&fx.d).1, Some("Folder Properties"));
    fx.d.run("edit.undo", &json!({})).unwrap();
    settle_jobs(&mut fx.d);
    assert!(info(&fx.d, folder).is_empty());

    // Only the name changed: one step, the rename.
    open(&mut fx);
    retype(&mut fx.d, NAME_ID, "Lisboa");
    fx.d.click("button:dialog.ok").unwrap();
    settle_jobs(&mut fx.d);
    assert!(photos.join("Lisboa").is_dir());
    assert_eq!(undo_steps(&fx.d).1, Some("Rename Folder"), "nothing else to set: no second step");
    fx.d.run("edit.undo", &json!({})).unwrap();
    settle_jobs(&mut fx.d);
    assert_eq!(undo_steps(&fx.d).1, None, "the only step was the rename");
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

/// The header (date · place and a description) and the dialog match their fingerprints, and fit
/// the small window.
#[test]
fn the_header_and_the_dialog_match_their_fingerprints_and_fit_the_small_window() {
    let mut fx = fixture(LARGE);
    let folder = fx.lisbon;
    fx.d.run("folder.info.set", &json!({ "id": folder, "description": "A weekend of trams and tiles", "place": "Lisbon, Portugal" })).unwrap();
    fx.d.app_mut().state_mut().folder = Some(folder);
    fx.d.settle(FRAMES).unwrap();
    check_sizes(&mut fx.d, &format!("section:folder:{folder}"), "region-header-info");

    fx.d.click(&part(folder, "description")).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.key("Esc").unwrap();

    fx.d.run("section.properties", &json!({ "folder": folder })).unwrap();
    // The draft as a user leaves it before OK: the box off, a date of their own.
    let top = fx.d.app_mut().state_mut().dialogs.last_mut().unwrap();
    top.fields.insert("useOldest".into(), json!(false));
    top.fields.insert("date".into(), json!("2026-06-01"));
    fx.d.settle(FRAMES).unwrap();
    check_sizes(&mut fx.d, "dialog:folderProperties", "dialog-folder-properties");
}
