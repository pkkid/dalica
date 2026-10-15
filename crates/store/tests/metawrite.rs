//! The metadata write queue (`data-model.md` §5, `xmp-interop.md` §1, §5, §7) on temp folders:
//! fields reach the files with everything else kept and the time put back; batching and the
//! debounce; `xmp-interop.md` §8 test 8 (first-scan import, the sidecar wins, a change made
//! elsewhere imported, both changed reported, the rebuild fallback) and test 9 (a fault at every
//! step leaves the old or the new file, never a torn one, and the time restored); offline roots
//! written on return; read-only files, formats that cannot be written and RAW `.xmp` sidecars; a
//! file changed between read and write read again.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all, points, run_at};
use dalica_fs::times::SET_TIMES;
use dalica_library::{Folder, Library, Op, Photo, Synced};
use dalica_meta::{Changes, WriteOptions};
use dalica_scenes::{Exif, Scene};
use dalica_store::metawrite::{
    DEBOUNCE, Field, FieldOutcome, FileResult, MAX_TRIES, MetaWriteQueue, Options, Request, Status, Values, WAIT_RETRY, write_file,
};
use dalica_store::writer::RootCheck;
use dalica_store::{Outcome, mapping, write_folder};

/// 2001-02-03 04:05:06 UTC: the time every test photo was "taken off the card".
fn old_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(981_173_106)
}

fn at(secs: u64) -> Timestamp {
    Timestamp::from_millis(1_791_000_000_000).saturating_add(Duration::from_secs(secs))
}

fn modified(p: &Path) -> SystemTime {
    fs::symlink_metadata(p).unwrap().modified().unwrap()
}

fn date(s: &str) -> DateTime {
    s.parse().unwrap()
}

/// A JPEG from a scene, with an EXIF date, written to `dir/name` with [`old_time`].
fn jpeg(dir: &Path, name: &str, seed: u64) -> PathBuf {
    let exif = Exif { date_time_original: Some(date("2026-06-04T19:42:10")), make: Some("Dalica".into()), ..Exif::default() };
    let bytes = dalica_scenes::jpeg(&Scene::new(seed, 64, 48), 85, Some(&exif)).unwrap();
    put(&dir.join(name), &bytes)
}

/// Writes `bytes` to `p` as another program would, with [`old_time`].
fn put(p: &Path, bytes: &[u8]) -> PathBuf {
    fs::write(p, bytes).unwrap();
    fs::File::options().write(true).open(p).unwrap().set_modified(old_time()).unwrap();
    p.to_path_buf()
}

/// Another application edits `p`'s metadata (and so its time).
fn other_app(p: &Path, changes: Changes) {
    let bytes = fs::read(p).unwrap();
    let options = WriteOptions { stars_as_ratings: true, ..WriteOptions::default() };
    let new = dalica_meta::write_owned(&bytes, &changes, &options).unwrap();
    fs::write(p, new).unwrap();
}

fn caption_change(c: &str) -> Changes {
    Changes { caption: Some(Some(c.to_owned())), ..Changes::default() }
}

/// What a file says, in the library's terms.
fn file_values(p: &Path) -> dalica_meta::OwnedFields {
    dalica_meta::read(&fs::read(p).unwrap(), &dalica_meta::Limits::default()).unwrap().owned
}

fn values(caption: &str, tags: &[&str], star: bool) -> Values {
    Values {
        caption: caption.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        star,
        place: None,
        date: Some(date("2026-06-04T19:42:10")),
    }
}

fn request(photo: u64, path: &Path, fields: &[Field], values: Values) -> Request {
    Request::new(PhotoId::new(photo), path.to_path_buf(), fields.iter().copied(), values)
}

/// Every option on, RAW sidecars and star ratings included.
fn all_on() -> Options {
    Options { stars_as_ratings: true, raw_xmp_sidecars: true, ..Options::default() }
}

fn outcomes(r: &FileResult) -> Vec<(Field, FieldOutcome)> {
    r.fields.clone()
}

fn temps(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().filter(|e| dalica_fs::atomic::is_temp_name(&e.as_ref().unwrap().file_name())).count()
}

#[test]
fn caption_tags_star_and_date_reach_the_file_with_everything_else_kept_and_the_time_put_back() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG_0001.jpg", 1);
    let before = fs::read(&p).unwrap();
    let ours = Values { date: Some(date("2026-06-05T08:00:00")), ..values("Pier at dawn, ünïcödé", &["sea", "trip/2026", "a, b"], true) };
    let r = write_file(&request(1, &p, &Field::ALL, ours.clone()), &all_on(), at(0));
    assert_eq!(r.status, Status::Written(p.clone()), "{r:?}");
    assert_eq!(
        outcomes(&r),
        [
            (Field::Caption, FieldOutcome::Written),
            (Field::Tags, FieldOutcome::Written),
            (Field::Rating, FieldOutcome::Written),
            (Field::Place, FieldOutcome::Unchanged),
            (Field::Date, FieldOutcome::Written),
        ]
    );
    let o = file_values(&p);
    assert_eq!(o.caption.as_deref(), Some("Pier at dawn, ünïcödé"));
    assert_eq!(o.tags, ["sea", "trip/2026", "a, b"]);
    assert_eq!(o.rating, Some(5));
    assert_eq!(o.date_taken.unwrap().date_time(), date("2026-06-05T08:00:00"));
    assert_eq!(modified(&p), old_time(), "the modification time is put back");
    // The synced hashes are those of what the file now says, which is Dalica's.
    assert_eq!(r.synced, Some(ours.hashes()));
    // The pixels are untouched: the image decodes to the same pixels.
    let decode = |b: &[u8]| dalica_image::decode(b, &dalica_image::Limits::default()).unwrap();
    assert_eq!(decode(&fs::read(&p).unwrap()), decode(&before));
    // Again: nothing to write, not a byte changed.
    let after = fs::read(&p).unwrap();
    let again = write_file(&Request { synced: r.synced, ..request(1, &p, &Field::ALL, ours) }, &all_on(), at(5));
    assert_eq!(again.status, Status::Unchanged);
    assert!(again.fields.iter().all(|(_, o)| *o == FieldOutcome::Unchanged), "{again:?}");
    assert_eq!(fs::read(&p).unwrap(), after);
    assert_eq!(temps(dir.path()), 0);
}

#[test]
fn a_png_is_written_too_and_the_option_can_keep_the_new_time() {
    let dir = tempfile::tempdir().unwrap();
    let png = put(&dir.path().join("a.png"), &dalica_scenes::png(&Scene::new(2, 32, 24)).unwrap());
    let r = write_file(&request(1, &png, &[Field::Caption, Field::Tags], values("A PNG", &["x"], false)), &Options::default(), at(0));
    assert_eq!(r.status, Status::Written(png.clone()));
    assert_eq!(file_values(&png).caption.as_deref(), Some("A PNG"));
    assert_eq!(modified(&png), old_time());
    let jpg = jpeg(dir.path(), "b.jpg", 3);
    let keep_new = Options { update_modification_time: true, ..Options::default() };
    let r = write_file(&request(2, &jpg, &[Field::Caption], values("New time", &[], false)), &keep_new, at(0));
    assert_eq!(r.status, Status::Written(jpg.clone()));
    assert_ne!(modified(&jpg), old_time(), "with the option on the file keeps the time of the write");
}

#[test]
fn each_field_follows_its_option() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 4);
    let before = fs::read(&p).unwrap();
    let off = Options { captions_tags_places: false, dates: false, stars_as_ratings: false, ..Options::default() };
    let ours = Values { date: Some(date("2020-01-01T00:00:00")), ..values("Caption", &["tag"], true) };
    let r = write_file(&request(1, &p, &Field::ALL, ours), &off, at(0));
    assert_eq!(r.status, Status::Unchanged);
    assert_eq!(fs::read(&p).unwrap(), before, "with every option off nothing is written");
    for f in [Field::Caption, Field::Tags, Field::Rating, Field::Date] {
        assert_eq!(r.field(f), Some(&FieldOutcome::Skipped("its write option is off".into())), "{f:?}");
    }
    // Stars off by default: the rest is written, the star is not.
    let r = write_file(&request(1, &p, &[Field::Caption, Field::Rating], values("Caption", &[], true)), &Options::default(), at(0));
    assert_eq!(
        (r.field(Field::Caption), r.field(Field::Rating)),
        (Some(&FieldOutcome::Written), Some(&FieldOutcome::Skipped("its write option is off".into())))
    );
    assert_eq!(file_values(&p).rating, None);
}

#[test]
fn requests_are_batched_per_file_and_debounced() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 5);
    let q2 = jpeg(dir.path(), "IMG2.jpg", 6);
    let mut q = MetaWriteQueue::new();
    q.enqueue(request(1, &p, &[Field::Caption], values("Typing…", &[], false)), at(0));
    q.enqueue(request(2, &q2, &[Field::Tags], values("", &["other"], false)), at(0));
    // Half a second later the caption is done and a tag added.
    let half = at(0).saturating_add(Duration::from_millis(500));
    q.enqueue(request(1, &p, &[Field::Tags], values("Typing done", &["beach"], false)), half);
    assert_eq!(q.pending(), 2);
    assert_eq!(q.due(at(1)), [PhotoId::new(2)], "photo 1 was asked for again: quiet only from 0.5 s");
    let first = q.write_due(&Options::default(), at(1));
    assert_eq!(first.len(), 1);
    assert_eq!(file_values(&q2).tags, ["other"]);
    let done = q.write_due(&Options::default(), half.saturating_add(DEBOUNCE));
    assert_eq!(done.len(), 1, "one write for both fields");
    assert_eq!(outcomes(&done[0]), [(Field::Caption, FieldOutcome::Written), (Field::Tags, FieldOutcome::Written)]);
    let o = file_values(&p);
    assert_eq!((o.caption.as_deref(), o.tags), (Some("Typing done"), vec!["beach".to_owned()]), "the latest values");
    assert_eq!(q.pending(), 0);
    assert!(q.write(PhotoId::new(1), &Options::default(), at(9)).is_none());
    // Flush writes at once; remove forgets.
    q.enqueue(request(1, &p, &[Field::Caption], values("Flushed", &["beach"], false)), at(20));
    q.enqueue(request(2, &q2, &[Field::Caption], values("Gone", &[], false)), at(20));
    q.remove(PhotoId::new(2));
    let flushed = q.flush(&Options::default(), at(20));
    assert_eq!(flushed.iter().map(|r| r.photo).collect::<Vec<_>>(), [PhotoId::new(1)]);
    assert_eq!(file_values(&p).caption.as_deref(), Some("Flushed"));
}

// xmp-interop.md §8 test 8: conflicts with the folder sidecar (§5).

#[test]
fn first_scan_imports_what_the_file_has_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 7);
    other_app(&p, Changes { tags: Some(vec!["from".into(), "elsewhere".into()]), star: Some(true), ..caption_change("Their caption") });
    let before = fs::read(&p).unwrap();
    let ours = Values { date: Some(date("2026-06-04T19:42:10")), ..Values::default() };
    let r = write_file(&Request { first_scan: true, ..request(1, &p, &[], ours) }, &all_on(), at(0));
    assert_eq!(r.status, Status::Unchanged);
    assert_eq!(fs::read(&p).unwrap(), before);
    assert_eq!(r.imports().collect::<Vec<_>>(), [Field::Caption, Field::Tags, Field::Rating]);
    let file = r.file.unwrap();
    assert_eq!((file.caption.as_str(), file.tags.len(), file.star), ("Their caption", 2, true));
    assert_eq!(r.synced, Some(file.hashes()));
}

#[test]
fn the_sidecar_wins_and_after_a_rebuild_plain_rule_2_applies() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 8);
    other_app(&p, Changes { tags: Some(vec!["theirs".into()]), ..caption_change("Their caption") });
    // No last-synced value (a rebuilt index): the caption asked for is written over theirs; their
    // tags, which Dalica did not change, are neither written nor imported.
    let r = write_file(&request(1, &p, &[Field::Caption], values("Dalica's caption", &[], false)), &Options::default(), at(0));
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Written)]);
    let o = file_values(&p);
    assert_eq!((o.caption.as_deref(), o.tags), (Some("Dalica's caption"), vec!["theirs".to_owned()]));
}

#[test]
fn a_change_made_elsewhere_is_imported_and_not_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 9);
    let ours = values("Ours", &["one"], false);
    let r = write_file(&request(1, &p, &[Field::Caption, Field::Tags], ours.clone()), &Options::default(), at(0));
    let synced = r.synced;
    // Another application changes the caption; Dalica then changes only the tags.
    other_app(&p, caption_change("Changed elsewhere"));
    let ours = values("Ours", &["one", "two"], false);
    let r = write_file(&Request { synced, ..request(1, &p, &[Field::Tags], ours) }, &Options::default(), at(10));
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Imported), (Field::Tags, FieldOutcome::Written)]);
    assert_eq!(r.file.as_ref().map(|f| f.caption.as_str()), Some("Changed elsewhere"));
    let o = file_values(&p);
    assert_eq!(o.caption.as_deref(), Some("Changed elsewhere"), "their caption is not overwritten");
    assert_eq!(o.tags, ["one", "two"]);
    // Even when the caption is asked for again with Dalica's unchanged value, theirs is imported.
    let r = write_file(&Request { synced, ..request(1, &p, &[Field::Caption], values("Ours", &["one", "two"], false)) }, &Options::default(), at(20));
    assert_eq!(r.field(Field::Caption), Some(&FieldOutcome::Imported));
    assert_eq!(file_values(&p).caption.as_deref(), Some("Changed elsewhere"));
}

#[test]
fn both_changed_is_reported_and_dalica_s_value_kept() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 10);
    let r = write_file(&request(1, &p, &[Field::Caption], values("Base", &[], false)), &Options::default(), at(0));
    let synced = r.synced;
    other_app(&p, caption_change("Theirs"));
    let r = write_file(&Request { synced, ..request(1, &p, &[Field::Caption], values("Mine", &[], false)) }, &Options::default(), at(10));
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Conflict { written: true })]);
    assert_eq!(r.file.unwrap().caption, "Theirs", "the other side, for Properties");
    assert_eq!(file_values(&p).caption.as_deref(), Some("Mine"));
    // With the option off it is still reported, and nothing is written.
    other_app(&p, caption_change("Theirs again"));
    let off = Options { captions_tags_places: false, ..Options::default() };
    let r = write_file(&Request { synced: r.synced, ..request(1, &p, &[Field::Caption], values("Mine 2", &[], false)) }, &off, at(20));
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Conflict { written: false })]);
    assert_eq!(file_values(&p).caption.as_deref(), Some("Theirs again"));
}

/// The queue remembers the synced hashes it produced: an undo queued before the engine applied
/// them (the request still carries the old ones) is written, not mistaken for an import.
#[test]
fn an_undo_queued_before_the_model_caught_up_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 11);
    let mut q = MetaWriteQueue::new();
    let a = values("A", &[], false);
    q.enqueue(request(1, &p, &[Field::Caption], a.clone()), at(0));
    let r = q.flush(&Options::default(), at(0)).remove(0);
    let synced_a = r.synced;
    q.enqueue(Request { synced: synced_a, ..request(1, &p, &[Field::Caption], values("B", &[], false)) }, at(2));
    q.flush(&Options::default(), at(2));
    assert_eq!(file_values(&p).caption.as_deref(), Some("B"));
    // Undo, with the model's synced hashes still those of "A".
    q.enqueue(Request { synced: synced_a, ..request(1, &p, &[Field::Caption], a) }, at(3));
    let r = q.flush(&Options::default(), at(3)).remove(0);
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Written)]);
    assert_eq!(file_values(&p).caption.as_deref(), Some("A"));
    // A scan's synced hashes (note_synced) replace what the queue remembers.
    other_app(&p, caption_change("Scanned"));
    let scanned = Values { caption: "Scanned".into(), ..values("", &[], false) };
    q.note_synced(PhotoId::new(1), Some(scanned.hashes()));
    q.enqueue(request(1, &p, &[Field::Tags], Values { tags: vec!["t".into()], ..scanned }), at(9));
    let r = q.flush(&Options::default(), at(9)).remove(0);
    assert_eq!(outcomes(&r), [(Field::Tags, FieldOutcome::Written)], "no conflict: the scan's hashes count");
}

// xmp-interop.md §8 test 9: atomicity.

/// An error or a crash at a step of a metadata write (one of each kind; `fs` tries every point of
/// the atomic replace and of putting the time back): the photo holds exactly the old or the new
/// bytes, never a torn file, and the old file keeps its time; the next pass of the queue finishes
/// the write with the time restored.
#[test]
fn a_fault_at_every_step_leaves_the_old_or_the_new_file_and_the_time_restored() {
    let ours = values("Atomic", &["fault"], false);
    let fields = [Field::Caption, Field::Tags];
    let points: Vec<&'static str> = ATOMIC_POINTS.to_vec();
    let mut cases = 0;
    for (point, fault) in crate::common::FAULTS {
        {
            let dir = tempfile::tempdir().unwrap();
            let p = jpeg(dir.path(), "IMG.jpg", 12);
            let old = fs::read(&p).unwrap();
            let mut q = MetaWriteQueue::new();
            q.enqueue(request(1, &p, &fields, ours.clone()), at(0));
            arm(point, fault);
            let first = q.write(PhotoId::new(1), &Options::default(), at(1)).unwrap();
            disarm_all();
            let case = format!("{point} {fault:?}: {:?}", first.status);
            let now = fs::read(&p).unwrap();
            let new_written = now != old;
            let rename = points.iter().position(|q| *q == points::RENAME).unwrap();
            let after_rename = points.iter().position(|q| *q == point).unwrap() > rename;
            assert_eq!(new_written, after_rename, "{case}: the rename is the moment the new file appears");
            if new_written {
                assert_eq!(file_values(&p).caption.as_deref(), Some("Atomic"), "{case}: the new file, whole");
            } else {
                assert_eq!(modified(&p), old_time(), "{case}: the old file keeps its time");
            }
            assert!(dalica_meta::read(&now, &dalica_meta::Limits::default()).is_ok(), "{case}: never torn");
            match fault {
                Fault::Error => assert_eq!(temps(dir.path()), 0, "{case}: an error cleans up"),
                Fault::Crash => assert!(temps(dir.path()) <= 1, "{case}"),
            }
            // The next pass finishes it.
            if q.is_pending(PhotoId::new(1)) {
                let second = q.write(PhotoId::new(1), &Options::default(), at(3)).unwrap();
                assert!(matches!(second.status, Status::Written(_) | Status::Unchanged), "{case}: {second:?}");
            }
            assert_eq!(q.pending(), 0, "{case}");
            assert_eq!(file_values(&p).caption.as_deref(), Some("Atomic"), "{case}");
            assert_eq!(modified(&p), old_time(), "{case}: the time is restored");
            cases += 1;
        }
    }
    assert_eq!(cases, crate::common::FAULTS.len());
}

/// M7.25: a crash (the process killed) during a metadata write leaves the old file with its old
/// time or the new file with the old time, right then, before any retry: the time is set on the
/// temporary file before the rename, not put back after it. A crash setting the time on the
/// temporary file, at the rename and after it (`fs` crashes every point).
#[test]
fn a_crash_at_every_point_leaves_the_old_or_the_new_file_with_the_old_time() {
    use dalica_fs::atomic::KEEP_TIMES;
    let ours = values("Crash", &["kill"], true);
    let fields = [Field::Caption, Field::Tags, Field::Rating];
    let crash_points = [KEEP_TIMES, points::RENAME, points::VERIFY];
    let mut new_files = 0;
    for &point in &crash_points {
        let dir = tempfile::tempdir().unwrap();
        let p = jpeg(dir.path(), "IMG.jpg", 21);
        let old = fs::read(&p).unwrap();
        let mut q = MetaWriteQueue::new();
        q.enqueue(request(1, &p, &fields, ours.clone()), at(0));
        arm(point, Fault::Crash);
        let first = q.write(PhotoId::new(1), &Options::default(), at(1)).unwrap();
        disarm_all();
        let case = format!("crash at {point}: {:?}", first.status);
        let now = fs::read(&p).unwrap();
        if now == old {
            assert!(q.is_pending(PhotoId::new(1)), "{case}: tried again");
        } else {
            assert_eq!(file_values(&p).caption.as_deref(), Some("Crash"), "{case}: the new file, whole");
            new_files += 1;
        }
        assert_eq!(modified(&p), old_time(), "{case}: old or new, the file has its old time");
        // The next pass finishes it, still with the old time.
        if q.is_pending(PhotoId::new(1)) {
            q.write(PhotoId::new(1), &Options::default(), at(3)).unwrap();
        }
        assert_eq!(q.pending(), 0, "{case}");
        assert_eq!(file_values(&p).caption.as_deref(), Some("Crash"), "{case}");
        assert_eq!(modified(&p), old_time(), "{case}");
    }
    // After the rename: the read-back.
    assert_eq!(new_files, 1);
}

/// Where the platform does not keep the time on the temporary file (an injected error at
/// [`KEEP_TIMES`](dalica_fs::atomic::KEEP_TIMES) stands for it), the time is put back after the
/// rename; if that fails, the queue remembers the time and the next pass puts it back.
#[test]
fn a_platform_that_does_not_keep_the_time_gets_it_put_back_after_the_rename() {
    use dalica_fs::atomic::KEEP_TIMES;
    for crash_after in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let p = jpeg(dir.path(), "IMG.jpg", 22);
        let mut q = MetaWriteQueue::new();
        q.enqueue(request(1, &p, &[Field::Caption], values("Fallback", &[], false)), at(0));
        arm(KEEP_TIMES, Fault::Error);
        if crash_after {
            arm(SET_TIMES, Fault::Crash);
        }
        let first = q.write(PhotoId::new(1), &Options::default(), at(1)).unwrap();
        disarm_all();
        let case = format!("crash after the rename: {crash_after}: {:?}", first.status);
        assert_eq!(file_values(&p).caption.as_deref(), Some("Fallback"), "{case}");
        if crash_after {
            assert!(matches!(first.status, Status::Requeued(_)), "{case}");
            assert_ne!(modified(&p), old_time(), "{case}: the fallback's own crash window");
            q.write(PhotoId::new(1), &Options::default(), at(3)).unwrap();
        } else {
            assert_eq!(first.status, Status::Written(p.clone()), "{case}");
        }
        assert_eq!(q.pending(), 0, "{case}");
        assert_eq!(modified(&p), old_time(), "{case}");
    }
}

#[test]
fn a_disk_error_that_persists_fails_after_a_few_tries() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 13);
    let old = fs::read(&p).unwrap();
    let mut q = MetaWriteQueue::new();
    q.enqueue(request(1, &p, &[Field::Caption], values("Never", &[], false)), at(0));
    let mut last = None;
    for i in 0..MAX_TRIES {
        arm(points::FSYNC, Fault::Error);
        last = q.write(PhotoId::new(1), &Options::default(), at(u64::from(i) * 2));
        disarm_all();
    }
    let last = last.unwrap();
    assert!(matches!(&last.status, Status::Failed(why) if why.contains("tried 3 times")), "{last:?}");
    assert_eq!(q.pending(), 0);
    assert_eq!(fs::read(&p).unwrap(), old);
}

// Offline volumes (M5.13, M6.43).

#[test]
fn a_file_under_an_offline_root_waits_and_is_written_on_return() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Card");
    let folder = root.join("DCIM");
    fs::create_dir_all(&folder).unwrap();
    let p = jpeg(&folder, "IMG.jpg", 14);
    let mut q = MetaWriteQueue::new();
    // Held: the engine found the root offline.
    q.hold(vec![root.clone()]);
    q.enqueue(request(1, &p, &[Field::Caption], values("Offline", &[], false)), at(0));
    let r = q.write_due(&Options::default(), at(1)).remove(0);
    assert!(matches!(&r.status, Status::Waiting(why) if why.contains("offline")), "{r:?}");
    assert_eq!(q.waiting().collect::<Vec<_>>(), [PhotoId::new(1)]);
    assert!(q.write_due(&Options::default(), at(2)).is_empty(), "not looked at again before the wait is over");
    assert_eq!(file_values(&p).caption, None);
    // The root returns.
    q.hold(Vec::new());
    let r = q.write_due(&Options::default(), at(3)).remove(0);
    assert_eq!(r.status, Status::Written(p.clone()));
    assert_eq!(file_values(&p).caption.as_deref(), Some("Offline"));

    // Unplugged between two checks: the folder is not there.
    fs::rename(&root, dir.path().join("Card-away")).unwrap();
    q.enqueue(request(1, &p, &[Field::Caption], values("Unplugged", &[], false)), at(10));
    let r = q.write_due(&Options::default(), at(12)).remove(0);
    assert!(matches!(&r.status, Status::Waiting(why) if why.contains("not there")), "{r:?}");
    assert!(!folder.exists(), "no folder is created");
    fs::rename(dir.path().join("Card-away"), &root).unwrap();
    let r = q.write_due(&Options::default(), at(12).saturating_add(WAIT_RETRY)).remove(0);
    assert_eq!(r.status, Status::Written(p.clone()));
    assert_eq!(file_values(&p).caption.as_deref(), Some("Unplugged"));

    // A guarded root failing its check right before the write.
    let ok = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ok);
    let check: RootCheck = Arc::new(move |_: &Path| flag.load(Ordering::SeqCst));
    q.guard(vec![(root.clone(), check)]);
    q.enqueue(request(1, &p, &[Field::Caption], values("Guarded", &[], false)), at(30));
    let r = q.flush(&Options::default(), at(31)).remove(0);
    assert!(matches!(&r.status, Status::Waiting(why) if why.contains("not the volume")), "{r:?}");
    ok.store(true, Ordering::SeqCst);
    let r = q.flush(&Options::default(), at(32)).remove(0);
    assert_eq!(r.status, Status::Written(p.clone()));
    assert_eq!(file_values(&p).caption.as_deref(), Some("Guarded"));
    // A check that panics fails.
    let panics: RootCheck = Arc::new(|_: &Path| panic!("probe failed"));
    q.guard(vec![(root, panics)]);
    q.enqueue(request(1, &p, &[Field::Caption], values("Panicked", &[], false)), at(40));
    assert!(matches!(q.flush(&Options::default(), at(41)).remove(0).status, Status::Waiting(_)));
}

// Files that cannot be written.

/// A library with one folder holding `name`, captioned `caption`, its sidecar written.
fn library_with_caption(folder: &Path, name: &str, caption: &str) -> (Library, FolderId) {
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: folder.to_path_buf() } }).unwrap();
    let id = lib.next_photo_id();
    let photo = Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 1, height: 1, date: None, star: false };
    lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
    lib.apply(Op::SetCaptions { captions: vec![(id, caption.into())] }).unwrap();
    let state = mapping::folder_state(&lib, f).unwrap();
    assert_eq!(write_folder(&state, at(0)), Outcome::Written);
    (lib, f)
}

fn set_read_only(p: &Path, read_only: bool) {
    let mut perms = fs::metadata(p).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(if read_only { 0o444 } else { 0o644 });
    }
    #[cfg(not(unix))]
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    fs::set_permissions(p, perms).unwrap();
}

#[test]
fn a_read_only_file_fails_with_its_reason_and_the_sidecar_keeps_the_value() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 15);
    library_with_caption(dir.path(), "IMG.jpg", "Kept in the sidecar");
    let sidecar = fs::read(dir.path().join(".dalica.json")).unwrap();
    set_read_only(&p, true);
    let before = fs::read(&p).unwrap();
    let mut q = MetaWriteQueue::new();
    q.enqueue(request(1, &p, &[Field::Caption], values("Kept in the sidecar", &[], false)), at(0));
    let r = q.flush(&Options::default(), at(1)).remove(0);
    set_read_only(&p, false);
    assert!(matches!(&r.status, Status::Failed(why) if why.contains("read-only")), "{r:?}");
    assert!(matches!(r.field(Field::Caption), Some(FieldOutcome::Failed(why)) if why.contains("read-only")));
    assert_eq!(q.pending(), 0, "not retried: the toast says why");
    assert_eq!(fs::read(&p).unwrap(), before);
    assert_eq!(modified(&p), old_time());
    assert_eq!(fs::read(dir.path().join(".dalica.json")).unwrap(), sidecar, "the sidecar is intact");
    assert!(String::from_utf8(sidecar).unwrap().contains("Kept in the sidecar"));
    assert!(r.synced.is_some(), "the file was read: its hashes still count");
}

#[test]
fn formats_that_cannot_be_written_damaged_files_and_missing_ones() {
    let dir = tempfile::tempdir().unwrap();
    // A GIF: skipped with the reason.
    let gif = put(&dir.path().join("anim.gif"), b"GIF89a\x01\x00\x01\x00\x00\x00\x00;");
    let r = write_file(&request(1, &gif, &[Field::Caption], values("No", &[], false)), &Options::default(), at(0));
    assert!(matches!(&r.status, Status::Skipped(why) if why.contains("GIF")), "{r:?}");
    assert!(matches!(r.field(Field::Caption), Some(FieldOutcome::Skipped(_))));
    assert_eq!(fs::read(&gif).unwrap(), b"GIF89a\x01\x00\x01\x00\x00\x00\x00;");
    // Something no format: skipped.
    let txt = put(&dir.path().join("notes.jpg"), b"just text, named like a photo");
    assert!(matches!(
        write_file(&request(2, &txt, &[Field::Caption], values("No", &[], false)), &Options::default(), at(0)).status,
        Status::Skipped(_)
    ));
    // A JPEG cut short: its metadata cannot be read, so it is not rewritten.
    let good = fs::read(jpeg(dir.path(), "whole.jpg", 16)).unwrap();
    let cut = put(&dir.path().join("cut.jpg"), &good[..40]);
    let r = write_file(&request(3, &cut, &[Field::Caption], values("No", &[], false)), &Options::default(), at(0));
    assert!(matches!(&r.status, Status::Failed(why) if why.contains("cannot be read")), "{r:?}");
    assert_eq!(fs::read(&cut).unwrap(), &good[..40]);
    // Gone from its folder.
    let r = write_file(&request(4, &dir.path().join("deleted.jpg"), &[Field::Caption], values("No", &[], false)), &Options::default(), at(0));
    assert_eq!(r.status, Status::Gone);
    // Over the size limit.
    let small = Options { max_file: 100, ..Options::default() };
    let big = jpeg(dir.path(), "big.jpg", 17);
    assert!(matches!(write_file(&request(5, &big, &[Field::Caption], values("No", &[], false)), &small, at(0)).status, Status::Failed(_)));
    // On the UI thread nothing touches the disk.
    let _ui = dalica_fs::mark_ui_thread();
    let r = write_file(&request(6, &big, &[Field::Caption], values("No", &[], false)), &Options::default(), at(0));
    drop(_ui);
    assert!(matches!(&r.status, Status::Failed(why) if why.contains("UI thread")), "{r:?}");
    #[cfg(unix)]
    {
        let link = dir.path().join("link.jpg");
        std::os::unix::fs::symlink(&big, &link).unwrap();
        let r = write_file(&request(7, &link, &[Field::Caption], values("No", &[], false)), &Options::default(), at(0));
        assert!(matches!(&r.status, Status::Failed(why) if why.contains("link")), "{r:?}");
    }
}

#[test]
fn a_file_changed_between_read_and_write_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 18);
    let mut q = MetaWriteQueue::new();
    let ours = values("", &["mine"], false);
    let r = write_file(&request(1, &p, &[Field::Tags], values("", &[], false)), &Options::default(), at(0));
    q.enqueue(Request { synced: r.synced, ..request(1, &p, &[Field::Tags], ours) }, at(0));
    // Another application writes its caption right as the queue is about to replace the file.
    let other = p.clone();
    run_at(points::CREATE_TEMP, move || other_app(&other, caption_change("Theirs, mid-write")));
    let r = q.write(PhotoId::new(1), &Options::default(), at(1)).unwrap();
    disarm_all();
    assert!(matches!(&r.status, Status::Requeued(why) if why.contains("changed")), "{r:?}");
    assert!(q.is_pending(PhotoId::new(1)));
    let o = file_values(&p);
    assert_eq!((o.caption.as_deref(), o.tags.len()), (Some("Theirs, mid-write"), 0), "their write stands");
    // The next pass reads the file again: their caption is imported, the tags written.
    let r = q.write_due(&Options::default(), at(3)).remove(0);
    assert_eq!(outcomes(&r), [(Field::Caption, FieldOutcome::Imported), (Field::Tags, FieldOutcome::Written)]);
    let o = file_values(&p);
    assert_eq!((o.caption.as_deref(), o.tags), (Some("Theirs, mid-write"), vec!["mine".to_owned()]));
}

// RAW files and their `.xmp` sidecars (xmp-interop.md §7).

const CR3: &[u8] = b"\0\0\0\x18ftypcrx \0\0\0\x01 and the rest of a Canon RAW file";

fn names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
}

#[test]
fn a_raw_gets_its_xmp_sidecar_only_with_the_option() {
    let dir = tempfile::tempdir().unwrap();
    let raw = put(&dir.path().join("IMG_0001.CR3"), CR3);
    let jpg = jpeg(dir.path(), "IMG_0001.JPG", 19);
    let ours = Values { place: None, date: Some(date("2026-06-04T19:42:10")), ..values("A RAW", &["raw"], true) };
    let fields = [Field::Caption, Field::Tags, Field::Rating, Field::Date];
    // The option off: nothing, with the reason.
    let r = write_file(&request(1, &raw, &fields, ours.clone()), &Options { raw_xmp_sidecars: false, ..all_on() }, at(0));
    assert!(matches!(&r.status, Status::Skipped(why) if why.contains("RAW")), "{r:?}");
    assert_eq!(names(dir.path()), BTreeSet::from(["IMG_0001.CR3".to_owned(), "IMG_0001.JPG".to_owned()]));
    // On: the stem sidecar is the RAW's; the RAW and the JPEG are untouched.
    let r = write_file(&request(1, &raw, &fields, ours.clone()), &all_on(), at(0));
    let xmp = dir.path().join("IMG_0001.xmp");
    assert_eq!(r.status, Status::Written(xmp.clone()), "{r:?}");
    assert_eq!(fs::read(&raw).unwrap(), CR3);
    assert_eq!(file_values(&jpg).caption, None);
    let read = dalica_meta::xmp_sidecar::read(Some(CR3), Some(&fs::read(&xmp).unwrap()), &dalica_meta::Limits::default());
    let o = read.metadata.owned;
    assert_eq!((o.caption.as_deref(), o.tags.as_slice(), o.rating), (Some("A RAW"), &["raw".to_owned()][..], Some(5)));
    assert_eq!(o.date_taken.map(|d| d.date_time()), Some(date("2026-06-04T19:42:10")), "the new sidecar has the date too");
    assert_eq!(r.synced, Some(ours.hashes()));
    // Again: nothing to write.
    let again = write_file(&Request { synced: r.synced, ..request(1, &raw, &fields, ours) }, &all_on(), at(5));
    assert_eq!(again.status, Status::Unchanged);
    // A second RAW sharing the stem gets its own `<file>.xmp`.
    let nef = put(&dir.path().join("IMG_0001.NEF"), CR3);
    let r = write_file(
        &Request { raws: vec!["IMG_0001.CR3".into()], ..request(2, &nef, &[Field::Caption], values("The NEF", &[], false)) },
        &all_on(),
        at(6),
    );
    assert_eq!(r.status, Status::Written(dir.path().join("IMG_0001.NEF.xmp")), "{r:?}");
}

#[test]
fn a_damaged_raw_sidecar_is_kept_aside_and_a_read_only_one_fails() {
    let dir = tempfile::tempdir().unwrap();
    let raw = put(&dir.path().join("IMG_0002.CR3"), CR3);
    let xmp = put(&dir.path().join("IMG_0002.xmp"), b"<x:xmpmeta>not closed");
    let r = write_file(&request(1, &raw, &[Field::Caption], values("Fresh", &[], false)), &all_on(), at(0));
    assert_eq!(r.status, Status::Written(xmp.clone()), "{r:?}");
    let aside = r.set_aside.clone().unwrap();
    assert!(aside.file_name().unwrap().to_str().unwrap().starts_with("IMG_0002.xmp.corrupt-"), "{aside:?}");
    assert_eq!(fs::read(&aside).unwrap(), b"<x:xmpmeta>not closed", "kept, byte for byte");
    let read = dalica_meta::xmp_sidecar::read(Some(CR3), Some(&fs::read(&xmp).unwrap()), &dalica_meta::Limits::default());
    assert_eq!(read.metadata.owned.caption.as_deref(), Some("Fresh"));
    // A read-only sidecar fails, untouched.
    set_read_only(&xmp, true);
    let before = fs::read(&xmp).unwrap();
    let r = write_file(&request(1, &raw, &[Field::Caption], values("Refused", &[], false)), &all_on(), at(1));
    set_read_only(&xmp, false);
    assert!(matches!(&r.status, Status::Failed(why) if why.contains("read-only")), "{r:?}");
    assert_eq!(fs::read(&xmp).unwrap(), before);
}

/// A sidecar's synced hashes start empty after a rebuild; this keeps the queue's memory of them
/// out of a request that has its own.
#[test]
fn queue_bookkeeping() {
    let dir = tempfile::tempdir().unwrap();
    let p = jpeg(dir.path(), "IMG.jpg", 20);
    let mut q = MetaWriteQueue::new();
    assert_eq!(format!("{q:?}"), "MetaWriteQueue { pending: 0, held: [], guarded: [], .. }");
    q.enqueue(request(1, &p, &[Field::Caption], values("x", &[], false)), at(0));
    assert!(q.is_pending(PhotoId::new(1)) && q.waiting().next().is_none() && q.held().is_empty());
    let r = q.flush(&Options::default(), at(0)).remove(0);
    // The model catching up (a request with the same hashes) lets the queue forget its own.
    q.enqueue(Request { synced: r.synced, ..request(1, &p, &[Field::Caption], values("x", &[], false)) }, at(1));
    q.note_synced(PhotoId::new(1), None);
    assert_eq!(q.flush(&Options::default(), at(1)).remove(0).status, Status::Unchanged);
    let _ = Synced::default();
}
