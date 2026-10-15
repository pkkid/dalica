//! Watched folders and the Folder Manager (`organizing.md` §2, `data-model.md` §7).
//!
//! - `folders.add` is what File → Add Folder… ran after the folder picker until the Folder Manager:
//!   a Scan Always change with no undo step.
//! - `folders.set` applies the Folder Manager's changes on OK: one library undo step.
//! - `folder.remove` is the folder context menu's Remove from Dalica… (the Remove radio's effect).
//! - `folders.browse` and `folders.list` feed the Folder Manager's tree and its watched list.
//!
//! Confirmations: `folder.remove`, and a volume root or the home folder set to Scan Always, ask
//! first. Without `"confirm": true` they refuse as `disabled` with the dialog's question (the same
//! way `tray.clear` does until the dialog host routes them; `control-protocol.md` §5.5).

use std::path::{Path, PathBuf};

use dalica_core::FolderId;
use dalica_scan::{Mode, Rule};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::folders::DRIVE_QUESTION;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// The most changes one `folders.set` takes.
pub const MAX_CHANGES: usize = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "folders.add",
            label: "Add Folder…",
            menu: "File/Add Folder…",
            keys: &[],
            params: r#"{"path":string (absolute), "confirm"?:bool}: watch a folder (Scan Always) and scan it in the background; a volume root or the home folder needs "confirm": true"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: add,
        },
        CommandSpec {
            id: "folders.set",
            label: "Change Folder Settings",
            menu: "",
            keys: &[],
            params: r#"{"changes":[{"path":string (absolute), "mode"?:"always"|"once"|"remove"|null (null: inherit), "faces"?:bool|null (null: inherit)}] (1–1000, each path once), "confirm"?:bool}: the Folder Manager's changes as one undo step; removed folders leave the library (files untouched), newly indexed ones are scanned in the background; a volume root or the home folder set to "always" needs "confirm": true"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: set,
        },
        CommandSpec {
            id: "folder.remove",
            label: "Remove from Dalica…",
            menu: "Folder/Remove from Dalica…",
            keys: &[],
            params: r#"{"id":FolderId, "confirm":bool}: remove the folder and its subfolders from Dalica (set to Remove; files stay on disk); asks first: needs "confirm": true"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: remove,
        },
        CommandSpec {
            id: "folders.browse",
            label: "Browse Folders",
            menu: "",
            keys: &[],
            params: r#"{"path"?:string (absolute), "refresh"?:bool}: without a path the standard places and volumes, with one its subfolders, each with its mode (effective), explicit and faces; read on a worker: {"pending":true} until it is there (ask again after a frame)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: browse,
        },
        CommandSpec {
            id: "folders.list",
            label: "Watched Folders",
            menu: "",
            keys: &[],
            params: r#"{}: the watched folders (set to Scan Always) with friendly names, and every folder's own setting"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
    ]
}

/// `path` as a folder path, or `BadParams`.
fn folder(path: &Path) -> Result<PathBuf, CmdError> {
    dalica_scan::folder_path(path).map_err(CmdError::BadParams)
}

/// Refuses (as `disabled`, with the question and how to go ahead) unless `confirm`.
fn ask(confirm: bool, question: String) -> Result<(), CmdError> {
    if confirm { Ok(()) } else { Err(CmdError::Disabled(format!("{question} Send \"confirm\": true to go ahead."))) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Add {
    path: PathBuf,
    #[serde(default)]
    confirm: bool,
}

fn add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Add { path, confirm } = params(p)?;
    let path = folder(&path)?;
    if s.is_drive_or_home(&path) && s.folder_rules().get(&path).is_none_or(|r| r.mode != Some(Mode::Always)) {
        ask(confirm, DRIVE_QUESTION.to_string())?;
    }
    let new = s.add_root(path.clone());
    Ok(json!({ "path": path, "new": new, "scanning": true }))
}

/// A field that may be absent (keep), `null` (clear) or a value (set).
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: PathBuf,
    #[serde(default, deserialize_with = "present")]
    mode: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    faces: Option<Option<bool>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    changes: Vec<Change>,
    #[serde(default)]
    confirm: bool,
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Set { changes, confirm } = params(p)?;
    if changes.is_empty() {
        return Err(CmdError::BadParams("`changes` is empty: give at least one {path, mode?, faces?}".into()));
    }
    if changes.len() > MAX_CHANGES {
        return Err(CmdError::Limit(format!("at most {MAX_CHANGES} changes at once")));
    }
    let mut rules: Vec<(PathBuf, Rule)> = Vec::with_capacity(changes.len());
    for c in changes {
        let path = folder(&c.path)?;
        if rules.iter().any(|(p, _)| *p == path) {
            return Err(CmdError::BadParams(format!("{} is changed twice", path.display())));
        }
        if c.mode.is_none() && c.faces.is_none() {
            return Err(CmdError::BadParams(format!("the change of {} sets neither `mode` nor `faces`", path.display())));
        }
        let old = s.folder_rules().get(&path).copied().unwrap_or_default();
        let mode = match c.mode {
            None => old.mode,
            Some(None) => None,
            Some(Some(id)) => Some(
                Mode::from_id(&id).ok_or_else(|| CmdError::BadParams(format!("`mode` is \"always\", \"once\", \"remove\" or null, not {id:?}")))?,
            ),
        };
        let faces = c.faces.unwrap_or(old.faces);
        // A Scan Once folder stays frozen while it stays Scan Once.
        let indexed = mode == Some(Mode::Once) && old.mode == Some(Mode::Once) && old.indexed;
        rules.push((path, Rule { mode, faces, indexed }));
    }
    let kept = s.folder_rules().iter().filter(|(p, _)| !rules.iter().any(|(q, _)| q == p)).count();
    if kept + rules.len() > dalica_scan::rules::MAX_RULES {
        return Err(CmdError::Limit(format!("at most {} folders may have their own setting", dalica_scan::rules::MAX_RULES)));
    }
    let drives: Vec<String> = rules
        .iter()
        .filter(|(p, r)| {
            r.mode == Some(Mode::Always) && s.folder_rules().get(p).is_none_or(|o| o.mode != Some(Mode::Always)) && s.is_drive_or_home(p)
        })
        .map(|(p, _)| p.display().to_string())
        .collect();
    if !drives.is_empty() {
        ask(confirm, format!("{DRIVE_QUESTION} ({})", drives.join(", ")))?;
    }
    let changed = s.change_rules(rules);
    let count = changed.before.0.len();
    if count > 0 {
        s.folders.change = Some(changed.before);
    }
    Ok(json!({
        "changed": count,
        "removedFolders": changed.removed_folders,
        "removedPhotos": changed.removed_photos,
        "scanning": changed.scans,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    id: FolderId,
    #[serde(default)]
    confirm: bool,
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Remove { id, confirm } = params(p)?;
    let path = s.library().folder(id).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {id}")))?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    ask(confirm, format!("Remove Folder: Remove the folder \"{name}\" and its subfolders from Dalica? The files stay on your disk."))?;
    let old = s.folder_rules().get(&path).copied().unwrap_or_default();
    let changed = s.change_rules(vec![(path.clone(), Rule { mode: Some(Mode::Remove), faces: old.faces, indexed: false })]);
    if !changed.before.is_empty() {
        s.folders.change = Some(changed.before);
    }
    Ok(json!({ "path": path, "removedFolders": changed.removed_folders, "removedPhotos": changed.removed_photos }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Browse {
    path: Option<PathBuf>,
    #[serde(default)]
    refresh: bool,
}

fn browse(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Browse { path, refresh } = params(p)?;
    let path = path.as_deref().map(folder).transpose()?;
    match s.browse(path.clone(), refresh) {
        Some(answer) => s.browse_reply(path.as_deref(), answer),
        None => Ok(json!({ "pending": true, "path": path })),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    let rules = s.folder_rules();
    let watched: Vec<Value> = rules
        .with_mode(Mode::Always)
        .iter()
        .map(|path| json!({ "path": path, "name": s.friendly_name(path).map_or_else(|| path.display().to_string(), str::to_string) }))
        .collect();
    let settings: Vec<Value> = rules
        .iter()
        .map(|(path, r)| json!({ "path": path, "mode": r.mode.map(Mode::id), "faces": r.faces, "indexed": r.mode == Some(Mode::Once) && r.indexed }))
        .collect();
    Ok(json!({ "watched": watched, "settings": settings }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{library, open};
    use crate::error::CmdError;

    #[test]
    fn add_scans_in_the_background_and_persists_the_folder() {
        let (dirs, mut s, ids) = library(3);
        assert_eq!(ids.len(), 3);
        assert_eq!(s.roots().len(), 1);
        // Adding the same folder again rescans and finds nothing new.
        let again = s.execute("folders.add", &json!({ "path": s.roots()[0] })).unwrap();
        assert_eq!(again["new"], false);
        s.wait_idle();
        assert_eq!(s.library().photos().count(), 3);
        assert!(matches!(s.execute("folders.add", &json!({ "path": "relative/dir" })), Err(CmdError::BadParams(_))));
        s.flush().unwrap();
        let lib_dir = s.dir().to_path_buf();
        drop(s);
        // A new session remembers the folder and the photos.
        let s = open(&lib_dir);
        assert_eq!(s.roots().len(), 1);
        assert_eq!(s.library().photos().count(), 3);
        drop(dirs);
    }

    #[test]
    fn every_folder_command_refuses_bad_params() {
        let (_dirs, mut s, _) = library(2);
        let root = s.roots()[0].clone();
        let bad = [
            json!({ "changes": [] }),
            json!({ "changes": [{ "path": root }] }),
            json!({ "changes": [{ "path": root, "mode": "sometimes" }] }),
            json!({ "changes": [{ "path": root, "mode": 3 }] }),
            json!({ "changes": [{ "path": root, "faces": "yes" }] }),
            json!({ "changes": [{ "path": "rel", "mode": "once" }] }),
            json!({ "changes": [{ "path": root.join("..").join("x"), "mode": "once" }] }),
            json!({ "changes": [{ "path": root, "mode": "once" }, { "path": root, "mode": "remove" }] }),
            json!({ "changes": [{ "path": root, "mode": "once", "extra": 1 }] }),
            json!({ "changes": { "path": root } }),
        ];
        for p in bad {
            assert!(matches!(s.execute("folders.set", &p), Err(CmdError::BadParams(_))), "{p}");
        }
        let many: Vec<_> = (0..=super::MAX_CHANGES).map(|i| json!({ "path": root.join(format!("d{i}")), "mode": "once" })).collect();
        assert!(matches!(s.execute("folders.set", &json!({ "changes": many })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("folder.remove", &json!({ "id": 9999, "confirm": true })), Err(CmdError::NotFound(_))));
        assert!(matches!(s.execute("folder.remove", &json!({ "id": -1 })), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("folders.browse", &json!({ "path": "rel" })), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("folders.list", &json!({ "x": 1 })), Err(CmdError::BadParams(_))));
        assert!(s.undo_stack().is_empty());
        assert_eq!(s.library().photos().count(), 2);
    }
}
