//! Places on the globe (`geo`): typed coordinates read and written, refusals, distances and the
//! Web Mercator tile maths of the Places pane's map.

use dalica_core::geo::{
    GeoBox, GeoError, LatLon, MAX_LATITUDE, MAX_TEXT, MAX_TILES, MAX_ZOOM, TILE_SIZE, Tile, WorldPx, from_world, tiles_covering, to_world, world_size,
};
use proptest::prelude::*;

use crate::common::cases;

fn at(lat: f64, lon: f64) -> LatLon {
    LatLon::new(lat, lon).unwrap()
}

/// Decimal and degrees-minutes-seconds text read back as the point printed (at six decimals).
#[test]
fn decimal_and_dms_round_trip() {
    let lisbon = at(38.7116, -9.1389);
    assert_eq!(lisbon.to_decimal(), "38.711600, -9.138900");
    assert_eq!(lisbon.to_dms(), "38°42'41.760\"N 9°8'20.040\"W");
    for p in [lisbon, at(-33.856784, 151.215297), at(0.0, 0.0), at(90.0, 180.0), at(-90.0, -180.0), at(64.1355, -21.8954), at(-0.000001, 0.000001)] {
        assert_eq!(LatLon::parse(&p.to_decimal()), Ok(p), "{p}");
        assert_eq!(LatLon::parse(&p.to_dms()).map(|q| q.round6()), Ok(p), "{}", p.to_dms());
        assert_eq!(p.to_string().parse::<LatLon>(), Ok(p));
    }
    // The spellings people type.
    for text in [
        "38.7116 -9.1389",
        "38.7116,-9.1389",
        "38.7116 N, 9.1389 W",
        "N 38.7116 W 9.1389",
        "9.1389 W 38.7116 N",
        "38°42′41.76″N 9°8′20.04″W",
        "38° 42' 41.76\" N; 9° 8' 20.04\" W",
        "38 42 41.76 -9 8 20.04",
        "38°42.696'N 9°8.334'W",
        "+38.7116 −9.1389",
    ] {
        assert_eq!(LatLon::parse(text).map(|p| p.round6()), Ok(lisbon), "{text}");
    }
    let json = serde_json::to_string(&lisbon).unwrap();
    assert_eq!(json, "[38.7116,-9.1389]");
    assert_eq!(serde_json::from_str::<LatLon>(&json).unwrap(), lisbon);
}

proptest! {
    #![proptest_config(cases(256, 32))]

    #[test]
    fn any_point_round_trips(lat in -90.0f64..=90.0, lon in -180.0f64..=180.0) {
        let p = at(lat, lon).round6();
        prop_assert_eq!(LatLon::parse(&p.to_decimal()), Ok(p));
        prop_assert_eq!(LatLon::parse(&p.to_dms()).map(|q| q.round6()), Ok(p));
        let w = to_world(p, 12).unwrap();
        let back = from_world(w, 12).unwrap();
        if lat.abs() <= MAX_LATITUDE {
            prop_assert!((back.lat() - p.lat()).abs() < 1e-9 && (back.lon() - p.lon()).abs() < 1e-9, "{p} → {back}");
        }
    }
}

/// `NaN`, infinities and values off the globe are refused, typed, in JSON or as numbers.
#[test]
fn nan_and_out_of_range_are_refused() {
    assert_eq!(LatLon::new(f64::NAN, 0.0), Err(GeoError::Latitude));
    assert_eq!(LatLon::new(0.0, f64::NAN), Err(GeoError::Longitude));
    assert_eq!(LatLon::new(f64::INFINITY, 0.0), Err(GeoError::Latitude));
    assert_eq!(LatLon::new(90.000001, 0.0), Err(GeoError::Latitude));
    assert_eq!(LatLon::new(0.0, -180.000001), Err(GeoError::Longitude));
    assert!(serde_json::from_str::<LatLon>("[91, 0]").is_err());
    assert!(serde_json::from_str::<LatLon>("[0, 1e999]").is_err());
    assert!(serde_json::from_str::<LatLon>("[1]").is_err());
    for text in [
        "",
        "38.7",
        "NaN 0",
        "inf 0",
        "91 0",
        "0 181",
        "1 2 3",
        "38 N 9 N",
        "-9 W 38 N",
        "38°60' 9°",
        "38.5°30' 9°",
        "38 42 61 9 0 0",
        "38, 9, 1",
        "38 x 9",
        "12°'",
        "°38 9",
        "1e5 0",
    ] {
        assert!(LatLon::parse(text).is_err(), "{text:?} read as {:?}", LatLon::parse(text));
    }
    assert_eq!(LatLon::parse("91 0"), Err(GeoError::Latitude));
    assert_eq!(LatLon::parse("0 181"), Err(GeoError::Longitude));
    assert_eq!(LatLon::parse(&format!("{}1 2", " ".repeat(MAX_TEXT))), Err(GeoError::Text));
}

/// Distances along the globe, in metres.
#[test]
fn great_circle_distance() {
    let lisbon = at(38.7223, -9.1393);
    let porto = at(41.1579, -8.6291);
    let d = lisbon.distance_m(&porto);
    assert!((d - 274_000.0).abs() < 1_000.0, "{d}");
    assert_eq!(lisbon.distance_m(&lisbon), 0.0);
    let half = at(0.0, 0.0).distance_m(&at(0.0, 180.0));
    assert!((half - std::f64::consts::PI * 6_371_008.8).abs() < 1e-3, "{half}");
}

/// A tile's corners map back to its pixel corners, and the tile under its centre is the tile.
#[test]
fn a_tiles_corners_map_back_to_their_points() {
    for zoom in [0, 1, 5, 12, MAX_ZOOM] {
        let n = 1u32 << zoom;
        for (x, y) in [(0, 0), (n - 1, n - 1), (n / 2, n / 3), (n / 7, n - 1)] {
            let tile = Tile::new(zoom, x, y).unwrap();
            let (nw, se) = tile.corners().unwrap();
            let o = tile.origin();
            let size = f64::from(TILE_SIZE);
            let w = to_world(nw, zoom).unwrap();
            assert!((w.x - o.x).abs() < 1e-6 && (w.y - o.y).abs() < 1e-6, "{tile:?}: {w:?} vs {o:?}");
            let w = to_world(se, zoom).unwrap();
            assert!((w.x - (o.x + size)).abs() < 1e-6 && (w.y - (o.y + size)).abs() < 1e-6, "{tile:?}: {w:?}");
            let centre = from_world(WorldPx { x: o.x + size / 2.0, y: o.y + size / 2.0 }, zoom).unwrap();
            assert_eq!(Tile::at(centre, zoom), Ok(tile));
        }
    }
    // The whole world at zoom 0 is one tile from the projection's north edge to its south edge.
    let (nw, se) = Tile::new(0, 0, 0).unwrap().corners().unwrap();
    assert!((nw.lat() - MAX_LATITUDE).abs() < 1e-9 && (se.lat() + MAX_LATITUDE).abs() < 1e-9);
    assert_eq!((nw.lon(), se.lon()), (-180.0, 180.0));
    // Past the edge, latitudes are clamped; longitudes wrap; zooms past the last are refused.
    assert!(to_world(at(89.0, 0.0), 3).unwrap().y.abs() < 1e-6);
    assert!((to_world(at(-90.0, 0.0), 3).unwrap().y - world_size(3).unwrap()).abs() < 1e-6);
    let wrapped = from_world(WorldPx { x: world_size(2).unwrap() * 1.25, y: 512.0 }, 2).unwrap();
    assert!((wrapped.lon() + 90.0).abs() < 1e-9, "{wrapped}");
    assert_eq!(world_size(MAX_ZOOM + 1), Err(GeoError::Zoom));
    assert_eq!(Tile::new(2, 4, 0), Err(GeoError::Tile));
}

/// The tiles covering a box, west to east across the antimeridian, north to south.
#[test]
fn tiles_covering_a_box_across_the_antimeridian() {
    // From 170° E to 170° W, 10° S to 10° N, at zoom 3 (8 × 8 tiles of 45°).
    let area = GeoBox::new(at(-10.0, 170.0), at(10.0, -170.0)).unwrap();
    assert!(area.crosses_antimeridian() && area.contains(at(0.0, 180.0)) && area.contains(at(0.0, -175.0)) && !area.contains(at(0.0, 0.0)));
    let tiles = tiles_covering(area, 3).unwrap();
    let xy: Vec<(u32, u32)> = tiles.iter().map(|t| (t.x, t.y)).collect();
    assert_eq!(xy, [(7, 3), (0, 3), (7, 4), (0, 4)]);
    // Not crossing: the columns in between.
    let lisbon = GeoBox::new(at(38.6, -9.3), at(38.8, -9.0)).unwrap();
    let tiles = tiles_covering(lisbon, 12).unwrap();
    assert!(!tiles.is_empty() && tiles.iter().all(|t| t.zoom == 12));
    for p in [at(38.6, -9.3), at(38.8, -9.0), at(38.7, -9.15)] {
        assert!(tiles.contains(&Tile::at(p, 12).unwrap()));
    }
    // A box past the projection's edge is clamped to it; too many tiles are refused.
    let world = GeoBox::new(at(-90.0, -180.0), at(90.0, 180.0)).unwrap();
    assert_eq!(tiles_covering(world, 2).unwrap().len(), 16);
    assert_eq!(tiles_covering(world, MAX_ZOOM), Err(GeoError::TooManyTiles));
    assert!(tiles_covering(world, 6).unwrap().len() <= MAX_TILES);
    assert_eq!(GeoBox::new(at(10.0, 0.0), at(-10.0, 1.0)), Err(GeoError::Box));
}
