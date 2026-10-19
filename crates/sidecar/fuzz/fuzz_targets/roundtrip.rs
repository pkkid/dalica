//! Fuzz target `roundtrip`: any bytes as a folder's `.dalica.json` (`security.md` §4.2, §7.1)
//! that reads is printed and read again, then every value Dalica reads is written back the way the
//! writer writes it. Whatever the input:
//!
//! - print → parse gives the same document, and printing it again gives the same bytes;
//! - writing back what was read reads back the same (an empty value reads as no key, which is
//!   what writing it means), and writing back a second time changes nothing;
//! - every key Dalica does not read, at the top level, in `"folder"` and in each photo's entry,
//!   is still there with the same value (unknown data preserved);
//! - the same holds for the originals record and `albums.json` (print → parse is stable).
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_sidecar::fields::{ALBUMS, CHANGED, FACES, FOLDER, FOLDER_KEYS, GEO, M8_FOLDER_KEYS, M8_PHOTO_KEYS, M9_FOLDER_KEYS, PEOPLE, PHOTO_KEYS};
use dalica_sidecar::{AlbumsFile, Field, FolderValue, Limits, Originals, PhotoFields, PhotoValue, Sidecar};
use libfuzzer_sys::fuzz_target;
use serde_json::{Map, Value};

/// A field read with an empty value as what writing it leaves: no key.
fn empty_as_absent<T>(f: Field<T>, empty: impl Fn(&T) -> bool) -> Field<T> {
    match f {
        Field::Value(v) if empty(&v) => Field::Absent,
        f => f,
    }
}

fn photo_norm(p: PhotoFields) -> PhotoFields {
    PhotoFields {
        star: empty_as_absent(p.star, |v| !v),
        caption: empty_as_absent(p.caption, String::is_empty),
        tags: empty_as_absent(p.tags, Vec::is_empty),
        hidden: empty_as_absent(p.hidden, |v| !v),
        edits: empty_as_absent(p.edits, Vec::is_empty),
        redo: empty_as_absent(p.redo, Vec::is_empty),
        date: p.date,
        changed: p.changed,
    }
}

/// Writes back every value Dalica reads (refusals change nothing and are ignored).
fn write_back(s: &mut Sidecar) {
    let names: Vec<String> = s.photo_names().map(str::to_owned).collect();
    for name in &names {
        let p = s.photo(name);
        let mut values = Vec::new();
        if let Field::Value(v) = p.star {
            values.push(PhotoValue::Star(v));
        }
        if let Field::Value(v) = p.caption {
            values.push(PhotoValue::Caption(v));
        }
        if let Field::Value(v) = p.tags {
            values.push(PhotoValue::Tags(v));
        }
        if let Field::Value(v) = p.hidden {
            values.push(PhotoValue::Hidden(v));
        }
        if let Field::Value(v) = p.edits {
            values.push(PhotoValue::Edits(v));
        }
        if let Field::Value(v) = p.redo {
            values.push(PhotoValue::Redo(v));
        }
        if let Field::Value(v) = p.date {
            values.push(PhotoValue::Date(Some(v)));
        }
        if let Field::Value(v) = p.changed {
            values.push(PhotoValue::Changed(Some(v)));
        }
        for v in &values {
            let _ = s.set_photo(name, v);
        }
        if let Field::Value(v) = s.saved_as(name) {
            let _ = s.set_saved_as(name, Some(&v));
        }
        if let Field::Value(v) = s.photo_albums(name) {
            let _ = s.set_photo_albums(name, &v.members, |_| true);
        }
        if let Field::Value(v) = s.photo_faces(name) {
            let _ = s.set_photo_faces(name, &v.faces);
        }
        if let Field::Value(v) = s.geo(name) {
            let _ = s.set_geo(name, Some(v));
        }
    }
    let f = s.folder();
    if let Field::Value(v) = f.description {
        let _ = s.set_folder(&FolderValue::Description(v));
    }
    if let Field::Value(v) = f.date {
        let _ = s.set_folder(&FolderValue::Date(Some(v)));
    }
    if let Field::Value(v) = f.place {
        let _ = s.set_folder(&FolderValue::Place(v));
    }
    if let Field::Value(v) = f.hidden {
        let _ = s.set_folder(&FolderValue::Hidden(v));
    }
    if let Field::Value(v) = s.project() {
        let _ = s.set_project(Some(&v));
    }
    if let Field::Value(v) = s.folder_sort() {
        let _ = s.set_folder_sort(Some(&v));
    }
    if let Field::Value(v) = s.folder_reverse() {
        let _ = s.set_folder_reverse(v);
    }
    if let Field::Value(v) = s.album_names() {
        s.set_album_names(&v.names);
    }
    if let Field::Value(v) = s.people() {
        s.set_people(&v.people);
    }
}

fn root_of(s: &Sidecar) -> Map<String, Value> {
    match serde_json::from_slice(&s.print()) {
        Ok(Value::Object(m)) => m,
        other => panic!("a printed sidecar is not a JSON object: {other:?}"),
    }
}

/// Every key of `before` not in `known` is in `after` with the same value.
fn unknown_kept(before: Option<&Value>, after: Option<&Value>, known: &[&str], at: &str) {
    let Some(Value::Object(before)) = before else { return };
    let after = after.and_then(Value::as_object);
    for (k, v) in before.iter().filter(|(k, _)| !known.contains(&k.as_str())) {
        assert_eq!(after.and_then(|a| a.get(k)), Some(v), "{at}/{k} changed");
    }
}

fn reads_same(a: &Sidecar, b: &Sidecar) {
    let names: Vec<String> = a.photo_names().map(str::to_owned).collect();
    for name in &names {
        assert_eq!(photo_norm(a.photo(name)), photo_norm(b.photo(name)), "photo {name:?}");
        assert_eq!(a.geo(name), b.geo(name), "geo of {name:?}");
        if let (Field::Value(x), Field::Value(y)) = (a.photo_faces(name), b.photo_faces(name)) {
            assert_eq!(x.faces, y.faces, "faces of {name:?}");
            assert_eq!(x.over, y.over, "faces past the cap of {name:?}");
        }
        if let (Field::Value(x), Field::Value(y)) = (a.photo_albums(name), b.photo_albums(name)) {
            assert_eq!(x.members, y.members, "albums of {name:?}");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    if let Ok(o) = Originals::parse(data, &limits) {
        let printed = o.print();
        let again = Originals::parse(&printed, &limits).expect("a printed originals record reads");
        assert_eq!(again, o);
        assert_eq!(again.print(), printed);
    }
    if let Ok(a) = AlbumsFile::parse(data, &limits) {
        let printed = a.print();
        let again = AlbumsFile::parse(&printed, &limits).expect("a printed albums.json reads");
        assert_eq!(again, a);
        assert_eq!(again.print(), printed);
    }
    let Ok(s) = Sidecar::parse(data, &limits) else { return };
    let printed = s.print();
    let t = Sidecar::parse(&printed, &limits).expect("a printed sidecar reads");
    assert_eq!(t, s, "print → parse changed the document");
    assert_eq!(t.print(), printed, "printing is not stable");

    let mut once = s.clone();
    write_back(&mut once);
    reads_same(&s, &once);
    let mut twice = once.clone();
    write_back(&mut twice);
    assert_eq!(twice.print(), once.print(), "writing back twice changed the document");
    let reread = Sidecar::parse(&once.print(), &limits).expect("a written sidecar reads");
    assert_eq!(reread, once);

    let (before, after) = (root_of(&s), root_of(&once));
    let top = ["dalica", "photos", FOLDER, ALBUMS, PEOPLE];
    unknown_kept(Some(&Value::Object(before.clone())), Some(&Value::Object(after.clone())), &top, "");
    let mut folder_known: Vec<&str> = FOLDER_KEYS.to_vec();
    folder_known.extend(M8_FOLDER_KEYS);
    folder_known.extend(M9_FOLDER_KEYS);
    unknown_kept(before.get(FOLDER), after.get(FOLDER), &folder_known, "/folder");
    let mut photo_known: Vec<&str> = PHOTO_KEYS.to_vec();
    photo_known.extend(M8_PHOTO_KEYS);
    photo_known.extend([ALBUMS, CHANGED, FACES, GEO]);
    if let Some(Value::Object(photos)) = before.get("photos") {
        for (name, entry) in photos {
            let later = after.get("photos").and_then(|p| p.get(name));
            unknown_kept(Some(entry), later, &photo_known, name);
        }
    }
});
