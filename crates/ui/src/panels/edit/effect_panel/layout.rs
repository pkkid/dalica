//! Where the parts of an effect's parameter panel go inside its rect (M12.16 adds the controls'
//! rows between the title and the buttons).

use egui::{Rect, pos2};

/// The title row's height.
pub const TITLE_H: f32 = 26.0;
/// The gap between the panel's rows.
pub const GAP: f32 = 8.0;

/// The parts of a panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub title: Rect,
    /// The row of Apply │ Cancel, right-aligned under the title.
    pub buttons: Rect,
}

/// The parts of a panel filling `rect`, its buttons `button_h` high; never inside out.
pub fn compute(rect: Rect, button_h: f32) -> Parts {
    let (l, r) = (rect.min.x, rect.max.x.max(rect.min.x));
    let bottom = rect.max.y.max(rect.min.y);
    let title = Rect::from_min_max(pos2(l, rect.min.y), pos2(r, (rect.min.y + TITLE_H).min(bottom)));
    let top = (title.max.y + GAP).min(bottom);
    let buttons = Rect::from_min_max(pos2(l, top), pos2(r, (top + button_h.max(0.0)).min(bottom)));
    Parts { title, buttons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_parts_stack_and_never_turn_inside_out() {
        let p = compute(Rect::from_min_size(pos2(8.0, 100.0), vec2(264.0, 400.0)), 24.0);
        assert_eq!(p.title, Rect::from_min_max(pos2(8.0, 100.0), pos2(272.0, 126.0)));
        assert_eq!(p.buttons, Rect::from_min_max(pos2(8.0, 134.0), pos2(272.0, 158.0)));
        for h in [0.0, 10.0, f32::NAN] {
            let p = compute(Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, h)), 24.0);
            for r in [p.title, p.buttons] {
                assert!(!(r.width() < 0.0 || r.height() < 0.0), "{h}: {r:?}");
            }
        }
    }
}
