//! The map's network half (`organizing.md` §7; the state is `crate::map_tiles` and
//! `crate::geocode`):
//!
//! - `map.allow {on}`: Allow map tiles (Options ▸ Network), which also allows place searches by
//!   name. Turning it off cancels the tiles and the search on their way. An effect outside Dalica:
//!   automation turns the network on only when started with `--automation-allow-external`.
//! - `places.search {text}`: typed coordinates (`38.7116, -9.1389`, `38°42′42″N 9°8′20″W`) are
//!   found here, without the network; any other text is a place name for the geocoding service,
//!   asked only while map tiles are allowed and the Places pane is open (`disabled` otherwise,
//!   nothing asked). The answer is `{coordinates, pending, search?, results}`: the places found at
//!   once (coordinates, or a name this session searched for before), or `pending: true` with the
//!   `search` id whose places `Session::take_place_search` hands over.

use dalica_core::geo::LatLon;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::geocode::{Place, Search};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "map.allow",
            label: "Allow Map Tiles",
            menu: "",
            keys: &[],
            params: r#"{"on":bool}: allow map tiles and place searches by name (Options ▸ Network ▸ Allow map tiles); off cancels what is on its way"#,
            effect: Effect::External,
            history: History::None,
            enabled: always,
            run: allow,
        },
        CommandSpec {
            id: "places.search",
            label: "Search for a Place",
            menu: "",
            keys: &[],
            params: r#"{"text":string}: typed coordinates (found without the network), else a place name for the geocoding service (only with map tiles allowed and the Places pane open); → {coordinates, pending, search?, results:[{name, place, lat, lon}]}"#,
            effect: Effect::External,
            history: History::None,
            enabled: always,
            run: search,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allow {
    on: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchParams {
    text: String,
}

fn allow(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Allow { on } = params(p)?;
    s.change_settings(|st| {
        let mut n = st.network();
        n.allow_map_tiles = on;
        st.set_network(&n);
    });
    if !on {
        // Nothing more goes out: the tiles on their way and the search are cancelled.
        let _ = s.take_map_tiles();
        s.cancel_place_search();
    }
    Ok(json!({ "on": on }))
}

fn results(places: &[Place]) -> Value {
    Value::Array(places.iter().map(Place::to_json).collect())
}

fn search(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SearchParams { text } = params(p)?;
    let text = text.trim();
    if text.is_empty() {
        return Err(CmdError::BadParams("text is empty".into()));
    }
    if let Ok(point) = LatLon::parse(text) {
        let place = Place { name: point.to_decimal(), place: point.to_dms(), point };
        return Ok(json!({ "coordinates": true, "pending": false, "results": results(&[place]) }));
    }
    match s.search_place(text)? {
        Search::Found(places) => Ok(json!({ "coordinates": false, "pending": false, "results": results(&places) })),
        Search::Pending(id) => Ok(json!({ "coordinates": false, "pending": true, "search": id, "results": [] })),
    }
}
