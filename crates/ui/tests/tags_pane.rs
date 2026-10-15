//! The Tags pane (M6.28, `organizing.md` §5.2, `menus-and-shortcuts.md` §4), driven headlessly by
//! widget id on a library of ten scenes photos:
//!
//! - Ctrl+T opens and closes the pane.
//! - Typing two tags into "Add a tag:" adds them to a selection of 8 (Enter; commas separate them),
//!   the list shows the counts ("beach (3 of 8)"), × on a hovered row removes one from all 8, and
//!   Undo puts it back.
//! - The autocomplete offers the tags in use; ↓ and Enter add the highlighted one.
//! - Rename Tag… from the tag menu merges onto an existing tag once confirmed, as one undo step.
//! - The pane and the Rename Tag dialog match their fingerprints at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/pane-tags-<w>x<h>.json`, `dialog-rename-tag-<w>x<h>.json`;
//!   `DALICA_BLESS=1` stores new ones, renders go to `target/test-output/ui/`), and every widget
//!   shown has a valid, unique id.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::headless::Click;
use dalica_ui::state::{DialogKind, Pane};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::{self, Role};
use egui::PointerButton;
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<u64>,
}

/// A library of ten photos in one folder, shown at `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, 10, 28).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let mut ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 10);
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, ids }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

/// The labels of the rows of "Tags in the selection:", top to bottom.
fn rows(t: &T) -> Vec<String> {
    t.d.widgets().into_iter().filter(|w| w.id.starts_with("tag:") && !w.id.contains("/")).map(|w| w.label).collect()
}

/// Photo `id`'s tags, as the engine has them.
fn tags_of(t: &T, id: u64) -> Vec<String> {
    let lib = t.d.app().session().unwrap().library();
    let mut tags = lib.tags_of(PhotoId::new(id)).to_vec();
    tags.sort();
    tags
}

/// Selects the first `n` photos.
fn select(t: &mut T, n: usize) {
    let ids: Vec<u64> = t.ids.iter().take(n).copied().collect();
    run(t, "select.set", json!({ "ids": ids }));
}

fn type_into(t: &mut T, field: &str, text: &str) {
    t.d.click(field).unwrap();
    t.d.type_text(text).unwrap();
    t.d.settle(FRAMES).unwrap();
}

fn key(t: &mut T, chord: &str) {
    t.d.key(chord).unwrap();
    t.d.settle(FRAMES).unwrap();
}

fn right_click(t: &mut T, id: &str) {
    t.d.click_with(id, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
}

#[test]
fn ctrl_t_opens_and_closes_the_pane() {
    let mut t = setup(1400, 900);
    key(&mut t, "Ctrl+T");
    assert_eq!(t.d.state().pane, Some(Pane::Tags));
    for id in ["pane:tags", "field:tags.add", "button:tags.add"] {
        assert!(has(&t, id), "{id}");
    }
    key(&mut t, "Ctrl+T");
    assert_eq!(t.d.state().pane, None);
}

/// Two tags typed into a selection of 8 (three of which already hold "beach"), the counts, × and
/// Undo.
#[test]
fn typed_tags_land_on_the_selection_with_counts_and_x_removes_one_until_undone() {
    let mut t = setup(1400, 900);
    let three: Vec<u64> = t.ids.iter().take(3).copied().collect();
    run(&mut t, "photo.tags.add", json!({ "ids": three, "tags": "beach" }));
    select(&mut t, 8);
    run(&mut t, "pane.tags", json!({}));
    assert_eq!(rows(&t), ["beach (3 of 8)"]);

    type_into(&mut t, "field:tags.add", "lisbon, tram");
    assert_eq!(t.d.state().tags_pane.draft, "lisbon, tram");
    key(&mut t, "Enter");
    assert_eq!(t.d.state().tags_pane.draft, "", "the field empties");
    assert_eq!(rows(&t), ["beach (3 of 8)", "lisbon", "tram"]);
    assert_eq!(tags_of(&t, t.ids[7]), ["lisbon", "tram"]);
    assert_eq!(tags_of(&t, t.ids[8]), Vec::<String>::new(), "only the selection");
    assert_eq!(t.d.app().session().unwrap().library().tag_index().count("tram"), 8, "one command for the selection, with both tags");
    // The field keeps the focus after Enter, ready for the next tag.
    assert!(t.d.widgets().iter().any(|w| w.id == "field:tags.add" && w.active), "focused");

    // × shows on a hovered row and removes the tag from all eight.
    assert!(!has(&t, "tag:tram/remove"), "× only on hover");
    t.d.hover("tag:tram", 0).unwrap();
    assert!(has(&t, "tag:tram/remove"));
    t.d.click("tag:tram/remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(rows(&t), ["beach (3 of 8)", "lisbon"]);
    assert!(t.d.app().session().unwrap().library().tag_index().count("tram") == 0);

    // Undo puts it back on all eight, in one step.
    key(&mut t, "Ctrl+Z");
    assert_eq!(rows(&t), ["beach (3 of 8)", "lisbon", "tram"]);
    assert_eq!(t.d.app().session().unwrap().library().tag_index().count("tram"), 8);
    key(&mut t, "Ctrl+Z");
    assert_eq!(rows(&t), ["beach (3 of 8)"], "the next undo takes back both typed tags");

    // + adds too; with nothing selected the field's text stays and the status line says why.
    run(&mut t, "select.none", json!({}));
    assert_eq!(rows(&t), Vec::<String>::new());
    assert!(has(&t, "field:tags.add"));
    type_into(&mut t, "field:tags.add", "harbour");
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:tags.add").unwrap().enabled, "+ waits for a selection");
    key(&mut t, "Enter");
    assert_eq!(t.d.state().tags_pane.draft, "harbour", "kept");
    assert_eq!(t.d.state().status, "Select some photos first.");
    select(&mut t, 2);
    t.d.click("button:tags.add").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(rows(&t), ["beach", "harbour"]);
}

/// The list under "Add a tag:" offers the tags in use; ↓ highlights, Enter adds the highlighted
/// one, Esc closes the list.
#[test]
fn the_autocomplete_offers_existing_tags() {
    let mut t = setup(1400, 900);
    let ids = t.ids.clone();
    run(&mut t, "photo.tags.add", json!({ "ids": [ids[9]], "tags": "tram, Trees, beach" }));
    select(&mut t, 4);
    run(&mut t, "pane.tags", json!({}));
    type_into(&mut t, "field:tags.add", "tr");
    let options: Vec<(String, String)> =
        t.d.widgets()
            .into_iter()
            .filter(|w| w.role == Role::Suggestion && w.id.starts_with("field:tags.add/option/"))
            .map(|w| (w.id, w.label))
            .collect();
    assert_eq!(options.iter().map(|o| o.1.as_str()).collect::<Vec<_>>(), ["tram", "Trees"]);
    assert_eq!(t.d.state().tags_pane.highlight, None, "nothing highlighted until ↓");
    key(&mut t, "Down");
    key(&mut t, "Down");
    assert_eq!(t.d.state().tags_pane.highlight, Some(1));
    key(&mut t, "Up");
    assert_eq!(t.d.state().tags_pane.highlight, Some(0));
    key(&mut t, "Enter");
    assert_eq!(rows(&t), ["tram"], "the highlighted tag, added to the four");
    assert_eq!(t.d.state().tags_pane, Default::default());

    // After a comma the last piece completes, keeping what came before.
    type_into(&mut t, "field:tags.add", "harbour, BE");
    let labels: Vec<String> = t.d.widgets().into_iter().filter(|w| w.role == Role::Suggestion).map(|w| w.label).collect();
    assert_eq!(labels, ["harbour, beach"]);
    // Esc closes the list and keeps the text; Enter then adds the text as typed.
    key(&mut t, "Esc");
    assert!(!t.d.widgets().iter().any(|w| w.role == Role::Suggestion));
    assert_eq!(t.d.state().tags_pane.draft, "harbour, BE");
    t.d.type_text("ACH").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.widgets().iter().any(|w| w.role == Role::Suggestion), "new text: the list again");
    key(&mut t, "Esc");
    key(&mut t, "Enter");
    assert_eq!(rows(&t), ["beach", "harbour", "tram"], "BEACH took the spelling in use");
}

/// Rename Tag… from the tag menu: the dialog says what will happen, the new name merges onto an
/// existing tag once Rename is pressed, and one Undo takes it all back.
#[test]
fn rename_tag_merges_after_confirming() {
    let mut t = setup(1400, 900);
    let ids = t.ids.clone();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..4], "tags": "tram" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[2..6], "tags": "Lisbon" }));
    select(&mut t, 6);
    run(&mut t, "pane.tags", json!({}));
    assert_eq!(rows(&t), ["Lisbon (4 of 6)", "tram (4 of 6)"]);

    // The tag menu: Add to All Selected, Search for This Tag, Remove from Selected,
    // Rename Tag….
    right_click(&mut t, "tag:tram");
    let items: Vec<(String, bool)> = t.d.widgets().into_iter().filter(|w| w.role == Role::MenuItem).map(|w| (w.label, w.enabled)).collect();
    assert_eq!(
        items,
        [
            ("Add to All Selected".to_string(), true),
            ("Search for This Tag".to_string(), true),
            ("Remove from Selected".to_string(), true),
            ("Rename Tag…".to_string(), true),
        ]
    );
    t.d.click("menuitem:tag.rename").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::RenameTag));
    assert!(has(&t, "dialog:renameTag") && has(&t, "field:dialog.to"));
    assert!(t.d.widgets().iter().any(|w| w.id == "field:dialog.to" && w.active), "the field has the focus");
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:dialog.ok").unwrap().enabled, "unchanged: Rename greyed");
    let said = t.d.app().state().dialogs.last().cloned().unwrap();
    assert_eq!(dalica_ui::dialogs::rename_tag::texts(t.d.app(), &said).0, "Rename “tram” on 4 photos to:");

    // Typing replaces the selected name; the dialog says the two will merge.
    t.d.type_text("lisbon").unwrap();
    t.d.settle(FRAMES).unwrap();
    let draft = t.d.app().state().dialogs.last().cloned().unwrap();
    assert_eq!(draft.fields["to"], json!("lisbon"));
    assert_eq!(
        dalica_ui::dialogs::rename_tag::texts(t.d.app(), &draft).1.as_deref(),
        Some("“Lisbon” is already on 4 photos: the two tags will become one.")
    );
    check_ids(&t.d, (1400, 900));
    key(&mut t, "Enter");
    assert!(t.d.state().dialogs.is_empty(), "renamed: the dialog closes");
    assert_eq!(t.d.state().status, "", "Enter answered once: nothing refused");
    assert_eq!(rows(&t), ["Lisbon"], "merged, in the spelling in use");
    let lib = t.d.app().session().unwrap().library();
    assert_eq!((lib.tag_index().count("tram"), lib.tag_index().count("lisbon")), (0, 6));
    assert_eq!(tags_of(&t, ids[2]), ["Lisbon"], "a photo that held both keeps one");

    // One undo puts both tags back exactly.
    key(&mut t, "Ctrl+Z");
    assert_eq!(rows(&t), ["Lisbon (4 of 6)", "tram (4 of 6)"]);
    assert_eq!(tags_of(&t, ids[2]), ["Lisbon", "tram"]);

    // Cancel renames nothing.
    run(&mut t, "tag.rename", json!({ "tag": "tram" }));
    t.d.type_text("bus").unwrap();
    key(&mut t, "Esc");
    assert!(t.d.state().dialogs.is_empty());
    assert_eq!(rows(&t), ["Lisbon (4 of 6)", "tram (4 of 6)"]);

    // Remove from Selected and Add to All Selected act on the six.
    right_click(&mut t, "tag:tram");
    t.d.click("menuitem:photo.tags.add").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(rows(&t), ["Lisbon (4 of 6)", "tram"]);
    right_click(&mut t, "tag:Lisbon");
    t.d.click("menuitem:photo.tags.remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(rows(&t), ["tram"]);
}

/// Every widget shown has a valid id, registered once, inside the window.
fn check_ids(d: &Driver, (w, h): (u32, u32)) {
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for wi in d.widgets() {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.clone()), "{w}×{h}: `{}` is registered twice", wi.id);
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the region widget `id` covers in the last frame against `tests/fingerprints/<name>.json`,
/// and writes it and the whole window to the test output.
fn check_region(d: &Driver, id: &str, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let r = d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}")).rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
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

/// Writes a screenshot of the whole window to `$DALICA_SHOTS/<name>.png`, when asked.
fn shot(d: &Driver, name: &str) {
    let Some(dir) = std::env::var_os("DALICA_SHOTS") else { return };
    let s = d.screenshot().unwrap();
    std::fs::write(Path::new(&dir).join(format!("{name}.png")), fingerprint::png_rgba(s.width, s.height, &s.rgba)).unwrap();
}

/// The pane with a partial tag, a whole one and a hovered row's ×, and the Rename Tag dialog
/// merging, at both window sizes.
#[test]
fn the_pane_and_the_rename_dialog_match_their_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let mut t = setup(w, h);
        let ids = t.ids.clone();
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon, tram" }));
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..3], "tags": "beach" }));
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[5..8], "tags": "a much longer tag that has to be cut short in the pane" }));
        select(&mut t, 8);
        run(&mut t, "pane.tags", json!({}));
        assert_eq!(t.d.state().pane, Some(Pane::Tags), "{w}×{h}");
        check_ids(&t.d, (w, h));
        check_region(&t.d, "pane:tags", &format!("pane-tags-{w}x{h}"));
        shot(&t.d, &format!("pane-tags-{w}x{h}"));
        t.d.hover("tag:tram", 0).unwrap();
        check_ids(&t.d, (w, h));
        shot(&t.d, &format!("pane-tags-hover-{w}x{h}"));

        run(&mut t, "tag.rename", json!({ "tag": "tram" }));
        t.d.type_text("Lisbon").unwrap();
        t.d.settle(FRAMES).unwrap();
        check_ids(&t.d, (w, h));
        check_region(&t.d, "dialog:renameTag", &format!("dialog-rename-tag-{w}x{h}"));
        shot(&t.d, &format!("dialog-rename-tag-{w}x{h}"));
    }
}
