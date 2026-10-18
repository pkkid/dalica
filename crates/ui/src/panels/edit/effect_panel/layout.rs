//! Where the parts of an effect's parameter panel go inside its rect: the title, the controls'
//! rows under it, then the row of Reset and Apply │ Cancel. A panel too short for every row (an
//! effect panel left open on the Basic Fixes tab of the 900 × 600 window) keeps the buttons on its
//! foot and scrolls the controls.

use egui::{Rect, pos2};

/// The title row's height.
pub const TITLE_H: f32 = 26.0;
/// The gap between the panel's parts.
pub const GAP: f32 = 8.0;

/// The parts of a panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub title: Rect,
    /// The controls' rows (scrolling when they need more than this).
    pub controls: Rect,
    /// The row of Reset (left) and Apply │ Cancel (right).
    pub buttons: Rect,
}

/// The parts of a panel filling `rect` whose controls need `need` points, its buttons `button_h`
/// high; never inside out.
pub fn compute(rect: Rect, button_h: f32, need: f32) -> Parts {
    let (l, r) = (rect.min.x, rect.max.x.max(rect.min.x));
    let bottom = if rect.max.y.is_finite() { rect.max.y.max(rect.min.y) } else { rect.min.y };
    let clamp = |y: f32| if y.is_finite() { y.clamp(rect.min.y, bottom) } else { bottom };
    let title = Rect::from_min_max(pos2(l, rect.min.y), pos2(r, clamp(rect.min.y + TITLE_H)));
    let button_h = if button_h.is_finite() { button_h.max(0.0) } else { 0.0 };
    let need = if need.is_finite() { need.max(0.0) } else { 0.0 };
    let top = clamp(title.max.y + GAP);
    // The buttons follow the controls, or sit on the foot when the controls need more room.
    let buttons_top = clamp((top + need + GAP).min(bottom - button_h).max(top));
    let buttons = Rect::from_min_max(pos2(l, buttons_top), pos2(r, clamp(buttons_top + button_h)));
    let controls = Rect::from_min_max(pos2(l, top), pos2(r, clamp(buttons_top - GAP).max(top)));
    Parts { title, controls, buttons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_parts_stack_and_never_turn_inside_out() {
        let p = compute(Rect::from_min_size(pos2(8.0, 100.0), vec2(264.0, 400.0)), 24.0, 92.0);
        assert_eq!(p.title, Rect::from_min_max(pos2(8.0, 100.0), pos2(272.0, 126.0)));
        assert_eq!(p.controls, Rect::from_min_max(pos2(8.0, 134.0), pos2(272.0, 226.0)));
        assert_eq!(p.buttons, Rect::from_min_max(pos2(8.0, 234.0), pos2(272.0, 258.0)));
        // Too short: the buttons on the foot, the controls between.
        let p = compute(Rect::from_min_size(pos2(0.0, 0.0), vec2(264.0, 150.0)), 24.0, 300.0);
        assert_eq!(p.buttons, Rect::from_min_max(pos2(0.0, 126.0), pos2(264.0, 150.0)));
        assert_eq!((p.controls.min.y, p.controls.max.y), (34.0, 118.0));
        for h in [0.0, 10.0, f32::NAN] {
            for need in [0.0, 500.0, f32::NAN, -3.0] {
                let p = compute(Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, h)), 24.0, need);
                for r in [p.title, p.controls, p.buttons] {
                    assert!(!(r.width() < 0.0 || r.height() < 0.0), "{h} {need}: {r:?}");
                }
            }
        }
    }
}
