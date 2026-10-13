//! Undo and redo (`architecture.md` §4). Without parameters they walk the library undo stack; with
//! `{id}` they walk that photo's own history, as `photo.history.undo` / `photo.history.redo` do (in
//! the edit view the UI passes the open photo).

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "edit.undo",
            label: "Undo",
            menu: "Edit/Undo",
            keys: &[(Context::Library, "Ctrl+Z")],
            params: r#"{} | {"id":PhotoId}: undo the last library step, or the photo's last edit"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match photo(p) {
                None if s.undo_stack().undo_label().is_none() => Err(Disabled("Nothing to undo.".into())),
                Some(id) if s.photo_history(id).is_some_and(|h| h.undo_label.is_none()) => Err(Disabled("Nothing to undo.".into())),
                _ => Ok(()),
            },
            run: undo,
        },
        CommandSpec {
            id: "edit.redo",
            label: "Redo",
            menu: "Edit/Redo",
            keys: &[(Context::Library, "win,linux:Ctrl+Y"), (Context::Library, "mac:Ctrl+Shift+Z")],
            params: r#"{} | {"id":PhotoId}: redo the last undone library step, or the photo's last undone edit"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match photo(p) {
                None if s.undo_stack().redo_label().is_none() => Err(Disabled("Nothing to redo.".into())),
                Some(id) if s.photo_history(id).is_some_and(|h| h.redo_label.is_none()) => Err(Disabled("Nothing to redo.".into())),
                _ => Ok(()),
            },
            run: redo,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    #[serde(default)]
    id: Option<PhotoId>,
}

/// The photo `p` names, if it names one (a malformed `id` is left to `run`, which reports it).
fn photo(p: &Value) -> Option<PhotoId> {
    p.get("id").and_then(Value::as_u64).map(PhotoId::new)
}

fn undo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Target { id } = params(p)?;
    if let Some(id) = id {
        return crate::cmd::history::undo_photo(s, id);
    }
    match s.undo()? {
        Some(label) => Ok(json!({ "undone": label })),
        None => Err(CmdError::Disabled("Nothing to undo.".into())),
    }
}

fn redo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Target { id } = params(p)?;
    if let Some(id) = id {
        return crate::cmd::history::redo_photo(s, id);
    }
    match s.redo()? {
        Some(label) => Ok(json!({ "redone": label })),
        None => Err(CmdError::Disabled("Nothing to redo.".into())),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn undo_and_redo_walk_the_stack_and_a_new_step_drops_redo() {
        let (_dirs, mut s, ids) = library(3);
        assert_eq!(s.execute("edit.undo", &json!({})), Err(CmdError::Disabled("Nothing to undo.".into())));
        assert_eq!(s.execute("edit.redo", &json!({})), Err(CmdError::Disabled("Nothing to redo.".into())));
        let m0 = s.library().model().clone();
        s.execute("photo.star.toggle", &json!({ "ids": [ids[0]] })).unwrap();
        let m1 = s.library().model().clone();
        s.execute("photo.star.toggle", &json!({ "ids": [ids[1]] })).unwrap();
        assert_eq!(s.execute("edit.undo", &json!({})).unwrap(), json!({ "undone": "Star / Unstar" }));
        assert_eq!(s.library().model(), &m1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.library().model(), &m0);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.library().model(), &m1);
        // A new command drops what could still be redone.
        s.execute("photo.star.toggle", &json!({ "ids": [ids[2]] })).unwrap();
        assert!(matches!(s.execute("edit.redo", &json!({})), Err(CmdError::Disabled(_))));
        hostile(&mut s, "edit.undo");
        hostile(&mut s, "edit.redo");
    }

    /// With `{id}` they walk the photo's own history and leave the library stack alone.
    #[test]
    fn with_a_photo_they_walk_its_history() {
        let (_dirs, mut s, ids) = library(2);
        s.execute("photo.star.toggle", &json!({ "ids": [ids[1]] })).unwrap();
        assert_eq!(s.execute("edit.undo", &json!({ "id": ids[0] })), Err(CmdError::Disabled("Nothing to undo.".into())));
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
        assert_eq!(s.execute("edit.undo", &json!({ "id": ids[0] })).unwrap(), json!({ "id": ids[0], "undone": "Auto Fix" }));
        assert!(s.library().info(ids[0]).edits.is_empty());
        assert_eq!(s.execute("edit.redo", &json!({ "id": ids[0] })).unwrap(), json!({ "id": ids[0], "redone": "Auto Fix" }));
        assert_eq!(s.execute("edit.redo", &json!({ "id": ids[0] })), Err(CmdError::Disabled("Nothing to redo.".into())));
        assert_eq!(s.undo_stack().len(), 1, "only the star");
        assert!(s.library().photo(ids[1]).unwrap().star);
        assert!(matches!(s.execute("edit.undo", &json!({ "id": 999 })), Err(CmdError::NotFound(_))));
        assert!(matches!(s.execute("edit.undo", &json!({ "id": "x" })), Err(CmdError::BadParams(_))));
    }
}
