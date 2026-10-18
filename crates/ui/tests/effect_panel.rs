//! The effects' parameter panel (`editing.md` §8.1), headless and by id: every parameter effect's
//! panel opens with the controls its catalogue row lists; Tint's colour picked in the popup
//! changes the preview and not the recipe, Apply records one "Tint" step with that colour, Cancel
//! leaves the recipe and the preview as before; the eyedropper on a planted patch sets the swatch
//! to its colour; Fade 100 shows the photo unchanged; Film Grain's Lighten and the slider's
//! double-click reset work; at 900 × 600 the panel and the popup fit; fingerprints of Tint's, Film
//! Grain's and Duo-Tone's panels and of the open picker at 1400 × 900 (the panel and the popup
//! only: nothing a renderer draws).
//!
//! `DALICA_SHOTS=<dir>` writes the panels' screenshots there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_edit::catalog::{self, Crosshair, Kind};
use dalica_engine::{Quality, Session};
use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata};
use dalica_ui::headless::{Click, PointerEvent, PointerKind};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2};
use serde_json::{Value, json};

/// The photo every test edits: 480 × 320, its left half the planted colour, its right half blue.
const NAME: &str = "Halves.png";
const SIZE: (u32, u32) = (480, 320);
const PLANTED: [u8; 3] = [200, 40, 30];
const RIGHT: [u8; 3] = [30, 60, 190];

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    photo: PhotoId,
}

fn halves() -> Vec<u8> {
    let mut data = Vec::with_capacity((SIZE.0 * SIZE.1 * 4) as usize);
    for _ in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let c = if x < SIZE.0 / 2 { PLANTED } else { RIGHT };
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    let img = Rgba8 { width: SIZE.0, height: SIZE.1, data };
    encode::png(&Image::rgba8(&img), &Metadata::default()).unwrap()
}

/// A library holding the photo, open in the edit view, the window at `size`.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(NAME), halves()).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let photo = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": photo })).unwrap();
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, photo }
}

/// The edit tab holding effect `name`.
fn tab_of(name: &str) -> &'static str {
    match catalog::lookup(name).and_then(|op| op.effect).map(|e| e.tab) {
        Some(4) => "effects2",
        Some(5) => "effects3",
        _ => "effects",
    }
}

/// Shows effect `name`'s tab and opens its panel from its tile.
fn open(t: &mut T, name: &str) {
    t.d.run("edit.tab", &json!({ "tab": tab_of(name) })).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.d.click(&format!("effect:{name}")).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().edit.effect.as_ref().map(|e| e.name.as_str()), Some(name));
}

fn edits(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().library().info(t.photo).edits.clone()
}

/// The recipe the preview shows now.
fn shown(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().preview().map(|p| p.recipe.clone()).unwrap_or_default()
}

/// Steps until the preview's final picture shows `recipe`; its pixels.
fn final_of(t: &mut T, recipe: &[String]) -> Rgba8 {
    t.d.step_until(
        |app| {
            std::thread::sleep(Duration::from_millis(1));
            app.session().and_then(|s| s.preview()).is_some_and(|p| p.recipe == recipe && p.quality == Quality::Final)
        },
        FRAMES,
    )
    .unwrap();
    (*t.d.app().session().unwrap().preview().unwrap().image).clone()
}

/// The value of parameter `p` set on the open panel (`null` when it is at its default unset).
fn param(t: &T, p: &str) -> Value {
    let e = t.d.state().edit.effect.clone().unwrap();
    e.params.get(p).map_or(Value::Null, |v| serde_json::to_value(v).unwrap())
}

/// The automation ids the catalogue row of `name` asks its panel for, derived here on their own.
fn expected_ids(name: &str) -> Vec<String> {
    let op = catalog::lookup(name).unwrap();
    let crosshair: &[&str] = match op.effect.unwrap().crosshair {
        Crosshair::Circle => &["x", "y"],
        Crosshair::Gradient => &["x", "y", "angle"],
        Crosshair::None => &[],
    };
    let mut ids: Vec<String> = op
        .params
        .iter()
        .filter(|p| !crosshair.contains(&p.name))
        .filter_map(|p| match p.kind {
            Kind::Number { .. } => Some(format!("slider:effect.{}", p.name)),
            Kind::Flag => Some(format!("check:effect.{}", p.name)),
            Kind::Choice(_) => Some(format!("choice:effect.{}", p.name)),
            Kind::Colour { .. } => Some(format!("color:effect.{}", p.name)),
            _ => None,
        })
        .collect();
    ids.sort();
    ids
}

/// The panel's control ids drawn in the last frame.
fn control_ids(t: &T) -> Vec<String> {
    let mut ids: Vec<String> = t
        .d
        .widgets()
        .into_iter()
        .map(|w| w.id)
        .filter(|id| ["slider:effect.", "check:effect.", "choice:effect.", "color:effect."].iter().any(|p| id.starts_with(p)) && !id.contains('/'))
        .collect();
    ids.sort();
    ids
}

fn parameter_effects() -> Vec<&'static str> {
    catalog::TABS.iter().flat_map(|n| catalog::effects(*n)).filter(|(_, e)| !e.one_click).map(|(op, _)| op.name).collect()
}

/// The panel from its title to its buttons, across the edit panel's content.
fn panel_rect(t: &T) -> [f32; 4] {
    let panel = widget(&t.d, "region:editPanel").rect;
    let first = t.d.widgets().into_iter().find(|w| control_ids(t).contains(&w.id)).unwrap().rect;
    let apply = widget(&t.d, "button:panel.apply").rect;
    // The title row (26) and the gap (8) above the first control.
    let top = first[1] - 34.0;
    [panel[0] + 8.0, top, panel[2] - 16.0, apply[1] + apply[3] - top]
}

/// Every parameter effect's panel shows exactly the controls its catalogue row lists, plus Reset,
/// Apply and Cancel; the grid is gone while it is open, and Cancel records nothing.
#[test]
fn every_parameter_effect_opens_with_its_catalogues_controls() {
    let mut t = setup(LARGE);
    let names = parameter_effects();
    assert_eq!(names.len(), 23);
    for name in names {
        open(&mut t, name);
        assert_eq!(control_ids(&t), expected_ids(name), "{name}");
        for id in ["button:panel.reset", "button:panel.apply", "button:panel.cancel"] {
            assert!(try_widget(&t.d, id).is_some(), "{name}: {id}");
        }
        assert!(!t.d.widgets().iter().any(|w| w.id.starts_with("effect:")), "{name}: the panel replaces the grid");
        t.d.click("button:panel.cancel").unwrap();
        t.d.settle(FRAMES).unwrap();
        assert_eq!(t.d.state().edit.tool, None, "{name}");
    }
    assert!(edits(&t).is_empty());
}

/// Tint's colour picked in the popup previews and leaves the recipe; Apply is one "Tint" step with
/// it; another session's Cancel leaves the recipe and the preview as they were. The open picker
/// matches its fingerprint, and so does Tint's panel.
#[test]
fn tints_colour_previews_until_apply_records_one_step_and_cancel_records_none() {
    let mut t = setup(LARGE);
    let plain = final_of(&mut t, &[]);
    open(&mut t, "tint");
    check_rect(&t.d, &format!("effect-panel-tint-{}x{}", LARGE.0, LARGE.1), panel_rect(&t));
    let default_shown = final_of(&mut t, &["tint(#80cfff,0)".to_string()]);
    assert_ne!(default_shown.data, plain.data, "the default tint shows");

    t.d.click("color:effect.color").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().edit.effect.as_ref().unwrap().picker.as_deref(), Some("color"));
    assert!(widget(&t.d, "color:effect.color").active, "the swatch is active while its popup is open");
    check_rect(&t.d, &format!("effect-panel-picker-{}x{}", LARGE.0, LARGE.1), widget(&t.d, "color:effect.color/popup").rect);
    shot(&t.d, "effect-panel-picker-window");
    // A hue low on the strip and a saturated, bright point of the field.
    t.d.click_with("color:effect.color/hue", &Click { fy: 0.8, ..Click::default() }).unwrap();
    t.d.click_with("color:effect.color/field", &Click { fx: 0.9, fy: 0.1, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    let picked = param(&t, "color").as_str().unwrap().to_owned();
    assert_ne!(picked, "#80cfff");
    let want = format!("tint({picked},0)");
    let previewed = final_of(&mut t, std::slice::from_ref(&want));
    assert_ne!(previewed.data, default_shown.data, "the preview follows the colour");
    assert!(edits(&t).is_empty(), "nothing reaches the recipe before Apply");

    // Esc closes the picker (it lies over the buttons), not the panel.
    t.d.key("Esc").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().edit.effect.as_ref().map(|e| e.picker.clone()), Some(None));
    assert!(try_widget(&t.d, "color:effect.color/popup").is_none());
    t.d.click("button:panel.apply").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&t), std::slice::from_ref(&want));
    assert_eq!(widget(&t.d, "button:edit.undo").label, "Undo Tint");
    assert_eq!(t.d.state().edit.tool, None);
    assert_eq!(shown(&t), std::slice::from_ref(&want));

    // A second session, cancelled.
    open(&mut t, "tint");
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": { "color": "#00ff00", "fade": 40 } })).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(shown(&t).last().map(String::as_str), Some("tint(#00ff00,40)"));
    t.d.key("Esc").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().edit.tool, None, "Esc cancels the panel");
    assert_eq!(edits(&t), std::slice::from_ref(&want), "Cancel records nothing");
    final_of(&mut t, &[want]);
}

/// The eyedropper, armed in the picker, then a click on the photo's planted half: the swatch takes
/// that colour (the photo as shown before the effect), the eyedropper disarms and the popup stays.
#[test]
fn the_eyedropper_on_a_planted_patch_sets_the_swatch_to_its_colour() {
    let mut t = setup(LARGE);
    open(&mut t, "tint");
    t.d.click("color:effect.color").unwrap();
    t.d.click("color:effect.color/eyedropper").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().edit.effect.as_ref().unwrap().eyedropper.as_deref(), Some("color"));
    // A point on the photo's left half, wherever the canvas fits it: the photo is centred in the
    // area, so a little left of the centre lies on the left half.
    let area = widget(&t.d, "canvas:edit").rect;
    let at = pos2(area[0] + area[2] * 0.45, area[1] + area[3] * 0.5);
    let ev = |kind| PointerEvent { kind, pos: at };
    t.d.pointer(&[ev(PointerKind::Move), ev(PointerKind::Down), ev(PointerKind::Up)], PointerButton::Primary, Modifiers::NONE).unwrap();
    let hex = dalica_core::color::hex_u8(PLANTED);
    t.d.step_until(
        |app| {
            std::thread::sleep(Duration::from_millis(1));
            app.state()
                .edit
                .effect
                .as_ref()
                .and_then(|e| e.params.get("color"))
                .is_some_and(|v| *v == dalica_ui::state::EffectValue::Text(hex.clone()))
        },
        FRAMES,
    )
    .unwrap();
    let e = t.d.state().edit.effect.clone().unwrap();
    assert_eq!((e.eyedropper, e.picker.as_deref()), (None, Some("color")), "disarmed, the popup still open");
    // The script's pick: the other half.
    t.d.run("edit.tool.action", &json!({ "name": "pick", "params": { "x": 0.9, "y": 0.5 } })).unwrap();
    let blue = dalica_ui::state::EffectValue::Text(dalica_core::color::hex_u8(RIGHT));
    t.d.step_until(
        |app| {
            std::thread::sleep(Duration::from_millis(1));
            app.state().edit.effect.as_ref().and_then(|e| e.params.get("color")).is_some_and(|v| *v == blue)
        },
        FRAMES,
    )
    .unwrap();
    assert!(edits(&t).is_empty());
}

/// Fade 100 previews the photo unchanged.
#[test]
fn fade_100_shows_the_photo_unchanged() {
    let mut t = setup(LARGE);
    let plain = final_of(&mut t, &[]);
    open(&mut t, "tint");
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": { "fade": 100 } })).unwrap();
    let faded = final_of(&mut t, &["tint(#80cfff,100)".to_string()]);
    assert_eq!((faded.width, faded.height), (plain.width, plain.height));
    let most = faded.data.iter().zip(&plain.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(most <= 1, "Fade 100 is the photo (off by {most})");
}

/// Film Grain's Lighten checkbox and its slider: a click on Lighten sets it in the preview, the
/// slider moves the grain and its double-click resets it; the panel matches its fingerprint.
#[test]
fn film_grains_lighten_and_the_sliders_double_click_reset() {
    let mut t = setup(LARGE);
    open(&mut t, "grain");
    check_rect(&t.d, &format!("effect-panel-grain-{}x{}", LARGE.0, LARGE.1), panel_rect(&t));
    t.d.click("check:effect.lighten").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(param(&t, "lighten"), json!(true));
    assert!(widget(&t.d, "check:effect.lighten").active);
    t.d.click_with("slider:effect.grain", &Click { fx: 0.9, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    let moved = param(&t, "grain").as_f64().unwrap();
    assert!(moved > 30.0, "a click near the end moves the grain up: {moved}");
    assert_eq!(shown(&t).last().cloned(), Some(format!("grain({moved},true)")));
    // Past the double-click time, so the earlier click does not join the next two.
    t.d.wait(600).unwrap();
    t.d.click_with("slider:effect.grain", &Click { fx: 0.9, count: 2, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(param(&t, "grain"), json!(10.0), "the double-click resets the default");
    // Reset puts every value back.
    t.d.click("button:panel.reset").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(param(&t, "lighten"), json!(false));
    t.d.click("button:panel.apply").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&t), ["grain(10,false)"]);
    assert_eq!(widget(&t.d, "button:edit.undo").label, "Undo Film Grain");
}

/// Duo-Tone's panel (three sliders and two swatches) matches its fingerprint; a bad value set by a
/// script changes nothing.
#[test]
fn duotones_panel_matches_its_fingerprint() {
    let mut t = setup(LARGE);
    open(&mut t, "duotone");
    check_rect(&t.d, &format!("effect-panel-duotone-{}x{}", LARGE.0, LARGE.1), panel_rect(&t));
    shot(&t.d, "effect-panel-duotone-window");
    let before = t.d.state().edit.effect.clone();
    for bad in [json!({ "brightness": 500 }), json!({ "first": "red" }), json!({ "nope": 1 }), json!({ "contrast": 10, "fade": "x" })] {
        assert!(t.d.run("edit.tool.action", &json!({ "name": "set", "params": bad })).is_err(), "{bad}");
        assert_eq!(t.d.state().edit.effect, before, "{bad}");
    }
}

/// At 900 × 600 the tallest panels lie inside the edit panel above Undo, and their pickers inside
/// the window.
#[test]
fn the_panel_and_the_picker_fit_the_small_window() {
    let mut t = setup(SMALL);
    for (name, colour) in [("drop_shadow", "background"), ("border", "inner_color"), ("duotone", "second"), ("pixelate", "")] {
        open(&mut t, name);
        let panel = widget(&t.d, "region:editPanel").rect;
        let undo = widget(&t.d, "button:edit.undo").rect;
        let mut ids = control_ids(&t);
        ids.extend(["button:panel.reset", "button:panel.apply", "button:panel.cancel"].map(String::from));
        for id in &ids {
            let r = widget(&t.d, id).rect;
            assert!(r[0] >= panel[0] && r[0] + r[2] <= panel[0] + panel[2] + 0.01, "{name}: {id} at {r:?} across {panel:?}");
            assert!(r[1] >= panel[1] && r[1] + r[3] <= undo[1] + 0.01, "{name}: {id} at {r:?} above Undo at {undo:?}");
        }
        check_fits(&t.d, name);
        if !colour.is_empty() {
            t.d.click(&format!("color:effect.{colour}")).unwrap();
            t.d.settle(FRAMES).unwrap();
            assert!(try_widget(&t.d, &format!("color:effect.{colour}/popup")).is_some(), "{name}");
            check_fits(&t.d, &format!("{name} picker"));
            shot(&t.d, &format!("effect-panel-{name}-{}x{}", SMALL.0, SMALL.1));
            t.d.key("Esc").unwrap();
            t.d.settle(FRAMES).unwrap();
            assert_eq!(t.d.state().edit.tool, Some(dalica_ui::state::ToolKind::Effect), "{name}: Esc closed the picker only");
        }
        t.d.click("button:panel.cancel").unwrap();
        t.d.settle(FRAMES).unwrap();
    }
}
