//! The effect tabs' tile grids (`editing.md` §8.1–§8.4), headless and by id: tab 3 shows its 12
//! tiles in the spec's order with their labels, each `gray 38` until its preview arrives and then
//! the photo's preview (the plain photo for an effect whose renderer is still a stub, something
//! else for one that renders); Sepia's click is one "Sepia" step and Ctrl+Shift+B one "B&W" step;
//! Sharpen's click opens its panel in the grid's place, a modal tool draws the grid disabled, and
//! Cancel brings it back with nothing recorded; tabs 4 and 5 show 7 and 8 tiles; at 900 × 600
//! every grid fits the panel; a fingerprint of each tab's grid at 1400 × 900 (the frame the tab
//! opens, its previews still `gray 38`: what each preview shows is pinned by its effect's own
//! fingerprint).
//!
//! `DALICA_SHOTS=<dir>` writes the grids' screenshots there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_edit::catalog;
use dalica_engine::Session;
use dalica_image::Rgba8;
use dalica_ui::{App, Driver};
use serde_json::json;

/// The photo every test edits, and its size.
const NAME: &str = "Plain.jpg";
const SIZE: (u32, u32) = (480, 320);
const SEED: u64 = 77;

/// The `placeholder` token: `gray 38`.
const GREY: [u8; 4] = [38, 38, 38, 255];

/// The preview's place in a tile (`metrics.effect_preview` 3 pt from the top, centred) and size.
const PREVIEW_TOP: f32 = 3.0;
const PREVIEW: (f32, f32) = (78.0, 48.0);

const TAB3: [(&str, &str); 12] = [
    ("sharpen", "Sharpen"),
    ("sepia", "Sepia"),
    ("bw", "B&W"),
    ("warmify", "Warmify"),
    ("grain", "Film Grain"),
    ("tint", "Tint"),
    ("saturation", "Saturation"),
    ("soft_focus", "Soft Focus"),
    ("glow", "Glow"),
    ("filtered_bw", "Filtered B&W"),
    ("focal_bw", "Focal B&W"),
    ("graduated_tint", "Graduated Tint"),
];
const TAB4: [&str; 7] = ["infrared", "plastic_lens", "hdr", "widescreen", "dream_glow", "invert", "duotone"];
const TAB5: [&str; 8] = ["boost", "soften", "vignette", "pixelate", "focal_zoom", "border", "drop_shadow", "museum_matte"];

struct T {
    dir: tempfile::TempDir,
    d: Driver,
    photo: PhotoId,
}

/// A library holding one 480 × 320 JPEG, open in the edit view, the window at `size`.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(SEED, SIZE.0, SIZE.1), 85, None).unwrap();
    std::fs::write(folder.join(NAME), bytes).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let photo = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": photo })).unwrap();
    d.settle(FRAMES).unwrap();
    T { dir, d, photo }
}

/// The ids of the effect tiles of the last frame, in the order drawn.
fn tiles(t: &T) -> Vec<String> {
    t.d.widgets().into_iter().filter(|w| w.id.starts_with("effect:")).map(|w| w.id).collect()
}

/// Shows tab `tab` and runs the one frame that asks for its tiles.
fn show_tab(t: &mut T, tab: &str) {
    t.d.run("edit.tab", &json!({ "tab": tab })).unwrap();
}

/// Steps until every tile asked for has arrived, then two frames more to take and draw them.
fn wait_tiles(t: &mut T) {
    t.d.step_until(
        |app| {
            std::thread::sleep(Duration::from_millis(1));
            app.session().is_some_and(|s| s.effect_tiles_pending() == 0)
        },
        FRAMES,
    )
    .unwrap();
    t.d.step();
    t.d.step();
}

/// The preview of tile `id` in the last frame: `(width, height, rgba)`, the badge's corner left
/// out (the left 56 points of the preview).
fn preview(t: &T, id: &str) -> (u32, u32, Vec<u8>) {
    let r = widget(&t.d, id).rect;
    let x = r[0] + (r[2] - PREVIEW.0) / 2.0;
    let s = t.d.screenshot().unwrap();
    crate::common::rect_pixels(&s, [x, r[1] + PREVIEW_TOP, PREVIEW.0 - 22.0, PREVIEW.1])
}

/// The photo as a tile shows it without an effect: decoded small, cut to the preview's shape
/// around its centre and brought to its size (as the engine makes a tile).
fn plain_tile(t: &T) -> Rgba8 {
    let bytes = std::fs::read(t.dir.path().join("photos").join(NAME)).unwrap();
    let limits = dalica_image::Limits::default();
    let (w, h) = (PREVIEW.0 as u32, PREVIEW.1 as u32);
    // The longer side covering the box twice over (`effect_tiles::COVER`).
    let edge = (2 * w * SIZE.0).div_ceil(SIZE.1);
    let decode = dalica_image::decode_thumbnail_with(&bytes, edge, &dalica_image::DecodeOptions::default(), &limits).unwrap();
    let scale = (f64::from(w) / f64::from(decode.width)).max(f64::from(h) / f64::from(decode.height));
    let recipe = dalica_edit::Recipe::parse(std::iter::empty::<&str>()).unwrap();
    let image = dalica_edit::render(dalica_edit::Source::upright(&decode), &recipe, dalica_edit::Target::Scale(scale)).unwrap();
    let (iw, ih) = (image.width, image.height);
    let (cw, ch) = if iw * h > ih * w { ((ih * w).div_ceil(h), ih) } else { (iw, (iw * h).div_ceil(w)) };
    let (x0, y0) = ((iw - cw) / 2, (ih - ch) / 2);
    let mut data = Vec::new();
    for y in y0..y0 + ch {
        let start = ((y * iw + x0) * 4) as usize;
        data.extend_from_slice(&image.data[start..start + (cw * 4) as usize]);
    }
    let cut = Rgba8 { width: cw, height: ch, data };
    if (cw, ch) == (w, h) { cut } else { dalica_image::resize::resize(&cut, w, h).unwrap() }
}

/// The largest difference of one channel between `shown` (the left part of a preview, see
/// [`preview`]) and the same part of `plain`.
fn largest_difference(shown: &(u32, u32, Vec<u8>), plain: &Rgba8) -> u8 {
    let (w, h, rgba) = shown;
    let mut most = 0;
    for y in 0..*h {
        for x in 0..*w {
            let a = &rgba[((y * w + x) * 4) as usize..][..3];
            let b = &plain.data[((y * plain.width + x) * 4) as usize..][..3];
            most = a.iter().zip(b).map(|(p, q)| p.abs_diff(*q)).fold(most, u8::max);
        }
    }
    most
}

/// What the Undo button reads.
fn undo_label(t: &T) -> String {
    widget(&t.d, "button:edit.undo").label
}

fn edits(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().library().info(t.photo).edits.clone()
}

/// Tab 3's twelve tiles in the spec's order, three to a row, with their labels; each `gray 38` in
/// the frame the tab opens; once the previews are in, each shows the photo, plain for an effect
/// still rendered by its stub and changed for one that renders.
#[test]
fn tab_three_shows_its_twelve_tiles_grey_until_their_previews_arrive() {
    let mut t = setup(LARGE);
    show_tab(&mut t, "effects");
    let ids: Vec<String> = TAB3.iter().map(|(name, _)| format!("effect:{name}")).collect();
    assert_eq!(tiles(&t), ids);
    for (i, (name, label)) in TAB3.iter().enumerate() {
        let w = widget(&t.d, &format!("effect:{name}"));
        assert_eq!(w.label, *label);
        assert!(w.enabled, "{name}");
        let first = widget(&t.d, &ids[i - i % 3]).rect;
        assert_eq!(w.rect[1], first[1], "{name} on its row");
        if i % 3 > 0 {
            assert!(w.rect[0] > widget(&t.d, &ids[i - 1]).rect[0], "{name} right of the one before");
        }
        let (_, _, rgba) = preview(&t, &ids[i]);
        assert!(rgba.chunks(4).all(|p| p == GREY), "{name} is gray 38 until its preview arrives");
    }
    wait_tiles(&mut t);
    let plain = plain_tile(&t);
    for (name, _) in TAB3 {
        let shown = preview(&t, &format!("effect:{name}"));
        assert!(!shown.2.chunks(4).all(|p| p == GREY), "{name}'s preview arrived");
        let most = largest_difference(&shown, &plain);
        if catalog::lookup(name).unwrap().renders() {
            assert!(most > 2, "{name} renders and its tile differs from the plain photo (by {most})");
        } else {
            assert!(most <= 2, "{name} is a stub and its tile is the plain photo (off by {most})");
        }
    }
}

/// Sepia's tile is one "Sepia" step of the photo's history; Ctrl+Shift+B in the edit view one
/// "B&W" step.
#[test]
fn sepia_is_one_click_and_ctrl_shift_b_is_bw() {
    let mut t = setup(LARGE);
    show_tab(&mut t, "effects");
    t.d.click("effect:sepia").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&t), ["sepia()"]);
    assert_eq!(undo_label(&t), "Undo Sepia");
    t.d.key("Ctrl+Shift+B").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&t), ["sepia()", "bw()"]);
    assert_eq!(undo_label(&t), "Undo B&W");
}

/// Sharpen's tile opens its panel in the grid's place (no tile takes a click); a modal tool draws
/// the grid disabled; Cancel brings the grid back, enabled, with nothing recorded.
#[test]
fn a_parameter_tile_opens_its_panel_and_the_grid_waits_for_cancel() {
    let mut t = setup(LARGE);
    show_tab(&mut t, "effects");
    t.d.click("effect:sharpen").unwrap();
    t.d.settle(FRAMES).unwrap();
    let edit = &t.d.state().edit;
    assert_eq!(edit.effect.as_ref().map(|e| e.name.as_str()), Some("sharpen"));
    assert!(tiles(&t).is_empty(), "the panel replaces the grid");
    show_tab(&mut t, "effects2");
    t.d.settle(FRAMES).unwrap();
    assert!(tiles(&t).is_empty(), "the panel stays on another effect tab");
    t.d.click("button:panel.cancel").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().edit.tool.is_none());
    assert_eq!(tiles(&t).len(), TAB4.len());
    assert!(tiles(&t).iter().all(|id| widget(&t.d, id).enabled), "enabled after Cancel");
    assert!(edits(&t).is_empty(), "Cancel recorded nothing");
    // A modal tool that leaves the tab content in place (Straighten): the grid is drawn, disabled.
    t.d.run("edit.tool.open", &json!({ "tool": "straighten" })).unwrap();
    show_tab(&mut t, "effects");
    t.d.settle(FRAMES).unwrap();
    assert_eq!(tiles(&t).len(), TAB3.len());
    assert!(tiles(&t).iter().all(|id| !widget(&t.d, id).enabled), "disabled while Straighten is open");
    assert!(t.d.click("effect:sepia").is_err(), "a disabled tile takes no click");
    assert!(edits(&t).is_empty());
}

/// Tabs 4 and 5 show 7 and 8 tiles in their order; at 900 × 600 each grid lies inside the edit
/// panel, above Undo / Redo, and every widget inside the window.
#[test]
fn tabs_four_and_five_show_their_tiles_and_every_grid_fits_the_small_window() {
    let mut t = setup(SMALL);
    for (tab, names) in [("effects", &TAB3.map(|(n, _)| n)[..]), ("effects2", &TAB4[..]), ("effects3", &TAB5[..])] {
        show_tab(&mut t, tab);
        t.d.settle(FRAMES).unwrap();
        let want: Vec<String> = names.iter().map(|n| format!("effect:{n}")).collect();
        assert_eq!(tiles(&t), want, "{tab}");
        let panel = widget(&t.d, "region:editPanel").rect;
        let undo = widget(&t.d, "button:edit.undo").rect;
        for id in &want {
            let r = try_widget(&t.d, id).unwrap().rect;
            assert!(r[0] >= panel[0] && r[0] + r[2] <= panel[0] + panel[2] + 0.01, "{id} at {r:?} across the panel {panel:?}");
            assert!(r[1] >= panel[1] && r[1] + r[3] <= undo[1] + 0.01, "{id} at {r:?} above Undo at {undo:?}");
            assert!((r[2], r[3]) == (86.0, 69.0), "{id} whole: {r:?}");
        }
        check_fits(&t.d, tab);
        shot(&t.d, &format!("effect-tiles-{tab}-{}x{}", SMALL.0, SMALL.1));
    }
}

/// Each tab's grid at 1400 × 900 in the frame the tab opens.
#[test]
fn each_tabs_grid_matches_its_fingerprint() {
    let mut t = setup(LARGE);
    let tabs = [
        ("effects", 3, "effect:sharpen", "effect:graduated_tint"),
        ("effects2", 4, "effect:infrared", "effect:duotone"),
        ("effects3", 5, "effect:boost", "effect:museum_matte"),
    ];
    for (tab, n, first, last) in tabs {
        show_tab(&mut t, "basic");
        t.d.settle(FRAMES).unwrap();
        show_tab(&mut t, tab);
        let (a, b) = (widget(&t.d, first).rect, widget(&t.d, last).rect);
        // The whole grid: three columns from the first tile's left, down to the last tile's foot.
        let width = 3.0 * a[2] + 2.0 * dalica_ui_kit::widgets::EFFECT_TILE_GAP;
        check_rect(&t.d, &format!("edit-effects-tab{n}-{}x{}", LARGE.0, LARGE.1), [a[0], a[1], width, b[1] + b[3] - a[1]]);
        wait_tiles(&mut t);
        shot(&t.d, &format!("effect-tiles-tab{n}-window"));
    }
}
