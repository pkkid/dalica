//! `dalica-library` tree model (`ui-layout.md` §4, `organizing.md` §10) on hand-built libraries:
//! rows and order in flat and tree mode for every sort and its reverse, year dividers, counts,
//! groups, collapsed groups and nodes, sections, and the build time of a 5 k-folder library.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_core::{FolderId, PhotoId};
use dalica_library::{Folder, FolderRow, FolderSort, Group, Library, Op, Photo, Row, SectionId, Tree, TreeMode, TreeOptions};
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
    let options = TreeOptions {
        projects_dir: Some(projects),
        hidden_folders: BTreeSet::from([secret]),
        hidden_pictures: true,
        small_pictures: true,
        ..TreeOptions::default()
    };
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
    assert_eq!(t.group(Group::Folders).map(|g| g.count), Some(5), "every tree row counts");
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
    assert_eq!(t.group(Group::Folders).map(|g| g.count), Some(4), "folded rows still count");
    assert_eq!(t.sections, [SectionId::Folder(a), SectionId::Folder(b), SectionId::Folder(c)]);
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

#[test]
fn five_thousand_folders_build_quickly() {
    const FOLDERS: usize = 5_000;
    const PER: usize = 20;
    let mut fx = Fx::new();
    let mut photos = Vec::with_capacity(FOLDERS * PER);
    let mut folders = Vec::with_capacity(FOLDERS);
    for i in 0..FOLDERS {
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
            println!("5k folders, {} photos, {mode:?} by {sort:?}: {took:?}", FOLDERS * PER);
            assert_eq!(t.sections.len(), FOLDERS);
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
}

const PATHS: [&str; 8] = ["a", "a/b", "a/b/c", "b", "d/e", "d/f", "Dalica/Collages", "x/y/z"];

fn spec() -> impl Strategy<Value = Spec> {
    (
        proptest::collection::vec((0u8..8, proptest::collection::vec((0u16..2000, 1u16..5000, any::<bool>()), 0..5)), 0..8),
        proptest::collection::vec(0u8..8, 0..3),
    )
        .prop_map(|(folders, hidden)| Spec { folders, hidden })
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
    let hidden = spec.hidden.iter().filter_map(|h| ids.iter().find(|(p, _)| p == h).map(|(_, id)| *id)).collect();
    (fx, hidden)
}

fn sorts() -> impl Strategy<Value = FolderSort> {
    prop_oneof![Just(FolderSort::Date), Just(FolderSort::RecentChanges), Just(FolderSort::Size), Just(FolderSort::Name)]
}

fn shown(photo: &Photo, small_pictures: bool) -> bool {
    small_pictures || !(photo.width < 64 && photo.height < 64)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_shown_photo_is_counted_once_and_sections_follow_rows(
        spec in spec(), sort in sorts(), reverse: bool, tree_mode: bool, small: bool, hidden_pictures: bool,
    ) {
        let (fx, hidden) = library(&spec);
        let mode = if tree_mode { TreeMode::Tree } else { TreeMode::Flat };
        let options = TreeOptions {
            mode, sort, reverse, small_pictures: small, hidden_pictures, hidden_folders: hidden.clone(),
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
