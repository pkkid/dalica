//! The Basic fixes Fill Light row (`editing.md` §6.5): the Fill Light glyph on its `inset` tile,
//! then the slider (`slider:basic.fill_light`, "Fill Light"). It edits the same `fill` as the
//! Tuning tab's Fill Light ([`super::tuning`]): both show the photo's Tuning (or the draft being
//! dragged), and each drag here is its own "Tuning" step, previewed live as the tab's are. On A│A's
//! right side it shows and edits the variant's Tuning, as the tab does there.
//!
//! ```text
//! [☀] Fill Light ─────○─────────
//! ```

use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::widgets::SliderSpec;
use egui::{CornerRadius, Rect, Ui, UiBuilder, Vec2, pos2};

use super::tuning;
use crate::app::App;
use crate::commands::edit_view;

/// The row's slider: Tuning's `fill`.
pub const SPEC: SliderSpec<'static> = SliderSpec { id: "slider:basic.fill_light", ..tuning::FILL };

/// The gap between the glyph's tile and the slider.
pub const GAP: f32 = 8.0;

/// The glyph's tile and the slider inside the row `rect` (`tile` the tile's size).
pub fn layout(rect: Rect, tile: Vec2) -> (Rect, Rect) {
    let tile = Rect::from_center_size(pos2(rect.min.x + tile.x / 2.0, rect.center().y), tile);
    let slider = Rect::from_min_max(pos2((tile.max.x + GAP).min(rect.max.x), rect.min.y), pos2(rect.max.x.max(rect.min.x), rect.max.y));
    (tile, slider)
}

/// Draws this part into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    tuning::poll(app, &ctx);
    let Some(photo) = edit_view::open_photo(app) else { return };
    let t = *app.tokens();
    let (tile, slider) = layout(ui.max_rect(), t.metrics.tool_tile);
    let p = ui.painter();
    p.rect_filled(tile, CornerRadius::same(t.radius), t.inset);
    let glyph = if ui.is_enabled() { t.icon } else { t.text_disabled };
    paint_edit(p, Rect::from_center_size(tile.center(), Vec2::splat(t.metrics.icon_tool)), EditIcon::FillLight, glyph);
    if slider.is_positive() {
        let value = tuning::shown(app, photo).fill;
        let mut row = ui.new_child(UiBuilder::new().id_salt(SPEC.id).max_rect(slider));
        tuning::slider(app, &mut row, &t, photo, &SPEC, value, |d, v| d.fill = v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_tile_then_the_slider_fill_the_row() {
        let (tile, slider) = layout(Rect::from_min_size(pos2(8.0, 200.0), vec2(264.0, 45.0)), vec2(44.0, 30.0));
        assert_eq!(tile, Rect::from_min_size(pos2(8.0, 207.5), vec2(44.0, 30.0)));
        assert_eq!((slider.min.x, slider.max.x, slider.height()), (60.0, 272.0, 45.0));
        let (_, slider) = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(20.0, 45.0)), vec2(44.0, 30.0));
        assert!(slider.width() >= 0.0);
        assert_eq!((SPEC.min, SPEC.max, SPEC.default), (tuning::FILL.min, tuning::FILL.max, 0.0));
    }
}
