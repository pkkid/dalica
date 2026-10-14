//! M9's settings (`roadmap.md` M9.3; `organizing.md` §4 and §9, `menus-and-shortcuts.md` §5): the
//! first-time move question, "Remove from albums without asking", "Delete from disk without
//! asking", the days Recently Updated covers and the last Move Folder destination round-trip
//! through `settings.json` with unknown keys kept; a value of the wrong type (or a number out of its
//! range) is its default and is reported by key, never by value, when the library opens.

use std::path::Path;
use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::settings::{FILE, OrganizeSettings, RECENT_DAYS_DEFAULT, RECENT_DAYS_RANGE, Settings};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, Arc::new(SystemClock), &options).unwrap();
    s.wait_idle();
    s
}

fn write_settings(library: &Path, v: &Value) {
    dalica_fs::create_dir_all(library).unwrap();
    dalica_fs::replace(&library.join(FILE), serde_json::to_vec_pretty(v).unwrap().as_slice(), Expect::Absent).unwrap();
}

fn stored(library: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(library.join(FILE)).unwrap()).unwrap()
}

#[test]
fn nothing_stored_means_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let o = s.settings().organize();
    assert_eq!(o, OrganizeSettings::default());
    assert!(o.ask_before_moving, "the first move asks");
    assert!(!o.remove_from_albums_without_asking && !o.delete_without_asking, "both ask");
    assert_eq!((o.recent_days, RECENT_DAYS_DEFAULT), (30, 30));
    assert_eq!(o.last_move_folder, None);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
}

#[test]
fn every_new_setting_round_trips_with_unknown_keys_kept() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    write_settings(&lib, &json!({ "future": [1], "organize": { "futureKey": "kept", "recentDays": 7 }, "save": { "explain": false } }));
    let mut s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().organize().recent_days, 7);
    let folder = dir.path().join("Pictures").join("Trips");
    let o = OrganizeSettings {
        ask_before_moving: false,
        remove_from_albums_without_asking: true,
        delete_without_asking: true,
        recent_days: 90,
        last_move_folder: Some(folder.clone()),
    };
    let ui = dalica_fs::mark_ui_thread();
    s.change_settings(|st| st.set_organize(&o));
    s.flush().unwrap();
    drop(ui);
    drop(s);

    let s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().organize(), o);
    assert!(!s.settings().save().explain, "the other settings untouched");
    let file = stored(&lib);
    assert_eq!(file["future"], json!([1]));
    assert_eq!(
        file["organize"],
        json!({
            "futureKey": "kept",
            "askBeforeMoving": false,
            "removeFromAlbumsWithoutAsking": true,
            "deleteWithoutAsking": true,
            "recentDays": 90,
            "lastMoveFolder": folder,
        }),
        "the documented keys, and the unknown one kept"
    );

    // Back to the defaults: the folder is removed, the rest written as defaults; out-of-range days
    // and a relative folder are never stored.
    let mut st = s.settings().clone();
    st.set_organize(&OrganizeSettings::default());
    assert_eq!(st.organize(), OrganizeSettings::default());
    assert_eq!(st.get("organize").and_then(|v| v.get("lastMoveFolder")), None);
    assert_eq!(st.get("organize").unwrap()["futureKey"], json!("kept"));
    for days in [0, RECENT_DAYS_RANGE.end() + 1, u32::MAX] {
        st.set_organize(&OrganizeSettings { recent_days: days, last_move_folder: Some("relative".into()), ..OrganizeSettings::default() });
        let back = Settings::parse(&st.print()).unwrap();
        assert_eq!(back.organize().recent_days, RECENT_DAYS_DEFAULT, "{days}");
        assert_eq!(back.organize().last_move_folder, None);
        assert!(back.problems().is_empty(), "{:?}", back.problems());
    }
    for days in [*RECENT_DAYS_RANGE.start(), *RECENT_DAYS_RANGE.end()] {
        st.set_organize(&OrganizeSettings { recent_days: days, ..OrganizeSettings::default() });
        assert_eq!(Settings::parse(&st.print()).unwrap().organize().recent_days, days);
    }
}

#[test]
fn wrong_types_read_as_defaults_and_are_reported_by_key_never_by_value() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    write_settings(
        &lib,
        &json!({
            "organize": {
                "askBeforeMoving": "no",
                "removeFromAlbumsWithoutAsking": 1,
                "deleteWithoutAsking": null,
                "recentDays": "secret-days",
                "lastMoveFolder": "relative/secret-folder",
            },
        }),
    );
    let s = open(&lib);
    assert_eq!(s.settings().organize(), OrganizeSettings::default());
    let notes = s.notices().join("\n");
    for key in [
        "organize.askBeforeMoving",
        "organize.removeFromAlbumsWithoutAsking",
        "organize.deleteWithoutAsking",
        "organize.recentDays",
        "organize.lastMoveFolder",
    ] {
        assert!(notes.contains(key), "{key} in {notes}");
    }
    assert!(!notes.contains("secret"), "values never reach a notice: {notes}");
    assert_eq!(s.notices().len(), 5, "each once: {notes}");
    drop(s);

    // Numbers out of range, and an object that is not one.
    for (bad, key) in [
        (json!({ "organize": { "recentDays": 0 } }), "organize.recentDays"),
        (json!({ "organize": { "recentDays": 100_000 } }), "organize.recentDays"),
        (json!({ "organize": { "recentDays": 2.5 } }), "organize.recentDays"),
        (json!({ "organize": { "recentDays": -3 } }), "organize.recentDays"),
        (json!({ "organize": [true] }), "\"organize\" is not an object"),
    ] {
        let st = Settings::parse(&serde_json::to_vec(&bad).unwrap()).unwrap();
        assert_eq!(st.organize(), OrganizeSettings::default(), "{bad}");
        let problems = st.problems();
        assert_eq!(problems.len(), 1, "{bad}: {problems:?}");
        assert!(problems[0].contains(key), "{bad}: {problems:?}");
    }
}
