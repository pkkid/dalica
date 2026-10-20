//! What the Folder Manager's tree lists: the roots (the standard places and the volumes) and each
//! expanded folder's subfolders, read through `folders.browse` on the engine's workers and kept
//! for the open dialog in the egui context's memory (a per-context cache, so the draft in the UI
//! state stays small). Asking never blocks a frame: while a worker reads, the listing is
//! [`Listing::Pending`] and the frame asks again (it requests a repaint, which also animates the
//! spinner row).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_ui_kit::icons::FolderIcon;
use egui::{Context, Id};
use serde_json::{Value, json};

use crate::app::App;
use crate::tl;

/// The most roots and subfolders kept from one answer (the engine caps them too).
const MAX_ENTRIES: usize = 4096;

/// A root of the tree: a standard place or a volume.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub path: PathBuf,
    /// "Pictures", "Home", a volume's name.
    pub name: String,
    pub icon: FolderIcon,
    /// A standard place ("Pictures"), whose name the watched list shows instead of its path.
    pub place: bool,
    /// A volume root or the home folder: watching it asks first.
    pub drive: bool,
}

/// One subfolder.
#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    pub name: String,
    pub path: PathBuf,
}

/// A folder's subfolders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Children {
    pub folders: Vec<Child>,
    /// The folder has more subfolders than were listed.
    pub truncated: bool,
}

/// What is known of one listing.
#[derive(Clone, Debug, PartialEq)]
pub enum Listing {
    /// A worker is reading it.
    Pending,
    Roots(Arc<Vec<Root>>),
    Children(Arc<Children>),
    /// It could not be read (the reason).
    Failed(String),
}

/// The listings of the open dialog, by folder (`None`: the roots).
#[derive(Clone, Default)]
struct Cache {
    listings: HashMap<Option<PathBuf>, Listing>,
    /// Listings asked for once already: the first ask of each reads the disk again (`refresh`),
    /// so a folder made since the dialog last opened shows up.
    asked: HashSet<Option<PathBuf>>,
}

/// The cache's key, one per language (the places' names are written in it).
fn key() -> Id {
    Id::new("dialog:folderManager/listings").with(crate::i18n::current().code)
}

/// Forgets every listing: the dialog has just opened.
pub fn clear(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Cache>(key()));
}

/// The listing of `path` (`None`: the roots) if it is here, without asking for it.
pub fn peek(ctx: &Context, path: Option<&Path>) -> Option<Listing> {
    let k = path.map(Path::to_path_buf);
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(key()).listings.get(&k).cloned())
}

/// The listing of `path` (`None`: the roots), asking the engine for it when it is not here yet.
pub fn get(app: &mut App, ctx: &Context, path: Option<&Path>) -> Listing {
    let k = path.map(Path::to_path_buf);
    let (known, first) = ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cache>(key());
        (c.listings.get(&k).filter(|l| **l != Listing::Pending).cloned(), c.asked.insert(k.clone()))
    });
    if let Some(l) = known {
        return l;
    }
    let listing = ask(app, path, first);
    if listing == Listing::Pending {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(key()).listings.insert(k, listing.clone());
    });
    listing
}

/// Runs `folders.browse` for `path` once and reads its answer.
pub fn ask(app: &mut App, path: Option<&Path>, refresh: bool) -> Listing {
    let Some(session) = app.session_mut() else { return Listing::Pending };
    let mut params = json!({ "refresh": refresh });
    if let (Some(p), Some(o)) = (path, params.as_object_mut()) {
        o.insert("path".into(), json!(p));
    }
    match session.execute("folders.browse", &params) {
        Ok(v) if v["pending"] == true => Listing::Pending,
        Ok(v) if path.is_none() => Listing::Roots(Arc::new(roots(&v))),
        Ok(v) => Listing::Children(Arc::new(children(&v))),
        Err(e) => Listing::Failed(e.to_string()),
    }
}

fn path_of(v: &Value) -> Option<PathBuf> {
    v["path"].as_str().filter(|p| !p.is_empty()).map(PathBuf::from)
}

fn name_of(v: &Value, path: &Path) -> String {
    v["name"].as_str().map_or_else(|| path.display().to_string(), str::to_string)
}

/// The roots of a `folders.browse {}` answer: the places that exist (Pictures, Documents, Desktop,
/// Home), then the volumes; a volume already listed as a place is left out.
pub fn roots(v: &Value) -> Vec<Root> {
    let mut out: Vec<Root> = Vec::new();
    let empty = Vec::new();
    for p in v["places"].as_array().unwrap_or(&empty).iter().take(MAX_ENTRIES) {
        let Some(path) = path_of(p) else { continue };
        // A standard place by its name in the language in effect (the engine's is English).
        let (icon, home, name) = match p["kind"].as_str() {
            Some("pictures") => (FolderIcon::Pictures, false, tl!(ctx = "place", "Pictures")),
            Some("documents") => (FolderIcon::Documents, false, tl!(ctx = "place", "Documents")),
            Some("desktop") => (FolderIcon::Desktop, false, tl!(ctx = "place", "Desktop")),
            _ => (FolderIcon::Home, true, tl!(ctx = "place", "Home")),
        };
        let drive = home || path.parent().is_none();
        out.push(Root { name, path, icon, place: true, drive });
    }
    for vol in v["volumes"].as_array().unwrap_or(&empty).iter().take(MAX_ENTRIES) {
        let Some(path) = path_of(vol) else { continue };
        if out.iter().any(|r| r.path == path) {
            continue;
        }
        let icon = if vol["kind"] == "network" { FolderIcon::NetworkDrive } else { FolderIcon::Drive };
        out.push(Root { name: name_of(vol, &path), path, icon, place: false, drive: true });
    }
    out
}

/// The subfolders of a `folders.browse {path}` answer.
pub fn children(v: &Value) -> Children {
    let empty = Vec::new();
    let all = v["folders"].as_array().unwrap_or(&empty);
    let folders: Vec<Child> = all.iter().take(MAX_ENTRIES).filter_map(|f| path_of(f).map(|path| Child { name: name_of(f, &path), path })).collect();
    Children { folders, truncated: v["truncated"] == true || all.len() > MAX_ENTRIES }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_defensively() {
        let v = json!({
            "places": [
                { "path": "/h/Pictures", "kind": "pictures", "name": "Pictures" },
                { "path": "/h", "kind": "home", "name": "Home" },
                { "path": 3 },
                { "kind": "documents" }
            ],
            "volumes": [{ "path": "/", "kind": "system", "name": "/" }, { "path": "/h", "kind": "mounted" }, { "path": "/n", "kind": "network", "name": "n" }]
        });
        let r = roots(&v);
        let got: Vec<(&str, FolderIcon, bool)> = r.iter().map(|r| (r.name.as_str(), r.icon, r.drive)).collect();
        assert_eq!(
            got,
            [
                ("Pictures", FolderIcon::Pictures, false),
                ("Home", FolderIcon::Home, true),
                ("/", FolderIcon::Drive, true),
                ("n", FolderIcon::NetworkDrive, true)
            ]
        );
        assert!(roots(&json!({})).is_empty() && roots(&json!([1])).is_empty());
        let c = children(&json!({ "folders": [{ "path": "/a/b", "name": "b" }, { "path": "/a/c" }, { "name": "x" }], "truncated": true }));
        assert_eq!(c.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["b", "/a/c"]);
        assert!(c.truncated);
        assert_eq!(children(&json!(null)), Children::default());
    }
}
