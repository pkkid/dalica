//! Save's first-time explanation (`data-model.md` §6 step 1): asked through the host before
//! `photo.save` runs (a row of [`super::confirm::ASKS`]), with a "Don't show this again" checkbox
//! whose state is the view state's `saveExplain.dontShow`. Its Save button runs `photo.save` with
//! `"confirm": true`, and with `"dontAsk": true` when the box is ticked (the engine then stops
//! asking).
//!
//! A stub until M8.19 (`ui-layout.md` §5.2, `editing.md` §5.1): it always asks, draws the question
//! and the checkbox's state as text, and answers.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, Look, confirm};
use crate::app::App;
use crate::state::SaveExplain;

/// The dialog's title and default button.
pub const TITLE: &str = "Save";
/// The explanation, in our own words.
pub const TEXT: &str = "Saving writes your edits into the photo's file. Dalica first keeps the original, unchanged, in a hidden \
                        .dalica-originals folder beside it, so Undo Save and Revert can always bring it back.";
/// The checkbox's label.
pub const DONT_SHOW: &str = "Don't show this again";

/// Whether `photo.save` with `params` asks first: until M8.19 reads the engine's setting, always.
pub fn asks(_app: &App, _params: &Value) -> bool {
    true
}

/// Called when the question opens: the checkbox starts unticked.
pub fn opened(app: &mut App, _params: &Value) {
    app.state_mut().save_explain = Some(SaveExplain::default());
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: confirm::WIDTH, buttons: vec![Button::cancel(), Button::new("save", TITLE, ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.label(RichText::new(TEXT).font(fonts::regular(t.body)).color(t.text));
    ui.add_space(8.0);
    let ticked = app.state().save_explain.is_some_and(|s| s.dont_show);
    let mark = if ticked { "[x]" } else { "[ ]" };
    ui.label(RichText::new(format!("{mark} {DONT_SHOW}")).font(fonts::regular(t.body)).color(t.text_label));
}

/// Save: runs `photo.save` with `"confirm": true` (and `"dontAsk": true` when the box is ticked).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let dont_show = app.state().save_explain.is_some_and(|s| s.dont_show);
    let extra: Vec<(&str, Value)> = if dont_show { vec![("dontAsk", Value::Bool(true))] } else { Vec::new() };
    let (id, params) = confirm::confirmed_params(d, &extra)?;
    app.run(&id, &params)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn photo_save_asks_first_and_the_checkbox_rides_along() {
        let mut app = App::new(None);
        assert_eq!(app.run("photo.save", &json!({ "ids": [1] })).unwrap(), json!({ "dialog": "saveExplain" }));
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(DialogKind::SaveExplain));
        assert_eq!(app.state().save_explain, Some(SaveExplain { dont_show: false }));
        let d = dialogs::top(&app).cloned().unwrap();
        assert_eq!(confirm::confirmed_params(&d, &[]).unwrap().1, json!({ "ids": [1], "confirm": true }));
        let inspect = dialogs::inspect(&app);
        assert_eq!((inspect["title"].as_str(), inspect["default"].as_str()), (Some(TITLE), Some("button:dialog.save")));
        // Ticked, Save passes dontAsk; without a library (and before M8.15) the command fails, so
        // the question stays.
        app.state_mut().save_explain = Some(SaveExplain { dont_show: true });
        assert!(dialogs::answer(&mut app, None).is_err());
        assert_eq!(app.state().dialogs.len(), 1);
        // Cancel closes it and clears its draft.
        dialogs::cancel(&mut app).unwrap();
        assert_eq!((app.state().dialogs.len(), app.state().save_explain), (0, None));
        // With confirm the question is not asked.
        assert!(dialogs::asking(&app, "photo.save", &json!({ "confirm": true })).is_none());
    }
}
