//! The Places pane and Tools ▸ Geotag (`organizing.md` §7, `menus-and-shortcuts.md` §1.7):
//!
//! - the pin tool (`places.pin`): the pin placed at a point (by hand, typed coordinates, a search
//!   result) or taken away; its bubble's OK puts the selection there;
//! - a drop of photos on the map (`places.drop`, run by `crate::drag` on release over the map):
//!   the bubble "Put %d photos here?" ("Move %d photos here?" when some are tagged) with a
//!   thumbnail stack, [OK] (`places.put`) [Cancel];
//! - the bubble open (`places.bubble`): a marker's "%d photos here" with [Show These Photos]
//!   (`places.showPhotos`) and [Remove Location] (`places.removeLocation`), or the pin's;
//! - Tools ▸ Geotag ▸ **Clear Location…** ([`CLEAR`], an [opener](super::Opener) of the engine's
//!   `geo.clear {ids, confirm}`: "Remove the location from %d photos?"), **Copy Location** and
//!   **Paste Location** (the engine's `geo.copy {id}` and `geo.paste {ids}`, M13.14; [`params_for`]
//!   gives Copy the focused photo, Paste the selection).
//!
//! Moving more than 20 tagged photos or removing the location of more than 5 asks first
//! (`crate::dialogs::geotag_confirm`). Each change is one library undo step (`photo.geo.set`).
//!
//! M13.4's stubs: the ids, parameters and dialogs are final; the pin, a drop and the bubble act on
//! the view state (`places`); putting, showing and removing check their parameters, then refuse
//! with [`NOT_YET`], and Clear Location… is greyed with it, until M13.22 makes them act.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, Opener, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, DialogKind, geotag_confirm};
use crate::state::{Bubble, LatLon, MAX_IDS, PlacePin, PlacesDrop, is_lat_lon};

/// Why the geotag commands do not act yet.
pub const NOT_YET: &str = "Arrives with geotags.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "places.pin",
        label: "Place the Pin",
        menu: "",
        keys: &[],
        params: "{at?: [lat, lon] | null}: put the pin tool's pin at a point (default: the map's centre); null takes it away",
        run: pin,
    },
    UiCommand {
        id: "places.drop",
        label: "Drop Photos on the Map",
        menu: "",
        keys: &[],
        params: "{ids, at: [lat, lon]}: photos dropped on the map at a point: opens the bubble \"Put %d photos here?\"",
        run: drop,
    },
    UiCommand {
        id: "places.bubble",
        label: "Open a Bubble",
        menu: "",
        keys: &[],
        params: "{marker?: n, pin?: true}: open marker n's bubble or the pin's; {} closes the bubble (and drops a drop waiting in it)",
        run: bubble,
    },
    UiCommand {
        id: "places.put",
        label: "Put Photos Here",
        menu: "",
        keys: &[],
        params: "{}: the drop or pin bubble's OK: geotag its photos there; one undo step",
        run: put,
    },
    UiCommand {
        id: "places.showPhotos",
        label: "Show These Photos",
        menu: "",
        keys: &[],
        params: "{marker?: n}: select the photos of the marker (default: the one whose bubble is open)",
        run: show_photos,
    },
    UiCommand {
        id: "places.removeLocation",
        label: "Remove Location",
        menu: "",
        keys: &[],
        params: "{marker?: n}: remove the location of the marker's photos (default: the one whose bubble is open); asks past 5",
        run: remove_location,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("places.put", not_yet), ("places.showPhotos", not_yet), ("places.removeLocation", not_yet)];

/// Tools ▸ Geotag ▸ Clear Location…: asks "Remove the location from %d photos?" for the selection.
pub const CLEAR: Opener = Opener { id: "geo.clear", dialog: "clearLocation", keys: &[], open: open_clear, check: not_yet };

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

/// The parameters a click or a shortcut passes to `id`, for Tools ▸ Geotag's engine commands: Copy
/// Location the focused photo (else the first selected) as `id`, Paste Location and Clear
/// Location… the selection as `ids`. `None` for every other command.
pub fn params_for(app: &App, id: &str) -> Option<Value> {
    let selection = &app.state().selection;
    match id {
        "geo.copy" => Some(selection.anchor.or_else(|| selection.ids.first().copied()).map_or_else(|| json!({}), |p| json!({ "id": p }))),
        "geo.paste" | "geo.clear" => Some(json!({ "ids": selection.ids })),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    #[serde(default, deserialize_with = "present")]
    at: Option<Option<LatLon>>,
}

/// A field that may be given as `null`: `Some(None)` then, `None` when left out.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<LatLon>>, D::Error> {
    Option::<LatLon>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Drop {
    ids: Vec<PhotoId>,
    at: LatLon,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BubbleParams {
    #[serde(default)]
    marker: Option<u32>,
    #[serde(default)]
    pin: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    #[serde(default)]
    marker: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
}

/// `p`, a point on the globe, or why not.
fn point(p: LatLon) -> Result<LatLon, CmdError> {
    if is_lat_lon(p) { Ok(p) } else { Err(CmdError::BadParams(format!("`at` is [latitude −90 … 90, longitude −180 … 180], not {p:?}"))) }
}

fn pin(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Pin = parse_object(params)?;
    let places = &app.state().places;
    let pin = match p.at {
        Some(None) => None,
        Some(Some(at)) => Some(PlacePin { at: point(at)?, place: None }),
        None => Some(PlacePin { at: places.centre, place: None }),
    };
    let places = &mut app.state_mut().places;
    if pin.is_none() && places.bubble == Some(Bubble::Pin) {
        places.bubble = None;
    }
    places.pin = pin;
    Ok(json!({ "pin": places.pin }))
}

fn drop(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let mut p: Drop = parse(params)?;
    let at = point(p.at)?;
    if p.ids.is_empty() || p.ids.len() > MAX_IDS {
        return Err(CmdError::BadParams(format!("`ids` holds 1 … {MAX_IDS} photos")));
    }
    p.ids = super::albums::known_photos(app, p.ids)?;
    let places = &mut app.state_mut().places;
    places.drop = Some(PlacesDrop { ids: p.ids, at });
    places.bubble = Some(Bubble::Drop);
    Ok(json!({ "bubble": places.bubble }))
}

fn bubble(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: BubbleParams = parse_object(params)?;
    let places = &app.state().places;
    let bubble = match (p.marker, p.pin) {
        (None, None | Some(false)) => None,
        (Some(n), None | Some(false)) => Some(Bubble::Marker(n)),
        (None, Some(true)) if places.pin.is_some() => Some(Bubble::Pin),
        (None, Some(true)) => return Err(CmdError::Disabled("Place the pin first.".into())),
        (Some(_), Some(true)) => return Err(CmdError::BadParams("give `marker` or `pin`, not both".into())),
    };
    let places = &mut app.state_mut().places;
    places.bubble = bubble;
    if bubble != Some(Bubble::Drop) {
        places.drop = None;
    }
    Ok(json!({ "bubble": bubble }))
}

fn put(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn show_photos(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Marker = parse_object(params)?;
    waits("select.set", json!({ "marker": p.marker }))
}

fn remove_location(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Marker = parse_object(params)?;
    waits("geo.clear", json!({ "marker": p.marker }))
}

/// A stub: what it will run once M13.22 lands (`command` with `params`), refused with
/// [`NOT_YET`] for now.
fn waits(_command: &str, _params: Value) -> Result<Value, CmdError> {
    Err(CmdError::Disabled(NOT_YET.into()))
}

/// Clear Location…'s question for `{ids?}` (default: the selection).
fn open_clear(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Ids = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => super::albums::known_photos(app, ids)?,
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    dialogs::open(app, geotag_confirm::dialog(DialogKind::ClearLocation, Map::from_iter([("ids".into(), json!(ids))])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_pin_a_drop_and_the_bubble_act_on_the_view_state() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        app.run("places.pin", &json!({ "at": [38.7116, -9.1389] })).unwrap();
        assert_eq!(app.state().places.pin.as_ref().map(|p| p.at), Some([38.7116, -9.1389]));
        app.run("places.bubble", &json!({ "pin": true })).unwrap();
        assert_eq!(app.state().places.bubble, Some(Bubble::Pin));
        app.run("places.pin", &json!({ "at": null })).unwrap();
        assert_eq!((app.state().places.pin.is_none(), app.state().places.bubble), (true, None), "the pin's bubble goes with it");
        assert_eq!(app.run("places.bubble", &json!({ "pin": true })), Err(CmdError::Disabled("Place the pin first.".into())));
        app.run("places.pin", &json!({})).unwrap();
        assert_eq!(app.state().places.pin.as_ref().map(|p| p.at), Some([0.0, 0.0]), "at the map's centre");
        app.run("places.drop", &json!({ "ids": [ids[0], ids[2]], "at": [51.5, -0.12] })).unwrap();
        assert_eq!(app.state().places.bubble, Some(Bubble::Drop));
        assert_eq!(app.state().places.drop.as_ref().map(|d| d.ids.clone()), Some(vec![ids[0], ids[2]]));
        app.run("places.bubble", &json!({ "marker": 3 })).unwrap();
        assert_eq!((app.state().places.bubble, app.state().places.drop.is_none()), (Some(Bubble::Marker(3)), true));
        app.run("places.bubble", &json!({})).unwrap();
        assert_eq!(app.state().places.bubble, None);
        for (id, bad) in [
            ("places.pin", json!({ "at": [91, 0] })),
            ("places.pin", json!({ "at": [0] })),
            ("places.drop", json!({ "ids": [], "at": [0, 0] })),
            ("places.drop", json!({ "ids": [ids[0]], "at": [0, 181] })),
            ("places.bubble", json!({ "marker": -1 })),
            ("places.bubble", json!({ "marker": 1, "pin": true })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        assert!(matches!(app.run("places.drop", &json!({ "ids": [999_999], "at": [0, 0] })), Err(CmdError::NotFound(_))));
        for id in ["places.put", "places.showPhotos", "places.removeLocation"] {
            assert_eq!(app.run(id, &json!({})), Err(CmdError::Disabled(NOT_YET.into())), "{id}");
        }
    }

    #[test]
    fn geotag_menu_rows_get_the_selection() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert_eq!(params_for(&app, "geo.copy"), Some(json!({})));
        app.run("select.set", &json!({ "ids": [ids[1], ids[2]] })).unwrap();
        let anchor = app.state().selection.anchor.unwrap();
        assert_eq!(params_for(&app, "geo.copy"), Some(json!({ "id": anchor })));
        assert_eq!(params_for(&app, "geo.paste"), Some(json!({ "ids": [ids[1], ids[2]] })));
        assert_eq!(params_for(&app, "photo.star.toggle"), None);
        assert_eq!(open_clear(&mut app, &json!({})).unwrap(), json!({ "dialog": "clearLocation" }));
        assert_eq!(geotag_confirm::question(dialogs::top(&app).unwrap()), "Remove the location from 2 photos?");
    }
}
