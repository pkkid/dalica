//! Search commands (`organizing.md` §8, `ui-layout.md` §9), run on the search worker
//! (`search.rs`):
//!
//! - `search.set {text?, filters?, age?, people?, withholdNames?}` sets the search field's text,
//!   the filter buttons, the age slider (in days) and the People pane's exact filter (person ids:
//!   the photos holding a face of every one; an unknown id is `not_found`); what is left out stays
//!   as it was (each filter too). `withholdNames` is per call: people's names and nicknames take
//!   no part in this search (`person:` matches nothing and is `unavailable`, plain words skip
//!   them); left out, names match. An automation surface without the people permission sets it on
//!   every call (`automation::NAME_SEARCHES`). No undo step. The
//!   query runs once the debounce has passed ([`crate::search::DEBOUNCE`] on the session clock),
//!   on the worker; the reply is `library.inspect` → `search` at once (`pending` until the result
//!   is applied by `poll`). A query asking for nothing leaves search mode.
//! - `search.clear` leaves search mode (the × in the field, Esc, Show All).
//! - `search.suggest {text, limit?}` (query): the autocomplete for the last word of `text`: tags,
//!   folders and albums with counts (`tag:` gives tags only).
//! - `search.saveAsAlbum {name}` (Tools ▸ Experimental ▸ Save Search Results…): a user album of the
//!   matches in light-table order, one undo step.

use dalica_core::PersonId;
use dalica_library::search::{Filters, MAX_PEOPLE, Suggestion};
use dalica_library::{AlbumInfo, Op, clean_album_name};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::search::{MAX_TEXT, random_bits};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "search.set",
            label: "Search",
            menu: "",
            keys: &[],
            params: r#"{"text"?:string (at most 16 KiB; the grammar reads 1 KiB), "filters"?:{"starred"?:bool, "faces"?:bool, "videos"?:bool, "geotagged"?:bool}, "age"?:days|0|null (0 or null: every age), "people"?:[PersonId] (the People pane's filter: photos holding a face of every one; at most 1024; [] clears it), "withholdNames"?:bool (this call only, default false: people's names and nicknames take no part, person: matches nothing and is unavailable; automation without --automation-allow-people always sends true)}: what is left out stays (but withholdNames); runs the search after the debounce, on the worker, without an undo step. Replies library.inspect's search: {active, mode, text, filters, age, people, matches, folders, bytes, seconds, sections, unavailable, truncated, pending, generation} and changed"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: set,
        },
        CommandSpec {
            id: "search.clear",
            label: "Show All",
            menu: "",
            keys: &[],
            params: "{}: leave search mode (no text, filters, age or people); replies library.inspect's search",
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: clear,
        },
        CommandSpec {
            id: "search.suggest",
            label: "Search Suggestions",
            menu: "",
            keys: &[],
            params: r#"{"text":string (at most 16 KiB), "limit"?:int=100 (max 100)}: the autocomplete for the last word of text: {text, suggestions:[{kind:"tag"|"folder"|"album", text, count, section:null|"folder:<id>"|"album:<id>"}]}; tags, then folders, then albums, each by count then name; "tag:" gives tags only"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: suggest,
        },
        CommandSpec {
            id: "search.saveAsAlbum",
            label: "Save Search Results…",
            menu: "Tools/Experimental/Save Search Results…",
            keys: &[],
            params: r#"{"name":string}: a new album of the search's matches in light-table order; one undo step. Replies {album, name, count}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: searching,
            run: save_as_album,
        },
    ]
}

/// Why Save Search as Album is greyed out while not searching.
const NOT_SEARCHING: &str = "Search for something first.";

fn searching(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.search_state().mode == Some(crate::search::SearchMode::Search) { Ok(()) } else { Err(Disabled(NOT_SEARCHING.into())) }
}

/// The filter buttons as a command names them: each one left out stays as it was.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FilterParams {
    starred: Option<bool>,
    faces: Option<bool>,
    videos: Option<bool>,
    geotagged: Option<bool>,
}

impl FilterParams {
    /// `base` with the buttons named here set.
    pub(crate) fn over(self, base: Filters) -> Filters {
        Filters {
            starred: self.starred.unwrap_or(base.starred),
            faces: self.faces.unwrap_or(base.faces),
            videos: self.videos.unwrap_or(base.videos),
            geotagged: self.geotagged.unwrap_or(base.geotagged),
        }
    }
}

/// A field that may be left out (`None`), `null` (`Some(None)`) or a value.
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// `Err(Limit)` for search text over [`MAX_TEXT`] bytes.
pub(crate) fn check_text(text: &str) -> Result<(), CmdError> {
    if text.len() > MAX_TEXT {
        return Err(CmdError::Limit(format!("the search text is at most {MAX_TEXT} bytes")));
    }
    Ok(())
}

/// The People pane's filter as a command gives it: at most [`MAX_PEOPLE`], each a person the
/// library has (`not_found` names the first it lacks), each kept once in the order first given.
pub(crate) fn check_people(s: &Session, mut people: Vec<PersonId>) -> Result<Vec<PersonId>, CmdError> {
    if people.len() > MAX_PEOPLE {
        return Err(CmdError::Limit(format!("`people` names at most {MAX_PEOPLE} people")));
    }
    let mut seen = std::collections::HashSet::with_capacity(people.len());
    people.retain(|p| seen.insert(p.clone()));
    if let Some(unknown) = people.iter().find(|p| s.library().person(p).is_none()) {
        return Err(CmdError::NotFound(format!("no person {unknown}")));
    }
    Ok(people)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SetParams {
    text: Option<String>,
    filters: Option<FilterParams>,
    #[serde(default, deserialize_with = "present")]
    age: Option<Option<u32>>,
    people: Option<Vec<PersonId>>,
    #[serde(default)]
    withhold_names: bool,
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetParams { text, filters, age, people, withhold_names } = params(p)?;
    if let Some(text) = &text {
        check_text(text)?;
    }
    let people = people.map(|p| check_people(s, p)).transpose()?;
    let filters = filters.unwrap_or_default().over(s.search_state().filters);
    let changed = s.set_search(text, filters, age, people, withhold_names);
    let mut reply = s.search_inspect();
    reply["changed"] = json!(changed);
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn clear(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    s.clear_search();
    Ok(s.search_inspect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestParams {
    text: String,
    limit: Option<usize>,
}

/// One suggestion as the commands give it: the section by its id (`folder:12`).
pub(crate) fn suggestion_json(s: &Suggestion) -> Value {
    json!({ "kind": s.kind, "text": s.text, "count": s.count, "section": s.section.as_ref().map(ToString::to_string) })
}

fn suggest(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SuggestParams { text, limit } = params(p)?;
    check_text(&text)?;
    let limit = limit.unwrap_or(dalica_library::search::MAX_SUGGESTIONS);
    if limit > dalica_library::search::MAX_SUGGESTIONS {
        return Err(CmdError::Limit(format!("limit is at most {}", dalica_library::search::MAX_SUGGESTIONS)));
    }
    let list = s.search_suggestions(&text, limit)?;
    Ok(json!({ "text": text, "suggestions": list.iter().map(suggestion_json).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveParams {
    name: String,
}

/// Save Search as Album: the album is made, then the matches added in light-table order; the two
/// ops are one undo step (`Session::execute` gathers them).
fn save_as_album(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SaveParams { name } = params(p)?;
    let name = clean_album_name(&name).ok_or_else(|| CmdError::BadParams("the album needs a name".into()))?;
    let mut ids = s.search_results_now()?.ok_or_else(|| CmdError::Disabled(NOT_SEARCHING.into()))?;
    ids.retain(|id| s.library().photo(*id).is_some());
    if ids.is_empty() {
        return Err(CmdError::Disabled("No photos match the search.".into()));
    }
    let album = s.library().new_album_id(random_bits).map_err(|e| CmdError::Internal(e.to_string()))?;
    let now = s.clock().now();
    let create = Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named(name.clone()))] };
    let create = s.library().stamped(create, now);
    s.apply(create)?;
    let add = match s.library().album_add_op(&album, &ids, None) {
        Ok(op) => s.library().stamped(op, now),
        Err(e) => {
            // Not one photo could be placed: the album goes again, so nothing changed.
            s.apply(Op::DeleteAlbums { ids: vec![album] })?;
            return Err(CmdError::Limit(e.to_string()));
        }
    };
    s.apply(add)?;
    Ok(json!({ "album": album, "name": name, "count": ids.len() }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn set_reports_the_query_and_clear_leaves_search_mode() {
        let (_dirs, mut s, _) = library(3);
        let v = s.execute("search.set", &json!({ "text": "img", "filters": { "starred": false } })).unwrap();
        assert_eq!(
            (v["active"].as_bool(), v["mode"].as_str(), v["changed"].as_bool(), v["pending"].as_bool()),
            (Some(true), Some("search"), Some(true), Some(true))
        );
        assert_eq!(v["matches"], json!(null));
        // The same query again changes nothing.
        assert_eq!(s.execute("search.set", &json!({ "text": "img" })).unwrap()["changed"], json!(false));
        s.wait_search();
        let v = s.search_inspect();
        assert_eq!((v["matches"].as_u64(), v["folders"].as_u64(), v["pending"].as_bool()), (Some(3), Some(1), Some(false)));
        let v = s.execute("search.clear", &json!({})).unwrap();
        assert_eq!((v["active"].as_bool(), v["text"].as_str(), v["matches"].clone()), (Some(false), Some(""), json!(null)));
        assert_eq!(s.undo_stack().len(), 0);
    }

    #[test]
    fn graceful_failures() {
        let (_dirs, mut s, _) = library(1);
        for bad in
            [json!({ "age": -1 }), json!({ "age": 1.5 }), json!({ "text": 3 }), json!({ "filters": { "people": true } }), json!({ "filters": 1 })]
        {
            assert!(matches!(s.execute("search.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        let long = "a".repeat(super::MAX_TEXT + 1);
        assert!(matches!(s.execute("search.set", &json!({ "text": long })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("search.suggest", &json!({ "text": long })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("search.suggest", &json!({ "text": "a", "limit": 101 })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("search.suggest", &json!({})), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("search.clear", &json!({ "x": 1 })), Err(CmdError::BadParams(_))));
        assert_eq!(s.execute("search.saveAsAlbum", &json!({ "name": "x" })), Err(CmdError::Disabled(super::NOT_SEARCHING.into())));
        s.execute("search.set", &json!({ "text": "zzz-nothing" })).unwrap();
        assert!(matches!(s.execute("search.saveAsAlbum", &json!({ "name": "  " })), Err(CmdError::BadParams(_))));
        assert_eq!(s.execute("search.saveAsAlbum", &json!({ "name": "x" })), Err(CmdError::Disabled("No photos match the search.".into())));
        assert_eq!(s.undo_stack().len(), 0);
    }
}
