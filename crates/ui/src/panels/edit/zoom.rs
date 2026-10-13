//! The "Zoomed to 200 %" read-out at the lower right of the photo area while zoomed (`editing.md`
//! §3): a `toast_bg` pill [`INSET`] from the area's bottom-right corner, its text 12.5 pt `text`,
//! registered as `text:zoom` with the text as its label. The navigator sits above it.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Rect, Ui, pos2, vec2};

use super::canvas::View;

pub const ID: &str = "text:zoom";
/// The pill's distance from the photo area's edges, its height and its side padding.
pub const INSET: f32 = 12.0;
pub const HEIGHT: f32 = 24.0;
const PAD: f32 = 10.0;

/// The read-out's text for `scale` (1 is 100 %): "Zoomed to 200 %".
pub fn text(scale: f32) -> String {
    let pct = if scale.is_finite() { (scale * 100.0).round().clamp(0.0, 1.0e6) } else { 0.0 };
    format!("Zoomed to {pct:.0} %")
}

/// Draws the read-out while `v` is zoomed; returns its rect.
pub fn show(ui: &mut Ui, t: &Tokens, v: &View) -> Option<Rect> {
    if !v.zoomed {
        return None;
    }
    let label = text(v.scale);
    let painter = ui.painter_at(v.area);
    let galley = painter.layout_no_wrap(label.clone(), fonts::regular(12.5), t.text);
    let size = vec2((galley.size().x + 2.0 * PAD).round(), HEIGHT);
    let rect = Rect::from_min_size(pos2(v.area.max.x - INSET - size.x, v.area.max.y - INSET - size.y), size);
    painter.rect_filled(rect, (HEIGHT / 2.0) as u8, t.toast_bg);
    painter.galley(rect.center() - galley.size() / 2.0, galley, t.text);
    widgets::register(ui.ctx(), ID, Role::Other, &label, Target::Painted { rect, clip: ui.clip_rect() }, false);
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_rounds_to_whole_percent() {
        assert_eq!(text(2.0), "Zoomed to 200 %");
        assert_eq!(text(1.004), "Zoomed to 100 %");
        assert_eq!(text(0.3333), "Zoomed to 33 %");
        assert_eq!(text(f32::NAN), "Zoomed to 0 %");
    }
}
