//! About Dalica (`contributors.md` → In Dalica, `menus-and-shortcuts.md` §1.8): opened by
//! `app.about` (Help ▸ About Dalica; on macOS, Dalica ▸ About Dalica). Its tabs **About ·
//! Contributors · Models** are `tab:about.about`, `tab:about.contributors` and `tab:about.models`;
//! the tab and how Contributors lists people are view state ([`crate::state::About`], `ui.set
//! {about}`), so the dialog reopens as it was left. It has no draft and one button, Close (Esc).
//!
//! The tabs show only their names and the version for now; the credits and the Models table
//! replace [`body`] later in this milestone.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets;
use egui::{RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::AboutTab;

pub const TITLE: &str = "About Dalica";
/// The dialog's width (points).
pub const WIDTH: f32 = 520.0;

/// The tabs: their state value, widget id and label.
pub const TABS: [(AboutTab, &str, &str); 3] = [
    (AboutTab::About, "tab:about.about", "About"),
    (AboutTab::Contributors, "tab:about.contributors", "Contributors"),
    (AboutTab::Models, "tab:about.models", "Models"),
];

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::About)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::close()] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let now = app.state().about.tab;
    ui.horizontal(|ui| {
        for (tab, id, label) in TABS {
            if widgets::text_button(ui, &t, id, label, now == tab).clicked() {
                app.state_mut().about.tab = tab;
            }
        }
    });
    ui.add_space(12.0);
    let text = match app.state().about.tab {
        AboutTab::About => format!("Dalica {}", env!("CARGO_PKG_VERSION")),
        AboutTab::Contributors => "Contributors".to_string(),
        AboutTab::Models => "Models".to_string(),
    };
    ui.label(RichText::new(text).font(fonts::regular(t.body)).color(t.text));
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}
