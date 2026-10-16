//! W03, Save, Undo Save, Revert (`mcp.md` §9; `data-model.md` §6, §10.9): an edit saved into the
//! file keeps the original in `.dalica-originals/`; Undo Save puts the original back with the edits
//! unsaved; saved again and reverted, the file holds the exact original bytes.

use serde_json::json;

use crate::harness::{Agent, HEIGHT, WIDTH, Workspace, bytes, edits, eventually, sidecar, sidecar_entry};

/// The crop of the workflow: the middle 80 % of each side.
const CROP: [f64; 4] = [0.1, 0.1, 0.9, 0.9];

/// A save job's result, checked to have done `id`.
fn saved(agent: &mut Agent, command: &str, id: u64) {
    let mut params = json!({ "ids": [id] });
    if command != "cmd_photo_undoSave" {
        params["confirm"] = json!(true);
    }
    let r = agent.tool(command, params);
    assert_eq!(r["result"]["photos"], json!([id]), "{command}: {r}");
    assert_eq!(r["result"]["failed"], json!([]), "{command}: {r}");
}

#[test]
fn w03_save_undo_revert() {
    let ws = Workspace::new();
    let folder = ws.folder("Trams", 3, 7);
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let id = agent.section_ids(&section)[0];
    let info = agent.photo(id);
    let name = info["name"].as_str().unwrap().to_string();
    let path = folder.join(&name);
    let backup = folder.join(dalica_sidecar::originals::DIR_NAME).join(&name);
    let original = bytes(&path);

    // An edit, saved: the file takes it, the original is kept aside.
    let applied = agent.tool("apply_edit", json!({ "ids": [id], "op": "crop", "params": { "rect": CROP } }));
    assert_eq!(applied["photos"][0]["changed"], json!(true), "{applied}");
    let recipe = agent.photo(id)["recipe"]["list"].clone();
    assert!(recipe.to_string().contains("crop"), "{recipe}");
    saved(&mut agent, "cmd_photo_save", id);
    assert_eq!(bytes(&backup), original, "the original is kept, byte for byte, in .dalica-originals/");
    assert_ne!(bytes(&path), original, "the file holds the edit");
    let info = agent.photo(id);
    assert_eq!((&info["unsavedEdits"], &info["originals"]), (&json!(false), &json!(true)), "{info}");
    let (w, h) = (info["dimensions"]["width"].as_u64().unwrap(), info["dimensions"]["height"].as_u64().unwrap());
    assert!(w < u64::from(WIDTH) && h < u64::from(HEIGHT), "the saved file is cropped: {info}");

    // Undo Save: the original is back, the edit unsaved again (and in the sidecar).
    saved(&mut agent, "cmd_photo_undoSave", id);
    assert_eq!(bytes(&path), original, "Undo Save puts the original back");
    let info = agent.photo(id);
    assert_eq!(info["unsavedEdits"], json!(true), "{info}");
    assert_eq!(info["recipe"]["list"], recipe, "the edits are unsaved again: {info}");
    eventually(
        "the sidecar records the unsaved crop",
        || edits(&sidecar_entry(&folder, &name)).iter().any(|e| e.starts_with("crop(")).then_some(()),
        || sidecar(&folder).to_string(),
    );

    // Saved again, then reverted: the exact original bytes, no edits, no backup.
    saved(&mut agent, "cmd_photo_save", id);
    assert_ne!(bytes(&path), original, "saved again");
    saved(&mut agent, "cmd_photo_revert", id);
    assert_eq!(bytes(&path), original, "Revert returns the exact original bytes");
    let info = agent.photo(id);
    assert_eq!(info["unsavedEdits"], json!(false), "{info}");
    assert_eq!(info["recipe"]["list"], json!([]), "Revert drops the edits: {info}");
    assert!(!backup.exists(), "the kept original went back into place");
    eventually("the sidecar records no edits", || edits(&sidecar_entry(&folder, &name)).is_empty().then_some(()), || sidecar(&folder).to_string());
    agent.quit();
}
