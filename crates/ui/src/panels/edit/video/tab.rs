//! The edit panel's video tab (`editing.md` §11), a stub until M15.21 puts it in the panel in
//! place of tabs 1–5 and M15.26 makes it act: **Save Frame as Picture** (`button:video.saveFrame`,
//! `player.saveFrame`: the frame shown, as a JPEG in `Pictures/Dalica/Video Frames`), **Export
//! Clip** (`button:video.exportClip`, `player.exportClip`: the trimmed part, where the save picker
//! says) and the rotate buttons (the photo's own). Until then the buttons are greyed with what they
//! wait for.

use dalica_ui_kit::widgets;
use egui::Ui;

use crate::app::App;
use crate::commands::video::LATER;

/// What the stub tab says.
pub const STUB: &str = "A video: Save Frame as Picture and Export Clip arrive with the video player.";
pub const SAVE_FRAME_ID: &str = "button:video.saveFrame";
pub const EXPORT_CLIP_ID: &str = "button:video.exportClip";

/// Draws the tab's page into `ui`.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    crate::dialogs::text_lines(app, ui, &[STUB.to_string()]);
    ui.add_space(8.0);
    for (id, label) in [(SAVE_FRAME_ID, "Save Frame as Picture"), (EXPORT_CLIP_ID, "Export Clip")] {
        let r = ui.add_enabled_ui(false, |ui| widgets::text_button(ui, &t, id, label, false)).inner;
        widgets::tooltip(r, &t, LATER, None);
        ui.add_space(6.0);
    }
}
