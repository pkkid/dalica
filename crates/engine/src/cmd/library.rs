//! Library commands: queries and rebuilding the index.
//!
//! - `library.query` lists photos in light-table order, paged: all of them, a folder's, a
//!   section's (`folder:12`, `album:starred`, `album:recent`, `album:<id>`, and `album:search`, the
//!   search mode's matches), and those a search finds (`text` in the search box's grammar, the
//!   filter buttons, the age), asked of the search worker (`search.rs`); the lists combine. Hidden
//!   photos and the photos of hidden folders are left out unless the view shows hidden pictures.
//! - `library.rebuild` rebuilds the index from the watched folders, in the background.

use std::collections::HashSet;

use dalica_core::{FolderId, PhotoId};
use dalica_library::search::{Age, Filters, Query as SearchQuery};
use dalica_library::{AlbumRef, Section, section_photos};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::search::{FilterParams, check_text};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "library.query",
            label: "Query Library",
            menu: "",
            keys: &[],
            params: r#"{"folder"?:FolderId, "section"?:"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"|"album:search"|"person:<id>", "text"?:string (the search box's grammar), "filters"?:{"starred"?:bool, "faces"?:bool, "videos"?:bool, "geotagged"?:bool}, "age"?:days (0: every age), "starred"?:bool, "offset"?:int=0, "limit"?:int=1000 (max 10000)}: photos in light-table order (a section's in its own order), every condition given applying, and the total; with text, filters or age also truncated and unavailable"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: query,
        },
        CommandSpec {
            id: "library.rebuild",
            label: "Rebuild Library…",
            menu: "Help/Rebuild Library…",
            keys: &[],
            params: "{}: rebuild the index from the watched folders (stars come back from the sidecars); runs in the background",
            effect: Effect::Library,
            history: History::None,
            enabled: |s, _| if s.roots().is_empty() { Err(crate::registry::Disabled("Add a folder first.".into())) } else { Ok(()) },
            run: rebuild,
        },
    ]
}

/// The most photos one query returns.
const MAX_LIMIT: usize = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    folder: Option<FolderId>,
    section: Option<String>,
    text: Option<String>,
    filters: Option<FilterParams>,
    age: Option<u32>,
    starred: Option<bool>,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}

/// The photos of `section` in its order: `album:search` lists the search mode's matches.
fn section_ids(s: &mut Session, text: &str) -> Result<Vec<PhotoId>, CmdError> {
    let section: Section = text.parse().map_err(|e: dalica_library::Error| CmdError::BadParams(e.to_string()))?;
    let lib = s.library();
    match &section {
        Section::Album(AlbumRef::Search) => {
            return s.search_results_now()?.ok_or_else(|| CmdError::NotFound("no album:search: not searching".into()));
        }
        Section::Folder(f) if lib.folder(*f).is_none() => return Err(CmdError::NotFound(format!("no folder {f}"))),
        Section::Album(AlbumRef::User(a)) if lib.album(a).is_none() => return Err(CmdError::NotFound(format!("no album {a}"))),
        Section::Person(p) if lib.person(p).is_none() => return Err(CmdError::NotFound(format!("no person {p}"))),
        _ => {}
    }
    let options = s.search_tree_options();
    Ok(section_photos(s.library(), std::slice::from_ref(&section), &options).into_iter().flat_map(|(_, ids)| ids).collect())
}

fn query(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let q: Query = params(p)?;
    let limit = q.limit.unwrap_or(1000);
    if limit > MAX_LIMIT {
        return Err(CmdError::Limit(format!("limit is at most {MAX_LIMIT}")));
    }
    if let Some(text) = &q.text {
        check_text(text)?;
    }
    if let Some(f) = q.folder
        && s.library().folder(f).is_none()
    {
        return Err(CmdError::NotFound(format!("no folder {f}")));
    }
    let search = SearchQuery {
        text: q.text.unwrap_or_default(),
        filters: q.filters.unwrap_or_default().over(Filters::default()),
        age: q.age.filter(|d| *d > 0).map(|days| Age { days, now: s.clock().now() }),
    };
    let section = match &q.section {
        Some(text) => Some(section_ids(s, text)?),
        None => None,
    };
    let mut found = None;
    let mut ids: Vec<PhotoId> = match section {
        Some(ids) => ids,
        None if search.is_active() => {
            let r = s.ask_search(search.clone())?;
            let ids = r.photos().to_vec();
            found = Some(r);
            ids
        }
        None => {
            // Hidden photos and those of hidden folders only while the view shows hidden pictures,
            // as a section and a search leave them out.
            let hidden_pictures = s.search_tree_options().hidden_pictures;
            let lib = s.library();
            let folders: Vec<FolderId> = match q.folder {
                Some(f) => vec![f],
                None => lib.folders_in_display_order().iter().map(|f| f.id).collect(),
            };
            folders
                .into_iter()
                .filter(|f| hidden_pictures || !lib.is_folder_hidden(*f))
                .flat_map(|f| lib.photos_in(f))
                .filter(|p| hidden_pictures || !lib.is_hidden(p.id))
                .map(|p| p.id)
                .collect()
        }
    };
    if search.is_active() && found.is_none() {
        let r = s.ask_search(search)?;
        let matched: HashSet<PhotoId> = r.photos().iter().copied().collect();
        ids.retain(|id| matched.contains(id));
        found = Some(r);
    }
    let lib = s.library();
    let matching: Vec<Value> = ids
        .into_iter()
        .filter_map(|id| lib.photo(id))
        .filter(|p| q.folder.is_none_or(|f| p.folder == f))
        .filter(|p| q.starred.is_none_or(|want| p.star == want))
        .map(|p| json!({ "id": p.id, "folder": p.folder, "name": p.name, "star": p.star, "width": p.width, "height": p.height, "date": p.date }))
        .collect();
    let total = matching.len();
    let page: Vec<Value> = matching.into_iter().skip(q.offset).take(limit).collect();
    let mut out = json!({ "total": total, "offset": q.offset, "photos": page });
    if let Some(r) = found {
        out["truncated"] = json!(r.truncated);
        out["unavailable"] = json!(r.unavailable);
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn rebuild(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    s.start_rebuild();
    Ok(json!({ "started": true, "folders": s.roots().len() }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn query_filters_pages_and_orders() {
        let (_dirs, mut s, ids) = library(6);
        s.execute("photo.star.set", &json!({ "ids": [ids[1], ids[4]], "on": true })).unwrap();
        let all = s.execute("library.query", &json!({})).unwrap();
        assert_eq!(all["total"], 6);
        let names: Vec<&str> = all["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0004.jpg", "IMG_0005.jpg", "IMG_0006.jpg"]);
        let starred = s.execute("library.query", &json!({ "starred": true })).unwrap();
        assert_eq!(starred["total"], 2);
        let page = s.execute("library.query", &json!({ "offset": 4, "limit": 1 })).unwrap();
        assert_eq!((page["total"].as_u64(), page["photos"][0]["name"].as_str()), (Some(6), Some("IMG_0005.jpg")));
        let folder = s.library().photo(ids[0]).unwrap().folder;
        assert_eq!(s.execute("library.query", &json!({ "folder": folder })).unwrap()["total"], 6);
        assert!(matches!(s.execute("library.query", &json!({ "folder": 999 })), Err(CmdError::NotFound(_))));
        assert!(matches!(s.execute("library.query", &json!({ "limit": 10_001 })), Err(CmdError::Limit(_))));
        assert_eq!(s.undo_stack().len(), 1);
    }

    #[test]
    fn rebuild_gives_back_the_stars_and_clears_undo() {
        let (_dirs, mut s, ids) = library(4);
        s.execute("photo.star.set", &json!({ "ids": [ids[0], ids[3]], "on": true })).unwrap();
        s.flush().unwrap();
        let starred: Vec<String> = s.library().starred().iter().map(|p| p.name.clone()).collect();
        s.execute("library.rebuild", &json!({})).unwrap();
        s.wait_idle();
        let again: Vec<String> = s.library().starred().iter().map(|p| p.name.clone()).collect();
        assert_eq!(again, starred);
        assert_eq!(s.library().photos().count(), 4);
        assert!(s.undo_stack().is_empty());
        assert!(matches!(s.execute("edit.undo", &json!({})), Err(CmdError::Disabled(_))));
    }
}
