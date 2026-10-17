//! M11's *Compare and batch edit* in the live app (`m11.rs` part 2): `dalica --headless` on a
//! folder of 100 `scenes` photos, a third of them already edited (a quarter turn, or Auto Contrast
//! and Auto Color), driven over its control channel:
//!
//! - all 100 selected, Picture ▸ Batch Edit ▸ Auto Fix: every recipe ends in `autofix()`, and it is
//!   one undo step ("Auto Fix"); one Ctrl+Z gives every photo back the recipe it had, in the library
//!   and in the folder's sidecar;
//! - one of them in the edit view, A│A: the right side focused and Auto Fix and Auto Contrast on it
//!   change the variant only (the photo's recipe and the sidecar stay as they were); the window
//!   matches `fingerprints/m11-1400x900-compare-same.json`; leaving A│A asks "Keep which
//!   version?", Right makes the variant one step of the photo's history: its recipe now ends in
//!   both operations, and the edit view's one Undo gives back the recipe before it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, dialog, idle, key, quiet, resize, run, secs};
use crate::live::{self, Client};

const LARGE: (u32, u32) = (1400, 900);
const PHOTOS: u64 = 100;
const SIDECAR: &str = ".dalica.json";

fn name(n: u64) -> String {
    format!("IMG_{n:04}.jpg")
}

/// Every photo's recipe, by id.
fn recipes(c: &mut Client, ids: &[Value]) -> Vec<Vec<String>> {
    whole_recipes(c, ids).iter().map(|r| edits_of(&json!({ "recipe": r }))).collect()
}

/// Every photo's whole recipe (edits and redo list), by id.
fn whole_recipes(c: &mut Client, ids: &[Value]) -> Vec<Value> {
    let r = c.exec("photo.inspect", json!({ "ids": ids }));
    r.as_array().unwrap().iter().map(|p| p["recipe"].clone()).collect()
}

/// The recipe's edits of a `photo.inspect` entry (`recipe.edits`) or a `photo.history.state`
/// answer (`edits`).
fn edits_of(p: &Value) -> Vec<String> {
    p.pointer("/recipe/edits")
        .unwrap_or(&p["edits"])
        .as_array()
        .unwrap_or_else(|| panic!("photo.inspect names the recipe: {p}"))
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect()
}

/// The folder's sidecar entries' recipes, by photo name (none: empty).
fn sidecar_recipes(folder: &Path) -> Vec<Vec<String>> {
    let v: Value = std::fs::read(folder.join(SIDECAR)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
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

pub fn edit(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder: PathBuf = home.join("Pictures").join("Market");
    for n in 1..=PHOTOS {
        // The photo compared is large enough to see; the batch's are small.
        let (w, h) = if n == 1 { (900, 600) } else { (96, 64) };
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3000 + n, w, h), 85, None).unwrap();
        app::put(&folder.join(name(n)), &jpeg, n);
    }
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let ids: Vec<Value> = (1..=PHOTOS).map(|n| by_path[&format!("Market/{}", name(n))].id.clone()).collect();
    // A third already edited: a quarter turn, or Auto Contrast and Auto Color.
    let turned: Vec<Value> = ids.iter().step_by(3).cloned().collect();
    let contrasted: Vec<Value> = ids.iter().skip(1).step_by(6).cloned().collect();
    run(c, "edit.apply", json!({ "ids": turned, "op": "rotate(90)" }));
    run(c, "edit.apply", json!({ "ids": contrasted, "op": "autocontrast" }));
    run(c, "edit.apply", json!({ "ids": contrasted, "op": "autocolor" }));
    let before = recipes(c, &ids);
    let whole_before = whole_recipes(c, &ids);
    let edited = before.iter().filter(|r| !r.is_empty()).count();
    assert!(edited >= 40, "{edited} photos start with a recipe");
    app::flush_until(c, || sidecar_recipes(&folder) == before);
    assert_eq!(sidecar_recipes(&folder), before, "the sidecar holds the recipes before the batch");
    let depth = undo(c)["depth"].as_u64().unwrap();

    // Picture ▸ Batch Edit ▸ Auto Fix on the 100 selected: one step.
    run(c, "select.set", json!({ "ids": ids }));
    c.ok("ui.menu.invoke", json!({ "path": "Picture/Batch Edit/Auto Fix", "wait": true }));
    idle(c);
    let after = recipes(c, &ids);
    for (b, now) in before.iter().zip(&after) {
        assert_eq!(now.last().map(String::as_str), Some("autofix()"), "every recipe ends in Auto Fix: {b:?} → {now:?}");
        assert_eq!(&now[..now.len() - 1], &b[..], "appended to the recipe it had");
    }
    let u = undo(c);
    assert_eq!((u["undo"].clone(), u["depth"].as_u64()), (json!("Auto Fix"), Some(depth + 1)), "one undo step for the 100: {u}");
    app::flush_until(c, || sidecar_recipes(&folder) == after);
    assert_eq!(sidecar_recipes(&folder), after, "the sidecar records the batch");

    // One Ctrl+Z: every recipe as it was.
    key(c, "Ctrl+Z");
    assert_eq!(whole_recipes(c, &ids), whole_before, "one undo restores every recipe and redo list");
    assert_eq!(undo(c)["depth"].as_u64(), Some(depth), "and the batch's step is gone");
    app::flush_until(c, || sidecar_recipes(&folder) == before);
    assert_eq!(sidecar_recipes(&folder), before, "the sidecar holds the recipes again");
    println!("  Batch Edit ▸ Auto Fix on {PHOTOS} photos ({edited} already edited): one step \"Auto Fix\"; one Ctrl+Z restored all {PHOTOS} recipes");

    // A│A on a photo with a quarter turn: the right side's edits stay off the photo until kept.
    let photo = turned[0].clone();
    let own = vec!["rotate(90)".to_string()];
    run(c, "view.edit", json!({ "id": photo }));
    click(c, "button:compare.same");
    assert_eq!(c.state()["edit"]["compare"], "same");
    click(c, "canvas:edit.other");
    assert_eq!(c.state()["edit"]["focus"], "right", "the right side, the variant, has the focus");
    click(c, "tool:autofix");
    click(c, "tool:autocontrast");
    let variant = c.exec("photo.history.state", json!({ "id": photo, "variant": true }));
    let kept = vec!["rotate(90)".to_string(), "autofix()".to_string(), "autocontrast()".to_string()];
    assert_eq!(edits_of(&variant), kept, "the variant took both: {variant}");
    assert_eq!(edits_of(&c.inspect(&photo)), own, "the photo's own recipe did not");
    app::flush(c);
    assert_eq!(sidecar_recipes(&folder)[0], own, "nor its sidecar entry");
    quiet(c, f64::from(LARGE.1));
    let masks = app::moving_parts(c);
    app::masked_shot(c, home, "m11-1400x900-compare-same", LARGE, &masks);

    // Leaving A│A asks; Right keeps the variant as one history step.
    click(c, "button:compare.one");
    assert_eq!(dialog(c).as_deref(), Some("keepVersion"), "Keep which version?");
    click(c, "button:dialog.right");
    assert_eq!((dialog(c), c.state()["edit"]["compare"].clone()), (None, json!("one")));
    assert_eq!(edits_of(&c.inspect(&photo)), kept, "the right side's recipe is the photo's");
    app::flush_until(c, || sidecar_recipes(&folder)[0] == kept);
    assert_eq!(sidecar_recipes(&folder)[0], kept, "and its sidecar entry's");
    click(c, "button:edit.undo");
    assert_eq!(edits_of(&c.inspect(&photo)), own, "one Undo: the version kept was one step of the photo's history");
    println!(
        "  A│A: Auto Fix and Auto Contrast on the right side left the photo alone (matches its fingerprint); Right kept them as one history step"
    );
    live::quit(a, cl);
    println!("  M11 compare and batch edit in {:.1} s", secs(started));
}
