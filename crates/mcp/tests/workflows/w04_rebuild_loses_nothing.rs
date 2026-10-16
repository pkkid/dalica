//! W04, Rebuild loses nothing (`mcp.md` §9; `data-model.md` §10.1): a star, a caption, tags, a
//! hidden photo, an edit and an album, then Rebuild Library: `doc_inspect` and every photo's
//! `photo_info` are the same before and after, and the folder's sidecar and photo files with them.
//!
//! A rebuild may give photos and folders new ids (`engine` scanning: "ids change"), so both sides
//! are compared by path: photo and folder ids are left out and a folder's section is named by its
//! path. What a rebuild resets by design is left out too: the undo history (cleared), the library
//! revision, the write queue and the scan progress. The hidden photo is listed through View ▸ Hidden Pictures, kept in
//! the library's `settings.json` between two sessions as W09 does (headless mode has no View menu).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, bytes, eventually, photo_name, sidecar, sidecar_entry};

/// What a rebuild resets by design (`doc_inspect` keys): the library revision, the undo history,
/// the write queue and the last scan's progress counters (the rebuild's own scan).
const RESET: &[&str] = &["revision", "undo", "pendingWrites", "scan"];

/// Stores View ▸ Hidden Pictures on in the library's settings, keeping every other setting.
fn show_hidden(library: &Path) {
    let path = library.join("settings.json");
    let mut settings: Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    if !settings["view"].is_object() {
        settings["view"] = json!({});
    }
    settings["view"]["hiddenPictures"] = json!(true);
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
}

/// Waits until nothing is waiting to be written (`pendingWrites` is `null`) and no scan runs.
fn settled(agent: &mut Agent) -> Value {
    let last = std::cell::RefCell::new(Value::Null);
    eventually(
        "every write done and no scan running",
        || {
            let i = agent.inspect();
            let w = &i["pendingWrites"];
            let done = w.is_null() && i["scan"]["running"] == json!(false);
            *last.borrow_mut() = json!({ "pendingWrites": w, "scan": i["scan"] });
            done.then_some(i)
        },
        || last.borrow().to_string(),
    )
}

/// The section of each folder (`folder:<id>`) and its path, from `doc_inspect`'s tree.
fn folder_paths(inspect: &Value) -> BTreeMap<String, String> {
    let groups = inspect["tree"]["groups"].as_array().unwrap();
    let folders = groups.iter().find(|g| g["group"] == json!("folders")).unwrap();
    folders["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["section"].as_str().unwrap().to_string(), r["path"].as_str().unwrap().to_string()))
        .collect()
}

/// `v` without ids: photo and folder ids dropped, folder sections named by path, the keys of
/// `drop` left out at every level.
fn by_path(v: &Value, folders: &BTreeMap<String, String>, drop: &[&str]) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !["id", "folder"].contains(&k.as_str()) && !drop.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), by_path(v, folders, drop)))
                .collect(),
        ),
        Value::Array(list) => Value::Array(list.iter().map(|v| by_path(v, folders, drop)).collect()),
        Value::String(s) => match folders.get(s) {
            Some(path) => json!(format!("folder at {path}")),
            None => v.clone(),
        },
        _ => v.clone(),
    }
}

/// Everything the agent can see, by path: `doc_inspect` of the folder (every photo listed, the
/// hidden one marked) and each photo's `photo_info`.
fn everything(agent: &mut Agent, folder: &Path) -> (Value, BTreeMap<String, Value>) {
    let overview = settled(agent);
    let folders = folder_paths(&overview);
    // By name: the server may spell the path otherwise (a resolved temporary folder on macOS).
    let section = folders.iter().find(|(_, p)| Path::new(p).file_name() == folder.file_name()).map(|(s, _)| s.clone()).unwrap();
    let page = agent.tool("doc_inspect", json!({ "photos": section, "limit": 1000 }));
    let mut infos = BTreeMap::new();
    for id in crate::harness::ids(&page) {
        let info = agent.photo(id);
        infos.insert(info["path"].as_str().unwrap().to_string(), by_path(&info, &folders, &[]));
    }
    (by_path(&page, &folders, RESET), infos)
}

/// The folder's files: its sidecar (parsed) and every photo's bytes.
fn on_disk(folder: &Path, n: u64) -> (Value, Vec<Vec<u8>>) {
    (sidecar(folder), (1..=n).map(|i| bytes(&folder.join(photo_name(i)))).collect())
}

#[test]
fn w04_rebuild_loses_nothing() {
    let ws = Workspace::new();
    let folder = ws.folder("Rome", 6, 40);
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let ids = agent.section_ids(&section);
    assert_eq!(ids.len(), 6, "{ids:?}");
    let name = |agent: &mut Agent, id: u64| agent.photo(id)["name"].as_str().unwrap().to_string();
    let (starred, captioned, tagged, hidden, edited) = (ids[0], ids[1], ids[2], ids[3], ids[4]);

    // Star, caption, tag, hide, edit and an album, each recorded in the folder's sidecar.
    agent.tool("set_metadata", json!({ "ids": [starred], "star": true }));
    agent.tool("set_metadata", json!({ "ids": [captioned, starred], "caption": "The Forum at dusk" }));
    agent.tool("set_metadata", json!({ "ids": [tagged, starred], "tags_add": ["ruins", "rome trip"] }));
    agent.tool("set_metadata", json!({ "ids": [hidden], "hidden": true }));
    agent.tool("apply_edit", json!({ "ids": [edited], "op": "crop", "params": { "rect": [0.1, 0.1, 0.9, 0.9] } }));
    agent.tool("apply_edit", json!({ "ids": [edited], "op": "sepia" }));
    let album = agent.tool("create_album", json!({ "name": "Best of Rome", "ids": [edited, starred, captioned], "description": "Three to keep" }));
    let album_section = album["section"].as_str().unwrap().to_string();
    assert_eq!(agent.section_ids(&album_section), vec![edited, starred, captioned], "{album}");
    let names: Vec<String> = [starred, captioned, tagged, hidden, edited].iter().map(|id| name(&mut agent, *id)).collect();
    settled(&mut agent);
    let entry = |n: usize| sidecar_entry(&folder, &names[n]);
    assert_eq!(entry(0)["star"], json!(true), "{}", entry(0));
    assert_eq!(entry(1)["caption"], json!("The Forum at dusk"), "{}", entry(1));
    assert!(entry(2)["tags"].to_string().contains("ruins"), "{}", entry(2));
    assert_eq!(entry(3)["hidden"], json!(true), "{}", entry(3));
    assert!(crate::harness::edits(&entry(4)).iter().any(|e| e.starts_with("sepia")), "{}", entry(4));
    agent.quit();

    // Next session, View ▸ Hidden Pictures on: everything the agent sees, then Rebuild Library.
    show_hidden(&ws.library());
    let mut agent = ws.agent();
    let (overview, infos) = everything(&mut agent, &folder);
    assert_eq!(infos.len(), 6, "every photo listed, the hidden one too: {overview}");
    let files = on_disk(&folder, 6);
    let r = agent.tool("cmd_library_rebuild", json!({ "confirm": true }));
    assert_eq!(r["started"], json!(true), "{r}");
    let (overview_after, infos_after) = everything(&mut agent, &folder);
    assert_eq!(overview_after, overview, "doc_inspect is the same after the rebuild");
    for (path, info) in &infos {
        assert_eq!(infos_after.get(path), Some(info), "photo_info of {path} is the same after the rebuild");
    }
    assert_eq!(infos_after.len(), infos.len());
    assert!(on_disk(&folder, 6) == files, "the rebuild changed the folder's sidecar or a photo file");
    agent.quit();
}
