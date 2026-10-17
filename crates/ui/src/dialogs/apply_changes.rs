//! "Apply changes to the current photo?" (`editing.md` §4): asked when the focused compare side, or
//! its photo, changes while a modal tool (Crop, Straighten, Retouch) has a draft not yet applied.
//! [Apply] (the default) applies the tool (`edit.tool.apply`), [Discard] drops its draft
//! (`edit.tool.cancel`); either then runs the action that asked (the draft's `then`, by
//! `commands::compare`); [Cancel] runs nothing and the tool stays open. "Don't ask again, always
//! apply" (`check:dialog.dontAsk`, the draft's `dontAsk`) turns the question off
//! (`compare.askBeforeApplying` in `settings.json`): from then on the tool is applied at once.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind};
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Apply Changes";
/// The question.
pub const TEXT: &str = "Apply changes to the current photo?";
/// The checkbox's label and automation id.
pub const DONT_ASK: &str = "Don't ask again, always apply";
pub const DONT_ASK_ID: &str = "check:dialog.dontAsk";
/// Why Apply and Discard refuse with no tool open.
pub const NO_TOOL: &str = "No tool is open.";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::ApplyChanges)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("apply", "Apply", ButtonKind::Default), Button::new("discard", "Discard", ButtonKind::Other), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(TEXT).font(fonts::regular(t.body)).color(t.text)).wrap());
    ui.add_space(10.0);
    let mut ticked = d.fields.get("dontAsk").and_then(Value::as_bool).unwrap_or(false);
    if widgets::checkbox(ui, &t, DONT_ASK_ID, DONT_ASK, &mut ticked).changed() {
        d.fields.insert("dontAsk".into(), json!(ticked));
    }
}

/// Apply or Discard the open tool, remember "always apply" when ticked, then run what asked.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let command = match action {
        "apply" => "edit.tool.apply",
        "discard" => "edit.tool.cancel",
        _ => return Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    };
    if app.state().edit.tool.is_none() {
        return Err(CmdError::Disabled(NO_TOOL.into()));
    }
    app.run(command, &json!({}))?;
    if d.fields.get("dontAsk") == Some(&Value::Bool(true))
        && let Some(s) = app.session_mut()
    {
        s.change_settings(|settings| {
            let mut c = settings.compare();
            c.ask_before_applying = false;
            settings.set_compare(&c);
        });
    }
    crate::commands::compare::run_then(app, d.fields.get("then"))
}
