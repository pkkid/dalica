//! The Import room's sources and listing (`roadmap.md` M11.7; `organizing.md` §3 items 1–3) on
//! temporary folders: cards and drives from mount folders given as `DALICA_DEVICE_ROOTS` reads
//! them, a listing streamed in batches, duplicates of library photos and of earlier candidates
//! (confirmed byte for byte), the room's choices, and every command's contract. The owner thread
//! is marked as the UI thread while the commands run: it never touches the disk.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::common::{jpeg_of, lisbon, off_owner, open};
use dalica_core::DateTime;
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_scenes::{Exif, Scene};
use serde_json::{Value, json};

/// How often the test's device list is listed again.
const REFRESH: Duration = Duration::from_millis(100);

/// Long enough for any machine; a listing that ends never waits for it.
const PATIENCE: Duration = Duration::from_secs(120);

/// A 96 × 64 JPEG of scene `seed` taken `secs` after 2026-06-04T08:00:00.
fn shot(seed: u64, secs: i64) -> Vec<u8> {
    let at = DateTime::from_unix_secs(1_780_560_000 + secs).unwrap();
    let exif = Exif { date_time_original: Some(at), ..Exif::default() };
    dalica_scenes::jpeg(&Scene::new(seed, 96, 64), 80, Some(&exif)).unwrap()
}

/// Writes `files` (`"100CANON/IMG_0001.JPG"` → bytes) under `root`, off the owner thread.
fn put_all(root: &Path, files: Vec<(String, Vec<u8>)>) {
    let root = root.to_path_buf();
    off_owner(move || {
        for (rel, bytes) in files {
            let path = rel.split('/').fold(root.clone(), |p, c| p.join(c));
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
        }
    });
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

fn code(s: &mut Session, id: &str, params: Value) -> &'static str {
    match s.execute(id, &params) {
        Ok(v) => panic!("{id} {params} should fail, gave {v}"),
        Err(e) => e.code(),
    }
}

/// Polls `import.list {limit: 0}` until the listing is no longer loading; the totals seen on the
/// way, and the last answer.
fn wait_listed(s: &mut Session) -> (Vec<u64>, Value) {
    let start = Instant::now();
    let mut totals = Vec::new();
    loop {
        let v = run(s, "import.list", json!({ "limit": 0 }));
        totals.push(v["total"].as_u64().unwrap());
        if v["state"] != "loading" {
            return (totals, v);
        }
        assert!(start.elapsed() < PATIENCE, "the listing never ended: {v}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Every candidate (one page: the tests list fewer than 1 000).
fn candidates(s: &mut Session) -> Vec<Value> {
    run(s, "import.list", json!({}))["candidates"].as_array().unwrap().clone()
}

fn by_name<'a>(list: &'a [Value], name: &str) -> &'a Value {
    list.iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("{name} not listed"))
}

/// The source of `import.sources` at `path`, if listed.
fn source(s: &mut Session, path: &Path) -> Option<Value> {
    let v = run(s, "import.sources", json!({}));
    v["sources"].as_array().unwrap().iter().find(|x| x["path"] == json!(path)).cloned()
}

/// Polls `import.sources` until `want` holds for the source at `path`; how long it took.
fn wait_source(s: &mut Session, path: &Path, want: impl Fn(Option<&Value>) -> bool) -> Duration {
    let start = Instant::now();
    loop {
        if want(source(s, path).as_ref()) {
            return start.elapsed();
        }
        assert!(start.elapsed() < PATIENCE, "import.sources never changed for {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A session on a fresh library whose device list is the folders inside `mounts`.
fn session_on(lib: &Path, mounts: &Path) -> Session {
    let mut s = open(lib);
    s.set_import_device_roots(Some(std::env::join_paths([mounts]).unwrap()), REFRESH);
    s
}

#[test]
fn a_card_appears_in_import_sources_within_a_refresh_and_leaves_when_it_is_removed() {
    let (lib, mounts) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let card = mounts.path().join("EOS_DIGITAL");
    let stick = mounts.path().join("USB STICK");
    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(source(&mut s, &card.join("DCIM")), None);
    put_all(&card, vec![("DCIM/100CANON/IMG_0001.JPG".into(), shot(1, 0))]);
    put_all(&stick, vec![("notes/readme.png".into(), jpeg_of(2, 32, 32))]);

    let took = wait_source(&mut s, &card.join("DCIM"), |x| x.is_some());
    assert!(took < REFRESH * 50, "listed within a few refreshes, took {took:?}");
    let listed = source(&mut s, &card.join("DCIM")).unwrap();
    assert_eq!(listed, json!({ "kind": "card", "name": "EOS_DIGITAL", "path": card.join("DCIM"), "root": card }));
    wait_source(&mut s, &stick, |x| x.is_some());
    let drive = source(&mut s, &stick).expect("a removable volume without DCIM is a drive");
    assert_eq!((drive["kind"].clone(), drive["name"].clone()), (json!("drive"), json!("USB STICK")));

    let v = run(&mut s, "import.open", json!({ "device": card.join("DCIM") }));
    assert_eq!(v["source"], json!({ "kind": "card", "name": "EOS_DIGITAL", "path": card.join("DCIM") }));
    let (_, end) = wait_listed(&mut s);
    assert_eq!((end["status"].clone(), end["total"].clone()), (json!("1 file"), json!(1)));
    let v = run(&mut s, "import.open", json!({ "device": stick }));
    assert_eq!(v["source"]["kind"], "drive", "a drive opens at its root");

    let gone = card.clone();
    off_owner(move || std::fs::remove_dir_all(gone).unwrap());
    wait_source(&mut s, &card.join("DCIM"), |x| x.is_none());
    assert_eq!(code(&mut s, "import.open", json!({ "device": card.join("DCIM") })), "not_found", "a card that left cannot be opened");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
}

#[test]
fn listing_300_photos_streams_in_batches_with_rising_counts_in_capture_groups() {
    let (lib, mounts) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("CARD").join("DCIM");
    // Three runs of 100 shots a minute apart, five hours between runs; written newest first, so
    // the listing's order is not the capture order.
    let files: Vec<(String, Vec<u8>)> = (0..300u64)
        .map(|i| (format!("100CANON/IMG_{:04}.JPG", i + 1), shot(i, ((299 - i) / 100 * 5 * 3600 + (299 - i) % 100 * 60) as i64)))
        .collect();
    put_all(&dcim, files);
    let ui = dalica_fs::mark_ui_thread();
    let v = run(&mut s, "import.open", json!({ "path": dcim }));
    assert_eq!(v["source"]["kind"], "folder");
    let (totals, end) = wait_listed(&mut s);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    assert!(totals.windows(2).all(|w| w[0] <= w[1]), "the count only rises: {totals:?}");
    assert!(totals.iter().any(|t| *t > 0 && *t < 300), "candidates arrive before the listing ends: {totals:?}");
    assert!(end["progress"]["batches"].as_u64().unwrap() >= 2, "{end}");
    assert_eq!((end["state"].clone(), end["status"].clone(), end["total"].clone()), (json!("done"), json!("300 files"), json!(300)));
    assert_eq!(end["groups"].as_array().unwrap().iter().map(|g| g["count"].as_u64().unwrap()).collect::<Vec<_>>(), [100, 100, 100]);
    assert_eq!(end["groups"][0]["start"], "2026-06-04T08:00:00");

    let list = candidates(&mut s);
    assert_eq!(list.len(), 300);
    let times: Vec<&str> = list.iter().map(|c| c["captured"].as_str().unwrap()).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "capture order");
    assert_eq!(list[0]["name"], "IMG_0300.JPG");
    assert_eq!((list[0]["group"].clone(), list[299]["group"].clone()), (json!(0), json!(2)));
    assert!(list.iter().all(|c| c["timeSource"] == "metadata" && c["thumbnail"] == true && c["duplicate"].is_null()));
    let page = run(&mut s, "import.list", json!({ "offset": 290, "limit": 20 }));
    assert_eq!(page["candidates"].as_array().unwrap().len(), 10, "a page ends with the listing");
    let id = list[5]["id"].as_u64().unwrap();
    let thumb = s.import_session().thumbnail(id).expect("the thumbnail is in memory");
    assert!(thumb.width.max(thumb.height) <= 256 && thumb.width > 0);
}

#[test]
fn copies_of_library_photos_and_repeats_on_the_card_are_duplicates_and_a_same_size_impostor_is_not() {
    let photos: Vec<Vec<u8>> = (0..20).map(|i| jpeg_of(i, 96 + i as u32, 64)).collect();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos, None);
    assert!(ids.iter().all(|id| s.library().content_digest(*id).is_none()), "no digest is cached yet");
    let mounts = tempfile::tempdir().unwrap();
    s.set_import_device_roots(Some(std::env::join_paths([mounts.path()]).unwrap()), REFRESH);
    let dcim = mounts.path().join("CARD").join("DCIM");
    let mut files: Vec<(String, Vec<u8>)> = photos.iter().enumerate().map(|(i, b)| (format!("100CANON/LIB_{i:02}.JPG"), b.clone())).collect();
    for i in 0..3u64 {
        files.push((format!("100CANON/NEW_{i}.JPG"), shot(100 + i, i as i64)));
        files.push((format!("101CANON/NEW_{i}.JPG"), shot(100 + i, i as i64)));
    }
    let mut impostor = photos[7].clone();
    let mid = impostor.len() / 2;
    impostor[mid] ^= 0x55;
    files.push(("102CANON/ODD.JPG".into(), impostor));
    files.push(("102CANON/FRESH.JPG".into(), shot(200, 600)));
    put_all(&dcim, files);

    let ui = dalica_fs::mark_ui_thread();
    let v = run(&mut s, "import.open", json!({ "path": dcim }));
    assert_eq!(v["excludeDuplicates"], true, "from Detect duplicates while importing");
    let (_, end) = wait_listed(&mut s);
    let list = candidates(&mut s);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    assert_eq!(end["total"], 28);
    for (i, id) in ids.iter().enumerate() {
        let c = by_name(&list, &format!("LIB_{i:02}.JPG"));
        assert_eq!(c["duplicate"], json!({ "photo": id }), "{c}");
        assert_eq!(c["willImport"], false, "duplicates are left out");
    }
    for i in 0..3 {
        let twins: Vec<&Value> = list.iter().filter(|c| c["name"] == format!("NEW_{i}.JPG")).collect();
        assert_eq!(twins.len(), 2);
        let (dup, first): (Vec<&Value>, Vec<&Value>) = twins.iter().partition(|c| !c["duplicate"].is_null());
        assert_eq!((dup.len(), first.len()), (1, 1), "one of each pair repeats the other: {twins:?}");
        assert_eq!(dup[0]["duplicate"], json!({ "candidate": first[0]["id"] }));
        assert!(dup[0]["id"].as_u64() > first[0]["id"].as_u64(), "the later one is the duplicate");
    }
    assert!(by_name(&list, "ODD.JPG")["duplicate"].is_null(), "the same size with other bytes is no duplicate");
    assert!(by_name(&list, "FRESH.JPG")["duplicate"].is_null());
    assert_eq!((end["duplicates"].clone(), end["toImport"].clone()), (json!(23), json!(5)));
    assert!(ids.iter().all(|id| s.library().content_digest(*id).is_some()), "the digests made are kept in the index");

    let v = run(&mut s, "import.options", json!({ "excludeDuplicates": false }));
    assert_eq!((v["toImport"].clone(), v["duplicates"].clone()), (json!(28), json!(23)), "with Exclude duplicates off they are imported");
}

#[test]
fn exclude_all_include_all_and_the_rooms_marks() {
    let (lib, mounts) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let folder = mounts.path().join("Pictures");
    put_all(&folder, (0..5u64).map(|i| (format!("IMG_{i}.JPG"), shot(i, i as i64 * 60))).collect());
    let revision = s.library().revision();
    run(&mut s, "import.open", json!({ "path": folder }));
    wait_listed(&mut s);
    let list = candidates(&mut s);
    let (a, b) = (list[0]["id"].clone(), list[1]["id"].clone());

    let v = run(&mut s, "import.excludeAll", json!({ "on": true }));
    assert_eq!((v["excluded"].clone(), v["toImport"].clone()), (json!(5), json!(0)));
    let v = run(&mut s, "import.exclude", json!({ "ids": [a], "on": false }));
    assert_eq!(v["toImport"], 1, "one included again");
    assert_eq!(candidates(&mut s)[0]["willImport"], true);
    let v = run(&mut s, "import.excludeAll", json!({ "on": false }));
    assert_eq!((v["excluded"].clone(), v["toImport"].clone()), (json!(0), json!(5)), "Include All");
    let v = run(&mut s, "import.exclude", json!({ "ids": [a, b], "on": true }));
    assert_eq!(v["toImport"], 3);

    run(&mut s, "import.mark", json!({ "ids": [b], "star": true, "rotate": -1 }));
    let c = candidates(&mut s)[1].clone();
    assert_eq!((c["starred"].clone(), c["rotation"].clone()), (json!(true), json!(270)));
    run(&mut s, "import.mark", json!({ "ids": [b], "rotate": 2 }));
    run(&mut s, "import.mark", json!({ "ids": [b], "star": false }));
    let c = candidates(&mut s)[1].clone();
    assert_eq!((c["starred"].clone(), c["rotation"].clone()), (json!(false), json!(90)));
    assert_eq!(s.library().revision(), revision, "the room's choices change nothing in the library");
}

#[test]
fn each_command_keeps_its_contract_and_fails_gracefully() {
    let (lib, mounts) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let (one, two) = (mounts.path().join("one"), mounts.path().join("two"));
    put_all(&one, (0..3u64).map(|i| (format!("A_{i}.JPG"), shot(i, 0))).collect());
    put_all(&two, (0..2u64).map(|i| (format!("B_{i}.JPG"), shot(10 + i, 0))).collect());
    let ui = dalica_fs::mark_ui_thread();

    // Nothing open: an empty listing; the room's choices are greyed out; Close is no error.
    let v = run(&mut s, "import.list", json!({}));
    assert_eq!((v["source"].clone(), v["status"].clone(), v["candidates"].clone()), (Value::Null, json!(""), json!([])));
    for (id, p) in [
        ("import.exclude", json!({ "ids": [0], "on": true })),
        ("import.excludeAll", json!({ "on": true })),
        ("import.mark", json!({ "ids": [0], "star": true })),
        ("import.options", json!({ "excludeDuplicates": false })),
    ] {
        assert_eq!(code(&mut s, id, p), "disabled", "{id}");
    }
    assert_eq!(run(&mut s, "import.close", json!({}))["closed"], false);

    // import.open takes exactly one of device and path; a path is absolute.
    assert_eq!(code(&mut s, "import.open", json!({})), "bad_params");
    assert_eq!(code(&mut s, "import.open", json!({ "device": one, "path": one })), "bad_params");
    assert_eq!(code(&mut s, "import.open", json!({ "path": "relative/folder" })), "bad_params");
    assert_eq!(code(&mut s, "import.open", json!({ "device": mounts.path().join("nowhere") })), "not_found");

    // A new source drops the listing before it.
    let g1 = run(&mut s, "import.open", json!({ "path": one }))["generation"].as_u64().unwrap();
    let g2 = run(&mut s, "import.open", json!({ "path": two }))["generation"].as_u64().unwrap();
    assert_eq!(g2, g1 + 1);
    let (_, end) = wait_listed(&mut s);
    assert_eq!((end["total"].clone(), end["source"]["path"].clone()), (json!(2), json!(two)));
    assert!(candidates(&mut s).iter().all(|c| c["name"].as_str().unwrap().starts_with("B_")), "only the new source's pictures");

    // Hostile values.
    assert_eq!(code(&mut s, "import.list", json!({ "limit": 1001 })), "bad_params");
    assert_eq!(code(&mut s, "import.exclude", json!({ "ids": [999], "on": true })), "not_found");
    assert_eq!(code(&mut s, "import.exclude", json!({ "ids": [], "on": true })), "bad_params");
    assert_eq!(code(&mut s, "import.mark", json!({ "ids": [0] })), "bad_params");
    assert_eq!(code(&mut s, "import.mark", json!({ "ids": [0], "rotate": 4 })), "bad_params");
    assert_eq!(code(&mut s, "import.options", json!({ "excludeDuplicates": "yes" })), "bad_params");
    assert_eq!(code(&mut s, "import.sources", json!({ "zzz": 1 })), "bad_params");

    // A folder that cannot be read says so in the status line.
    run(&mut s, "import.open", json!({ "path": mounts.path().join("missing") }));
    let (_, end) = wait_listed(&mut s);
    assert_eq!(end["total"], 0);
    assert!(end["status"].as_str().unwrap().starts_with("The source cannot be read"), "{end}");

    // Close drops the listing; the generation stays.
    let v = run(&mut s, "import.close", json!({}));
    assert_eq!((v["closed"].clone(), v["generation"].clone()), (json!(true), json!(g2 + 1)));
    assert_eq!(run(&mut s, "import.list", json!({}))["source"], Value::Null);
    assert_eq!(code(&mut s, "import.excludeAll", json!({ "on": true })), "disabled");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
}
