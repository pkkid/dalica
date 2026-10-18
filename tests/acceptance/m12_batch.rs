//! M12's Batch Edit in the live app (`m12.rs` part 5): `dalica --headless` on a folder of 20
//! `scenes` photos, five of them already turned, driven over its control channel: all 20 selected,
//! Picture ▸ Batch Edit ▸ Sepia appends `sepia()` to every recipe as one undo step "Sepia" (the
//! folder's sidecar recording it), and one Ctrl+Z gives every photo back the recipe it had, in the
//! library and the sidecar.

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, idle, key, resize, run, secs};
use crate::common::{LARGE, edits};
use crate::live::{self, Client};

const PHOTOS: u64 = 20;

fn name(n: u64) -> String {
    format!("IMG_{n:04}.jpg")
}

fn recipes(c: &mut Client, ids: &[Value]) -> Vec<Vec<String>> {
    ids.iter().map(|id| edits(c, id)).collect()
}

/// The folder's sidecar entries' recipes, in photo order (none: empty).
fn sidecar(folder: &Path) -> Vec<Vec<String>> {
    let v: Value = std::fs::read(folder.join(".dalica.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    (1..=PHOTOS)
        .map(|n| {
            v.pointer(&format!("/photos/{}/edits", name(n)))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default()
        })
        .collect()
}

fn undo(c: &mut Client) -> Value {
    c.exec("library.inspect", json!({}))["undo"].clone()
}

pub fn batch(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder = home.join("Pictures").join("Market");
    for n in 1..=PHOTOS {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(6000 + n, 96, 64), 85, None).unwrap();
        app::put(&folder.join(name(n)), &jpeg, n);
    }
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let ids: Vec<Value> = (1..=PHOTOS).map(|n| by_path[&format!("Market/{}", name(n))].id.clone()).collect();
    let turned: Vec<Value> = ids.iter().step_by(4).cloned().collect();
    run(c, "edit.apply", json!({ "ids": turned, "op": "rotate(90)" }));
    let before = recipes(c, &ids);
    app::flush_until(c, || sidecar(&folder) == before);
    assert_eq!(sidecar(&folder), before, "the sidecar holds the recipes before the batch");
    let depth = undo(c)["depth"].as_u64().unwrap();

    run(c, "select.set", json!({ "ids": ids }));
    c.ok("ui.menu.invoke", json!({ "path": "Picture/Batch Edit/Sepia", "wait": true }));
    idle(c);
    let after = recipes(c, &ids);
    for (b, now) in before.iter().zip(&after) {
        assert_eq!(now.last().map(String::as_str), Some("sepia()"), "every recipe ends in Sepia: {b:?} → {now:?}");
        assert_eq!(&now[..now.len() - 1], &b[..], "appended to the recipe it had");
    }
    let u = undo(c);
    assert_eq!((u["undo"].clone(), u["depth"].as_u64()), (json!("Sepia"), Some(depth + 1)), "one undo step for the {PHOTOS}: {u}");
    app::flush_until(c, || sidecar(&folder) == after);
    assert_eq!(sidecar(&folder), after, "the sidecar records the batch");

    key(c, "Ctrl+Z");
    assert_eq!(recipes(c, &ids), before, "one Ctrl+Z restores every recipe");
    assert_eq!(undo(c)["depth"].as_u64(), Some(depth), "and the batch's step is gone");
    app::flush_until(c, || sidecar(&folder) == before);
    assert_eq!(sidecar(&folder), before, "the sidecar holds the recipes again");
    println!("  Batch Edit ▸ Sepia on {PHOTOS} photos ({} turned already): one step \"Sepia\"; one Ctrl+Z took all {PHOTOS} back", turned.len());
    live::quit(a, cl);
    println!("  M12 Batch Edit in {:.1} s", secs(started));
}
