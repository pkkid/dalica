//! M12's effects placed on the photo, in the live app (`m12.rs` part 3): `dalica --headless` on two
//! textured PNGs (a red checker of 2-pixel squares over colour ramps), driven over its control
//! channel at 1400 × 900; where the photo shows is read from a screenshot of the canvas.
//!
//! - **The crosshair on a turned photo:** the first photo turned 90° clockwise; for each of Soft
//!   Focus, Focal B&W, Graduated Tint and Focal Zoom, its crosshair (`crosshair:effect`, first at
//!   the photo's centre) dragged to the shown point (0.25, 0.3) and the panel applied: the entry's
//!   centre is the stored point under it, (0.3, 0.75), within 0.01; one "Undo <effect>" step each.
//! - **Soften painted and erased:** on the second photo, Soften at full softness and no fade, one stroke
//!   painted across the photo at a third of its height, then its right part erased; Apply, then
//!   Save (the original kept in `.dalica-originals/`): in the saved picture the checker is blurred
//!   where the paint stayed and as sharp as before where it was erased and where nothing was
//!   painted.

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, idle, resize, run, secs};
use crate::common::{LARGE, checker, decode, edits, panel, photo_rect, screen, stored_of_turned, textured_png, undo_label};
use crate::live::{self, Client};

const PHOTO: (u32, u32) = (600, 400);

/// The shown point the crosshair is dragged to.
const TO: [f64; 2] = [0.25, 0.3];

/// The first two numbers of a recipe entry (`soft_focus(0.3,0.75,0.3,0.5)` → (0.3, 0.75)).
fn centre(entry: &str) -> [f64; 2] {
    let inner = entry.split_once('(').and_then(|(_, r)| r.strip_suffix(')')).unwrap_or_else(|| panic!("{entry}"));
    let v: Vec<f64> = inner.split(',').take(2).map(|n| n.parse().unwrap_or_else(|_| panic!("{entry}"))).collect();
    [v[0], v[1]]
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
}

/// Opens effect `name`'s panel from its tile on tab `tab`.
fn open(c: &mut Client, tab: &str, name: &str) {
    run(c, "edit.tab", json!({ "tab": tab }));
    click(c, &format!("effect:{name}"));
    assert_eq!(panel(c)["name"], name, "{name}'s panel");
}

/// A stroke across the photo shown at `rect`, from the shown point `from` to `to`, in 16 moves.
fn stroke(c: &mut Client, rect: [f64; 4], from: [f64; 2], to: [f64; 2]) {
    let (a, b) = (screen(rect, from), screen(rect, to));
    let mut events = vec![json!({ "kind": "move", "x": a.0, "y": a.1 }), json!({ "kind": "down", "x": a.0, "y": a.1 })];
    for i in 1..=16 {
        let t = f64::from(i) / 16.0;
        events.push(json!({ "kind": "drag", "x": a.0 + (b.0 - a.0) * t, "y": a.1 + (b.1 - a.1) * t }));
    }
    events.push(json!({ "kind": "up", "x": b.0, "y": b.1 }));
    c.ok("ui.pointer", json!({ "events": events }));
    idle(c);
}

fn crosshairs(c: &mut Client, home: &Path, id: &Value) {
    run(c, "photo.history.apply", json!({ "id": id, "op": "rotate(90)" }));
    run(c, "view.edit", json!({ "id": id }));
    let want = stored_of_turned(TO);
    for (tab, name, label) in [
        ("effects", "soft_focus", "Soft Focus"),
        ("effects", "focal_bw", "Focal B&W"),
        ("effects", "graduated_tint", "Graduated Tint"),
        ("effects3", "focal_zoom", "Focal Zoom"),
    ] {
        open(c, tab, name);
        let rect = photo_rect(c, home, &format!("m12-crosshair-{name}"));
        assert!(rect[3] > rect[2], "{name}: the turned photo shows taller than wide: {rect:?}");
        let mark = app::rect_of(c, "crosshair:effect").expect("the crosshair on the photo");
        let (cx, cy) = app::center(mark);
        let (mx, my) = screen(rect, [0.5, 0.5]);
        assert!(
            (cx - mx).abs() <= 2.0 && (cy - my).abs() <= 2.0,
            "{name}: the crosshair starts at the photo's centre ({cx}, {cy}), not ({mx}, {my})"
        );
        let (x, y) = screen(rect, TO);
        c.ok("ui.dragWidget", json!({ "id": "crosshair:effect", "toX": x, "toY": y }));
        idle(c);
        let p = panel(c)["params"].clone();
        let placed = [p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()];
        assert!(near(placed, want), "{name}: dragged to the shown {TO:?}, the panel holds {placed:?}, not the stored {want:?}");
        let before = edits(c, id);
        click(c, "button:panel.apply");
        let after = edits(c, id);
        assert_eq!(after.len(), before.len() + 1, "{name}: one entry: {after:?}");
        let entry = after.last().unwrap();
        assert!(entry.starts_with(&format!("{name}(")) && near(centre(entry), want), "{name}: applied at {entry}, not {want:?}");
        assert_eq!(undo_label(c, id), format!("Undo {label}"));
    }
    println!("  the crosshairs of Soft Focus, Focal B&W, Graduated Tint and Focal Zoom on a turned photo landed where they were put");
}

fn soften(c: &mut Client, home: &Path, id: &Value, folder: &Path) {
    run(c, "view.edit", json!({ "id": id }));
    open(c, "effects3", "soften");
    c.exec("edit.tool.action", json!({ "name": "set", "params": { "softness": 100, "fade": 0 } }));
    idle(c);
    let rect = photo_rect(c, home, "m12-soften");
    stroke(c, rect, [0.05, 0.3], [0.95, 0.3]);
    click(c, "button:effect.erase");
    stroke(c, rect, [0.55, 0.3], [0.98, 0.3]);
    let strokes = panel(c)["strokes"].clone();
    let erased: Vec<bool> = strokes.as_array().unwrap().iter().map(|s| s["erase"] == true).collect();
    assert_eq!(erased, [false, true], "one stroke painted, one erased: {strokes}");
    click(c, "button:panel.apply");
    let recipe = edits(c, id);
    assert!(recipe.len() == 1 && recipe[0].starts_with("soften(100,0") && recipe[0].matches(';').count() == 2, "{recipe:?}");
    assert_eq!(undo_label(c, id), "Undo Soften");

    let original = std::fs::read(folder.join("Soften.png")).unwrap();
    let done = run(c, "photo.save", json!({ "ids": [id], "confirm": true }));
    assert_eq!(done["state"], "done", "{done}");
    let kept = std::fs::read(folder.join(".dalica-originals").join("Soften.png")).expect("the original kept in .dalica-originals/");
    assert!(kept == original, "the original kept byte for byte");
    let saved = decode(&std::fs::read(folder.join("Soften.png")).unwrap());
    assert_eq!((saved.width, saved.height), PHOTO);
    let plain = decode(&original);
    let (painted, erased_at, untouched) = ([0.3, 0.3], [0.8, 0.3], [0.3, 0.8]);
    let measure = |at: [f64; 2]| (checker(&saved, at), checker(&plain, at));
    let (p, p0) = measure(painted);
    let (e, e0) = measure(erased_at);
    let (u, u0) = measure(untouched);
    assert!(p < p0 / 4.0, "softened where the paint stayed: {p:.1} (before {p0:.1})");
    assert!((e - e0).abs() < 3.0, "as sharp as before where it was erased: {e:.1} (before {e0:.1})");
    assert!((u - u0).abs() < 3.0, "as sharp as before where nothing was painted: {u:.1} (before {u0:.1})");
    println!(
        "  Soften painted then partly erased, saved: the checker {p:.0} where painted (was {p0:.0}), {e:.0} where erased, {u:.0} where never painted; the original kept"
    );
}

pub fn focal(exe: &Path, home: &Path) {
    let started = Instant::now();
    let pictures = home.join("Pictures");
    let folder = pictures.join("Focal");
    let png = textured_png(PHOTO.0, PHOTO.1, 2);
    app::put(&folder.join("Turned.png"), &png, 1);
    app::put(&folder.join("Soften.png"), &png, 2);
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &pictures);
    crosshairs(c, home, &by_path["Focal/Turned.png"].id);
    soften(c, home, &by_path["Focal/Soften.png"].id, &folder);
    live::quit(a, cl);
    println!("  M12 crosshair and Soften in {:.1} s", secs(started));
}
