//! The inline caption editor (`organizing.md` §5.3): Picture ▸ Edit Caption (`photo.caption.edit`)
//! opens a one-line editor under a photo's cell, the focused photo's unless the command names one.
//! The editor is view state ([`crate::state::CaptionEdit`]: the photo and the text typed so far);
//! it commits through the engine's `photo.caption.set` (one undo step), and Esc leaves the caption
//! as it was.
//!
//! For now the command opens the editor in the view state only: the cell draws it, commits it and
//! cancels it later in this milestone.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::state::CaptionEdit;

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "photo.caption.edit",
    label: "Edit Caption",
    menu: "Picture/Edit Caption",
    keys: &[],
    params: r#"{"id"?: PhotoId}: open the inline caption editor under the photo (default: the focused photo); replies {captionEdit: {photo, draft}}"#,
    run: edit,
}];

/// The commands of this module that cannot always run.
pub const CHECKS: &[(&str, Check)] = &[("photo.caption.edit", can_edit)];

/// The photo Edit Caption captions without a named one: the focused photo, else the first selected.
fn focused(app: &App) -> Option<PhotoId> {
    let sel = &app.state().selection;
    sel.anchor.or_else(|| sel.ids.first().copied())
}

fn can_edit(app: &App, _params: &Value) -> Result<(), String> {
    focused(app).map(|_| ()).ok_or_else(|| "Select a photo first.".to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    id: Option<PhotoId>,
}

fn edit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Edit = parse_object(params)?;
    let photo = match p.id {
        Some(id) => id,
        None => focused(app).ok_or_else(|| CmdError::Disabled("Select a photo first.".into()))?,
    };
    if !app.session().is_some_and(|s| s.library().photo(photo).is_some()) {
        return Err(CmdError::NotFound(format!("no photo {photo}")));
    }
    // The model holds no captions yet: the editor starts empty.
    let open = CaptionEdit { photo, draft: String::new() };
    app.state_mut().caption_edit = Some(open.clone());
    Ok(json!({ "captionEdit": open }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn edit_caption_opens_the_editor_on_the_focused_or_named_photo() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("photo.caption.edit", &json!({})), Err(CmdError::Disabled(m)) if m == "Select a photo first."));
        assert_eq!(super::super::enabled(&app, "photo.caption.edit", &json!({})), Err("Select a photo first.".into()));
        app.run("select.set", &json!({ "ids": [ids[0].get(), ids[2].get()] })).unwrap();
        assert_eq!(super::super::enabled(&app, "photo.caption.edit", &json!({})), Ok(()));
        let r = app.run("photo.caption.edit", &Value::Null).unwrap();
        assert_eq!(r, json!({ "captionEdit": { "photo": ids[2].get(), "draft": "" } }), "the focused photo");
        app.run("photo.caption.edit", &json!({ "id": ids[1].get() })).unwrap();
        assert_eq!(app.state().caption_edit.as_ref().map(|e| e.photo), Some(ids[1]));
        assert!(matches!(app.run("photo.caption.edit", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
        for bad in [json!({ "id": -1 }), json!({ "id": "x" }), json!({ "ids": [1] }), json!([1])] {
            assert!(matches!(app.run("photo.caption.edit", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert_eq!(app.state().caption_edit.as_ref().map(|e| e.photo), Some(ids[1]), "failures change nothing");
    }
}
