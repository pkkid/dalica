//! Save's first-time explanation (`data-model.md` §6 step 1, `ui-layout.md` §5.2,
//! `control-protocol.md` §5.5): asked through the host before `photo.save` runs (a row of
//! [`super::confirm::ASKS`]) while the engine's `save` setting has `explain` on, with a "Don't show
//! this again" checkbox (`check:dialog.dontShow`) whose state is the view state's
//! `saveExplain.dontShow`. Its Save button (the default) runs `photo.save` with `"confirm": true`,
//! and with `"dontAsk": true` when the box is ticked, which turns the setting off: the next Save
//! asks nothing.
//!
//! It asks only when the Save would do something (the command's `enabled` accepts the photos
//! named): a Save with nothing to save, or of an offline folder, says why on the status line
//! without a question first.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind};
use egui::{Label, RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, Look, confirm};
use crate::app::App;
use crate::state::SaveExplain;
use crate::tl;

/// The dialog's title and default button.
pub const TITLE: &str = "Save";
/// The explanation, in our own words (the engine answers it over automation).
pub const TEXT: &str = dalica_engine::cmd::save::SAVE_EXPLANATION;
/// The checkbox's label.
pub const DONT_SHOW: &str = "Don't show this again";
/// The checkbox's automation id (`control-protocol.md` §7).
pub const DONT_SHOW_ID: &str = "check:dialog.dontShow";
/// The command it explains.
const COMMAND: &str = "photo.save";

/// Whether `photo.save` with `params` asks first: while the explanation is on, unless `dontAsk`
/// is sent, and only for a Save the command would run.
pub fn asks(app: &App, params: &Value) -> bool {
    let Some(s) = app.session() else { return false };
    if !s.settings().save().explain || params.get("dontAsk") == Some(&Value::Bool(true)) {
        return false;
    }
    s.spec(COMMAND).is_some_and(|spec| (spec.enabled)(s, params).is_ok())
}

/// Called when the question opens: the checkbox starts unticked.
pub fn opened(app: &mut App, _params: &Value) {
    app.state_mut().save_explain = Some(SaveExplain::default());
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: confirm::text(TITLE),
        width: confirm::WIDTH,
        buttons: vec![Button::cancel(), Button::new("save", confirm::text(TITLE), ButtonKind::Default)],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(confirm::text(TEXT)).font(fonts::regular(t.body)).color(t.text)).wrap());
    ui.add_space(10.0);
    let mut ticked = app.state().save_explain.is_some_and(|s| s.dont_show);
    if widgets::checkbox(ui, &t, DONT_SHOW_ID, &tl!("Don't show this again"), &mut ticked).changed() {
        app.state_mut().save_explain = Some(SaveExplain { dont_show: ticked });
    }
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
    use crate::app::tests::session_with_photos;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn photo_save_asks_first_while_the_explanation_is_on_and_the_checkbox_rides_along() {
        let (_dir, mut s, ids) = session_with_photos(2);
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
        let mut app = App::new(Some(s));
        // Nothing to save: no question, the reason on the status line.
        assert!(matches!(app.run("photo.save", &json!({ "ids": [ids[1]] })), Err(CmdError::Disabled(_))));
        assert!(app.state().dialogs.is_empty());
        assert_eq!(app.run("photo.save", &json!({ "ids": [ids[0]] })).unwrap(), json!({ "dialog": "saveExplain" }));
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(DialogKind::SaveExplain));
        assert_eq!(app.state().save_explain, Some(SaveExplain { dont_show: false }));
        let d = dialogs::top(&app).cloned().unwrap();
        assert_eq!(confirm::confirmed_params(&d, &[]).unwrap().1, json!({ "ids": [ids[0]], "confirm": true }));
        let inspect = dialogs::inspect(&app);
        assert_eq!((inspect["title"].as_str(), inspect["default"].as_str()), (Some(TITLE), Some("button:dialog.save")));
        // Cancel closes it and clears its draft; nothing was saved.
        dialogs::cancel(&mut app).unwrap();
        assert_eq!((app.state().dialogs.len(), app.state().save_explain), (0, None));
        assert!(app.session().unwrap().library().has_unsaved_edits(ids[0]));
        // With confirm, or dontAsk, the question is not asked.
        assert!(dialogs::asking(&app, "photo.save", &json!({ "ids": [ids[0]], "confirm": true })).is_none());
        assert!(dialogs::asking(&app, "photo.save", &json!({ "ids": [ids[0]], "dontAsk": true })).is_none());
        // Ticked, Save passes dontAsk, the setting goes off and the next Save asks nothing.
        app.run("photo.save", &json!({ "ids": [ids[0]] })).unwrap();
        app.state_mut().save_explain = Some(SaveExplain { dont_show: true });
        let reply = dialogs::answer(&mut app, None).unwrap();
        assert_eq!(reply["pending"], json!(true), "{reply}");
        assert_eq!(app.state().dialogs.len(), 0);
        assert!(!app.session().unwrap().settings().save().explain);
        assert!(dialogs::asking(&app, "photo.save", &json!({ "ids": [ids[0]] })).is_none());
    }

    #[test]
    fn without_a_library_nothing_asks() {
        let mut app = App::new(None);
        assert!(!asks(&app, &json!({ "ids": [1] })));
        assert!(matches!(app.run("photo.save", &json!({ "ids": [1] })), Err(CmdError::Disabled(_))));
        assert!(app.state().dialogs.is_empty());
    }
}
