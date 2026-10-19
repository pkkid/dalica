//! The previous organizer's per-folder sidecar as a scan finds it (`data-model.md` §9, M14.11).
//!
//! On the scan's worker, a folder without `.dalica.json` that holds a legacy sidecar (either of
//! [`sidecar_names`], the current one first) has it read through `dalica-fs` (never through a link,
//! never a FIFO or device) with `dalica-sidecar`'s bounded reader and converted into what a
//! `.dalica.json` gives the model: each photo's star, hidden flag, caption, tags, recipe and redo
//! list (through `dalica-edit`'s mapping), faces, album memberships and geotag; the folder's
//! description, date, place and hidden flag; the album definitions; and, for the faces, the names of
//! their contacts (the folder's own, then those of its ancestors already read in the walk). Albums
//! and contacts stay keyed by their legacy ids here: the engine finds or makes Dalica's albums and
//! people for them.
//!
//! - A folder with both files reads only `.dalica.json` (the fact [`FolderSidecar::Both`]); with
//!   [`Options::legacy`](crate::Options::legacy) off the legacy file is not looked at.
//! - Sections are matched to photos by their exact name, else ignoring case when that matches one
//!   section and one photo only. Sections naming no photo are left alone, and no path found in a
//!   value is ever opened.
//! - A damaged or oversized legacy file is reported and skipped; it is never set aside, renamed or
//!   written (legacy files are read-only). A value that decoded lossily (bytes that are not text
//!   in the file's encoding) is left out, so it never reaches a file Dalica writes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_core::Rect01;
use dalica_core::geo::LatLon;
use dalica_image::Orientation;
use dalica_library::{AlbumInfo, FolderInfo, Geo, MAX_FACES, PhotoInfo};
use dalica_sidecar::legacy::{LegacyLimits, LegacySidecar, Text};

pub use dalica_library::FolderSidecar;
pub use dalica_sidecar::legacy::{LegacyPerson, sidecar_names};

/// What a folder's legacy sidecar says beyond the fields the model already has a place for in
/// [`FoundFolder`](crate::FoundFolder): its album definitions and the names of the contacts its
/// faces show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyFolder {
    /// The legacy sidecar read.
    pub path: PathBuf,
    /// Album definitions by token (32 lowercase hexadecimal digits), each cleaned as the library
    /// cleans an album's name, description and place; a definition whose name is nothing once
    /// cleaned is left out.
    pub albums: BTreeMap<String, AlbumInfo>,
    /// The name of each contact (16 lowercase hexadecimal digits) a face of the folder shows, from
    /// the folder's own sidecar, else from the nearest ancestor's read in the same walk; a contact
    /// named nowhere is not listed (its faces are unnamed).
    pub contacts: BTreeMap<String, String>,
    /// Values left out because they did not decode as text.
    pub lossy: usize,
}

/// What a folder's legacy sidecar says about one photo beyond its star, [`PhotoInfo`] and geotag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyPhoto {
    /// The faces, in file order, in the photo's stored frame (after its EXIF orientation): those
    /// with an empty rectangle and those past [`MAX_FACES`] left out.
    pub faces: Vec<LegacyFace>,
    /// The albums the photo is in, by token, in file order, each once.
    pub albums: Vec<String>,
}

/// A face from a legacy sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyFace {
    pub rect: Rect01,
    /// A contact id (named in [`LegacyFolder::contacts`] or not) or the ignore marker.
    pub person: LegacyPerson,
}

// A `Rect01` is always finite (never NaN), so equality is reflexive.
impl Eq for LegacyFace {}

/// One photo's converted data, as the walk puts it into its [`FoundPhoto`](crate::FoundPhoto).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Converted {
    pub star: bool,
    pub info: PhotoInfo,
    pub geo: Option<Geo>,
    pub legacy: LegacyPhoto,
}

/// Where a folder's legacy sidecar is, if it has one: the first of [`sidecar_names`] that is
/// there in any form (a link or a FIFO counts: it is reported when read, never followed).
pub(crate) fn locate(dir: &Path) -> Option<PathBuf> {
    sidecar_names().into_iter().map(|n| dir.join(n)).find(|p| !matches!(dalica_fs::stat(p), Ok(None)))
}

/// Reads the legacy sidecar at `path`. `Err` says why it was skipped.
pub(crate) fn read(path: &Path) -> Result<LegacySidecar, String> {
    let limits = LegacyLimits::default();
    match dalica_fs::stat(path) {
        Ok(Some(s)) if s.is_file => {}
        Ok(Some(_)) => return Err("not a regular file: the legacy sidecar is not read".into()),
        Ok(None) => return Err("the legacy sidecar went away before it was read".into()),
        Err(dalica_fs::Error::Link(_)) => return Err("a link: the legacy sidecar is never read through a link".into()),
        Err(e) => return Err(format!("the legacy sidecar could not be read: {e}")),
    }
    let bytes = match dalica_fs::read(path, limits.max_bytes as u64) {
        Ok(Some(r)) => r.bytes,
        Ok(None) => return Err("the legacy sidecar went away before it was read".into()),
        Err(dalica_fs::Error::Link(_)) => return Err("a link: the legacy sidecar is never read through a link".into()),
        Err(e) => return Err(format!("the legacy sidecar was not read: {e}")),
    };
    LegacySidecar::parse(&bytes, &limits).map_err(|e| format!("the legacy sidecar was not read: {e}"))
}

/// The cleaned names of `sidecar`'s contacts.
pub(crate) fn contacts(sidecar: &LegacySidecar) -> BTreeMap<String, String> {
    sidecar.contacts().into_iter().filter(|(_, t)| !t.lossy).filter_map(|(id, t)| Some((id, dalica_library::clean_person_name(&t.text)?))).collect()
}

/// For each of `photos`, the section of `sidecar` that describes it:
/// the one of its exact name, else the one section whose name equals it ignoring case when no
/// photo has that section's exact name and no other photo equals it ignoring case.
pub(crate) fn matches<'a>(sidecar: &'a LegacySidecar, photos: &[&str]) -> Vec<Option<&'a str>> {
    let sections = sidecar.file_names();
    let mut folded: BTreeMap<String, Vec<&'a str>> = BTreeMap::new();
    for s in &sections {
        folded.entry(s.to_lowercase()).or_default().push(s);
    }
    let mut photos_folded: BTreeMap<String, usize> = BTreeMap::new();
    for p in photos {
        let n = photos_folded.entry(p.to_lowercase()).or_default();
        *n = n.saturating_add(1);
    }
    let exact = |name: &str| sections.iter().copied().find(|s| *s == name);
    photos
        .iter()
        .map(|p| {
            if let Some(s) = exact(p) {
                return Some(s);
            }
            let key = p.to_lowercase();
            match (folded.get(&key).map(Vec::as_slice), photos_folded.get(&key)) {
                (Some([only]), Some(1)) if !photos.contains(only) => Some(*only),
                _ => None,
            }
        })
        .collect()
}

/// What `sidecar`'s folder section gives the folder, cleaned as `.dalica.json`'s is.
pub(crate) fn folder_info(sidecar: &LegacySidecar, lossy: &mut usize) -> FolderInfo {
    let f = sidecar.folder();
    FolderInfo {
        description: text(f.description, dalica_library::clean_description, lossy),
        date: f.date.map(|d| d.date()),
        place: text(f.place, dalica_library::clean_place, lossy),
        hidden: f.hidden,
    }
}

/// `sidecar`'s album definitions, cleaned.
pub(crate) fn albums(sidecar: &LegacySidecar, lossy: &mut usize) -> BTreeMap<String, AlbumInfo> {
    let mut out = BTreeMap::new();
    for a in sidecar.albums() {
        if a.name.lossy {
            *lossy = lossy.saturating_add(1);
            continue;
        }
        let Some(name) = dalica_library::clean_album_name(&a.name.text) else { continue };
        let info = AlbumInfo {
            name,
            description: text(a.description, dalica_library::clean_description, lossy),
            date: a.date.map(|d| d.date()),
            use_newest: false,
            place: text(a.place, dalica_library::clean_place, lossy),
        };
        out.insert(a.token, info);
    }
    out
}

/// Photo `section` of `sidecar`, converted; `orientation` is the photo's EXIF orientation (legacy
/// rectangles are on the stored pixels before it).
pub(crate) fn photo(sidecar: &LegacySidecar, section: &str, orientation: Orientation, lossy: &mut usize) -> Converted {
    let Some(f) = sidecar.file(section) else { return Converted::default() };
    let mut tags = Vec::new();
    for k in &f.keywords {
        if k.lossy {
            *lossy = lossy.saturating_add(1);
        } else {
            tags.push(k.text.as_str());
        }
    }
    let text_overlay = match &f.text {
        Some(t) if t.raw.lossy => {
            *lossy = lossy.saturating_add(1);
            None
        }
        Some(t) => Some(t.raw.text.clone()),
        None => None,
    };
    let edits = dalica_edit::legacy::Legacy {
        filters: f.edits.clone(),
        redo: f.redo.clone(),
        quarters: f.rotation.unwrap_or(0),
        flipped: f.flip.is_some_and(|n| n != 0),
        crop: f.crop.map(|[x0, y0, x1, y1]| dalica_edit::Rect::new(x0, y0, x1, y1)),
        text_active: text_overlay.as_ref().and(f.text_active).map(|on| if on { "1" } else { "0" }.to_string()),
        text: text_overlay,
        orientation,
    };
    // Past the recipe's caps: no recipe (the legacy file keeps it), as for `.dalica.json`.
    let (recipe, redo) = match dalica_edit::legacy::convert(&edits) {
        Ok(c) => (crate::walk::operations(c.edits_text()), crate::walk::operations(c.redo_text())),
        Err(_) => (Vec::new(), Vec::new()),
    };
    let dihedral = dalica_edit::Dihedral::from_exif(orientation);
    let faces = f
        .faces
        .iter()
        .filter_map(|face| {
            let [x0, y0, x1, y1] = face.rect;
            let r = dalica_edit::Rect::new(x0, y0, x1, y1).transform(dihedral);
            Some(LegacyFace { rect: Rect01::new(r.x0, r.y0, r.x1, r.y1).ok()?, person: face.person.clone() })
        })
        .take(MAX_FACES)
        .collect();
    Converted {
        star: f.star,
        info: PhotoInfo {
            caption: text(f.caption, dalica_library::clean_caption, lossy),
            tags: dalica_library::clean_tags(tags),
            hidden: f.hidden,
            edits: recipe,
            redo,
            date: None,
        },
        geo: f.geotag.and_then(|g| LatLon::new(g.lat(), g.lon()).ok()).map(|p| Geo::At(p.round6())),
        legacy: LegacyPhoto { faces, albums: f.albums },
    }
}

/// A text value, cleaned (trimmed and cut) as the library stores it; a lossy one left out (and
/// counted).
fn text(t: Option<Text>, clean: fn(&str) -> String, lossy: &mut usize) -> String {
    match t {
        Some(t) if t.lossy => {
            *lossy = lossy.saturating_add(1);
            String::new()
        }
        Some(t) => clean(&t.text),
        None => String::new(),
    }
}
