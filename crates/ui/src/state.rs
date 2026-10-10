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

/// The side pane shown at the right of the light table (`ui-layout.md` §7); one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pane {
    People,
    Places,
    Tags,
    Properties,
}

/// Default, smallest library-tree width, and the width the light table keeps beside it (points,
/// `ui-layout.md` §2: the tree is 200 … W − 480 wide).
pub const TREE_DEFAULT: f32 = 240.0;
pub const TREE_MIN: f32 = 200.0;
pub const LIGHT_TABLE_MIN: f32 = 480.0;

/// Smallest, default and largest thumbnail cell edge (points, `design-system.md` §4).
pub const THUMB_MIN: f32 = 64.0;
pub const THUMB_DEFAULT: f32 = 144.0;
pub const THUMB_MAX: f32 = 320.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiState {
    pub view: View,
    /// Library-tree width in points as the user set it; the layout clamps it to `TREE_MIN ..= W −
    /// LIGHT_TABLE_MIN` (`panels::layout`).
    pub tree_width: f32,
    /// The open side pane, `None` when closed.
    pub pane: Option<Pane>,
    /// The side pane at its large width (« in its header).
    pub pane_large: bool,
    /// Thumbnail cell edge in points, `THUMB_MIN ..= THUMB_MAX`.
    pub thumb_size: f32,
    /// The folder whose section is selected (folder list and light table), if any.
    pub folder: Option<FolderId>,
    pub selection: Selection,
    /// The window fills the screen (F11).
    pub full_screen: bool,
    /// The line the status bar shows: the result of the last action, or why it was refused.
    pub status: String,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            view: View::Library,
            tree_width: TREE_DEFAULT,
            pane: None,
            pane_large: false,
            thumb_size: THUMB_DEFAULT,
            folder: None,
            selection: Selection::default(),
            full_screen: false,
            status: String::new(),
        }
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
        assert_eq!(
            (&v["treeWidth"], &v["pane"], &v["paneLarge"], &v["fullScreen"]),
            (&240.0.into(), &serde_json::Value::Null, &false.into(), &false.into())
        );
        assert_eq!(v["selection"], serde_json::json!({ "ids": [3, 1], "anchor": 1 }));
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        assert!(serde_json::from_value::<UiState>(serde_json::json!({ "view": "library", "zoom": 2 })).is_err());
    }

    #[test]
    fn the_side_pane_is_named_as_in_the_protocol() {
        let mut s = UiState { pane: Some(Pane::Properties), pane_large: true, ..UiState::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!((&v["pane"], &v["paneLarge"]), (&"properties".into(), &true.into()));
        for (name, pane) in [("people", Pane::People), ("places", Pane::Places), ("tags", Pane::Tags)] {
            s.pane = Some(pane);
            assert_eq!(serde_json::to_value(&s).unwrap()["pane"], name);
        }
        let mut bad = v.clone();
        bad["pane"] = "attic".into();
        assert!(serde_json::from_value::<UiState>(bad).is_err());
    }
}
