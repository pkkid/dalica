//! W07, Batch edit is one step (`mcp.md` §9; `editing.md` §9): Auto Fix through `apply_edit` on
//! the 20 photos of a folder held in the Photo Tray, all 20 recipes ending in `autofix()` in the
//! library and the folder's sidecar, one undo step named "Auto Fix", and one `cmd_edit_undo`
//! taking all 20 back; the photo files are never touched.

use serde_json::json;

use crate::harness::{Workspace, bytes, edits, eventually, photo_name, sidecar, sidecar_entry};

#[test]
fn w07_batch_edit_one_step() {
    let ws = Workspace::new();
    let folder = ws.folder("Market", 20, 700);
    let names: Vec<String> = (1..=20).map(photo_name).collect();
    let originals: Vec<Vec<u8>> = names.iter().map(|n| bytes(&folder.join(n))).collect();
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let ids = agent.section_ids(&section);
    assert_eq!(ids.len(), 20, "{section}: {ids:?}");

    // The 20 in the Photo Tray; apply_edit without ids acts on it.
    let t = agent.tool("select_photos", json!({ "ids": ids, "section": section }));
    assert_eq!(t["tray"].as_array().map(Vec::len), Some(20), "{t}");
    let applied = agent.tool("apply_edit", json!({ "op": "autofix" }));
    assert_eq!(applied["entry"], json!("autofix()"), "{applied}");
    let photos = applied["photos"].as_array().unwrap();
    assert_eq!(photos.len(), 20, "{applied}");
    assert!(photos.iter().all(|p| p["changed"] == true && p["recipe"] == "autofix()"), "{applied}");
    for id in &ids {
        let info = agent.photo(*id);
        assert_eq!(info["recipe"]["list"], json!(["autofix()"]), "photo {id}: {info}");
        assert_eq!(info["unsavedEdits"], json!(true), "photo {id}: {info}");
    }
    let recorded = |want: &[&str]| names.iter().all(|n| edits(&sidecar_entry(&folder, n)) == want);
    eventually("the sidecar records autofix() for all 20", || recorded(&["autofix()"]).then_some(()), || sidecar(&folder).to_string());
    let undo = agent.inspect()["undo"].clone();
    assert_eq!(undo["undo"], json!("Auto Fix"), "one step for all 20, named by the operation: {undo}");

    // One undo takes all 20 back, in the library and in the sidecar.
    agent.tool("cmd_edit_undo", json!({}));
    for id in &ids {
        let info = agent.photo(*id);
        assert_eq!(info["recipe"]["list"], json!([]), "photo {id}: {info}");
    }
    eventually("the sidecar records no edits after one undo", || recorded(&[]).then_some(()), || sidecar(&folder).to_string());
    let undo = agent.inspect()["undo"].clone();
    assert_ne!(undo["undo"], json!("Auto Fix"), "nothing of the batch is left to undo: {undo}");

    // Edits are data: no photo file changed.
    let now: Vec<Vec<u8>> = names.iter().map(|n| bytes(&folder.join(n))).collect();
    assert!(now == originals, "the photo files are untouched");
    agent.quit();
}
