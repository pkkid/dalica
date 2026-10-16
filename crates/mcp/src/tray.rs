//! Headless MCP's own selection and Photo Tray (`ui-layout.md` §1.5, §5.4; `mcp.md` §4.2): the
//! light table's selection lives in `ui`, so a session without a window keeps its own. Photo tools
//! run without `ids` act on the tray, as the bottom-bar buttons do.
//!
//! - The **selection** is one section's photos at a time (`folder:3`, `album:a91c0e`): selecting
//!   in another section replaces it, whatever the mode.
//! - **Held** photos stay in the tray across sections until released.
//! - The **tray** is the held photos, then the selected photos not held, each once (the order
//!   `ui`'s `tray::photos` gives).
//!
//! [`fill_ids`] is the Photo Tray rule both modes apply before a command runs: the headless
//! backend with this [`Tray`], connect mode with the app's tray (`library.inspect`, M10.54).

use dalica_core::PhotoId;
use serde_json::{Map, Value, json};

/// The Photo Tray rule (`mcp.md` §4.1, §4.4), one implementation for headless and connect mode: a
/// command whose params doc (`params_doc`) names `ids` as required (`"ids":`, not `"ids"?:`), run
/// without `ids`, acts on the Photo Tray, as the bottom-bar buttons do. `tray` is asked for the
/// tray's photos only then; an empty tray gives `ids: []`, which the command itself refuses
/// ("Select some photos first."), in both modes alike. Returns whether `ids` was filled.
pub fn fill_ids<E>(params: &mut Map<String, Value>, params_doc: &str, tray: impl FnOnce() -> Result<Vec<PhotoId>, E>) -> Result<bool, E> {
    if params.contains_key("ids") || !params_doc.contains("\"ids\":") {
        return Ok(false);
    }
    let ids = tray()?;
    params.insert("ids".into(), json!(ids));
    Ok(true)
}

/// How [`Tray::select`] changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Replace,
    Add,
    Remove,
}

impl Mode {
    /// `replace`, `add` or `remove`.
    pub fn parse(text: &str) -> Option<Mode> {
        match text {
            "replace" => Some(Mode::Replace),
            "add" => Some(Mode::Add),
            "remove" => Some(Mode::Remove),
            _ => None,
        }
    }
}

/// The selection and the held photos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tray {
    section: Option<String>,
    selection: Vec<PhotoId>,
    held: Vec<PhotoId>,
}

fn push_unique(list: &mut Vec<PhotoId>, id: PhotoId) {
    if !list.contains(&id) {
        list.push(id);
    }
}

impl Tray {
    /// Changes the selection to, by or without `ids` in `section` (`None`: the section stays as it
    /// is). Selecting in another section starts a new selection there.
    pub fn select(&mut self, ids: &[PhotoId], section: Option<&str>, mode: Mode) {
        let other = section.is_some_and(|s| self.section.as_deref() != Some(s));
        if other || mode == Mode::Replace {
            self.selection.clear();
        }
        if let Some(s) = section {
            self.section = Some(s.to_string());
        }
        match mode {
            Mode::Replace | Mode::Add => ids.iter().for_each(|id| push_unique(&mut self.selection, *id)),
            Mode::Remove if other => {}
            Mode::Remove => self.selection.retain(|id| !ids.contains(id)),
        }
    }

    /// Holds the selected photos: they stay in the tray when the selection changes.
    pub fn hold(&mut self) {
        let selected = self.selection.clone();
        selected.into_iter().for_each(|id| push_unique(&mut self.held, id));
    }

    /// Releases `ids` (`None`: every held photo).
    pub fn release(&mut self, ids: Option<&[PhotoId]>) {
        match ids {
            Some(ids) => self.held.retain(|id| !ids.contains(id)),
            None => self.held.clear(),
        }
    }

    /// The tray's photos: held first, then the selected ones not held, each once.
    pub fn photos(&self) -> Vec<PhotoId> {
        let mut out = self.held.clone();
        out.extend(self.selection.iter().copied().filter(|id| !self.held.contains(id)));
        out
    }

    /// The section the selection is in.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    /// The selected photos, in the order selected.
    pub fn selection(&self) -> &[PhotoId] {
        &self.selection
    }

    /// The held photos, in the order held.
    pub fn held(&self) -> &[PhotoId] {
        &self.held
    }

    /// Drops photos that no longer exist (deleted, or gone in a rebuild).
    pub fn retain(&mut self, exists: impl Fn(PhotoId) -> bool) {
        self.selection.retain(|id| exists(*id));
        self.held.retain(|id| exists(*id));
    }

    /// `{section, selection, held, tray}`.
    pub fn to_json(&self) -> Value {
        json!({ "section": self.section, "selection": self.selection, "held": self.held, "tray": self.photos() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(r: std::ops::RangeInclusive<u64>) -> Vec<PhotoId> {
        r.map(PhotoId::new).collect()
    }

    /// One section at a time; held photos stay across sections; the tray lists each photo once,
    /// held first; photos that are gone leave.
    #[test]
    fn the_tray_is_held_photos_then_the_selection() {
        let mut t = Tray::default();
        t.select(&ids(1..=2), Some("folder:1"), Mode::Replace);
        t.select(&ids(3..=3), None, Mode::Add);
        assert_eq!(t.selection(), ids(1..=3));
        t.hold();
        t.select(&ids(10..=12), Some("folder:2"), Mode::Add);
        assert_eq!((t.section(), t.selection()), (Some("folder:2"), &ids(10..=12)[..]), "another section starts a new selection");
        t.select(&[PhotoId::new(1)], Some("folder:1"), Mode::Remove);
        assert!(t.selection().is_empty(), "a remove in another section selects nothing there");
        t.select(&ids(2..=4), Some("folder:1"), Mode::Replace);
        assert_eq!(t.photos(), [ids(1..=3), ids(4..=4)].concat(), "held first, each once");
        t.select(&ids(2..=2), None, Mode::Remove);
        assert_eq!(t.selection(), [PhotoId::new(3), PhotoId::new(4)]);
        t.retain(|id| id != PhotoId::new(1));
        assert_eq!(t.held(), ids(2..=3));
        t.release(Some(&ids(2..=2)));
        assert_eq!(t.photos(), ids(3..=4));
        t.release(None);
        assert_eq!(t.to_json()["tray"], json!([3, 4]));
        assert_eq!(Mode::parse("toggle"), None);
    }

    /// `ids` comes from the tray only when the params doc requires it and none was given; the tray
    /// is not asked otherwise, and its error is the answer.
    #[test]
    fn ids_come_from_the_tray_only_when_required_and_missing() {
        let tray = || Ok::<_, String>(ids(2..=3));
        let required = r#"{"ids":[PhotoId], "on":bool}"#;
        let mut p = Map::new();
        assert_eq!(fill_ids(&mut p, required, tray), Ok(true));
        assert_eq!(Value::Object(p), json!({ "ids": [2, 3] }));
        let mut given = json!({ "ids": [7] }).as_object().cloned().unwrap();
        assert_eq!(fill_ids(&mut given, required, || Err("not asked".to_string())), Ok(false));
        assert_eq!(given["ids"], json!([7]));
        let mut optional = Map::new();
        assert_eq!(fill_ids(&mut optional, r#"{"ids"?:[PhotoId]}"#, || Err("not asked".to_string())), Ok(false));
        assert!(optional.is_empty());
        assert_eq!(fill_ids(&mut Map::new(), required, || Err::<Vec<PhotoId>, _>("unreachable")), Err("unreachable"));
    }
}
