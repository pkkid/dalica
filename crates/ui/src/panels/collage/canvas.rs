//! The collage canvas (`creating.md` §3), a stub until M14.19 draws it: the page at its ratio
//! (`canvas:collage`) with each picture where the engine's placements put it (`item:<n>`, the
//! selected ones outlined in `accent`), the buttons above and below it, and the gestures that move,
//! turn, swap and order the pictures (`crate::commands::collage_canvas`). Until then the empty page
//! at the default format, 16:9 landscape.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Rect, Ui, vec2};

use crate::app::App;

/// The canvas's automation id.
pub const CANVAS_ID: &str = "canvas:collage";
/// The space round the page (points).
pub const MARGIN: f32 = 24.0;
/// The default page's width over its height (16:9, landscape).
pub const DEFAULT_RATIO: f32 = 16.0 / 9.0;

/// A picture's automation id: `item:<n>`, its number in the engine's placements.
pub fn item_id(n: u32) -> String {
    format!("item:{n}")
}

/// The page of width over height `ratio`, as large as fits in `area` less [`MARGIN`], centred;
/// `None` when nothing fits.
pub fn page_rect(area: Rect, ratio: f32) -> Option<Rect> {
    let room = area.shrink(MARGIN);
    if !(room.is_finite() && room.is_positive() && ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let w = room.width().min(room.height() * ratio);
    Some(Rect::from_center_size(room.center(), vec2(w, w / ratio)))
}

/// Draws the canvas into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let area = ui.max_rect();
    let Some(page) = page_rect(area, DEFAULT_RATIO) else { return };
    ui.painter().rect_filled(page, 0, t.canvas);
    widgets::register(ui.ctx(), CANVAS_ID, Role::Other, "Collage page", Target::Painted { rect: page, clip: area }, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn the_page_fits_its_area_at_its_ratio() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1048.0, 548.0));
        let page = page_rect(area, DEFAULT_RATIO).unwrap();
        assert!((page.width() / page.height() - DEFAULT_RATIO).abs() < 0.01);
        assert!(area.shrink(MARGIN).contains_rect(page));
        assert_eq!(page_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 40.0)), DEFAULT_RATIO), None);
        assert_eq!(page_rect(area, f32::NAN), None);
    }
}
