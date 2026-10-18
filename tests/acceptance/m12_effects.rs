//! M12's 27 effects in the live app (`m12.rs` part 2): `dalica --headless` on a folder of 28 copies
//! of one textured PNG (a red checker of 16-pixel squares over colour ramps, detail a thumbnail
//! still shows), driven over its control channel. Photo 0 stays unedited; photo k gets the
//! k-th effect of `effect.list`:
//!
//! - in the edit view, on the effect's tab, a one-click tile's click applies it; a parameter tile's
//!   click opens its panel, where a value changed and Cancel record nothing (the recipe and the
//!   history's Undo stay as they were, the panel closes), and the tile again then Apply apply it;
//!   either way the recipe holds the one entry and the history offers "Undo <the effect>";
//! - back in the library each edited thumbnail differs from what it showed before the edit (its
//!   badge corner left out), while the unedited photo's stays as it was;
//! - quit and started again, then after Tools ▸ Rebuild (`library.rebuild`), every photo has its
//!   recipe still, found by its path.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, idle, label, quiet, resize, run, secs};
use crate::common::{LARGE, edits, effect_list, panel, px, tab, textured_png, undo_label};
use crate::live::{self, Client, Shot};

const PHOTO: (u32, u32) = (600, 400);

fn name(n: usize) -> String {
    format!("IMG_{n:04}.png")
}

/// The share of a cell judged: its top 60 %, clear of the badge row in the picture's bottom-right
/// corner.
const TOP: f64 = 0.6;

/// The least mean difference per channel by which an edited thumbnail differs from its unedited self.
const CHANGED: f64 = 0.5;

/// The mean difference of one channel (RGB) between a cell's judged part in two screenshots.
fn cell_difference_between(a: &Shot, b: &Shot, r: [f64; 4]) -> f64 {
    let (w, h) = (r[2] as u32, (r[3] * TOP) as u32);
    let mut sum = 0u64;
    for y in 0..h {
        for x in 0..w {
            let p = px(&a.rgba, a.width, r[0] as u32 + x, r[1] as u32 + y);
            let q = px(&b.rgba, b.width, r[0] as u32 + x, r[1] as u32 + y);
            sum += (0..3).map(|i| u64::from(p[i].abs_diff(q[i]))).sum::<u64>();
        }
    }
    sum as f64 / f64::from(w * h * 3)
}

/// Every photo's recipe, by path relative to `root`.
fn recipes(c: &mut Client, root: &Path) -> BTreeMap<String, Vec<String>> {
    let by_path = app::photos(c, root);
    by_path.iter().map(|(rel, p)| (rel.clone(), edits(c, &p.id))).collect()
}

/// The folder's sidecar entries' recipes, by photo name.
fn sidecar(folder: &Path) -> BTreeMap<String, Vec<String>> {
    let v: Value = std::fs::read(folder.join(".dalica.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let mut out = BTreeMap::new();
    for (name, entry) in v["photos"].as_object().into_iter().flatten() {
        let e: Vec<String> = entry["edits"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
        if !e.is_empty() {
            out.insert(format!("Effects/{name}"), e);
        }
    }
    out
}

/// The library's thumbnails of the folder, shown from its tree row: a screenshot and each cell's rect.
fn cells(c: &mut Client, home: &Path, folder: u64, ids: &[Value], label: &str) -> (Shot, Vec<[f64; 4]>) {
    click(c, &format!("tree:folder:{folder}"));
    quiet(c, f64::from(LARGE.1));
    let rects: Vec<[f64; 4]> =
        ids.iter().map(|id| app::rect_of(c, &format!("thumb:{id}")).unwrap_or_else(|| panic!("{label}: thumb:{id} is not shown"))).collect();
    (live::screenshot(c, home, None, label, LARGE.0, LARGE.1), rects)
}

/// The first number parameter of an effect's row and a value in its range other than its default.
fn another_value(e: &Value) -> Option<(String, f64)> {
    let p = e["params"].as_array()?.iter().find(|p| p["kind"] == "number")?;
    let (min, max) = (p["min"].as_f64()?, p["max"].as_f64()?);
    let default: f64 = p["default"].as_str()?.parse().ok()?;
    let v = if (default - min).abs() > (max - default).abs() { (min + default) / 2.0 } else { (default + max) / 2.0 };
    Some((p["name"].as_str()?.to_string(), v))
}

pub fn effects(exe: &Path, home: &Path) {
    let started = Instant::now();
    let pictures = home.join("Pictures");
    let folder = pictures.join("Effects");
    let png = textured_png(PHOTO.0, PHOTO.1, 16);
    for n in 0..=27 {
        app::put(&folder.join(name(n)), &png, n as u64);
    }
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &pictures);
    let ids: Vec<Value> = (0..=27).map(|n| by_path[&format!("Effects/{}", name(n))].id.clone()).collect();
    let folder_id = by_path[&format!("Effects/{}", name(0))].folder;
    let effects = effect_list(c);
    assert_eq!(effects.len(), 27, "the catalogue's 27 effects");
    assert!(effects.iter().all(|e| e["renders"] == true), "every effect renders: {effects:?}");
    let (before, before_rects) = cells(c, home, folder_id, &ids, "m12-effects-cells-before");

    let mut applied = BTreeMap::new();
    for (k, e) in effects.iter().enumerate() {
        let (effect, label_text) = (e["name"].as_str().unwrap(), e["label"].as_str().unwrap());
        let id = ids[k + 1].clone();
        run(c, "view.edit", json!({ "id": id }));
        run(c, "edit.tab", json!({ "tab": tab(e["tab"].as_u64().unwrap()) }));
        let tile = format!("effect:{effect}");
        let undo_before = label(c, "button:edit.undo");
        click(c, &tile);
        if e["oneClick"] != true {
            assert_eq!(panel(c)["name"], effect, "{tile} opens its panel");
            if let Some((p, v)) = another_value(e) {
                c.exec("edit.tool.action", json!({ "name": "set", "params": { p.clone(): v } }));
                idle(c);
                assert_eq!(panel(c)["params"][&p].as_f64(), Some(v), "{effect}: {p} set");
            }
            click(c, "button:panel.cancel");
            assert_eq!(panel(c), Value::Null, "{effect}: Cancel closes the panel");
            assert_eq!(edits(c, &id), Vec::<String>::new(), "{effect}: Cancel records nothing");
            assert_eq!(label(c, "button:edit.undo"), undo_before, "{effect}: nothing to undo after Cancel");
            click(c, &tile);
            click(c, "button:panel.apply");
            assert_eq!(panel(c), Value::Null, "{effect}: Apply closes the panel");
        }
        let recipe = edits(c, &id);
        assert!(recipe.len() == 1 && recipe[0].starts_with(&format!("{effect}(")), "{effect}: one entry: {recipe:?}");
        let want = format!("Undo {label_text}");
        assert_eq!(undo_label(c, &id), want, "{effect}: one step of the photo's history named after it");
        assert_eq!(label(c, "button:edit.undo"), want, "{effect}: the edit view's Undo");
        applied.insert(format!("Effects/{}", name(k + 1)), recipe);
    }
    println!(
        "  27 effects applied, each one step named after it: 4 from their tiles, 23 from their panels (each cancelled once first: nothing recorded)"
    );

    // Every edited thumbnail differs from the unedited photo's.
    click(c, "button:edit.back");
    let (after, rects) = cells(c, home, folder_id, &ids, "m12-effects-cells-after");
    assert_eq!(rects, before_rects, "the cells stay in place");
    let unedited = cell_difference_between(&before, &after, rects[0]);
    assert!(unedited < 0.5, "the unedited photo's thumbnail is as it was ({unedited:.2})");
    let mut faint = Vec::new();
    let mut shown = Vec::new();
    for (k, e) in effects.iter().enumerate() {
        let d = cell_difference_between(&before, &after, rects[k + 1]);
        if d < CHANGED {
            faint.push(format!("{} ({d:.2})", e["name"]));
        }
        shown.push(format!("{} {d:.1}", e["name"].as_str().unwrap()));
    }
    println!("  thumbnails changed by (mean per channel): {}", shown.join(", "));
    assert!(faint.is_empty(), "these thumbnails do not show their effect: {faint:?} ({})", after.path.display());
    println!("  every edited thumbnail changed, the unedited one did not");

    // Over a restart and an index rebuild.
    let pictures_c = pictures.clone();
    app::flush_until(c, || sidecar(&folder) == applied);
    assert_eq!(sidecar(&folder), applied, "the sidecar holds the 27 recipes");
    live::quit(a, cl);
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    idle(c);
    let mut kept = recipes(c, &pictures_c);
    kept.retain(|_, r| !r.is_empty());
    assert_eq!(kept, applied, "every recipe after a restart");
    run(c, "library.rebuild", json!({ "confirm": true }));
    let mut rebuilt = recipes(c, &pictures_c);
    rebuilt.retain(|_, r| !r.is_empty());
    assert_eq!(rebuilt, applied, "every recipe after an index rebuild");
    println!("  the 27 recipes after a restart and after an index rebuild");
    live::quit(a, cl);
    println!("  M12 the 27 effects in {:.1} s", secs(started));
}
