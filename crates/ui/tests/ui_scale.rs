//! UI scale and font size (`design-system.md` §11.4–§11.5, `crates/ui/src/scale.rs`): the factor
//! for system scales of 100, 125 and 150 % and a 3840 px monitor, a fixed 150 % on a 200 % system
//! (egui's zoom 0.75, 1.5 pixels per point, and the window's keys never change it), a fixed 150 %
//! laying the headless window out at 150 % (933 × 600 points in 1400 × 900 pixels), Auto following
//! the window to another monitor, a saved Use animations off making the light table's scroll to a
//! section and the edit view's Fit and 1:1 jump; and the main window at 150 % and with the Large font, each matching its
//! fingerprint at 1400 × 900 points and fitting 900 × 600.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_pixels, new_driver, write_png};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::panels::edit::canvas;
use dalica_ui::scale::{self, factor, zoom};
use dalica_ui::state::{ScaleChoice, ScrollAnchor, UiState};
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::Tokens;
use serde_json::json;

/// A library in `root` with two folders of scenes photos, added and scanned.
fn session(root: &Path) -> Session {
    let files = crate::common::demo_library(2, 12, 3).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

/// The library of [`session`] in a window of `points` at `ppp` pixels per point, with `options`
/// saved through `options.set` (none when empty).
fn driver(root: &Path, points: (u32, u32), ppp: f32, options: serde_json::Value) -> Driver {
    let mut s = session(root);
    if options.as_object().is_some_and(|o| !o.is_empty()) {
        s.execute("options.set", &json!({ "values": options })).unwrap();
    }
    let (w, h) = ((points.0 as f32 * ppp).round() as u32, (points.1 as f32 * ppp).round() as u32);
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), w, h, ppp);
    d.settle(FRAMES).unwrap();
    d
}

#[test]
fn the_factor_follows_the_system_and_the_4k_rule_and_a_fixed_choice_ignores_both() {
    for (system, want) in [(1.0, 1.0), (1.25, 1.25), (1.5, 1.5)] {
        assert_eq!(factor(ScaleChoice::Auto, Some(system), Some(1920)), want, "{system}");
        assert_eq!(zoom(ScaleChoice::Auto, Some(system), Some(1920)), 1.0, "Auto adds nothing to the system's own scale");
    }
    assert_eq!(factor(ScaleChoice::Auto, Some(1.0), Some(3840)), 2.0, "a 4K monitor at 100 %");
    assert_eq!(zoom(ScaleChoice::Auto, Some(1.0), Some(3840)), 2.0);
    assert_eq!(factor(ScaleChoice::Auto, Some(1.5), Some(3840)), 1.5, "the system already scales");
    assert_eq!(factor(ScaleChoice::Auto, Some(1.0), None), 1.0, "an unknown monitor follows the system");
    assert_eq!(factor(ScaleChoice::Percent(150), Some(2.0), Some(3840)), 1.5);
    assert_eq!(zoom(ScaleChoice::Percent(150), Some(2.0), Some(3840)), 0.75, "a fixed 150 % on a 200 % system");
    for bad in [None, Some(f32::NAN), Some(0.0), Some(-1.0), Some(100.0)] {
        assert_eq!(zoom(ScaleChoice::Auto, bad, None), 1.0, "{bad:?} counts as 100 %");
        assert_eq!(zoom(ScaleChoice::Percent(150), bad, None), 1.5, "{bad:?}");
    }
}

#[test]
fn a_fixed_150_percent_on_a_200_percent_system_is_150_percent_and_keys_never_zoom_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (700, 450), 2.0, json!({ "interface.uiScale": 150 }));
    assert!((d.ctx().zoom_factor() - 0.75).abs() < 1e-4, "{}", d.ctx().zoom_factor());
    assert!((d.ctx().pixels_per_point() - 1.5).abs() < 1e-4, "{}", d.ctx().pixels_per_point());
    for key in ["Ctrl+-", "Ctrl+=", "Ctrl+0"] {
        d.key(key).unwrap();
        d.settle(FRAMES).unwrap();
        assert!((d.ctx().zoom_factor() - 0.75).abs() < 1e-4, "{key}: {}", d.ctx().zoom_factor());
    }
}

#[test]
fn a_fixed_150_percent_lays_the_headless_window_out_at_150_percent() {
    let dir = tempfile::tempdir().unwrap();
    let d = driver(dir.path(), LARGE, 1.0, json!({ "interface.uiScale": 150 }));
    assert!((d.ctx().zoom_factor() - 1.5).abs() < 1e-4, "{}", d.ctx().zoom_factor());
    let size = d.ctx().content_rect().size();
    assert!((size.x - 1400.0 / 1.5).abs() < 0.1 && (size.y - 600.0).abs() < 0.1, "933 × 600 points: {:?}", (size.x, size.y));
    let shot = d.screenshot().unwrap();
    write_png("ui-scale-150-fixed-window", &fingerprint::png_rgba(shot.width, shot.height, &shot.rgba));
    assert_eq!((shot.width, shot.height), (1400, 900), "drawn at 1.5 pixels per point");
}

/// One pass of `ctx` on a monitor `monitor` points large at the native scale 1, with
/// [`scale::apply`] at its start; returns the zoom the next pass runs at (egui takes a new zoom at
/// the start of a pass).
fn pass_on_monitor(ctx: &egui::Context, tokens: &mut Tokens, state: &UiState, monitor: egui::Vec2) -> f32 {
    let input = || {
        let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
        input.viewports.entry(egui::ViewportId::ROOT).or_default().monitor_size = Some(monitor);
        input
    };
    ctx.run_ui(input(), |ui| scale::apply(ui.ctx(), state, None, tokens)).textures_delta.clear();
    let mut next = 0.0;
    ctx.run_ui(input(), |ui| next = ui.ctx().zoom_factor()).textures_delta.clear();
    next
}

#[test]
fn auto_follows_the_window_to_another_monitor() {
    let ctx = egui::Context::default();
    let (mut tokens, state) = (Tokens::default(), UiState::default());
    assert_eq!(pass_on_monitor(&ctx, &mut tokens, &state, egui::vec2(1920.0, 1080.0)), 1.0);
    // Moved to a 4K monitor (portrait: its long side counts): 200 %.
    assert_eq!(pass_on_monitor(&ctx, &mut tokens, &state, egui::vec2(2160.0, 3840.0)), 2.0);
    // Its size is now given at 2 pixels per point; it stays 4K.
    assert_eq!(pass_on_monitor(&ctx, &mut tokens, &state, egui::vec2(1080.0, 1920.0)), 2.0);
    // Back on the 1080p monitor (960 × 540 points at 2): 100 %.
    assert_eq!(pass_on_monitor(&ctx, &mut tokens, &state, egui::vec2(960.0, 540.0)), 1.0);
}

#[test]
fn with_animations_off_the_scroll_to_a_section_jumps() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE, 1.0, json!({ "interface.animations": false }));
    assert!(!scale::animations(d.ctx()));
    assert_eq!(d.ctx().global_style().animation_time, 0.0);
    d.app_mut().state_mut().thumb_size = dalica_ui::state::THUMB_MAX;
    d.settle(FRAMES).unwrap();
    let last = d.app().panels().unwrap().sections.list.last().unwrap().id;
    d.app_mut().state_mut().scroll = ScrollAnchor { section: Some(last), offset: 0.0 };
    d.step();
    let lt = &d.app().panels().unwrap().light_table;
    assert!(!lt.animating() && lt.offset() > 0.0, "there in one frame: {}", lt.offset());
}

#[test]
fn with_animations_off_fit_and_1to1_jump() {
    // One photo larger than the photo area.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Large");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("BIG.jpg"), dalica_scenes::jpeg(&dalica_scenes::Scene::new(5, 2400, 1600), 85, None).unwrap()).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.execute("options.set", &json!({ "values": { "interface.animations": false } })).unwrap();
    let photo = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": photo.get() })).unwrap();
    d.settle(FRAMES).unwrap();
    let fit = canvas::view(d.ctx()).unwrap().fit;
    assert!(fit < 1.0, "the photo is larger than the area: {fit}");
    for (command, want) in [("edit.zoom.actual", 1.0), ("edit.zoom.fit", fit)] {
        d.run(command, &json!({})).unwrap();
        d.step();
        let v = canvas::view(d.ctx()).unwrap();
        assert!(!v.animating && (v.scale - want).abs() < 1e-4, "{command}: {v:?}");
    }
}

/// The window at `ppp` (`points` large) matches `<name>-1400x900`, then fits 900 × 600 points.
fn check_window(d: &mut Driver, ppp: f32, name: &str) {
    let s = d.screenshot().unwrap();
    write_png(&format!("{name}-window"), &fingerprint::png_rgba(s.width, s.height, &s.rgba));
    check_pixels(&format!("{name}-{}x{}", LARGE.0, LARGE.1), s.width, s.height, &s.rgba);
    d.set_size((SMALL.0 as f32 * ppp).round() as u32, (SMALL.1 as f32 * ppp).round() as u32, ppp);
    d.settle(FRAMES).unwrap();
    let shot = d.screenshot().unwrap();
    write_png(&format!("{name}-{}x{}", SMALL.0, SMALL.1), &fingerprint::png_rgba(shot.width, shot.height, &shot.rgba));
    for w in d.widgets() {
        let [x, y, rw, rh] = w.rect;
        let inside = x >= 0.0 && y >= 0.0 && x + rw <= SMALL.0 as f32 + 0.01 && y + rh <= SMALL.1 as f32 + 0.01;
        assert!(rw > 0.0 && rh > 0.0 && inside, "{name} at 900 × 600: `{}` at {:?}", w.id, w.rect);
    }
}

#[test]
fn the_main_window_at_150_percent_matches_its_fingerprint_and_fits_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE, 1.5, json!({}));
    assert_eq!(d.ctx().content_rect().size(), egui::vec2(1400.0, 900.0), "1400 × 900 points");
    check_window(&mut d, 1.5, "ui-scale-150");
}

#[test]
fn the_main_window_with_the_large_font_matches_its_fingerprint_and_fits_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE, 1.0, json!({ "interface.fontSize": "large" }));
    assert!((d.app().tokens().body - 17.0).abs() < 1e-3);
    check_window(&mut d, 1.0, "ui-scale-large");
}
