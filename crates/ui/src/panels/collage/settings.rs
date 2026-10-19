//! The Collage room's Settings tab (`creating.md` §3), a stub until M14.18 draws it: the style
//! dropdown (`collage.style`), Picture borders, Grid spacing, Background, Page format with its
//! orientation, Draw shadows and Show captions (`collage.settings`), each greyed for the styles
//! that do not offer it; then Create Collage, Reset and Close (`collage.createCollage`,
//! `collage.resetSettings`, `collage.closeRoom`). Until then it names what it will hold.

use egui::Ui;

use crate::app::App;

/// What the stub tab says.
pub const STUB: &str = "The collage's style, borders, spacing, background and page format.";

/// Draws the tab's page into `ui`.
pub fn show(app: &mut App, ui: &mut Ui) {
    crate::dialogs::text_lines(app, ui, &[STUB.to_string()]);
}
