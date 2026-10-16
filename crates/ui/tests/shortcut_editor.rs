//! Options ▸ Keyboard Shortcuts, the shortcut editor (`menus-and-shortcuts.md` §2.0), headless and
//! by automation id, on a temp library of one folder with two photos:
//!
//! - Ctrl+/ opens the dialog on this tab; a search for "ctrl+1" lists both Ctrl+1 bindings and the
//!   Library filter hides the edit view's;
//! - F8 recorded on Star and applied shows in the Picture menu and stars the selected photo, and
//!   Ctrl+8 no longer does; Esc cancels recording, Backspace removes a key;
//! - Ctrl+1 recorded on Star in the Library asks, and Replace takes it from Small Thumbnails;
//! - Cancel before Apply changes nothing; Reset All restores every default and the ↺ marks go;
//! - the editor (and its conflict) match their fingerprints at 1400 × 900 and fit 900 × 600.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, check_small_and_back, new_driver, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::{Context, Key, Session};
use dalica_ui::dialogs::options::shortcuts::{
    CONFLICT_ID, CONTEXT_ID, REPLACE_ID, RESET_CONFIRM_ID, RESET_ID, RESET_QUESTION, RESET_QUESTION_ID, SEARCH_ID, cell_id, restore_id, row_id,
};
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::state::OptionsTab;
use dalica_ui::{App, Driver};
use serde_json::json;

const DIALOG: &str = "dialog:options";
const APPLY: &str = "button:dialog.apply";
const CANCEL: &str = "button:dialog.cancel";
const STAR: &str = "photo.star.toggle";

/// "Harbour" with a.jpg and b.jpg, in a 1400 × 900 window; returns a.jpg's id.
fn fixture() -> (tempfile::TempDir, Driver, PhotoId) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Photos").join("Harbour");
    crate::common::put_jpeg(&folder.join("a.jpg"), 1);
    crate::common::put_jpeg(&folder.join("b.jpg"), 2);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(dir.path().join("Pictures")));
    s.wait_idle();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let a = s.library().photos().find(|p| p.name == "a.jpg").unwrap().id;
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    (dir, d, a)
}

/// Opens the editor (Help ▸ Keyboard Shortcuts…) with `search` in its search field.
fn open(d: &mut Driver, search: &str) {
    d.run("app.shortcuts", &json!({})).unwrap();
    d.app_mut().state_mut().options.as_mut().unwrap().shortcuts.search = search.into();
    d.settle(FRAMES).unwrap();
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

fn key(d: &mut Driver, chord: &str) {
    d.key(chord).unwrap();
    d.settle(FRAMES).unwrap();
}

fn label(d: &Driver, id: &str) -> String {
    widget(d, id).label
}

/// `key` as this platform shows it (`⌘8` on macOS).
fn shown(key: &str) -> String {
    dalica_ui_kit::widgets::key_text(key, dalica_engine::keymap::Platform::current() == dalica_engine::keymap::Platform::Mac)
}

fn star_cell(slot: u8) -> String {
    cell_id(STAR, Context::Library, slot)
}

/// The keys of `command` in `context` in the session's keymap, as it writes them.
fn keys(d: &Driver, command: &str, context: Context) -> Vec<String> {
    d.app().session().unwrap().keymap().keys_for(command, context).iter().map(Key::to_string).collect()
}

/// The dialog matches `tests/fingerprints/<name>-1400x900.json` and fits 900 × 600. The baselines
/// show the keys as Windows and Linux write them; on macOS the caps are glyphs (`⌘8`), so there
/// only the fit is checked.
fn check_dialog(d: &mut Driver, name: &str) {
    if cfg!(target_os = "macos") {
        check_small_and_back(d, DIALOG);
    } else {
        check_sizes(d, DIALOG, name);
    }
}

fn starred(d: &Driver, id: PhotoId) -> bool {
    d.app().session().unwrap().library().photo(id).is_some_and(|p| p.star)
}

#[test]
fn ctrl_slash_opens_the_editor_where_ctrl_1_lists_both_bindings_and_the_library_filter_hides_the_edit_view() {
    let (_dir, mut d, _) = fixture();
    key(&mut d, "Ctrl+/");
    assert_eq!(d.state().options.as_ref().map(|o| o.tab), Some(OptionsTab::Shortcuts));
    assert!(widget(&d, "tab:options.shortcuts").active);
    click(&mut d, SEARCH_ID);
    d.type_text("ctrl+1").unwrap();
    d.settle(FRAMES).unwrap();
    let (small, back) = (row_id("view.thumbsSmall", Context::Library), row_id("view.library", Context::EditView));
    assert_eq!(label(&d, &small), format!("View › Small Thumbnails, Library: {}", shown("Ctrl+1")));
    assert!(label(&d, &back).contains(&format!(", Edit view: {}", shown("Ctrl+1"))), "{}", label(&d, &back));
    click(&mut d, CONTEXT_ID);
    click(&mut d, &format!("{CONTEXT_ID}/library"));
    assert_eq!(d.state().options.as_ref().unwrap().shortcuts.context, Some(Context::Library));
    assert!(try_widget(&d, &small).is_some() && try_widget(&d, &back).is_none(), "the edit view's row is hidden");
}

#[test]
fn f8_recorded_on_star_and_applied_stars_the_photo_and_ctrl_8_no_longer_does() {
    let (_dir, mut d, a) = fixture();
    open(&mut d, "star");
    // Esc cancels recording: the dialog stays open and nothing changes.
    click(&mut d, &star_cell(0));
    assert_eq!(label(&d, &star_cell(0)), "Press the new shortcut…");
    key(&mut d, "Esc");
    assert!(try_widget(&d, DIALOG).is_some(), "Esc went to the cell, not the dialog");
    assert_eq!(label(&d, &star_cell(0)), shown("Ctrl+8"));
    assert!(!widget(&d, APPLY).enabled);
    // F8 replaces Ctrl+8; Backspace removes F4.
    click(&mut d, &star_cell(0));
    key(&mut d, "F8");
    assert_eq!(label(&d, &star_cell(0)), shown("F8"));
    click(&mut d, &star_cell(1));
    key(&mut d, "Backspace");
    assert_eq!(label(&d, &star_cell(1)), "No shortcut");
    assert!(try_widget(&d, &restore_id(STAR, Context::Library)).is_some(), "the row differs from its defaults");
    assert_eq!(keys(&d, STAR, Context::Library), ["Ctrl+8", "F4"], "nothing applies before Apply");
    click(&mut d, APPLY);
    assert_eq!(keys(&d, STAR, Context::Library), ["F8"]);
    assert_eq!(keys(&d, STAR, Context::EditView), ["Ctrl+8", "Space"], "the edit view keeps its own");
    let bar = menus::bar(d.app(), Platform::current());
    let item = menus::find(&bar, &Lookup::Path(vec!["Picture".into(), "Star / Unstar".into()])).unwrap();
    assert_eq!(item.shortcut.as_deref(), Some("F8"), "the menu shows the key in effect at once");
    click(&mut d, CANCEL);
    assert!(d.state().options.is_none());
    // F8 on the selected photo stars it; Ctrl+8 does nothing now.
    click(&mut d, &format!("thumb:{a}"));
    key(&mut d, "F8");
    assert!(starred(&d, a));
    key(&mut d, "Ctrl+8");
    assert!(starred(&d, a), "Ctrl+8 no longer unstars");
    key(&mut d, "F8");
    assert!(!starred(&d, a));
}

#[test]
fn ctrl_1_on_star_asks_and_replace_takes_it_from_small_thumbnails() {
    let (_dir, mut d, _) = fixture();
    open(&mut d, "star");
    check_dialog(&mut d, "dialog-options-shortcuts");
    click(&mut d, &star_cell(0));
    key(&mut d, "Ctrl+1");
    assert_eq!(label(&d, CONFLICT_ID), format!("{} is already Small Thumbnails in Library.", shown("Ctrl+1")));
    assert!(d.state().options.as_ref().unwrap().shortcuts.changes.is_empty(), "nothing changes until Replace");
    check_dialog(&mut d, "dialog-options-shortcuts-conflict");
    click(&mut d, REPLACE_ID);
    assert!(try_widget(&d, CONFLICT_ID).is_none());
    assert_eq!(label(&d, &star_cell(0)), shown("Ctrl+1"));
    d.app_mut().state_mut().options.as_mut().unwrap().shortcuts.search = "small thumbnails".into();
    d.settle(FRAMES).unwrap();
    assert_eq!(label(&d, &cell_id("view.thumbsSmall", Context::Library, 0)), "No shortcut");
    click(&mut d, APPLY);
    let resolved = d.app().session().unwrap().keymap().resolve(&Key::parse("Ctrl+1").unwrap(), Context::Library, false);
    assert_eq!(resolved.as_deref(), Some(STAR));
    assert!(keys(&d, "view.thumbsSmall", Context::Library).is_empty());
    assert!(keys(&d, "view.library", Context::EditView).contains(&"Ctrl+1".to_string()), "another view keeps its Ctrl+1");
}

#[test]
fn cancel_before_apply_changes_nothing_and_reset_all_restores_every_default() {
    let (_dir, mut d, _) = fixture();
    open(&mut d, "star");
    click(&mut d, &star_cell(0));
    key(&mut d, "F8");
    assert!(widget(&d, APPLY).enabled);
    click(&mut d, CANCEL);
    assert!(d.app().session().unwrap().keymap().overrides().is_empty(), "Cancel applied nothing");
    open(&mut d, "star");
    assert_eq!(label(&d, &star_cell(0)), shown("Ctrl+8"));
    click(&mut d, CANCEL);
    // Two bindings changed earlier (as an agent would): the editor marks them, Reset All undoes both.
    d.run("app.setShortcut", &json!({ "command": STAR, "context": "library", "key": "F8" })).unwrap();
    d.run("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": null })).unwrap();
    open(&mut d, "");
    d.app_mut().state_mut().options.as_mut().unwrap().shortcuts.search = "star".into();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, &restore_id(STAR, Context::Library)).is_some());
    click(&mut d, RESET_ID);
    assert_eq!(label(&d, RESET_QUESTION_ID), RESET_QUESTION);
    click(&mut d, RESET_CONFIRM_ID);
    assert!(try_widget(&d, &restore_id(STAR, Context::Library)).is_none(), "the ↺ marks go");
    assert_eq!(label(&d, &star_cell(0)), shown("Ctrl+8"));
    click(&mut d, APPLY);
    assert!(d.app().session().unwrap().keymap().overrides().is_empty(), "every default is back");
    assert_eq!(keys(&d, "view.thumbsSmall", Context::Library), ["Ctrl+1"]);
}
