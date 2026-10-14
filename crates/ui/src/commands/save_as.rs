//! Save As (`data-model.md` §6, File ▸ Save As…): `photo.saveAs` opens the save picker
//! (`crate::picker`: `rfd` in the app, the scripted picker in tests, the stub answered by
//! `ui.filePicker` over automation) for one photo with edits; the chosen path runs the engine's
//! `photo.saveTo {id, path}`, the way `folders.manager` runs `folders.set`.
//!
//! - `photo.saveAs {id?}`: the photo named, else the photo open in the edit view, else the one
//!   selected photo; disabled unless it is one photo with edits.
//!
//! A stub until M8.22, which suggests the last folder and format and asks again for a name that is
//! taken: here the picker opens in the photo's folder with its name as a JPEG.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::picker::{IMAGE_FILTERS, SaveRequest};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "photo.saveAs",
    label: "Save As…",
    menu: "File/Save As…",
    keys: &[],
    params: "{\"id\":PhotoId?}: open the save picker for this photo (else the one open or selected; it needs edits). Replies {dialog: \"filePicker\"}; the path chosen runs photo.saveTo {id, path} (answer the stub with ui.filePicker)",
    run: save_as,
}];

pub const CHECKS: &[(&str, Check)] = &[("photo.saveAs", can_save_as)];

/// The save dialog's title.
pub const TITLE: &str = "Save As";
/// Why Save As cannot run: not one photo, or one without edits.
pub const ONE_PHOTO: &str = "Select one photo to save as a new file.";
pub const NO_EDITS: &str = "This photo has no edits to save.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveAs {
    id: Option<PhotoId>,
}

/// The photo Save As writes: the one named, else the one open in the edit view, else the one
/// selected photo; it must exist and have edits.
fn target(app: &App, params: &Value) -> Result<PhotoId, CmdError> {
    let p: SaveAs = parse_object(params)?;
    let state = app.state();
    let id = match p.id {
        Some(id) => id,
        None => match super::edit_view::open_photo(app) {
            Some(open) => open,
            None => match state.selection.ids.as_slice() {
                [one] => *one,
                _ => return Err(CmdError::Disabled(ONE_PHOTO.into())),
            },
        },
    };
    let library = app.session().map(dalica_engine::Session::library).ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    if library.photo(id).is_none() {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    if library.info(id).edits.is_empty() {
        return Err(CmdError::Disabled(NO_EDITS.into()));
    }
    Ok(id)
}

fn can_save_as(app: &App, params: &Value) -> Result<(), String> {
    target(app, params).map(|_| ()).map_err(|e| e.to_string())
}

/// The save dialog for photo `id`: its name with a `.jpg` extension, in its folder, then
/// `photo.saveTo {id, path}`.
pub fn request(app: &App, id: PhotoId) -> Option<SaveRequest> {
    let library = app.session()?.library();
    let photo = library.photo(id)?;
    let folder = library.folder(photo.folder).map(|f| f.path.clone());
    let stem = std::path::Path::new(&photo.name).file_stem().map_or_else(|| photo.name.clone(), |s| s.to_string_lossy().into_owned());
    Some(SaveRequest {
        title: TITLE,
        name: format!("{stem}.jpg"),
        folder,
        filters: IMAGE_FILTERS,
        command: "photo.saveTo",
        params: json!({ "id": id }),
    })
}

fn save_as(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = target(app, params)?;
    let request = request(app, id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    app.pick_save(request).map_err(CmdError::Disabled)?;
    Ok(json!({ "dialog": crate::menus::FILE_PICKER }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::picker::ScriptedPicker;

    #[test]
    fn save_as_opens_the_save_picker_for_one_photo_with_edits() {
        let (_dir, mut s, ids) = session_with_photos(2);
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
        let scripted = ScriptedPicker::new();
        let mut app = App::with_picker(Some(s), Arc::new(scripted.clone()));
        // Nothing selected, two selected, one without edits.
        assert!(matches!(app.run("photo.saveAs", &json!({})), Err(CmdError::Disabled(m)) if m == ONE_PHOTO));
        app.run("select.set", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(super::super::enabled(&app, "photo.saveAs", &json!({})), Err(ONE_PHOTO.into()));
        assert!(matches!(app.run("photo.saveAs", &json!({ "id": ids[1] })), Err(CmdError::Disabled(m)) if m == NO_EDITS));
        app.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        assert_eq!(super::super::enabled(&app, "photo.saveAs", &json!({})), Ok(()));
        assert_eq!(app.run("photo.saveAs", &json!({})).unwrap(), json!({ "dialog": "filePicker" }));
        assert!(app.can_pick().is_err(), "one dialog at a time");
        assert!(matches!(app.run("photo.saveAs", &json!({})), Err(CmdError::Disabled(_))));
        // The scripted picker was asked for a JPEG beside the photo, then photo.saveTo {id, path}.
        let asked = (0..1000)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(2));
                scripted.last_save()
            })
            .unwrap();
        assert_eq!((asked.title, asked.command, asked.params.clone()), (TITLE, "photo.saveTo", json!({ "id": ids[0] })));
        assert!(asked.name.ends_with(".jpg") && asked.folder.is_some(), "{asked:?}");
        // Graceful failures.
        for (bad, code) in [
            (json!({ "id": 999_999 }), "not_found"),
            (json!({ "id": "x" }), "bad_params"),
            (json!({ "zzz": 1 }), "bad_params"),
            (json!(7), "bad_params"),
        ] {
            assert_eq!(app.run("photo.saveAs", &bad).unwrap_err().code(), code, "{bad}");
        }
        // Without a picker there is no save dialog.
        let (_dir2, s2, ids2) = session_with_photos(1);
        let mut s2 = s2;
        s2.execute("photo.history.apply", &json!({ "id": ids2[0], "op": "rotate(90)" })).unwrap();
        let mut bare = App::new(Some(s2));
        assert!(matches!(bare.run("photo.saveAs", &json!({ "id": ids2[0] })), Err(CmdError::Disabled(_))));
    }
}
