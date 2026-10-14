//! The photo's own history (`architecture.md` §4, `editing.md` §5.1): `photo.history.apply`,
//! `photo.history.undo`, `photo.history.redo` and the `photo.history.state` query (which names
//! Undo Save for a saved photo with no newer edits, M8.15). Each takes the
//! photo explicitly; in the edit view the UI routes Edit ▸ Undo / Redo, the rotations, the flips
//! and the one-click fixes here for the open photo. A photo step is never a library undo step.
//! The rules (merging, the redo list, the labels) are in [`crate::history`].

use dalica_core::PhotoId;
use dalica_edit::Op as EditOp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::history::{Change, PhotoHistory};
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.history.apply",
            label: "Apply Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "op":"name(params)"}: one step of the photo's own history: the operation appended to its recipe (a rotation right after a rotation merges into it, the same flip twice cancels), its redo list cleared; answers the history as photo.history.state does"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: crate::registry::always,
            run: apply,
        },
        CommandSpec {
            id: "photo.history.undo",
            label: "Undo Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: undo the photo's last edit (it moves to the redo list; a merged step gives back the entry it merged into)"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match target(s, p) {
                Some(h) if h.undo_label.is_none() => Err(Disabled("Nothing to undo.".into())),
                _ => Ok(()),
            },
            run: undo,
        },
        CommandSpec {
            id: "photo.history.redo",
            label: "Redo Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: redo the photo's last undone edit"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match target(s, p) {
                Some(h) if h.redo_label.is_none() => Err(Disabled("Nothing to redo.".into())),
                _ => Ok(()),
            },
            run: redo,
        },
        CommandSpec {
            id: "photo.history.state",
            label: "Photo History",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: the photo's recipe ("edits") and redo list, whether each way is possible, and the button labels ("Undo Crop", "Redo Tuning"; "Undo" / "Redo" when there is nothing); a saved photo with no newer edits answers "undoLabel": "Undo Save", "canUndo": true and "undoSave": true (its Undo runs photo.undoSave)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: state,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    id: PhotoId,
    op: String,
}

/// The history of the photo `p` names, when `p` names a known photo; anything else is left to
/// `run`, which reports it.
fn target(s: &Session, p: &Value) -> Option<PhotoHistory> {
    let id = p.get("id").and_then(Value::as_u64)?;
    s.photo_history(PhotoId::new(id))
}

/// Photo `id`'s history, `NotFound` for an unknown photo.
pub(crate) fn history_of(s: &Session, id: PhotoId) -> Result<PhotoHistory, CmdError> {
    s.photo_history(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))
}

/// What `photo.history.state` answers.
pub(crate) fn state_json(id: PhotoId, h: &PhotoHistory) -> Value {
    json!({
        "id": id,
        "edits": h.edits,
        "redo": h.redo,
        "canUndo": h.undo_label.is_some(),
        "canRedo": h.redo_label.is_some(),
        "undoLabel": h.undo_text(),
        "redoLabel": h.redo_text(),
    })
}

/// The operation `text` stands for, when Dalica knows it; legacy and unknown text is refused (only
/// a sidecar may hold it).
fn operation(text: &str) -> Result<EditOp, CmdError> {
    if text.len() > dalica_library::MAX_OPERATION {
        return Err(CmdError::Limit(format!("an operation is at most {} bytes", dalica_library::MAX_OPERATION)));
    }
    let op = EditOp::parse(text);
    if !op.is_known() {
        let shown: String = text.chars().take(64).collect();
        return Err(CmdError::BadParams(format!("op: not an operation Dalica applies: {shown:?}")));
    }
    Ok(op)
}

fn apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Apply { id, op } = params(p)?;
    history_of(s, id)?;
    let op = operation(&op)?;
    s.photo_step(id, Change::Append(op), None)?;
    Ok(state_json(id, &history_of(s, id)?))
}

fn undo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    undo_photo(s, id)
}

/// Undoes photo `id`'s last edit (also `edit.undo {id}`).
pub(crate) fn undo_photo(s: &mut Session, id: PhotoId) -> Result<Value, CmdError> {
    history_of(s, id)?;
    match s.photo_undo(id)? {
        Some(label) => Ok(json!({ "id": id, "undone": label })),
        None => Err(CmdError::Disabled("Nothing to undo.".into())),
    }
}

fn redo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    redo_photo(s, id)
}

/// Redoes photo `id`'s last undone edit (also `edit.redo {id}`).
pub(crate) fn redo_photo(s: &mut Session, id: PhotoId) -> Result<Value, CmdError> {
    history_of(s, id)?;
    match s.photo_redo(id)? {
        Some(label) => Ok(json!({ "id": id, "redone": label })),
        None => Err(CmdError::Disabled("Nothing to redo.".into())),
    }
}

fn state(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    let h = history_of(s, id)?;
    let mut v = state_json(id, &h);
    // A saved photo without newer edits: Undo reads "Undo Save" and runs `photo.undoSave`
    // (`editing.md` §5.1), which `photo.history.undo` does not.
    if h.edits.is_empty() && s.library().can_undo_save(id) {
        v["canUndo"] = json!(true);
        v["undoLabel"] = json!(UNDO_SAVE);
        v["undoSave"] = json!(true);
    }
    Ok(v)
}

/// The Undo button's label for a saved photo whose edits are all saved.
pub const UNDO_SAVE: &str = "Undo Save";

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn disabled_unknown_and_hostile_params() {
        let (_dirs, mut s, ids) = library(1);
        let id = ids[0];
        assert_eq!(s.execute("photo.history.undo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to undo.".into())));
        assert_eq!(s.execute("photo.history.redo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to redo.".into())));
        for cmd in ["photo.history.undo", "photo.history.redo", "photo.history.state"] {
            assert!(matches!(s.execute(cmd, &json!({ "id": 999 })), Err(CmdError::NotFound(_))), "{cmd}");
            assert!(matches!(s.execute(cmd, &json!({})), Err(CmdError::BadParams(_))), "{cmd}");
            assert!(matches!(s.execute(cmd, &json!({ "id": "x" })), Err(CmdError::BadParams(_))), "{cmd}");
            hostile(&mut s, cmd);
        }
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": 999, "op": "sepia()" })), Err(CmdError::NotFound(_))));
        for op in ["", "future_op(1)", "legacy:redeye(1)", "rotate(45)", "sepia(", "rotate(090)"] {
            assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": op })), Err(CmdError::BadParams(_))), "{op}");
        }
        let long = format!("sharpen({})", "1".repeat(dalica_library::MAX_OPERATION));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": long })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": 7 })), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id })), Err(CmdError::BadParams(_))));
        hostile(&mut s, "photo.history.apply");
        assert!(s.library().info(id).is_empty());
    }

    /// A recipe at the cap takes no new entry; a rotation that merges into the last one still
    /// does.
    #[test]
    fn a_full_recipe_is_a_limit_and_changes_nothing() {
        let (_dirs, mut s, ids) = library(1);
        let mut edits = vec!["future_op()".to_string(); dalica_edit::MAX_OPERATIONS - 1];
        edits.push("rotate(90)".into());
        s.apply(dalica_library::Op::SetRecipes { recipes: vec![(ids[0], dalica_library::Recipe { edits, redo: vec![] })] }).unwrap();
        let model = s.library().model().clone();
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()" })), Err(CmdError::Limit(_))));
        assert_eq!(s.library().model(), &model);
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
        assert_eq!(s.library().info(ids[0]).edits.last().map(String::as_str), Some("rotate(180)"));
    }
}
