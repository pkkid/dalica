//! W12, Destructive commands ask (`mcp.md` §9; `organizing.md` §9; `security.md` §5.7): Delete
//! from Disk without `confirm` answers `needs_confirm` with the app's question and changes nothing;
//! with `confirm` the file goes to the trash (the stand-in trash of a test build, never the user's)
//! and the photo leaves the library, its data kept in the undo step and its sidecar entry where it
//! was; Undo brings the file and the data back where the trash can give items back (not macOS,
//! where Undo says why instead).

use serde_json::json;

use crate::harness::{Workspace, bytes, eventually, sidecar, sidecar_entry, trashed};

#[test]
fn w12_destructive_asks() {
    let ws = Workspace::new();
    let folder = ws.folder("Attic", 4, 120);
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let ids = agent.section_ids(&section);
    let doomed = ids[2];
    agent.tool("set_metadata", json!({ "ids": [doomed], "star": true, "caption": "Grandma's chair", "tags_add": ["family"] }));
    // The caption and tags are written into the file too: wait for that before taking its bytes.
    eventually("every write done", || agent.inspect()["pendingWrites"].is_null().then_some(()), String::new);
    let info = agent.photo(doomed);
    let name = info["name"].as_str().unwrap().to_string();
    let path = folder.join(&name);
    let contents = bytes(&path);
    let entry = eventually(
        "the sidecar records the photo's data",
        || Some(sidecar_entry(&folder, &name)).filter(|e| e["caption"] == json!("Grandma's chair") && e["star"] == json!(true)),
        || sidecar(&folder).to_string(),
    );
    let photos = agent.inspect()["counts"]["photos"].clone();
    assert_eq!(photos, json!(4));

    // Without confirm: the question, and nothing changes (library, file, sidecar, trash).
    let r = agent.call("cmd_photo_delete", json!({ "ids": [doomed] }));
    assert_eq!((&r["isError"], &r["structuredContent"]["code"]), (&json!(true), &json!("needs_confirm")), "{r}");
    let prompt = &r["structuredContent"]["prompt"];
    assert!(prompt["buttons"].as_array().is_some_and(|b| b.len() == 2) && prompt["text"].is_string(), "the app's question: {r}");
    assert_eq!(agent.photo(doomed), info, "the photo is unchanged");
    assert_eq!(agent.inspect()["counts"]["photos"], photos);
    assert_eq!(bytes(&path), contents, "the file is still there");
    assert_eq!(sidecar_entry(&folder, &name), entry);
    assert!(trashed(&contents).is_empty(), "nothing went to the trash");

    // With confirm: the file is in the trash, the photo leaves the library, its entry stays.
    let r = agent.tool("cmd_photo_delete", json!({ "ids": [doomed], "confirm": true }));
    assert_eq!(r["state"], json!("done"), "{r}");
    assert!(!path.exists(), "the file left its folder");
    assert_eq!(trashed(&contents).len(), 1, "the file is in the (stand-in) trash, byte for byte");
    let gone = agent.call("photo_info", json!({ "id": doomed }));
    assert_eq!((&gone["isError"], &gone["structuredContent"]["code"]), (&json!(true), &json!("not_found")), "{gone}");
    assert_eq!(agent.section_ids(&section), ids.iter().copied().filter(|id| *id != doomed).collect::<Vec<_>>());
    assert_eq!(agent.inspect()["counts"]["photos"], json!(3));
    assert_eq!(sidecar_entry(&folder, &name), entry, "the sidecar entry stays where it was");

    // Undo: the file comes back from the trash with everything said about it.
    let undo = agent.inspect()["undo"]["undo"].clone();
    let r = agent.call("cmd_edit_undo", json!({}));
    if cfg!(target_os = "macos") {
        assert_eq!(r["isError"], json!(true), "the Trash cannot be listed here: {r}");
        assert!(r["structuredContent"]["error"].as_str().is_some_and(|e| e.contains("Finder")), "Undo says why: {r}");
        assert!(!path.exists() && trashed(&contents).len() == 1, "the file stays in the trash");
    } else {
        assert!(undo.as_str().is_some_and(|u| u.starts_with("Delete")), "the undo was the delete's: {undo}");
        assert_eq!(r["isError"], json!(false), "{r}");
        assert_eq!(bytes(&path), contents, "the file is back, byte for byte");
        assert!(trashed(&contents).is_empty(), "and no longer in the trash");
        assert_eq!(agent.photo(doomed), info, "the photo is back with its star, caption and tags");
        assert_eq!(agent.section_ids(&section), ids);
        eventually("the sidecar entry is as it was", || (sidecar_entry(&folder, &name) == entry).then_some(()), || sidecar(&folder).to_string());
    }
    agent.quit();
}
