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
/// The widest tree width kept in the view state: wider than any window (the layout clamps it to
/// the window's W − 480 anyway).
pub const TREE_MAX: f32 = 16_384.0;

/// Smallest, default and largest thumbnail cell edge (points, `design-system.md` §4).
pub const THUMB_MIN: f32 = 64.0;
pub const THUMB_DEFAULT: f32 = 144.0;
pub const THUMB_MAX: f32 = 320.0;

/// `v` clamped to `min ..= max`, or `default` when it is not a finite number.
fn clamp_or(v: f32, min: f32, max: f32, default: f32) -> f32 {
    if v.is_finite() { v.clamp(min, max) } else { default }
}

/// A thumbnail cell edge safe to lay out: `THUMB_MIN ..= THUMB_MAX`, the default when not finite.
pub fn thumb_size(v: f32) -> f32 {
    clamp_or(v, THUMB_MIN, THUMB_MAX, THUMB_DEFAULT)
}

/// A tree width safe to keep: `TREE_MIN ..= TREE_MAX`, the default when not finite.
pub fn tree_width(v: f32) -> f32 {
    clamp_or(v, TREE_MIN, TREE_MAX, TREE_DEFAULT)
}

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
    /// The status line's text as last drawn (`ui-layout.md` §6.1): `"No selection"`, the photo or
    /// the pictures selected, or a transient [`UiState::status`] message. Written by the status line
    /// each frame; `ui.state` reports it as `status`, and `ui.set` cannot change it.
    #[serde(rename = "status")]
    pub status_line: String,
    /// A transient message for the status line: the result of the last action, or why it was
    /// refused. It replaces the line's text for 4 s, then clears itself. Not part of the protocol's
    /// view state (`status` there is [`UiState::status_line`]).
    #[serde(skip)]
    pub status: String,
}

impl UiState {
    /// This state with every number in its range (a non-finite one at its default): what a writer
    /// that did not check (a settings file, a script) hands over is made safe before it is used.
    pub fn sanitized(mut self) -> UiState {
        self.thumb_size = thumb_size(self.thumb_size);
        self.tree_width = tree_width(self.tree_width);
        self
    }
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
            status_line: String::new(),
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
    fn status_is_the_line_as_drawn_and_the_transient_message_is_not_serialised() {
        let s = UiState { status_line: "No selection".into(), status: "Select some photos first.".into(), ..UiState::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["status"], "No selection");
        assert!(v.as_object().unwrap().values().all(|f| f != "Select some photos first."), "{v}");
        let back: UiState = serde_json::from_value(v).unwrap();
        assert_eq!((back.status_line.as_str(), back.status.as_str()), ("No selection", ""));
    }

    #[test]
    fn the_tree_and_pane_fields_are_plain_serde_fields() {
        let v = serde_json::json!({ "treeWidth": 300.5, "pane": "people", "paneLarge": true });
        let mut merged = serde_json::to_value(UiState::default()).unwrap();
        merged.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
        let s: UiState = serde_json::from_value(merged).unwrap();
        assert_eq!((s.tree_width, s.pane, s.pane_large), (300.5, Some(Pane::People), true));
    }

    #[test]
    fn sanitized_puts_every_number_in_its_range() {
        for (bad, thumb, tree) in [
            (f32::NAN, THUMB_DEFAULT, TREE_DEFAULT),
            (f32::INFINITY, THUMB_DEFAULT, TREE_DEFAULT),
            (f32::NEG_INFINITY, THUMB_DEFAULT, TREE_DEFAULT),
            (-1.0, THUMB_MIN, TREE_MIN),
            (1e9, THUMB_MAX, TREE_MAX),
            (f32::MIN_POSITIVE, THUMB_MIN, TREE_MIN),
        ] {
            let s = UiState { thumb_size: bad, tree_width: bad, ..UiState::default() }.sanitized();
            assert_eq!((s.thumb_size, s.tree_width), (thumb, tree), "{bad}");
        }
        let fine = UiState { thumb_size: 200.0, tree_width: 300.0, ..UiState::default() };
        assert_eq!(fine.clone().sanitized(), fine, "numbers in range are kept");
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
