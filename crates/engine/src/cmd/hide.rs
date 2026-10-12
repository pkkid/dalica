//! Hiding photos and folders: `photo.hide` / `photo.unhide`, `folder.hide` / `folder.unhide`
//! (`organizing.md` §9, `data-model.md` §8).
//!
//! Nothing on disk moves: the flag is data in the sidecar (`"hidden": true` on the photo's entry,
//! or in the folder's own `"folder"` object, so a folder stays hidden when it moves). Hidden photos
//! show only with View ▸ Hidden Pictures; a hidden folder moves to the Hidden Folders group. Each
//! command is one library undo step for all it changes, and none when nothing changes. Unhide is
//! disabled unless something it names is hidden.
//!
//! The folder commands name their folders the way the Folder menu's other commands do
//! (`section.refreshThumbs`): the section as in its widget id (`"folder:12"`), or the photos of
//! the selection, whose folders are meant.

use std::collections::BTreeSet;

use dalica_core::{FolderId, PhotoId};
use dalica_library::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{Ids, MAX_IDS, has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

const FOLDER_PARAMS: &str = r#"{"section"?:"folder:<FolderId>", "ids"?:[PhotoId]}: the section's folder, and the folders of the photos `ids` names"#;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.hide",
            label: "Hide",
            menu: "Picture/Hide",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: hide the photos (they show only with View ▸ Hidden Pictures)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: |s, p| set_photos(s, p, true),
        },
        CommandSpec {
            id: "photo.unhide",
            label: "Unhide",
            menu: "Picture/Unhide",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: show the photos again; disabled unless one of them is hidden"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: can_unhide_photos,
            run: |s, p| set_photos(s, p, false),
        },
        CommandSpec {
            id: "folder.hide",
            label: "Hide Folder",
            menu: "Folder/Hide",
            keys: &[],
            params: FOLDER_PARAMS,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_folders,
            run: |s, p| set_folders(s, p, true),
        },
        CommandSpec {
            id: "folder.unhide",
            label: "Unhide Folder",
            menu: "Folder/Unhide",
            keys: &[],
            params: FOLDER_PARAMS,
            effect: Effect::Library,
            history: History::Library,
            enabled: can_unhide_folders,
            run: |s, p| set_folders(s, p, false),
        },
    ]
}

const NO_FOLDER: &str = "Select a folder or a photo first.";

/// Disabled when every photo named is known and none is hidden. Malformed params and unknown ids
/// are left to `run`, which reports them.
fn can_unhide_photos(s: &Session, p: &Value) -> Result<(), Disabled> {
    has_ids(s, p)?;
    let Ok(Ids { ids }) = params::<Ids>(p) else { return Ok(()) };
    let lib = s.library();
    if ids.len() <= MAX_IDS && ids.iter().all(|id| lib.photo(*id).is_some() && !lib.is_hidden(*id)) {
        return Err(Disabled(if ids.len() == 1 { "The photo is not hidden." } else { "None of these photos is hidden." }.into()));
    }
    Ok(())
}

fn set_photos(s: &mut Session, p: &Value, on: bool) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let changed: Vec<(PhotoId, bool)> = ids.iter().filter(|id| s.library().is_hidden(**id) != on).map(|id| (*id, on)).collect();
    let n = changed.len();
    if n > 0 {
        s.apply(Op::SetHidden { hidden: changed })?;
    }
    Ok(json!({ "ids": ids, "hidden": on, "changed": n }))
}

/// The folder commands' parameters.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
}

/// `"folder:12"` → folder 12.
fn parse_section(text: &str) -> Result<FolderId, CmdError> {
    let id = text.strip_prefix("folder:").and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0);
    id.map(FolderId::new).ok_or_else(|| CmdError::BadParams(format!("section must be \"folder:<id>\", not {text:?}")))
}

/// The folders `p` names, in id order: the section's, and those of the photos `ids` names.
fn folders(s: &Session, p: &Value) -> Result<BTreeSet<FolderId>, CmdError> {
    let Target { section, ids } = params(p)?;
    let mut out = BTreeSet::new();
    if let Some(text) = &section {
        let id = parse_section(text)?;
        if s.library().folder(id).is_none() {
            return Err(CmdError::NotFound(format!("no folder {id}")));
        }
        out.insert(id);
    }
    let ids = ids.unwrap_or_default();
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    for id in ids {
        out.insert(s.library().photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?.folder);
    }
    Ok(out)
}

/// Disabled when nothing is named. Malformed params and unknown ids are left to `run`.
fn has_folders(s: &Session, p: &Value) -> Result<(), Disabled> {
    match folders(s, p) {
        Ok(f) if f.is_empty() => Err(Disabled(NO_FOLDER.into())),
        _ => Ok(()),
    }
}

/// Disabled when nothing is named, or no folder named is hidden.
fn can_unhide_folders(s: &Session, p: &Value) -> Result<(), Disabled> {
    has_folders(s, p)?;
    match folders(s, p) {
        Ok(f) if f.iter().all(|id| !s.library().is_folder_hidden(*id)) => {
            Err(Disabled(if f.len() == 1 { "The folder is not hidden." } else { "None of these folders is hidden." }.into()))
        }
        _ => Ok(()),
    }
}

fn set_folders(s: &mut Session, p: &Value, on: bool) -> Result<Value, CmdError> {
    let folders = folders(s, p)?;
    if folders.is_empty() {
        return Err(CmdError::Disabled(NO_FOLDER.into()));
    }
    let changed: Vec<(FolderId, bool)> = folders.iter().filter(|f| s.library().is_folder_hidden(**f) != on).map(|f| (*f, on)).collect();
    let n = changed.len();
    if n > 0 {
        s.apply(Op::SetFoldersHidden { hidden: changed })?;
    }
    Ok(json!({ "folders": folders, "hidden": on, "changed": n }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn disabled_and_hostile_params() {
        let (_dirs, mut s, ids) = library(1);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        for id in ["photo.hide", "photo.unhide"] {
            assert_eq!(s.execute(id, &json!({ "ids": [] })), Err(CmdError::Disabled("Select some photos first.".into())));
            assert!(matches!(s.execute(id, &json!({ "ids": [999] })), Err(CmdError::NotFound(_))));
            hostile(&mut s, id);
        }
        assert_eq!(s.execute("photo.unhide", &json!({ "ids": [ids[0]] })), Err(CmdError::Disabled("The photo is not hidden.".into())));
        for id in ["folder.hide", "folder.unhide"] {
            for empty in [json!({}), json!(null), json!({ "ids": [] }), json!({ "section": null })] {
                assert_eq!(s.execute(id, &empty), Err(CmdError::Disabled(super::NO_FOLDER.into())), "{id} {empty}");
            }
            assert!(matches!(s.execute(id, &json!({ "section": "folder:999" })), Err(CmdError::NotFound(_))));
            for bad in ["folder:", "folder:0", "folder:x", "album:1", ""] {
                assert!(matches!(s.execute(id, &json!({ "section": bad })), Err(CmdError::BadParams(_))), "{id} {bad}");
            }
            hostile(&mut s, id);
        }
        let section = json!({ "section": format!("folder:{folder}") });
        assert_eq!(s.execute("folder.unhide", &section), Err(CmdError::Disabled("The folder is not hidden.".into())));
        assert_eq!(s.undo_stack().len(), 0);
    }
}
