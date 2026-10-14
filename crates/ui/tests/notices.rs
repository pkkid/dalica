//! Failed metadata writes as toasts (`roadmap.md` M6.33, M7.27; `xmp-interop.md` §1 principle 4,
//! `design-system.md` §6 `toast`), driven headlessly: a caption set on a read-only photo and a
//! writable one gives exactly one `caution` toast, naming the file and the field, drawn as
//! `toast:<n>` and reported by `ui.inspect` → `toasts`; later frames add none; a later failure
//! adds its own; the folder sidecar keeps the value. `ui.settle` with `jobs` waits for the queued
//! write to land (debounce included; without `jobs` it does not, as for any long job), and the toast shows the whole message, wrapped, at 1400 × 900 and 900 × 600
//! (`tests/fingerprints/toast-caution-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones). Renders go
//! to `target/test-output/ui/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_scenes::fingerprint;
use dalica_ui::{App, Driver};
use dalica_ui_kit::widgets::{TOAST_LINES, ToastText, toast_galley};
use egui::{LayerId, Painter, Rect, pos2, vec2};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const WAIT: Duration = Duration::from_secs(30);

/// A library watching one folder `Lisbon` of `a.jpg` and `b.jpg`, `a.jpg` read-only.
fn setup(size: (u32, u32)) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Driver, PhotoId, PhotoId) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for (i, name) in ["a.jpg", "b.jpg"].into_iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(name), &bytes, Expect::Absent).unwrap();
    }
    set_read_only(&folder.join("a.jpg"), true);
    let mut s = Session::open(lib.path(), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = |s: &Session, n: &str| s.library().photos().find(|p| p.name == n).unwrap().id;
    let (a, b) = (id(&s, "a.jpg"), id(&s, "b.jpg"));
    let mut d = Driver::new(App::new(Some(s)), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    (lib, disk, folder, d, a, b)
}

fn set_read_only(p: &Path, read_only: bool) {
    let mut perms = std::fs::metadata(p).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(if read_only { 0o444 } else { 0o644 });
    }
    #[cfg(not(unix))]
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    std::fs::set_permissions(p, perms).unwrap();
}

fn session(d: &mut Driver) -> &mut Session {
    d.app_mut().session_mut().unwrap()
}

/// `ui.inspect` → `toasts` now.
fn toasts(d: &Driver) -> Vec<Value> {
    let now = d.ctx().input(|i| i.time);
    d.app().notices().inspect(now).as_array().cloned().unwrap_or_default()
}

/// Sets `caption` on `ids` and writes the files now; the next frame polls the session.
fn caption(d: &mut Driver, ids: &[PhotoId], caption: &str) {
    session(d).execute("photo.caption.set", &json!({ "ids": ids, "caption": caption })).unwrap();
    assert!(session(d).flush_metadata(WAIT));
    d.step();
}

fn save(d: &Driver, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

#[test]
fn a_read_only_file_gives_one_toast_naming_the_file_and_field() {
    for size in [(1400, 900), (900, 600)] {
        let (_l, _d, folder, mut d, a, b) = setup(size);
        assert!(toasts(&d).is_empty());
        caption(&mut d, &[a, b], "Tram 28");
        let list = toasts(&d);
        assert_eq!(list.len(), 1, "{list:?}");
        let text = list[0]["text"].as_str().unwrap();
        assert!(text.starts_with("Could not write the caption into a.jpg: ") && text.contains("read-only"), "{text}");
        assert_eq!(list[0]["kind"], "caution");
        let id = list[0]["id"].as_str().unwrap().to_string();
        assert!(d.widgets().iter().any(|w| w.id == id), "the toast is drawn");
        // Later frames add nothing: each failure shows once.
        for _ in 0..30 {
            d.step();
        }
        save(&d, &format!("notices-failed-write-{}x{}", size.0, size.1));
        assert_eq!(toasts(&d).len(), 1);
        // The sidecar keeps the value; the writable photo has it in its file.
        session(&mut d).flush().unwrap();
        let sidecar: Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
        assert_eq!(sidecar["photos"]["a.jpg"]["caption"], json!("Tram 28"));
        let b_file = std::fs::read(folder.join("b.jpg")).unwrap();
        assert!(b_file.windows(7).any(|w| w == b"Tram 28"), "b.jpg holds the caption");
        assert!(!std::fs::read(folder.join("a.jpg")).unwrap().windows(7).any(|w| w == b"Tram 28"), "a.jpg is as it was");
        // A later failure gives its own toast; a write that works gives none.
        caption(&mut d, &[a], "Tram 28 again");
        let list = toasts(&d);
        assert_eq!(list.len(), 2, "{list:?}");
        assert_ne!(list[1]["id"], json!(id));
        caption(&mut d, &[b], "Fine");
        assert_eq!(toasts(&d).len(), 2);
        set_read_only(&folder.join("a.jpg"), false);
    }
}

/// The message a read-only file's failed write gives.
const MESSAGE: &str = "Could not write the caption into a.jpg: the file is read-only; the folder sidecar keeps the value";

/// The toast's text as it is laid out now: what it shows and on how many lines.
fn shown(d: &Driver, text: &str) -> (String, usize) {
    let (w, h, ppp) = d.size();
    let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(w as f32 / ppp, h as f32 / ppp));
    let area = dalica_ui::panels::layout::compute(window, d.state()).light_table;
    let painter = Painter::new(d.ctx().clone(), LayerId::debug(), Rect::EVERYTHING);
    let galley = toast_galley(&painter, d.app().tokens(), ToastText::new(text, 0), area.width());
    (galley.job.text.clone(), galley.rows.len())
}

/// Checks the region of widget `id` against `tests/fingerprints/<name>.json` and writes it to the
/// test output.
fn check_region(d: &Driver, id: &str, name: &str) {
    let s = d.screenshot().unwrap();
    let r = d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no widget {id}")).rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&s.pixel(x, y).unwrap());
        }
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(dir.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// Sets `caption` on `ids` without a flush and checks the writes are queued, then `ui.settle` with
/// `jobs`: it must settle with nothing left to write.
fn caption_and_settle(d: &mut Driver, ids: &[PhotoId], caption: &str) {
    session(d).execute("photo.caption.set", &json!({ "ids": ids, "caption": caption })).unwrap();
    assert!(session(d).pending_metadata_writes() > 0, "the writes are queued, not written");
    assert!(d.busy(true).iter().any(|b| b.starts_with("metadata writes: ")), "{:?}", d.busy(true));
    assert!(!d.busy(false).iter().any(|b| b.starts_with("metadata writes")), "a long job: only with `jobs`");
    let s = d.settle_for(FRAMES, true);
    assert!(s.settled, "{s:?}");
    assert_eq!(session(d).pending_metadata_writes(), 0, "settled means written");
}

/// Without a flush: `ui.settle` with `jobs` returns only once the queued (debounced) write has
/// landed, and the caution toast a failed one gives shows the whole message, wrapped within three
/// lines, at both sizes.
///
/// The writable photo settles first, the read-only one after it. Captioning both at once made the
/// toast's presence depend on disk speed: the failed write landed first, and while `settle` waited
/// for the other file's write it ran frames as fast as it could, each 1/60 s of frame time, so a
/// write taking about a second of wall time used up the toast's 6 s before `settle` returned (seen
/// on Windows CI: `settle` came back with no toast). Now the failure is the last write `settle`
/// waits for, so the toast is checked a fixed few frames after it appeared, whatever the disk.
#[test]
fn settle_waits_for_the_write_and_the_caution_toast_shows_the_whole_message() {
    for size in [(1400, 900), (900, 600)] {
        let (_l, _d, folder, mut d, a, b) = setup(size);
        caption_and_settle(&mut d, &[b], "Tram 28");
        assert!(std::fs::read(folder.join("b.jpg")).unwrap().windows(7).any(|w| w == b"Tram 28"), "b.jpg holds the caption");
        assert_eq!(toasts(&d), Vec::<Value>::new(), "a write that works gives no toast");
        caption_and_settle(&mut d, &[a], "Tram 28");
        let list = toasts(&d);
        assert_eq!(list.len(), 1, "{list:?}");
        let remaining = list[0]["remaining"].as_f64().unwrap();
        assert!(remaining > 5.0, "checked soon after the failure landed, not near the toast's end: {remaining} s left");
        assert_eq!((list[0]["text"].as_str(), list[0]["kind"].as_str()), (Some(MESSAGE), Some("caution")));
        let (text, lines) = shown(&d, MESSAGE);
        assert_eq!(text, MESSAGE, "the whole message shows");
        assert!((2..=TOAST_LINES).contains(&lines), "wrapped, not cut: {lines} lines");
        let id = list[0]["id"].as_str().unwrap().to_string();
        save(&d, &format!("toast-caution-window-{}x{}", size.0, size.1));
        check_region(&d, &id, &format!("toast-caution-{}x{}", size.0, size.1));
        set_read_only(&folder.join("a.jpg"), false);
    }
}
