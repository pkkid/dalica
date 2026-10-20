//! The languages, the pseudo-locale and keyboard-only use in the live app (`m15.rs` part 2;
//! `roadmap.md` M15 → Localisation and accessibility's *Done when*), driven over the control
//! channel of the real `dalica --headless` on a library of five painted photos (`one.jpg`,
//! `pair-1.jpg`, `pair-2.jpg`, `other-1.jpg`, `other-2.jpg` in `Garden`):
//!
//! - **English and Thai**, switched with Options ▸ General ▸ Language (`interface.language`) and no
//!   restart: the menu bar, the library, the edit view and the Options dialog draw Thai; the search
//!   banner's plurals read right for 0, 1, 2 and 5 pictures in each language (Thai's one form the
//!   same sentence for every count); switching in the edit view keeps the view, the photo, the
//!   selection and the undo history (Undo in Thai takes back the star given in English); at
//!   900 × 600 every menu title stays inside the window. Fingerprints at 1400 × 900: the library
//!   in English (`m15-1400x900-library-en`) and, in a build with the fallback faces
//!   (`CRAFT_FONTS_DIR`, as the review's run and every release build have), in Thai
//!   (`m15-1400x900-library-th`); without them that one is skipped and says so.
//! - **The pseudo-locale** `qps`: in the library, the edit view and the Options dialog every
//!   label a widget carries (its AccessKit name) is bracketed, but user data (the folder's and the
//!   photos' names, the folder's path), the automation landmarks (`region:*`, whose AccessKit
//!   names are translated separately) and the languages' own names.
//! - **Keyboard only**: F10 opens the menu bar and the arrows walk it, Esc closes it; Ctrl+,
//!   opens Options, and Tab walks its controls without leaving the dialog and comes round again;
//!   Esc closes it.
//!
//! The AccessKit audit (no interactive node without a name in any view or dialog) and the
//! exhaustive keyboard walk (every enabled menu row, every command reachable without a pointer)
//! read the tree egui hands a screen reader, which only an in-process frame has: they run as
//! `dalica-ui`'s `accessibility` and `keyboard_only` suites in `m15_checks.rs`, beside the
//! catalogs' cut-and-wrap sweeps at both sizes (`i18n_window`, `i18n_dialogs`, `i18n_views`,
//! `i18n_languages`).

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

use crate::app::{self, click, idle, key, resize, run, widget};
use crate::common::{self, COLOURS, LARGE, SMALL, steady_shot};
use crate::live::{self, Client, Launch};

/// The photos' names, in `Garden`.
const NAMES: [&str; 5] = ["one.jpg", "pair-1.jpg", "pair-2.jpg", "other-1.jpg", "other-2.jpg"];

fn has_thai(s: &str) -> bool {
    s.chars().any(|c| ('\u{0E01}'..='\u{0E5B}').contains(&c))
}

fn set_language(c: &mut Client, code: &str) {
    let r = c.exec("options.set", json!({ "values": { "interface.language": code } }));
    assert_eq!(r["restart"], json!([]), "{code}: no restart: {r}");
    idle(c);
}

/// The search banner's sentence before its time and size, for a search of `text`.
fn banner(c: &mut Client, text: &str) -> String {
    c.exec("search.set", json!({ "text": text }));
    idle(c);
    let label = app::label(c, "banner:search");
    label.split(" (").next().unwrap_or_default().replace('\u{2009}', "")
}

/// The labels of the widgets drawn now with a letter in them, by id.
fn labels(c: &mut Client) -> Vec<(String, String)> {
    c.widgets("")
        .iter()
        .filter_map(|w| Some((w["id"].as_str()?.to_string(), w["label"].as_str()?.to_string())))
        .filter(|(_, l)| l.chars().any(char::is_alphabetic))
        .collect()
}

/// The widgets whose labels are neither bracketed by the pseudo-locale nor `user` data, landmarks or
/// the languages' names.
fn untranslated(c: &mut Client, user: &BTreeSet<String>) -> Vec<(String, String)> {
    labels(c)
        .into_iter()
        .filter(|(id, l)| {
            !dalica_ui::i18n::pseudo::is_marked(l)
                && !id.starts_with("region:")
                && !id.starts_with("choice:option.interface.language")
                && !user.contains(l)
                && !user.iter().any(|u| l.starts_with(&format!("{u} (")))
        })
        .collect()
}

pub fn languages(exe: &Path, home: &Path) {
    let garden = home.join("Garden");
    std::fs::create_dir_all(&garden).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        app::put(&garden.join(name), &common::painted_jpeg(320, 240, COLOURS[i]), i as u64);
    }
    let (app_, mut c) = live::start_with(exe, home, Launch::default().arg("--automation-write-root").arg(home));
    run(&mut c, "folders.add", json!({ "path": garden }));
    idle(&mut c);
    assert_eq!(c.state()["language"], "en");

    // --- The library in English, then the plurals in each language.
    app::park(&mut c, f64::from(LARGE.1));
    steady_shot(&mut c, home, "m15-1400x900-library-en", &[]);
    let english = [
        ("zzzz", "Showing 0 pictures in 0 folders"),
        ("one", "Showing 1 picture in 1 folder"),
        ("pair", "Showing 2 pictures in 1 folder"),
        ("jpg", "Showing 5 pictures in 1 folder"),
    ];
    for (text, said) in english {
        assert_eq!(banner(&mut c, text), said, "the banner for {text:?}");
    }
    set_language(&mut c, "th");
    let five = banner(&mut c, "jpg");
    assert!(has_thai(&five) && five.contains('5'), "{five}");
    for (text, n, folders) in [("zzzz", 0, 0), ("one", 1, 1), ("pair", 2, 1)] {
        let said = banner(&mut c, text);
        // Thai has one plural form: the sentence for 5 with the numbers changed.
        let expected = five.replacen('5', &n.to_string(), 1).replacen(" 1 ", &format!(" {folders} "), 1);
        assert_eq!(said, expected, "the Thai banner for {text:?}");
    }
    c.exec("search.clear", json!({}));
    idle(&mut c);
    for menu in ["menu:File", "menu:Edit", "menu:View", "menu:Folder", "menu:Picture", "menu:Create", "menu:Tools", "menu:Help"] {
        let l = app::label(&mut c, menu);
        assert!(has_thai(&l), "{menu} in Thai: {l:?}");
    }
    if dalica_ui_kit::fonts::CRAFT_FONTS.is_empty() {
        println!("  (built without CRAFT_FONTS_DIR: no Thai face, so m15-1400x900-library-th is skipped; the review's run has the faces)");
    } else {
        app::park(&mut c, f64::from(LARGE.1));
        steady_shot(&mut c, home, "m15-1400x900-library-th", &[]);
    }
    resize(&mut c, SMALL);
    for w in c.widgets("menu:") {
        let r: Vec<f64> = w["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(r[0] >= 0.0 && r[0] + r[2] <= f64::from(SMALL.0), "{} inside the window at 900 × 600: {w}", w["id"]);
    }
    resize(&mut c, LARGE);
    set_language(&mut c, "en");

    // --- Switching in the edit view keeps the view, the photo, the selection and the history.
    let ids: Vec<u64> = c.photos().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let first = ids[0];
    c.exec("select.set", json!({ "ids": [first] }));
    let starred = c.inspect(&json!(first))["star"].clone();
    run(&mut c, "photo.star.toggle", json!({ "ids": [first] }));
    run(&mut c, "view.edit", json!({}));
    let kept = |c: &mut Client| {
        let s = c.state();
        (s["view"].clone(), s["edit"]["photo"].clone(), s["selection"]["ids"].clone())
    };
    let before = kept(&mut c);
    assert_eq!(before, (json!("edit"), json!(first), json!([first])));
    let basic = app::label(&mut c, "tab:edit.basic");
    set_language(&mut c, "th");
    assert_eq!(kept(&mut c), before, "Thai: the view, the photo and the selection kept");
    let thai_basic = app::label(&mut c, "tab:edit.basic");
    assert!(has_thai(&thai_basic) && thai_basic != basic, "the edit view in Thai: {thai_basic:?}");
    run(&mut c, "app.preferences", json!({}));
    let tabs: Vec<String> = c.widgets("tab:options.").iter().filter_map(|w| w["label"].as_str().map(str::to_string)).collect();
    assert!(!tabs.is_empty() && tabs.iter().all(|t| has_thai(t)), "the Options dialog in Thai: {tabs:?}");
    key(&mut c, "Escape");
    assert_eq!(kept(&mut c), before, "the Options dialog closed, the edit view still there");
    set_language(&mut c, "en");
    assert_eq!(kept(&mut c), before, "English again: kept");
    set_language(&mut c, "th");
    // Back in the library, Undo takes back the star given in English, in Thai.
    key(&mut c, "Escape");
    run(&mut c, "edit.undo", json!({}));
    assert_eq!(c.inspect(&json!(first))["star"], starred, "Undo in Thai takes back the star given in English");
    set_language(&mut c, "en");

    // --- The pseudo-locale: nothing but user data left unbracketed.
    let mut user: BTreeSet<String> = NAMES.iter().map(|n| n.to_string()).collect();
    user.insert("Garden".into());
    user.insert(garden.to_string_lossy().into_owned());
    user.insert(std::fs::canonicalize(&garden).unwrap().to_string_lossy().into_owned());
    set_language(&mut c, "qps");
    let mut left: Vec<(String, String)> = Vec::new();
    left.extend(untranslated(&mut c, &user).into_iter().map(|(i, l)| (format!("library: {i}"), l)));
    run(&mut c, "view.edit", json!({}));
    left.extend(untranslated(&mut c, &user).into_iter().map(|(i, l)| (format!("edit view: {i}"), l)));
    key(&mut c, "Escape");
    run(&mut c, "app.preferences", json!({}));
    left.extend(untranslated(&mut c, &user).into_iter().map(|(i, l)| (format!("Options: {i}"), l)));
    key(&mut c, "Escape");
    set_language(&mut c, "en");
    assert!(left.is_empty(), "the pseudo-locale leaves no untranslated text outside user data: {left:#?}");

    // --- Keyboard only: the menu bar and a dialog's Tab order.
    keyboard(&mut c);
    live::quit(app_, c);
}

/// The open popup's kind and menu (`ui.inspect` → `popup`).
fn popup(c: &mut Client) -> (Value, Value) {
    let p = c.ok("ui.inspect", json!({}))["popup"].clone();
    (p["kind"].clone(), p["menu"].clone())
}

fn focused(c: &mut Client) -> Option<String> {
    c.ok("ui.inspect", json!({}))["focused"].as_str().map(str::to_string)
}

fn keyboard(c: &mut Client) {
    key(c, "F10");
    assert_eq!(popup(c), (json!("menu"), json!("File")), "F10 opens the menu bar's first menu");
    key(c, "ArrowRight");
    assert_eq!(popup(c), (json!("menu"), json!("Edit")), "the arrows walk the menu bar");
    key(c, "Escape");
    assert_eq!(popup(c).0, Value::Null, "Esc closes it");

    key(c, "Ctrl+,");
    assert_eq!(app::dialog(c).as_deref(), Some("options"), "Ctrl+, opens Options");
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..60 {
        key(c, "Tab");
        let f = focused(c).unwrap_or_else(|| panic!("Tab focuses a control: after {seen:?}"));
        assert!(widget(c, &f).is_some(), "the focused control {f} is drawn");
        assert_eq!(app::dialog(c).as_deref(), Some("options"), "Tab stays in the dialog");
        if seen.first() == Some(&f) {
            break;
        }
        seen.push(f);
    }
    assert!(seen.len() >= 3, "Tab walks the dialog's controls: {seen:?}");
    assert_eq!(focused(c).as_ref(), seen.first(), "and comes round again: {seen:?}");
    key(c, "Escape");
    assert_eq!(app::dialog(c), None, "Esc closes Options");
    let _ = click;
}
