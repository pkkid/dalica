//! The toolbar (`ui-layout.md` §3): 40 high under the menu bar. A frame for now; its controls
//! (Import, New Album, Flat | Tree, view options, the filters, the age slider, the search field and
//! the activity indicator) arrive with M3.7.

use egui::Ui;

use crate::app::App;

/// Paints the toolbar into `ui`'s rect (the toolbar region of [`super::layout`]).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    let rule = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max);
    ui.painter().rect_filled(rule, 0, t.divider);
}
