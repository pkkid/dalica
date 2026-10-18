//! The Places pane's map (`organizing.md` §7): its zoom buttons and wheel (`map.zoomIn`,
//! `map.zoomOut`), a drag (`map.pan` to a centre), "Search for a place:" on Enter (`map.search`:
//! typed coordinates are found without the network, a name goes to the geocoding service) and the
//! pane's one-time question "Show map tiles from %provider%? …" [Allow] [Not Now]
//! (`map.allowTiles`, which sets the engine's Allow map tiles through `map.allow`, M13.20).
//!
//! M13.4's stubs: zooming and panning act on the view state (`places.centre`, `places.zoom`);
//! searching and allowing tiles check their parameters, then refuse with [`NOT_YET`] until M13.28
//! makes them act. No command here touches the network.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse};
use crate::app::App;
use crate::state::{LatLon, MAP_LAT_MAX, MAP_ZOOM_MAX, MAX_DRAFT, is_lat_lon};

/// Why searching and allowing tiles do not act yet.
pub const NOT_YET: &str = "Arrives with the map's tiles and place search.";

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
        params: "{text}: find a place (typed coordinates without the network; a name through the geocoding service)",
        run: search,
    },
    UiCommand {
        id: "map.allowTiles",
        label: "Allow Map Tiles",
        menu: "",
        keys: &[],
        params: "{on}: the Places pane's question about map tiles: Allow (true) or Not Now (false)",
        run: allow_tiles,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("map.search", not_yet), ("map.allowTiles", not_yet)];

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
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

fn search(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Search = parse(params)?;
    if p.text.chars().count() > MAX_DRAFT {
        return Err(CmdError::BadParams(format!("`text` is at most {MAX_DRAFT} characters")));
    }
    waits("places.search", json!({ "text": p.text }))
}

/// A stub: what it will run once M13.28 lands (the engine's `command` with `params`), refused with
/// [`NOT_YET`] for now.
fn waits(_command: &str, _params: Value) -> Result<Value, CmdError> {
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn allow_tiles(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Allow = parse(params)?;
    waits("map.allow", json!({ "on": p.on }))
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
        assert_eq!(app.run("map.search", &json!({ "text": "Lisbon" })), Err(CmdError::Disabled(NOT_YET.into())));
        assert_eq!(app.run("map.allowTiles", &json!({ "on": true })), Err(CmdError::Disabled(NOT_YET.into())));
    }
}
