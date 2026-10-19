// Ported from LightCraft (`crates/engine/src/devices.rs` at 3b4279fc, MIT OR Apache-2.0; see
// `NOTICE`): `DeviceWatch`, `devices_in` and `dcim_in`. Changed for Dalica: drives without a
// `DCIM` folder are listed too (from `volumes`), each device carries its kind and the folder to
// import from, names are display names ("Removable drive (E:)" on Windows), listings are capped,
// links among the mount folders are skipped, and nothing panics.

//! Cards and drives an import can read from (`organizing.md` §3 item 1, `security.md` §5.9).
//!
//! A **card** is a mounted volume holding a `DCIM` folder (the layout every camera writes), under
//! the platform's mount folders or on a drive letter; it imports from that folder. A **drive** is
//! another removable volume of [`volumes`](crate::volumes()) (kind
//! [`VolumeKind::Mounted`]); it imports from its root. The startup disk and network shares are
//! neither.
//!
//! Menus and the Import room's source list ask often, so [`devices`] never touches the disk: it
//! serves the last listing and, when that is older than [`MAX_AGE`] (2 s), starts one background
//! listing (coalesced; no lock is held while it runs). A changed list calls the [`on_change`]
//! hook. [`devices_now`] lists on the calling thread for explicit requests (call it off the UI
//! thread). [`DEVICE_ROOTS_ENV`] replaces the mount folders (tests, agents' sandboxes).
//!
//! On Linux a phone or camera the desktop already mounted over MTP or PTP (gvfs under
//! `$XDG_RUNTIME_DIR/gvfs`, KDE's kio-fuse under `$XDG_RUNTIME_DIR/kio-fuse-*/mtp`) is listed as
//! a drive too ([`mtp_mounts_in`]): a folder source, read like any other.
//!
//! **Cameras** (M15.9): devices with a PTP or MTP interface, spoken to directly over USB
//! ([`crate::ptp`]), are listed beside cards and drives by [`cameras`] (the same cached,
//! background listing): each [`Camera`] with its vendor, model and storages, or, when another
//! program or driver holds it, why it cannot be reached ([`CameraAccess`]). [`open_camera`] opens
//! a session on one. A camera is described once when it appears (a session opened, its storages
//! read, closed again) and kept while it stays plugged in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::ptp::{self, Limits, PtpError, Session, Transport, TransportError};
use crate::volumes::{VOLUMES_CAP, Volume, VolumeKind, children_capped};

/// The environment variable that, when set (paths joined like `PATH`), replaces the platform's
/// volumes with the folders inside these mount folders.
pub const DEVICE_ROOTS_ENV: &str = "DALICA_DEVICE_ROOTS";

/// How long a listing is served before [`devices`] starts another one.
pub const MAX_AGE: Duration = Duration::from_secs(2);

/// Most entries of a volume's root looked at for a `DCIM` folder in another spelling.
const ROOT_ENTRIES_CAP: usize = 4096;

/// What kind of import source a device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// A camera's card: a volume with a `DCIM` folder.
    Card,
    /// Another removable volume.
    Drive,
}

/// A card or drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub kind: DeviceKind,
    /// The name the source list shows: the volume's own name ("EOS_DIGITAL"), or
    /// "Removable drive (E:)" for a drive letter.
    pub name: String,
    /// The volume's root (its mount point or drive).
    pub root: PathBuf,
    /// The folder to import from: a card's `DCIM` folder (in the spelling found on the volume),
    /// a drive's root.
    pub folder: PathBuf,
}

type Lister<T> = dyn Fn() -> Vec<T> + Send + Sync;
type Notify = dyn Fn() + Send + Sync;

struct WatchState<T> {
    list: Vec<T>,
    /// When the served list was made (`None`: never).
    listed: Option<Instant>,
    /// A background listing is running.
    refreshing: bool,
}

impl<T> Default for WatchState<T> {
    fn default() -> Self {
        WatchState { list: Vec::new(), listed: None, refreshing: false }
    }
}

/// A cached device list (cards and drives, or [`Camera`]s), refreshed off the calling thread.
pub struct DeviceWatch<T = Device> {
    lister: Box<Lister<T>>,
    max_age: Duration,
    state: Mutex<WatchState<T>>,
    notify: Mutex<Option<Arc<Notify>>>,
}

impl<T: Clone + PartialEq + Send + 'static> DeviceWatch<T> {
    /// A watch serving what `lister` lists, refreshed when older than `max_age`.
    pub fn new(lister: impl Fn() -> Vec<T> + Send + Sync + 'static, max_age: Duration) -> Arc<DeviceWatch<T>> {
        Arc::new(DeviceWatch { lister: Box::new(lister), max_age, state: Mutex::default(), notify: Mutex::new(None) })
    }

    fn state(&self) -> MutexGuard<'_, WatchState<T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Calls `f` (on the listing thread) whenever a listing changes the list.
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.notify.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
    }

    /// The last listing, at once; starts a background listing when it is stale and none is
    /// running. Empty until the first listing lands.
    pub fn get(self: &Arc<Self>) -> Vec<T> {
        let mut s = self.state();
        if !s.refreshing && s.listed.is_none_or(|t| t.elapsed() >= self.max_age) {
            s.refreshing = true;
            let me = Arc::clone(self);
            // The listing runs without the state lock: `get` only ever waits for a clone.
            let spawned = std::thread::Builder::new().name("dalica-devices".into()).spawn(move || {
                let list = (me.lister)();
                me.store(list);
            });
            if spawned.is_err() {
                s.refreshing = false;
            }
        }
        s.list.clone()
    }

    /// Lists now, on this thread, and serves the result to later [`DeviceWatch::get`] calls.
    pub fn list_now(&self) -> Vec<T> {
        let list = (self.lister)();
        self.store(list.clone());
        list
    }

    fn store(&self, list: Vec<T>) {
        let changed = {
            let mut s = self.state();
            s.refreshing = false;
            s.listed = Some(Instant::now());
            let changed = s.list != list;
            s.list = list;
            changed
        };
        if changed {
            let f = self.notify.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(f) = f {
                f();
            }
        }
    }

    /// A background listing is running.
    pub fn refreshing(&self) -> bool {
        self.state().refreshing
    }
}

fn global() -> &'static Arc<DeviceWatch> {
    static WATCH: OnceLock<Arc<DeviceWatch>> = OnceLock::new();
    WATCH.get_or_init(|| DeviceWatch::new(system_devices, MAX_AGE))
}

/// The cards and drives, without blocking: the last listing, refreshed in the background when
/// older than [`MAX_AGE`] (empty until the first listing lands).
pub fn devices() -> Vec<Device> {
    global().get()
}

/// Lists the cards and drives now, on this thread (it may wait on a slow volume: never the UI
/// thread), and serves the result to later [`devices`] calls.
pub fn devices_now() -> Vec<Device> {
    global().list_now()
}

/// Runs `f` whenever a listing changes the device list (the UI repaints).
pub fn on_change(f: impl Fn() + Send + Sync + 'static) {
    global().on_change(f);
}

/// The devices of this system: those inside the folders [`DEVICE_ROOTS_ENV`] names when it is
/// set, else those among the platform's volumes and (Linux) the desktop's MTP mounts.
fn system_devices() -> Vec<Device> {
    match device_roots(std::env::var_os(DEVICE_ROOTS_ENV)) {
        Some(parents) => devices_in(&parents, &[]),
        None => {
            let mut list = devices_in(&[], &crate::volumes());
            if cfg!(target_os = "linux")
                && let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_absolute())
            {
                for d in mtp_mounts_in(&runtime) {
                    if list.len() < VOLUMES_CAP && !list.iter().any(|x| x.root == d.root) {
                        list.push(d);
                    }
                }
            }
            list
        }
    }
}

/// The mount folders a value of [`DEVICE_ROOTS_ENV`] names (`None` when unset or empty; paths
/// joined like `PATH`, empty parts left out). Does not touch the disk.
pub fn device_roots(value: Option<OsString>) -> Option<Vec<PathBuf>> {
    let value = value.filter(|v| !v.is_empty())?;
    let roots: Vec<PathBuf> = std::env::split_paths(&value).filter(|p| !p.as_os_str().is_empty()).collect();
    (!roots.is_empty()).then_some(roots)
}

/// The devices among the folders inside `parents` (each a folder whose children are mount
/// points, like `/Volumes`; links among them are skipped: macOS lists the startup disk there as a
/// link to `/`), then among `volumes` (only [`VolumeKind::Mounted`] ones), in that order, each root
/// once, at most [`VOLUMES_CAP`].
pub fn devices_in(parents: &[PathBuf], volumes: &[Volume]) -> Vec<Device> {
    let mut roots: Vec<(PathBuf, String)> = Vec::new();
    for parent in parents {
        let Ok(list) = children_capped(parent, VOLUMES_CAP) else { continue };
        roots.extend(list.folders.into_iter().filter(|c| !c.is_link).map(|c| (c.path, c.name)));
    }
    roots.extend(volumes.iter().filter(|v| v.kind == VolumeKind::Mounted).map(|v| (v.path.clone(), display_name(v))));
    let mut out: Vec<Device> = Vec::new();
    for (root, name) in roots {
        if out.len() >= VOLUMES_CAP {
            break;
        }
        if out.iter().any(|d| d.root == root) {
            continue;
        }
        let device = match dcim_in(&root) {
            Some(folder) => Device { kind: DeviceKind::Card, name, root, folder },
            None => Device { kind: DeviceKind::Drive, name, folder: root.clone(), root },
        };
        out.push(device);
    }
    out
}

/// The name the source list shows for `volume`: "Removable drive (E:)" for a drive letter, the
/// volume's own name otherwise.
pub fn display_name(volume: &Volume) -> String {
    let b = volume.name.as_bytes();
    if b.len() == 2 && b.first().is_some_and(u8::is_ascii_alphabetic) && b.get(1) == Some(&b':') {
        format!("Removable drive ({})", volume.name)
    } else {
        volume.name.clone()
    }
}

/// The volume's `DCIM` folder, if it has one: the usual spellings are checked directly, so the
/// root is listed (for other spellings on a case-sensitive volume, at most 4096 entries) only off
/// Windows, whose volumes ignore case. A link named `DCIM` is not followed.
pub fn dcim_in(root: &Path) -> Option<PathBuf> {
    let is_folder = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
    if let Some(p) = ["DCIM", "dcim"].iter().map(|n| root.join(n)).find(|p| is_folder(p)) {
        return Some(p);
    }
    if cfg!(windows) {
        return None;
    }
    std::fs::read_dir(root)
        .ok()?
        .take(ROOT_ENTRIES_CAP)
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("DCIM")) && is_folder(p))
}

/// The phones and cameras the desktop mounted over MTP or PTP, under the runtime folder
/// `runtime` (`$XDG_RUNTIME_DIR`): gvfs's `gvfs/mtp:host=…` and `gvfs/gphoto2:host=…`, and
/// kio-fuse's `kio-fuse-*/mtp/<device>`. Each is a drive (a folder source, imported from its
/// root), named from its mount name; links are not followed; at most [`VOLUMES_CAP`].
pub fn mtp_mounts_in(runtime: &Path) -> Vec<Device> {
    let mut out = Vec::new();
    let folders = |p: &Path| children_capped(p, VOLUMES_CAP).map(|l| l.folders).unwrap_or_default();
    for c in folders(&runtime.join("gvfs")).into_iter().filter(|c| !c.is_link) {
        if let Some(name) = gvfs_name(&c.name) {
            out.push(Device { kind: DeviceKind::Drive, name, folder: c.path.clone(), root: c.path });
        }
    }
    for kio in folders(runtime).into_iter().filter(|c| !c.is_link && c.name.starts_with("kio-fuse-")) {
        for c in folders(&kio.path.join("mtp")).into_iter().filter(|c| !c.is_link) {
            let name = dalica_core::names::sanitize(&c.name, "Phone");
            out.push(Device { kind: DeviceKind::Drive, name, folder: c.path.clone(), root: c.path });
        }
    }
    out.truncate(VOLUMES_CAP);
    out
}

/// The name of a gvfs MTP or PTP mount (`mtp:host=Google_Pixel_7_28041FDH2001Y3` → "Google Pixel
/// 7": percent escapes decoded, underscores as spaces, and with three or more parts the last one,
/// the serial number, left out). `None` for a mount of another kind.
pub fn gvfs_name(mount: &str) -> Option<String> {
    let host = mount.strip_prefix("mtp:host=").or_else(|| mount.strip_prefix("gphoto2:host="))?;
    let bytes = host.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |at: usize| bytes.get(at).and_then(|c| (*c as char).to_digit(16));
        match (b, hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                decoded.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
                i += 3;
            }
            _ => {
                decoded.push(b);
                i += 1;
            }
        }
    }
    let text = String::from_utf8_lossy(&decoded);
    let mut parts: Vec<&str> = text.split('_').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 {
        parts.pop();
    }
    Some(dalica_core::names::sanitize(&parts.join(" "), "Phone"))
}

/// Whether a camera can be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraAccess {
    /// A session opened: its storages are listed.
    Open,
    /// Another program or a driver holds it (the desktop's MTP service, the system's
    /// portable-device driver), with what the system said.
    Held(String),
    /// This user may not open it, with what the system said.
    Denied(String),
    /// It answered wrongly or not at all.
    Failed(String),
}

/// A storage of a camera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraStorage {
    pub id: u32,
    /// Its label, else its description (cleaned).
    pub name: String,
    pub capacity: u64,
    pub free: u64,
}

/// A camera or phone with a PTP or MTP interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
    /// The system's id for it while it stays plugged in ([`open_camera`] takes it).
    pub id: String,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    /// Its storages (empty unless [`CameraAccess::Open`]).
    pub storages: Vec<CameraStorage>,
    pub access: CameraAccess,
}

impl Camera {
    /// The name the source list shows: "Vendor Model", the model alone when it already names the
    /// vendor.
    pub fn name(&self) -> String {
        if self.vendor.is_empty() || self.model.to_lowercase().starts_with(&self.vendor.to_lowercase()) {
            self.model.clone()
        } else {
            format!("{} {}", self.vendor, self.model)
        }
    }
}

/// Describes the camera behind `transport`: opens a session, reads its info and storages, closes
/// it. `fallback` gives the vendor and model when the device cannot be reached.
pub fn describe<T: Transport>(id: &str, transport: Result<T, TransportError>, fallback: (&str, &str), limits: Limits) -> Camera {
    let mut camera = Camera {
        id: id.to_string(),
        vendor: dalica_core::names::sanitize(fallback.0, "Camera"),
        model: dalica_core::names::sanitize(fallback.1, "Camera"),
        serial: String::new(),
        storages: Vec::new(),
        access: CameraAccess::Open,
    };
    let opened = transport.map_err(PtpError::from).and_then(|t| Session::open(t, limits));
    let mut session = match opened {
        Ok(s) => s,
        Err(e) => {
            camera.access = access_of(&e);
            return camera;
        }
    };
    let info = session.device_info().clone();
    if !info.manufacturer.trim().is_empty() {
        camera.vendor = dalica_core::names::sanitize(&info.manufacturer, "Camera");
    }
    if !info.model.trim().is_empty() {
        camera.model = dalica_core::names::sanitize(&info.model, "Camera");
    }
    if !info.serial.trim().is_empty() {
        camera.serial = dalica_core::names::sanitize(&info.serial, "");
    }
    match session.storage_ids() {
        Ok(ids) => {
            for id in ids {
                if let Ok(s) = session.storage_info(id) {
                    let storage = ptp::Storage { id, info: s };
                    camera.storages.push(CameraStorage {
                        id,
                        name: storage.name(),
                        capacity: storage.info.max_capacity,
                        free: storage.info.free_bytes,
                    });
                }
            }
        }
        Err(e) => camera.access = access_of(&e),
    }
    session.close();
    camera
}

fn access_of(e: &PtpError) -> CameraAccess {
    match e {
        PtpError::Transport(TransportError::Busy(why)) => CameraAccess::Held(why.clone()),
        PtpError::Transport(TransportError::Denied(why)) => CameraAccess::Denied(why.clone()),
        e => CameraAccess::Failed(e.to_string()),
    }
}

fn camera_watch() -> &'static Arc<DeviceWatch<Camera>> {
    static WATCH: OnceLock<Arc<DeviceWatch<Camera>>> = OnceLock::new();
    WATCH.get_or_init(|| DeviceWatch::new(system_cameras, MAX_AGE))
}

/// The cameras and phones, without blocking: the last listing, refreshed in the background when
/// older than [`MAX_AGE`] (empty until the first listing lands).
pub fn cameras() -> Vec<Camera> {
    camera_watch().get()
}

/// Lists the cameras now, on this thread (it opens new devices: never the UI thread), and serves
/// the result to later [`cameras`] calls.
pub fn cameras_now() -> Vec<Camera> {
    camera_watch().list_now()
}

/// Runs `f` whenever a listing changes the camera list.
pub fn on_cameras_change(f: impl Fn() + Send + Sync + 'static) {
    camera_watch().on_change(f);
}

/// The cameras on USB: each described once when it appears, kept while it stays plugged in
/// (one that could not be reached is tried again at each listing).
#[cfg(all(feature = "usb", any(target_os = "linux", target_os = "windows")))]
fn system_cameras() -> Vec<Camera> {
    static KNOWN: Mutex<Vec<Camera>> = Mutex::new(Vec::new());
    let Ok(found) = ptp::usb::usb_cameras() else { return Vec::new() };
    let known = KNOWN.lock().map(|k| k.clone()).unwrap_or_else(|e| e.into_inner().clone());
    let list: Vec<Camera> = found
        .iter()
        .map(|c| match known.iter().find(|k| k.id == c.id && k.access == CameraAccess::Open) {
            Some(k) => k.clone(),
            None => {
                let fallback = (c.manufacturer.as_deref().unwrap_or("Camera"), c.product.as_deref().unwrap_or("Camera"));
                describe(&c.id, ptp::usb::open(c), fallback, Limits::default())
            }
        })
        .collect();
    *KNOWN.lock().unwrap_or_else(|e| e.into_inner()) = list.clone();
    list
}

#[cfg(not(all(feature = "usb", any(target_os = "linux", target_os = "windows"))))]
fn system_cameras() -> Vec<Camera> {
    Vec::new()
}

/// Opens a session on the camera `id` (from [`cameras`]).
#[cfg(all(feature = "usb", any(target_os = "linux", target_os = "windows")))]
pub fn open_camera(id: &str, limits: Limits) -> Result<Session<Box<dyn Transport>>, PtpError> {
    let found = ptp::usb::usb_cameras()?;
    let camera = found.iter().find(|c| c.id == id).ok_or(PtpError::Transport(TransportError::Disconnected))?;
    let transport: Box<dyn Transport> = Box::new(ptp::usb::open(camera)?);
    Session::open(transport, limits)
}

/// Opens a session on the camera `id`: no USB transport on this system (see the crate README).
#[cfg(not(all(feature = "usb", any(target_os = "linux", target_os = "windows"))))]
pub fn open_camera(id: &str, limits: Limits) -> Result<Session<Box<dyn Transport>>, PtpError> {
    let _ = (id, limits);
    Err(PtpError::Transport(TransportError::Other("cameras over USB are not reachable on this system yet".into())))
}
