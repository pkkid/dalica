//! "Delete copied files from the card" (`roadmap.md` M11.8; `organizing.md` §3, `security.md`
//! §5.7) on a temporary card: only the files the import copied and verified leave the card (one
//! excluded and one changed since its copy stay), never without `confirm`, never over automation,
//! and afterwards Undo Import is disabled with its reason. The owner thread is marked as the UI
//! thread while the commands run: it never touches the disk.

use crate::common::{exists, files, off_owner, read, shared, wait_files};
use crate::import_run::{candidates, code, import, open_card, put_all, run, session_on, shot};
use dalica_engine::automation::{Policy, Refusal};
use serde_json::json;

#[test]
fn removal_takes_exactly_the_verified_copies_asks_first_is_never_automated_and_disables_undo() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("CARD").join("DCIM");
    let roll = dcim.join("100CANON");
    put_all(&dcim, (0..6u64).map(|i| (format!("100CANON/IMG_{i}.JPG"), shot(i, i as i64 * 60))).collect());
    let folder = pictures.path().join("2026-06-04");

    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(code(&mut s, "import.removeFromSource", json!({ "confirm": true })), "disabled", "nothing imported yet");
    open_card(&mut s, &dcim);
    let excluded = candidates(&mut s).into_iter().find(|c| c["name"] == "IMG_5.JPG").unwrap()["id"].clone();
    run(&mut s, "import.exclude", json!({ "ids": [excluded], "on": true }));
    let status = import(&mut s, json!({ "destination": pictures.path(), "folderName": { "dateTaken": true } }));
    assert_eq!((status["imported"].clone(), status["removable"].clone()), (json!(5), json!(true)), "{status}");
    drop(ui);

    // One file changes on the card after its copy.
    let changed = roll.join("IMG_4.JPG");
    let other = shot(77, 0);
    let target = changed.clone();
    off_owner(move || std::fs::write(target, other).unwrap());

    let ui = dalica_fs::mark_ui_thread();
    let e = s.execute("import.removeFromSource", &json!({})).unwrap_err();
    assert_eq!(e.code(), "disabled");
    assert!(e.to_string().starts_with("Remove the imported files from the card? This cannot be undone."), "the question: {e}");
    let spec = *s.spec("import.removeFromSource").unwrap();
    let mut policy = Policy::default();
    assert_eq!(policy.check_effect(spec.effect, spec.id).unwrap_err().kind, Refusal::ExternalDenied, "refused over automation");
    policy.set_allow_external(true);
    assert!(policy.check_effect(spec.effect, spec.id).is_err(), "whatever automation was granted");
    assert_eq!(files(&roll).len(), 6, "nothing left the card yet");

    let v = run(&mut s, "import.removeFromSource", json!({ "confirm": true }));
    assert_eq!(v["total"], 5);
    wait_files(&mut s);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    assert_eq!(files(&roll), ["IMG_4.JPG", "IMG_5.JPG"], "the changed and the excluded file stay on the card");
    assert_eq!(files(&folder), ["IMG_0.JPG", "IMG_1.JPG", "IMG_2.JPG", "IMG_3.JPG", "IMG_4.JPG"], "every copy stays");
    assert_eq!(read(&folder.join("IMG_0.JPG")), shot(0, 0));
    assert!(exists(&changed));
    let status = run(&mut s, "import.status", json!({}));
    let removal = &status["removal"];
    assert_eq!((removal["running"].clone(), removal["removed"].clone()), (json!(false), json!(4)), "{status}");
    assert_eq!(removal["kept"].as_array().unwrap().len(), 1, "{status}");
    assert_eq!(removal["kept"][0]["file"], json!(changed));

    assert_eq!(status["undoBlocked"], "The originals were removed from the card.");
    let e = s.execute("edit.undo", &json!({})).unwrap_err();
    assert_eq!((e.code(), e.to_string()), ("disabled", "The originals were removed from the card.".to_string()));
    assert_eq!(s.undo_stack().undo_blocked().as_deref(), Some("The originals were removed from the card."));
    assert_eq!(files(&folder).len(), 5, "the copies are all still there");
    // Asking again removes nothing more: the changed file is still refused.
    let v = run(&mut s, "import.removeFromSource", json!({ "confirm": true }));
    assert_eq!(v["total"], 1);
    wait_files(&mut s);
    assert!(exists(&changed), "a file changed since its copy never leaves the card");
}

#[test]
fn a_folder_import_is_never_removed_from_its_source() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let source = pictures.path().join("Scans");
    put_all(&source, vec![("a.JPG".into(), shot(1, 0))]);
    run(&mut s, "import.openSource", json!({ "path": source }));
    let start = std::time::Instant::now();
    while run(&mut s, "import.list", json!({ "limit": 0 }))["state"] == "loading" {
        assert!(start.elapsed() < std::time::Duration::from_secs(120));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let status = import(&mut s, json!({ "destination": pictures.path().join("Photos"), "folderName": { "today": true } }));
    assert_eq!((status["imported"].clone(), status["removable"].clone()), (json!(1), json!(false)), "{status}");
    let e = s.execute("import.removeFromSource", &json!({ "confirm": true })).unwrap_err();
    assert_eq!(e.code(), "disabled");
    assert!(exists(&source.join("a.JPG")));
}
