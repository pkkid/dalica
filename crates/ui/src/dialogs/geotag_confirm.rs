//! The geotag confirmations (`organizing.md` §7): changing the location of more than 20 photos
//! ("Change the location of %d photos?" [Change Location] [Cancel]) and removing it, from more than
//! 5 in the Places pane and always from Tools ▸ Geotag ▸ Clear Location… ("Remove the location from
//! %d photos?" [Remove Location] [Cancel]). Their buttons run `photo.geo.set {ids, geo, confirm:
//! true}` and `geo.clear {ids, confirm: true}`, one library undo step.
//!
//! M13.4's stubs: their titles, buttons and drafts (`dialogs[].fields`: `ids`, and for a change
//! `at`, the point) are final; the bodies show the question with the count, and their default
//! buttons say what they wait for. M13.22 makes them act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const CHANGE_TITLE: &str = "Change Location";
pub const CLEAR_TITLE: &str = "Clear Location";
/// The dialogs' width (points).
pub const WIDTH: f32 = 420.0;
/// Why their buttons do not act yet.
pub const NOT_YET: &str = "Arrives with geotags.";

/// A dialog of `kind` (one of this module's) with `draft`.
pub fn dialog(kind: DialogKind, draft: Map<String, Value>) -> Dialog {
    let mut d = Dialog::new(kind);
    d.fields = draft;
    d
}

/// How many photos the draft names.
fn count(d: &Dialog) -> usize {
    d.fields.get("ids").and_then(Value::as_array).map_or(0, Vec::len)
}

/// The question, with the count: "Remove the location from 6 photos?".
pub fn question(d: &Dialog) -> String {
    let n = count(d);
    let photos = if n == 1 { "1 photo".to_string() } else { format!("{n} photos") };
    match d.kind {
        DialogKind::ChangeLocation => format!("Change the location of {photos}?"),
        _ => format!("Remove the location from {photos}?"),
    }
}

/// The default button of each kind: its action and label.
fn default_button(kind: DialogKind) -> (&'static str, &'static str) {
    match kind {
        DialogKind::ChangeLocation => ("change", "Change Location"),
        _ => ("remove", "Remove Location"),
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let title = if d.kind == DialogKind::ChangeLocation { CHANGE_TITLE } else { CLEAR_TITLE };
    let (action, label) = default_button(d.kind);
    Look { title: title.into(), width: WIDTH, buttons: vec![Button::new(action, label, ButtonKind::Default), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::text_lines(app, ui, &[question(d)]);
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    if action == default_button(d.kind).0 {
        return Err(CmdError::Disabled(NOT_YET.into()));
    }
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}
