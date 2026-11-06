//! The Edited filter and `is:edited` (M16.34, `organizing.md` §8.1 and §8.2) through the engine's
//! commands on a temporary folder: a mixed library (photos with unsaved edits, a saved photo, a
//! starred one, untouched ones) filters to exactly the photos with unsaved edits, the toggle and
//! the keyword agree and combine with the other filters and the text, `library.query` takes the
//! same filter, and once the folder's counted edits are discarded (the section header's Discard:
//! `edit.undoAll` over `Library::unsaved_in`) no photo is left; one Undo brings them back.

use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::Session;
use serde_json::{Value, json};

use crate::common::{jpeg, lisbon};

/// The current search's matches, once the worker has answered.
fn matches(s: &mut Session, params: Value) -> Vec<PhotoId> {
    s.execute("search.set", &params).unwrap();
    s.wait_search();
    let mut ids = s.search_state().result.as_ref().map(|r| r.photos().to_vec()).unwrap_or_default();
    ids.sort();
    ids
}

#[test]
fn the_edited_filter_finds_exactly_the_unsaved_edits_and_none_after_discard() {
    let photos: Vec<Vec<u8>> = (1..=5).map(jpeg).collect();
    let (_lib, _disk, folder, mut s, ids) = lisbon(&photos, None);
    let [a, b, saved, starred, _plain] = [ids[0], ids[1], ids[2], ids[3], ids[4]];
    // Photo 3 is edited then saved: its edits are in its file, none left unsaved.
    s.execute("photo.history.apply", &json!({ "id": saved, "op": "autofix()" })).unwrap();
    let reply = s.execute("photo.save", &json!({ "ids": [saved], "confirm": true })).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    assert!(s.wait_job(job, Duration::from_secs(120)).unwrap().state.is_finished());
    assert!(s.library().saved(saved).is_some() && !s.library().has_unsaved_edits(saved));
    s.execute("photo.history.apply", &json!({ "id": a, "op": "autofix()" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": b, "op": "rotate(90)" })).unwrap();
    s.execute("photo.star.set", &json!({ "ids": [a, starred], "on": true })).unwrap();

    let edited = vec![a, b];
    let toggle = matches(&mut s, json!({ "filters": { "edited": true } }));
    assert_eq!(toggle, edited, "the toggle: exactly the photos with unsaved edits");
    assert_eq!(s.search_inspect()["filters"]["edited"], json!(true));
    // With another filter, and with text.
    assert_eq!(matches(&mut s, json!({ "filters": { "starred": true } })), vec![a], "edited and starred");
    assert_eq!(matches(&mut s, json!({ "text": "IMG_0002", "filters": { "starred": false } })), vec![b], "edited and the text");
    s.execute("search.clear", &json!({})).unwrap();
    // The keyword gives the same set, and negated the rest.
    assert_eq!(matches(&mut s, json!({ "text": "is:edited" })), edited);
    assert_eq!(matches(&mut s, json!({ "text": "-is:edited" })), vec![saved, starred, ids[4]]);
    let q = s.execute("library.query", &json!({ "filters": { "edited": true } })).unwrap();
    let mut queried: Vec<PhotoId> = serde_json::from_value(q["photos"].as_array().unwrap().iter().map(|p| p["id"].clone()).collect()).unwrap();
    queried.sort();
    assert_eq!(queried, edited, "library.query takes the filter too");

    // Discard the folder's counted edits: the search follows and finds none.
    let fid = s.library().folder_by_path(&folder).unwrap();
    let counted = s.library().unsaved_in(fid);
    assert_eq!(counted.len(), 2);
    s.execute("edit.undoAll", &json!({ "ids": counted, "confirm": true })).unwrap();
    assert!(matches(&mut s, json!({ "text": "", "filters": { "edited": true } })).is_empty(), "no edited photo left");
    assert!(s.library().saved(saved).is_some(), "the saved photo keeps its save");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(matches(&mut s, json!({ "filters": { "edited": true } })), edited, "one Undo brings both back");
    s.execute("search.clear", &json!({})).unwrap();
    assert!(!s.search_state().active(), "Show All clears it");
}
