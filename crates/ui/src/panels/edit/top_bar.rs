//! The edit top bar (`editing.md` §2): Back to Library, Play, the filmstrip, the compare toggle
//! and, in compare modes, Switch Focus and Switch Layout.
//!
//! A stub until M7.10: only Back to Library (`button:edit.back`, `view.library`) is drawn, so the
//! frame can be left with the pointer as well as with Esc.

use dalica_ui_kit::widgets;
use egui::{Align, Layout, Ui, UiBuilder, vec2};
use serde_json::json;

use crate::app::App;

/// Back to Library.
pub const BACK_ID: &str = "button:edit.back";
pub const BACK: &str = "Back to Library";

/// Draws the top bar's left group into `ui`'s rect (the whole bar).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink2(vec2(8.0, 0.0));
    let mut row = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    if widgets::text_button(&mut row, &t, BACK_ID, BACK, false).clicked() {
        let _ = app.run("view.library", &json!({}));
    }
}
