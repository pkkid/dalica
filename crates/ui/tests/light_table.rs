//! The light table's skeleton and layout (M4.6, `ui-layout.md` §5.1–§5.3): justified columns at
//! every width and cell size, the sticky header pinned and pushed up, offsets and sections round
//! trip, the cells a frame visits do not grow from 10 k to 100 k photos, `section.scrollTo` eases a
//! header to the top in 0.2 s of virtual time (and fails gracefully), Ctrl+wheel changes the
//! thumbnail size, small pictures are left out unless asked for, and the "No pictures yet" state
//! adds a folder.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::common::{FRAMES, new_driver};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Session};
use dalica_library::{Folder, Library, Op, Photo, TreeOptions};
use dalica_ui::dialogs::folder_manager::{State, mode_id, row_id};
use dalica_ui::headless::KeyState;
use dalica_ui::panels::light_table::layout::{Grid, Layout, STICKY, Spacing};
use dalica_ui::panels::light_table::{self, Sections};
use dalica_ui::state::{ScrollAnchor, Section, THUMB_MAX, THUMB_MIN};
use dalica_ui::{App, Driver};
use serde_json::json;

fn folder(n: u64) -> Section {
    Section::Folder(FolderId::new(n))
}

/// A session on a library under `dir` with `folders` folders sharing `photos` scenes photos; the
/// folder ids in the light table's order.
fn session(dir: &Path, folders: usize, photos: usize) -> Session {
    let files = crate::common::demo_library(folders, photos, 5).unwrap();
    let mut roots = BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(dir: &Path, folders: usize, photos: usize, size: (u32, u32)) -> Driver {
    let mut d = new_driver(App::new(Some(session(dir, folders, photos))), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn sections(d: &Driver) -> Vec<Section> {
    d.app().panels().unwrap().sections.list.iter().map(|s| s.id).collect()
}

fn info(d: &Driver) -> light_table::FrameInfo {
    d.app().panels().unwrap().light_table.frame_info().clone()
}

/// Writes the last frame to `target/test-output/ui/<name>.png`, to look at.
fn save(d: &Driver, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// The on-screen top of `section`'s header block in the last frame, if drawn.
fn header_top(d: &Driver, section: Section) -> Option<f32> {
    info(d).headers.iter().find(|(s, _)| *s == section).map(|(_, r)| r.min.y)
}

#[test]
fn columns_justify_at_every_width_and_size() {
    let s = Spacing::default();
    let mut cases = 0;
    for width in (480..=2000).step_by(7).map(|w| w as f32).chain([480.0, 2000.0, 1136.0, 636.5]) {
        for cell in (64..=320).step_by(4).map(|c| c as f32).chain([64.0, 320.0, 144.0, 96.5]) {
            let g = Grid::new(width, cell, &s);
            assert_eq!((g.cell, g.margin), (cell, 24.0));
            assert_eq!(g.x_of(0), 24.0, "{width} {cell}: the left margin");
            let used = |cols: usize| cols as f32 * cell + (cols as f32 - 1.0) * 12.0;
            assert!(used(g.cols + 1) > width - 48.0, "{width} {cell}: one more column would not fit");
            if g.cols > 1 {
                assert!(g.gap >= 12.0 - 1e-3, "{width} {cell}: gap {}", g.gap);
                let right = g.x_of(g.cols - 1) + cell;
                assert!((right - (width - 24.0)).abs() < 0.01, "{width} {cell}: the last column ends at the right margin: {right}");
            } else {
                assert_eq!(g.gap, 0.0);
            }
            cases += 1;
        }
    }
    assert!(cases > 1000);
    // Hostile numbers lay out one safe column.
    for (w, c) in [(f32::NAN, 144.0), (-5.0, 144.0), (800.0, f32::NAN), (800.0, 1e9), (f32::INFINITY, 64.0)] {
        let g = Grid::new(w, c, &s);
        assert!(g.cols >= 1 && g.cell.is_finite() && g.gap.is_finite() && (THUMB_MIN..=THUMB_MAX).contains(&g.cell), "{w} {c}: {g:?}");
    }
}

/// Three sections of 20, 3 and 40 photos at 1100 × 144 (6 columns).
fn three() -> Layout {
    Layout::new([(folder(1), 20), (folder(2), 3), (folder(3), 40)], 1, 1100.0, 144.0, false, Spacing::default())
}

#[test]
fn a_sticky_header_is_pinned_then_pushed_up_by_the_next() {
    let l = three();
    let (t1, t2) = (l.top(1).unwrap(), l.top(2).unwrap());
    // At the very top and with a header exactly at the top: nothing pinned.
    assert_eq!(l.sticky(0.0), None);
    assert_eq!(l.sticky(t1), None);
    // Scrolled into the first section: its header pinned at the top.
    let s = l.sticky(10.0).unwrap();
    assert_eq!((s.section, s.y), (folder(1), 0.0));
    let s = l.sticky(t1 - 40.0).unwrap();
    assert_eq!((s.section, s.y), (folder(1), 0.0), "the next header is 40 below: still pinned");
    // The next header pushes it up point for point.
    for d in [31.0, 20.0, 5.0, 0.5] {
        let s = l.sticky(t1 - d).unwrap();
        assert_eq!(s.section, folder(1));
        assert!((s.y - (d - f64::from(STICKY))).abs() < 1e-9, "{d}: {}", s.y);
    }
    // Past the next header's top, the next section is pinned.
    assert_eq!(l.sticky(t1 + 1.0).unwrap().section, folder(2));
    assert_eq!(l.sticky(t2 + 500.0).unwrap(), dalica_ui::panels::light_table::layout::Sticky { index: 2, section: folder(3), y: 0.0 });
    assert_eq!(l.sticky(f64::NAN), None);
}

#[test]
fn offsets_and_sections_round_trip() {
    let l = three();
    for i in 0..l.len() {
        let top = l.top(i).unwrap();
        let section = l.section(i).unwrap();
        assert_eq!(l.index_at(top), Some(i));
        for below in [0.0f32, 1.0, 95.5, 300.0] {
            let a = ScrollAnchor { section: Some(section), offset: below };
            if top + f64::from(below) >= l.top(i + 1).unwrap_or(l.height()) {
                continue;
            }
            let y = l.offset_of(&a).unwrap();
            assert_eq!(y, top + f64::from(below));
            let back = l.anchor_at(y);
            if y > 0.0 {
                assert_eq!(back, a, "section {i} + {below}");
            } else {
                assert_eq!(back, ScrollAnchor::default(), "offset 0 is the top of the library");
            }
        }
    }
    let mut y = 0.0;
    while y < l.height() {
        let back = l.offset_of(&l.anchor_at(y)).unwrap();
        assert!((back - y).abs() < 1e-3, "{y} → {back}");
        y += 37.25;
    }
    // A section that is not laid out has no offset; the top of the library is offset 0.
    assert_eq!(l.offset_of(&ScrollAnchor { section: Some(folder(99)), offset: 0.0 }), None);
    assert_eq!(l.offset_of(&ScrollAnchor::default()), Some(0.0));
    assert_eq!(l.index_at(-5.0), Some(0));
    assert_eq!(l.index_at(1e12), Some(2));
}

/// A library of `folders` folders of `per` photos each, built through ops (no files).
fn library(folders: usize, per: usize) -> Library {
    let mut lib = Library::new();
    for f in 0..folders {
        let id = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id, path: PathBuf::from(format!("/photos/{f:05}")) } }).unwrap();
        let photos: Vec<Photo> = (0..per)
            .map(|i| {
                let pid = lib.next_photo_id();
                let date = format!("20{:02}-{:02}-{:02}T12:00:00", 10 + f % 15, 1 + f % 12, 1 + i % 28);
                serde_json::from_value(json!({
                    "id": pid, "folder": id, "name": format!("IMG_{i:05}.jpg"), "size": 1000, "mtime": 1_700_000_000_000i64,
                    "width": if i % 3 == 0 { 320 } else { 480 }, "height": if i % 3 == 0 { 480 } else { 320 }, "date": date, "star": false
                }))
                .unwrap()
            })
            .collect();
        lib.apply(Op::AddPhotos { photos }).unwrap();
    }
    lib
}

#[test]
fn cells_visited_per_frame_are_equal_at_10k_and_100k_photos() {
    let visits = |folders: usize| {
        let lib = library(folders, 100);
        let started = Instant::now();
        let mut sections = Sections::default();
        assert!(sections.refresh(&lib, &[], &TreeOptions::default()));
        assert!(!sections.refresh(&lib, &[], &TreeOptions::default()), "cached for the same revision and options");
        let built = started.elapsed();
        let started = Instant::now();
        let l = Layout::new(sections.list.iter().map(|s| (s.id, s.photos.len())), sections.generation(), 1136.0, 144.0, false, Spacing::default());
        let laid = started.elapsed();
        assert_eq!(sections.list.iter().map(|s| s.photos.len()).sum::<usize>(), folders * 100);
        // The same screens: the top, a few pages down, and the middle of the 50th section.
        let mut counts = Vec::new();
        let started = Instant::now();
        for offset in [0.0, 720.0, 5000.0, 12_345.5, l.top(50).unwrap() + 400.0, l.top(60).unwrap() - 10.0] {
            let n: usize = l.visible(offset, 720.0).iter().map(|v| v.cells.len()).sum();
            counts.push(n);
        }
        let visit = started.elapsed();
        eprintln!("{} photos: sections {built:?}, layout {laid:?}, 6 visits {visit:?}, cells {counts:?}", folders * 100);
        counts
    };
    let small = visits(100);
    let big = visits(1000);
    assert_eq!(small, big, "a frame lays out the same cells whatever the library's size");
    assert!(small.iter().all(|&n| n > 0 && n <= 7 * 6), "one screen of cells (7 columns, at most 6 rows of 166 in 720): {small:?}");
}

#[test]
fn small_pictures_are_left_out_unless_asked_for() {
    let mut lib = library(2, 4);
    let tiny: Vec<Photo> = (0..3)
        .map(|i| {
            serde_json::from_value(json!({
                "id": 1000 + i, "folder": 1, "name": format!("icon{i}.png"), "size": 10, "mtime": 0, "width": 32, "height": 32, "date": null, "star": false
            }))
            .unwrap()
        })
        .collect();
    lib.apply(Op::AddPhotos { photos: tiny }).unwrap();
    let mut sections = Sections::default();
    sections.refresh(&lib, &[], &TreeOptions::default());
    let counts = |s: &Sections| s.list.iter().map(|s| (s.folder, s.photos.len())).collect::<Vec<_>>();
    assert!(counts(&sections).iter().all(|&(_, n)| n == 4), "{:?}", counts(&sections));
    sections.refresh(&lib, &[], &TreeOptions { small_pictures: true, ..TreeOptions::default() });
    let with = counts(&sections);
    assert!(with.contains(&(Some(FolderId::new(1)), 7)), "{with:?}");
    assert!(sections.get(folder(1)).unwrap().photos.contains(&PhotoId::new(1000)));
}

#[test]
fn section_scroll_to_brings_the_header_to_the_top_in_0_2_s() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 3, 90, (1400, 900));
    let order = sections(&d);
    assert_eq!(order.len(), 3);
    let area_top = info(&d).area.unwrap().min.y;
    let target = order[1];
    assert_ne!(header_top(&d, target), Some(area_top));

    // The command answers at once; the view eases there on the frame clock.
    assert_eq!(d.run("section.scrollTo", &json!({ "section": target })).unwrap(), json!({ "section": target.to_string() }));
    assert_eq!(d.state().scroll, ScrollAnchor { section: Some(target), offset: 0.0 }, "the anchor is what was asked");
    let goal = d.app().panels().unwrap().light_table.layout().unwrap().top(1).unwrap();
    d.wait(100).unwrap();
    // Ease-out: under way at half the time, and more than half the way there.
    let mid = info(&d).offset;
    assert!(info(&d).animating && mid > goal / 2.0 && mid < goal, "at 0.1 s: {mid} of {goal}");
    d.wait(100).unwrap();
    assert_eq!(header_top(&d, target), Some(area_top), "at the top after 0.2 s");
    assert!(!info(&d).animating);
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, target), Some(area_top));

    // Scrolled a little into it, the same command goes back to its top.
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    d.scroll(Some(&cell.id), egui::vec2(0.0, -150.0)).unwrap();
    assert_eq!(d.state().scroll, ScrollAnchor { section: Some(target), offset: 150.0 }, "the wheel writes the anchor");
    save(&d, "light-table-sticky-1400x900");
    let sticky = info(&d).sticky.expect("its header is pinned while it scrolls under");
    assert_eq!((sticky.0, sticky.1.min.y, sticky.1.height()), (target, area_top, STICKY));
    d.run("section.scrollTo", &json!({ "section": target })).unwrap();
    d.wait(200).unwrap();
    assert_eq!(header_top(&d, target), Some(area_top), "again = the top of that section");
    assert_eq!(info(&d).sticky, None);

    // A tree row's click does the same.
    d.click(&format!("tree:folder:{}", order[2].folder().unwrap().get())).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[2]), Some(area_top));
}

#[test]
fn section_scroll_to_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 6, (900, 600));
    let before = d.state().clone();
    for (params, bad) in [
        (json!({}), true),
        (json!(null), true),
        (json!([]), true),
        (json!("folder:1"), true),
        (json!({ "section": 12 }), true),
        (json!({ "section": "" }), true),
        (json!({ "section": "album:x" }), true),
        (json!({ "section": "folder:-1" }), true),
        (json!({ "section": "folder:1", "offset": 3 }), true),
        (json!({ "section": "folder:99999" }), false),
    ] {
        let e = d.app_mut().run("section.scrollTo", &params).unwrap_err();
        if bad {
            assert!(matches!(e, CmdError::BadParams(_)), "{params}: {e:?}");
        } else {
            assert!(matches!(e, CmdError::NotFound(_)), "{params}: {e:?}");
        }
        d.step();
        assert_eq!(d.state().scroll, before.scroll, "{params}: nothing changed");
    }
    // Without a library: not found, never a panic.
    let mut empty = App::new(None);
    assert!(matches!(empty.run("section.scrollTo", &json!({ "section": "folder:1" })), Err(CmdError::NotFound(_))));
}

#[test]
fn ctrl_wheel_changes_the_thumbnail_size_and_keeps_the_section_in_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 3, 90, (1400, 900));
    let order = sections(&d);
    d.run("section.scrollTo", &json!({ "section": order[1] })).unwrap();
    d.settle(FRAMES).unwrap();
    let before = d.state().thumb_size;
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    // A plain wheel scrolls; with Ctrl held it zooms.
    d.key_state("Ctrl+F13", KeyState::Down).unwrap();
    d.scroll(Some(&cell.id), egui::vec2(0.0, 60.0)).unwrap();
    let bigger = d.state().thumb_size;
    assert!(bigger > before, "Ctrl+wheel up grows the cells: {before} → {bigger}");
    for _ in 0..20 {
        d.scroll(None, egui::vec2(0.0, -120.0)).unwrap();
    }
    d.key_state("Ctrl+F13", KeyState::Up).unwrap();
    assert_eq!(d.state().thumb_size, THUMB_MIN, "and stops at the smallest");
    d.settle(FRAMES).unwrap();
    let cells: Vec<f32> = d.widgets().iter().filter(|w| w.id.starts_with("thumb:") && !w.id.contains('/')).map(|w| w.rect[2]).collect();
    assert!(!cells.is_empty() && cells.iter().all(|&w| w == THUMB_MIN), "{cells:?}");
    assert_eq!(d.state().scroll.section, Some(order[1]), "the section stays at the top");
    assert_eq!(header_top(&d, order[1]), Some(info(&d).area.unwrap().min.y));
}

/// Add a Folder… opens the Folder Manager, as File ▸ Add Folder… does (`organizing.md` §2): the
/// photos come once a folder is set to Scan Always there.
#[test]
fn no_pictures_yet_offers_to_add_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let pictures = home.join("Pictures");
    let files = crate::common::demo_library(1, 4, 3).unwrap();
    for f in &files {
        let path = pictures.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let mut session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    session.set_places(dalica_scan::Places::under(&home), Vec::new());
    let mut d = new_driver(App::new(Some(session)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    let widgets = d.widgets();
    let get = |id: &str| widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}")).clone();
    assert_eq!(get(light_table::EMPTY_ID).label, "No pictures yet");
    assert!(get(light_table::ADD_FOLDER_ID).enabled);
    assert!(get(light_table::IMPORT_ID).enabled, "Import… opens the Import room (M11.2)");
    let area = info(&d).area.unwrap();
    let block = get(light_table::EMPTY_ID).rect;
    assert!(area.contains(egui::pos2(block[0] + block[2] / 2.0, block[1] + block[3] / 2.0)), "centred in the light table");
    d.click(light_table::IMPORT_ID).unwrap();
    assert_eq!(d.state().room, dalica_ui::state::Room::Import, "Import… opens the Import room");
    d.app_mut().run("room.close", &serde_json::json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(light_table::ADD_FOLDER_ID).unwrap();
    assert_eq!(d.state().dialogs.iter().map(|d| d.kind.name()).collect::<Vec<_>>(), ["folderManager"]);
    let row = row_id(&pictures, &pictures);
    for _ in 0..FRAMES {
        if d.widgets().iter().any(|w| w.id == row) {
            break;
        }
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    d.click(&row).unwrap();
    d.click(&mode_id(State::Always)).unwrap();
    d.click("button:dialog.ok").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.widgets().iter().filter(|w| w.id.starts_with("thumb:")).count(), 4);
    assert!(!d.widgets().iter().any(|w| w.id.starts_with("empty:")), "the empty state is gone");
}
