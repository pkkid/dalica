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
//!
//! A folder move (Folder ▸ Move…'s answer, `folder.move {folder, path}`) asks the same question
//! ([`run_or_ask_folder`], op `moveFolder`) when the folder would merge into a library folder of
//! the same name in the place chosen and some of its photos' names are taken there: its buttons
//! run `folder.move` again with the choice, Cancel moves nothing. Each folder move that runs
//! remembers the place as the last one a folder moved into (`"organize"` → `lastMoveFolder`,
//! [`remember_move_folder`]), where Move Folder's picker opens next.

use std::path::{Path, PathBuf};

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogCommand, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{CollisionQuestion, FileOp, MAX_CLASHES};
use crate::{tl, tln};

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
        FileOp::MoveFolder => "folder.move",
    }
}

/// Runs `op` (`photo.moveTo` or `photo.copyTo`) with `params`, or asks the collision question first
/// when names of `params.ids` clash in `params.folder` and `params` makes no choice
/// (`collisions`). Parameters that do not read as photos and a folder go to the command as they
/// are, for it to refuse. Replies the command's reply, or `{dialog: "collision"}`.
pub fn run_or_ask(app: &mut App, op: FileOp, params: Value) -> Result<Value, CmdError> {
    if op == FileOp::MoveFolder {
        return run_or_ask_folder(app, params);
    }
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
    open_for(app, CollisionQuestion { op, ids, folder, files, to: None }, Some(params))
}

/// The place `params` of `folder.move` names (`path`, or `to` as a path or a library folder's id).
fn place(app: &App, params: &Value) -> Option<PathBuf> {
    let to = params.get("path").or_else(|| params.get("to"))?;
    match to {
        Value::String(p) => Some(PathBuf::from(p)),
        Value::Number(n) => app.session()?.library().folder(FolderId::new(n.as_u64()?)).map(|f| f.path.clone()),
        _ => None,
    }
}

/// Runs `folder.move` with `params` (`{folder, path}`), or asks the collision question first when
/// the folder would merge into a library folder of its name in that place and names of its photos
/// clash there, and `params` makes no choice (`collisions`). Parameters that do not read as a
/// folder and a place go to the command as they are, for it to refuse. A move that runs remembers
/// the place ([`remember_move_folder`]). Replies the command's reply, or `{dialog: "collision"}`.
pub fn run_or_ask_folder(app: &mut App, params: Value) -> Result<Value, CmdError> {
    let folder: Option<FolderId> = params.get("folder").and_then(|v| serde_json::from_value(v.clone()).ok());
    let to = place(app, &params);
    let (Some(folder), Some(to), None) = (folder, to.clone(), params.get("collisions")) else {
        let reply = app.run(command(FileOp::MoveFolder), &params)?;
        if let Some(to) = to {
            remember_move_folder(app, &to);
        }
        return Ok(reply);
    };
    let files: Vec<String> = app
        .session()
        .and_then(|s| {
            let lib = s.library();
            let name = lib.folder(folder)?.path.file_name()?.to_os_string();
            let dest = lib.folder_by_path(&to.join(name)).filter(|d| *d != folder)?;
            let ids: Vec<PhotoId> = lib.photos_in(folder).iter().map(|p| p.id).collect();
            Some(dalica_engine::cmd::files::clashing_names(s, &ids, dest, false))
        })
        .unwrap_or_default()
        .into_iter()
        .take(MAX_CLASHES)
        .collect();
    if files.is_empty() {
        let reply = app.run(command(FileOp::MoveFolder), &params)?;
        remember_move_folder(app, &to);
        return Ok(reply);
    }
    open_for(app, CollisionQuestion { op: FileOp::MoveFolder, ids: Vec::new(), folder, files, to: Some(to) }, Some(params))
}

/// Remembers `to` as the place a folder last moved into (`"organize"` → `lastMoveFolder`), where
/// Move Folder's picker opens next; a place that is not an absolute path is not kept.
pub fn remember_move_folder(app: &mut App, to: &Path) {
    let Some(s) = app.session_mut() else { return };
    if !to.is_absolute() || s.settings().organize().last_move_folder.as_deref() == Some(to) {
        return;
    }
    s.change_settings(|st| {
        let mut o = st.organize();
        o.last_move_folder = Some(to.to_path_buf());
        st.set_organize(&o);
    });
}

/// The names the question lists: the first [`NAMED`], then "and %d more".
pub fn named(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(NAMED).map(String::as_str).collect();
    let list = shown.join(&tl!(ctx = "list", ", "));
    match files.len().saturating_sub(NAMED) {
        0 => list,
        more => tln!(more, "{names} and {n} more", "{names} and {n} more", names = list),
    }
}

/// The command and parameters a button runs for `q`: `rename` or `skip`.
pub fn run_params(q: &CollisionQuestion, collisions: &str) -> (&'static str, Value) {
    match q.op {
        FileOp::Move => ("photo.moveTo", json!({ "ids": q.ids, "folder": q.folder, "collisions": collisions, "confirm": true })),
        FileOp::Copy => ("photo.copyTo", json!({ "ids": q.ids, "folder": q.folder, "collisions": collisions })),
        FileOp::MoveFolder => ("folder.move", json!({ "folder": q.folder, "path": q.to, "collisions": collisions })),
    }
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: tl!("Files with the Same Names"),
        width: confirm::WIDTH + 60.0,
        buttons: vec![
            Button::new("renameDuplicates", tl!("Rename Duplicates"), ButtonKind::Default),
            Button::new("skipDuplicates", tl!("Skip Duplicates"), ButtonKind::Other),
            Button::cancel(),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(tl!("The folder already has files with the same names.")).font(fonts::regular(t.body)).color(t.text)).wrap());
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
    let reply = app.run(id, &params)?;
    if let (FileOp::MoveFolder, Some(to)) = (q.op, &q.to) {
        remember_move_folder(app, to);
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use dalica_core::{FolderId, PhotoId};

    use super::*;
    use crate::dialogs;
    use crate::state::DialogKind;

    #[test]
    fn the_collision_question_names_the_files_and_runs_each_choice() {
        let files: Vec<String> = (1..=7).map(|i| format!("IMG_{i}.jpg")).collect();
        assert_eq!(named(&files[..2]), "IMG_1.jpg, IMG_2.jpg");
        assert_eq!(named(&files), "IMG_1.jpg, IMG_2.jpg, IMG_3.jpg, IMG_4.jpg, IMG_5.jpg and 2 more");
        let q = CollisionQuestion { op: FileOp::Move, ids: vec![PhotoId::new(1)], folder: FolderId::new(3), files: files.clone(), to: None };
        assert_eq!(run_params(&q, "skip"), ("photo.moveTo", json!({ "ids": [1], "folder": 3, "collisions": "skip", "confirm": true })));
        let copy = CollisionQuestion { op: FileOp::Copy, ..q.clone() };
        assert_eq!(run_params(&copy, "rename"), ("photo.copyTo", json!({ "ids": [1], "folder": 3, "collisions": "rename" })));
        let folder = CollisionQuestion { op: FileOp::MoveFolder, ids: vec![], to: Some(PathBuf::from("/photos/2024")), ..q.clone() };
        assert_eq!(run_params(&folder, "skip"), ("folder.move", json!({ "folder": 3, "path": "/photos/2024", "collisions": "skip" })));
        assert_eq!(command(FileOp::MoveFolder), "folder.move");
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

    fn put_jpeg(path: &Path, seed: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 80, None).unwrap()).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".jpg")).collect();
        v.sort();
        v
    }

    /// A folder moved by Folder ▸ Move… onto a place holding a folder of its name whose photos'
    /// names clash asks the collision question (not the engine's refusal); Rename Duplicates merges
    /// with new names, and the place is remembered as the last one a folder moved into, as soon as
    /// the move runs. A move without a clash runs at once and is remembered too.
    #[test]
    fn a_folder_merge_with_clashing_names_asks_and_the_place_is_remembered() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        let (a, b, c) = (photos.join("A").join("Trip"), photos.join("B").join("Trip"), photos.join("C").join("Other"));
        put_jpeg(&a.join("IMG_1.jpg"), 1);
        put_jpeg(&a.join("IMG_2.jpg"), 2);
        put_jpeg(&b.join("IMG_1.jpg"), 3);
        put_jpeg(&c.join("IMG_9.jpg"), 4);
        let mut s = dalica_engine::Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.execute("folders.add", &json!({ "path": photos })).unwrap();
        s.wait_idle();
        let (a_id, c_id) = (s.library().folder_by_path(&a).unwrap(), s.library().folder_by_path(&c).unwrap());
        let picker = Arc::new(crate::picker::ScriptedPicker::new());
        let mut d = crate::headless::Driver::new(App::with_picker(Some(s), picker.clone()), 900, 600, 1.0);
        d.settle(3600).unwrap();
        let last = |d: &crate::headless::Driver| d.app().session().unwrap().settings().organize().last_move_folder;
        assert_eq!(last(&d), None);

        // Folder ▸ Move… for A/Trip, answered with B: the question, naming the clash.
        picker.push_folder(photos.join("B"));
        let open = crate::commands::folder_ops::MOVE.open;
        open(d.app_mut(), &json!({ "folder": a_id })).unwrap();
        d.settle(3600).unwrap();
        assert_eq!(d.app().state().dialogs.last().map(|x| x.kind), Some(DialogKind::Collision), "{:?}", d.app().state().status);
        let q = d.app().state().collision.clone().unwrap();
        assert_eq!(
            (q.op, q.folder, q.files.clone(), q.to.clone()),
            (FileOp::MoveFolder, a_id, vec!["IMG_1.jpg".to_string()], Some(photos.join("B")))
        );
        assert_eq!(names(&a), ["IMG_1.jpg", "IMG_2.jpg"], "nothing moved yet");
        assert_eq!(last(&d), None, "nothing remembered before the move runs");
        // Cancel moves nothing.
        dialogs::cancel(d.app_mut()).unwrap();
        assert_eq!(names(&b), ["IMG_1.jpg"]);
        assert_eq!(last(&d), None);
        // Asked again, Rename Duplicates merges with a new name.
        picker.push_folder(photos.join("B"));
        open(d.app_mut(), &json!({ "folder": a_id })).unwrap();
        d.settle(3600).unwrap();
        dialogs::answer(d.app_mut(), Some("renameDuplicates")).unwrap();
        assert_eq!(last(&d), Some(photos.join("B")), "remembered as the move runs");
        assert!(d.settle_for(3600, true).settled);
        assert_eq!(names(&b), ["IMG_1-001.jpg", "IMG_1.jpg", "IMG_2.jpg"]);

        // No clash: the move runs at once, and its place is remembered.
        let reply = run_or_ask_folder(d.app_mut(), json!({ "folder": c_id, "path": photos.join("A") })).unwrap();
        assert!(reply.get("job").is_some(), "{reply}");
        assert_eq!(last(&d), Some(photos.join("A")));
        assert!(d.settle_for(3600, true).settled);
        assert_eq!(names(&photos.join("A").join("Other")), ["IMG_9.jpg"]);
        // Parameters that do not read as a folder and a place go to the command, which refuses.
        assert!(run_or_ask_folder(d.app_mut(), json!({ "folder": "x" })).is_err());
        assert!(run_or_ask_folder(d.app_mut(), json!({ "folder": c_id, "to": 99_999 })).is_err());
        assert_eq!(last(&d), Some(photos.join("A")));
    }
}
