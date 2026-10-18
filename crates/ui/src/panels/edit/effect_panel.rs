//! An effect's parameter panel (`editing.md` §8.1): in place of the effect tab's grid while
//! `edit.tool` is `effect`, the panel of [`crate::state::EditState::effect`]: the effect's name as
//! its title, up to four sliders (`slider:effect.<param>`) with their labels centred above, the
//! checkboxes (`check:effect.<param>`), the choices (`choice:effect.<param>`), the colour swatches
//! (`color:effect.<param>`) with their picker popup (`popup:color`) and eyedropper
//! (`button:effect.pick/<param>`), Fade, Soften's Paint and Erase (`button:effect.paint`,
//! `button:effect.erase`), and Apply │ Cancel (`button:panel.apply`, `button:panel.cancel`). Its
//! session is [`crate::commands::effect_session`].
//!
//! **A stub until M12.16**, which owns this file and the [`effect_panel`](self) folder: the
//! title and Apply │ Cancel ([`layout`]).

pub mod layout;

use dalica_ui_kit::widgets;
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Ui, UiBuilder};
use serde_json::json;

use crate::app::App;
use crate::state::effect_row;

/// The panel's buttons.
pub const APPLY_ID: &str = "button:panel.apply";
pub const CANCEL_ID: &str = "button:panel.cancel";

/// Draws the open effect's panel in `ui`'s rect; nothing when no effect is open.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.state().edit.effect.as_ref().map(|e| e.name.clone()) else { return };
    let t: Tokens = *app.tokens();
    let title = effect_row(&name).map_or(name.as_str(), |row| row.label).to_owned();
    let l = layout::compute(ui.max_rect(), t.metrics.button_h);
    ui.painter().text(l.title.left_center(), Align2::LEFT_CENTER, &title, fonts::semibold(t.title), t.text);
    let mut clicked = None;
    let mut row =
        ui.new_child(UiBuilder::new().id_salt("effect.buttons").max_rect(l.buttons).layout(egui::Layout::right_to_left(egui::Align::Center)));
    if widgets::text_button(&mut row, &t, CANCEL_ID, "Cancel", false).clicked() {
        clicked = Some("edit.tool.cancel");
    }
    if widgets::primary_button(&mut row, &t, APPLY_ID, "Apply").clicked() {
        clicked = Some("edit.tool.apply");
    }
    if let Some(id) = clicked
        && let Err(e) = app.run(id, &json!({}))
    {
        app.toast(&e.to_string(), widgets::ToastKind::Caution);
    }
}
