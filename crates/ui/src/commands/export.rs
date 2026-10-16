//! Export (`creating.md` §2): `output.export` opens the Export dialog on the host with a draft
//! (`crate::dialogs::export`), whose Export button runs the engine's `export.run`, the way
//! `folders.manager` opens the Folder Manager and runs `folders.set`.
//!
//! - `output.export {ids?, section?}` (File ▸ Export Pictures to Folder…, Ctrl+Shift+S in the
//!   library and the edit view, the bottom bar's Export, a section header's Export): the photos
//!   named (a header passes the selection inside its section with the section), else the section
//!   named, else the tray's photos (in the edit view the selection follows the photo shown, so the
//!   key exports that photo with any held ones, as the bottom bar's Export does); disabled while
//!   there is nothing to export ("tray not empty").
//! - `output.export.location {path}` (no menu): sets the open dialog's location; its Browse…
//!   folder picker runs it with the folder chosen, so over automation `ui.filePicker {path}` answers
//!   Browse… with it (checked as a place to write), never with Add Folder's `folders.add`.
//!
//! The draft starts from the engine's `export.defaults` for what is exported (the location, the
//! suggested folder name and the remembered options: size, quality, numbers, watermark); when the
//! engine cannot answer, from the dialog's built-in defaults and the name of the folder the photos
//! share.

use std::path::PathBuf;

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::{self, Dialog, DialogKind};
use crate::state::{ExportDraft, MAX_IDS, MAX_LOCATION, Quality, RESIZE_MAX, RESIZE_MIN, Section};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "output.export",
        label: "Export Pictures to Folder…",
        menu: "File/Export Pictures to Folder…",
        keys: &[(Context::Library, "Ctrl+Shift+S"), (Context::EditView, "Ctrl+Shift+S")],
        params: "{\"ids\":[PhotoId]=tray, \"section\":\"folder:<FolderId>\"?}: open the Export dialog for these photos or this section (replies {dialog: \"export\"}; its Export runs export.run)",
        run: export,
    },
    UiCommand {
        id: "output.export.location",
        label: "Export Location",
        menu: "",
        keys: &[],
        params: "{\"path\":string (absolute)}: set the open Export dialog's location (what its Browse… folder picker runs with the folder chosen); disabled while the dialog is closed",
        run: set_location,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("output.export", can_export), ("output.export.location", can_set_location)];

/// Why the Export dialog's location cannot be set: the dialog is closed.
pub const NO_DIALOG: &str = "The Export dialog is not open.";

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
        // An album section is the engine's to check (`export.defaults`, `export.run`).
        if let Section::Folder(f) = section
            && !library.is_some_and(|l| l.folder(f).is_some())
        {
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
        Some(s) => s.folder(),
        None => {
            let mut folders = ids.iter().filter_map(|id| library.photo(*id).map(|p| p.folder));
            let first = folders.next();
            first.filter(|f| folders.all(|g| g == *f))
        }
    };
    folder.and_then(|f| library.folder(f)).and_then(|f| f.path.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

/// The draft `export.defaults` gives for `ids` (else the `section`): the location (empty when
/// there is none), the folder name and the remembered options. `None` when it cannot answer.
fn defaults(app: &mut App, ids: &[PhotoId], section: Option<Section>) -> Option<ExportDraft> {
    // A header's Export names the selection inside its section: the photos are what is exported.
    let params = match section {
        Some(s) if ids.is_empty() => json!({ "section": s }),
        _ => json!({ "ids": ids }),
    };
    let v = app.session_mut()?.execute("export.defaults", &params).ok()?;
    Some(draft_from(&v))
}

/// The draft of an `export.defaults` reply; what it lacks keeps the dialog's built-in default.
pub fn draft_from(v: &Value) -> ExportDraft {
    let base = ExportDraft::default();
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let flag = |k: &str, or: bool| v.get(k).and_then(Value::as_bool).unwrap_or(or);
    let quality = v.get("quality").and_then(|q| serde_json::from_value::<Quality>(q.clone()).ok()).unwrap_or(base.quality);
    let custom = v.get("customQuality").and_then(Value::as_u64).and_then(|c| u8::try_from(c).ok()).unwrap_or(base.custom);
    let size = v.get("size").and_then(Value::as_u64).filter(|s| (u64::from(RESIZE_MIN)..=u64::from(RESIZE_MAX)).contains(s));
    ExportDraft {
        location: text("location").unwrap_or_default(),
        folder_name: text("folderName").unwrap_or_default(),
        numbers: flag("numbers", base.numbers),
        original_size: !flag("resize", !base.original_size),
        resize: size.map_or(base.resize, |s| s.to_string()),
        quality,
        custom,
        watermark: flag("watermark", base.watermark),
        watermark_text: text("watermarkText").unwrap_or_default(),
        ..base
    }
    .sanitized()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    path: PathBuf,
}

fn can_set_location(app: &App, _params: &Value) -> Result<(), String> {
    if app.state().export.is_some() { Ok(()) } else { Err(NO_DIALOG.into()) }
}

/// `output.export.location {path}`: the Export dialog's location becomes `path` (absolute, valid
/// Unicode of at most [`MAX_LOCATION`] bytes). Replies `{location}`.
fn set_location(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Location = parse_object(params)?;
    let text = p.path.to_str().ok_or_else(|| CmdError::BadParams("path: not valid Unicode".into()))?.to_string();
    if text.len() > MAX_LOCATION {
        return Err(CmdError::Limit(format!("path: at most {MAX_LOCATION} bytes")));
    }
    if !p.path.is_absolute() || text.contains('\0') {
        return Err(CmdError::BadParams(format!("path: an absolute folder, not {text:?}")));
    }
    let draft = app.state_mut().export.as_mut().ok_or_else(|| CmdError::Disabled(NO_DIALOG.into()))?;
    draft.location = text.clone();
    Ok(json!({ "location": text }))
}

fn export(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let (ids, section) = targets(app, params)?;
    if ids.is_empty() && section.is_none() {
        return Err(CmdError::Disabled(NOTHING.into()));
    }
    dialogs::can_open(app, DialogKind::Export).map_err(CmdError::Disabled)?;
    let draft = match defaults(app, &ids, section) {
        Some(d) => ExportDraft { ids, section, ..d },
        None => ExportDraft { folder_name: folder_name(app, &ids, section), ids, section, ..ExportDraft::default() },
    };
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

    #[test]
    fn output_export_location_sets_the_open_dialogs_location() {
        let (dir, s, ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        let out = dir.path().join("Out");
        // No dialog: disabled, nothing set.
        let e = app.run("output.export.location", &json!({ "path": out })).unwrap_err();
        assert_eq!(e.code(), "disabled");
        assert_eq!(super::super::enabled(&app, "output.export.location", &json!({})), Err(NO_DIALOG.into()));
        app.run("output.export", &json!({ "ids": [ids[0]] })).unwrap();
        let before = app.state().export.clone().unwrap();
        // Graceful failures, nothing changed.
        let long = format!("/{}", "a".repeat(MAX_LOCATION));
        for (bad, code) in [
            (json!({}), "bad_params"),
            (json!({ "path": "relative/folder" }), "bad_params"),
            (json!({ "path": "" }), "bad_params"),
            (json!({ "path": 5 }), "bad_params"),
            (json!({ "path": out, "zzz": 1 }), "bad_params"),
            (json!([1]), "bad_params"),
            (json!({ "path": long }), "limit"),
        ] {
            assert_eq!(app.run("output.export.location", &bad).unwrap_err().code(), code, "{bad}");
        }
        assert_eq!(app.state().export.as_ref(), Some(&before));
        let reply = app.run("output.export.location", &json!({ "path": out })).unwrap();
        assert_eq!(reply, json!({ "location": out }));
        assert_eq!(app.state().export.as_ref().map(|e| PathBuf::from(&e.location)), Some(out));
    }
}
