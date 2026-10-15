//! The collision question (`organizing.md` §9): a move or copy onto a folder that already has
//! files of the same names asks "The folder already has files with the same names." with [Rename
//! Duplicates] [Skip Duplicates] [Cancel], naming the clashing files (the first [`NAMED`], then "and
//! %d more"). Its draft is the view state's `collision {op, ids, folder, files}`.
//!
//! [`run_or_ask`] is how the UI runs a move or a copy: it asks the engine which names clash
//! (`dalica_engine::cmd::files::clashing_names`, from the model, no disk) and opens this question
//! when some do and no choice was sent, else runs the command. The first-time move question's Move
//! (`super::move_confirm`), the copy question's Copy (`super::copy_confirm`) and
//! `crate::commands::files::{move_photos, copy_photos}` go through it, so a clash always asks before
//! anything moves. The dialog keeps the call it interrupts (`command`: the move's `"confirm": true`
//! and its "Don't ask again" as `"dontAsk": true`), and its buttons run that call again with the
//! choice: Rename Duplicates (the default) with `"collisions": "rename"` (the newcomers get `-001`,
//! …), Skip Duplicates with `"collisions": "skip"` (those photos stay where they are). Cancel runs
//! nothing: nothing moves, and a "Don't ask again" ticked under it is not applied.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogCommand, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{CollisionQuestion, FileOp, MAX_CLASHES};

/// The dialog's title, question and buttons.
pub const TITLE: &str = "Files with the Same Names";
pub const TEXT: &str = "The folder already has files with the same names.";
pub const RENAME: &str = "Rename Duplicates";
pub const SKIP: &str = "Skip Duplicates";
/// How many clashing names the question lists before it counts the rest.
pub const NAMED: usize = 5;

/// Asks what to do about the clash `q`. Replies `{dialog: "collision"}`.
pub fn open(app: &mut App, q: CollisionQuestion) -> Result<Value, CmdError> {
    open_for(app, q, None)
}

/// [`open`], keeping the call it interrupts (`params` of `photo.moveTo` or `photo.copyTo`) to run
/// again with the choice.
fn open_for(app: &mut App, q: CollisionQuestion, params: Option<Value>) -> Result<Value, CmdError> {
    let command = params.map(|params| DialogCommand { id: command(q.op).into(), params });
    let reply = super::open(app, Dialog { command, ..Dialog::new(DialogKind::Collision) })?;
    app.state_mut().collision = Some(q);
    Ok(reply)
}

/// The engine command of `op`.
pub fn command(op: FileOp) -> &'static str {
    match op {
        FileOp::Move => "photo.moveTo",
        FileOp::Copy => "photo.copyTo",
    }
}

/// Runs `op` (`photo.moveTo` or `photo.copyTo`) with `params`, or asks the collision question first
/// when names of `params.ids` clash in `params.folder` and `params` makes no choice
/// (`collisions`). Parameters that do not read as photos and a folder go to the command as they
/// are, for it to refuse. Replies the command's reply, or `{dialog: "collision"}`.
pub fn run_or_ask(app: &mut App, op: FileOp, params: Value) -> Result<Value, CmdError> {
    let ids: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    let folder: Option<FolderId> = params.get("folder").and_then(|v| serde_json::from_value(v.clone()).ok());
    let (Some(ids), Some(folder), None) = (ids, folder, params.get("collisions")) else { return app.run(command(op), &params) };
    let files: Vec<String> = app
        .session()
        .map(|s| dalica_engine::cmd::files::clashing_names(s, &ids, folder, op == FileOp::Copy))
        .unwrap_or_default()
        .into_iter()
        .take(MAX_CLASHES)
        .collect();
    if files.is_empty() {
        return app.run(command(op), &params);
    }
    open_for(app, CollisionQuestion { op, ids, folder, files }, Some(params))
}

/// The names the question lists: the first [`NAMED`], then "and %d more".
pub fn named(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(NAMED).map(String::as_str).collect();
    match files.len().saturating_sub(NAMED) {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

/// The command and parameters a button runs for `q`: `rename` or `skip`.
pub fn run_params(q: &CollisionQuestion, collisions: &str) -> (&'static str, Value) {
    match q.op {
        FileOp::Move => ("photo.moveTo", json!({ "ids": q.ids, "folder": q.folder, "collisions": collisions, "confirm": true })),
        FileOp::Copy => ("photo.copyTo", json!({ "ids": q.ids, "folder": q.folder, "collisions": collisions })),
    }
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: confirm::WIDTH + 60.0,
        buttons: vec![
            Button::new("renameDuplicates", RENAME, ButtonKind::Default),
            Button::new("skipDuplicates", SKIP, ButtonKind::Other),
            Button::cancel(),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(TEXT).font(fonts::regular(t.body)).color(t.text)).wrap());
    if let Some(q) = &app.state().collision
        && !q.files.is_empty()
    {
        ui.add_space(6.0);
        ui.add(Label::new(RichText::new(named(&q.files)).font(fonts::regular(t.body)).color(t.text_label)).wrap());
    }
}

/// Rename Duplicates or Skip Duplicates: runs the move or copy again with the choice, and with the
/// rest of the call it interrupted (a move's "Don't ask again").
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let q = app.state().collision.clone().ok_or_else(|| CmdError::Internal("the collision question has no draft".into()))?;
    let choice = if action == "skipDuplicates" { "skip" } else { "rename" };
    let (id, mut params) = run_params(&q, choice);
    if let (Some(Value::Object(kept)), Some(p)) = (d.command.as_ref().filter(|c| c.id == id).map(|c| &c.params), params.as_object_mut()) {
        for (k, v) in kept {
            p.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    app.run(id, &params)
}

#[cfg(test)]
mod tests {
    use dalica_core::{FolderId, PhotoId};

    use super::*;
    use crate::dialogs;

    #[test]
    fn the_collision_question_names_the_files_and_runs_each_choice() {
        let files: Vec<String> = (1..=7).map(|i| format!("IMG_{i}.jpg")).collect();
        assert_eq!(named(&files[..2]), "IMG_1.jpg, IMG_2.jpg");
        assert_eq!(named(&files), "IMG_1.jpg, IMG_2.jpg, IMG_3.jpg, IMG_4.jpg, IMG_5.jpg and 2 more");
        let q = CollisionQuestion { op: FileOp::Move, ids: vec![PhotoId::new(1)], folder: FolderId::new(3), files: files.clone() };
        assert_eq!(run_params(&q, "skip"), ("photo.moveTo", json!({ "ids": [1], "folder": 3, "collisions": "skip", "confirm": true })));
        let copy = CollisionQuestion { op: FileOp::Copy, ..q.clone() };
        assert_eq!(run_params(&copy, "rename"), ("photo.copyTo", json!({ "ids": [1], "folder": 3, "collisions": "rename" })));
        let mut app = App::new(None);
        open(&mut app, q.clone()).unwrap();
        assert_eq!(app.state().collision, Some(q));
        let v = dialogs::inspect(&app);
        assert_eq!(v["buttons"], json!(["button:dialog.renameDuplicates", "button:dialog.skipDuplicates", "button:dialog.cancel"]));
        assert_eq!(v["default"], "button:dialog.renameDuplicates");
        // No library: each choice fails and the question stays; Cancel closes it.
        assert!(dialogs::answer(&mut app, Some("skipDuplicates")).is_err());
        assert_eq!(app.state().dialogs.len(), 1);
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().collision, None);
    }
}
