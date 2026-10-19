//! A camera in the Import room (M15.19, `organizing.md` §3 item 1), driven headlessly by
//! automation id with `dalica-scan`'s in-memory responder as the camera: Ctrl+M opens the room on
//! the camera (no card is in), "Import from:" shows its name and lists it beside a camera another
//! program holds, greyed out with its reason; its six pictures fill the tray with the camera's own
//! thumbnails (no picture's bytes are read to list it); the owner thread made no file call.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, new_driver, shot, widget};
use dalica_engine::Session;
use dalica_engine::camera::Opener;
use dalica_scan::devices::{Camera, CameraAccess, CameraStorage};
use dalica_scan::ptp::codes::{format, op};
use dalica_scan::ptp::responder::{self, MemoryCamera};
use dalica_scan::ptp::{Limits, PtpError, Transport, TransportError};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A camera with six pictures in `DCIM/100CANON`, a minute apart from 08:00 on 2026-06-04, each
/// with a thumbnail of the camera's own; the camera, its storage.
fn camera() -> (MemoryCamera, u32) {
    let mut cam = MemoryCamera::new("Canon", "Canon EOS R6");
    let storage = cam.add_storage("SD1", 64 << 30);
    let dcim = cam.add_folder(storage, 0, "DCIM");
    let folder = cam.add_folder(storage, dcim, "100CANON");
    let mut sizes = Vec::new();
    for i in 0..6u64 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 1, 192, 128), 85, None).unwrap();
        sizes.push(bytes.len());
        let thumb = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 1, 160, 120), 80, None).unwrap();
        let h = cam.add_file(storage, folder, &format!("IMG_{:04}.JPG", i + 1), &format!("20260604T08{i:02}00"), bytes);
        let o = cam.object_mut(h).unwrap();
        o.info.thumb_format = format::EXIF_JPEG;
        o.info.thumb_size = thumb.len() as u32;
        o.thumb = thumb;
    }
    sizes.sort_unstable();
    sizes.dedup();
    assert_eq!(sizes.len(), 6, "no two pictures of one size: none needs its bytes read to list it");
    (cam, storage)
}

/// The library window with the camera connected (and a Nikon another program holds); the
/// camera's handle.
fn driver(dir: &std::path::Path) -> (Driver, Arc<Mutex<MemoryCamera>>) {
    let (cam, storage) = camera();
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
    let nikon = Camera {
        id: "usb-1-5".into(),
        vendor: "Nikon".into(),
        model: "Z6".into(),
        serial: String::new(),
        storages: Vec::new(),
        access: CameraAccess::Held("busy".into()),
    };
    let mounts = dir.join("mounts");
    std::fs::create_dir_all(&mounts).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_import_device_roots(Some(std::env::join_paths([&mounts]).unwrap()), Duration::from_millis(50));
    s.set_camera_backend(Some((Box::new(move || vec![canon.clone(), nikon.clone()]), opener)), Duration::from_millis(50));
    s.set_pictures_dir(Some(dir.join("Pictures")));
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    (d, handle)
}

fn listing(d: &mut Driver) -> Value {
    d.app_mut().session_mut().unwrap().execute("import.list", &json!({})).unwrap()
}

/// Steps frames until `done` holds; at most a minute.
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&mut Driver) -> bool) {
    let start = Instant::now();
    while !done(d) {
        assert!(start.elapsed() < Duration::from_secs(60), "never: {what}");
        d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The candidate cells on screen.
fn cells(d: &Driver) -> usize {
    d.widgets().iter().filter(|w| w.id.strip_prefix("candidate:").is_some_and(|n| n.parse::<u64>().is_ok())).count()
}

#[test]
fn the_camera_shows_in_import_from_and_its_pictures_fill_the_tray_with_its_thumbnails() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, cam) = driver(dir.path());
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();

    d.key("Ctrl+M").unwrap();
    until(&mut d, "the camera listed with its thumbnails", |d| {
        let l = listing(d);
        l["state"] == "done" && l["candidates"].as_array().is_some_and(|c| c.len() == 6 && c.iter().all(|c| c["thumbnail"] == true)) && cells(d) == 6
    });
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "choice:import.source").label, "Canon EOS R6");
    assert_eq!(listing(&mut d)["source"]["kind"], "camera");
    assert_eq!(widget(&d, "text:import.status").label, "6 files \u{b7} 6 to import");

    d.click("choice:import.source").unwrap();
    d.settle(FRAMES).unwrap();
    let canon = widget(&d, "choice:import.source/0");
    assert_eq!((canon.label.as_str(), canon.enabled, canon.active), ("Canon EOS R6: Camera or phone", true, true));
    let nikon = widget(&d, "choice:import.source/1");
    assert!(nikon.label.starts_with("Nikon Z6: Another program is using this camera (busy)"), "{}", nikon.label);
    assert!(!nikon.enabled, "a held camera cannot be chosen");
    shot(&d, "import-camera-1400x900-menu");
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();
    shot(&d, "import-camera-1400x900");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no file call");

    let log = responder::lock(&cam).log.clone();
    assert_eq!(log.iter().filter(|c| c.code == op::GET_THUMB).count(), 6, "each picture's thumbnail came from the camera");
    assert!(!log.iter().any(|c| c.code == op::GET_OBJECT || c.code == op::GET_PARTIAL_OBJECT), "no picture was read to list it");
}
