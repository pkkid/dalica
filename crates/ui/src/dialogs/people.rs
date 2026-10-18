//! The person menu's dialogs (`organizing.md` §6.3), opened by the person menu's rows
//! (`crate::commands::people`) on a person's tree row or section header:
//!
//! - **Rename Person**: Name (`field:dialog.name`, the person's name, focused) and Nickname
//!   (`field:dialog.nickname`, optional), [OK] [Cancel]; OK (greyed while the name is empty) runs
//!   `people.rename {person, name, nickname}`. A name another person has (ignoring case) asks to
//!   merge instead: the merge question for the two takes Rename's place.
//! - **Merge People**: "Merge Ana into Rui? Ana's faces become Rui's." [Merge] [Cancel]; Merge runs
//!   `people.merge {person, into}` (the target's name is kept).
//! - **Delete Person**: "Delete Ana? Their faces go back to Unnamed." [Delete] [Cancel]; Delete runs
//!   `people.delete {person, confirm: true}`.
//!
//! Each is one library undo step. The drafts (`dialogs[].fields`) are `person`, Rename's `name` and
//! `nickname`, Merge's `into`. When a person goes (merged, deleted), their section leaves the
//! selection and Zoom to Faces forgets them.

use dalica_core::PersonId;
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind, Form, TextField};
use egui::{Id, Label, RichText, Ui};
use serde_json::{Map, Value, json};

use super::new_person::{NAME_ID, NAME_LABEL, NICKNAME_ID, NICKNAME_LABEL, OPTIONAL};
use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::{MAX_DRAFT, Section};

pub const RENAME_TITLE: &str = "Rename Person";
pub const MERGE_TITLE: &str = "Merge People";
pub const DELETE_TITLE: &str = "Delete Person";
/// The dialogs' width (points).
pub const WIDTH: f32 = 420.0;
/// Why a button refuses with no person in the draft.
pub const NO_PERSON: &str = "Choose a person first.";
/// Why OK is greyed with no name.
pub const NO_NAME: &str = "Type the person's name.";

/// A dialog of `kind` (one of this module's) with `draft`.
pub fn dialog(kind: DialogKind, draft: Map<String, Value>) -> Dialog {
    let mut d = Dialog::new(kind);
    d.fields = draft;
    d
}

/// "Merge Ana into Rui? Ana's faces become Rui's."
pub fn merge_text(person: &str, into: &str) -> String {
    format!("Merge {person} into {into}? {person}'s faces become {into}'s.")
}

/// "Delete Ana? Their faces go back to Unnamed."
pub fn delete_text(person: &str) -> String {
    format!("Delete {person}? Their faces go back to Unnamed.")
}

/// The default button of each kind: its action and label.
fn default_button(kind: DialogKind) -> (&'static str, &'static str) {
    match kind {
        DialogKind::MergePeople => ("merge", "Merge"),
        DialogKind::DeletePerson => ("delete", "Delete"),
        _ => ("ok", "OK"),
    }
}

fn text<'a>(d: &'a Dialog, key: &str) -> &'a str {
    d.fields.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The person under `key` of the draft, if it is one.
fn person(d: &Dialog, key: &str) -> Option<PersonId> {
    PersonId::parse(text(d, key)).ok()
}

/// The name of `person` (their id while the library does not know them).
fn name_of(app: &App, person: Option<&PersonId>) -> String {
    let Some(id) = person else { return String::new() };
    app.session().and_then(|s| s.library().person(id).map(|p| p.name.clone())).unwrap_or_else(|| id.to_string())
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let title = match d.kind {
        DialogKind::MergePeople => MERGE_TITLE,
        DialogKind::DeletePerson => DELETE_TITLE,
        _ => RENAME_TITLE,
    };
    let (action, label) = default_button(d.kind);
    let ok = Button::new(action, label, ButtonKind::Default);
    let ok = if d.kind == DialogKind::RenamePerson { ok.enabled(!text(d, "name").trim().is_empty()) } else { ok };
    Look { title: title.into(), width: WIDTH, buttons: vec![ok, Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let line = |ui: &mut Ui, s: String| {
        ui.add(Label::new(RichText::new(s).font(fonts::regular(t.body)).color(t.text)).wrap());
    };
    match d.kind {
        DialogKind::MergePeople => {
            let (who, into) = (name_of(app, person(d, "person").as_ref()), name_of(app, person(d, "into").as_ref()));
            line(ui, merge_text(&who, &into));
        }
        DialogKind::DeletePerson => line(ui, delete_text(&name_of(app, person(d, "person").as_ref()))),
        _ => {
            let ctx = ui.ctx().clone();
            // Name takes the focus on the dialog's first two passes (the window takes no input on
            // its first, when egui sizes it).
            let seen = Id::new(NAME_ID).with("renamePerson.passes");
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
        }
    }
}

/// After `person` went (merged away, deleted): their section leaves the selection, and Zoom to
/// Faces forgets them.
fn forget(app: &mut App, person: &PersonId) {
    let state = app.state_mut();
    state.people.zoom_to_faces.retain(|p| p != person);
    if state.selection.section.and_then(Section::person).as_ref() == Some(person) {
        state.selection = crate::state::Selection::default();
    }
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    if action != default_button(d.kind).0 {
        return Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())));
    }
    let who = person(d, "person").ok_or_else(|| CmdError::Disabled(NO_PERSON.into()))?;
    match d.kind {
        DialogKind::MergePeople => {
            let into = person(d, "into").ok_or_else(|| CmdError::Disabled(NO_PERSON.into()))?;
            let reply = app.run("people.merge", &json!({ "person": who, "into": into }))?;
            forget(app, &who);
            Ok(reply)
        }
        DialogKind::DeletePerson => {
            let reply = app.run("people.delete", &json!({ "person": who, "confirm": true }))?;
            forget(app, &who);
            Ok(reply)
        }
        _ => {
            let name = text(d, "name").trim().to_string();
            if name.is_empty() {
                return Err(CmdError::Disabled(NO_NAME.into()));
            }
            // Another person's name: ask to merge into them instead.
            let holder = app.session().and_then(|s| s.library().person_by_name(&name).cloned()).filter(|h| *h != who);
            if let Some(into) = holder {
                return dialogs_open_merge(app, &who, &into);
            }
            app.run("people.rename", &json!({ "person": who, "name": name, "nickname": text(d, "nickname").trim() }))
        }
    }
}

fn dialogs_open_merge(app: &mut App, who: &PersonId, into: &PersonId) -> Result<Value, CmdError> {
    super::open(app, dialog(DialogKind::MergePeople, Map::from_iter([("person".into(), json!(who)), ("into".into(), json!(into))])))
}
