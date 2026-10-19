//! The PTP/MTP reader (M15.9) against the in-memory responder: listing a camera, reading objects
//! in chunks, hostile devices refused or timed out without a panic, deletes sent only for the
//! handle asked, the desktop's MTP mounts as drives, random containers.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_scan::devices::{CameraAccess, DeviceKind, describe, gvfs_name, mtp_mounts_in};
use dalica_scan::ptp::codes::op;
use dalica_scan::ptp::responder::{MemoryCamera, MemoryTransport, lock};
use dalica_scan::ptp::{self, Limits, ObjectKind, PtpError, Session, Transport, TransportError, container, dataset, parse_date};
use proptest::prelude::*;

use crate::common::full;

/// Bytes of the `i`th test object: its own pattern, `len` long.
fn bytes(i: usize, len: usize) -> Vec<u8> {
    (0..len).map(|k| (k.wrapping_mul(31).wrapping_add(i.wrapping_mul(7)) % 251) as u8).collect()
}

fn quick_limits() -> Limits {
    Limits { timeout: Duration::from_millis(20), transaction: Duration::from_millis(400), ..Limits::default() }
}

/// A camera with two cards, each with `DCIM/100TEST` holding `per_card` JPEGs.
fn camera(per_card: usize) -> MemoryCamera {
    let mut cam = MemoryCamera::new("Acme", "Acme Shooter 5");
    for card in 0..2 {
        let storage = cam.add_storage(&format!("CARD{card}"), 32 << 30);
        let dcim = cam.add_folder(storage, 0, "DCIM");
        let folder = cam.add_folder(storage, dcim, "100TEST");
        for i in 0..per_card {
            let n = card * per_card + i;
            let date = format!("2026{:02}{:02}T{:02}{:02}{:02}", 1 + n % 12, 1 + n % 28, n % 24, n % 60, (n * 7) % 60);
            cam.add_file(storage, folder, &format!("IMG_{n:04}.JPG"), &date, bytes(n, 100 + n));
        }
    }
    cam
}

fn open(cam: MemoryCamera, limits: Limits) -> (Session<MemoryTransport>, Arc<Mutex<MemoryCamera>>) {
    let (t, shared) = cam.connect();
    (Session::open(t, limits).unwrap(), shared)
}

#[test]
fn a_camera_with_two_storages_lists_every_picture_with_its_name_size_and_date() {
    let (mut s, _) = open(camera(150), Limits::default());
    let listing = ptp::list(&mut s, &|| true).unwrap();
    assert_eq!((listing.vendor.as_str(), listing.model.as_str()), ("Acme", "Acme Shooter 5"));
    assert_eq!(listing.storages.iter().map(|s| s.name()).collect::<Vec<_>>(), ["CARD0", "CARD1"]);
    assert_eq!(listing.objects.len(), 300, "folders are left out");
    assert_eq!(listing.pictures().count(), 300);
    assert!(!listing.truncated && listing.warnings.is_empty());
    for (n, o) in listing.objects.iter().enumerate() {
        assert_eq!(o.name, format!("IMG_{n:04}.JPG"));
        assert_eq!(o.size, Some(100 + n as u64));
        assert_eq!(o.kind(), ObjectKind::Picture);
        let d = o.captured.unwrap();
        assert_eq!((d.date().month() as usize, d.date().day() as usize, d.hour() as usize), (1 + n % 12, 1 + n % 28, n % 24));
        assert_eq!(o.storage, listing.storages[n / 150].id);
    }
    s.close();

    // The device list describes it: vendor, model, storages; a device held elsewhere says so.
    let (t, _) = camera(1).connect();
    let c = describe("usb-1", Ok(t), ("", ""), Limits::default());
    assert_eq!((c.name().as_str(), c.access.clone(), c.storages.len()), ("Acme Shooter 5", CameraAccess::Open, 2));
    assert_eq!((c.storages[1].name.as_str(), c.storages[1].capacity), ("CARD1", 32 << 30));
    let held = describe::<MemoryTransport>("usb-2", Err(TransportError::Busy("in use".into())), ("Google", "Pixel 7"), Limits::default());
    assert_eq!((held.name().as_str(), held.access), ("Google Pixel 7", CameraAccess::Held("in use".into())));
}

#[test]
fn get_object_in_chunks_gives_the_exact_bytes() {
    let mut cam = camera(0);
    let storage = cam.storages[0].0;
    let size = if full() { 3 << 20 } else { 200_000 };
    let data = bytes(9, size);
    let h = cam.add_file(storage, 0, "BIG.JPG", "20260604T194210", data.clone());
    cam.object_mut(h).unwrap().thumb = vec![0xFF, 0xD8, 0xFF, 0xD9];
    for no_partial in [false, true] {
        let mut cam = cam.clone();
        cam.faults.no_partial = no_partial;
        let limits = Limits { chunk: 7000, ..Limits::default() };
        let (mut s, shared) = open(cam, limits);
        let mut got = Vec::new();
        let n = s
            .read_object(h, size as u64, &mut |p| {
                got.extend_from_slice(p);
                true
            })
            .unwrap();
        assert_eq!((n, got == data), (size as u64, true), "no_partial {no_partial}");
        let partials = lock(&shared).log.iter().filter(|c| c.code == op::GET_PARTIAL_OBJECT).count();
        // Every chunk, then one byte past the end to check the object is no larger.
        assert_eq!(partials, if no_partial { 0 } else { size.div_ceil(7000) + 1 });
        assert_eq!(s.thumb(h).unwrap(), [0xFF, 0xD8, 0xFF, 0xD9]);
    }
}

#[test]
fn hostile_devices_are_refused_or_timed_out_without_a_panic() {
    // A container length shorter than its header.
    let mut cam = camera(2);
    cam.faults.short_length = Some(op::GET_OBJECT_INFO);
    let (mut s, _) = open(cam, quick_limits());
    assert!(matches!(s.object_info(4), Err(PtpError::Malformed(_))));
    assert_eq!(s.object_info(4), Err(PtpError::Broken), "a broken session refuses what follows");

    // A handle count of 2³² − 1.
    let mut cam = camera(2);
    cam.faults.handle_count = Some(u32::MAX);
    let (mut s, _) = open(cam, quick_limits());
    let storage = s.storage_ids().unwrap()[0];
    assert!(matches!(s.object_handles(storage), Err(PtpError::TooLarge(_))));

    // An object larger than its info: chunked, streamed whole, and a chunk larger than asked.
    for (no_partial, oversend) in [(false, 0), (true, 0), (false, 5)] {
        let mut cam = camera(2);
        cam.faults.no_partial = no_partial;
        cam.faults.oversend = oversend;
        let info_size = if oversend > 0 { 102 } else { 92 };
        cam.object_mut(4).unwrap().info.compressed_size = info_size; // it holds 101
        let (mut s, _) = open(cam, Limits { chunk: 50, ..quick_limits() });
        let r = s.read_object(4, u64::from(info_size), &mut |_| true);
        assert!(matches!(r, Err(PtpError::TooLarge(_))), "no_partial {no_partial} oversend {oversend}: {r:?}");
    }

    // A size the device says does not fit 32 bits, and one past 4 GiB, are never read.
    let mut cam = camera(2);
    cam.object_mut(4).unwrap().info.compressed_size = u32::MAX;
    let (mut s, _) = open(cam, quick_limits());
    let listing = ptp::list(&mut s, &|| true).unwrap();
    assert_eq!(listing.objects.iter().find(|o| o.handle == 4).unwrap().size, None);
    assert!(matches!(s.read_object(4, 5 << 30, &mut |_| true), Err(PtpError::TooLarge(_))));

    // A device that stops answering mid-listing is timed out.
    let mut cam = camera(20);
    cam.faults.silent_after = Some(10);
    let (mut s, _) = open(cam, quick_limits());
    let start = Instant::now();
    assert_eq!(ptp::list(&mut s, &|| true), Err(PtpError::TimedOut));
    assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());

    // An unplugged device.
    let (mut s, shared) = open(camera(2), quick_limits());
    lock(&shared).faults.unplugged = true;
    assert_eq!(s.object_info(4), Err(PtpError::Transport(TransportError::Disconnected)));
}

#[test]
fn delete_object_is_sent_only_for_the_handle_asked() {
    let (mut s, shared) = open(camera(3), Limits::default());
    assert!(matches!(s.delete_object(u32::MAX), Err(PtpError::Refused(_))));
    assert!(matches!(s.delete_object(0), Err(PtpError::Refused(_))));
    s.delete_object(5).unwrap();
    let cam = lock(&shared);
    let deletes: Vec<&Vec<u32>> = cam.log.iter().filter(|c| c.code == op::DELETE_OBJECT).map(|c| &c.params).collect();
    assert_eq!(deletes, [&vec![5]]);
    assert!(cam.objects.iter().all(|o| o.handle != 5));
    assert_eq!(cam.objects.len(), 2 * (2 + 3) - 1);
}

#[test]
fn object_dates_parse_from_their_strings() {
    let t = |s: &str| parse_date(s).map(|d| d.to_string());
    assert_eq!(t("20260604T194210").as_deref(), Some("2026-06-04T19:42:10"));
    assert_eq!(t("20260604T194210.5").as_deref(), Some("2026-06-04T19:42:10"));
    assert_eq!(t("20260604T194210Z").as_deref(), Some("2026-06-04T19:42:10"));
    assert_eq!(t("20260604T194210.0+0530").as_deref(), Some("2026-06-04T19:42:10"));
    for bad in ["", "2026-06-04T19:42:10", "20260230T120000", "20260604T246000", "20260604T194210+2500", "20260604T194210.", "２0260604T194210"] {
        assert_eq!(t(bad), None, "{bad}");
    }
}

#[test]
fn the_desktops_mtp_mounts_are_drives() {
    assert_eq!(gvfs_name("mtp:host=Google_Pixel_7_28041FDH2001Y3").as_deref(), Some("Google Pixel 7"));
    assert_eq!(gvfs_name("gphoto2:host=Canon_EOS%20R6").as_deref(), Some("Canon EOS R6"));
    assert_eq!(gvfs_name("smb-share:server=nas,share=photos"), None);
    if cfg!(windows) {
        return; // gvfs's names hold `:`, which Windows forbids; the mounts are Linux's
    }
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path();
    for p in
        ["gvfs/mtp:host=Google_Pixel_7_28041FDH2001Y3/Internal shared storage/DCIM", "gvfs/sftp:host=nas", "kio-fuse-AbCd/mtp/Galaxy S24/Phone/DCIM"]
    {
        std::fs::create_dir_all(run.join(Path::new(p))).unwrap();
    }
    let got: Vec<(String, DeviceKind, bool)> = mtp_mounts_in(run).into_iter().map(|d| (d.name, d.kind, d.folder == d.root)).collect();
    assert_eq!(got, [("Google Pixel 7".to_string(), DeviceKind::Drive, true), ("Galaxy S24".to_string(), DeviceKind::Drive, true)]);
}

/// A device that answers anything with the given transfers, then times out.
struct Script(std::collections::VecDeque<Vec<u8>>);

impl Transport for Script {
    fn send(&mut self, _: &[u8], _: Duration) -> Result<(), TransportError> {
        Ok(())
    }
    fn receive(&mut self, buf: &mut [u8], _: Duration) -> Result<usize, TransportError> {
        let next = self.0.pop_front().ok_or(TransportError::TimedOut)?;
        let n = next.len().min(buf.len());
        buf[..n].copy_from_slice(&next[..n]);
        Ok(n)
    }
}

/// A container with a random header (often a plausible one) and payload.
fn any_container() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<bool>(),
        0u32..40,
        1u16..6,
        prop::sample::select(vec![0x1001u16, 0x1004, 0x1005, 0x1007, 0x1008, 0x1009, 0x101B, 0x2001, 0x2002, 0x4002]),
        0u32..4,
        prop::collection::vec(any::<u8>(), 0..300),
    )
        .prop_map(|(honest, len_off, kind, code, tid, payload)| {
            let len = if honest { payload.len() as u32 + 12 } else { (payload.len() as u32).wrapping_add(len_off).wrapping_sub(20) };
            let mut b = len.to_le_bytes().to_vec();
            b.extend_from_slice(&kind.to_le_bytes());
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(&tid.to_le_bytes());
            b.extend_from_slice(&payload);
            b
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if full() { 4096 } else { 32 }))]

    #[test]
    fn random_containers_never_panic(stream in prop::collection::vec(any_container(), 0..12), cut in 1usize..700) {
        let _ = container::parse(stream.first().map(Vec::as_slice).unwrap_or_default());
        for c in &stream {
            let _ = dataset::DeviceInfo::parse(c);
            let _ = dataset::ObjectInfo::parse(c);
            let _ = dataset::StorageInfo::parse(c);
        }
        let joined: Vec<u8> = stream.concat();
        let transfers = joined.chunks(cut).map(<[u8]>::to_vec).collect();
        let limits = Limits { timeout: Duration::from_millis(1), transaction: Duration::from_millis(50), max_handles: 64, ..Limits::default() };
        if let Ok(mut s) = Session::open(Script(transfers), limits) {
            let _ = s.storage_ids();
            let _ = s.object_handles(0x10001);
            let _ = s.object_info(1);
            let _ = s.read_object(1, 100, &mut |_| true);
            let _ = ptp::list(&mut s, &|| true);
        }
    }
}
