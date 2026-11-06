//! `folder.info.set` on a temporary library (`roadmap.md` M6.14; `organizing.md` §10,
//! `data-model.md` §3): only the fields given change; texts trimmed and capped, "" clears; a date
//! or the oldest photo's (`"oldest"` / `null`); the hidden flag left alone; one undo step and none
//! when nothing changes; undo and redo exact; the sidecar's `"folder"` object after the command
//! and after undo; folder info surviving a restart and a rebuild of the index; a folder's music
//! (M16.16) set, cleared, refused when it names a file or nothing, undone.

use std::path::{Path, PathBuf};

use crate::common::{jpeg, lisbon, open};
use dalica_core::{Date, FolderId};
use dalica_engine::Session;
use dalica_library::{FolderInfo, MAX_DESCRIPTION, MAX_PLACE};
use serde_json::{Value, json};

/// A library watching one folder `Lisbon` of two photos.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, FolderId) {
    let (lib, disk, folder, s, _) = lisbon(&[jpeg(0), jpeg(1)], None);
    let id = s.library().folder_by_path(&folder).unwrap();
    (lib, disk, folder, s, id)
}

/// The folder's sidecar after every pending write landed without its `"dalica"` header (an empty object when there is no file:
/// a file left with only the header says the same as none).
fn sidecar(s: &mut Session, folder: &Path) -> Value {
    s.flush().unwrap();
    let mut v: Value = std::fs::read(folder.join(".dalica.json")).map_or(json!({}), |b| serde_json::from_slice(&b).unwrap());
    if let Some(o) = v.as_object_mut() {
        o.remove("dalica");
    }
    v
}

fn info(s: &Session, id: FolderId) -> FolderInfo {
    s.library().folder_info(id).clone()
}

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn only_the_fields_given_change_and_undo_redo_restore_model_and_sidecar() {
    let (_l, _d, folder, mut s, id) = setup();
    let (m0, file0) = (s.library().model().clone(), sidecar(&mut s, &folder));
    let v = s
        .execute(
            "folder.info.set",
            &json!({ "id": id, "description": "  Weekend in Lisbon with the family \n", "place": "Lisbon,\tPortugal ", "date": "2026-06-04" }),
        )
        .unwrap();
    assert_eq!(
        v,
        json!({ "id": id, "description": "Weekend in Lisbon with the family", "date": "2026-06-04", "place": "Lisbon, Portugal", "music": "", "changed": true })
    );
    assert_eq!((s.undo_stack().len(), s.undo_stack().undo_label()), (1, Some("Folder Properties")));
    let file1 = sidecar(&mut s, &folder);
    assert_eq!(file1["folder"], json!({ "description": "Weekend in Lisbon with the family", "date": "2026-06-04", "place": "Lisbon, Portugal" }));
    let m1 = s.library().model().clone();

    // The same values again: nothing changes, no step.
    let again = json!({ "id": id, "description": "Weekend in Lisbon with the family", "place": "Lisbon, Portugal", "date": "2026-06-04" });
    assert_eq!(s.execute("folder.info.set", &again).unwrap()["changed"], json!(false));
    assert_eq!(s.undo_stack().len(), 1);

    // Only the description: the place and the date stay.
    s.execute("folder.info.set", &json!({ "id": id, "description": "Trams" })).unwrap();
    assert_eq!(
        info(&s, id),
        FolderInfo {
            description: "Trams".into(),
            date: Some(day("2026-06-04")),
            place: "Lisbon, Portugal".into(),
            hidden: false,
            music: String::new()
        }
    );
    // "oldest" goes back to the oldest photo's date; "" clears a text.
    s.execute("folder.info.set", &json!({ "id": id, "date": "oldest", "place": "" })).unwrap();
    assert_eq!(info(&s, id), FolderInfo { description: "Trams".into(), ..FolderInfo::default() });
    s.execute("folder.info.set", &json!({ "id": id, "date": "2025-12-31" })).unwrap();
    s.execute("folder.info.set", &json!({ "id": id, "date": null, "description": "" })).unwrap();
    assert!(info(&s, id).is_empty());
    assert_eq!(sidecar(&mut s, &folder)["folder"], Value::Null, "empty info leaves no folder object");
    assert_eq!(s.undo_stack().len(), 5);

    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(s.library().model(), &m1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m0);
    assert_eq!(sidecar(&mut s, &folder), file0, "after undo the sidecar is as it was");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    assert_eq!(sidecar(&mut s, &folder), file1);
}

#[test]
fn the_hidden_flag_is_left_alone_and_texts_are_capped() {
    let (_l, _d, _f, mut s, id) = setup();
    s.execute("folder.hide", &json!({ "section": format!("folder:{id}") })).unwrap();
    s.execute("folder.info.set", &json!({ "id": id, "description": "x".repeat(MAX_DESCRIPTION + 10), "place": "ü".repeat(MAX_PLACE) })).unwrap();
    let i = info(&s, id);
    assert!(i.hidden, "Folder Properties never unhides");
    assert_eq!(i.description.len(), MAX_DESCRIPTION);
    assert!(i.place.len() <= MAX_PLACE && i.place.chars().all(|c| c == 'ü'), "cut at a character boundary");
    // A place is one line: control characters become spaces.
    s.execute("folder.info.set", &json!({ "id": id, "place": "Lisbon\nPortugal" })).unwrap();
    assert_eq!(info(&s, id).place, "Lisbon Portugal");
}

#[test]
fn folder_info_survives_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, folder, mut s, id) = setup();
    s.execute("folder.info.set", &json!({ "id": id, "description": "Weekend 🚋", "place": "Lisbon", "date": "2026-06-04" })).unwrap();
    let want = info(&s, id);
    let written = sidecar(&mut s, &folder)["folder"].clone();
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(info(&s, id), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let id = s.library().folder_by_path(&folder).unwrap();
    let photo = s.library().photos().next().unwrap().id;
    s.execute("photo.star.set", &json!({ "ids": [photo], "on": true })).unwrap();
    assert_eq!(sidecar(&mut s, &folder)["folder"], written, "a write after a rebuild keeps the folder info in the sidecar");
    assert_eq!(info(&s, id), want, "after a rebuild of the index");
}

/// The music (M16.16): a folder is set (trimmed) with the other fields in one step and written to
/// the sidecar, "" clears it, a file or a path that is not there is refused with nothing changed,
/// and undo takes each step back.
#[test]
fn a_folders_music_is_set_cleared_refused_for_a_file_and_undone() {
    let (_l, disk, folder, mut s, id) = setup();
    let music = disk.path().join("Music");
    std::fs::create_dir(&music).unwrap();
    std::fs::write(music.join("song.wav"), b"RIFF").unwrap();
    let text = music.to_str().unwrap().to_string();
    let v = s.execute("folder.info.set", &json!({ "id": id, "music": format!(" {text} "), "place": "Lisbon" })).unwrap();
    assert_eq!((v["music"].as_str(), v["changed"].as_bool()), (Some(text.as_str()), Some(true)));
    assert_eq!((info(&s, id).music, info(&s, id).place), (text.clone(), "Lisbon".to_string()));
    assert_eq!(s.undo_stack().len(), 1, "one step with the place");
    assert_eq!(sidecar(&mut s, &folder)["folder"]["music"], json!(text));

    for bad in [music.join("song.wav"), disk.path().join("Gone")] {
        let e = s.execute("folder.info.set", &json!({ "id": id, "music": bad, "description": "x" }));
        assert!(matches!(e, Err(dalica_engine::CmdError::BadParams(_))), "{bad:?}: {e:?}");
    }
    assert_eq!((info(&s, id).music, info(&s, id).description, s.undo_stack().len()), (text.clone(), String::new(), 1), "nothing changed");

    s.execute("folder.info.set", &json!({ "id": id, "music": "" })).unwrap();
    assert_eq!((info(&s, id).music, s.undo_stack().len()), (String::new(), 2));
    assert_eq!(sidecar(&mut s, &folder)["folder"].get("music"), None, "cleared: no key");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(info(&s, id).music, text);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(info(&s, id).is_empty());
}
