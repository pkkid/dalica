//! `Session::execute_group` (`roadmap.md` M10.1): several library commands as one library undo
//! step. A star, a caption and a tag are one step that one undo takes back; a step failing part-way
//! undoes the steps before it, leaving the model, the sidecars, the photo files and the undo stack
//! as they were; a
//! step refused before the group starts runs nothing.

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use serde_json::{Value, json};

use crate::common::{jpeg, lisbon, read};

/// What the model says about `id`: its star, caption and tags.
fn said(s: &Session, id: PhotoId) -> (bool, String, Vec<String>) {
    let info = s.library().info(id);
    (s.library().photo(id).unwrap().star, info.caption.clone(), info.tags.clone())
}

fn group(s: &mut Session, id: PhotoId, third: Value) -> Result<Vec<Value>, CmdError> {
    let steps = [
        ("photo.star.set", json!({ "ids": [id], "on": true })),
        ("photo.caption.set", json!({ "ids": [id], "caption": "Tram 28 at dusk" })),
        ("photo.tags.add", third),
    ];
    s.execute_group(&steps, "Set Metadata")
}

#[test]
fn a_star_a_caption_and_a_tag_are_one_undo_step() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2)], None);
    let before = said(&s, ids[0]);
    let steps = s.undo_stack().len();
    let results = group(&mut s, ids[0], json!({ "ids": [ids[0]], "tags": "lisbon, tram" })).unwrap();
    assert_eq!(results.len(), 3, "one result per step");
    assert_eq!(said(&s, ids[0]), (true, "Tram 28 at dusk".into(), vec!["lisbon".into(), "tram".into()]));
    assert_eq!(s.undo_stack().len(), steps + 1, "one step");
    assert_eq!(s.undo_stack().undo_label(), Some("Set Metadata"));

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(said(&s, ids[0]), before, "one undo restores all three");
    assert_eq!(s.undo_stack().redo_label(), Some("Set Metadata"));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(said(&s, ids[0]).0);
    s.flush().unwrap();
    let sidecar: Value = serde_json::from_slice(&read(&folder.join(".dalica.json"))).unwrap();
    assert_eq!(sidecar["photos"]["IMG_0001.jpg"]["caption"], json!("Tram 28 at dusk"), "{sidecar}");
}

#[test]
fn a_failing_third_step_leaves_the_model_the_sidecars_and_the_undo_stack_unchanged() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2)], None);
    // A sidecar on disk to compare with, and a step on the stack that must stay on top.
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    s.flush().unwrap();
    assert!(s.flush_metadata(std::time::Duration::from_secs(60)));
    let sidecar = read(&folder.join(".dalica.json"));
    let photo = read(&folder.join("IMG_0001.jpg"));
    let before = (said(&s, ids[0]), said(&s, ids[1]));
    let steps = s.undo_stack().len();

    // The third step passes its check (it names photos) but fails as it runs: the photo is unknown.
    let err = group(&mut s, ids[0], json!({ "ids": [9_999_999], "tags": "tram" })).unwrap_err();
    assert!(matches!(&err, CmdError::NotFound(m) if m.starts_with("step 3 (photo.tags.add): ")), "{err:?}");
    assert_eq!((said(&s, ids[0]), said(&s, ids[1])), before, "the star and the caption were taken back");
    assert_eq!(s.undo_stack().len(), steps, "no step recorded");
    assert_eq!(s.undo_stack().undo_label(), Some("Set Star"));
    s.flush().unwrap();
    assert_eq!(read(&folder.join(".dalica.json")), sidecar, "the sidecar is as it was");
    assert!(s.flush_metadata(std::time::Duration::from_secs(60)));
    assert_eq!(read(&folder.join("IMG_0001.jpg")), photo, "and so is the photo file");
}

#[test]
fn a_step_refused_before_the_group_starts_runs_nothing() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let before = said(&s, ids[0]);
    // Disabled (no photos named), not a library command, unknown.
    let refused = [
        (json!({ "ids": [], "tags": "tram" }), "photo.tags.add", "disabled"),
        (json!({ "ids": [ids[0]] }), "photo.save", "bad_params"),
        (json!({}), "no.such.command", "unknown_command"),
    ];
    for (params, id, code) in refused {
        let steps = [("photo.star.set", json!({ "ids": [ids[0]], "on": true })), (id, params)];
        let err = s.execute_group(&steps, "Set Metadata").unwrap_err();
        assert_eq!(err.code(), code, "{id}: {err}");
        assert_eq!(said(&s, ids[0]), before, "{id}: the first step never ran");
    }
    assert!(s.undo_stack().is_empty());
    let many = vec![("photo.star.set", json!({ "ids": [ids[0]], "on": true })); dalica_engine::session::MAX_GROUP_STEPS + 1];
    assert_eq!(s.execute_group(&many, "Set Metadata").unwrap_err().code(), "limit");
}
