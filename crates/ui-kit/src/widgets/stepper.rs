//! A whole-number stepper on an overlay (`viewing.md` §1.3, the slideshow's display time,
//! `design-system.md` §6): `[−] 3 seconds [+]`, one step per click, stopping at the ends of its
//! range. Light on dark like the overlay's buttons.

use std::ops::RangeInclusive;

use egui::{CornerRadius, Id, Response, Sense, Stroke, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, access_range, focus_ring, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;
use crate::text::{self, Text};

/// The edge of the stepper's − and + buttons (points).
pub const STEPPER_BUTTON: f32 = 22.0;
/// The gap between a button and the value (points).
pub const STEPPER_GAP: f32 = 8.0;

/// What a [`stepper`] did this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepperOut {
    /// The value after this pass's clicks (always within the range).
    pub value: i32,
    /// Whether a click changed it.
    pub changed: bool,
}

/// `range` with its ends in order (a reversed range is turned round).
fn ordered(range: &RangeInclusive<i32>) -> (i32, i32) {
    let (a, b) = (*range.start(), *range.end());
    (a.min(b), a.max(b))
}

/// `value` moved by `delta` steps and kept within `range` (its ends in either order); never
/// overflows.
pub fn stepped(value: i32, delta: i32, range: RangeInclusive<i32>) -> i32 {
    let (lo, hi) = ordered(&range);
    value.saturating_add(delta).clamp(lo, hi)
}

/// The automation ids of a stepper's − and + buttons.
pub fn stepper_ids(id: &str) -> (String, String) {
    (format!("{id}/minus"), format!("{id}/plus"))
}

/// A stepper (`viewing.md` §1.3): a − button, the value as `text(value)` (13 pt `overlay_text`,
/// centred in the width of the widest of `text(min)` and `text(max)`, so the bar never moves as it
/// changes) and a + button, [`STEPPER_GAP`] apart. The buttons are [`STEPPER_BUTTON`] squares,
/// radius 4: `overlay_hover` at rest, `overlay_pressed` hovered or held, their sign
/// `overlay_text`; at the end of the range (or disabled) a button has no plate, its sign
/// `overlay_disabled`, and takes no clicks. A value outside the range is first brought into it.
/// Registers `<id>/minus` and `<id>/plus` (role `button`) and `<id>` (role `other`, the value's
/// text, painted).
pub fn stepper(ui: &mut Ui, t: &Tokens, id: &str, value: i32, range: RangeInclusive<i32>, text: impl Fn(i32) -> String) -> StepperOut {
    let (lo, hi) = ordered(&range);
    let start = value.clamp(lo, hi);
    let enabled = ui.is_enabled();
    let font = fonts::regular(t.body);
    let widest = [text(lo), text(hi)].into_iter().map(|s| ui.painter().layout_no_wrap(s, font.clone(), t.overlay_text).size().x).fold(0.0, f32::max);
    let (minus_id, plus_id) = stepper_ids(id);
    let mut value = start;
    let texts = text::texts(ui.ctx());
    let minus = button(ui, t, &minus_id, false, enabled && start > lo, texts.get(Text::Decrease));
    if minus.clicked() {
        value = stepped(value, -1, lo..=hi);
    }
    let (_, label_rect) = ui.allocate_space(vec2(widest.ceil() + 2.0 * STEPPER_GAP, STEPPER_BUTTON));
    let plus = button(ui, t, &plus_id, true, enabled && start < hi, texts.get(Text::Increase));
    if plus.clicked() {
        value = stepped(value, 1, lo..=hi);
    }
    let shown = text(value);
    // Both buttons carry the value they change, as a spin button's do.
    for r in [&minus, &plus] {
        access_range(r, f64::from(value), f64::from(lo), f64::from(hi), 1.0, &shown);
    }
    let color = if enabled { t.overlay_text } else { t.overlay_disabled };
    let galley = ui.painter().layout_no_wrap(shown.clone(), font, color);
    let pos = label_rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos.round(), galley, color);
    register(ui.ctx(), id, Role::Other, &shown, Target::Painted { rect: label_rect, clip: ui.clip_rect() }, false);
    StepperOut { value, changed: value != start }
}

/// One of the stepper's buttons: a − or (`plus`) a + sign.
fn button(ui: &mut Ui, t: &Tokens, id: &str, plus: bool, enabled: bool, tooltip: &str) -> Response {
    let (_, rect) = ui.allocate_space(Vec2::splat(STEPPER_BUTTON));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled {
        let fill = if r.hovered() || r.is_pointer_button_down_on() { t.overlay_pressed } else { t.overlay_hover };
        p.rect_filled(rect, CornerRadius::same(t.radius), fill);
    }
    focus_ring(p, t, &r, rect, t.radius);
    let color = if enabled { t.overlay_text } else { t.overlay_disabled };
    let stroke = Stroke::new(1.5, color);
    let c = rect.center();
    let arm = (STEPPER_BUTTON * 0.25).round();
    p.line_segment([pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)], stroke);
    if plus {
        p.line_segment([pos2(c.x, c.y - arm), pos2(c.x, c.y + arm)], stroke);
    }
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, tooltip));
    register(ui.ctx(), id, Role::Button, tooltip, Target::Widget(&r), false);
    super::tooltip(r, t, tooltip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_within_the_range_whatever_its_order() {
        assert_eq!(stepped(3, 1, 1..=60), 4);
        assert_eq!(stepped(60, 1, 1..=60), 60);
        assert_eq!(stepped(1, -1, 1..=60), 1);
        let (hi, lo) = (60, 1);
        assert_eq!(stepped(5, 1, hi..=lo), 6, "a reversed range is turned round");
        assert_eq!(stepped(i32::MAX, 1, i32::MIN..=i32::MAX), i32::MAX);
        assert_eq!(stepped(i32::MIN, -1, i32::MIN..=i32::MAX), i32::MIN);
        assert_eq!(stepped(99, 0, 1..=60), 60, "a value outside the range is brought into it");
    }
}
