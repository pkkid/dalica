//! Export (`creating.md` §2): `output.export` opens the Export dialog on the host with a draft
//! (`crate::dialogs::export`), whose Export button runs the engine's `export.run`, the way
//! `folders.manager` opens the Folder Manager and runs `folders.set`.
//!
//! - `output.export {ids?, section?}` (File ▸ Export Pictures to Folder…, Ctrl+Shift+S, the bottom
//!   bar's Export, a section header's Export): the photos named (a header passes the selection
//!   inside its section with the section), else the section named, else the tray's photos;
//!   disabled while there is nothing to export ("tray not empty").
//!
//! A stub until M8.20, which fills the draft from `export.defaults`: here the draft has the
//! dialog's built-in defaults and the folder name of the photos' folder when they share one.

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::Value;

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::{self, Dialog, DialogKind};
use crate::state::{ExportDraft, MAX_IDS, Section};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "output.export",
    label: "Export Pictures to Folder…",
    menu: "File/Export Pictures to Folder…",
    keys: &[(Context::Library, "Ctrl+Shift+S")],
    params: "{\"ids\":[PhotoId]=tray, \"section\":\"folder:<FolderId>\"?}: open the Export dialog for these photos or this section (replies {dialog: \"export\"}; its Export runs export.run)",
    run: export,
}];

pub const CHECKS: &[(&str, Check)] = &[("output.export", can_export)];

/// Why there is nothing to export.
pub const NOTHING: &str = "Put some photos in the tray first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    ids: Option<Vec<PhotoId>>,
    section: Option<Section>,
}

/// What `params` exports: the photos named (with the section they were picked in, from a header),
/// else the section named, else the tray's photos.
fn targets(app: &App, params: &Value) -> Result<(Vec<PhotoId>, Option<Section>), CmdError> {
    let p: Export = parse_object(params)?;
    let library = app.session().map(dalica_engine::Session::library);
    if let Some(section) = p.section {
        let Section::Folder(f) = section;
        if !library.is_some_and(|l| l.folder(f).is_some()) {
            return Err(CmdError::NotFound(format!("no folder {f}")));
        }
    }
    if let Some(ids) = p.ids {
        if ids.len() > MAX_IDS {
            return Err(CmdError::Limit(format!("`ids` holds at most {MAX_IDS} photos")));
        }
        if let Some(id) = ids.iter().find(|id| !library.is_some_and(|l| l.photo(**id).is_some())) {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        }
        if !ids.is_empty() || p.section.is_some() {
            return Ok((ids, p.section));
        }
    }
    if p.section.is_some() {
        return Ok((Vec::new(), p.section));
    }
    Ok((crate::tray::photos(app.state()), None))
}

fn can_export(app: &App, params: &Value) -> Result<(), String> {
    match targets(app, params) {
        Ok((ids, section)) if ids.is_empty() && section.is_none() => Err(NOTHING.into()),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// The folder name the dialog suggests: the section's folder, or the folder all the photos share.
fn folder_name(app: &App, ids: &[PhotoId], section: Option<Section>) -> String {
    let Some(library) = app.session().map(dalica_engine::Session::library) else { return String::new() };
    let folder = match section {
        Some(Section::Folder(f)) => Some(f),
        None => {
            let mut folders = ids.iter().filter_map(|id| library.photo(*id).map(|p| p.folder));
            let first = folders.next();
            first.filter(|f| folders.all(|g| g == *f))
        }
    };
    folder.and_then(|f| library.folder(f)).and_then(|f| f.path.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

fn export(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let (ids, section) = targets(app, params)?;
    if ids.is_empty() && section.is_none() {
        return Err(CmdError::Disabled(NOTHING.into()));
    }
    dialogs::can_open(app, DialogKind::Export).map_err(CmdError::Disabled)?;
    let draft = ExportDraft { folder_name: folder_name(app, &ids, section), ids, section, ..ExportDraft::default() };
    let reply = dialogs::open(app, Dialog::new(DialogKind::Export))?;
    app.state_mut().export = Some(draft.sanitized());
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn output_export_opens_the_dialog_for_the_tray_or_what_it_names() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        // An empty tray: nothing to export.
        assert!(matches!(app.run("output.export", &json!({})), Err(CmdError::Disabled(m)) if m == NOTHING));
        assert_eq!(super::super::enabled(&app, "output.export", &json!({})), Err(NOTHING.into()));
        app.run("select.set", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(app.run("output.export", &json!({})).unwrap(), json!({ "dialog": "export" }));
        let draft = app.state().export.clone().unwrap();
        assert_eq!(draft.ids, [ids[0], ids[1]]);
        assert!(!draft.folder_name.is_empty(), "the photos' folder");
        assert!(matches!(app.run("output.export", &json!({})), Err(CmdError::Disabled(_))), "already open");
        crate::dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().export, None, "closing drops the draft");
        // A section.
        let folder = app.session().unwrap().library().photo(ids[2]).unwrap().folder;
        app.run("output.export", &json!({ "section": format!("folder:{folder}") })).unwrap();
        assert_eq!(app.state().export.as_ref().map(|e| (e.ids.len(), e.section)), Some((0, Some(Section::Folder(folder)))));
        crate::dialogs::cancel(&mut app).unwrap();
        // A header's Export: the selection inside its section, with the section.
        app.run("output.export", &json!({ "section": format!("folder:{folder}"), "ids": [ids[2]] })).unwrap();
        assert_eq!(app.state().export.as_ref().map(|e| (e.ids.clone(), e.section)), Some((vec![ids[2]], Some(Section::Folder(folder)))));
        crate::dialogs::cancel(&mut app).unwrap();
        // Graceful failures, nothing opened.
        for (bad, code) in [
            (json!({ "ids": [999_999] }), "not_found"),
            (json!({ "section": "folder:999999" }), "not_found"),
            (json!({ "section": "album:x" }), "bad_params"),
            (json!({ "ids": "x" }), "bad_params"),
            (json!({ "zzz": 1 }), "bad_params"),
            (json!([1]), "bad_params"),
        ] {
            let e = app.run("output.export", &bad).unwrap_err();
            assert_eq!(e.code(), code, "{bad}");
        }
        assert!(app.state().dialogs.is_empty() && app.state().export.is_none());
    }
}
