//! The bottom bar at the Large font (M16.26, `ui-layout.md` §6.5 and §11): at 1400 × 900 and
//! 900 × 600, in the library and the edit view, no label of the bottom bar is ellipsized or cut at
//! the side of the area it is drawn in; the action buttons grow to fit their labels, and in the
//! narrow window they fold behind More…, whose menu rows are not cut either. `DALICA_SHOTS=<dir>`
//! writes the screens there.

use std::sync::Arc;

use crate::accessibility::frame;
use crate::common::{FRAMES, LARGE, SMALL, demo_open, new_driver, shot, try_widget, widget};
use dalica_ui::{App, Driver};
use egui::Rect;
use egui::epaint::{ClippedShape, Shape};
use serde_json::json;

/// The demo library at `w × h`, UI scale 100 % and the Large font, one photo selected.
fn large(dir: &std::path::Path, (w, h): (u32, u32)) -> Driver {
    let mut s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    s.execute("options.set", &json!({ "values": { "interface.uiScale": 100, "interface.fontSize": "large" } })).unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.settle(FRAMES).unwrap();
    d
}

/// Every text of `shapes`: its words, the rect it is drawn in, its clip and whether it was cut
/// short with an ellipsis.
fn texts(shapes: &[ClippedShape]) -> Vec<(String, Rect, Rect, bool)> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect, bool)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            Shape::Text(t) if !t.galley.text().trim().is_empty() => {
                out.push((t.galley.text().to_owned(), t.visual_bounding_rect(), clip, t.galley.elided));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, s.clip_rect, &mut out);
    }
    out
}

/// What is cut in the next frame of `d`, one line each: the texts drawn in `area` (and those
/// named in `also`, wherever they are) that are ellipsized or run past the side of their clip.
fn cut(d: &mut Driver, place: &str, area: Rect, also: &[&str]) -> (usize, Vec<String>) {
    let out = frame(d, Vec::new());
    let mut seen = 0;
    let mut problems = Vec::new();
    for (text, r, clip, elided) in texts(&out.shapes) {
        if !area.contains(r.center()) && !also.contains(&text.as_str()) {
            continue;
        }
        seen += 1;
        if elided {
            problems.push(format!("{place}: {text:?} ellipsized in {r:?}"));
        }
        if r.left() < clip.left() - 0.5 || r.right() > clip.right() + 0.5 {
            problems.push(format!("{place}: {text:?} cut at the side: {r:?} in {clip:?}"));
        }
    }
    (seen, problems)
}

/// The bottom area: everything under the status line.
fn bottom(d: &Driver) -> Rect {
    let status = widget(d, "region:statusLine").rect;
    let screen = d.ctx().content_rect();
    Rect::from_min_max(egui::pos2(screen.left(), status[1] + status[3]), screen.max)
}

/// The rect of widget `id`.
fn rect(d: &Driver, id: &str) -> Rect {
    let r = widget(d, id).rect;
    Rect::from_min_size(egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3]))
}

#[test]
fn at_the_large_font_no_bottom_bar_label_is_cut_at_either_window_size() {
    let mut problems = Vec::new();
    for (size, name) in [(LARGE, "1400x900"), (SMALL, "900x600")] {
        let dir = tempfile::tempdir().unwrap();
        let mut d = large(dir.path(), size);
        let area = bottom(&d);
        let (seen, p) = cut(&mut d, &format!("library {name}"), area, &[]);
        problems.extend(p);
        if size == LARGE {
            // Export, Collage, Slideshow and their icons' band: three labels at least.
            assert!(seen >= 3, "{seen} texts in the bottom bar at {name}");
            // Each label sits inside its own button, which keeps clear of the next one.
            let buttons: Vec<Rect> = ["action:export", "action:collage", "action:slideshow"].iter().map(|id| rect(&d, id)).collect();
            for pair in buttons.windows(2) {
                assert!(pair[0].right() <= pair[1].left(), "{name}: the action buttons overlap: {pair:?}");
            }
            assert!(buttons.iter().all(|b| area.contains_rect(*b)), "{name}: an action button leaves the window: {buttons:?}");
            shot(&d, "large-font-bottom-1400x900");
        } else {
            assert!(try_widget(&d, "action:slideshow").is_none(), "below 1100 the actions fold behind More…");
            assert!(seen >= 1, "More…'s label is drawn at {name}");
            d.click("action:more").unwrap();
            d.settle(FRAMES).unwrap();
            let (rows, p) = cut(&mut d, &format!("More… menu {name}"), Rect::NOTHING, &["Export", "Collage", "Slideshow"]);
            assert!(rows >= 3, "{rows} texts with More… open");
            problems.extend(p);
            shot(&d, "large-font-more-900x600");
            d.click("action:more").unwrap();
            d.settle(FRAMES).unwrap();
        }
        // The edit view: Fit │ 1:1 and the zoom slider take the loupe's and the size slider's place.
        d.run("view.edit", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        let (_, p) = cut(&mut d, &format!("edit view {name}"), area, &[]);
        problems.extend(p);
    }
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} bottom-bar labels cut at the Large font:\n{}", problems.len(), problems.join("\n"));
}
