//! Undo and redo. In the library they walk the library undo stack; the edit view's per-photo
//! history arrives with the edit view (M7) and the same command ids (`architecture.md` §4).

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
            params: "{}: undo the last library step",
            effect: Effect::Library,
            history: History::None,
            enabled: |s, _| if s.undo_stack().undo_label().is_some() { Ok(()) } else { Err(Disabled("Nothing to undo.".into())) },
            run: undo,
        },
        CommandSpec {
            id: "edit.redo",
            label: "Redo",
            menu: "Edit/Redo",
            keys: &[(Context::Library, "win,linux:Ctrl+Y"), (Context::Library, "mac:Ctrl+Shift+Z")],
            params: "{}: redo the last undone library step",
            effect: Effect::Library,
            history: History::None,
            enabled: |s, _| if s.undo_stack().redo_label().is_some() { Ok(()) } else { Err(Disabled("Nothing to redo.".into())) },
            run: redo,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn undo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    match s.undo()? {
        Some(label) => Ok(json!({ "undone": label })),
        None => Err(CmdError::Disabled("Nothing to undo.".into())),
    }
}

fn redo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
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
}
