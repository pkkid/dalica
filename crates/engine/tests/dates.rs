//! `photo.dates.set` on a temporary library (`roadmap.md` M6.16; `organizing.md` §9 Adjust Date
//! and Time, §10 sorting; `data-model.md` §3): shifting three photos by the same amount across
//! midnight and a month end, from their effective dates and from the file time for a photo
//! without one; setting one date on photos with and without dates; one undo step, exact undo and
//! redo, none when nothing changes; the folder's photos re-sorted; the sidecar after the command and
//! after undo; dates surviving a restart and a rebuild of the index; the owner thread making no
//! `fs` call; and the command contract's failure cases.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_core::{DateTime, PhotoId};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

/// The capture dates of the photos `setup` writes; `IMG_0004.jpg` has none.
const TAKEN: [Option<&str>; 4] = [Some("2026-01-31T23:30:00"), Some("2026-01-31T22:00:00"), Some("2026-02-28T23:59:59"), None];

/// A library watching `Lisbon` with `IMG_0001.jpg` … `IMG_0004.jpg` (dates as in [`TAKEN`]); ids in
/// name order.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for (i, taken) in TAKEN.iter().enumerate() {
        let exif = dalica_scenes::Exif { date_time_original: taken.map(dt), ..dalica_scenes::Exif::default() };
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, Some(&exif)).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = by_name(&s).into_values().collect();
    (lib, disk, folder, s, ids)
}

fn by_name(s: &Session) -> BTreeMap<String, PhotoId> {
    s.library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

/// Each photo's effective date, by name.
fn effective(s: &Session) -> BTreeMap<String, Option<DateTime>> {
    s.library().photos().map(|p| (p.name.clone(), s.library().effective_date(p.id))).collect()
}

/// The dates the user set, by name (ids change in a rebuild).
fn user_dates(s: &Session) -> BTreeMap<String, DateTime> {
    s.library().photos().filter_map(|p| Some((p.name.clone(), s.library().info(p.id).date?))).collect()
}

/// The folder's photos in display order, by name.
fn order(s: &Session) -> Vec<String> {
    let folder = s.library().folders().next().unwrap().id;
    s.library().photos_in(folder).iter().map(|p| p.name.clone()).collect()
}

/// The `"date"` of every entry of the folder's sidecar, after every pending write landed.
fn sidecar_dates(s: &mut Session, folder: &Path) -> BTreeMap<String, String> {
    s.flush().unwrap();
    let v: Value = std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap());
    v["photos"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, e)| Some((k.clone(), e.get("date")?.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn shifting_three_photos_crosses_midnight_and_a_month_end_in_one_step_and_undo_redo_are_exact() {
    let (_l, _d, folder, mut s, ids) = setup();
    assert_eq!(order(&s), names(&["IMG_0002.jpg", "IMG_0001.jpg", "IMG_0003.jpg", "IMG_0004.jpg"]));
    let (m0, e0) = (s.library().model().clone(), effective(&s));
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let v = s.execute("photo.dates.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "shift": 3600 })).unwrap();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!(v["changed"], json!(3));
    assert_eq!(
        v["dates"],
        json!([{ "id": ids[0], "date": "2026-02-01T00:30:00" }, { "id": ids[1], "date": "2026-01-31T23:00:00" }, { "id": ids[2], "date": "2026-03-01T00:59:59" }])
    );
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Adjust Date"));
    let e = effective(&s);
    assert_eq!(e["IMG_0001.jpg"], Some(dt("2026-02-01T00:30:00")), "across midnight into the next month");
    assert_eq!(e["IMG_0003.jpg"], Some(dt("2026-03-01T00:59:59")), "across the end of February");
    // The file's own dates stay beside the user's.
    assert_eq!(s.library().photo(ids[0]).unwrap().date, Some(dt("2026-01-31T23:30:00")));
    let written = sidecar_dates(&mut s, &folder);
    assert_eq!(
        written,
        BTreeMap::from([
            ("IMG_0001.jpg".into(), "2026-02-01T00:30:00".into()),
            ("IMG_0002.jpg".into(), "2026-01-31T23:00:00".into()),
            ("IMG_0003.jpg".into(), "2026-03-01T00:59:59".into()),
        ])
    );
    let m1 = s.library().model().clone();

    // Shifting by nothing changes nothing and records no step.
    assert_eq!(s.execute("photo.dates.set", &json!({ "ids": [ids[0]], "shift": 0 })).unwrap()["changed"], json!(0));
    assert_eq!(s.undo_stack().len(), 1);

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m0);
    assert!(sidecar_dates(&mut s, &folder).is_empty(), "after undo no date is left in the sidecar");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    assert_eq!(sidecar_dates(&mut s, &folder), written);

    // Shifting back lands on the files' own dates: the user's dates go.
    let v = s.execute("photo.dates.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "shift": -3600 })).unwrap();
    assert_eq!(v["changed"], json!(3));
    assert!(user_dates(&s).is_empty());
    assert!(sidecar_dates(&mut s, &folder).is_empty());
    assert_eq!(effective(&s), e0);
}

#[test]
fn a_shift_moves_a_photo_without_a_date_from_its_file_time_and_the_folder_re_sorts() {
    let (_l, _d, _f, mut s, ids) = setup();
    let file_time = DateTime::from_unix_secs(s.library().photo(ids[3]).unwrap().mtime.as_secs()).unwrap();
    assert_eq!(s.library().effective_date(ids[3]), None);
    // Back ten years: the photo without a date now sorts first.
    let ten_years = -10 * 365 * 86_400;
    let v = s.execute("photo.dates.set", &json!({ "ids": [ids[3]], "shift": ten_years })).unwrap();
    assert_eq!(v["changed"], json!(1));
    let date: DateTime = v["dates"][0]["date"].as_str().unwrap().parse().unwrap();
    assert_eq!(s.library().effective_date(ids[3]), Some(date));
    assert_eq!(Some(date), DateTime::from_unix_secs(s.library().photo(ids[3]).unwrap().mtime.as_secs() + ten_years));
    assert!(date < file_time);
    assert_eq!(order(&s), names(&["IMG_0004.jpg", "IMG_0002.jpg", "IMG_0001.jpg", "IMG_0003.jpg"]));
    // IMG_0002 moves past the others by two months.
    s.execute("photo.dates.set", &json!({ "ids": [ids[1]], "shift": 61 * 86_400 })).unwrap();
    assert_eq!(order(&s), names(&["IMG_0004.jpg", "IMG_0001.jpg", "IMG_0003.jpg", "IMG_0002.jpg"]));
    // The folder's date is its oldest photo's effective date.
    let folder = s.library().folders().next().unwrap().id;
    assert_eq!(s.library().folder_date(folder), Some(date));
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(order(&s), names(&["IMG_0002.jpg", "IMG_0001.jpg", "IMG_0003.jpg", "IMG_0004.jpg"]));
}

#[test]
fn setting_one_date_on_photos_with_and_without_dates() {
    let (_l, _d, folder, mut s, ids) = setup();
    let v = s.execute("photo.dates.set", &json!({ "ids": [ids[3], ids[0], ids[2]], "set": "2025-12-24T18:00:00" })).unwrap();
    assert_eq!(v["changed"], json!(3));
    assert_eq!(s.undo_stack().len(), 1);
    for id in [ids[3], ids[0], ids[2]] {
        assert_eq!(s.library().effective_date(id), Some(dt("2025-12-24T18:00:00")));
    }
    // Equal dates sort by name.
    assert_eq!(order(&s), names(&["IMG_0001.jpg", "IMG_0003.jpg", "IMG_0004.jpg", "IMG_0002.jpg"]));
    assert_eq!(sidecar_dates(&mut s, &folder).len(), 3);
    // Setting a photo to its file's own date clears the user's date; setting the same again
    // changes nothing.
    let v = s.execute("photo.dates.set", &json!({ "ids": [ids[0]], "set": "2026-01-31T23:30:00" })).unwrap();
    assert_eq!(v["changed"], json!(1));
    assert_eq!(s.library().info(ids[0]).date, None);
    assert_eq!(s.execute("photo.dates.set", &json!({ "ids": [ids[0]], "set": "2026-01-31T23:30:00" })).unwrap()["changed"], json!(0));
    assert_eq!(s.undo_stack().len(), 2);
    // A space instead of the T is read too (the ISO forms `dalica-core` accepts).
    s.execute("photo.dates.set", &json!({ "ids": [ids[1]], "set": "2024-02-29 12:00:00" })).unwrap();
    assert_eq!(s.library().effective_date(ids[1]), Some(dt("2024-02-29T12:00:00")));
}

#[test]
fn dates_survive_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, folder, mut s, ids) = setup();
    s.execute("photo.dates.set", &json!({ "ids": [ids[0], ids[3]], "shift": 90 * 60 })).unwrap();
    s.execute("photo.dates.set", &json!({ "ids": [ids[2]], "set": "2020-05-05T10:00:00" })).unwrap();
    let want = user_dates(&s);
    assert_eq!(want.len(), 3);
    let written = sidecar_dates(&mut s, &folder);
    assert_eq!(written.len(), 3);
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(user_dates(&s), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(by_name(&s).len(), 4);
    assert_eq!(user_dates(&s), want, "after a rebuild of the index");
    // A later write to the folder keeps them in the sidecar.
    let other = by_name(&s)["IMG_0002.jpg"];
    s.execute("photo.star.set", &json!({ "ids": [other], "on": true })).unwrap();
    assert_eq!(sidecar_dates(&mut s, &folder), written);
    assert_eq!(order(&s), names(&["IMG_0003.jpg", "IMG_0002.jpg", "IMG_0001.jpg", "IMG_0004.jpg"]));
}

#[test]
fn disabled_bad_and_out_of_range_dates_change_nothing() {
    let (_l, _d, _f, mut s, ids) = setup();
    let m = s.library().model().clone();
    let spec = *s.spec("photo.dates.set").unwrap();
    assert!((spec.enabled)(&s, &json!({ "ids": [], "shift": 60 })).is_err());
    assert!((spec.enabled)(&s, &json!({ "ids": [ids[0]], "shift": 60 })).is_ok());
    // Dates `cmd::dates`'s tests do not try.
    for bad in [
        json!({ "ids": [ids[0]], "set": "2026-13-01T00:00:00" }),
        json!({ "ids": [ids[0]], "set": "2026-01-01" }),
        json!({ "ids": [ids[0]], "set": "0000:00:00 00:00:00" }),
    ] {
        assert!(matches!(s.execute("photo.dates.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
    // Past year 9999 or before year 1: refused for the whole selection, nothing changes.
    for shift in [i64::MAX, i64::MIN, 400_000_000_000, -100_000_000_000] {
        assert!(matches!(s.execute("photo.dates.set", &json!({ "ids": [ids[1], ids[0]], "shift": shift })), Err(CmdError::Limit(_))), "{shift}");
    }
    let too_many: Vec<u64> = vec![ids[0].get(); 100_001];
    assert!(matches!(s.execute("photo.dates.set", &json!({ "ids": too_many, "shift": 60 })), Err(CmdError::Limit(_))));
    assert_eq!(s.library().model(), &m);
    assert_eq!(s.undo_stack().len(), 0);
}
