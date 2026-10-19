//! The Places pane (`organizing.md` §7), in the side pane, top to bottom:
//!
//! - the status line ("No places in the selection" / "9 places in the selection",
//!   `text:places.status`) for the places of the selection, or with nothing selected of the
//!   folder selected in the tree ([`markers`]);
//! - the map `map:places` (ui-kit's map widget on its offline layer; no tile is fetched here): a
//!   marker per place (`marker:<n>`, markers close on screen drawn as one with the sum of their
//!   counts), zoom + / − (`button:map.zoomIn`, `button:map.zoomOut`), the pin tool
//!   (`button:map.pin`: armed, a click on the map places the pin `map:places/pin`), and the bubble
//!   `map:places/bubble` ([`bubble`]). Photos dragged from the light table over the map offer to
//!   geotag them where they are let go (`crate::drag`, `places.drop`). A click on a marker opens
//!   its bubble, or zooms in two levels on a group of places until they part; a click on the map
//!   closes the bubble. The bubble of a marker drawn as a group of places counts, shows, selects
//!   and removes the location of every photo the group's badge counts ([`group_action`]). The view (`places.centre`, `places.zoom`) is the view state's;
//! - "Search for a place:" with its field `field:places.search` (the view state's
//!   `places.search`): Enter on typed coordinates places the pin there and moves the map to it
//!   (`places.pin {text}`, read without the network); other text goes to `map.search`, the place
//!   search of the geocoding service (M13.28).
//!
//! The pane's body registers as the region `region:places` (`ui.screenshot {region: "places"}`).
//! [`tiles`] and [`search`] are where the tiles and the search's results arrive (M13.28).

pub mod bubble;
pub mod markers;
pub mod search;
pub mod tiles;

use dalica_core::PhotoId;
use dalica_core::geo::{LatLon as Point, MAX_ZOOM};
use dalica_engine::CmdError;
use dalica_engine::cmd::places::CLEAR_ASKS_PAST;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Ending, MARKER_GROUP, Map, MapBubble, MapMarker, MapView, MarkerGroup, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Id, Rect, Ui, pos2, vec2};
use serde_json::{Map as Draft, json};

use crate::app::App;
use crate::dialogs::{self, DialogKind, geotag_confirm};
use crate::state::{Bubble, DropAt, MAP_LAT_MAX, MAX_DRAFT};

/// The status line with nothing geotagged.
pub const NONE: &str = "No places in the selection";
/// The map's name: it registers `map:places`.
pub const MAP: &str = "places";
pub const MAP_ID: &str = "map:places";
pub const REGION_ID: &str = "region:places";
pub const STATUS_ID: &str = "text:places.status";
pub const SEARCH_ID: &str = "field:places.search";
/// The search field's caption and hint.
pub const SEARCH_CAPTION: &str = "Search for a place:";
pub const SEARCH_HINT: &str = "Coordinates, such as 38.7, -9.14";
/// The pane's inset, the status line's and the search part's heights, and the gaps (points).
const INSET: f32 = 8.0;
const STATUS_H: f32 = 20.0;
const CAPTION_H: f32 = 18.0;
const GAP: f32 = 8.0;
/// How many levels a click on a group of places zooms in.
const GROUP_ZOOM: u8 = 2;

/// Where the pane's parts go in its body `rect`: the status line, the map, the search caption and
/// the field's row.
fn parts(t: &Tokens, rect: Rect) -> (Rect, Rect, Rect, Rect) {
    let inner = rect.shrink(INSET);
    let field_h = t.metrics.text_field_h;
    let status = Rect::from_min_size(inner.min, vec2(inner.width(), STATUS_H));
    let field = Rect::from_min_max(pos2(inner.min.x, inner.max.y - field_h), inner.max);
    let caption = Rect::from_min_max(pos2(inner.min.x, field.min.y - CAPTION_H), pos2(inner.max.x, field.min.y));
    let map = Rect::from_min_max(pos2(inner.min.x, status.max.y + GAP / 2.0), pos2(inner.max.x, (caption.min.y - GAP).max(status.max.y + GAP / 2.0)));
    (status, map, caption, field)
}

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let body = ui.max_rect();
    widgets::register(&ctx, REGION_ID, Role::Other, "Places", Target::Painted { rect: body, clip: ui.clip_rect() }, false);
    let (status, map_rect, caption, field) = parts(&t, body);
    let places = markers::cached(app, &ctx);

    let painter = ui.painter_at(body);
    let text = markers::status(places.len());
    widgets::label_in(
        &painter.with_clip_rect(status.intersect(ui.clip_rect())),
        status,
        Align2::LEFT_CENTER,
        &text,
        fonts::regular(t.body),
        t.text_label,
    );
    widgets::register(&ctx, STATUS_ID, Role::Other, &text, Target::Painted { rect: status, clip: ui.clip_rect() }, false);

    if map_rect.is_positive() {
        map_part(app, ui, map_rect, &places);
    }

    widgets::label_in(&painter, caption, Align2::LEFT_CENTER, SEARCH_CAPTION, fonts::regular(t.body), t.text_label);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)));
    search_field(app, &mut row, field.width());
    tiles::show(app, &mut row);
    search::show(app, &mut row);
}

/// "Search for a place:": the field, and Enter's search.
fn search_field(app: &mut App, ui: &mut Ui, width: f32) {
    let t = *app.tokens();
    let mut text = app.state().places.search.clone();
    let out = widgets::text_field(ui, &t, SEARCH_ID, &mut text, SEARCH_HINT, width);
    if text != app.state().places.search {
        if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
            text.truncate(at);
        }
        app.state_mut().places.search = text.clone();
    }
    if out.ending == Some(Ending::Return) && !text.trim().is_empty() {
        // A refusal's reason is on the status line.
        let _ = find(app, &text);
    }
}

/// Finds `text`: typed coordinates place the pin there (without the network); anything else is a
/// place's name for the geocoding service (`map.search`).
pub fn find(app: &mut App, text: &str) -> Result<serde_json::Value, dalica_engine::CmdError> {
    if crate::commands::places::typed(text).is_ok() {
        app.run("places.pin", &json!({ "text": text }))
    } else {
        app.run("map.search", &json!({ "text": text }))
    }
}

/// The view state's map view.
fn view_of(app: &App) -> MapView {
    let p = &app.state().places;
    let zoom = u8::try_from(p.zoom.round().clamp(0.0, f32::from(MAX_ZOOM)) as i64).unwrap_or(1);
    Point::new(p.centre[0], p.centre[1]).map_or_else(|_| MapView::default(), |c| MapView::new(c, zoom))
}

/// Keeps `view` in the view state.
fn keep_view(app: &mut App, view: &MapView) {
    if let Some(c) = view.center() {
        let places = &mut app.state_mut().places;
        places.centre = [c.lat().clamp(-MAP_LAT_MAX, MAP_LAT_MAX), c.lon().clamp(-180.0, 180.0)];
        places.zoom = f32::from(view.zoom());
    }
}

fn pin_tool_id() -> Id {
    Id::new("places.pinTool")
}

/// The map with its markers, the pin, the bubble and a drop of photos.
fn map_part(app: &mut App, ui: &mut Ui, rect: Rect, places: &[markers::Place]) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let view = view_of(app);
    let marks: Vec<MapMarker> = places.iter().map(|p| MapMarker { at: p.at, count: p.ids.len() }).collect();
    let state = &app.state().places;
    // A drop waiting for its bubble's answer shows where it went as the pin.
    let pin = match (state.bubble, &state.drop, &state.pin) {
        (Some(Bubble::Drop), Some(d), _) => Some(d.at),
        (_, _, Some(p)) => Some(p.at),
        _ => None,
    }
    .and_then(|[lat, lon]| Point::new(lat, lon).ok());
    // Grouped as the map is about to draw them, so a group's bubble speaks for all its places.
    let groups = widgets::group_markers(&view, rect, &marks, MARKER_GROUP);
    let shown = bubble::shown(app, places, &groups);
    let thumbs = match &shown {
        Some(b) => bubble::thumbs(app, &ctx, &b.ids),
        None => Vec::new(),
    };
    let armed = ctx.data(|d| d.get_temp::<bool>(pin_tool_id())).unwrap_or(false);
    let payload = app.state().drag.as_ref().is_some_and(|d| d.faces.is_empty() && !d.ids.is_empty());
    let layer = tiles::layer(app, &ctx, view.zoom());
    let mut m = Map::new(MAP, view);
    m.tiles = &layer.tiles;
    m.attribution = layer.attribution.as_deref();
    m.markers = &marks;
    m.pin = pin;
    m.pin_tool = armed;
    m.payload = payload;
    m.bubble = shown.as_ref().map(|b| MapBubble { anchor: b.anchor, thumbs: &thumbs, text: &b.text, buttons: &b.buttons });
    let out = widgets::map(ui, &t, rect, &m);
    tiles::after_map(app, ui, rect, &out, layer.attribution.as_deref());

    if out.changed {
        keep_view(app, &out.view);
    }
    if let (Some(at), Some(session)) = (out.drop_hover, app.session()) {
        // Millionths of a degree: both fit an `i32` (at most 180 000 000).
        let e6 = |v: f64| (v * 1e6).round() as i32;
        let target = DropAt::Map { lat_e6: e6(at.lat()), lon_e6: e6(at.lon()) };
        let _ = crate::drag::offer(&ctx, session, app.state(), target, MAP_ID);
    }
    if out.pin_tool_clicked {
        ctx.data_mut(|d| d.insert_temp(pin_tool_id(), !armed));
    }
    // A refusal's reason is on the status line.
    if let Some(at) = out.pin_placed {
        ctx.data_mut(|d| d.insert_temp(pin_tool_id(), false));
        let _ = app.run("places.pin", &json!({ "at": [at.lat(), at.lon()] }));
    }
    if out.pin_clicked && app.state().places.bubble != Some(Bubble::Drop) {
        let _ = app.run("places.bubble", &json!({ "pin": true }));
    }
    if let Some(members) = &out.marker_clicked {
        let group = out.groups.iter().find(|g| &g.members == members);
        match (members.first(), group) {
            (Some(_), Some(g)) if members.len() > 1 && out.view.zoom() < MAX_ZOOM => {
                let zoomed = out.view.zoomed(rect, g.at, out.view.zoom().saturating_add(GROUP_ZOOM).min(MAX_ZOOM));
                keep_view(app, &zoomed);
            }
            (Some(&first), _) => {
                let _ = app.run("places.bubble", &json!({ "marker": first }));
            }
            (None, _) => {}
        }
    }
    if out.clicked.is_some() && app.state().places.bubble.is_some() {
        crate::commands::places::close_bubble(app);
    }
    if let Some(b) = out.bubble_button.and_then(|i| shown.as_ref().and_then(|s| s.buttons.get(i))) {
        let ids = shown.as_ref().map(|s| s.ids.clone()).unwrap_or_default();
        // A refusal's reason is on the status line.
        if group_action(app, &groups, b.id, ids).is_some() {
            return;
        }
        let _ = match b.id {
            bubble::OK_ID => app.run("places.put", &json!({})),
            bubble::SHOW_ID => app.run("places.showPhotos", &json!({})),
            bubble::REMOVE_ID => app.run("places.removeLocation", &json!({})),
            _ => app.run("places.bubble", &json!({})),
        };
    }
}

/// A marker bubble's Show These Photos or Remove Location on the photos `ids` it shows, when the
/// marker whose bubble is open is drawn among `groups` as a group of several places: selects them
/// (closing the bubble), or removes their location as one undo step, asking first past
/// [`CLEAR_ASKS_PAST`] as for one place. `None` for any other bubble or button (the commands
/// `places.showPhotos` and `places.removeLocation` act on the one place).
fn group_action(app: &mut App, groups: &[MarkerGroup], button: &str, ids: Vec<PhotoId>) -> Option<Result<serde_json::Value, CmdError>> {
    let Some(Bubble::Marker(n)) = app.state().places.bubble else { return None };
    let n = usize::try_from(n).ok()?;
    groups.iter().find(|g| g.members.contains(&n)).filter(|g| g.members.len() > 1)?;
    match button {
        bubble::SHOW_ID => Some(app.run("select.set", &json!({ "ids": ids })).inspect(|_| crate::commands::places::close_bubble(app))),
        bubble::REMOVE_ID if ids.len() > CLEAR_ASKS_PAST => {
            Some(dialogs::open(app, geotag_confirm::dialog(DialogKind::ClearLocation, Draft::from_iter([("ids".into(), json!(ids))]))))
        }
        bubble::REMOVE_ID => Some(crate::commands::places::clear_location(app, ids, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_stack_inside_the_body() {
        let t = Tokens::default();
        let body = Rect::from_min_size(pos2(1000.0, 102.0), vec2(280.0, 600.0));
        let (status, map, caption, field) = parts(&t, body);
        assert!(status.max.y < map.min.y && map.max.y < caption.min.y && caption.max.y <= field.min.y);
        assert!(body.contains_rect(field) && field.max.y == body.max.y - INSET);
        // A body too short for the map gives it no height, never a negative one.
        let (_, map, _, _) = parts(&t, Rect::from_min_size(pos2(0.0, 0.0), vec2(280.0, 40.0)));
        assert!(map.height() >= 0.0);
    }
}
