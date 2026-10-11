//! `UiState`: the serialisable view state (`architecture.md` §10, `control-protocol.md` §6.5).
//! Plain data, so `ui.state` and `ui.set` can read and drive it and tests can assert on it. Library
//! state never lives here: it is the engine's.

use std::fmt;
use std::str::FromStr;

use dalica_core::{FolderId, PhotoId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Which view fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum View {
    #[default]
    Library,
}

/// A section of the light table: the photos of one folder (albums join in M9). In the protocol
/// it is written as in its widget id, `"folder:12"` (`control-protocol.md` §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Folder(FolderId),
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Section::Folder(id) => write!(f, "folder:{id}"),
        }
    }
}

impl FromStr for Section {
    type Err = String;

    fn from_str(s: &str) -> Result<Section, String> {
        let bad = || format!("`{}` is not a section; sections are written `folder:<FolderId>`", s.chars().take(40).collect::<String>());
        match s.split_once(':') {
            Some(("folder", id)) => id.parse::<FolderId>().map(Section::Folder).map_err(|_| bad()),
            _ => Err(bad()),
        }
    }
}

impl Serialize for Section {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Section {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Section, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The photos selected on the light table (`ui-layout.md` §5.4). A selection never crosses a
/// section: `section` is the one its photos belong to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    /// Selected photo ids, in the order they were selected.
    pub ids: Vec<PhotoId>,
    /// The focused photo: the anchor of a Shift range, drawn with the white outline.
    pub anchor: Option<PhotoId>,
    /// The section the selected photos belong to; `None` when nothing is selected (or, until the
    /// selection model of M4.13, when a script selected photos of several folders).
    pub section: Option<Section>,
    /// The section was selected as a unit (a tree row or a section header): the tray shows it as
    /// one token ("Folder selected — 48 photos", `ui-layout.md` §6.2) instead of its photos.
    pub whole: bool,
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

/// How the library tree shows folders (`organizing.md` §10): every folder as one row (`flat`, the
/// default) or the disk hierarchy under each watched root (`tree`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TreeMode {
    #[default]
    Flat,
    Tree,
}

/// The order of the folders in the tree and of their sections on the light table (`organizing.md`
/// §10, the view-options popup of `ui-layout.md` §3): by date (a folder's oldest capture date,
/// newest first; the default), by recent changes (newest file first), by size, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FolderSort {
    #[default]
    Date,
    Recent,
    Size,
    Name,
}

/// The groups of the library tree (`ui-layout.md` §4), named as in their widget ids
/// (`tree:group:<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Albums,
    People,
    Projects,
    Folders,
    Other,
    Hidden,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Albums, Group::People, Group::Projects, Group::Folders, Group::Other, Group::Hidden];
}

/// The line under each thumbnail (View ▸ Thumbnail Caption, `menus-and-shortcuts.md` §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Caption {
    #[default]
    None,
    Filename,
    Caption,
    Tags,
    Resolution,
    Date,
}

/// The Photo Tray's own state (`ui-layout.md` §6.2). What it shows is the selection plus the held
/// photos; it is not persisted across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tray {
    /// Photos kept in the tray by Hold (Ctrl+H), in the order they were held.
    pub held: Vec<PhotoId>,
    /// The tray's own selection (what Clear removes), photos the tray shows.
    pub selected: Vec<PhotoId>,
}

/// Where the light table is scrolled to, kept by its content rather than by pixels so it survives a
/// resize, a new thumbnail size or a caption change: the section at the top of the view and how far
/// below that section's top the view starts, in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScrollAnchor {
    /// `None`: the top of the library.
    pub section: Option<Section>,
    /// `0 ..= SCROLL_OFFSET_MAX`.
    pub offset: f32,
}

/// The farthest below its section's top a scroll anchor may be, in points (more than any section
/// of the largest library is tall).
pub const SCROLL_OFFSET_MAX: f32 = 1.0e9;

/// The most photos the tray holds or selects, and the most folders kept collapsed.
pub const MAX_IDS: usize = 1_000_000;

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
    /// Flat list or folder tree (View ▸ Folder View, the toolbar's Flat | Tree).
    pub tree_mode: TreeMode,
    /// The folder order (the view-options popup).
    pub folder_sort: FolderSort,
    /// Reverse Order in the view-options popup.
    pub folder_sort_reverse: bool,
    /// The tree groups folded by their disclosure triangle; their sections leave the light table.
    pub collapsed_groups: Vec<Group>,
    /// Tree mode: the folders whose children are folded.
    pub collapsed_folders: Vec<FolderId>,
    /// View ▸ Thumbnail Caption.
    pub caption: Caption,
    /// View ▸ Small Pictures: show folders and photos of small pictures (both sides under 64 px).
    pub small_pictures: bool,
    /// View ▸ Hidden Pictures: show hidden photos (dimmed) and the Hidden Folders group.
    pub hidden_pictures: bool,
    /// The Photo Tray's held photos and own selection.
    pub tray: Tray,
    /// Where the light table is scrolled to.
    pub scroll: ScrollAnchor,
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
        self.scroll.offset = clamp_or(self.scroll.offset, 0.0, SCROLL_OFFSET_MAX, 0.0);
        dedup(&mut self.collapsed_groups);
        dedup(&mut self.collapsed_folders);
        dedup(&mut self.tray.held);
        dedup(&mut self.tray.selected);
        self
    }
}

/// Drops repeated entries, keeping the first of each (lists are short or bounded by [`MAX_IDS`]).
fn dedup<T: PartialEq + Copy>(v: &mut Vec<T>) {
    let mut kept: Vec<T> = Vec::with_capacity(v.len());
    for x in v.iter() {
        if !kept.contains(x) {
            kept.push(*x);
        }
    }
    *v = kept;
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
            tree_mode: TreeMode::Flat,
            folder_sort: FolderSort::Date,
            folder_sort_reverse: false,
            collapsed_groups: Vec::new(),
            collapsed_folders: Vec::new(),
            caption: Caption::None,
            small_pictures: false,
            hidden_pictures: false,
            tray: Tray::default(),
            scroll: ScrollAnchor::default(),
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
        assert_eq!(v["selection"], serde_json::json!({ "ids": [3, 1], "anchor": 1, "section": null, "whole": false }));
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

    #[test]
    fn the_m4_fields_have_their_protocol_names_and_defaults() {
        let v = serde_json::to_value(UiState::default()).unwrap();
        let fields = ["treeMode", "folderSort", "folderSortReverse", "caption", "smallPictures", "hiddenPictures"].map(|k| v[k].clone());
        assert_eq!(fields, [serde_json::json!("flat"), "date".into(), false.into(), "none".into(), false.into(), false.into()]);
        assert_eq!((&v["collapsedGroups"], &v["collapsedFolders"]), (&serde_json::json!([]), &serde_json::json!([])));
        assert_eq!(v["tray"], serde_json::json!({ "held": [], "selected": [] }));
        assert_eq!(v["scroll"], serde_json::json!({ "section": null, "offset": 0.0 }));
        let names = |all: &[serde_json::Value]| all.iter().map(|g| g.as_str().unwrap().to_string()).collect::<Vec<_>>();
        let groups: Vec<serde_json::Value> = Group::ALL.iter().map(|g| serde_json::to_value(g).unwrap()).collect();
        assert_eq!(names(&groups), ["albums", "people", "projects", "folders", "other", "hidden"]);
        for (sort, name) in [(FolderSort::Date, "date"), (FolderSort::Recent, "recent"), (FolderSort::Size, "size"), (FolderSort::Name, "name")] {
            assert_eq!(serde_json::to_value(sort).unwrap(), name);
        }
        for (c, name) in [
            (Caption::None, "none"),
            (Caption::Filename, "filename"),
            (Caption::Caption, "caption"),
            (Caption::Tags, "tags"),
            (Caption::Resolution, "resolution"),
            (Caption::Date, "date"),
        ] {
            assert_eq!(serde_json::to_value(c).unwrap(), name);
        }
        assert_eq!(serde_json::to_value(TreeMode::Tree).unwrap(), "tree");
    }

    #[test]
    fn a_section_is_written_as_its_widget_id() {
        let s = Section::Folder(FolderId::new(12));
        assert_eq!(s.to_string(), "folder:12");
        assert_eq!(serde_json::to_value(s).unwrap(), "folder:12");
        assert_eq!(serde_json::from_value::<Section>("folder:12".into()).unwrap(), s);
        for bad in ["", "folder:", "folder:-1", "folder:1.5", "folder: 1", "album:a91c0e", "12", "folder:99999999999999999999999", ":12"] {
            assert!(bad.parse::<Section>().is_err(), "{bad:?}");
        }
        assert!(serde_json::from_value::<Section>(serde_json::json!(12)).is_err());
    }

    #[test]
    fn sanitized_drops_repeats_and_keeps_the_scroll_offset_in_range() {
        let s = UiState {
            collapsed_groups: vec![Group::Folders, Group::Albums, Group::Folders],
            collapsed_folders: vec![FolderId::new(2), FolderId::new(2)],
            tray: Tray { held: vec![PhotoId::new(1), PhotoId::new(1), PhotoId::new(2)], selected: vec![PhotoId::new(2), PhotoId::new(2)] },
            scroll: ScrollAnchor { section: None, offset: f32::NAN },
            ..UiState::default()
        }
        .sanitized();
        assert_eq!(s.collapsed_groups, [Group::Folders, Group::Albums]);
        assert_eq!(s.collapsed_folders, [FolderId::new(2)]);
        assert_eq!(
            (s.tray.held.as_slice(), s.tray.selected.as_slice()),
            ([PhotoId::new(1), PhotoId::new(2)].as_slice(), [PhotoId::new(2)].as_slice())
        );
        assert_eq!(s.scroll.offset, 0.0);
        for (bad, kept) in [(-5.0, 0.0), (f32::INFINITY, 0.0), (2e9, SCROLL_OFFSET_MAX), (120.5, 120.5)] {
            let s = UiState { scroll: ScrollAnchor { section: None, offset: bad }, ..UiState::default() }.sanitized();
            assert_eq!(s.scroll.offset, kept, "{bad}");
        }
    }
}
