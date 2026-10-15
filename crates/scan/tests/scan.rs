//! Scans of temporary trees (`architecture.md` §11): nested folders, skip rules, content
//! sniffing, a verification pass that emits exactly the ops for what changed on disk, streamed
//! per-folder results, cancelling, excluded subtrees and the per-file guard.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::common::{apply_all, put};
use dalica_image::Format;
use dalica_library::{Library, Op};
use dalica_scan::{Event, HeaderReader, Options, default_reader, diff, scan, scan_streaming, scan_with};
use dalica_scenes::{Exif, Scene, demo_library, jpeg, png};

fn camera(seed: u64, o: u8) -> Vec<u8> {
    let exif = Exif { orientation: o, date_time_original: "2026-06-04T19:42:10".parse().ok(), ..Exif::default() };
    jpeg(&Scene::new(seed, 120, 80), 80, Some(&exif)).unwrap()
}

/// A tree with nested folders, things to skip and a lying extension.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    put(&r.join("top.jpg"), &camera(1, 1));
    put(&r.join("a/1.jpg"), &camera(2, 6));
    put(&r.join("a/b/2.png"), &png(&Scene::new(3, 64, 48)).unwrap());
    put(&r.join("a/b/really-a-png.jpg"), &png(&Scene::new(4, 40, 30)).unwrap());
    put(&r.join("a/notes.txt"), b"not a photo, and long enough to be sniffed as nothing at all");
    put(&r.join("a/fake.jpg"), b"text pretending to be a JPEG file, long enough to sniff");
    put(&r.join(".hidden/3.jpg"), &camera(5, 1));
    put(&r.join("a/.dalica-originals/1.jpg"), &camera(6, 1));
    put(&r.join("@eaDir/thumb.jpg"), &camera(7, 1));
    put(&r.join("a/.dot.jpg"), &camera(8, 1));
    std::fs::create_dir_all(r.join("empty")).unwrap();
    dir
}

fn names(lib: &Library, root: &Path) -> Vec<String> {
    let mut v: Vec<String> = lib
        .photos()
        .map(|p| {
            let folder = &lib.folder(p.folder).unwrap().path;
            let rel = folder.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
            [rel, vec![p.name.clone()]].concat().join("/")
        })
        .collect();
    v.sort();
    v
}

#[test]
fn finds_nested_folders_and_skips_what_it_should() {
    let dir = tree();
    let s = scan(&[dir.path().to_path_buf()], None);
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    assert_eq!(names(&lib, dir.path()), ["a/1.jpg", "a/b/2.png", "a/b/really-a-png.jpg", "top.jpg"]);
    assert_eq!(lib.folders().count(), 3);
    // Size as displayed: orientation 6 swaps the stored 80 × 120 back to 120 × 80.
    let one = lib.photos().find(|p| p.name == "1.jpg").unwrap();
    assert_eq!((one.width, one.height), (120, 80));
    assert_eq!(one.date.unwrap().to_string(), "2026-06-04T19:42:10");
    assert_eq!(lib.photos().find(|p| p.name == "really-a-png.jpg").unwrap().width, 40);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    // A second pass over an unchanged tree finds nothing to do.
    assert_eq!(diff(&mut lib, &scan(&[dir.path().to_path_buf()], None)), []);
}

#[test]
fn verification_emits_exactly_the_ops_for_what_changed() {
    let dir = tree();
    let r = dir.path().to_path_buf();
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    let id_of = |lib: &Library, name: &str| lib.photos().find(|p| p.name == name).unwrap().id;
    let (one, two, top) = (id_of(&lib, "1.jpg"), id_of(&lib, "2.png"), id_of(&lib, "top.jpg"));
    lib.apply(Op::SetStars { stars: vec![(one, true)] }).unwrap();

    // Add, delete, rename (keeps size and time) and rewrite (new size).
    put(&r.join("a/new.jpg"), &camera(9, 3));
    std::fs::remove_file(r.join("a/b/2.png")).unwrap();
    std::fs::rename(r.join("a/1.jpg"), r.join("a/renamed.jpg")).unwrap();
    put(&r.join("top.jpg"), &camera(10, 8));

    let s = scan(std::slice::from_ref(&r), None);
    let before_next = lib.clone().next_photo_id();
    let ops = diff(&mut lib, &s);
    let a = lib.folder_by_path(&r.join("a")).unwrap();
    let kinds: Vec<String> = ops
        .iter()
        .map(|op| match op {
            Op::RemovePhotos { ids } => format!("remove {ids:?}"),
            Op::UpdatePhotos { photos } => format!("update {:?}", photos.iter().map(|p| (p.id, p.name.as_str(), p.star)).collect::<Vec<_>>()),
            Op::AddPhotos { photos } => format!("add {:?}", photos.iter().map(|p| (p.id, p.folder, p.name.as_str())).collect::<Vec<_>>()),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            format!("remove {:?}", [two]),
            format!("update {:?}", [(top, "top.jpg", false), (one, "renamed.jpg", true)]),
            format!("add {:?}", [(before_next, a, "new.jpg")]),
        ]
    );
    apply_all(&mut lib, ops);
    assert_eq!(names(&lib, &r), ["a/b/really-a-png.jpg", "a/new.jpg", "a/renamed.jpg", "top.jpg"]);
    // The rename kept the photo's id and star; the rewrite took the new orientation's size.
    assert!(lib.photo(one).unwrap().star);
    assert_eq!((lib.photo(top).unwrap().width, lib.photo(top).unwrap().height), (120, 80));
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), []);

    // A folder that lost its last photo is removed.
    std::fs::remove_file(r.join("a/b/really-a-png.jpg")).unwrap();
    let b = lib.folder_by_path(&r.join("a/b")).unwrap();
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), [Op::RemoveFolder { id: b }]);
}

#[test]
fn an_offline_root_removes_nothing() {
    let dir = tree();
    let r = dir.path().join("card");
    put(&r.join("x.jpg"), &camera(11, 1));
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    std::fs::rename(&r, dir.path().join("card-unplugged")).unwrap();
    let s = scan(std::slice::from_ref(&r), None);
    assert!(s.walked.is_empty() && s.warnings.len() == 1, "{s:?}");
    assert_eq!(diff(&mut lib, &s), []);
}

#[test]
fn stars_come_from_the_folder_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("p/x.jpg"), &camera(12, 1));
    put(&dir.path().join("p/y.jpg"), &camera(13, 1));
    put(&dir.path().join("p/.dalica.json"), br#"{"photos": {"y.jpg": {"star": true}, "gone.jpg": {"star": true}}}"#);
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(&[dir.path().to_path_buf()], None));
    apply_all(&mut lib, ops);
    assert_eq!(lib.starred().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["y.jpg"]);
    // A damaged sidecar is reported, not fatal.
    put(&dir.path().join("p/.dalica.json"), b"{oops");
    let s = scan(&[dir.path().to_path_buf()], None);
    assert_eq!(s.folders[0].photos.len(), 2);
    assert!(s.warnings.iter().any(|w| w.path.ends_with(".dalica.json")), "{:?}", s.warnings);
}

/// Everything the user said in a sidecar comes in with the photos and the folder a scan adds, in
/// the very op that adds them; an unusable value is left out (the file keeps it); a folder and
/// photos already known keep the model's word.
#[test]
fn every_sidecar_field_comes_in_with_the_photos_in_the_same_op() {
    use dalica_library::{FolderInfo, MAX_CAPTION, PhotoInfo};
    let dir = tempfile::tempdir().unwrap();
    for (i, n) in ["a.jpg", "b.jpg", "c.jpg", "d.jpg"].iter().enumerate() {
        put(&dir.path().join("p").join(n), &camera(20 + i as u64, 1));
    }
    let long = "é".repeat(MAX_CAPTION);
    let full_recipe = vec!["\"flip_h()\""; 10_001].join(",");
    let sidecar = format!(
        r#"{{
  "dalica": {{ "version": 1 }},
  "folder": {{ "description": "Weekend", "date": "2026-06-04", "place": "Lisbon", "hidden": true, "future": 1 }},
  "photos": {{
    "a.jpg": {{ "star": true, "caption": "Tram 28", "tags": ["tram", " Lisbon ", "TRAM"], "hidden": true,
               "edits": ["rotate(90)", "future_op(1)", "legacy:x(2)"], "redo": ["glow(0.4,3.0)"], "date": "2026-06-04T19:42:10" }},
    "b.jpg": {{ "caption": 7, "tags": "tram", "hidden": "yes", "edits": [1], "date": "June" }},
    "c.jpg": {{ "caption": "{long}", "edits": [{full_recipe}] }},
    "gone.jpg": {{ "caption": "not here" }}
  }}
}}"#
    );
    put(&dir.path().join("p/.dalica.json"), sidecar.as_bytes());
    let s = scan(&[dir.path().to_path_buf()], None);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let found = &s.folders[0];
    assert_eq!(found.info, FolderInfo { description: "Weekend".into(), date: "2026-06-04".parse().ok(), place: "Lisbon".into(), hidden: true });
    let info = |name: &str| found.photos.iter().find(|p| p.name == name).unwrap().info.clone();
    assert_eq!(
        info("a.jpg"),
        PhotoInfo {
            caption: "Tram 28".into(),
            tags: vec!["tram".into(), "Lisbon".into()],
            hidden: true,
            edits: vec!["rotate(90)".into(), "future_op(1)".into(), "legacy:x(2)".into()],
            redo: vec!["glow(0.4,3.0)".into()],
            date: "2026-06-04T19:42:10".parse().ok(),
        }
    );
    assert_eq!(info("b.jpg"), PhotoInfo::default(), "values of the wrong type are not used");
    let c = info("c.jpg");
    assert!(c.caption.len() <= MAX_CAPTION && long.starts_with(&c.caption) && !c.caption.is_empty(), "a caption past the cap is cut");
    assert!(c.edits.is_empty(), "a recipe past the cap is not used");
    assert_eq!(info("d.jpg"), PhotoInfo::default());

    // The diff adds the folder and the photos with what they bring, each in one op.
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    assert!(matches!(&ops[0], Op::Batch { ops } if matches!(ops.as_slice(), [Op::AddFolder { .. }, Op::SetFolderInfo { .. }])), "{:?}", ops[0]);
    assert!(matches!(&ops[1], Op::Batch { ops } if matches!(ops.as_slice(), [Op::AddPhotos { .. }, Op::SetPhotoInfo { .. }])), "{:?}", ops[1]);
    assert_eq!(ops.len(), 2);
    apply_all(&mut lib, ops);
    let id = |lib: &Library, name: &str| lib.photos().find(|p| p.name == name).unwrap().id;
    let a = id(&lib, "a.jpg");
    assert_eq!(*lib.info(a), info("a.jpg"));
    assert!(lib.photo(a).unwrap().star);
    assert!(lib.is_folder_hidden(lib.folder_by_path(&dir.path().join("p")).unwrap()));
    assert_eq!(lib.tag_index().count("tram"), 1);
    assert!(diff(&mut lib, &s).is_empty(), "diffing again gives nothing");

    // Known photos and folders keep the model's word: a rescan does not take the file's value.
    lib.apply(Op::SetCaptions { captions: vec![(a, "Mine".into())] }).unwrap();
    let again = scan(&[dir.path().to_path_buf()], None);
    assert!(diff(&mut lib, &again).is_empty());
    assert_eq!(lib.caption(a), "Mine");
}

/// A folder without a sidecar adds its photos with plain ops, as before.
#[test]
fn without_sidecar_data_the_ops_are_plain() {
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("p/x.jpg"), &camera(30, 1));
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(&[dir.path().to_path_buf()], None));
    assert!(matches!(ops.as_slice(), [Op::AddFolder { .. }, Op::AddPhotos { .. }]), "{ops:?}");
}

#[test]
fn a_set_cancel_flag_stops_the_scan() {
    let dir = tree();
    let cancel = AtomicBool::new(true);
    let s = scan(&[dir.path().to_path_buf()], Some(&cancel));
    assert!(s.folders.is_empty());
    assert!(s.warnings.iter().any(|w| w.reason.contains("Cancelled")), "{:?}", s.warnings);
    // A root not walked to the end is not walked: nothing under it may be removed.
    assert!(s.cancelled && s.walked.is_empty(), "{s:?}");
}

/// At full strength 1 000 JPEGs in 10 folders; in a task's gate 40 in 4, the same checks.
#[test]
fn scanning_1000_scenes_jpegs() {
    let dir = tempfile::tempdir().unwrap();
    let (folders, photos) = if crate::common::full() { (10, 1000) } else { (4, 40) };
    let files = demo_library(folders, photos, 7).unwrap();
    for f in &files {
        put(&dir.path().join(&f.path), &f.bytes);
    }
    let roots: Vec<PathBuf> = vec![dir.path().to_path_buf()];
    let start = Instant::now();
    let s = scan(&roots, None);
    let scanned = start.elapsed();
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    let total = start.elapsed();
    println!("scan of {photos} JPEGs in {folders} folders: walk + headers {scanned:?}, with diff and apply {total:?}");
    assert_eq!((lib.folders().count(), lib.photos().count()), (folders, photos));
    // Every orientation was read: portrait and landscape both appear as displayed.
    assert!(lib.photos().all(|p| (p.width, p.height) == (480, 320) || (p.width, p.height) == (320, 480)));
    assert!(total.as_secs() < 20, "{total:?}");
}

/// A FIFO named like a photo (or any device) is skipped: opening it would block the scan forever.
#[cfg(unix)]
#[test]
fn a_fifo_never_blocks_a_scan() {
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("p/real.jpg"), &camera(14, 1));
    assert!(std::process::Command::new("mkfifo").arg(dir.path().join("p/pipe.jpg")).status().unwrap().success());
    let root = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(scan(&[root], None)).unwrap());
    let s = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the scan blocked on the FIFO");
    assert_eq!(s.folders.len(), 1);
    assert_eq!(s.folders[0].photos.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["real.jpg"]);
}

/// The default reader, recording every file it is asked about.
fn recording() -> (HeaderReader, Arc<Mutex<Vec<PathBuf>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (inner, seen) = (default_reader(), log.clone());
    let reader: HeaderReader = Arc::new(move |p: &Path, l: &dalica_image::Limits| {
        seen.lock().unwrap().push(p.to_path_buf());
        inner(p, l)
    });
    (reader, log)
}

fn folder_names(s: &dalica_scan::Scan, root: &Path) -> Vec<String> {
    s.folders.iter().map(|f| f.path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect()
}

#[test]
fn an_excluded_subtree_is_never_walked() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    put(&r.join("a/x.jpg"), &camera(20, 1));
    put(&r.join("a/removed/y.jpg"), &camera(21, 1));
    put(&r.join("a/removed/deeper/z.jpg"), &camera(22, 1));
    put(&r.join("b/w.jpg"), &camera(23, 1));
    let (reader, log) = recording();
    let mut progress = Vec::new();
    let options = Options { excluded: vec![r.join("a/removed")], reader, ..Options::default() };
    let summary = scan_streaming(&[r.to_path_buf()], &options, |e| {
        if let Event::Progress(p) = e {
            progress.push(p);
        }
        true
    });
    assert!(log.lock().unwrap().iter().all(|p| !p.starts_with(r.join("a/removed"))), "{log:?}");
    assert_eq!(log.lock().unwrap().len(), 2);
    // The root, a and b: the excluded folder and the one inside it were never listed.
    assert_eq!(progress.last().unwrap().folders_done, 3, "{progress:?}");
    assert_eq!(summary.walked, [r.to_path_buf()]);
    let s = scan_with(&[r.to_path_buf()], &Options { excluded: vec![r.join("a/removed")], ..Options::default() });
    assert_eq!(folder_names(&s, r), ["a", "b"]);
    // An excluded root is walked as an empty one: its folders leave the library.
    let s = scan_with(&[r.join("a")], &Options { excluded: vec![r.join("a")], ..Options::default() });
    assert!(s.folders.is_empty() && s.walked == [r.join("a")], "{s:?}");
}

#[test]
fn results_arrive_per_folder_before_the_walk_ends() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().to_path_buf();
    for (i, f) in ["a", "b", "c"].iter().enumerate() {
        put(&r.join(f).join("p.jpg"), &camera(30 + i as u64, 1));
        put(&r.join(f).join("q.jpg"), &camera(40 + i as u64, 1));
    }
    // The last folder's photos wait until the test has seen the first folder's.
    let (open_tx, open_rx) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(open_rx));
    let inner = default_reader();
    let reader: HeaderReader = Arc::new(move |p: &Path, l: &dalica_image::Limits| {
        if p.parent().is_some_and(|d| d.ends_with("c")) {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(20));
        }
        inner(p, l)
    });
    let options = Options { reader, ..Options::default() };
    let (tx, rx) = mpsc::channel();
    let roots = vec![r.clone()];
    let scanning = std::thread::spawn(move || scan_streaming(&roots, &options, |e| tx.send(e).is_ok()));
    let mut events = Vec::new();
    loop {
        let e = rx.recv_timeout(Duration::from_secs(20)).expect("no folder arrived while the walk was held");
        let first_folder = matches!(&e, Event::Folder(f) if f.path.ends_with("a"));
        events.push(e);
        if first_folder {
            break;
        }
    }
    // The walk is still held inside c, and a's photos are already here.
    assert!(!scanning.is_finished());
    open_tx.send(()).unwrap();
    open_tx.send(()).unwrap();
    events.extend(rx.iter());
    let summary = scanning.join().unwrap();
    let folders: Vec<&Path> = events.iter().filter_map(|e| if let Event::Folder(f) = e { Some(f.path.as_path()) } else { None }).collect();
    assert_eq!(folders, [r.join("a"), r.join("b"), r.join("c")]);
    // Progress only rises, and ends at the totals.
    let progress: Vec<_> = events.iter().filter_map(|e| if let Event::Progress(p) = e { Some(*p) } else { None }).collect();
    assert!(
        progress.windows(2).all(|w| w[0].folders_done < w[1].folders_done && w[0].found <= w[1].found && w[0].files <= w[1].files),
        "{progress:?}"
    );
    let last = progress.last().unwrap();
    assert_eq!((last.folders_done, last.folders_queued, last.files, last.found, last.failed), (4, 0, 6, 6, 0));
    assert_eq!(summary.progress, *last);
    assert_eq!(summary.walked, [r]);
    assert!(!summary.cancelled);
}

#[test]
fn a_cancelled_scan_stops_at_the_next_file() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().to_path_buf();
    put(&r.join("a/0.jpg"), &camera(50, 1));
    for i in 1..=5 {
        put(&r.join(format!("b/{i}.jpg")), &camera(50 + i, 1));
    }
    put(&r.join("c/9.jpg"), &camera(59, 1));
    let cancel = Arc::new(AtomicBool::new(false));
    let (inner, flag) = (default_reader(), cancel.clone());
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    let reader: HeaderReader = Arc::new(move |p: &Path, l: &dalica_image::Limits| {
        seen.lock().unwrap().push(p.file_name().unwrap().to_string_lossy().into_owned());
        if p.ends_with("2.jpg") {
            flag.store(true, Ordering::Relaxed);
        }
        inner(p, l)
    });
    let options = Options { cancel: Some(cancel), reader, ..Options::default() };
    let mut events = Vec::new();
    let summary = scan_streaming(std::slice::from_ref(&r), &options, |e| {
        events.push(e);
        true
    });
    assert_eq!(*log.lock().unwrap(), ["0.jpg", "1.jpg", "2.jpg"]);
    // Folder a was handed out; b, read halfway, was not; c was never reached.
    let folders: Vec<&Path> = events.iter().filter_map(|e| if let Event::Folder(f) = e { Some(f.path.as_path()) } else { None }).collect();
    assert_eq!(folders, [r.join("a")]);
    assert!(summary.cancelled && summary.walked.is_empty(), "{summary:?}");
    assert!(events.iter().any(|e| matches!(e, Event::Warning(w) if w.reason.contains("Cancelled"))));

    // A receiver that goes away stops the scan the same way.
    let (tx, rx) = mpsc::channel();
    drop(rx);
    let summary = scan_streaming(&[r], &Options::default(), |e| tx.send(e).is_ok());
    assert!(summary.cancelled && summary.walked.is_empty(), "{summary:?}");
}

#[test]
fn a_panicking_or_slow_file_is_reported_once_and_retried_when_it_changes() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().to_path_buf();
    put(&r.join("p/boom.jpg"), &camera(60, 1));
    put(&r.join("p/slow.jpg"), &camera(61, 1));
    put(&r.join("p/fine.jpg"), &camera(62, 1));
    // A truncated JPEG: recognised, its header unreadable.
    put(&r.join("p/cut.jpg"), &camera(63, 1)[..40]);
    let calls: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let (inner, count) = (default_reader(), calls.clone());
    let reader: HeaderReader = Arc::new(move |p: &Path, l: &dalica_image::Limits| {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        *count.lock().unwrap().entry(name.clone()).or_default() += 1;
        match name.as_str() {
            "boom.jpg" => panic!("a planted panic"),
            "slow.jpg" => std::thread::sleep(Duration::from_secs(6)),
            _ => {}
        }
        inner(p, l)
    });
    // Generous enough for a busy CI machine opening a small file; far below the stall.
    let options = Options { budget: Duration::from_secs(1), reader, ..Options::default() };
    let run = |options: &Options| {
        let mut failed = Vec::new();
        let start = Instant::now();
        let summary = scan_streaming(std::slice::from_ref(&r), options, |e| {
            if let Event::Failed(f) = e {
                failed.push(f);
            }
            true
        });
        (summary, failed, start.elapsed(), scan_with(std::slice::from_ref(&r), &Options { failures: options.failures.clone(), ..options.clone() }))
    };
    let (first, failed, took, _) = run(&options);
    let mut reasons: Vec<(String, String)> =
        failed.iter().map(|f| (f.path.file_name().unwrap().to_string_lossy().into_owned(), f.reason.clone())).collect();
    reasons.sort();
    assert_eq!(reasons.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["boom.jpg", "cut.jpg", "slow.jpg"], "{reasons:?}");
    assert!(reasons[0].1.contains("panicked") && reasons[0].1.contains("a planted panic"), "{reasons:?}");
    assert!(reasons[2].1.contains("took longer"), "{reasons:?}");
    assert!(failed.iter().all(|f| f.format == Some(Format::Jpeg)));
    assert!(took < Duration::from_secs(5), "the scan waited for the slow file: {took:?}");
    assert_eq!(first.failures.len(), 3);
    assert_eq!(first.progress.failed, 3);

    // Given back unchanged: not read again, not reported again, still listed (at 0 × 0).
    let again = Options { failures: first.failures.clone(), ..options.clone() };
    let before = calls.lock().unwrap().clone();
    let (second, failed, _, listed) = run(&again);
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(second.failures, first.failures);
    let now = calls.lock().unwrap().clone();
    for name in ["boom.jpg", "slow.jpg"] {
        assert_eq!(now.get(name), before.get(name), "{name} was read again");
    }
    let photos: Vec<(&str, u32, u32)> = listed.folders[0].photos.iter().map(|p| (p.name.as_str(), p.width, p.height)).collect();
    assert_eq!(photos, [("boom.jpg", 0, 0), ("cut.jpg", 0, 0), ("fine.jpg", 120, 80), ("slow.jpg", 0, 0)]);

    // Once its modification time changes, it is read (and reported) again.
    let file = std::fs::File::options().write(true).open(r.join("p/boom.jpg")).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(10)).unwrap();
    drop(file);
    let before = calls.lock().unwrap().clone();
    let (third, failed, _, _) = run(&Options { failures: second.failures.clone(), ..options.clone() });
    assert_eq!(failed.iter().map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["boom.jpg"]);
    let now = calls.lock().unwrap().clone();
    assert_eq!(now.get("boom.jpg").copied(), before.get("boom.jpg").map(|n| n + 2), "read once by the streaming scan and once by the gathering one");
    assert_eq!(now.get("slow.jpg"), before.get("slow.jpg"));
    assert_eq!(third.failures.len(), 3);
}

#[test]
fn a_big_folder_reports_progress_as_it_goes() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().to_path_buf();
    put(&r.join("big/photo.jpg"), &camera(70, 1));
    for i in 0..600 {
        put(&r.join(format!("big/note{i:03}.txt")), b"not a photo, and long enough to be sniffed as nothing at all");
    }
    let mut progress = Vec::new();
    scan_streaming(std::slice::from_ref(&r), &Options::default(), |e| {
        if let Event::Progress(p) = e {
            progress.push((p.folders_done, p.files, p.found));
        }
        true
    });
    // The root, then 256 and 512 files into the big folder, then its end.
    assert_eq!(progress, [(1, 0, 0), (1, 256, 0), (1, 512, 0), (2, 601, 1)]);
}
