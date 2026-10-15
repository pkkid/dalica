//! Options ▸ General (`menus-and-shortcuts.md` §5 General): the Interface, Files and Import groups,
//! UI scale and font size. A stub: it names its groups and lists the draft's changed values until
//! M10.10 draws them.

use egui::Ui;

use crate::app::App;

/// The tab's groups, in order.
pub const GROUPS: [&str; 3] = ["Interface", "Files", "Import"];

pub fn body(app: &mut App, ui: &mut Ui) {
    super::super::text_lines(app, ui, &stub_lines(app, &GROUPS));
}

/// A stub tab's lines: its groups, then the draft's changed values (`key: value`), so the draft is
/// visible on screen and in screenshots.
pub fn stub_lines(app: &App, groups: &[&str]) -> Vec<String> {
    let mut lines = vec![groups.join(" · ")];
    if let Some(o) = &app.state().options {
        lines.extend(o.values.iter().map(|(k, v)| format!("{k}: {v}")));
    }
    lines
}
