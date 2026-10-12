//! Confirmations: the questions of commands whose UI asks first (`control-protocol.md` §5.5,
//! `data-model.md` §1.5), asked through the host. [`ASKS`] is the one table: the host opens the
//! question when such a command runs without `"confirm": true` and its question applies
//! ([`super::asking`]), `engine.execute` answers `needs_confirm` with the same title, text and
//! buttons ([`prompt`]), and the default button runs the command again with `"confirm": true`.
//!
//! To route another command's question through the host: give the command a `confirm` parameter,
//! add its [`DialogKind`] variant (the `<kind>` of `dialog:<kind>`), a row here and the variant to
//! the arms of `super::{look, body, act}` that send confirmations to this module.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogCommand, DialogKind, Look};
use crate::app::App;
use crate::tray;

/// The width of a confirmation (points).
pub const WIDTH: f32 = 360.0;

/// A command whose UI asks first, and its question.
pub struct Ask {
    /// The command; it takes `"confirm": true` to go ahead.
    pub command: &'static str,
    pub kind: DialogKind,
    pub title: &'static str,
    pub text: &'static str,
    /// The default button: its action (`button:dialog.<action>`) and label.
    pub action: &'static str,
    pub label: &'static str,
    /// Whether running the command with these parameters asks (some ask only sometimes: Clear asks
    /// only when held photos would go).
    pub asks: fn(&App, &Value) -> bool,
}

/// Every command that asks through the host.
pub static ASKS: &[Ask] = &[Ask {
    command: "tray.clear",
    kind: DialogKind::ClearTray,
    title: tray::CLEAR_TITLE,
    text: tray::CLEAR_QUESTION,
    action: "clearTray",
    label: tray::CLEAR_TITLE,
    asks: tray_clear_asks,
}];

/// `tray.clear` asks when no photos are named and it would empty a tray holding held photos.
fn tray_clear_asks(app: &App, params: &Value) -> bool {
    params.get("ids").is_none_or(Value::is_null) && !tray::is_empty(app.state()) && tray::clear_asks(app.state())
}

/// The row of `kind`.
pub fn of(kind: DialogKind) -> Option<&'static Ask> {
    ASKS.iter().find(|a| a.kind == kind)
}

/// The dialog asking `ask`'s question for running its command with `params`.
pub fn dialog(ask: &Ask, params: &Value) -> Dialog {
    let params = if params.is_null() { Value::Object(Map::new()) } else { params.clone() };
    Dialog { command: Some(DialogCommand { id: ask.command.into(), params }), ..Dialog::new(ask.kind) }
}

/// `needs_confirm`'s `prompt` (`control-protocol.md` §5.5): the dialog's title, text and buttons.
pub fn prompt(ask: &Ask) -> Value {
    json!({ "title": ask.title, "text": ask.text, "buttons": [ask.label, "Cancel"], "dialog": ask.kind.name() })
}

pub fn look(d: &Dialog) -> Look {
    let (title, action, label) = of(d.kind).map_or(("Confirm", "ok", "OK"), |a| (a.title, a.action, a.label));
    Look { title: title.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new(action, label, ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let text = of(d.kind).map_or("", |a| a.text);
    ui.label(RichText::new(text).font(fonts::regular(t.body)).color(t.text));
}

/// The default button: runs the command asked about with `"confirm": true`.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let c = d.command.as_ref().ok_or_else(|| CmdError::Internal(format!("the {} confirmation has no command", d.kind.name())))?;
    let mut params = match &c.params {
        Value::Object(o) => o.clone(),
        _ => Map::new(),
    };
    params.insert("confirm".into(), Value::Bool(true));
    app.run(&c.id, &Value::Object(params))
}
