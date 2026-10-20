//! The main window through the catalogs (M15.31, `localization.md` → Checking a translation),
//! headless in the pseudo-locale `qps`: in the library with the tree, a folder and an album
//! selected, the tray holding photos, every side pane open in turn and a notice showing, every text
//! a frame draws is bracketed except user data (folder, file, album and people's names, captions,
//! tags, paths), the keyboard shortcuts written beside menu rows (key names, as the keymap writes
//! them) and the map's attribution (a third party's credit, kept as it is); so are the menu bar,
//! each of its menus with each of their submenus, and the photo menu. At 1400 × 900 and 900 × 600
//! no label is cut at the side of the area it is drawn in. `DALICA_SHOTS=<dir>` writes the window
//! in the pseudo-locale there (`i18n-window-*.png`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, demo_open, new_driver, shot};
use dalica_engine::Key;
use dalica_ui::headless::Click;
use dalica_ui::i18n::pseudo;
use dalica_ui::state::Section;
use dalica_ui::{App, Driver};
use egui::epaint::{ClippedShape, Shape};
use egui::{PointerButton, Rect};
use serde_json::json;

/// The demo library in the pseudo-locale, an album of three of its photos made.
fn pseudo_library(dir: &std::path::Path, (w, h): (u32, u32)) -> (Driver, String) {
    let s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.run("options.set", &json!({ "values": { "interface.language": "qps" } })).unwrap();
    let ids: Vec<u64> = d.app().session().unwrap().library().photos().take(3).map(|p| p.id.get()).collect();
    let album = d.run("album.create", &json!({ "name": "Lisbon Trip", "ids": ids })).unwrap()["album"].as_str().unwrap().to_string();
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    (d, album)
}

/// The texts no catalog may translate: what the library holds (names, captions, tags, paths, the
/// people's names, the album's name).
fn user_data(d: &Driver) -> BTreeSet<String> {
    let lib = d.app().session().unwrap().library();
    let mut out = BTreeSet::new();
    for f in lib.folders() {
        out.insert(f.path.display().to_string());
        out.extend(f.path.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    for p in lib.photos() {
        out.insert(p.name.clone());
        let info = lib.info(p.id);
        out.insert(info.caption.clone());
        out.extend(info.tags.iter().cloned());
    }
    for (_, a) in lib.albums() {
        out.insert(a.name.clone());
    }
    for (_, p) in lib.people() {
        out.insert(p.name.clone());
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// A text that is user data: one of `data`, user data with a count after it ("Lisbon (20)"), or
/// user data cut short in its middle or at its end to fit ("2026-06… Garden").
fn is_user_data(text: &str, data: &BTreeSet<String>) -> bool {
    let bare = text.trim();
    let counted = bare.rsplit_once(" (").filter(|(_, n)| n.trim_end_matches(')').chars().all(|c| c.is_ascii_digit())).map(|(name, _)| name);
    let elided = bare
        .split_once('…')
        .is_some_and(|(head, tail)| data.iter().any(|d| d.starts_with(head) && d.ends_with(tail) && d.len() > bare.len() - '…'.len_utf8()));
    data.contains(bare) || counted.is_some_and(|n| data.contains(n)) || elided
}

/// A keyboard shortcut as a menu row writes it ("Ctrl+Shift+R", "F11", "Delete").
fn is_shortcut(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && !text.contains(' ') && (Key::parse(text).is_ok() || Key::parse(&text.replace("Cmd+", "Ctrl+")).is_ok())
}

/// The map's attribution line (the offline layer's, or a tile server's credit).
fn is_attribution(text: &str) -> bool {
    text == dalica_ui_kit::widgets::OFFLINE_ATTRIBUTION || text.contains('©')
}

/// The texts the next frame draws that went through no catalog and are not user data, each with
/// `place`.
fn untranslated(d: &mut Driver, place: &str, data: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    d.step();
    let drawn = d.drawn_texts().to_vec();
    assert!(drawn.iter().any(|t| pseudo::is_marked(t)), "{place}: the pseudo-locale is drawn");
    for t in pseudo::untranslated(drawn.iter().map(String::as_str)) {
        if !is_user_data(t, data) && !is_shortcut(t) && !is_attribution(t) {
            out.insert(format!("{place}: {t:?}"));
        }
    }
}

/// Every text of `shapes` with the rect it is drawn in and its clip.
fn texts(shapes: &[ClippedShape]) -> Vec<(String, Rect, Rect)> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            Shape::Text(t) if !t.galley.text().trim().is_empty() => out.push((t.galley.text().to_owned(), t.visual_bounding_rect(), clip)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, s.clip_rect, &mut out);
    }
    out
}

/// The labels the next frame cuts at the side of the area they are drawn in, or past the window.
fn clipped(d: &mut Driver, place: &str, out: &mut BTreeSet<String>) {
    let shapes = crate::accessibility::frame(d, Vec::new()).shapes;
    let screen = d.ctx().content_rect();
    for (text, r, clip) in texts(&shapes) {
        if !r.intersect(clip).is_positive() {
            continue; // scrolled out of sight
        }
        if r.left() < clip.left() - 0.5 || r.right() > clip.right() + 0.5 || r.right() > screen.right() + 0.5 {
            out.insert(format!("{place}: {text:?} cut at the side: {r:?} in {clip:?}"));
        }
    }
}

/// What the window draws untranslated and what it clips at `size`, through each state of the
/// library the task names; the window painted as `i18n-window-<name>-<size>.png` after each.
fn walk(size: (u32, u32)) -> (BTreeSet<String>, BTreeSet<String>) {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, album) = pseudo_library(dir.path(), size);
    let mut data = user_data(&d);
    // The search's text, which names its album.
    data.extend(["harbour".to_string(), "zzzz".to_string()]);
    let (mut text, mut cut) = (BTreeSet::new(), BTreeSet::new());
    let tag = format!("{}x{}", size.0, size.1);
    let look = |d: &mut Driver, place: &str, text: &mut BTreeSet<String>, cut: &mut BTreeSet<String>| {
        d.settle(FRAMES).unwrap();
        untranslated(d, place, &data, text);
        clipped(d, place, cut);
        d.settle(FRAMES).unwrap();
        shot(d, &format!("i18n-window-{place}-{tag}"));
    };
    look(&mut d, "library", &mut text, &mut cut);

    // A folder selected as a unit, with the tray holding its photos and one held.
    let library = d.app().session().unwrap().library();
    let folder = library.photos().next().unwrap().folder;
    let held = library.photos().nth(5).unwrap().id.get();
    d.run("select.section", &json!({ "section": Section::Folder(folder) })).unwrap();
    look(&mut d, "folder", &mut text, &mut cut);
    d.run("select.set", &json!({ "ids": [held] })).unwrap();
    d.run("tray.hold", &json!({})).unwrap();
    d.run("select.section", &json!({ "section": format!("album:{album}") })).unwrap();
    look(&mut d, "album", &mut text, &mut cut);

    for pane in ["properties", "tags", "people", "places"] {
        d.run(&format!("pane.{pane}"), &json!({})).unwrap();
        look(&mut d, pane, &mut text, &mut cut);
    }
    d.run("pane.places", &json!({})).unwrap();

    // Search mode: the banner over the matches, then a search that matches nothing.
    d.app_mut().state_mut().search.text = "harbour".into();
    look(&mut d, "search", &mut text, &mut cut);
    assert!(d.widgets().iter().any(|w| w.id == "banner:search"), "the banner shows");
    d.app_mut().state_mut().search.text = "zzzz".into();
    look(&mut d, "no-matches", &mut text, &mut cut);
    dalica_ui::commands::search::clear(d.app_mut());

    // A notice: three photos moved to another folder.
    let library = d.app().session().unwrap().library();
    let other = library.folders().map(|f| f.id).find(|f| *f != folder).unwrap();
    let moved: Vec<u64> = library.photos_in(folder).iter().take(3).map(|p| p.id.get()).collect();
    d.run("photo.moveTo", &json!({ "ids": moved, "folder": other, "confirm": true })).unwrap();
    d.step_until(|app| !app.notices().showing(0.0).collect::<Vec<_>>().is_empty() || app.session().is_some_and(|s| !s.busy()), FRAMES).unwrap();
    d.settle(FRAMES).unwrap();
    let toast = d.widgets().into_iter().find(|w| w.id.starts_with("toast:")).map(|w| w.label);
    assert!(toast.as_deref().is_some_and(pseudo::is_marked), "a notice shows, bracketed: {toast:?}");
    untranslated(&mut d, "notice", &data, &mut text);
    clipped(&mut d, "notice", &mut cut);
    shot(&d, &format!("i18n-window-notice-{tag}"));

    // The menu bar and each menu, with each of its submenus open in turn.
    let menus: Vec<String> = d.widgets().into_iter().filter(|w| w.id.starts_with("menu:")).map(|w| w.id).collect();
    assert!(menus.len() >= 8, "{menus:?}");
    for m in &menus {
        d.click(m).unwrap();
        d.settle(FRAMES).unwrap();
        untranslated(&mut d, m, &data, &mut text);
        clipped(&mut d, m, &mut cut);
        let subs: Vec<String> = d.widgets().into_iter().filter(|w| w.id.starts_with("submenu:")).map(|w| w.id).collect();
        for s in subs {
            d.hover(&s, 400).unwrap();
            d.settle(FRAMES).unwrap();
            untranslated(&mut d, &s, &data, &mut text);
        }
        d.key("Escape").unwrap();
        d.settle(FRAMES).unwrap();
    }

    // The photo menu on a thumbnail.
    let first = d.app().panels().unwrap().sections.list.iter().find(|s| s.folder.is_some()).map(|s| (s.id, s.photos[0])).unwrap();
    d.run("select.set", &json!({ "ids": [first.1.get()] })).unwrap();
    d.run("section.scrollTo", &json!({ "section": first.0 })).unwrap();
    d.settle(FRAMES).unwrap();
    d.click_with(&format!("thumb:{}", first.1.get()), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.widgets().iter().any(|w| w.id == "menuitem:view.edit"), "the photo menu is open");
    untranslated(&mut d, "photo menu", &data, &mut text);
    clipped(&mut d, "photo menu", &mut cut);
    shot(&d, &format!("i18n-window-photo-menu-{tag}"));
    (text, cut)
}

#[test]
fn every_text_of_the_main_window_goes_through_the_catalogs_and_no_label_clips() {
    for size in [LARGE, SMALL] {
        let (text, cut) = walk(size);
        assert!(text.is_empty(), "{size:?}: {} texts drawn without a catalog:\n{}", text.len(), text.into_iter().collect::<Vec<_>>().join("\n"));
        assert!(cut.is_empty(), "{size:?}: {} labels clipped:\n{}", cut.len(), cut.into_iter().collect::<Vec<_>>().join("\n"));
    }
}

#[test]
fn english_draws_no_pseudo_mark() {
    let dir = tempfile::tempdir().unwrap();
    let s = demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    d.step();
    let drawn = d.drawn_texts().to_vec();
    assert!(drawn.iter().any(|t| t == "File") && drawn.iter().all(|t| !pseudo::is_marked(t)), "{drawn:?}");
}
