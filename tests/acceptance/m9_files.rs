//! M9's *File operations* in the live app (`m9.rs` part 3), on temporary folders, with the app's
//! OS trash inside its temporary home on Linux:
//!
//! - `data-model.md` §10.3: photos moved with `photo.moveTo` carry their star, caption, tags,
//!   recipe, album places, sidecar entries and their `.dalica-originals/` backup; undo and redo;
//! - the same through the window: a thumbnail dragged onto a tree folder (the drag pill held over
//!   it, `fingerprints/m9-*-drag.json`), the first-time question (`m9-*-move.json`) with "Don't
//!   ask again" ticked, so the next drag moves without asking; Ctrl+Z;
//! - copies and their undo, the collision question from Copy and Paste (`m9-*-collision.json`)
//!   and Rename Duplicates; Move to New Folder; Rename Files from F2 (`m9-*-rename.json`) and a
//!   single rename; a folder renamed and moved; Delete from Disk from the Delete key
//!   (`m9-*-delete.json`) to the trash and back (where the system restores, else Undo says why);
//!   a folder deleted with its non-picture files; each with its undo;
//! - §10.7: hard names through move, copy, rename and delete and the undo of each;
//! - §10.2: a folder moved outside Dalica while it watches and between two sessions keeps its id,
//!   its photos' ids, their data and their album order;
//! - a move across two file systems copies, verifies, then sends the source to the trash, and undo
//!   brings it back: the live app between its temporary folder and `/dev/shm` when they are two
//!   devices (Linux); elsewhere the engine in this process with the cross-volume path forced (the
//!   app is built without fault points). It prints which ran.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_engine::cmd::delete::RESTORES;
use serde_json::{Value, json};

use crate::app::{self, click, entry, files, idle, key, quiet, read, redo, resize, run, undo};
use crate::live::{self, Client};

/// Where the app's trash keeps a file sent there from inside its home (Linux: the freedesktop
/// trash under `XDG_DATA_HOME`); `None` elsewhere (the system's own trash).
fn in_trash(home: &Path, name: &str) -> Option<bool> {
    cfg!(target_os = "linux").then(|| home.join("data").join("Trash").join("files").join(name).exists())
}

/// The names `data-model.md` §10.7 lists: `]`, `"`, `;`, `=`, non-ASCII and a very long one (243
/// bytes: the freedesktop trash names its record `<name>.trashinfo`, so a longer name cannot go to
/// the trash there). Windows refuses `"`: an apostrophe takes its place there, so every system has
/// the same five photos, in the same order, and the same window for the fingerprints.
fn hard_names() -> Vec<String> {
    let long = format!("{}xx.jpg", "長".repeat(79));
    assert_eq!(long.len(), 243);
    let quote = if cfg!(windows) { "q'uote.jpg" } else { "q\"uote.jpg" };
    let mut v = vec!["a]b.jpg".to_string(), "c;d=e.jpg".into(), "Praça à noite 東京.jpg".into(), long, quote.into()];
    v.sort();
    v
}

const ODD_DEST: &str = "Dest ]; a=b ç";

/// `Pictures`: `Lisbon` (four photos and a note), `Porto` (one), `Odd` (the hard names) and
/// `ODD_DEST` (one).
fn write(pictures: &Path) {
    let mut i = 0;
    let mut put = |rel: String| {
        i += 1;
        let jpeg = app::jpeg(200 + i, (160, 120), "Dalica", "Scenes 1", &format!("2024-06-04T11:{i:02}:00"));
        app::put(&app::under(pictures, &rel), &jpeg, i);
    };
    for n in ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0004.jpg"] {
        put(format!("Lisbon/{n}"));
    }
    put("Porto/PORTO_0001.jpg".into());
    for n in hard_names() {
        put(format!("Odd/{n}"));
    }
    put(format!("{ODD_DEST}/IMG_0001.jpg"));
    std::fs::write(pictures.join("Lisbon").join("notes.txt"), b"not a picture").unwrap();
}

/// What the user said about a photo, from `photo.inspect`: (star, caption, tags, recipe, albums).
fn said(p: &app::Photo) -> Value {
    let i = &p.inspected;
    json!({ "star": i["star"], "caption": i["caption"], "tags": i["tags"], "edits": i["edits"], "albums": i["albums"] })
}

/// The photo with id `id`: where it is now and what is said of it.
fn find(c: &mut Client, root: &Path, id: &Value) -> (String, app::Photo) {
    app::photos(c, root).into_iter().find(|(_, p)| &p.id == id).unwrap_or_else(|| panic!("photo {id} is in the library"))
}

/// Where photo `id` is now (relative to `root`), if the library lists it.
fn where_is(c: &mut Client, root: &Path, id: &Value) -> Option<String> {
    app::photos(c, root).into_iter().find(|(_, p)| &p.id == id).map(|(rel, _)| rel)
}

/// The sidecar entry keys that must travel with a photo.
fn carried(e: &Value) -> Value {
    json!({ "star": e.get("star"), "caption": e.get("caption"), "tags": e.get("tags"), "edits": e.get("edits"), "albums": e.get("albums") })
}

/// Part 3a: §10.3 over `engine.execute`, then the same move through the window.
fn moves(c: &mut Client, home: &Path, root: &Path) {
    let (lisbon, porto) = (root.join("Lisbon"), root.join("Porto"));
    let all = app::photos(c, root);
    let (one, two) = (all["Lisbon/IMG_0001.jpg"].id.clone(), all["Lisbon/IMG_0002.jpg"].id.clone());
    let porto_id = all["Porto/PORTO_0001.jpg"].folder;
    run(c, "photo.star.set", json!({ "ids": [one], "on": true }));
    run(c, "photo.caption.set", json!({ "ids": [one], "caption": "Tram 28" }));
    run(c, "photo.history.apply", json!({ "id": one, "op": "sepia()" }));
    run(c, "photo.tags.add", json!({ "ids": [two], "tags": ["lisbon", "tram"] }));
    run(c, "photo.history.apply", json!({ "id": two, "op": "autofix()" }));
    // Saved: its original goes to `.dalica-originals/`.
    run(c, "photo.save", json!({ "ids": [two], "confirm": true }));
    let original = read(&lisbon.join(".dalica-originals").join("IMG_0002.jpg")).expect("the save kept the original");
    let trip =
        run(c, "album.create", json!({ "name": "Trip", "ids": [all["Porto/PORTO_0001.jpg"].id, two, one] }))["album"].as_str().unwrap().to_string();
    app::flush(c);
    let all = app::photos(c, root);
    let said_before: Vec<Value> = ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg"].iter().map(|r| said(&all[*r])).collect();
    let entries: Vec<Value> = ["IMG_0001.jpg", "IMG_0002.jpg"].iter().map(|n| carried(&entry(&lisbon, n).unwrap())).collect();
    let bytes: Vec<Vec<u8>> = ["IMG_0001.jpg", "IMG_0002.jpg"].iter().map(|n| read(&lisbon.join(n)).unwrap()).collect();
    let order = app::section(c, root, &format!("album:{trip}"));
    let count = all.len();

    // The first move asks (Ask before moving is on).
    let refused = c.refused("photo.moveTo", json!({ "ids": [one, two], "folder": porto_id }));
    assert_eq!(refused["code"], "needs_confirm", "the first move asks: {refused}");
    assert!(lisbon.join("IMG_0001.jpg").is_file(), "nothing moved before the answer");

    let check_moved = |c: &mut Client, case: &str| {
        app::flush(c);
        for (i, n) in ["IMG_0001.jpg", "IMG_0002.jpg"].into_iter().enumerate() {
            let (rel, p) = find(c, root, if i == 0 { &one } else { &two });
            assert_eq!(rel, format!("Porto/{n}"), "{case}: the same photo, now in Porto");
            assert_eq!(said(&p), said_before[i], "{case}: {n}'s star, caption, tags, recipe and albums");
            assert!(read(&porto.join(n)).unwrap() == bytes[i], "{case}: {n}'s bytes");
            assert!(!lisbon.join(n).exists(), "{case}: {n} left Lisbon");
            assert_eq!(carried(&entry(&porto, n).unwrap()), entries[i], "{case}: {n}'s sidecar entry went with it");
            assert_eq!(entry(&lisbon, n), None, "{case}: and left Lisbon's sidecar");
        }
        assert!(read(&porto.join(".dalica-originals").join("IMG_0002.jpg")).unwrap() == original, "{case}: the kept original went with it");
        assert!(!lisbon.join(".dalica-originals").join("IMG_0002.jpg").exists(), "{case}");
        assert_eq!(
            app::section(c, root, &format!("album:{trip}")),
            order.iter().map(|r| r.replace("Lisbon/", "Porto/")).collect::<Vec<_>>(),
            "{case}: the album's order"
        );
        assert_eq!(app::photos(c, root).len(), count, "{case}: no photo new or lost");
    };
    let check_home = |c: &mut Client, case: &str| {
        app::flush(c);
        for (i, n) in ["IMG_0001.jpg", "IMG_0002.jpg"].into_iter().enumerate() {
            let (rel, p) = find(c, root, if i == 0 { &one } else { &two });
            assert_eq!(rel, format!("Lisbon/{n}"), "{case}");
            assert_eq!(said(&p), said_before[i], "{case}: {n}");
            assert!(read(&lisbon.join(n)).unwrap() == bytes[i] && !porto.join(n).exists(), "{case}: {n}");
            assert_eq!(carried(&entry(&lisbon, n).unwrap()), entries[i], "{case}: {n}'s sidecar entry");
        }
        assert!(read(&lisbon.join(".dalica-originals").join("IMG_0002.jpg")).unwrap() == original, "{case}: the kept original");
        assert_eq!(app::section(c, root, &format!("album:{trip}")), order, "{case}: the album's order");
    };
    run(c, "photo.moveTo", json!({ "ids": [one, two], "folder": porto_id, "confirm": true }));
    check_moved(c, "moved");
    undo(c);
    check_home(c, "Edit ▸ Undo");
    redo(c);
    check_moved(c, "Edit ▸ Redo");
    undo(c);
    check_home(c, "undone again");
    println!("  §10.3: two photos moved with their data, sidecar entries, album places and kept original; undone and redone");

    // Through the window: IMG_0003 dragged onto Porto's tree row. Held over it first: the pill.
    let three = all["Lisbon/IMG_0003.jpg"].id.clone();
    let row = format!("tree:folder:{porto_id}");
    let thumb = format!("thumb:{three}");
    quiet(c, 900.0);
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        click(c, &format!("tree:folder:{}", all["Lisbon/IMG_0003.jpg"].folder));
        run(c, "select.none", json!({}));
        let (from, to) = (app::center(app::rect_of(c, &thumb).expect("the thumbnail")), app::center(app::rect_of(c, &row).expect("Porto's row")));
        let mut events = vec![json!({ "kind": "move", "x": from.0, "y": from.1 }), json!({ "kind": "down", "x": from.0, "y": from.1 })];
        events.extend((1..=12).map(|i| {
            let t = f64::from(i) / 12.0;
            json!({ "kind": "drag", "x": from.0 + (to.0 - from.0) * t, "y": from.1 + (to.1 - from.1) * t })
        }));
        c.ok("ui.pointer", json!({ "events": events }));
        c.ok("ui.settle", json!({ "timeoutMs": 5000 }));
        let drag = c.ok("ui.inspect", json!({}))["drag"].clone();
        assert_eq!((drag["op"].as_str(), drag["pill"].as_str()), (Some("move"), Some("Move 1 photo to Porto")), "{drag}");
        assert_eq!(app::label(c, "drag:pill"), "Move 1 photo to Porto");
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-drag", size.0, size.1), size, &masks);
        c.ok("ui.key", json!({ "key": "Escape" }));
        c.ok("ui.pointer", json!({ "events": [{ "kind": "up", "x": to.0, "y": to.1 }] }));
        idle(c);
        assert!(lisbon.join("IMG_0003.jpg").is_file(), "Esc dropped nothing");
    }
    resize(c, (1400, 900));
    c.ok("ui.dragWidget", json!({ "id": thumb, "toWidget": row }));
    idle(c);
    assert_eq!(app::dialog(c).as_deref(), Some("moveFiles"), "the first drag onto a folder asks");
    assert!(lisbon.join("IMG_0003.jpg").is_file(), "nothing moved before the answer");
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-move", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));
    click(c, "check:dialog.dontAsk");
    click(c, "button:dialog.move");
    assert_eq!(app::dialog(c), None);
    assert!(porto.join("IMG_0003.jpg").is_file() && !lisbon.join("IMG_0003.jpg").exists(), "the drop moved it");
    key(c, "Ctrl+Z");
    assert!(lisbon.join("IMG_0003.jpg").is_file() && !porto.join("IMG_0003.jpg").exists(), "Ctrl+Z moved it back");
    // "Don't ask again": the next drag moves at once.
    c.ok("ui.dragWidget", json!({ "id": thumb, "toWidget": row }));
    idle(c);
    assert_eq!(app::dialog(c), None, "not asked again");
    assert!(porto.join("IMG_0003.jpg").is_file(), "moved without asking");
    key(c, "Ctrl+Z");
    assert!(lisbon.join("IMG_0003.jpg").is_file());
    println!("  the window: the drag pill, the first-time question, Don't ask again, Ctrl+Z");
}

/// Part 3b: copies and collisions, Move to New Folder, renames, folder rename and move, deletes.
fn the_other_operations(c: &mut Client, home: &Path, root: &Path) {
    let (lisbon, porto) = (root.join("Lisbon"), root.join("Porto"));
    let all = app::photos(c, root);
    let four = all["Lisbon/IMG_0004.jpg"].id.clone();
    let (lisbon_id, porto_id) = (all["Lisbon/IMG_0004.jpg"].folder, all["Porto/PORTO_0001.jpg"].folder);
    let count = all.len();
    run(c, "photo.caption.set", json!({ "ids": [four], "caption": "Alfama" }));
    app::flush(c);
    let four_bytes = read(&lisbon.join("IMG_0004.jpg")).unwrap();

    // Copy: a new photo with the source's data; undo sends the copy to the trash.
    run(c, "photo.copyTo", json!({ "ids": [four], "folder": porto_id }));
    app::flush(c);
    assert!(read(&porto.join("IMG_0004.jpg")).unwrap() == four_bytes && lisbon.join("IMG_0004.jpg").is_file(), "copied");
    assert_eq!(entry(&porto, "IMG_0004.jpg").unwrap()["caption"], "Alfama", "the copy has the source's entry");
    assert_eq!(app::photos(c, root).len(), count + 1, "a new photo");
    // Copy and Paste into Porto again: the copy question, then the collision question.
    run(c, "select.set", json!({ "ids": [four] }));
    key(c, "Ctrl+C");
    click(c, &format!("tree:folder:{porto_id}"));
    key(c, "Ctrl+V");
    assert_eq!(app::dialog(c).as_deref(), Some("copyFiles"), "Paste of a copy asks");
    click(c, "button:dialog.copy");
    assert_eq!(app::dialog(c).as_deref(), Some("collision"), "a name Porto already holds asks");
    assert!(app::widget(c, "button:dialog.renameDuplicates").is_some() && app::widget(c, "button:dialog.skipDuplicates").is_some());
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-collision", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));
    click(c, "button:dialog.renameDuplicates");
    assert!(read(&porto.join("IMG_0004-001.jpg")).unwrap() == four_bytes, "Rename Duplicates: IMG_0004-001.jpg");
    key(c, "Ctrl+Z");
    assert!(!porto.join("IMG_0004-001.jpg").exists(), "undo sent the second copy to the trash");
    undo(c);
    assert!(!porto.join("IMG_0004.jpg").exists() && lisbon.join("IMG_0004.jpg").is_file(), "and the first");
    assert!(in_trash(home, "IMG_0004.jpg").unwrap_or(true), "the copies are in the trash");
    assert_eq!(app::photos(c, root).len(), count);
    println!("  copies, the copy and collision questions, Rename Duplicates; undone");

    // Move to New Folder: beside the photos' folder, undone with the folder.
    let (three, four) = (all["Lisbon/IMG_0003.jpg"].id.clone(), four);
    let r = run(c, "photo.moveToNewFolder", json!({ "ids": [three, four], "name": "Lisbon: Day 2", "description": "Alfama" }));
    let made = files(root);
    let new = made
        .iter()
        .find(|n| n.starts_with("Lisbon") && *n != "Lisbon")
        .cloned()
        .unwrap_or_else(|| panic!("a new folder beside Lisbon: {made:?} ({r})"));
    // The new folder's sidecar may still be on its way (its atomic write's temporary file).
    let moved: Vec<String> = files(&root.join(&new)).into_iter().filter(|n| !n.starts_with('.')).collect();
    assert_eq!(moved, ["IMG_0003.jpg", "IMG_0004.jpg"], "the photos moved into {new}");
    undo(c);
    assert!(!root.join(&new).exists(), "Edit ▸ Undo moved them back and removed the empty folder");
    assert!(lisbon.join("IMG_0003.jpg").is_file() && lisbon.join("IMG_0004.jpg").is_file());
    println!("  Move to New Folder: {new:?} beside Lisbon; undone");

    // Rename Files from F2: three photos, "Lisbon" → Lisbon-001.jpg … in light-table order.
    let ids: Vec<Value> = ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg"].iter().map(|r| all[*r].id.clone()).collect();
    let said_before: Vec<Value> = ids.iter().map(|id| said(&find(c, root, id).1)).collect();
    click(c, &format!("tree:folder:{lisbon_id}"));
    run(c, "select.set", json!({ "ids": ids }));
    key(c, "F2");
    assert_eq!(app::dialog(c).as_deref(), Some("renameFiles"), "F2 opens Rename Files");
    c.ok("ui.type", json!({ "text": "Lisbon" }));
    idle(c);
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-rename", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));
    click(c, "button:dialog.rename");
    app::flush(c);
    for (i, id) in ids.iter().enumerate() {
        let (rel, p) = find(c, root, id);
        assert_eq!(rel, format!("Lisbon/Lisbon-{:03}.jpg", i + 1), "renamed in light-table order");
        assert_eq!(said(&p), said_before[i], "with its data");
    }
    assert!(entry(&lisbon, "Lisbon-001.jpg").is_some_and(|e| e["star"] == true), "the entry renamed with it");
    key(c, "Ctrl+Z");
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(find(c, root, id).0, format!("Lisbon/IMG_{:04}.jpg", i + 1), "Ctrl+Z puts the names back");
    }
    run(c, "photo.rename", json!({ "ids": [ids[0]], "name": "Tram" }));
    assert_eq!(find(c, root, &ids[0]).0, "Lisbon/Tram.jpg", "one photo takes the name");
    undo(c);
    assert_eq!(find(c, root, &ids[0]).0, "Lisbon/IMG_0001.jpg");
    println!("  Rename Files (F2) Lisbon-001 … -003 and a single rename; undone");

    // A folder renamed, and moved into another; ids kept; undone.
    run(c, "folder.rename", json!({ "folder": porto_id, "name": "Oporto" }));
    assert!(root.join("Oporto").join("PORTO_0001.jpg").is_file() && !porto.exists(), "renamed on disk");
    assert_eq!(find(c, root, &all["Porto/PORTO_0001.jpg"].id).1.folder, porto_id, "the folder keeps its id");
    undo(c);
    assert!(porto.join("PORTO_0001.jpg").is_file());
    run(c, "folder.move", json!({ "folder": porto_id, "to": lisbon_id }));
    assert!(lisbon.join("Porto").join("PORTO_0001.jpg").is_file() && !porto.exists(), "moved into Lisbon");
    assert_eq!(find(c, root, &all["Porto/PORTO_0001.jpg"].id).0, "Lisbon/Porto/PORTO_0001.jpg", "its photo keeps its id");
    undo(c);
    assert!(porto.join("PORTO_0001.jpg").is_file() && !lisbon.join("Porto").exists(), "Edit ▸ Undo moved it back");
    println!("  a folder renamed and moved, ids kept; undone");

    // Delete from Disk with the Delete key: the question, Move to Trash, Edit ▸ Undo.
    let gone = all["Lisbon/IMG_0004.jpg"].id.clone();
    // A click on its thumbnail selects it and puts the keys in the light table (in the tree,
    // Delete is Folder ▸ Delete).
    click(c, &format!("tree:folder:{lisbon_id}"));
    click(c, &format!("thumb:{gone}"));
    key(c, "Delete");
    assert_eq!(app::dialog(c).as_deref(), Some("deletePhotos"), "Delete asks");
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-delete", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));
    click(c, "button:dialog.moveToTrash");
    assert!(!lisbon.join("IMG_0004.jpg").exists(), "the file went");
    assert!(in_trash(home, "IMG_0004.jpg").unwrap_or(true), "to the trash");
    assert!(app::photos(c, root).values().all(|p| p.id != gone), "and the photo left the library");
    if RESTORES {
        key(c, "Ctrl+Z");
        assert!(read(&lisbon.join("IMG_0004.jpg")).is_some(), "Ctrl+Z brought it back from the trash");
        let back = find(c, root, &gone).1;
        assert_eq!(back.inspected["caption"], "Alfama", "with its data");
        println!("  Delete from Disk (the Delete key) to the trash; Ctrl+Z restored it");
    } else {
        let r = c.send("engine.execute", json!({ "command": "edit.undo", "params": {} }));
        assert_eq!(r["ok"], false, "undo of a delete is disabled here: {r}");
        assert!(r["error"].as_str().unwrap_or_default().contains("Trash"), "and says why: {r}");
        println!("  Delete from Disk (the Delete key) to the trash; undo disabled with its reason: {}", r["error"]);
    }

    // A folder deleted with its non-picture files; back on undo where the system restores.
    let note = read(&lisbon.join("notes.txt")).unwrap();
    run(c, "folder.delete", json!({ "folder": lisbon_id, "confirm": true }));
    assert!(!lisbon.exists(), "the folder and everything in it (notes.txt too) went to the trash");
    if RESTORES {
        undo(c);
        assert!(read(&lisbon.join("notes.txt")).unwrap() == note && lisbon.join("IMG_0001.jpg").is_file(), "Edit ▸ Undo brought it all back");
        println!("  a folder deleted with its non-picture files; undone");
    }
}

/// Part 3c: §10.7, hard names through move, copy, rename and delete and the undo of each.
fn hard_names_round_trip(c: &mut Client, root: &Path) {
    let (odd, dest) = (root.join("Odd"), root.join(ODD_DEST));
    let names = hard_names();
    let all = app::photos(c, root);
    let ids: Vec<Value> = names.iter().map(|n| all[&format!("Odd/{n}")].id.clone()).collect();
    let (odd_id, dest_id) = (all[&format!("Odd/{}", names[0])].folder, all[&format!("{ODD_DEST}/IMG_0001.jpg")].folder);
    run(c, "photo.star.set", json!({ "ids": ids, "on": true }));
    app::flush(c);
    let bytes: Vec<Vec<u8>> = names.iter().map(|n| read(&odd.join(n)).unwrap()).collect();
    let at_home = |c: &mut Client, case: &str| {
        app::flush(c);
        assert_eq!(files(&odd), names, "{case}: the exact names");
        for (i, n) in names.iter().enumerate() {
            assert!(read(&odd.join(n)).unwrap() == bytes[i], "{case}: {n}");
            assert_eq!(entry(&odd, n).map(|e| e["star"].clone()), Some(json!(true)), "{case}: {n}'s entry");
            assert_eq!(find(c, root, &ids[i]).0, format!("Odd/{n}"), "{case}: {n}");
        }
    };
    at_home(c, "before");
    run(c, "photo.moveTo", json!({ "ids": ids, "folder": dest_id, "confirm": true }));
    for (i, n) in names.iter().enumerate() {
        assert!(read(&dest.join(n)).unwrap() == bytes[i], "moved: {n}");
    }
    undo(c);
    at_home(c, "move undone");
    run(c, "photo.copyTo", json!({ "ids": ids, "folder": dest_id }));
    for (i, n) in names.iter().enumerate() {
        assert!(read(&dest.join(n)).unwrap() == bytes[i], "copied: {n}");
    }
    undo(c);
    assert_eq!(files(&dest), ["IMG_0001.jpg"], "copy undone");
    at_home(c, "copy undone");
    run(c, "photo.rename", json!({ "ids": ids, "name": "Tram ]28; a=b ç 東京" }));
    assert!(files(&odd).iter().all(|n| n.starts_with("Tram ]28; a=b ç 東京-00")), "{:?}", files(&odd));
    undo(c);
    at_home(c, "rename undone");
    run(c, "folder.rename", json!({ "folder": odd_id, "name": "Odd ]; ç=1 東" }));
    assert_eq!(files(&root.join("Odd ]; ç=1 東")), names);
    undo(c);
    at_home(c, "folder rename undone");
    run(c, "photo.delete", json!({ "ids": ids, "confirm": true }));
    assert!(files(&odd).is_empty(), "every hard name went to the trash");
    if RESTORES {
        undo(c);
        at_home(c, "delete undone");
    }
    println!("  §10.7: {} hard names through move, copy, rename, folder rename and delete, each undone", names.len());
}

/// Waits (at most `within` of real time: the session's clock is the system's, even headless)
/// until `ok` holds, letting the frames and jobs settle between looks; how long it took.
fn until(c: &mut Client, what: &str, within: Duration, mut ok: impl FnMut(&mut Client) -> bool) -> Duration {
    let started = Instant::now();
    while !ok(c) {
        assert!(started.elapsed() < within, "{what}: not within {within:?}");
        c.ok("ui.wait", json!({ "ms": 250 }));
        idle(c);
        std::thread::sleep(Duration::from_millis(250));
    }
    started.elapsed()
}

/// Part 3d: §10.2, `Trips/Porto` (here `Porto`) moved outside Dalica while it watches, then
/// between two sessions. Returns the app and its client after the restart.
fn moved_outside(exe: &Path, home: &Path, root: &Path, app_: live::App, mut cl: Client) -> (live::App, Client) {
    let c = &mut cl;
    let all = app::photos(c, root);
    let porto = all["Porto/PORTO_0001.jpg"].clone();
    let other = all[&format!("{ODD_DEST}/IMG_0001.jpg")].id.clone();
    run(c, "photo.caption.set", json!({ "ids": [porto.id], "caption": "Ponte Luís I" }));
    let river = run(c, "album.create", json!({ "name": "River", "ids": [porto.id, other] }))["album"].as_str().unwrap().to_string();
    let said_before = said(&find(c, root, &porto.id).1);
    let order = |c: &mut Client| -> Vec<Value> {
        let r = c.ok("engine.execute", json!({ "command": "library.query", "params": { "section": format!("album:{river}") } }));
        r["photos"].as_array().unwrap().iter().map(|p| p["id"].clone()).collect()
    };
    let members = order(c);
    let check = |c: &mut Client, rel: &str, case: &str| {
        let (now, p) = find(c, root, &porto.id);
        assert_eq!(now, rel, "{case}: the photo keeps its id at the new path");
        assert_eq!(p.folder, porto.folder, "{case}: the folder keeps its id");
        assert_eq!(said(&p), said_before, "{case}: its caption and album");
        assert_eq!(order(c), members, "{case}: the album's order");
        assert_eq!(app::photos(c, root).len(), all.len(), "{case}: nothing new, nothing lost");
    };

    // While watching.
    std::fs::create_dir_all(root.join("Elsewhere")).unwrap();
    std::fs::rename(root.join("Porto"), root.join("Elsewhere").join("Oporto")).unwrap();
    let watched = until(c, "the watcher found the moved folder", Duration::from_secs(120), |c| {
        where_is(c, root, &porto.id).as_deref() == Some("Elsewhere/Oporto/PORTO_0001.jpg")
    });
    check(c, "Elsewhere/Oporto/PORTO_0001.jpg", "moved while watching");
    live::quit(app_, cl);

    // Between sessions: the next session's verification pass (a minute after it opens).
    std::fs::rename(root.join("Elsewhere").join("Oporto"), root.join("Porto 2024")).unwrap();
    let (app_, mut cl) = live::start(exe, home);
    let c = &mut cl;
    idle(c);
    // The verification pass runs a minute after the session opens (`watching::FIRST_VERIFICATION`).
    let verified = until(c, "the verification pass found the moved folder", Duration::from_secs(180), |c| {
        where_is(c, root, &porto.id).as_deref() == Some("Porto 2024/PORTO_0001.jpg")
    });
    check(c, "Porto 2024/PORTO_0001.jpg", "moved between sessions");
    println!(
        "  §10.2: a folder moved outside Dalica while watching (found in {:.1} s) and between sessions (found in {:.1} s) keeps its ids, data and album order",
        watched.as_secs_f64(),
        verified.as_secs_f64()
    );
    (app_, cl)
}

/// Part 3 in one app: the operations and §10.3, §10.7 and §10.2.
pub fn file_operations(exe: &Path, home: &Path) {
    let started = Instant::now();
    let root = home.join("Pictures");
    write(&root);
    let (app_, mut cl) = live::start(exe, home);
    run(&mut cl, "folders.add", json!({ "path": root }));
    moves(&mut cl, home, &root);
    the_other_operations(&mut cl, home, &root);
    hard_names_round_trip(&mut cl, &root);
    let (app_, cl) = moved_outside(exe, home, &root, app_, cl);
    live::quit(app_, cl);
    println!("M9 file operations in the live app: passed in {:.1} s", app::secs(started));
}

// --- two file systems ---------------------------------------------------------------------------

/// A folder in `/dev/shm` when that is another device than `here` (Linux).
#[cfg(target_os = "linux")]
fn shm(here: &Path) -> Option<tempfile::TempDir> {
    use std::os::unix::fs::MetadataExt;
    let shm = Path::new("/dev/shm");
    let (a, b) = (std::fs::metadata(here).ok()?, std::fs::metadata(shm).ok()?);
    if !b.is_dir() || a.dev() == b.dev() {
        return None;
    }
    tempfile::Builder::new().prefix("dalica-accept-m9-").tempdir_in(shm).ok()
}

#[cfg(not(target_os = "linux"))]
fn shm(_here: &Path) -> Option<tempfile::TempDir> {
    None
}

/// `/dev/shm`'s trash folder for this user.
#[cfg(target_os = "linux")]
fn shm_trash(dir: &Path) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata(dir).ok()?.uid();
    Some(Path::new("/dev/shm").join(format!(".Trash-{uid}")))
}

#[cfg(not(target_os = "linux"))]
fn shm_trash(_dir: &Path) -> Option<std::path::PathBuf> {
    None
}

/// Takes out of `/dev/shm`'s trash what was sent there from `dir` (the entries whose record names
/// a path under it), and nothing else; the trash folder itself goes too when this test made it
/// (`made`) and nothing else is left in it. Returns how many entries were ours.
fn clean_shm_trash(dir: &Path, made: bool) -> usize {
    let Some(trash) = shm_trash(dir) else { return 0 };
    // The record's path is percent-encoded; the temporary folder's name is plain ASCII.
    let name = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = 0;
    for e in std::fs::read_dir(trash.join("info")).into_iter().flatten().flatten() {
        let info = e.path();
        let text = std::fs::read_to_string(&info).unwrap_or_default();
        let ours = !name.is_empty() && text.lines().filter_map(|l| l.strip_prefix("Path=")).any(|p| p.contains(&name));
        if ours && let Some(stem) = info.file_stem() {
            let file = trash.join("files").join(stem);
            let _ = if file.is_dir() { std::fs::remove_dir_all(&file) } else { std::fs::remove_file(&file) };
            let _ = std::fs::remove_file(&info);
            n += 1;
        }
    }
    let empty = |d: &Path| std::fs::read_dir(d).map(|mut e| e.next().is_none()).unwrap_or(true);
    if made && empty(&trash.join("files")) && empty(&trash.join("info")) {
        for sub in ["files", "info", "expunged"] {
            let _ = std::fs::remove_dir(trash.join(sub));
        }
        let _ = std::fs::remove_dir(&trash);
    }
    n
}

/// A move across two file systems copies, verifies, then sends the source to the trash; undo
/// brings it back. Returns which ran.
pub fn two_volumes(exe: &Path) -> &'static str {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    // `DALICA_ACCEPT_FORCED_VOLUMES=1` runs the forced path even where `/dev/shm` is another device.
    let forced_asked = std::env::var_os("DALICA_ACCEPT_FORCED_VOLUMES").is_some_and(|v| v == "1");
    let Some(there) = shm(home.path()).filter(|_| !forced_asked) else {
        forced(home.path());
        let why = if forced_asked { "DALICA_ACCEPT_FORCED_VOLUMES=1" } else { "one device here" };
        println!("two volumes: {why}, so the engine ran the cross-volume path forced, in this process ({:.1} s)", app::secs(started));
        return "forced";
    };
    let (here_root, there_root) = (home.path().join("Pictures"), there.path().join("Away"));
    let (lisbon, porto) = (here_root.join("Lisbon"), there_root.join("Porto"));
    for (i, n) in ["IMG_0001.jpg", "IMG_0002.jpg"].iter().enumerate() {
        app::put(&lisbon.join(n), &app::jpeg(300 + i as u64, (160, 120), "Dalica", "Scenes 1", "2024-06-04T12:00:00"), i as u64);
    }
    app::put(&porto.join("PORTO_0001.jpg"), &app::jpeg(310, (160, 120), "Dalica", "Scenes 1", "2024-06-04T12:30:00"), 9);
    let made = shm_trash(there.path()).is_some_and(|t| !t.exists());
    // Both watched from an earlier session: automation reads only inside the app's home.
    let library = home.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let watched = json!({ "version": 2, "folders": [{ "path": here_root, "mode": "always" }, { "path": there_root, "mode": "always" }] });
    std::fs::write(library.join("folders.json"), serde_json::to_vec_pretty(&watched).unwrap()).unwrap();
    let (app_, mut cl) = live::start(exe, home.path());
    let c = &mut cl;
    // A new library reads its watched folders when it is rebuilt.
    run(c, "library.rebuild", json!({ "confirm": true }));
    idle(c);
    let mut ids = BTreeMap::new();
    for root in [&here_root, &there_root] {
        for (rel, p) in app::photos(c, root) {
            ids.insert(rel, p);
        }
    }
    assert_eq!(ids.len(), 3, "both volumes' photos: {:?}", ids.keys());
    let (one, two) = (ids["Lisbon/IMG_0001.jpg"].id.clone(), ids["Lisbon/IMG_0002.jpg"].id.clone());
    run(c, "photo.caption.set", json!({ "ids": [one], "caption": "Across" }));
    app::flush(c);
    let bytes: Vec<Vec<u8>> = ["IMG_0001.jpg", "IMG_0002.jpg"].iter().map(|n| read(&lisbon.join(n)).unwrap()).collect();
    let porto_id = ids["Porto/PORTO_0001.jpg"].folder;
    run(c, "photo.moveTo", json!({ "ids": [one, two], "folder": porto_id, "confirm": true }));
    app::flush(c);
    for (i, n) in ["IMG_0001.jpg", "IMG_0002.jpg"].iter().enumerate() {
        assert!(read(&porto.join(n)).unwrap() == bytes[i], "{n}: the same bytes on the other file system");
        assert!(!lisbon.join(n).exists(), "{n}: the source left");
        assert_eq!(in_trash(home.path(), n), Some(true), "{n}: to the trash, once the copy verified");
    }
    assert_eq!(entry(&porto, "IMG_0001.jpg").unwrap()["caption"], "Across", "the entry went across");
    let state = |c: &mut Client| {
        let here: Vec<_> = app::photos(c, &here_root).into_iter().map(|(rel, p)| (rel, p.id)).collect();
        let there: Vec<_> = app::photos(c, &there_root).into_iter().map(|(rel, p)| (rel, p.id)).collect();
        format!("here {here:?}, there {there:?}")
    };
    assert_eq!(where_is(c, &there_root, &one).as_deref(), Some("Porto/IMG_0001.jpg"), "the same photo, now there: {}", state(c));
    // The emptied Lisbon leaves the library at the watcher's next rescan; the undo brings it back
    // (the library could not follow before M9.25: `crates/engine/tests/undo_emptied_folder.rs`).
    let lisbon_id = ids["Lisbon/IMG_0001.jpg"].folder;
    let row = format!("tree:folder:{lisbon_id}");
    let end = Instant::now() + Duration::from_secs(60);
    while c.has(&row) {
        assert!(Instant::now() < end, "the emptied Lisbon left the tree within a minute");
        std::thread::sleep(Duration::from_millis(250));
        idle(c);
    }
    undo(c);
    app::flush(c);
    for (i, n) in ["IMG_0001.jpg", "IMG_0002.jpg"].iter().enumerate() {
        assert!(read(&lisbon.join(n)).unwrap() == bytes[i], "{n}: back, byte for byte");
        assert!(!porto.join(n).exists(), "{n}: gone from the other file system");
    }
    assert_eq!(entry(&lisbon, "IMG_0001.jpg").unwrap()["caption"], "Across");
    assert_eq!(where_is(c, &here_root, &one).as_deref(), Some("Lisbon/IMG_0001.jpg"), "undo: the same photo, back: {}", state(c));
    assert_eq!(where_is(c, &here_root, &two).as_deref(), Some("Lisbon/IMG_0002.jpg"), "{}", state(c));
    assert!(c.has(&row), "Lisbon is back in the tree with its id");
    live::quit(app_, cl);
    let cleaned = clean_shm_trash(there.path(), made);
    println!(
        "two volumes: the live app moved two photos from {} to {} (another device): copied, verified, sources to the trash; undo brought them back, and the emptied Lisbon with its id ({cleaned} trash entries of ours removed from /dev/shm; {:.1} s)",
        home.path().display(),
        there.path().display(),
        app::secs(started)
    );
    "real"
}

/// The same with the engine in this process and the cross-volume path forced (one photo per move:
/// the forced switch fires once). The session's trash is the system's: what the test sent there is
/// put back and deleted with the temporary folder where the system restores (as
/// `crates/engine/tests/two_volumes.rs` does).
fn forced(dir: &Path) {
    use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
    use dalica_fs::faults::{self, Fault, points};
    let (lisbon, porto) = (dir.join("Pictures").join("Lisbon"), dir.join("Away").join("Porto"));
    app::put(&lisbon.join("IMG_0001.jpg"), &app::jpeg(300, (160, 120), "Dalica", "Scenes 1", "2024-06-04T12:00:00"), 1);
    app::put(&porto.join("PORTO_0001.jpg"), &app::jpeg(310, (160, 120), "Dalica", "Scenes 1", "2024-06-04T12:30:00"), 2);
    let bytes = read(&lisbon.join("IMG_0001.jpg")).unwrap();
    let source = dalica_fs::identity(&lisbon.join("IMG_0001.jpg")).unwrap().unwrap();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.join("library"), Arc::new(dalica_core::SystemClock), &options).unwrap();
    for root in [dir.join("Pictures"), dir.join("Away")] {
        s.execute("folders.add", &json!({ "path": root })).unwrap();
    }
    s.wait_idle();
    let lib = s.library();
    let id = lib.photo_by_name(lib.folder_by_path(&lisbon).unwrap(), "IMG_0001.jpg").unwrap().id;
    let porto_id = lib.folder_by_path(&porto).unwrap();
    let patience = Duration::from_secs(120);
    let settle = |s: &mut Session| {
        assert!(s.wait_file_jobs(patience));
        faults::disarm_all();
        s.wait_idle();
        s.flush().unwrap();
        s.file_op_events().last().cloned().unwrap()
    };
    faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
    s.execute("photo.moveTo", &json!({ "ids": [id], "folder": porto_id, "confirm": true })).unwrap();
    let e = settle(&mut s);
    assert!(e.failures.is_empty() && e.done == 1, "{e:?}");
    assert!(read(&porto.join("IMG_0001.jpg")).unwrap() == bytes, "copied and verified");
    assert!(dalica_fs::identity(&porto.join("IMG_0001.jpg")).unwrap() != Some(source), "a copy, not a rename");
    assert!(!lisbon.join("IMG_0001.jpg").exists(), "the source to the trash");
    let copy = dalica_fs::identity(&porto.join("IMG_0001.jpg")).unwrap().unwrap();
    faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
    s.execute("edit.undo", &json!({})).unwrap();
    let e = settle(&mut s);
    assert!(e.failures.is_empty() && e.done == 1, "{e:?}");
    assert!(read(&lisbon.join("IMG_0001.jpg")).unwrap() == bytes, "undo: back, byte for byte");
    assert!(!porto.join("IMG_0001.jpg").exists());
    assert_eq!(s.library().photo(id).map(|p| p.folder), s.library().folder_by_path(&lisbon), "the same photo, back in Lisbon");
    drop(s);
    // The trash as it was: the source the move sent there, and the copy the undo sent there.
    if RESTORES {
        for (path, identity) in [(lisbon.join("IMG_0001.jpg"), source), (porto.join("IMG_0001.jpg"), copy)] {
            let _ = std::fs::remove_file(&path);
            let original = std::fs::canonicalize(path.parent().unwrap()).unwrap().join(path.file_name().unwrap());
            let back = dalica_fs::trash::restore(&[dalica_fs::trash::Trashed { original, identity, folder: false }]).unwrap();
            assert_eq!(back, [dalica_fs::trash::Restore::Restored], "back from the trash");
        }
    }
}
