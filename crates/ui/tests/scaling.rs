//! UI scale and font size (M15.29, `design-system.md` §9 and §11.4–§11.5): at UI scale 150 % with the
//! Large font, the main window at 1400 × 900 pixels (933 × 600 points), the edit view and every
//! dialog keep their controls inside the window and their labels unclipped: no control is cut by
//! the window or by the region it is drawn in, and no text is cut off at the side of the area it is
//! drawn in (a text running past the bottom of a scrolled list is scrolled, not clipped). An
//! ellipsized label keeps its full text in AccessKit (`accessibility.rs` checks the cells').
//! `DALICA_SHOTS=<dir>` writes the screens there.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::accessibility::frame;
use crate::common::{FRAMES, LARGE, demo_open, new_driver, shot};
use dalica_ui::state::{Dialog, DialogKind};
use dalica_ui::{App, Driver};
use egui::epaint::{ClippedShape, Shape};
use egui::{Id, Rect};
use serde_json::json;

/// The demo library at 1400 × 900 pixels, UI scale 150 % and the Large font.
fn scaled(dir: &Path) -> Driver {
    let mut s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    s.execute("options.set", &json!({ "values": { "interface.uiScale": 150, "interface.fontSize": "large" } })).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    assert!((d.ctx().zoom_factor() - 1.5).abs() < 1e-4, "UI scale 150 %: zoom {}", d.ctx().zoom_factor());
    d
}

/// Every text of `shapes` with the rect it is drawn in and its clip.
fn texts(shapes: &[ClippedShape]) -> Vec<(String, Rect, Rect)> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            Shape::Text(t) if !t.galley.text().trim().is_empty() => out.push((t.galley.text().to_owned(), t.visual_bounding_rect(), clip)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, s.clip_rect, &mut out);
    }
    out
}

/// What is clipped in the next frame of `d`, one line each.
fn clipped(d: &mut Driver, place: &str) -> Vec<String> {
    let out = frame(d, Vec::new());
    let screen = d.ctx().content_rect();
    let mut problems = Vec::new();
    // Texts cut at the side of their clip: started inside it, ran out past its left or right.
    for (text, r, clip) in texts(&out.shapes) {
        let visible = r.intersect(clip);
        if !visible.is_positive() {
            continue; // scrolled out of sight
        }
        if r.left() < clip.left() - 0.5 || r.right() > clip.right() + 0.5 {
            problems.push(format!("{place}: text {text:?} cut at the side: {r:?} in {clip:?}"));
        }
        if r.right() > screen.right() + 0.5 {
            problems.push(format!("{place}: text {text:?} outside the window: {r:?}"));
        }
    }
    // Controls cut by the window or by their region.
    let widgets = d.widgets();
    let ids: Vec<(Id, String, [f32; 4])> =
        widgets.iter().filter(|w| !w.id.starts_with("region:")).filter_map(|w| Some((w.egui_id?, w.id.clone(), w.rect))).collect();
    let rects: Vec<(String, Rect, Rect)> = d.ctx().viewport(|v| {
        ids.iter()
            .filter_map(|(id, name, shown)| {
                let w = v.this_pass.widgets.get(*id).or_else(|| v.prev_pass.widgets.get(*id))?;
                let shown = Rect::from_min_size(egui::pos2(shown[0], shown[1]), egui::vec2(shown[2], shown[3]));
                Some((name.clone(), w.rect, shown))
            })
            .collect()
    });
    for (name, full, shown) in rects {
        if !full.is_positive() || !shown.is_positive() {
            continue;
        }
        let cut_x = shown.width() + 0.5 < full.width();
        if full.right() > screen.right() + 0.5 || full.left() < screen.left() - 0.5 {
            problems.push(format!("{place}: {name} outside the window: {full:?}"));
        } else if cut_x {
            problems.push(format!("{place}: {name} cut at the side: {full:?} shown {shown:?}"));
        }
    }
    problems
}

#[test]
fn at_150_percent_with_the_large_font_nothing_is_clipped_in_the_window_the_edit_view_and_every_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = scaled(dir.path());
    let mut problems = Vec::new();
    d.run("pane.properties", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    problems.extend(clipped(&mut d, "library"));
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    problems.extend(clipped(&mut d, "edit view"));
    d.run("view.library", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let mut drawn = BTreeSet::new();
    for kind in DialogKind::ALL {
        d.app_mut().state_mut().dialogs.clear();
        if dalica_ui::dialogs::open(d.app_mut(), Dialog::new(kind)).is_err() {
            continue;
        }
        d.settle(FRAMES).unwrap();
        if d.state().dialogs.last().map(|x| x.kind) != Some(kind) {
            continue;
        }
        drawn.insert(kind.name());
        problems.extend(clipped(&mut d, &format!("{kind:?}")));
    }
    d.app_mut().state_mut().dialogs.clear();
    assert!(drawn.len() > 30, "{} dialogs drawn", drawn.len());
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} clipped at 150 % with the Large font:\n{}", problems.len(), problems.join("\n"));
}

/// The screens checked above, painted (no frame here runs outside the driver, so every texture
/// reaches the picture): `scaling-150-large-*.png` in the test output and `DALICA_SHOTS`.
#[test]
fn the_scaled_screens_are_painted_for_a_look() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = scaled(dir.path());
    d.run("pane.properties", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    shot(&d, "scaling-150-large-library-1400x900");
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    shot(&d, "scaling-150-large-edit-1400x900");
    d.run("view.library", &json!({})).unwrap();
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    shot(&d, "scaling-150-large-options-1400x900");
}
