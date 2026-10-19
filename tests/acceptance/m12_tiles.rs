//! M12's effect tabs' grids in the live app (`m12.rs` part 1): `dalica --headless` on three copies
//! of a 6 MP `scenes` photo and a small one, in the edit view at 1400 × 900, driven over its
//! control channel; for each tab a copy is opened (from the small photo) while the tab shows, so
//! its tiles wait behind its preview, as when a user steps to a photo:
//!
//! - each effect tab (3, 4, 5) lists `effect:<name>` tiles in `effect.list`'s order, labelled
//!   as the catalogue labels them;
//! - the window never waits for the previews: frame after frame is drawn while `ui.settle` still
//!   reports the tiles on their way, and no frame drawn while they are on their way (the frames
//!   that lay out the opened photo aside) comes near the time the grid takes to fill in UI time
//!   (`ui.inspect` → `perf.maxFrameMs`);
//! - once settled every tile holds its preview: most of its preview box is no longer the
//!   `gray 38` placeholder;
//! - each tab, every tile in, matches `fingerprints/m12-1400x900-effects-tab<n>.json`, and Duo-Tone's
//!   panel, open with its final preview, `fingerprints/m12-1400x900-effect-panel-duotone.json` (the
//!   status line and the activity indicator masked).

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, idle, quiet, resize, run, secs, widget};
use crate::common::{LARGE, effect_list, panel, px, tab};
use crate::live::{self, Client};

/// The photo: a 6 MP camera's, so the grid takes a while to fill.
const PHOTO: (u32, u32) = (3000, 2000);
/// The `placeholder` token.
const GREY: [u8; 3] = [38, 38, 38];
/// A tile's preview: 78 × 48 points, 3 points under the tile's top, centred.
const PREVIEW: (f64, f64) = (78.0, 48.0);
const PREVIEW_TOP: f64 = 3.0;
/// The frames a step to another photo takes to lay it out (egui may lay a new view out twice).
const OPENING_FRAMES: u64 = 3;

/// The share of a tile's preview box that is not the placeholder.
fn filled(shot: &live::Shot, rect: [f64; 4]) -> f64 {
    let x0 = (rect[0] + (rect[2] - PREVIEW.0) / 2.0).round() as u32;
    let y0 = (rect[1] + PREVIEW_TOP).round() as u32;
    let (w, h) = (PREVIEW.0 as u32, PREVIEW.1 as u32);
    let mut not_grey = 0u32;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            if px(&shot.rgba, shot.width, x, y) != GREY {
                not_grey += 1;
            }
        }
    }
    f64::from(not_grey) / f64::from(w * h)
}

/// Opens photo `id` in the edit view while effect tab `name` shows (another photo open on it
/// first), as a user stepping to a photo on that tab does: its tiles wait behind its preview. Then
/// lets the grid fill one frame at a time until it settles. Returns (frames drawn while the tiles
/// were on their way, the fill's wall time in ms, the largest UI time of a frame meanwhile in ms).
///
/// The frames that lay out the newly opened photo ([`OPENING_FRAMES`]) are left out of the largest
/// frame: they wait for nothing, but they are the dearest frames of the step, and with the tiles'
/// workers on every core a busy runner stretches them past half of a short fill (macOS and Windows
/// CI, 50 of 95 ms and 68 of 132 ms). The fill's time still runs from the step.
fn fill(c: &mut Client, name: &str, other: &Value, id: &Value) -> (u64, f64, f64) {
    run(c, "view.edit", json!({ "id": other }));
    run(c, "edit.tab", json!({ "tab": name }));
    let started = Instant::now();
    c.exec("view.edit", json!({ "id": id }));
    assert_eq!(c.state()["edit"]["tab"], name, "the edit view stays on the tab");
    let opening = c.ok("ui.settle", json!({ "timeoutMs": 17 * OPENING_FRAMES }));
    let busy = opening["busy"].as_array().unwrap();
    assert!(
        busy.iter().any(|b| b.as_str().is_some_and(|b| b.starts_with("effect tiles:"))),
        "{name}: the tiles are still on their way after the opening frames: {opening}"
    );
    // A reset keeps the last frame drawn as the largest; one more frame makes that one a frame of
    // the fill, not the opening.
    c.ok("ui.settle", json!({ "timeoutMs": 17 }));
    c.ok("ui.inspect", json!({ "resetPerf": true }));
    let mut pending_frames = 0;
    loop {
        let s = c.ok("ui.settle", json!({ "timeoutMs": 17 }));
        let busy: Vec<String> = s["busy"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect();
        if busy.iter().any(|b| b.starts_with("effect tiles:")) {
            pending_frames += s["frames"].as_u64().unwrap_or(0);
        }
        if busy.is_empty() {
            break;
        }
        assert!(started.elapsed().as_secs() < 300, "{name}: the grid never filled: {s}");
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    c.settle();
    let perf = c.ok("ui.inspect", json!({}))["perf"].clone();
    let max = perf["maxFrameMs"].as_f64().unwrap_or_else(|| panic!("ui.inspect perf: {perf}"));
    (pending_frames, ms, max)
}

pub fn tiles(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder = home.join("Pictures").join("Harbour");
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1212, PHOTO.0, PHOTO.1), 90, None).unwrap();
    // One copy per tab (a preview already made is kept), and a small photo to step from.
    for n in 3..=5 {
        app::put(&folder.join(format!("Harbour {n}.jpg")), &jpeg, n);
    }
    let small = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1213, 300, 200), 90, None).unwrap();
    app::put(&folder.join("Small.jpg"), &small, 1);
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let other = by_path["Harbour/Small.jpg"].id.clone();
    let effects = effect_list(c);
    assert_eq!(effects.len(), 27, "the catalogue's 27 effects");

    for (n, count) in [(3u64, 12usize), (4, 7), (5, 8)] {
        let rows: Vec<&Value> = effects.iter().filter(|e| e["tab"] == n).collect();
        assert_eq!(rows.len(), count, "tab {n}");
        let (pending_frames, ms, max) = fill(c, tab(n), &other, &by_path[&format!("Harbour/Harbour {n}.jpg")].id);
        assert!(pending_frames > 0, "tab {n}: frames went on while the tiles were on their way");
        assert!(max < ms / 2.0, "tab {n}: a frame took {max:.0} ms of the {ms:.0} ms the grid took to fill: the window waited");
        idle(c);
        quiet(c, f64::from(LARGE.1));
        let shot = live::screenshot(c, home, None, &format!("m12-tab{n}-tiles"), LARGE.0, LARGE.1);
        let tiles: Vec<Value> = c.widgets("effect:").into_iter().filter(|w| w["id"].as_str().is_some_and(|id| id.starts_with("effect:"))).collect();
        let ids: Vec<String> = tiles.iter().map(|w| w["id"].as_str().unwrap().to_string()).collect();
        let want: Vec<String> = rows.iter().map(|e| format!("effect:{}", e["name"].as_str().unwrap())).collect();
        assert_eq!(ids, want, "tab {n}: the tiles in the catalogue's order");
        for (w, e) in tiles.iter().zip(&rows) {
            assert_eq!(w["label"], e["label"], "{}", w["id"]);
            assert_eq!(w["enabled"], true, "{}", w["id"]);
            let r = [0, 1, 2, 3].map(|i| w["rect"][i].as_f64().unwrap());
            let share = filled(&shot, r);
            assert!(share > 0.5, "{}: its preview arrived ({:.0} % of the box is not gray 38; {})", w["id"], share * 100.0, shot.path.display());
        }
        println!(
            "  tab {n}: {count} tiles in order, the grid filled in {ms:.0} ms with {pending_frames} frames drawn meanwhile (longest frame {max:.0} ms); every preview in"
        );
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m12-1400x900-effects-tab{n}"), LARGE, &masks);
    }

    // Duo-Tone's panel, its final preview in.
    run(c, "edit.tab", json!({ "tab": "effects2" }));
    click(c, "effect:duotone");
    assert_eq!(panel(c)["name"], "duotone", "the tile opened its panel");
    assert!(widget(c, "color:effect.first").is_some() && widget(c, "slider:effect.fade").is_some(), "the panel's swatches and sliders");
    idle(c);
    quiet(c, f64::from(LARGE.1));
    let masks = app::moving_parts(c);
    app::masked_shot(c, home, "m12-1400x900-effect-panel-duotone", LARGE, &masks);
    click(c, "button:panel.cancel");
    println!("  Duo-Tone's panel matches its fingerprint");
    live::quit(a, cl);
    println!("  M12 tile grids in {:.1} s", secs(started));
}
