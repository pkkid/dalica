//! Cameras and phones as import sources (`organizing.md` §3; `roadmap.md` M15 → *Cameras*): the
//! devices connected over MTP/PTP, listed beside cards and drives in Import from, read through
//! `dalica-scan`'s PTP reader on the import's own threads, never on the owner thread.
//!
//! - **Listing the cameras** ([`Cameras::list`], `camera.list`): `dalica-scan`'s cached camera
//!   list (refreshed in the background at most every 2 s; never waits on a device). A camera
//!   another program or driver holds, or this user may not open, is listed with that reason and
//!   cannot be opened: Dalica never takes a device from its holder and never guesses.
//! - **One session per camera** ([`Link`]): a camera takes one PTP session at a time, so the
//!   listing, the import's copies and the removal from the camera share one, opened on the first
//!   device call (on a worker) and kept while something holds the link. A session an error broke is
//!   dropped and opened again on the next call; a camera that is gone fails that call.
//! - **Copies** ([`copy_object`], on the import job's thread): the object is read in chunks into a
//!   new temporary file through `dalica-fs` inside a private staging folder in the destination
//!   folder, `fsync`ed and committed there, given the object's capture date as its time, read back
//!   and checked against the MD5 digest of what the camera sent, and only then moved to its name
//!   in the destination by a rename that never replaces a file (a taken name gets `-001`, …).
//! - **Removal** ([`delete_copied`]): DeleteObject for one object, only while the camera still
//!   reports the size and dates it had when it was listed and the copy is still the verified one.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_core::{DateTime, names};
use dalica_fs::Identity;
use dalica_fs::transfer::{self, Moved, Outcome};
use dalica_image::Rgba8;
use dalica_scan::devices::{Camera, CameraAccess, DeviceWatch};
use dalica_scan::ptp::{self, Limits, PtpError, Transport, TransportError};
use serde_json::{Value, json};

use crate::session::Session;

/// A PTP session on a camera, over whatever transport reaches it.
pub type PtpSession = ptp::Session<Box<dyn Transport>>;

/// Opens a session on the camera of an id `camera.list` gave (on a worker thread).
pub type Opener = Arc<dyn Fn(&str) -> Result<PtpSession, PtpError> + Send + Sync>;

/// Lists the cameras connected (a test's own list: [`Session::set_camera_backend`]).
pub type Lister = Box<dyn Fn() -> Vec<Camera> + Send + Sync>;

/// The most bytes of one object read into memory, to make its thumbnail when the camera has none
/// or to compare it with a library photo of its size; a larger object is neither (it is still
/// listed and imported).
pub const READ_CAP: u64 = 64 * 1024 * 1024;

/// The prefix of the private staging folder a copy is written in before it gets its name.
pub const STAGING: &str = ".dalica-camera";

/// Why a camera cannot be opened, by its access.
pub fn unreachable_reason(access: &CameraAccess) -> Option<String> {
    match access {
        CameraAccess::Open => None,
        CameraAccess::Held(why) => Some(format!("Another program is using this camera ({why}); close it, or import from the folder it shows")),
        CameraAccess::Denied(why) => Some(format!("Dalica may not open this camera ({why})")),
        CameraAccess::Failed(why) => Some(format!("The camera did not answer as expected ({why})")),
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The one session on a camera, shared by its listing, its import and its removal.
pub struct Link {
    id: String,
    opener: Opener,
    session: Mutex<Option<PtpSession>>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Link {
    pub(crate) fn new(id: &str, opener: Opener) -> Link {
        Link { id: id.to_string(), opener, session: Mutex::new(None) }
    }

    /// Runs `f` on the camera's session, opening one first when none is open (or the last one
    /// broke). Talks to the device: never on the owner thread.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut PtpSession) -> Result<R, PtpError>) -> Result<R, PtpError> {
        let mut held = lock(&self.session);
        if held.as_ref().is_some_and(PtpSession::is_broken) {
            *held = None;
        }
        let session = match held.as_mut() {
            Some(s) => s,
            None => held.insert((self.opener)(&self.id)?),
        };
        let result = f(session);
        if session.is_broken() {
            *held = None;
        }
        result
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // Closing talks to the device: never on the thread that drops the last holder (it may be
        // the owner's).
        let Some(session) = self.session.get_mut().unwrap_or_else(PoisonError::into_inner).take() else { return };
        let spawned = std::thread::Builder::new().name("dalica-camera-close".into()).spawn(move || session.close());
        if let Err(e) = spawned {
            log::warn!("a camera session was not closed: {e}");
        }
    }
}

/// An object on the camera as it was listed: what removal and a second copy check it against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraObject {
    pub handle: u32,
    pub storage: u32,
    /// Its name on the camera, cleaned by `core::names::sanitize`.
    pub name: String,
    pub size: u64,
    pub captured: Option<DateTime>,
    pub modified: Option<DateTime>,
}

impl CameraObject {
    /// The object `handle` the camera describes with `info`.
    pub(crate) fn of(handle: u32, info: &ptp::ObjectInfo) -> Option<CameraObject> {
        Some(CameraObject {
            handle,
            storage: info.storage,
            name: names::sanitize(&info.filename, &format!("object-{handle:08x}")),
            size: info.size()?,
            captured: ptp::parse_date(&info.capture_date),
            modified: ptp::parse_date(&info.modification_date),
        })
    }

    /// The date it is filed under: its capture date, else its modification date.
    pub fn date(&self) -> Option<DateTime> {
        self.captured.or(self.modified)
    }

    /// Whether the camera's description `info` still says the size and dates it had.
    fn unchanged(&self, info: &ptp::ObjectInfo) -> bool {
        info.size() == Some(self.size)
            && ptp::parse_date(&info.capture_date) == self.captured
            && ptp::parse_date(&info.modification_date) == self.modified
    }
}

/// A camera object and the session that reaches it, as an import's item and its ledger hold them.
#[derive(Clone, Debug)]
pub struct CameraFile {
    pub(crate) link: Arc<Link>,
    pub object: CameraObject,
}

/// The cameras as the session lists and reaches them.
#[derive(Default)]
pub struct Cameras {
    /// A test's camera list and opener (`None`: `dalica-scan`'s, of this system).
    backend: Option<(Arc<DeviceWatch<Camera>>, Opener)>,
    /// The sessions in use, by camera id.
    links: HashMap<String, Arc<Link>>,
}

impl std::fmt::Debug for Cameras {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cameras").field("links", &self.links.keys().collect::<Vec<_>>()).finish_non_exhaustive()
    }
}

impl Cameras {
    /// The cameras connected, as last listed (never waits on a device).
    pub fn list(&self) -> Vec<Camera> {
        match &self.backend {
            Some((watch, _)) => watch.get(),
            None => dalica_scan::devices::cameras(),
        }
    }

    /// The link to camera `id`: the one in use, else a new one (nothing is opened yet). Links of
    /// cameras no longer listed are let go.
    pub(crate) fn link(&mut self, id: &str, listed: &[Camera]) -> Arc<Link> {
        self.links.retain(|k, _| listed.iter().any(|c| &c.id == k));
        if let Some(l) = self.links.get(id) {
            return Arc::clone(l);
        }
        let opener: Opener = match &self.backend {
            Some((_, o)) => Arc::clone(o),
            None => Arc::new(|id: &str| dalica_scan::devices::open_camera(id, Limits::default())),
        };
        let link = Arc::new(Link::new(id, opener));
        self.links.insert(id.to_string(), Arc::clone(&link));
        link
    }
}

/// What a source path names for a camera storage (never a folder on disk).
pub fn source_path(id: &str, storage: Option<u32>) -> PathBuf {
    match storage {
        Some(s) => PathBuf::from(format!("camera:{id}:{s:08x}")),
        None => PathBuf::from(format!("camera:{id}")),
    }
}

/// A camera as `camera.list` answers it.
fn camera_json(c: &Camera) -> Value {
    let access = match c.access {
        CameraAccess::Open => "open",
        CameraAccess::Held(_) => "held",
        CameraAccess::Denied(_) => "denied",
        CameraAccess::Failed(_) => "failed",
    };
    let storages: Vec<Value> = c.storages.iter().map(|s| json!({ "id": s.id, "name": s.name, "capacity": s.capacity, "free": s.free })).collect();
    json!({
        "id": c.id, "name": c.name(), "vendor": c.vendor, "model": c.model, "access": access,
        "reason": unreachable_reason(&c.access), "storages": storages,
    })
}

/// Whether this build reaches cameras over USB on this system (`crates/scan/README.md`).
pub const USB_REACHABLE: bool = cfg!(any(target_os = "linux", target_os = "windows"));

impl Session {
    /// The camera sources.
    pub fn cameras(&self) -> &Cameras {
        &self.cameras
    }

    /// Test hook: the cameras are those `lister` lists (refreshed when older than `max_age`) and
    /// `opener` opens their sessions; `None` gives back this system's own.
    #[doc(hidden)]
    pub fn set_camera_backend(&mut self, backend: Option<(Lister, Opener)>, max_age: Duration) {
        self.cameras.links.clear();
        self.cameras.backend = backend.map(|(lister, opener)| (DeviceWatch::new(lister, max_age), opener));
    }

    /// `camera.list`: the cameras and phones connected, each with its storages or why it cannot be
    /// reached.
    pub(crate) fn camera_list(&self) -> Value {
        let cameras: Vec<Value> = self.cameras.list().iter().map(camera_json).collect();
        json!({ "cameras": cameras, "usb": USB_REACHABLE })
    }

    /// The camera `id` as last listed, and its link, when it can be opened.
    pub(crate) fn camera_link(&mut self, id: &str) -> Result<(Camera, Arc<Link>), String> {
        let listed = self.cameras.list();
        let Some(camera) = listed.iter().find(|c| c.id == id).cloned() else {
            return Err(format!("no camera {id} is connected (camera.list lists them)"));
        };
        if let Some(why) = unreachable_reason(&camera.access) {
            return Err(why);
        }
        let link = self.cameras.link(id, &listed);
        Ok((camera, link))
    }
}

// --- On the import's threads ---------------------------------------------------------------------

/// Whether `e` means the camera can no longer be reached (unplugged, silent, the session lost),
/// rather than one object failing.
pub fn lost(e: &PtpError) -> bool {
    matches!(e, PtpError::Transport(_) | PtpError::TimedOut | PtpError::Broken)
}

/// What a lost camera's import says.
pub fn lost_reason(e: &PtpError) -> String {
    match e {
        PtpError::Transport(TransportError::Disconnected) => "The camera was disconnected".into(),
        e => format!("The camera can no longer be reached: {e}"),
    }
}

/// A thumbnail of the picture in `bytes`, at most `long_side` pixels, upright: its embedded one
/// when that decodes, else a scaled decode (as the listing of a card makes them).
pub(crate) fn thumbnail_of(bytes: &[u8], long_side: u32) -> Option<Rgba8> {
    let limits = dalica_image::Limits::default();
    let made = std::panic::catch_unwind(|| {
        let header = dalica_image::read_header(bytes, &limits).ok();
        if let Some(h) = &header
            && let Some(embedded) = h.thumbnail(bytes)
            && let Ok(small) = dalica_image::decode_thumbnail(embedded, long_side, &limits)
            && let Ok(upright) = dalica_image::orient::apply(small, h.orientation)
        {
            return Some(upright);
        }
        dalica_image::decode_thumbnail(bytes, long_side, &limits).ok()
    });
    made.ok().flatten()
}

/// Reads the whole object into memory (at most [`READ_CAP`] bytes).
pub(crate) fn read_bytes(link: &Link, object: &CameraObject) -> Result<Vec<u8>, PtpError> {
    if object.size > READ_CAP {
        return Err(PtpError::TooLarge(format!("{} bytes", object.size)));
    }
    let mut out = Vec::with_capacity(usize::try_from(object.size).unwrap_or(0));
    link.with(|s| {
        s.read_object(object.handle, object.size, &mut |piece| {
            out.extend_from_slice(piece);
            true
        })
    })?;
    Ok(out)
}

/// `md5:` and 32 lowercase hex digits (the digest every listing and Find Duplicates keep).
fn digest_text(context: md5::Context) -> String {
    format!("{}{:x}", dalica_scan::import::DIGEST_PREFIX, context.finalize())
}

/// The digest of `bytes`.
pub(crate) fn digest_of(bytes: &[u8]) -> String {
    let mut c = md5::Context::new();
    c.consume(bytes);
    digest_text(c)
}

/// Days from 1970-01-01 to `y-m-d` (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `d` as a time, the wall clock read as UTC (as every date Dalica derives from a file time).
pub fn system_time(d: DateTime) -> Option<SystemTime> {
    let date = d.date();
    let days = days_from_civil(i64::from(date.year()), i64::from(date.month()), i64::from(date.day()));
    let secs = days.checked_mul(86_400)?.checked_add(i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second()))?;
    if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(u64::try_from(secs).ok()?))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
    }
}

/// Why a copy from the camera failed.
#[derive(Debug)]
pub(crate) enum CopyError {
    /// The camera can no longer be reached: the import stops (the reason to show).
    Lost(String),
    /// This object failed; the import goes on.
    File(String),
}

fn file_err(e: dalica_fs::Error) -> CopyError {
    CopyError::File(e.to_string())
}

fn ptp_err(e: PtpError) -> CopyError {
    if lost(&e) { CopyError::Lost(lost_reason(&e)) } else { CopyError::File(format!("the camera refused the file: {e}")) }
}

/// A copy from the camera, verified.
#[derive(Clone, Debug)]
pub(crate) struct Copied {
    pub name: String,
    pub copy: Identity,
    /// The digest of the bytes received (and read back).
    pub digest: String,
}

/// Copies `file` into the folder `dir` (which exists) under its name, or the first free
/// `-001`, … (`exact`: that name or nothing); see the module docs. `received` counts the bytes
/// the camera sent.
pub(crate) fn copy_object(file: &CameraFile, dir: &Path, exact: Option<&str>, received: &dyn Fn(u64)) -> Result<Copied, CopyError> {
    let object = &file.object;
    let info = file.link.with(|s| s.object_info(object.handle)).map_err(ptp_err)?;
    if !object.unchanged(&info) {
        return Err(CopyError::File(format!("{} changed on the camera since it was listed, so it is not copied", object.name)));
    }
    let staging = dalica_fs::TempDir::new_in(dir, STAGING).map_err(file_err)?;
    let staged = staging.path().join(&object.name);
    let mut temp = dalica_fs::pack::TempFile::create(&staged).map_err(file_err)?;
    let mut context = md5::Context::new();
    let mut failed: Option<dalica_fs::Error> = None;
    let read = file.link.with(|s| {
        s.read_object(object.handle, object.size, &mut |piece| {
            context.consume(piece);
            received(piece.len() as u64);
            match temp.write(piece) {
                Ok(_) => true,
                Err(e) => {
                    failed = Some(e);
                    false
                }
            }
        })
    });
    if let Some(e) = failed {
        return Err(file_err(e));
    }
    read.map_err(ptp_err)?;
    let digest = digest_text(context);
    temp.commit().map_err(file_err)?;
    let mut identity = dalica_fs::identity(&staged).map_err(file_err)?.ok_or_else(|| CopyError::File("the copy vanished".into()))?;
    if let Some(when) = object.date().and_then(system_time) {
        let times = dalica_fs::Times { modified: when, accessed: None };
        identity = dalica_fs::set_times(&staged, &times, identity).map_err(file_err)?;
    }
    // Read back: the bytes on disk are those the camera sent.
    if identity.size() != object.size || read_back_digest(&staged).as_deref() != Some(digest.as_str()) {
        return Err(CopyError::File(format!("the copy of {} did not read back as the camera sent it", object.name)));
    }
    let names: Box<dyn Iterator<Item = String>> = match exact {
        Some(n) => Box::new(std::iter::once(n.to_string())),
        None => Box::new(names::candidates(&object.name)),
    };
    for new_name in names {
        match transfer::move_file(staging.path(), &object.name, identity, dir, &new_name).map_err(file_err)? {
            Outcome::Done(Moved::Renamed(copy) | Moved::Copied { identity: copy, .. }) => {
                if let Err(e) = staging.close() {
                    log::warn!("a camera import's staging folder stayed: {e}");
                }
                return Ok(Copied { name: new_name, copy, digest });
            }
            Outcome::Exists if exact.is_none() => {}
            Outcome::Exists => return Err(CopyError::File(format!("the folder already has a file named {new_name}"))),
        }
    }
    Err(CopyError::File(format!("no free name for {}", object.name)))
}

/// The digest of the file at `path`, read in chunks; `None` when it cannot be read.
fn read_back_digest(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut context = md5::Context::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match f.read(&mut buf) {
            Ok(0) => return Some(digest_text(context)),
            Ok(n) => context.consume(buf.get(..n)?),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// Deletes `file` from the camera when the camera still describes it as it was listed and the copy
/// at `copy` is still the verified `copy_identity`.
pub(crate) fn delete_copied(file: &CameraFile, copy: &Path, copy_identity: Identity) -> Result<(), String> {
    let object = &file.object;
    match dalica_fs::identity(copy).map_err(|e| e.to_string())? {
        Some(id) if id == copy_identity => {}
        _ => return Err(format!("The copy of {} changed or is gone, so it stays on the camera.", object.name)),
    }
    let info = file.link.with(|s| s.object_info(object.handle)).map_err(|e| e.to_string())?;
    if !object.unchanged(&info) {
        return Err(format!("{} changed on the camera since it was copied, so it stays.", object.name));
    }
    file.link.with(|s| s.delete_object(object.handle)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_core::Date;

    #[test]
    fn device_dates_become_times_read_as_utc() {
        let at = |y, m, d, h, mi, s| system_time(DateTime::new(Date::new(y, m, d).unwrap(), h, mi, s).unwrap()).unwrap();
        assert_eq!(at(1970, 1, 1, 0, 0, 0), UNIX_EPOCH);
        let t = at(2026, 6, 4, 8, 0, 0);
        let secs = t.duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(DateTime::from_unix_secs(secs as i64).unwrap().to_string(), "2026-06-04T08:00:00");
        assert!(at(1969, 12, 31, 23, 59, 59) < UNIX_EPOCH);
    }
}
