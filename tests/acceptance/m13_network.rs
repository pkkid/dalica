//! No network without the user's action, in the live app (`m13.rs` part 4): `dalica --headless`
//! with `--automation-allow-external` (so the window's Allow and Download can be clicked over the
//! control channel at all), every connection it tries going to the loopback proxy, which records
//! the host asked for and refuses it:
//!
//! - start-up, a folder's scan and the Places pane open with tiles off (panned and zoomed, its
//!   question unanswered) make no connection, nor does a place name searched before tiles are
//!   allowed;
//! - the pane's Allow: the tiles are asked for, from the tile server only; a name typed in "Search
//!   for a place:" asks nothing until Enter, which asks the geocoding service;
//! - the pane closed: no more connections, the map moved meanwhile;
//! - a `catalog.json` model's Download stays disabled until the licence box is ticked, and nothing
//!   connects before; once ticked, Download connects to the model's site.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::app::{self, click, dialog, run, secs};
use crate::common::{self, Proxy};
use crate::live::{self, Client};

const TILES: &str = "tile.openstreetmap.org:443";
const GEOCODER: &str = "nominatim.openstreetmap.org:443";
const MODEL_SITE: &str = "models.accept.test:443";
const MODEL: &str = "my-catalogue-model";

/// Lets frames and workers run for `ms` of real time.
fn pause(c: &mut Client, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        c.ok("ui.settle", json!({ "timeoutMs": 50 }));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until no new connection has come for a second; the count then.
fn quiet(c: &mut Client, proxy: &Proxy) -> usize {
    let mut last = proxy.count();
    for _ in 0..60 {
        pause(c, 1000);
        let now = proxy.count();
        if now == last {
            return now;
        }
        last = now;
    }
    panic!("the app kept connecting for a minute: {:?}", proxy.connections());
}

pub fn network(exe: &Path, home: &Path) {
    let started = Instant::now();
    let proxy = Proxy::start();
    let models = home.join("models");
    std::fs::create_dir_all(&models).unwrap();
    let bytes = common::recogniser_bytes();
    let catalog = json!({ "models": [{
        "id": MODEL, "name": "My catalogue model", "version": "1", "role": "embedder",
        "url": format!("https://{}/weights/model.onnx", MODEL_SITE.trim_end_matches(":443")),
        "sha256": dalica_faces::hash::sha256_hex(&bytes), "sizeBytes": bytes.len(),
        "licence": { "name": "Research use only", "commercial": "no", "notice": "Not for commercial use." },
        "provenance": "Trained on a test set.",
        "output": { "kind": "embedding", "dim": 8 },
        "thresholds": { "matchCosine": 0.4 }
    }] });
    std::fs::write(models.join("catalog.json"), serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    let folder = home.join("Pictures").join("Lisbon");
    for i in 1..=3u64 {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1400 + i, 320, 240), 85, None).unwrap();
        app::put(&folder.join(format!("IMG_{i:04}.jpg")), &jpeg, i);
    }

    let (a, mut cl) = live::start_with(exe, home, common::launch(&proxy, &models));
    let c = &mut cl;
    app::resize(c, common::LARGE);
    pause(c, 1500);
    assert_eq!(proxy.connections(), Vec::<String>::new(), "start-up made no connection");
    run(c, "folders.add", json!({ "path": folder }));
    pause(c, 1000);
    assert_eq!(proxy.connections(), Vec::<String>::new(), "the folder's scan made no connection");

    // The pane with tiles off.
    run(c, "pane.places", json!({}));
    run(c, "map.pan", json!({ "centre": [39.6, -8.0], "zoom": 6 }));
    run(c, "map.zoomIn", json!({}));
    run(c, "map.zoomOut", json!({}));
    assert!(app::widget(c, "button:map.allowTiles").is_some(), "the pane asks before any tile");
    let searched = c.send("engine.execute", json!({ "command": "map.search", "params": { "text": "Lisbon" } }));
    pause(c, 1500);
    assert_eq!(proxy.connections(), Vec::<String>::new(), "the Places pane with tiles off made no connection (a name searched: {searched})");
    println!("  start-up, the scan and the Places pane with tiles off: no connection");

    // Allow: tiles from the tile server only.
    click(c, "button:map.allowTiles");
    let tiles = proxy.wait_beyond(0);
    assert!(!tiles.is_empty() && tiles.iter().all(|t| t == TILES), "once allowed and open, tiles only: {tiles:?}");
    let before = quiet(c, &proxy);
    c.ok("ui.clickWidget", json!({ "id": "field:places.search" }));
    c.ok("ui.type", json!({ "text": "Lisbon" }));
    pause(c, 1500);
    let typed = proxy.connections().split_off(before);
    assert!(!typed.iter().any(|t| t == GEOCODER), "typing asks nothing: {typed:?}");
    app::key(c, "Enter");
    let asked = proxy.wait_beyond(proxy.count());
    let all = proxy.connections();
    assert!(all[before..].iter().any(|t| t == GEOCODER), "Enter asks the geocoding service: {asked:?}");
    println!("  after Allow: {} connections, all to {TILES}; a place searched on Enter asked {GEOCODER}", tiles.len());

    // The pane closed: no more connections, the map moved meanwhile.
    run(c, "pane.places", json!({}));
    assert!(app::widget(c, "map:places").is_none(), "the pane is closed");
    let closed = quiet(c, &proxy);
    let _ = c.send("engine.execute", json!({ "command": "map.pan", "params": { "centre": [41.15, -8.61], "zoom": 9 } }));
    let _ = c.send("engine.execute", json!({ "command": "map.zoomIn", "params": {} }));
    pause(c, 2000);
    assert_eq!(proxy.count(), closed, "no tile once the pane is closed: {:?}", proxy.connections().split_off(closed));
    println!("  the pane closed: no more connections");

    // A catalogue model: nothing before the licence box is ticked.
    run(c, "faceModel.open", json!({ "entry": MODEL }));
    assert_eq!(dialog(c).as_deref(), Some("faceModel"));
    let refused = c.fails("ui.clickWidget", json!({ "id": "button:dialog.download" }));
    assert_eq!(refused["code"], "disabled", "Download waits for the box: {refused}");
    pause(c, 1500);
    assert_eq!(proxy.count(), closed, "nothing connects before the licence is accepted");
    c.ok("ui.clickWidget", json!({ "id": "check:dialog.accept" }));
    c.settle();
    pause(c, 1000);
    assert_eq!(proxy.count(), closed, "ticking the box connects nothing by itself");
    c.ok("ui.clickWidget", json!({ "id": "button:dialog.download" }));
    let fetched = proxy.wait_beyond(closed);
    assert_eq!(fetched.first().map(String::as_str), Some(MODEL_SITE), "Download connects to the model's site: {fetched:?}");
    assert!(!models.join(MODEL).exists(), "the refused download installed nothing");
    println!("  a catalogue model: Download disabled and no connection until the box is ticked, then {MODEL_SITE}");
    live::quit(a, cl);
    println!("  M13 network in {:.1} s", secs(started));
}
