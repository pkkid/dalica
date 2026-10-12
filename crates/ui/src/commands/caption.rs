//! The inline caption editor (`organizing.md` §5.3): Picture ▸ Edit Caption (`photo.caption.edit`)
//! opens a one-line editor under a photo's cell, the focused photo's unless the command names one,
//! starting from its caption, and scrolls the cell into view. The editor is view state
//! ([`crate::state::CaptionEdit`]: the photo and the text typed so far); the cell draws it, and it
//! commits through the engine's `photo.caption.set` (one undo step) or, with Esc, leaves the caption
//! as it was ([`crate::panels::light_table::caption_edit`]). Opening it on another photo commits
//! what was typed for the first, as a click elsewhere would.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::panels::light_table::caption_edit;
use crate::selection::Table;
use crate::state::{CaptionEdit, MAX_DRAFT};

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

/// Why the editor cannot hold `photo`'s caption: one longer than a draft holds is edited in the
/// edit view's caption field, never cut here.
fn too_long(app: &App, photo: PhotoId) -> Option<String> {
    let caption = app.session()?.library().caption(photo);
    (caption.chars().count() > MAX_DRAFT).then(|| format!("This caption is longer than {MAX_DRAFT} characters; it cannot be edited here."))
}

fn can_edit(app: &App, _params: &Value) -> Result<(), String> {
    let photo = focused(app).ok_or_else(|| "Select a photo first.".to_string())?;
    too_long(app, photo).map_or(Ok(()), Err)
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
    let Some(caption) = app.session().and_then(|s| s.library().photo(photo).map(|_| s.library().caption(photo).to_owned())) else {
        return Err(CmdError::NotFound(format!("no photo {photo}")));
    };
    if let Some(why) = too_long(app, photo) {
        return Err(CmdError::Disabled(why));
    }
    // The cell scrolls into view (when a frame has been drawn and it is not in view).
    let reveal = Table::of(app).and_then(|t| t.section_of(photo).and_then(|s| t.reveal(s, photo)));
    // Open on this photo already: it goes on with what was typed. Open on another: what was typed
    // there is kept, as a click elsewhere keeps it.
    let open = match app.state_mut().caption_edit.take() {
        Some(old) if old.photo == photo => old,
        Some(old) => {
            caption_edit::commit(app, old.photo, &old.draft);
            CaptionEdit { photo, draft: caption }
        }
        None => CaptionEdit { photo, draft: caption },
    };
    let state = app.state_mut();
    state.caption_edit = Some(open.clone());
    if let Some(anchor) = reveal {
        state.scroll = anchor;
    }
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
