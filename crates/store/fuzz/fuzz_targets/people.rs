//! Fuzz target `people`: any bytes as the library's `people.json` (`security.md` §4.2, §7.1,
//! `faces.md` §6). Whatever the input: refused as damaged (not JSON, not an object, past a limit),
//! or read; then
//!
//! - print → parse gives the same document, and printing it again gives the same bytes;
//! - every person read has a name, and every rejected suggestion a rectangle inside the picture;
//! - writing back what was read (people, legacy ids and rejected suggestions) reads back the same
//!   people, legacy ids and suggestions, keeps every top-level key Dalica does not know, and
//!   writing it back a second time changes nothing.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_store::people::{self, PEOPLE, PeopleState, REJECTED};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(root) = people::parse(data) else { return };
    let printed = people::print(&root);
    let again = people::parse(&printed).unwrap_or_else(|e| panic!("a printed people.json does not read: {e}"));
    assert_eq!(again, root, "print → parse changed the document");
    assert_eq!(people::print(&again), printed, "printing is not stable");

    let read = people::reading(&root);
    for p in read.people.values() {
        assert!(!p.name.is_empty());
    }
    for r in &read.rejected {
        let (x0, y0, x1, y1) = (r.rect.x0(), r.rect.y0(), r.rect.x1(), r.rect.y1());
        assert!([x0, y0, x1, y1].iter().all(|v| (0.0..=1.0).contains(v)) && x0 <= x1 && y0 <= y1);
    }

    let state = PeopleState { people: read.people.clone(), rejected: Some(read.rejected.clone()), legacy: read.legacy.clone() };
    let once = people::merged(Some(&root), &state);
    let back = people::reading(&once);
    assert_eq!((&back.people, &back.rejected, &back.legacy), (&read.people, &read.rejected, &read.legacy), "writing back what was read changed it");
    for (k, v) in root.iter().filter(|(k, _)| k.as_str() != PEOPLE && k.as_str() != REJECTED && k.as_str() != "dalica") {
        assert_eq!(once.get(k), Some(v), "the unknown key {k:?} changed");
    }
    let twice = people::merged(Some(&once), &state);
    assert_eq!(twice, once, "writing back twice changed the document");
    let reread = people::parse(&people::print(&once)).unwrap_or_else(|e| panic!("a written people.json does not read: {e}"));
    assert_eq!(reread, once);
});
