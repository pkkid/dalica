//! A whole device listed: its storages and every file on them, as an import lists a card.

use dalica_core::DateTime;
use dalica_core::names::sanitize;

use super::codes::{ALL, format};
use super::dataset::{ObjectInfo, StorageInfo, parse_date};
use super::{PtpError, Session, Transport};

/// A storage of the device (a card slot, a phone's internal storage).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Storage {
    pub id: u32,
    pub info: StorageInfo,
}

impl Storage {
    /// The name the source list shows: the volume label, else the description, else
    /// "Storage n" (cleaned by the one sanitizer).
    pub fn name(&self) -> String {
        let fallback = format!("Storage {}", self.id & 0xFFFF);
        let pick = if !self.info.label.trim().is_empty() { &self.info.label } else { &self.info.description };
        sanitize(pick, &fallback)
    }
}

/// What a file on the device holds, from its format code (else its name's extension).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Picture,
    Video,
    Other,
}

const PICTURE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "jxl", "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf",
    "sr2", "raf", "orf", "rw2", "pef", "srw", "x3f", "3fr", "iiq", "erf", "kdc", "mrw", "rwl",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "avi", "mts", "m2ts", "3gp", "mkv", "mpg", "mpeg", "wmv"];

/// A file on the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub handle: u32,
    pub storage: u32,
    /// The folder's handle (0: the storage's root).
    pub parent: u32,
    pub format: u16,
    /// The file name, cleaned by `core::names::sanitize` (a name it cannot use becomes
    /// `object-<handle>`).
    pub name: String,
    /// The name was changed by cleaning it.
    pub renamed: bool,
    /// Its size; `None` when the device says it does not fit 32 bits (never read: past 4 GiB).
    pub size: Option<u64>,
    /// The capture date the device gives, when it parses.
    pub captured: Option<DateTime>,
    /// The modification date the device gives, when it parses.
    pub modified: Option<DateTime>,
    pub width: u32,
    pub height: u32,
    /// The device says it has a thumbnail.
    pub has_thumb: bool,
}

impl Object {
    fn of(handle: u32, info: &ObjectInfo) -> Object {
        let name = sanitize(&info.filename, &format!("object-{handle:08x}"));
        Object {
            handle,
            storage: info.storage,
            parent: info.parent,
            format: info.format,
            renamed: name != info.filename,
            name,
            size: info.size(),
            captured: parse_date(&info.capture_date),
            modified: parse_date(&info.modification_date),
            width: info.width,
            height: info.height,
            has_thumb: info.thumb_size > 0 && info.thumb_format != 0,
        }
    }

    pub fn kind(&self) -> ObjectKind {
        match self.format {
            format::IMAGE_FIRST..=format::IMAGE_LAST => return ObjectKind::Picture,
            format::AVI | format::MPEG | format::ASF | format::QUICKTIME | format::MTP_VIDEO_FIRST..=format::MTP_VIDEO_LAST => {
                return ObjectKind::Video;
            }
            _ => {}
        }
        let ext = self.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if PICTURE_EXTENSIONS.contains(&ext.as_str()) {
            ObjectKind::Picture
        } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            ObjectKind::Video
        } else {
            ObjectKind::Other
        }
    }

    /// The date an import files it under: the capture date, else the modification date.
    pub fn date(&self) -> Option<DateTime> {
        self.captured.or(self.modified)
    }
}

/// A device's storages and files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub storages: Vec<Storage>,
    /// Every file (folders left out), storage by storage in the device's order.
    pub objects: Vec<Object>,
    /// What could not be listed (an object the device refused to describe), at most 100.
    pub warnings: Vec<String>,
    /// The handle limit cut the listing short.
    pub truncated: bool,
}

impl Listing {
    /// The files that hold pictures.
    pub fn pictures(&self) -> impl Iterator<Item = &Object> {
        self.objects.iter().filter(|o| o.kind() == ObjectKind::Picture)
    }
}

const MAX_WARNINGS: usize = 100;

/// Lists the device: its storages, then every object on each, folders left out, at most
/// [`Limits::max_handles`](super::Limits::max_handles) over all storages. An object the device
/// refuses to describe is a warning; a transport error or a timeout ends the listing with that
/// error. `keep_going` is asked before each object (a cancel flag).
pub fn list<T: Transport>(session: &mut Session<T>, keep_going: &dyn Fn() -> bool) -> Result<Listing, PtpError> {
    let info = session.device_info().clone();
    let mut listing = Listing {
        vendor: sanitize(&info.manufacturer, "Camera"),
        model: sanitize(&info.model, "Camera"),
        serial: if info.serial.trim().is_empty() { String::new() } else { sanitize(&info.serial, "") },
        ..Listing::default()
    };
    let max = session.limits().max_handles as usize;
    let mut seen: usize = 0;
    for id in session.storage_ids()? {
        if id == 0 || id == ALL {
            continue;
        }
        let storage = match session.storage_info(id) {
            Ok(s) => s,
            Err(PtpError::Device { .. }) => continue, // an empty card slot
            Err(e) => return Err(e),
        };
        listing.storages.push(Storage { id, info: storage });
        let handles = match session.object_handles(id) {
            Ok(h) => h,
            Err(e @ PtpError::Device { .. }) => {
                warn(&mut listing.warnings, format!("storage {id:#x}: {e}"));
                continue;
            }
            Err(e) => return Err(e),
        };
        for handle in handles {
            if seen >= max {
                listing.truncated = true;
                break;
            }
            seen += 1;
            if !keep_going() {
                return Err(PtpError::Stopped);
            }
            match session.object_info(handle) {
                Ok(o) if o.format == format::ASSOCIATION => {}
                Ok(o) => listing.objects.push(Object::of(handle, &o)),
                Err(e @ (PtpError::Device { .. } | PtpError::Malformed(_))) if !session.is_broken() => {
                    warn(&mut listing.warnings, format!("object {handle:#x}: {e}"));
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(listing)
}

fn warn(warnings: &mut Vec<String>, w: String) {
    if warnings.len() < MAX_WARNINGS {
        warnings.push(w);
    }
}
