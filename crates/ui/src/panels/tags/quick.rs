//! The Tags pane's quick tags (`organizing.md` §5.2): ten buttons in rows of 2/3/2/3
//! (`quicktag:1` … `quicktag:10`) and the gear (`button:quicktags.configure`) opening Configure
//! Quick Tags (`quicktags.configure`).
//!
//! For now it shows its caption and the gear; the ten buttons arrive later in this milestone.

use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::widgets;
use egui::Ui;
use serde_json::json;

use crate::app::App;

pub const CONFIGURE_ID: &str = "button:quicktags.configure";
/// The gear's edge (points).
const BUTTON: f32 = 24.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let gear = ui
        .horizontal(|ui| {
            super::caption(app, ui, "Quick tags");
            ui.add_space((ui.available_width() - BUTTON).max(0.0));
            widgets::icon_button(ui, &t, CONFIGURE_ID, Icon::More, BUTTON, false, true, "Configure Quick Tags")
        })
        .inner;
    if gear.clicked() {
        // A refusal (the dialog is already open) is on the status line.
        let _ = app.run("quicktags.configure", &json!({}));
    }
}
