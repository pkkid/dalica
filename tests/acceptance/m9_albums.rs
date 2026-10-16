//! M9's *Albums* in the live app (`m9.rs` part 1): every album command and its undo and redo over
//! `engine.execute`, a reorder by dragging in the album's section, the album in the tree and its
//! section (whole-window fingerprints), and `data-model.md` §10.12: the album's order over three
//! folders survives deleting `albums.json` and the index and rebuilding.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, album, click, idle, quiet, resize, run, section, undo};
use crate::live::{self, Client};

/// The photos of `Trips` (relative paths), each written with its own seed and minute.
pub const TRIPS: [&str; 7] = [
    "Trips/Lisbon/IMG_0001.jpg",
    "Trips/Lisbon/IMG_0002.jpg",
    "Trips/Lisbon/IMG_0003.jpg",
    "Trips/Porto/IMG_0101.jpg",
    "Trips/Porto/IMG_0102.jpg",
    "Trips/Sintra/IMG_0201.jpg",
    "Trips/Sintra/IMG_0202.jpg",
];

/// The album's first order: over the three folders, in no folder's order.
const ORDER: [&str; 5] =
    ["Trips/Sintra/IMG_0202.jpg", "Trips/Lisbon/IMG_0002.jpg", "Trips/Porto/IMG_0101.jpg", "Trips/Lisbon/IMG_0003.jpg", "Trips/Porto/IMG_0102.jpg"];

/// Writes `Trips` into `home/Pictures`; returns `Pictures`.
pub fn trips(home: &Path) -> PathBuf {
    let pictures = home.join("Pictures");
    for (i, rel) in TRIPS.iter().enumerate() {
        let jpeg = app::jpeg(100 + i as u64, (320, 240), "Dalica", "Scenes 1", &format!("2024-06-{:02}T10:{i:02}:00", 3 + i % 3));
        app::put(&app::under(&pictures, rel), &jpeg, i as u64);
    }
    pictures
}

fn ids(c: &mut Client, root: &Path, rels: &[&str]) -> Vec<Value> {
    let all = app::photos(c, root);
    rels.iter().map(|r| all.get(*r).unwrap_or_else(|| panic!("no photo {r}")).id.clone()).collect()
}

fn order(c: &mut Client, root: &Path, a: &str) -> Vec<String> {
    section(c, root, &format!("album:{a}"))
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The album's fields `albums.list` reports that the commands change.
fn fields(c: &mut Client, a: &str) -> Value {
    let e = album(c, a).unwrap_or_else(|| panic!("albums.list lists {a}"));
    json!({ "name": e["name"], "description": e["description"], "place": e["place"], "date": e["date"], "cover": e["cover"], "count": e["count"] })
}

/// Checks the album's order and fields, then Edit ▸ Undo gives back `before` and Edit ▸ Redo
/// `after` again: one undo step each way.
fn undo_redo(c: &mut Client, root: &Path, a: &str, what: &str, before: &(Vec<String>, Value), after: &(Vec<String>, Value)) {
    let now = (order(c, root, a), fields(c, a));
    assert_eq!(&now, after, "{what}");
    undo(c);
    let back = (order(c, root, a), fields(c, a));
    assert_eq!(&back, before, "{what}: Edit ▸ Undo");
    app::redo(c);
    let again = (order(c, root, a), fields(c, a));
    assert_eq!(&again, after, "{what}: Edit ▸ Redo");
    println!("  {what}: undone and redone");
}

/// The members' sidecar entries list the album (`"albums": {id: key}`), and only theirs, once the
/// sidecar writes have landed ([`app::flush_until`]).
fn on_disk(c: &mut Client, root: &Path, a: &str, members: &[String]) {
    let look = |rel: &str| {
        let path = app::under(root, rel);
        let e = app::entry(path.parent().unwrap(), path.file_name().unwrap().to_str().unwrap()).unwrap_or_default();
        (e["albums"].get(a).is_some(), e)
    };
    let member = |rel: &str| members.iter().any(|m| m == rel);
    app::flush_until(c, || TRIPS.iter().all(|rel| look(rel).0 == member(rel)));
    for rel in TRIPS {
        let (listed, e) = look(rel);
        assert_eq!(listed, member(rel), "{rel}'s sidecar entry: {e}");
    }
}

/// Part 1: the album commands and their undo, the reorder by drag, the tree and the section, and
/// §10.12. Returns the album's id.
pub fn albums(exe: &Path, home: &Path) -> String {
    let started = Instant::now();
    let root = trips(home);
    let (app_, mut cl) = live::start(exe, home);
    let c = &mut cl;
    run(c, "folders.add", json!({ "path": root }));
    assert_eq!(app::photos(c, &root).len(), TRIPS.len());

    // album.create, in a custom order over three folders.
    let members = ids(c, &root, &ORDER);
    let r = run(c, "album.create", json!({ "name": "Trip", "ids": members }));
    let a = r["album"].as_str().unwrap_or_else(|| panic!("album.create names the album: {r}")).to_string();
    assert_eq!((r["name"].as_str(), r["count"].as_u64()), (Some("Trip"), Some(5)), "{r}");
    assert_eq!(order(c, &root, &a), strings(&ORDER), "the album keeps the order given");
    let created = (strings(&ORDER), fields(c, &a));
    undo(c);
    assert!(album(c, &a).is_none(), "Edit ▸ Undo removes the new album");
    let first = app::photos(c, &root)[ORDER[0]].inspected.clone();
    assert_eq!(first["albums"], json!([]), "and every membership: {first}");
    app::redo(c);
    assert_eq!((order(c, &root, &a), fields(c, &a)), created, "Edit ▸ Redo brings it back with its id and order");
    println!("  album.create: undone and redone");
    on_disk(c, &root, &a, &created.0);

    // album.info.set.
    let info = json!({ "album": a, "description": "Five days in Portugal", "place": "Portugal", "date": "2024-06-03", "useNewest": false });
    run(c, "album.info.set", info);
    let set = (strings(&ORDER), fields(c, &a));
    assert_eq!((set.1["description"].as_str(), set.1["place"].as_str()), (Some("Five days in Portugal"), Some("Portugal")), "{}", set.1);
    undo_redo(c, &root, &a, "album.info.set", &created, &set);

    // album.add after a member.
    let [one, p101] = ids(c, &root, &["Trips/Lisbon/IMG_0001.jpg", "Trips/Porto/IMG_0101.jpg"]).try_into().unwrap();
    run(c, "album.add", json!({ "album": a, "ids": [one], "after": p101 }));
    let mut added = strings(&ORDER);
    added.insert(3, "Trips/Lisbon/IMG_0001.jpg".into());
    let added = (added, fields(c, &a));
    undo_redo(c, &root, &a, "album.add", &set, &added);

    // album.reorder: the last member before the first.
    let [last, firstm] = ids(c, &root, &["Trips/Porto/IMG_0102.jpg", "Trips/Sintra/IMG_0202.jpg"]).try_into().unwrap();
    run(c, "album.reorder", json!({ "album": a, "ids": [last], "before": firstm }));
    let mut moved = added.0.clone();
    let x = moved.pop().unwrap();
    moved.insert(0, x);
    let reordered = (moved, fields(c, &a));
    undo_redo(c, &root, &a, "album.reorder", &added, &reordered);

    // album.remove: asks first; the file stays.
    let three = ids(c, &root, &["Trips/Lisbon/IMG_0003.jpg"]).remove(0);
    let refused = c.refused("album.remove", json!({ "album": a, "ids": [three] }));
    assert_eq!(refused["code"], "needs_confirm", "Remove from Album asks first: {refused}");
    run(c, "album.remove", json!({ "album": a, "ids": [three], "confirm": true }));
    assert!(app::under(&root, "Trips/Lisbon/IMG_0003.jpg").is_file(), "the file stays");
    let removed = (reordered.0.iter().filter(|r| *r != "Trips/Lisbon/IMG_0003.jpg").cloned().collect(), fields(c, &a));
    undo_redo(c, &root, &a, "album.remove", &reordered, &removed);

    // album.setCover; undone, the cover is the first photo again.
    assert_eq!(fields(c, &a)["cover"], ids(c, &root, &[removed.0[0].as_str()])[0], "the first photo is the cover until one is set");
    run(c, "album.setCover", json!({ "album": a, "id": p101 }));
    let covered = (removed.0.clone(), fields(c, &a));
    assert_eq!(covered.1["cover"], p101);
    undo_redo(c, &root, &a, "album.setCover", &removed, &covered);

    // Sort Album By ▸ File Name, then Reverse Order.
    run(c, "section.sort", json!({ "section": format!("album:{a}"), "by": "name" }));
    let mut by_name = covered.0.clone();
    by_name.sort_by_key(|r| r.rsplit('/').next().unwrap().to_string());
    let sorted = (by_name, fields(c, &a));
    undo_redo(c, &root, &a, "section.sort by name", &covered, &sorted);
    run(c, "section.sort", json!({ "section": format!("album:{a}"), "reverse": true }));
    let reversed = (sorted.0.iter().rev().cloned().collect::<Vec<_>>(), fields(c, &a));
    undo_redo(c, &root, &a, "section.sort reverse", &sorted, &reversed);
    undo(c);
    undo(c);
    assert_eq!((order(c, &root, &a), fields(c, &a)), covered, "the custom order back");

    // album.delete: asks first, keeps the files, undo brings every membership back in order.
    let refused = c.refused("album.delete", json!({ "album": a }));
    assert_eq!(refused["code"], "needs_confirm", "Delete Album asks first: {refused}");
    run(c, "album.delete", json!({ "album": a, "confirm": true }));
    assert!(album(c, &a).is_none(), "deleted");
    assert!(TRIPS.iter().all(|r| app::under(&root, r).is_file()), "its photos stay on disk");
    assert!(app::photos(c, &root).values().all(|p| p.inspected["albums"] == json!([])), "no membership left");
    undo(c);
    assert_eq!((order(c, &root, &a), fields(c, &a)), covered, "Edit ▸ Undo: the album with its order, info and cover");
    app::redo(c);
    assert!(album(c, &a).is_none(), "Edit ▸ Redo deletes it again");
    undo(c);
    println!("  album.delete: undone and redone");

    // A reorder by dragging in the album's section: the photo dropped on the left half of the
    // first cell goes before it, exactly as album.reorder put it.
    click(c, &format!("tree:album:{a}"));
    let target = format!("thumb:{}/in/{a}", ids(c, &root, &[covered.0[0].as_str()])[0]);
    let cell = app::rect_of(c, &target).unwrap_or_else(|| panic!("the album's first cell {target}"));
    let from = format!("thumb:{last}/in/{a}");
    assert!(app::widget(c, &from).is_some(), "the album's last cell {from}");
    c.ok("ui.dragWidget", json!({ "id": from, "toX": cell[0] + cell[2] * 0.2, "toY": cell[1] + cell[3] / 2.0 }));
    idle(c);
    let mut dragged = covered.0.clone();
    let x = dragged.iter().position(|r| r == "Trips/Porto/IMG_0102.jpg").unwrap();
    let x = dragged.remove(x);
    dragged.insert(0, x);
    assert_eq!(order(c, &root, &a), dragged, "the drop put it first");
    undo(c);
    assert_eq!(order(c, &root, &a), covered.0, "Edit ▸ Undo of the drop");
    app::redo(c);
    assert_eq!(order(c, &root, &a), dragged);
    println!("  a reorder by dragging in the album: undone and redone");

    // The album in the tree and its section, at both sizes.
    run(c, "select.none", json!({}));
    click(c, &format!("tree:album:{a}"));
    assert!(c.has(&format!("section:album:{a}")), "the album's section");
    quiet(c, 900.0);
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-albums", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));

    // §10.12: everything but the folders lost, then rebuilt.
    let kept = order(c, &root, &a);
    live::quit(app_, cl);
    let library = home.join("library");
    for name in ["index.log", "index.snap", "albums.json"] {
        match std::fs::remove_file(library.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{name}: {e}"),
        }
    }
    let (app_, mut cl) = live::start(exe, home);
    let c = &mut cl;
    run(c, "library.rebuild", json!({ "confirm": true }));
    idle(c);
    let found = album(c, &a).unwrap_or_else(|| panic!("the album and its id come back from the sidecars"));
    assert_eq!(found["name"], "Trip", "its name from the folders' sidecars: {found}");
    assert_eq!(order(c, &root, &a), kept, "data-model.md §10.12: the exact order after a rebuild");
    live::quit(app_, cl);
    println!("§10.12: the album over three folders keeps its order {kept:?} after albums.json and the index were deleted and rebuilt");
    println!("M9 albums in the live app: passed in {:.1} s", app::secs(started));
    a
}
