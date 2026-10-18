//! The Unnamed grid's two dialogs (`organizing.md` §6.2):
//!
//! - **New Person**: Name (`field:dialog.name`, prefilled with what was typed, focused), Nickname
//!   (`field:dialog.nickname`, optional), [OK] [Cancel]; asked when a name typed under a group or
//!   for a face is nobody's yet, and by the face menu's Move to New Person…. OK (greyed while the
//!   name is empty) makes the person and names the faces (`faces.name {faces, name, nickname,
//!   create: true}`) as one undo step; a name someone has already names the faces as theirs.
//! - **"Ignore these faces?"** [Ignore] [Cancel] with "Don't ask again" (`check:dialog.dontAsk`): a
//!   group's × and Delete on selected faces. Ignore runs `faces.ignore {faces}`, one undo step;
//!   with the box ticked the question is off from then on (`"people": {"askBeforeIgnoring":
//!   false}` in `settings.json`).
//!
//! The drafts (`dialogs[].fields`) are New Person's `name`, `nickname` and `faces`, and the
//! question's `faces` and `dontAsk`.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind, Form, TextField};
use egui::{Id, Label, RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::{FaceRef, MAX_DRAFT};

pub const TITLE: &str = "New Person";
pub const IGNORE_TITLE: &str = "Ignore Faces";
/// The question of the ignore dialog.
pub const IGNORE_TEXT: &str = "Ignore these faces?";
/// What the question adds: where ignored faces go.
pub const IGNORE_NOTE: &str = "They leave Unnamed and are never found again; Show Ignored Faces shows them.";
/// The checkbox's label and automation id.
pub const DONT_ASK: &str = "Don't ask again";
pub const DONT_ASK_ID: &str = "check:dialog.dontAsk";
/// The fields of New Person.
pub const NAME_ID: &str = "field:dialog.name";
pub const NICKNAME_ID: &str = "field:dialog.nickname";
/// The labels of New Person's rows.
pub const NAME_LABEL: &str = "Name:";
pub const NICKNAME_LABEL: &str = "Nickname:";
/// The nickname field's hint.
pub const OPTIONAL: &str = "Optional";
/// Why OK is greyed with no name.
pub const NO_NAME: &str = "Type the person's name.";
/// The dialogs' width (points).
pub const WIDTH: f32 = 400.0;
/// Where "Don't ask again" is kept (`settings.json`).
pub const SETTINGS_KEY: &str = "people";
pub const ASK_KEY: &str = "askBeforeIgnoring";

/// New Person for `faces`, its Name prefilled with `name`.
pub fn dialog(name: &str, faces: &[FaceRef]) -> Dialog {
    let mut d = Dialog::new(DialogKind::NewPerson);
    d.fields = Map::from_iter([("name".into(), json!(name)), ("nickname".into(), json!("")), ("faces".into(), json!(faces))]);
    d
}

/// "Ignore these faces?" for `faces`.
pub fn ignore_dialog(faces: &[FaceRef]) -> Dialog {
    let mut d = Dialog::new(DialogKind::IgnoreFaces);
    d.fields = Map::from_iter([("faces".into(), json!(faces)), ("dontAsk".into(), json!(false))]);
    d
}

/// Whether ignoring faces asks first (on until "Don't ask again" is ticked).
pub fn asks_before_ignoring(app: &App) -> bool {
    app.session().and_then(|s| s.settings().get(SETTINGS_KEY)).and_then(|p| p.get(ASK_KEY)).and_then(Value::as_bool).unwrap_or(true)
}

fn text<'a>(d: &'a Dialog, key: &str) -> &'a str {
    d.fields.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The faces of the draft (anything unreadable: none).
fn faces(d: &Dialog) -> Vec<FaceRef> {
    d.fields.get("faces").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    match d.kind {
        DialogKind::IgnoreFaces => {
            Look { title: IGNORE_TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ignore", "Ignore", ButtonKind::Default)] }
        }
        _ => {
            let ok = Button::new("ok", "OK", ButtonKind::Default).enabled(!text(d, "name").trim().is_empty());
            Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), ok] }
        }
    }
}

/// `n faces`, `1 face`.
fn count(n: usize) -> String {
    if n == 1 { "1 face".into() } else { format!("{n} faces") }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    match d.kind {
        DialogKind::IgnoreFaces => {
            let n = faces(d).len();
            ui.add(Label::new(RichText::new(IGNORE_TEXT).font(fonts::regular(t.body)).color(t.text)).wrap());
            ui.add_space(4.0);
            let note = format!("{} {IGNORE_NOTE}", count(n));
            ui.add(Label::new(RichText::new(note).font(fonts::regular(t.small)).color(t.text_dim)).wrap());
            ui.add_space(10.0);
            let mut ticked = d.fields.get("dontAsk").and_then(Value::as_bool).unwrap_or(false);
            if widgets::checkbox(ui, &t, DONT_ASK_ID, DONT_ASK, &mut ticked).changed() {
                d.fields.insert("dontAsk".into(), json!(ticked));
            }
        }
        _ => {
            let ctx = ui.ctx().clone();
            // A dialog drawn anew puts the focus in Name, on its first two passes (the window takes
            // no input on its first, when egui sizes it).
            let seen = Id::new(NAME_ID).with("newPerson.passes");
            let pass = ctx.cumulative_pass_nr();
            let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
                Some((first, last)) if last.checked_add(1) == Some(pass) => first,
                _ => pass,
            };
            ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
            if pass.saturating_sub(first) < 2 {
                widgets::focus_field(&ctx, NAME_ID);
            }
            let form = Form::new(ui, &t, &[NAME_LABEL, NICKNAME_LABEL]);
            let width = (ui.available_width() - form.label_w - widgets::FORM_GAP).max(40.0);
            for (label, key, id, hint) in [(NAME_LABEL, "name", NAME_ID, ""), (NICKNAME_LABEL, "nickname", NICKNAME_ID, OPTIONAL)] {
                let mut value = text(d, key).to_owned();
                form.row(ui, &t, label, |ui| TextField::new(id, hint, width).show(ui, &t, &mut value));
                if let Some((at, _)) = value.char_indices().nth(MAX_DRAFT) {
                    value.truncate(at);
                }
                d.fields.insert(key.into(), Value::String(value));
            }
            let n = faces(d).len();
            if n > 0 {
                ui.add_space(2.0);
                ui.label(RichText::new(format!("Names {}.", count(n))).font(fonts::regular(t.small)).color(t.text_dim));
            }
        }
    }
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match (d.kind, action) {
        (DialogKind::NewPerson, "ok") => {
            let name = text(d, "name").trim().to_string();
            if name.is_empty() {
                return Err(CmdError::Disabled(NO_NAME.into()));
            }
            let nickname = text(d, "nickname").trim().to_string();
            let faces = faces(d);
            let old = crate::commands::faces::grid_now(app);
            let reply = if faces.is_empty() {
                let mut p = json!({ "name": name });
                if !nickname.is_empty() {
                    p["nickname"] = json!(nickname);
                }
                app.run("people.create", &p)?
            } else {
                let mut p = json!({ "faces": faces, "name": name, "create": true });
                if !nickname.is_empty() {
                    p["nickname"] = json!(nickname);
                }
                app.run("faces.name", &p)?
            };
            crate::commands::faces::settle(app, &old);
            Ok(reply)
        }
        (DialogKind::IgnoreFaces, "ignore") => {
            if faces(d).is_empty() {
                return Err(CmdError::Disabled(crate::commands::faces::NO_FACES.into()));
            }
            let old = crate::commands::faces::grid_now(app);
            let reply = app.run("faces.ignore", &json!({ "faces": faces(d) }))?;
            if d.fields.get("dontAsk") == Some(&Value::Bool(true))
                && let Some(s) = app.session_mut()
            {
                s.change_settings(|settings| settings.merge(SETTINGS_KEY, &[(ASK_KEY, Some(json!(false)))]));
            }
            crate::commands::faces::settle(app, &old);
            Ok(reply)
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
