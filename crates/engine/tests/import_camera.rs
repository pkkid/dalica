//! Importing from a camera over PTP (`roadmap.md` M15.19; `organizing.md` §3) with `dalica-scan`'s
//! in-memory responder as the camera: its 40 pictures land with their bytes and the camera's dates
//! as their times; a second listing finds them all duplicates and its import copies nothing; the
//! removal asks first and then deletes exactly the 40 handles copied; a camera unplugged after
//! picture 11 stops the import with 11 kept and the reason. The owner thread is marked as the UI
//! thread while the commands run: it never touches the disk.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::common::{files, names, off_owner, open, read, shared, wait_files};
use crate::import_run::{code, run, shot};
use dalica_engine::Session;
use dalica_engine::camera::Opener;
use dalica_scan::devices::{Camera, CameraAccess, CameraStorage};
use dalica_scan::ptp::codes::op;
use dalica_scan::ptp::responder::{self, MemoryCamera};
use dalica_scan::ptp::{Limits, PtpError, Transport, TransportError};
use serde_json::{Value, json};

/// How often the test's camera list is listed again.
const REFRESH: Duration = Duration::from_millis(50);
/// Long enough for any machine; nothing that ends waits for it.
const PATIENCE: Duration = Duration::from_secs(120);
/// The camera's id in the list.
const ID: &str = "usb-1-4";
/// 2026-06-04T08:00:00, read as UTC.
const FIRST: u64 = 1_780_560_000;

/// A camera holding `n` pictures a minute apart from 08:00 on 2026-06-04 in `DCIM/100CANON`
/// (and a text file, which is no picture); the storage's id.
fn camera(n: u64) -> (MemoryCamera, u32, Vec<(String, Vec<u8>)>) {
    let mut cam = MemoryCamera::new("Canon", "Canon EOS R6");
    let storage = cam.add_storage("SD1", 64 << 30);
    let dcim = cam.add_folder(storage, 0, "DCIM");
    let folder = cam.add_folder(storage, dcim, "100CANON");
    let mut pictures = Vec::new();
    for i in 0..n {
        let name = format!("IMG_{:04}.JPG", i + 1);
        let bytes = shot(i, i as i64 * 60);
        cam.add_file(storage, folder, &name, &format!("20260604T08{:02}00", i % 60), bytes.clone());
        pictures.push((name, bytes));
    }
    cam.add_file(storage, folder, "NOTES.TXT", "20260604T090000", b"not a picture".to_vec());
    (cam, storage, pictures)
}

/// Makes `cam` the session's one camera (listed as connected, its session opened on the first
/// call from a worker); the handle to change it and read its log.
fn attach(s: &mut Session, cam: MemoryCamera, storage: u32) -> Arc<Mutex<MemoryCamera>> {
    let (transport, handle) = cam.connect();
    let slot = Mutex::new(Some(transport));
    let opener: Opener = Arc::new(move |_id: &str| {
        let t = slot.lock().unwrap().take().ok_or(PtpError::Transport(TransportError::Disconnected))?;
        dalica_scan::ptp::Session::open(Box::new(t) as Box<dyn Transport>, Limits::default())
    });
    let listed = Camera {
        id: ID.into(),
        vendor: "Canon".into(),
        model: "Canon EOS R6".into(),
        serial: "0123456789".into(),
        storages: vec![CameraStorage { id: storage, name: "SD1".into(), capacity: 64 << 30, free: 32 << 30 }],
        access: CameraAccess::Open,
    };
    s.set_camera_backend(Some((Box::new(move || vec![listed.clone()]), opener)), REFRESH);
    handle
}

/// Opens the camera's storage (once the camera list shows it) and waits for its listing; the
/// listing's last answer.
fn open_camera(s: &mut Session, storage: u32) -> Value {
    let start = Instant::now();
    loop {
        match s.execute("camera.open", &json!({ "id": ID, "storage": storage })) {
            Ok(_) => break,
            Err(e) => assert!(start.elapsed() < PATIENCE, "the camera never showed: {e}"),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        let v = run(s, "import.list", json!({ "limit": 0 }));
        if v["state"] != "loading" {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "the listing never ended: {v}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn modified(path: &Path) -> SystemTime {
    let path = path.to_path_buf();
    off_owner(move || std::fs::metadata(path).unwrap().modified().unwrap())
}

/// Whether a camera copy's staging folder is in `folder`.
fn staged(folder: &Path) -> bool {
    names(folder).iter().any(|n| n.starts_with(dalica_engine::camera::STAGING))
}

/// The handles DeleteObject was sent for, in order.
fn deleted(cam: &Mutex<MemoryCamera>) -> Vec<u32> {
    responder::lock(cam).log.iter().filter(|c| c.code == op::DELETE_OBJECT).filter_map(|c| c.params.first().copied()).collect()
}

#[test]
fn forty_pictures_import_from_a_camera_then_list_as_duplicates_and_leave_it_after_the_question() {
    let _faults = shared();
    let (lib, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = open(lib.path());
    let (cam, storage, shots) = camera(40);
    let handle = attach(&mut s, cam, storage);
    let dest = pictures.path().join("Imported");

    let ui = dalica_fs::mark_ui_thread();
    let listed = run(&mut s, "camera.list", json!({}));
    let listed = if listed["cameras"].as_array().is_some_and(|c| c.is_empty()) {
        std::thread::sleep(REFRESH * 2);
        run(&mut s, "camera.list", json!({}))
    } else {
        listed
    };
    assert_eq!(listed["cameras"][0]["name"], "Canon EOS R6", "{listed}");
    let first = open_camera(&mut s, storage);
    assert_eq!(
        (first["total"].clone(), first["duplicates"].clone(), first["source"]["kind"].clone()),
        (json!(40), json!(0), json!("camera")),
        "{first}"
    );
    let v = run(&mut s, "import.run", json!({ "destination": dest, "folderName": { "dateTaken": true }, "after": "delete" }));
    assert_eq!(v["total"], 40, "{v}");
    wait_files(&mut s);
    let status = run(&mut s, "import.status", json!({}));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    let day = dest.join("2026-06-04");
    assert_eq!((status["state"].clone(), status["imported"].clone(), status["failed"].clone()), (json!("done"), json!(40), json!([])), "{status}");
    assert_eq!(status["bytes"], json!(shots.iter().map(|(_, b)| b.len() as u64).sum::<u64>()));
    assert_eq!(files(&day).len(), 40, "every picture");
    assert!(!staged(&day), "no staging folder is left: {:?}", names(&day));
    for (i, (name, bytes)) in shots.iter().enumerate() {
        assert_eq!(&read(&day.join(name)), bytes, "{name} holds the camera's bytes");
        let taken = UNIX_EPOCH + Duration::from_secs(FIRST + i as u64 * 60);
        assert_eq!(modified(&day.join(name)), taken, "{name} has the camera's date as its time");
    }
    assert_eq!(s.library().photos().count(), 40, "the copies are library photos");
    assert!(deleted(&handle).is_empty(), "nothing left the camera by itself");

    let ui = dalica_fs::mark_ui_thread();
    let again = open_camera(&mut s, storage);
    assert_eq!((again["duplicates"].clone(), again["toImport"].clone()), (json!(40), json!(0)), "{again}");
    // "Delete copied files from the card": asked first, then exactly the 40 copied.
    assert_eq!(code(&mut s, "import.removeFromSource", json!({})), "disabled");
    assert!(deleted(&handle).is_empty(), "nothing is removed before the answer");
    run(&mut s, "import.removeFromSource", json!({ "confirm": true }));
    wait_files(&mut s);
    let removed = run(&mut s, "import.status", json!({}));
    let skipped = {
        let v = run(&mut s, "import.run", json!({ "destination": dest, "folderName": { "dateTaken": true } }));
        wait_files(&mut s);
        (v, run(&mut s, "import.status", json!({})))
    };
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    assert_eq!(removed["removal"]["removed"], 40, "{removed}");
    let mut gone = deleted(&handle);
    gone.sort_unstable();
    // Handles 1 and 2 are the folders, 3 … 42 the pictures, 43 the text file.
    assert_eq!(gone, (3..=42).collect::<Vec<u32>>(), "one DeleteObject for each copied picture, and no other");
    let left: Vec<String> = responder::lock(&handle).objects.iter().map(|o| o.info.filename.clone()).collect();
    assert_eq!(left, ["DCIM", "100CANON", "NOTES.TXT"], "the folders and the text file stay");
    assert_eq!(files(&day).len(), 40, "the copies stay");
    assert_eq!((skipped.1["imported"].clone(), skipped.1["skipped"]["duplicates"].clone()), (json!(0), json!(40)), "{skipped:?}");
}

#[test]
fn a_camera_unplugged_at_picture_twelve_keeps_eleven_and_says_why() {
    let _faults = shared();
    let (lib, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = open(lib.path());
    let (cam, storage, shots) = camera(40);
    let handle = attach(&mut s, cam, storage);
    let dest = pictures.path().to_path_buf();
    let unplug = Arc::clone(&handle);
    // After the 11th copy (place 10) the cable comes out.
    s.set_import_gate(Some(Arc::new(move |order| {
        if order == 10 {
            responder::lock(&unplug).faults.unplugged = true;
        }
    })));

    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(open_camera(&mut s, storage)["total"], 40);
    run(&mut s, "import.run", json!({ "destination": dest, "folderName": "Trip" }));
    wait_files(&mut s);
    let status = run(&mut s, "import.status", json!({}));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    let why = "The camera was disconnected";
    assert_eq!(
        (status["state"].clone(), status["imported"].clone(), status["stopped"].clone()),
        (json!("cancelled"), json!(11), json!(why)),
        "{status}"
    );
    assert_eq!(status["failed"], json!([{ "file": status["failed"][0]["file"], "reason": why }]), "only picture 12 failed: {status}");
    assert!(status["failed"][0]["file"].as_str().unwrap().ends_with("IMG_0012.JPG"), "{status}");
    let trip = dest.join("Trip");
    let kept = files(&trip);
    assert_eq!(kept, shots[..11].iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(), "the eleven copied stay");
    assert!(!staged(&trip), "no staging folder is left: {:?}", names(&trip));
    assert_eq!(read(&trip.join("IMG_0011.JPG")), shots[10].1);
    assert_eq!(s.library().photos().count(), 11);
    assert_eq!(s.undo_stack().undo_label(), Some("Import"), "what was copied is one undo step");
    let event = s.file_job_events().last().unwrap();
    assert_eq!(event.message, format!("Import stopped: 11 of 40 pictures copied. {why}."));
}
