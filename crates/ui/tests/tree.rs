//! The library tree pane (M4.15, `ui-layout.md` §4), driven headlessly:
//!
//! - rows, ids and order in flat and tree mode for every sort and its reverse, year dividers only
//!   across years, against an order worked out here from the photos themselves;
//! - a row's click leaves its section's header at the top of the light table after 0.2 s and
//!   selects the folder as a unit;
//! - collapsing Folders removes its sections and docks its header at the bottom of the pane;
//! - the focused pane's arrows: ↑/↓ move the selection by sections, ←/→ fold and unfold in tree
//!   mode, and none of them moves the light table's photo selection;
//! - directories that are no library folder fold by their own ids;
//! - a 5 000-folder synthetic library lays out as many rows per frame as a 50-folder one, within the
//!   frame budget.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_engine::Session;
use dalica_engine::synthetic::{self, Spec};
use dalica_library::Library;
use dalica_ui::panels::folders::{self, model::Kind};
use dalica_ui::panels::light_table::FrameInfo;
use dalica_ui::state::{Group, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::Role;
use serde_json::json;

const FRAMES: usize = 60 * 60;

fn clock() -> Arc<dyn dalica_core::Clock> {
    Arc::new(dalica_core::SystemClock)
}

/// An index-only synthetic library of `spec` in a `w × h` window.
fn synthetic_driver(dir: &Path, spec: &Spec, (w, h): (u32, u32)) -> Driver {
    let s = synthetic::open(dir, spec, clock()).unwrap();
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// A watched root `photos` holding `Trip` (3 photos), `Trip/Day 2` (3, taken later) and `Home`
/// (2, taken last): a folder with photos that has a child, under a directory that is no folder.
fn nested_driver(dir: &Path) -> Driver {
    let files = dalica_scenes::demo_library(1, 8, 3).unwrap();
    let root = dir.join("photos");
    for (i, f) in files.iter().enumerate() {
        let folder = match i {
            0..3 => root.join("Trip"),
            3..6 => root.join("Trip").join("Day 2"),
            _ => root.join("Home"),
        };
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(f.path.file_name().unwrap()), &f.bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), clock()).unwrap();
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn library(d: &Driver) -> &Library {
    d.app().session().unwrap().library()
}

fn folder_by_name(d: &Driver, name: &str) -> FolderId {
    library(d).folders().find(|f| f.path.file_name().is_some_and(|n| n == name)).unwrap().id
}

fn row_id(f: FolderId) -> String {
    format!("tree:folder:{}", f.get())
}

fn sections(d: &Driver) -> Vec<Section> {
    d.app().panels().unwrap().sections.list.iter().map(|s| s.id).collect()
}

fn info(d: &Driver) -> FrameInfo {
    d.app().panels().unwrap().light_table.frame_info().clone()
}

/// The pane as tokens: `year:2024` for a divider, the row id for a row (headers left out).
fn items(d: &Driver) -> Vec<String> {
    folders::current_model(d.ctx())
        .items
        .iter()
        .filter_map(|it| match &it.kind {
            Kind::Year(y) => Some(format!("year:{y}")),
            Kind::Row(r) => Some(r.id.clone()),
            Kind::Header { .. } => None,
        })
        .collect()
}

/// The tree rows registered last frame, top to bottom.
fn row_widgets(d: &Driver) -> Vec<dalica_ui_kit::WidgetInfo> {
    let mut rows: Vec<_> = d.widgets().into_iter().filter(|w| w.role == Role::TreeRow).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows
}

fn widget(d: &Driver, id: &str) -> dalica_ui_kit::WidgetInfo {
    d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}`"))
}

fn region(d: &Driver, name: &str) -> [f32; 4] {
    widget(d, &format!("region:{name}")).rect
}

fn save(d: &Driver, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// What a folder (or a subtree) sorts by, worked out from its photos.
#[derive(Clone, Debug, Default)]
struct Keys {
    date: Option<DateTime>,
    mtime: Option<Timestamp>,
    bytes: u64,
    name: String,
    path: PathBuf,
}

impl Keys {
    fn of_folder(lib: &Library, id: FolderId) -> Keys {
        let path = lib.folder(id).unwrap().path.clone();
        let mut k = Keys { name: path.file_name().unwrap().to_string_lossy().into_owned(), path, ..Keys::default() };
        for p in lib.photos_in(id) {
            k.date = k.date.into_iter().chain(p.sort_date()).min();
            k.mtime = k.mtime.max(Some(p.mtime));
            k.bytes += p.size;
        }
        k
    }

    fn merge(mut self, o: &Keys) -> Keys {
        self.date = self.date.into_iter().chain(o.date).min();
        self.mtime = self.mtime.max(o.mtime);
        self.bytes += o.bytes;
        self
    }
}

fn compare(a: &Keys, b: &Keys, sort: &str, reverse: bool) -> Ordering {
    let primary = match sort {
        "date" => b.date.cmp(&a.date),
        "recent" => b.mtime.cmp(&a.mtime),
        "size" => b.bytes.cmp(&a.bytes),
        _ => Ordering::Equal,
    };
    let o =
        primary.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)).then_with(|| a.path.cmp(&b.path));
    if reverse { o.reverse() } else { o }
}

/// Flat mode: every folder, sorted; year dividers before each year when sorted by date and the
/// folders span several years.
fn expected_flat(lib: &Library, sort: &str, reverse: bool) -> Vec<String> {
    let mut keys: Vec<(FolderId, Keys)> = lib.folders().map(|f| (f.id, Keys::of_folder(lib, f.id))).collect();
    keys.sort_by(|a, b| compare(&a.1, &b.1, sort, reverse));
    let years: BTreeSet<u16> = keys.iter().filter_map(|(_, k)| k.date).map(|d| d.date().year()).collect();
    let dividers = sort == "date" && years.len() > 1;
    let mut out = Vec::new();
    let mut last = None;
    for (id, k) in keys {
        let y = k.date.map(|d| d.date().year());
        if let Some(year) = y.filter(|_| dividers && y != last) {
            out.push(format!("year:{year}"));
            last = y;
        }
        out.push(row_id(id));
    }
    out
}

/// Tree mode on a synthetic library: the root, its year directories (no library folders), their
/// folders; siblings sorted by the keys of their whole subtree.
fn expected_tree(lib: &Library, root: &Path, sort: &str, reverse: bool) -> Vec<String> {
    type Kids = Vec<(FolderId, Keys)>;
    let mut years: BTreeMap<PathBuf, Kids> = BTreeMap::new();
    for f in lib.folders() {
        years.entry(f.path.parent().unwrap().to_path_buf()).or_default().push((f.id, Keys::of_folder(lib, f.id)));
    }
    let mut dirs: Vec<(PathBuf, Keys, Kids)> = years
        .into_iter()
        .map(|(path, mut kids)| {
            kids.sort_by(|a, b| compare(&a.1, &b.1, sort, reverse));
            let base = Keys { name: path.file_name().unwrap().to_string_lossy().into_owned(), path: path.clone(), ..Keys::default() };
            let keys = kids.iter().fold(base, |k, (_, c)| k.merge(c));
            (path, keys, kids)
        })
        .collect();
    dirs.sort_by(|a, b| compare(&a.1, &b.1, sort, reverse));
    let mut out = vec![folders::model::dir_id(root)];
    for (path, _, kids) in dirs {
        out.push(folders::model::dir_id(&path));
        out.extend(kids.into_iter().map(|(id, _)| row_id(id)));
    }
    out
}

/// The done-when, part 1: row ids and order in both modes and every sort (and its reverse); the
/// rows drawn are the model's, 24 high under a 30-high header (dividers 18), and the light table's
/// sections follow the tree's order.
#[test]
fn rows_and_their_order_in_both_modes_and_every_sort() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = synthetic_driver(dir.path(), &Spec::new(80, 9, synthetic::SEED), (1400, 900));
    let root = synthetic::photos_dir(dir.path());
    let lib = library(&d).clone();
    let years: BTreeSet<u16> = lib.photos().filter_map(|p| p.sort_date()).map(|t| t.date().year()).collect();
    assert!(years.len() > 2, "the fixture spans several years: {years:?}");
    let tree_rect = region(&d, "tree");
    for mode in ["flat", "tree"] {
        d.run("view.folderView", &json!({ "mode": mode })).unwrap();
        for sort in ["date", "recent", "size", "name"] {
            for reverse in [false, true] {
                d.run("view.folderSort", &json!({ "by": sort })).unwrap();
                if d.state().folder_sort_reverse != reverse {
                    d.run("view.folderSort", &json!({ "reverse": true })).unwrap();
                }
                d.settle(FRAMES).unwrap();
                let case = format!("{mode} {sort} reverse={reverse}");
                let want = if mode == "flat" { expected_flat(&lib, sort, reverse) } else { expected_tree(&lib, &root, sort, reverse) };
                let got = items(&d);
                assert_eq!(got, want, "{case}");
                if mode == "tree" || sort != "date" {
                    assert!(got.iter().all(|t| !t.starts_with("year:")), "{case}: dividers only in flat mode by date");
                }

                // The rows drawn: the model's rows, in order, at their heights.
                let rows = row_widgets(&d);
                let ids: Vec<&str> = rows.iter().map(|w| w.id.as_str()).collect();
                let model_rows: Vec<&str> = got.iter().filter(|t| !t.starts_with("year:")).map(String::as_str).collect();
                assert_eq!(ids, model_rows, "{case}: every row fits and is drawn");
                let mut y = tree_rect[1] + 30.0;
                for t in &got {
                    if t.starts_with("year:") {
                        y += 18.0;
                        continue;
                    }
                    let w = rows.iter().find(|w| &w.id == t).unwrap();
                    assert_eq!(w.rect, [tree_rect[0], y, tree_rect[2], 24.0], "{case}: {t}");
                    y += 24.0;
                }
                assert_eq!(widget(&d, "tree:group:folders").rect, [tree_rect[0], tree_rect[1], tree_rect[2], 30.0], "{case}");
                assert_eq!(folders::info(d.ctx()).rows_visited, got.len(), "{case}: rows and dividers laid out");

                if sort == "date" && !reverse {
                    save(&d, &format!("tree-{mode}-1400x900"));
                }

                // The light table shows the folders' sections in the same order.
                let order: Vec<String> = sections(&d).iter().map(|s| format!("tree:{s}")).collect();
                assert_eq!(order, model_rows.iter().filter(|t| t.starts_with("tree:folder:")).map(|t| t.to_string()).collect::<Vec<_>>(), "{case}");

                // The toolbar's Flat | Tree pair and the popup's sort rows read the view state.
                assert_eq!(widget(&d, "button:tree.tree").active, mode == "tree", "{case}");
                assert_eq!(widget(&d, "button:tree.flat").active, mode == "flat", "{case}");
            }
        }
    }
    // At the smallest window the same rows, as many as fit.
    d.resize(900, 600);
    d.run("view.folderSort", &json!({ "by": "date", "reverse": true })).unwrap();
    for mode in ["flat", "tree"] {
        d.run("view.folderView", &json!({ "mode": mode })).unwrap();
        d.settle(FRAMES).unwrap();
        save(&d, &format!("tree-{mode}-900x600"));
        let rows = row_widgets(&d);
        let tree_rect = region(&d, "tree");
        assert!(!rows.is_empty() && rows.iter().all(|w| w.rect[1] + w.rect[3] <= tree_rect[1] + tree_rect[3] + 0.01), "{mode}: clipped to the pane");
    }
    // Directory rows carry their own ids and the label with their own count.
    let root_row = widget(&d, &folders::model::dir_id(&root));
    assert_eq!(root_row.label, "photos (0)");
    assert_eq!(root_row.role, Role::TreeRow);
}

/// The view-options popup's check marks follow the view state.
#[test]
fn the_view_options_popup_checks_the_current_sort() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = synthetic_driver(dir.path(), &Spec::new(20, 3, synthetic::SEED), (1400, 900));
    let checked = |d: &mut Driver| {
        d.click("button:toolbar.viewOptions").unwrap();
        let rows: Vec<(String, bool)> =
            d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:view.folderSort/")).map(|w| (w.id, w.active)).collect();
        d.click("button:toolbar.viewOptions").unwrap();
        d.settle(FRAMES).unwrap();
        rows.into_iter().filter(|(_, on)| *on).map(|(id, _)| id).collect::<Vec<_>>()
    };
    assert_eq!(checked(&mut d), ["menuitem:view.folderSort/date"]);
    d.run("view.folderSort", &json!({ "by": "size", "reverse": true })).unwrap();
    assert_eq!(checked(&mut d), ["menuitem:view.folderSort/size", "menuitem:view.folderSort/reverse"]);
    // A click on a row runs its command, and its check follows.
    d.click("button:toolbar.viewOptions").unwrap();
    d.click("menuitem:view.folderSort/name").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().folder_sort, dalica_ui::state::FolderSort::Name);
    assert_eq!(checked(&mut d), ["menuitem:view.folderSort/name", "menuitem:view.folderSort/reverse"]);
}

/// The done-when, part 2: a row's click leaves its section's header at the top after 0.2 s and
/// selects the folder as a unit; the row is drawn selected.
#[test]
fn a_row_click_scrolls_its_header_to_the_top_in_0_2_s_and_selects_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = synthetic_driver(dir.path(), &Spec::new(400, 12, synthetic::SEED), (1400, 900));
    let order = sections(&d);
    let target = order[6];
    let Section::Folder(folder) = target;
    let area_top = info(&d).area.unwrap().min.y;
    let header_top = |d: &Driver| info(d).headers.iter().find(|(s, _)| *s == target).map(|(_, r)| r.min.y);
    assert_ne!(header_top(&d), Some(area_top));

    // Press and release, one frame each: the scroll starts on the release.
    d.click(&row_id(folder)).unwrap();
    assert!(info(&d).animating, "eases there");
    d.wait(200).unwrap();
    assert_eq!(header_top(&d), Some(area_top), "at the top after 0.2 s");
    let s = d.state();
    let photos = d.app().panels().unwrap().sections.get(target).unwrap().photos.clone();
    assert_eq!((s.folder, s.selection.section, s.selection.whole), (Some(folder), Some(target), true), "the folder as a unit");
    assert_eq!(s.selection.ids, photos);
    let row = widget(&d, &row_id(folder));
    assert!(row.active, "drawn selected");
    assert!(folders::has_focus(d.ctx()), "a click in the pane gives it the focus");
    save(&d, "tree-row-clicked-1400x900");

    // Scrolled into it, the same click goes back to its top.
    d.scroll(Some(&format!("thumb:{}", photos[0].get())), egui::vec2(0.0, -120.0)).unwrap();
    assert_ne!(header_top(&d), Some(area_top));
    d.click(&row_id(folder)).unwrap();
    d.wait(200).unwrap();
    assert_eq!(header_top(&d), Some(area_top));
}

/// The done-when, part 3: collapsing Folders removes its sections; its header docks at the bottom
/// of the pane; unfolding brings everything back.
#[test]
fn collapsing_folders_removes_its_sections_and_docks_its_header() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = synthetic_driver(dir.path(), &Spec::new(60, 5, synthetic::SEED), (1400, 900));
    let before = sections(&d);
    assert_eq!(before.len(), 5);
    let tree = region(&d, "tree");
    d.click("tree:group:folders").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().collapsed_groups, [Group::Folders]);
    assert_eq!(sections(&d), [], "its sections leave the light table");
    assert!(row_widgets(&d).is_empty(), "and its rows the pane");
    let header = widget(&d, "tree:group:folders");
    assert!(!header.active);
    assert_eq!(header.rect, [tree[0], tree[1] + tree[3] - 30.0, tree[2], 30.0], "docked at the bottom");
    assert_eq!(header.label, "Folders (5)", "the count stays");
    assert_eq!(folders::info(d.ctx()).rows_visited, 0);
    save(&d, "tree-folders-collapsed-1400x900");
    d.click("tree:group:folders").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(sections(&d), before);
    assert_eq!(widget(&d, "tree:group:folders").rect, [tree[0], tree[1], tree[2], 30.0]);
}

/// The arrows of the focused pane (and the lead's condition for touching the shortcut dispatch):
/// they never move the light table's photo selection; ↑/↓ select the previous or next folder as a
/// unit and scroll to it; a press elsewhere hands the arrows back to the light table.
#[test]
fn an_arrow_in_the_focused_pane_does_not_move_the_light_table_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = nested_driver(dir.path());
    let (trip, day2, home) = (folder_by_name(&d, "Trip"), folder_by_name(&d, "Day 2"), folder_by_name(&d, "Home"));
    assert_eq!(sections(&d), [Section::Folder(home), Section::Folder(day2), Section::Folder(trip)], "newest first");

    // Unfocused, Right is the light table's: it moves the photo selection.
    let first = d.app().panels().unwrap().sections.get(Section::Folder(home)).unwrap().photos[0];
    d.click(&format!("thumb:{}", first.get())).unwrap();
    assert!(!folders::has_focus(d.ctx()));
    d.key("Right").unwrap();
    assert_ne!(d.state().selection.ids, [first], "the light table took Right");

    // Focused, the arrows are the pane's.
    d.click(&row_id(home)).unwrap();
    assert!(folders::has_focus(d.ctx()));
    let unit = d.state().selection.clone();
    assert!(unit.whole);
    for key in ["Right", "Left"] {
        d.key(key).unwrap();
        assert_eq!(d.state().selection, unit, "{key}: flat mode, nothing to fold, and the photos stay as they are");
    }
    d.key("Down").unwrap();
    let s = d.state();
    assert_eq!((s.folder, s.selection.section, s.selection.whole), (Some(day2), Some(Section::Folder(day2)), true), "the next folder as a unit");
    assert_eq!(s.scroll.section, Some(Section::Folder(day2)), "the light table scrolls to it");
    d.key("Down").unwrap();
    assert_eq!(d.state().folder, Some(trip));
    d.key("Down").unwrap();
    assert_eq!(d.state().folder, Some(trip), "the last row stays");
    d.key("Up").unwrap();
    assert_eq!(d.state().folder, Some(day2));
    // Shift and Ctrl arrows are still the light table's.
    assert!(!folders::is_tree_key(egui::Key::ArrowDown, egui::Modifiers::SHIFT));

    // A press on the light table takes the focus away: Right moves photos again.
    let p = d.app().panels().unwrap().sections.get(Section::Folder(day2)).unwrap().photos.clone();
    d.click(&format!("thumb:{}", p[0].get())).unwrap();
    assert!(!folders::has_focus(d.ctx()));
    d.key("Right").unwrap();
    assert_eq!(d.state().selection.ids, [p[1]]);
    assert!(!d.state().selection.whole);
}

/// Tree mode: ← folds the selected row (its sections stay on the light table) or goes up to its
/// parent, → unfolds it; a directory that is no library folder folds by its own disclosure id.
#[test]
fn tree_mode_folds_with_the_arrows_and_the_disclosure_triangles() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = nested_driver(dir.path());
    let root = dir.path().join("photos");
    let (trip, day2, home) = (folder_by_name(&d, "Trip"), folder_by_name(&d, "Day 2"), folder_by_name(&d, "Home"));
    d.click("button:tree.tree").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().tree_mode, dalica_ui::state::TreeMode::Tree);
    let root_id = folders::model::dir_id(&root);
    assert_eq!(items(&d), [root_id.clone(), row_id(home), row_id(trip), row_id(day2)]);
    let x0 = region(&d, "tree")[0];
    let indent = |d: &Driver, id: &str| {
        let m = folders::current_model(d.ctx());
        m.items.iter().find_map(|it| match &it.kind {
            Kind::Row(r) if r.id == id => Some(r.depth),
            _ => None,
        })
    };
    assert_eq!([indent(&d, &root_id), indent(&d, &row_id(trip)), indent(&d, &row_id(day2))], [Some(0), Some(1), Some(2)]);
    let triangle = widget(&d, &format!("tree:disclosure:{}", trip.get()));
    assert_eq!(triangle.rect[0], x0 + 16.0, "the triangle moves in 16 per level");
    assert!(triangle.active, "unfolded");
    save(&d, "tree-mode-nested-1400x900");

    // ← on Trip folds it: Day 2's row goes, its section stays.
    d.click(&row_id(trip)).unwrap();
    d.key("Left").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().collapsed_folders, [trip]);
    assert_eq!(items(&d), [root_id.clone(), row_id(home), row_id(trip)]);
    assert_eq!(sections(&d).len(), 3, "folded rows keep their sections");
    assert_eq!(d.state().folder, Some(trip), "the selection stays");
    d.key("Right").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().collapsed_folders, []);
    assert_eq!(items(&d).len(), 4);

    // ← on Day 2 (no children) goes up to Trip.
    d.key("Down").unwrap();
    assert_eq!(d.state().folder, Some(day2));
    d.key("Left").unwrap();
    assert_eq!(d.state().folder, Some(trip));

    // The disclosure triangles: Trip's by its folder id, the root's by its directory id.
    d.click(&format!("tree:disclosure:{}", trip.get())).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().collapsed_folders, [trip]);
    assert_eq!(d.state().folder, Some(trip), "a triangle's click folds, it does not select");
    d.click(&format!("tree:disclosure:{}", trip.get())).unwrap();
    d.settle(FRAMES).unwrap();
    let root_hash = root_id.trim_start_matches("tree:dir:");
    let root_triangle = format!("tree:disclosure:dir:{root_hash}");
    d.click(&root_triangle).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(items(&d), std::slice::from_ref(&root_id), "everything under the root is folded away");
    assert!(!widget(&d, &root_triangle).active);
    assert_eq!(sections(&d).len(), 3, "the light table still shows the whole library");
    // A click on the row of a directory without photos unfolds it too.
    d.click(&root_id).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(items(&d).len(), 4);
    assert_eq!(d.state().folder, Some(trip), "a directory without photos selects nothing");
}

/// The done-when, part 4: a 5 000-folder synthetic library lays out as many rows a frame as a
/// 50-folder one wherever the pane is scrolled, and the pane's frames stay within the 8 ms budget.
#[test]
fn a_5k_folder_library_stays_within_the_frame_budget_with_rows_visited_flat() {
    let run = |spec: &Spec| {
        let dir = tempfile::tempdir().unwrap();
        let mut d = synthetic_driver(dir.path(), spec, (1400, 900));
        let build = folders::info(d.ctx());
        let first = row_widgets(&d)[0].id.clone();
        // Over the first row, then wherever the pointer stays.
        let mut at = Some(first.as_str());
        let mut visited = Vec::new();
        let mut times = Vec::new();
        for step in 0..24 {
            // Down the list a page at a time, then back up.
            let dy = if step < 12 { -260.0 } else { 260.0 };
            d.scroll(at.take(), egui::vec2(0.0, dy)).unwrap();
            let i = folders::info(d.ctx());
            assert!(!i.rebuilt, "scrolling never rebuilds the model");
            visited.push(i.rows_visited);
            times.push(i.time);
            assert_eq!(d.perf().report().tree_rows_visited, i.rows_visited as u64, "the perf counter agrees");
        }
        let offset = folders::info(d.ctx()).offset;
        assert_eq!(offset, 0.0, "back at the top");
        times.sort();
        let p95 = times[times.len() * 95 / 100];
        eprintln!(
            "{} folders: model built in {:?} (generation {}), content {} pt; rows visited {visited:?}; pane p95 {p95:?}, max {:?}",
            spec.folders,
            build.build,
            build.generation,
            build.height,
            times.last().unwrap()
        );
        (visited, p95, build.view_h)
    };
    let (small, _, view_h) = run(&Spec::new(1_000, 50, synthetic::SEED));
    let (big, p95, _) = run(&Spec::FOLDERS_5K);
    let most = (view_h / 18.0).ceil() as usize + 1;
    // Dividers are shorter than rows, and the small library has one every few rows: a pane of it
    // holds a few more items than a pane of the big one, never fewer.
    assert!(small.iter().chain(&big).all(|n| (1..=most).contains(n)), "one pane of rows at most ({most}): {small:?} {big:?}");
    assert!(big.iter().max() <= small.iter().max(), "flat: {small:?} against {big:?}");
    assert!(p95 <= Duration::from_millis(8), "the pane's p95 frame time is {p95:?}");
}

/// A folder of photos whose header the click asked for: the photos of the selection are that
/// section's, never another's (a regression guard for the shared selection model).
#[test]
fn a_whole_folder_selection_holds_only_that_folders_photos() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = nested_driver(dir.path());
    let trip = folder_by_name(&d, "Trip");
    d.click(&row_id(trip)).unwrap();
    let lib = library(&d);
    let ids: Vec<PhotoId> = d.state().selection.ids.clone();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| lib.photo(*id).unwrap().folder == trip));
}
