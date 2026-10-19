//! `dalica-library` tree model (`ui-layout.md` §4, `organizing.md` §10) on hand-built libraries:
//! rows and order in flat and tree mode for every sort and its reverse, year dividers, counts,
//! groups (Projects holding folders with the Exported mark wherever they are), collapsed groups
//! and nodes, sections, offline roots, and the build time of a 5 k-folder library. M9: the Albums
//! group (automatic albums, then user albums in the folder sort), the section filter of search
//! mode in flat and tree mode with the search album, each section's photos in its order, and
//! section names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dalica_core::{AlbumId, FolderId, PhotoId, Timestamp};
use dalica_library::{
    AlbumInfo, AlbumRef, AlbumTree, Folder, FolderRow, FolderSort, Group, Library, Op, Photo, PhotoSort, Row, Section, SectionFilter, SectionId,
    SortBy, Tree, TreeMode, TreeOptions, section_photos,
};
use proptest::prelude::*;
use serde_json::json;

fn base() -> PathBuf {
    PathBuf::from("/photos")
}

/// A library built by hand through ops.
struct Fx {
    lib: Library,
}

/// One photo: capture date (`None` for a file without one), bytes, modification time in seconds,
/// and its size in pixels.
#[derive(Clone, Copy)]
struct P {
    date: Option<(u16, u8, u8)>,
    size: u64,
    mtime: i64,
    px: (u32, u32),
}

fn p(year: u16, month: u8, day: u8) -> P {
    P { date: Some((year, month, day)), size: 1000, mtime: 1_700_000_000, px: (480, 320) }
}

impl Fx {
    fn new() -> Fx {
        Fx { lib: Library::new() }
    }

    fn folder_at(&mut self, path: PathBuf) -> FolderId {
        let id = self.lib.next_folder_id();
        self.lib.apply(Op::AddFolder { folder: Folder { id, path } }).unwrap();
        id
    }

    fn folder(&mut self, rel: &str) -> FolderId {
        let mut path = base();
        for part in rel.split('/') {
            path = path.join(part);
        }
        self.folder_at(path)
    }

    fn photos(&mut self, folder: FolderId, list: &[P]) {
        let photos: Vec<Photo> = list
            .iter()
            .map(|p| {
                let id = self.lib.next_photo_id();
                let date = p.date.map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}T12:00:00"));
                // Built from JSON so fields the model gains later take their defaults.
                serde_json::from_value(json!({
                    "id": id, "folder": folder, "name": format!("IMG_{}.jpg", id.get()), "size": p.size,
                    "mtime": p.mtime * 1000, "width": p.px.0, "height": p.px.1, "date": date, "star": false
                }))
                .unwrap()
            })
            .collect();
        self.lib.apply(Op::AddPhotos { photos }).unwrap();
    }

    fn tree(&self, roots: &[PathBuf], options: &TreeOptions) -> Tree {
        Tree::build(&self.lib, roots, options)
    }
}

fn opts(mode: TreeMode, sort: FolderSort, reverse: bool) -> TreeOptions {
    TreeOptions { mode, sort, reverse, ..TreeOptions::default() }
}

/// The rows of one group as short strings: `2026` for a divider, `name (count)` for a folder,
/// indented two spaces per tree level.
fn lines(tree: &Tree, group: Group) -> Vec<String> {
    tree.group(group)
        .map(|g| {
            g.rows
                .iter()
                .map(|r| match r {
                    Row::Year(y) => format!("{y}"),
                    Row::Folder(f) => format!("{}{} ({})", "  ".repeat(f.depth), f.name, f.count),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn folder_rows(tree: &Tree) -> Vec<&FolderRow> {
    tree.rows()
        .filter_map(|(_, r)| match r {
            Row::Folder(f) => Some(f),
            Row::Year(_) => None,
        })
        .collect()
}

fn groups(tree: &Tree) -> Vec<Group> {
    tree.groups.iter().map(|g| g.group).collect()
}

/// Five folders over three years with distinct sizes, change times and names:
///
/// | folder | date | bytes | newest mtime |
/// |---|---|---|---|
/// | Beach | 2024-07-01 | 3 × 1000 | 500 |
/// | attic | 2025-01-10 | 1 × 9000 | 100 |
/// | Concert | 2025-11-20 | 2 × 1000 | 400 |
/// | Dunes | 2026-03-02 | 1 × 5000 | 300 |
/// | Easter | 2026-04-05 | 4 × 1000 | 200 |
fn five_folders() -> Fx {
    let mut fx = Fx::new();
    let mut add = |name: &str, date: (u16, u8, u8), n: usize, size: u64, mtime: i64| {
        let f = fx.folder(name);
        let list: Vec<P> =
            (0..n).map(|i| P { date: Some((date.0, date.1, date.2 + i as u8)), size, mtime: mtime - i as i64, px: (480, 320) }).collect();
        fx.photos(f, &list);
    };
    add("Beach", (2024, 7, 1), 3, 1000, 500);
    add("attic", (2025, 1, 10), 1, 9000, 100);
    add("Concert", (2025, 11, 20), 2, 1000, 400);
    add("Dunes", (2026, 3, 2), 1, 5000, 300);
    add("Easter", (2026, 4, 5), 4, 1000, 200);
    fx
}

#[test]
fn flat_by_date_newest_first_with_year_dividers() {
    let fx = five_folders();
    let t = fx.tree(&[base()], &TreeOptions::default());
    assert_eq!(groups(&t), [Group::Folders]);
    assert_eq!(lines(&t, Group::Folders), ["2026", "Easter (4)", "Dunes (1)", "2025", "Concert (2)", "attic (1)", "2024", "Beach (3)"]);
    let reverse = fx.tree(&[base()], &opts(TreeMode::Flat, FolderSort::Date, true));
    assert_eq!(lines(&reverse, Group::Folders), ["2024", "Beach (3)", "2025", "attic (1)", "Concert (2)", "2026", "Dunes (1)", "Easter (4)"]);
}

#[test]
fn every_sort_and_its_reverse() {
    let fx = five_folders();
    let cases: [(FolderSort, [&str; 5]); 4] = [
        (FolderSort::Date, ["Easter (4)", "Dunes (1)", "Concert (2)", "attic (1)", "Beach (3)"]),
        (FolderSort::RecentChanges, ["Beach (3)", "Concert (2)", "Dunes (1)", "Easter (4)", "attic (1)"]),
        (FolderSort::Size, ["attic (1)", "Dunes (1)", "Easter (4)", "Beach (3)", "Concert (2)"]),
        (FolderSort::Name, ["attic (1)", "Beach (3)", "Concert (2)", "Dunes (1)", "Easter (4)"]),
    ];
    for mode in [TreeMode::Flat, TreeMode::Tree] {
        for (sort, want) in cases {
            let depth = if mode == TreeMode::Tree { "  " } else { "" };
            let want: Vec<String> = want.iter().map(|w| format!("{depth}{w}")).collect();
            let rows = |reverse| -> Vec<String> {
                lines(&fx.tree(&[base()], &opts(mode, sort, reverse)), Group::Folders)
                    .into_iter()
                    .filter(|l| !l.chars().all(|c| c.is_ascii_digit()) && l != "photos (0)")
                    .collect()
            };
            assert_eq!(rows(false), want, "{mode:?} {sort:?}");
            let mut reversed = want.clone();
            reversed.reverse();
            assert_eq!(rows(true), reversed, "{mode:?} {sort:?} reversed");
        }
    }
}

#[test]
fn year_dividers_only_across_years_and_only_by_date() {
    let mut fx = Fx::new();
    for (name, day) in [("a", 1), ("b", 9), ("c", 20)] {
        let f = fx.folder(name);
        fx.photos(f, &[p(2026, 5, day)]);
    }
    let t = fx.tree(&[base()], &TreeOptions::default());
    assert_eq!(lines(&t, Group::Folders), ["c (1)", "b (1)", "a (1)"], "one year: no divider");

    let fx = five_folders();
    for sort in [FolderSort::RecentChanges, FolderSort::Size, FolderSort::Name] {
        for reverse in [false, true] {
            let t = fx.tree(&[base()], &opts(TreeMode::Flat, sort, reverse));
            assert!(t.rows().all(|(_, r)| matches!(r, Row::Folder(_))), "{sort:?}: no dividers");
        }
    }
    let t = fx.tree(&[base()], &opts(TreeMode::Tree, FolderSort::Date, false));
    assert!(t.rows().all(|(_, r)| matches!(r, Row::Folder(_))), "tree mode: no dividers");
}

#[test]
fn counts_follow_small_pictures() {
    let mut fx = Fx::new();
    let a = fx.folder("a");
    let tiny = P { px: (32, 40), ..p(2026, 1, 1) };
    let wide = P { px: (32, 640), ..p(2026, 1, 2) };
    let unknown = P { px: (0, 0), ..p(2026, 1, 3) };
    fx.photos(a, &[p(2026, 1, 1), p(2026, 1, 2), tiny, wide, unknown]);
    let t = fx.tree(&[base()], &TreeOptions::default());
    assert_eq!(lines(&t, Group::Folders), ["a (4)"], "a 32 × 40 picture is small; 32 × 640 and unknown sizes are not");
    assert_eq!(t.group(Group::Folders).map(|g| g.count), Some(1));
    let shown = fx.tree(&[base()], &TreeOptions { small_pictures: true, ..TreeOptions::default() });
    assert_eq!(lines(&shown, Group::Folders), ["a (5)"]);
    let edge = fx.tree(&[base()], &TreeOptions { small_threshold: 32, ..TreeOptions::default() });
    assert_eq!(lines(&edge, Group::Folders), ["a (5)"], "smaller than the threshold, not equal to it");
}

/// A library with a folder in every flat group.
fn every_group() -> (Fx, TreeOptions) {
    let mut fx = Fx::new();
    let projects = base().join("Dalica");
    let exported = fx.folder("Dalica/Exported");
    fx.photos(exported, &[p(2026, 2, 1)]);
    let trip = fx.folder("Trip");
    fx.photos(trip, &[p(2026, 3, 1), p(2026, 3, 2)]);
    let icons = fx.folder("icons");
    let tiny = P { px: (16, 16), ..p(2020, 1, 1) };
    fx.photos(icons, &[tiny, tiny, p(2020, 1, 2)]);
    let secret = fx.folder("Secret");
    fx.photos(secret, &[p(2026, 4, 1)]);
    fx.folder("Empty");
    fx.lib.apply(Op::SetFoldersHidden { hidden: vec![(secret, true)] }).unwrap();
    let options = TreeOptions { projects_dir: Some(projects), hidden_pictures: true, small_pictures: true, ..TreeOptions::default() };
    (fx, options)
}

#[test]
fn flat_groups_in_order_each_only_when_non_empty() {
    let (fx, options) = every_group();
    let t = fx.tree(&[base()], &options);
    assert_eq!(groups(&t), [Group::Projects, Group::Folders, Group::Other, Group::Hidden]);
    assert_eq!(lines(&t, Group::Projects), ["Exported (1)"]);
    assert_eq!(lines(&t, Group::Folders), ["Trip (2)"], "an empty folder disappears");
    assert_eq!(lines(&t, Group::Other), ["icons (3)"], "most pictures small");
    assert_eq!(lines(&t, Group::Hidden), ["Secret (1)"]);
    assert!(t.group(Group::Hidden).is_some_and(|g| g.rows.iter().all(|r| matches!(r, Row::Folder(f) if f.hidden))));
    let ids: Vec<&str> = t.groups.iter().map(|g| g.group.id()).collect();
    assert_eq!(ids, ["projects", "folders", "other", "hidden"]);

    // Hidden Pictures off: the hidden folder is gone. Small Pictures off: Other keeps its one
    // ordinary picture.
    let plain = fx.tree(&[base()], &TreeOptions { hidden_pictures: false, small_pictures: false, ..options.clone() });
    assert_eq!(groups(&plain), [Group::Projects, Group::Folders, Group::Other]);
    assert_eq!(lines(&plain, Group::Other), ["icons (1)"]);
    assert!(!plain.sections.contains(&SectionId::Folder(FolderId::new(4))), "no section for the hidden folder");
}

#[test]
fn tree_mode_has_only_folders_in_the_disk_hierarchy() {
    let (fx, options) = every_group();
    let t = fx.tree(&[base()], &TreeOptions { mode: TreeMode::Tree, sort: FolderSort::Name, ..options.clone() });
    assert_eq!(groups(&t), [Group::Folders]);
    assert_eq!(lines(&t, Group::Folders), ["photos (0)", "  Dalica (0)", "    Exported (1)", "  icons (3)", "  Secret (1)", "  Trip (2)"]);
    let rows = folder_rows(&t);
    assert_eq!(rows.first().map(|r| (r.folder, r.has_children, r.path.clone())), Some((None, true, base())));
    assert!(rows.iter().find(|r| r.name == "Secret").is_some_and(|r| r.hidden));
    // Hidden Pictures off: the hidden folder leaves the tree.
    let t = fx.tree(&[base()], &TreeOptions { mode: TreeMode::Tree, sort: FolderSort::Name, hidden_pictures: false, ..options });
    assert_eq!(lines(&t, Group::Folders), ["photos (0)", "  Dalica (0)", "    Exported (1)", "  icons (3)", "  Trip (2)"]);
}

#[test]
fn tree_parents_show_their_own_count_and_sort_by_their_subtree() {
    let mut fx = Fx::new();
    let year = fx.folder("2025");
    fx.photos(year, &[p(2025, 12, 31)]);
    let jan = fx.folder("2025/January");
    fx.photos(jan, &[p(2025, 1, 5), p(2025, 1, 6)]);
    let old = fx.folder("2019/Wedding");
    fx.photos(old, &[p(2019, 6, 1)]);
    let t = fx.tree(&[base()], &opts(TreeMode::Tree, FolderSort::Date, false));
    // 2025's subtree starts in January 2025, which is newer than 2019.
    assert_eq!(lines(&t, Group::Folders), ["photos (0)", "  2025 (1)", "    January (2)", "  2019 (0)", "    Wedding (1)"]);
    assert_eq!(t.group(Group::Folders).map(|g| g.count), Some(3), "library folders only: the photos and 2019 directories do not count");
    let rows = folder_rows(&t);
    assert_eq!(rows.iter().map(|r| r.depth).collect::<Vec<_>>(), [0, 1, 2, 1, 2]);
    assert_eq!(rows.iter().map(|r| r.has_children).collect::<Vec<_>>(), [true, true, false, true, false]);
    assert_eq!(rows.iter().map(|r| r.folder.is_some()).collect::<Vec<_>>(), [false, true, true, false, true]);
    // Parents with photos of their own are sections; "(0)" directories are not.
    assert_eq!(t.sections, [SectionId::Folder(year), SectionId::Folder(jan), SectionId::Folder(old)]);
}

#[test]
fn tree_tops_are_roots_and_folders_under_no_root() {
    let mut fx = Fx::new();
    let a = fx.folder("a/inner");
    fx.photos(a, &[p(2026, 1, 1)]);
    let elsewhere = fx.folder_at(PathBuf::from("/elsewhere").join("Scans"));
    fx.photos(elsewhere, &[p(2020, 1, 1)]);
    let nested = fx.folder_at(PathBuf::from("/elsewhere").join("Scans").join("More"));
    fx.photos(nested, &[p(2021, 1, 1)]);
    // A root inside another root is not a top of its own; an empty root shows nothing.
    let roots = [base(), base().join("a"), PathBuf::from("/empty")];
    let t = fx.tree(&roots, &opts(TreeMode::Tree, FolderSort::Name, false));
    assert_eq!(lines(&t, Group::Folders), ["photos (0)", "  a (0)", "    inner (1)", "Scans (1)", "  More (1)"]);
    // No roots at all: every folder tree starts at its outermost folder.
    let t = fx.tree(&[], &opts(TreeMode::Tree, FolderSort::Name, false));
    assert_eq!(lines(&t, Group::Folders), ["inner (1)", "Scans (1)", "  More (1)"]);
}

#[test]
fn a_collapsed_tree_node_hides_rows_but_keeps_sections() {
    let mut fx = Fx::new();
    let a = fx.folder("a");
    fx.photos(a, &[p(2026, 1, 1)]);
    let b = fx.folder("a/b");
    fx.photos(b, &[p(2026, 2, 1)]);
    let c = fx.folder("a/b/c");
    fx.photos(c, &[p(2026, 3, 1)]);
    let options = TreeOptions { mode: TreeMode::Tree, collapsed_folders: BTreeSet::from([base().join("a")]), ..TreeOptions::default() };
    let t = fx.tree(&[base()], &options);
    assert_eq!(lines(&t, Group::Folders), ["photos (0)", "  a (1)"]);
    assert!(folder_rows(&t).iter().find(|r| r.name == "a").is_some_and(|r| r.has_children && !r.expanded));
    assert_eq!(t.group(Group::Folders).map(|g| g.count), Some(3), "folded folders still count; the photos directory does not");
    assert_eq!(t.sections, [SectionId::Folder(a), SectionId::Folder(b), SectionId::Folder(c)]);
}

/// Found at M4.R: tree mode said "Folders (212)" where flat mode had 200 folders, because it
/// counted the directory rows between the roots and the folders.
#[test]
fn tree_mode_counts_the_same_folders_as_flat_mode() {
    let (fx, options) = every_group();
    let flat: usize = fx.tree(&[base()], &options).groups.iter().map(|g| g.count).sum();
    let tree = fx.tree(&[base()], &TreeOptions { mode: TreeMode::Tree, ..options });
    assert_eq!(flat, 4);
    assert_eq!(tree.group(Group::Folders).map(|g| g.count), Some(flat));
    assert!(folder_rows(&tree).iter().any(|r| r.folder.is_none()), "the tree has directory rows, and they are not counted");
}

/// `data-model.md` §7: folders under an offline root keep their rows, counts and sections, and
/// are marked offline; folders under other roots are not.
#[test]
fn offline_roots_keep_their_rows_and_are_marked() {
    let mut fx = Fx::new();
    let home = fx.folder("Trip");
    fx.photos(home, &[p(2026, 1, 1)]);
    let card = PathBuf::from("/media").join("CARD");
    let dcim = fx.folder_at(card.join("DCIM"));
    fx.photos(dcim, &[p(2026, 2, 1), p(2026, 2, 2)]);
    let roots = [base(), card.clone()];
    for mode in [TreeMode::Flat, TreeMode::Tree] {
        let online = TreeOptions { mode, ..TreeOptions::default() };
        let offline = TreeOptions { offline_roots: BTreeSet::from([card.clone()]), ..online.clone() };
        let (a, b) = (fx.tree(&roots, &online), fx.tree(&roots, &offline));
        assert_eq!(a, b, "{mode:?}: an offline root changes no row, count or section");
        let marked: Vec<(String, bool)> = folder_rows(&b).iter().map(|r| (r.name.clone(), offline.is_offline(&r.path))).collect();
        let want: Vec<(String, bool)> = match mode {
            TreeMode::Flat => vec![("DCIM".into(), true), ("Trip".into(), false)],
            TreeMode::Tree => vec![("CARD".into(), true), ("DCIM".into(), true), ("photos".into(), false), ("Trip".into(), false)],
        };
        assert_eq!(marked, want, "{mode:?}");
        assert!(folder_rows(&a).iter().all(|r| !online.is_offline(&r.path)));
    }
    // A sibling whose name merely starts like the root is not under it.
    let options = TreeOptions { offline_roots: BTreeSet::from([card.clone()]), ..TreeOptions::default() };
    assert!(!options.is_offline(&PathBuf::from("/media").join("CARD2").join("DCIM")));
}

#[test]
fn a_collapsed_group_drops_its_rows_and_sections() {
    let (fx, options) = every_group();
    let open = fx.tree(&[base()], &options);
    let section_ids = |t: &Tree| -> Vec<String> { t.sections.iter().map(ToString::to_string).collect() };
    assert_eq!(section_ids(&open), ["folder:1", "folder:2", "folder:3", "folder:4"], "sections follow the rows");
    let rows_in_order: Vec<SectionId> = folder_rows(&open).iter().filter_map(|r| r.section()).collect();
    assert_eq!(open.sections, rows_in_order);

    let options = TreeOptions { collapsed_groups: BTreeSet::from([Group::Folders, Group::Other]), ..options };
    let t = fx.tree(&[base()], &options);
    assert_eq!(groups(&t), [Group::Projects, Group::Folders, Group::Other, Group::Hidden], "collapsed groups keep their header");
    let folders = t.group(Group::Folders).unwrap();
    assert!(folders.collapsed && folders.rows.is_empty());
    assert_eq!(folders.count, 1);
    assert_eq!(section_ids(&t), ["folder:1", "folder:4"]);
}

#[test]
fn an_empty_library_has_no_groups() {
    let mut fx = Fx::new();
    let _ = fx.folder("nothing");
    for mode in [TreeMode::Flat, TreeMode::Tree] {
        let t = fx.tree(&[base()], &opts(mode, FolderSort::Date, false));
        assert!(t.groups.is_empty() && t.sections.is_empty(), "{mode:?}");
    }
    assert_eq!(Tree::build(&Library::new(), &[], &TreeOptions::default()), Tree::default());
}

#[test]
fn undated_folders_stay_first_and_the_revision_is_recorded() {
    let mut fx = five_folders();
    let undated = fx.folder("zz undated");
    // A modification time no calendar date can hold, and no capture date.
    fx.photos(undated, &[P { date: None, size: 1, mtime: i64::MAX / 1000, px: (480, 320) }]);
    for reverse in [false, true] {
        let t = fx.tree(&[base()], &opts(TreeMode::Flat, FolderSort::Date, reverse));
        assert_eq!(lines(&t, Group::Folders).first().map(String::as_str), Some("zz undated (1)"), "reverse {reverse}");
        assert_eq!(t.revision, fx.lib.revision());
    }
}

#[test]
fn a_photo_without_a_capture_date_sorts_by_its_file_time() {
    let mut fx = Fx::new();
    let scans = fx.folder("scans");
    // 1_800_000_000 s is 2027-01-15 (UTC).
    fx.photos(scans, &[P { date: None, size: 1, mtime: 1_800_000_000, px: (480, 320) }]);
    let trip = fx.folder("trip");
    fx.photos(trip, &[p(2026, 1, 1)]);
    let t = fx.tree(&[base()], &TreeOptions::default());
    assert_eq!(lines(&t, Group::Folders), ["2027", "scans (1)", "2026", "trip (1)"]);
}

/// At full strength 5 000 folders; in a task's gate 500, the same checks.
#[test]
fn five_thousand_folders_build_quickly() {
    let folders_n: usize = if crate::common::full() { 5_000 } else { 500 };
    const PER: usize = 20;
    let mut fx = Fx::new();
    let mut photos = Vec::with_capacity(folders_n * PER);
    let mut folders = Vec::with_capacity(folders_n);
    for i in 0..folders_n {
        let id = fx.lib.next_folder_id();
        let year = 2000 + (i % 27);
        folders.push(Folder { id, path: base().join(format!("{year}")).join(format!("event {i:04}")) });
        for k in 0..PER {
            let pid = fx.lib.next_photo_id();
            photos.push(json!({
                "id": pid, "folder": id, "name": format!("IMG_{k:04}.jpg"), "size": 1000 + i * 7 + k,
                "mtime": 1_700_000_000_000i64 + (i * 977 % 5_000) as i64 * 1000, "width": 480, "height": 320,
                "date": format!("{year:04}-{:02}-{:02}T10:{:02}:00", 1 + i % 12, 1 + i % 28, k % 60), "star": false
            }));
        }
    }
    let photos: Vec<Photo> = serde_json::from_value(serde_json::Value::Array(photos)).unwrap();
    let ops = folders.into_iter().map(|folder| Op::AddFolder { folder }).chain([Op::AddPhotos { photos }]).collect();
    fx.lib.apply(Op::Batch { ops }).unwrap();
    for mode in [TreeMode::Flat, TreeMode::Tree] {
        for sort in [FolderSort::Date, FolderSort::RecentChanges, FolderSort::Size, FolderSort::Name] {
            let start = Instant::now();
            let t = fx.tree(&[base()], &opts(mode, sort, false));
            let took = start.elapsed();
            println!("{folders_n} folders, {} photos, {mode:?} by {sort:?}: {took:?}", folders_n * PER);
            assert_eq!(t.sections.len(), folders_n);
            // Generous: catches quadratic work even in an unoptimised build on a slow runner.
            assert!(took.as_secs_f64() < 5.0, "{mode:?} {sort:?} took {took:?}");
        }
    }
}

// Property tests over random libraries.

/// A random photo: (day offset, size, small).
type SpecPhoto = (u16, u16, bool);

#[derive(Clone, Debug)]
struct Spec {
    /// (folder path index, its photos)
    folders: Vec<(u8, Vec<SpecPhoto>)>,
    hidden: Vec<u8>,
    /// Indices into `OFFLINE`: the roots whose volume is offline.
    offline: Vec<u8>,
}

/// Candidate offline roots (relative to `base()`; empty for `base()` itself).
const OFFLINE: [&str; 4] = ["", "a", "a/b", "d"];

fn offline_roots(spec: &Spec) -> BTreeSet<PathBuf> {
    spec.offline.iter().map(|&i| OFFLINE[usize::from(i)].split('/').filter(|p| !p.is_empty()).fold(base(), |path, part| path.join(part))).collect()
}

const PATHS: [&str; 8] = ["a", "a/b", "a/b/c", "b", "d/e", "d/f", "Dalica/Collages", "x/y/z"];

fn spec() -> impl Strategy<Value = Spec> {
    (
        proptest::collection::vec((0u8..8, proptest::collection::vec((0u16..2000, 1u16..5000, any::<bool>()), 0..5)), 0..8),
        proptest::collection::vec(0u8..8, 0..3),
        proptest::collection::vec(0u8..4, 0..3),
    )
        .prop_map(|(folders, hidden, offline)| Spec { folders, hidden, offline })
}

fn library(spec: &Spec) -> (Fx, BTreeSet<FolderId>) {
    let mut fx = Fx::new();
    let mut ids: Vec<(u8, FolderId)> = Vec::new();
    for (path, list) in &spec.folders {
        let id = match ids.iter().find(|(p, _)| p == path) {
            Some((_, id)) => *id,
            None => {
                let id = fx.folder(PATHS[usize::from(*path)]);
                ids.push((*path, id));
                id
            }
        };
        let list: Vec<P> = list
            .iter()
            .map(|&(day, size, small)| P {
                date: Some((2020 + (day / 365), 1 + ((day / 30) % 12) as u8, 1 + (day % 28) as u8)),
                size: u64::from(size),
                mtime: 1_700_000_000 + i64::from(size) * 37,
                px: if small { (20, 20) } else { (400, 300) },
            })
            .collect();
        fx.photos(id, &list);
    }
    let hidden: BTreeSet<FolderId> = spec.hidden.iter().filter_map(|h| ids.iter().find(|(p, _)| p == h).map(|(_, id)| *id)).collect();
    fx.lib.apply(Op::SetFoldersHidden { hidden: hidden.iter().map(|&id| (id, true)).collect() }).unwrap();
    (fx, hidden)
}

fn sorts() -> impl Strategy<Value = FolderSort> {
    prop_oneof![Just(FolderSort::Date), Just(FolderSort::RecentChanges), Just(FolderSort::Size), Just(FolderSort::Name)]
}

fn shown(photo: &Photo, small_pictures: bool) -> bool {
    small_pictures || !(photo.width < 64 && photo.height < 64)
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    #[test]
    fn every_shown_photo_is_counted_once_and_sections_follow_rows(
        spec in spec(), sort in sorts(), reverse: bool, tree_mode: bool, small: bool, hidden_pictures: bool,
    ) {
        let (fx, hidden) = library(&spec);
        let mode = if tree_mode { TreeMode::Tree } else { TreeMode::Flat };
        let options = TreeOptions {
            mode, sort, reverse, small_pictures: small, hidden_pictures,
            projects_dir: Some(base().join("Dalica")), ..TreeOptions::default()
        };
        let t = fx.tree(&[base()], &options);
        let rows = folder_rows(&t);
        // Counts add up to the photos shown.
        let want: usize = fx.lib.photos()
            .filter(|p| shown(p, small) && (hidden_pictures || !hidden.contains(&p.folder)))
            .count();
        prop_assert_eq!(rows.iter().map(|r| r.count).sum::<usize>(), want);
        // Each folder appears at most once; sections are the rows with photos, in order.
        let ids: Vec<FolderId> = rows.iter().filter_map(|r| r.folder).collect();
        let unique: BTreeSet<FolderId> = ids.iter().copied().collect();
        prop_assert_eq!(unique.len(), ids.len());
        let sections: Vec<SectionId> = rows.iter().filter_map(|r| r.section()).collect();
        prop_assert_eq!(&t.sections, &sections);
        // Group order and headers.
        let order: Vec<Group> = groups(&t);
        let mut sorted = order.clone();
        sorted.sort();
        prop_assert_eq!(&order, &sorted);
        for g in &t.groups {
            prop_assert!(g.count > 0);
            if mode == TreeMode::Tree {
                prop_assert_eq!(g.group, Group::Folders);
            }
            if g.group == Group::Hidden {
                prop_assert!(hidden_pictures);
            }
        }
        // Reverse turns the folder order around (flat mode, apart from dividers).
        if mode == TreeMode::Flat {
            let back = fx.tree(&[base()], &TreeOptions { reverse: !reverse, ..options.clone() });
            for g in &t.groups {
                let mut names: Vec<&Path> = lines_of(g.group, &t);
                names.reverse();
                prop_assert_eq!(names, lines_of(g.group, &back));
            }
        }
        // Headers count library folders with photos: tree mode's one group counts what the flat
        // groups count together.
        let other = TreeOptions { mode: if tree_mode { TreeMode::Flat } else { TreeMode::Tree }, ..options.clone() };
        let counted = |t: &Tree| t.groups.iter().map(|g| g.count).sum::<usize>();
        prop_assert_eq!(counted(&t), counted(&fx.tree(&[base()], &other)));
        prop_assert_eq!(counted(&t), sections.len());
        // Offline roots change no row, count or section; a row is offline exactly when it is under
        // one of them.
        let roots = offline_roots(&spec);
        let offline = TreeOptions { offline_roots: roots.clone(), ..options.clone() };
        let dimmed = fx.tree(&[base()], &offline);
        prop_assert_eq!(&dimmed, &t);
        for r in folder_rows(&dimmed) {
            prop_assert_eq!(offline.is_offline(&r.path), roots.iter().any(|root| r.path.starts_with(root)));
            prop_assert!(!options.is_offline(&r.path));
        }
        // Collapsing every group drops every row and section and keeps the headers.
        let closed = fx.tree(&[base()], &TreeOptions { collapsed_groups: Group::ALL.into_iter().collect(), ..options });
        prop_assert!(closed.sections.is_empty() && closed.rows().next().is_none());
        prop_assert_eq!(closed.groups.iter().map(|g| (g.group, g.count)).collect::<Vec<_>>(),
            t.groups.iter().map(|g| (g.group, g.count)).collect::<Vec<_>>());
    }
}

fn lines_of(group: Group, t: &Tree) -> Vec<&Path> {
    t.group(group)
        .map(|g| {
            g.rows
                .iter()
                .filter_map(|r| match r {
                    Row::Folder(f) => Some(f.path.as_path()),
                    Row::Year(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn photo_ids_in_json_round_trip() {
    // The fixture builds photos from JSON; make sure that keeps their ids.
    let mut fx = Fx::new();
    let f = fx.folder("a");
    fx.photos(f, &[p(2026, 1, 1)]);
    assert_eq!(fx.lib.photos().map(|p| p.id).collect::<Vec<_>>(), [PhotoId::new(1)]);
}

// M6: hidden folders and photos from the model, effective dates and folder dates.

/// Every row of the tree as `group: line`, for comparing whole trees.
fn all_lines(t: &Tree) -> Vec<String> {
    t.groups.iter().flat_map(|g| lines(t, g.group).into_iter().map(move |l| format!("{}: {l}", g.group.id()))).collect()
}

#[test]
fn hidden_folders_in_both_modes_with_hidden_pictures_on_and_off() {
    let mut fx = Fx::new();
    let trip = fx.folder("Trip");
    fx.photos(trip, &[p(2026, 3, 1), p(2026, 3, 2)]);
    let secret = fx.folder("a/Secret");
    fx.photos(secret, &[p(2026, 4, 1)]);
    let before = fx.tree(&[base()], &TreeOptions::default());
    let undo = fx.lib.apply(Op::SetFoldersHidden { hidden: vec![(secret, true)] }).unwrap();
    assert!(fx.lib.is_folder_hidden(secret));
    let on = TreeOptions { hidden_pictures: true, ..TreeOptions::default() };
    let off = TreeOptions::default();

    // Flat, on: the folder moves to Hidden Folders, marked hidden, with its section after Folders'.
    let t = fx.tree(&[base()], &on);
    assert_eq!(all_lines(&t), ["folders: Trip (2)", "hidden: Secret (1)"]);
    assert_eq!(t.sections, [SectionId::Folder(trip), SectionId::Folder(secret)]);
    assert!(folder_rows(&t).iter().find(|r| r.folder == Some(secret)).is_some_and(|r| r.hidden));
    // Flat, off: no Hidden Folders group, no section.
    let t = fx.tree(&[base()], &off);
    assert_eq!(all_lines(&t), ["folders: Trip (2)"]);
    assert_eq!(t.sections, [SectionId::Folder(trip)]);
    // Tree, on: the folder keeps its place in the disk hierarchy, marked hidden.
    let tree_on = TreeOptions { mode: TreeMode::Tree, sort: FolderSort::Name, ..on.clone() };
    let t = fx.tree(&[base()], &tree_on);
    assert_eq!(all_lines(&t), ["folders: photos (0)", "folders:   a (0)", "folders:     Secret (1)", "folders:   Trip (2)"]);
    assert_eq!(t.sections, [SectionId::Folder(secret), SectionId::Folder(trip)]);
    assert!(folder_rows(&t).iter().find(|r| r.folder == Some(secret)).is_some_and(|r| r.hidden));
    // Tree, off: the folder and the directory that only led to it are gone.
    let t = fx.tree(&[base()], &TreeOptions { hidden_pictures: false, ..tree_on });
    assert_eq!(all_lines(&t), ["folders: photos (0)", "folders:   Trip (2)"]);
    assert_eq!(t.sections, [SectionId::Folder(trip)]);

    // Unhiding (the inverse) puts everything back.
    fx.lib.apply(undo).unwrap();
    assert_eq!(fx.tree(&[base()], &TreeOptions::default()).groups, before.groups);
}

#[test]
fn hidden_photos_are_counted_only_with_hidden_pictures() {
    let mut fx = Fx::new();
    let a = fx.folder("a");
    fx.photos(a, &[p(2026, 1, 1), p(2026, 1, 2), p(2026, 1, 3)]);
    let b = fx.folder("b");
    fx.photos(b, &[p(2025, 6, 1)]);
    let in_a: Vec<PhotoId> = fx.lib.photos_in(a).iter().map(|p| p.id).collect();
    let in_b: Vec<PhotoId> = fx.lib.photos_in(b).iter().map(|p| p.id).collect();
    fx.lib.apply(Op::SetHidden { hidden: vec![(in_a[0], true), (in_b[0], true)] }).unwrap();
    let off = fx.tree(&[base()], &TreeOptions::default());
    assert_eq!(all_lines(&off), ["folders: a (2)"], "a folder whose photos are all hidden disappears");
    assert_eq!(off.sections, [SectionId::Folder(a)]);
    // The hidden photo no longer gives its folder its date either.
    assert_eq!(folder_rows(&off).first().and_then(|r| r.date).map(|d| d.to_string()), Some("2026-01-02T12:00:00".into()));
    let on = fx.tree(&[base()], &TreeOptions { hidden_pictures: true, ..TreeOptions::default() });
    assert_eq!(all_lines(&on), ["folders: 2026", "folders: a (3)", "folders: 2025", "folders: b (1)"]);
    let tree = fx.tree(&[base()], &TreeOptions { mode: TreeMode::Tree, ..TreeOptions::default() });
    assert_eq!(all_lines(&tree), ["folders: photos (0)", "folders:   a (2)"]);
}

#[test]
fn folder_dates_and_sorts_use_effective_dates() {
    let mut fx = five_folders();
    let id = |fx: &Fx, name: &str| fx.lib.folders().find(|f| f.path.ends_with(name)).map(|f| f.id).unwrap();
    let by_date = |fx: &Fx| lines(&fx.tree(&[base()], &TreeOptions::default()), Group::Folders);
    assert_eq!(by_date(&fx), ["2026", "Easter (4)", "Dunes (1)", "2025", "Concert (2)", "attic (1)", "2024", "Beach (3)"]);

    // A date set by the user on attic's one photo moves attic to 2023; the file's date stays.
    let attic = id(&fx, "attic");
    let photo = fx.lib.photos_in(attic)[0].clone();
    let user: dalica_core::DateTime = "2023-05-05T08:00:00".parse().unwrap();
    let undo = fx.lib.apply(Op::SetDates { dates: vec![(photo.id, Some(user))] }).unwrap();
    assert_eq!(fx.lib.effective_date(photo.id), Some(user));
    assert_eq!(fx.lib.photo(photo.id).unwrap().date, photo.date, "the file's own date is kept beside it");
    assert_eq!(fx.lib.sort_date(&photo), Some(user));
    assert_eq!(by_date(&fx), ["2026", "Easter (4)", "Dunes (1)", "2025", "Concert (2)", "2024", "Beach (3)", "2023", "attic (1)"]);
    fx.lib.apply(undo).unwrap();
    assert_eq!(fx.lib.effective_date(photo.id), photo.date);

    // A folder date set in Folder Properties beats its oldest photo's, in flat and tree mode.
    let beach = id(&fx, "Beach");
    let info = dalica_library::FolderInfo { date: Some("2027-01-01".parse().unwrap()), ..Default::default() };
    fx.lib.apply(Op::SetFolderInfo { folders: vec![(beach, info)] }).unwrap();
    assert_eq!(by_date(&fx), ["2027", "Beach (3)", "2026", "Easter (4)", "Dunes (1)", "2025", "Concert (2)", "attic (1)"]);
    let t = fx.tree(&[base()], &opts(TreeMode::Tree, FolderSort::Date, false));
    assert_eq!(lines(&t, Group::Folders).get(1).map(String::as_str), Some("  Beach (3)"));
    let row = folder_rows(&t).into_iter().find(|r| r.folder == Some(beach)).cloned();
    assert_eq!(row.and_then(|r| r.date).map(|d| d.to_string()), Some("2027-01-01T00:00:00".into()));
}

/// A folder with the Exported mark lists in Projects wherever it is (an export made elsewhere) in
/// flat mode, and stays under its parent in tree mode; a hidden one still goes to Hidden Folders.
#[test]
fn exported_folders_list_in_projects_wherever_they_are() {
    let (mut fx, options) = every_group();
    let outside = fx.folder("Trip/Exports/Lisbon");
    fx.photos(outside, &[p(2026, 3, 5), p(2026, 3, 6)]);
    let flat = |fx: &Fx| fx.tree(&[base()], &options);
    assert_eq!(lines(&flat(&fx), Group::Projects), ["Exported (1)"]);
    assert_eq!(lines(&flat(&fx), Group::Folders), ["Lisbon (2)", "Trip (2)"]);

    let undo = fx.lib.apply(Op::SetProjects { projects: vec![(outside, Some(dalica_library::Project::Exported))] }).unwrap();
    let t = flat(&fx);
    assert_eq!(lines(&t, Group::Projects), ["Lisbon (2)", "Exported (1)"], "sorted with the folders under the projects directory");
    assert_eq!(lines(&t, Group::Folders), ["Trip (2)"]);
    assert_eq!(t.group(Group::Projects).map(|g| g.count), Some(2));
    // No projects directory known: the mark alone puts it there.
    let t = fx.tree(&[base()], &TreeOptions { projects_dir: None, ..options.clone() });
    assert_eq!(lines(&t, Group::Projects), ["Lisbon (2)"]);
    // Tree mode: under its parent, as on disk.
    let t = fx.tree(&[base()], &TreeOptions { mode: TreeMode::Tree, sort: FolderSort::Name, ..options.clone() });
    assert_eq!(groups(&t), [Group::Folders]);
    assert_eq!(
        lines(&t, Group::Folders),
        ["photos (0)", "  Dalica (0)", "    Exported (1)", "  icons (3)", "  Secret (1)", "  Trip (2)", "    Exports (0)", "      Lisbon (2)"]
    );
    // Hidden wins over the mark.
    fx.lib.apply(Op::SetFoldersHidden { hidden: vec![(outside, true)] }).unwrap();
    assert_eq!(lines(&flat(&fx), Group::Hidden), ["Secret (1)", "Lisbon (2)"]);
    fx.lib.apply(Op::SetFoldersHidden { hidden: vec![(outside, false)] }).unwrap();
    // Undoing the mark puts it back in Folders.
    fx.lib.apply(undo).unwrap();
    assert_eq!(lines(&flat(&fx), Group::Folders), ["Lisbon (2)", "Trip (2)"]);
}

/// A folder marked Collages lists in Projects wherever it is, as an Exported one does, and the mark
/// says which kind of project it is (the window draws the collage icon from it).
#[test]
fn a_collages_folder_lists_in_projects() {
    let (mut fx, options) = every_group();
    let collages = fx.folder("Trip/Collages");
    fx.photos(collages, &[p(2026, 5, 1)]);
    let flat = |fx: &Fx| fx.tree(&[base()], &options);
    assert_eq!(lines(&flat(&fx), Group::Projects), ["Exported (1)"]);
    let undo = fx.lib.apply(Op::SetProjects { projects: vec![(collages, Some(dalica_library::Project::Collages))] }).unwrap();
    assert_eq!(lines(&flat(&fx), Group::Projects), ["Collages (1)", "Exported (1)"]);
    assert_eq!(lines(&flat(&fx), Group::Folders), ["Trip (2)"]);
    assert_eq!(fx.lib.project(collages), Some(dalica_library::Project::Collages));
    fx.lib.apply(undo).unwrap();
    assert_eq!(lines(&flat(&fx), Group::Folders), ["Collages (1)", "Trip (2)"]);
}

// M9: the Albums group, album sections, the section filter of search mode and section photos.

fn aid(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

/// The photos of `folder` in id order.
fn ids_in(fx: &Fx, folder: FolderId) -> Vec<PhotoId> {
    fx.lib.photos().filter(|p| p.folder == folder).map(|p| p.id).collect()
}

fn folder_named(fx: &Fx, name: &str) -> FolderId {
    fx.lib.folders().find(|f| f.path.file_name().is_some_and(|n| n == name)).unwrap().id
}

/// The five folders with three albums: `Zoo` (empty), `alpha` (two Beach photos and one Easter
/// photo, dated by its newest photo) and `Mid` (one Concert photo, its own date 2025-06-01);
/// two Beach photos and an Easter photo starred. Every photo was first indexed at time 0, before
/// its file's time, so each counts as Recently Updated by its file's time.
fn with_albums() -> (Fx, [AlbumId; 3]) {
    let mut fx = five_folders();
    let all: Vec<PhotoId> = fx.lib.photos().map(|p| p.id).collect();
    let indexed = fx.lib.first_indexed_op(&all, Timestamp::EPOCH);
    fx.lib.apply(indexed).unwrap();
    let (beach, easter, concert) = (folder_named(&fx, "Beach"), folder_named(&fx, "Easter"), folder_named(&fx, "Concert"));
    let albums = [aid("a0001"), aid("a0002"), aid("a0003")];
    let mid = AlbumInfo { date: "2025-06-01".parse().ok(), use_newest: false, ..AlbumInfo::named("Mid") };
    fx.lib
        .apply(Op::CreateAlbums {
            albums: vec![(albums[0].clone(), AlbumInfo::named("Zoo")), (albums[1].clone(), AlbumInfo::named("alpha")), (albums[2].clone(), mid)],
        })
        .unwrap();
    let b = ids_in(&fx, beach);
    let e = ids_in(&fx, easter);
    let c = ids_in(&fx, concert);
    let op = fx.lib.album_add_op(&albums[1], &[e[0], b[1], b[0]], None).unwrap();
    fx.lib.apply(op).unwrap();
    let op = fx.lib.album_add_op(&albums[2], &c[..1], None).unwrap();
    fx.lib.apply(op).unwrap();
    fx.lib.apply(Op::SetStars { stars: vec![(b[0], true), (b[2], true), (e[1], true)] }).unwrap();
    (fx, albums)
}

fn album_lines(t: &AlbumTree) -> Vec<String> {
    t.rows.iter().map(|r| format!("{} ({})", r.name, r.count)).collect()
}

#[test]
fn the_albums_group_lists_automatic_then_user_albums_in_the_folder_sort() {
    let (fx, albums) = with_albums();
    // No album, no star, no clock: no group.
    assert_eq!(AlbumTree::build(&five_folders().lib, &TreeOptions::default()).count, 0);

    // Every photo's file time is within 30 days of this now.
    let now = Timestamp::from_millis(600_000);
    let options = TreeOptions { now: Some(now), ..TreeOptions::default() };
    let t = AlbumTree::build(&fx.lib, &options);
    assert_eq!(t.revision, fx.lib.revision());
    // By date: the undated empty album first, then newest first (alpha: Easter 2026-04; Mid: its
    // own 2025-06-01).
    assert_eq!(album_lines(&t), ["Starred Photos (3)", "Recently Updated (11)", "Zoo (0)", "alpha (3)", "Mid (1)"]);
    assert_eq!(t.count, 5);
    let sections: Vec<String> = t.sections.iter().map(Section::to_string).collect();
    assert_eq!(sections, ["album:starred", "album:recent", "album:a0002", "album:a0003"], "an empty album has no section");
    let alpha = &t.rows[3];
    assert_eq!(
        (alpha.album.clone(), alpha.cover, alpha.date),
        (AlbumRef::User(albums[1].clone()), fx.lib.album_cover(&albums[1]), fx.lib.album_date(&albums[1]))
    );

    let by_name = |reverse| album_lines(&AlbumTree::build(&fx.lib, &TreeOptions { sort: FolderSort::Name, reverse, ..options.clone() }));
    assert_eq!(by_name(false)[2..], ["alpha (3)", "Mid (1)", "Zoo (0)"], "ignoring case");
    assert_eq!(by_name(true)[2..], ["Zoo (0)", "Mid (1)", "alpha (3)"], "the automatic albums stay first");
    let by_size = album_lines(&AlbumTree::build(&fx.lib, &TreeOptions { sort: FolderSort::Size, ..options.clone() }));
    assert_eq!(by_size[2..], ["alpha (3)", "Mid (1)", "Zoo (0)"]);

    // Recently Updated follows the clock; collapsed, the group keeps its count only.
    let later = TreeOptions { now: Some(Timestamp::from_millis(31 * 86_400_000)), ..options.clone() };
    assert!(!album_lines(&AlbumTree::build(&fx.lib, &later)).iter().any(|l| l.starts_with("Recently")));
    let collapsed = TreeOptions { collapsed_groups: [Group::Albums].into(), ..options.clone() };
    let t = AlbumTree::build(&fx.lib, &collapsed);
    assert!(t.collapsed && t.count == 5 && t.rows.is_empty() && t.sections.is_empty());
    // A hidden photo leaves the counts unless Hidden Pictures is on.
    let mut fx = fx;
    let starred = fx.lib.starred_photos()[0];
    fx.lib.apply(Op::SetHidden { hidden: vec![(starred, true)] }).unwrap();
    assert_eq!(album_lines(&AlbumTree::build(&fx.lib, &options))[0], "Starred Photos (2)");
    assert_eq!(album_lines(&AlbumTree::build(&fx.lib, &TreeOptions { hidden_pictures: true, ..options }))[0], "Starred Photos (3)");
}

#[test]
fn the_section_filter_drops_sections_without_matches_in_flat_and_tree_mode() {
    let (fx, albums) = with_albums();
    let (beach, dunes) = (folder_named(&fx, "Beach"), folder_named(&fx, "Dunes"));
    let b = ids_in(&fx, beach);
    let d = ids_in(&fx, dunes);
    // The search found the second Beach photo, the Dunes photo and the first Beach photo, in
    // that order.
    let filter = Arc::new(SectionFilter::new([b[1], d[0], b[0], b[1]]));
    assert_eq!(filter.len(), 3);
    let searching = |mode| TreeOptions { mode, filter: Some(filter.clone()), search: Some("beach".into()), ..TreeOptions::default() };

    let flat = Tree::build(&fx.lib, &[base()], &searching(TreeMode::Flat));
    assert_eq!(lines(&flat, Group::Folders), ["2026", "Dunes (1)", "2024", "Beach (2)"]);
    assert_eq!(flat.sections, [SectionId::Folder(dunes), SectionId::Folder(beach)]);
    let tree = Tree::build(&fx.lib, &[base()], &searching(TreeMode::Tree));
    assert_eq!(lines(&tree, Group::Folders), ["photos (0)", "  Dunes (1)", "  Beach (2)"]);
    assert_eq!(tree.sections, flat.sections);
    // Nothing found: no folder at all.
    let none = TreeOptions { filter: Some(Arc::new(SectionFilter::new([]))), ..TreeOptions::default() };
    assert!(Tree::build(&fx.lib, &[base()], &none).groups.is_empty());

    // The search album first, listing every match; albums without one drop out, the others count
    // their matches.
    let t = AlbumTree::build(&fx.lib, &searching(TreeMode::Flat));
    assert_eq!(album_lines(&t), ["Search results for \"beach\" (3)", "Starred Photos (1)", "alpha (2)"]);
    assert_eq!(t.rows[0].cover, Some(b[1]), "the first match");
    assert_eq!(
        t.sections_with(&flat).iter().map(Section::to_string).collect::<Vec<_>>(),
        ["album:search".to_string(), "album:starred".into(), format!("album:{}", albums[1]), format!("folder:{dunes}"), format!("folder:{beach}"),]
    );
    // A search with no match still shows its album, empty.
    let t = AlbumTree::build(&fx.lib, &TreeOptions { search: Some("zzz".into()), ..none });
    assert_eq!(album_lines(&t), ["Search results for \"zzz\" (0)"]);
    assert!(t.sections.is_empty());
}

#[test]
fn section_photos_come_in_each_sections_order() {
    let (mut fx, albums) = with_albums();
    let beach = folder_named(&fx, "Beach");
    let b = ids_in(&fx, beach);
    let e = ids_in(&fx, folder_named(&fx, "Easter"));
    fx.lib.apply(Op::SetFolderSorts { sorts: vec![(beach, PhotoSort::new(SortBy::Date, true))] }).unwrap();
    let now = Timestamp::from_millis(600_000);
    let options = TreeOptions { now: Some(now), ..TreeOptions::default() };
    let sections = [
        Section::Folder(beach),
        Section::Album(AlbumRef::User(albums[1].clone())),
        Section::Album(AlbumRef::Starred),
        Section::Album(AlbumRef::Recent),
        Section::Album(AlbumRef::User(albums[0].clone())),
        Section::Album(AlbumRef::Search),
    ];
    let got = section_photos(&fx.lib, &sections, &options);
    assert_eq!(got.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(), sections);
    assert_eq!(got[0].1, [b[2], b[1], b[0]], "the folder's own sort: newest first");
    assert_eq!(got[1].1, [e[0], b[1], b[0]], "the album's order");
    assert_eq!(got[2].1, [b[0], b[2], e[1]], "Starred Photos by date");
    // Newest file time first: Beach's files are the newest.
    assert_eq!(got[3].1[..3], [b[0], b[1], b[2]]);
    assert!(got[4].1.is_empty() && got[5].1.is_empty(), "an empty album; no search");

    // A hidden photo leaves every section unless Hidden Pictures is on; the filter narrows them.
    fx.lib.apply(Op::SetHidden { hidden: vec![(b[1], true)] }).unwrap();
    let got = section_photos(&fx.lib, &sections[..2], &options);
    assert_eq!((got[0].1.clone(), got[1].1.clone()), (vec![b[2], b[0]], vec![e[0], b[0]]));
    let shown = section_photos(&fx.lib, &sections[..2], &TreeOptions { hidden_pictures: true, ..options.clone() });
    assert_eq!(shown[0].1, [b[2], b[1], b[0]]);
    let filtered = TreeOptions { filter: Some(Arc::new(SectionFilter::new([b[0], e[0]]))), ..options };
    let got = section_photos(&fx.lib, &[sections[0].clone(), sections[1].clone(), sections[5].clone()], &filtered);
    assert_eq!((got[0].1.clone(), got[1].1.clone(), got[2].1.clone()), (vec![b[0]], vec![e[0], b[0]], vec![b[0], e[0]]));
}

#[test]
fn section_names_round_trip() {
    let names = ["folder:12", "album:starred", "album:recent", "album:search", "album:a91c0e"];
    for name in names {
        let section: Section = name.parse().unwrap();
        assert_eq!(section.to_string(), name);
    }
    assert_eq!(Section::from(SectionId::Folder(FolderId::new(3))), Section::Folder(FolderId::new(3)));
    for bad in ["", "folder:", "folder:x", "folder:-1", "album:", "album:A91C0E", "album:../x", "people:p1234", "starred"] {
        assert!(bad.parse::<Section>().is_err(), "{bad:?}");
    }
}
