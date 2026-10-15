//! Find Duplicates (`roadmap.md` M9.16; `organizing.md` §8.3) on temporary libraries:
//! `search.duplicates` as a job with progress and Cancel; photos grouped by size, digested on
//! workers (cached in the index by size and modification time), each digest group confirmed byte
//! for byte; the groups (largest total first) shown in search mode `duplicates` with the banner's
//! text; `search.clear` leaves it. No sleeps: jobs end through `wait_job`, and the cancel test holds
//! a worker inside a file through the digest hook. The sessions run on a virtual clock that never
//! moves, so the watcher's verification never rescans a folder under a test.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{jpeg_of, options, pick};
use dalica_core::{Clock, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::duplicates::{RUNNING, md5_digest};
use dalica_engine::jobs::JobState;
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use dalica_library::{Folder, Library, Op, Photo};
use serde_json::{Value, json};

/// Long enough for any machine; a job that ends never waits for it.
const WAIT: Duration = Duration::from_secs(600);

fn clock() -> Arc<dyn Clock> {
    Arc::new(VirtualClock::new(Timestamp::from_millis(1_781_049_600_000)))
}

fn open(dir: &Path) -> Session {
    Session::open_with(dir, clock(), &options()).unwrap()
}

/// A scanned library watching `Trips/`:
///
/// | folder | file | bytes |
/// |---|---|---|
/// | `A`, `B`, `C` | `IMG_0001.jpg` | picture X (three copies) |
/// | `D` | `IMG_0002.jpg`, `IMG_0003.jpg` | picture Y (a pair, another size) |
/// | `E` | `IMG_0004.jpg` | X with one byte changed: the same size, other bytes |
/// | `F` | `IMG_0005.jpg` | picture W, a size of its own |
struct Fx {
    _lib: tempfile::TempDir,
    _disk: tempfile::TempDir,
    root: PathBuf,
    s: Session,
}

fn fixture() -> Fx {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = disk.path().join("Trips");
    let x = jpeg_of(1, 96, 64);
    let y = jpeg_of(2, 160, 120);
    let w = jpeg_of(3, 64, 64);
    let mut z = x.clone();
    let mid = z.len() / 2;
    z[mid] ^= 0x55;
    assert!(x.len() != y.len() && x.len() != w.len() && y.len() != w.len(), "the fixture's sizes must differ");
    for (name, bytes) in [
        ("A/IMG_0001.jpg", &x),
        ("B/IMG_0001.jpg", &x),
        ("C/IMG_0001.jpg", &x),
        ("D/IMG_0002.jpg", &y),
        ("D/IMG_0003.jpg", &y),
        ("E/IMG_0004.jpg", &z),
        ("F/IMG_0005.jpg", &w),
    ] {
        let path = root.join(name);
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        dalica_fs::replace(&path, bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 7, "every file is a photo of the library");
    Fx { _lib: lib, _disk: disk, root, s }
}

impl Fx {
    fn id(&self, rel: &str) -> PhotoId {
        let path = self.root.join(rel);
        let lib = self.s.library();
        lib.photos().find(|p| lib.folder(p.folder).unwrap().path.join(&p.name) == path).unwrap().id
    }

    fn ids(&self, rels: &[&str]) -> Vec<PhotoId> {
        rels.iter().map(|r| self.id(r)).collect()
    }

    /// The two groups the fixture holds, largest total first.
    fn expected(&self) -> Vec<Vec<PhotoId>> {
        let triple = self.ids(&["A/IMG_0001.jpg", "B/IMG_0001.jpg", "C/IMG_0001.jpg"]);
        let pair = self.ids(&["D/IMG_0002.jpg", "D/IMG_0003.jpg"]);
        let total = |g: &Vec<PhotoId>| self.s.library().photo(g[0]).unwrap().size * g.len() as u64;
        let mut groups = vec![triple, pair];
        groups.sort_by_key(|g| std::cmp::Reverse(total(g)));
        groups
    }

    /// Runs Find Duplicates to its end and returns the job's entry as JSON.
    fn run(&mut self) -> Value {
        let job = self.s.execute("search.duplicates", &json!({})).unwrap()["job"].as_u64().unwrap();
        let entry = self.s.wait_job(job, WAIT).unwrap();
        assert!(entry.state.is_finished(), "the job ended");
        serde_json::to_value(entry).unwrap()
    }

    fn search(&self) -> Value {
        self.s.search_inspect()
    }

    fn groups(&self) -> Vec<Vec<PhotoId>> {
        self.s.search_state().result.as_ref().map(|r| r.groups.clone()).unwrap_or_default()
    }
}

#[test]
fn identical_files_make_groups_and_a_same_size_file_stays_out() {
    let mut fx = fixture();
    let (revision, undo) = (fx.s.library().revision(), fx.s.undo_stack().len());
    // The owner thread makes no file system call: every read is a worker's.
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let entry = fx.run();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");

    assert_eq!(entry["state"], json!("done"));
    assert_eq!(entry["kind"], json!("duplicates"));
    assert_eq!(entry["label"], json!("2 duplicate groups (5 files)"));
    let r = &entry["result"];
    assert_eq!((r["groups"].clone(), r["files"].clone()), (json!(2), json!(5)));
    // W has a size of its own: never read. The other six are read once each for a digest, and the
    // five in digest groups once more for the comparison.
    assert_eq!((r["candidates"].clone(), r["hashed"].clone(), r["cached"].clone()), (json!(6), json!(6), json!(0)));
    assert_eq!((r["compared"].clone(), r["skipped"].clone()), (json!(5), json!(0)));
    assert_eq!(entry["progress"], json!({ "done": 11, "total": 11 }));

    let expected = fx.expected();
    assert_eq!(fx.groups(), expected);
    let z = fx.id("E/IMG_0004.jpg");
    assert!(!fx.groups().iter().flatten().any(|id| *id == z), "the same size with other bytes is no duplicate");
    let v = fx.search();
    assert_eq!((v["active"].clone(), v["mode"].clone(), v["matches"].clone()), (json!(true), json!("duplicates"), json!(5)));
    assert_eq!(v["banner"], json!("2 duplicate groups (5 files)"));
    assert_eq!(v["folders"], json!(4));
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    for (g, ids) in groups.iter().zip(&expected) {
        let size = fx.s.library().photo(ids[0]).unwrap().size;
        assert_eq!(g, &json!({ "count": ids.len(), "size": size, "bytes": size * ids.len() as u64, "ids": ids }));
    }
    let shown: Vec<PhotoId> = expected.iter().flatten().copied().collect();
    assert_eq!(fx.s.search_filter().unwrap().ids(), shown.as_slice(), "the light table shows the groups' photos in order");

    // The digests are kept in the index (derived: no revision, no undo step).
    for id in fx.s.library().photos().map(|p| p.id).collect::<Vec<_>>() {
        let wanted = id != fx.id("F/IMG_0005.jpg");
        assert_eq!(fx.s.library().content_digest(id).is_some(), wanted, "{id}");
    }
    let x = std::fs::read(fx.root.join("A/IMG_0001.jpg")).unwrap();
    assert_eq!(fx.s.library().content_digest(fx.id("B/IMG_0001.jpg")), Some(md5_digest(&x).as_str()));
    assert_eq!((fx.s.library().revision(), fx.s.undo_stack().len()), (revision, undo));

    // search.clear leaves duplicates mode.
    let v = fx.s.execute("search.clear", &json!({})).unwrap();
    assert_eq!(
        (v["active"].clone(), v["mode"].clone(), v["groups"].clone(), v["banner"].clone()),
        (json!(false), json!(null), json!(null), json!(null))
    );
    assert!(fx.s.search_filter().is_none());
}

#[test]
fn a_second_run_reads_no_file_whose_digest_is_cached() {
    let mut fx = fixture();
    fx.run();
    assert_eq!(fx.s.duplicates_stats().reads(), 11);
    fx.s.execute("search.clear", &json!({})).unwrap();
    let entry = fx.run();
    let stats = fx.s.duplicates_stats();
    assert_eq!((stats.hashed, stats.compared, stats.cached, stats.candidates), (0, 0, 6, 6), "{entry}");
    assert_eq!(fx.groups(), fx.expected());
    assert_eq!(entry["progress"], json!({ "done": 0, "total": 0 }));
}

#[test]
fn digests_survive_a_restart() {
    let mut fx = fixture();
    fx.run();
    fx.s.flush().unwrap();
    let Fx { _lib: lib, _disk: disk, root, s } = fx;
    drop(s);
    let mut fx = Fx { s: open(lib.path()), _lib: lib, _disk: disk, root };
    fx.run();
    let stats = fx.s.duplicates_stats();
    // No digest is made again; this session confirms the groups byte for byte once.
    assert_eq!((stats.hashed, stats.cached, stats.compared), (0, 6, 5));
    assert_eq!(fx.groups(), fx.expected());
}

#[test]
fn a_planted_digest_collision_is_split_by_the_byte_comparison() {
    let mut fx = fixture();
    // Every file gets the same digest: only the sizes and the bytes tell them apart.
    fx.s.set_duplicates_digest(Some(Arc::new(|_: &[u8]| "md5:00000000000000000000000000000000".to_string())));
    let entry = fx.run();
    assert_eq!(entry["state"], json!("done"));
    assert_eq!(fx.groups(), fx.expected());
    let z = fx.id("E/IMG_0004.jpg");
    assert!(!fx.groups().iter().flatten().any(|id| *id == z));
    // X's bucket held four files (E's among them), Y's two.
    assert_eq!(fx.s.duplicates_stats().compared, 6);
}

#[test]
fn cancel_stops_the_run_and_keeps_the_digests_made() {
    let mut fx = fixture();
    // The digest hook tells the test a worker is inside a file, then holds it until released.
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (entered, go) = (Mutex::new(entered_tx), Mutex::new(go_rx));
    fx.s.set_duplicates_digest(Some(Arc::new(move |bytes: &[u8]| {
        let _ = entered.lock().unwrap().send(());
        let _ = go.lock().unwrap().recv();
        md5_digest(bytes)
    })));
    let job = fx.s.execute("search.duplicates", &json!({})).unwrap()["job"].as_u64().unwrap();
    entered_rx.recv().unwrap();
    // While it runs: listed with its progress, and greyed out.
    assert_eq!(fx.s.duplicates_job(), Some(job));
    let listed = fx.s.jobs().into_iter().find(|j| j.job == job).unwrap();
    assert_eq!((listed.state, listed.cancellable, listed.progress.total), (JobState::Running, true, 6));
    assert_eq!(fx.s.execute("search.duplicates", &json!({})), Err(CmdError::Disabled(RUNNING.into())));
    assert_eq!(fx.s.cancel_jobs(Some(job)).unwrap(), vec![job]);
    drop(go_tx);
    let entry = fx.s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Cancelled);
    let stats = fx.s.duplicates_stats();
    assert!(stats.hashed >= 1 && stats.hashed < 6, "the workers stopped before their next file: {stats:?}");
    assert_eq!(stats.compared, 0);
    assert!(!fx.search()["active"].as_bool().unwrap(), "the light table stays as it was");
    let kept = fx.s.library().photos().filter(|p| fx.s.library().content_digest(p.id).is_some()).count() as u64;
    assert_eq!(kept, stats.hashed, "the digests made before Cancel are kept");

    // Enabled again; the next run makes only the digests still missing.
    assert_eq!(fx.s.duplicates_job(), None);
    fx.s.set_duplicates_digest(None);
    fx.run();
    assert_eq!(fx.s.duplicates_stats().hashed, 6 - stats.hashed);
    assert_eq!(fx.groups(), fx.expected());
}

#[test]
fn a_search_started_while_it_runs_keeps_the_light_table() {
    let mut fx = fixture();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (entered, go) = (Mutex::new(entered_tx), Mutex::new(go_rx));
    fx.s.set_duplicates_digest(Some(Arc::new(move |bytes: &[u8]| {
        let _ = entered.lock().unwrap().send(());
        let _ = go.lock().unwrap().recv();
        md5_digest(bytes)
    })));
    let job = fx.s.execute("search.duplicates", &json!({})).unwrap()["job"].as_u64().unwrap();
    entered_rx.recv().unwrap();
    fx.s.execute("search.set", &json!({ "text": "img" })).unwrap();
    drop(go_tx);
    let entry = fx.s.wait_job(job, WAIT).unwrap();
    assert_eq!((entry.state, entry.label.as_str()), (JobState::Done, "2 duplicate groups (5 files)"));
    assert_eq!(fx.search()["mode"], json!("search"), "the search typed meanwhile stays");
}

#[test]
fn graceful_failures() {
    let mut fx = fixture();
    let (model, undo, jobs) = (fx.s.library().model().clone(), fx.s.undo_stack().len(), fx.s.jobs().len());
    let deep = serde_json::from_str::<Value>(&format!("{}{}", "[".repeat(100), "]".repeat(100))).unwrap();
    for bad in [json!([]), json!("text"), json!(42), json!(true), json!({ "zzz": 1 }), json!({ "ids": [1] }), deep] {
        let r = fx.s.execute("search.duplicates", &bad);
        assert!(matches!(r, Err(CmdError::BadParams(_))), "{bad}: {r:?}");
    }
    assert_eq!((fx.s.library().model(), fx.s.undo_stack().len(), fx.s.jobs().len()), (&model, undo, jobs));
    // An empty library finds nothing and shows the empty result.
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    let job = s.execute("search.duplicates", &Value::Null).unwrap()["job"].as_u64().unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert_eq!((entry.state, entry.label.as_str()), (JobState::Done, "0 duplicate groups (0 files)"));
    assert_eq!(s.search_inspect()["banner"], json!("0 duplicate groups (0 files)"));
    // A photo whose file is gone is left out and counted (the frozen clock keeps the watcher from
    // noticing).
    std::fs::remove_file(fx.root.join("B/IMG_0001.jpg")).unwrap();
    fx.run();
    let stats = fx.s.duplicates_stats();
    assert_eq!(stats.skipped, 1);
    assert_eq!(fx.groups()[0].len().min(fx.groups()[1].len()), 2, "the triple is a pair now");
    assert_eq!(fx.s.undo_stack().len(), undo);
}

/// Writes `n` files into 20 folders under `root` and returns the library of them (sizes and times
/// as on disk). One file in 20 is a copy of the one before it; the others are distinct, and every
/// size is shared by `n / 400` files (`n` a multiple of 800), so every file is read for its digest.
fn ten_k_files(root: &Path, n: usize) -> Library {
    let content = |i: usize| -> Vec<u8> {
        let len = 2048 + (i % 400) * 16;
        let mut state = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    };
    let mut ops = Vec::new();
    let mut photos = Vec::new();
    for f in 0..20u64 {
        let path = root.join(format!("F{f:02}"));
        std::fs::create_dir_all(&path).unwrap();
        ops.push(Op::AddFolder { folder: Folder { id: FolderId::new(f + 1), path } });
    }
    for i in 0..n {
        let bytes = if i % 20 == 19 { content(i - 1) } else { content(i) };
        let f = (i % 20) as u64;
        let name = format!("IMG_{i:05}.jpg");
        let path = root.join(format!("F{f:02}")).join(&name);
        std::fs::write(&path, &bytes).unwrap();
        let mtime = Timestamp::from_system_time(std::fs::metadata(&path).unwrap().modified().unwrap());
        photos.push(Photo {
            id: PhotoId::new(i as u64 + 1),
            folder: FolderId::new(f + 1),
            name,
            size: bytes.len() as u64,
            mtime,
            width: 64,
            height: 64,
            date: None,
            star: false,
        });
    }
    ops.push(Op::AddPhotos { photos });
    let mut library = Library::new();
    library.apply(Op::Batch { ops }).unwrap();
    library
}

/// 10 000 photos at the review gate (timed), 800 in a task's gate.
#[test]
fn ten_thousand_photos_timed() {
    let (n, label) = pick((800, "800"), (10_000, "10 000"));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    let library = ten_k_files(&root, n);
    let lib_dir = tmp.path().join("library");
    std::fs::create_dir_all(&lib_dir).unwrap();
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &library, 0).unwrap();
    let roots = json!({ "version": 1, "folders": [{ "path": root }] });
    std::fs::write(lib_dir.join(dalica_engine::session::FOLDERS_FILE), serde_json::to_vec(&roots).unwrap()).unwrap();
    let mut s = open(&lib_dir);
    let mut run = || {
        let start = Instant::now();
        let job = s.execute("search.duplicates", &json!({})).unwrap()["job"].as_u64().unwrap();
        let entry = s.wait_job(job, WAIT).unwrap();
        assert_eq!(entry.state, JobState::Done);
        (start.elapsed(), s.duplicates_stats(), s.search_state().result.as_ref().map(|r| (r.groups.len(), r.count())))
    };
    let (first, stats, found) = run();
    let all = n as u64;
    assert_eq!((stats.candidates, stats.hashed, stats.compared), (all, all, all / 10));
    assert_eq!(found, Some((n / 20, n / 10)), "one file in 20 copies the one before it");
    let (second, again, found_again) = run();
    assert_eq!((again.hashed, again.compared, again.cached), (0, 0, all));
    assert_eq!(found_again, found);
    let pairs: BTreeSet<usize> = s.search_state().result.as_ref().unwrap().groups.iter().map(Vec::len).collect();
    assert_eq!(pairs, BTreeSet::from([2]));
    eprintln!(
        "Find Duplicates, {label} photos ({} build): first run {:.2} s ({n} digests, {} files compared), second run {:.3} s (all cached)",
        if cfg!(debug_assertions) { "debug" } else { "release" },
        n / 10,
        first.as_secs_f64(),
        second.as_secs_f64()
    );
}
