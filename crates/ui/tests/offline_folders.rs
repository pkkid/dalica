//! Offline folders and the tree's scroll bar (M5.19, `ui-layout.md` §4, `data-model.md` §7),
//! driven headlessly.
//!
//! - A watched root "unplugged" by renaming it away: once the session's volume check has seen it
//!   gone, its folders' rows stay (place, count, section) but are dimmed with a `caution`
//!   "offline" marker (the pane's model flags them); renaming it back restores them. Rows of other
//!   roots are untouched.
//! - A tree longer than the pane shows a scroll bar (`scroll:tree`, `scroll:tree/thumb`) that drags
//!   and pages the rows; a short one shows none.
//!
//! The tree region is checked against fingerprints at 1400 × 900 and 900 × 600
//! (`region-tree-offline-<w>.json`, `region-tree-scroll-<w>.json`); renders go to
//! `target/test-output/ui/`, and `DALICA_BLESS=1` stores new fingerprints. The UI thread is marked
//! while frames run: nothing it does touches the disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::headless::{PointerEvent, PointerKind, Target};
use dalica_ui::panels::folders;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// `n` folders of scenes photos under `root` (one photo each when `n` is the photo count).
fn demo(root: &Path, folders: usize, photos: usize, seed: u64) -> Vec<PathBuf> {
    let files = dalica_scenes::demo_library(folders, photos, seed).unwrap();
    let mut dirs = Vec::new();
    for f in &files {
        let path = root.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        let dir = path.parent().unwrap().to_path_buf();
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// A library watching each of `roots`, scanned.
fn session(dir: &Path, roots: &[PathBuf]) -> Session {
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for root in roots {
        s.execute("folders.add", &serde_json::json!({ "path": root })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(session: Session, (w, h): (u32, u32)) -> Driver {
    let mut d = Driver::new(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn widget(d: &Driver, id: &str) -> WidgetInfo {
    d.widgets()
        .into_iter()
        .find(|w| w.id == id)
        .unwrap_or_else(|| panic!("no `{id}` in {:?}", d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

fn has(d: &Driver, id: &str) -> bool {
    d.widgets().iter().any(|w| w.id == id)
}

/// The folder rows on screen, top to bottom: `(id, label)`.
fn rows(d: &Driver) -> Vec<(String, String)> {
    let mut rows: Vec<WidgetInfo> = d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.into_iter().map(|w| (w.id, w.label)).collect()
}

/// The ids of the rows the pane's model marks offline.
fn offline_rows(d: &Driver) -> Vec<String> {
    let model = folders::current_model(d.ctx());
    (0..model.items.len()).filter_map(|i| model.row(i)).filter(|r| r.offline).map(|r| r.id.clone()).collect()
}

/// The row id of the library folder at `path`.
fn row_of(d: &Driver, path: &Path) -> String {
    let folder = d.app().session().unwrap().library().folder_by_path(path).unwrap_or_else(|| panic!("no folder {}", path.display()));
    format!("tree:folder:{folder}")
}

/// Checks the volumes now (on the checker thread; this thread waits for the answer), then lets
/// the frames catch up.
fn check_volumes(d: &mut Driver) {
    assert!(d.app_mut().session_mut().unwrap().refresh_volumes(Duration::from_secs(10)), "the volume check answered");
    d.wait(100).unwrap();
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the tree region (the pane and its splitter) of the last frame against `<name>.json`.
fn check_tree(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    let (tree, splitter) = (widget(d, "region:tree").rect, widget(d, "region:splitter").rect);
    let x0 = tree[0].min(splitter[0]).round() as u32;
    let y0 = tree[1].min(splitter[1]).round() as u32;
    let x1 = ((tree[0] + tree[2]).max(splitter[0] + splitter[2]).round() as u32).min(shot.width);
    let y1 = ((tree[1] + tree[3]).max(splitter[1] + splitter[3]).round() as u32).min(shot.height);
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// Pixels of `rect` in the last frame close to the `caution` colour (`#e3a83c`).
fn caution_pixels(d: &Driver, rect: [f32; 4]) -> usize {
    let shot = d.screenshot().unwrap();
    let mut n = 0;
    for y in rect[1] as u32..(rect[1] + rect[3]) as u32 {
        for x in rect[0] as u32..(rect[0] + rect[2]) as u32 {
            let p = shot.pixel(x, y).unwrap();
            if p[0] > 150 && p[1] > 100 && p[2] < 110 && p[0] > p[2] + 80 {
                n += 1;
            }
        }
    }
    n
}

/// The done-when at both window sizes: a root renamed away dims its rows with the marker; renamed
/// back, they are restored. Rows, counts and the other root's rows do not change.
#[test]
fn a_root_renamed_away_dims_its_rows_and_renaming_it_back_restores_them() {
    for size in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("card");
        let home = dir.path().join("photos");
        let card_folders = demo(&card, 2, 6, 3);
        let home_folders = demo(&home, 1, 3, 7);
        let mut d = driver(session(dir.path(), &[card.clone(), home.clone()]), size);
        let w = size.0;
        let card_rows: Vec<String> = card_folders.iter().map(|p| row_of(&d, p)).collect();
        let home_row = row_of(&d, &home_folders[0]);
        let before = rows(&d);
        assert_eq!(before.len(), 3, "{w}: {before:?}");
        assert_eq!(offline_rows(&d), Vec::<String>::new(), "{w}");
        let count = d.app().session().unwrap().library().photos().count();

        // Unplugged: renamed away. Once checked, its rows are dimmed and marked; the others not.
        let away = dir.path().join("card-away");
        std::fs::rename(&card, &away).unwrap();
        let ui_thread = dalica_fs::mark_ui_thread();
        let refused = dalica_fs::guard::refused();
        check_volumes(&mut d);
        let offline = rows(&d);
        assert_eq!(offline.len(), 3, "{w}: the rows stay: {offline:?}");
        let mut marked = offline_rows(&d);
        marked.sort();
        let mut want_marked = card_rows.clone();
        want_marked.sort();
        assert_eq!(marked, want_marked, "{w}: the card's rows, and only them");
        for (id, _) in &offline {
            let want = card_rows.contains(id);
            assert_eq!(caution_pixels(&d, widget(&d, id).rect) > 10, want, "{w}: {id}'s marker");
        }
        assert_eq!(offline, before, "{w}: places, labels and counts are kept");
        assert_eq!(d.app().session().unwrap().library().photos().count(), count, "{w}: its photos stay");
        assert!(!offline_rows(&d).contains(&home_row));
        check_tree(&d, &format!("region-tree-offline-{w}"));
        assert_eq!(dalica_fs::guard::refused(), refused, "{w}: the UI thread touched the disk");
        drop(ui_thread);

        // Plugged back in: restored.
        std::fs::rename(&away, &card).unwrap();
        check_volumes(&mut d);
        assert_eq!(rows(&d), before, "{w}: restored");
        assert_eq!(offline_rows(&d), Vec::<String>::new(), "{w}: nothing marked");
        assert!(card_rows.iter().all(|id| caution_pixels(&d, widget(&d, id).rect) == 0), "{w}: no marker left");
    }
}

/// The done-when: a tree longer than the pane shows a scroll bar whose thumb drags the rows and
/// whose track pages them; a short tree shows none. The model is not rebuilt by scrolling.
#[test]
fn a_tree_longer_than_the_pane_shows_a_scroll_bar_that_drags_the_rows() {
    for size in SIZES {
        let w = size.0;
        // A short tree: no bar.
        let dir = tempfile::tempdir().unwrap();
        demo(&dir.path().join("photos"), 2, 4, 5);
        let d = driver(session(dir.path(), &[dir.path().join("photos")]), size);
        assert!(!has(&d, "scroll:tree") && !has(&d, "scroll:tree/thumb"), "{w}: a tree that fits has no scroll bar");
        assert_eq!(folders::info(d.ctx()).scrollbar, None);

        // Forty folders: taller than the pane.
        let dir = tempfile::tempdir().unwrap();
        demo(&dir.path().join("photos"), 40, 40, 9);
        let mut d = driver(session(dir.path(), &[dir.path().join("photos")]), size);
        let _ui_thread = dalica_fs::mark_ui_thread();
        let (track, thumb, pane) = (widget(&d, "scroll:tree"), widget(&d, "scroll:tree/thumb"), widget(&d, "region:tree"));
        assert_eq!(track.rect[0] + track.rect[2], pane.rect[0] + pane.rect[2], "{w}: at the pane's right edge");
        assert_eq!((track.rect[2], track.rect[3]), (10.0, pane.rect[3]), "{w}: 10 wide, the pane's height");
        assert!(thumb.rect[3] < track.rect[3] && thumb.rect[1] == track.rect[1], "{w}: a proportional thumb at the top");
        // The rows end left of the bar.
        let first = rows(&d)[0].0.clone();
        let row = widget(&d, &first);
        assert!(row.rect[0] + row.rect[2] <= track.rect[0] + 0.01, "{w}: {:?} under the bar", row.rect);
        let info = folders::info(d.ctx());
        assert_eq!((info.offset, info.scrollbar.is_some()), (0.0, true));
        assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{w}: the bar's widgets have ids");
        let generation = info.generation;

        // Dragging the thumb down moves the rows up with it.
        d.drag("scroll:tree/thumb", Target::Delta(egui::vec2(0.0, 60.0)), &Default::default()).unwrap();
        let info = folders::info(d.ctx());
        let travel = track.rect[3] - thumb.rect[3];
        let want = 60.0 * (info.height - info.view_h) / travel;
        assert!((info.offset - want).abs() < 1.0, "{w}: offset {} after a 60 pt drag, want {want}", info.offset);
        assert_ne!(rows(&d)[0].0, first, "{w}: other rows at the top");
        let moved = widget(&d, "scroll:tree/thumb");
        assert!((moved.rect[1] - thumb.rect[1] - 60.0).abs() < 1.0, "{w}: the thumb followed the pointer: {:?}", moved.rect);
        check_tree(&d, &format!("region-tree-scroll-{w}"));

        // A press in the track below the thumb pages one view down; dragging back to the top
        // returns.
        let before = folders::info(d.ctx()).offset;
        let point = egui::pos2(track.rect[0] + 5.0, track.rect[1] + track.rect[3] - 4.0);
        let press = [PointerKind::Move, PointerKind::Down, PointerKind::Up].map(|kind| PointerEvent { kind, pos: point });
        d.pointer(&press, egui::PointerButton::Primary, egui::Modifiers::NONE).unwrap();
        d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: egui::pos2(700.0, 500.0) }], egui::PointerButton::Primary, egui::Modifiers::NONE)
            .unwrap();
        let after = folders::info(d.ctx());
        assert!((after.offset - (before + after.view_h).min(after.height - after.view_h)).abs() < 0.5, "{w}: {before} → {}", after.offset);
        d.drag("scroll:tree/thumb", Target::Delta(egui::vec2(0.0, -2000.0)), &Default::default()).unwrap();
        assert_eq!(folders::info(d.ctx()).offset, 0.0, "{w}: back at the top");
        assert_eq!(rows(&d)[0].0, first);
        assert_eq!(folders::info(d.ctx()).generation, generation, "{w}: scrolling rebuilt nothing");
    }
}
