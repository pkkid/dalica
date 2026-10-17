//! The quick full-screen preview (`viewing.md` §2): holding Ctrl+Alt (Cmd+Option) over a thumbnail,
//! or in the edit view, fills the window with that photo until the keys are released. A hold
//! gesture is listed in the shortcut editor but not rebound, and Ctrl+Alt alone is no key the
//! keymap can bind, so `peek` has no default key: the light table and the edit view watch the
//! modifiers themselves and run it (M11.14), and so can an agent.
//!
//! `peek {id}` shows photo `id`; `peek {id: null}` ends the preview. The view state's `peek` is
//! the photo shown.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, parse};
use crate::app::App;

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "peek",
    label: "Quick Preview (hold Ctrl+Alt)",
    menu: "",
    keys: &[],
    params: "{id: PhotoId | null}: show a photo full screen while Ctrl+Alt is held; null ends the preview",
    run: peek,
}];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Peek {
    /// Required: a photo, or `null` to end the preview.
    #[serde(deserialize_with = "required")]
    id: Option<PhotoId>,
}

/// An `Option` field that must be given (`null` for none): missing is an error, not `None`.
fn required<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PhotoId>, D::Error> {
    Option::<PhotoId>::deserialize(d)
}

fn peek(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Peek = parse(params)?;
    if let Some(id) = p.id
        && !app.session().is_some_and(|s| s.library().photo(id).is_some())
    {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    app.state_mut().peek = p.id;
    Ok(json!({ "peek": p.id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_shows_a_photo_of_the_library_and_null_ends_it() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert_eq!(app.run("peek", &json!({ "id": ids[0] })).unwrap(), json!({ "peek": ids[0] }));
        assert_eq!(app.state().peek, Some(ids[0]));
        assert!(matches!(app.run("peek", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
        assert_eq!(app.state().peek, Some(ids[0]), "a refused peek changes nothing");
        app.run("peek", &json!({ "id": null })).unwrap();
        assert_eq!(app.state().peek, None);
        assert!(matches!(app.run("peek", &json!({})), Err(CmdError::BadParams(_))), "`id` is required");
    }
}
