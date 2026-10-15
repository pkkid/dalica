//! The Options dialog's openers (`menus-and-shortcuts.md` §1.7, §1.8, §5): `app.preferences`
//! (Tools ▸ Options…, Ctrl+,; Dalica ▸ Preferences… on macOS) opens it on General, or on the tab
//! named; `app.shortcuts` (Help ▸ Keyboard Shortcuts…, Ctrl+/) opens it on Keyboard Shortcuts.
//! Both are Global keys, so they work in every view. Neither changes anything: what the dialog
//! changes is applied by its Apply or OK (`crate::dialogs::options`).

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::Value;

use super::{UiCommand, no_params, parse_object};
use crate::app::App;
use crate::dialogs::options;
use crate::state::OptionsTab;

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
];

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
}
