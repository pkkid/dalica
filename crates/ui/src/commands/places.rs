//! The Places pane and Tools ▸ Geotag (`organizing.md` §7, `menus-and-shortcuts.md` §1.7):
//!
//! - the pin tool (`places.pin`): the pin placed at a point (by hand with the tool armed, or typed
//!   coordinates: `{text}`, "Search for a place:" on Enter, read without the network by
//!   `core::geo::LatLon::parse`, the map moved there) or taken away; placing it opens its bubble
//!   "Put %d photos here?" for the selection, whose OK puts them there;
//! - a drop of photos on the map (`places.drop`, run by `crate::drag` on release over the map):
//!   the bubble "Put %d photos here?" ("Move %d photos here?" when some have a location) with a
//!   thumbnail stack, [OK] (`places.put`) [Cancel] (`places.bubble {}`);
//! - the bubble open (`places.bubble`): a marker's "%d photos here" with [Show These Photos]
//!   (`places.showPhotos`, which selects them) and [Remove Location] (`places.removeLocation`),
//!   or the pin's. Marker `n` is the pane's `n`-th place (`crate::panels::places::markers`); the
//!   two buttons' commands take every place the map draws in one marker with place `n`, as the
//!   bubble counts them (`crate::panels::places::groups`: the view state's centre and zoom, the
//!   rect the map was last drawn in), a marker of one place its own photos;
//! - Tools ▸ Geotag ▸ **Clear Location…** ([`CLEAR`], an [opener](super::Opener) of the engine's
//!   `geo.clear {ids, confirm}`: "Remove the location from %d photos?"), **Copy Location** and
//!   **Paste Location** (the engine's `geo.copy {id}` and `geo.paste {ids}`, M13.14; [`params_for`]
//!   gives Copy the focused photo, Paste the selection).
//!
//! Changing the location of more than 20 photos that have one, or removing it from more than 5,
//! asks first (`crate::dialogs::geotag_confirm`), as the engine does (`MOVE_ASKS_PAST`,
//! `CLEAR_ASKS_PAST`); fewer go at once. Each change is one library undo step (`photo.geo.set`,
//! `geo.clear`). Nothing here touches the network.

use dalica_core::PhotoId;
use dalica_core::geo::LatLon as Point;
use dalica_engine::CmdError;
use dalica_engine::cmd::places::{CLEAR_ASKS_PAST, MOVE_ASKS_PAST, NONE_LOCATED};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, Opener, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, DialogKind, geotag_confirm};
use crate::panels::places::markers::Place;
use crate::state::{Bubble, LatLon, MAP_LAT_MAX, MAX_DRAFT, MAX_IDS, PlacePin, PlacesDrop, is_lat_lon};

/// Nothing selected.
pub const SELECT_FIRST: &str = "Select some photos first.";
/// `places.put` with no drop or pin bubble open.
pub const NO_BUBBLE: &str = "Drop photos on the map or place the pin first.";
/// Typed text that is no coordinates.
pub const NOT_COORDINATES: &str = "Type coordinates, such as 38.7116, -9.1389 or 38°42'41.8\"N 9°8'20\"W.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "places.pin",
        label: "Place the Pin",
        menu: "",
        keys: &[],
        params: "{at?: [lat, lon] | null, text?}: put the pin tool's pin at a point (default: the map's centre) or at typed coordinates (`text`, \"38.7116, -9.1389\"; the map moves there), opening its bubble \"Put %d photos here?\" for the selection; null takes it away",
        run: pin,
    },
    UiCommand {
        id: "places.drop",
        label: "Drop Photos on the Map",
        menu: "",
        keys: &[],
        params: "{ids, at: [lat, lon]}: photos dropped on the map at a point: opens the bubble \"Put %d photos here?\" (\"Move %d photos here?\" when some have a location)",
        run: drop,
    },
    UiCommand {
        id: "places.bubble",
        label: "Open a Bubble",
        menu: "",
        keys: &[],
        params: "{marker?: n, pin?: true}: open marker n's bubble (\"%d photos here\") or the pin's; {} closes the bubble (and drops a drop waiting in it)",
        run: bubble,
    },
    UiCommand {
        id: "places.put",
        label: "Put Photos Here",
        menu: "",
        keys: &[],
        params: "{}: the drop or pin bubble's OK: geotag its photos there (the pin's: the selection), one undo step; changing the location of more than 20 photos that have one asks first",
        run: put,
    },
    UiCommand {
        id: "places.showPhotos",
        label: "Show These Photos",
        menu: "",
        keys: &[],
        params: "{marker?: n}: select the photos of marker n (default: the one whose bubble is open), with every place the map draws in one marker with it (the group its badge counts)",
        run: show_photos,
    },
    UiCommand {
        id: "places.removeLocation",
        label: "Remove Location",
        menu: "",
        keys: &[],
        params: "{marker?: n}: remove the location of marker n's photos (default: the one whose bubble is open), with every place the map draws in one marker with it (the group its badge counts), one undo step; asks first past 5",
        run: remove_location,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("places.put", put_check), ("places.showPhotos", marker_check), ("places.removeLocation", marker_check)];

/// Tools ▸ Geotag ▸ Clear Location…: asks "Remove the location from %d photos?" for the selection.
pub const CLEAR: Opener = Opener { id: "geo.clear", dialog: "clearLocation", keys: &[], open: open_clear, check: clear_check };

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
    #[serde(default)]
    text: Option<String>,
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

/// The point typed coordinates name, read without the network.
pub fn typed(text: &str) -> Result<LatLon, CmdError> {
    if text.chars().count() > MAX_DRAFT {
        return Err(CmdError::BadParams(format!("`text` is at most {MAX_DRAFT} characters")));
    }
    let p = Point::parse(text.trim()).map_err(|_| CmdError::BadParams(NOT_COORDINATES.into()))?;
    Ok([p.lat(), p.lon()])
}

fn pin(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Pin = parse_object(params)?;
    let places = &app.state().places;
    let (pin, centre) = match (p.at, p.text) {
        (Some(_), Some(_)) => return Err(CmdError::BadParams("give `at` or `text`, not both".into())),
        (None, Some(text)) => {
            let at = typed(&text)?;
            (Some(PlacePin { at, place: None }), Some([at[0].clamp(-MAP_LAT_MAX, MAP_LAT_MAX), at[1]]))
        }
        (Some(None), None) => (None, None),
        (Some(Some(at)), None) => (Some(PlacePin { at: point(at)?, place: None }), None),
        (None, None) => (Some(PlacePin { at: places.centre, place: None }), None),
    };
    let places = &mut app.state_mut().places;
    if pin.is_some() {
        places.bubble = Some(Bubble::Pin);
        places.drop = None;
    } else if places.bubble == Some(Bubble::Pin) {
        places.bubble = None;
    }
    places.pin = pin;
    if let Some(c) = centre {
        places.centre = c;
    }
    Ok(json!({ "pin": places.pin, "bubble": places.bubble }))
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
        (Some(n), None | Some(false)) => {
            place(app, Some(n))?;
            Some(Bubble::Marker(n))
        }
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

/// Closes the bubble and forgets a drop waiting in it.
pub fn close_bubble(app: &mut App) {
    let places = &mut app.state_mut().places;
    places.bubble = None;
    places.drop = None;
}

/// The photos and the point the open drop or pin bubble puts there, or why there are none.
pub fn putting(app: &App) -> Result<(Vec<PhotoId>, LatLon), String> {
    let places = &app.state().places;
    match (places.bubble, &places.drop, &places.pin) {
        (Some(Bubble::Drop), Some(d), _) => Ok((d.ids.clone(), d.at)),
        (Some(Bubble::Pin), _, Some(pin)) => {
            let ids = app.state().selection.ids.clone();
            if ids.is_empty() { Err(SELECT_FIRST.into()) } else { Ok((ids, pin.at)) }
        }
        _ => Err(NO_BUBBLE.into()),
    }
}

/// How many of `ids` have a location other than `at` (a move, which asks past 20); with no
/// library, none.
pub fn moving(app: &App, ids: &[PhotoId], at: LatLon) -> usize {
    let Some(lib) = app.session().map(dalica_engine::Session::library) else { return 0 };
    let target = Point::new(at[0], at[1]).ok().map(|p| p.round6());
    ids.iter().filter(|id| lib.is_geotagged(**id) && lib.geotag(**id) != target).count()
}

/// Whether some of `ids` have a location: the drop bubble then says "Move".
pub fn some_located(app: &App, ids: &[PhotoId]) -> bool {
    app.session().is_some_and(|s| ids.iter().any(|id| s.library().is_geotagged(*id)))
}

fn put_check(app: &App, _: &Value) -> Result<(), String> {
    putting(app).map(|_| ())
}

fn put(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let (ids, at) = putting(app).map_err(CmdError::Disabled)?;
    if moving(app, &ids, at) > MOVE_ASKS_PAST {
        let draft = Map::from_iter([("ids".into(), json!(ids)), ("at".into(), json!(at))]);
        return dialogs::open(app, geotag_confirm::dialog(DialogKind::ChangeLocation, draft));
    }
    set_location(app, ids, at, false)
}

/// Gives `ids` the point `at` (`photo.geo.set`, `confirm` as given), then closes the drop or pin
/// bubble, the pin going with its own.
pub fn set_location(app: &mut App, ids: Vec<PhotoId>, at: LatLon, confirm: bool) -> Result<Value, CmdError> {
    let reply = app.run("photo.geo.set", &json!({ "ids": ids, "geo": at, "confirm": confirm }))?;
    if app.state().places.bubble == Some(Bubble::Pin) {
        app.state_mut().places.pin = None;
    }
    if matches!(app.state().places.bubble, Some(Bubble::Pin | Bubble::Drop)) {
        close_bubble(app);
    }
    Ok(reply)
}

/// Marker `n` (default: the one whose bubble is open) as the map draws it: its place with every
/// other place of its group (`crate::panels::places::drawn`), or why there is none.
fn place(app: &App, n: Option<u32>) -> Result<Place, CmdError> {
    let n = match (n, app.state().places.bubble) {
        (Some(n), _) | (None, Some(Bubble::Marker(n))) => n,
        (None, _) => return Err(CmdError::Disabled("Open a marker's bubble first.".into())),
    };
    usize::try_from(n).ok().and_then(|i| crate::panels::places::drawn(app, i)).ok_or_else(|| CmdError::NotFound(format!("no marker {n} on the map")))
}

fn marker_check(app: &App, params: &Value) -> Result<(), String> {
    let p: Marker = parse_object(params).map_err(|e| e.to_string())?;
    place(app, p.marker).map(|_| ()).map_err(|e| e.to_string())
}

fn show_photos(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Marker = parse_object(params)?;
    let place = place(app, p.marker)?;
    let reply = app.run("select.set", &json!({ "ids": place.ids }))?;
    close_bubble(app);
    Ok(reply)
}

fn remove_location(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Marker = parse_object(params)?;
    let place = place(app, p.marker)?;
    if place.ids.len() > CLEAR_ASKS_PAST {
        return dialogs::open(app, geotag_confirm::dialog(DialogKind::ClearLocation, Map::from_iter([("ids".into(), json!(place.ids))])));
    }
    clear_location(app, place.ids, false)
}

/// Removes the location of `ids` (`geo.clear`, `confirm` as given), then closes a marker's bubble.
pub fn clear_location(app: &mut App, ids: Vec<PhotoId>, confirm: bool) -> Result<Value, CmdError> {
    let reply = app.run("geo.clear", &json!({ "ids": ids, "confirm": confirm }))?;
    if matches!(app.state().places.bubble, Some(Bubble::Marker(_))) {
        close_bubble(app);
    }
    Ok(reply)
}

/// Clear Location…'s photos: `{ids?}`, else the selection.
fn clear_ids(app: &App, params: &Value) -> Result<Vec<PhotoId>, CmdError> {
    let p: Ids = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => super::albums::known_photos(app, ids)?,
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled(SELECT_FIRST.into()));
    }
    if !some_located(app, &ids) {
        return Err(CmdError::Disabled(NONE_LOCATED.into()));
    }
    Ok(ids)
}

fn clear_check(app: &App, params: &Value) -> Result<(), String> {
    clear_ids(app, params).map(|_| ()).map_err(|e| match e {
        CmdError::Disabled(why) => why,
        other => other.to_string(),
    })
}

/// Clear Location…'s question for `{ids?}` (default: the selection), which always asks.
fn open_clear(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let ids = clear_ids(app, params)?;
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
        assert_eq!(app.state().places.bubble, Some(Bubble::Pin), "placing the pin opens its bubble");
        app.run("places.bubble", &json!({})).unwrap();
        app.run("places.bubble", &json!({ "pin": true })).unwrap();
        assert_eq!(app.state().places.bubble, Some(Bubble::Pin));
        app.run("places.pin", &json!({ "at": null })).unwrap();
        assert_eq!((app.state().places.pin.is_none(), app.state().places.bubble), (true, None), "the pin's bubble goes with it");
        assert_eq!(app.run("places.bubble", &json!({ "pin": true })), Err(CmdError::Disabled("Place the pin first.".into())));
        app.run("places.pin", &json!({})).unwrap();
        assert_eq!(app.state().places.pin.as_ref().map(|p| p.at), Some([0.0, 0.0]), "at the map's centre");
        // Typed coordinates: the pin and the map go there.
        app.run("places.pin", &json!({ "text": "41°9'N 8°37'W" })).unwrap();
        let at = app.state().places.pin.as_ref().unwrap().at;
        assert!((at[0] - 41.15).abs() < 1e-9 && (at[1] + 8.616_666).abs() < 1e-5, "{at:?}");
        assert_eq!(app.state().places.centre, at);
        assert_eq!(app.run("places.pin", &json!({ "text": "Lisbon" })), Err(CmdError::BadParams(NOT_COORDINATES.into())));
        app.run("places.drop", &json!({ "ids": [ids[0], ids[2]], "at": [51.5, -0.12] })).unwrap();
        assert_eq!(app.state().places.bubble, Some(Bubble::Drop));
        assert_eq!(app.state().places.drop.as_ref().map(|d| d.ids.clone()), Some(vec![ids[0], ids[2]]));
        app.run("places.bubble", &json!({})).unwrap();
        assert_eq!((app.state().places.bubble, app.state().places.drop.is_none()), (None, true));
        for (id, bad) in [
            ("places.pin", json!({ "at": [91, 0] })),
            ("places.pin", json!({ "at": [0] })),
            ("places.pin", json!({ "at": [0, 0], "text": "1, 2" })),
            ("places.drop", json!({ "ids": [], "at": [0, 0] })),
            ("places.drop", json!({ "ids": [ids[0]], "at": [0, 181] })),
            ("places.bubble", json!({ "marker": -1 })),
            ("places.bubble", json!({ "marker": 1, "pin": true })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        assert!(matches!(app.run("places.drop", &json!({ "ids": [999_999], "at": [0, 0] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("places.bubble", &json!({ "marker": 0 })), Err(CmdError::NotFound(_))), "no marker yet");
        assert_eq!(app.run("places.put", &json!({})), Err(CmdError::Disabled(NO_BUBBLE.into())));
        for id in ["places.showPhotos", "places.removeLocation"] {
            assert!(matches!(app.run(id, &json!({})), Err(CmdError::Disabled(_))), "{id}");
        }
    }

    #[test]
    fn put_geotags_the_drop_and_the_pin_puts_the_selection() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        app.run("places.drop", &json!({ "ids": [ids[0], ids[1]], "at": [38.7116, -9.1389] })).unwrap();
        assert!(!some_located(&app, &[ids[0], ids[1]]));
        app.run("places.put", &json!({})).unwrap();
        let lib = app.session().unwrap().library();
        assert_eq!(lib.geotag(ids[1]), Some(Point::new(38.7116, -9.1389).unwrap()));
        assert_eq!((app.state().places.bubble, app.state().places.drop.is_none()), (None, true));
        // The pin's OK: the selection.
        app.run("select.set", &json!({ "ids": [ids[2]] })).unwrap();
        app.run("places.pin", &json!({ "text": "41.1579, -8.6291" })).unwrap();
        app.run("places.put", &json!({})).unwrap();
        assert_eq!(app.session().unwrap().library().geotag(ids[2]), Some(Point::new(41.1579, -8.6291).unwrap()));
        assert!(app.state().places.pin.is_none(), "the pin goes once its photos are there");
        // Markers: Lisbon (2 photos) then Porto, in the selection's order, apart at zoom 7 (one
        // marker at the default zoom 1).
        app.run("select.set", &json!({ "ids": ids })).unwrap();
        app.run("places.showPhotos", &json!({ "marker": 1 })).unwrap();
        assert_eq!(app.state().selection.ids, ids, "grouped: every photo");
        app.run("select.set", &json!({ "ids": ids })).unwrap();
        app.run("map.pan", &json!({ "centre": [40.0, -9.0], "zoom": 7 })).unwrap();
        assert_eq!(moving(&app, &ids, [0.0, 0.0]), 3);
        app.run("places.showPhotos", &json!({ "marker": 0 })).unwrap();
        assert_eq!(app.state().selection.ids, vec![ids[0], ids[1]]);
        app.run("places.bubble", &json!({ "marker": 0 })).unwrap();
        app.run("places.removeLocation", &json!({})).unwrap();
        assert!(!some_located(&app, &[ids[0], ids[1]]));
        assert_eq!(app.state().places.bubble, None);
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
        assert_eq!(clear_check(&app, &json!({})), Err(NONE_LOCATED.into()));
        app.run("photo.geo.set", &json!({ "ids": [ids[1]], "geo": [1, 2] })).unwrap();
        assert_eq!(clear_check(&app, &json!({})), Ok(()));
        assert_eq!(open_clear(&mut app, &json!({})).unwrap(), json!({ "dialog": "clearLocation" }));
        assert_eq!(geotag_confirm::question(dialogs::top(&app).unwrap()), "Remove the location from 2 photos?");
    }
}
