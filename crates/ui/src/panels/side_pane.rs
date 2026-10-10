//! The side pane (`ui-layout.md` §7): right of the light table's scroll bar when a pane is open,
//! 280 wide (large: min(30 % W, 392)), pushing the light table. A frame for now; the header (« ,
//! title, ×) comes with M3.8 and the pane contents with their milestones.

use egui::Ui;

use crate::app::App;

/// Paints the side pane into `ui`'s rect (the side-pane region of [`super::layout`]); called only
/// while a pane is open.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    let rule = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 1.0, rect.max.y));
    ui.painter().rect_filled(rule, 0, t.divider);
}
