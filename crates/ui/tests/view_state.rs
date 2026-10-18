//! M4's view state (M4.5), driven through the control channel as an agent would: every new field of
//! `UiState` round-trips through `ui.state` / `ui.set` (bad values are refused and change nothing);
//! the persisted ones survive a restart through `settings.json`; the view commands change the state
//! and light up their menu and view-options rows, whose check marks follow the state; File ▸ Exit
//! (Dalica ▸ Quit Dalica on macOS) and View ▸ Properties / Tags / People / Places are live; every
//! UI command fails gracefully.
//!
//! M10's view state (M10.2): the Options dialog's draft (`options`: its tab, the changed values,
//! the last Apply's error, the scale and font size previewed, the shortcut editor) round-trips the
//! same way; Ctrl+, and Ctrl+/ open the dialog on General and on Keyboard Shortcuts, Esc closes it,
//! and Tools ▸ Options…, Help ▸ Keyboard Shortcuts… and the macOS Preferences… item are live.
//!
//! M11's view state (M11.2): the rooms, the Import room, the slideshow, the quick preview, the
//! compare fields of `edit` and the photo viewer round-trip the same way; Ctrl+M opens the Import
//! tab and the strip appears 28 high, closing it removes the strip; each view's keys act only in
//! their context (a Slideshow key does nothing in the library); M11's stub dialogs and Options
//! tabs are drawn with their ids.
//!
//! M12's view state (M12.2): `edit.effect`, the open effect's parameter panel, round-trips the
//! same way with `edit.tool: "effect"`; `edit.tool.open {tool: "effect", name: "sharpen"}` opens
//! the stub panel and Esc closes it; Ctrl+Shift+B (B&W) does nothing in the library.
//!
//! M13's view state (M13.4): the Unnamed view, the People group's and the person sections' modes,
//! the People pane, the Places pane and the face model dialog round-trip the same way (bad values
//! `bad_params`, missing photos `not_found`, nothing changed); `pane.people` and `pane.places`
//! open the stub panes (`region:people`, `region:places`) and `unnamed.open` puts the Unnamed
//! grid in the light table's place; the face and person menus and Tools ▸ Geotag are listed and
//! greyed until their tasks; M13's stub dialogs and the Options tabs Network and People are drawn
//! with their ids.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_engine::{CmdError, Session};
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::state::{Caption, FolderSort, Group, OptionsTab, Pane, TreeMode};
use dalica_ui::{App, Driver, UI_COMMANDS};
use serde_json::{Value, json};

/// A session on a library under `dir` holding `folders` folders of 3 scenes photos each; its photo
/// ids (in display order) and folder ids.
fn session(dir: &Path, folders: usize) -> (Session, Vec<(u64, u64)>, Vec<u64>) {
    let files = crate::common::demo_library(folders, folders * 3, 5).unwrap();
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
    let all = found["photos"].as_array().unwrap();
    let photos: Vec<(u64, u64)> = all.iter().map(|p| (p["id"].as_u64().unwrap(), p["folder"].as_u64().unwrap())).collect();
    let mut ids: Vec<u64> = all.iter().map(|p| p["folder"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!((photos.len(), ids.len()), (folders * 3, folders));
    (s, photos, ids)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    photos: Vec<u64>,
    /// `(photo, folder)` of each photo.
    homes: Vec<(u64, u64)>,
    folders: Vec<u64>,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, homes, folders) = session(dir.path(), 2);
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let photos = homes.iter().map(|(p, _)| *p).collect();
    T { _dir: dir, control, conn: Connection::new(), d, photos, homes, folders }
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

/// Each new field round-trips through `ui.set` and `ui.state`, alone and together.
#[test]
fn every_m4_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    let (p, f) = (t.photos.clone(), t.folders.clone());
    let defaults = state(&mut t);
    for (field, value) in [
        ("treeMode", json!("tree")),
        ("folderSort", json!("recent")),
        ("folderSort", json!("size")),
        ("folderSort", json!("name")),
        ("folderSortReverse", json!(true)),
        ("collapsedGroups", json!(["folders", "albums", "hidden"])),
        ("collapsedFolders", json!([f[1], f[0]])),
        ("caption", json!("filename")),
        ("caption", json!("resolution")),
        ("caption", json!("date")),
        ("smallPictures", json!(true)),
        ("hiddenPictures", json!(true)),
        ("tray", json!({ "held": [p[0], p[4]], "selected": [p[4]] })),
        ("scroll", json!({ "section": format!("folder:{}", f[1]), "offset": 312.5 })),
    ] {
        let reply = ok(&mut t, "ui.set", json!({ field: value }));
        assert_eq!(reply[field], value, "the reply is the new state: {field}");
        assert_eq!(state(&mut t)[field], value, "{field}");
        // Back to the default, and nothing else moved.
        ok(&mut t, "ui.set", json!({ field: defaults[field] }));
        assert_eq!(state(&mut t), defaults, "{field}");
    }
    // Several at once.
    let many = json!({ "treeMode": "tree", "caption": "tags", "smallPictures": true, "collapsedGroups": ["people"] });
    ok(&mut t, "ui.set", many.clone());
    let now = state(&mut t);
    for (k, v) in many.as_object().unwrap() {
        assert_eq!(&now[k], v, "{k}");
    }
    let app = t.d.app().state();
    assert_eq!((app.tree_mode, app.caption, app.small_pictures), (TreeMode::Tree, Caption::Tags, true));
    assert_eq!(app.collapsed_groups, [Group::People]);
    // Repeated ids are kept once.
    ok(&mut t, "ui.set", json!({ "collapsedGroups": ["albums", "albums"], "tray": { "held": [p[1], p[1]], "selected": [] } }));
    assert_eq!(state(&mut t)["collapsedGroups"], json!(["albums"]));
    assert_eq!(state(&mut t)["tray"]["held"], json!([p[1]]));
}

/// The selection's section and whole-section form are reported; the selection stays the select
/// commands' (`ui.set` refuses it).
#[test]
fn the_selection_reports_its_section_and_stays_read_only() {
    let mut t = setup();
    let sel = state(&mut t)["selection"].clone();
    assert_eq!(sel, json!({ "ids": [], "anchor": null, "section": null, "whole": false }));
    // Photos of one folder: that folder's section.
    let (first, home) = t.homes[0];
    let (other, _) = *t.homes.iter().find(|(_, f)| *f != home).unwrap();
    let neighbour = t.homes.iter().find(|(p, f)| *f == home && *p != first).unwrap().0;
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [first, neighbour] } }));
    let sel = state(&mut t)["selection"].clone();
    assert_eq!(sel, json!({ "ids": [first, neighbour], "anchor": neighbour, "section": format!("folder:{home}"), "whole": false }));
    // Toggling one off keeps the section; photos of two folders (a script can ask) have none.
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [neighbour], "mode": "toggle" } }));
    assert_eq!(state(&mut t)["selection"]["section"], format!("folder:{home}"));
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [other], "mode": "toggle" } }));
    assert_eq!(state(&mut t)["selection"]["section"], Value::Null);
    let before = state(&mut t);
    for bad in [json!({ "selection": { "ids": [], "anchor": null, "section": null, "whole": true } }), json!({ "selection": null })] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
    }
    assert_eq!(state(&mut t), before);
    ok(&mut t, "engine.execute", json!({ "command": "select.none" }));
    assert_eq!(state(&mut t)["selection"], json!({ "ids": [], "anchor": null, "section": null, "whole": false }));
}

/// Bad values are `bad_params` (ids that do not exist `not_found`), and nothing changes.
#[test]
fn bad_values_are_refused_and_change_nothing() {
    let mut t = setup();
    let (p, f) = (t.photos.clone(), t.folders.clone());
    ok(&mut t, "ui.set", json!({ "caption": "date", "tray": { "held": [p[0]], "selected": [] } }));
    let before = state(&mut t);
    for bad in [
        json!({ "treeMode": "spiral" }),
        json!({ "treeMode": null }),
        json!({ "folderSort": "colour" }),
        json!({ "folderSort": 1 }),
        json!({ "folderSortReverse": "yes" }),
        json!({ "collapsedGroups": ["attic"] }),
        json!({ "collapsedGroups": "folders" }),
        json!({ "collapsedFolders": [-1] }),
        json!({ "collapsedFolders": ["x"] }),
        json!({ "caption": "Filename" }),
        json!({ "caption": true }),
        json!({ "smallPictures": 1 }),
        json!({ "hiddenPictures": null }),
        json!({ "tray": { "held": [] } }),
        json!({ "tray": { "held": [], "selected": [], "pinned": [] } }),
        json!({ "tray": [] }),
        json!({ "tray": { "held": [], "selected": [p[2]] } }),
        json!({ "scroll": { "section": "album:x", "offset": 0 } }),
        json!({ "scroll": { "section": "folder:x", "offset": 0 } }),
        json!({ "scroll": { "section": null, "offset": -1 } }),
        json!({ "scroll": { "section": null, "offset": 2e9 } }),
        json!({ "scroll": { "section": null, "offset": 1e40 } }),
        json!({ "scroll": { "section": null } }),
        json!({ "scroll": 0 }),
        // A good field next to a bad one: neither is applied.
        json!({ "caption": "none", "treeMode": "spiral" }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
    let (no_photo, no_folder) = (p.iter().max().unwrap() + 100, f.iter().max().unwrap() + 100);
    for bad in [
        json!({ "collapsedFolders": [no_folder] }),
        json!({ "scroll": { "section": format!("folder:{no_folder}"), "offset": 0 } }),
        json!({ "tray": { "held": [no_photo], "selected": [] } }),
        json!({ "tray": { "held": [p[0], u64::MAX], "selected": [] } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
}

/// The persisted fields (tree mode, folder sort, caption, small and hidden pictures, thumbnail size)
/// survive a restart through `settings.json`; the others start fresh.
#[test]
fn the_persisted_fields_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (s, homes, folders) = session(dir.path(), 1);
    let library = dir.path().join("library");
    let mut app = App::new(Some(s));
    app.persist_view();
    for (id, params) in [
        ("view.folderView", json!({ "mode": "tree" })),
        ("view.folderSort", json!({ "by": "name", "reverse": true })),
        ("view.caption", json!({ "caption": "resolution" })),
        ("view.smallPictures", json!({})),
        ("view.hiddenPictures", json!({ "on": true })),
        ("view.thumbsSmall", json!({})),
        ("view.groupCollapse", json!({ "group": "folders" })),
        ("view.folderCollapse", json!({ "folder": folders[0] })),
    ] {
        app.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    }
    app.state_mut().tray.held = vec![dalica_core::PhotoId::new(homes[0].0)];
    app.save_view();
    app.take_session().unwrap().flush().unwrap();
    drop(app);

    let reopen = || Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    let mut app = App::new(Some(reopen()));
    assert_eq!(app.state().tree_mode, TreeMode::Flat, "headless sessions do not read the settings");
    app.persist_view();
    let s = app.state();
    assert_eq!((s.tree_mode, s.folder_sort, s.folder_sort_reverse), (TreeMode::Tree, FolderSort::Name, true));
    assert_eq!((s.caption, s.small_pictures, s.hidden_pictures, s.thumb_size), (Caption::Resolution, true, true, menus::THUMB_SMALL));
    assert!(s.collapsed_groups.is_empty() && s.collapsed_folders.is_empty() && s.tray.held.is_empty(), "not persisted");

    // Back to the defaults is saved too.
    app.run("view.folderView", &json!({ "mode": "flat" })).unwrap();
    app.run("view.caption", &json!({ "caption": "none" })).unwrap();
    app.run("view.smallPictures", &json!({ "on": false })).unwrap();
    app.run("view.thumbsNormal", &json!({})).unwrap();
    app.save_view();
    app.take_session().unwrap().flush().unwrap();
    let mut app = App::new(Some(reopen()));
    app.persist_view();
    let s = app.state();
    assert_eq!((s.tree_mode, s.caption, s.small_pictures, s.thumb_size), (TreeMode::Flat, Caption::None, false, 144.0));
    assert_eq!((s.folder_sort, s.hidden_pictures), (FolderSort::Name, true));

    // Names this version does not know, and hostile values, give the defaults.
    let mut session = app.take_session().unwrap();
    session.change_settings(|st| {
        st.merge(
            "view",
            &[
                ("treeMode", Some(json!("spiral"))),
                ("folderSort", Some(json!(7))),
                ("caption", Some(json!("emoji"))),
                ("thumbSize", Some(json!(1e9))),
                ("smallPictures", Some(json!("yes"))),
            ],
        )
    });
    let mut app = App::new(Some(session));
    app.persist_view();
    let s = app.state();
    assert_eq!(
        (s.tree_mode, s.folder_sort, s.caption, s.thumb_size, s.small_pictures),
        (TreeMode::Flat, FolderSort::Date, Caption::None, 144.0, false)
    );
    // A stored size beyond the slider's range is clamped to it.
    let mut session = app.take_session().unwrap();
    session.change_settings(|st| st.merge("view", &[("thumbSize", Some(json!(2000.0)))]));
    let mut app = App::new(Some(session));
    app.persist_view();
    assert_eq!(app.state().thumb_size, 320.0);
}

/// The menu row `id` with `params` as `ui.menu.tree` reports it.
fn row(t: &mut T, id: &str, params: Value) -> Value {
    fn find(nodes: &[Value], id: &str, params: &Value) -> Option<Value> {
        nodes.iter().find_map(|n| {
            if let Some(children) = n["children"].as_array() {
                return find(children, id, params);
            }
            let fits = params.as_object().is_none_or(|p| p.iter().all(|(k, v)| n["params"].get(k) == Some(v)));
            (n["id"] == id && fits).then(|| n.clone())
        })
    }
    let tree = ok(t, "ui.menu.tree", json!({}));
    find(tree.as_array().unwrap(), id, &params).unwrap_or_else(|| panic!("no row {id} {params}"))
}

/// File ▸ Exit and the four View pane items are live; the view commands light up their rows, and
/// the check marks follow the state.
#[test]
fn the_view_commands_light_up_their_menu_rows_and_check_marks_follow_the_state() {
    let mut t = setup();
    for (id, params) in [
        ("app.quit", Value::Null),
        ("pane.properties", Value::Null),
        ("pane.tags", Value::Null),
        ("pane.people", Value::Null),
        ("pane.places", Value::Null),
        ("view.smallPictures", Value::Null),
        ("view.hiddenPictures", Value::Null),
        ("view.caption", json!({ "caption": "filename" })),
        ("view.folderView", json!({ "mode": "tree" })),
    ] {
        let r = row(&mut t, id, params.clone());
        assert_eq!((&r["live"], &r["enabled"]), (&json!(true), &json!(true)), "{id} {params}: {r}");
    }
    // macOS keeps Quit in its application menu; elsewhere it is File ▸ Exit. Both layouts are checked
    // on every platform; the control channel shows this platform's.
    for (platform, path, label) in [(Platform::Other, "File/Exit", "Exit"), (Platform::Mac, "Dalica/Quit Dalica", "Quit Dalica")] {
        let lookup = Lookup::Path(menus::split_path(path).unwrap());
        let bar = menus::bar(t.d.app(), platform);
        let item = menus::find(&bar, &lookup).unwrap_or_else(|| panic!("{platform:?}: no {path}"));
        assert_eq!((item.id, item.label.as_str(), item.enabled), ("app.quit", label, true), "{platform:?}");
    }
    let (quit_label, quit_path) = if cfg!(target_os = "macos") { ("Quit Dalica", "Dalica/Quit Dalica") } else { ("Exit", "File/Exit") };
    assert_eq!(row(&mut t, "app.quit", Value::Null)["label"], quit_label);
    assert_eq!(row(&mut t, "pane.tags", Value::Null)["shortcut"], if cfg!(target_os = "macos") { "Cmd+T" } else { "Ctrl+T" });
    // The pane rows follow the open pane, whoever opened it.
    assert_eq!(row(&mut t, "pane.tags", Value::Null)["checked"], false);
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Tags" }));
    assert_eq!(t.d.state().pane, Some(Pane::Tags));
    assert_eq!(row(&mut t, "pane.tags", Value::Null)["checked"], true);
    t.d.click("toggle:pane.people").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(
        (row(&mut t, "pane.people", Value::Null)["checked"].clone(), row(&mut t, "pane.tags", Value::Null)["checked"].clone()),
        (json!(true), json!(false))
    );
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/People" }));
    assert_eq!(t.d.state().pane, None, "the open pane's item closes it");
    // Small and Hidden Pictures.
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Small Pictures" }));
    assert!(t.d.state().small_pictures);
    assert_eq!(row(&mut t, "view.smallPictures", Value::Null)["checked"], true);
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Small Pictures" }));
    assert_eq!(row(&mut t, "view.smallPictures", Value::Null)["checked"], false);
    ok(&mut t, "ui.set", json!({ "hiddenPictures": true }));
    assert_eq!(row(&mut t, "view.hiddenPictures", Value::Null)["checked"], true);
    // The caption radio group: one dot, on the state's caption.
    let dots = |t: &mut T| {
        ["none", "filename", "caption", "tags", "resolution", "date"]
            .iter()
            .filter(|c| row(t, "view.caption", json!({ "caption": c }))["checked"] == true)
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(dots(&mut t), ["none"]);
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Thumbnail Caption/Date" }));
    assert_eq!((t.d.state().caption, dots(&mut t)), (Caption::Date, vec!["date".to_string()]));
    // Folder View.
    ok(&mut t, "ui.menu.invoke", json!({ "id": "view.folderView", "params": { "mode": "tree" } }));
    assert_eq!(t.d.state().tree_mode, TreeMode::Tree);
    assert_eq!(row(&mut t, "view.folderView", json!({ "mode": "tree" }))["checked"], true);
    assert_eq!(row(&mut t, "view.folderView", json!({ "mode": "flat" }))["checked"], false);
    // File ▸ Exit (Dalica ▸ Quit Dalica on macOS) asks the window to close; the next frame passes it on.
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": quit_path })), json!({ "quitting": true }));
    assert!(!t.d.app().quit_requested(), "the frames after the invoke sent the close request");
    t.d.app_mut().run("app.quit", &json!({})).unwrap();
    assert!(t.d.app().quit_requested());
    t.d.step();
    assert!(!t.d.app().quit_requested(), "the frame sent the close request");
}

/// The view-options popup's rows run `view.folderSort`, and the model's check marks follow the
/// state (the popup's own drawing reads them in the tree pane's task).
#[test]
fn the_view_options_rows_are_live_and_sort_the_folders() {
    let mut t = setup();
    t.d.click("button:toolbar.viewOptions").unwrap();
    t.d.settle(FRAMES).unwrap();
    for id in [
        "menuitem:view.folderSort/date",
        "menuitem:view.folderSort/recent",
        "menuitem:view.folderSort/size",
        "menuitem:view.folderSort/name",
        "menuitem:view.folderSort/reverse",
    ] {
        let w = t.d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}"));
        assert!(w.enabled, "{id} is live");
    }
    t.d.click("menuitem:view.folderSort/name").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().folder_sort, FolderSort::Name);
    let checked = |t: &T, params: Value| menus::checked(t.d.app(), "view.folderSort", &params);
    assert!(checked(&t, json!({ "by": "name" })) && !checked(&t, json!({ "by": "date" })) && !checked(&t, json!({ "reverse": true })));
    ok(&mut t, "engine.execute", json!({ "command": "view.folderSort", "params": { "reverse": true } }));
    assert!(t.d.state().folder_sort_reverse && checked(&t, json!({ "reverse": true })));
    ok(&mut t, "engine.execute", json!({ "command": "view.folderSort", "params": { "reverse": true } }));
    assert!(!t.d.state().folder_sort_reverse, "Reverse Order flips back");
    ok(&mut t, "engine.execute", json!({ "command": "view.folderSort", "params": { "by": "recent", "reverse": true } }));
    assert_eq!((t.d.state().folder_sort, t.d.state().folder_sort_reverse), (FolderSort::Recent, true));
    // The Flat | Tree pair runs view.folderView.
    t.d.click("button:tree.tree").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().tree_mode, TreeMode::Tree);
}

/// Group and folder disclosure, and the keys of the new commands.
#[test]
fn groups_and_folders_fold_and_the_new_keys_run_their_commands() {
    let mut t = setup();
    let f = t.folders.clone();
    let run = |t: &mut T, id: &str, params: Value| ok(t, "engine.execute", json!({ "command": id, "params": params }));
    assert_eq!(run(&mut t, "view.groupCollapse", json!({ "group": "folders" })), json!({ "group": "folders", "collapsed": true }));
    assert_eq!(t.d.state().collapsed_groups, [Group::Folders]);
    run(&mut t, "view.groupCollapse", json!({ "group": "albums", "collapsed": true }));
    run(&mut t, "view.groupCollapse", json!({ "group": "albums", "collapsed": true }));
    assert_eq!(t.d.state().collapsed_groups, [Group::Folders, Group::Albums], "setting twice keeps one entry");
    run(&mut t, "view.groupCollapse", json!({ "group": "folders" }));
    run(&mut t, "view.groupCollapse", json!({ "group": "hidden", "collapsed": false }));
    assert_eq!(t.d.state().collapsed_groups, [Group::Albums]);
    assert_eq!(run(&mut t, "view.folderCollapse", json!({ "folder": f[1] })), json!({ "folder": f[1], "collapsed": true }));
    run(&mut t, "view.folderCollapse", json!({ "folder": f[1], "collapsed": false }));
    assert!(t.d.state().collapsed_folders.is_empty());
    // Keys: Ctrl+T (Tags), Alt+Enter (Properties), Ctrl+Q (quit).
    t.d.key("Ctrl+T").unwrap();
    assert_eq!(t.d.state().pane, Some(Pane::Tags));
    t.d.key("Alt+Enter").unwrap();
    assert_eq!(t.d.state().pane, Some(Pane::Properties));
    t.d.key("Alt+Enter").unwrap();
    assert_eq!(t.d.state().pane, None);
    let quit = dalica_engine::Key::parse("Ctrl+Q").unwrap();
    let bound = t.d.app().session().unwrap().keymap().resolve(&quit, dalica_engine::Context::Library, false);
    assert_eq!(bound.as_deref(), Some("app.quit"));
}

/// Hostile parameters for every UI command: an error, never a panic, and the state is unchanged.
#[test]
fn every_ui_command_fails_gracefully() {
    let mut t = setup();
    let hostile = [
        json!([1]),
        json!("x"),
        json!(7),
        json!(true),
        json!({ "zzz": 1 }),
        json!({ "on": "yes" }),
        json!({ "ids": "1" }),
        json!({ "caption": 3 }),
        json!({ "mode": "spiral" }),
        json!({ "by": "colour" }),
        json!({ "reverse": false }),
        json!({ "group": "attic" }),
        json!({ "folder": -1 }),
        json!({ "folder": u64::MAX }),
        json!({ "collapsed": "no" }),
    ];
    let before = t.d.state().clone();
    for c in UI_COMMANDS {
        for params in &hostile {
            match t.d.app_mut().run(c.id, params) {
                Err(CmdError::BadParams(_) | CmdError::NotFound(_)) => {}
                other => panic!("{} {params}: {other:?}", c.id),
            }
        }
    }
    let mut after = t.d.state().clone();
    after.status.clone_from(&before.status);
    assert_eq!(after, before, "nothing changed");
    // Commands that need something to work on say what is missing.
    for (id, params) in [
        ("view.caption", json!({})),
        ("view.folderView", json!({})),
        ("view.folderSort", json!({})),
        ("view.groupCollapse", json!({})),
        ("view.folderCollapse", json!({})),
        ("select.set", json!({})),
    ] {
        assert!(matches!(t.d.app_mut().run(id, &params), Err(CmdError::BadParams(_))), "{id}");
    }
    assert!(matches!(t.d.app_mut().run("view.folderCollapse", &json!({ "folder": 999_999 })), Err(CmdError::NotFound(_))));
    // Without a library, a folder cannot be found.
    let mut bare = App::new(None);
    assert!(matches!(bare.run("view.folderCollapse", &json!({ "folder": 1 })), Err(CmdError::NotFound(_))));
    assert!(bare.run("view.caption", &json!({ "caption": "date" })).is_ok(), "view state needs no library");
    assert!(menus::find(&menus::bar(&bare, Platform::Other), &Lookup::Id { id: "app.quit".into(), params: Value::Null }).is_some_and(|i| i.enabled));
}

/// Each M10 field round-trips through `ui.set` and `ui.state`; `options` merges field by field.
#[test]
fn every_m10_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    assert_eq!(state(&mut t)["options"], Value::Null, "closed: no draft");
    let defaults = state(&mut t);
    // A partial draft over none takes the defaults for the rest.
    ok(&mut t, "ui.set", json!({ "options": { "tab": "fileTypes" } }));
    let fresh = json!({
        "tab": "fileTypes", "values": {}, "error": null, "previewScale": null, "previewFontSize": null,
        "shortcuts": { "search": "", "context": null, "showUnbound": false, "recording": null, "conflict": null,
                       "changes": [], "resetAll": false }
    });
    assert_eq!(state(&mut t)["options"], fresh);
    let cell = json!({ "command": "photo.star.toggle", "context": "library", "slot": 1 });
    for (field, value) in [
        ("tab", json!("general")),
        ("tab", json!("export")),
        ("tab", json!("shortcuts")),
        ("values", json!({ "interface.animations": false, "files.thumbnailThreshold": 96, "export.watermarkText": "© me", "language": null })),
        ("error", json!("The settings file could not be written: disk full")),
        ("previewScale", json!(150)),
        ("previewScale", json!("auto")),
        ("previewScale", json!(300)),
        ("previewFontSize", json!("large")),
        ("previewFontSize", json!("medium")),
        (
            "shortcuts",
            json!({ "search": "ctrl+1", "context": "editView", "showUnbound": true, "recording": cell,
                    "conflict": { "cell": cell, "key": "Ctrl+1", "command": "view.thumbsSmall", "context": "library" },
                    "changes": [{ "command": "photo.star.toggle", "context": "library", "slot": 1, "key": "F4" },
                                { "command": "view.edit", "context": "library", "slot": 0, "key": null }],
                    "resetAll": true }),
        ),
    ] {
        let reply = ok(&mut t, "ui.set", json!({ "options": { field: value } }));
        assert_eq!(reply["options"][field], value, "the reply is the new state: {field}");
        let now = state(&mut t);
        assert_eq!(now["options"][field], value, "{field}");
        // Field by field: the rest of the draft is as it was.
        for (k, v) in fresh.as_object().unwrap() {
            if k != field && k != "tab" {
                assert_eq!(&now["options"][k], v, "{field} left {k} alone");
            }
        }
        ok(&mut t, "ui.set", json!({ "options": { field: fresh[field] } }));
        assert_eq!(state(&mut t)["options"], fresh, "{field}");
    }
    let o = t.d.app().state().options.clone().unwrap();
    assert_eq!(o.tab, OptionsTab::FileTypes);
    // A draft's shortcut editor is replaced whole; closing drops the draft.
    ok(&mut t, "ui.set", json!({ "options": { "shortcuts": { "search": "star" } } }));
    assert_eq!(state(&mut t)["options"]["shortcuts"]["showUnbound"], false);
    ok(&mut t, "ui.set", json!({ "options": null }));
    assert_eq!(state(&mut t), defaults);
}

/// Bad M10 values are `bad_params` (a command the keymap does not know `not_found`), and nothing
/// changes.
#[test]
fn bad_m10_values_are_refused_and_change_nothing() {
    let mut t = setup();
    ok(&mut t, "ui.set", json!({ "options": { "tab": "export", "values": { "interface.tooltips": false } } }));
    let before = state(&mut t);
    let cell = |command: &str, slot: u64| json!({ "command": command, "context": "library", "slot": slot });
    let long = "x".repeat(4097);
    let many: serde_json::Map<String, Value> = (0..257).map(|i| (format!("k{i}"), json!(true))).collect();
    for bad in [
        json!({ "options": { "tab": "attic" } }),
        json!({ "options": { "tab": null } }),
        json!({ "options": { "colour": 1 } }),
        json!({ "options": 3 }),
        json!({ "options": { "values": [] } }),
        json!({ "options": { "values": { "a": [1] } } }),
        json!({ "options": { "values": { "a": { "b": 1 } } } }),
        json!({ "options": { "values": { "": 1 } } }),
        json!({ "options": { "values": { "a": long } } }),
        json!({ "options": { "values": many } }),
        json!({ "options": { "error": long } }),
        json!({ "options": { "error": 5 } }),
        json!({ "options": { "previewScale": 160 } }),
        json!({ "options": { "previewScale": 50 } }),
        json!({ "options": { "previewScale": 325 } }),
        json!({ "options": { "previewScale": 1.5 } }),
        json!({ "options": { "previewScale": "Auto" } }),
        json!({ "options": { "previewFontSize": "huge" } }),
        json!({ "options": { "shortcuts": { "search": long } } }),
        json!({ "options": { "shortcuts": { "context": "attic" } } }),
        json!({ "options": { "shortcuts": { "showUnbound": "yes" } } }),
        json!({ "options": { "shortcuts": { "recording": cell("photo.star.toggle", 2) } } }),
        json!({ "options": { "shortcuts": { "recording": cell("", 0) } } }),
        json!({ "options": { "shortcuts": { "recording": { "command": "view.edit", "context": "library" } } } }),
        json!({ "options": { "shortcuts": { "changes": [{ "command": "view.edit", "context": "library", "slot": 0, "key": "Ctrl+Hyper+1" }] } } }),
        json!({ "options": { "shortcuts": { "changes": [{ "command": "view.edit", "context": "library", "slot": 0 }] } } }),
        json!({ "options": { "shortcuts": { "conflict": { "cell": cell("view.edit", 0), "key": "", "command": "view.library", "context": "library" } } } }),
        json!({ "options": { "shortcuts": { "captured": ["Esc"] } } }),
        // A good field next to a bad one: neither is applied.
        json!({ "options": { "tab": "general", "previewFontSize": "huge" } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
    for bad in [
        json!({ "options": { "shortcuts": { "recording": cell("photo.nope", 0) } } }),
        json!({ "options": { "shortcuts": { "changes": [{ "command": "photo.nope", "context": "global", "slot": 0, "key": "F4" }] } } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
}

/// Ctrl+, opens Options on General and Ctrl+/ on Keyboard Shortcuts, Esc closes it; a tab click
/// changes the tab; the menu items are live, the macOS Preferences… item too.
#[test]
fn the_options_keys_open_the_dialog_and_its_menu_items_are_live() {
    let mut t = setup();
    for (combo, tab) in [("Ctrl+,", "general"), ("Ctrl+/", "shortcuts")] {
        ok(&mut t, "ui.key", json!({ "combo": combo }));
        let v = ok(&mut t, "ui.inspect", json!({}));
        assert_eq!((v["dialog"]["kind"].as_str(), v["dialog"]["widget"].as_str()), (Some("options"), Some("dialog:options")), "{combo}");
        assert_eq!(v["options"]["tab"], tab, "{combo}");
        assert_eq!(v["dialog"]["default"], "button:dialog.ok");
        let widgets = ok(&mut t, "ui.widgets", json!({ "filter": "options" }));
        let ids: Vec<&str> = widgets.as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap()).collect();
        for id in ["dialog:options", "tab:options.general", "tab:options.fileTypes", "tab:options.export", "tab:options.shortcuts"] {
            assert!(ids.contains(&id), "{id} in {ids:?}");
        }
        let apply = ok(&mut t, "ui.widgets", json!({ "filter": "button:dialog.apply" }));
        assert_eq!(apply[0]["enabled"], false, "nothing changed yet");
        ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
        let v = ok(&mut t, "ui.inspect", json!({}));
        assert_eq!((&v["dialog"], &v["options"]), (&Value::Null, &Value::Null), "{combo}: Esc closes it");
    }
    // A tab click shows that tab; an applied change would light Apply.
    ok(&mut t, "engine.execute", json!({ "command": "app.preferences" }));
    ok(&mut t, "ui.clickWidget", json!({ "id": "tab:options.export" }));
    assert_eq!(state(&mut t)["options"]["tab"], "export");
    ok(&mut t, "ui.set", json!({ "options": { "values": { "export.removeLocation": true } } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert_eq!(ok(&mut t, "ui.widgets", json!({ "filter": "button:dialog.apply" }))[0]["enabled"], true);
    ok(&mut t, "ui.dialog.cancel", json!({}));
    assert_eq!(state(&mut t)["options"], Value::Null, "Cancel drops the draft");
    for id in ["app.preferences", "app.shortcuts"] {
        let r = row(&mut t, id, Value::Null);
        assert_eq!((r["live"].as_bool(), r["enabled"].as_bool()), (Some(true), Some(true)), "{id}");
    }
    // The rows write the primary modifier as the platform names it.
    let ctrl = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    assert_eq!(row(&mut t, "app.preferences", Value::Null)["shortcut"], format!("{ctrl}+,"));
    assert_eq!(row(&mut t, "app.shortcuts", Value::Null)["shortcut"], format!("{ctrl}+/"));
    let mac = menus::bar(t.d.app(), Platform::Mac);
    let prefs = menus::find(&mac[..1], &Lookup::Id { id: "app.preferences".into(), params: Value::Null }).unwrap();
    assert!(prefs.live && prefs.enabled && prefs.shortcut.as_deref() == Some("Cmd+,"), "{prefs:?}");
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "id": "app.shortcuts" })), json!({ "dialog": "options" }));
    assert_eq!(state(&mut t)["options"]["tab"], "shortcuts");
}

/// Each M11 field round-trips through `ui.set` and `ui.state`; `import` merges field by field.
#[test]
fn every_m11_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    let p = t.photos.clone();
    let defaults = state(&mut t);
    assert_eq!((&defaults["room"], &defaults["rooms"]), (&json!("library"), &json!(["library"])));
    assert_eq!((&defaults["slideshow"], &defaults["peek"], &defaults["viewer"]), (&Value::Null, &Value::Null, &Value::Null));
    // The transition running is the show's to say (M11.19): none while a show set here stands still.
    let show = json!({ "photos": [p[2], p[0], p[1]], "index": 1, "playing": false, "bar": true, "zoom": 2.5, "pan": [0.25, 0.75],
                       "transition": null, "from": "edit" });
    let viewer = json!({ "index": 4, "backdrop": "opaque", "controls": false, "zoom": 2.0 });
    let destination = json!({ "place": "/media/pictures", "folderName": { "dateTaken": true }, "after": "delete" });
    for change in [
        json!({ "rooms": ["library", "import"] }),
        json!({ "rooms": ["library", "import"], "room": "import" }),
        json!({ "import": { "focus": 7, "selection": [7, 3, 9], "preview": 3 } }),
        json!({ "import": { "destination": destination } }),
        json!({ "import": { "destination": { "place": null, "folderName": "Holiday", "after": "leave" } } }),
        json!({ "import": { "destination": { "place": null, "folderName": { "today": true }, "after": "leave" } } }),
        json!({ "view": "slideshow", "slideshow": show }),
        json!({ "peek": p[3] }),
        json!({ "view": "viewer", "viewer": viewer }),
        json!({ "view": "edit", "edit": { "photo": p[0], "compare": "two", "second": p[1], "focus": "right", "layout": "stacked", "showOriginal": true } }),
    ] {
        let reply = ok(&mut t, "ui.set", change.clone());
        let now = state(&mut t);
        for (field, value) in change.as_object().unwrap() {
            // `import` and `edit` merge field by field: each field set reads back as set.
            match value.as_object().filter(|_| field == "import" || field == "edit") {
                Some(fields) => {
                    for (k, v) in fields {
                        assert_eq!(&now[field][k], v, "{field}.{k}");
                        assert_eq!(&reply[field][k], v, "the reply is the new state: {field}.{k}");
                    }
                }
                None => assert_eq!(&now[field], value, "{field}"),
            }
        }
        // Back to the defaults, and nothing else moved.
        let back: serde_json::Map<String, Value> = change.as_object().unwrap().keys().map(|k| (k.clone(), defaults[k].clone())).collect();
        ok(&mut t, "ui.set", Value::Object(back));
        assert_eq!(state(&mut t), defaults, "{change}");
    }
    // A partial slideshow and viewer take the defaults for the rest.
    ok(&mut t, "ui.set", json!({ "view": "slideshow", "slideshow": { "photos": [p[0]] } }));
    let s = state(&mut t)["slideshow"].clone();
    assert_eq!(
        (&s["index"], &s["playing"], &s["zoom"], &s["pan"], &s["from"]),
        (&json!(0), &json!(true), &json!(1.0), &json!([0.5, 0.5]), &json!("library"))
    );
    ok(&mut t, "ui.set", json!({ "view": "viewer", "slideshow": null, "viewer": {} }));
    assert_eq!(state(&mut t)["viewer"], json!({ "index": 0, "backdrop": "translucent", "controls": true, "zoom": "fit" }));
    // Repeated ids are kept once; the Library tab is always first.
    ok(&mut t, "ui.set", json!({ "view": "library", "viewer": null, "import": { "selection": [5, 5, 2] } }));
    assert_eq!(state(&mut t)["import"]["selection"], json!([5, 2]));
}

/// Bad M11 values are `bad_params` (photos that do not exist `not_found`), and nothing changes.
#[test]
fn bad_m11_values_are_refused_and_change_nothing() {
    let mut t = setup();
    let p = t.photos.clone();
    ok(&mut t, "ui.set", json!({ "rooms": ["library", "import"], "import": { "selection": [1] } }));
    let before = state(&mut t);
    let long = "x".repeat(4097);
    for bad in [
        json!({ "room": "collage" }),
        json!({ "room": "attic" }),
        json!({ "rooms": ["import"] }),
        json!({ "rooms": ["library", "import", "import"] }),
        json!({ "rooms": ["library", "library"] }),
        json!({ "rooms": [] }),
        json!({ "rooms": ["library"], "room": "import" }),
        json!({ "import": { "selection": ["a"] } }),
        json!({ "import": { "focus": -1 } }),
        json!({ "import": { "colour": 1 } }),
        json!({ "import": { "destination": { "after": "shred" } } }),
        json!({ "import": { "destination": { "folderName": { "dateTaken": false } } } }),
        json!({ "import": { "destination": { "folderName": { "today": true, "dateTaken": true } } } }),
        json!({ "import": { "destination": { "folderName": 3 } } }),
        json!({ "import": { "destination": { "folderName": long } } }),
        json!({ "import": { "destination": { "place": "x".repeat(4097) } } }),
        json!({ "view": "slideshow" }),
        json!({ "slideshow": { "photos": [p[0]] } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [] } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "index": 1 } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "zoom": 0.5 } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "zoom": 9 } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "pan": [0.5, 1.5] } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "transition": { "from": 2, "progress": 0.5 } } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "transition": { "from": 0, "progress": 2 } } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "from": "attic" } }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0]], "speed": 2 } }),
        json!({ "view": "viewer" }),
        json!({ "view": "viewer", "viewer": { "zoom": 5 } }),
        json!({ "view": "viewer", "viewer": { "backdrop": "glass" } }),
        json!({ "view": "viewer", "viewer": { "index": -1 } }),
        json!({ "view": "spiral" }),
        json!({ "peek": "x" }),
        json!({ "edit": { "second": p[1] } }),
        json!({ "edit": { "focus": "middle" } }),
        json!({ "edit": { "layout": "grid" } }),
        json!({ "edit": { "showOriginal": "yes" } }),
        // A good field next to a bad one: neither is applied.
        json!({ "room": "library", "rooms": ["import"] }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
    let no_photo = p.iter().max().unwrap() + 100;
    for bad in [
        json!({ "peek": no_photo }),
        json!({ "view": "slideshow", "slideshow": { "photos": [p[0], no_photo] } }),
        json!({ "view": "edit", "edit": { "photo": p[0], "second": no_photo } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
}

/// The rect of widget `id` in the last frame, if it was drawn.
fn rect_of(t: &T, id: &str) -> Option<[f32; 4]> {
    t.d.widgets().into_iter().find(|w| w.id == id).map(|w| w.rect)
}

/// Ctrl+M opens the Import tab: the strip appears under the menu bar, 28 high, with the Library
/// and Import tabs, and the room's content replaces the library's; the Library tab brings the
/// library back under the strip; closing Import removes the strip.
#[test]
fn ctrl_m_opens_the_import_tab_and_the_strip_appears_28_high() {
    let mut t = setup();
    assert_eq!(rect_of(&t, "region:tabStrip"), None, "no room open: no strip");
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+M" }));
    let s = state(&mut t);
    assert_eq!((&s["room"], &s["rooms"]), (&json!("import"), &json!(["library", "import"])));
    assert_eq!(rect_of(&t, "region:tabStrip"), Some([0.0, 32.0, 1400.0, 28.0]));
    assert_eq!(rect_of(&t, "region:room"), Some([0.0, 60.0, 1400.0, 900.0 - 60.0 - 20.0]));
    assert_eq!(rect_of(&t, "region:statusLine"), Some([0.0, 880.0, 1400.0, 20.0]));
    for gone in ["region:toolbar", "region:tree", "region:lightTable", "region:tray"] {
        assert_eq!(rect_of(&t, gone), None, "{gone} is replaced by the room");
    }
    let tab = t.d.widgets().into_iter().find(|w| w.id == "tab:room/import").unwrap();
    assert!(tab.active && tab.enabled);
    assert!(rect_of(&t, "tab:room/library/close").is_none(), "the Library tab has no ×");
    assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty());
    // The Library tab: the library under the strip, the toolbar moved down by its height.
    ok(&mut t, "ui.clickWidget", json!({ "id": "tab:room/library" }));
    assert_eq!(state(&mut t)["room"], "library");
    assert_eq!(rect_of(&t, "region:toolbar"), Some([0.0, 60.0, 1400.0, 40.0]));
    assert_eq!(rect_of(&t, "region:tabStrip"), Some([0.0, 32.0, 1400.0, 28.0]));
    // Ctrl+M again brings Import to the front; its × closes it and the strip goes.
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+M" }));
    assert_eq!(state(&mut t)["room"], "import");
    ok(&mut t, "ui.clickWidget", json!({ "id": "tab:room/import/close" }));
    let s = state(&mut t);
    assert_eq!((&s["room"], &s["rooms"]), (&json!("library"), &json!(["library"])));
    assert_eq!(rect_of(&t, "region:tabStrip"), None);
    assert_eq!(rect_of(&t, "region:toolbar"), Some([0.0, 32.0, 1400.0, 40.0]));
}

/// Each view's keys act in their context only: Space, End and Esc run nothing of the slideshow's
/// in the library, X nothing of the Import room's; in a slideshow → advances and Esc exits; in the
/// Import room X answers for the room; in the viewer Space switches the background.
#[test]
fn a_views_keys_act_only_in_its_context() {
    let mut t = setup();
    let p = t.photos.clone();
    let not_playing = dalica_ui::commands::slideshow::NOT_PLAYING;
    for combo in ["Space", "End", "Esc", "Plus"] {
        ok(&mut t, "ui.key", json!({ "combo": combo }));
        assert_ne!(t.d.app().state().status, not_playing, "{combo} ran a slideshow command in the library");
    }
    ok(&mut t, "ui.key", json!({ "combo": "X" }));
    assert_ne!(t.d.app().state().status, dalica_ui::commands::import::NOT_YET, "X ran the Import room's command in the library");
    // A slideshow: → advances, Space pauses, End goes to the last photo, Esc exits to the library.
    ok(&mut t, "ui.set", json!({ "view": "slideshow", "slideshow": { "photos": [p[0], p[1], p[2]] } }));
    ok(&mut t, "ui.key", json!({ "combo": "Right" }));
    assert_eq!(state(&mut t)["slideshow"]["index"], 1);
    ok(&mut t, "ui.key", json!({ "combo": "Space" }));
    assert_eq!(state(&mut t)["slideshow"]["playing"], false);
    ok(&mut t, "ui.key", json!({ "combo": "End" }));
    assert_eq!(state(&mut t)["slideshow"]["index"], 2);
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+1" }));
    assert_eq!(t.d.app().state().thumb_size, dalica_ui::state::THUMB_DEFAULT, "a Library key does nothing in a slideshow");
    ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
    let s = state(&mut t);
    assert_eq!((&s["view"], &s["slideshow"]), (&json!("library"), &Value::Null));
    // The Import room in front: its X answers (its command waits for importing).
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+M" }));
    ok(&mut t, "ui.key", json!({ "combo": "X" }));
    assert_eq!(t.d.app().state().status, dalica_ui::commands::import::NOT_YET);
    ok(&mut t, "engine.execute", json!({ "command": "room.close" }));
    // The viewer: Space switches the background, Esc closes it.
    ok(&mut t, "ui.set", json!({ "view": "viewer", "viewer": {} }));
    ok(&mut t, "ui.key", json!({ "combo": "Space" }));
    assert_eq!(state(&mut t)["viewer"]["backdrop"], "opaque");
    ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
    assert_eq!((state(&mut t)["view"].as_str(), state(&mut t)["viewer"].is_null()), (Some("library"), true));
}

/// M11's stub dialogs are drawn with their ids and buttons, and Options shows its Slideshow and
/// Photo Viewer tabs.
#[test]
fn m11s_stub_dialogs_and_options_tabs_are_drawn_with_their_ids() {
    use dalica_ui::dialogs::{self, Dialog, DialogKind};
    let mut t = setup();
    for (kind, buttons) in [
        (DialogKind::KeepVersion, &["button:dialog.left", "button:dialog.right", "button:dialog.cancel"][..]),
        (DialogKind::ApplyChanges, &["button:dialog.apply", "button:dialog.discard", "button:dialog.cancel"][..]),
        (DialogKind::UndoAllEdits, &["button:dialog.removeEdits", "button:dialog.cancel"][..]),
        (DialogKind::RemoveFromCard, &["button:dialog.remove", "button:dialog.cancel"][..]),
        (DialogKind::ViewerSettings, &["button:dialog.ok", "button:dialog.cancel"][..]),
    ] {
        dialogs::open(t.d.app_mut(), Dialog::new(kind)).unwrap();
        ok(&mut t, "ui.wait", json!({ "ms": 50 }));
        let v = ok(&mut t, "ui.inspect", json!({}));
        assert_eq!(v["dialog"]["widget"], format!("dialog:{}", kind.name()));
        assert_eq!(v["dialog"]["buttons"], json!(buttons), "{kind:?}");
        for id in buttons {
            assert!(rect_of(&t, id).is_some(), "{kind:?}: {id} drawn");
        }
        // A button other than Cancel says what it waits for, and the dialog stays.
        if let Some(action) = buttons.first().and_then(|b| b.strip_prefix("button:dialog.")).filter(|a| *a != "cancel") {
            assert!(matches!(dialogs::answer(t.d.app_mut(), Some(action)), Err(CmdError::Disabled(_))), "{kind:?}");
        }
        ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
        assert!(t.d.app().state().dialogs.is_empty(), "{kind:?}: Esc closes it");
    }
    ok(&mut t, "engine.execute", json!({ "command": "app.preferences" }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    for (tab, id) in [("slideshow", "tab:options.slideshow"), ("viewer", "tab:options.viewer")] {
        ok(&mut t, "ui.clickWidget", json!({ "id": id }));
        assert_eq!(state(&mut t)["options"]["tab"], tab);
    }
}

/// The edit view on photo `p` with the Sharpen panel open, set through `ui.set`.
fn sharpen_panel(p: u64) -> Value {
    json!({ "view": "edit", "edit": { "photo": p, "tab": "effects", "tool": "effect",
            "effect": { "name": "sharpen", "params": { "a": 1.5 }, "drag": { "part": "centre", "from": [0.25, 0.75] },
                        "strokes": [{ "points": [[0.125, 0.25], [0.375, 0.5]], "radius": 0.125, "erase": true }], "brush": 0.25,
                        "erase": true, "picker": null, "eyedropper": null } } })
}

/// `edit.effect` round-trips through `ui.set` and `ui.state` with `edit.tool: "effect"`, and goes
/// back to `null` with it; a colour parameter's picker and eyedropper read back as set.
#[test]
fn the_effect_panel_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    let p = t.photos.clone();
    let defaults = state(&mut t);
    assert_eq!(defaults["edit"]["effect"], Value::Null);
    for change in [
        sharpen_panel(p[0]),
        json!({ "view": "edit", "edit": { "photo": p[0], "tool": "effect",
                "effect": { "name": "tint", "params": { "color": "#102030", "fade": 40.0 }, "drag": null, "strokes": [], "brush": 0.0625,
                            "erase": false, "picker": "color", "eyedropper": "color" } } }),
    ] {
        let reply = ok(&mut t, "ui.set", change.clone());
        let now = state(&mut t);
        for (k, v) in change["edit"].as_object().unwrap() {
            assert_eq!(&now["edit"][k], v, "edit.{k}");
            assert_eq!(&reply["edit"][k], v, "the reply is the new state: edit.{k}");
        }
        ok(&mut t, "ui.set", json!({ "view": "library", "edit": defaults["edit"].clone() }));
        assert_eq!(state(&mut t), defaults, "{change}");
    }
    // A partial panel takes the defaults for the rest.
    ok(&mut t, "ui.set", json!({ "view": "edit", "edit": { "photo": p[0], "tool": "effect", "effect": { "name": "grain" } } }));
    let mut e = state(&mut t)["edit"]["effect"].clone();
    let brush = e.as_object_mut().unwrap().remove("brush").and_then(|b| b.as_f64()).unwrap();
    assert!((brush - f64::from(dalica_ui::state::SOFTEN_BRUSH_DEFAULT)).abs() < 1e-6, "{brush}");
    assert_eq!(e, json!({ "name": "grain", "params": {}, "drag": null, "strokes": [], "erase": false, "picker": null, "eyedropper": null }));
}

/// Bad `edit.effect` values are `bad_params`, and nothing changes.
#[test]
fn bad_effect_values_are_refused_and_change_nothing() {
    let mut t = setup();
    let p = t.photos.clone();
    ok(&mut t, "ui.set", sharpen_panel(p[0]));
    let before = state(&mut t);
    let effect = |e: Value| json!({ "edit": { "effect": e } });
    let stroke = |points: Value, radius: f64| json!({ "name": "sharpen", "strokes": [{ "points": points, "radius": radius }] });
    for bad in [
        json!({ "edit": { "tool": null } }),
        json!({ "edit": { "tool": "crop" } }),
        json!({ "edit": { "effect": null } }),
        effect(json!({ "name": "nope" })),
        effect(json!({ "name": "crop" })),
        effect(json!({ "name": "x".repeat(5000) })),
        effect(json!({ "name": "sharpen", "params": { "a": 9 } })),
        effect(json!({ "name": "sharpen", "params": { "a": "high" } })),
        effect(json!({ "name": "sharpen", "params": { "amount": 1 } })),
        effect(json!({ "name": "grain", "params": { "lighten": 1 } })),
        effect(json!({ "name": "tint", "params": { "color": "blue" } })),
        effect(json!({ "name": "tint", "picker": "fade" })),
        effect(json!({ "name": "sharpen", "eyedropper": "a" })),
        effect(json!({ "name": "sharpen", "drag": { "part": "centre", "from": [2, 0] } })),
        effect(json!({ "name": "sharpen", "drag": { "part": "middle", "from": [0, 0] } })),
        effect(json!({ "name": "sharpen", "brush": 0 })),
        effect(stroke(json!([[0.5, 1.5]]), 0.1)),
        effect(stroke(json!([[0.5, 0.5]]), 9.0)),
        effect(stroke(json!(vec![[0.5, 0.5]; 1001]), 0.1)),
        effect(json!({ "name": "sharpen", "colour": 1 })),
        effect(json!("sharpen")),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad:.200}");
        assert_eq!(state(&mut t), before, "{bad:.200}");
    }
}

/// `edit.tool.open {tool: "effect", name: "sharpen"}` opens the stub panel in place of the grid:
/// its title, Apply │ Cancel, the draft at Sharpen's defaults; Esc closes it and the grid's empty
/// is back. A one-click effect has no panel.
#[test]
fn an_effect_panel_opens_and_esc_closes_it() {
    let mut t = setup();
    let p = t.photos.clone();
    ok(&mut t, "engine.execute", json!({ "command": "view.edit", "params": { "id": p[0] } }));
    ok(&mut t, "engine.execute", json!({ "command": "edit.tab", "params": { "tab": "effects" } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert!(rect_of(&t, "effect:sharpen").is_some(), "the grid");
    let opened = ok(&mut t, "engine.execute", json!({ "command": "edit.tool.open", "params": { "tool": "effect", "name": "sharpen" } }));
    assert_eq!(opened, json!({ "tool": "effect", "name": "sharpen" }), "{opened}");
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    let e = state(&mut t)["edit"].clone();
    assert_eq!((&e["tool"], &e["effect"]["name"], &e["effect"]["params"]), (&json!("effect"), &json!("sharpen"), &json!({ "a": 0.6 })));
    for id in ["button:panel.apply", "button:panel.cancel"] {
        assert!(rect_of(&t, id).is_some(), "{id} drawn");
    }
    assert!(rect_of(&t, "effect:sharpen").is_none(), "the panel replaces the grid");
    assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty());
    ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
    let e = state(&mut t)["edit"].clone();
    assert_eq!((&e["tool"], &e["effect"]), (&Value::Null, &Value::Null));
    assert_eq!(state(&mut t)["view"], "edit", "Esc closed the panel and stayed");
    assert!(rect_of(&t, "effect:sharpen").is_some());
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.tool.open", "params": { "tool": "effect", "name": "sepia" } })), "bad_params");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.tool.open", "params": { "tool": "effect" } })), "bad_params");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.tool.open", "params": { "tool": "crop", "name": "sharpen" } })), "bad_params");
}

/// Ctrl+Shift+B is the edit view's (B&W, `edit.bw`): in the library it runs nothing; in the edit
/// view it appends B&W to the open photo.
#[test]
fn ctrl_shift_b_does_nothing_in_the_library() {
    let mut t = setup();
    let p = t.photos.clone();
    let edits = |t: &mut T| ok(t, "engine.execute", json!({ "command": "photo.history.state", "params": { "id": p[0] } }))["edits"].clone();
    let before = state(&mut t);
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+Shift+B" }));
    assert_eq!(edits(&mut t), json!([]), "Ctrl+Shift+B ran edit.bw in the library");
    assert_eq!(state(&mut t), before);
    ok(&mut t, "engine.execute", json!({ "command": "view.edit", "params": { "id": p[0] } }));
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+Shift+B" }));
    assert_eq!(edits(&mut t), json!(["bw()"]));
}

/// Each M13 field round-trips through `ui.set` and `ui.state`; `unnamed`, `people`, `peoplePane`
/// and `places` merge field by field, `faceModel` is replaced whole.
#[test]
fn every_m13_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    let p = t.photos.clone();
    let defaults = state(&mut t);
    assert_eq!(defaults["unnamed"], json!({ "open": false, "expanded": false, "showIgnored": false, "selection": [], "names": {} }));
    assert_eq!(defaults["people"], json!({ "sort": "name", "zoomToFaces": [], "onlySuggestions": [] }));
    assert_eq!(defaults["peoplePane"], json!({ "selected": [], "hovered": null, "manual": null }));
    assert_eq!(
        defaults["places"],
        json!({ "centre": [0.0, 0.0], "zoom": 1.0, "pin": null, "bubble": null, "search": "", "results": [], "drop": null })
    );
    assert_eq!(defaults["faceModel"], Value::Null);
    let f0 = format!("{}.0", p[0]);
    let f1 = format!("{}.3", p[1]);
    for change in [
        json!({ "unnamed": { "open": true, "expanded": true, "showIgnored": true } }),
        json!({ "unnamed": { "selection": [f0, f1], "names": { "0": "Ana", "7": "Rui" } } }),
        json!({ "people": { "sort": "count", "zoomToFaces": ["p00a1", "p00b2"] } }),
        json!({ "people": { "sort": "top10", "onlySuggestions": ["p00a1"] } }),
        json!({ "peoplePane": { "selected": ["p00a1", "p00b2"], "hovered": f1 } }),
        json!({ "view": "edit", "edit": { "photo": p[0] }, "peoplePane": { "manual": { "rect": [0.125, 0.25, 0.5, 0.75], "name": "Ana" } } }),
        json!({ "places": { "centre": [38.7116, -9.1389], "zoom": 14.5 } }),
        json!({ "places": { "pin": { "at": [38.7116, -9.1389], "place": "Lisbon" }, "bubble": "pin" } }),
        json!({ "places": { "bubble": { "marker": 3 }, "search": "Lisbon", "results": [{ "name": "Lisbon", "at": [38.7, -9.1] }] } }),
        json!({ "places": { "drop": { "ids": [p[0], p[2]], "at": [51.5, -0.12] }, "bubble": "drop" } }),
        json!({ "faceModel": { "entry": "sface", "file": null, "accepted": true, "progress": 0.25 } }),
        json!({ "faceModel": { "entry": null, "file": "/models/face.onnx", "accepted": false, "progress": null } }),
    ] {
        let reply = ok(&mut t, "ui.set", change.clone());
        let now = state(&mut t);
        for (field, value) in change.as_object().unwrap() {
            match value.as_object().filter(|_| ["unnamed", "people", "peoplePane", "places", "edit"].contains(&field.as_str())) {
                Some(fields) => {
                    for (k, v) in fields {
                        assert_eq!(&now[field][k], v, "{field}.{k}");
                        assert_eq!(&reply[field][k], v, "the reply is the new state: {field}.{k}");
                    }
                }
                None => assert_eq!(&now[field], value, "{field}"),
            }
        }
        let back: serde_json::Map<String, Value> = change.as_object().unwrap().keys().map(|k| (k.clone(), defaults[k].clone())).collect();
        ok(&mut t, "ui.set", Value::Object(back));
        assert_eq!(state(&mut t), defaults, "{change}");
    }
    // A partial face model draft takes the defaults; repeated faces and people are kept once.
    ok(&mut t, "ui.set", json!({ "faceModel": { "accepted": true } }));
    assert_eq!(state(&mut t)["faceModel"], json!({ "entry": null, "file": null, "accepted": true, "progress": null }));
    ok(&mut t, "ui.set", json!({ "faceModel": null, "unnamed": { "selection": [f0, f0] }, "people": { "zoomToFaces": ["p00a1", "p00a1"] } }));
    let s = state(&mut t);
    assert_eq!((&s["unnamed"]["selection"], &s["people"]["zoomToFaces"]), (&json!([f0]), &json!(["p00a1"])));
}

/// Bad M13 values are `bad_params` (photos that do not exist `not_found`), and nothing changes.
#[test]
fn bad_m13_values_are_refused_and_change_nothing() {
    let mut t = setup();
    let p = t.photos.clone();
    let before = state(&mut t);
    let long = "x".repeat(4097);
    let f0 = format!("{}.0", p[0]);
    for bad in [
        json!({ "unnamed": { "selection": [format!("{}.256", p[0])] } }),
        json!({ "unnamed": { "selection": [p[0]] } }),
        json!({ "unnamed": { "selection": ["face"] } }),
        json!({ "unnamed": { "names": { "x": "Ana" } } }),
        json!({ "unnamed": { "names": { "0": long } } }),
        json!({ "unnamed": { "open": "yes" } }),
        json!({ "unnamed": { "colour": 1 } }),
        json!({ "people": { "sort": "age" } }),
        json!({ "people": { "zoomToFaces": ["ana"] } }),
        json!({ "people": { "onlySuggestions": ["a00a1"] } }),
        json!({ "peoplePane": { "selected": [1] } }),
        json!({ "peoplePane": { "hovered": "x" } }),
        json!({ "peoplePane": { "manual": { "rect": [0.1, 0.2, 0.4, 0.6] } } }),
        json!({ "view": "edit", "edit": { "photo": p[0] }, "peoplePane": { "manual": { "rect": [0.4, 0.2, 0.1, 0.6] } } }),
        json!({ "view": "edit", "edit": { "photo": p[0] }, "peoplePane": { "manual": { "rect": [0.1, 0.2, 0.4, 1.5] } } }),
        json!({ "view": "edit", "edit": { "photo": p[0] }, "peoplePane": { "manual": { "rect": [0.1, 0.2, 0.4, 0.6], "name": long } } }),
        json!({ "places": { "centre": [86, 0] } }),
        json!({ "places": { "centre": [0, 181] } }),
        json!({ "places": { "centre": [0] } }),
        json!({ "places": { "zoom": 20 } }),
        json!({ "places": { "zoom": -1 } }),
        json!({ "places": { "pin": { "at": [91, 0] } } }),
        json!({ "places": { "bubble": "pin" } }),
        json!({ "places": { "bubble": "drop" } }),
        json!({ "places": { "bubble": { "marker": -1 } } }),
        json!({ "places": { "bubble": "balloon" } }),
        json!({ "places": { "search": long } }),
        json!({ "places": { "results": [{ "name": "x", "at": [0, 200] }] } }),
        json!({ "places": { "drop": { "ids": [], "at": [0, 0] } } }),
        json!({ "places": { "drop": { "ids": [p[0]], "at": [0, 200] } } }),
        json!({ "faceModel": { "entry": "a", "file": "/m.onnx" } }),
        json!({ "faceModel": { "entry": "" } }),
        json!({ "faceModel": { "progress": 2 } }),
        json!({ "faceModel": { "licence": true } }),
        // A good field next to a bad one: neither is applied.
        json!({ "unnamed": { "open": true }, "places": { "zoom": 99 } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
    let no_photo = p.iter().max().unwrap() + 100;
    for bad in [
        json!({ "unnamed": { "selection": [f0, format!("{no_photo}.0")] } }),
        json!({ "peoplePane": { "hovered": format!("{no_photo}.1") } }),
        json!({ "places": { "drop": { "ids": [p[0], no_photo], "at": [0, 0] } } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
}

/// `pane.people` opens the People pane and `pane.places` the Places pane, each registering its
/// region (M13.24, M13.22); `unnamed.open` shows the Unnamed grid in the light table's place, and
/// its header's toggles act.
#[test]
fn the_people_and_places_panes_and_the_unnamed_grid_open_as_stubs() {
    let mut t = setup();
    for (command, pane, region) in [("pane.people", "pane:people", "people"), ("pane.places", "pane:places", "places")] {
        ok(&mut t, "engine.execute", json!({ "command": command }));
        ok(&mut t, "ui.wait", json!({ "ms": 50 }));
        assert!(rect_of(&t, pane).is_some_and(|r| r[2] > 0.0 && r[3] > 0.0), "{pane}");
        assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty(), "{pane}");
        let widgets = ok(&mut t, "ui.widgets", json!({ "region": region }));
        // Each pane registers its body as its region.
        assert_eq!(rect_of(&t, &format!("region:{region}")), rect_of(&t, pane), "{region}");
        assert_eq!(ok(&mut t, "ui.inspect", json!({}))["regions"][region], json!(rect_of(&t, pane)), "{region}");
        if region == "places" {
            // The map is in it (M13.22).
            assert!(widgets.as_array().unwrap().iter().any(|w| w["id"] == "map:places"), "{widgets}");
        }
    }
    ok(&mut t, "engine.execute", json!({ "command": "pane.places" }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert_eq!(rect_of(&t, "pane:places"), None, "closed");
    // The Unnamed grid replaces the light table.
    let thumb = format!("thumb:{}", t.photos[0]);
    assert!(rect_of(&t, &thumb).is_some());
    ok(&mut t, "ui.set", json!({ "unnamed": { "open": true } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert!(rect_of(&t, &thumb).is_none(), "no thumbnails under the grid");
    assert!(rect_of(&t, "region:lightTable").is_some());
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:faces.showIgnored" }));
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:faces.expandGroups" }));
    let s = state(&mut t);
    assert_eq!((&s["unnamed"]["showIgnored"], &s["unnamed"]["expanded"]), (&json!(true), &json!(true)));
    assert!(t.d.widgets().into_iter().any(|w| w.id == "button:faces.showIgnored" && w.active), "the toggle shows it is on");
    assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty());
    // Delete in the grid deletes no photo.
    let first = t.photos[0];
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [first] } }));
    ok(&mut t, "ui.key", json!({ "combo": "Delete" }));
    assert!(t.d.app().state().dialogs.is_empty(), "no delete question");
}

/// The item rows of a menu model as `ui.menu.tree` reports them, submenus walked.
fn rows_of(nodes: &Value) -> Vec<Value> {
    fn walk(nodes: &Value, out: &mut Vec<Value>) {
        for n in nodes.as_array().unwrap() {
            match n.get("children") {
                Some(children) => walk(children, out),
                None if n.get("id").is_some() => out.push(n.clone()),
                None => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

/// The face and person menus and M13's Picture, Tools and View items are listed and greyed until
/// their tasks make them act or there is something to act on, with the reason; View ▸ People and
/// Places are live.
#[test]
fn the_new_menu_items_are_listed_and_greyed() {
    use dalica_ui::commands::{faces, people};
    let mut t = setup();
    let first = t.photos[0];
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [first] } }));
    let face = menus::nodes_json(&menus::face_menu(t.d.app(), Platform::Other));
    let rows = rows_of(&face);
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["faces.moveToNewPerson", "faces.removeFromPerson", "people.thumbnail", "faces.ignoreSelected"]);
    assert!(face.as_array().unwrap().iter().any(|n| n["widget"] == "submenu:Face/Move to Person"), "{face}");
    for r in &rows {
        // M13.21 and M13.25 made the face rows act: with no face selected they wait for one.
        let why = if r["id"] == "people.thumbnail" { people::NO_FACE } else { faces::NO_FACES };
        assert_eq!((&r["live"], &r["enabled"], &r["reason"]), (&json!(true), &json!(false), &json!(why)), "{r}");
    }
    assert_eq!(rows[0]["dialog"], "newPerson");
    let person = ok(&mut t, "ui.menu.tree", json!({ "context": "person" }));
    assert_eq!(person, menus::nodes_json(&menus::person_menu(t.d.app(), Platform::current())));
    let rows = rows_of(&person);
    assert_eq!(rows.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(), [json!("people.rename"), json!("people.delete")]);
    for r in &rows {
        // M13.25 made them act: with no person's section selected they wait for one.
        assert_eq!((&r["live"], &r["enabled"], &r["reason"]), (&json!(true), &json!(false), &json!(people::NO_PERSON)), "{r}");
    }
    assert!(person.as_array().unwrap().iter().any(|n| n["widget"] == "submenu:Person/Merge Into"));
    // The bar: Tools ▸ Geotag's Clear Location… greyed with no located photo selected (M13.22),
    // Copy and Paste Location are the engine's (M13.14: Copy live, Paste greyed with nothing
    // copied); the other M13 rows not live yet; View ▸ People and Places live.
    let copy = row(&mut t, "geo.copy", Value::Null);
    assert_eq!((&copy["live"], &copy["enabled"]), (&json!(true), &json!(true)), "geo.copy: {copy}");
    let paste = row(&mut t, "geo.paste", Value::Null);
    assert_eq!((&paste["live"], &paste["enabled"]), (&json!(true), &json!(false)), "geo.paste: {paste}");
    assert!(paste["reason"].is_string(), "geo.paste: {paste}");
    // Picture ▸ Find Faces Again is the engine's (M13.18).
    let reset = row(&mut t, "faces.reset", Value::Null);
    assert_eq!((&reset["live"], &reset["enabled"]), (&json!(true), &json!(true)), "faces.reset: {reset}");
    for (id, live, reason) in [
        ("geo.clear", true, dalica_engine::cmd::places::NONE_LOCATED),
        ("faces.writeXmp", false, "Not available yet."),
        ("people.manager", false, "Not available yet."),
    ] {
        let r = row(&mut t, id, Value::Null);
        assert_eq!((&r["live"], &r["enabled"], &r["reason"]), (&json!(live), &json!(false), &json!(reason)), "{id}: {r}");
    }
    assert_eq!(row(&mut t, "geo.clear", Value::Null)["dialog"], "clearLocation");
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": "Tools/Geotag/Clear Location" })), "disabled");
    for id in ["pane.people", "pane.places"] {
        let r = row(&mut t, id, Value::Null);
        assert_eq!((&r["live"], &r["enabled"]), (&json!(true), &json!(true)), "{id}");
    }
    // The people sorts' marks follow the sort chosen.
    ok(&mut t, "ui.set", json!({ "people": { "sort": "count" } }));
    assert!(menus::checked(t.d.app(), people::SORT_ID, &json!({ "by": "count" })));
    assert!(!menus::checked(t.d.app(), people::SORT_ID, &json!({ "by": "name" })));
}

/// M13's stub dialogs are drawn with their ids and buttons, their default buttons say what they
/// wait for, and Options shows its Network and People tabs.
#[test]
fn m13s_stub_dialogs_and_options_tabs_are_drawn_with_their_ids() {
    use dalica_ui::dialogs::{self, Dialog, DialogKind};
    let mut t = setup();
    for (kind, buttons) in [
        // M13.21 drew these two (Cancel then the default button, rightmost); empty, they refuse.
        (DialogKind::NewPerson, &["button:dialog.cancel", "button:dialog.ok"][..]),
        (DialogKind::IgnoreFaces, &["button:dialog.cancel", "button:dialog.ignore"][..]),
        (DialogKind::RenamePerson, &["button:dialog.ok", "button:dialog.cancel"][..]),
        (DialogKind::MergePeople, &["button:dialog.merge", "button:dialog.cancel"][..]),
        (DialogKind::DeletePerson, &["button:dialog.delete", "button:dialog.cancel"][..]),
        (DialogKind::ChangeLocation, &["button:dialog.change", "button:dialog.cancel"][..]),
        (DialogKind::ClearLocation, &["button:dialog.remove", "button:dialog.cancel"][..]),
    ] {
        dialogs::open(t.d.app_mut(), Dialog::new(kind)).unwrap();
        ok(&mut t, "ui.wait", json!({ "ms": 50 }));
        let v = ok(&mut t, "ui.inspect", json!({}));
        assert_eq!(v["dialog"]["widget"], format!("dialog:{}", kind.name()));
        assert_eq!(v["dialog"]["buttons"], json!(buttons), "{kind:?}");
        for id in buttons {
            assert!(rect_of(&t, id).is_some(), "{kind:?}: {id} drawn");
        }
        assert!(matches!(dialogs::answer(t.d.app_mut(), None), Err(CmdError::Disabled(_))), "{kind:?}: the stub waits");
        ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
        assert!(t.d.app().state().dialogs.is_empty(), "{kind:?}: Esc closes it");
    }
    // The face model dialog: Download waits for the licence box.
    ok(&mut t, "engine.execute", json!({ "command": "faceModel.open", "params": { "entry": "sface" } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    let download = t.d.widgets().into_iter().find(|w| w.id == "button:dialog.download").unwrap();
    assert!(!download.enabled, "disabled until the box is ticked");
    ok(&mut t, "ui.set", json!({ "faceModel": { "entry": "sface", "accepted": true } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert!(t.d.widgets().into_iter().any(|w| w.id == "button:dialog.download" && w.enabled));
    ok(&mut t, "ui.key", json!({ "combo": "Esc" }));
    assert_eq!(state(&mut t)["faceModel"], Value::Null, "closing drops the draft");
    // Options: the Network and People tabs between Slideshow and Photo Viewer.
    ok(&mut t, "engine.execute", json!({ "command": "app.preferences", "params": { "tab": "network" } }));
    ok(&mut t, "ui.wait", json!({ "ms": 50 }));
    assert_eq!(state(&mut t)["options"]["tab"], "network");
    let x = |t: &T, id: &str| rect_of(t, id).unwrap()[0];
    assert!(x(&t, "tab:options.slideshow") < x(&t, "tab:options.network") && x(&t, "tab:options.network") < x(&t, "tab:options.people"));
    assert!(x(&t, "tab:options.people") < x(&t, "tab:options.viewer"));
    ok(&mut t, "ui.clickWidget", json!({ "id": "tab:options.people" }));
    assert_eq!(state(&mut t)["options"]["tab"], "people");
}
