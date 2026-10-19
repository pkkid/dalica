//! The Collage room's Clips tab (`creating.md` §3), a stub until M14.18 draws it: "Clips (N)", the
//! clips' thumbnails (`clip:<PhotoId>`, selected through `collage.selectClips`), + and −
//! (`collage.addClips`, `collage.removeClips`) and Get More (`collage.getMore`). Until then it
//! names what it will hold.

use egui::Ui;

use crate::app::App;

/// What the stub tab says.
pub const STUB: &str = "The pictures this collage can use.";

/// A clip's automation id: `clip:<PhotoId>`.
pub fn clip_id(photo: dalica_core::PhotoId) -> String {
    format!("clip:{photo}")
}

/// Draws the tab's page into `ui`.
pub fn show(app: &mut App, ui: &mut Ui) {
    crate::dialogs::text_lines(app, ui, &[STUB.to_string()]);
}
