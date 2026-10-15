//! View commands that live in the engine because they change what the library shows or how it is
//! decoded (View menu).
//!
//! - `view.colorManagement` (View → Use Color Management, checkable, on by default): whether
//!   thumbnails and previews apply the photos' embedded colour profiles (`formats.md` §4). Saved in
//!   `settings.json`; it joins the thumbnail cache key, and the cells on screen are made again
//!   ([`crate::thumbs`]).

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "view.colorManagement",
        label: "Use Color Management",
        menu: "View/Use Color Management",
        keys: &[],
        params: r#"{"on"?:bool}: apply embedded colour profiles to thumbnails and previews; toggles without "on"; saved in settings.json"#,
        effect: Effect::Library,
        history: History::None,
        enabled: always,
        run: color_management,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    on: Option<bool>,
}

fn color_management(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let On { on } = params(p)?;
    let on = on.unwrap_or(!s.color_management());
    let changed = s.set_color_management(on);
    Ok(json!({ "on": on, "changed": changed }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn color_management_toggles_sets_and_is_saved() {
        let (dirs, mut s, _) = library(1);
        assert!(s.color_management(), "on by default");
        assert_eq!(s.execute("view.colorManagement", &json!({})).unwrap(), json!({ "on": false, "changed": true }));
        assert!(!s.color_management());
        assert_eq!(s.execute("view.colorManagement", &json!({ "on": false })).unwrap(), json!({ "on": false, "changed": false }));
        assert_eq!(s.undo_stack().len(), 0, "a view setting is not an undo step");
        s.flush().unwrap();
        drop(s);
        let s = crate::cmd::testing::open(dirs.library.path());
        assert!(!s.color_management(), "saved in settings.json");
        assert!(!s.settings().color_management());
    }

    #[test]
    fn bad_params_are_refused() {
        let (_dirs, mut s, _) = library(1);
        for p in [json!({ "on": "yes" }), json!({ "on": 1 }), json!({ "on": true, "x": 1 }), json!(["on"])] {
            assert!(matches!(s.execute("view.colorManagement", &p), Err(CmdError::BadParams(_))), "{p}");
        }
        assert!(s.color_management(), "nothing changed");
    }
}
