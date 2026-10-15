//! Headless MCP's own selection and Photo Tray (`ui-layout.md` §1.5, §5.4; `mcp.md` §4.2): the
//! light table's selection lives in `ui`, so a session without a window keeps its own. Photo tools
//! run without `ids` act on the tray, as the bottom-bar buttons do.
//!
//! - The **selection** is one section's photos at a time (`folder:3`, `album:a91c0e`): selecting
//!   in another section replaces it, whatever the mode.
//! - **Held** photos stay in the tray across sections until released.
//! - The **tray** is the held photos, then the selected photos not held, each once (the order
//!   `ui`'s `tray::photos` gives).

use dalica_core::PhotoId;
use serde_json::{Value, json};

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
}
