//! Options ▸ Export (`menus-and-shortcuts.md` §5 Export): the default export location, size and
//! quality, removing the location, the default watermark text. A stub: it names its groups and
//! lists the draft's changed values until M10.11 draws them.

use egui::Ui;

use crate::app::App;

/// The tab's groups, in order.
pub const GROUPS: [&str; 4] = ["Default export location", "Default size and quality", "Remove location", "Default watermark text"];

pub fn body(app: &mut App, ui: &mut Ui) {
    super::super::text_lines(app, ui, &super::general::stub_lines(app, &GROUPS));
}
