//! Compare and before/after in the edit view (`editing.md` §4), headless and by id: A│B shows two
//! photos and the filmstrip changes the focused one; Auto Fix on A│A's right side stays off the
//! photo's sidecar until "Keep which version?" → Right makes it one history step, and Cancel stays
//! in A│A; switching sides with Crop open asks "Apply changes to the current photo?" and Discard
//! drops the draft; `\` held shows the original and let go shows the edits; a wheel notch over one
//! side zooms both. Fingerprints of A│B side by side and stacked, A│A and the keep question at
//! 1400 × 900 (at 900 × 600 they only have to fit).
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::preview::PaneShows;
use dalica_scenes::Scene;
use dalica_ui::panels::edit::canvas;
use dalica_ui::state::{Compare, CompareLayout, Side, ToolKind};
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::{Value, json};

/// A library under `dir` holding one folder of three 900 × 600 photos: the folder, the session and
/// the photos in name order.
fn session(dir: &Path) -> (PathBuf, Session, Vec<PhotoId>) {
    let folder = dir.join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(60 + i, 900, 600), 90, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (folder, s, ids.into_iter().map(|(_, id)| id).collect())
}

/// The window in the edit view on the first photo.
fn editing(dir: &Path) -> (PathBuf, Driver, Vec<PhotoId>) {
    let (folder, s, ids) = session(dir);
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.app_mut().run("view.edit", &json!({ "id": ids[0] })).unwrap();
    d.settle(FRAMES).unwrap();
    (folder, d, ids)
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

fn edits(d: &Driver, id: PhotoId) -> Vec<String> {
    d.app().session().unwrap().library().info(id).edits.clone()
}

/// The folder's sidecar after every pending write landed (`Null` when there is none).
fn sidecar(d: &mut Driver, folder: &Path) -> Value {
    d.app_mut().session_mut().unwrap().flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

#[test]
fn a_b_shows_two_photos_and_the_filmstrip_changes_the_focused_one() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut d, ids) = editing(dir.path());
    click(&mut d, "button:compare.two");
    let e = &d.state().edit;
    assert_eq!((e.compare, e.photo, e.second, e.focus), (Compare::Two, Some(ids[0]), Some(ids[1]), Side::Right));
    let (focused, other) = (canvas::view(d.ctx()).unwrap(), canvas::other_view(d.ctx()).unwrap());
    assert_eq!((focused.photo, other.photo), (ids[1], ids[0]), "the right side has the focus");
    assert!(other.area.max.x < focused.area.min.x, "side by side");
    let s = d.app().session().unwrap();
    assert_eq!((s.preview_photo_in(0), s.preview_photo_in(1)), (Some(ids[1]), Some(ids[0])), "each side from its pane");
    assert!(s.preview_in(0).is_some() && s.preview_in(1).is_some(), "both drawn");
    let pill = widget(&d, "label:compare.selected").rect;
    assert!(pill[0] >= focused.area.min.x && pill[0] + pill[2] <= focused.area.max.x, "the pill is on the focused side");
    check_sizes(&mut d, "region:canvas", "edit-compare-two");

    // The filmstrip changes the focused side's photo; the other side keeps its own.
    click(&mut d, &format!("film:{}", ids[2]));
    assert_eq!((d.state().edit.photo, d.state().edit.second), (Some(ids[0]), Some(ids[2])));
    // A click on the other side gives it the focus; the filmstrip then changes that one.
    click(&mut d, "canvas:edit.other");
    assert_eq!(d.state().edit.focus, Side::Left);
    click(&mut d, &format!("film:{}", ids[1]));
    assert_eq!((d.state().edit.photo, d.state().edit.second), (Some(ids[1]), Some(ids[2])));

    click(&mut d, "button:compare.switchLayout");
    assert_eq!(d.state().edit.layout, CompareLayout::Stacked);
    let (focused, other) = (canvas::view(d.ctx()).unwrap(), canvas::other_view(d.ctx()).unwrap());
    assert!(focused.area.max.y < other.area.min.y, "one above the other, the focused one on top");
    check_sizes(&mut d, "region:canvas", "edit-compare-stacked");
}

#[test]
fn auto_fix_on_the_right_of_a_a_stays_off_the_photo_until_the_right_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, mut d, ids) = editing(dir.path());
    let photo = ids[0];
    click(&mut d, "button:compare.same");
    assert_eq!(d.state().edit.compare, Compare::Same);
    click(&mut d, "canvas:edit.other");
    assert_eq!(d.state().edit.focus, Side::Right);
    click(&mut d, "tool:autofix");
    assert_eq!(d.app().session().unwrap().variant().edits(), Some(&["autofix()".to_string()][..]), "the variant took the step");
    assert!(edits(&d, photo).is_empty(), "the photo's own recipe did not");
    let v = sidecar(&mut d, &folder);
    assert!(v["photos"]["IMG_0001.jpg"]["edits"].as_array().is_none_or(Vec::is_empty), "nothing in the sidecar: {v}");
    assert_eq!(d.app().session().unwrap().preview_shows(0), Some(PaneShows::Variant), "the focused right side shows the variant");
    // The tool panels do not act on a variant yet.
    click(&mut d, "label:edit.toolsLater");
    assert_eq!(d.state().edit.tool, None);
    assert!(d.app_mut().run("edit.tool.open", &json!({ "tool": "crop" })).is_err());
    check_sizes(&mut d, "region:canvas", "edit-compare-same");

    // Leaving with sides that differ asks; Cancel stays in A│A.
    click(&mut d, "button:compare.one");
    assert_eq!(widget(&d, "dialog:keepVersion").label, "Keep Which Version?");
    check_sizes(&mut d, "dialog:keepVersion", "dialog-keep-version");
    click(&mut d, "button:dialog.cancel");
    assert_eq!(d.state().edit.compare, Compare::Same);
    assert!(d.app().session().unwrap().variant().photo().is_some());

    // Right: the variant's recipe is one step of the photo's history.
    click(&mut d, "button:compare.one");
    click(&mut d, "button:dialog.right");
    assert_eq!(d.state().edit.compare, Compare::One);
    assert_eq!(edits(&d, photo), ["autofix()"]);
    assert_eq!(sidecar(&mut d, &folder)["photos"]["IMG_0001.jpg"]["edits"], json!(["autofix()"]));
    click(&mut d, "button:edit.undo");
    assert!(edits(&d, photo).is_empty(), "one Undo gives the recipe before it back");
}

#[test]
fn switching_sides_with_crop_open_asks_and_discard_drops_the_draft() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut d, ids) = editing(dir.path());
    click(&mut d, "button:compare.two");
    click(&mut d, "tool:crop");
    assert_eq!(d.state().edit.tool, Some(ToolKind::Crop));
    click(&mut d, "button:compare.switchFocus");
    assert_eq!(widget(&d, "dialog:applyChanges").label, "Apply Changes");
    assert_eq!(d.state().edit.focus, Side::Right, "nothing switched while asking");
    click(&mut d, "button:dialog.discard");
    assert_eq!((d.state().edit.tool, d.state().edit.crop.is_none()), (None, true), "the draft is dropped");
    assert_eq!(d.state().edit.focus, Side::Left, "and the switch done");
    assert!(edits(&d, ids[1]).is_empty() && edits(&d, ids[0]).is_empty(), "nothing applied");
}

#[test]
fn holding_backslash_shows_the_original_and_letting_go_shows_the_edits() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut d, _) = editing(dir.path());
    click(&mut d, "tool:autofix");
    let shows = |d: &Driver| d.app().session().unwrap().preview_shows(0);
    d.key_state("\\", dalica_ui::headless::KeyState::Down).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().edit.show_original);
    assert_eq!(shows(&d), Some(PaneShows::Unedited));
    assert!(canvas::view(d.ctx()).unwrap().size.0 > 0.0);
    d.key_state("\\", dalica_ui::headless::KeyState::Up).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!d.state().edit.show_original);
    assert_eq!(shows(&d), Some(PaneShows::Edited));
}

/// A wheel notch arrives in lines, which the driver does not send: these frames run on egui
/// directly.
#[test]
fn a_wheel_notch_over_one_side_zooms_both() {
    let dir = tempfile::tempdir().unwrap();
    let (_, s, ids) = session(dir.path());
    let mut app = App::new(Some(s));
    let ctx = egui::Context::default();
    app.install(&ctx);
    let mut time = 0.0;
    let mut frame = |app: &mut App, events: Vec<egui::Event>| {
        let mut input = dalica_ui_kit::raw_input(LARGE.0, LARGE.1, 1.0);
        input.time = Some(time);
        input.events = events;
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        time += 1.0 / 60.0;
    };
    app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
    app.run("edit.compare", &json!({ "mode": "two" })).unwrap();
    for _ in 0..2000 {
        frame(&mut app, vec![]);
        let s = app.session().unwrap();
        if s.preview_in(0).is_some() && s.preview_in(1).is_some() && s.preview_pending() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let other = canvas::other_view(&ctx).unwrap();
    frame(&mut app, vec![egui::Event::PointerMoved(other.area.center())]);
    let notch = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&mut app, vec![notch]);
    frame(&mut app, vec![]);
    let (focused, other) = (canvas::view(&ctx).unwrap(), canvas::other_view(&ctx).unwrap());
    assert!(other.scale > other.fit * 1.05, "the side under the wheel zoomed: {other:?}");
    assert!((focused.scale - other.scale).abs() < 1e-4, "and the other side with it: {} vs {}", focused.scale, other.scale);
}
