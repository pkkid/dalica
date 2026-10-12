//! The rubber band at the light table's edges (M5.23, `ui-layout.md` §5.4), driven headlessly: a
//! band held below the view scrolls it, faster the farther past the edge, and selects the rows that
//! come in, never crossing into the next section (the view stops at its section's end); held above
//! the view it scrolls back up to the section's top; a drag that is not a band never scrolls.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::state::Section;
use dalica_ui::{App, Driver};
use egui::{PointerButton, Pos2, Rect, pos2};
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// A session on a library under `dir`: `folders` folders sharing `photos` scenes photos.
fn session(dir: &Path, folders: usize, photos: usize) -> Session {
    let files = dalica_scenes::demo_library(folders, photos, 5).unwrap();
    let mut roots = BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    s
}

/// A 900 × 600 window with 128-point thumbnails: a few rows on screen, each section taller than the
/// view.
fn driver(dir: &Path) -> Driver {
    let mut d = Driver::new(App::new(Some(session(dir, 2, 80))), 900, 600, 1.0);
    d.app_mut().state_mut().thumb_size = 128.0;
    d.settle(FRAMES).unwrap();
    d
}

fn sections(d: &Driver) -> Vec<(Section, Vec<PhotoId>)> {
    d.app().panels().unwrap().sections.list.iter().map(|s| (s.id, s.photos.clone())).collect()
}

fn rect_of(d: &Driver, id: &str) -> Rect {
    let w = d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no widget {id}"));
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]))
}

fn thumb(id: PhotoId) -> String {
    format!("thumb:{}", id.get())
}

/// The grid's area on screen.
fn area(d: &Driver) -> Rect {
    d.app().panels().unwrap().light_table.frame_info().area.unwrap()
}

fn offset(d: &Driver) -> f64 {
    d.app().panels().unwrap().light_table.offset()
}

fn selected(d: &Driver) -> HashSet<PhotoId> {
    d.state().selection.ids.iter().copied().collect()
}

/// Presses at `from`, drags to `to` in 10 frames and holds there for `hold` frames; the button
/// stays down.
fn press_drag_hold(d: &mut Driver, from: Pos2, to: Pos2, hold: usize) {
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: from }, PointerEvent { kind: PointerKind::Down, pos: from }];
    for i in 1..=10 {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: from.lerp(to, i as f32 / 10.0) });
    }
    events.extend(std::iter::repeat_n(PointerEvent { kind: PointerKind::Drag, pos: to }, hold));
    d.pointer(&events, PointerButton::Primary, egui::Modifiers::NONE).unwrap();
}

/// Holds the button at `at` for `frames` more frames.
fn hold(d: &mut Driver, at: Pos2, frames: usize) {
    let events = vec![PointerEvent { kind: PointerKind::Drag, pos: at }; frames];
    d.pointer(&events, PointerButton::Primary, egui::Modifiers::NONE).unwrap();
}

fn release(d: &mut Driver, at: Pos2) {
    d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: at }], PointerButton::Primary, egui::Modifiers::NONE).unwrap();
}

/// The done-when: a band held below the view scrolls it and selects the rows that come in, and
/// never crosses into the next section; held above, it scrolls back to the section's top.
#[test]
fn a_band_held_past_an_edge_scrolls_and_keeps_selecting_inside_its_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path());
    let s = sections(&d);
    let ((a, pa), (_, pb)) = (s[0].clone(), s[1].clone());
    let lt = d.app().panels().unwrap().light_table.layout().unwrap();
    let (cols, rows) = (lt.grid().cols, lt.rows(pa.len()));
    let a_end = lt.top(0).unwrap() + lt.section_height(pa.len());
    let view = area(&d);
    assert!(a_end > f64::from(view.height()) * 2.0, "section A ({} photos, {rows} rows) is taller than two views", pa.len());
    assert_eq!(offset(&d), 0.0);

    // From the left margin of A's first row to a little below the view.
    let c0 = rect_of(&d, &thumb(pa[0]));
    let from = pos2(c0.left() - 6.0, c0.center().y);
    let right = rect_of(&d, &thumb(pa[cols - 1])).right() + 4.0;
    let below = pos2(right, view.bottom() + 12.0);
    press_drag_hold(&mut d, from, below, 0);
    let (o1, sel1) = (offset(&d), selected(&d));
    assert!(o1 > 0.0, "the view started scrolling");
    hold(&mut d, below, 30);
    let (o2, sel2) = (offset(&d), selected(&d));
    assert!(o2 > o1, "it keeps scrolling while held: {o1} → {o2}");
    assert!(sel2.len() > sel1.len() && sel1.is_subset(&sel2), "the rows that come in are selected: {} → {}", sel1.len(), sel2.len());
    assert_eq!(d.state().selection.section, Some(a));

    // Farther past the edge is faster.
    let slow = o2 - o1;
    let far = pos2(right, view.bottom() + 80.0);
    hold(&mut d, far, 1);
    let o3 = offset(&d);
    hold(&mut d, far, 30);
    let fast = offset(&d) - o3;
    assert!(fast > slow * 1.5, "30 frames 12 pt past the edge moved {slow}, 80 pt past {fast}");

    // Held long enough, the band reaches the end of A, selects all of it and stops there.
    hold(&mut d, far, 240);
    let o4 = offset(&d);
    assert!(o4 <= a_end - f64::from(view.height()) + 1.0, "the view stops at A's end ({o4} vs {a_end})");
    hold(&mut d, far, 30);
    assert_eq!(offset(&d), o4, "and stays there");
    assert_eq!(selected(&d), pa.iter().copied().collect::<HashSet<_>>(), "all of A");
    assert!(pb.iter().all(|id| !selected(&d).contains(id)), "nothing of B");
    assert_eq!(d.state().selection.section, Some(a));
    release(&mut d, far);
    assert_eq!(selected(&d).len(), pa.len(), "the selection stays after the release");
    let anchor = d.state().scroll;
    assert_eq!(anchor.section, Some(a), "the anchor follows the view");
    hold_still(&mut d);
    assert_eq!(offset(&d), o4, "released: the view rests");

    // Back up: from the left margin of a row on screen to above the view, to A's top.
    let last = rect_of(&d, &thumb(*pa.last().unwrap()));
    let from = pos2(c0.left() - 6.0, last.center().y);
    let above = pos2(right, view.top() - 60.0);
    press_drag_hold(&mut d, from, above, 300);
    assert_eq!(offset(&d), 0.0, "held above the view, it scrolls back to A's top");
    assert_eq!(selected(&d), pa.iter().copied().collect::<HashSet<_>>());
    release(&mut d, above);
}

/// Lets a few frames pass with the pointer up.
fn hold_still(d: &mut Driver) {
    for _ in 0..10 {
        d.step();
    }
}

/// A band in the second section held above the view never scrolls into the first, and one held
/// inside the view does not scroll at all; a drag that starts on a thumbnail is no band and never
/// scrolls.
#[test]
fn only_a_band_scrolls_and_never_out_of_its_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path());
    let s = sections(&d);
    let ((_, pa), (b, pb)) = (s[0].clone(), s[1].clone());
    let view = area(&d);
    let b_top = d.app().panels().unwrap().light_table.layout().unwrap().top(1).unwrap();

    // A drag from a thumbnail held below the view: nothing scrolls.
    let c3 = rect_of(&d, &thumb(pa[3]));
    press_drag_hold(&mut d, c3.center(), pos2(c3.center().x, view.bottom() + 40.0), 30);
    release(&mut d, pos2(c3.center().x, view.bottom() + 40.0));
    assert_eq!(offset(&d), 0.0, "a drag from a thumbnail is not a band");

    // Show B's first rows, then band from its left margin and hold inside the view: no scroll.
    d.run("section.scrollTo", &json!({ "section": b })).unwrap();
    d.settle(FRAMES).unwrap();
    let o = offset(&d);
    assert!((o - b_top).abs() < 1.0, "B is at the top ({o} vs {b_top})");
    let c0 = rect_of(&d, &thumb(pb[0]));
    let from = pos2(c0.left() - 6.0, c0.center().y + 40.0);
    let inside = pos2(c0.right() + 4.0, view.center().y);
    press_drag_hold(&mut d, from, inside, 30);
    assert_eq!(offset(&d), o, "inside the view the band does not scroll");

    // Above the view: B's top is already in view, so the band never scrolls into A.
    let above = pos2(inside.x, view.top() - 60.0);
    hold(&mut d, above, 60);
    assert_eq!(offset(&d), o, "B's top is the band's limit");
    assert_eq!(d.state().selection.section, Some(b));
    assert!(pa.iter().all(|id| !selected(&d).contains(id)), "nothing of A");
    release(&mut d, above);
}
