//! M9's view state, UI commands, the photo context menu's frame and the stub dialogs (M9.4),
//! driven through the control channel as an agent would: every new field (`search`, `clipboard`,
//! the drafts of Album Properties, Rename Files, the new-folder dialog and the move, copy,
//! collision and delete questions) round-trips through `ui.state` / `ui.set` (bad values are
//! `bad_params`, ids that do not exist `not_found`, and nothing changes); each opener shows its
//! dialog (`dialog:<kind>`, or the folder picker) and Esc closes it; `ui.menu.tree {context:
//! "photo"}` lists the photo menu's rows in order and a right-click opens it; the fourth menu reads
//! Album with the album rows on an album section; Delete and Ctrl+Delete route by section; the
//! new menu items light up; a press-and-drag on a thumbnail is reported as `drag`; search mode
//! reserves the filter banner; every new command fails gracefully; every interactive widget of the
//! new dialogs has a valid, unique id.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::{AlbumId, PhotoId};
use dalica_engine::{Key, Session};
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::state::{AlbumKey, AlbumSection, Section, Selection};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A session on a library under `dir` holding one folder of 4 scenes photos; their ids (in the
/// light table's order) and the folder's id.
fn session(dir: &Path) -> (Session, Vec<u64>, u64) {
    let files = dalica_scenes::demo_library(1, 4, 13).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    let folder = s.library().photo(PhotoId::new(ids[0])).unwrap().folder.get();
    (s, ids, folder)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    ids: Vec<u64>,
    folder: u64,
}

fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, _, folder) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    // The light table's order, as the sections list them.
    let shown = d.app().panels().unwrap().sections.get(Section::Folder(dalica_core::FolderId::new(folder))).unwrap().photos.clone();
    let ids = shown.iter().map(|p| p.get()).collect();
    T { _dir: dir, control, conn: Connection::new(), d, ids, folder }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

/// The view state without `status` (the status line's text, which a frame may rewrite).
fn state(t: &mut T) -> Value {
    let mut v = ok(t, "ui.state", json!({}));
    v.as_object_mut().unwrap().remove("status");
    v
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    ok(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn run_code(t: &mut T, id: &str, params: Value) -> &'static str {
    code(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    t.d.settle(FRAMES).unwrap();
}

fn kinds(t: &mut T) -> Vec<String> {
    state(t)["dialogs"].as_array().unwrap().iter().map(|d| d["kind"].as_str().unwrap().to_string()).collect()
}

/// Waits (off the frame loop) for the stubbed picker to show a request.
fn picker_open(t: &T) -> bool {
    (0..2500).any(|_| {
        if t.control.stub().waiting().is_some() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        false
    })
}

/// The rows of a menu model (submenus flattened), as `ui.menu.tree` returns them.
fn rows(children: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for row in children.as_array().unwrap() {
        match row.get("children") {
            Some(kids) => out.extend(rows(kids)),
            None if row.get("id").is_some() => out.push(row.clone()),
            None => {}
        }
    }
    out
}

/// The section of a user album (the library has no albums before M9.10: tests put the selection
/// in one directly, as selecting an album section will).
fn album_section() -> Section {
    Section::Album(AlbumSection::User(AlbumKey::of(&AlbumId::parse("a91c0e").unwrap())))
}

/// Each new field round-trips through `ui.set` and `ui.state`; resetting them gives back the
/// defaults and moves nothing else.
#[test]
fn every_m9_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup((1400, 900));
    let defaults = state(&mut t);
    assert_eq!(
        defaults["search"],
        json!({ "text": "", "highlight": null, "filters": { "starred": false, "faces": false, "videos": false, "geotagged": false }, "age": 0.0, "savedScroll": null })
    );
    let fields = ["clipboard", "albumProperties", "renameFiles", "newFolder", "moveFiles", "copyFiles", "collision", "deletePhotos", "deleteFolder"];
    for field in fields {
        assert_eq!(defaults[field], Value::Null, "{field}");
    }
    let (a, b, f) = (t.ids[0], t.ids[1], t.folder);
    let full = json!({
        "search": { "text": "lisbon tram", "highlight": 1, "filters": { "starred": true, "faces": false, "videos": true, "geotagged": false },
                    "age": 0.5, "savedScroll": { "section": format!("folder:{f}"), "offset": 120.0 } },
        "clipboard": { "op": "copy", "ids": [a, b] },
        "albumProperties": { "album": "a91c0e", "ids": [], "name": "Trip", "date": "2026-06-04", "useNewest": false, "place": "Lisbon", "description": "Trams" },
        "renameFiles": { "ids": [a], "name": "Lisbon", "addDate": true, "addSize": true },
        "newFolder": { "ids": [b], "name": "Day 2", "date": "", "useOldest": true, "place": "", "description": "" },
        "moveFiles": { "ids": [a], "folder": f, "dontAsk": true },
        "copyFiles": { "ids": [b], "folder": f },
        "collision": { "op": "move", "ids": [a], "folder": f, "files": ["IMG_0041.jpg"] },
        "deletePhotos": { "ids": [a, b] },
        "deleteFolder": { "folder": f }
    });
    let v = ok(&mut t, "ui.set", full.clone());
    for field in std::iter::once("search").chain(fields) {
        assert_eq!(v[field], full[field], "{field}");
    }
    assert_eq!(state(&mut t)["renameFiles"], full["renameFiles"]);
    // Search mode: the banner is reserved over the light table.
    t.d.settle(FRAMES).unwrap();
    let regions = ok(&mut t, "ui.inspect", json!({}))["regions"].clone();
    let (banner, table) = (&regions["banner"], &regions["lightTable"]);
    assert_eq!(banner[3], json!(32.0), "{banner}");
    assert_eq!(table[1].as_f64().unwrap(), banner[1].as_f64().unwrap() + 32.0, "the light table starts under the banner");
    // A partial draft is replaced whole (only `edit` and `export` merge); repeated ids are kept once.
    let v = ok(&mut t, "ui.set", json!({ "renameFiles": { "ids": [a, a] } }));
    assert_eq!(v["renameFiles"], json!({ "ids": [a], "name": "", "addDate": false, "addSize": false }));
    let v = ok(&mut t, "ui.set", json!({ "albumProperties": {} }));
    assert_eq!(
        v["albumProperties"],
        json!({ "album": null, "ids": [], "name": "New Album", "date": "", "useNewest": true, "place": "", "description": "" })
    );
    // Back to the defaults: nothing else moved, and the banner is gone.
    let mut reset = serde_json::Map::new();
    reset.insert("search".into(), defaults["search"].clone());
    for field in fields {
        reset.insert(field.into(), Value::Null);
    }
    ok(&mut t, "ui.set", Value::Object(reset));
    assert_eq!(state(&mut t), defaults);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["regions"]["banner"], Value::Null);
}

/// Bad values are `bad_params`, photos and folders that do not exist (and album sections the light
/// table does not show) `not_found`; nothing changes either way.
#[test]
fn bad_m9_values_are_refused_and_change_nothing() {
    let mut t = setup((1400, 900));
    let before = state(&mut t);
    let long = "x".repeat(5000);
    let f = t.folder;
    for bad in [
        json!({ "search": { "text": long } }),
        json!({ "search": { "highlight": 100 } }),
        json!({ "search": { "age": 1.5 } }),
        json!({ "search": { "age": -0.1 } }),
        json!({ "search": { "age": "old" } }),
        json!({ "search": { "filters": { "people": true } } }),
        json!({ "search": { "savedScroll": { "section": "folder:1", "offset": -5 } } }),
        json!({ "search": { "zzz": 1 } }),
        json!({ "clipboard": { "op": "paste", "ids": [] } }),
        json!({ "clipboard": { "ids": [1] } }),
        json!({ "albumProperties": { "album": "b1234" } }),
        json!({ "albumProperties": { "name": long } }),
        json!({ "albumProperties": { "date": "1".repeat(65) } }),
        json!({ "renameFiles": { "name": 7 } }),
        json!({ "newFolder": { "description": long } }),
        json!({ "moveFiles": { "ids": [1] } }),
        json!({ "moveFiles": { "ids": [1], "folder": f, "dontAsk": "yes" } }),
        json!({ "copyFiles": { "ids": "all", "folder": f } }),
        json!({ "collision": { "op": "link", "ids": [], "folder": f } }),
        json!({ "collision": { "op": "move", "ids": [], "folder": f, "files": [long] } }),
        json!({ "deletePhotos": { "ids": [-1] } }),
        json!({ "deleteFolder": { "folder": "x" } }),
        json!({ "drag": { "from": "thumb:1" } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad} changed nothing");
    }
    for bad in [
        json!({ "clipboard": { "op": "cut", "ids": [999_999] } }),
        json!({ "renameFiles": { "ids": [999_999] } }),
        json!({ "moveFiles": { "ids": [], "folder": 999_999 } }),
        json!({ "deleteFolder": { "folder": 999_999 } }),
        json!({ "search": { "savedScroll": { "section": "folder:999999", "offset": 0 } } }),
        json!({ "scroll": { "section": "album:a91c0e", "offset": 0 } }),
        json!({ "search": { "savedScroll": { "section": "album:starred", "offset": 0 } } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad} changed nothing");
    }
}

/// Each opener shows its dialog, named by `dialog:<kind>` (Move Folder: the folder picker), with
/// its draft; Esc closes it and drops the draft.
#[test]
fn each_opener_shows_its_dialog_and_esc_closes_it() {
    for size in [(1400, 900), (900, 600)] {
        let mut t = setup(size);
        let (a, b) = (t.ids[0], t.ids[1]);
        let check = |t: &mut T, kind: &str, draft: &str| {
            t.d.settle(FRAMES).unwrap();
            assert_eq!(kinds(t), [kind], "{size:?}");
            assert!(t.d.widgets().iter().any(|w| w.id == format!("dialog:{kind}")), "{kind} drawn");
            assert_ne!(state(t)[draft], Value::Null, "{kind} has its draft");
            key(t, "Escape");
            assert!(kinds(t).is_empty(), "Esc closed {kind}");
            assert_eq!(state(t)[draft], Value::Null, "{kind} dropped its draft");
        };
        // New Album… (Ctrl+N), for the tray's photos.
        run(&mut t, "select.set", json!({ "ids": [a, b] }));
        key(&mut t, "Ctrl+N");
        assert_eq!(state(&mut t)["albumProperties"]["ids"], json!([a, b]));
        check(&mut t, "albumProperties", "albumProperties");
        // Rename… (F2) for the selection.
        key(&mut t, "F2");
        assert_eq!(state(&mut t)["renameFiles"]["ids"], json!([a, b]));
        check(&mut t, "renameFiles", "renameFiles");
        // File ▸ Move to New Folder… for the selection.
        assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "File/Move to New Folder" })), json!({ "dialog": "newFolder" }));
        assert_eq!(state(&mut t)["newFolder"]["ids"], json!([a, b]));
        check(&mut t, "newFolder", "newFolder");
        // Split Folder Here… from the photo menu: the photo and every photo after it.
        let thumb = format!("thumb:{}", t.ids[2]);
        ok(&mut t, "ui.clickWidget", json!({ "id": thumb, "button": "right" }));
        t.d.settle(FRAMES).unwrap();
        ok(&mut t, "ui.clickWidget", json!({ "id": "menuitem:photo.splitFolder" }));
        assert_eq!(state(&mut t)["newFolder"]["ids"], json!([t.ids[2], t.ids[3]]));
        check(&mut t, "newFolder", "newFolder");
        // Folder ▸ Move… opens the folder picker for the current folder; a cancel changes nothing.
        let section = format!("folder:{}", t.folder);
        run(&mut t, "select.section", json!({ "section": section }));
        let before = state(&mut t);
        assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "Folder/Move" })), json!({ "dialog": "filePicker" }));
        assert!(picker_open(&t));
        let waiting = ok(&mut t, "ui.inspect", json!({}))["filePicker"].clone();
        assert_eq!((&waiting["command"], &waiting["kind"], &waiting["title"]), (&json!("folder.move"), &json!("folder"), &json!("Move Folder")));
        assert_eq!(ok(&mut t, "ui.filePicker", json!({ "cancel": true })), json!({}));
        t.d.settle(FRAMES).unwrap();
        assert_eq!(state(&mut t), before);
    }
}

/// `ui.menu.tree {context: "photo"}` lists the photo menu's rows in order (`organizing.md` §9.1),
/// greyed until their commands exist; a right-click on a thumbnail selects it and opens the same
/// rows. In an album section Delete from Disk becomes Remove from Album, Show in Folder and Set as
/// Album Cover join, and the fourth menu reads Album with the album rows.
#[test]
fn the_photo_menu_lists_its_rows_and_opens_on_a_right_click() {
    let mut t = setup((1400, 900));
    let (a, c) = (t.ids[0], t.ids[2]);
    run(&mut t, "select.set", json!({ "ids": [a] }));
    let menu = ok(&mut t, "ui.menu.tree", json!({ "context": "photo" }));
    let labels: Vec<String> = menu
        .as_array()
        .unwrap()
        .iter()
        .map(|r| if r.get("separator").is_some() { "—".to_string() } else { r["label"].as_str().unwrap().to_string() })
        .collect();
    assert_eq!(
        labels,
        [
            "View and Edit",
            "Add to Album",
            "—",
            "Rotate Clockwise",
            "Rotate Counterclockwise",
            "—",
            "Undo All Edits…",
            "—",
            "Hide",
            "—",
            "Move to New Folder…",
            "Split Folder Here…",
            "—",
            "Open in External Editor",
            "Open With",
            "—",
            "Save",
            "Revert…",
            "—",
            "Locate on Disk",
            "Delete from Disk…",
            "Copy Path",
            "—",
            "Find Faces Again",
            "—",
            "Properties"
        ]
    );
    let all = rows(&menu);
    let row = |id: &str| all.iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("{id}")).clone();
    for id in
        ["view.edit", "photo.rotate.cw", "photo.hide", "photo.splitFolder", "photo.moveToNewFolder", "album.new", "pane.properties", "photo.delete"]
    {
        assert_eq!((row(id)["live"].clone(), row(id)["enabled"].clone()), (json!(true), json!(true)), "{id}");
    }
    for id in ["photo.locate", "photo.copyPath", "photo.openExternal", "photo.openWith"] {
        assert_eq!((row(id)["live"].clone(), row(id)["enabled"].clone()), (json!(false), json!(false)), "{id} waits for its command");
    }
    assert_eq!(row("photo.splitFolder")["dialog"], json!("newFolder"));
    assert_eq!(row("photo.splitFolder")["widget"], json!("menuitem:photo.splitFolder"));
    // A right-click on an unselected photo selects it and opens the menu.
    ok(&mut t, "ui.clickWidget", json!({ "id": format!("thumb:{c}"), "button": "right" }));
    t.d.settle(FRAMES).unwrap();
    let v = ok(&mut t, "ui.inspect", json!({}));
    assert_eq!(v["selection"]["ids"], json!([c]));
    assert_eq!(v["popup"]["kind"], json!("contextMenu"));
    let shown: Vec<&str> = v["popup"]["items"].as_array().unwrap().iter().map(|i| i["widget"].as_str().unwrap()).collect();
    for w in ["menuitem:view.edit", "menuitem:photo.splitFolder", "menuitem:photo.delete", "menuitem:pane.properties"] {
        assert!(shown.contains(&w), "{w} in {shown:?}");
    }
    key(&mut t, "Escape");
    // The album variant, once the selection is in an album section (an album the library has, so
    // its Album Properties opens on it).
    let made = run(&mut t, "album.create", json!({ "name": "Trip", "ids": [a] }));
    let album = AlbumId::parse(made["album"].as_str().unwrap()).unwrap();
    let in_album = Section::Album(AlbumSection::User(AlbumKey::of(&album)));
    t.d.app_mut().state_mut().selection = Selection::new(vec![PhotoId::new(a)], Some(PhotoId::new(a)), Some(in_album), false);
    let all = rows(&ok(&mut t, "ui.menu.tree", json!({ "context": "photo" })));
    let ids: Vec<&str> = all.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"photo.delete"));
    let at = |id: &str| ids.iter().position(|i| *i == id).unwrap_or_else(|| panic!("{id} in {ids:?}"));
    assert!(at("photo.showInFolder") < at("album.setCover") && at("album.setCover") < at("photo.locate") && at("photo.locate") < at("album.remove"));
    let show = all.iter().find(|r| r["id"] == "photo.showInFolder").unwrap();
    assert_eq!((show["live"].clone(), show["enabled"].clone()), (json!(true), json!(true)));
    let bar = ok(&mut t, "ui.menu.tree", json!({}));
    let fourth = bar.as_array().unwrap().iter().find(|m| m["widget"] == "menu:Folder").unwrap().clone();
    assert_eq!(fourth["label"], "Album");
    let album_rows: Vec<String> = rows(&fourth["children"]).iter().map(|r| r["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(album_rows[..2], ["Album Properties…".to_string(), "Delete Album…".to_string()]);
    assert!(album_rows.contains(&"Invert Selection".to_string()) && !album_rows.contains(&"Remove from Dalica…".to_string()));
    let section = ok(&mut t, "ui.menu.tree", json!({ "context": "section" }));
    assert_eq!(section, fourth["children"], "the section menu is the fourth menu's rows");
    // Album Properties… opens on the album.
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "Album/Album Properties" })), json!({ "dialog": "albumProperties" }));
    assert_eq!(state(&mut t)["albumProperties"]["album"], json!(album.as_str()));
    assert_eq!(state(&mut t)["albumProperties"]["name"], json!("Trip"), "the draft starts from the library");
    key(&mut t, "Escape");
    // Show in Folder selects the photo in its folder: the folder's menu again.
    let item = rows(&ok(&mut t, "ui.menu.tree", json!({ "context": "photo" }))).into_iter().find(|r| r["id"] == "photo.showInFolder").unwrap();
    assert_eq!(item["widget"], json!("menuitem:photo.showInFolder"));
    run(&mut t, "photo.showInFolder", json!({}));
    assert_eq!(state(&mut t)["selection"]["section"], json!(format!("folder:{}", t.folder)));
    let bar = ok(&mut t, "ui.menu.tree", json!({}));
    assert_eq!(bar.as_array().unwrap().iter().find(|m| m["widget"] == "menu:Folder").unwrap()["label"], "Folder");
}

/// Delete runs Remove from Album in an album section and Delete from Disk elsewhere; Ctrl+Delete
/// always deletes from disk; a Delete key the user gave another command runs that command. In a
/// folder either press asks first, and Esc deletes nothing.
#[test]
fn delete_and_ctrl_delete_route_by_section() {
    let mut t = setup((1400, 900));
    let a = t.ids[0];
    let (del, ctrl_del) = (Key::parse("Delete").unwrap(), Key::parse("Ctrl+Delete").unwrap());
    let route = |t: &T, k: &Key, bound: Option<&str>| dalica_ui::keys::route(t.d.app(), k, bound.map(str::to_string));
    run(&mut t, "select.set", json!({ "ids": [a] }));
    assert_eq!(route(&t, &del, None).as_deref(), Some("photo.delete"));
    assert_eq!(route(&t, &del, Some("photo.delete")).as_deref(), Some("photo.delete"));
    assert_eq!(route(&t, &ctrl_del, None).as_deref(), Some("photo.delete"));
    assert_eq!(route(&t, &del, Some("photo.hide")).as_deref(), Some("photo.hide"), "a rebinding wins");
    assert_eq!(route(&t, &Key::parse("Ctrl+8").unwrap(), Some("photo.star.toggle")).as_deref(), Some("photo.star.toggle"));
    // Pressed in a folder: each asks the delete question first (M9.15), and Esc deletes nothing.
    for k in ["Delete", "Ctrl+Delete"] {
        key(&mut t, k);
        assert_eq!(kinds(&mut t), ["deletePhotos"], "{k}");
        key(&mut t, "Escape");
        assert!(kinds(&mut t).is_empty(), "{k}");
    }
    assert_eq!(run(&mut t, "library.query", json!({ "limit": 10 }))["photos"].as_array().unwrap().len(), 4);
    // In an album: Remove from Album, with the album; Ctrl+Delete still deletes from disk.
    t.d.app_mut().state_mut().selection = Selection::new(vec![PhotoId::new(a)], Some(PhotoId::new(a)), Some(album_section()), false);
    assert_eq!(route(&t, &del, None).as_deref(), Some("album.remove"));
    assert_eq!(route(&t, &del, Some("photo.delete")).as_deref(), Some("album.remove"));
    assert_eq!(route(&t, &ctrl_del, Some("photo.delete")).as_deref(), Some("photo.delete"));
    assert_eq!(dalica_ui::keys::params_for(t.d.app(), "album.remove"), json!({ "album": "a91c0e", "ids": [a] }));
    t.d.app_mut().state_mut().selection = Selection::new(vec![PhotoId::new(a)], None, Some(Section::Album(AlbumSection::Starred)), false);
    assert_eq!(route(&t, &del, None).as_deref(), Some("album.remove"), "in every album section");
    assert_eq!(dalica_ui::keys::params_for(t.d.app(), "album.remove")["album"], json!("starred"));
}

/// The new menu items light up: New Album…, Rename…, Move to New Folder…, Cut, Copy and Paste,
/// Folder ▸ Move…, each enabled when what it acts on is there and greyed with the reason when not.
#[test]
fn the_new_menu_items_light_up() {
    let mut t = setup((1400, 900));
    let item = |t: &mut T, id: &str| {
        let tree = ok(t, "ui.menu.tree", json!({}));
        let all: Vec<Value> = tree.as_array().unwrap().iter().flat_map(|m| rows(&m["children"])).collect();
        all.into_iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("{id}"))
    };
    let state_of = |r: &Value| (r["live"].as_bool().unwrap(), r["enabled"].as_bool().unwrap(), r.get("reason").cloned().unwrap_or(Value::Null));
    let ctrl = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    let new_album = item(&mut t, "album.new");
    assert_eq!((state_of(&new_album).0, state_of(&new_album).1, new_album["dialog"].clone()), (true, true, json!("albumProperties")));
    assert_eq!(new_album["shortcut"], json!(format!("{ctrl}+N")));
    for id in ["photo.rename", "photo.moveToNewFolder", "photo.cut", "photo.copy"] {
        assert_eq!(state_of(&item(&mut t, id)), (true, false, json!("Select some photos first.")), "{id}");
    }
    assert_eq!(item(&mut t, "photo.rename")["shortcut"], json!("F2"));
    assert_eq!(item(&mut t, "photo.cut")["shortcut"], json!(format!("{ctrl}+X")));
    assert_eq!(state_of(&item(&mut t, "photo.paste")), (true, false, json!("Cut or copy some photos first.")));
    assert_eq!(state_of(&item(&mut t, "folder.move")), (true, false, json!("Select a folder first.")));
    let first = t.ids[0];
    run(&mut t, "select.set", json!({ "ids": [first] }));
    for id in ["photo.rename", "photo.moveToNewFolder", "photo.cut", "photo.copy", "folder.move"] {
        assert!(state_of(&item(&mut t, id)).1, "{id} enabled with a selection");
    }
    key(&mut t, "Ctrl+X");
    assert_eq!(state(&mut t)["clipboard"], json!({ "op": "cut", "ids": [first] }));
    assert_eq!(state_of(&item(&mut t, "photo.paste")), (true, true, Value::Null), "Paste moves the cut photos (M9.13)");
    key(&mut t, "Ctrl+C");
    assert_eq!(state(&mut t)["clipboard"]["op"], json!("copy"));
    // Ctrl+F asks for the search field; it opens nothing.
    key(&mut t, "Ctrl+F");
    key(&mut t, "F3");
    assert!(kinds(&mut t).is_empty());
}

/// Every new command fails gracefully over `engine.execute`: wrong types and unknown fields are
/// `bad_params`, ids that do not exist `not_found`, and nothing changes; the openers' engine ids
/// open no dialog there.
#[test]
fn the_new_commands_fail_gracefully() {
    let mut t = setup((1400, 900));
    let before = state(&mut t);
    for (id, bad) in [
        ("album.new", json!({ "ids": "1" })),
        ("album.new", json!({ "name": "x" })),
        ("photo.cut", json!({ "ids": [-1] })),
        ("photo.copy", json!({ "zzz": true })),
        ("photo.paste", json!({ "folder": "x" })),
        ("search.focus", json!({ "text": "x" })),
        ("photo.splitFolder", json!({ "id": [1] })),
        ("photo.showInFolder", json!({ "ids": [1] })),
    ] {
        assert_eq!(run_code(&mut t, id, bad.clone()), "bad_params", "{id} {bad}");
        assert_eq!(state(&mut t), before, "{id} {bad} changed nothing");
    }
    for (id, bad) in [
        ("album.new", json!({ "ids": [999_999] })),
        ("photo.cut", json!({ "ids": [999_999] })),
        ("photo.splitFolder", json!({ "id": 999_999 })),
        ("photo.showInFolder", json!({ "id": 999_999 })),
    ] {
        assert_eq!(run_code(&mut t, id, bad.clone()), "not_found", "{id} {bad}");
    }
    for id in ["photo.cut", "photo.copy", "photo.paste", "photo.splitFolder", "photo.showInFolder"] {
        assert_eq!(run_code(&mut t, id, json!({})), "disabled", "{id} with nothing to act on");
    }
    // The openers' ids are the engine's: never a dialog over automation.
    let first = t.ids[0];
    // Until its engine command lands an opener's id is unknown to the engine; once it has, the
    // engine refuses the missing input. Either way nothing changes.
    for id in ["photo.rename", "photo.moveToNewFolder", "folder.move"] {
        let code = run_code(&mut t, id, json!({ "ids": [first] }));
        assert!(code == "unknown_command" || code == "bad_params", "{id}: {code}");
        assert_eq!(state(&mut t), before, "{id} changed nothing");
    }
    assert_eq!(state(&mut t), before);
}

/// A press on a thumbnail dragged past the threshold starts a drag of the selection (the photo is
/// selected) or of that photo alone; `ui.inspect` → `drag` reports it while the button is held and
/// `null` once it is released. It is not part of `ui.state`.
#[test]
fn a_thumbnail_drag_is_reported_while_the_button_is_held() {
    let mut t = setup((1400, 900));
    let (a, b) = (t.ids[0], t.ids[1]);
    let rect = |t: &T, id: u64| t.d.widgets().into_iter().find(|w| w.id == format!("thumb:{id}")).unwrap().rect;
    let [x, y, w, h] = rect(&t, a);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["drag"], Value::Null);
    ok(
        &mut t,
        "ui.pointer",
        json!({ "events": [{ "kind": "down", "x": cx, "y": cy }, { "kind": "drag", "x": cx + 20.0, "y": cy + 4.0 }, { "kind": "drag", "x": cx + 60.0, "y": cy + 10.0 }] }),
    );
    let drag = ok(&mut t, "ui.inspect", json!({}))["drag"].clone();
    assert_eq!(drag, json!({ "from": format!("thumb:{a}"), "ids": [a], "target": null, "op": null, "pill": "1 photo" }));
    assert!(state(&mut t).get("drag").is_none());
    ok(&mut t, "ui.pointer", json!({ "events": [{ "kind": "up", "x": cx + 60.0, "y": cy + 10.0 }] }));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["drag"], Value::Null);
    // From a selected photo: the selection.
    run(&mut t, "select.set", json!({ "ids": [a, b] }));
    t.d.settle(FRAMES).unwrap();
    let [x, y, w, h] = rect(&t, b);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    ok(
        &mut t,
        "ui.pointer",
        json!({ "events": [{ "kind": "down", "x": cx, "y": cy }, { "kind": "drag", "x": cx + 30.0, "y": cy }, { "kind": "drag", "x": cx + 80.0, "y": cy }] }),
    );
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["drag"]["ids"], json!([a, b]));
    ok(&mut t, "ui.pointer", json!({ "events": [{ "kind": "up", "x": cx + 80.0, "y": cy }] }));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["drag"], Value::Null);
    assert_eq!(state(&mut t)["selection"]["ids"], json!([a, b]), "a drag selects nothing");
}
