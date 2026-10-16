//! W05, Move carries data (`mcp.md` §9; `data-model.md` §8, §10.3): three photos starred,
//! captioned and saved (each with its original kept in `.dalica-originals/`) are moved to another
//! folder as a drag does (`cmd_photo_moveTo`): their files, sidecar entries and originals backups
//! go with them and nothing is left behind; one Undo moves all of it back.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, bytes, eventually, photo_name, sidecar, sidecar_entry};

/// What one moved photo is made of on disk.
struct Item {
    name: String,
    /// The photo's bytes (the saved edit).
    file: Vec<u8>,
    /// Its original, kept by the Save.
    original: Vec<u8>,
    /// Its entry in the folder's sidecar.
    entry: Value,
}

/// A sidecar entry without its last-change time (`changed`, Recently Updated), which the move
/// itself sets (`data-model.md` §3: a move is a change).
fn data(entry: &Value) -> Value {
    let mut entry = entry.clone();
    if let Some(map) = entry.as_object_mut() {
        map.remove("changed");
    }
    entry
}

fn backup(folder: &Path, name: &str) -> PathBuf {
    folder.join(dalica_sidecar::originals::DIR_NAME).join(name)
}

/// Waits until `folder` holds every item (file, original backup, sidecar entry) or none of them.
fn wait_holds(folder: &Path, items: &[Item], holds: bool) {
    let check = || {
        items.iter().all(|i| {
            if holds {
                bytes(&folder.join(&i.name)) == i.file
                    && bytes(&backup(folder, &i.name)) == i.original
                    && data(&sidecar_entry(folder, &i.name)) == data(&i.entry)
            } else {
                !folder.join(&i.name).exists() && !backup(folder, &i.name).exists() && sidecar_entry(folder, &i.name).is_null()
            }
        })
    };
    eventually(
        &format!("{} holds the photos, their backups and sidecar entries: {holds}", folder.display()),
        || check().then_some(()),
        || sidecar(folder).to_string(),
    );
}

/// The ids of `section`'s photos with their names.
fn names(agent: &mut Agent, section: &str) -> Vec<(u64, String)> {
    agent.section_ids(section).into_iter().map(|id| (id, agent.photo(id)["name"].as_str().unwrap().to_string())).collect()
}

#[test]
fn w05_move_carries_data() {
    let ws = Workspace::new();
    let from = ws.folder("Card", 5, 60);
    // The destination holds a photo of its own, named unlike the ones that come.
    let to = ws.folder("Venice", 1, 90);
    let resident = bytes(&to.join(photo_name(1)));
    let mut agent = ws.agent();
    let s_from = agent.scan(&from);
    let s_to = agent.scan(&to);
    let target: u64 = s_to.strip_prefix("folder:").and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{s_to}"));
    let ids = agent.section_ids(&s_from);
    let moving = &ids[1..4];

    // Starred, captioned and saved: the originals are kept aside, the sidecar records it all.
    agent.tool("set_metadata", json!({ "ids": moving, "star": true, "caption": "Grand Canal" }));
    agent.tool("apply_edit", json!({ "ids": moving, "op": "crop", "params": { "rect": [0.1, 0.1, 0.9, 0.9] } }));
    let saved = agent.tool("cmd_photo_save", json!({ "ids": moving, "confirm": true }));
    assert_eq!(saved["result"]["failed"], json!([]), "{saved}");
    // The caption is written into the files too: wait for every write before taking their bytes.
    eventually("every write done", || agent.inspect()["pendingWrites"].is_null().then_some(()), String::new);
    let mut items = Vec::new();
    for id in moving {
        let info = agent.photo(*id);
        assert_eq!((&info["star"], &info["caption"], &info["originals"]), (&json!(true), &json!("Grand Canal"), &json!(true)), "{info}");
        let name = info["name"].as_str().unwrap().to_string();
        let entry = eventually(
            &format!("the sidecar records {name} starred and captioned"),
            || Some(sidecar_entry(&from, &name)).filter(|e| e["star"] == json!(true) && e["caption"] == json!("Grand Canal")),
            || sidecar(&from).to_string(),
        );
        items.push(Item { file: bytes(&from.join(&name)), original: bytes(&backup(&from, &name)), entry, name });
    }
    let staying = names(&mut agent, &s_from).into_iter().filter(|(id, _)| !moving.contains(id)).collect::<Vec<_>>();
    let staying_files: Vec<Vec<u8>> = staying.iter().map(|(_, n)| bytes(&from.join(n))).collect();
    wait_holds(&from, &items, true);

    // Moved, as a drop onto the other folder does: everything goes, nothing is left behind.
    let r = agent.tool("cmd_photo_moveTo", json!({ "ids": moving, "folder": target, "confirm": true }));
    assert_eq!(r["state"], json!("done"), "{r}");
    wait_holds(&to, &items, true);
    wait_holds(&from, &items, false);
    for id in moving {
        let info = agent.photo(*id);
        assert_eq!(info["section"], json!(s_to), "{info}");
        assert_eq!((&info["star"], &info["caption"], &info["originals"]), (&json!(true), &json!("Grand Canal"), &json!(true)), "{info}");
    }
    let mut after: Vec<u64> = agent.section_ids(&s_to);
    after.sort_unstable();
    assert_eq!(after.len(), 4, "the three moved photos join the one already there: {after:?}");
    assert_eq!(names(&mut agent, &s_from).len(), 2);
    assert!(staying.iter().zip(&staying_files).all(|((_, n), b)| bytes(&from.join(n)) == *b), "the photos not moved are untouched");

    // One Undo moves the files, backups and sidecar entries back.
    let undo = agent.inspect()["undo"]["undo"].clone();
    assert_eq!(undo, json!("Move"), "the next undo is the move");
    agent.tool("cmd_edit_undo", json!({}));
    wait_holds(&from, &items, true);
    wait_holds(&to, &items, false);
    for i in &items {
        assert_eq!(sidecar_entry(&from, &i.name), i.entry, "the undo gives back the entry as it was, its last-change time too");
    }
    for id in moving {
        assert_eq!(agent.photo(*id)["section"], json!(s_from));
    }
    assert_eq!(bytes(&to.join(photo_name(1))), resident, "the destination's own photo stays untouched");
    agent.quit();
}
