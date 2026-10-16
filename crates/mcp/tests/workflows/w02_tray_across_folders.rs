//! W02, Tray across folders (`mcp.md` §9; `ui-layout.md` §5.4): two photos selected in one folder
//! and held, three selected in another, the Photo Tray holds all five; Rotate Clockwise on the
//! tray records `rotate(90)` in both folders' sidecars, and one Undo takes all five back.

use std::path::Path;

use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, edits, eventually, sidecar, sidecar_entry};

/// Each photo's (name, edits recorded in its folder's sidecar).
fn recorded(folder: &Path, names: &[String]) -> Vec<(String, Vec<String>)> {
    names.iter().map(|n| (n.clone(), edits(&sidecar_entry(folder, n)))).collect()
}

/// Waits until every photo of `names` in `folder` records exactly `want`.
fn wait_recorded(folder: &Path, names: &[String], want: &[&str]) {
    let want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    eventually(
        &format!("the sidecar of {} records {want:?} for {names:?}", folder.display()),
        || recorded(folder, names).iter().all(|(_, e)| *e == want).then_some(()),
        || sidecar(folder).to_string(),
    );
}

fn names(agent: &mut Agent, ids: &[u64]) -> Vec<String> {
    ids.iter().map(|id| agent.photo(*id)["name"].as_str().unwrap().to_string()).collect()
}

fn rotations(agent: &mut Agent, ids: &[u64]) -> Vec<Value> {
    ids.iter().map(|id| agent.photo(*id)["rotation"].clone()).collect()
}

#[test]
fn w02_tray_across_folders() {
    let ws = Workspace::new();
    let a = ws.folder("Lisbon", 4, 0);
    let b = ws.folder("Porto", 4, 100);
    let mut agent = ws.agent();
    let sa = agent.scan(&a);
    let sb = agent.scan(&b);
    let in_a = agent.section_ids(&sa);
    let in_b = agent.section_ids(&sb);
    assert_eq!((in_a.len(), in_b.len()), (4, 4), "{sa}: {in_a:?}, {sb}: {in_b:?}");

    // Two in A, held; then three in B: the tray holds all five.
    let held = &in_a[..2];
    let picked = &in_b[..3];
    let t = agent.tool("select_photos", json!({ "ids": held, "section": sa, "hold": true }));
    assert_eq!(t["held"], json!(held), "{t}");
    let t = agent.tool("select_photos", json!({ "ids": picked, "section": sb }));
    let mut tray: Vec<u64> = t["tray"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    tray.sort_unstable();
    let mut five: Vec<u64> = held.iter().chain(picked).copied().collect();
    five.sort_unstable();
    assert_eq!(tray, five, "the tray holds the held two and the selected three: {t}");
    for id in &five {
        let info = agent.photo(*id);
        assert_eq!(info["held"], json!(held.contains(id)), "photo {id}: {info}");
    }

    // Rotate Clockwise with no ids acts on the tray, in both folders.
    let names_a = names(&mut agent, held);
    let names_b = names(&mut agent, picked);
    let others_a = names(&mut agent, &in_a[2..]);
    let others_b = names(&mut agent, &in_b[3..]);
    agent.tool("cmd_photo_rotate_cw", json!({}));
    assert_eq!(rotations(&mut agent, &five), vec![json!(90); 5]);
    assert_eq!(rotations(&mut agent, &[in_a[2], in_a[3], in_b[3]]), vec![json!(0); 3], "photos outside the tray are not rotated");
    wait_recorded(&a, &names_a, &["rotate(90)"]);
    wait_recorded(&b, &names_b, &["rotate(90)"]);
    assert!(recorded(&a, &others_a).iter().chain(&recorded(&b, &others_b)).all(|(_, e)| e.is_empty()), "only the tray's photos record it");
    let undo = agent.inspect()["undo"].clone();
    assert!(undo.to_string().contains("Rotate"), "the next undo is the rotation: {undo}");

    // One Undo takes all five back, in the library and in both sidecars.
    agent.tool("cmd_edit_undo", json!({}));
    assert_eq!(rotations(&mut agent, &five), vec![json!(0); 5]);
    wait_recorded(&a, &names_a, &[]);
    wait_recorded(&b, &names_b, &[]);
    agent.quit();
}
