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

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

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

type Lister = dyn Fn() -> Vec<Device> + Send + Sync;
type Notify = dyn Fn() + Send + Sync;

#[derive(Default)]
struct WatchState {
    list: Vec<Device>,
    /// When the served list was made (`None`: never).
    listed: Option<Instant>,
    /// A background listing is running.
    refreshing: bool,
}

/// A cached device list, refreshed off the calling thread.
pub struct DeviceWatch {
    lister: Box<Lister>,
    max_age: Duration,
    state: Mutex<WatchState>,
    notify: Mutex<Option<Arc<Notify>>>,
}

impl DeviceWatch {
    /// A watch serving what `lister` lists, refreshed when older than `max_age`.
    pub fn new(lister: impl Fn() -> Vec<Device> + Send + Sync + 'static, max_age: Duration) -> Arc<DeviceWatch> {
        Arc::new(DeviceWatch { lister: Box::new(lister), max_age, state: Mutex::default(), notify: Mutex::new(None) })
    }

    fn state(&self) -> MutexGuard<'_, WatchState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Calls `f` (on the listing thread) whenever a listing changes the list.
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.notify.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
    }

    /// The last listing, at once; starts a background listing when it is stale and none is
    /// running. Empty until the first listing lands.
    pub fn get(self: &Arc<Self>) -> Vec<Device> {
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
    pub fn list_now(&self) -> Vec<Device> {
        let list = (self.lister)();
        self.store(list.clone());
        list
    }

    fn store(&self, list: Vec<Device>) {
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
/// set, else those among the platform's volumes.
fn system_devices() -> Vec<Device> {
    match device_roots(std::env::var_os(DEVICE_ROOTS_ENV)) {
        Some(parents) => devices_in(&parents, &[]),
        None => devices_in(&[], &crate::volumes()),
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
