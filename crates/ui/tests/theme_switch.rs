//! Switching themes in the window (M16.5; `design-system.md` §2 Themes, `menus-and-shortcuts.md`
//! §1.3 and §5 General, `control-protocol.md` §6.5; `crates/ui/src/theme.rs`), headless and by id:
//! View ▸ Theme ▸ puts a theme on the window in the next frame and checks its row; `ui.set {theme}`
//! does the same and refuses an id it does not know; the Options row previews, Cancel reverts and
//! Apply keeps it; a font size change keeps the theme; the theme survives a restart; the engine's
//! themes, `ui-kit`'s and the submenu's rows are one list; at 900 × 600 the submenu stays inside
//! the window. The switch's frame is timed and printed (`--nocapture`).

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::common::{FRAMES, LARGE, SMALL, new_driver, shot, widget};
use dalica_engine::Session;
use dalica_engine::settings::{THEME_COMMANDS, THEMES};
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::menu_catalog::{self, Entry};
use dalica_ui::{App, Driver};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::theme::ThemeKind;
use serde_json::{Value, json};

const MENU: &str = "menu:View";
const SUBMENU: &str = "submenu:View/Theme";
const OPTION: &str = "choice:option.interface.theme";

struct T {
    control: Control,
    conn: Connection,
    d: Driver,
}

/// A library in `root` (one folder of scenes photos, scanned) in a window of `size` points.
fn open(root: &Path, size: (u32, u32)) -> T {
    let library = root.join("library");
    let fresh = !library.exists();
    let mut s = Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    if fresh {
        let files = crate::common::demo_library(1, 4, 7).unwrap();
        for f in &files {
            let path = root.join("photos").join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &f.bytes).unwrap();
        }
        let folder = root.join("photos").join(&files[0].path).parent().unwrap().to_path_buf();
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
    }
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    T { control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Result<Value, dalica_ui::control::Error> {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

/// The theme the context's tokens wear.
fn worn(d: &Driver) -> ThemeKind {
    Tokens::get(d.ctx()).kind
}

/// The saved `interface.theme`.
fn saved(d: &Driver) -> &'static str {
    d.app().session().unwrap().settings().interface().theme
}

/// The View ▸ Theme ▸ rows that are checked, by command id.
fn checked(t: &mut T) -> Vec<String> {
    let tree = call(t, "ui.menu.tree", json!({})).unwrap();
    let view = tree.as_array().unwrap().iter().find(|m| m["widget"] == MENU).unwrap().clone();
    let theme = view["children"].as_array().unwrap().iter().find(|n| n["label"] == "Theme").unwrap().clone();
    theme["children"].as_array().unwrap().iter().filter(|n| n["checked"] == true).map(|n| n["id"].as_str().unwrap().to_string()).collect()
}

/// Opens View ▸ Theme ▸.
fn open_submenu(d: &mut Driver) {
    d.click(MENU).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(SUBMENU).unwrap();
    d.settle(FRAMES).unwrap();
}

#[test]
fn the_engines_themes_ui_kits_and_the_submenus_rows_are_one_list() {
    assert!(dalica_ui::theme::same_themes(), "settings::THEMES and ThemeKind::ALL");
    let rows: Vec<(&str, &str)> = menu_catalog::THEME
        .iter()
        .map(|e| match e {
            Entry::Item(i) => {
                assert_eq!(i.mark, menu_catalog::Mark::Radio, "{}", i.label);
                (i.id, i.label)
            }
            other => panic!("only rows in View ▸ Theme ▸: {other:?}"),
        })
        .collect();
    let kinds: Vec<(String, &str)> = ThemeKind::ALL.iter().map(|k| (format!("theme.{}", k.id()), k.label())).collect();
    assert_eq!(rows.iter().map(|(id, l)| (id.to_string(), *l)).collect::<Vec<_>>(), kinds);
    let engine: Vec<(&str, String)> = THEME_COMMANDS.iter().map(|(id, menu)| (*id, menu.replace("View/Theme/", ""))).collect();
    assert_eq!(engine, rows.iter().map(|(id, l)| (*id, l.to_string())).collect::<Vec<_>>(), "the commands' menu paths");
    assert_eq!(THEMES.len(), ThemeKind::ALL.len());
    // An id `ui-kit` does not know reads as Dark.
    assert_eq!(dalica_ui::theme::kind("noSuchTheme"), ThemeKind::Dark);
}

#[test]
fn view_theme_switches_in_the_next_frame_checks_its_row_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = open(dir.path(), LARGE);
    assert_eq!(worn(&t.d), ThemeKind::Dark, "Dark by default");
    assert_eq!(checked(&mut t), ["theme.dark"]);
    // Ordinary frames, to compare the switch's frame with.
    let calm = (0..20)
        .map(|_| {
            let at = Instant::now();
            t.d.step();
            at.elapsed()
        })
        .max()
        .unwrap();
    open_submenu(&mut t.d);
    t.d.click("menuitem:theme.proLight").unwrap();
    let at = Instant::now();
    t.d.step();
    let switch = at.elapsed();
    eprintln!("theme switch: the switch's frame {switch:?}, the slowest of 20 ordinary frames {calm:?}");
    assert_eq!(worn(&t.d), ThemeKind::ProLight, "in the next frame");
    assert_eq!(*t.d.app().tokens(), Tokens::for_kind(ThemeKind::ProLight), "the app's tokens too");
    assert_eq!(saved(&t.d), "proLight");
    t.d.settle(FRAMES).unwrap();
    assert_eq!(checked(&mut t), ["theme.proLight"]);
    assert_eq!(call(&mut t, "ui.state", json!(null)).unwrap()["theme"], "proLight");
    shot(&t.d, "theme-switch-pro-light-1400x900");
    drop(t);
    // A restart opens in it.
    let t = open(dir.path(), LARGE);
    assert_eq!(worn(&t.d), ThemeKind::ProLight, "after a restart");
}

#[test]
fn ui_set_theme_runs_its_command_and_an_unknown_id_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = open(dir.path(), LARGE);
    let state = call(&mut t, "ui.state", json!(null)).unwrap();
    assert_eq!(state["theme"], "dark");
    // The theme saved already runs nothing.
    call(&mut t, "ui.set", json!({ "theme": "dark" })).unwrap();
    assert_eq!((worn(&t.d), saved(&t.d)), (ThemeKind::Dark, "dark"));
    call(&mut t, "ui.set", json!({ "theme": "classic" })).unwrap();
    assert_eq!(worn(&t.d), ThemeKind::Classic, "in the next frame");
    assert_eq!(saved(&t.d), "classic");
    t.d.settle(FRAMES).unwrap();
    assert_eq!(checked(&mut t), ["theme.classic"]);
    for bad in [json!("noSuchTheme"), json!("Classic"), json!(""), json!(7), json!(null)] {
        let caption = t.d.state().caption;
        let e = call(&mut t, "ui.set", json!({ "theme": bad, "caption": "filename" })).unwrap_err();
        assert_eq!(e.code, dalica_ui::control::Code::BadParams, "{bad}: {e:?}");
        assert_eq!((worn(&t.d), saved(&t.d), t.d.state().caption), (ThemeKind::Classic, "classic", caption), "{bad}: nothing changed");
    }
}

#[test]
fn the_options_row_previews_cancel_reverts_and_apply_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = open(dir.path(), LARGE);
    let d = &mut t.d;
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(OPTION).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(&format!("{OPTION}/studio")).unwrap();
    d.step();
    assert_eq!(worn(d), ThemeKind::Studio, "previewed in the next frame");
    assert_eq!(saved(d), "dark", "not saved before Apply");
    d.settle(FRAMES).unwrap();
    shot(d, "theme-switch-options-preview-1400x900");
    d.key("Esc").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().options.is_none());
    assert_eq!((worn(d), saved(d)), (ThemeKind::Dark, "dark"), "Cancel reverts");

    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(OPTION).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(&format!("{OPTION}/breezeDark")).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(worn(d), ThemeKind::BreezeDark);
    d.click("button:dialog.apply").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((worn(d), saved(d)), (ThemeKind::BreezeDark, "breezeDark"), "Apply keeps it");
    d.key("Esc").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().options.is_none());
    assert_eq!(worn(d), ThemeKind::BreezeDark, "and closing the dialog after Apply keeps it");
}

#[test]
fn a_font_size_change_after_a_theme_change_keeps_the_theme() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = open(dir.path(), LARGE);
    t.d.run("theme.adwaitaDark", &json!({})).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(worn(&t.d), ThemeKind::AdwaitaDark);
    t.d.run("options.set", &json!({ "values": { "interface.fontSize": "large" } })).unwrap();
    t.d.settle(FRAMES).unwrap();
    let tokens = Tokens::get(t.d.ctx());
    assert_eq!(tokens.kind, ThemeKind::AdwaitaDark, "the theme is kept");
    assert!((tokens.body - 17.0).abs() < 1e-3, "at the Large size: {}", tokens.body);
    // And a theme change keeps the font size, never compounding it.
    t.d.run("theme.solarizedDark", &json!({})).unwrap();
    t.d.settle(FRAMES).unwrap();
    let tokens = Tokens::get(t.d.ctx());
    assert_eq!(tokens.kind, ThemeKind::SolarizedDark);
    assert_eq!(tokens, Tokens::for_kind(ThemeKind::SolarizedDark).with_font_size(dalica_ui_kit::theme::FontSize::Large));
}

#[test]
fn at_900_by_600_the_submenu_stays_inside_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = open(dir.path(), SMALL);
    open_submenu(&mut t.d);
    let rows: Vec<[f32; 4]> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:theme.")).map(|w| w.rect).collect();
    assert_eq!(rows.len(), ThemeKind::ALL.len(), "every theme's row is drawn");
    let (w, h) = (SMALL.0 as f32, SMALL.1 as f32);
    for r in &rows {
        assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= w + 0.5 && r[1] + r[3] <= h + 0.5, "{r:?} outside {w} × {h}");
    }
    assert!(widget(&t.d, "menuitem:theme.dark").active, "the theme in effect is checked");
    shot(&t.d, "theme-switch-submenu-900x600");
}
