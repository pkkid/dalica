//! W09, Hidden photos (`mcp.md` §9; `organizing.md` §9): four photos hidden leave the light table
//! and `query_photos`, their sidecar entries say so, View ▸ Hidden Pictures shows them again
//! (marked hidden), and unhidden they come back.
//!
//! Headless mode has no window, so no View menu: Hidden Pictures is turned on the way the app keeps
//! it between runs, `"view": {"hiddenPictures": true}` in the library's `settings.json`, between two
//! sessions. That also shows the hidden flag outlives a restart.

use std::path::Path;

use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, eventually, sidecar, sidecar_entry};

/// Stores View ▸ Hidden Pictures in the library's settings, keeping every other setting.
fn hidden_pictures(library: &Path, on: bool) {
    let path = library.join("settings.json");
    let mut settings: Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    if !settings["view"].is_object() {
        settings["view"] = json!({});
    }
    settings["view"]["hiddenPictures"] = json!(on);
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
}

/// Waits until the sidecar entries of `names` say `hidden` (or not).
fn wait_hidden(folder: &Path, names: &[String], hidden: bool) {
    eventually(
        &format!("the sidecar records hidden = {hidden} for {names:?}"),
        || names.iter().all(|n| (sidecar_entry(folder, n)["hidden"] == json!(true)) == hidden).then_some(()),
        || sidecar(folder).to_string(),
    );
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

/// What the light table and the queries show: (the section's photos, what `query_photos` finds
/// searching for words every name holds, what it finds in the section, the hidden count of
/// `doc_inspect`).
fn shown(agent: &mut Agent, section: &str) -> (Vec<u64>, Vec<u64>, Vec<u64>, Value) {
    let listed = sorted(agent.section_ids(section));
    let searched = sorted(agent.query(json!({ "query": "img" })));
    let in_section = sorted(agent.query(json!({ "section": section })));
    (listed, searched, in_section, agent.inspect()["counts"]["hidden"].clone())
}

#[test]
fn w09_hidden_photos() {
    let ws = Workspace::new();
    let folder = ws.folder("Beach", 8, 20);
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let all = sorted(agent.section_ids(&section));
    assert_eq!(all.len(), 8, "{all:?}");
    let hide: Vec<u64> = all.iter().copied().step_by(2).collect();
    let keep: Vec<u64> = all.iter().copied().filter(|id| !hide.contains(id)).collect();
    let names: Vec<String> = hide.iter().map(|id| agent.photo(*id)["name"].as_str().unwrap().to_string()).collect();

    // Hidden: they leave the light table and the queries; the sidecar keeps the flag.
    agent.tool("set_metadata", json!({ "ids": hide, "hidden": true }));
    assert_eq!(shown(&mut agent, &section), (keep.clone(), keep.clone(), keep.clone(), json!(4)));
    for id in &hide {
        let info = agent.photo(*id);
        assert_eq!(info["hidden"], json!(true), "{info}");
    }
    wait_hidden(&folder, &names, true);
    agent.quit();

    // View ▸ Hidden Pictures on (next session): all eight show, the four marked hidden.
    hidden_pictures(&ws.library(), true);
    let mut agent = ws.agent();
    let page = agent.tool("doc_inspect", json!({ "photos": section, "limit": 1000 }));
    assert_eq!(page["view"]["hiddenPictures"], json!(true), "{}", page["view"]);
    let marked: Vec<u64> =
        page["photos"].as_array().unwrap().iter().filter(|p| p["hidden"] == json!(true)).map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!((sorted(crate::harness::ids(&page)), sorted(marked)), (all.clone(), hide.clone()), "{page}");

    // Unhidden, and Hidden Pictures off again: all eight come back.
    agent.tool("set_metadata", json!({ "ids": hide, "hidden": false }));
    for id in &hide {
        let info = agent.photo(*id);
        assert_eq!(info["hidden"], json!(false), "{info}");
    }
    wait_hidden(&folder, &names, false);
    agent.quit();
    hidden_pictures(&ws.library(), false);
    let mut agent = ws.agent();
    assert_eq!(shown(&mut agent, &section), (all.clone(), all.clone(), all, json!(0)));
    agent.quit();
}
