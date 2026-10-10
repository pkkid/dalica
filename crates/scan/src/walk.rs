//! Walking the roots and reading what is found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use dalica_core::{DateTime, Timestamp};
use dalica_fs::{Entry, Limits};
use dalica_sidecar::{Field, Sidecar};

/// A photo file on disk and what its headers say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundPhoto {
    pub name: String,
    pub size: u64,
    pub mtime: Timestamp,
    /// As displayed (after EXIF orientation).
    pub width: u32,
    pub height: u32,
    pub date: Option<DateTime>,
    /// From the folder's sidecar.
    pub star: bool,
}

/// A folder holding at least one photo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFolder {
    pub path: PathBuf,
    /// Sorted by name.
    pub photos: Vec<FoundPhoto>,
}

/// Something a scan could not read; the scan went on without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub path: PathBuf,
    pub reason: String,
}

/// What a scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Roots that were walked; folders under other roots are left as they are.
    pub walked: Vec<PathBuf>,
    /// Sorted by path.
    pub folders: Vec<FoundFolder>,
    pub warnings: Vec<Warning>,
}

/// OS and tool folders that never hold the user's photos (`data-model.md` §7).
const SYSTEM_FOLDERS: [&str; 7] = ["$RECYCLE.BIN", "System Volume Information", "lost+found", "@eaDir", "#recycle", "$Recycle.Bin", "node_modules"];

/// Whether a walk leaves `entry` out (and, for a folder, everything in it): dot files and folders
/// (`.dalica.json`, `.dalica-originals`, `.git`, …) and OS system folders.
pub fn skipped(entry: &Entry) -> bool {
    let name = entry.name();
    name.starts_with('.') || (entry.is_dir && SYSTEM_FOLDERS.iter().any(|s| s.eq_ignore_ascii_case(&name)))
}

/// Scans `roots` (each walked through `dalica-fs`, bounded). `cancel` stops it early.
pub fn scan(roots: &[PathBuf], cancel: Option<&AtomicBool>) -> Scan {
    let mut out = Scan::default();
    let mut by_folder: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
    for root in roots {
        let walk = match dalica_fs::walk(root, Limits { cancel, ..Limits::default() }, skipped) {
            Ok(w) => w,
            Err(e) => {
                out.warnings.push(Warning { path: root.clone(), reason: e.to_string() });
                continue;
            }
        };
        out.walked.push(root.clone());
        for w in walk.warnings {
            out.warnings.push(Warning { path: root.clone(), reason: format!("{w:?}") });
        }
        for entry in walk.entries.into_iter().filter(|e| !e.is_dir) {
            let parent = entry.path.parent().map(Path::to_path_buf).unwrap_or_default();
            by_folder.entry(parent).or_default().push(entry);
        }
    }
    let limits = dalica_image::Limits::default();
    for (path, entries) in by_folder {
        let stars = read_stars(&path, &mut out.warnings);
        let mut photos = Vec::new();
        for entry in entries {
            let Some(name) = entry.path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                out.warnings.push(Warning { path: entry.path.clone(), reason: "the file name is not valid Unicode".into() });
                continue;
            };
            match dalica_image::read_header_file(&entry.path, &limits) {
                Ok((h, _)) if h.format.supported() => {
                    let (width, height) = h.display_size();
                    let date = h.exif.as_ref().and_then(|e| e.date_time_original);
                    let star = stars.as_ref().is_some_and(|s| s.star(&name) == Field::Value(true));
                    photos.push(FoundPhoto { name, size: entry.size, mtime: entry.modified, width, height, date, star });
                }
                // Other formats arrive in M8; anything else is not a photo.
                Ok(_) | Err(dalica_image::Error::Unknown | dalica_image::Error::Unsupported(_)) => {}
                Err(dalica_image::Error::Truncated) if entry.size < dalica_image::sniff::SNIFF_LEN as u64 => {}
                Err(e) => out.warnings.push(Warning { path: entry.path.clone(), reason: e.to_string() }),
            }
        }
        if !photos.is_empty() {
            photos.sort_by(|a, b| a.name.cmp(&b.name));
            out.folders.push(FoundFolder { path, photos });
        }
    }
    out
}

/// The folder's sidecar, if it has a readable one (a damaged one is the store's to set aside).
fn read_stars(folder: &Path, warnings: &mut Vec<Warning>) -> Option<Sidecar> {
    let path = folder.join(dalica_sidecar::FILE_NAME);
    let limits = dalica_sidecar::Limits::default();
    match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => None,
        Ok(Some(r)) => match Sidecar::parse(&r.bytes, &limits) {
            Ok(s) => Some(s),
            Err(e) => {
                warnings.push(Warning { path, reason: e.to_string() });
                None
            }
        },
        Err(e) => {
            warnings.push(Warning { path, reason: e.to_string() });
            None
        }
    }
}
