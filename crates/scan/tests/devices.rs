//! Cards and drives (`organizing.md` §3 item 1): temp folders as mount folders, the way
//! `DALICA_DEVICE_ROOTS` makes the app see them; the cached list never blocks its caller.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dalica_scan::devices::{Device, DeviceKind, DeviceWatch, device_roots, devices_in, display_name};
use dalica_scan::{Volume, VolumeKind};

#[test]
fn a_volume_with_dcim_is_a_card_and_one_without_is_a_drive() {
    let base = tempfile::tempdir().unwrap();
    let b = base.path();
    std::fs::create_dir_all(b.join("EOS_DIGITAL/DCIM/100CANON")).unwrap();
    std::fs::create_dir_all(b.join("SD/dcim")).unwrap();
    std::fs::create_dir_all(b.join("STICK/Photos")).unwrap();
    // A file named DCIM is no camera folder.
    std::fs::create_dir_all(b.join("ODD")).unwrap();
    std::fs::write(b.join("ODD/DCIM"), b"not a folder").unwrap();
    #[cfg(unix)]
    {
        // macOS lists the startup disk among the volumes as a link to `/`.
        std::os::unix::fs::symlink("/", b.join("Macintosh HD")).unwrap();
        // Mixed case only on a volume that tells case apart.
        std::fs::create_dir_all(b.join("X/Dcim")).unwrap();
    }
    let devices = devices_in(&[b.to_path_buf(), b.join("missing")], &[]);
    let kinds: Vec<(&str, DeviceKind)> = devices.iter().map(|d| (d.name.as_str(), d.kind)).collect();
    let mut want = vec![("EOS_DIGITAL", DeviceKind::Card), ("ODD", DeviceKind::Drive), ("SD", DeviceKind::Card), ("STICK", DeviceKind::Drive)];
    if cfg!(unix) {
        want.push(("X", DeviceKind::Card));
    }
    assert_eq!(kinds, want, "{devices:?}");
    for d in &devices {
        assert_eq!(d.root, b.join(&d.name));
        match d.kind {
            // The DCIM folder itself, in the spelling found on the volume.
            DeviceKind::Card => {
                assert!(d.folder.is_dir() && d.folder.file_name().unwrap().eq_ignore_ascii_case("DCIM"), "{d:?}");
                assert_eq!(d.folder.parent(), Some(d.root.as_path()));
            }
            DeviceKind::Drive => assert_eq!(d.folder, d.root),
        }
    }
}

#[test]
fn only_mounted_volumes_are_devices_and_drive_letters_get_a_display_name() {
    let base = tempfile::tempdir().unwrap();
    let card = base.path().join("card");
    std::fs::create_dir_all(card.join("DCIM")).unwrap();
    let vol = |name: &str, path: PathBuf, kind| Volume { name: name.into(), path, kind };
    let volumes = [
        vol("/", base.path().to_path_buf(), VolumeKind::System),
        vol("share", base.path().join("share"), VolumeKind::Network),
        vol("E:", card.clone(), VolumeKind::Mounted),
        vol("BACKUP", base.path().join("backup"), VolumeKind::Mounted),
    ];
    let devices = devices_in(&[], &volumes);
    let got: Vec<(&str, DeviceKind)> = devices.iter().map(|d| (d.name.as_str(), d.kind)).collect();
    assert_eq!(got, [("Removable drive (E:)", DeviceKind::Card), ("BACKUP", DeviceKind::Drive)]);
    assert_eq!(display_name(&vol("EOS_DIGITAL", card, VolumeKind::Mounted)), "EOS_DIGITAL");
    // A root found among the mount folders and among the volumes is listed once.
    assert_eq!(devices_in(&[base.path().to_path_buf()], &volumes).iter().filter(|d| d.root == base.path().join("card")).count(), 1);
}

#[test]
fn the_override_names_mount_folders_like_path() {
    assert_eq!(device_roots(None), None);
    assert_eq!(device_roots(Some(OsString::new())), None);
    let joined = std::env::join_paths([PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]).unwrap();
    assert_eq!(device_roots(Some(joined)), Some(vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]));
}

/// Waits (bounded) for `f`.
fn until(f: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn a_slow_listing_never_blocks_and_a_new_card_still_arrives() {
    const SLOW: Duration = Duration::from_millis(400);
    let base = tempfile::tempdir().unwrap();
    let parents = vec![base.path().to_path_buf()];
    let runs = Arc::new(AtomicUsize::new(0));
    let r = runs.clone();
    // A lister as slow as a sleeping volume, over real folders.
    let watch = DeviceWatch::new(
        move || {
            r.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(SLOW);
            devices_in(&parents, &[])
        },
        Duration::from_millis(20),
    );
    let changes = Arc::new(AtomicUsize::new(0));
    let c = changes.clone();
    watch.on_change(move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    // Cold: served at once (empty) while one listing runs in the background.
    let t = Instant::now();
    for _ in 0..50 {
        assert!(watch.get().is_empty());
    }
    assert!(t.elapsed() < SLOW / 2, "get() waited for the listing: {:?}", t.elapsed());
    assert!(until(|| !watch.refreshing()));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "requests while listing are coalesced");
    assert_eq!(changes.load(Ordering::SeqCst), 0, "nothing changed");
    // A card is put in: the next stale request lists again, still without waiting.
    std::fs::create_dir_all(base.path().join("EOS_DIGITAL/DCIM")).unwrap();
    std::thread::sleep(Duration::from_millis(40));
    let t = Instant::now();
    assert!(watch.get().is_empty(), "the old list is served while listing");
    assert!(t.elapsed() < SLOW / 2, "a warm get() waited: {:?}", t.elapsed());
    assert!(until(|| changes.load(Ordering::SeqCst) == 1), "a changed list notifies");
    let got: Vec<Device> = watch.get();
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].name.as_str(), got[0].kind), ("EOS_DIGITAL", DeviceKind::Card));
    // Taken out: an explicit request lists on the calling thread.
    assert!(until(|| !watch.refreshing()));
    std::fs::remove_dir_all(base.path().join("EOS_DIGITAL")).unwrap();
    assert!(watch.list_now().is_empty());
    assert_eq!(changes.load(Ordering::SeqCst), 2);
}
