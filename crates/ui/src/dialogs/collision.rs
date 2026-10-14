//! The collision question (`organizing.md` §9): a move or copy onto a folder that already has
//! files of the same names asks "The folder already has files with the same names." with [Rename
//! Duplicates] [Skip Duplicates] [Cancel], naming the clashing files. [`open`] asks it (M9.19 opens
//! it when `photo.moveTo` or `photo.copyTo` answers `needs_confirm` for a clash); its draft is the
//! view state's `collision {op, ids, folder, files}`. Rename Duplicates (the default) runs the
//! command again with `"collisions": "rename"` (the copies get `-001`, …), Skip Duplicates with
//! `"collisions": "skip"`; a move is sent with `"confirm": true` so it does not ask again.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{CollisionQuestion, FileOp};

/// The dialog's title, question and buttons.
pub const TITLE: &str = "Files with the Same Names";
pub const TEXT: &str = "The folder already has files with the same names.";
pub const RENAME: &str = "Rename Duplicates";
pub const SKIP: &str = "Skip Duplicates";
/// How many clashing names the question lists before it counts the rest.
pub const NAMED: usize = 5;

/// Asks what to do about the clash `q`. Replies `{dialog: "collision"}`.
pub fn open(app: &mut App, q: CollisionQuestion) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::Collision))?;
    app.state_mut().collision = Some(q);
    Ok(reply)
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

/// Rename Duplicates or Skip Duplicates: runs the move or copy again with the choice.
pub fn act(app: &mut App, _d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let q = app.state().collision.clone().ok_or_else(|| CmdError::Internal("the collision question has no draft".into()))?;
    let choice = if action == "skipDuplicates" { "skip" } else { "rename" };
    let (id, params) = run_params(&q, choice);
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
