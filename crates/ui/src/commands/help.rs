//! The Help menu's windows (`menus-and-shortcuts.md` §1.8): `app.about` (Help ▸ About Dalica; on
//! macOS, Dalica ▸ About Dalica) and `help.releaseNotes` (Help ▸ Release Notes) open their dialogs
//! on the host. Neither changes anything; both close with Close or Esc.

use dalica_engine::CmdError;
use serde_json::Value;

use super::{UiCommand, no_params};
use crate::app::App;
use crate::dialogs::{self, about, release_notes};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "app.about",
        label: "About Dalica",
        menu: "Help/About Dalica",
        keys: &[],
        params: r#"{}: open About Dalica (replies {dialog: "about"}; its tab is ui.set {about: {tab}})"#,
        run: open_about,
    },
    UiCommand {
        id: "help.releaseNotes",
        label: "Release Notes",
        menu: "Help/Release Notes",
        keys: &[],
        params: r#"{}: open the release notes (replies {dialog: "releaseNotes"})"#,
        run: open_release_notes,
    },
];

fn open_about(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    dialogs::open(app, about::dialog())
}

fn open_release_notes(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    dialogs::open(app, release_notes::dialog())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn about_and_release_notes_open_once_and_need_no_library() {
        let mut app = App::new(None);
        assert_eq!(app.run("app.about", &json!({})).unwrap(), json!({ "dialog": "about" }));
        assert!(matches!(app.run("app.about", &json!({})), Err(CmdError::Disabled(_))), "already open");
        assert_eq!(app.run("help.releaseNotes", &Value::Null).unwrap(), json!({ "dialog": "releaseNotes" }));
        for bad in [json!({ "tab": "models" }), json!([1]), json!("x")] {
            assert!(matches!(app.run("help.releaseNotes", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert_eq!(app.state().dialogs.len(), 2);
        // Close (the Cancel button) closes the top one; there is no default button.
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))));
        assert_eq!(dialogs::answer(&mut app, Some("cancel")).unwrap(), json!({}));
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(dialogs::DialogKind::About));
    }
}
