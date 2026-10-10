//! Library commands: queries and rebuilding the index.

use dalica_core::FolderId;
use serde::Deserialize;
use serde_json::{Value, json};

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
            params: r#"{"folder"?:FolderId, "starred"?:bool, "offset"?:int=0, "limit"?:int=1000 (max 10000)}: photos in display order, and the total"#,
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
    starred: Option<bool>,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}

fn query(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let q: Query = params(p)?;
    let limit = q.limit.unwrap_or(1000);
    if limit > MAX_LIMIT {
        return Err(CmdError::Limit(format!("limit is at most {MAX_LIMIT}")));
    }
    let lib = s.library();
    let folders: Vec<FolderId> = match q.folder {
        Some(f) if lib.folder(f).is_none() => return Err(CmdError::NotFound(format!("no folder {f}"))),
        Some(f) => vec![f],
        None => lib.folders_in_display_order().iter().map(|f| f.id).collect(),
    };
    let matching: Vec<Value> = folders
        .into_iter()
        .flat_map(|f| lib.photos_in(f))
        .filter(|p| q.starred.is_none_or(|want| p.star == want))
        .map(|p| json!({ "id": p.id, "folder": p.folder, "name": p.name, "star": p.star, "width": p.width, "height": p.height, "date": p.date }))
        .collect();
    let total = matching.len();
    let page: Vec<Value> = matching.into_iter().skip(q.offset).take(limit).collect();
    Ok(json!({ "total": total, "offset": q.offset, "photos": page }))
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

    use crate::cmd::testing::{hostile, library};
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
        hostile(&mut s, "library.query");
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
        hostile(&mut s, "library.rebuild");
    }
}
