//! Options ▸ General and the dialog's OK / Cancel / Apply (`design-system.md` §11.3,
//! `menus-and-shortcuts.md` §5 General), headless and by automation id: Apply's state follows the
//! draft (a change put back disables it again); an applied change survives Cancel, an unapplied one
//! does not; a planted write failure shows its error in the dialog and keeps the draft; Large font
//! previews at once and reverts on Cancel; every option of the tab that can be changed reaches
//! `options.set` under its key; options whose feature is still to come are drawn disabled with
//! their reason; the tab matches its fingerprint at 1400 × 900 and fits 900 × 600.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_sizes, new_driver, shot, try_widget, widget};
use dalica_engine::Session;
use dalica_ui::dialogs::options::{ERROR_ID, general};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const DIALOG: &str = "dialog:options";
const APPLY: &str = "button:dialog.apply";
const CANCEL: &str = "button:dialog.cancel";
const TOOLTIPS: &str = "check:option.interface.tooltips";
const ANIMATIONS: &str = "check:option.interface.animations";
const FONT: &str = "choice:option.interface.fontSize";

struct Fx {
    dir: tempfile::TempDir,
    d: Driver,
}

/// An empty library in a 1400 × 900 window, Options open on General.
fn open() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    Fx { dir, d }
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
    for key in ["interface.language", "import.detectDuplicates", "files.readLegacyData", "import.folder"] {
        let row = rows.iter().find(|r| r.key == key).unwrap();
        assert!(row.reason.as_deref().is_some_and(|r| r.starts_with("Arrives with")), "{key}: {:?}", row.reason);
    }
}

#[test]
fn the_general_tab_matches_its_fingerprint_and_fits_the_small_window() {
    let mut fx = open();
    check_sizes(&mut fx.d, DIALOG, "dialog-options-general");
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
