//! The export progress window and the file-job notices (M8.21, `creating.md` §2,
//! `ui-layout.md` §4), driven headlessly.
//!
//! - An export that starts opens `dialog:exportProgress` with its count ("Exporting 3 of 12…") and
//!   a bar; the window closes when the job ends, and the job's toast "Exported 12 pictures" offers
//!   Show Folder, which (once the scan has brought the folder in) selects the export folder as a
//!   unit and scrolls the light table to it.
//! - The window's Cancel stops the job: it ends cancelled, only complete files are left in the
//!   export folder, the window closes and the toast says so.
//! - A window set aside while its job runs stays set aside.
//! - A Save that fails on a read-only file gives exactly one `caution` toast, naming the file and
//!   the reason (a Save job on M8.3's runner, as M8.15's will run).
//!
//! The window is checked against fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//! (`dialog-export-progress-1400x900.json`, a job held at "1 of 4"); renders go to
//! `target/test-output/ui/`, and `DALICA_BLESS=1` stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_rect, new_driver, out_dir, try_widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{Done, FileItem, FileJob, JobKind, JobState, Session};
use dalica_fs::Expect;
use dalica_scenes::fingerprint;
use dalica_ui::dialogs::export_progress;
use dalica_ui::notices::{SHOW_FOLDER_ID, event_toast};
use dalica_ui::state::{ExportProgress, Section};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(300);

fn jpeg(seed: u64, width: u32, height: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, width, height), 85, None).unwrap()
}

struct T {
    dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<PhotoId>,
}

impl T {
    /// A library watching `Photos` (its `Lisbon` folder holds `n` photos of `w` × `h`), with
    /// `Pictures` as the Pictures folder, in a `size` window.
    fn new(n: usize, (w, h): (u32, u32), size: (u32, u32)) -> T {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Photos").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64, w, h), Expect::Absent).unwrap();
        }
        let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), size.0, size.1, 1.0);
        d.settle(FRAMES).unwrap();
        T { dir, d, ids: photos.into_iter().map(|(_, id)| id).collect() }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("Out")
    }

    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    /// Steps frames (letting the workers run between them) until `done` holds.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Driver) -> bool) {
        let start = Instant::now();
        while !done(&self.d) {
            assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
            self.d.step();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn job(&self, job: u64) -> dalica_engine::JobEntry {
        self.session().job(job).unwrap()
    }

    fn toasts(&self) -> Vec<Value> {
        let now = self.d.ctx().input(|i| i.time);
        self.d.app().notices().inspect(now).as_array().unwrap().clone()
    }
}

/// The files in `dir`, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// `path` decoded whole: its pixel size.
fn decoded_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    let img = dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap();
    (img.width, img.height)
}

/// Writes the last frame to `target/test-output/ui/<name>.png`.
fn save_window(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// Checks the region `rect` (points, at scale 1) of the last frame against `<name>.json`.
fn check_region(d: &Driver, rect: [f32; 4], name: &str) {
    check_rect(d, name, rect);
}

/// An export of 12 photos: the window shows its count and bar while it runs and closes when it
/// ends; the toast's Show Folder selects the export folder and scrolls the light table to it.
#[test]
fn the_window_follows_an_export_and_show_folder_brings_the_folder_up() {
    let mut t = T::new(12, (1200, 900), (1400, 900));
    let out = t.out();
    let reply = t.d.run("export.run", &json!({ "ids": t.ids, "dir": out, "folderName": "Lisbon Weekend", "size": "original" })).unwrap();
    let job = reply["job"].as_u64().unwrap();
    t.d.step();
    assert_eq!(t.d.state().export_progress, Some(ExportProgress { job, shown: true }), "the export opened the window");
    assert!(try_widget(&t.d, export_progress::ID).is_some());
    let bar = try_widget(&t.d, export_progress::BAR_ID).expect("the bar");
    assert!(bar.label.starts_with("Exporting ") && bar.label.ends_with(" of 12…"), "{}", bar.label);
    let cancel = try_widget(&t.d, export_progress::CANCEL_ID).expect("Cancel");
    assert!(cancel.enabled);
    // Not modal: the light table keeps its input (a photo can still be clicked).
    t.d.click(&format!("thumb:{}", t.ids[0])).unwrap();
    assert_eq!(t.d.state().selection.ids, [t.ids[0]], "the window under it keeps taking input");
    // The count follows the job.
    t.until("a photo done", |d| d.app().session().unwrap().job(job).unwrap().progress.done >= 1);
    t.d.step();
    let label = try_widget(&t.d, export_progress::BAR_ID).map(|w| w.label);
    let entry = t.job(job);
    if entry.state == JobState::Running {
        assert_eq!(label, Some(format!("{}…", entry.label)), "the line is the job's progress");
        save_window(&t.d, "export-progress-window-1400x900");
    }

    // The window closes when the job ends, and the job's toast offers Show Folder.
    t.until("the window to close", |d| d.state().export_progress.is_none());
    assert_eq!(t.job(job).state, JobState::Done);
    t.d.step();
    assert!(try_widget(&t.d, export_progress::ID).is_none(), "closed");
    let lisbon = out.join("Lisbon Weekend");
    let toasts = t.toasts();
    let toast = toasts.iter().find(|v| v["text"] == "Exported 12 pictures").unwrap_or_else(|| panic!("{toasts:?}"));
    assert_eq!(toast["kind"], "info");
    assert_eq!(toast["action"], json!({ "id": SHOW_FOLDER_ID, "folder": lisbon }));
    assert!(try_widget(&t.d, SHOW_FOLDER_ID).is_some(), "the button is drawn");
    save_window(&t.d, "export-progress-toast-1400x900");
    // Show Folder, pressed at once: it waits for the scan to bring the folder in, then selects it
    // as a unit and scrolls the light table to it.
    t.d.click(SHOW_FOLDER_ID).unwrap();
    t.until("the folder to be shown", |d| {
        let folder = d.app().session().unwrap().library().folder_by_path(&lisbon);
        folder.is_some() && d.state().folder == folder
    });
    let folder: FolderId = t.session().library().folder_by_path(&lisbon).unwrap();
    let photos = t.session().library().photos_in(folder).len();
    assert_eq!(photos, 12);
    assert_eq!(t.d.state().selection.ids.len(), 12, "the whole folder is selected");
    assert_eq!(t.d.state().scroll.section, Some(Section::Folder(folder)), "the light table scrolls to it");
    assert!(!t.toasts().iter().any(|v| v["text"] == "Exported 12 pictures"), "the toast went with its button");
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().notices().wanted(), None);
    save_window(&t.d, "export-progress-shown-folder-1400x900");
}

/// Cancel in the window stops the job: it ends cancelled with only complete files in the export
/// folder, the window closes and the toast says how many were exported.
#[test]
fn cancel_stops_the_job_and_leaves_only_complete_files() {
    let mut t = T::new(12, (1200, 900), (1400, 900));
    let out = t.out();
    let job = t.d.run("export.run", &json!({ "ids": t.ids, "dir": out, "folderName": "Big", "size": "original" })).unwrap()["job"].as_u64().unwrap();
    t.until("a photo done", |d| d.app().session().unwrap().job(job).unwrap().progress.done >= 1);
    t.d.step();
    if t.job(job).state == JobState::Running {
        t.d.click(export_progress::CANCEL_ID).unwrap();
        // Until the job ends the window says it is cancelling, its button disabled.
        if t.d.state().export_progress.is_some() {
            let bar = try_widget(&t.d, export_progress::BAR_ID).unwrap();
            assert_eq!(bar.label, export_progress::CANCELLING);
            assert!(!try_widget(&t.d, export_progress::CANCEL_ID).unwrap().enabled);
        }
    }
    t.until("the window to close", |d| d.state().export_progress.is_none());
    let done = t.job(job);
    assert_eq!(done.state, JobState::Cancelled, "{done:?}");
    let result = done.result.unwrap();
    let written = result["written"].as_array().unwrap().len();
    assert!((1..12).contains(&written), "{written} written");
    // Only complete files: each decodes whole at full size; no temporary file is left.
    let big = out.join("Big");
    let names = files(&big);
    assert_eq!(names.len(), written + 1, "{names:?}");
    for name in names.iter().filter(|n| *n != ".dalica.json") {
        assert!(name.starts_with("IMG_") && name.ends_with(".jpg"), "{name}");
        assert_eq!(decoded_size(&big.join(name)), (1200, 900));
    }
    t.d.step();
    let toasts = t.toasts();
    let text = format!("Export cancelled: {written} of 12 pictures exported");
    assert!(toasts.iter().any(|v| v["text"] == text.as_str() && v["kind"] == "info"), "{toasts:?}");
    assert!(try_widget(&t.d, export_progress::ID).is_none());
}

/// A window set aside while its job runs is not opened again; the next export opens it.
#[test]
fn a_window_set_aside_stays_aside_while_its_job_runs() {
    let mut t = T::new(6, (1200, 900), (900, 600));
    let out = t.out();
    let job = t.d.run("export.run", &json!({ "ids": t.ids, "dir": out, "folderName": "A", "size": "original" })).unwrap()["job"].as_u64().unwrap();
    t.d.step();
    assert_eq!(t.d.state().export_progress, Some(ExportProgress { job, shown: true }));
    t.d.app_mut().state_mut().export_progress = None;
    t.d.step();
    assert_eq!(t.d.state().export_progress, None, "not taken up again");
    t.d.app_mut().session_mut().unwrap().wait_job(job, PATIENCE).unwrap();
    t.d.app_mut().session_mut().unwrap().wait_idle();
    t.d.step();
    let second = t.d.run("export.run", &json!({ "ids": [t.ids[0]], "dir": out, "folderName": "B", "size": 100 })).unwrap()["job"].as_u64().unwrap();
    t.d.step();
    let state = t.d.state().export_progress;
    assert!(state == Some(ExportProgress { job: second, shown: true }) || t.job(second).state.is_finished(), "{state:?}");
    t.until("the second window to close", |d| d.state().export_progress.is_none());
}

/// One item of a test file job.
struct Item(PathBuf);

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.0
    }
}

/// A Save of a read-only file fails: one `caution` toast names the file and the reason.
#[test]
fn a_failed_save_of_a_read_only_file_gives_one_toast() {
    let mut t = T::new(2, (200, 150), (1400, 900));
    let path = t.dir.path().join("Photos").join("Lisbon").join("IMG_0001.jpg");
    let writable = std::fs::metadata(&path).unwrap().permissions();
    let mut perms = writable.clone();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms).unwrap();
    let before = t.toasts().len();
    // As a Save refuses a file it may not write: before touching it.
    let job = FileJob::new(JobKind::Save, "Saving", vec![Item(path.clone())], |item: Item| {
        let read_only = std::fs::metadata(&item.0).map_err(|e| e.to_string())?.permissions().readonly();
        if read_only { Err("the file is read-only".to_string()) } else { Ok(Done { changes: Vec::new(), value: () }) }
    });
    let id = t.d.app_mut().session_mut().unwrap().start_file_job(job);
    t.until("the Save to end", |d| d.app().session().unwrap().job(id).is_some_and(|e| e.state.is_finished()));
    t.d.step();
    t.d.step();
    let toasts = t.toasts();
    let fresh: Vec<&Value> = toasts.iter().filter(|v| v["text"].as_str().is_some_and(|s| s.contains("IMG_0001.jpg"))).collect();
    assert_eq!(fresh.len(), 1, "{toasts:?}");
    assert_eq!(toasts.len(), before + 1, "{toasts:?}");
    assert_eq!(fresh[0]["text"], "Could not save IMG_0001.jpg: the file is read-only");
    assert_eq!(fresh[0]["kind"], "caution");
    assert!(fresh[0].get("action").is_none(), "no folder to show");
    // The event stays in the session: later frames show nothing more.
    t.d.wait(100).unwrap();
    let again = t.toasts();
    assert_eq!(again.iter().filter(|v| v["text"].as_str().is_some_and(|s| s.contains("IMG_0001.jpg"))).count(), 1);
    let event = t.session().file_job_events().last().unwrap().clone();
    assert_eq!(event_toast(&event).whole, "IMG_0001.jpg", "the name stays whole when cut short");
    save_window(&t.d, "export-progress-failed-save-1400x900");
    // Writable again, so the temporary folder can go (Windows refuses to delete a read-only file).
    std::fs::set_permissions(&path, writable).unwrap();
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

fn open(gate: &Gate) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

/// The window, its job held at "1 of 4", matches its fingerprint, and fits the small window.
#[test]
fn the_window_matches_its_fingerprints() {
    let (w, h) = LARGE;
    let mut t = T::new(4, (200, 150), (w, h));
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let held = Arc::clone(&gate);
    let items: Vec<Item> = (1..=4).map(|i| Item(PathBuf::from(format!("IMG_{i:04}.jpg")))).collect();
    let job = FileJob::new(JobKind::Export, "Exporting", items, move |item: Item| {
        if item.0 != Path::new("IMG_0001.jpg") {
            let (lock, cv) = &*held;
            let _ = cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(60), |open| !*open).unwrap();
        }
        Ok(Done { changes: Vec::new(), value: () })
    });
    let id = t.d.app_mut().session_mut().unwrap().start_file_job(job);
    t.until("one item done", |d| d.app().session().unwrap().job(id).unwrap().progress.done == 1);
    // A few frames for the window and its line to settle.
    for _ in 0..4 {
        t.d.step();
    }
    let bar = try_widget(&t.d, export_progress::BAR_ID).unwrap();
    assert_eq!(bar.label, "Exporting 1 of 4…");
    let win = try_widget(&t.d, export_progress::ID).unwrap();
    let rect = win.rect;
    assert!((rect[2] - export_progress::WIDTH).abs() < 1.0, "{rect:?}");
    assert!(rect[0] + rect[2] <= w as f32 && rect[1] + rect[3] <= h as f32, "inside the window: {rect:?}");
    save_window(&t.d, &format!("export-progress-held-{w}x{h}"));
    check_region(&t.d, rect, &format!("dialog-export-progress-{w}x{h}"));
    t.d.resize(SMALL.0, SMALL.1);
    for _ in 0..4 {
        t.d.step();
    }
    let r = try_widget(&t.d, export_progress::ID).unwrap().rect;
    assert!((r[2] - export_progress::WIDTH).abs() < 1.0 && r[0] >= 0.0 && r[1] >= 0.0, "{r:?}");
    assert!(r[0] + r[2] <= SMALL.0 as f32 && r[1] + r[3] <= SMALL.1 as f32, "inside the small window: {r:?}");
    open(&gate);
    t.until("the window to close", |d| d.state().export_progress.is_none());
}
