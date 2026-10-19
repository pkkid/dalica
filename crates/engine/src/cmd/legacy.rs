//! The previous organizer's data (`data-model.md` §9; the state is `crate::legacy`, M14.13):
//!
//! - `legacy.inspect {folders?}`: each folder's source (the sidecar in effect: `dalica`,
//!   `legacy`, `both` or `none`) and what the model holds for it, by default every folder with
//!   legacy data; the option, the notice, and the albums and people that carry a legacy id.
//! - `legacy.noticeShown {}`: the first-run notice answered (`files.legacyNoticeShown` set, so it
//!   is not given again).

use std::collections::BTreeSet;

use dalica_core::{FolderId, PersonId};
use dalica_library::{FolderSidecar, Library, PhotoInfo};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// The most folders one `legacy.inspect` names.
pub const MAX_FOLDERS: usize = 100_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "legacy.inspect",
            label: "Inspect Legacy Data",
            menu: "",
            keys: &[],
            params: r#"{"folders"?:[FolderId] (at most 100000; default: every folder with legacy data)}: the previous organizer's data: {readLegacyData (the option), notice (the folder count the first-run notice waits to give, or null), noticeShown, albums, people (how many carry a legacy id), folders:[{folder, path, source ("dalica"|"legacy"|"both"|"none": the sidecar in effect), photos, starred, captioned, tagged, hidden, edited, legacyEdits (recipe entries kept as legacy:), faces, geotagged, albums, people (distinct ones with a legacy id)}]}; an unknown folder is not_found"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: inspect,
        },
        CommandSpec {
            id: "legacy.noticeShown",
            label: "Legacy Notice Shown",
            menu: "",
            keys: &[],
            params: r#"{}: the first-run notice of legacy data was answered: files.legacyNoticeShown is set (one write of settings.json, no undo step) and the notice is not given again; answers {"changed": bool}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: notice_shown,
        },
    ]
}

/// A source's name as the control API gives it.
pub fn source_name(s: FolderSidecar) -> &'static str {
    match s {
        FolderSidecar::None => "none",
        FolderSidecar::Dalica => "dalica",
        FolderSidecar::Legacy => "legacy",
        FolderSidecar::Both => "both",
    }
}

/// Each folder's source as the control API gives it: the fact the scans recorded where legacy
/// data is involved (`legacy`, `both`), else `dalica` for a folder the model says something
/// about (what Dalica's own file holds, or will once written) and `none` for one it says nothing
/// about. Made once per query: one pass over the photos and the memberships.
pub struct Sources<'a> {
    lib: &'a Library,
    own: std::collections::HashSet<FolderId>,
}

impl<'a> Sources<'a> {
    pub fn new(lib: &'a Library) -> Sources<'a> {
        let mut own: std::collections::HashSet<FolderId> = lib
            .photos()
            .filter(|p| p.star || !lib.info(p.id).is_empty() || !lib.faces(p.id).is_empty() || lib.geo(p.id).is_some())
            .map(|p| p.folder)
            .collect();
        for (_, members) in lib.memberships() {
            own.extend(members.keys().filter_map(|p| lib.photo(*p)).map(|p| p.folder));
        }
        own.extend(
            lib.folders()
                .filter(|f| !lib.folder_info(f.id).is_empty() || lib.project(f.id).is_some() || !lib.folder_sort(f.id).is_default())
                .map(|f| f.id),
        );
        Sources { lib, own }
    }

    /// Folder `id`'s source.
    pub fn of(&self, id: FolderId) -> &'static str {
        match self.lib.folder_sidecar(id) {
            FolderSidecar::None | FolderSidecar::Dalica if self.own.contains(&id) => source_name(FolderSidecar::Dalica),
            FolderSidecar::None | FolderSidecar::Dalica => source_name(FolderSidecar::None),
            other => source_name(other),
        }
    }
}

/// How many of a recipe's entries are kept as `legacy:` (an operation of the previous organizer
/// Dalica has no equivalent for).
pub fn legacy_entries(info: &PhotoInfo) -> usize {
    info.edits.iter().filter(|e| e.starts_with("legacy:")).count()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspect {
    folders: Option<Vec<FolderId>>,
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Inspect { folders } = params(p)?;
    let lib = s.library();
    let folders = match folders {
        Some(list) => {
            if list.len() > MAX_FOLDERS {
                return Err(CmdError::Limit(format!("at most {MAX_FOLDERS} folders at once")));
            }
            let mut seen = BTreeSet::new();
            let mut out = Vec::with_capacity(list.len());
            for id in list {
                if lib.folder(id).is_none() {
                    return Err(CmdError::NotFound(format!("no folder {id}")));
                }
                if seen.insert(id) {
                    out.push(id);
                }
            }
            out
        }
        None => lib.legacy_folders(),
    };
    let sources = Sources::new(lib);
    let rows: Vec<Value> = folders.into_iter().filter_map(|id| folder(lib, &sources, id)).collect();
    let settings = s.settings().legacy();
    Ok(json!({
        "readLegacyData": settings.read_legacy_data,
        "notice": s.legacy().notice(),
        "noticeShown": settings.legacy_notice_shown,
        "albums": lib.album_legacies().count(),
        "people": lib.person_legacies().count(),
        "folders": rows,
    }))
}

/// One folder's source and what the model holds for it.
fn folder(lib: &Library, sources: &Sources<'_>, id: FolderId) -> Option<Value> {
    let path = &lib.folder(id)?.path;
    let photos = lib.photos_in(id);
    let (mut starred, mut captioned, mut tagged, mut hidden, mut edited, mut entries, mut faces, mut geotagged) =
        (0usize, 0, 0, 0, 0, 0usize, 0usize, 0);
    let mut albums = BTreeSet::new();
    let mut people: BTreeSet<&PersonId> = BTreeSet::new();
    for p in &photos {
        let info = lib.info(p.id);
        starred += usize::from(p.star);
        captioned += usize::from(!info.caption.is_empty());
        tagged += usize::from(!info.tags.is_empty());
        hidden += usize::from(info.hidden);
        edited += usize::from(!info.edits.is_empty());
        entries = entries.saturating_add(legacy_entries(info));
        let list = lib.faces(p.id);
        faces = faces.saturating_add(list.len());
        people.extend(list.iter().filter_map(|f| f.person.person()).filter(|id| lib.person_legacy(id).is_some()));
        geotagged += usize::from(lib.is_geotagged(p.id));
        albums.extend(lib.albums_of(p.id).into_iter().map(|(a, _)| a).filter(|a| lib.album_legacy(a).is_some()));
    }
    Some(json!({
        "folder": id,
        "path": path,
        "source": sources.of(id),
        "photos": photos.len(),
        "starred": starred,
        "captioned": captioned,
        "tagged": tagged,
        "hidden": hidden,
        "edited": edited,
        "legacyEdits": entries,
        "faces": faces,
        "geotagged": geotagged,
        "albums": albums.len(),
        "people": people.len(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn notice_shown(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    let changed = s.legacy_notice_shown();
    Ok(json!({ "changed": changed }))
}
