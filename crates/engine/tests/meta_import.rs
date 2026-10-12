//! Importing file metadata on scan (`roadmap.md` M6.32; `xmp-interop.md` §5, §7) on temp folders
//! while the session runs: a JPEG tagged by another tool shows its caption, tags, date and star
//! after the first scan with no undo step; a caption (and rating) edited outside, found by the
//! watcher or by the verification pass, comes in as one undoable step tagged as coming from the
//! files; a field edited on both sides keeps Dalica's value and is reported for Properties; a RAW
//! imports from its `.xmp` sidecar; after a rebuild the sidecar wins (plain rule 2). The owner
//! thread is marked as the UI thread: it makes no `fs` call. The test's own file changes run on a
//! helper thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dalica_core::{DateTime, PhotoId, Timestamp, VirtualClock};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, Field, Options, Origin, Session, ThumbOptions};
use dalica_meta::xmp_sidecar::{self, SidecarWrite};
use dalica_meta::{Changes, DateTaken, WriteOptions};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(60);

/// Runs `f` on a helper thread: the test's own file changes never count against the owner.
fn off_owner<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

fn date(s: &str) -> DateTime {
    s.parse().unwrap()
}

/// 2001-02-03 04:05:06 UTC plus `n` seconds: the times the test gives its files, so every change
/// it makes is a new modification time.
fn time(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(981_173_106 + n)
}

/// What another tool writes into a file (stars as ratings: 5, or 0 to unstar).
fn options() -> WriteOptions {
    WriteOptions { stars_as_ratings: true, ..WriteOptions::default() }
}

fn caption(c: &str) -> Changes {
    Changes { caption: Some(Some(c.into())), ..Changes::default() }
}

/// A JPEG another tool tagged: caption, keywords, rating 5 and a date taken.
fn tagged_jpeg(seed: u64) -> Vec<u8> {
    let plain = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 85, None).unwrap();
    let changes = Changes {
        caption: Some(Some("Elétrico 28".into())),
        tags: Some(vec!["lisbon".into(), "tram".into()]),
        star: Some(true),
        date_taken: Some(DateTaken::local(date("2026-06-04T19:42:10"))),
        ..Changes::default()
    };
    dalica_meta::write_owned(&plain, &changes, &options()).unwrap()
}

/// Writes `bytes` at `path` with modification time `time(n)`, on a helper thread.
fn put(path: &Path, bytes: Vec<u8>, n: u64) {
    let path = path.to_path_buf();
    off_owner(move || {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(time(n)).unwrap();
    });
}

/// Another application edits `path`'s metadata; the file gets modification time `time(n)`.
fn other_app(path: &Path, changes: Changes, n: u64) {
    let p = path.to_path_buf();
    let bytes = off_owner(move || std::fs::read(p).unwrap());
    put(path, dalica_meta::write_owned(&bytes, &changes, &options()).unwrap(), n);
}

struct Fx {
    _tmp: tempfile::TempDir,
    library: PathBuf,
    /// The watched root, holding `Lisbon` with `tagged.jpg` (tagged by another tool),
    /// `plain.jpg` (no metadata) and `tagged.png` (a caption).
    root: PathBuf,
    clock: Arc<VirtualClock>,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Pictures");
        let lisbon = root.join("Lisbon");
        put(&lisbon.join("tagged.jpg"), tagged_jpeg(1), 0);
        put(&lisbon.join("plain.jpg"), dalica_scenes::jpeg(&dalica_scenes::Scene::new(2, 64, 48), 85, None).unwrap(), 0);
        let png = dalica_scenes::png(&dalica_scenes::Scene::new(3, 64, 48)).unwrap();
        put(&lisbon.join("tagged.png"), dalica_meta::write_owned(&png, &caption("A PNG caption"), &options()).unwrap(), 0);
        Fx { library: tmp.path().join("library"), root, _tmp: tmp, clock: Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000))) }
    }

    fn lisbon(&self) -> PathBuf {
        self.root.join("Lisbon")
    }

    fn open(&self, mode: WatchMode) -> Session {
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&self.library, self.clock.clone(), &options).unwrap();
        s.set_watching(mode);
        s
    }

    /// A library watching the root, scanned.
    fn scanned(&self, mode: WatchMode) -> Session {
        let mut s = self.open(mode);
        s.execute("folders.add", &json!({ "path": self.root })).unwrap();
        s.wait_idle();
        s
    }

    /// Drives the polling watcher on virtual time: a few one-second ticks, each waited for.
    fn settle(&self, s: &mut Session) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(PATIENCE), "the watcher answered");
            s.wait_idle();
        }
    }

    fn sidecar(&self, folder: &Path) -> Value {
        let path = folder.join(".dalica.json");
        serde_json::from_slice(&off_owner(move || std::fs::read(path).unwrap())).unwrap()
    }
}

fn photo(s: &Session, folder: &Path, name: &str) -> PhotoId {
    let f = s.library().folder_by_path(folder).unwrap();
    s.library().photos_in(f).into_iter().find(|p| p.name == name).unwrap().id
}

/// Caption, tags, star and effective date of photo `id`.
fn shown(s: &Session, id: PhotoId) -> (String, Vec<String>, bool, Option<DateTime>) {
    let l = s.library();
    (l.caption(id).to_string(), l.tags_of(id).to_vec(), l.photo(id).unwrap().star, l.effective_date(id))
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| (*t).to_string()).collect()
}

/// `photo.metadata` once its file is read.
fn metadata(s: &mut Session, id: PhotoId) -> Value {
    let start = Instant::now();
    loop {
        let v = s.execute("photo.metadata", &json!({ "id": id })).unwrap();
        if v["pending"] == json!(false) {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "still pending: {v}");
        assert!(s.wait_photo_metadata(PATIENCE));
    }
}

/// `xmp-interop.md` §5 rule 1: the first scan imports every field the file has (a rating of 5 as
/// a star) into the model and the folder sidecar, with no undo step; the synced hashes are set.
#[test]
fn a_jpeg_tagged_by_another_tool_shows_its_caption_tags_date_and_star_after_the_first_scan() {
    let fx = Fx::new();
    let mut s = fx.open(WatchMode::Off);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.execute("folders.add", &json!({ "path": fx.root })).unwrap();
    s.wait_idle();
    let lisbon = fx.lisbon();
    let (tagged, plain, png) = (photo(&s, &lisbon, "tagged.jpg"), photo(&s, &lisbon, "plain.jpg"), photo(&s, &lisbon, "tagged.png"));
    assert_eq!(shown(&s, tagged), ("Elétrico 28".into(), tags(&["lisbon", "tram"]), true, Some(date("2026-06-04T19:42:10"))));
    assert_eq!(s.library().info(tagged).date, None, "the file's own date shows; it is not a date set by the user");
    assert_eq!(shown(&s, plain), (String::new(), vec![], false, None));
    assert_eq!(s.library().caption(png), "A PNG caption");
    assert!(s.undo_stack().is_empty(), "a first scan's import is no step");
    for id in [tagged, plain, png] {
        assert!(s.library().synced(id).is_some(), "the synced hashes are set from what was read");
    }
    let (changes, _) = s.take_metadata_changes();
    assert!(changes.iter().all(|c| c.origin == Origin::Files), "imports are never written back: {changes:?}");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");

    // Imported into the folder sidecar.
    s.flush().unwrap();
    let sidecar = fx.sidecar(&lisbon);
    let entry = &sidecar["photos"]["tagged.jpg"];
    assert_eq!((&entry["caption"], &entry["tags"], &entry["star"]), (&json!("Elétrico 28"), &json!(["lisbon", "tram"]), &json!(true)));
    assert!(sidecar["photos"].get("plain.jpg").is_none(), "nothing to import, no entry");

    // A second scan of unchanged files reads nothing again and changes nothing.
    let model = s.library().model().clone();
    for job in s.start_verification() {
        let _ = job;
    }
    s.wait_idle();
    assert_eq!(s.library().model(), &model);
}

/// `xmp-interop.md` §5 rule 3 through the watcher: a caption and rating edited in another
/// application while Dalica runs come in as one undoable step ("Import from Files"), tagged as from
/// the files; undo and redo go back and forth as commands.
#[test]
fn a_caption_edited_outside_while_dalica_runs_is_imported_as_one_undoable_step() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Polling);
    let lisbon = fx.lisbon();
    let id = photo(&s, &lisbon, "tagged.jpg");
    s.take_metadata_changes();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    // The watcher is up and has seen the folder.
    fx.settle(&mut s);
    assert!(s.watched_roots().iter().any(|(r, _)| *r == fx.root), "{:?}", s.watched_roots());
    assert!(s.undo_stack().is_empty());

    other_app(&lisbon.join("tagged.jpg"), Changes { star: Some(false), ..caption("Tram 28 at night") }, 10);
    fx.settle(&mut s);
    assert_eq!(shown(&s, id), ("Tram 28 at night".into(), tags(&["lisbon", "tram"]), false, Some(date("2026-06-04T19:42:10"))));
    assert_eq!((s.undo_stack().len(), s.undo_stack().undo_label()), (1, Some("Import from Files")), "one step");
    let (changes, _) = s.take_metadata_changes();
    assert!(changes.iter().any(|c| c.origin == Origin::Files && c.photos.iter().any(|(p, f)| *p == id && f.contains(Field::Star))), "{changes:?}");
    assert!(changes.iter().all(|c| c.origin == Origin::Files), "{changes:?}");

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(shown(&s, id), ("Elétrico 28".into(), tags(&["lisbon", "tram"]), true, Some(date("2026-06-04T19:42:10"))));
    let (changes, _) = s.take_metadata_changes();
    assert!(changes.iter().any(|c| c.origin == Origin::Command), "undoing an import is the user's change: {changes:?}");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().caption(id), "Tram 28 at night");

    // Nothing changed since: settling again imports nothing more.
    fx.settle(&mut s);
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.library().caption(id), "Tram 28 at night");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    s.flush().unwrap();
    assert_eq!(fx.sidecar(&lisbon)["photos"]["tagged.jpg"]["caption"], json!("Tram 28 at night"));
}

/// The same through the verification pass (the watcher off), and both sides changed: Dalica's
/// value is kept and `photo.metadata` reports "Changed in another application"; tags changed
/// only outside still come in.
#[test]
fn a_field_edited_on_both_sides_keeps_dalicas_value_and_is_reported() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Off);
    let lisbon = fx.lisbon();
    let id = photo(&s, &lisbon, "tagged.jpg");
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Ours" })).unwrap();

    other_app(&lisbon.join("tagged.jpg"), Changes { tags: Some(vec!["lisbon".into(), "night".into()]), ..caption("Theirs") }, 10);
    s.start_verification();
    s.wait_idle();
    assert_eq!(s.library().caption(id), "Ours", "Dalica's value kept");
    assert_eq!(s.library().tags_of(id), tags(&["lisbon", "night"]), "changed only outside: imported");
    assert_eq!(s.undo_stack().undo_label(), Some("Import from Files"));
    let v = metadata(&mut s, id);
    assert_eq!(v["changedElsewhere"], json!({ "fields": ["caption"], "note": "Changed in another application; Dalica's value kept" }));
    let plain = photo(&s, &lisbon, "plain.jpg");
    assert_eq!(metadata(&mut s, plain).get("changedElsewhere"), None);

    // Undoing the import leaves the user's caption; the conflict is not imported later either.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((s.library().caption(id), s.library().tags_of(id)), ("Ours", &tags(&["lisbon", "tram"])[..]));
    s.start_verification();
    s.wait_idle();
    assert_eq!(s.library().caption(id), "Ours");
    assert_eq!(s.undo_stack().undo_label(), Some("Set Caption"), "nothing imported again");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
}

/// `xmp-interop.md` §7: a RAW's `.xmp` sidecar is read on scan whatever the write option says; the
/// JPEG of a RAW+JPEG pair keeps its own metadata.
#[test]
fn a_raw_with_an_xmp_sidecar_imports_from_it() {
    let fx = Fx::new();
    let raws = fx.root.join("Raw");
    // A Canon CR3's start: recognised as a RAW, holding nothing `dalica-meta` reads.
    let cr3 = b"\0\0\0\x18ftypcrx \0\0\0\x01 and the rest of a Canon RAW file".to_vec();
    put(&raws.join("IMG_0001.CR3"), cr3.clone(), 0);
    put(&raws.join("IMG_0002.CR3"), cr3, 0);
    let changes = Changes { tags: Some(vec!["raw".into()]), star: Some(true), ..caption("From the sidecar") };
    let Ok(SidecarWrite::Write(xmp)) = xmp_sidecar::merge(None, None, &changes, &options()) else { panic!("a new sidecar") };
    put(&raws.join("IMG_0001.xmp"), xmp, 0);
    put(&raws.join("IMG_0001.JPG"), dalica_meta::write_owned(&tagged_jpeg(4), &caption("The JPEG's own"), &options()).unwrap(), 0);

    let mut s = fx.open(WatchMode::Off);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.execute("folders.add", &json!({ "path": fx.root })).unwrap();
    s.wait_idle();
    let raw = photo(&s, &raws, "IMG_0001.CR3");
    assert_eq!(shown(&s, raw), ("From the sidecar".into(), tags(&["raw"]), true, None));
    assert_eq!(shown(&s, photo(&s, &raws, "IMG_0002.CR3")), (String::new(), vec![], false, None));
    assert_eq!(s.library().caption(photo(&s, &raws, "IMG_0001.JPG")), "The JPEG's own");
    assert!(s.undo_stack().is_empty());
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
}

/// `xmp-interop.md` §5 after an index rebuild: no last-synced value, so the sidecar wins (plain
/// rule 2) and nothing is reported; a photo with no sidecar entry still imports what its file has.
#[test]
fn after_a_rebuild_the_sidecar_wins() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Off);
    let lisbon = fx.lisbon();
    let id = photo(&s, &lisbon, "tagged.jpg");
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Ours" })).unwrap();
    s.flush().unwrap();
    other_app(&lisbon.join("tagged.jpg"), caption("Theirs"), 10);
    other_app(&lisbon.join("plain.jpg"), caption("Added elsewhere"), 10);

    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let (id, plain) = (photo(&s, &lisbon, "tagged.jpg"), photo(&s, &lisbon, "plain.jpg"));
    assert_eq!(s.library().caption(id), "Ours", "the sidecar wins");
    assert_eq!(s.library().caption(plain), "Added elsewhere", "no entry: imported");
    assert!(s.library().synced(id).is_some());
    assert_eq!(metadata(&mut s, id).get("changedElsewhere"), None);
    assert!(s.undo_stack().is_empty());
}
