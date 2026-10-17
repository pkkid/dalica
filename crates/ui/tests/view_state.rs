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
        json!({ "options": { "tab": "people" } }),
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
    let show = json!({ "photos": [p[2], p[0], p[1]], "index": 1, "playing": false, "bar": true, "zoom": 2.5, "pan": [0.25, 0.75],
                       "transition": { "from": 0, "progress": 0.5 }, "from": "edit" });
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
    let tab = t.d.widgets().into_iter().find(|w| w.id == "tab:room.import").unwrap();
    assert!(tab.active && tab.enabled);
    assert!(rect_of(&t, "button:room.close.library").is_none(), "the Library tab has no ×");
    assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty());
    // The Library tab: the library under the strip, the toolbar moved down by its height.
    ok(&mut t, "ui.clickWidget", json!({ "id": "tab:room.library" }));
    assert_eq!(state(&mut t)["room"], "library");
    assert_eq!(rect_of(&t, "region:toolbar"), Some([0.0, 60.0, 1400.0, 40.0]));
    assert_eq!(rect_of(&t, "region:tabStrip"), Some([0.0, 32.0, 1400.0, 28.0]));
    // Ctrl+M again brings Import to the front; its × closes it and the strip goes.
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+M" }));
    assert_eq!(state(&mut t)["room"], "import");
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:room.close.import" }));
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
