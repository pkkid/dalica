//! The face box (`organizing.md` §6.4): a face's rectangle drawn over the photo in the edit view,
//! with the person's name under it, highlighted while the pointer is on it or on the face in the
//! People pane; and, for Add a Person Manually, a box to fit to the face by its eight handles or
//! moved whole.
//!
//! It works in its own normalised coordinates, as the crosshair does: (0, 0) is the top-left of the
//! area it is given (the photo as shown), (1, 1) its bottom-right, so the caller maps the box to
//! the stored frame and nothing here depends on the zoom. An edited box never leaves the area and
//! never gets smaller than [`FACE_BOX_MIN`] points a side.

use egui::{Align2, CornerRadius, CursorIcon, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The side of a handle (points).
pub const FACE_BOX_HANDLE: f32 = 7.0;
/// The side of the square round a handle that takes a drag (points).
pub const FACE_BOX_GRAB: f32 = 14.0;
/// The smallest side an edited box gets (points).
pub const FACE_BOX_MIN: f32 = 16.0;
/// The space between the box and its label (points).
const LABEL_GAP: f32 = 3.0;
/// The label plate's padding (points).
const LABEL_PAD: Vec2 = vec2(6.0, 2.0);

/// One of the eight handles of a box in edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl BoxHandle {
    /// Every handle, clockwise from the top-left.
    pub const ALL: [BoxHandle; 8] = [
        BoxHandle::TopLeft,
        BoxHandle::Top,
        BoxHandle::TopRight,
        BoxHandle::Right,
        BoxHandle::BottomRight,
        BoxHandle::Bottom,
        BoxHandle::BottomLeft,
        BoxHandle::Left,
    ];

    /// The handle's name in its automation id (`<id>/nw`).
    pub fn name(self) -> &'static str {
        match self {
            BoxHandle::TopLeft => "nw",
            BoxHandle::Top => "n",
            BoxHandle::TopRight => "ne",
            BoxHandle::Right => "e",
            BoxHandle::BottomRight => "se",
            BoxHandle::Bottom => "s",
            BoxHandle::BottomLeft => "sw",
            BoxHandle::Left => "w",
        }
    }

    /// Which edges the handle moves: (left, top, right, bottom).
    fn edges(self) -> (bool, bool, bool, bool) {
        match self {
            BoxHandle::TopLeft => (true, true, false, false),
            BoxHandle::Top => (false, true, false, false),
            BoxHandle::TopRight => (false, true, true, false),
            BoxHandle::Right => (false, false, true, false),
            BoxHandle::BottomRight => (false, false, true, true),
            BoxHandle::Bottom => (false, false, false, true),
            BoxHandle::BottomLeft => (true, false, false, true),
            BoxHandle::Left => (true, false, false, false),
        }
    }

    /// Where the handle sits on `r`.
    fn on(self, r: Rect) -> Pos2 {
        let (x0, y0, x1, y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
        let (cx, cy) = (r.center().x, r.center().y);
        match self {
            BoxHandle::TopLeft => pos2(x0, y0),
            BoxHandle::Top => pos2(cx, y0),
            BoxHandle::TopRight => pos2(x1, y0),
            BoxHandle::Right => pos2(x1, cy),
            BoxHandle::BottomRight => pos2(x1, y1),
            BoxHandle::Bottom => pos2(cx, y1),
            BoxHandle::BottomLeft => pos2(x0, y1),
            BoxHandle::Left => pos2(x0, cy),
        }
    }

    fn cursor(self) -> CursorIcon {
        match self {
            BoxHandle::TopLeft => CursorIcon::ResizeNorthWest,
            BoxHandle::Top => CursorIcon::ResizeNorth,
            BoxHandle::TopRight => CursorIcon::ResizeNorthEast,
            BoxHandle::Right => CursorIcon::ResizeEast,
            BoxHandle::BottomRight => CursorIcon::ResizeSouthEast,
            BoxHandle::Bottom => CursorIcon::ResizeSouth,
            BoxHandle::BottomLeft => CursorIcon::ResizeSouthWest,
            BoxHandle::Left => CursorIcon::ResizeWest,
        }
    }
}

/// A face box: where it is and how it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox<'a> {
    /// The rectangle in the box's own normalised coordinates ((0, 0) the area's top-left, (1, 1)
    /// its bottom-right).
    pub rect: Rect,
    /// The name shown under it.
    pub label: Option<&'a str>,
    /// Highlighted from outside (the pointer on this face in the People pane).
    pub highlighted: bool,
    /// Edit mode: eight handles, and the box moves and resizes.
    pub edit: bool,
}

/// What a [`face_box`] did this pass.
#[derive(Debug, Clone)]
pub struct FaceBoxOut {
    /// The box's new rectangle (normalised), when a drag moved or resized it.
    pub rect: Option<Rect>,
    /// A drag began: open an undo step (one per drag).
    pub drag_started: bool,
    /// It ended.
    pub drag_stopped: bool,
    /// The pointer is on the box (its edges, inside or its handles).
    pub hovered: bool,
    /// The box's own response (clicks on it, its drag to move it).
    pub response: Response,
    /// Where the box is drawn (screen points).
    pub screen: Rect,
}

/// The automation id of `handle` of box `id`: `<id>/<nw|n|ne|e|se|s|sw|w>`.
pub fn face_box_handle_id(id: &str, handle: BoxHandle) -> String {
    format!("{id}/{}", handle.name())
}

/// `v` within `lo` … `hi` (`lo` when the range is empty or `v` is no number).
fn within(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_nan() || hi < lo { lo } else { v.max(lo).min(hi) }
}

/// A normalised rectangle made whole: its corners ordered, each a number (NaN is the middle) and
/// on the area.
pub fn face_box_rect(r: Rect) -> Rect {
    let n = |v: f32| if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) };
    Rect::from_two_pos(pos2(n(r.min.x), n(r.min.y)), pos2(n(r.max.x), n(r.max.y)))
}

/// The box `start` (normalised) dragged by `delta` (normalised): moved whole when `handle` is
/// `None`, else the handle's edges moved. The result stays on the area (0 … 1) and keeps at least
/// `min` (normalised) a side, or its own size when it started smaller; a move keeps its size. Pure,
/// shared with the tests.
pub fn dragged_box(start: Rect, handle: Option<BoxHandle>, delta: Vec2, min: Vec2) -> Rect {
    let s = face_box_rect(start);
    let d = vec2(if delta.x.is_finite() { delta.x } else { 0.0 }, if delta.y.is_finite() { delta.y } else { 0.0 });
    // The minimum, but never more than the box already is (a drag never grows a box to it).
    let m = |v: f32, size: f32| if v.is_finite() { v.clamp(0.0, 1.0).min(size) } else { 0.0 };
    let (mw, mh) = (m(min.x, s.width()), m(min.y, s.height()));
    let Some(handle) = handle else {
        let x = within(s.min.x + d.x, 0.0, 1.0 - s.width());
        let y = within(s.min.y + d.y, 0.0, 1.0 - s.height());
        return Rect::from_min_size(pos2(x, y), s.size());
    };
    let (left, top, right, bottom) = handle.edges();
    let (mut x0, mut y0, mut x1, mut y1) = (s.min.x, s.min.y, s.max.x, s.max.y);
    if left {
        x0 = within(s.min.x + d.x, 0.0, x1 - mw);
    }
    if right {
        x1 = within(s.max.x + d.x, x0 + mw, 1.0);
    }
    if top {
        y0 = within(s.min.y + d.y, 0.0, y1 - mh);
    }
    if bottom {
        y1 = within(s.max.y + d.y, y0 + mh, 1.0);
    }
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

/// Where a drag began: the box then and the pointer's press.
#[derive(Clone, Copy)]
struct Grab {
    start: Rect,
    origin: Pos2,
}

/// Follows a drag taken by `r` (`handle` `None` for the body): the box as it was when the drag
/// began plus the pointer's way since, through [`dragged_box`].
fn follow(ui: &Ui, r: &Response, current: Rect, handle: Option<BoxHandle>, area: Rect, out: &mut FaceBoxOut) {
    let key = r.id.with("grab");
    if r.drag_started() {
        out.drag_started = true;
        // From where the button went down, so the way before egui saw a drag is not lost.
        if let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(r.interact_pointer_pos()) {
            ui.data_mut(|d| d.insert_temp(key, Grab { start: current, origin }));
        }
    }
    if r.dragged() || r.drag_started() {
        let grab = ui.data(|d| d.get_temp::<Grab>(key));
        if let (Some(g), Some(now)) = (grab, r.interact_pointer_pos()) {
            let size = vec2(area.width().max(1.0), area.height().max(1.0));
            let delta = (now - g.origin) / size;
            let min = Vec2::splat(FACE_BOX_MIN) / size;
            let moved = dragged_box(g.start, handle, delta, min);
            if moved != current {
                out.rect = Some(moved);
            }
        }
    }
    if r.drag_stopped() {
        out.drag_stopped = true;
        ui.data_mut(|d| d.remove::<Grab>(key));
    }
}

/// A face box over `area` (the photo as shown): the `face_box` rectangle (1.5 pt `face_box` over
/// a 1 pt `face_box_outer` edge outside it), `accent` while the pointer is on it, it is dragged or
/// it is `highlighted`; its label under it (12 pt `overlay_text` on a `caption_plate`, radius 3),
/// above it when there is no room below, inside its bottom when neither fits, kept across the
/// area. In edit mode eight [`FACE_BOX_HANDLE`] handles (`face_box` squares on a `face_box_outer`
/// edge, `accent` under the pointer) resize it and a drag inside moves it, all kept inside the
/// area; [`FaceBoxOut::rect`] reports the new rectangle. Registers `id` (role `other`, active while
/// highlighted, hovered or dragged, labelled with the name or "Face") and, in edit mode, each
/// handle as [`face_box_handle_id`].
pub fn face_box(ui: &mut Ui, t: &Tokens, id: &str, area: Rect, b: &FaceBox<'_>) -> FaceBoxOut {
    let enabled = ui.is_enabled();
    let norm = face_box_rect(b.rect);
    let to_screen = |r: Rect| {
        Rect::from_min_max(
            pos2(area.min.x + r.min.x * area.width(), area.min.y + r.min.y * area.height()),
            pos2(area.min.x + r.max.x * area.width(), area.min.y + r.max.y * area.height()),
        )
    };
    let screen = to_screen(norm);
    let body_sense = match (enabled, b.edit) {
        (false, _) => Sense::hover(),
        (true, false) => Sense::click(),
        (true, true) => Sense::click_and_drag(),
    };
    let body = ui.interact(screen, Id::new(id), body_sense);
    let edit = b.edit && enabled;
    let handles: Vec<(BoxHandle, String, Response)> = if b.edit {
        BoxHandle::ALL
            .into_iter()
            .map(|h| {
                let hid = face_box_handle_id(id, h);
                let grab = Rect::from_center_size(h.on(screen), Vec2::splat(FACE_BOX_GRAB));
                let r = ui.interact(grab, Id::new(&hid), if edit { Sense::drag() } else { Sense::hover() });
                (h, hid, r)
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut out = FaceBoxOut { rect: None, drag_started: false, drag_stopped: false, hovered: false, response: body.clone(), screen };
    if edit {
        for (h, _, r) in &handles {
            follow(ui, r, norm, Some(*h), area, &mut out);
        }
        if out.rect.is_none() {
            follow(ui, &body, norm, None, area, &mut out);
        }
    }
    let hot = |r: &Response| enabled && (r.hovered() || r.dragged());
    out.hovered = enabled && (body.contains_pointer() || handles.iter().any(|(_, _, r)| r.contains_pointer()));
    let dragging = body.dragged() || handles.iter().any(|(_, _, r)| r.dragged());
    let lit = b.highlighted || out.hovered || dragging;
    let shown = out.rect.map_or(screen, to_screen);
    out.screen = shown;

    let p = ui.painter_at(area.expand(FACE_BOX_GRAB));
    let light = if lit { t.accent } else { t.face_box };
    p.rect_stroke(shown.expand(1.25), 0, Stroke::new(1.0, t.face_box_outer), StrokeKind::Middle);
    p.rect_stroke(shown, 0, Stroke::new(1.5, light), StrokeKind::Middle);
    if let Some(label) = b.label.filter(|l| !l.is_empty()) {
        let galley = one_line(&p, label, fonts::regular(t.count), t.overlay_text, area.width().max(1.0) - 2.0 * LABEL_PAD.x);
        let size = galley.size() + 2.0 * LABEL_PAD;
        let below = shown.max.y + LABEL_GAP;
        let above = shown.min.y - LABEL_GAP - size.y;
        let y = if below + size.y <= area.max.y {
            below
        } else if above >= area.min.y {
            above
        } else {
            shown.max.y - size.y - LABEL_GAP
        };
        let x = within(shown.center().x - size.x / 2.0, area.min.x, area.max.x - size.x);
        let plate = Rect::from_min_size(pos2(x, y).round(), size);
        p.rect_filled(plate, CornerRadius::same(3), t.caption_plate);
        p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), plate).min.round(), galley, t.overlay_text);
    }
    for (h, _, r) in &handles {
        let c = h.on(shown);
        let square = Rect::from_center_size(c, Vec2::splat(FACE_BOX_HANDLE));
        p.rect_filled(square.expand(1.0), 0, t.face_box_outer);
        p.rect_filled(square, 0, if hot(r) { t.accent } else { t.face_box });
        if hot(r) {
            ui.ctx().set_cursor_icon(h.cursor());
        }
    }
    if edit && hot(&body) && !handles.iter().any(|(_, _, r)| hot(r)) {
        ui.ctx().set_cursor_icon(CursorIcon::Move);
    }

    let label = b.label.filter(|l| !l.is_empty()).unwrap_or("Face");
    register(ui.ctx(), id, Role::Other, label, Target::Widget(&body), lit);
    body.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Other, enabled, label));
    for (h, hid, r) in &handles {
        register(ui.ctx(), hid, Role::Other, h.name(), Target::Widget(r), r.dragged());
        r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Other, edit, h.name()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Vec2 = vec2(0.05, 0.05);

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    fn close(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 1e-5 && (a.max - b.max).length() < 1e-5
    }

    #[test]
    fn a_move_keeps_the_size_and_stays_on_the_area() {
        let start = r(0.2, 0.2, 0.4, 0.5);
        assert!(close(dragged_box(start, None, vec2(0.1, 0.1), MIN), r(0.3, 0.3, 0.5, 0.6)));
        assert!(close(dragged_box(start, None, vec2(5.0, -5.0), MIN), r(0.8, 0.0, 1.0, 0.3)));
        assert!(close(dragged_box(start, None, vec2(f32::NAN, f32::INFINITY), MIN), start));
    }

    #[test]
    fn handles_move_their_edges_within_the_area_and_the_minimum() {
        let start = r(0.2, 0.2, 0.4, 0.5);
        assert!(close(dragged_box(start, Some(BoxHandle::BottomRight), vec2(0.1, 0.1), MIN), r(0.2, 0.2, 0.5, 0.6)));
        assert!(close(dragged_box(start, Some(BoxHandle::TopLeft), vec2(-1.0, -1.0), MIN), r(0.0, 0.0, 0.4, 0.5)));
        assert!(close(dragged_box(start, Some(BoxHandle::Right), vec2(-1.0, 0.3), MIN), r(0.2, 0.2, 0.25, 0.5)), "the minimum holds");
        assert!(close(dragged_box(start, Some(BoxHandle::Top), vec2(0.3, 0.9), MIN), r(0.2, 0.45, 0.4, 0.5)));
        // A box already smaller than the minimum is not grown by a drag that shrinks it.
        let tiny = r(0.5, 0.5, 0.51, 0.51);
        let out = dragged_box(tiny, Some(BoxHandle::Left), vec2(0.2, 0.0), MIN);
        assert!(out.width() >= 0.0 && out.min.x <= 0.51 && out.max.x <= 1.0, "{out:?}");
        // Hostile input stays a rectangle on the area.
        let wild = dragged_box(r(f32::NAN, 2.0, -3.0, f32::INFINITY), Some(BoxHandle::BottomLeft), vec2(0.1, 0.1), vec2(f32::NAN, 9.0));
        assert!([wild.min.x, wild.min.y, wild.max.x, wild.max.y].iter().all(|v| (0.0..=1.0).contains(v)), "{wild:?}");
        assert!(wild.min.x <= wild.max.x && wild.min.y <= wild.max.y);
    }
}
