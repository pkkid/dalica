//! The quick full-screen preview (`viewing.md` §2), headless: Ctrl+Alt held down
//! (`ui.key {state: "down"}`) over a thumbnail shows that photo on the very first frame, from the
//! light table's thumbnail, and the screen-size preview replaces it in place; the wheel steps three
//! photos without scrolling the light table, and the release focuses the third, scrolled into view,
//! letting go of the preview pane; over empty space nothing happens; in the edit view the open
//! photo shows; during A│B neither compare pane is touched. The fingerprint is taken at 1400 × 900;
//! at 900 × 600 the photo only has to fit the window.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_pixels, new_driver};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::Scene;
use dalica_ui::headless::{KeyState, PointerEvent, PointerKind};
use dalica_ui::panels::light_table::cell::TOOLTIP_SECONDS;
use dalica_ui::panels::peek::{self, PANE, Shown, WHEEL_STEP};
use dalica_ui::panels::thumbnails::Drawn;
use dalica_ui::state::{Compare, View};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2, vec2};
use serde_json::json;

const HOLD: &str = "Ctrl+Alt+Space";

/// A session on a library under `dir` holding one folder of six 900 × 600 scenes JPEGs.
fn session(dir: &Path) -> Session {
    let folder = dir.join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..6u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(60 + i, 900, 600), 85, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

fn setup((w, h): (u32, u32)) -> (tempfile::TempDir, Driver) {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    (dir, d)
}

/// The light table's cells in display order (top to bottom, left to right).
fn cells(d: &Driver) -> Vec<PhotoId> {
    let mut cells: Vec<_> = d.widgets().into_iter().filter(|w| w.id.starts_with("thumb:") && !w.id[6..].contains('/')).collect();
    cells.sort_by(|a, b| (a.rect[1], a.rect[0]).partial_cmp(&(b.rect[1], b.rect[0])).unwrap());
    cells.iter().map(|w| PhotoId::new(w.id[6..].parse().unwrap())).collect()
}

fn move_to(d: &mut Driver, x: f32, y: f32) {
    d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: pos2(x, y) }], PointerButton::Primary, Modifiers::NONE).unwrap();
}

fn offset(d: &Driver) -> f64 {
    d.app().panels().unwrap().light_table.frame_info().offset
}

/// The bounds of what is not black in the window: where the photo is drawn.
fn photo_bounds(d: &Driver) -> Option<[u32; 4]> {
    let s = d.screenshot().unwrap();
    let mut b: Option<[u32; 4]> = None;
    for y in 0..s.height {
        for x in 0..s.width {
            let [r, g, bl, _] = s.pixel(x, y).unwrap();
            if u32::from(r) + u32::from(g) + u32::from(bl) > 24 {
                let e = b.get_or_insert([x, y, x, y]);
                *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
            }
        }
    }
    b
}

#[test]
fn a_hold_over_a_thumbnail_shows_it_at_once_the_wheel_steps_and_the_release_focuses() {
    let (_dir, mut d) = setup(LARGE);
    let order = cells(&d);
    assert!(order.len() >= 4, "{order:?}");
    d.hover(&format!("thumb:{}", order[0].get()), 0).unwrap();
    let before = offset(&d);

    d.key_state(HOLD, KeyState::Down).unwrap();
    assert_eq!(d.app().state().peek, Some(order[0]));
    let st = peek::stats(d.ctx());
    assert_eq!(st.grey_frames, 0, "never a grey frame: {st:?}");
    assert!(
        matches!(st.first, Some(Shown::Thumbnail(Drawn::Image | Drawn::Upscaled)) | Some(Shown::Preview)),
        "the first frame draws the photo from the thumbnail cache: {st:?}"
    );
    d.settle(FRAMES).unwrap();
    assert_eq!(peek::stats(d.ctx()).last, Some(Shown::Preview), "refined by the screen-size preview");
    assert_eq!(d.app().session().unwrap().preview_photo_in(PANE), Some(order[0]));
    assert_eq!(peek::stats(d.ctx()).grey_frames, 0);
    // Held on past the cell's tooltip delay: the tooltip of the cell under the pointer is not
    // drawn over the photo (and the fingerprint does not depend on how long the settle took).
    d.wait((TOOLTIP_SECONDS * 1000.0) as u64 + 400).unwrap();
    assert_eq!(d.app().tooltip(), None, "no tooltip over the preview");
    let s = d.screenshot().unwrap();
    check_pixels(&format!("peek-{}x{}", LARGE.0, LARGE.1), s.width, s.height, &s.rgba);

    for _ in 0..3 {
        d.scroll(None, vec2(0.0, -WHEEL_STEP)).unwrap();
    }
    assert_eq!(d.app().state().peek, Some(order[3]), "three wheel steps, three photos on");
    assert_eq!(offset(&d), before, "the wheel steps the preview, not the light table");
    d.settle(FRAMES).unwrap();

    d.key_state(HOLD, KeyState::Up).unwrap();
    let state = d.app().state();
    assert_eq!(state.peek, None);
    assert_eq!(state.selection.ids, vec![order[3]], "the last photo shown is focused");
    assert_eq!(state.selection.anchor, Some(order[3]));
    assert_eq!(d.app().session().unwrap().preview_photo_in(PANE), None, "the preview's pane is let go");
    assert!(d.widgets().iter().any(|w| w.id == format!("thumb:{}", order[3].get())), "scrolled into view");
}

#[test]
fn the_photo_fits_the_small_window() {
    let (_dir, mut d) = setup(SMALL);
    let order = cells(&d);
    d.hover(&format!("thumb:{}", order[1].get()), 0).unwrap();
    d.key_state(HOLD, KeyState::Down).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(peek::stats(d.ctx()).last, Some(Shown::Preview));
    // A 3:2 photo fitted to 900 × 600 fills it exactly: nothing else is drawn.
    let [x0, y0, x1, y1] = photo_bounds(&d).expect("the photo is drawn");
    assert!(x0 <= 1 && y0 <= 1 && x1 + 2 >= SMALL.0 && y1 + 2 >= SMALL.1, "{:?}", [x0, y0, x1, y1]);
    d.key_state(HOLD, KeyState::Up).unwrap();
    assert_eq!(d.app().state().peek, None);
}

#[test]
fn a_hold_over_empty_space_does_nothing() {
    let (_dir, mut d) = setup(LARGE);
    let area = d.app().panels().unwrap().light_table.frame_info().area.unwrap();
    move_to(&mut d, area.right() - 40.0, area.bottom() - 40.0);
    let selection = d.app().state().selection.clone();
    d.key_state(HOLD, KeyState::Down).unwrap();
    assert_eq!(d.app().state().peek, None);
    assert_eq!(peek::stats(d.ctx()).frames, 0);
    d.key_state(HOLD, KeyState::Up).unwrap();
    assert_eq!(d.app().state().selection, selection);
}

/// The edit view on the second photo, the pointer on the photo.
fn edit_view() -> (tempfile::TempDir, Driver, Vec<PhotoId>) {
    let (dir, mut d) = setup(LARGE);
    let order = cells(&d);
    d.app_mut().run("view.edit", &json!({ "id": order[1] })).unwrap();
    d.settle(FRAMES).unwrap();
    move_to(&mut d, 600.0, 400.0);
    (dir, d, order)
}

#[test]
fn in_the_edit_view_it_shows_the_open_photo() {
    let (_dir, mut d, order) = edit_view();
    d.key_state(HOLD, KeyState::Down).unwrap();
    assert_eq!(d.app().state().peek, Some(order[1]));
    assert_eq!(peek::stats(d.ctx()).grey_frames, 0);
    d.settle(FRAMES).unwrap();
    assert_eq!(peek::stats(d.ctx()).last, Some(Shown::Preview));
    assert_eq!(d.app().session().unwrap().preview_photo_in(PANE), None, "the edit view's own pane is used");
    d.key_state(HOLD, KeyState::Up).unwrap();
    assert_eq!(d.app().state().peek, None);
    assert_eq!(d.app().state().view, View::Edit);
    assert_eq!(d.app().state().edit.photo, Some(order[1]));
}

#[test]
fn a_peek_during_a_b_leaves_both_compare_panes_as_they_were() {
    let (_dir, mut d, order) = edit_view();
    {
        let state = d.app_mut().state_mut();
        state.edit.compare = Compare::Two;
        state.edit.second = Some(order[4]);
    }
    d.app_mut().session_mut().unwrap().open_preview_in(PANE, order[4], (600, 400)).unwrap();
    d.settle(FRAMES).unwrap();
    let panes = |d: &Driver| {
        let s = d.app().session().unwrap();
        (s.preview_photo_in(0), s.preview_photo_in(PANE))
    };
    let was = panes(&d);
    assert_eq!(was, (Some(order[1]), Some(order[4])));

    d.key_state(HOLD, KeyState::Down).unwrap();
    assert_eq!(d.app().state().peek, Some(order[1]));
    d.scroll(None, vec2(0.0, -WHEEL_STEP)).unwrap();
    assert_eq!(d.app().state().peek, Some(order[2]), "a photo no pane shows");
    d.settle(FRAMES).unwrap();
    assert_eq!(panes(&d), was, "drawn from its thumbnail, no pane taken");
    assert_eq!(peek::stats(d.ctx()).grey_frames, 0);
    d.scroll(None, vec2(0.0, WHEEL_STEP)).unwrap();
    d.key_state(HOLD, KeyState::Up).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.app().state().peek, None);
    assert_eq!(panes(&d), was);
}
