//! The Options dialog's openers (`menus-and-shortcuts.md` §1.7, §1.8, §5): `app.preferences`
//! (Tools ▸ Options…, Ctrl+,; Dalica ▸ Preferences… on macOS) opens it on General, or on the tab
//! named; `app.shortcuts` (Help ▸ Keyboard Shortcuts…, Ctrl+/) opens it on Keyboard Shortcuts.
//! Both are Global keys, so they work in every view. Neither changes anything: what the dialog
//! changes is applied by its Apply or OK (`crate::dialogs::options`).
//!
//! `app.preferences.folder {key, path}` (no menu) puts a folder into the open dialog's draft for a
//! folder option (`export.location`): what a tab's Browse… folder picker runs with the folder
//! chosen, so over automation `ui.filePicker {path}` answers it (checked as a place to write). The
//! path is checked as the option's own check does (an absolute folder); like any change in the
//! dialog it is saved by Apply or OK, and choosing the saved folder again takes it out of the draft.

use std::path::PathBuf;

use dalica_engine::options::{Kind, find};
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, no_params, parse_object};
use crate::app::App;
use crate::dialogs::options;
use crate::dialogs::options::{file_types, general};
use crate::state::{MAX_DRAFT, OptionsTab};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "app.preferences",
        label: "Options…",
        menu: "Tools/Options…",
        keys: &[(Context::Global, "Ctrl+,")],
        params: r#"{"tab"?: "general"|"fileTypes"|"export"|"shortcuts"}: open Options on that tab, General by default (replies {dialog: "options"}; its draft is ui.state → options)"#,
        run: preferences,
    },
    UiCommand {
        id: "app.shortcuts",
        label: "Keyboard Shortcuts…",
        menu: "Help/Keyboard Shortcuts…",
        keys: &[(Context::Global, "Ctrl+/")],
        params: r#"{}: open Options on Keyboard Shortcuts, the shortcut editor (replies {dialog: "options"})"#,
        run: shortcuts,
    },
    UiCommand {
        id: "app.preferences.folder",
        label: "Choose Folder",
        menu: "",
        keys: &[],
        params: r#"{"key": string (a folder option, "export.location"), "path": string (absolute)}: put the folder into the open Options dialog's draft (what a tab's Browse… folder picker runs with the folder chosen; replies {key, path}); disabled while Options is closed"#,
        run: folder,
    },
];

/// Why a folder cannot go into the draft: the dialog is closed.
pub const NOT_OPEN: &str = "Options is not open.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tab {
    tab: Option<OptionsTab>,
}

fn preferences(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Tab = parse_object(params)?;
    options::open(app, p.tab.unwrap_or_default())
}

fn shortcuts(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    options::open(app, OptionsTab::Shortcuts)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Folder {
    key: String,
    path: PathBuf,
}

/// `app.preferences.folder {key, path}`: `path` into the draft for folder option `key`.
fn folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Folder = parse_object(params)?;
    if app.state().options.is_none() {
        return Err(CmdError::Disabled(NOT_OPEN.into()));
    }
    let key: String = p.key.chars().take(64).collect();
    let spec = find(&p.key).filter(|o| o.kind == Kind::Folder).ok_or_else(|| CmdError::BadParams(format!("key: `{key}` is no folder option")))?;
    if let Some(reason) = spec.unavailable {
        return Err(CmdError::Disabled(format!("{} cannot be changed in this build: {reason}", spec.key)));
    }
    let text = p.path.to_str().ok_or_else(|| CmdError::BadParams("path: not valid Unicode".into()))?.to_string();
    if text.chars().count() > MAX_DRAFT {
        return Err(CmdError::Limit(format!("path: at most {MAX_DRAFT} characters")));
    }
    let value = spec.check(&json!(text)).map_err(CmdError::BadParams)?;
    let opt = file_types::rows(app, spec.tab)
        .into_iter()
        .find(|o| o.row.key == spec.key)
        .ok_or_else(|| CmdError::BadParams(format!("key: `{key}` is not shown here")))?;
    general::set(app, &opt.row, value);
    Ok(json!({ "key": spec.key, "path": text }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn preferences_opens_on_the_tab_named_and_shortcuts_on_the_editor() {
        let mut app = App::new(None);
        assert_eq!(app.run("app.preferences", &json!({ "tab": "export" })).unwrap(), json!({ "dialog": "options" }));
        assert_eq!(app.state().options.as_ref().map(|o| o.tab), Some(OptionsTab::Export));
        assert!(matches!(app.run("app.shortcuts", &json!({})), Err(CmdError::Disabled(_))), "already open");
        crate::dialogs::cancel(&mut app).unwrap();
        app.run("app.shortcuts", &Value::Null).unwrap();
        assert_eq!(app.state().options.as_ref().map(|o| o.tab), Some(OptionsTab::Shortcuts));
        for bad in [json!({ "tab": "people" }), json!({ "tab": 1 })] {
            assert!(matches!(app.run("app.preferences", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }

    #[test]
    fn a_folder_goes_into_the_open_drafts_folder_option_and_nothing_else() {
        let mut app = App::new(None);
        let out = std::env::temp_dir().join("Exports");
        let path = out.to_str().unwrap();
        let p = json!({ "key": "export.location", "path": path });
        assert_eq!(app.run("app.preferences.folder", &p).unwrap_err(), CmdError::Disabled(NOT_OPEN.into()), "Options is closed");
        app.run("app.preferences", &json!({ "tab": "export" })).unwrap();
        for bad in [
            json!({ "key": "export.location", "path": "relative/place" }),
            json!({ "key": "export.size", "path": path }),
            json!({ "key": "no.such", "path": path }),
            json!({ "key": "export.location" }),
            json!({ "key": "export.location", "path": path, "more": 1 }),
            json!({ "key": 1, "path": path }),
        ] {
            assert!(matches!(app.run("app.preferences.folder", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(
            matches!(app.run("app.preferences.folder", &json!({ "key": "import.folder", "path": path })), Err(CmdError::Disabled(_))),
            "arrives with Import"
        );
        assert!(app.state().options.as_ref().unwrap().values.is_empty(), "nothing went into the draft");
        assert_eq!(app.run("app.preferences.folder", &p).unwrap(), json!({ "key": "export.location", "path": path }));
        assert_eq!(app.state().options.as_ref().unwrap().values.get("export.location"), Some(&json!(path)));
    }
}
