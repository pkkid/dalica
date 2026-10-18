//! W11, Effects stack (`mcp.md` §9; `editing.md` §8): Sepia, then Vignette, then Border applied
//! through `apply_edit` on one photo. The recipe keeps that order (in the library and the folder's
//! sidecar), `render_photo` grows by the Border's bands (at full size and scaled down), the render's
//! fingerprint holds within tolerance (`architecture.md` §11), and the photo file is never touched.
//! `list_edit_ops` lists the 27 effects, each one's `available` saying whether its module renders.

use std::path::PathBuf;

use dalica_edit::catalog;
use dalica_scenes::fingerprint;
use serde_json::{Value, json};

use crate::common::base64_decode;
use crate::harness::{HEIGHT, WIDTH, Workspace, bytes, edits, eventually, photo_name, sidecar, sidecar_entry};

/// The Border applied: outer and inner bands of 80 and 20 thousandths of the shorter side, so
/// 12 px on every side of the 160 × 120 photo.
const BORDER: (u32, u32) = (80, 20);
/// The fingerprint's tolerance (0–255 per cell value): the render is a PNG of a deterministic
/// pipeline, so only floating-point noise between systems is allowed.
const TOLERANCE: f64 = 1.0;

/// The image of a `render_photo` result, decoded: its size and RGBA pixels.
fn rendered(result: &Value) -> (u32, u32, Vec<u8>) {
    assert_eq!(result["isError"], json!(false), "{result}");
    let block = &result["content"][0];
    assert_eq!(block["type"], "image", "{result}");
    let d = dalica_image::decode(&base64_decode(block["data"].as_str().unwrap()), &Default::default()).unwrap();
    let meta = &result["structuredContent"];
    assert_eq!((meta["width"].as_u64(), meta["height"].as_u64()), (Some(u64::from(d.width)), Some(u64::from(d.height))), "{meta}");
    (d.width, d.height, d.data)
}

/// Checks the render against `tests/fingerprints/<case>.json`, or stores it under `DALICA_BLESS=1`;
/// the render is always written to `target/test-output/mcp/<case>.png` to be looked at.
fn check_fingerprint(case: &str, (width, height, rgba): &(u32, u32, Vec<u8>)) {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/mcp");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join(format!("{case}.png")), fingerprint::png_rgba(*width, *height, rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{case}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::create_dir_all(stored.parent().unwrap()).unwrap();
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(*width, *height, rgba)) + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&stored)
        .unwrap_or_else(|e| panic!("{case}: no fingerprint at {} ({e}); bless it with DALICA_BLESS=1", stored.display()));
    let expected = fingerprint::from_json(&text).unwrap();
    if let Err(m) = fingerprint::check(*width, *height, rgba, &expected, TOLERANCE) {
        std::fs::write(out.join(format!("{case}-actual.png")), &m.actual_png).unwrap();
        panic!("{case}: {}", m.message);
    }
}

#[test]
fn w11_effects_stack() {
    let ws = Workspace::new();
    let folder = ws.folder("Harbour", 1, 1100);
    let name = photo_name(1);
    let original = bytes(&folder.join(&name));
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let ids = agent.section_ids(&section);
    let [id] = ids[..] else { panic!("{section}: {ids:?}") };

    // The 27 effects are listed, each `available` exactly when its module renders.
    let listed = agent.tool("list_edit_ops", json!({}));
    let effects: Vec<_> = catalog::catalog().iter().filter(|op| op.effect.is_some()).collect();
    assert_eq!(effects.len(), 27, "the catalogue's effects");
    for op in &effects {
        let row = listed["ops"].as_array().unwrap().iter().find(|r| r["name"] == op.name);
        let row = row.unwrap_or_else(|| panic!("list_edit_ops leaves out {}: {listed}", op.name));
        assert_eq!(row["available"], json!(op.renders()), "{}: {row}", op.name);
    }
    for op in ["sepia", "vignette", "border"] {
        assert!(catalog::lookup(op).is_some_and(|r| r.renders()), "{op} renders");
    }

    // Sepia, then Vignette, then Border: the recipe keeps that order.
    let before = rendered(&agent.call("render_photo", json!({ "id": id, "format": "png" })));
    assert_eq!((before.0, before.1), (WIDTH, HEIGHT), "the photo at its own size");
    let sepia = agent.tool("apply_edit", json!({ "ids": [id], "op": "sepia" }));
    let vignette = agent.tool("apply_edit", json!({ "ids": [id], "op": "vignette" }));
    let toned = rendered(&agent.call("render_photo", json!({ "id": id, "format": "png" })));
    assert_eq!((toned.0, toned.1), (WIDTH, HEIGHT), "Sepia and Vignette keep the size");
    let border = agent.tool("apply_edit", json!({ "ids": [id], "op": "border", "params": { "outer": BORDER.0, "inner": BORDER.1 } }));
    let stack: Vec<Value> = [&sepia, &vignette, &border].iter().map(|a| a["entry"].clone()).collect();
    for (entry, op) in stack.iter().zip(["sepia(", "vignette(", "border("]) {
        assert!(entry.as_str().is_some_and(|e| e.starts_with(op)), "{stack:?}");
    }
    let info = agent.photo(id);
    assert_eq!(info["recipe"]["list"], json!(stack), "the recipe in the order applied: {info}");
    let recipe = agent.tool("get_recipe", json!({ "id": id }));
    let names: Vec<&str> = recipe["ops"].as_array().unwrap().iter().filter_map(|o| o["op"].as_str()).collect();
    assert_eq!(names, ["sepia", "vignette", "border"], "{recipe}");
    let want: Vec<String> = stack.iter().filter_map(Value::as_str).map(str::to_string).collect();
    eventually(
        "the sidecar records the three effects in order",
        || (edits(&sidecar_entry(&folder, &name)) == want).then_some(()),
        || sidecar(&folder).to_string(),
    );

    // Border grows the render by its bands on every side, at full size and scaled down.
    let band = (BORDER.0 + BORDER.1) * HEIGHT.min(WIDTH) / 1000;
    let framed = rendered(&agent.call("render_photo", json!({ "id": id, "format": "png" })));
    assert_eq!((framed.0, framed.1), (WIDTH + 2 * band, HEIGHT + 2 * band), "Border's bands around the photo");
    let half = (WIDTH + 2 * band) / 2;
    let scaled = rendered(&agent.call("render_photo", json!({ "id": id, "size": half, "format": "png" })));
    assert_eq!((scaled.0, scaled.1), (half, (HEIGHT + 2 * band) / 2), "the scaled render keeps the framed shape");
    // The frame is the Border's: black outside, white inside, the toned photo within.
    let px = |img: &(u32, u32, Vec<u8>), x: u32, y: u32| -> [u8; 3] {
        let i = ((y * img.0 + x) * 4) as usize;
        [img.2[i], img.2[i + 1], img.2[i + 2]]
    };
    assert_eq!(px(&framed, 0, 0), [0, 0, 0], "the outer band");
    assert_eq!(px(&framed, band - 1, framed.1 / 2), [255, 255, 255], "the inner band");
    assert_eq!(px(&framed, band + WIDTH / 2, band + HEIGHT / 2), px(&toned, WIDTH / 2, HEIGHT / 2), "the photo copied unchanged");
    check_fingerprint("w11_effects_stack", &framed);

    // Edits are data: the photo file is untouched.
    assert!(bytes(&folder.join(&name)) == original, "the photo file is untouched");
    agent.quit();
}
