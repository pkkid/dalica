//! The places the Places pane's map shows (`organizing.md` §7): the geotags of the photos of the
//! selection, or, with nothing selected, of the folder selected in the tree. Photos at the same
//! point (six decimals, a geotag's precision) are one place, one marker `marker:<n>`: `n` is the
//! place's number in [`places`], in the order its first photo comes in the selection (or the
//! folder). The map draws markers close on screen as one; a group goes by its first place's number,
//! and its bubble speaks for every place in it ([`grouped`]).
//!
//! Worked out from the library's model alone (a lookup per photo, never the files); the pane keeps
//! the result for a library revision and a selection ([`cached`]), so a frame costs nothing more
//! while neither changes.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_core::geo::LatLon;
use dalica_library::Library;
use dalica_ui_kit::widgets::MarkerGroup;
use egui::Id;

use crate::app::App;
use crate::{tl, tln};

/// A place on the map: the point and its photos, in the order they come.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub at: LatLon,
    pub ids: Vec<PhotoId>,
}

/// The photos the pane shows: the selection, else the photos of the folder selected in the tree.
pub fn photos(app: &App) -> Vec<PhotoId> {
    let state = app.state();
    if !state.selection.ids.is_empty() {
        return state.selection.ids.clone();
    }
    match (app.session(), state.folder) {
        (Some(s), Some(folder)) => s.library().photos_in(folder).into_iter().map(|p| p.id).collect(),
        _ => Vec::new(),
    }
}

/// A point in whole micro-degrees, the key two photos at one place share.
fn micro(p: &LatLon) -> (i64, i64) {
    // Both values are on the globe (at most 180 by their type), so the products fit an `i64`.
    let m = |v: f64| (v * 1e6).round() as i64;
    (m(p.lat()), m(p.lon()))
}

/// The places of `ids` in `library`: each geotagged photo joins the place at its point, the places
/// in the order of their first photos. Photos without a location are left out.
pub fn places_of(library: &Library, ids: &[PhotoId]) -> Vec<Place> {
    let mut places: Vec<Place> = Vec::new();
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    for (id, at) in library.geotags_of(ids) {
        let key = micro(&at);
        match index.get(&key).and_then(|&i| places.get_mut(i)) {
            Some(place) => place.ids.push(id),
            None => {
                index.insert(key, places.len());
                places.push(Place { at: at.round6(), ids: vec![id] });
            }
        }
    }
    places
}

/// The places the pane shows now ([`photos`] in [`places_of`]); none before a library is open.
pub fn places(app: &App) -> Vec<Place> {
    match app.session() {
        Some(s) => places_of(s.library(), &photos(app)),
        None => Vec::new(),
    }
}

/// Marker `n` as the map draws it among `groups`: the place `n` merged with the other places of
/// its group, their photos in the group's order (the group's first place's point); `n` alone when
/// no group holds it. `None` for a marker not on the map.
pub fn grouped(places: &[Place], groups: &[MarkerGroup], n: usize) -> Option<Place> {
    let own = places.get(n)?;
    let Some(group) = groups.iter().find(|g| g.members.contains(&n)) else { return Some(own.clone()) };
    let members: Vec<&Place> = group.members.iter().filter_map(|&i| places.get(i)).collect();
    let at = members.first().map_or(own.at, |p| p.at);
    Some(Place { at, ids: members.iter().flat_map(|p| p.ids.iter().copied()).collect() })
}

/// The places of [`places`], worked out again only when the library's revision, the selection or
/// the tree's folder changed since the last frame (kept in `ctx`'s memory).
pub fn cached(app: &App, ctx: &egui::Context) -> Arc<Vec<Place>> {
    let state = app.state();
    let mut h = DefaultHasher::new();
    app.session().map(|s| s.library().revision()).hash(&mut h);
    state.selection.ids.hash(&mut h);
    state.folder.hash(&mut h);
    let key = h.finish();
    let id = Id::new("places.markers");
    if let Some((k, kept)) = ctx.data(|d| d.get_temp::<(u64, Arc<Vec<Place>>)>(id))
        && k == key
    {
        return kept;
    }
    let made = Arc::new(places(app));
    ctx.data_mut(|d| d.insert_temp(id, (key, Arc::clone(&made))));
    made
}

/// The pane's status line: "No places in the selection", "1 place in the selection", "9 places in
/// the selection".
pub fn status(places: usize) -> String {
    match places {
        0 => tl!("No places in the selection"),
        n => tln!(n, "{n} place in the selection", "{n} places in the selection"),
    }
}

/// "1 photo", "3 photos".
pub fn photos_text(n: usize) -> String {
    tln!(n, "{n} photo", "{n} photos")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn photos_at_one_point_are_one_place_in_the_order_they_come() {
        let (_dir, s, ids) = session_with_photos(4);
        let mut app = App::new(Some(s));
        assert!(places(&app).is_empty(), "nothing selected, no folder");
        let n: Vec<u64> = ids.iter().map(|i| i.get()).collect();
        app.run("photo.geo.set", &json!({ "ids": [n[2], n[0]], "geo": [38.7116, -9.1389] })).unwrap();
        app.run("photo.geo.set", &json!({ "ids": [n[1]], "geo": [41.1579, -8.6291] })).unwrap();
        app.run("select.set", &json!({ "ids": [n[1], n[0], n[2], n[3]] })).unwrap();
        let got = places(&app);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].ids.clone(), got[1].ids.clone()), (vec![ids[1]], vec![ids[0], ids[2]]));
        assert_eq!(got[1].at, LatLon::new(38.7116, -9.1389).unwrap());
        assert_eq!(status(got.len()), "2 places in the selection");
        assert_eq!((status(0).as_str(), status(1).as_str()), ("No places in the selection", "1 place in the selection"));
        // Nothing selected: the tree's folder.
        app.run("select.none", &json!({})).unwrap();
        let folder = app.session().unwrap().library().photo(ids[0]).unwrap().folder;
        app.state_mut().folder = Some(folder);
        assert_eq!(places(&app).iter().map(|p| p.ids.len()).sum::<usize>(), 3);
        // Kept per revision: a change shows at once.
        let ctx = egui::Context::default();
        assert_eq!(cached(&app, &ctx).len(), 2);
        app.run("geo.clear", &json!({ "ids": [n[1]] })).unwrap();
        assert_eq!(cached(&app, &ctx).len(), 1);
    }

    #[test]
    fn a_grouped_marker_holds_every_photo_of_its_places() {
        let at = |lat, lon| LatLon::new(lat, lon).unwrap();
        let id = PhotoId::new;
        let places = [
            Place { at: at(1.0, 1.0), ids: vec![id(1), id(2)] },
            Place { at: at(2.0, 2.0), ids: vec![id(3)] },
            Place { at: at(3.0, 3.0), ids: vec![id(4)] },
        ];
        let g = |members: Vec<usize>| MarkerGroup { id: members[0], members, count: 0, at: egui::Pos2::ZERO };
        let groups = [g(vec![0, 2]), g(vec![1])];
        assert_eq!(grouped(&places, &groups, 0), Some(Place { at: at(1.0, 1.0), ids: vec![id(1), id(2), id(4)] }));
        assert_eq!(grouped(&places, &groups, 2).map(|p| p.ids.len()), Some(3), "any member speaks for the group");
        assert_eq!(grouped(&places, &groups, 1).map(|p| p.ids), Some(vec![id(3)]));
        assert_eq!(grouped(&places, &[], 2).map(|p| p.ids), Some(vec![id(4)]), "no group drawn: the place alone");
        assert_eq!(grouped(&places, &groups, 3), None);
    }
}
