//! The previous organizer's data in the window (`roadmap.md` M14.15; `organizing.md` §1 item 4,
//! `data-model.md` §9), driven headlessly by widget id on a library of two legacy-only folders
//! (fixtures written here from the documented format, their names from `dalica-scan`'s constant):
//!
//! - the notice gives the count once, stays until OK, and is not given after a restart;
//! - Options ▸ General's "Read photo data left by the previous organizer" off takes the folders'
//!   legacy stars away and on brings them back;
//! - Folder Properties names the folder's source before (the previous organizer's file) and after
//!   the first change (both, Dalica's in use);
//! - the edit panel counts a recipe's two `legacy:` entries;
//! - File ▸ Revert… on a photo the previous organizer saved brings its kept original back;
//! - the notice and the edit panel's line match their fingerprints at 1400 × 900
//!   (`tests/fingerprints/legacy-notice-1400x900.json`, `legacy-edit-line-1400x900.json`) and fit
//!   900 × 600.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_small_and_back, new_driver, shot, try_widget, widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_library::FolderSidecar;
use dalica_ui::dialogs::folder_properties::{SOURCE_ID, source_text};
use dalica_ui::notices::{OK_ID, legacy_text};
use dalica_ui::panels::edit::history::{LEGACY_ID, legacy_text as edits_text};
use dalica_ui::{App, Driver};
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(300);
const OPTION: &str = "check:option.files.readLegacyData";

fn put(path: &Path, bytes: &[u8]) {
    dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
    dalica_fs::replace(path, bytes, Expect::Absent).unwrap();
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 160, 120), 85, None).unwrap()
}

/// The legacy sidecar of folder `dir` holding `lines` (CRLF, as the old program writes it).
fn legacy(dir: &Path, lines: &[&str]) {
    let name = &dalica_scan::legacy::sidecar_names()[0];
    put(&dir.join(name), (lines.join("\r\n") + "\r\n").as_bytes());
}

/// The legacy originals folder's name, from the same constant: the sidecar is `.<stem>.ini`, the
/// originals folder `.<stem>originals` (`dalica-sidecar`'s `legacy::names`).
fn originals_dir() -> String {
    let name = &dalica_scan::legacy::sidecar_names()[0];
    let stem = name.trim_start_matches('.').trim_end_matches(".ini");
    format!(".{stem}originals")
}

struct Fx {
    dir: tempfile::TempDir,
    d: Driver,
}

/// `Pictures/Garden` (legacy-only): `a.jpg` starred, `b.jpg` with two edits Dalica has no
/// equivalent for, `c.jpg` saved by the old program with its original kept; `Pictures/Lake`
/// (legacy-only): `a.jpg` starred.
fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let garden = dir.path().join("Pictures").join("Garden");
    let lake = dir.path().join("Pictures").join("Lake");
    for (i, name) in ["a.jpg", "b.jpg", "c.jpg"].iter().enumerate() {
        put(&garden.join(name), &jpeg(10 + i as u64));
    }
    put(&lake.join("a.jpg"), &jpeg(20));
    legacy(&garden, &["[a.jpg]", "star=yes", "[b.jpg]", "filters=redeye=1;tilt=1,0.25;"]);
    legacy(&lake, &["[a.jpg]", "star=yes"]);
    // `c.jpg` as saved; its original and recipe in the originals folder.
    let kept = garden.join(originals_dir());
    put(&kept.join("c.jpg"), &jpeg(30));
    put(&kept.join(&dalica_scan::legacy::sidecar_names()[0]), b"[c.jpg]\r\nfilters=bw=1;\r\n");
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in [&garden, &lake] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, d }
}

impl Fx {
    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    fn folder(&self, name: &str) -> FolderId {
        self.session().library().folder_by_path(&self.dir.path().join("Pictures").join(name)).unwrap()
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        let f = self.folder(folder);
        self.session().library().photo_by_name(f, name).unwrap().id
    }

    fn path(&self, folder: &str, name: &str) -> PathBuf {
        self.dir.path().join("Pictures").join(folder).join(name)
    }

    fn star(&self, folder: &str, name: &str) -> bool {
        let id = self.photo(folder, name);
        self.session().library().photo(id).unwrap().star
    }

    fn click(&mut self, id: &str) {
        self.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        self.d.settle(FRAMES).unwrap();
    }

    /// Steps frames (letting the workers run) until `done` holds, nothing runs and no job is
    /// left, then settles.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Fx) -> bool) {
        let start = Instant::now();
        while !(done(self) && !self.session().busy() && !self.session().file_jobs_running()) {
            assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
            self.d.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        self.d.settle(FRAMES).unwrap();
    }

    /// The window at 900 × 600 as `<name>.png` in the test output, then large again.
    fn small_shot(&mut self, name: &str) {
        self.d.resize(SMALL.0, SMALL.1);
        self.d.settle(FRAMES).unwrap();
        shot(&self.d, name);
        self.d.resize(LARGE.0, LARGE.1);
        self.d.settle(FRAMES).unwrap();
    }

    /// The legacy notice's toast id, while it shows.
    fn notice(&self) -> Option<String> {
        let text = legacy_text(2);
        self.d.widgets().into_iter().find(|w| w.id.starts_with("toast:") && w.label == text).map(|w| w.id)
    }

    /// Folder Properties for `folder`: what its Photo data row says (the window as `<shot>.png` in
    /// the test output).
    fn source_said(&mut self, folder: &str, name: &str) -> String {
        let f = self.folder(folder);
        self.d.run("section.properties", &json!({ "folder": f })).unwrap();
        self.d.settle(FRAMES).unwrap();
        let said = widget(&self.d, SOURCE_ID).label;
        shot(&self.d, name);
        self.click("button:dialog.cancel");
        said
    }

    /// Scrolls the Options body until widget `id` is in view.
    fn reveal(&mut self, id: &str) {
        for _ in 0..40 {
            let body = widget(&self.d, "dialog:options").rect;
            let (top, bottom) = (body[1] + 80.0, body[1] + body[3] - 110.0);
            let dy = match try_widget(&self.d, id).map(|w| w.rect) {
                Some(r) if r[1] >= top && r[1] + r[3] <= bottom => return,
                Some(r) if r[1] < top => 60.0,
                _ => -60.0,
            };
            self.d.scroll(Some("dialog:options"), egui::vec2(0.0, dy)).unwrap();
            self.d.settle(FRAMES).unwrap();
        }
        panic!("{id} never came into view");
    }

    /// Options ▸ General's row for the previous organizer's data clicked, then Apply.
    fn toggle_option(&mut self) {
        self.d.run("app.preferences", &json!({ "tab": "general" })).unwrap();
        self.d.settle(FRAMES).unwrap();
        self.reveal(OPTION);
        shot(&self.d, "legacy-options-row");
        self.click(OPTION);
        self.click("button:dialog.ok");
        assert!(self.d.state().dialogs.is_empty(), "OK closed Options");
    }
}

/// The notice gives the number of folders once, stays until OK, and is not given again, not even
/// after a restart; it matches its fingerprint at 1400 × 900 and fits 900 × 600.
#[test]
fn the_notice_shows_once_with_the_count_and_not_after_a_restart() {
    let mut fx = fixture();
    let id = fx.notice().expect("the notice shows after the first scan");
    check_region(&fx.d, &id, "legacy-notice-1400x900");
    check_small_and_back(&mut fx.d, &id);
    fx.small_shot("legacy-notice-900x600");
    // It has no time of its own: still there long after a toast's.
    fx.d.wait(7000).unwrap();
    assert_eq!(fx.notice().as_deref(), Some(id.as_str()), "it waits for OK");
    fx.click(OK_ID);
    assert_eq!(fx.notice(), None, "OK closes it");
    let v = fx.d.run("legacy.inspect", &json!({})).unwrap();
    assert_eq!((&v["noticeShown"], &v["notice"]), (&json!(true), &json!(null)));
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.notice(), None, "given once");

    // A restart: the setting keeps it from coming again.
    let Fx { dir, d } = fx;
    drop(d.into_app());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let fx = Fx { dir, d };
    assert!(fx.star("Lake", "a.jpg"), "the legacy data is still read");
    assert_eq!(fx.notice(), None, "not after a restart");
}

/// The option off takes the legacy stars away (a rescan, nothing written) and on brings them back.
#[test]
fn the_option_off_hides_the_legacy_stars_and_on_brings_them_back() {
    let mut fx = fixture();
    assert!(fx.star("Garden", "a.jpg") && fx.star("Lake", "a.jpg"));
    fx.toggle_option();
    fx.until("the folders read without legacy data", |fx| !fx.star("Garden", "a.jpg") && !fx.star("Lake", "a.jpg"));
    assert!(!fx.path("Garden", ".dalica.json").exists(), "nothing written");
    fx.toggle_option();
    fx.until("the legacy stars back", |fx| fx.star("Garden", "a.jpg") && fx.star("Lake", "a.jpg"));
    assert!(!fx.path("Lake", ".dalica.json").exists(), "nothing written");
}

/// Folder Properties names the previous organizer's file before the first change, and both
/// (Dalica's in use) after it.
#[test]
fn folder_properties_names_the_source_before_and_after_the_first_change() {
    let mut fx = fixture();
    assert_eq!(fx.source_said("Lake", "legacy-folder-properties-before"), source_text("legacy"));
    let a = fx.photo("Lake", "a.jpg");
    fx.d.run("photo.star.set", &json!({ "ids": [a], "on": false })).unwrap();
    let lake = fx.folder("Lake");
    fx.until("the first write", |fx| fx.session().library().folder_sidecar(lake) == FolderSidecar::Both);
    assert!(fx.path("Lake", ".dalica.json").exists());
    assert_eq!(fx.source_said("Lake", "legacy-folder-properties-after"), source_text("both"));
}

/// The edit panel counts the recipe's two `legacy:` entries; the line matches its fingerprint at
/// 1400 × 900 and fits 900 × 600. A photo without any shows no line.
#[test]
fn the_edit_panel_counts_the_previous_programs_edits() {
    let mut fx = fixture();
    let b = fx.photo("Garden", "b.jpg");
    let edits = fx.session().library().info(b).edits.clone();
    assert_eq!(edits.iter().filter(|e| e.starts_with("legacy:")).count(), 2, "{edits:?}");
    fx.d.run("view.edit", &json!({ "id": b })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let line = widget(&fx.d, LEGACY_ID);
    assert_eq!(Some(line.label), edits_text(2));
    assert_eq!(edits_text(2).as_deref(), Some("2 edits from the previous program (not supported)"));
    check_region(&fx.d, LEGACY_ID, "legacy-edit-line-1400x900");
    check_small_and_back(&mut fx.d, LEGACY_ID);
    fx.small_shot("legacy-edit-line-900x600");
    let a = fx.photo("Garden", "a.jpg");
    fx.d.run("view.edit", &json!({ "id": a })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.widgets().iter().all(|w| w.id != LEGACY_ID), "no line without such edits");
}

/// File ▸ Revert… on a photo the old program saved asks M8's question; Revert brings the kept
/// original back and leaves the originals folder alone.
#[test]
fn revert_from_the_menu_restores_the_legacy_original() {
    let mut fx = fixture();
    let c = fx.photo("Garden", "c.jpg");
    let kept = fx.path("Garden", &originals_dir()).join("c.jpg");
    let original = std::fs::read(&kept).unwrap();
    assert_ne!(std::fs::read(fx.path("Garden", "c.jpg")).unwrap(), original);
    fx.d.run("select.set", &json!({ "ids": [c] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.click("menu:File");
    fx.click("menuitem:photo.revert");
    assert_eq!(fx.d.state().dialogs.last().map(|d| d.kind.name()), Some("revert"));
    fx.click("button:dialog.revert");
    fx.until("reverted", |fx| std::fs::read(fx.path("Garden", "c.jpg")).is_ok_and(|b| b == original));
    assert!(fx.session().library().info(c).edits.is_empty(), "the edits dropped");
    assert_eq!(std::fs::read(&kept).unwrap(), original, "the originals folder is never written");
}
