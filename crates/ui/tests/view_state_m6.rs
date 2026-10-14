//! M6's view state (M6.6), driven through the control channel as an agent would: every new field of
//! `UiState` round-trips through `ui.state` / `ui.set` (bad values are `bad_params`, unknown ids
//! `not_found`, and nothing changes); each command that opens a dialog shows it as `dialog:<kind>`
//! and Esc closes it; Picture ▸ Edit Caption opens the caption editor; the Properties and Tags panes
//! open from their toggles and View items with their parts; the menu items light up and grey with
//! a reason; toasts reach `ui.inspect`; every new command fails gracefully; with every new pane and
//! dialog open at both window sizes each interactive widget has a valid, unique id.
//!
//! `DALICA_SHOTS=<dir>` writes screenshots of the panes and dialogs at 1400 × 900 and 900 × 600 there.

use std::path::Path;
use std::sync::Arc;

use dalica_engine::{CmdError, Session};
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::state::{AboutTab, DialogKind, Pane};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry;
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;

/// A session on a library under `dir` holding two folders of 3 scenes photos each; its photo ids
/// with their folders, and the folder ids.
fn session(dir: &Path) -> (Session, Vec<(u64, u64)>, Vec<u64>) {
    let files = dalica_scenes::demo_library(2, 6, 5).unwrap();
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
    let homes: Vec<(u64, u64)> =
        found["photos"].as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), p["folder"].as_u64().unwrap())).collect();
    let mut folders: Vec<u64> = homes.iter().map(|(_, f)| *f).collect();
    folders.sort_unstable();
    folders.dedup();
    assert_eq!((homes.len(), folders.len()), (6, 2));
    (s, homes, folders)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    homes: Vec<(u64, u64)>,
    folders: Vec<u64>,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, homes, folders) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = Driver::new(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, homes, folders }
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

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

/// Writes a screenshot to `$DALICA_SHOTS/<name>-<w>x<h>.png` at both window sizes, when asked.
fn shots(t: &mut T, name: &str) {
    let Some(dir) = std::env::var_os("DALICA_SHOTS") else { return };
    // A narrowing window closes the side pane: open it again at each size, as a user would.
    let pane = state(t)["pane"].clone();
    for (w, h) in [(1400, 900), (900, 600)] {
        ok(t, "ui.resize", json!({ "width": w, "height": h }));
        ok(t, "ui.set", json!({ "pane": pane }));
        let path = Path::new(&dir).join(format!("{name}-{w}x{h}.png"));
        let _ = std::fs::remove_file(&path);
        t.control = Control::new(Config { write_roots: vec![Path::new(&dir).to_path_buf()], ..Config::default() });
        ok(t, "ui.screenshot", json!({ "path": path }));
    }
    ok(t, "ui.resize", json!({ "width": 1400, "height": 900 }));
    ok(t, "ui.set", json!({ "pane": pane }));
}

/// Each new field round-trips through `ui.set` and `ui.state`, alone and together, and goes back
/// to its default without moving anything else.
#[test]
fn every_m6_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup();
    let (photo, folder) = t.homes[4];
    let defaults = state(&mut t);
    assert_eq!((&defaults["captionEdit"], &defaults["descriptionEdit"], &defaults["propertiesShowAll"]), (&Value::Null, &Value::Null, &json!(false)));
    assert_eq!(defaults["tagsPane"], json!({ "draft": "", "highlight": null }));
    assert_eq!(defaults["about"], json!({ "tab": "about", "list": "grabBag", "sort": "firstCommit", "ascending": true, "names": "username" }));
    let about = |tab: &str, list: &str, sort: &str, ascending: bool, names: &str| json!({ "tab": tab, "list": list, "sort": sort, "ascending": ascending, "names": names });
    let mut cases = vec![
        ("captionEdit", json!({ "photo": photo, "draft": "Tram 28 at Graça" })),
        ("captionEdit", json!({ "photo": photo, "draft": "" })),
        ("descriptionEdit", json!({ "section": format!("folder:{folder}"), "draft": "Lisbon, spring" })),
        ("propertiesShowAll", json!(true)),
        ("tagsPane", json!({ "draft": "bea", "highlight": 0 })),
        ("tagsPane", json!({ "draft": "lisbon, tram", "highlight": null })),
        ("about", about("contributors", "table", "name", false, "displayName")),
        ("about", about("models", "grabBag", "lastCommit", true, "realName")),
    ];
    for sort in ["prs", "commits", "linesAdded", "linesDeleted", "linesDelta", "binaryAdded", "binaryDeleted", "firstCommit"] {
        cases.push(("about", about("contributors", "table", sort, true, "username")));
    }
    for (field, value) in cases {
        let reply = ok(&mut t, "ui.set", json!({ field: value }));
        assert_eq!(reply[field], value, "the reply is the new state: {field}");
        assert_eq!(state(&mut t)[field], value, "{field}");
        ok(&mut t, "ui.set", json!({ field: defaults[field] }));
        assert_eq!(state(&mut t), defaults, "{field}");
    }
    // Several at once.
    let many = json!({ "propertiesShowAll": true, "tagsPane": { "draft": "x", "highlight": 3 }, "about": about("models", "table", "commits", false, "realName") });
    ok(&mut t, "ui.set", many.clone());
    let now = state(&mut t);
    for (k, v) in many.as_object().unwrap() {
        assert_eq!(&now[k], v, "{k}");
    }
    let s = t.d.state();
    assert!(s.properties_show_all && s.tags_pane.highlight == Some(3) && s.about.tab == AboutTab::Models);
    // A draft at the limit is kept whole.
    let full = "é".repeat(dalica_ui::state::MAX_DRAFT);
    ok(&mut t, "ui.set", json!({ "tagsPane": { "draft": full, "highlight": null } }));
    assert_eq!(t.d.state().tags_pane.draft.chars().count(), dalica_ui::state::MAX_DRAFT);
}

/// Bad values are `bad_params`, ids that do not exist `not_found`, and nothing changes; the open
/// dialogs stay read-only.
#[test]
fn bad_m6_values_are_refused_and_change_nothing() {
    let mut t = setup();
    let (photo, folder) = t.homes[0];
    ok(&mut t, "ui.set", json!({ "propertiesShowAll": true, "captionEdit": { "photo": photo, "draft": "kept" } }));
    let before = state(&mut t);
    let long = "x".repeat(dalica_ui::state::MAX_DRAFT + 1);
    for bad in [
        json!({ "captionEdit": { "photo": photo } }),
        json!({ "captionEdit": { "photo": "x", "draft": "" } }),
        json!({ "captionEdit": { "photo": photo, "draft": 3 } }),
        json!({ "captionEdit": { "photo": photo, "draft": "", "extra": 1 } }),
        json!({ "captionEdit": { "photo": photo, "draft": long } }),
        json!({ "captionEdit": 5 }),
        json!({ "descriptionEdit": { "section": "album:x", "draft": "" } }),
        json!({ "descriptionEdit": { "section": folder, "draft": "" } }),
        json!({ "descriptionEdit": { "section": format!("folder:{folder}"), "draft": long } }),
        json!({ "descriptionEdit": { "draft": "" } }),
        json!({ "propertiesShowAll": "yes" }),
        json!({ "propertiesShowAll": null }),
        json!({ "tagsPane": { "draft": "", "highlight": -1 } }),
        json!({ "tagsPane": { "draft": "", "highlight": 100 } }),
        json!({ "tagsPane": { "draft": "", "highlight": 1.5 } }),
        json!({ "tagsPane": { "draft": long, "highlight": null } }),
        json!({ "tagsPane": "beach" }),
        json!({ "about": { "tab": "credits", "list": "table", "sort": "name", "ascending": true, "names": "username" } }),
        json!({ "about": { "tab": "about", "list": "grid", "sort": "name", "ascending": true, "names": "username" } }),
        json!({ "about": { "tab": "about", "list": "table", "sort": "stars", "ascending": true, "names": "username" } }),
        json!({ "about": { "tab": "about", "list": "table", "sort": "name", "ascending": "no", "names": "username" } }),
        json!({ "about": { "tab": "about", "list": "table", "sort": "name", "ascending": true, "names": "nickname" } }),
        json!({ "about": { "tab": "about" } }),
        json!({ "dialogs": [{ "kind": "about", "fields": {} }] }),
        // A good field next to a bad one: neither is applied.
        json!({ "propertiesShowAll": false, "tagsPane": { "draft": "", "highlight": 100 } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
    let (no_photo, no_folder) = (t.homes.iter().map(|h| h.0).max().unwrap() + 100, t.folders.iter().max().unwrap() + 100);
    for bad in [
        json!({ "captionEdit": { "photo": no_photo, "draft": "" } }),
        json!({ "descriptionEdit": { "section": format!("folder:{no_folder}"), "draft": "" } }),
        json!({ "propertiesShowAll": false, "captionEdit": { "photo": u64::MAX, "draft": "" } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}");
    }
}

/// Each opener shows its dialog as `dialog:<kind>` and Esc closes it; Close answers About and
/// Release Notes; Picture ▸ Edit Caption opens the caption editor on the focused photo.
#[test]
fn each_opener_shows_its_dialog_and_esc_closes_it() {
    let mut t = setup();
    let (photo, folder) = t.homes[1];
    let other = t.homes.iter().find(|(p, f)| *f == folder && *p != photo).unwrap().0;
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [other, photo] } }));
    let openers = [
        ("section.properties", json!({}), DialogKind::FolderProperties, "Folder Properties"),
        ("photo.adjustDate", json!({}), DialogKind::AdjustDate, "Adjust Picture Date — 2 pictures"),
        ("quicktags.configure", json!({}), DialogKind::QuickTags, "Configure Quick Tags"),
        ("tag.rename", json!({ "tag": "beach" }), DialogKind::RenameTag, "Rename Tag"),
        ("app.about", json!({}), DialogKind::About, "About Dalica"),
        ("help.releaseNotes", json!({}), DialogKind::ReleaseNotes, "Release Notes"),
    ];
    for (id, params, kind, title) in openers {
        assert_eq!(run(&mut t, id, params.clone()), json!({ "dialog": kind.name() }), "{id}");
        t.d.settle(FRAMES).unwrap();
        let widget = format!("dialog:{}", kind.name());
        assert!(has(&t, &widget), "{id}: no {widget}");
        assert!(has(&t, &format!("{widget}/backdrop")) && has(&t, "button:dialog.cancel"), "{id}");
        let inspect = ok(&mut t, "ui.inspect", json!({}));
        assert_eq!((inspect["dialog"]["widget"].as_str(), inspect["dialog"]["title"].as_str()), (Some(widget.as_str()), Some(title)), "{id}");
        shots(&mut t, &format!("dialog-{}", kind.name()));
        ok(&mut t, "ui.key", json!({ "key": "Escape" }));
        assert!(t.d.state().dialogs.is_empty(), "{id}: Esc closes it");
        assert!(!has(&t, &widget), "{id}");
    }
    // The drafts start from the selection.
    run(&mut t, "photo.adjustDate", json!({}));
    let draft = ok(&mut t, "ui.inspect", json!({}))["dialog"]["fields"].clone();
    assert_eq!((draft["ids"].clone(), draft["mode"].clone()), (json!([other, photo]), json!("shift")));
    ok(&mut t, "ui.dialog.cancel", json!({}));
    run(&mut t, "section.properties", json!({}));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["dialog"]["fields"]["folder"], json!(folder));
    ok(&mut t, "ui.dialog.cancel", json!({}));
    // The menu items open theirs, as a click does.
    for (path, kind) in
        [("Folder/Edit Description…", "folderProperties"), ("Tools/Adjust Date and Time…", "adjustDate"), ("Help/Release Notes", "releaseNotes")]
    {
        assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": path })), json!({ "dialog": kind }), "{path}");
        assert!(has(&t, &format!("dialog:{kind}")), "{path}");
        ok(&mut t, "ui.dialog.cancel", json!({}));
    }
    let about = if cfg!(target_os = "macos") { "Dalica/About Dalica" } else { "Help/About Dalica" };
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": about })), json!({ "dialog": "about" }));
    // About's tabs switch the view state, which `ui.set` drives too; Close closes it.
    t.d.click("tab:about.contributors").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().about.tab, AboutTab::Contributors);
    ok(&mut t, "ui.set", json!({ "about": { "tab": "models", "list": "grabBag", "sort": "firstCommit", "ascending": true, "names": "username" } }));
    assert!(t.d.widgets().iter().any(|w| w.id == "tab:about.models" && w.active));
    t.d.click("button:dialog.cancel").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().dialogs.is_empty());
    assert_eq!(t.d.state().about.tab, AboutTab::Models, "the tab is kept for the next opening");

    // Edit Caption: the focused photo's editor, by command and by menu.
    let r = ok(&mut t, "ui.menu.invoke", json!({ "path": "Picture/Edit Caption" }));
    assert_eq!(r, json!({ "captionEdit": { "photo": photo, "draft": "" } }));
    assert_eq!(state(&mut t)["captionEdit"], json!({ "photo": photo, "draft": "" }));
    run(&mut t, "photo.caption.edit", json!({ "id": other }));
    assert_eq!(t.d.state().caption_edit.as_ref().map(|e| e.photo.get()), Some(other));
}

/// The menu items of the new commands are live; those that need a selection are greyed with the
/// reason until there is one.
#[test]
fn the_menu_items_light_up() {
    let mut t = setup();
    let row = |t: &mut T, id: &str| -> Value {
        fn find(nodes: &[Value], id: &str) -> Option<Value> {
            nodes.iter().find_map(|n| match n["children"].as_array() {
                Some(children) => find(children, id),
                None => (n["id"] == id).then(|| n.clone()),
            })
        }
        let tree = ok(t, "ui.menu.tree", json!({}));
        find(tree.as_array().unwrap(), id).unwrap_or_else(|| panic!("no row {id}"))
    };
    for (id, reason) in [
        ("photo.caption.edit", Some("Select a photo first.")),
        ("section.properties", Some("Select a folder first.")),
        ("photo.adjustDate", Some("Select some photos first.")),
        ("help.releaseNotes", None),
        ("app.about", None),
    ] {
        let r = row(&mut t, id);
        assert_eq!(r["live"], true, "{id}: {r}");
        assert_eq!(r["enabled"], reason.is_none(), "{id}: {r}");
        assert_eq!(r["reason"].as_str(), reason, "{id}: {r}");
    }
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": "Picture/Edit Caption" })), "disabled");
    let (photo, _) = t.homes[0];
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [photo] } }));
    for id in ["photo.caption.edit", "section.properties", "photo.adjustDate"] {
        let r = row(&mut t, id);
        assert_eq!((&r["live"], &r["enabled"]), (&json!(true), &json!(true)), "{id}: {r}");
    }
    // `engine.commands` says the same.
    let all = ok(&mut t, "engine.commands", json!({ "filter": "quicktags" }));
    assert_eq!((all[0]["id"].as_str(), all[0]["ui"].as_bool(), all[0]["enabled"].as_bool()), (Some("quicktags.configure"), Some(true), Some(true)));
    ok(&mut t, "engine.execute", json!({ "command": "select.none" }));
    let listed = ok(&mut t, "engine.commands", json!({ "filter": "adjustDate" }));
    assert_eq!(listed[0]["enabled"], false);
}

/// The Properties and Tags panes open from their toggles and View items and draw their parts; Show
/// All and the quick-tags gear work by id; the field edits the Tags pane's draft.
#[test]
fn the_properties_and_tags_panes_open_from_their_toggles_and_view_items() {
    let mut t = setup();
    let (photo, _) = t.homes[2];
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [photo] } }));
    // Properties from its toggle.
    t.d.click("toggle:pane.properties").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().pane, Some(Pane::Properties));
    assert!(has(&t, "pane:properties") && has(&t, "button:properties.showAll"));
    shots(&mut t, "pane-properties");
    t.d.click("button:properties.showAll").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().properties_show_all);
    assert!(t.d.widgets().iter().any(|w| w.id == "button:properties.showAll" && w.active));
    // Tags from View ▸ Tags.
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Tags" }));
    assert_eq!(t.d.state().pane, Some(Pane::Tags));
    for id in ["pane:tags", "field:tags.add", "button:tags.add", "button:quicktags.configure"] {
        assert!(has(&t, id), "{id}");
    }
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:tags.add").unwrap().enabled, "+ waits for text");
    t.d.click("field:tags.add").unwrap();
    t.d.type_text("beach").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().tags_pane.draft, "beach");
    assert!(t.d.widgets().iter().find(|w| w.id == "button:tags.add").unwrap().enabled);
    shots(&mut t, "pane-tags");
    t.d.click("button:quicktags.configure").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::QuickTags));
    ok(&mut t, "ui.key", json!({ "key": "Escape" }));
    // The toggle and the View item close them again.
    t.d.click("toggle:pane.tags").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().pane, None);
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Properties" }));
    assert_eq!(t.d.state().pane, Some(Pane::Properties));
    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Properties" }));
    assert_eq!(t.d.state().pane, None);
}

/// A toast pushed by the app shows as `toast:<n>` and is reported by `ui.inspect` → `toasts` until
/// its time is over.
#[test]
fn toasts_reach_ui_inspect() {
    let mut t = setup();
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["toasts"], json!([]));
    t.d.app_mut().notices_mut().push("Could not write the caption of IMG_0001.jpg", dalica_ui_kit::widgets::ToastKind::Error, 0.0);
    // The toast was shown at time 0: the frame clock has passed that, so it starts from now.
    let now = t.d.ctx().input(|i| i.time);
    t.d.app_mut().notices_mut().push("Starred 1 photo", dalica_ui_kit::widgets::ToastKind::Info, now);
    t.d.step();
    let toasts = ok(&mut t, "ui.inspect", json!({}))["toasts"].clone();
    let texts: Vec<&str> = toasts.as_array().unwrap().iter().filter_map(|v| v["text"].as_str()).collect();
    assert!(texts.contains(&"Starred 1 photo"), "{toasts}");
    let id = toasts.as_array().unwrap().iter().find(|v| v["text"] == "Starred 1 photo").unwrap()["id"].as_str().unwrap().to_string();
    assert!(t.d.widgets().iter().any(|w| w.id.starts_with("toast:")), "a toast is drawn");
    t.d.settle(FRAMES).unwrap();
    for _ in 0..200 {
        t.d.step();
    }
    let toasts = ok(&mut t, "ui.inspect", json!({}))["toasts"].clone();
    assert!(!toasts.as_array().unwrap().iter().any(|v| v["id"] == id), "the info toast is over: {toasts}");
}

/// Hostile parameters for every new command: an error, never a panic, and the state is unchanged.
#[test]
fn every_new_command_fails_gracefully() {
    let mut t = setup();
    let hostile = [
        json!([1]),
        json!("x"),
        json!(7),
        json!(true),
        json!({ "zzz": 1 }),
        json!({ "id": "1" }),
        json!({ "id": -1 }),
        json!({ "ids": "1" }),
        json!({ "ids": [-1] }),
        json!({ "ids": [1.5] }),
        json!({ "folder": "x" }),
        json!({ "folder": -1 }),
        json!({ "tag": 3 }),
        json!({ "tag": "" }),
        json!({ "tag": null }),
        json!({ "tag": "x".repeat(10_000) }),
    ];
    let new = ["photo.caption.edit", "section.properties", "photo.adjustDate", "quicktags.configure", "tag.rename", "app.about", "help.releaseNotes"];
    let before = t.d.state().clone();
    for id in new {
        for params in &hostile {
            match t.d.app_mut().run(id, params) {
                Err(CmdError::BadParams(_) | CmdError::NotFound(_) | CmdError::Disabled(_)) => {}
                other => panic!("{id} {params}: {other:?}"),
            }
        }
    }
    // Targets that do not exist.
    for (id, params) in [
        ("photo.caption.edit", json!({ "id": u64::MAX })),
        ("section.properties", json!({ "folder": u64::MAX })),
        ("photo.adjustDate", json!({ "ids": [u64::MAX] })),
    ] {
        assert!(matches!(t.d.app_mut().run(id, &params), Err(CmdError::NotFound(_))), "{id}");
    }
    // Nothing to work on: the reason.
    for id in ["photo.caption.edit", "section.properties", "photo.adjustDate"] {
        assert!(matches!(t.d.app_mut().run(id, &json!({})), Err(CmdError::Disabled(_))), "{id}");
    }
    assert!(matches!(t.d.app_mut().run("photo.adjustDate", &json!({ "ids": [] })), Err(CmdError::Disabled(_))));
    let mut after = t.d.state().clone();
    after.status.clone_from(&before.status);
    assert_eq!(after, before, "nothing changed");
    // A dialog already open is refused, not opened twice.
    t.d.app_mut().run("app.about", &json!({})).unwrap();
    assert!(matches!(t.d.app_mut().run("app.about", &json!({})), Err(CmdError::Disabled(_))));
    assert_eq!(t.d.state().dialogs.len(), 1);
    // Without a library the dialogs that need one find nothing.
    let mut bare = App::new(None);
    assert!(matches!(bare.run("section.properties", &json!({ "folder": 1 })), Err(CmdError::NotFound(_))));
    assert!(matches!(bare.run("photo.caption.edit", &json!({ "id": 1 })), Err(CmdError::NotFound(_))));
    assert!(bare.run("help.releaseNotes", &json!({})).is_ok());
}

/// With each new pane and dialog open, at both window sizes, every interactive widget has an id in
/// the spec grammar, unique within its frame.
#[test]
fn the_new_panes_and_dialogs_register_every_widget() {
    for (w, h) in [(1400, 900), (900, 600)] {
        let mut t = setup();
        ok(&mut t, "ui.resize", json!({ "width": w, "height": h }));
        let (photo, _) = t.homes[0];
        ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [photo] } }));
        let check = |t: &T, what: &str| {
            let missing = t.d.app().panels().unwrap().unregistered_widgets();
            assert_eq!(missing, [], "{w}×{h}, {what}: widgets without an id");
            let mut seen = std::collections::HashSet::new();
            for wi in t.d.widgets() {
                assert!(registry::is_valid_id(&wi.id), "{w}×{h}, {what}: `{}`", wi.id);
                assert!(seen.insert(wi.id.clone()), "{w}×{h}, {what}: `{}` twice", wi.id);
            }
        };
        for pane in ["pane.properties", "pane.tags"] {
            run(&mut t, pane, json!({}));
            t.d.settle(FRAMES).unwrap();
            assert!(t.d.state().pane.is_some(), "{w}×{h}: {pane} (900 wide fits a 280 pane)");
            check(&t, pane);
        }
        for (id, params) in [
            ("section.properties", json!({})),
            ("photo.adjustDate", json!({})),
            ("quicktags.configure", json!({})),
            ("tag.rename", json!({ "tag": "beach" })),
            ("app.about", json!({})),
            ("help.releaseNotes", json!({})),
        ] {
            run(&mut t, id, params);
            t.d.settle(FRAMES).unwrap();
            check(&t, id);
            ok(&mut t, "ui.dialog.cancel", json!({}));
        }
    }
}
