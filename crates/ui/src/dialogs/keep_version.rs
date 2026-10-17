//! "Keep which version?" (`editing.md` §4): asked when A│A is left while its two sides differ (the
//! compare toggle, Back to Library, another photo, a slideshow). "This photo has two different sets
//! of edits. Which one do you want to keep?" [Left] [Right] ([Top] [Bottom] while stacked)
//! [Cancel] with "Don't ask again" (`check:dialog.dontAsk`, the draft's `dontAsk`).
//!
//! Left drops the variant; Right makes the variant's recipe one step of the photo's history
//! (`edit.variant.close`), so one Undo gives the recipe before it back. Either goes back to one
//! photo and then runs the action that asked (the draft's `then`). When the right side cannot be
//! kept (it would drop an entry this version of Dalica cannot read) the refusal is shown as a
//! toast and the question stays. Cancel stays in A│A. "Don't ask again" stores the side chosen
//! (`compare.keep` in `settings.json`), which is then kept without asking.

use dalica_engine::CmdError;
use dalica_engine::settings::KeepVersion;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind, ToastKind};
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::commands::compare;
use crate::state::CompareLayout;

pub const TITLE: &str = "Keep Which Version?";
/// The question and the line under it.
pub const TEXT: &str = "Keep which version?";
pub const DETAIL: &str = "This photo has two different sets of edits. Which one do you want to keep?";
/// The checkbox's label and automation id.
pub const DONT_ASK: &str = "Don't ask again";
pub const DONT_ASK_ID: &str = "check:dialog.dontAsk";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::KeepVersion)
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let (left, right) = match app.state().edit.layout {
        CompareLayout::SideBySide => ("Left", "Right"),
        CompareLayout::Stacked => ("Top", "Bottom"),
    };
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("left", left, ButtonKind::Other), Button::new("right", right, ButtonKind::Other), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(TEXT).font(fonts::semibold(t.body)).color(t.text)).wrap());
    ui.add_space(4.0);
    ui.add(Label::new(RichText::new(DETAIL).font(fonts::regular(t.body)).color(t.text)).wrap());
    ui.add_space(10.0);
    let mut ticked = d.fields.get("dontAsk").and_then(Value::as_bool).unwrap_or(false);
    if widgets::checkbox(ui, &t, DONT_ASK_ID, DONT_ASK, &mut ticked).changed() {
        d.fields.insert("dontAsk".into(), json!(ticked));
    }
}

/// Keeps the side chosen, remembers it when "Don't ask again" is ticked, goes back to one photo and
/// runs what asked.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let keep = match action {
        "left" => KeepVersion::Left,
        "right" => KeepVersion::Right,
        _ => return Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    };
    if let Err(e) = compare::keep_side(app, action) {
        app.toast(&e.to_string(), ToastKind::Caution);
        return Err(e);
    }
    if d.fields.get("dontAsk") == Some(&Value::Bool(true))
        && let Some(s) = app.session_mut()
    {
        s.change_settings(|settings| {
            let mut c = settings.compare();
            c.keep = keep;
            settings.set_compare(&c);
        });
    }
    compare::one_photo(app);
    compare::run_then(app, d.fields.get("then"))
}
