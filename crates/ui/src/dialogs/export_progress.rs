//! The export progress window ("Exporting 12 of 48…", `creating.md` §2): while the view state's
//! `exportProgress` is shown, a small window under the toolbar's right end follows its job. It is
//! not a dialog of the stack: no backdrop, and the window under it keeps taking input (the export
//! runs in the background). `dialog:exportProgress` on screen.
//!
//! A stub until M8.21: a frame with the job's label, no bar and no Cancel yet.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Area, Frame, Id, Margin, RichText, Stroke, Vec2};

use crate::app::App;

/// The window's automation id.
pub const ID: &str = "dialog:exportProgress";
/// Its title.
pub const TITLE: &str = "Exporting";
/// Its width, and its gap from the window's right edge and top (under the toolbar), in points.
pub const WIDTH: f32 = 280.0;
const RIGHT: f32 = 16.0;
const TOP: f32 = 88.0;

/// The line under the title: the job's label as the activity popover shows it.
pub fn text(app: &App) -> Option<String> {
    let p = app.state().export_progress.filter(|p| p.shown)?;
    let label = app.session().and_then(|s| s.job(p.job)).map(|e| e.label).unwrap_or_default();
    Some(if label.is_empty() { format!("{TITLE}…") } else { label })
}

/// Draws the window while it is shown.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(line) = text(app) else { return };
    let t: Tokens = *app.tokens();
    let shown =
        Area::new(Id::new(ID)).anchor(Align2::RIGHT_TOP, Vec2::new(-RIGHT, TOP)).order(egui::Order::Middle).interactable(false).show(ctx, |ui| {
            Frame::new()
                .fill(t.chrome)
                .stroke(Stroke::new(1.0, t.button_border))
                .corner_radius(t.window_radius)
                .shadow(t.window_shadow)
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(WIDTH - 24.0);
                    ui.label(RichText::new(TITLE).font(fonts::semibold(t.title)).color(t.text));
                    let mut line: String = line.chars().take(120).collect();
                    line.retain(|c| !c.is_control());
                    ui.label(RichText::new(line).font(fonts::regular(t.body)).color(t.text_label));
                })
        });
    let rect = shown.response.rect;
    widgets::register(ctx, ID, Role::Other, TITLE, Target::Painted { rect, clip: ctx.content_rect() }, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ExportProgress;

    #[test]
    fn the_window_shows_only_while_shown() {
        let mut app = App::new(None);
        assert_eq!(text(&app), None);
        app.state_mut().export_progress = Some(ExportProgress { job: 3, shown: false });
        assert_eq!(text(&app), None, "hidden");
        app.state_mut().export_progress = Some(ExportProgress { job: 3, shown: true });
        assert_eq!(text(&app).as_deref(), Some("Exporting…"), "a job not known yet");
    }
}
