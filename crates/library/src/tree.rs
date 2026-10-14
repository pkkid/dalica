//! The library tree (`ui-layout.md` §4, `organizing.md` §10) as a pure function of the library,
//! the watched roots and the view options: [`Tree::build`].
//!
//! - **Groups** in order Albums, People, Projects, Folders, Other, Hidden Folders, each only when it
//!   has rows. Projects, Other and Hidden Folders exist in flat mode only; tree mode has Albums,
//!   People and Folders. [`Tree`] holds the folder groups; the Albums group is an [`AlbumTree`]
//!   (below), and People stays empty until the model holds people.
//! - **Flat mode:** every folder with something to show is one row. Projects are the folders under
//!   the projects directory and the folders with a project mark ([`crate::Project`], such as an
//!   export made elsewhere) wherever they are; tree mode keeps those under their parents. Other is
//!   the folders where most photos are small pictures, Hidden Folders
//!   the hidden ones (Hide Folder; only with Hidden Pictures on, sections included); the rest are
//!   Folders. Sorted by date, the
//!   Folders group gets a year divider before each year when its folders span more than one year.
//! - **Tree mode:** the disk hierarchy under each watched root: a row per directory on the way from
//!   a root to a folder with something to show, each with its own count (a parent often shows 0).
//!   Folders under no watched root are tops of their own. Siblings sort by the same keys taken over
//!   their whole subtree.
//! - **Hidden:** a hidden folder ([`crate::FolderInfo::hidden`]) and a hidden photo show only with
//!   Hidden Pictures on: in flat mode the folder moves to Hidden Folders, in tree mode it keeps its
//!   place marked [`FolderRow::hidden`]. With the option off they leave the tree and the sections.
//! - **Dates** are effective dates: a photo's date set by the user, else its file's, else its
//!   modification time; a folder's date is the one set in Folder Properties, else its oldest photo
//!   shown.
//! - **Sorts:** by date (a folder's date, newest folder first), by recent changes (newest
//!   photo modification time first), by size (most bytes first) and by name (A to Z); `reverse`
//!   turns each around. Undated folders stay first under the header in both directions.
//! - **Counts** are the photos a row shows (small pictures only with Small Pictures on, hidden ones
//!   only with Hidden Pictures on); a group's
//!   count is its library folders with photos shown, those inside collapsed tree nodes included
//!   (year dividers and tree mode's directory rows aside), so tree mode's Folders count is the
//!   flat groups' counts added up.
//! - **Offline:** folders under a watched root whose volume is offline (`data-model.md` §7) keep
//!   their rows, counts and sections; [`TreeOptions::is_offline`] says which rows to dim.
//! - **Sections:** the light table's sections are the rows with photos of the expanded groups, in
//!   row order; a collapsed group drops its rows and its sections. Collapsing a tree node hides the
//!   rows below it but keeps their sections (the light table still shows the whole library).
//!
//! - **The section filter** ([`TreeOptions::filter`], search mode): only the photos in the set
//!   count and show; folders, directories and albums left with none drop out with their sections.
//!
//! **The Albums group** is built apart ([`AlbumTree::build`]) so the folder rows keep their types:
//! the search album first while searching (named after the search, listing every photo of the
//! filter in its order), Starred Photos and Recently Updated when they hold a photo shown, then
//! the user albums (an empty one too, except in search mode) in the tree's folder sort: by date
//! (the album's date, newest first), by recent changes (its newest member's file time), by size
//! (its members' bytes) or by name. [`AlbumTree::sections_with`] puts the album sections before
//! the folder sections, and [`section_photos`] lists each section's photos in its order: an
//! album's by position key, Starred Photos by date, Recently Updated newest first, a folder's in
//! its own photo sort ([`crate::PhotoSort`]).
//!
//! Work is one pass over the photos and one sort per level: no per-frame cost. Callers cache the
//! tree per (library revision, roots, options); [`Tree::revision`] says which revision it shows.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use dalica_core::{AlbumId, Date, DateTime, FolderId, PhotoId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::model::{Library, Photo};

/// The default small-picture threshold (`formats.md` §6): smaller than this on both sides.
pub const SMALL_PICTURE_PX: u32 = 64;
/// How many days back Recently Updated looks by default (the setting's default).
pub const RECENT_DAYS: u32 = 30;

/// How the Folders group lays out folders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TreeMode {
    /// Every folder as one row (the default).
    #[default]
    Flat,
    /// The disk hierarchy under each watched root.
    Tree,
}

/// The folder sort of View ▸ Folder View.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderSort {
    /// A folder's oldest photo date, newest folder first (the default).
    #[default]
    Date,
    /// The newest photo modification time, most recent first.
    RecentChanges,
    /// Total bytes of the photos shown, largest first.
    Size,
    /// The folder name, A to Z, ignoring case.
    Name,
}

/// A group of the library tree, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// Every group, in display order.
    pub const ALL: [Group; 6] = [Group::Albums, Group::People, Group::Projects, Group::Folders, Group::Other, Group::Hidden];

    /// The id used in widget ids (`tree:group:<id>`, `control-protocol.md` §7).
    pub fn id(self) -> &'static str {
        match self {
            Group::Albums => "albums",
            Group::People => "people",
            Group::Projects => "projects",
            Group::Folders => "folders",
            Group::Other => "other",
            Group::Hidden => "hidden",
        }
    }
}

/// A light-table section. Albums and people join when the model holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionId {
    Folder(FolderId),
}

impl fmt::Display for SectionId {
    /// `folder:12`, as `control-protocol.md` §6.4 names sections.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SectionId::Folder(id) => write!(f, "folder:{id}"),
        }
    }
}

/// An entry of the Albums group, in display order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlbumRef {
    /// The temporary album of the current search ("Search results for …").
    Search,
    /// Starred Photos.
    Starred,
    /// Recently Updated.
    Recent,
    /// A user album.
    User(AlbumId),
}

impl fmt::Display for AlbumRef {
    /// `search`, `starred`, `recent` or the album id: the part after `album:` in a section name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlbumRef::Search => f.write_str("search"),
            AlbumRef::Starred => f.write_str("starred"),
            AlbumRef::Recent => f.write_str("recent"),
            AlbumRef::User(id) => write!(f, "{id}"),
        }
    }
}

/// A light-table section of any kind: an album's or a folder's. [`SectionId`] names the folder
/// sections [`Tree`] lists; this one adds the albums of [`AlbumTree`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Album(AlbumRef),
    Folder(FolderId),
}

impl From<SectionId> for Section {
    fn from(id: SectionId) -> Section {
        match id {
            SectionId::Folder(f) => Section::Folder(f),
        }
    }
}

impl fmt::Display for Section {
    /// `folder:12`, `album:starred`, `album:recent`, `album:search` or `album:a91c0e`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Section::Album(a) => write!(f, "album:{a}"),
            Section::Folder(id) => write!(f, "folder:{id}"),
        }
    }
}

impl FromStr for Section {
    type Err = crate::Error;

    /// Reads what [`Section`]'s `Display` writes.
    fn from_str(s: &str) -> Result<Section, crate::Error> {
        let bad = crate::Error::BadText { what: "section", why: "not folder:<id>, album:<id>, album:starred, album:recent or album:search" };
        match s.split_once(':') {
            Some(("folder", id)) => id.parse().map(Section::Folder).map_err(|_| bad),
            Some(("album", "search")) => Ok(Section::Album(AlbumRef::Search)),
            Some(("album", "starred")) => Ok(Section::Album(AlbumRef::Starred)),
            Some(("album", "recent")) => Ok(Section::Album(AlbumRef::Recent)),
            Some(("album", id)) => AlbumId::parse(id).map(|id| Section::Album(AlbumRef::User(id))).map_err(|_| bad),
            _ => Err(bad),
        }
    }
}

/// The photos search mode shows (`organizing.md` §8.1), in the order the search gives them: the
/// tree's sections without one of them drop out, and the search album lists them all.
#[derive(Clone, Debug, Default)]
pub struct SectionFilter {
    order: Vec<PhotoId>,
    set: HashSet<PhotoId>,
}

impl SectionFilter {
    /// The filter of `ids`, in that order (a repeat counts once, at its first place).
    pub fn new(ids: impl IntoIterator<Item = PhotoId>) -> SectionFilter {
        let mut out = SectionFilter::default();
        for id in ids {
            if out.set.insert(id) {
                out.order.push(id);
            }
        }
        out
    }

    pub fn contains(&self, id: PhotoId) -> bool {
        self.set.contains(&id)
    }

    /// The photos in the search's order.
    pub fn ids(&self) -> &[PhotoId] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

impl PartialEq for SectionFilter {
    fn eq(&self, other: &SectionFilter) -> bool {
        self.order == other.order
    }
}

impl Eq for SectionFilter {}

/// What the tree depends on besides the library and the watched roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeOptions {
    pub mode: TreeMode,
    pub sort: FolderSort,
    pub reverse: bool,
    /// Collapsed groups: their rows and sections are left out.
    pub collapsed_groups: BTreeSet<Group>,
    /// Tree mode: directories whose children are folded away (their sections stay).
    pub collapsed_folders: BTreeSet<PathBuf>,
    /// View ▸ Small Pictures.
    pub small_pictures: bool,
    /// View ▸ Hidden Pictures: hidden photos and folders are shown (flat mode: hidden folders in
    /// Hidden Folders).
    pub hidden_pictures: bool,
    /// The directory holding the project folders (`Pictures/Dalica`), if known.
    pub projects_dir: Option<PathBuf>,
    /// Options ▸ File Types: pictures smaller than this on both sides are small.
    pub small_threshold: u32,
    /// Watched roots whose volume is offline (the session's online checks): the rows under them
    /// stay, and [`TreeOptions::is_offline`] marks them.
    pub offline_roots: BTreeSet<PathBuf>,
    /// Search mode: only these photos count and show.
    pub filter: Option<Arc<SectionFilter>>,
    /// The search text while searching: the Albums group then starts with the search album.
    pub search: Option<String>,
    /// Now, for Recently Updated (no such album without it).
    pub now: Option<Timestamp>,
    /// How many days back Recently Updated looks.
    pub recent_days: u32,
}

impl TreeOptions {
    /// Whether the row at `path` is under an offline root (drawn dimmed with an "offline" marker).
    /// It keeps its place, count and section.
    pub fn is_offline(&self, path: &Path) -> bool {
        self.offline_roots.iter().any(|r| path.starts_with(r))
    }
}

impl Default for TreeOptions {
    fn default() -> TreeOptions {
        TreeOptions {
            mode: TreeMode::Flat,
            sort: FolderSort::Date,
            reverse: false,
            collapsed_groups: BTreeSet::new(),
            collapsed_folders: BTreeSet::new(),
            small_pictures: false,
            hidden_pictures: false,
            projects_dir: None,
            small_threshold: SMALL_PICTURE_PX,
            offline_roots: BTreeSet::new(),
            filter: None,
            search: None,
            now: None,
            recent_days: RECENT_DAYS,
        }
    }
}

/// One folder or directory row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    /// The library folder at `path`; `None` for a directory between a watched root and its folders.
    pub folder: Option<FolderId>,
    pub path: PathBuf,
    /// The directory name (the whole path when it has none).
    pub name: String,
    /// Photos shown directly in this folder.
    pub count: usize,
    /// Indent level: 0 in flat mode and for tree tops.
    pub depth: usize,
    /// The folder date (its oldest photo shown); tree mode: over the subtree.
    pub date: Option<DateTime>,
    /// Tree mode: has rows below it (a disclosure triangle).
    pub has_children: bool,
    /// Tree mode: its children are shown.
    pub expanded: bool,
    /// Hidden with Hide Folder (shown because Hidden Pictures is on).
    pub hidden: bool,
}

impl FolderRow {
    /// The section this row scrolls to, if it shows photos.
    pub fn section(&self) -> Option<SectionId> {
        match self.folder {
            Some(id) if self.count > 0 => Some(SectionId::Folder(id)),
            _ => None,
        }
    }
}

/// A row of a group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Row {
    /// A year divider (flat mode, Folders group, sorted by date).
    Year(u16),
    Folder(FolderRow),
}

/// A group with its rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRows {
    pub group: Group,
    pub collapsed: bool,
    /// The header count: library folders with photos shown, including those folded away (not
    /// year dividers, not tree mode's directory rows).
    pub count: usize,
    /// Empty when collapsed.
    pub rows: Vec<Row>,
}

/// The library tree for one library revision and one set of options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    /// The library revision this tree shows.
    pub revision: u64,
    pub mode: TreeMode,
    pub groups: Vec<GroupRows>,
    /// The light table's sections, in order: the rows with photos of the expanded groups.
    pub sections: Vec<SectionId>,
}

/// What one folder holds, counted once per build.
#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    /// Every photo, shown or not, and the small ones among them (for Other).
    all: usize,
    small: usize,
    /// The photos shown.
    count: usize,
    bytes: u64,
    date: Option<DateTime>,
    mtime: Option<Timestamp>,
}

impl Stats {
    fn add_shown(&mut self, p: &Photo, date: Option<DateTime>) {
        self.count = self.count.saturating_add(1);
        self.bytes = self.bytes.saturating_add(p.size);
        if let Some(d) = date {
            self.date = Some(self.date.map_or(d, |old| old.min(d)));
        }
        self.mtime = Some(self.mtime.map_or(p.mtime, |old| old.max(p.mtime)));
    }

    /// Adds a subtree's shown photos (tree mode sort keys).
    fn merge(&mut self, o: &Stats) {
        self.count = self.count.saturating_add(o.count);
        self.bytes = self.bytes.saturating_add(o.bytes);
        self.date = match (self.date, o.date) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.mtime = self.mtime.max(o.mtime);
    }
}

/// What a row sorts by.
struct Key {
    stats: Stats,
    /// Lowercased name, then the name, path and id break ties.
    fold: String,
    name: String,
    path: PathBuf,
    folder: Option<FolderId>,
}

fn compare(a: &Key, b: &Key, sort: FolderSort, reverse: bool) -> Ordering {
    // Undated folders stay first under the header in both directions.
    if sort == FolderSort::Date {
        let undated = a.stats.date.is_some().cmp(&b.stats.date.is_some());
        if undated != Ordering::Equal {
            return undated;
        }
    }
    let primary = match sort {
        FolderSort::Date => b.stats.date.cmp(&a.stats.date),
        FolderSort::RecentChanges => b.stats.mtime.cmp(&a.stats.mtime),
        FolderSort::Size => b.stats.bytes.cmp(&a.stats.bytes),
        FolderSort::Name => Ordering::Equal,
    };
    let order = primary
        .then_with(|| a.fold.cmp(&b.fold))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.path.cmp(&b.path))
        .then_with(|| a.folder.cmp(&b.folder));
    if reverse { order.reverse() } else { order }
}

fn name_of(path: &Path) -> String {
    match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => path.to_string_lossy().into_owned(),
    }
}

fn key(path: &Path, folder: Option<FolderId>, stats: Stats) -> Key {
    let name = name_of(path);
    Key { stats, fold: name.to_lowercase(), name, path: path.to_path_buf(), folder }
}

/// A folder date as the start of its day, to compare with photo dates.
fn midnight(date: Date) -> Option<DateTime> {
    DateTime::new(date, 0, 0, 0).ok()
}

fn is_small(p: &Photo, threshold: u32) -> bool {
    p.width > 0 && p.height > 0 && p.width < threshold && p.height < threshold
}

impl Tree {
    /// Builds the tree of `library` for the watched `roots` and `options`.
    pub fn build(library: &Library, roots: &[PathBuf], options: &TreeOptions) -> Tree {
        let mut stats: HashMap<FolderId, Stats> = HashMap::new();
        for p in library.photos() {
            let s = stats.entry(p.folder).or_default();
            s.all = s.all.saturating_add(1);
            let small = is_small(p, options.small_threshold);
            if small {
                s.small = s.small.saturating_add(1);
            }
            let info = library.info(p.id);
            let in_filter = options.filter.as_ref().is_none_or(|f| f.contains(p.id));
            if in_filter && (!small || options.small_pictures) && (options.hidden_pictures || !info.hidden) {
                s.add_shown(p, info.date.or_else(|| p.sort_date()));
            }
        }
        // A hidden folder shows nothing unless Hidden Pictures is on; a folder with a date of its
        // own sorts by it.
        let shown = |id: FolderId| -> Stats {
            let s = stats.get(&id).copied().unwrap_or_default();
            let info = library.folder_info(id);
            if info.hidden && !options.hidden_pictures {
                Stats { count: 0, bytes: 0, date: None, mtime: None, ..s }
            } else if let Some(date) = info.date.filter(|_| s.count > 0).and_then(midnight) {
                Stats { date: Some(date), ..s }
            } else {
                s
            }
        };
        let groups = match options.mode {
            TreeMode::Flat => flat(library, options, &shown),
            TreeMode::Tree => vec![(Group::Folders, tree(library, roots, options, &shown))],
        };
        let mut out = Tree { revision: library.revision(), mode: options.mode, groups: Vec::new(), sections: Vec::new() };
        for (group, (rows, (sections, folded))) in groups {
            let count = rows.iter().filter(|r| matches!(r, Row::Folder(f) if f.section().is_some())).count().saturating_add(folded);
            if count == 0 {
                continue;
            }
            let collapsed = options.collapsed_groups.contains(&group);
            if collapsed {
                out.groups.push(GroupRows { group, collapsed, count, rows: Vec::new() });
            } else {
                out.sections.extend(sections);
                out.groups.push(GroupRows { group, collapsed, count, rows });
            }
        }
        out
    }

    /// Every row of the expanded groups, in order.
    pub fn rows(&self) -> impl Iterator<Item = (Group, &Row)> {
        self.groups.iter().flat_map(|g| g.rows.iter().map(move |r| (g.group, r)))
    }

    /// The group, if it is shown.
    pub fn group(&self, group: Group) -> Option<&GroupRows> {
        self.groups.iter().find(|g| g.group == group)
    }
}

/// Rows, then (sections in order, library folders with photos folded away).
type Built = (Vec<Row>, (Vec<SectionId>, usize));

fn flat(library: &Library, options: &TreeOptions, shown: &dyn Fn(FolderId) -> Stats) -> Vec<(Group, Built)> {
    let mut buckets: [(Group, Vec<Key>); 4] =
        [(Group::Projects, Vec::new()), (Group::Folders, Vec::new()), (Group::Other, Vec::new()), (Group::Hidden, Vec::new())];
    for f in library.folders() {
        let s = shown(f.id);
        if s.count == 0 {
            continue;
        }
        let group = if library.is_folder_hidden(f.id) {
            Group::Hidden
        } else if library.project(f.id).is_some() || options.projects_dir.as_deref().is_some_and(|d| f.path.starts_with(d)) {
            Group::Projects
        } else if s.small.saturating_mul(2) > s.all {
            Group::Other
        } else {
            Group::Folders
        };
        if let Some((_, keys)) = buckets.iter_mut().find(|(g, _)| *g == group) {
            keys.push(key(&f.path, Some(f.id), s));
        }
    }
    buckets
        .into_iter()
        .map(|(group, mut keys)| {
            keys.sort_by(|a, b| compare(a, b, options.sort, options.reverse));
            let years: BTreeSet<u16> = keys.iter().filter_map(|k| k.stats.date).map(|d| d.date().year()).collect();
            let dividers = group == Group::Folders && options.sort == FolderSort::Date && years.len() > 1;
            let mut rows = Vec::with_capacity(keys.len());
            let mut sections = Vec::with_capacity(keys.len());
            let mut year = None;
            for k in keys {
                let y = k.stats.date.map(|d| d.date().year());
                if dividers && y.is_some() && y != year {
                    year = y;
                    if let Some(y) = y {
                        rows.push(Row::Year(y));
                    }
                }
                let row = FolderRow {
                    folder: k.folder,
                    name: k.name,
                    count: k.stats.count,
                    depth: 0,
                    date: k.stats.date,
                    has_children: false,
                    expanded: true,
                    hidden: k.folder.is_some_and(|id| library.is_folder_hidden(id)),
                    path: k.path,
                };
                sections.extend(row.section());
                rows.push(Row::Folder(row));
            }
            (group, (rows, (sections, 0)))
        })
        .collect()
}

/// A tree-mode node: a directory on the way from a top to a folder with something to show.
struct Node {
    path: PathBuf,
    folder: Option<FolderId>,
    own: Stats,
    subtree: Stats,
    depth: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

fn tree(library: &Library, roots: &[PathBuf], options: &TreeOptions, shown: &dyn Fn(FolderId) -> Stats) -> Built {
    // Folders with something to show.
    let leaves: Vec<(PathBuf, FolderId, Stats)> =
        library.folders().filter_map(|f| Some((f.path.clone(), f.id, shown(f.id))).filter(|(_, _, s)| s.count > 0)).collect();
    // Tops: the watched roots, plus folders under none of them; none inside another.
    let mut tops: Vec<PathBuf> = roots.to_vec();
    tops.extend(leaves.iter().filter(|(p, _, _)| !roots.iter().any(|r| p.starts_with(r))).map(|(p, _, _)| p.clone()));
    tops.sort();
    tops.dedup();
    let mut nested_free: Vec<PathBuf> = Vec::with_capacity(tops.len());
    for t in tops {
        // Sorted by components, a directory's descendants follow it directly.
        if nested_free.last().is_some_and(|last| t.starts_with(last)) {
            continue;
        }
        nested_free.push(t);
    }
    let tops: HashSet<PathBuf> = nested_free.into_iter().collect();

    let mut nodes: Vec<Node> = Vec::new();
    let mut index: HashMap<PathBuf, usize> = HashMap::new();
    for (path, _, _) in &leaves {
        // The chain from the folder up to its top, bounded by the path's own depth.
        let mut chain: Vec<&Path> = Vec::new();
        let mut top_found = false;
        for a in path.ancestors() {
            chain.push(a);
            if tops.contains(a) {
                top_found = true;
                break;
            }
        }
        if !top_found {
            continue;
        }
        let mut parent: Option<usize> = None;
        let mut depth = 0usize;
        for a in chain.into_iter().rev() {
            let at = match index.get(a) {
                Some(&i) => i,
                None => {
                    let folder = library.folder_by_path(a);
                    let own = folder.map(shown).unwrap_or_default();
                    let i = nodes.len();
                    nodes.push(Node { path: a.to_path_buf(), folder, own, subtree: own, depth, parent, children: Vec::new() });
                    index.insert(a.to_path_buf(), i);
                    if let Some(node) = parent.and_then(|p| nodes.get_mut(p)) {
                        node.children.push(i);
                    }
                    i
                }
            };
            parent = Some(at);
            depth = depth.saturating_add(1);
        }
    }
    // Subtree keys, deepest nodes first.
    let mut by_depth: Vec<usize> = (0..nodes.len()).collect();
    by_depth.sort_by_key(|&i| std::cmp::Reverse(nodes.get(i).map_or(0, |n| n.depth)));
    for i in by_depth {
        let Some((sub, parent)) = nodes.get(i).map(|n| (n.subtree, n.parent)) else { continue };
        if let Some(p) = parent.and_then(|p| nodes.get_mut(p)) {
            p.subtree.merge(&sub);
        }
    }
    let keys: Vec<Key> = nodes.iter().map(|n| key(&n.path, n.folder, n.subtree)).collect();
    let order = |list: &mut Vec<usize>| {
        list.sort_by(|&a, &b| match (keys.get(a), keys.get(b)) {
            (Some(ka), Some(kb)) => compare(ka, kb, options.sort, options.reverse),
            _ => a.cmp(&b),
        })
    };
    let mut top_list: Vec<usize> = nodes.iter().enumerate().filter(|(_, n)| n.parent.is_none()).map(|(i, _)| i).collect();
    order(&mut top_list);
    let mut children: Vec<Vec<usize>> = nodes.iter().map(|n| n.children.clone()).collect();
    for list in &mut children {
        order(list);
    }

    // Depth-first, without recursion. A collapsed node's descendants keep their sections.
    let mut rows = Vec::with_capacity(nodes.len());
    let mut sections = Vec::new();
    let mut folded = 0usize;
    // (node, inside a collapsed node)
    let mut stack: Vec<(usize, bool)> = top_list.into_iter().rev().map(|i| (i, false)).collect();
    while let Some((i, folded_away)) = stack.pop() {
        let Some(n) = nodes.get(i) else { continue };
        let kids = children.get(i).map(Vec::as_slice).unwrap_or_default();
        let expanded = !options.collapsed_folders.contains(&n.path);
        let row = FolderRow {
            folder: n.folder,
            path: n.path.clone(),
            name: name_of(&n.path),
            count: n.own.count,
            depth: n.depth,
            date: n.subtree.date,
            has_children: !kids.is_empty(),
            expanded,
            hidden: n.folder.is_some_and(|id| library.is_folder_hidden(id)),
        };
        sections.extend(row.section());
        if folded_away {
            if row.section().is_some() {
                folded = folded.saturating_add(1);
            }
        } else {
            rows.push(Row::Folder(row));
        }
        let below = folded_away || !expanded;
        stack.extend(kids.iter().rev().map(|&k| (k, below)));
    }
    (rows, (sections, folded))
}

/// Whether `p` shows under `options`: small pictures only with Small Pictures on, hidden photos
/// and photos of hidden folders only with Hidden Pictures on, and only photos of the filter.
fn visible(library: &Library, p: &Photo, options: &TreeOptions) -> bool {
    let small = is_small(p, options.small_threshold);
    let hidden = library.is_hidden(p.id) || library.is_folder_hidden(p.folder);
    (!small || options.small_pictures) && (options.hidden_pictures || !hidden) && options.filter.as_ref().is_none_or(|f| f.contains(p.id))
}

/// One row of the Albums group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRow {
    pub album: AlbumRef,
    /// A user album's name; the automatic albums' names in English until localisation (M15):
    /// `Search results for "x"`, `Starred Photos`, `Recently Updated`.
    pub name: String,
    /// The photos it shows.
    pub count: usize,
    /// A user album's date ([`Library::album_date`]); `None` for the automatic ones.
    pub date: Option<DateTime>,
    /// The photo it shows as its icon: a user album's cover ([`Library::album_cover`]), else the
    /// first photo it shows.
    pub cover: Option<PhotoId>,
}

impl AlbumRow {
    /// The section this row scrolls to, if it shows photos.
    pub fn section(&self) -> Option<Section> {
        (self.count > 0).then(|| Section::Album(self.album.clone()))
    }
}

/// The Albums group for one library revision and one set of options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumTree {
    /// The library revision it shows.
    pub revision: u64,
    pub collapsed: bool,
    /// The header count: its rows, those of a collapsed group included. Zero: no group.
    pub count: usize,
    /// Empty when collapsed.
    pub rows: Vec<AlbumRow>,
    /// The album sections, in row order (none when collapsed).
    pub sections: Vec<Section>,
}

impl AlbumTree {
    /// The Albums group of `library` for `options` (see the module docs).
    pub fn build(library: &Library, options: &TreeOptions) -> AlbumTree {
        let shown = |id: &PhotoId| library.photo(*id).is_some_and(|p| visible(library, p, options));
        let first_shown = |ids: &[PhotoId]| ids.iter().find(|id| shown(id)).copied();
        let mut rows = Vec::new();
        if let Some(text) = &options.search {
            let ids = options.filter.as_ref().map(|f| f.ids()).unwrap_or_default();
            let count = ids.iter().filter(|id| shown(id)).count();
            rows.push(AlbumRow {
                album: AlbumRef::Search,
                name: format!("Search results for \"{text}\""),
                count,
                date: None,
                cover: first_shown(ids),
            });
        }
        let starred = library.starred_photos();
        let count = starred.iter().filter(|id| shown(id)).count();
        if count > 0 {
            rows.push(AlbumRow { album: AlbumRef::Starred, name: "Starred Photos".into(), count, date: None, cover: first_shown(starred) });
        }
        if let Some(now) = options.now {
            let recent = library.recently_updated(now, options.recent_days);
            let count = recent.iter().filter(|id| shown(id)).count();
            if count > 0 {
                rows.push(AlbumRow { album: AlbumRef::Recent, name: "Recently Updated".into(), count, date: None, cover: first_shown(&recent) });
            }
        }
        let mut keys: Vec<(Key, AlbumRow)> = Vec::with_capacity(library.model.albums.len());
        for (id, info) in library.albums() {
            let mut stats = Stats::default();
            for p in library.album_members(id).iter().filter_map(|m| library.photo(*m)).filter(|p| visible(library, p, options)) {
                stats.add_shown(p, None);
            }
            if stats.count == 0 && options.filter.is_some() {
                continue;
            }
            let album = library.album_stats(id);
            stats.date = album.date;
            let fold = info.name.to_lowercase();
            let key = Key { stats, fold, name: info.name.clone(), path: PathBuf::from(id.as_str()), folder: None };
            let row =
                AlbumRow { album: AlbumRef::User(id.clone()), name: info.name.clone(), count: stats.count, date: album.date, cover: album.cover };
            keys.push((key, row));
        }
        keys.sort_by(|(a, _), (b, _)| compare(a, b, options.sort, options.reverse));
        rows.extend(keys.into_iter().map(|(_, row)| row));
        let collapsed = options.collapsed_groups.contains(&Group::Albums);
        let count = rows.len();
        if collapsed || count == 0 {
            return AlbumTree { revision: library.revision(), collapsed, count, rows: Vec::new(), sections: Vec::new() };
        }
        let sections = rows.iter().filter_map(AlbumRow::section).collect();
        AlbumTree { revision: library.revision(), collapsed, count, rows, sections }
    }

    /// Every light-table section in order: the album sections, then `folders`' sections.
    pub fn sections_with(&self, folders: &Tree) -> Vec<Section> {
        self.sections.iter().cloned().chain(folders.sections.iter().map(|s| Section::from(*s))).collect()
    }
}

/// The photos of each of `sections` that show under `options`, in the section's order: a user
/// album's by position key, Starred Photos by date, Recently Updated newest first (none without
/// [`TreeOptions::now`]), the search album in the filter's order, a folder's in its own photo
/// sort. One pass over the photos for all the folders asked for.
pub fn section_photos(library: &Library, sections: &[Section], options: &TreeOptions) -> Vec<(Section, Vec<PhotoId>)> {
    let shown = |ids: &[PhotoId]| -> Vec<PhotoId> {
        ids.iter().copied().filter(|id| library.photo(*id).is_some_and(|p| visible(library, p, options))).collect()
    };
    let wanted: HashSet<FolderId> = sections
        .iter()
        .filter_map(|s| match s {
            Section::Folder(id) => Some(*id),
            Section::Album(_) => None,
        })
        .collect();
    let mut by_folder: HashMap<FolderId, Vec<&Photo>> = HashMap::with_capacity(wanted.len());
    if !wanted.is_empty() {
        for p in library.photos().filter(|p| wanted.contains(&p.folder) && visible(library, p, options)) {
            by_folder.entry(p.folder).or_default().push(p);
        }
    }
    sections
        .iter()
        .map(|s| {
            let ids = match s {
                Section::Folder(id) => {
                    let photos = by_folder.get(id).map(Vec::as_slice).unwrap_or_default();
                    library.sorted(photos.iter().copied(), library.folder_sort(*id))
                }
                Section::Album(AlbumRef::User(a)) => shown(library.album_members(a)),
                Section::Album(AlbumRef::Starred) => shown(library.starred_photos()),
                Section::Album(AlbumRef::Recent) => {
                    options.now.map(|now| shown(&library.recently_updated(now, options.recent_days))).unwrap_or_default()
                }
                Section::Album(AlbumRef::Search) => shown(options.filter.as_ref().map(|f| f.ids()).unwrap_or_default()),
            };
            (s.clone(), ids)
        })
        .collect()
}
