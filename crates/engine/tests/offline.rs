//! Offline volumes and queued writes (`roadmap.md` M5.13; `data-model.md` §7, `security.md` §5.9):
//! a volume "unplugged" by renaming its root shows offline with its photos, rows and thumbnails
//! intact; a star made meanwhile is journalled, no folder appears at the old path, `pendingWrites`
//! counts it, and once the root is renamed back the sidecar holds the star, also across restarts.
//! The owner thread makes no `fs` call: it is marked as the UI thread, and the online checks
//! record the thread they run on. File changes the test makes itself run on a helper thread.
//!
//! A root that goes between two online checks (M6.43): emptied (an unmounted volume's mount point)
//! or replaced by another volume (its device changed), just before a due write. The write is
//! checked right before it lands: no sidecar, the edit is reported unsaved, and it lands once the
//! files return.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use crate::common::off_owner;
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::offline::{CHECK_EVERY, OnlineCheck, PENDING_FILE, RootProbe, SETTLE};
use dalica_engine::{CacheDir, CmdError, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_library::{Row, Tree, TreeMode, TreeOptions};
use dalica_sidecar::{Field, Limits, Sidecar};
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(60);

struct Fx {
    _tmp: tempfile::TempDir,
    library: PathBuf,
    /// The watched root (a "card") and its one folder of photos (the root itself in [`Fx::flat`]).
    card: PathBuf,
    trip: PathBuf,
    away: PathBuf,
    clock: Arc<VirtualClock>,
    /// The threads the online checks ran on.
    checked_on: Arc<Mutex<Vec<ThreadId>>>,
}

impl Fx {
    fn new() -> Fx {
        Fx::with_photos_in(Some("2026 Trip"))
    }

    /// The photos directly in the root, as on a card or drive with no folders.
    fn flat() -> Fx {
        Fx::with_photos_in(None)
    }

    fn with_photos_in(folder: Option<&str>) -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("CARD");
        let trip = folder.map_or_else(|| card.clone(), |f| card.join(f));
        dalica_fs::create_dir_all(&trip).unwrap();
        for i in 0..3 {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 120, 80), 80, None).unwrap();
            dalica_fs::replace(&trip.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
        }
        Fx {
            library: tmp.path().join("library"),
            away: tmp.path().join("CARD (unplugged)"),
            card,
            trip,
            _tmp: tmp,
            clock: Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000))),
            checked_on: Arc::default(),
        }
    }

    /// Opens the library with a recording online check.
    fn open(&self) -> Session {
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&self.library, self.clock.clone(), &options).unwrap();
        let on = Arc::clone(&self.checked_on);
        let check: OnlineCheck = Arc::new(move |root: &Path| {
            on.lock().unwrap().push(std::thread::current().id());
            dalica_scan::volumes::online(root)
        });
        s.set_online_check(Some(check));
        s
    }

    /// A new library watching the card, scanned, with the card checked online.
    fn scanned(&self) -> (Session, Vec<PhotoId>) {
        self.scanned_with(None)
    }

    /// [`Fx::scanned`] with `probe` planted before the first check.
    fn scanned_with(&self, probe: Option<RootProbe>) -> (Session, Vec<PhotoId>) {
        let mut s = self.open();
        s.set_root_probe(probe);
        s.execute("folders.add", &json!({ "path": self.card })).unwrap();
        s.wait_idle();
        assert!(s.refresh_volumes(PATIENCE));
        assert_eq!(s.root_online(&self.card), Some(true));
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        (s, photos.into_iter().map(|(_, id)| id).collect())
    }

    fn unplug(&self) {
        let (from, to) = (self.card.clone(), self.away.clone());
        off_owner(move || std::fs::rename(from, to).unwrap());
    }

    fn replug(&self) {
        let (from, to) = (self.away.clone(), self.card.clone());
        off_owner(move || std::fs::rename(from, to).unwrap());
    }

    /// Something else at the card's path while the card is away: an empty folder (an unmounted
    /// volume's mount point), or another volume with the same folder and another file in it.
    fn stand_in(&self, other_volume: bool) {
        let (card, trip) = (self.card.clone(), self.trip.clone());
        off_owner(move || {
            std::fs::create_dir(&card).unwrap();
            if other_volume {
                dalica_fs::create_dir_all(&trip).unwrap();
                dalica_fs::replace(&trip.join("OTHER.jpg"), b"not ours", Expect::Absent).unwrap();
            }
        });
    }

    /// Takes the stand-in away and plugs the card back in.
    fn restore(&self) {
        let card = self.card.clone();
        off_owner(move || std::fs::remove_dir_all(card).unwrap());
        self.replug();
    }

    /// Advances the clock to the next scheduled check and waits for it.
    fn check(&self, s: &mut Session) {
        self.clock.advance(CHECK_EVERY);
        assert!(s.refresh_volumes(PATIENCE));
    }

    fn star_in_sidecar(&self, name: &str) -> Field<bool> {
        let path = self.trip.join(".dalica.json");
        let bytes = off_owner(move || std::fs::read(path).unwrap());
        Sidecar::parse(&bytes, &Limits::default()).unwrap().star(name)
    }

    fn exists(&self, path: &Path) -> bool {
        let path = path.to_path_buf();
        off_owner(move || path.exists())
    }
}

fn tree(s: &Session) -> Tree {
    let options = TreeOptions { mode: TreeMode::Tree, offline_roots: s.offline_roots(), ..TreeOptions::default() };
    Tree::build(s.library(), s.roots(), &options)
}

fn offline_rows(s: &Session) -> Vec<(String, usize, bool)> {
    let options = TreeOptions { offline_roots: s.offline_roots(), ..TreeOptions::default() };
    tree(s)
        .rows()
        .filter_map(|(_, r)| match r {
            Row::Folder(f) => Some((f.name.clone(), f.count, options.is_offline(&f.path))),
            Row::Year(_) => None,
        })
        .collect()
}

#[test]
fn an_unplugged_root_shows_offline_and_a_star_made_meanwhile_lands_when_it_returns() {
    let fx = Fx::new();
    let (mut s, ids) = fx.scanned();
    let folder = s.library().folder_by_path(&fx.trip).unwrap();
    assert!(!matches!(s.thumbnail(ids[0], 128), Thumb::NotScanned));
    assert_eq!(offline_rows(&s), [("CARD".to_string(), 0, false), ("2026 Trip".to_string(), 3, false)]);
    assert_eq!(s.pending_writes(), None);

    let ui = dalica_fs::mark_ui_thread();
    let owner = std::thread::current().id();

    // Unplugged: the next check finds the root offline; photos, rows and thumbnails stay, even
    // after a rescan of the root.
    fx.unplug();
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(false));
    assert_eq!(s.offline_roots(), BTreeSet::from([fx.card.clone()]));
    assert!(s.is_folder_offline(folder));
    s.execute("folders.add", &json!({ "path": fx.card })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 3, "the photos stay");
    assert_eq!(offline_rows(&s), [("CARD".to_string(), 0, true), ("2026 Trip".to_string(), 3, true)]);
    assert!(!matches!(s.thumbnail(ids[0], 128), Thumb::NotScanned), "the cached thumbnail stays");

    // A star meanwhile: applied at once, its sidecar write waits and is counted.
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    assert!(s.library().photo(ids[0]).unwrap().star);
    s.poll();
    let pending = s.pending_writes().unwrap();
    assert_eq!((pending.sidecars, pending.metadata, pending.index), (1, 0, 0));
    assert!(pending.error.as_deref().is_some_and(|e| e.contains("CARD")), "{pending:?}");
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("not there")), "the write is reported unsaved");
    assert!(!fx.exists(&fx.card) && !fx.exists(&fx.trip), "no folder is created at the old path");
    fx.clock.advance(CHECK_EVERY * 3);
    s.poll();
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1), "still waiting");
    assert!(!fx.exists(&fx.trip));

    // Back: the next check releases the write; the sidecar holds the star.
    fx.replug();
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(true));
    assert!(s.offline_roots().is_empty() && !s.is_folder_offline(folder));
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0001.jpg"), Field::Value(true));
    assert_eq!(fx.star_in_sidecar("IMG_0002.jpg"), Field::Absent);
    // Counted until a check made a while later still finds the root there.
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    fx.clock.advance(SETTLE);
    assert!(s.refresh_volumes(PATIENCE));
    assert_eq!(s.pending_writes(), None);
    assert_eq!(offline_rows(&s), [("CARD".to_string(), 0, false), ("2026 Trip".to_string(), 3, false)]);

    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    let on = fx.checked_on.lock().unwrap();
    assert!(on.len() >= 4 && on.iter().all(|t| *t != owner), "online checks run off the owner thread");
}

/// Nothing is lost if the app quits while a root is offline: the journal replays the star, the
/// waiting folder is remembered, and it is written once the root is found online, whether it comes
/// back while the app is closed or after it opened again.
#[test]
fn a_write_still_waiting_at_quit_is_made_after_a_restart() {
    let fx = Fx::new();
    let (mut s, ids) = fx.scanned();
    fx.unplug();
    fx.check(&mut s);
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    s.poll();
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    assert!(s.flush().is_err());
    drop(s);
    let list = std::fs::read_to_string(fx.library.join(PENDING_FILE)).unwrap();
    assert!(list.contains("2026 Trip"), "{list}");

    // Reopened while still unplugged: the journal has the star, and the write still waits.
    let mut s = fx.open();
    assert!(s.library().photo(ids[1]).unwrap().star, "the journal replays the star");
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(false));
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    drop(s);

    // Plugged in while closed: the next session writes it.
    fx.replug();
    let mut s = fx.open();
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(true));
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0002.jpg"), Field::Value(true));
    fx.clock.advance(SETTLE);
    assert!(s.refresh_volumes(PATIENCE));
    assert_eq!(s.pending_writes(), None);
    s.flush().unwrap();
    let list = std::fs::read_to_string(fx.library.join(PENDING_FILE)).unwrap();
    assert!(!list.contains("2026 Trip"), "the list is emptied once written: {list}");
}

/// An edit made just before the volume went away, before its write could land, is still counted
/// as waiting and written on return.
#[test]
fn an_edit_just_before_the_unplug_waits_too() {
    let fx = Fx::new();
    let (mut s, ids) = fx.scanned();
    // The writer has not had a chance: the root goes away in the same instant.
    fx.unplug();
    s.execute("photo.star.set", &json!({ "ids": [ids[2]], "on": true })).unwrap();
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(false));
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    assert!(s.flush().is_err(), "unsaved while offline");
    fx.replug();
    fx.check(&mut s);
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0003.jpg"), Field::Value(true));
}

/// Checks are sent by `poll` on the session clock and after a scan, one at a time, and a check
/// that hangs (a sleeping network share) never holds the owner: `poll` keeps answering at once.
#[test]
fn checks_are_scheduled_and_a_hung_check_never_blocks_poll() {
    let fx = Fx::new();
    let (mut s, _ids) = fx.scanned();
    let gate = Arc::new((Mutex::new((0usize, false)), Condvar::new()));
    let g = Arc::clone(&gate);
    let check: OnlineCheck = Arc::new(move |root: &Path| {
        let (lock, cv) = &*g;
        let mut st = lock.lock().unwrap();
        st.0 += 1;
        cv.notify_all();
        let _ = cv.wait_timeout_while(st, PATIENCE, |st| !st.1).unwrap();
        dalica_scan::volumes::online(root)
    });
    s.set_online_check(Some(check));
    let calls = |gate: &Arc<(Mutex<(usize, bool)>, Condvar)>| gate.0.lock().unwrap().0;

    // Not due yet: nothing is sent.
    s.poll();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(calls(&gate), 0);
    // Due: one check, which hangs; polls go on at once and send no second one.
    fx.clock.advance(CHECK_EVERY);
    s.poll();
    let start = Instant::now();
    while calls(&gate) == 0 {
        assert!(start.elapsed() < PATIENCE);
        std::thread::yield_now();
    }
    fx.clock.advance(CHECK_EVERY * 5);
    let start = Instant::now();
    for _ in 0..100 {
        s.poll();
    }
    assert!(start.elapsed() < Duration::from_secs(2), "poll never waits for a check");
    assert_eq!(calls(&gate), 1, "one check at a time");
    assert_eq!(s.root_online(&fx.card), Some(true), "the last known state stands");
    {
        let (lock, cv) = &*gate;
        lock.lock().unwrap().1 = true;
        cv.notify_all();
    }
    assert!(s.refresh_volumes(PATIENCE));
    let after_open = calls(&gate);

    // A scan ending asks for a check, without waiting for the schedule (polled each frame, as the
    // UI does; a scan that starts and ends between two polls is caught by the schedule, or by its
    // counts: so it finds one photo more than the last one did, and is seen to end however fast).
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(9, 120, 80), 80, None).unwrap();
    let new = fx.card.join("NEW_0001.jpg");
    off_owner(move || std::fs::write(new, bytes).unwrap());
    s.execute("folders.add", &json!({ "path": fx.card })).unwrap();
    let start = Instant::now();
    while s.busy() {
        assert!(start.elapsed() < PATIENCE);
        s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let start = Instant::now();
    while calls(&gate) == after_open {
        assert!(start.elapsed() < PATIENCE, "no check after the scan");
        std::thread::sleep(Duration::from_millis(2));
        s.poll();
    }
}

/// A check that panics counts as offline (the writes wait; nothing is lost) and the session goes on.
#[test]
fn a_panicking_check_counts_as_offline() {
    let fx = Fx::new();
    let (mut s, ids) = fx.scanned();
    s.set_online_check(Some(Arc::new(|_: &Path| panic!("deliberate"))));
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(false));
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.poll();
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    s.set_online_check(None);
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(true));
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0001.jpg"), Field::Value(true));
}

/// With every root online, edits never count as waiting.
#[test]
fn online_edits_are_not_pending() {
    let fx = Fx::new();
    let (mut s, ids) = fx.scanned();
    for id in &ids {
        s.execute("photo.star.toggle", &json!({ "ids": [id] })).unwrap();
        s.poll();
    }
    fx.check(&mut s);
    assert_eq!(s.pending_writes(), None);
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0003.jpg"), Field::Value(true));
}

/// The volume is unmounted between two online checks, leaving its empty mount point, and a star is
/// made before the next check: the write is checked right before it lands, so no sidecar appears
/// in the mount point and the edit is reported unsaved. The next check counts it as waiting, and
/// it lands when the volume is back. The owner thread makes no `fs` call.
#[test]
fn a_root_emptied_just_before_a_due_write_gets_no_sidecar() {
    let fx = Fx::flat();
    let (mut s, ids) = fx.scanned();
    let sidecar = fx.card.join(".dalica.json");
    assert!(s.library().folder_by_path(&fx.card).is_some(), "the photos are in the root itself");
    let ui = dalica_fs::mark_ui_thread();

    fx.unplug();
    fx.stand_in(false);
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.poll();
    assert_eq!(s.root_online(&fx.card), Some(true), "no online check has seen it go yet");
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("not the volume")), "the edit is reported unsaved");
    assert!(!fx.exists(&sidecar), "no sidecar lands in the empty mount point");

    // The next check finds it offline and counts the write; the writer still writes nothing.
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(false));
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));
    assert!(s.flush().is_err());
    assert!(!fx.exists(&sidecar));

    // Back: the write lands.
    fx.restore();
    fx.check(&mut s);
    assert_eq!(s.root_online(&fx.card), Some(true));
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0001.jpg"), Field::Value(true));
    assert_eq!(fx.star_in_sidecar("IMG_0002.jpg"), Field::Absent);

    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
}

/// Another volume mounted in the card's place between two checks (another device, the same folder,
/// none of the library's files): a star made then gets no sidecar there and is reported unsaved.
/// When the card returns, before any online check saw it go and with a new device number but its
/// files, the write lands.
#[test]
fn a_root_whose_device_changed_just_before_a_due_write_gets_no_sidecar() {
    let fx = Fx::new();
    let device = Arc::new(AtomicU64::new(1));
    let d = Arc::clone(&device);
    let probe: RootProbe = Arc::new(move |root: &Path, samples: &[PathBuf]| dalica_scan::volumes::RootState {
        device: Some(d.load(Ordering::SeqCst)),
        ..dalica_scan::volumes::root_state(root, samples)
    });
    let (mut s, ids) = fx.scanned_with(Some(probe));
    let sidecar = fx.trip.join(".dalica.json");
    let ui = dalica_fs::mark_ui_thread();

    fx.unplug();
    fx.stand_in(true);
    device.store(2, Ordering::SeqCst);
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("not the volume")), "the edit is reported unsaved");
    assert!(fx.exists(&fx.trip) && !fx.exists(&sidecar), "no sidecar on the other volume, though its folder is there");

    // The card is back on yet another device number, with its files: accepted, the write lands.
    fx.restore();
    device.store(3, Ordering::SeqCst);
    s.flush().unwrap();
    assert_eq!(fx.star_in_sidecar("IMG_0002.jpg"), Field::Value(true));
    assert_eq!(s.pending_writes(), None, "no online check saw it go");

    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
}
