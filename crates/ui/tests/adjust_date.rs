//! Adjust Date and Time (M6.29, `organizing.md` §9), driven headlessly by widget id on a folder of
//! four photos taken a day apart at 10:00:
//!
//! - Tools ▸ Adjust Date and Time… opens "Adjust Picture Date — 3 pictures" for three selected
//!   photos, shift mode on; typing a new time shifts all three by +1 h 30 min in one undo step;
//!   one photo set to a date of its own moves to the front of its folder;
//! - the date and time fields refuse a day that does not exist and step with the arrows;
//! - an adjusted date re-orders the light table and relabels what shows dates: the Date caption,
//!   the tooltip, the status line and the section's date;
//! - undo takes each step back whole;
//! - every widget has an id, and the dialog matches its fingerprints at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/dialog-adjust-date-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones,
//!   renders go to `target/test-output/ui/`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{Date, DateTime, FolderId, PhotoId};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::dialogs::adjust_date::{DATE_ID, SET_ID, SHIFT_ID, TIME_ID};
use dalica_ui::state::{DialogKind, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::{WidgetInfo, registry};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A JPEG of `seed` taken on 2026-06-`day` at 10:00 (EXIF DateTimeOriginal).
fn put_jpeg(path: &Path, seed: u64, day: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let taken = DateTime::new(Date::new(2026, 6, day).unwrap(), 10, 0, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, Some(&exif)).unwrap();
    std::fs::write(path, jpeg).unwrap();
}

struct Fx {
    _dir: tempfile::TempDir,
    folder: FolderId,
    /// The photos in date order: taken on June 4, 5, 6 and 7.
    ids: Vec<PhotoId>,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos").join("Lisbon Weekend");
    for (i, day) in [4, 5, 6, 7].into_iter().enumerate() {
        put_jpeg(&photos.join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.wait_idle();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    let folder = s.library().folder_by_path(&photos).unwrap();
    let mut ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    ids.sort_by_key(|id| s.library().photo(*id).map(|p| p.name.clone()));
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, folder, ids, d }
}

fn widget(d: &Driver, id: &str) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == id)
}

fn has(d: &Driver, id: &str) -> bool {
    widget(d, id).is_some()
}

fn fields(d: &Driver) -> serde_json::Map<String, Value> {
    let top = d.state().dialogs.last().expect("a dialog is open");
    assert_eq!(top.kind, DialogKind::AdjustDate);
    top.fields.clone()
}

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

/// The photo's effective date.
fn date(d: &Driver, id: PhotoId) -> Option<DateTime> {
    d.app().session().unwrap().library().effective_date(id)
}

fn undo_steps(d: &Driver) -> (usize, Option<&'static str>) {
    let u = d.app().session().unwrap().undo_stack();
    (u.len(), u.undo_label())
}

/// The folder's photos in light-table order.
fn order(d: &Driver, folder: FolderId) -> Vec<PhotoId> {
    d.app().panels().unwrap().sections.get(Section::Folder(folder)).expect("the folder's section").photos.clone()
}

fn select(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Tools ▸ Adjust Date and Time…, clicked.
fn open(d: &mut Driver) {
    d.click("menu:Tools").unwrap();
    d.settle(FRAMES).unwrap();
    d.click("menuitem:photo.adjustDate").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(has(d, "dialog:adjustDate"), "the menu item opens the dialog");
}

/// Replaces the text of a field: click, select all, type.
fn retype(d: &mut Driver, id: &str, text: &str) {
    d.click(id).unwrap();
    d.key("Ctrl+A").unwrap();
    d.type_text(text).unwrap();
}

/// Checks the last frame: no unregistered interactive widget, unique ids in the grammar, every
/// rect on screen.
fn check_ids(d: &Driver) {
    let (w, h, _) = d.size();
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for wi in d.widgets() {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.clone()), "{w}×{h}: `{}` is registered twice", wi.id);
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{w}×{h}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes the whole window to the test output as `<name>.png` (and to `$DALICA_SHOTS` when set).
fn shot(d: &Driver, name: &str) {
    let s = d.screenshot().unwrap();
    let png = fingerprint::png_rgba(s.width, s.height, &s.rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
}

/// Checks the region widget `id` covers against `tests/fingerprints/<name>.json`, and writes it
/// and the whole window to the test output.
fn check_region(d: &Driver, id: &str, name: &str) {
    shot(d, &format!("{name}-window"));
    let s = d.screenshot().unwrap();
    let r = widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&s.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
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

/// Three photos shifted by +1 h 30 min and one set to a date of its own, all by widget id, each in
/// one undo step; undo takes them back.
#[test]
fn shift_three_photos_and_set_one_by_id_then_undo() {
    let mut fx = fixture((1400, 900));
    let (folder, ids) = (fx.folder, fx.ids.clone());
    assert_eq!(order(&fx.d, folder), ids);

    // Three photos: shift mode by default, the first photo's date as the current one.
    select(&mut fx.d, &ids[..3]);
    open(&mut fx.d);
    let f = fields(&fx.d);
    assert_eq!(
        Value::Object(f),
        json!({ "ids": &ids[..3], "current": "2026-06-04T10:00:00", "mode": "shift", "shift": 0, "set": "2026-06-04T10:00:00" })
    );
    let inspect = dalica_ui::dialogs::inspect(fx.d.app());
    assert_eq!(inspect["title"], "Adjust Picture Date — 3 pictures");
    check_ids(&fx.d);
    assert!(widget(&fx.d, SHIFT_ID).unwrap().active && !widget(&fx.d, SET_ID).unwrap().active, "Shift is the default");

    // The arrows step the hour and back; a day that does not exist is refused.
    fx.d.click(TIME_ID).unwrap();
    fx.d.key("Home").unwrap();
    fx.d.key("Up").unwrap();
    assert_eq!(fields(&fx.d)["shift"], 3600, "↑ on the hour: one hour later");
    fx.d.key("Down").unwrap();
    assert_eq!(fields(&fx.d)["shift"], 0);
    retype(&mut fx.d, DATE_ID, "2026-02-31");
    assert_eq!(fields(&fx.d)["shift"], 0, "31 February is refused");
    fx.d.key("Tab").unwrap();
    assert_eq!(fields(&fx.d)["set"], "2026-06-04T10:00:00", "leaving the field keeps the date it had");

    // +1 h 30 min, typed.
    retype(&mut fx.d, TIME_ID, "11:30:00");
    fx.d.key("Tab").unwrap();
    let f = fields(&fx.d);
    assert_eq!((f["mode"].clone(), f["shift"].clone(), f["set"].clone()), (json!("shift"), json!(5400), json!("2026-06-04T11:30:00")));
    assert!(widget(&fx.d, "button:dialog.ok").unwrap().enabled);
    shot(&fx.d, "adjust-date-shift-1400x900");
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty(), "OK closes the dialog");
    let dates: Vec<Option<DateTime>> = ids.iter().map(|id| date(&fx.d, *id)).collect();
    assert_eq!(
        dates,
        [Some(dt("2026-06-04T11:30:00")), Some(dt("2026-06-05T11:30:00")), Some(dt("2026-06-06T11:30:00")), Some(dt("2026-06-07T10:00:00"))]
    );
    assert_eq!(undo_steps(&fx.d), (1, Some("Adjust Date")), "one undo step");

    // One photo: no radios; set to June 1, 08:00, it moves to the front.
    select(&mut fx.d, &ids[3..]);
    open(&mut fx.d);
    assert!(!has(&fx.d, SHIFT_ID) && !has(&fx.d, SET_ID), "one photo: no radios");
    assert_eq!(dalica_ui::dialogs::inspect(fx.d.app())["title"], "Adjust Picture Date");
    check_ids(&fx.d);
    retype(&mut fx.d, DATE_ID, "2026-06-01");
    retype(&mut fx.d, TIME_ID, "08:00:00");
    fx.d.key("Tab").unwrap();
    assert_eq!(fields(&fx.d)["set"], "2026-06-01T08:00:00");
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(date(&fx.d, ids[3]), Some(dt("2026-06-01T08:00:00")));
    assert_eq!(undo_steps(&fx.d), (2, Some("Adjust Date")));
    assert_eq!(order(&fx.d, folder), [ids[3], ids[0], ids[1], ids[2]], "the light table re-sorts by the new date");

    // Set mode for several: all to one date and time.
    select(&mut fx.d, &ids[..2]);
    open(&mut fx.d);
    fx.d.click(SET_ID).unwrap();
    assert_eq!(fields(&fx.d)["mode"], "set");
    retype(&mut fx.d, DATE_ID, "2026-06-09");
    fx.d.key("Tab").unwrap();
    fx.d.click("button:dialog.cancel").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!((date(&fx.d, ids[0]), undo_steps(&fx.d).0), (Some(dt("2026-06-04T11:30:00")), 2), "Cancel changes nothing");

    // Undo takes each step back whole.
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(date(&fx.d, ids[3]), Some(dt("2026-06-07T10:00:00")));
    assert_eq!(order(&fx.d, folder), ids);
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let dates: Vec<Option<DateTime>> = ids.iter().map(|id| date(&fx.d, *id)).collect();
    let taken = [4, 5, 6, 7].map(|day| Some(DateTime::new(Date::new(2026, 6, day).unwrap(), 10, 0, 0).unwrap()));
    assert_eq!(dates, taken);
    assert_eq!(undo_steps(&fx.d).0, 0);
}

/// An adjusted date re-orders the folder and relabels everything that shows a photo's date: the
/// Date caption, the tooltip, the status line and the section's date line; undo puts them back.
#[test]
fn an_adjusted_date_reorders_and_relabels() {
    let mut fx = fixture((1400, 900));
    let (folder, ids) = (fx.folder, fx.ids.clone());
    fx.d.run("view.caption", &json!({ "caption": "date" })).unwrap();
    let last = ids[3];
    fx.d.run("photo.dates.set", &json!({ "ids": [last], "set": "2026-05-30T08:15:00" })).unwrap();
    select(&mut fx.d, &[last]);
    fx.d.settle(FRAMES).unwrap();

    assert_eq!(order(&fx.d, folder), [ids[3], ids[0], ids[1], ids[2]], "re-ordered");
    let cells = &fx.d.app().panels().unwrap().light_table.cells;
    let drawn: Vec<PhotoId> = cells.shown().iter().map(|s| s.photo).collect();
    assert_eq!(drawn, [ids[3], ids[0], ids[1], ids[2]], "drawn in the new order");
    assert_eq!(cells.get(last).and_then(|s| s.caption.clone()).as_deref(), Some("May 30, 2026"), "the Date caption");
    assert_eq!(cells.get(ids[0]).and_then(|s| s.caption.clone()).as_deref(), Some("June 4, 2026"));
    let status = fx.d.state().status_line.clone();
    assert!(status.contains("May 30, 2026 8:15 AM"), "the status line: {status}");
    let (header_date, _) = fx.d.app().panels().unwrap().light_table.headers.shown(Section::Folder(folder)).unwrap();
    assert_eq!(header_date, "Saturday, May 30, 2026", "the folder's date is its oldest photo's");
    let section = fx.d.app().panels().unwrap().sections.get(Section::Folder(folder)).unwrap().subtitle.clone();
    assert!(section.starts_with("May 30, 2026"), "the section's date: {section}");

    // The tooltip after the pointer rests on the cell.
    fx.d.hover(&format!("thumb:{}", last.get()), 0).unwrap();
    for _ in 0..40 {
        fx.d.step();
    }
    let (photo, tip) = fx.d.app().panels().unwrap().light_table.cells.tooltip().expect("the tooltip");
    assert_eq!(photo, last);
    assert!(tip.detail.starts_with("May 30, 2026 8:15 AM"), "the tooltip: {}", tip.detail);

    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(order(&fx.d, folder), ids);
    let cells = &fx.d.app().panels().unwrap().light_table.cells;
    assert_eq!(cells.get(last).and_then(|s| s.caption.clone()).as_deref(), Some("June 7, 2026"));
    assert!(fx.d.state().status_line.contains("June 7, 2026 10:00 AM"), "{}", fx.d.state().status_line);
}

/// Every widget has an id with the dialog open, and the dialog (three photos, shifted by +1 h 30
/// min) matches its fingerprints at both window sizes.
#[test]
fn the_dialog_matches_its_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let mut fx = fixture((w, h));
        let ids = fx.ids.clone();
        select(&mut fx.d, &ids[..3]);
        fx.d.run("photo.adjustDate", &json!({})).unwrap();
        let top = fx.d.app_mut().state_mut().dialogs.last_mut().unwrap();
        top.fields.insert("shift".into(), json!(5400));
        top.fields.insert("set".into(), json!("2026-06-04T11:30:00"));
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d);
        check_region(&fx.d, "dialog:adjustDate", &format!("dialog-adjust-date-{w}x{h}"));
    }
}
