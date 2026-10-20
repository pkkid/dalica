//! Rename Tag (the Tags pane's tag menu, `menus-and-shortcuts.md` §4, `organizing.md` §5.2):
//! opened by `tag.rename {tag}`. The draft is `{from, to}`: the tag and its new name (starting as
//! the tag). The dialog is the confirmation the rename asks for: it says how many photos the
//! rename touches ("Rename “beach” on 12 photos to:"), takes the new name in `field:dialog.to`
//! (focused with its text selected, so typing replaces it) and, when the new name is a tag already
//! in use, says the two will merge. Rename (`button:dialog.ok`, Enter) runs `tags.rename {from, to,
//! confirm: true}` once: one undo step renaming the tag on every photo, merging onto the existing
//! tag. It is greyed while the name is empty, unchanged or longer than a tag may be.

use dalica_engine::CmdError;
use dalica_library::MAX_TAG;
use dalica_library::tags::fold;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind, TextField};
use egui::{Id, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::MAX_DRAFT;
use crate::{tl, tln};

pub const TITLE: &str = "Rename Tag";
/// The new name's field.
pub const FIELD_ID: &str = "field:dialog.to";
/// The default button's label.
pub const RENAME: &str = "Rename";
/// The dialog's width (points).
pub const WIDTH: f32 = 360.0;

/// The dialog renaming `tag`.
pub fn dialog(tag: &str) -> Dialog {
    let mut d = Dialog::new(DialogKind::RenameTag);
    if let Value::Object(o) = json!({ "from": tag, "to": tag }) {
        d.fields = o;
    }
    d
}

fn from(d: &Dialog) -> &str {
    d.fields.get("from").and_then(Value::as_str).unwrap_or("")
}

/// The new name in the draft, trimmed.
fn to(d: &Dialog) -> &str {
    d.fields.get("to").and_then(Value::as_str).unwrap_or("").trim()
}

/// Why the draft cannot be renamed yet, if it cannot.
pub fn refused(d: &Dialog) -> Option<String> {
    refusal(d).map(|r| r.text(false))
}

/// Why Rename is greyed.
#[derive(Clone, Copy)]
enum Refusal {
    Empty,
    Same,
    TooLong,
}

impl Refusal {
    /// In English (a refused Rename answers with it), or in the language in effect (`shown`).
    fn text(self, shown: bool) -> String {
        match (self, shown) {
            (Refusal::Empty, false) => "Type the tag's new name.".into(),
            (Refusal::Empty, true) => tl!("Type the tag's new name."),
            (Refusal::Same, false) => "The new name is the same.".into(),
            (Refusal::Same, true) => tl!("The new name is the same."),
            (Refusal::TooLong, false) => format!("A tag is at most {MAX_TAG} bytes."),
            (Refusal::TooLong, true) => tl!("A tag is at most {max} bytes.", max = MAX_TAG),
        }
    }
}

fn refusal(d: &Dialog) -> Option<Refusal> {
    let to = to(d);
    if to.is_empty() {
        Some(Refusal::Empty)
    } else if to == from(d).trim() {
        Some(Refusal::Same)
    } else if to.len() > MAX_TAG {
        Some(Refusal::TooLong)
    } else {
        None
    }
}

/// What the dialog says: how many photos hold the tag, and whether the new name merges it into a
/// tag in use (that tag's spelling and its count).
pub fn texts(app: &App, d: &Dialog) -> (String, Option<String>) {
    let index = app.session().map(|s| s.library().tag_index());
    let (from, to) = (from(d), to(d));
    let holders = index.map_or(0, |i| i.count(from));
    let question = tln!(holders, "Rename “{tag}” on {n} photo to:", "Rename “{tag}” on {n} photos to:", tag = from);
    let merge = index.filter(|_| fold(from) != fold(to)).and_then(|i| {
        let spelling = i.spelling(to)?;
        Some(tln!(
            i.count(to),
            "“{tag}” is already on {n} photo: the two tags will become one.",
            "“{tag}” is already on {n} photos: the two tags will become one.",
            tag = spelling
        ))
    });
    (question, merge)
}

/// Rename is greyed while [`refused`] says why.
pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", tl!(ctx = "verb", "Rename"), ButtonKind::Default).enabled(refused(d).is_none());
    Look { title: tl!("Rename Tag"), width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let (question, merge) = texts(app, d);
    ui.label(RichText::new(question).font(fonts::regular(t.body)).color(t.text_label));
    ui.add_space(6.0);
    // A dialog drawn anew (not drawn last pass) puts the focus in its field. The window cannot
    // take input on its first pass (egui sizes it then), so the focus is asked for on its first
    // two passes.
    let seen = Id::new(FIELD_ID).with("rename.passes");
    let pass = ctx.cumulative_pass_nr();
    let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
        Some((first, last)) if last.checked_add(1) == Some(pass) => first,
        _ => pass,
    };
    ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
    if pass.saturating_sub(first) < 2 {
        widgets::focus_field(&ctx, FIELD_ID);
    }
    let mut text = d.fields.get("to").and_then(Value::as_str).unwrap_or("").to_owned();
    TextField::new(FIELD_ID, &tl!("New name"), ui.available_width()).select_on_focus(true).show(ui, &t, &mut text);
    if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
        text.truncate(at);
    }
    d.fields.insert("to".into(), Value::String(text));
    let note = merge.or_else(|| refusal(d).filter(|_| to(d).len() > MAX_TAG).map(|r| r.text(true)));
    if let Some(note) = note {
        ui.add_space(6.0);
        ui.label(RichText::new(note).font(fonts::regular(t.small)).color(t.text_dim));
    }
}

/// Rename: `tags.rename {from, to, confirm: true}` (this dialog is the question it asks).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    if let Some(why) = refused(d) {
        return Err(CmdError::Disabled(why));
    }
    app.run("tags.rename", &json!({ "from": from(d), "to": to(d), "confirm": true }))
}
