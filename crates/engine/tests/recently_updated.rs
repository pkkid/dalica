//! Recently Updated counts changes (`organizing.md` §4.1): a fresh scan leaves it empty (being
//! indexed is no change); a caption, a star, a rotation and a file rewritten on disk each put the
//! photo first, on a virtual clock, and it leaves after the setting's days; every command that
//! changes a photo's data is stamped in `Session::execute`, and undoing the change puts the earlier
//! time back (Adjust Date's own undo op included). The test's file changes run on a helper thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use crate::common::{jpeg, off_owner, options};
use dalica_core::{Clock, PhotoId, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_engine::watching::WatchMode;
use serde_json::json;

const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);

/// Writes `bytes` at `path` with modification time `at`, on a helper thread.
fn put(path: &Path, bytes: Vec<u8>, at: Timestamp) {
    let path = path.to_path_buf();
    let time = UNIX_EPOCH + Duration::from_millis(u64::try_from(at.as_millis()).unwrap());
    off_owner(move || {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
    });
}

struct Fx {
    _tmp: tempfile::TempDir,
    lisbon: PathBuf,
    clock: Arc<VirtualClock>,
    s: Session,
    /// The photos in name order: `IMG_0001.jpg` …
    ids: Vec<PhotoId>,
}

/// Four photos whose files are a year older than the clock, scanned.
fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let lisbon = tmp.path().join("Pictures").join("Lisbon");
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
    for i in 0..4u64 {
        put(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), jpeg(i), clock.now().saturating_sub(DAY * 365));
    }
    let mut s = Session::open_with(&tmp.path().join("library"), clock.clone(), &options()).unwrap();
    s.set_watching(WatchMode::Off);
    s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    Fx { _tmp: tmp, lisbon, clock, s, ids: ids.into_iter().map(|(_, id)| id).collect() }
}

fn recent(s: &Session) -> Vec<PhotoId> {
    s.library().recently_updated(s.clock().now(), s.settings().organize().recent_days)
}

#[test]
fn a_fresh_scan_leaves_it_empty_and_each_change_puts_its_photo_first_until_the_days_pass() {
    let mut fx = fx();
    let ids = fx.ids.clone();
    assert!(recent(&fx.s).is_empty(), "being indexed is no change");
    let row = |s: &mut Session| {
        let v = s.execute("albums.list", &json!({})).unwrap();
        v.as_array().unwrap().iter().find(|r| r["kind"] == "recent").map(|r| r["count"].clone())
    };
    assert_eq!(row(&mut fx.s), Some(json!(0)));

    fx.clock.advance(HOUR);
    fx.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    assert_eq!(recent(&fx.s), [ids[0]], "a caption");
    fx.clock.advance(HOUR);
    fx.s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    assert_eq!(recent(&fx.s), [ids[1], ids[0]], "a star");
    fx.clock.advance(HOUR);
    fx.s.execute("photo.rotate.cw", &json!({ "ids": [ids[2]] })).unwrap();
    assert_eq!(recent(&fx.s), [ids[2], ids[1], ids[0]], "a rotation");
    assert_eq!(fx.s.library().last_changed(ids[2]), Some(fx.clock.now()), "stamped with the session's clock");

    // A file rewritten on disk after it was indexed, found by the verification pass.
    fx.clock.advance(HOUR);
    put(&fx.lisbon.join("IMG_0004.jpg"), jpeg(40), fx.clock.now());
    fx.s.start_verification();
    fx.s.wait_idle();
    assert_eq!(recent(&fx.s), [ids[3], ids[2], ids[1], ids[0]], "a file changed on disk");
    assert_eq!(fx.s.library().last_changed(ids[3]), None, "counted by the file's own time");
    assert_eq!(row(&mut fx.s), Some(json!(4)));

    // The setting's days, counted from each change.
    let days = fx.s.settings().organize().recent_days;
    fx.clock.set(Timestamp::from_millis(1_790_000_000_000).saturating_add(DAY * days).saturating_add(HOUR * 3 / 2));
    assert_eq!(recent(&fx.s), [ids[3], ids[2], ids[1]], "the caption left after {days} days");
    fx.clock.advance(HOUR * 3);
    assert!(recent(&fx.s).is_empty(), "every change left");
}

#[test]
fn undo_puts_the_earlier_time_back_and_redo_the_change_time() {
    let mut fx = fx();
    let ids = fx.ids.clone();
    fx.clock.advance(HOUR);
    let first = fx.clock.now();
    fx.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "one" })).unwrap();
    fx.clock.advance(HOUR);
    fx.s.execute("photo.tags.add", &json!({ "ids": [ids[1]], "tags": "tram" })).unwrap();
    fx.clock.advance(HOUR);
    let third = fx.clock.now();
    fx.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "two" })).unwrap();
    assert_eq!(recent(&fx.s), [ids[0], ids[1]]);

    // Undo is no change of its own: the caption's earlier time comes back, later on the clock.
    fx.clock.advance(HOUR);
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(fx.s.library().last_changed(ids[0]), Some(first));
    assert_eq!(recent(&fx.s), [ids[1], ids[0]]);
    fx.s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(fx.s.library().last_changed(ids[0]), Some(third), "redo brings the change's own time");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    fx.s.execute("edit.undo", &json!({})).unwrap();
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(recent(&fx.s).is_empty(), "every change undone: none was ever changed");

    // Adjust Date undoes with an op of its own; the times come back with it.
    fx.clock.advance(HOUR);
    fx.s.execute("photo.dates.set", &json!({ "ids": [ids[2]], "set": "2020-01-02T03:04:05" })).unwrap();
    assert_eq!(recent(&fx.s), [ids[2]], "a date");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(recent(&fx.s).is_empty());
}

/// The last-changed times are in the sidecars, so a rebuild keeps the list, in its order; a file
/// changed on disk counts by its time only against a first-indexed time, which a rebuild does not
/// record, so it leaves.
#[test]
fn a_rebuild_keeps_the_list() {
    let mut fx = fx();
    let ids = fx.ids.clone();
    fx.clock.advance(HOUR);
    fx.s.execute("photo.rotate.cw", &json!({ "ids": [ids[2]] })).unwrap();
    fx.clock.advance(HOUR);
    fx.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    fx.clock.advance(HOUR);
    fx.s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    fx.clock.advance(HOUR);
    put(&fx.lisbon.join("IMG_0004.jpg"), jpeg(40), fx.clock.now());
    fx.s.start_verification();
    fx.s.wait_idle();
    assert_eq!(recent(&fx.s), [ids[3], ids[1], ids[0], ids[2]]);
    let times: Vec<_> = ids.iter().map(|id| fx.s.library().last_changed(*id)).collect();
    fx.s.flush().unwrap();
    let entry = crate::common::entry(&fx.lisbon, "IMG_0002.jpg").unwrap();
    assert_eq!(entry["changed"], json!(dalica_sidecar::utc_text(times[1].unwrap()).unwrap()), "in the photo's entry: {entry}");

    fx.s.execute("library.rebuild", &json!({})).unwrap();
    fx.s.wait_idle();
    assert_eq!(fx.s.library().first_indexed_all().count(), 0, "a rebuild records no first-indexed time");
    let back: Vec<PhotoId> = ["IMG_0002.jpg", "IMG_0001.jpg", "IMG_0003.jpg"].iter().map(|n| photo_named(&fx.s, n)).collect();
    assert_eq!(recent(&fx.s), back, "Dalica's changes come back in their order");
    let rebuilt: Vec<_> =
        ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0004.jpg"].iter().map(|n| fx.s.library().last_changed(photo_named(&fx.s, n))).collect();
    assert_eq!(rebuilt, times, "every time exactly");
}

fn photo_named(s: &Session, name: &str) -> PhotoId {
    s.library().photos().find(|p| p.name == name).unwrap().id
}

#[test]
fn commands_that_change_no_photo_stamp_nothing() {
    let mut fx = fx();
    let ids = fx.ids.clone();
    fx.clock.advance(HOUR);
    let lisbon = fx.s.library().photo(ids[0]).unwrap().folder;
    fx.s.execute("folder.info.set", &json!({ "id": lisbon, "place": "Lisbon" })).unwrap();
    fx.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "" })).unwrap();
    assert!(recent(&fx.s).is_empty(), "folder info, and a caption that changed nothing");
    assert_eq!(fx.s.library().last_changed_all().count(), 0);
}
