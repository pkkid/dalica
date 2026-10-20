//! The Import room's destination, importing and finishing (M11.20, `organizing.md` §3 items 4–6
//! and its flow), driven headlessly by automation id on a temporary camera card the session's
//! device list finds inside a mount folder: the import buttons stay disabled until a folder name;
//! Date taken imports into dated folders, the room closes, the library selects the first and the
//! toast names them; Cancel part-way keeps what was copied; with "Delete copied files from the
//! card" the question appears, Keep Files keeps them and Remove Imported Files removes only the
//! copied ones; the empty library's Import… opens the room; fingerprints of the strip in its
//! states and of the question (the small window has to fit).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_sizes, new_driver, try_widget, widget};
use dalica_core::{Date, DateTime};
use dalica_engine::Session;
use dalica_ui::state::{Room, Section};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

/// A 192 × 128 JPEG of scene `seed` taken on 2026-06-04 at `h:m` (`day` days later).
fn shot(seed: u64, day: u8, h: u8, m: u8) -> Vec<u8> {
    let taken = DateTime::new(Date::new(2026, 6, 4 + day).unwrap(), h, m, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 192, 128), 85, Some(&exif)).unwrap()
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The card's pictures: four on June 4, one on June 5; IMG_0001 is the library's photo too (a
/// duplicate, never copied).
const CARD: [(&str, u64, u8, u8, u8); 5] = [
    ("IMG_0001.JPG", 1, 0, 8, 0),
    ("IMG_0002.JPG", 2, 0, 8, 5),
    ("IMG_0003.JPG", 3, 0, 8, 10),
    ("IMG_0004.JPG", 4, 0, 13, 0),
    ("IMG_0005.JPG", 5, 1, 9, 30),
];

/// The card's roll folder.
fn roll(dir: &Path) -> PathBuf {
    dir.join("mounts/EOS_DIGITAL/DCIM/100CANON")
}

/// The folder imported into.
fn pictures(dir: &Path) -> PathBuf {
    dir.join("Pictures")
}

/// A library holding IMG_0001's twin, the card `EOS_DIGITAL` in a mount folder, the room open on
/// it with every thumbnail in, "Import to:" the temporary Pictures folder.
fn driver(dir: &Path, picker: Option<ScriptedPicker>) -> Driver {
    let (_, seed, day, hour, min) = CARD[0];
    put(&dir.join("photos/2026/twin.jpg"), &shot(seed, day, hour, min));
    for (name, seed, day, hour, min) in CARD {
        put(&roll(dir).join(name), &shot(seed, day, hour, min));
    }
    std::fs::create_dir_all(pictures(dir)).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": dir.join("photos/2026") })).unwrap();
    s.wait_idle();
    s.set_import_device_roots(Some(std::env::join_paths([dir.join("mounts")]).unwrap()), Duration::from_millis(50));
    // "Import to:" starts from a Pictures folder of the test's own, never the machine's.
    s.set_pictures_dir(Some(dir.join("Library Pictures")));
    let app = match picker {
        Some(p) => App::with_picker(Some(s), Arc::new(p)),
        None => App::new(Some(s)),
    };
    let mut d = new_driver(app, LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.key("Ctrl+M").unwrap();
    until(&mut d, "the card listed with its thumbnails", listed);
    d.settle(FRAMES).unwrap();
    d
}

fn run(d: &mut Driver, id: &str, params: Value) -> Value {
    d.app_mut().session_mut().unwrap().execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Steps frames (letting the workers run) until `done` holds; at most a minute.
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&mut Driver) -> bool) {
    let start = Instant::now();
    while !done(d) {
        assert!(start.elapsed() < Duration::from_secs(60), "never: {what}");
        d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The listing is done and every thumbnail is in memory.
fn listed(d: &mut Driver) -> bool {
    let l = run(d, "import.list", json!({}));
    l["state"] == "done" && l["candidates"].as_array().is_some_and(|c| c.len() == CARD.len() && c.iter().all(|c| c["thumbnail"] == true))
}

fn id_of(d: &mut Driver, name: &str) -> u64 {
    let l = run(d, "import.list", json!({}));
    l["candidates"].as_array().unwrap().iter().find(|c| c["name"] == name).and_then(|c| c["id"].as_u64()).unwrap()
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

/// The file names in `dir`, sorted (none when it does not exist).
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map_or_else(
        |_| Vec::new(),
        |r| r.filter_map(Result::ok).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
    );
    names.sort();
    names
}

fn status(d: &mut Driver) -> Value {
    run(d, "import.status", json!({}))
}

/// The toasts showing now.
fn toasts(d: &Driver) -> Vec<String> {
    let now = d.ctx().input(|i| i.time);
    let v = d.app().notices().inspect(now);
    v.as_array().map(|a| a.iter().filter_map(|t| t["text"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// "Import to:" ▸ Choose… with the picker answering the temporary Pictures folder.
fn choose_pictures(d: &mut Driver) {
    click(d, "choice:import.destination");
    click(d, "choice:import.destination/choose");
    assert_eq!(widget(d, "choice:import.destination").label, "Pictures");
}

#[test]
fn the_buttons_wait_for_a_folder_name_and_date_taken_imports_into_dated_folders() {
    let dir = tempfile::tempdir().unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir.path()));
    let mut d = driver(dir.path(), Some(picker));
    assert_eq!(widget(&d, "choice:import.destination").label, "Library Pictures", "the default destination");
    choose_pictures(&mut d);
    assert_eq!(d.state().import.destination.place.as_deref(), pictures(dir.path()).to_str());

    // No folder name: both import buttons disabled, the hint says why, Enter imports nothing.
    let first = id_of(&mut d, "IMG_0002.JPG");
    click(&mut d, &format!("candidate:{first}"));
    assert_eq!(widget(&d, "text:import.hint").label, "Choose a folder name to continue");
    assert!(!widget(&d, "button:import.importAll").enabled && !widget(&d, "button:import.importSelected").enabled);
    assert_eq!(widget(&d, "button:import.importSelected").label, "Import Selected (1)");
    assert!(widget(&d, "choice:import.after").label == "Leave files on the card", "a card shows After copying");
    d.key("Enter").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(status(&mut d)["state"], Value::Null, "nothing imported");
    check_region(&d, "importDestination", "import-destination-no-name-1400x900");

    // Date taken: the hint says what it does, and the buttons wake up.
    click(&mut d, "choice:import.folderName");
    click(&mut d, "choice:import.folderName/dateTaken");
    assert_eq!(widget(&d, "field:import.folderName").label, "Folder name");
    assert_eq!(widget(&d, "text:import.hint").label, "Each capture date gets its own folder");
    assert!(widget(&d, "button:import.importAll").enabled && widget(&d, "button:import.importSelected").enabled);
    check_sizes(&mut d, "importDestination", "import-destination-date-taken");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    crate::common::shot(&d, "import-destination-900x600-window");
    d.resize(LARGE.0, LARGE.1);
    d.settle(FRAMES).unwrap();

    // Import All: one folder per capture date, the duplicate left out; the room closes, the
    // library selects the first new folder, and the toast names the folders.
    click(&mut d, "button:import.importAll");
    until(&mut d, "the import finished and the room closed", |d| d.state().rooms == [Room::Library]);
    let (june4, june5) = (pictures(dir.path()).join("2026-06-04"), pictures(dir.path()).join("2026-06-05"));
    assert_eq!(files(&june4), ["IMG_0002.JPG", "IMG_0003.JPG", "IMG_0004.JPG"]);
    assert_eq!(files(&june5), ["IMG_0005.JPG"]);
    assert_eq!(std::fs::read(june4.join("IMG_0002.JPG")).unwrap(), shot(2, 0, 8, 5), "a byte-for-byte copy");
    let folder = d.app().session().unwrap().library().folder_by_path(&june4).expect("the new folder is in the library");
    let sel = &d.state().selection;
    assert_eq!((sel.section, sel.whole, sel.ids.len()), (Some(Section::Folder(folder)), true, 3));
    assert_eq!(d.state().scroll.section, Some(Section::Folder(folder)), "scrolled to it");
    assert!(toasts(&d).iter().any(|t| t == "Imported 4 pictures into 2 folders"), "{:?}", toasts(&d));
    assert_eq!(files(&roll(dir.path())).len(), CARD.len(), "the card keeps every file");
}

#[test]
fn cancel_part_way_keeps_what_was_copied() {
    let dir = tempfile::tempdir().unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir.path()));
    let mut d = driver(dir.path(), Some(picker));
    choose_pictures(&mut d);
    // A typed folder name.
    click(&mut d, "field:import.folderName");
    d.type_text("Card").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().import.destination.folder_name, Some(dalica_ui::state::FolderName::Text("Card".into())));
    assert_eq!(widget(&d, "text:import.hint").label, "Click Import All or Import Selected");

    // The job waits after its second copy until the test lets it go on.
    let (go, wait) = mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let gate = Arc::clone(&wait);
    d.app_mut().session_mut().unwrap().set_import_gate(Some(Arc::new(move |n| {
        if n == 1 {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(30));
        }
    })));
    click_now(&mut d, "button:import.importAll");
    // The second copy is made and the job holds it at the gate.
    let card = pictures(dir.path()).join("Card");
    until(&mut d, "the second copy made", |d| status(d)["imported"] == 1 && files(&card).contains(&"IMG_0003.JPG".to_string()));
    d.step();
    assert!(widget(&d, "progress:import").label.starts_with("Copying "), "{}", widget(&d, "progress:import").label);
    assert!(d.state().status_line.starts_with("Copying "), "the status line: {}", d.state().status_line);
    assert!(!widget(&d, "button:import.importAll").enabled, "no second import while one copies");

    // Cancel stops after the current file; what was copied stays, and the room stays on the result.
    click_now(&mut d, "button:import.cancel");
    go.send(()).unwrap();
    until(&mut d, "the import ended", |d| status(d)["state"] == "cancelled");
    d.settle(FRAMES).unwrap();
    assert_eq!(files(&card), ["IMG_0002.JPG", "IMG_0003.JPG"]);
    assert_eq!(d.state().room, Room::Import, "the room stays open on a cancelled import");
    assert_eq!(widget(&d, "text:import.result").label, "Import cancelled: 2 of 4 pictures copied");
    assert!(try_widget(&d, "dialog:removeFromCard").is_none());
}

/// Clicks `id` and runs a few frames, without waiting for the jobs it starts to settle.
fn click_now(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    for _ in 0..4 {
        d.step();
    }
}

/// The room set to import every picture but IMG_0003 (excluded) into "Card", deleting from the
/// card after copying; Import All; the question open.
fn import_and_ask(dir: &Path) -> Driver {
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir));
    let mut d = driver(dir, Some(picker));
    choose_pictures(&mut d);
    let third = id_of(&mut d, "IMG_0003.JPG");
    click(&mut d, &format!("candidate:{third}"));
    d.key("X").unwrap();
    click(&mut d, "field:import.folderName");
    d.type_text("Card").unwrap();
    d.settle(FRAMES).unwrap();
    click(&mut d, "choice:import.after");
    click(&mut d, "choice:import.after/delete");
    assert_eq!(widget(&d, "choice:import.after").label, "Delete copied files from the card");
    click(&mut d, "button:import.importAll");
    until(&mut d, "the question", |d| try_widget(d, "dialog:removeFromCard").is_some());
    d.settle(FRAMES).unwrap();
    assert_eq!(files(&pictures(dir).join("Card")), ["IMG_0002.JPG", "IMG_0004.JPG", "IMG_0005.JPG"]);
    assert_eq!(d.state().room, Room::Import, "the room waits for the answer");
    d
}

#[test]
fn delete_copied_files_asks_and_keep_files_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = import_and_ask(dir.path());
    assert_eq!(widget(&d, "button:dialog.remove").label, "Remove Imported Files");
    assert_eq!(widget(&d, "button:dialog.cancel").label, "Keep Files");
    check_sizes(&mut d, "dialog:removeFromCard", "dialog-remove-from-card");
    // Enter removes nothing: the question has no default button.
    assert!(dalica_ui::dialogs::answer(d.app_mut(), None).is_err());

    click(&mut d, "button:dialog.cancel");
    assert!(try_widget(&d, "dialog:removeFromCard").is_none());
    assert_eq!(files(&roll(dir.path())).len(), CARD.len(), "Keep Files: nothing left the card");
    assert_eq!(d.state().rooms, [Room::Library], "then the room finishes");
    assert_eq!(status(&mut d)["removal"], Value::Null);
}

#[test]
fn remove_imported_files_removes_only_the_copied_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = import_and_ask(dir.path());
    click(&mut d, "button:dialog.remove");
    until(&mut d, "the removal ended", |d| status(d)["removal"]["running"] == false);
    d.settle(FRAMES).unwrap();
    assert_eq!(files(&roll(dir.path())), ["IMG_0001.JPG", "IMG_0003.JPG"], "the duplicate and the excluded file stay on the card");
    assert_eq!(files(&pictures(dir.path()).join("Card")), ["IMG_0002.JPG", "IMG_0004.JPG", "IMG_0005.JPG"], "every copy stays");
    assert_eq!(d.state().rooms, [Room::Library]);
    assert_eq!(status(&mut d)["undoBlocked"], "The originals were removed from the card.");
}

#[test]
fn the_empty_librarys_import_opens_the_room() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    click(&mut d, "button:empty.import");
    assert_eq!((d.state().rooms.clone(), d.state().room), (vec![Room::Library, Room::Import], Room::Import));
    // No source yet: no destination strip.
    assert!(try_widget(&d, "importDestination").is_none());
}

#[test]
fn a_cancelled_delete_after_import_never_asks() {
    let dir = tempfile::tempdir().unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir.path()));
    let mut d = driver(dir.path(), Some(picker));
    choose_pictures(&mut d);
    click(&mut d, "field:import.folderName");
    d.type_text("Card").unwrap();
    d.settle(FRAMES).unwrap();
    click(&mut d, "choice:import.after");
    click(&mut d, "choice:import.after/delete");
    let (go, wait) = mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let gate = Arc::clone(&wait);
    d.app_mut().session_mut().unwrap().set_import_gate(Some(Arc::new(move |n| {
        if n == 1 {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(30));
        }
    })));
    click_now(&mut d, "button:import.importAll");
    let card = pictures(dir.path()).join("Card");
    until(&mut d, "the second copy made", |d| status(d)["imported"] == 1 && files(&card).contains(&"IMG_0003.JPG".to_string()));
    click_now(&mut d, "button:import.cancel");
    go.send(()).unwrap();
    until(&mut d, "the import ended", |d| status(d)["state"] == "cancelled");
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "dialog:removeFromCard").is_none(), "no question after a cancelled import");
    assert_eq!(files(&roll(dir.path())).len(), CARD.len(), "nothing left the card");
}

#[cfg(unix)]
#[test]
fn an_import_with_a_failure_stays_open_listing_it_and_never_asks() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir.path()));
    let mut d = driver(dir.path(), Some(picker));
    let bad = roll(dir.path()).join("IMG_0004.JPG");
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o000)).unwrap();
    choose_pictures(&mut d);
    click(&mut d, "field:import.folderName");
    d.type_text("Card").unwrap();
    d.settle(FRAMES).unwrap();
    click(&mut d, "choice:import.after");
    click(&mut d, "choice:import.after/delete");
    click(&mut d, "button:import.importAll");
    until(&mut d, "the import ended", |d| status(d)["state"] != "running");
    d.settle(FRAMES).unwrap();
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o644)).unwrap();
    let st = status(&mut d);
    assert_eq!(d.state().room, Room::Import, "the room stays open: {st}");
    assert!(try_widget(&d, "dialog:removeFromCard").is_none(), "no question after a failure");
    assert_eq!(widget(&d, "text:import.result").label, "Imported 3 pictures; 1 could not be imported:", "{st}");
    assert!(widget(&d, "text:import.failure/0").label.starts_with("IMG_0004.JPG: "), "the failed file has its own line");
    assert_eq!(files(&roll(dir.path())).len(), CARD.len(), "nothing left the card");
}

#[test]
fn todays_date_in_the_room_and_an_import_ending_behind_the_library_tab() {
    let dir = tempfile::tempdir().unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(pictures(dir.path()));
    let mut d = driver(dir.path(), Some(picker));
    choose_pictures(&mut d);
    click(&mut d, "choice:import.folderName");
    click(&mut d, "choice:import.folderName/today");
    assert_eq!(widget(&d, "text:import.hint").label, "Pictures will go into a folder named with today's date");
    let (go, wait) = mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let gate = Arc::clone(&wait);
    d.app_mut().session_mut().unwrap().set_import_gate(Some(Arc::new(move |n| {
        if n == 1 {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(30));
        }
    })));
    let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    click_now(&mut d, "button:import.importAll");
    until(&mut d, "the second copy made", |d| status(d)["imported"] == 1);
    // The Library tab in front while the job ends.
    d.run("room.select", &json!({ "room": "library" })).unwrap();
    go.send(()).unwrap();
    until(&mut d, "the import ended", |d| status(d)["state"] != "running");
    for _ in 0..10 {
        d.step();
    }
    assert_eq!(d.state().room, Room::Library, "the library stays in front");
    d.run("room.select", &json!({ "room": "import" })).unwrap();
    until(&mut d, "finished when the room is next drawn", |d| d.state().rooms == [Room::Library]);
    let after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let days: Vec<PathBuf> =
        [before, after].iter().map(|t| pictures(dir.path()).join(dalica_core::DateTime::from_unix_secs(*t).unwrap().date().to_string())).collect();
    let made = days.iter().find(|p| p.exists()).unwrap_or_else(|| panic!("no folder {days:?}"));
    assert_eq!(files(made), ["IMG_0002.JPG", "IMG_0003.JPG", "IMG_0004.JPG", "IMG_0005.JPG"]);
}
