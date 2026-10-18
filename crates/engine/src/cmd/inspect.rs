//! `library.inspect {photos?, section?, offset?, limit?}` (`control-protocol.md` §6.4): the library
//! as the user sees it, one implementation for every surface. The engine query of that name runs
//! [`inspect`] over the tree options the library's stored view settings ask for
//! ([`tree_options`]); the control method of the same name runs it over the window's own (its
//! collapsed groups, its folder sort) and adds the window's fields (`current`,
//! `visibleSections`, `selection`, `tray`, `scroll`); MCP's `doc_inspect` gives the engine's JSON
//! headless and the control method's in connect mode.
//!
//! What it gives: `revision`, `demo`, `watched`, `counts`, the `tree` (the Albums group first,
//! then the folder groups, each row with its widget id and section), the light table's `sections`
//! in order, `search` (`Session::search_inspect`), `view`, `undo`, `unsavedEdits`,
//! `pendingWrites`, `problems`, `fileOpRepairs` and `scan`; with `photos` the photos in light-table
//! order, paged. It walks the library a few times (a query an agent asks for, never per frame); a
//! page renders at most [`MAX_LIMIT`] photos and `problems` holds at most [`MAX_PROBLEMS`] entries.
//!
//! People data is withheld: there is no people model yet (M13), so `counts.people` is 0, the
//! People group has no rows and no photo carries a face or a name. When it comes, names appear
//! only for an app started with `--automation-allow-people` (`control-protocol.md` §6.4).

use std::collections::HashMap;
use std::path::Path;

use dalica_core::{FolderId, PhotoId};
use dalica_library::{
    AlbumRef, AlbumRow, AlbumTree, FolderRow, FolderSort, Group, Library, Row, Section, Tree, TreeMode, TreeOptions, section_photos,
};
use dalica_store::Recovery;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::search::SearchMode;
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "library.inspect",
        label: "Inspect Library",
        menu: "",
        keys: &[],
        params: r#"{"photos"?:bool|"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"|"album:search", "section"?:section id, "offset"?:int=0 (at most 1000000000), "limit"?:int=200 (at most 1000)}: the library as the user sees it: {revision, demo, watched, counts, tree, sections, search, view, undo, unsavedEdits, pendingWrites, problems, fileOpRepairs, scan}; with photos (true, or a section id, as section restricts) also photos [{id, section, path, name, star, caption, tags, hidden, rotate, edited, recipe, geo, place, date, width, height, displayed, bytes, video}] in light-table order (geo whether it is geotagged, place its [lat, lon] or null) (width × height the file's; displayed {width, height} the size it shows at, as photo.inspect's: grown by the recipe's frames and turned, a crop not counted) and page {offset, limit, total}; offset and limit only with photos"#,
        effect: Effect::Query,
        history: History::None,
        enabled: always,
        run,
    }]
}

/// The photos a page lists by default, and at most.
pub const DEFAULT_LIMIT: u64 = 200;
pub const MAX_LIMIT: u64 = 1_000;
/// The farthest a page may start.
pub const MAX_OFFSET: u64 = 1_000_000_000;
/// The most entries `problems` lists.
pub const MAX_PROBLEMS: usize = 1_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    photos: Option<Value>,
    section: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
}

/// The listing asked for: `None` for none, `Some(None)` for every section, `Some(Some(s))` for one.
type Listing = Option<Option<Section>>;

impl Params {
    /// Checks everything before anything is worked out.
    fn listing(&self) -> Result<Listing, CmdError> {
        let bad = |why: String| Err(CmdError::BadParams(why));
        let parse = |text: &str| text.parse::<Section>().map_err(|e| CmdError::BadParams(format!("`{text}`: {e}")));
        let off = matches!(self.photos, Some(Value::Bool(false)));
        let from_photos = match &self.photos {
            None | Some(Value::Null) | Some(Value::Bool(false)) => None,
            Some(Value::Bool(true)) => Some(None),
            Some(Value::String(s)) => Some(Some(parse(s)?)),
            Some(other) => return bad(format!("`photos` is true, false or a section id, not {other}")),
        };
        let section = match &self.section {
            Some(text) => Some(parse(text)?),
            None => None,
        };
        let listing = match (from_photos, section) {
            (Some(Some(a)), Some(b)) if a != b => return bad(format!("`photos` names {a} and `section` {b}; give one")),
            (_, Some(s)) if off => return bad(format!("`section` {s} lists photos; leave out `photos: false`")),
            (_, Some(s)) | (Some(Some(s)), None) => Some(Some(s)),
            (Some(None), None) => Some(None),
            (None, None) => None,
        };
        if listing.is_none() && (self.offset.is_some() || self.limit.is_some()) {
            return bad("`offset` and `limit` page the photo list; ask for it with `photos: true`".into());
        }
        if self.offset.is_some_and(|o| o > MAX_OFFSET) {
            return bad(format!("`offset` is at most {MAX_OFFSET}"));
        }
        if self.limit.is_some_and(|l| l > MAX_LIMIT) {
            return bad(format!("`limit` is at most {MAX_LIMIT}"));
        }
        Ok(listing)
    }
}

fn run(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    inspect(s, p, None)
}

/// The tree options the engine query inspects the library with, as the window draws it from the
/// stored view settings: Flat or Tree, the folder sort and its direction, Small and Hidden
/// Pictures, the projects folder, the small-picture threshold, every group expanded, and in search
/// mode its matches (with the search album while searching for text). Recently Updated is left
/// out, as the window leaves it out today.
pub fn tree_options(s: &Session) -> TreeOptions {
    let mut options = TreeOptions { now: None, ..s.search_tree_options() };
    options.small_threshold = s.settings().small_picture_threshold();
    options.filter = s.search_filter();
    let search = s.search_state();
    options.search = (options.filter.is_some() && search.mode == Some(SearchMode::Search)).then(|| search.text.trim().to_string());
    options
}

/// `library.inspect` over `tree` (the window's tree options; `None`: [`tree_options`]); the
/// offline roots always come from the session. Changes nothing.
pub fn inspect(s: &Session, p: &Value, tree: Option<TreeOptions>) -> Result<Value, CmdError> {
    let q: Params = params(p)?;
    let listing = q.listing()?;
    if let Some(Some(section)) = &listing {
        exists(s, section)?;
    }
    let mut options = tree.unwrap_or_else(|| tree_options(s));
    options.offline_roots = s.offline_roots();
    let lib = s.library();
    let folders = Tree::build(lib, s.roots(), &options);
    let albums = AlbumTree::build(lib, &options);
    let order = albums.sections_with(&folders);
    let shown = section_photos(lib, &order, &options);
    let mut out = Map::new();
    out.insert("revision".into(), json!(lib.revision()));
    out.insert("demo".into(), json!(is_demo(s.dir())));
    out.insert("watched".into(), watched(s));
    out.insert("counts".into(), counts(lib));
    out.insert("tree".into(), tree_json(&folders, &albums, &options));
    out.insert("sections".into(), sections(lib, &folders, &albums, &shown));
    out.insert("search".into(), s.search_inspect());
    out.insert("view".into(), view(s));
    let undo = s.undo_stack();
    out.insert("undo".into(), json!({ "undo": undo.undo_label(), "redo": undo.redo_label(), "depth": undo.len() }));
    out.insert("unsavedEdits".into(), unsaved(lib));
    out.insert("pendingWrites".into(), pending(s));
    out.insert("problems".into(), Value::Array(problems(s)));
    out.insert("fileOpRepairs".into(), json!(s.file_op_repairs().collect::<Vec<_>>()));
    let scan = s.scan_progress();
    out.insert("scan".into(), json!({ "running": scan.running, "foldersQueued": scan.folders_queued, "found": scan.found, "indexed": scan.indexed }));
    if let Some(one) = listing {
        let listed = match one {
            Some(section) => section_photos(lib, std::slice::from_ref(&section), &options),
            None => shown,
        };
        let total = listed.iter().fold(0usize, |n, (_, ids)| n.saturating_add(ids.len()));
        let offset = usize::try_from(q.offset.unwrap_or(0)).unwrap_or(usize::MAX);
        let limit = usize::try_from(q.limit.unwrap_or(DEFAULT_LIMIT)).unwrap_or(usize::MAX);
        let page: Vec<Value> = listed
            .iter()
            .flat_map(|(section, ids)| ids.iter().map(move |id| (section, *id)))
            .skip(offset)
            .take(limit)
            .filter_map(|(section, id)| photo(lib, section, id))
            .collect();
        out.insert("photos".into(), Value::Array(page));
        out.insert("page".into(), json!({ "offset": offset, "limit": limit, "total": total }));
    }
    Ok(Value::Object(out))
}

/// `not_found` for a section that is not in the library: an unknown folder or album, or the search
/// album outside search mode.
fn exists(s: &Session, section: &Section) -> Result<(), CmdError> {
    let lib = s.library();
    let known = match section {
        Section::Folder(f) => lib.folder(*f).is_some(),
        Section::Album(AlbumRef::User(a)) => lib.album(a).is_some(),
        Section::Album(AlbumRef::Search) => s.search_filter().is_some(),
        Section::Album(AlbumRef::Starred | AlbumRef::Recent) => true,
    };
    if known { Ok(()) } else { Err(CmdError::NotFound(format!("no section {section} in the library"))) }
}

/// Whether the library is a demo's, `<temp>/dalica-demo[-…]/library` (`demo.rs`), read from the
/// path alone (the session is not told).
fn is_demo(dir: &Path) -> bool {
    let name = |p: &Path| p.file_name().and_then(|n| n.to_str()).map(str::to_string).unwrap_or_default();
    let parent = dir.parent().map(name).unwrap_or_default();
    name(dir) == "library" && (parent == "dalica-demo" || parent.starts_with("dalica-demo-"))
}

fn watched(s: &Session) -> Value {
    let rules = s.folder_rules();
    let roots: Vec<Value> =
        s.roots().iter().map(|r| json!({ "path": r, "mode": rules.effective(r).mode.id(), "online": s.root_online(r).unwrap_or(true) })).collect();
    Value::Array(roots)
}

/// `photos` counts every photo and video, as `library.query` lists them; `videos` the videos
/// among them; `hidden` the photos hidden by Hide or in a hidden folder.
fn counts(lib: &Library) -> Value {
    let (mut photos, mut videos, mut hidden) = (0usize, 0usize, 0usize);
    for p in lib.photos() {
        photos = photos.saturating_add(1);
        if lib.facts(p.id).video {
            videos = videos.saturating_add(1);
        }
        if lib.is_hidden(p.id) || lib.is_folder_hidden(p.folder) {
            hidden = hidden.saturating_add(1);
        }
    }
    json!({ "photos": photos, "videos": videos, "folders": lib.folders().count(), "albums": lib.albums().count(), "people": 0, "hidden": hidden })
}

fn sort_name(sort: FolderSort) -> &'static str {
    match sort {
        FolderSort::Date => "date",
        FolderSort::RecentChanges => "recent",
        FolderSort::Size => "size",
        FolderSort::Name => "name",
    }
}

/// The automation id of a directory row that is no library folder (`control-protocol.md` §7):
/// FNV-1a over its path's text, as the window's tree gives it.
fn dir_widget(path: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("tree:dir:{h:016x}")
}

/// `{mode, sort, reverse, groups}`: the Albums group (when it has rows), then the folder groups.
/// An album row is `{widget, section, name, count}`, a folder row `{widget, section, name, count,
/// path, depth, year, hidden, offline}`, a year divider of the flat Folders group `{divider}`.
fn tree_json(folders: &Tree, albums: &AlbumTree, options: &TreeOptions) -> Value {
    let mut groups = Vec::with_capacity(folders.groups.len().saturating_add(1));
    if albums.count > 0 {
        let rows: Vec<Value> = albums
            .rows
            .iter()
            .map(|r| {
                json!({ "widget": format!("tree:album:{}", r.album), "section": r.section().map(|s| s.to_string()), "name": r.name, "count": r.count })
            })
            .collect();
        groups.push(json!({ "group": Group::Albums.id(), "collapsed": albums.collapsed, "count": albums.count, "rows": rows }));
    }
    for g in &folders.groups {
        let rows: Vec<Value> = g
            .rows
            .iter()
            .map(|r| match r {
                Row::Year(year) => json!({ "divider": year }),
                Row::Folder(f) => json!({
                    "widget": f.folder.map_or_else(|| dir_widget(&f.path), |id| format!("tree:folder:{id}")),
                    "section": f.section().map(|s| s.to_string()),
                    "name": f.name, "count": f.count, "path": f.path, "depth": f.depth,
                    "year": f.date.map(|d| d.date().year()), "hidden": f.hidden, "offline": options.is_offline(&f.path),
                }),
            })
            .collect();
        groups.push(json!({ "group": g.group.id(), "collapsed": g.collapsed, "count": g.count, "rows": rows }));
    }
    let mode = match options.mode {
        TreeMode::Flat => "flat",
        TreeMode::Tree => "tree",
    };
    json!({ "mode": mode, "sort": sort_name(options.sort), "reverse": options.reverse, "groups": groups })
}

/// The light table's sections in order: `{section, name, count, shown, date, place, description,
/// edited}`; `count` is every photo the section holds, `shown` those the view shows, `edited`
/// those shown with unsaved edits.
fn sections(lib: &Library, folders: &Tree, albums: &AlbumTree, shown: &[(Section, Vec<PhotoId>)]) -> Value {
    let mut in_folder: HashMap<FolderId, usize> = HashMap::new();
    for p in lib.photos() {
        let n = in_folder.entry(p.folder).or_insert(0);
        *n = n.saturating_add(1);
    }
    let folder_rows: HashMap<FolderId, &FolderRow> = folders
        .rows()
        .filter_map(|(_, r)| match r {
            Row::Folder(f) => f.folder.map(|id| (id, f)),
            Row::Year(_) => None,
        })
        .collect();
    let album_rows: HashMap<&AlbumRef, &AlbumRow> = albums.rows.iter().map(|r| (&r.album, r)).collect();
    let list: Vec<Value> = shown
        .iter()
        .map(|(section, ids)| {
            let edited = ids.iter().filter(|id| lib.has_unsaved_edits(**id)).count();
            let (name, count, date, place, description) = match section {
                Section::Folder(f) => {
                    let row = folder_rows.get(f);
                    let info = lib.folder_info(*f);
                    let name = row.map(|r| r.name.clone()).unwrap_or_default();
                    let date = row.and_then(|r| r.date).map(|d| d.date().to_string());
                    (name, in_folder.get(f).copied().unwrap_or(0), date, info.place.clone(), info.description.clone())
                }
                Section::Album(a) => {
                    let row = album_rows.get(a);
                    let name = row.map(|r| r.name.clone()).unwrap_or_default();
                    let date = row.and_then(|r| r.date).map(|d| d.date().to_string());
                    let (count, place, description) = match a {
                        AlbumRef::User(id) => {
                            let info = lib.album(id);
                            (
                                lib.album_members(id).len(),
                                info.map(|i| i.place.clone()).unwrap_or_default(),
                                info.map(|i| i.description.clone()).unwrap_or_default(),
                            )
                        }
                        AlbumRef::Starred => (lib.starred_photos().len(), String::new(), String::new()),
                        AlbumRef::Recent | AlbumRef::Search => (ids.len(), String::new(), String::new()),
                    };
                    (name, count, date, place, description)
                }
            };
            json!({
                "section": section.to_string(), "name": name, "count": count, "shown": ids.len(), "date": date,
                "place": place, "description": description, "edited": edited,
            })
        })
        .collect();
    Value::Array(list)
}

/// The stored view settings (`thumbSize` `null` for the default).
fn view(s: &Session) -> Value {
    let v = s.settings().view_options();
    json!({
        "hiddenPictures": v.hidden_pictures,
        "smallPictures": v.small_pictures,
        "caption": v.caption.as_deref().unwrap_or("none"),
        "thumbSize": v.thumb_size,
    })
}

/// `{photos, sections}`: how many photos have unsaved edits, and their folders' sections by id.
fn unsaved(lib: &Library) -> Value {
    let counts = lib.unsaved_counts();
    let photos = counts.values().fold(0usize, |a, n| a.saturating_add(*n));
    let mut folders: Vec<FolderId> = counts.keys().copied().collect();
    folders.sort();
    let sections: Vec<String> = folders.into_iter().map(|f| Section::Folder(f).to_string()).collect();
    json!({ "photos": photos, "sections": sections })
}

fn pending(s: &Session) -> Value {
    match s.pending_writes() {
        None => Value::Null,
        Some(p) => json!({ "sidecars": p.sidecars, "metadata": p.metadata, "index": p.index, "error": p.error }),
    }
}

/// What is wrong, `{kind, text, path}`: what opening the library repaired (`index`), damaged
/// files kept aside and other notices (`notice`), files scans could not read (`unreadable`) and
/// their other warnings (`scan`), and watched roots that are offline (`offline`). At most
/// [`MAX_PROBLEMS`].
fn problems(s: &Session) -> Vec<Value> {
    let mut out = Vec::new();
    for r in s.recovery() {
        let (text, path) = match r {
            Recovery::JournalTail { kept, dropped_bytes, reason } => {
                (format!("the index journal ended in a damaged record; kept {kept} changes and dropped {dropped_bytes} bytes: {reason}"), None)
            }
            Recovery::SnapshotSetAside { kept_as, reason } => (format!("the index snapshot was kept aside: {reason}"), Some(kept_as.clone())),
            Recovery::JournalReset { reason } => (format!("the index journal started again: {reason}"), None),
            Recovery::RebuildNeeded { reason } => (format!("the index was kept aside and is built again from the folders: {reason}"), None),
        };
        out.push(json!({ "kind": "index", "text": text, "path": path }));
    }
    let notices = s.notices().iter().map(|n| json!({ "kind": "notice", "text": n, "path": null }));
    let unreadable = s.scan_failures().iter().map(|f| json!({ "kind": "unreadable", "text": f.reason, "path": f.path }));
    let warnings = s.scan_warnings().iter().map(|w| json!({ "kind": "scan", "text": w, "path": null }));
    out.extend(notices.chain(unreadable).chain(warnings).take(MAX_PROBLEMS));
    out.extend(s.offline_roots().into_iter().map(|root| json!({ "kind": "offline", "text": "the volume is offline", "path": root })));
    out.truncate(MAX_PROBLEMS);
    out
}

/// One photo of a page: `{id, section, path, name, star, caption, tags, hidden, rotate, edited,
/// recipe, geo, place, date, width, height, displayed, bytes, video}`. `rotate` is the recipe's
/// clockwise turn in degrees, `edited` whether it has unsaved edits, `recipe` its operations, `geo`
/// whether it is geotagged (the library's word on its place, else its file's GPS: M13.14) and
/// `place` its geotag (`[lat, lon]`, or null), `date` its effective date (the user's, else the
/// file's), `width` and `height` the file's size and `displayed` the size it shows at
/// (`photo.inspect`'s).
fn photo(lib: &Library, section: &Section, id: PhotoId) -> Option<Value> {
    let p = lib.photo(id)?;
    let folder = lib.folder(p.folder)?;
    let info = lib.info(id);
    let facts = lib.facts(id);
    let turn = dalica_edit::Recipe::parse(&info.edits).map(|r| r.orientation()).unwrap_or(dalica_edit::Dihedral::IDENTITY);
    let (dw, dh) = crate::cmd::photo::displayed_size(&info.edits, p.width, p.height);
    Some(json!({
        "id": id, "section": section.to_string(), "path": folder.path.join(&p.name), "name": p.name, "star": p.star,
        "caption": info.caption, "tags": info.tags, "hidden": info.hidden || lib.is_folder_hidden(p.folder),
        "rotate": u16::from(turn.quarters()).saturating_mul(90), "edited": lib.has_unsaved_edits(id), "recipe": info.edits,
        "geo": lib.is_geotagged(id), "place": lib.geotag(id), "date": lib.effective_date(id), "width": p.width, "height": p.height,
        "displayed": { "width": dw, "height": dh }, "bytes": p.size, "video": facts.video,
    }))
}
