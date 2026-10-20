//! Fuzz target `parse`: any bytes as each of the three documents this crate reads (`security.md`
//! §4.2, §7.1): a folder's `.dalica.json`, a folder's originals record and the library's
//! `albums.json`, under the default limits and under small ones (so every limit is reached).
//! Whatever the input: refused by a named limit or as not JSON, or read; then every typed read
//! returns: every photo's keys, the folder's, the saved copies, the albums, the faces, the geotags
//! and the people; every face rectangle is inside the picture with its corners in order, every
//! geotag on the globe, every problem a JSON pointer, every usable originals entry under a plain
//! name. Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_sidecar::fields::GeoTag;
use dalica_sidecar::{AlbumsFile, Field, Limits, Originals, Sidecar, plain_name};
use libfuzzer_sys::fuzz_target;

fn small() -> Limits {
    Limits { max_bytes: 1 << 16, max_depth: 6, max_string: 256, max_photos: 16, max_keys_per_entry: 8, max_list: 16 }
}

fn rect_ok(r: [f64; 4]) -> bool {
    r.iter().all(|v| (0.0..=1.0).contains(v)) && r[0] <= r[2] && r[1] <= r[3]
}

fn sidecar(s: &Sidecar) {
    let _ = s.version();
    for p in s.problems() {
        assert!(p.pointer.starts_with('/'), "{p:?}");
    }
    let _ = s.folder();
    let _ = (s.project(), s.folder_sort(), s.folder_reverse(), s.album_names());
    if let Field::Value(people) = s.people() {
        assert!(people.people.values().all(|p| !p.name.is_empty() && p.nickname.as_ref().is_none_or(|n| !n.is_empty())));
    }
    let names: Vec<String> = s.photo_names().map(str::to_owned).collect();
    for name in &names {
        let _ = s.photo(name);
        let _ = (s.saved_as(name), s.photo_albums(name));
        if let Field::Value(faces) = s.photo_faces(name) {
            assert!(faces.faces.len() <= dalica_sidecar::fields::MAX_FACES);
            assert!(faces.faces.iter().all(|f| rect_ok(f.rect.corners())));
        }
        if let Field::Value(GeoTag::At(g)) = s.geo(name) {
            assert!((-90.0..=90.0).contains(&g.lat()) && (-180.0..=180.0).contains(&g.lon()));
        }
    }
}

fn originals(o: &Originals) {
    let _ = o.version();
    let _ = o.is_empty();
    for p in o.problems() {
        assert!(p.pointer.starts_with('/'), "{p:?}");
    }
    for name in o.names() {
        let _ = o.entry(name);
    }
    for (name, _) in o.entries() {
        assert!(plain_name(&name), "{name:?}");
    }
}

fn albums(a: &AlbumsFile) {
    let _ = a.version();
    let _ = a.ids();
    let _ = a.legacy_ids();
    for p in a.problems() {
        assert!(p.pointer.starts_with('/'), "{p:?}");
    }
    for album in a.albums().albums.values() {
        assert!(!album.name.is_empty());
    }
}

fuzz_target!(|data: &[u8]| {
    for limits in [Limits::default(), small()] {
        if let Ok(s) = Sidecar::parse(data, &limits) {
            sidecar(&s);
        }
        if let Ok(o) = Originals::parse(data, &limits) {
            originals(&o);
        }
        if let Ok(a) = AlbumsFile::parse(data, &limits) {
            albums(&a);
        }
    }
});
