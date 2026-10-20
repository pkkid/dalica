//! Save As (`data-model.md` §6, File ▸ Save As…): `photo.saveAs` opens the save picker
//! (`crate::picker`: `rfd` in the app, the scripted picker in tests, the stub answered by
//! `ui.filePicker` over automation) for one photo with edits; the chosen path runs the engine's
//! `photo.saveTo {id, path}`, the way `folders.manager` runs `folders.set`.
//!
//! - `photo.saveAs {id?}`: the photo named, else the photo open in the edit view, else the one
//!   selected photo; disabled unless it is one photo with edits.
//! - **What the picker suggests** ([`request`]): the photo's name with the extension of the format
//!   Save As last wrote (`.jpg` until a PNG is written), in the folder it last wrote into (the
//!   photo's own folder until then), that format's filter first. When the library already knows a
//!   file of that name there, the suggestion is numbered like Save a Copy's (`IMG_0041-001.jpg`, the
//!   first number the library does not know), so the name offered is not one the save will refuse.
//! - **A name that is taken asks again** ([`follow`]): Save As never replaces a file. The engine
//!   learns that a path is taken (`exists`) or its folder is gone (`no_folder`) only on the job's
//!   worker, so the app follows the job its picker started; when it ends with one of those codes,
//!   the status line gives the job's reason and the picker opens again: titled "Choose Another
//!   Name" with the name tried, in its folder, for a taken name; in the photo's folder for a
//!   missing one. A path the command refuses outright (an extension that is not JPEG or PNG) asks
//!   again the same way. Only a save its own picker answered is followed: `photo.saveTo` run by a
//!   script never opens a picker.
//! - **Save a Copy** (`photo.saveCopy`) is the engine's command; the UI passes the selection in
//!   the library and the photo open in the edit view (`crate::commands::save`).

use std::path::{Path, PathBuf};

use dalica_core::PhotoId;
use dalica_engine::settings::SaveFormat;
use dalica_engine::{CmdError, JobId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::picker::{Filter, IMAGE_FILTERS, SaveRequest};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "photo.saveAs",
    label: "Save As…",
    menu: "File/Save As…",
    keys: &[],
    params: "{\"id\":PhotoId?}: open the save picker for this photo (else the one open or selected; it needs edits), suggesting its name in the folder and format Save As last wrote. Replies {dialog: \"filePicker\"}; the path chosen runs photo.saveTo {id, path} (answer the stub with ui.filePicker), and a path that is taken (job result code \"exists\"), a folder that is gone (\"no_folder\") or a name photo.saveTo refuses opens the picker again",
    run: save_as,
}];

pub const CHECKS: &[(&str, Check)] = &[("photo.saveAs", can_save_as)];

/// The save dialog's title.
pub const TITLE: &str = "Save As";
/// Its title when it opens again because the name chosen is taken.
pub const ANOTHER_NAME: &str = "Choose Another Name";
/// Why Save As cannot run: not one photo, or one without edits.
pub const ONE_PHOTO: &str = "Select one photo to save as a new file.";
pub const NO_EDITS: &str = "This photo has no edits to save.";
/// The filters with PNG first, for when Save As last wrote a PNG.
const PNG_FIRST: &[Filter] = &[Filter { name: "PNG", extensions: &["png"] }, Filter { name: "JPEG", extensions: &["jpg", "jpeg"] }];
/// The most numbered names tried for a suggestion (`-001` … `-999`, as Save a Copy's).
const MAX_NUMBER: u32 = 999;

/// What Save As follows from frame to frame ([`App`] keeps one).
#[derive(Debug, Default)]
pub struct Following {
    /// The photo whose save picker is open, until its answer runs or it closes.
    asked: Option<PhotoId>,
    /// The `photo.saveTo` job the picker's answer started.
    job: Option<Followed>,
    /// A picker to open again as soon as no other file dialog is open.
    again: Option<SaveRequest>,
}

impl Following {
    /// The job followed, if any.
    pub fn job(&self) -> Option<JobId> {
        self.job.as_ref().map(|f| f.job)
    }

    /// Whether a picker waits to open again.
    pub fn asking_again(&self) -> bool {
        self.again.is_some()
    }
}

/// A Save As job: the job, the photo and the path it writes.
#[derive(Debug)]
struct Followed {
    job: JobId,
    id: PhotoId,
    path: Option<PathBuf>,
}

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

/// The save dialog for photo `id`: its name with the extension of the format Save As last wrote,
/// in the folder it last wrote into (else the photo's), numbered when the library knows a file of
/// that name there; then `photo.saveTo {id, path}`.
pub fn request(app: &App, id: PhotoId) -> Option<SaveRequest> {
    let session = app.session()?;
    let library = session.library();
    let photo = library.photo(id)?;
    let save = session.settings().save();
    let folder = save.last_folder.clone().or_else(|| library.folder(photo.folder).map(|f| f.path.clone()));
    let (ext, filters) = match save.last_format {
        SaveFormat::Jpeg => ("jpg", IMAGE_FILTERS),
        SaveFormat::Png => ("png", PNG_FIRST),
    };
    let stem = dalica_core::names::split_ext(&photo.name).0;
    let name = free_name(app, folder.as_deref(), &format!("{stem}.{ext}"));
    Some(SaveRequest { title: crate::tl!("Save As"), name, folder, filters, command: "photo.saveTo", params: json!({ "id": id }) })
}

/// `name`, or the first numbered name (`-001` …) the library knows no file of in `folder`. The
/// library's model is asked, never the disk: the engine still refuses a name that is taken.
fn free_name(app: &App, folder: Option<&Path>, name: &str) -> String {
    let (Some(session), Some(folder)) = (app.session(), folder) else { return name.to_string() };
    let library = session.library();
    let Some(f) = library.folder_by_path(folder) else { return name.to_string() };
    let taken: std::collections::HashSet<String> = library.photos_in(f).iter().map(|p| p.name.to_lowercase()).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_string();
    }
    (1..=MAX_NUMBER).map(|n| dalica_core::names::numbered(name, n)).find(|n| !taken.contains(&n.to_lowercase())).unwrap_or_else(|| name.to_string())
}

/// The picker opened again for photo `id` after `tried` could not be written: `title`, the name
/// tried, in its folder when `keep_folder` and it names one, else in the folder [`request`] offers.
fn again(app: &App, id: PhotoId, tried: Option<&Path>, title: String, keep_folder: bool) -> Option<SaveRequest> {
    let mut r = request(app, id)?;
    r.title = title;
    if let Some(tried) = tried {
        if let Some(name) = tried.file_name() {
            r.name = name.to_string_lossy().into_owned();
        }
        if keep_folder
            && tried.is_absolute()
            && let Some(parent) = tried.parent()
        {
            r.folder = Some(parent.to_path_buf());
        }
    }
    Some(r)
}

fn save_as(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = target(app, params)?;
    let request = request(app, id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    app.pick_save(request).map_err(CmdError::Disabled)?;
    app.save_as.again = None;
    app.save_as.asked = Some(id);
    Ok(json!({ "dialog": crate::menus::FILE_PICKER }))
}

/// Called by [`App::run`] after every command: a `photo.saveTo` that answers the open Save As
/// picker for its photo is followed ([`follow`]); one refused for its path asks again at once.
pub fn ran(app: &mut App, id: &str, params: &Value, result: &Result<Value, CmdError>) {
    if id != "photo.saveTo" {
        return;
    }
    let Some(asked) = app.save_as.asked else { return };
    if params.get("id").and_then(Value::as_u64) != Some(asked.get()) {
        return;
    }
    app.save_as.asked = None;
    let tried = params.get("path").and_then(Value::as_str).map(PathBuf::from);
    match result {
        Ok(reply) => {
            if let Some(job) = reply.get("job").and_then(Value::as_u64) {
                let path = reply.get("path").and_then(Value::as_str).map(PathBuf::from).or(tried);
                if let Some(p) = &path {
                    app.notices_mut().name_tried(job, p);
                }
                app.save_as.job = Some(Followed { job, id: asked, path });
            }
        }
        // A name Save As cannot write (`photo.tif`, a path into `.dalica-originals`): ask again;
        // the status line already gives the reason.
        Err(CmdError::BadParams(_) | CmdError::Limit(_)) => app.save_as.again = again(app, asked, tried.as_deref(), crate::tl!("Save As"), true),
        Err(_) => {}
    }
}

/// Called by the app when the save picker has closed (answered or cancelled): no answer is
/// awaited any more.
pub fn closed(app: &mut App) {
    app.save_as.asked = None;
}

/// Once a frame: when the Save As job followed has ended with a taken name (`exists`) or a missing
/// folder (`no_folder`), the status line gives its reason and the picker opens again; a picker
/// waiting to open again opens once no other file dialog is open. Cheap when nothing is followed.
pub fn follow(app: &mut App) {
    if let Some(f) = &app.save_as.job {
        let entry = app.session().and_then(|s| s.job(f.job));
        if entry.as_ref().is_some_and(|e| !e.state.is_finished()) {
            return;
        }
        let Some(f) = app.save_as.job.take() else { return };
        let failed = entry.and_then(|e| e.result).and_then(|r| r.get("failed").and_then(Value::as_array).and_then(|a| a.first().cloned()));
        if let Some(failed) = failed {
            let code = failed.get("code").and_then(Value::as_str).unwrap_or_default();
            let reason = failed.get("reason").and_then(Value::as_str).unwrap_or_default().to_string();
            let asked = match code {
                "exists" => again(app, f.id, f.path.as_deref(), crate::tl!("Choose Another Name"), true),
                "no_folder" => again(app, f.id, f.path.as_deref(), crate::tl!("Save As"), false),
                _ => None,
            };
            if asked.is_some() {
                app.state_mut().status = reason;
                app.save_as.again = asked;
            }
        }
    }
    if app.save_as.again.is_none() || app.save_dialog().is_some() || app.folder_request().is_some() {
        return;
    }
    let Some(r) = app.save_as.again.take() else { return };
    let id = r.params.get("id").and_then(Value::as_u64).map(PhotoId::new);
    match app.pick_save(r) {
        Ok(()) => app.save_as.asked = id,
        Err(e) => app.state_mut().status = e,
    }
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
        assert_eq!((asked.title.as_str(), asked.command, asked.params.clone()), (TITLE, "photo.saveTo", json!({ "id": ids[0] })));
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
