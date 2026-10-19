//! The splitter between two panes (`design-system.md` §4, §6): the library tree's right edge.

use egui::{CornerRadius, CursorIcon, Id, Key, Modifiers, Rect, Response, Sense, Stroke, Ui, WidgetType, vec2};

use super::{Target, focus_ring, register};
use crate::Tokens;
use crate::registry::Role;
use crate::text::{self, Text};

/// A splitter filling `rect`: vertical (it moves left and right) when `rect` is taller than wide,
/// horizontal otherwise. `chrome` at rest with a grip of two 1 pt lines 48 pt long in its middle
/// (`button_border`; `icon` and a `hover × 0.5` strip hovered; `text` and a `hover` strip dragged).
/// The caller moves the split by the response's drag delta along its axis plus [`arrow_delta`] (the
/// keyboard, with the focus), kept in range by [`split_position`]; a double-click is the caller's
/// "reset to default".
pub fn splitter(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect) -> Response {
    let enabled = ui.is_enabled();
    let vertical = rect.height() >= rect.width();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let p = ui.painter();
    let dragging = enabled && (r.dragged() || r.is_pointer_button_down_on());
    let hovered = enabled && r.hovered();
    let strip = if dragging {
        t.hover
    } else if hovered {
        t.hover.gamma_multiply(0.5)
    } else {
        t.chrome
    };
    p.rect_filled(rect, CornerRadius::ZERO, strip);
    let grip_color = if !enabled {
        t.text_disabled
    } else if dragging {
        t.text
    } else if hovered {
        t.icon
    } else {
        t.button_border
    };
    let c = rect.center();
    let half = (t.metrics.grip_h / 2.0).min(if vertical { rect.height() } else { rect.width() } / 2.0);
    let stroke = Stroke::new(1.0, grip_color);
    let grip = if vertical {
        for dx in [-1.5, 1.5] {
            p.vline(c.x + dx, (c.y - half)..=(c.y + half), stroke);
        }
        Rect::from_center_size(c, vec2(rect.width(), 2.0 * half))
    } else {
        for dy in [-1.5, 1.5] {
            p.hline((c.x - half)..=(c.x + half), c.y + dy, stroke);
        }
        Rect::from_center_size(c, vec2(2.0 * half, rect.height()))
    };
    focus_ring(p, t, &r, grip.shrink(1.0), 2);
    if hovered || dragging {
        ui.ctx().set_cursor_icon(if vertical { CursorIcon::ResizeColumn } else { CursorIcon::ResizeRow });
    }
    let name = text::get(ui.ctx(), Text::Splitter);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ResizeHandle, enabled, &name));
    register(ui.ctx(), id, Role::Other, &name, Target::Widget(&r), dragging);
    r
}

/// The keyboard move of a focused splitter this pass: −8, 0 or +8 points along its axis (←/→ for a
/// vertical splitter, ↑/↓ for a horizontal one). Consumes the keys.
pub fn arrow_delta(ui: &Ui, r: &Response, vertical: bool) -> f32 {
    if !r.has_focus() {
        return 0.0;
    }
    let (less, more) = if vertical { (Key::ArrowLeft, Key::ArrowRight) } else { (Key::ArrowUp, Key::ArrowDown) };
    ui.input_mut(|i| {
        let a = if i.consume_key(Modifiers::NONE, less) { -8.0 } else { 0.0 };
        let b = if i.consume_key(Modifiers::NONE, more) { 8.0 } else { 0.0 };
        a + b
    })
}

/// The split moved from `current` by `delta`, kept in `min ..= max` (in order whichever is larger);
/// a value that is not a finite number leaves the split where it was (or at `min`).
pub fn split_position(current: f32, delta: f32, min: f32, max: f32) -> f32 {
    let (lo, hi) = if min.is_finite() && max.is_finite() { (min.min(max), min.max(max)) } else { (0.0, 0.0) };
    let base = if current.is_finite() { current } else { lo };
    let moved = if delta.is_finite() { base + delta } else { base };
    moved.max(lo).min(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_split_stays_in_its_range() {
        assert_eq!(split_position(240.0, 30.0, 200.0, 920.0), 270.0);
        assert_eq!(split_position(240.0, -100.0, 200.0, 920.0), 200.0);
        assert_eq!(split_position(900.0, 100.0, 200.0, 920.0), 920.0);
        assert_eq!(split_position(240.0, 10.0, 920.0, 200.0), 250.0, "a reversed range is ordered");
        assert_eq!(split_position(f32::NAN, 10.0, 200.0, 920.0), 210.0);
        assert_eq!(split_position(240.0, f32::INFINITY, 200.0, 920.0), 240.0);
        assert_eq!(split_position(240.0, 1.0, f32::NAN, 920.0), 0.0);
    }
}
