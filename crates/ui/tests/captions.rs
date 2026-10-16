//! Captions in the library (`roadmap.md` M6.25; `organizing.md` §5.3, `ui-layout.md` §5.3),
//! driven headlessly: Picture ▸ Edit Caption opens `field:caption.inline` under the focused photo's
//! cell with its caption; typing and Enter set it through `photo.caption.set` (the sidecar holds it,
//! one undo removes it); Esc changes nothing; a click elsewhere and scrolling the cell away commit;
//! the view's shortcuts (Ctrl+8, Ctrl+A, Ctrl+Z) do nothing while typing; the Caption and Tags
//! thumbnail-caption modes show the model's text, middle-elided; the cells with captions and the
//! open editor match their fingerprints at 1400 × 900 (at 900 × 600 they only have to fit). Renders go to
//! `target/test-output/ui/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_pixels, new_driver, out_dir};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_scenes::fingerprint;
use dalica_ui::panels::light_table::caption_edit::{FIELD_ID, PLACEHOLDER};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use egui::vec2;
use serde_json::{Value, json};

/// A library watching one folder `Lisbon` of `n` photos `IMG_0001.jpg`…; ids in name order.
fn setup(n: usize, size: (u32, u32)) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Driver, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    // A clock that moves only when told, a day after the files were written: the captions a test
    // sets are stamped at one instant, so Recently Updated (newest change first, then by name,
    // `organizing.md` §4.1) lists them in the same order on every run, and being indexed is no change.
    let start = dalica_core::Clock::now(&dalica_core::SystemClock).saturating_add(std::time::Duration::from_secs(86_400));
    let mut s = Session::open(lib.path(), Arc::new(dalica_core::VirtualClock::new(start))).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    let mut d = new_driver(App::new(Some(s)), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    (lib, disk, folder, d, ids.into_iter().map(|(_, id)| id).collect())
}

fn session(d: &mut Driver) -> &mut Session {
    d.app_mut().session_mut().unwrap()
}

fn caption(d: &Driver, id: PhotoId) -> String {
    d.app().session().unwrap().library().caption(id).to_owned()
}

/// The folder's sidecar after every pending write landed (`Null` when there is none).
fn sidecar(d: &mut Driver, folder: &Path) -> Value {
    session(d).flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

fn sidecar_caption(v: &Value, name: &str) -> Option<String> {
    v["photos"][name]["caption"].as_str().map(str::to_owned)
}

fn field(d: &Driver) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == FIELD_ID)
}

fn focused(d: &Driver) -> bool {
    d.ctx().egui_wants_keyboard_input()
}

fn draft(d: &Driver) -> Option<String> {
    d.state().caption_edit.as_ref().map(|e| e.draft.clone())
}

fn undo_len(d: &Driver) -> usize {
    d.app().session().unwrap().undo_stack().len()
}

/// Selects `id` and opens Picture ▸ Edit Caption on it.
fn open(d: &mut Driver, id: PhotoId) {
    d.run("select.set", &json!({ "ids": [id.get()] })).unwrap();
    d.run("photo.caption.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
}

#[test]
fn a_caption_typed_by_id_and_enter_is_in_the_sidecar_and_one_undo_removes_it() {
    let (_l, _d, folder, mut d, ids) = setup(3, (1400, 900));
    d.run("select.set", &json!({ "ids": [ids[1].get()] })).unwrap();
    // Picture ▸ Edit Caption from the menu bar.
    d.click("menu:Picture").unwrap();
    d.click("menuitem:photo.caption.edit").unwrap();
    d.settle(FRAMES).unwrap();
    let w = field(&d).expect("the editor is on screen");
    assert_eq!(w.label, PLACEHOLDER);
    assert!(focused(&d), "the editor opens ready for typing");
    assert_eq!(draft(&d).as_deref(), Some(""));
    // Under the photo's cell, inside its width (the id is on the text inside the field's frame).
    let cell = d.widgets().into_iter().find(|w| w.id == format!("thumb:{}", ids[1].get())).unwrap();
    assert!(
        w.rect[0] >= cell.rect[0] && w.rect[0] + w.rect[2] <= cell.rect[0] + cell.rect[2] && w.rect[2] >= cell.rect[2] - 16.0,
        "{w:?} in {cell:?}"
    );
    assert!(
        w.rect[1] >= cell.rect[1] + cell.rect[3] - 2.0 && w.rect[1] + w.rect[3] <= cell.rect[1] + cell.rect[3] + 22.0 + 0.5,
        "{w:?} under {cell:?}"
    );

    d.click(FIELD_ID).unwrap();
    d.type_text("  Tram 28 at sunset ").unwrap();
    assert_eq!(draft(&d).as_deref(), Some("  Tram 28 at sunset "), "the view state holds the text typed so far");
    assert_eq!(caption(&d, ids[1]), "", "nothing is applied before the commit");
    d.key("Enter").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().caption_edit, None, "Enter closes the editor");
    assert!(field(&d).is_none());
    assert_eq!(caption(&d, ids[1]), "Tram 28 at sunset", "photo.caption.set trims it");
    assert_eq!(undo_len(&d), 1, "one undo step");
    assert_eq!(sidecar_caption(&sidecar(&mut d, &folder), "IMG_0002.jpg").as_deref(), Some("Tram 28 at sunset"));

    // Reopened, it starts from the caption; Enter with nothing changed records no step.
    open(&mut d, ids[1]);
    assert_eq!(draft(&d).as_deref(), Some("Tram 28 at sunset"));
    d.key("Enter").unwrap();
    assert_eq!(undo_len(&d), 1);

    d.key("Ctrl+Z").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(caption(&d, ids[1]), "", "one undo removes it");
    assert_eq!(sidecar_caption(&sidecar(&mut d, &folder), "IMG_0002.jpg"), None);
}

#[test]
fn esc_leaves_the_caption_as_it_was() {
    let (_l, _d, folder, mut d, ids) = setup(2, (1400, 900));
    session(&mut d).execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Alfama" })).unwrap();
    let steps = undo_len(&d);
    open(&mut d, ids[0]);
    assert_eq!(draft(&d).as_deref(), Some("Alfama"));
    d.type_text(" at night").unwrap();
    assert_eq!(draft(&d).as_deref(), Some("Alfama at night"));
    d.key("Esc").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().caption_edit, None, "Esc closes the editor");
    assert!(!focused(&d));
    assert_eq!(caption(&d, ids[0]), "Alfama");
    assert_eq!(undo_len(&d), steps, "no step");
    assert_eq!(sidecar_caption(&sidecar(&mut d, &folder), "IMG_0001.jpg").as_deref(), Some("Alfama"));
}

#[test]
fn the_views_shortcuts_do_nothing_while_typing_and_work_again_after() {
    let (_l, _d, _f, mut d, ids) = setup(3, (1400, 900));
    open(&mut d, ids[0]);
    d.type_text("Belém").unwrap();
    for key in ["Ctrl+8", "F4", "Ctrl+A", "Ctrl+Z", "Ctrl+2"] {
        d.key(key).unwrap();
    }
    d.settle(FRAMES).unwrap();
    let lib = d.app().session().unwrap().library();
    assert!(ids.iter().all(|id| !lib.photo(*id).unwrap().star), "Ctrl+8 typed into the editor stars nothing");
    assert_eq!(d.state().selection.ids, vec![ids[0]], "Ctrl+A selects text, not photos");
    assert_eq!(undo_len(&d), 0);
    assert!(d.state().caption_edit.is_some(), "still editing");
    assert!(focused(&d));
    // Ctrl+Z was the field's own undo: the typing is gone from the field, not from the library.
    d.type_text("Belém").unwrap();
    d.key("Enter").unwrap();
    assert_eq!(caption(&d, ids[0]), "Belém");
    // Out of the field, the star key stars again.
    d.key("Ctrl+8").unwrap();
    assert!(d.app().session().unwrap().library().photo(ids[0]).unwrap().star);
}

#[test]
fn a_click_elsewhere_and_scrolling_the_cell_away_commit() {
    let (_l, _d, folder, mut d, ids) = setup(60, (900, 600));
    open(&mut d, ids[0]);
    d.type_text("Rossio").unwrap();
    d.click(&format!("thumb:{}", ids[1].get())).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().caption_edit, None);
    assert_eq!(caption(&d, ids[0]), "Rossio", "a click elsewhere commits");

    // Opening the editor on another photo commits the first one's text too.
    open(&mut d, ids[2]);
    d.type_text("Chiado").unwrap();
    d.run("photo.caption.edit", &json!({ "id": ids[3].get() })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(caption(&d, ids[2]), "Chiado");
    assert_eq!(d.state().caption_edit.as_ref().map(|e| e.photo), Some(ids[3]));
    d.key("Esc").unwrap();

    open(&mut d, ids[4]);
    d.type_text("Graça").unwrap();
    d.scroll(Some("region:lightTable"), vec2(0.0, -4000.0)).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(field(&d).is_none());
    assert_eq!(d.state().caption_edit, None, "scrolling the cell away closes the editor");
    assert_eq!(caption(&d, ids[4]), "Graça", "and keeps what was typed");
    assert!(!focused(&d), "the window's keys are back");
    assert_eq!(undo_len(&d), 3);
    assert_eq!(sidecar_caption(&sidecar(&mut d, &folder), "IMG_0005.jpg").as_deref(), Some("Graça"));

    // Picture ▸ Edit Caption on a photo out of view scrolls its cell into view.
    let last = *ids.last().unwrap();
    d.run("scroll.top", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    open(&mut d, last);
    assert!(field(&d).is_some(), "the cell came into view with its editor");
    assert!(focused(&d));
}

#[test]
fn the_caption_and_tags_modes_show_the_models_text_middle_elided() {
    let (_l, _d, _f, mut d, ids) = setup(3, (1400, 900));
    let long = "A tram climbing the hill to the castle in the late afternoon light";
    let s = session(&mut d);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": long })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [ids[0]], "tags": ["lisbon", "tram"] })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [ids[1]], "tags": ["night"] })).unwrap();
    let text = |d: &Driver, id: PhotoId| d.app().panels().unwrap().light_table.cells.get(id).and_then(|s| s.caption.clone()).expect("a caption line");

    d.run("view.caption", &json!({ "caption": "caption" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(text(&d, ids[0]), "Tram 28");
    let cut = text(&d, ids[1]);
    assert!(cut.contains('…') && cut.starts_with("A tram") && cut.ends_with("light"), "middle-elided: {cut}");
    assert_eq!(text(&d, ids[2]), "", "no caption, an empty line");

    d.run("view.caption", &json!({ "caption": "tags" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(text(&d, ids[0]), "lisbon, tram", "tags joined with \", \" in the order added");
    assert_eq!(text(&d, ids[1]), "night");
    assert_eq!(text(&d, ids[2]), "");

    // The open editor takes its cell's caption line (the folder's cell; the photo's copy in
    // Recently Updated keeps its line).
    d.run("view.caption", &json!({ "caption": "caption" })).unwrap();
    open(&mut d, ids[0]);
    let cells = &d.app().panels().unwrap().light_table.cells;
    assert!(cells.editor_shown());
    let lines: Vec<Option<String>> = cells.shown().iter().filter(|s| s.photo == ids[0]).map(|s| s.caption.clone()).collect();
    assert_eq!(lines, [Some("Tram 28".to_string()), None], "the copy in Recently Updated, then the folder's cell");
    assert_eq!(text(&d, ids[1]), cut);
}

#[test]
fn a_caption_longer_than_a_draft_is_not_cut_by_the_inline_editor() {
    let (_l, _d, _f, mut d, ids) = setup(1, (1400, 900));
    let long = "x".repeat(dalica_ui::state::MAX_DRAFT + 1);
    session(&mut d).execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": long })).unwrap();
    d.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
    assert!(d.run("photo.caption.edit", &json!({})).is_err());
    assert_eq!(d.state().caption_edit, None);
    assert_eq!(caption(&d, ids[0]).len(), long.len());
}

// ---- Fingerprints -------------------------------------------------------------------------------

/// The light table's cells (headers blacked out), as `library_window.rs` cuts the cells region.
fn check_cells(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    let lt = d.widgets().into_iter().find(|w| w.id == "region:lightTable").unwrap().rect;
    let info = d.app().panels().unwrap().light_table.frame_info().clone();
    let headers: Vec<egui::Rect> = info.headers.iter().map(|(_, r)| *r).chain(info.sticky.map(|(_, r)| r)).collect();
    let [x0, y0] = [lt[0].round() as u32, lt[1].round() as u32];
    let [x1, y1] = [((lt[0] + lt[2]).round() as u32).min(shot.width), ((lt[1] + lt[3]).round() as u32).min(shot.height)];
    let mut rgba = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let header = headers.iter().any(|r| r.contains(p));
            rgba.extend_from_slice(&if header { [0, 0, 0, 255] } else { shot.pixel(x, y).unwrap() });
        }
    }
    check_pixels(name, x1 - x0, y1 - y0, &rgba);
    std::fs::write(out_dir().join(format!("window-{name}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

#[test]
fn the_cells_with_captions_and_the_open_editor_match_their_fingerprints() {
    let (size, case) = (LARGE, "1400");
    let (_l, _d, _f, mut d, ids) = setup(8, size);
    let s = session(&mut d);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[2]], "caption": "A tram climbing the hill to the castle in the late afternoon" })).unwrap();
    d.run("view.caption", &json!({ "caption": "caption" })).unwrap();
    open(&mut d, ids[1]);
    d.type_text("Miradouro").unwrap();
    d.settle(FRAMES).unwrap();
    check_cells(&d, &format!("region-cells-caption-{case}"));
}
