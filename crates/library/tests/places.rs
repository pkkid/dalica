//! Geotags in the model (`organizing.md` §7, `data-model.md` §2): set, moved and cleared with
//! undo, the model's word over the file's GPS, the geotagged and geotags queries, and the
//! Geotagged filter reading them.

use std::path::PathBuf;

use dalica_core::geo::LatLon;
use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::search::{Extract, Filters, Query, Scope, SearchIndex, Update};
use dalica_library::{Facts, Folder, Geo, Library, Op, Photo};

use crate::common::photo;

const F: FolderId = FolderId::new(1);
const X: PhotoId = PhotoId::new(1);
const Y: PhotoId = PhotoId::new(2);
const Z: PhotoId = PhotoId::new(3);

fn at(lat: f64, lon: f64) -> LatLon {
    LatLon::new(lat, lon).unwrap()
}

/// X has GPS in its file only, Y nothing, Z GPS in its file too.
fn fixture() -> Library {
    let mut lib = Library::new();
    lib.apply(Op::AddFolder { folder: Folder { id: F, path: PathBuf::from("/p/trip") } }).unwrap();
    let big = |id, name| Photo {
        width: 4000,
        height: 3000,
        mtime: Timestamp::from_millis(1_700_000_000_000 + id as i64),
        ..photo(PhotoId::new(id), F, name)
    };
    lib.apply(Op::AddPhotos { photos: vec![big(1, "x.jpg"), big(2, "y.jpg"), big(3, "z.jpg")] }).unwrap();
    let gps = Facts { geotagged: true, ..Facts::default() };
    lib.apply(Op::SetFacts { facts: vec![(X, gps.clone()), (Z, gps)] }).unwrap();
    lib
}

/// A geotag set, moved and cleared, each one undo step; points are kept at six decimals.
#[test]
fn a_geotag_set_moved_and_cleared_with_undo() {
    let mut lib = fixture();
    let start = lib.model().clone();
    let lisbon = at(38.711_6, -9.138_9);
    let set = lib.apply(Op::SetGeo { geo: vec![(Y, Some(Geo::At(lisbon)))] }).unwrap();
    assert_eq!((lib.geotag(Y), lib.is_geotagged(Y)), (Some(lisbon), true));
    let after_set = lib.model().clone();

    let porto = at(41.157_944_4, -8.629_105_3);
    let moved = lib.apply(Op::SetGeo { geo: vec![(Y, Some(Geo::At(porto)))] }).unwrap();
    assert_eq!(lib.geotag(Y), Some(at(41.157_944, -8.629_105)), "stored at six decimals");
    let after_move = lib.model().clone();

    let cleared = lib.apply(Op::SetGeo { geo: vec![(Y, None)] }).unwrap();
    assert_eq!((lib.geotag(Y), lib.geo(Y), lib.is_geotagged(Y)), (None, None, false));

    lib.apply(cleared).unwrap();
    assert_eq!(lib.model(), &after_move);
    lib.apply(moved).unwrap();
    assert_eq!(lib.model(), &after_set);
    lib.apply(set).unwrap();
    assert_eq!(lib.model(), &start);

    // Removing the photo takes its geotag; putting it back brings it.
    lib.apply(Op::SetGeo { geo: vec![(Y, Some(Geo::At(lisbon)))] }).unwrap();
    let undo = lib.apply(Op::RemovePhotos { ids: vec![Y] }).unwrap();
    assert_eq!(lib.geo(Y), None);
    lib.apply(undo).unwrap();
    assert_eq!(lib.geotag(Y), Some(lisbon));
    // An unknown photo is refused, changing nothing.
    let model = lib.model().clone();
    assert!(lib.apply(Op::SetGeo { geo: vec![(Y, None), (PhotoId::new(9), Some(Geo::Removed))] }).is_err());
    assert_eq!(lib.model(), &model);
}

/// The model's word wins over the file's GPS: a point geotags a photo whose file has none, and a
/// location the user removed un-geotags one whose file has it. Without a word of the model's the
/// file's GPS counts. The Geotagged filter reads the same rule, and follows the ops.
#[test]
fn the_geotagged_query_takes_the_models_geotag_over_the_files() {
    let mut lib = fixture();
    assert_eq!(lib.geotagged_photos(), [X, Z], "the files' GPS");
    let lisbon = at(38.7116, -9.1389);
    let mut index = SearchIndex::from_library(&lib);
    let op = Op::SetGeo { geo: vec![(Y, Some(Geo::At(lisbon))), (Z, Some(Geo::Removed))] };
    lib.apply(op.clone()).unwrap();
    index.apply(Update::for_op(&lib, &op));
    assert!(lib.is_geotagged(Y) && !lib.is_geotagged(Z) && lib.is_geotagged(X));
    assert_eq!(lib.geotagged_photos(), [X, Y]);
    assert_eq!(lib.geotags_of(&[Z, Y, X, Y]), [(Y, lisbon)], "points of the model only, each once");

    let geotagged = Query { filters: Filters { geotagged: true, ..Filters::default() }, ..Query::default() };
    assert_eq!(index.matching(&geotagged, &Scope::default()), [X, Y]);
    assert_eq!(Extract::from_library(&lib).matching(&geotagged, &Scope::default()), [X, Y]);
    assert!(index == SearchIndex::from_library(&lib));

    // Forgetting the model's word lets the file's GPS count again.
    let op = Op::SetGeo { geo: vec![(Z, None)] };
    lib.apply(op.clone()).unwrap();
    index.apply(Update::for_op(&lib, &op));
    assert_eq!(index.matching(&geotagged, &Scope::default()), [X, Y, Z]);
    // Geotags stamp the last-changed time; the journal names the op.
    let now = Timestamp::from_millis(1_790_000_000_000);
    let op = lib.stamped(Op::SetGeo { geo: vec![(X, Some(Geo::Removed))] }, now);
    lib.apply(op).unwrap();
    assert_eq!(lib.last_changed(X), Some(now));
    let json = serde_json::to_value(Op::SetGeo { geo: vec![(X, Some(Geo::At(lisbon))), (Y, Some(Geo::Removed)), (Z, None)] }).unwrap();
    assert_eq!(json["op"], "setGeo");
    assert_eq!(json["geo"][0][1], serde_json::json!({ "at": [38.7116, -9.1389] }));
    assert_eq!(json["geo"][1][1], "removed");
}
