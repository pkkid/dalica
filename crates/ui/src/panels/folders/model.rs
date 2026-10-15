//! The tree pane's rows as laid out (`ui-layout.md` §4): a pure function of a
//! [`dalica_library::Tree`], its Albums group ([`dalica_library::AlbumTree`]) and the row heights,
//! built once per tree and read every frame.
//!
//! - **Items** stack from the top: a group header (30) per group, then its rows: album rows (24:
//!   the search results while searching, Starred Photos, Recently Updated, then the user albums),
//!   folder and directory rows (24) and, in flat mode sorted by date, year dividers (18). The
//!   Albums group comes first, when it has rows. Each item knows its
//!   content offset, so the rows a frame draws are found by binary search ([`Model::visible`]): a
//!   frame's work is the rows on screen, never the library's.
//! - **Docked headers:** the collapsed groups after the last open one ([`Model::docked`]) sit at the
//!   bottom of the pane while everything fits in it, and follow the list once it is taller.
//! - **Ids** (`control-protocol.md` §7): `tree:group:<group>`, `tree:album:<AlbumId>` for a user
//!   album's row (`tree:album:starred`, `tree:album:recent`, `tree:album:search` for the automatic
//!   ones), `tree:folder:<FolderId>` for a library folder's row, and for a directory between a watched root and its folders (no library
//!   folder of its own) `tree:dir:<hash>`, sixteen hex digits of the FNV-1a hash of its path
//!   ([`dir_id`]); their disclosure triangles `tree:disclosure:<FolderId>` and
//!   `tree:disclosure:dir:<hash>`.
//! - **Moving** ([`Model::step`]): ↑/↓ go to the previous or next row that has a section (a row
//!   with photos); directories, empty folders and empty albums are passed over.
//! - **Search mode** ([`Model::search`]): the group headers show a magnifier in place of their
//!   disclosure triangle, and the search album's row is highlighted.
//! - **Projects:** the rows of the Projects group (the folders an export made, marked Exported
//!   wherever they are, and the folders under the projects directory) are flagged
//!   [`Row::project`], so they are drawn with the project icon.
//! - **Offline** ([`Model::mark_offline`]): rows under a watched root whose volume is offline
//!   (`data-model.md` §7) keep their place, count and section and are marked, once per build, so
//!   a frame never tests a path.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use dalica_core::FolderId;
use dalica_library::{AlbumTree, Group, Row as TreeRow, Tree, TreeMode};

use crate::panels::light_table::album_section;
use crate::state::{AlbumSection, Section};

/// The heights of the pane's items and the indent per tree level (`design-system.md` §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heights {
    pub header: f32,
    pub row: f32,
    pub year: f32,
    pub indent: f32,
}

impl Default for Heights {
    fn default() -> Heights {
        let m = dalica_ui_kit::theme::Metrics::DEFAULT;
        Heights { header: m.group_header_h, row: m.tree_row_h, year: m.year_divider_h, indent: m.tree_indent }
    }
}

impl Heights {
    pub fn from_metrics(m: &dalica_ui_kit::theme::Metrics) -> Heights {
        Heights { header: m.group_header_h, row: m.tree_row_h, year: m.year_divider_h, indent: m.tree_indent }
    }
}

/// A folder, directory or album row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The library folder, `None` for a directory that is not one and for an album.
    pub folder: Option<FolderId>,
    /// The album of an album row.
    pub album: Option<AlbumSection>,
    pub path: PathBuf,
    pub name: String,
    /// Photos shown directly in it.
    pub count: usize,
    /// Tree level (0 in flat mode).
    pub depth: usize,
    /// Tree mode: it has rows below it (a disclosure triangle) and whether they are shown.
    pub has_children: bool,
    pub expanded: bool,
    /// The light-table section it scrolls to; `None` without photos.
    pub section: Option<Section>,
    /// Its widget id and its disclosure triangle's (tree mode, with children).
    pub id: String,
    pub disclosure: Option<String>,
    /// Under a watched root whose volume is offline: drawn dimmed with the "offline" marker.
    pub offline: bool,
    /// A row of the Projects group (an export folder, a folder under the projects directory):
    /// drawn with the project icon.
    pub project: bool,
}

/// The marker an offline row carries.
pub const OFFLINE: &str = "offline";

impl Row {
    /// What `ui.widgets` says for it: `Lisbon Weekend (48)`. An offline row says the same (labels
    /// are what scripts match on); the pane's model ([`super::current_model`]) carries the flag.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.count)
    }
}

/// What an item is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A group header: its group, the title with the count (`Folders (174)`), folded or not.
    Header {
        group: Group,
        title: String,
        collapsed: bool,
    },
    /// A year divider.
    Year(u16),
    Row(Row),
}

/// One item of the pane at its content offset.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    /// Its top, from the top of the content.
    pub top: f32,
    pub height: f32,
}

/// The pane's items for one tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub items: Vec<Item>,
    /// The content's height.
    pub height: f32,
    /// Where the collapsed group headers after the last open group start (`items.len()` when
    /// there are none): they dock at the bottom of the pane while the content fits.
    pub docked: usize,
    pub mode: TreeMode,
    /// Search mode: the headers show a magnifier, the search album's row is highlighted.
    pub search: bool,
    /// Each library folder's row.
    by_folder: HashMap<FolderId, usize>,
    /// Each row's section.
    by_section: HashMap<Section, usize>,
}

/// A group's name in its header.
pub fn group_name(g: Group) -> &'static str {
    match g {
        Group::Albums => "Albums",
        Group::People => "People",
        Group::Projects => "Projects",
        Group::Folders => "Folders",
        Group::Other => "Other",
        Group::Hidden => "Hidden Folders",
    }
}

/// The header title with its count: `Folders (174)`.
pub fn group_title(g: Group, count: usize) -> String {
    format!("{} ({count})", group_name(g))
}

/// FNV-1a over `bytes`: stable across runs and releases (unlike the standard hasher).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The id of a directory row that is no library folder: `tree:dir:<16 hex digits>`.
pub fn dir_id(path: &Path) -> String {
    format!("tree:dir:{:016x}", fnv1a(path.to_string_lossy().as_bytes()))
}

impl Model {
    /// The items of `tree` with heights `h`.
    pub fn build(tree: &Tree, h: Heights) -> Model {
        Model::with_albums(tree, &AlbumTree::default(), false, h)
    }

    /// The items of `albums` (the Albums group, first) and `tree` with heights `h`; `search`: in
    /// search mode.
    pub fn with_albums(tree: &Tree, albums: &AlbumTree, search: bool, h: Heights) -> Model {
        let rows = albums.rows.len() + tree.groups.iter().map(|g| g.rows.len() + 1).sum::<usize>();
        let mut items = Vec::with_capacity(rows + 1);
        let mut by_folder = HashMap::new();
        let mut y = 0.0f32;
        let mut push = |items: &mut Vec<Item>, kind: Kind, height: f32| {
            items.push(Item { kind, top: y, height });
            y += height;
        };
        if albums.count > 0 {
            let header = Kind::Header { group: Group::Albums, title: group_title(Group::Albums, albums.count), collapsed: albums.collapsed };
            push(&mut items, header, h.header);
            for a in &albums.rows {
                let album = album_section(&a.album);
                let id = format!("tree:album:{}", a.album);
                let row = Row {
                    folder: None,
                    album: Some(album),
                    path: PathBuf::new(),
                    name: a.name.clone(),
                    count: a.count,
                    depth: 0,
                    has_children: false,
                    expanded: false,
                    section: a.section().map(|_| Section::Album(album)),
                    id,
                    disclosure: None,
                    offline: false,
                    project: false,
                };
                push(&mut items, Kind::Row(row), h.row);
            }
        }
        for g in &tree.groups {
            push(&mut items, Kind::Header { group: g.group, title: group_title(g.group, g.count), collapsed: g.collapsed }, h.header);
            for r in &g.rows {
                match r {
                    TreeRow::Year(year) => push(&mut items, Kind::Year(*year), h.year),
                    TreeRow::Folder(f) => {
                        let (id, disclosure) = match f.folder {
                            Some(id) => (format!("tree:folder:{id}"), format!("tree:disclosure:{id}")),
                            None => {
                                let dir = dir_id(&f.path);
                                let hash = dir.trim_start_matches("tree:dir:").to_owned();
                                (dir, format!("tree:disclosure:dir:{hash}"))
                            }
                        };
                        if let Some(folder) = f.folder {
                            by_folder.insert(folder, items.len());
                        }
                        let row = Row {
                            folder: f.folder,
                            album: None,
                            path: f.path.clone(),
                            name: f.name.clone(),
                            count: f.count,
                            depth: f.depth,
                            has_children: f.has_children,
                            expanded: f.expanded,
                            section: f.section().map(|dalica_library::SectionId::Folder(id)| Section::Folder(id)),
                            id,
                            disclosure: f.has_children.then_some(disclosure),
                            offline: false,
                            project: g.group == Group::Projects,
                        };
                        push(&mut items, Kind::Row(row), h.row);
                    }
                }
            }
        }
        // The trailing run of collapsed headers.
        let mut docked = items.len();
        while docked > 0 && matches!(items.get(docked - 1), Some(Item { kind: Kind::Header { collapsed: true, .. }, .. })) {
            docked -= 1;
        }
        let by_section =
            items.iter().enumerate().filter_map(|(i, it)| if let Kind::Row(r) = &it.kind { r.section.map(|s| (s, i)) } else { None }).collect();
        Model { items, height: y, docked, mode: tree.mode, search, by_folder, by_section }
    }

    /// Marks the rows `is_offline` says are under an offline root (`TreeOptions::is_offline`);
    /// the others are unmarked. Run once per build.
    pub fn mark_offline(&mut self, is_offline: impl Fn(&Path) -> bool) {
        for item in &mut self.items {
            if let Kind::Row(r) = &mut item.kind {
                r.offline = is_offline(&r.path);
            }
        }
    }

    /// Whether any row is marked offline.
    pub fn any_offline(&self) -> bool {
        self.items.iter().any(|it| matches!(&it.kind, Kind::Row(r) if r.offline))
    }

    /// The farthest the content scrolls in a pane `view_h` high.
    pub fn max_offset(&self, view_h: f32) -> f32 {
        (self.height - view_h).max(0.0)
    }

    /// Whether the docked headers sit at the bottom of a pane `view_h` high (the content fits).
    pub fn docks(&self, view_h: f32) -> bool {
        self.docked < self.items.len() && self.height <= view_h
    }

    /// The top of item `i` on screen, from the pane's top, scrolled to `offset` in a pane `view_h`
    /// high: docked headers sit at the bottom while the content fits.
    pub fn screen_top(&self, i: usize, offset: f32, view_h: f32) -> Option<f32> {
        let item = self.items.get(i)?;
        if i >= self.docked && self.docks(view_h) {
            let below: f32 = self.items.get(i..).map_or(0.0, |rest| rest.iter().map(|it| it.height).sum());
            return Some(view_h - below);
        }
        Some(item.top - offset)
    }

    /// The items a pane `view_h` high scrolled to `offset` shows, in order.
    pub fn visible(&self, offset: f32, view_h: f32) -> Range<usize> {
        if !(offset.is_finite() && view_h.is_finite()) || view_h <= 0.0 {
            return 0..0;
        }
        let start = self.items.partition_point(|it| it.top + it.height <= offset);
        let end = self.items.partition_point(|it| it.top < offset + view_h);
        start..end.max(start)
    }

    /// The item of `folder`'s row, if it is shown.
    pub fn row_of(&self, folder: FolderId) -> Option<usize> {
        self.by_folder.get(&folder).copied()
    }

    /// The item of the row that scrolls to `section` (a folder's or an album's), if it is shown.
    pub fn row_of_section(&self, section: Section) -> Option<usize> {
        self.by_section.get(&section).copied()
    }

    pub fn row(&self, i: usize) -> Option<&Row> {
        match self.items.get(i).map(|it| &it.kind) {
            Some(Kind::Row(r)) => Some(r),
            _ => None,
        }
    }

    /// The next row below (`down`) or above item `from` that has a section; from nowhere, the first
    /// (down) or last such row.
    pub fn step(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let has_section = |i: &usize| self.row(*i).is_some_and(|r| r.section.is_some());
        let n = self.items.len();
        match (from, down) {
            (Some(i), true) => (i.saturating_add(1)..n).find(has_section),
            (Some(i), false) => (0..i.min(n)).rev().find(has_section),
            (None, true) => (0..n).find(has_section),
            (None, false) => (0..n).rev().find(has_section),
        }
    }

    /// Tree mode: the nearest row above item `i` one level up that has a section.
    pub fn parent_with_section(&self, i: usize) -> Option<usize> {
        let mut depth = self.row(i)?.depth;
        for k in (0..i.min(self.items.len())).rev() {
            match self.items.get(k).map(|it| &it.kind) {
                Some(Kind::Row(r)) if r.depth < depth => {
                    if r.section.is_some() {
                        return Some(k);
                    }
                    depth = r.depth;
                }
                Some(Kind::Header { .. }) | None => return None,
                _ => {}
            }
        }
        None
    }

    /// The offset that shows item `i` whole in a pane `view_h` high, from `offset`: unchanged when
    /// it is already in view.
    pub fn reveal(&self, i: usize, offset: f32, view_h: f32) -> f32 {
        let Some(it) = self.items.get(i) else { return offset };
        let to = if it.top < offset {
            it.top
        } else if it.top + it.height > offset + view_h {
            it.top + it.height - view_h
        } else {
            offset
        };
        to.clamp(0.0, self.max_offset(view_h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_library::{FolderRow, GroupRows};

    fn folder(id: Option<u64>, path: &str, count: usize, depth: usize, has_children: bool) -> TreeRow {
        TreeRow::Folder(FolderRow {
            folder: id.map(FolderId::new),
            path: PathBuf::from(path),
            name: Path::new(path).file_name().unwrap().to_string_lossy().into_owned(),
            count,
            depth,
            date: None,
            has_children,
            expanded: true,
            hidden: false,
        })
    }

    fn tree() -> Tree {
        Tree {
            revision: 1,
            mode: TreeMode::Tree,
            groups: vec![
                GroupRows {
                    group: Group::Folders,
                    collapsed: false,
                    count: 4,
                    rows: vec![
                        folder(None, "/p", 0, 0, true),
                        folder(Some(1), "/p/a", 3, 1, true),
                        folder(None, "/p/a/x", 0, 2, true),
                        folder(Some(2), "/p/a/x/y", 5, 3, false),
                        folder(Some(3), "/p/b", 0, 1, false),
                    ],
                },
                GroupRows { group: Group::Hidden, collapsed: true, count: 2, rows: vec![] },
            ],
            sections: vec![],
        }
    }

    #[test]
    fn items_stack_with_their_heights_and_ids() {
        let m = Model::build(&tree(), Heights::default());
        let tops: Vec<f32> = m.items.iter().map(|it| it.top).collect();
        assert_eq!(tops, [0.0, 30.0, 54.0, 78.0, 102.0, 126.0, 150.0]);
        assert_eq!(m.height, 180.0);
        assert_eq!(m.docked, 6, "the collapsed Hidden Folders header docks");
        let ids: Vec<String> = m
            .items
            .iter()
            .filter_map(|it| match &it.kind {
                Kind::Row(r) => Some(r.id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(ids[1], "tree:folder:1");
        assert!(ids[0].starts_with("tree:dir:") && ids[0].len() == "tree:dir:".len() + 16, "{}", ids[0]);
        assert_ne!(ids[0], ids[2]);
        assert_eq!(ids[0], dir_id(Path::new("/p")), "stable for a path");
        let hash = ids[0].trim_start_matches("tree:dir:");
        assert_eq!(m.row(1).unwrap().disclosure, Some(format!("tree:disclosure:dir:{hash}")));
        assert_eq!(m.row(2).unwrap().disclosure.as_deref(), Some("tree:disclosure:1"));
        assert_eq!(m.row(4).unwrap().disclosure, None, "no children, no triangle");
        assert_eq!(m.row(4).unwrap().label(), "y (5)");
        let Kind::Header { title, .. } = &m.items[6].kind else { panic!() };
        assert_eq!(title, "Hidden Folders (2)");
    }

    #[test]
    fn rows_of_the_projects_group_are_flagged() {
        let mut t = tree();
        t.mode = TreeMode::Flat;
        t.groups.insert(
            0,
            GroupRows { group: Group::Projects, collapsed: false, count: 7, rows: vec![folder(Some(9), "/x/Exports/Lisbon", 7, 0, false)] },
        );
        let m = Model::build(&t, Heights::default());
        let flagged: Vec<(String, bool)> = (0..m.items.len()).filter_map(|i| m.row(i)).map(|r| (r.name.clone(), r.project)).collect();
        assert_eq!(flagged[0], ("Lisbon".into(), true), "the export folder");
        assert!(flagged[1..].iter().all(|(_, p)| !p), "the Folders group's rows are no projects: {flagged:?}");
        let Kind::Header { title, .. } = &m.items[0].kind else { panic!() };
        assert_eq!(title, "Projects (7)");
    }

    #[test]
    fn rows_under_an_offline_root_are_marked_and_say_so() {
        let mut m = Model::build(&tree(), Heights::default());
        assert!(!m.any_offline(), "nothing is offline until marked");
        let before = m.clone();
        m.mark_offline(|p| p.starts_with("/p/a"));
        let marked: Vec<(String, bool)> = (0..m.items.len()).filter_map(|i| m.row(i)).map(|r| (r.name.clone(), r.offline)).collect();
        assert_eq!(marked, [("p".into(), false), ("a".into(), true), ("x".into(), true), ("y".into(), true), ("b".into(), false)]);
        assert_eq!(m.row(2).unwrap().label(), "a (3)", "the label is unchanged");
        // Place, count, section and height are kept.
        assert_eq!((m.height, m.items.len(), m.row_of(FolderId::new(2))), (before.height, before.items.len(), before.row_of(FolderId::new(2))));
        assert_eq!(m.row(4).unwrap().section, before.row(4).unwrap().section);
        m.mark_offline(|_| false);
        assert_eq!(m, before, "unmarked again");
    }

    #[test]
    fn visible_items_are_found_by_offset() {
        let m = Model::build(&tree(), Heights::default());
        assert_eq!(m.visible(0.0, 60.0), 0..3);
        assert_eq!(m.visible(54.0, 24.0), 2..3);
        assert_eq!(m.visible(1000.0, 60.0), 7..7);
        assert_eq!(m.visible(f32::NAN, 60.0), 0..0);
        assert_eq!(m.visible(0.0, 0.0), 0..0);
        // Docked at the bottom while the content fits, in the flow once it does not.
        assert_eq!(m.screen_top(6, 0.0, 400.0), Some(370.0));
        assert_eq!(m.screen_top(6, 20.0, 100.0), Some(130.0));
        assert_eq!(m.screen_top(99, 0.0, 400.0), None);
    }

    #[test]
    fn steps_pass_over_rows_without_photos() {
        let m = Model::build(&tree(), Heights::default());
        assert_eq!(m.step(None, true), Some(2));
        assert_eq!(m.step(Some(2), true), Some(4));
        assert_eq!(m.step(Some(4), true), None, "the last row with photos");
        assert_eq!(m.step(Some(4), false), Some(2));
        assert_eq!(m.step(None, false), Some(4));
        assert_eq!(m.parent_with_section(4), Some(2), "over the directory in between");
        assert_eq!(m.parent_with_section(2), None);
        assert_eq!(m.row_of(FolderId::new(2)), Some(4));
        assert_eq!(m.reveal(4, 0.0, 60.0), 66.0);
        assert_eq!(m.reveal(1, 66.0, 60.0), 30.0);
        assert_eq!(m.reveal(4, 80.0, 60.0), 80.0, "already in view");
    }
}
