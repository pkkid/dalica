//! Options ▸ File Types (`menus-and-shortcuts.md` §5 File Types): "Show JPEG files and:" with one
//! checkbox per type, and the small-picture threshold. A stub: it names its groups and lists the
//! draft's changed values until M10.11 draws them.

use egui::Ui;

use crate::app::App;

/// The tab's groups, in order.
pub const GROUPS: [&str; 2] = ["Show JPEG files and:", "Small-picture threshold"];

pub fn body(app: &mut App, ui: &mut Ui) {
    super::super::text_lines(app, ui, &super::general::stub_lines(app, &GROUPS));
}
