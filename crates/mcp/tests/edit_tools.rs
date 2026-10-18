//! The editing helpers (`mcp.md` §4.2): `list_edit_ops` is the catalogue, `get_recipe` and
//! `set_recipe` go through `edit.recipe.set` (which keeps what Dalica cannot read unless told to
//! drop it), and `apply_edit` is Batch Edit (`edit.apply`) with the edit view's rules, one library
//! undo step for all its photos.
//! What the engine commands do is proved in `dalica-engine`; these tests prove the tools' wiring,
//! arguments and results.

use std::path::Path;

use dalica_core::PhotoId;
use dalica_mcp::tray::Mode;
use dalica_mcp::{Headless, Source};
use serde_json::{Value, json};

use crate::common::{Client, code, initialize, ok};

/// A server over a folder of `n` procedural photos of `width` × `height` (ids 1…n, by name), its
/// sidecar `sidecar` written before the scan, and photos `tray` selected in the Photo Tray.
pub fn folder(dir: &Path, n: u64, (width, height): (u32, u32), sidecar: Option<Value>, tray: &[u64]) -> Client {
    let photos = dir.join("Photos");
    std::fs::create_dir_all(&photos).unwrap();
    for i in 0..n {
        let scene = dalica_scenes::Scene::new(i, width, height);
        std::fs::write(photos.join(format!("IMG_{:04}.jpg", i + 1)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
    }
    if let Some(s) = sidecar {
        std::fs::write(photos.join(".dalica.json"), serde_json::to_vec(&s).unwrap()).unwrap();
    }
    let tray: Vec<PhotoId> = tray.iter().map(|id| PhotoId::new(*id)).collect();
    let mut c = Client::start(move || {
        let mut h = Headless::open(Source::Empty, &[photos]).unwrap();
        h.session_mut().wait_idle();
        assert_eq!(h.session().library().photos().count() as u64, n);
        if !tray.is_empty() {
            h.tray_mut().select(&tray, Some("folder:1"), Mode::Replace);
        }
        Box::new(h)
    });
    initialize(&mut c);
    c
}

/// A `tools/call` that fails as a protocol error (`-32602`, the arguments).
pub fn invalid(c: &mut Client, id: u64, name: &str, args: Value) {
    let r = c.request(id, "tools/call", json!({ "name": name, "arguments": args }));
    assert_eq!(r["error"]["code"], -32602, "{name} {args}: {r}");
}

fn recipe(c: &mut Client, id: u64, photo: u64) -> String {
    ok(&c.call(id, "get_recipe", json!({ "id": photo })))["recipe"].as_str().unwrap().to_string()
}

#[test]
fn list_edit_ops_matches_the_catalogue() {
    let mut c = Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    initialize(&mut c);
    let listed = c.request(2, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let hints = |n: &str| {
        let t = tools.iter().find(|t| t["name"] == n).unwrap_or_else(|| panic!("{n} is listed"));
        (t["annotations"]["readOnlyHint"].as_bool().unwrap(), t["annotations"]["destructiveHint"].as_bool().unwrap())
    };
    assert_eq!(
        [hints("list_edit_ops"), hints("get_recipe"), hints("set_recipe"), hints("apply_edit")],
        [(true, false), (true, false), (false, true), (false, false)]
    );

    let all = ok(&c.call(3, "list_edit_ops", json!({}))).clone();
    let rows = all["ops"].as_array().unwrap();
    let catalog = dalica_edit::catalog::catalog();
    assert_eq!(rows.len(), catalog.len());
    for (row, op) in rows.iter().zip(catalog) {
        assert_eq!(
            (&row["name"], &row["label"], &row["syntax"], &row["changes_size"], &row["milestone"], &row["available"]),
            (&json!(op.name), &json!(op.label), &json!(op.syntax), &json!(op.changes_size), &json!(op.milestone), &json!(op.renders())),
        );
        let names: Vec<&str> = row["params"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, op.params.iter().map(|p| p.name).collect::<Vec<_>>(), "{}", op.name);
        assert_eq!(row["default_entry"], json!(op.default_entry()));
    }
    let crop = ok(&c.call(4, "list_edit_ops", json!({ "op": "crop" }))).clone();
    assert_eq!(crop["ops"].as_array().map(Vec::len), Some(1));
    assert_eq!(crop["ops"][0]["params"][2], json!({ "name": "x1", "label": "Right", "default": "1", "kind": "number", "min": 0.0, "max": 1.0 }));
    let rotate = ok(&c.call(5, "list_edit_ops", json!({ "op": "rotate" }))).clone();
    assert_eq!(rotate["ops"][0]["params"][0]["choices"], json!(["90", "180", "270"]));

    assert_eq!(code(&c.call(6, "list_edit_ops", json!({ "op": "redeye" }))), "not_found");
    invalid(&mut c, 7, "list_edit_ops", json!({ "name": "crop" }));
    invalid(&mut c, 8, "list_edit_ops", json!({ "op": 3 }));
    c.close();
}

/// Photo 2 holds an operation Dalica cannot read (a legacy one): `set_recipe` refuses to drop it
/// unless `drop_unknown`, keeps it when the new recipe does, and each change is one undo step.
#[test]
fn set_recipe_refuses_to_drop_an_unknown_operation_unless_told() {
    let dir = tempfile::tempdir().unwrap();
    let sidecar = json!({ "photos": { "IMG_0002.jpg": { "edits": ["rotate(90)", "legacy:redeye(1)"] } } });
    let mut c = folder(dir.path(), 2, (64, 48), Some(sidecar), &[]);

    let got = ok(&c.call(2, "get_recipe", json!({ "id": 2 }))).clone();
    assert_eq!((&got["recipe"], &got["redo"], &got["saved"]), (&json!("rotate(90)|legacy:redeye(1)"), &json!([]), &json!(false)), "{got}");
    assert_eq!(got["ops"][0], json!({ "op": "rotate", "params": "90", "entry": "rotate(90)", "known": true }));
    assert_eq!(got["ops"][1], json!({ "op": "legacy:redeye", "params": "1", "entry": "legacy:redeye(1)", "known": false }));

    let refused = c.call(3, "set_recipe", json!({ "id": 2, "recipe": "autofix()" }));
    assert_eq!(code(&refused), "bad_params");
    let why = refused["structuredContent"]["error"].as_str().unwrap();
    assert!(why.contains("legacy:redeye(1)") && why.contains("dropUnknown"), "{why}");
    assert_eq!(recipe(&mut c, 4, 2), "rotate(90)|legacy:redeye(1)", "refused: nothing changed");

    // Keeping it (the ops of get_recipe, as {op, params} items) is allowed.
    let kept =
        ok(&c.call(5, "set_recipe", json!({ "id": 2, "recipe": [{ "op": "legacy:redeye", "params": "1" }, "autofix()"], "label": "agent" }))).clone();
    assert_eq!((&kept["recipe"], &kept["changed"]), (&json!("legacy:redeye(1)|autofix()"), &json!(true)), "{kept}");
    let dropped = ok(&c.call(6, "set_recipe", json!({ "id": 2, "recipe": "autofix()", "drop_unknown": true }))).clone();
    assert_eq!((&dropped["recipe"], &dropped["changed"]), (&json!("autofix()"), &json!(true)));
    let same = ok(&c.call(7, "set_recipe", json!({ "id": 2, "recipe": ["autofix()"] }))).clone();
    assert_eq!(same["changed"], false, "the same recipe takes no step");
    ok(&c.call(8, "command_run", json!({ "id": "photo.history.undo", "params": { "id": 2 } })));
    assert_eq!(recipe(&mut c, 9, 2), "legacy:redeye(1)|autofix()", "one undo gives the whole recipe before back");

    // Graceful failures and strict arguments.
    assert_eq!(code(&c.call(10, "get_recipe", json!({ "id": 99 }))), "not_found");
    assert_eq!(code(&c.call(11, "set_recipe", json!({ "id": 99, "recipe": "" }))), "not_found");
    assert_eq!(code(&c.call(12, "set_recipe", json!({ "id": 1, "recipe": "future_op(1)" }))), "bad_params", "adding one it cannot read");
    assert_eq!(code(&c.call(13, "set_recipe", json!({ "id": 1, "recipe": [{ "op": 3 }] }))), "bad_params");
    assert_eq!(code(&c.call(14, "get_recipe", json!({ "id": -1 }))), "not_found");
    invalid(&mut c, 15, "get_recipe", json!({}));
    invalid(&mut c, 16, "set_recipe", json!({ "id": 1 }));
    // A union type (text or a list) is the command's to check.
    assert_eq!(code(&c.call(17, "set_recipe", json!({ "id": 1, "recipe": 5 }))), "bad_params");
    invalid(&mut c, 18, "set_recipe", json!({ "id": 1, "recipe": "", "dropUnknown": true }));
    assert_eq!(recipe(&mut c, 19, 1), "", "photo 1 never changed");
    c.close();
}

#[test]
fn apply_edit_follows_the_edit_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = folder(dir.path(), 3, (600, 400), None, &[2, 3]);

    // A crop twice: the same one takes no step; another counts alone (not cropped twice).
    let first = ok(&c.call(2, "apply_edit", json!({ "ids": [1], "op": "crop", "params": { "rect": [0, 0, 0.5, 0.5] } }))).clone();
    assert_eq!(first, json!({ "entry": "crop(0,0,0.5,0.5)", "photos": [{ "id": 1, "recipe": "crop(0,0,0.5,0.5)", "changed": true }] }));
    let again = ok(&c.call(3, "apply_edit", json!({ "ids": [1], "op": "crop", "params": { "x0": 0, "y0": 0, "x1": 0.5, "y1": 0.5 } }))).clone();
    assert_eq!(again["photos"][0], json!({ "id": 1, "recipe": "crop(0,0,0.5,0.5)", "changed": false }), "one crop kept");
    ok(&c.call(4, "apply_edit", json!({ "ids": [1], "op": "crop", "params": "0.5,0.5,1,1" })));
    let shown = ok(&c.call(5, "render_photo", json!({ "id": 1, "size": 2048 }))).clone();
    assert_eq!((shown["width"].as_u64(), shown["height"].as_u64()), (Some(300), Some(200)), "only the last crop counts: {shown}");

    // Tuning merges into the trailing tuning entry: the values given replace its values.
    ok(&c.call(6, "apply_edit", json!({ "ids": [1], "op": "tuning", "params": { "fill": 0.3 } })));
    let merged = ok(&c.call(7, "apply_edit", json!({ "ids": [1], "op": "tuning", "params": { "shadows": 0.2 } }))).clone();
    let edits = merged["photos"][0]["recipe"].as_str().unwrap().to_string();
    let tunings: Vec<&str> = edits.split('|').filter(|e| e.starts_with("tuning(")).collect();
    assert_eq!(tunings, ["tuning(0.3,0,0.2,0,none)"], "{edits}");

    // Without ids: the Photo Tray (2 and 3).
    let tray = ok(&c.call(8, "apply_edit", json!({ "op": "autofix" }))).clone();
    assert_eq!(tray["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect::<Vec<_>>(), [2, 3]);
    ok(&c.call(9, "apply_edit", json!({ "ids": [2], "op": "rotate", "params": { "deg": 90 } })));
    ok(&c.call(10, "apply_edit", json!({ "ids": [2], "op": "rotate", "params": { "deg": "90" } })));
    assert_eq!(recipe(&mut c, 11, 2), "autofix()|rotate(180)", "a rotation after a rotation merges");
    assert_eq!(recipe(&mut c, 13, 3), "autofix()");

    // Refused before any photo changes; strict arguments.
    assert_eq!(code(&c.call(14, "apply_edit", json!({ "ids": [3, 99], "op": "autofix" }))), "not_found");
    assert_eq!(code(&c.call(15, "apply_edit", json!({ "ids": [3], "op": "redeye" }))), "bad_params");
    assert_eq!(code(&c.call(16, "apply_edit", json!({ "ids": [3], "op": "autofix", "params": { "amount": 1 } }))), "bad_params");
    assert_eq!(code(&c.call(17, "apply_edit", json!({ "ids": [3], "op": "crop", "params": { "rect": [0.6, 0.6, 0.1, 0.1] } }))), "bad_params");
    assert_eq!(code(&c.call(18, "apply_edit", json!({ "ids": [3], "op": "crop", "params": { "rect": [0, 1] } }))), "bad_params");
    assert_eq!(code(&c.call(19, "apply_edit", json!({ "ids": [3], "op": "trim" }))), "bad_params", "a parameter with no default yet");
    assert_eq!(code(&c.call(20, "apply_edit", json!({ "ids": [], "op": "autofix" }))), "disabled");
    assert_eq!(recipe(&mut c, 21, 3), "autofix()", "nothing applied");
    invalid(&mut c, 22, "apply_edit", json!({ "ids": [3] }));
    assert_eq!(code(&c.call(23, "apply_edit", json!({ "ids": [3], "op": "autofix", "params": 5 }))), "bad_params");
    invalid(&mut c, 24, "apply_edit", json!({ "ids": "3", "op": "autofix" }));
    invalid(&mut c, 25, "apply_edit", json!({ "id": 3, "op": "autofix" }));
    c.close();
}

/// `apply_edit` on three photos is one library undo step named by the operation: one Edit ▸ Undo
/// takes all three back and Redo applies them again. Tuning merged into different trailing
/// entries is one step too.
#[test]
fn apply_edit_on_three_photos_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let sidecar = json!({ "photos": { "IMG_0001.jpg": { "edits": ["tuning(0.3,0,0,0,none)"] } } });
    let mut c = folder(dir.path(), 3, (64, 48), Some(sidecar), &[]);
    let undo_label = |c: &mut Client, id: u64| ok(&c.call(id, "doc_inspect", json!({})))["undo"]["undo"].clone();

    let applied = ok(&c.call(2, "apply_edit", json!({ "ids": [1, 2, 3], "op": "autofix" }))).clone();
    assert!(applied["photos"].as_array().unwrap().iter().all(|p| p["changed"] == true), "{applied}");
    assert_eq!(undo_label(&mut c, 3), json!("Auto Fix"), "one step named by the operation");
    ok(&c.call(4, "cmd_edit_undo", json!({})));
    let recipes: Vec<String> = (1..=3).map(|id| recipe(&mut c, 10 + id, id)).collect();
    assert_eq!(recipes, ["tuning(0.3,0,0,0,none)", "", ""], "one undo takes all three back");
    ok(&c.call(5, "cmd_edit_redo", json!({})));
    let recipes: Vec<String> = (1..=3).map(|id| recipe(&mut c, 20 + id, id)).collect();
    assert_eq!(recipes, ["tuning(0.3,0,0,0,none)|autofix()", "autofix()", "autofix()"], "one redo applies them again");

    // Tuning merges into photo 1's trailing tuning after a Tuning of its own; still one step.
    ok(&c.call(6, "apply_edit", json!({ "ids": [1], "op": "tuning", "params": { "fill": 0.3 } })));
    let merged = ok(&c.call(7, "apply_edit", json!({ "ids": [1, 2, 3], "op": "tuning", "params": { "shadows": 0.2 } }))).clone();
    let recipes: Vec<&str> = merged["photos"].as_array().unwrap().iter().map(|p| p["recipe"].as_str().unwrap()).collect();
    assert_eq!(
        recipes,
        ["tuning(0.3,0,0,0,none)|autofix()|tuning(0.3,0,0.2,0,none)", "autofix()|tuning(0,0,0.2,0,none)", "autofix()|tuning(0,0,0.2,0,none)"]
    );
    ok(&c.call(8, "cmd_edit_undo", json!({})));
    let recipes: Vec<String> = (1..=3).map(|id| recipe(&mut c, 30 + id, id)).collect();
    assert_eq!(recipes, ["tuning(0.3,0,0,0,none)|autofix()|tuning(0.3,0,0,0,none)", "autofix()", "autofix()"], "one undo for the merged Tuning");
    c.close();
}
