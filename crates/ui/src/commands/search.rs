//! Search in the UI (`organizing.md` §8, `ui-layout.md` §9): the view state's `search` (the field's
//! text, the autocomplete highlight, the filters, the age slider's position, and the scroll anchor
//! saved when search mode starts) is driven by the toolbar, the banner and `ui.set`; the engine
//! runs the query on its worker (`search.set`, `search.clear`), and the toolbar, the filter banner
//! and the light table show its answer.
//!
//! - `search.focus` (Ctrl+F, F3 in the library): puts the keyboard focus in the toolbar's search
//!   field; the toolbar takes the request on its next frame.
//! - `search.tag {tag}` (the Tags pane's Search for This Tag): searches for the photos with the tag,
//!   the field reading `tag:<tag>` ([`tag_word`]).
//! - Save Search Results… (Tools ▸ Experimental) is an opener ([`SAVE_RESULTS`]): Album Properties
//!   named `Search results for "x"`, whose OK runs the engine's `search.saveAsAlbum {name}`.
//!
//! **The view state and the engine stay in step** ([`sync`], once a frame before the sections are
//! built): a change of the view state's query (typing, a filter, the age, `ui.set`) is sent as
//! `search.set`; a change of the engine's (a script's `search.set` or `search.clear`, Find
//! Duplicates entering its mode) is copied into the view state. The engine's age is in days, the
//! slider's a position (0 … 1), turned into days by `ui-kit`'s logarithmic map
//! ([`widgets::age_days`], [`widgets::age_position`]). When search mode starts, the light table's
//! scroll anchor is saved (unless `ui.set` gave one) and the view goes to the top; when it ends, the
//! saved anchor comes back. [`clear`] (the field's ×, Esc in the field, Show All) leaves search
//! mode, Find Duplicates' too.

use std::time::Duration;

use dalica_engine::search::SearchMode;
use dalica_engine::{CmdError, Context};
use dalica_ui_kit::widgets;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, Opener, UiCommand, no_params, parse_object};
use crate::app::App;
use crate::dialogs::album_properties;
use crate::state::{AlbumDraft, Filters, MAX_DRAFT, ScrollAnchor, SearchState};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "search.focus",
        label: "Find",
        menu: "",
        keys: &[(Context::Library, "Ctrl+F"), (Context::Library, "F3")],
        params: "{}: put the keyboard focus in the toolbar's search field",
        run: focus,
    },
    UiCommand {
        id: "search.tag",
        label: "Search for This Tag",
        menu: "",
        keys: &[],
        params: "{\"tag\":string}: search for the photos with this tag (the search field reads `tag:<tag>`, its spaces as dashes); replies library.inspect's search",
        run: search_tag,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("search.tag", library_open)];

/// Tools ▸ Experimental ▸ Save Search Results…: Album Properties for the search's matches; its OK
/// runs the engine's `search.saveAsAlbum {name}`.
pub const SAVE_RESULTS: Opener = Opener { id: SAVE_COMMAND, dialog: "albumProperties", keys: &[], open: open_save, check: can_save };

/// The engine command Album Properties' OK runs for Save Search Results….
pub const SAVE_COMMAND: &str = "search.saveAsAlbum";

/// Why the commands wait for the library.
const OPENING: &str = "The library is still opening.";
/// Why Save Search Results… is greyed out with no match.
pub const NO_MATCHES: &str = "No photos match the search.";

/// `search.focus`: asks the toolbar to focus the search field. Replies `{}`.
fn focus(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().focus_search = true;
    Ok(json!({}))
}

fn library_open(app: &App, _: &Value) -> Result<(), String> {
    if app.session().is_some() { Ok(()) } else { Err(OPENING.into()) }
}

/// The search word of a tag: `tag:` and the tag, its runs of spaces as single dashes (the grammar
/// splits a value at punctuation, so `tag:new-york` finds the photos tagged "new york").
pub fn tag_word(tag: &str) -> String {
    format!("tag:{}", tag.split_whitespace().collect::<Vec<_>>().join("-"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagParams {
    tag: String,
}

/// `search.tag {tag}`: the search field reads `tag:<tag>` and the search runs (the filters and the
/// age stay). Replies `library.inspect`'s search.
fn search_tag(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: TagParams = parse_object(params)?;
    let tag = p.tag.trim();
    if tag.is_empty() {
        return Err(CmdError::BadParams("`tag` names a tag".into()));
    }
    if tag.chars().count() > MAX_DRAFT - 4 {
        return Err(CmdError::Limit(format!("`tag` is at most {} characters", MAX_DRAFT - 4)));
    }
    library_open(app, params).map_err(CmdError::Disabled)?;
    let search = &mut app.state_mut().search;
    search.text = tag_word(tag);
    search.highlight = None;
    push(app)
}

/// The query of the view state as the engine takes it: the text, the filters and the age in days
/// (`None`: every age).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Query {
    text: String,
    filters: Filters,
    days: Option<u32>,
}

fn query_of(s: &SearchState) -> Query {
    Query { text: s.text.clone(), filters: s.filters, days: widgets::age_days(s.age) }
}

/// Sends the view state's query to the engine (`search.set`). Replies what the engine replies.
fn push(app: &mut App) -> Result<Value, CmdError> {
    let q = query_of(&app.state().search);
    let f = q.filters;
    let params = json!({
        "text": q.text,
        "filters": { "starred": f.starred, "faces": f.faces, "videos": f.videos, "geotagged": f.geotagged },
        "age": q.days,
    });
    match app.session_mut() {
        Some(s) => s.execute("search.set", &params),
        None => Err(CmdError::Disabled(OPENING.into())),
    }
}

/// Leaves search mode (the field's ×, Esc in the field, Show All): no text, filters or age, and
/// the engine's search (or Find Duplicates' groups) cleared. The scroll anchor saved when search
/// mode started comes back on the next frame ([`sync`]).
pub fn clear(app: &mut App) {
    let s = &mut app.state_mut().search;
    s.text.clear();
    s.highlight = None;
    s.filters = Filters::default();
    s.age = 0.0;
    if let Some(session) = app.session_mut() {
        let _ = session.execute("search.clear", &json!({}));
    }
}

/// What [`sync`] keeps between frames: the view state's query and the engine's generation as of
/// the last frame (`None` before the first), and whether search mode was on.
#[derive(Debug, Clone, Default)]
struct Synced {
    query: Option<Query>,
    generation: u64,
    active: bool,
}

fn synced_id() -> egui::Id {
    egui::Id::new("dalica-search-sync")
}

/// How often the window looks for the search's answer while one is on its way.
const POLL: Duration = Duration::from_millis(33);

/// Keeps the view state's search and the engine's in step (see the module docs), once a frame
/// before the sections are built; saves and restores the scroll anchor as search mode starts and
/// ends. No search work: comparing two queries, and at most one `search.set` that only starts the
/// engine's debounce.
pub fn sync(app: &mut App, ctx: &egui::Context) {
    let Some(engine) = app.session().map(dalica_engine::Session::search_state) else { return };
    let (generation, mode, pending) = (engine.generation, engine.mode, engine.pending());
    let theirs = Query { text: engine.text.clone(), filters: ui_filters(engine.filters), days: engine.age_days };
    let memo: Synced = ctx.data(|d| d.get_temp(synced_id())).unwrap_or_default();
    let ours = query_of(&app.state().search);
    let pull = match &memo.query {
        // The first frame: an engine searching already (a script before the window) is shown.
        None => ours == Query::default() && theirs != Query::default(),
        // The engine changed by a command since the last frame, and the view state did not.
        Some(last) => *last == ours && generation != memo.generation && ours != theirs,
    };
    let push_ours = !pull && memo.query.as_ref() != Some(&ours);
    if pull {
        let s = &mut app.state_mut().search;
        s.text = theirs.text.chars().take(MAX_DRAFT).collect();
        s.filters = theirs.filters;
        s.age = widgets::age_position(theirs.days);
        s.highlight = None;
    } else if push_ours {
        let _ = push(app);
    }
    let generation = app.session().map_or(generation, |s| s.search_state().generation);
    let state = app.state_mut();
    state.search.duplicates = mode == Some(SearchMode::Duplicates);
    let active = state.search.active();
    if active && !memo.active {
        if state.search.saved_scroll.is_none() {
            state.search.saved_scroll = Some(state.scroll);
        }
        state.scroll = ScrollAnchor::default();
    } else if !active
        && memo.active
        && let Some(anchor) = state.search.saved_scroll.take()
    {
        state.scroll = anchor;
    }
    let memo = Synced { query: Some(query_of(&state.search)), generation, active };
    ctx.data_mut(|d| d.insert_temp(synced_id(), memo));
    if pending {
        ctx.request_repaint_after(POLL);
    }
}

fn ui_filters(f: dalica_library::search::Filters) -> Filters {
    Filters { starred: f.starred, faces: f.faces, videos: f.videos, geotagged: f.geotagged }
}

/// The search album's name for the text typed: `Search results for "x"` (the library's), or
/// `Search results` when only filters or the age are set.
pub fn results_name(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() { "Search results".to_string() } else { format!("Search results for \"{text}\"") }
}

/// Whether Save Search Results… can open: in search mode (the engine's `enabled`), with a match
/// once the answer came.
fn can_save(app: &App, params: &Value) -> Result<(), String> {
    let session = app.session().ok_or_else(|| OPENING.to_string())?;
    let spec = session.spec(SAVE_COMMAND).ok_or_else(|| "Not available yet.".to_string())?;
    (spec.enabled)(session, params).map_err(|d| d.0)?;
    let engine = session.search_state();
    if !engine.pending() && engine.result.as_ref().is_some_and(|r| r.count() == 0) {
        return Err(NO_MATCHES.into());
    }
    Ok(())
}

/// Save Search Results…: Album Properties with the search album's name, dated by the newest match.
fn open_save(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    can_save(app, params).map_err(CmdError::Disabled)?;
    let draft = search_draft(app);
    album_properties::open_for_search(app, draft)
}

/// The draft Save Search Results… opens with.
fn search_draft(app: &App) -> AlbumDraft {
    let name = results_name(&app.state().search.text);
    let date = app.session().and_then(album_properties::newest_match).map(|d| d.to_string()).unwrap_or_default();
    AlbumDraft { name, date, ..AlbumDraft::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_focus_asks_the_toolbar_for_the_focus_and_takes_no_params() {
        let mut app = App::new(None);
        assert_eq!(app.run("search.focus", &json!({})).unwrap(), json!({}));
        assert!(app.state().focus_search);
        app.state_mut().focus_search = false;
        for bad in [json!({ "text": "x" }), json!([1]), json!("x")] {
            assert!(matches!(app.run("search.focus", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(!app.state().focus_search, "nothing changed");
    }

    #[test]
    fn a_tag_reads_as_one_search_word() {
        assert_eq!(tag_word("lisbon"), "tag:lisbon");
        assert_eq!(tag_word("  new   york "), "tag:new-york");
        assert_eq!(results_name(" tram "), "Search results for \"tram\"");
        assert_eq!(results_name(""), "Search results");
    }

    #[test]
    fn search_tag_refuses_what_it_cannot_search_for() {
        let mut app = App::new(None);
        assert!(matches!(app.run("search.tag", &json!({ "tag": "x" })), Err(CmdError::Disabled(_))), "no library");
        let (_dir, s, _) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        for bad in [json!({}), json!({ "tag": 3 }), json!({ "tag": "  " }), json!({ "tag": "x", "more": 1 }), json!([])] {
            assert!(matches!(app.run("search.tag", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        let long = "x".repeat(MAX_DRAFT);
        assert!(matches!(app.run("search.tag", &json!({ "tag": long })), Err(CmdError::Limit(_))));
        assert_eq!(app.state().search, SearchState::default(), "nothing changed");
        let v = app.run("search.tag", &json!({ "tag": "beach" })).unwrap();
        assert_eq!((v["text"].as_str(), v["active"].as_bool()), (Some("tag:beach"), Some(true)));
        assert_eq!(app.state().search.text, "tag:beach");
    }

    #[test]
    fn save_search_results_waits_for_a_search() {
        let (_dir, s, _) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(can_save(&app, &json!({})).is_err(), "not searching");
        assert!(matches!(open_save(&mut app, &json!({})), Err(CmdError::Disabled(_))));
        assert!(app.state().dialogs.is_empty());
        app.run("search.tag", &json!({ "tag": "beach" })).unwrap();
        assert_eq!(can_save(&app, &json!({})), Ok(()), "searching, the answer on its way");
        assert!(matches!(open_save(&mut app, &json!({ "name": "x" })), Err(CmdError::BadParams(_))));
        app.session_mut().unwrap().wait_search();
        assert_eq!(can_save(&app, &json!({})), Err(NO_MATCHES.to_string()), "nothing is tagged beach");
    }
}
