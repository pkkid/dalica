//! The library tree's scroll bar (`ui-layout.md` §4, §5.6: "the folder list keeps an ordinary
//! position scrollbar"): a [`WIDTH`]-wide strip at the pane's right edge, shown only while the
//! content is taller than the pane (the rows then end left of it).
//!
//! - The **thumb** (`scroll:tree/thumb`) is as tall as the share of the content on screen (at
//!   least [`MIN_THUMB`]) and sits at the proportional place in the track ([`thumb`]). Dragged, it
//!   moves the rows with it ([`dragged`]): the offset follows the pointer, scaled so the thumb's
//!   travel covers the whole content.
//! - A press in the **track** (`scroll:tree`) above or below the thumb pages the pane one view
//!   height that way ([`paged`]).
//! - The look of the light table's bar: the track `inset`, the thumb a rounded pill in `track`,
//!   `thumb` while hovered or held.
//!
//! [`input`] runs before the rows are laid out, so a frame paints them where the bar put them;
//! [`paint`] runs after them and registers the two ids.

use dalica_ui_kit::Tokens;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{CornerRadius, Id, Rect, Response, Sense, Ui, pos2, vec2};

/// The automation ids of the track and the thumb (`control-protocol.md` §7).
pub const TRACK_ID: &str = "scroll:tree";
pub const THUMB_ID: &str = "scroll:tree/thumb";
/// The strip's width (points).
pub const WIDTH: f32 = 10.0;
/// The thumb's least height (points).
pub const MIN_THUMB: f32 = 24.0;

/// The strip at the right edge of `pane` when content `content_h` high does not fit in it; `None`
/// when it fits.
pub fn strip(pane: Rect, content_h: f32) -> Option<Rect> {
    (content_h.is_finite() && content_h > pane.height() && pane.width() > WIDTH && pane.is_finite())
        .then(|| Rect::from_min_max(pos2(pane.max.x - WIDTH, pane.min.y), pane.max))
}

/// The thumb in `track` for a view `view_h` high over `content_h` points scrolled to `offset`.
pub fn thumb(track: Rect, view_h: f32, content_h: f32, offset: f32) -> Rect {
    let max = (content_h - view_h).max(0.0);
    let h = if content_h > 0.0 { (track.height() * view_h / content_h).clamp(MIN_THUMB.min(track.height()), track.height()) } else { track.height() };
    let travel = (track.height() - h).max(0.0);
    let share = if max > 0.0 && offset.is_finite() { (offset / max).clamp(0.0, 1.0) } else { 0.0 };
    Rect::from_min_size(pos2(track.min.x, track.min.y + travel * share), vec2(track.width(), h))
}

/// The offset after the thumb, pressed at offset `from`, has been dragged `dy` points in `track`.
pub fn dragged(track: Rect, view_h: f32, content_h: f32, from: f32, dy: f32) -> f32 {
    let max = (content_h - view_h).max(0.0);
    let travel = track.height() - thumb(track, view_h, content_h, from).height();
    if !(travel > 0.0 && dy.is_finite() && from.is_finite()) {
        return from.clamp(0.0, max);
    }
    (from + dy * max / travel).clamp(0.0, max)
}

/// The offset after a press in the track at `y`: one view height towards it (nothing on the thumb).
pub fn paged(track: Rect, view_h: f32, content_h: f32, offset: f32, y: f32) -> f32 {
    let max = (content_h - view_h).max(0.0);
    let th = thumb(track, view_h, content_h, offset);
    let to = if y < th.min.y {
        offset - view_h
    } else if y >= th.max.y {
        offset + view_h
    } else {
        offset
    };
    if to.is_finite() { to.clamp(0.0, max) } else { 0.0 }
}

/// The bar's state between frames: where a drag of the thumb started.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bar {
    /// While the thumb is held: the offset and the pointer's y when it was pressed.
    held: Option<(f32, f32)>,
}

/// This frame's interaction with the bar, for [`paint`].
pub struct Frame {
    track: Response,
    thumb: Response,
}

/// Before the rows: the thumb's drag and a press in the track, applied to `offset`.
pub fn input(ui: &mut Ui, bar: &mut Bar, strip: Rect, view_h: f32, content_h: f32, offset: &mut f32) -> Frame {
    let th = thumb(strip, view_h, content_h, *offset);
    let track = ui.interact(strip, Id::new(TRACK_ID), Sense::click());
    let handle = ui.interact(th, Id::new(THUMB_ID), Sense::drag());
    let pointer_y = ui.input(|i| i.pointer.interact_pos()).map(|p| p.y);
    if handle.drag_started()
        && let Some(y) = pointer_y
    {
        bar.held = Some((*offset, y));
    }
    match (bar.held, handle.dragged() || handle.drag_started(), pointer_y) {
        (Some((from, y0)), true, Some(y)) => *offset = dragged(strip, view_h, content_h, from, y - y0),
        (_, false, _) => bar.held = None,
        _ => {}
    }
    if track.clicked()
        && bar.held.is_none()
        && let Some(p) = track.interact_pointer_pos()
        && !th.contains(p)
    {
        *offset = paged(strip, view_h, content_h, *offset, p.y);
    }
    Frame { track, thumb: handle }
}

/// After the rows: the bar at the final `offset`, and its ids.
pub fn paint(ui: &Ui, t: &Tokens, f: &Frame, strip: Rect, view_h: f32, content_h: f32, offset: f32) {
    let painter = ui.painter().with_clip_rect(strip.intersect(ui.clip_rect()));
    painter.rect_filled(strip, 0, t.inset);
    let th = thumb(strip, view_h, content_h, offset);
    let active = f.thumb.hovered() || f.thumb.is_pointer_button_down_on() || f.thumb.dragged();
    let pill = th.shrink2(vec2(2.0, 2.0));
    if pill.is_positive() {
        painter.rect_filled(pill, CornerRadius::same(3), if active { t.thumb } else { t.track });
    }
    widgets::register(
        ui.ctx(),
        TRACK_ID,
        Role::ScrollTrack,
        "Scroll the library tree",
        Target::Widget(&f.track),
        f.track.is_pointer_button_down_on(),
    );
    widgets::register(ui.ctx(), THUMB_ID, Role::ScrollThumb, "Library tree position", Target::Widget(&f.thumb), f.thumb.dragged());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Rect {
        Rect::from_min_size(pos2(230.0, 64.0), vec2(WIDTH, 400.0))
    }

    #[test]
    fn the_strip_shows_only_when_the_content_does_not_fit() {
        let pane = Rect::from_min_size(pos2(0.0, 64.0), vec2(240.0, 400.0));
        assert_eq!(strip(pane, 400.0), None);
        assert_eq!(strip(pane, 401.0), Some(Rect::from_min_max(pos2(230.0, 64.0), pos2(240.0, 464.0))));
        assert_eq!(strip(pane, f32::NAN), None);
        assert_eq!(strip(Rect::from_min_size(pos2(0.0, 0.0), vec2(5.0, 10.0)), 100.0), None, "too narrow");
    }

    #[test]
    fn the_thumb_is_proportional_and_drags_cover_the_content() {
        // 1000 of content in a 400 view: a 160 thumb, 240 of travel for 600 of offset.
        let t = thumb(track(), 400.0, 1000.0, 0.0);
        assert_eq!((t.min.y, t.height()), (64.0, 160.0));
        assert_eq!(thumb(track(), 400.0, 1000.0, 600.0).max.y, 464.0, "at the end");
        assert_eq!(thumb(track(), 400.0, 1000.0, 300.0).min.y, 184.0, "half way");
        assert_eq!(thumb(track(), 400.0, 100_000.0, 0.0).height(), MIN_THUMB);
        assert_eq!(dragged(track(), 400.0, 1000.0, 0.0, 120.0), 300.0);
        assert_eq!(dragged(track(), 400.0, 1000.0, 300.0, -1000.0), 0.0, "kept in range");
        assert_eq!(dragged(track(), 400.0, 1000.0, 0.0, 1e9), 600.0);
        assert_eq!(dragged(track(), 400.0, 1000.0, 0.0, f32::NAN), 0.0);
        assert_eq!(dragged(track(), 400.0, 300.0, 50.0, 10.0), 0.0, "nothing to scroll");
    }

    #[test]
    fn a_press_in_the_track_pages_towards_it() {
        assert_eq!(paged(track(), 400.0, 1000.0, 0.0, 400.0), 400.0, "below the thumb");
        assert_eq!(paged(track(), 400.0, 1000.0, 400.0, 400.0), 600.0, "kept in range");
        assert_eq!(paged(track(), 400.0, 1000.0, 600.0, 70.0), 200.0, "above");
        assert_eq!(paged(track(), 400.0, 1000.0, 0.0, 100.0), 0.0, "on the thumb");
        assert_eq!(paged(track(), 400.0, 1000.0, f32::NAN, f32::NAN), 0.0);
    }
}
