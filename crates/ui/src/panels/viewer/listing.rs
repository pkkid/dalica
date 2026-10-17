//! The photo viewer's folder as the engine lists it (`viewer.list`, `viewing.md` §3.4): the photos
//! Dalica can show in the viewed file's folder, in name order, each with its path, its library id
//! when the library holds it, its star and its size; and whether the viewer is read-only.
//!
//! The panel keeps the listing between frames and asks again only while it is on its way or when
//! the library changed ([`cached`]); the commands ask once when they run ([`current`]).
//!
//! **Where the viewer is.** The view state's `viewer.index` is the photo shown, by its place in
//! this listing. The first listing that arrives for a file puts it on that file's place
//! ([`sync`]): the view state starts at 0 before the folder is read. A photo the library dropped
//! since the folder was read (sent to the trash from the viewer) is left out, and the index stays
//! inside the listing.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde_json::{Value, json};

use crate::app::App;

/// Why a command waits while the folder is read.
pub const READING: &str = "The photo viewer is still reading the folder.";
/// Why a command has nothing to act on.
pub const EMPTY: &str = "The folder holds no photo Dalica can show.";
/// The most photos of one folder the viewer takes in (a hostile reply is cut there).
pub const MAX_PHOTOS: usize = 1_000_000;

/// One photo of the folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub name: String,
    pub path: PathBuf,
    /// The library's id; `None` while the library does not hold its folder.
    pub id: Option<PhotoId>,
    pub star: bool,
    pub width: u32,
    pub height: u32,
}

/// The folder as listed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Listing {
    /// The file the viewer was opened with.
    pub file: PathBuf,
    /// The file's place among [`Listing::photos`].
    pub index: Option<usize>,
    pub photos: Vec<Photo>,
    /// The library holds the folder.
    pub in_library: bool,
    /// Why the viewer's changes are refused ("Dalica is already open"), when they are.
    pub read_only: Option<String>,
}

impl Listing {
    /// The listing in a `viewer.list` reply. A photo of a folder the library holds that the library
    /// no longer has (sent to the trash) is left out.
    pub fn from_reply(v: &Value) -> Listing {
        let in_library = v.get("inLibrary").and_then(Value::as_bool).unwrap_or(false);
        let file = v.get("file").and_then(Value::as_str).map(PathBuf::from).unwrap_or_default();
        let photos: Vec<Photo> = v
            .get("photos")
            .and_then(Value::as_array)
            .map(|a| a.iter().take(MAX_PHOTOS).filter_map(photo).filter(|p| !(in_library && p.id.is_none())).collect())
            .unwrap_or_default();
        let index = photos.iter().position(|p| p.path == file);
        let read_only = v
            .get("readOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .then(|| v.get("reason").and_then(Value::as_str).unwrap_or(dalica_engine::viewer::ALREADY_OPEN).to_string());
        Listing { file, index, photos, in_library, read_only }
    }

    /// The photo at `index`, held inside the listing; `None` when it is empty.
    pub fn at(&self, index: usize) -> Option<(usize, &Photo)> {
        let last = self.photos.len().checked_sub(1)?;
        let i = index.min(last);
        self.photos.get(i).map(|p| (i, p))
    }
}

fn photo(v: &Value) -> Option<Photo> {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    Some(Photo {
        name: v.get("name")?.as_str()?.to_string(),
        path: PathBuf::from(v.get("path")?.as_str()?),
        id: v.get("id").and_then(Value::as_u64).map(PhotoId::new),
        star: v.get("star").and_then(Value::as_bool).unwrap_or(false),
        width: n("width"),
        height: n("height"),
    })
}

/// What asking the engine gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// The folder is still being read (or no library is open yet).
    Pending,
    Ready(Listing),
    /// The file cannot be shown, or no viewer is open: why.
    Failed(String),
}

/// Asks the engine for the viewer's listing (`viewer.list`), straight from the session so the
/// status line is left alone.
pub fn fetch(app: &mut App) -> Fetched {
    let Some(s) = app.session_mut() else { return Fetched::Pending };
    match s.execute("viewer.list", &json!({})) {
        Ok(v) if v.get("pending").and_then(Value::as_bool) == Some(true) => Fetched::Pending,
        Ok(v) => Fetched::Ready(Listing::from_reply(&v)),
        Err(e) => Fetched::Failed(e.to_string()),
    }
}

thread_local! {
    /// The file whose listing last put the view state on its place (the UI thread's).
    static SYNCED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Puts the view state on the opened file's place the first time its listing arrives, with the
/// background the settings ask for; afterwards keeps the index inside the listing.
pub fn sync(app: &mut App, l: &Listing) {
    let first = SYNCED.with(|s| s.borrow().as_ref() != Some(&l.file));
    let translucent = app.session().is_none_or(|s| s.settings().viewer().translucent);
    let Some(v) = app.state_mut().viewer.as_mut() else { return };
    if first {
        v.index = l.index.unwrap_or(0);
        v.backdrop = if translucent { crate::state::Backdrop::Translucent } else { crate::state::Backdrop::Opaque };
        SYNCED.with(|s| *s.borrow_mut() = Some(l.file.clone()));
    }
    v.index = v.index.min(l.photos.len().saturating_sub(1));
}

/// The listing and the photo shown, for a command: refused while the folder is read, when it holds
/// nothing, or when no viewer is open.
pub fn current(app: &mut App) -> Result<(Listing, usize), CmdError> {
    match fetch(app) {
        Fetched::Pending => Err(CmdError::Disabled(READING.into())),
        Fetched::Failed(why) => Err(CmdError::Disabled(why)),
        Fetched::Ready(l) => {
            sync(app, &l);
            let index = app.state().viewer.map_or(0, |v| v.index);
            let (i, _) = l.at(index).ok_or_else(|| CmdError::Disabled(EMPTY.into()))?;
            Ok((l, i))
        }
    }
}

/// What the panel keeps between frames.
#[derive(Clone, Default)]
struct Cache {
    /// The library revision the listing was asked at; `None` while it is on its way.
    revision: Option<u64>,
    listing: Option<Arc<Listing>>,
    failed: Option<String>,
}

/// The panel's view of the listing: asked again while it is on its way and whenever the library
/// changed (a star, a rotation, the folder joining the library), else the one kept. `Err` says why
/// nothing can be shown; `Ok(None)` while the folder is read.
pub fn cached(app: &mut App, ctx: &egui::Context) -> Result<Option<Arc<Listing>>, String> {
    let id = egui::Id::new("viewer.listing");
    let mut c = ctx.data_mut(|d| d.get_temp::<Cache>(id)).unwrap_or_default();
    let revision = app.session().map(|s| s.library().revision());
    if c.revision.is_none() || c.revision != revision {
        match fetch(app) {
            Fetched::Pending => c.revision = None,
            Fetched::Ready(l) => {
                c.listing = Some(Arc::new(l));
                c.failed = None;
                c.revision = revision;
            }
            Fetched::Failed(why) => {
                c.failed = Some(why);
                c.revision = revision;
            }
        }
    }
    if let Some(l) = &c.listing {
        sync(app, l);
    }
    let out = match (&c.failed, &c.listing) {
        (Some(why), None) => Err(why.clone()),
        (_, l) => Ok(l.clone()),
    };
    let pending = c.revision.is_none();
    ctx.data_mut(|d| d.insert_temp(id, c));
    if pending {
        // The worker answers within a few frames: look again at once.
        ctx.request_repaint();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_becomes_a_listing_without_the_photos_the_library_dropped() {
        let reply = json!({
            "file": "/p/b.jpg", "folder": "/p", "index": 1, "inLibrary": true, "readOnly": false, "reason": null,
            "photos": [
                { "name": "a.jpg", "path": "/p/a.jpg", "id": 4, "star": true, "width": 30, "height": 20 },
                { "name": "gone.jpg", "path": "/p/gone.jpg", "id": null, "star": false, "width": 30, "height": 20 },
                { "name": "b.jpg", "path": "/p/b.jpg", "id": 5, "star": false, "width": 20, "height": 30 },
                { "name": 7 },
            ],
        });
        let l = Listing::from_reply(&reply);
        assert_eq!(l.photos.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a.jpg", "b.jpg"]);
        assert_eq!((l.index, l.read_only.as_deref()), (Some(1), None));
        assert_eq!(l.at(9).map(|(i, p)| (i, p.id)), Some((1, Some(PhotoId::new(5)))));
        let locked = Listing::from_reply(&json!({ "readOnly": true, "reason": "Dalica is already open", "photos": "no" }));
        assert_eq!((locked.photos.len(), locked.read_only.as_deref(), locked.at(0)), (0, Some("Dalica is already open"), None));
    }
}
