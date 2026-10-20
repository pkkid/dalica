//! A camera imported through the Import room (`m15.rs` part 5; `roadmap.md` M15 → OS
//! integration's *Done when*: "a camera (the in-memory PTP responder) lists its pictures and
//! imports them verified with their times"), in process: `dalica-scan`'s in-memory PTP responder
//! is the session's one camera, reached over the PTP reader as a USB camera is, and the window's
//! own keys and clicks drive `dalica_ui`'s headless `Driver`. Ctrl+M opens the room on the camera
//! ("Import from:" naming it), its five pictures fill the tray, Import All (the folder named by the
//! date taken) copies them into `Pictures/2026-06-04`, each byte for byte and with the camera's date as its modification time, and the
//! copies join the library; the camera keeps its pictures.
//!
//! A real camera or phone is imported by hand on each OS (`m15.md`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_engine::Session;
use dalica_engine::camera::Opener;
use dalica_scan::devices::{Camera, CameraAccess, CameraStorage};
use dalica_scan::ptp::codes::op;
use dalica_scan::ptp::responder::{self, MemoryCamera};
use dalica_scan::ptp::{Limits, PtpError, Transport, TransportError};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

use crate::common::LARGE;

const FRAMES: usize = 3600;
const PATIENCE: Duration = Duration::from_secs(300);
/// The pictures on the camera.
const PICTURES: u64 = 5;
/// 2026-06-04T08:00:00, the first picture's date (the camera's dates are read as UTC).
const FIRST: u64 = 1_780_560_000;

/// Steps frames until `done` holds; at most [`PATIENCE`].
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&mut Driver) -> bool) {
    let start = Instant::now();
    while !done(d) {
        assert!(start.elapsed() < PATIENCE, "never: {what}");
        d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn execute(d: &mut Driver, command: &str) -> Value {
    d.app_mut().session_mut().unwrap().execute(command, &json!({})).unwrap()
}

/// Every file under `dir` with its path.
fn tree(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn import(home: &Path) {
    let mut cam = MemoryCamera::new("Canon", "Canon EOS R6");
    let storage = cam.add_storage("SD1", 64 << 30);
    let dcim = cam.add_folder(storage, 0, "DCIM");
    let folder = cam.add_folder(storage, dcim, "100CANON");
    let mut shots = Vec::new();
    for i in 0..PICTURES {
        let name = format!("IMG_{:04}.JPG", i + 1);
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 1, 192 + 8 * i as u32, 128), 85, None).unwrap();
        cam.add_file(storage, folder, &name, &format!("20260604T08{i:02}00"), bytes.clone());
        shots.push((name, bytes));
    }
    let (transport, handle) = cam.connect();
    let slot = Mutex::new(Some(transport));
    let opener: Opener = Arc::new(move |_id: &str| {
        let t = slot.lock().unwrap().take().ok_or(PtpError::Transport(TransportError::Disconnected))?;
        dalica_scan::ptp::Session::open(Box::new(t) as Box<dyn Transport>, Limits::default())
    });
    let canon = Camera {
        id: "usb-1-4".into(),
        vendor: "Canon".into(),
        model: "Canon EOS R6".into(),
        serial: String::new(),
        storages: vec![CameraStorage { id: storage, name: "SD1".into(), capacity: 64 << 30, free: 32 << 30 }],
        access: CameraAccess::Open,
    };
    let mounts = home.join("mounts");
    std::fs::create_dir_all(&mounts).unwrap();
    let pictures = home.join("Pictures");
    let mut s = Session::open(&home.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_import_device_roots(Some(std::env::join_paths([&mounts]).unwrap()), Duration::from_millis(50));
    s.set_camera_backend(Some((Box::new(move || vec![canon.clone()]), opener)), Duration::from_millis(50));
    s.set_pictures_dir(Some(pictures.clone()));
    let mut d = Driver::new(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();

    d.key("Ctrl+M").unwrap();
    until(&mut d, "the camera's pictures listed", |d| {
        let l = execute(d, "import.list");
        l["state"] == "done" && l["candidates"].as_array().is_some_and(|c| c.len() == PICTURES as usize)
    });
    d.settle(FRAMES).unwrap();
    let source = d.widgets().into_iter().find(|w| w.id == "choice:import.source").expect("Import from:");
    assert_eq!(source.label, "Canon EOS R6", "Import from: names the camera");
    assert_eq!(execute(&mut d, "import.list")["source"]["kind"], "camera");

    // "Folder name:" ▸ Date taken (no name, no import), then Import All.
    for id in ["choice:import.folderName", "choice:import.folderName/dateTaken", "button:import.importAll"] {
        d.click(id).unwrap();
        d.settle(FRAMES).unwrap();
    }
    until(&mut d, "the import done", |d| execute(d, "import.status")["state"] == "done");
    let status = execute(&mut d, "import.status");
    assert_eq!((&status["imported"], &status["failed"]), (&json!(PICTURES), &json!([])), "{status}");
    until(&mut d, "the copies in the library", |d| d.app().session().unwrap().library().photos().count() == PICTURES as usize);

    let copies = tree(&pictures.join("2026-06-04"));
    assert_eq!(copies.len(), PICTURES as usize, "{copies:?}");
    for (i, (name, bytes)) in shots.iter().enumerate() {
        let copy = copies.iter().find(|p| p.file_name().is_some_and(|n| n == name.as_str())).unwrap_or_else(|| panic!("{name} copied: {copies:?}"));
        assert_eq!(&std::fs::read(copy).unwrap(), bytes, "{name}: the camera's bytes");
        let taken = UNIX_EPOCH + Duration::from_secs(FIRST + i as u64 * 60);
        assert_eq!(std::fs::metadata(copy).unwrap().modified().unwrap(), taken, "{name}: the camera's date as its time");
    }
    let log = responder::lock(&handle).log.clone();
    assert!(!log.iter().any(|c| c.code == op::DELETE_OBJECT), "the camera keeps its pictures");
}
