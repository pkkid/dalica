//! `UiState`: the serialisable view state (`architecture.md` §10, `control-protocol.md` §6.5).
//! Plain data, so `ui.state` and `ui.set` can read and drive it and tests can assert on it. Library
//! state never lives here: it is the engine's.

use dalica_core::{FolderId, PhotoId};
use serde::{Deserialize, Serialize};

/// Which view fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum View {
    #[default]
    Library,
}

/// The photos selected on the light table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    /// Selected photo ids, in the order they were selected.
    pub ids: Vec<PhotoId>,
    /// The focused photo: the anchor of a Shift range, drawn with the white outline.
    pub anchor: Option<PhotoId>,
}

impl Selection {
    pub fn contains(&self, id: PhotoId) -> bool {
        self.ids.contains(&id)
    }
}

/// Smallest, default and largest thumbnail cell edge (points, `design-system.md` §4).
pub const THUMB_MIN: f32 = 64.0;
pub const THUMB_DEFAULT: f32 = 144.0;
pub const THUMB_MAX: f32 = 320.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiState {
    pub view: View,
    /// Thumbnail cell edge in points, `THUMB_MIN ..= THUMB_MAX`.
    pub thumb_size: f32,
    /// The folder whose section is selected (folder list and light table), if any.
    pub folder: Option<FolderId>,
    pub selection: Selection,
    /// The line the status bar shows: the result of the last action, or why it was refused.
    pub status: String,
}

impl Default for UiState {
    fn default() -> Self {
        UiState { view: View::Library, thumb_size: THUMB_DEFAULT, folder: None, selection: Selection::default(), status: String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_round_trips_as_camel_case_json_and_refuses_unknown_fields() {
        let mut s = UiState::default();
        s.selection.ids = vec![PhotoId::new(3), PhotoId::new(1)];
        s.selection.anchor = Some(PhotoId::new(1));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["thumbSize"], 144.0);
        assert_eq!(v["view"], "library");
        assert_eq!(v["selection"], serde_json::json!({ "ids": [3, 1], "anchor": 1 }));
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        assert!(serde_json::from_value::<UiState>(serde_json::json!({ "view": "library", "zoom": 2 })).is_err());
    }
}
