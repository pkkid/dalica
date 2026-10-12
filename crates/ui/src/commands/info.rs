//! The dialogs that edit what is known about photos and folders, opened on the host:
//!
//! - `section.properties {folder?}` (Folder ▸ Edit Description…, a double-click on a tree row or
//!   a section title): Folder Properties for the folder named, else the selected section's, else
//!   the selected folder's (`organizing.md` §10); OK runs `folder.info.set`;
//! - `photo.adjustDate {ids?}` (Tools ▸ Adjust Date and Time…): for the photos named, else the
//!   selection (`organizing.md` §9); OK runs `photo.dates.set`;
//! - `quicktags.configure` (the gear of the Tags pane's quick tags, `button:quicktags.configure`):
//!   Configure Quick Tags (`organizing.md` §5.2); OK runs `tags.quick.set`;
//! - `tag.rename {tag}` (the Tags pane's tag menu, Rename Tag…): OK runs `tags.rename`.
//!
//! Each replies `{dialog: <kind>}`; the dialog's buttons (or `ui.dialog.confirm`) then run its one
//! engine command, as one undo step.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::Value;

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, adjust_date, folder_properties, quick_tags, rename_tag};
use crate::state::{MAX_DRAFT, MAX_IDS, Section};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "section.properties",
        label: "Edit Description…",
        menu: "Folder/Edit Description…",
        keys: &[],
        params: r#"{"folder"?: FolderId}: open Folder Properties (default: the selected section's folder); replies {dialog: "folderProperties"}"#,
        run: section_properties,
    },
    UiCommand {
        id: "photo.adjustDate",
        label: "Adjust Date and Time…",
        menu: "Tools/Adjust Date and Time…",
        keys: &[],
        params: r#"{"ids"?: [PhotoId]}: open Adjust Date and Time for the photos (default: the selection); replies {dialog: "adjustDate"}"#,
        run: adjust_date,
    },
    UiCommand {
        id: "quicktags.configure",
        label: "Configure Quick Tags…",
        menu: "",
        keys: &[],
        params: r#"{}: open Configure Quick Tags; replies {dialog: "quickTags"}"#,
        run: configure_quick_tags,
    },
    UiCommand {
        id: "tag.rename",
        label: "Rename Tag…",
        menu: "",
        keys: &[],
        params: r#"{"tag": string}: open Rename Tag for the tag (renames it on every photo once confirmed); replies {dialog: "renameTag"}"#,
        run: rename_tag,
    },
];

/// The commands of this module that cannot always run.
pub const CHECKS: &[(&str, Check)] = &[("section.properties", can_show_properties), ("photo.adjustDate", can_adjust_date)];

/// The folder Folder Properties shows without a named one: the selected section's, else the
/// selected folder's.
fn current_folder(app: &App) -> Option<FolderId> {
    let state = app.state();
    let section = state.selection.section.map(|s| match s {
        Section::Folder(f) => f,
    });
    section.or(state.folder)
}

fn can_show_properties(app: &App, _params: &Value) -> Result<(), String> {
    current_folder(app).map(|_| ()).ok_or_else(|| "Select a folder first.".to_string())
}

fn can_adjust_date(app: &App, _params: &Value) -> Result<(), String> {
    if app.state().selection.ids.is_empty() { Err("Select some photos first.".into()) } else { Ok(()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Properties {
    folder: Option<FolderId>,
}

fn section_properties(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Properties = parse_object(params)?;
    let folder = match p.folder {
        Some(f) => f,
        None => current_folder(app).ok_or_else(|| CmdError::Disabled("Select a folder first.".into()))?,
    };
    if !app.session().is_some_and(|s| s.library().folder(folder).is_some()) {
        return Err(CmdError::NotFound(format!("no folder {folder}")));
    }
    let d = folder_properties::dialog(app, folder);
    dialogs::open(app, d)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdjustDate {
    ids: Option<Vec<PhotoId>>,
}

fn adjust_date(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: AdjustDate = parse_object(params)?;
    let mut ids = p.ids.unwrap_or_else(|| app.state().selection.ids.clone());
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = std::collections::HashSet::with_capacity(ids.len());
    ids.retain(|id| seen.insert(*id));
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let library = app.session().map(dalica_engine::Session::library);
    if let Some(id) = ids.iter().find(|id| !library.is_some_and(|l| l.photo(**id).is_some())) {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    let d = adjust_date::dialog(app, &ids);
    dialogs::open(app, d)
}

fn configure_quick_tags(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let d = quick_tags::dialog(app);
    dialogs::open(app, d)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameTag {
    tag: String,
}

fn rename_tag(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: RenameTag = parse(params)?;
    let tag = p.tag.trim();
    if tag.is_empty() || tag.chars().count() > MAX_DRAFT || tag.chars().any(char::is_control) {
        return Err(CmdError::BadParams(format!("`tag` is a tag of 1–{MAX_DRAFT} characters without control characters")));
    }
    dialogs::open(app, rename_tag::dialog(tag))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs::DialogKind;

    #[test]
    fn the_openers_target_the_selection_or_what_they_name() {
        let (_dir, s, ids) = session_with_photos(3);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let mut app = App::new(Some(s));
        let check = |app: &App, id: &str| super::super::enabled(app, id, &json!({}));
        assert_eq!(check(&app, "section.properties"), Err("Select a folder first.".into()));
        assert_eq!(check(&app, "photo.adjustDate"), Err("Select some photos first.".into()));
        assert!(matches!(app.run("section.properties", &json!({})), Err(CmdError::Disabled(_))));
        assert!(matches!(app.run("photo.adjustDate", &json!({})), Err(CmdError::Disabled(_))));
        // Named targets.
        assert_eq!(app.run("section.properties", &json!({ "folder": folder.get() })).unwrap(), json!({ "dialog": "folderProperties" }));
        let d = dialogs::top(&app).unwrap();
        assert_eq!((d.fields["folder"].clone(), d.fields["useOldest"].clone()), (json!(folder.get()), json!(true)));
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.run("photo.adjustDate", &json!({ "ids": [ids[1].get(), ids[1].get()] })).unwrap(), json!({ "dialog": "adjustDate" }));
        let d = dialogs::top(&app).unwrap();
        assert_eq!((d.fields["ids"].clone(), d.fields["mode"].clone()), (json!([ids[1].get()]), json!("shift")));
        dialogs::cancel(&mut app).unwrap();
        // The selection.
        app.run("select.set", &json!({ "ids": [ids[0].get(), ids[2].get()] })).unwrap();
        assert_eq!((check(&app, "section.properties"), check(&app, "photo.adjustDate")), (Ok(()), Ok(())));
        app.run("photo.adjustDate", &json!({})).unwrap();
        assert_eq!(dialogs::top(&app).unwrap().fields["ids"], json!([ids[0].get(), ids[2].get()]));
        assert_eq!(dialogs::inspect(&app)["title"], "Adjust Picture Date — 2 pictures");
        app.run("section.properties", &json!({})).unwrap();
        assert_eq!(dialogs::top(&app).unwrap().fields["folder"], json!(folder.get()));
        // Unknown targets, hostile parameters.
        assert!(matches!(app.run("section.properties", &json!({ "folder": 999_999 })), Err(CmdError::NotFound(_) | CmdError::Disabled(_))));
        assert!(matches!(app.run("photo.adjustDate", &json!({ "ids": [999_999] })), Err(CmdError::NotFound(_) | CmdError::Disabled(_))));
        for (id, bad) in [
            ("section.properties", json!({ "folder": "x" })),
            ("section.properties", json!({ "id": 1 })),
            ("photo.adjustDate", json!({ "ids": 1 })),
            ("photo.adjustDate", json!({ "ids": [-1] })),
            ("quicktags.configure", json!({ "slots": [] })),
            ("tag.rename", json!({})),
            ("tag.rename", json!({ "tag": "  " })),
            ("tag.rename", json!({ "tag": 3 })),
            ("tag.rename", json!({ "tag": "a\u{7}b" })),
            ("tag.rename", json!({ "tag": "x".repeat(MAX_DRAFT + 1) })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
    }

    #[test]
    fn quick_tags_and_rename_tag_open_their_drafts() {
        let mut app = App::new(None);
        assert_eq!(app.run("quicktags.configure", &Value::Null).unwrap(), json!({ "dialog": "quickTags" }));
        let d = dialogs::top(&app).unwrap();
        assert_eq!((d.fields["useRecent"].clone(), d.fields["slots"].as_array().map(Vec::len)), (json!(true), Some(quick_tags::SLOTS)));
        assert_eq!(app.run("tag.rename", &json!({ "tag": " beach " })).unwrap(), json!({ "dialog": "renameTag" }));
        let d = dialogs::top(&app).unwrap();
        assert_eq!((d.kind, d.fields["from"].clone(), d.fields["to"].clone()), (DialogKind::RenameTag, json!("beach"), json!("beach")));
        // OK is disabled while the new name is empty; OK runs the engine command (none without a
        // library), and a failure keeps the dialog open.
        app.state_mut().dialogs.last_mut().unwrap().fields.insert("to".into(), json!(" "));
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))));
        app.state_mut().dialogs.last_mut().unwrap().fields.insert("to".into(), json!("sea"));
        assert!(dialogs::answer(&mut app, None).is_err());
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(DialogKind::RenameTag));
    }
}
