//! The light table's scroll bar, in the 16-wide strip right of the grid (`ui-layout.md` §2): for
//! now a position bar (a handle as long as the visible share, dragged to scroll; a click on the
//! track pages), shown only when the content is taller than the view. The speed-control bar of
//! `ui-layout.md` §5.6 replaces it behind the same boundary.
//!
//! **Boundary.** Two calls a frame. [`input`] runs before the content is painted, with the frame at
//! the offset the view has so far, and returns a [`ScrollRequest`] (jump, or animate to an anchor)
//! that the parent applies at once, so the content is painted where the bar put it; [`paint`] runs
//! after the content with the final offset, paints the bar and registers `scroll:track` and
//! `scroll:thumb`. The frame gives the layout (section tops for ⇑/⇓), the offset, the view's height
//! and the clock; what the bar keeps between frames lives in [`Scrollbar`].

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{CornerRadius, Id, Rect, Response, Sense, Ui, pos2, vec2};

use super::Frame;
use crate::state::ScrollAnchor;

/// The shortest the handle gets (points).
pub const MIN_HANDLE: f32 = 24.0;
/// The painted handle's width, centred in the strip.
const HANDLE_W: f32 = 6.0;

/// What the scroll bar asks the light table to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollRequest {
    /// Put the view's top at this content offset now.
    To(f64),
    /// Ease the view to this anchor.
    Animate(ScrollAnchor),
}

/// What the scroll bar keeps between frames.
#[derive(Debug, Default)]
pub struct Scrollbar {
    /// While the handle is dragged: where on the handle it was grabbed (points from its top).
    grab: Option<f32>,
    /// This frame's interaction with the strip.
    response: Option<Response>,
}

/// The handle in `track` for a view `view_h` high over `content` points scrolled to `offset`: its
/// length is the visible share (at least [`MIN_HANDLE`]), its top follows the offset. Hostile
/// numbers stay inside the track.
pub fn handle(track: Rect, view_h: f32, content: f64, offset: f64) -> Rect {
    let h = track.height().max(0.0);
    let content = if content.is_finite() { content.max(f64::from(view_h)).max(1.0) } else { 1.0 };
    let share = (f64::from(view_h.max(0.0)) / content).clamp(0.0, 1.0) as f32;
    let len = (h * share).max(MIN_HANDLE).min(h);
    let max = content - f64::from(view_h);
    let frac = if max > 0.0 && offset.is_finite() { (offset / max).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let top = track.min.y + frac * (h - len);
    Rect::from_x_y_ranges(track.x_range(), top..=top + len)
}

/// Before the content: a drag on the handle or a click on the track.
pub fn input(ui: &mut Ui, f: &Frame<'_>, bar: &mut Scrollbar, rect: Rect) -> Option<ScrollRequest> {
    let view_h = f.view_h();
    let max = f.layout.max_offset(view_h);
    if max <= 0.0 || !rect.is_positive() {
        bar.grab = None;
        bar.response = None;
        return None;
    }
    let r = ui.interact(rect, Id::new("lightTable.scrollbar"), Sense::click_and_drag());
    let thumb = handle(rect, view_h, f.layout.height(), f.offset);
    let mut request = None;
    if r.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        bar.grab = Some(if thumb.contains(origin) { origin.y - thumb.min.y } else { thumb.height() / 2.0 });
    }
    if r.dragged()
        && let (Some(grab), Some(pos)) = (bar.grab, r.interact_pointer_pos())
    {
        let room = (rect.height() - thumb.height()).max(f32::MIN_POSITIVE);
        let frac = f64::from(((pos.y - grab - rect.min.y) / room).clamp(0.0, 1.0));
        request = Some(ScrollRequest::To(frac * max));
    } else if r.clicked()
        && let Some(pos) = r.interact_pointer_pos()
    {
        let page = f64::from(view_h);
        if pos.y < thumb.min.y {
            request = Some(ScrollRequest::To(f.offset - page));
        } else if pos.y > thumb.max.y {
            request = Some(ScrollRequest::To(f.offset + page));
        }
    }
    if r.drag_stopped() {
        bar.grab = None;
    }
    bar.response = Some(r);
    request
}

/// After the content: the bar at the final offset, and its ids.
pub fn paint(ui: &mut Ui, f: &Frame<'_>, bar: &mut Scrollbar, rect: Rect) {
    let Some(r) = &bar.response else { return };
    let t = f.t;
    let thumb = handle(rect, f.view_h(), f.layout.height(), f.offset);
    let active = r.dragged() || r.hovered();
    let pill = Rect::from_center_size(pos2(thumb.center().x, thumb.center().y), vec2(HANDLE_W, (thumb.height() - 4.0).max(HANDLE_W)));
    ui.painter().rect_filled(pill, CornerRadius::same(3), if active { t.thumb_hover } else { t.thumb });
    widgets::register(ui.ctx(), "scroll:track", Role::ScrollTrack, "Scroll bar", Target::Widget(r), r.dragged());
    widgets::register(ui.ctx(), "scroll:thumb", Role::ScrollThumb, "Scroll position", Target::Painted { rect: thumb, clip: rect }, r.dragged());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_follows_the_offset_and_never_leaves_its_track() {
        let track = Rect::from_min_size(pos2(490.0, 100.0), vec2(16.0, 400.0));
        let top = handle(track, 400.0, 1600.0, 0.0);
        assert_eq!((top.min.y, top.height()), (100.0, 100.0));
        let bottom = handle(track, 400.0, 1600.0, 1200.0);
        assert_eq!((bottom.min.y, bottom.max.y), (400.0, 500.0));
        assert_eq!(handle(track, 400.0, 1600.0, 600.0).min.y, 250.0);
        // A long list keeps a grabbable handle; hostile numbers stay inside the track.
        assert_eq!(handle(track, 400.0, 1e9, 0.0).height(), MIN_HANDLE);
        for (content, offset) in [(f64::NAN, 0.0), (0.0, 0.0), (1600.0, f64::NAN), (1600.0, -50.0), (1600.0, 1e9), (f64::INFINITY, 5.0)] {
            let r = handle(track, 400.0, content, offset);
            assert!(r.min.y >= 100.0 && r.max.y <= 500.0 && r.height() >= 0.0, "{content} {offset}: {r:?}");
        }
    }
}
