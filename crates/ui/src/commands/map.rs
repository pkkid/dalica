//! The Places pane's map (`organizing.md` §7): its zoom buttons and wheel (`map.zoomIn`,
//! `map.zoomOut`), a drag (`map.pan` to a centre), "Search for a place:" on Enter (`map.search`),
//! a place found picked from its results (`map.showResult`) and the pane's one-time question
//! "Show map tiles from %provider%? …" [Allow] [Not Now] (`map.allowTiles`).
//!
//! - **`map.search {text}`**: typed coordinates place the pin there and move the map to it, without
//!   the network (`places.pin {text}`); any other text is a place name for the geocoding service
//!   (the engine's `places.search`, M13.20), asked only while map tiles are allowed and the Places
//!   pane is open. The places found go to the view state's `places.results` (at once when this
//!   session searched for the name before, else when the search arrives:
//!   [`crate::panels::places::search`] takes it each frame).
//! - **`map.showResult {n}`**: a click on result `n` puts the pin there, named after the place,
//!   opens its bubble ("Put %d photos here?") and moves the map to it; the list closes.
//! - **`map.allowTiles {on}`**: Allow (`true`) turns Allow map tiles on (the engine's `map.allow`);
//!   Not Now (`false`) leaves the offline map (and turns tiles off if they were on). Either answer
//!   is remembered (`settings.json` → `network.tilesAsked`), so the pane asks once; from then on
//!   the switch is Options ▸ Network's.
//!
//! **Privacy** (`security.md` §8.2, `control-protocol.md` §5.6): a search by name and Allow reach
//! outside Dalica, so while a control request drives the app they are refused unless it was started
//! with `--automation-allow-external` ([`crate::control::automated`]); the person's own clicks and
//! keys are never refused. Zooming, panning and picking a result never touch the network.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse};
use crate::app::App;
use crate::state::{Bubble, LatLon, MAP_LAT_MAX, MAP_ZOOM_MAX, MAX_DRAFT, MAX_PLACE_RESULTS, PlacePin, PlaceResult, is_lat_lon};

/// Where the pane's answer to its question is kept (`settings.json`).
pub const SETTINGS_KEY: &str = "network";
pub const ASKED_KEY: &str = "tilesAsked";
/// The zoom a picked result shows at least (a town fills the pane).
pub const RESULT_ZOOM: f32 = 12.0;
/// Why a command that reaches outside Dalica is refused under automation.
pub const EXTERNAL: &str = "This reaches outside Dalica; start the app with --automation-allow-external to allow it.";
/// With no library open.
const NO_LIBRARY: &str = "The library is still opening.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand { id: "map.zoomIn", label: "Zoom In", menu: "", keys: &[], params: "{}: zoom the Places pane's map in one level", run: zoom_in },
    UiCommand { id: "map.zoomOut", label: "Zoom Out", menu: "", keys: &[], params: "{}: zoom the Places pane's map out one level", run: zoom_out },
    UiCommand {
        id: "map.pan",
        label: "Move the Map",
        menu: "",
        keys: &[],
        params: "{centre: [lat, lon], zoom?}: centre the Places pane's map there (latitude within ±85.05), at a zoom of 0 … 19",
        run: pan,
    },
    UiCommand {
        id: "map.search",
        label: "Search for a Place",
        menu: "",
        keys: &[],
        params: "{text}: typed coordinates place the pin there (without the network); a name is looked up with the geocoding service (only with map tiles allowed and the Places pane open), its places listed in `places.results`",
        run: search,
    },
    UiCommand {
        id: "map.showResult",
        label: "Show a Place Found",
        menu: "",
        keys: &[],
        params: "{n}: put the pin at the n-th place of `places.results` (from 0), named after it, open its bubble and move the map there",
        run: show_result,
    },
    UiCommand {
        id: "map.allowTiles",
        label: "Allow Map Tiles",
        menu: "",
        keys: &[],
        params: "{on}: the Places pane's question about map tiles: Allow (true: Allow map tiles on) or Not Now (false: the offline map); either answer is remembered",
        run: allow_tiles,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("map.search", library), ("map.allowTiles", library), ("map.showResult", has_results)];

fn library(app: &App, _: &Value) -> Result<(), String> {
    app.session().map(|_| ()).ok_or_else(|| NO_LIBRARY.into())
}

fn has_results(app: &App, _: &Value) -> Result<(), String> {
    if app.state().places.results.is_empty() { Err("Search for a place by name first.".into()) } else { Ok(()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pan {
    centre: LatLon,
    #[serde(default)]
    zoom: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allow {
    on: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    n: usize,
}

/// Zooms by `by` levels, kept within 0 … 19.
fn zoom_by(app: &mut App, by: f32) -> Value {
    let places = &mut app.state_mut().places;
    places.zoom = (places.zoom + by).clamp(0.0, MAP_ZOOM_MAX);
    json!({ "zoom": places.zoom })
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Ok(zoom_by(app, 1.0))
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Ok(zoom_by(app, -1.0))
}

fn pan(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Pan = parse(params)?;
    if !(is_lat_lon(p.centre) && p.centre[0].abs() <= MAP_LAT_MAX) {
        return Err(CmdError::BadParams(format!("`centre` is [latitude −{MAP_LAT_MAX} … {MAP_LAT_MAX}, longitude −180 … 180]")));
    }
    if p.zoom.is_some_and(|z| !(z.is_finite() && (0.0..=MAP_ZOOM_MAX).contains(&z))) {
        return Err(CmdError::BadParams(format!("`zoom` is 0 … {MAP_ZOOM_MAX}")));
    }
    let places = &mut app.state_mut().places;
    places.centre = p.centre;
    if let Some(z) = p.zoom {
        places.zoom = z;
    }
    Ok(json!({ "centre": places.centre, "zoom": places.zoom }))
}

/// Refuses an effect outside Dalica while a control request without `--automation-allow-external`
/// drives the app.
fn external_allowed() -> Result<(), CmdError> {
    match crate::control::automated() {
        Some(config) if !config.allow_external => Err(CmdError::Disabled(EXTERNAL.into())),
        _ => Ok(()),
    }
}

fn search(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Search = parse(params)?;
    if p.text.chars().count() > MAX_DRAFT {
        return Err(CmdError::BadParams(format!("`text` is at most {MAX_DRAFT} characters")));
    }
    let text = p.text.trim();
    if text.is_empty() {
        return Err(CmdError::BadParams("`text` is empty".into()));
    }
    if crate::commands::places::typed(text).is_ok() {
        app.state_mut().places.results.clear();
        return app.run("places.pin", &json!({ "text": text }));
    }
    if app.session().is_none() {
        return Err(CmdError::Disabled(NO_LIBRARY.into()));
    }
    external_allowed()?;
    let reply = app.run("places.search", &json!({ "text": text }))?;
    let pending = reply.get("pending").and_then(Value::as_bool).unwrap_or(false);
    let found = if pending { Vec::new() } else { results_of(reply.get("results").unwrap_or(&Value::Null)) };
    app.state_mut().places.results = found;
    Ok(reply)
}

/// The places of an engine reply (`[{name, place, lat, lon}]`) as the view state lists them: by
/// their full place ("Lisbon, Portugal"), else their name; at most [`MAX_PLACE_RESULTS`], each
/// on the globe and its text within [`MAX_DRAFT`] characters (anything else is passed over).
pub fn results_of(v: &Value) -> Vec<PlaceResult> {
    let Some(list) = v.as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|r| {
            let text = |k: &str| r.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
            let name = text("place").or_else(|| text("name"))?;
            let at = [r.get("lat")?.as_f64()?, r.get("lon")?.as_f64()?];
            let name: String = name.chars().take(MAX_DRAFT).collect();
            is_lat_lon(at).then_some(PlaceResult { name, at })
        })
        .take(MAX_PLACE_RESULTS)
        .collect()
}

fn show_result(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let Pick { n } = parse(params)?;
    let places = &app.state().places;
    let Some(r) = places.results.get(n).cloned() else {
        return Err(CmdError::NotFound(format!("no place {n} among the {} found", places.results.len())));
    };
    let places = &mut app.state_mut().places;
    places.pin = Some(PlacePin { at: r.at, place: Some(r.name.clone()) });
    places.bubble = Some(Bubble::Pin);
    places.drop = None;
    places.centre = [r.at[0].clamp(-MAP_LAT_MAX, MAP_LAT_MAX), r.at[1]];
    places.zoom = places.zoom.max(RESULT_ZOOM);
    places.results.clear();
    Ok(json!({ "pin": places.pin, "bubble": places.bubble, "centre": places.centre, "zoom": places.zoom }))
}

/// Whether the pane's question was answered (or tiles were ever allowed).
pub fn tiles_asked(app: &App) -> bool {
    app.session().and_then(|s| s.settings().get(SETTINGS_KEY)).and_then(|n| n.get(ASKED_KEY)).and_then(Value::as_bool).unwrap_or(false)
}

/// Remembers that the question was answered (one settings write, the first time only).
pub fn mark_asked(app: &mut App) {
    if tiles_asked(app) {
        return;
    }
    if let Some(s) = app.session_mut() {
        s.change_settings(|st| st.merge(SETTINGS_KEY, &[(ASKED_KEY, Some(json!(true)))]));
    }
}

/// Whether the pane asks its question now: map tiles are off and it was never answered.
pub fn asks(app: &App) -> bool {
    app.session().is_some_and(|s| !s.map_tiles_allowed()) && !tiles_asked(app)
}

fn allow_tiles(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let Allow { on } = parse(params)?;
    if app.session().is_none() {
        return Err(CmdError::Disabled(NO_LIBRARY.into()));
    }
    if on {
        external_allowed()?;
    }
    let reply = app.run("map.allow", &json!({ "on": on }))?;
    mark_asked(app);
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_zooms_and_pans_within_its_ranges() {
        let mut app = App::new(None);
        assert_eq!(app.run("map.zoomIn", &json!({})).unwrap(), json!({ "zoom": 2.0 }));
        for _ in 0..30 {
            app.run("map.zoomIn", &json!({})).unwrap();
        }
        assert_eq!(app.state().places.zoom, MAP_ZOOM_MAX);
        for _ in 0..30 {
            app.run("map.zoomOut", &Value::Null).unwrap();
        }
        assert_eq!(app.state().places.zoom, 0.0);
        app.run("map.pan", &json!({ "centre": [38.7, -9.1], "zoom": 12 })).unwrap();
        assert_eq!((app.state().places.centre, app.state().places.zoom), ([38.7, -9.1], 12.0));
        for bad in [json!({ "centre": [86, 0] }), json!({ "centre": [0, 0], "zoom": 20 }), json!({ "centre": [0, 0, 0] })] {
            assert!(matches!(app.run("map.pan", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }

    #[test]
    fn searching_and_allowing_need_a_library_and_typed_coordinates_need_no_network() {
        let mut app = App::new(None);
        assert_eq!(app.run("map.search", &json!({ "text": "Lisbon" })), Err(CmdError::Disabled(NO_LIBRARY.into())));
        assert_eq!(app.run("map.allowTiles", &json!({ "on": true })), Err(CmdError::Disabled(NO_LIBRARY.into())));
        assert!(matches!(app.run("map.search", &json!({ "text": "  " })), Err(CmdError::BadParams(_))));
        let pinned = app.run("map.search", &json!({ "text": "38.7116, -9.1389" })).unwrap();
        assert_eq!(pinned["pin"]["at"], json!([38.7116, -9.1389]));
        assert!(!asks(&app), "no library: no question");
    }

    #[test]
    fn a_result_picked_moves_the_pin_and_the_map_and_closes_the_list() {
        let mut app = App::new(None);
        let replied = json!([
            { "name": "Lisbon", "place": "Lisbon, Portugal", "lat": 38.7077, "lon": -9.1366 },
            { "name": "Nowhere", "place": "", "lat": 95.0, "lon": 0.0 },
            { "name": "Lisbon", "place": "", "lat": 44.4, "lon": -71.9 },
            { "place": "No point" },
        ]);
        let found = results_of(&replied);
        assert_eq!(
            found,
            [PlaceResult { name: "Lisbon, Portugal".into(), at: [38.7077, -9.1366] }, PlaceResult { name: "Lisbon".into(), at: [44.4, -71.9] }]
        );
        assert!(results_of(&json!({ "not": "a list" })).is_empty());
        assert!(matches!(app.run("map.showResult", &json!({ "n": 0 })), Err(CmdError::NotFound(_))));
        app.state_mut().places.results = found;
        app.run("map.showResult", &json!({ "n": 1 })).unwrap();
        let p = &app.state().places;
        assert_eq!(p.pin, Some(PlacePin { at: [44.4, -71.9], place: Some("Lisbon".into()) }));
        assert_eq!((p.bubble, p.centre, p.zoom, p.results.is_empty()), (Some(Bubble::Pin), [44.4, -71.9], RESULT_ZOOM, true));
    }
}
