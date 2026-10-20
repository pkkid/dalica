//! The dialogs through the catalogs (M15.32, `localization.md` → Checking a translation),
//! headless in the pseudo-locale `qps`: each dialog opened by id, and every tab of Options, draws
//! only bracketed text outside user data (folder, file, album and people's names, captions, tags,
//! paths, the contributors' names), the keyboard shortcuts written as keys and the languages'
//! endonyms. At 1400 × 900 and 900 × 600 no label is cut at the side of the area it is drawn in.
//! `DALICA_SHOTS=<dir>` writes each dialog in the pseudo-locale there (`i18n-dialog-*.png`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, demo_open, new_driver, shot};
use dalica_engine::Key;
use dalica_ui::i18n::pseudo;
use dalica_ui::state::{Dialog, DialogKind};
use dalica_ui::{App, Driver};
use egui::Rect;
use egui::epaint::{ClippedShape, Shape};
use serde_json::json;

/// The demo library in the pseudo-locale, one photo selected.
fn pseudo_library(dir: &std::path::Path, (w, h): (u32, u32)) -> Driver {
    let s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.run("options.set", &json!({ "values": { "interface.language": "qps" } })).unwrap();
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    d
}

/// The texts no catalog may translate: what the library holds (names, captions, tags, paths), the
/// values the settings hold (addresses, a credit, folders), the contributors' and models' names,
/// the build's facts, and the languages' endonyms.
fn user_data(d: &mut Driver) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tab in dalica_engine::options::Tab::ALL {
        let listed = d.run("options.list", &json!({ "tab": tab.id() })).unwrap();
        for row in listed["options"].as_array().into_iter().flatten() {
            out.extend(row["value"].as_str().map(str::to_string));
        }
    }
    let lib = d.app().session().unwrap().library();
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
    out.extend(dalica_ui::i18n::choices().into_iter().map(|(_, endonym)| endonym.to_string()));
    let baked = dalica_ui::credits::Baked::compiled();
    for c in baked.contributors {
        out.insert(format!("@{}", c.login));
        out.extend(c.display_name.into_iter().chain(c.real_name).map(str::to_string));
    }
    for m in baked.models {
        out.extend([m.company, m.model, m.version].map(str::to_string));
    }
    out.extend(baked.commit.map(|c| c.chars().take(dalica_core::build::SHORT_SHA).collect::<String>()));
    out.extend(baked.build_date.map(str::to_string));
    out.insert(dalica_ui::dialogs::about::LICENCE.to_string());
    out.retain(|s| !s.trim().is_empty());
    out
}

/// The names of the drives and folders the Folder Manager lists (the file system's): every root
/// that is not a standard place, and the subfolders of each listing it has read.
fn listed_folders(d: &Driver) -> BTreeSet<String> {
    use dalica_ui::dialogs::folder_manager::listing::{self, Listing};
    let mut out = BTreeSet::new();
    let Some(Listing::Roots(roots)) = listing::peek(d.ctx(), None) else { return out };
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for r in roots.iter() {
        if !r.place {
            out.insert(r.name.clone());
        }
        paths.push(r.path.clone());
    }
    while let Some(p) = paths.pop() {
        if let Some(Listing::Children(c)) = listing::peek(d.ctx(), Some(&p)) {
            for f in &c.folders {
                out.insert(f.name.clone());
                if paths.len() < 256 {
                    paths.push(f.path.clone());
                }
            }
        }
    }
    out
}

/// A text that is user data: one of `data`, user data with a count after it ("Lisbon (20)"), or
/// user data cut short in its middle or at its end to fit ("2026-06… Garden"), or a path.
fn is_user_data(text: &str, data: &BTreeSet<String>) -> bool {
    let bare = text.trim();
    let counted = bare.rsplit_once(" (").filter(|(_, n)| n.trim_end_matches(')').chars().all(|c| c.is_ascii_digit())).map(|(name, _)| name);
    let elided = bare
        .split_once('…')
        .is_some_and(|(head, tail)| data.iter().any(|d| d.starts_with(head) && d.ends_with(tail) && d.len() > bare.len() - '…'.len_utf8()));
    let path = bare.starts_with('/') || bare.starts_with('~') || bare.get(1..3) == Some(":\\");
    data.contains(bare) || counted.is_some_and(|n| data.contains(n)) || elided || path
}

/// A keyboard shortcut as the shortcut editor writes it ("Ctrl+Shift+R", "F11", "Delete").
fn is_shortcut(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && !text.contains(' ') && (Key::parse(text).is_ok() || Key::parse(&text.replace("Cmd+", "Ctrl+")).is_ok())
}

/// The texts the next frame draws that went through no catalog and are not user data, each with
/// `place`.
fn untranslated(d: &mut Driver, place: &str, data: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    d.step();
    let mut data = data.clone();
    data.extend(listed_folders(d));
    let data = &data;
    let drawn = d.drawn_texts().to_vec();
    assert!(drawn.iter().any(|t| pseudo::is_marked(t)), "{place}: the pseudo-locale is drawn");
    for t in pseudo::untranslated(drawn.iter().map(String::as_str)) {
        if !is_user_data(t, data) && !is_shortcut(t) {
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

/// The commands that open a dialog with the draft it has in use (its photos, folder, album, tag),
/// run before the dialogs opened bare by id (which some close at once without their context):
/// `(command, params, dialog)`.
fn opened_by(d: &mut Driver) -> Vec<(&'static str, serde_json::Value, &'static str)> {
    let lib = d.app().session().unwrap().library();
    let photos: Vec<u64> = lib.photos().take(3).map(|p| p.id.get()).collect();
    let folder = lib.photos().next().unwrap().folder;
    let other = lib.folders().map(|f| f.id).find(|f| *f != folder).unwrap();
    let tag = lib.photos().flat_map(|p| lib.info(p.id).tags.clone()).next();
    let ids: Vec<u64> = photos.iter().take(2).copied().collect();
    d.run("edit.apply", &json!({ "ids": ids, "op": "autofix" })).unwrap();
    let album = d.run("album.create", &json!({ "name": "Lisbon Trip", "ids": photos })).unwrap()["album"].as_str().unwrap().to_string();
    let mut out = vec![
        ("folders.manager", json!({}), "folderManager"),
        ("output.export", json!({}), "export"),
        ("album.new", json!({}), "albumProperties"),
        // F2 on the selection.
        ("key:F2", json!({ "ids": ids }), "renameFiles"),
        ("section.properties", json!({ "folder": folder }), "folderProperties"),
        ("photo.delete", json!({ "ids": ids }), "deletePhotos"),
        ("folder.delete", json!({ "folder": folder }), "deleteFolder"),
        ("folder.remove", json!({ "id": folder }), "removeFolder"),
        ("photo.moveTo", json!({ "ids": ids, "folder": other }), "moveFiles"),
        ("album.delete", json!({ "album": album }), "deleteAlbum"),
        ("album.remove", json!({ "album": album, "ids": ids }), "removeFromAlbum"),
        ("edit.undoAll", json!({ "ids": ids }), "undoAllEdits"),
        ("photo.adjustDate", json!({ "ids": ids }), "adjustDate"),
        ("quicktags.configure", json!({}), "quickTags"),
        ("faceModel.open", json!({}), "faceModel"),
        ("app.about", json!({}), "about"),
        ("help.releaseNotes", json!({}), "releaseNotes"),
    ];
    if let Some(tag) = tag {
        out.push(("tag.rename", json!({ "tag": tag }), "renameTag"));
    }
    out
}

/// What the dialogs draw untranslated and what they clip at `size`; each dialog painted as
/// `i18n-dialog-<kind>-<size>.png`.
fn walk(size: (u32, u32)) -> (BTreeSet<String>, BTreeSet<String>, usize) {
    let dir = tempfile::tempdir().unwrap();
    let mut d = pseudo_library(dir.path(), size);
    let openers = opened_by(&mut d);
    let data = user_data(&mut d);
    let (mut text, mut cut) = (BTreeSet::new(), BTreeSet::new());
    let tag = format!("{}x{}", size.0, size.1);
    let mut seen = BTreeSet::new();
    let look = |d: &mut Driver, place: &str, text: &mut BTreeSet<String>, cut: &mut BTreeSet<String>| {
        d.settle(FRAMES).unwrap();
        untranslated(d, place, &data, text);
        clipped(d, place, cut);
        shot(d, &format!("i18n-dialog-{place}-{tag}"));
    };
    for (command, params, kind) in openers {
        d.app_mut().state_mut().dialogs.clear();
        let opened = match command.strip_prefix("key:") {
            Some(key) => {
                d.run("select.set", &params).unwrap();
                d.key(key).map(|()| json!({})).map_err(|e| e.to_string())
            }
            // A dialog's own opener when the window has one (the engine's command of that id runs
            // without asking), else the command as the window runs it.
            None => match dalica_ui::commands::opener(command) {
                Some(o) => (o.open)(d.app_mut(), &params).map_err(|e| e.to_string()),
                None => d.app_mut().run(command, &params).map_err(|e| e.to_string()),
            },
        };
        d.settle(FRAMES).unwrap();
        assert_eq!(d.state().dialogs.last().map(|x| x.kind.name()), Some(kind), "{command}: {opened:?}");
        seen.insert(kind.to_string());
        look(&mut d, kind, &mut text, &mut cut);
    }
    // Options, on each of its tabs.
    d.app_mut().state_mut().dialogs.clear();
    d.run("app.preferences", &json!({})).unwrap();
    for (tab, id, _) in dalica_ui::dialogs::options::TABS {
        d.app_mut().state_mut().options.as_mut().unwrap().tab = tab;
        look(&mut d, &format!("options-{}", id.trim_start_matches("tab:options.")), &mut text, &mut cut);
    }
    seen.insert("options".into());
    // Every other dialog, opened bare by id.
    for kind in DialogKind::ALL {
        if seen.contains(kind.name()) {
            continue;
        }
        d.app_mut().state_mut().dialogs.clear();
        if dalica_ui::dialogs::open(d.app_mut(), Dialog::new(kind)).is_err() {
            continue;
        }
        d.settle(FRAMES).unwrap();
        if d.state().dialogs.last().map(|x| x.kind) != Some(kind) {
            continue;
        }
        seen.insert(kind.name().to_string());
        look(&mut d, kind.name(), &mut text, &mut cut);
    }
    d.app_mut().state_mut().dialogs.clear();
    (text, cut, seen.len())
}

#[test]
fn every_text_of_the_dialogs_goes_through_the_catalogs_and_no_label_clips() {
    for size in [LARGE, SMALL] {
        let (text, cut, opened) = walk(size);
        assert!(opened >= DialogKind::ALL.len() - 4, "{size:?}: {opened} dialogs drawn");
        assert!(text.is_empty(), "{size:?}: {} texts drawn without a catalog:\n{}", text.len(), text.into_iter().collect::<Vec<_>>().join("\n"));
        assert!(cut.is_empty(), "{size:?}: {} labels clipped:\n{}", cut.len(), cut.into_iter().collect::<Vec<_>>().join("\n"));
    }
}
