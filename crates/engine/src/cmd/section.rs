//! Commands on a light-table section (a folder or an album as a whole: the Folder menu).
//!
//! - `section.refreshThumbs` (Folder → Refresh Thumbnails, `organizing.md` §9): the section's
//!   thumbnails are made again from the files (their cache entries made dead first, sketches
//!   too, failures tried again), in the background; photos whose folder cannot be reached keep
//!   what they have. The section is named as in its widget id (`"folder:12"`); from a menu or a
//!   shortcut the selection's photos name theirs.

use std::collections::BTreeSet;

use dalica_core::{FolderId, PhotoId};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "section.refreshThumbs",
        label: "Refresh Thumbnails",
        menu: "Folder/Refresh Thumbnails",
        keys: &[],
        params: r#"{"section"?:"folder:<FolderId>", "ids"?:[PhotoId]}: make the thumbnails of the section (or of the folders of the photos `ids` names) again, in the background"#,
        effect: Effect::Library,
        history: History::None,
        enabled: has_target,
        run: refresh_thumbs,
    }]
}

/// The most photos named at once.
const MAX_IDS: usize = 100_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
}

fn has_target(_: &Session, p: &Value) -> Result<(), Disabled> {
    let none = p.get("section").is_none_or(Value::is_null) && p.get("ids").and_then(Value::as_array).is_none_or(Vec::is_empty);
    if none { Err(Disabled("Select a folder or a photo first.".into())) } else { Ok(()) }
}

/// `"folder:12"` → folder 12.
fn parse_section(text: &str) -> Result<FolderId, CmdError> {
    let id = text.strip_prefix("folder:").and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0);
    id.map(FolderId::new).ok_or_else(|| CmdError::BadParams(format!("section must be \"folder:<id>\", not {text:?}")))
}

fn refresh_thumbs(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Target { section, ids } = params(p)?;
    let mut folders: BTreeSet<FolderId> = BTreeSet::new();
    if let Some(text) = &section {
        let id = parse_section(text)?;
        if s.library().folder(id).is_none() {
            return Err(CmdError::NotFound(format!("no folder {id}")));
        }
        folders.insert(id);
    }
    let ids = ids.unwrap_or_default();
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    for id in &ids {
        let photo = s.library().photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        folders.insert(photo.folder);
    }
    if folders.is_empty() {
        return Err(CmdError::Disabled("Select a folder or a photo first.".into()));
    }
    let photos: Vec<PhotoId> = s.library().photos().filter(|p| folders.contains(&p.folder)).map(|p| p.id).collect();
    let queued = s.refresh_thumbnails(&photos);
    Ok(json!({ "folders": folders, "photos": queued }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn refresh_makes_a_folders_thumbnails_again() {
        let (_dirs, mut s, ids) = library(3);
        let before = s.thumbnail_stats().decodes;
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let v = s.execute("section.refreshThumbs", &json!({ "section": format!("folder:{folder}") })).unwrap();
        assert_eq!(v["photos"], json!(3));
        s.wait_idle();
        let stats = s.thumbnail_stats();
        assert_eq!(stats.decodes - before, 3, "each photo decoded again: {stats:?}");
        assert_eq!((stats.scanned, stats.failed), (3, 0));
        // By a photo of the folder, as the menu passes the selection.
        s.execute("section.refreshThumbs", &json!({ "ids": [ids[2]] })).unwrap();
        s.wait_idle();
        assert_eq!(s.thumbnail_stats().decodes - before, 6);
        assert_eq!(s.undo_stack().len(), 0);
    }

    #[test]
    fn disabled_and_hostile_params() {
        let (_dirs, mut s, _) = library(1);
        for empty in [json!({}), json!(null), json!({ "ids": [] }), json!({ "section": null })] {
            assert_eq!(s.execute("section.refreshThumbs", &empty), Err(CmdError::Disabled("Select a folder or a photo first.".into())), "{empty}");
        }
        assert!(matches!(s.execute("section.refreshThumbs", &json!({ "section": "folder:999" })), Err(CmdError::NotFound(_))));
        assert!(matches!(s.execute("section.refreshThumbs", &json!({ "ids": [999] })), Err(CmdError::NotFound(_))));
        for bad in ["folder:", "folder:0", "folder:-1", "folder:x", "album:1", "", "folder:99999999999999999999999"] {
            assert!(matches!(s.execute("section.refreshThumbs", &json!({ "section": bad })), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(s.execute("section.refreshThumbs", &json!({ "section": 3 })), Err(CmdError::BadParams(_))));
        hostile(&mut s, "section.refreshThumbs");
    }
}
