//! The Folder Manager's draft: what the dialog shows and what OK applies, plain data in the open
//! dialog's fields (`ui.state` → `dialogs[].fields`, `ui.inspect` → `dialog.fields`). Nothing in
//! it reaches the library before OK.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The most folders kept expanded, changed or confirmed (more is a script gone wrong).
pub const MAX_ENTRIES: usize = 1000;

/// One folder's change: its own mode (`"always"`, `"once"`, `"remove"`) and faces flag, each
/// `None` to keep what the library has.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<bool>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.mode.is_none() && self.faces.is_none()
    }
}

/// The draft.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    /// Set on the dialog's first frame, when the library's selected folder is pre-selected.
    pub ready: bool,
    /// The folder selected in the tree, whose radios and faces toggle the right side shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The folders whose subfolders the tree shows.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expanded: Vec<String>,
    /// The changes OK applies, by folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, Change>,
    /// Volume roots and home folders whose Scan Always the Watch Drive question confirmed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub confirmed: Vec<String>,
    /// The volume roots and the home folder the roots' listing named (setting one to Scan Always
    /// asks first).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<String>,
    /// A folder to show: its parents are expanded and the tree scrolls to it once it is listed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<String>,
    /// The dialog's `[width, height]` in points, as its grip left it (M10.25; `None`: the first
    /// size, ≈ 640 × 480): set on the first frame to the size the grip left it at when it last
    /// closed in this session, drawn within the minimum and the window (`super::fit`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
}

/// A path as the draft keys it.
pub fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Draft {
    /// The draft in `fields`; a draft that does not read (never written by this code) is a new one.
    pub fn read(fields: &Map<String, Value>) -> Draft {
        serde_json::from_value(Value::Object(fields.clone())).unwrap_or_default()
    }

    /// Writes the draft into `fields`.
    pub fn write(&self, fields: &mut Map<String, Value>) {
        if let Ok(Value::Object(o)) = serde_json::to_value(self) {
            *fields = o;
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        let k = key(path);
        self.expanded.contains(&k)
    }

    pub fn set_expanded(&mut self, path: &Path, open: bool) {
        let k = key(path);
        self.expanded.retain(|e| *e != k);
        if open && self.expanded.len() < MAX_ENTRIES {
            self.expanded.push(k);
        }
    }

    /// Records `change` for `path`, dropping what matches `own` (what the library already sets
    /// there): a change back to how it was is no change.
    pub fn set(&mut self, path: &Path, mut change: Change, own: &Change) {
        let k = key(path);
        if change.mode.is_some() && change.mode == own.mode {
            change.mode = None;
        }
        if change.faces.is_some() && change.faces == own.faces {
            change.faces = None;
        }
        if change.is_empty() {
            self.changes.remove(&k);
        } else if self.changes.contains_key(&k) || self.changes.len() < MAX_ENTRIES {
            self.changes.insert(k, change);
        }
    }

    pub fn is_confirmed(&self, path: &Path) -> bool {
        let k = key(path);
        self.confirmed.contains(&k)
    }

    pub fn confirm(&mut self, path: &Path) {
        let k = key(path);
        if !self.confirmed.contains(&k) && self.confirmed.len() < MAX_ENTRIES {
            self.confirmed.push(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_draft_round_trips_and_a_change_back_is_no_change() {
        let mut d = Draft { ready: true, selected: Some("/p".into()), ..Draft::default() };
        let own = Change { mode: Some("always".into()), faces: None };
        d.set(Path::new("/p"), Change { mode: Some("once".into()), faces: None }, &own);
        assert_eq!(d.changes.len(), 1);
        d.set(Path::new("/p"), Change { mode: Some("always".into()), faces: None }, &own);
        assert!(d.changes.is_empty(), "back to the library's own mode");
        d.set(Path::new("/p"), Change { mode: None, faces: Some(false) }, &own);
        d.set_expanded(Path::new("/p"), true);
        d.set_expanded(Path::new("/p"), true);
        d.confirm(Path::new("/"));
        let mut fields = Map::new();
        d.write(&mut fields);
        assert_eq!(fields["changes"], json!({ "/p": { "faces": false } }));
        assert_eq!(fields["expanded"], json!(["/p"]));
        assert_eq!(Draft::read(&fields), d);
        assert!(Draft::read(&fields).is_confirmed(Path::new("/")));
        // Anything else reads as a new draft.
        let bad: Map<String, Value> = json!({ "changes": { "/p": { "mode": 3 } } }).as_object().unwrap().clone();
        assert_eq!(Draft::read(&bad), Draft::default());
        assert_eq!(Draft::read(&Map::new()), Draft::default());
    }
}
