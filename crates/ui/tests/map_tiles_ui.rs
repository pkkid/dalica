//! Options ▸ Network and the Places pane's map tiles and place search (M13.28, `organizing.md` §7,
//! `menus-and-shortcuts.md` §5 Network, reserved decision 4), headless, on a library of three
//! scenes photos whose session talks to a counting in-memory tile server and geocoding service at
//! the default addresses (never the network):
//!
//! - no request until Allow: the pane asks "Show map tiles from OpenStreetMap? …"; after Allow the
//!   visible tiles are fetched once each and drawn with OpenStreetMap's attribution, moving within
//!   them asks nothing more, and closing the pane stops the requests (a view moved while it is
//!   closed asks nothing; reopened, the cached tiles come back without a request);
//! - Not Now keeps the offline map (its attribution, no request) and the question is not asked
//!   again, even after a restart;
//! - a name searched on Enter lists the places found and a click on one moves the pin and the map
//!   there; before tiles are allowed a name asks nothing;
//! - the tab's values survive a restart, the tab fits 900 × 600, and the tab and the pane with
//!   tiles match their fingerprints at 1400 × 900 (`tests/fingerprints/dialog-options-network-` and
//!   `places-tiles-1400x900.json`; `DALICA_BLESS=1` stores new ones; renders go to
//!   `target/test-output/ui/`).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_sizes, check_small, new_driver, shot, try_widget, widget};
use dalica_core::geo::Tile;
use dalica_engine::Session;
use dalica_engine::geocode::query_url;
use dalica_engine::map_tiles::{DEFAULT_ATTRIBUTION, DEFAULT_GEOCODER, SharedTransport};
use dalica_fetch::{Limits, MemoryTransport, Reply, Request, Served, Transport, TransportError};
use dalica_ui::dialogs::options::network;
use dalica_ui::panels::places::{search, tiles};
use dalica_ui::state::Pane;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(30);
const TILE_PREFIX: &str = "https://tile.openstreetmap.org/";
/// Portugal at zoom 6, as the Places pane test looks at it.
const PORTUGAL: [f64; 2] = [39.6, -8.0];

/// The tile server and the geocoding service: every tile address answers a 256 × 256 picture of
/// its own (a pale land colour with a darker grid and a shade from its place), a search address
/// answers what was registered; every request is counted.
struct Server {
    inner: MemoryTransport,
    seen: Mutex<Vec<String>>,
}

impl Server {
    fn new() -> Arc<Server> {
        Arc::new(Server { inner: MemoryTransport::new(), seen: Mutex::new(Vec::new()) })
    }

    fn urls(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn tiles(&self) -> usize {
        self.urls().iter().filter(|u| u.starts_with(TILE_PREFIX)).count()
    }

    fn searches(&self) -> usize {
        self.urls().iter().filter(|u| !u.starts_with(TILE_PREFIX)).count()
    }
}

/// A tile's picture: land with a grid every 32 pixels, shaded by its column and row.
fn tile_png(x: u32, y: u32) -> Vec<u8> {
    let shade = ((x * 7 + y * 13) % 24) as u8;
    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for py in 0..256u32 {
        for px in 0..256u32 {
            let grid = px % 32 == 0 || py % 32 == 0;
            let edge = px == 0 || py == 0;
            let (r, g, b) = if edge {
                (150, 140, 120)
            } else if grid {
                (200, 194, 178)
            } else {
                (226 - shade, 220 - shade, 200 - shade / 2)
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    dalica_scenes::fingerprint::png_rgba(256, 256, &rgba)
}

impl Transport for Server {
    fn get(&self, request: &Request, limits: &Limits<'_>) -> Result<Reply, TransportError> {
        let url = request.url().to_string();
        self.seen.lock().unwrap().push(url.clone());
        if let Some(rest) = url.strip_prefix(TILE_PREFIX) {
            let parts: Vec<u32> = rest.trim_end_matches(".png").split('/').filter_map(|p| p.parse().ok()).collect();
            if let [_, x, y] = parts[..] {
                self.inner.serve(&url, Served::new(tile_png(x, y)));
            }
        }
        self.inner.get(request, limits)
    }
}

struct T {
    dir: tempfile::TempDir,
    server: Arc<Server>,
    d: Driver,
}

/// A session on the library in `dir` talking to `server`, its map cache in `dir/cache`.
fn session(dir: &Path, server: &Arc<Server>) -> Session {
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_network_transport(Some(server.clone() as SharedTransport));
    s.set_map_cache_dir(Some(dir.join("cache")));
    s
}

/// Three photos in one folder at `w × h`, nothing opened yet.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 3, 31).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let server = Server::new();
    let mut s = session(dir.path(), &server);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { dir, server, d }
}

/// The same library after a restart (the settings written first).
fn restart(t: T) -> T {
    let T { dir, server, d } = t;
    let (w, h, _) = d.size();
    let mut app = d.into_app();
    app.take_session().unwrap().flush().unwrap();
    drop(app);
    let mut d = new_driver(App::new(Some(session(dir.path(), &server))), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { dir, server, d }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    t.d.settle(FRAMES).unwrap();
}

fn has(t: &T, id: &str) -> bool {
    try_widget(&t.d, id).is_some()
}

fn session_mut(t: &mut T) -> &mut Session {
    t.d.app_mut().session_mut().unwrap()
}

/// Lets the tiles asked for arrive and be drawn.
fn tiles_arrive(t: &mut T) {
    assert!(session_mut(t).wait_map_tiles(WAIT), "the tiles arrived");
    t.d.settle(FRAMES).unwrap();
}

/// Lets the place search arrive and be listed.
fn search_arrives(t: &mut T) {
    assert!(session_mut(t).wait_place_search(WAIT), "the search arrived");
    t.d.settle(FRAMES).unwrap();
}

/// Opens the Places pane on Portugal at zoom 6.
fn open_pane(t: &mut T) {
    run(t, "pane.places", json!({}));
    run(t, "map.pan", json!({ "centre": PORTUGAL, "zoom": 6 }));
    assert_eq!(t.d.state().pane, Some(Pane::Places));
}

fn attribution(t: &T) -> String {
    widget(&t.d, tiles::ATTRIBUTION_ID).label
}

/// The tiles the map shows now (the same arithmetic as the pane's request).
fn visible_tiles(t: &T) -> Vec<Tile> {
    let r = widget(&t.d, "map:places").rect;
    let rect = egui::Rect::from_min_size(egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3]));
    let p = &t.d.state().places;
    let view = dalica_ui_kit::widgets::MapView::new(dalica_core::geo::LatLon::new(p.centre[0], p.centre[1]).unwrap(), p.zoom as u8);
    dalica_core::geo::tiles_covering(view.visible(rect).unwrap(), view.zoom()).unwrap()
}

fn tiles_asked(t: &T) -> bool {
    t.d.app().session().unwrap().settings().get("network").and_then(|n| n.get("tilesAsked")).and_then(Value::as_bool) == Some(true)
}

#[test]
fn no_request_until_allow_then_tiles_with_the_attribution_until_the_pane_closes() {
    let mut t = setup(LARGE);
    open_pane(&mut t);
    assert!(t.d.app().session().unwrap().map_tiles().places_open(), "the pane told the session");
    let q = widget(&t.d, tiles::QUESTION_ID);
    assert_eq!(q.label, tiles::question(dalica_engine::map_tiles::DEFAULT_TILE_SERVER));
    assert!(q.label.starts_with("Show map tiles from OpenStreetMap? Dalica fetches map pictures"), "{}", q.label);
    assert_eq!(attribution(&t), dalica_ui_kit::widgets::OFFLINE_ATTRIBUTION);
    run(&mut t, "map.zoomIn", json!({}));
    run(&mut t, "map.zoomOut", json!({}));
    assert_eq!(t.server.urls(), Vec::<String>::new(), "nothing asked before Allow");
    shot(&t.d, "places-tiles-question-1400x900");

    click(&mut t, tiles::ALLOW_ID);
    tiles_arrive(&mut t);
    assert!(!has(&t, tiles::QUESTION_ID) && tiles_asked(&t), "answered once, remembered");
    assert!(t.d.app().session().unwrap().map_tiles_allowed());
    let want = visible_tiles(&t);
    assert!((2..=40).contains(&want.len()), "{want:?}");
    let mut asked: Vec<String> = t.server.urls();
    asked.sort();
    let mut expected: Vec<String> = want.iter().map(|w| format!("{TILE_PREFIX}{}/{}/{}.png", w.zoom, w.x, w.y)).collect();
    expected.sort();
    assert_eq!(asked, expected, "exactly the visible tiles, once each");
    assert_eq!(t.d.app().session().unwrap().map_tiles().stats().fetched, want.len() as u64);
    assert_eq!(attribution(&t), DEFAULT_ATTRIBUTION, "the provider's attribution with its tiles");
    check_region(&t.d, "pane:places", "places-tiles-1400x900");

    // Moving a little within the same tiles asks nothing more; zooming in asks the new ones.
    let before = t.server.tiles();
    run(&mut t, "map.pan", json!({ "centre": [PORTUGAL[0] + 0.01, PORTUGAL[1] + 0.01] }));
    assert_eq!(t.server.tiles(), before, "the same tiles");
    run(&mut t, "map.zoomIn", json!({}));
    tiles_arrive(&mut t);
    let zoomed = t.server.tiles();
    assert!(zoomed > before, "the next zoom's tiles");

    // Closed: the session is told, and nothing more is asked whatever the view does.
    run(&mut t, "pane.places", json!({}));
    assert_eq!(t.d.state().pane, None);
    assert!(!t.d.app().session().unwrap().map_tiles().places_open());
    run(&mut t, "map.pan", json!({ "centre": [41.0, -8.6], "zoom": 9 }));
    for _ in 0..5 {
        t.d.settle(FRAMES).unwrap();
    }
    assert_eq!(t.server.tiles(), zoomed, "no request with the pane closed");
    assert_eq!(t.d.app().session().unwrap().map_tiles().pending(), 0);

    // Reopened on the first view: its tiles come from the cache, with no request.
    run(&mut t, "map.pan", json!({ "centre": PORTUGAL, "zoom": 6 }));
    run(&mut t, "pane.places", json!({}));
    tiles_arrive(&mut t);
    assert_eq!(t.server.tiles(), zoomed, "cached tiles are not fetched again");
    assert_eq!(attribution(&t), DEFAULT_ATTRIBUTION);
    shot(&t.d, "places-tiles-window-1400x900");
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    if t.d.state().pane.is_none() {
        // Narrowing the window closes the pane (`ui-layout.md` §7): open it again there.
        run(&mut t, "pane.places", json!({}));
    }
    tiles_arrive(&mut t);
    check_small(&mut t.d, "pane:places");
    shot(&t.d, "places-tiles-window-900x600");
}

#[test]
fn not_now_keeps_the_offline_map_and_is_not_asked_again() {
    let mut t = setup(LARGE);
    open_pane(&mut t);
    click(&mut t, tiles::NOT_NOW_ID);
    assert!(!has(&t, tiles::QUESTION_ID) && tiles_asked(&t));
    assert!(!t.d.app().session().unwrap().map_tiles_allowed());
    run(&mut t, "map.zoomIn", json!({}));
    run(&mut t, "map.pan", json!({ "centre": [41.0, -8.6] }));
    assert_eq!(attribution(&t), dalica_ui_kit::widgets::OFFLINE_ATTRIBUTION);
    // A name asks nothing while tiles are off; the reason is on the status line.
    click(&mut t, "field:places.search");
    t.d.type_text("Lisbon").unwrap();
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().status.contains("allow map tiles"), "{}", t.d.state().status);
    assert!(t.d.state().places.results.is_empty());
    assert_eq!(t.server.urls(), Vec::<String>::new(), "no request after Not Now");
    let mut t = restart(t);
    open_pane(&mut t);
    assert!(!has(&t, tiles::QUESTION_ID), "asked once");
    assert_eq!(t.server.urls(), Vec::<String>::new());
}

#[test]
fn a_name_searched_on_enter_lists_the_places_and_a_click_moves_the_pin_there() {
    let mut t = setup(LARGE);
    let reply = json!([
        { "name": "Lisboa", "display_name": "Lisboa, Portugal", "lat": "38.7077", "lon": "-9.1366" },
        { "name": "Lisbon", "display_name": "Lisbon, Grafton County, New Hampshire, United States", "lat": "44.2134", "lon": "-71.9112" },
        { "name": "Lisbon", "display_name": "Lisbon, Columbiana County, Ohio, United States", "lat": "40.7720", "lon": "-80.7681" },
    ]);
    let url = query_url(DEFAULT_GEOCODER, "Lisbon").unwrap();
    t.server.inner.serve(&url, Served::new(reply.to_string().into_bytes()));
    open_pane(&mut t);
    click(&mut t, tiles::ALLOW_ID);
    tiles_arrive(&mut t);
    let tiles_before = t.server.tiles();

    // Typed coordinates need no search.
    click(&mut t, "field:places.search");
    t.d.type_text("38.7, -9.14").unwrap();
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().places.pin.as_ref().map(|p| p.at), Some([38.7, -9.14]));
    assert_eq!(t.server.searches(), 0);
    run(&mut t, "places.bubble", json!({}));

    // A name: one request on Enter, the places listed.
    click(&mut t, "field:places.search");
    t.d.key("Ctrl+A").unwrap();
    t.d.type_text("Lisbon").unwrap();
    assert_eq!(t.server.searches(), 0, "nothing as it is typed");
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    search_arrives(&mut t);
    assert_eq!(t.server.searches(), 1);
    assert_eq!(widget(&t.d, search::LIST_ID).label, "3 places found");
    let rows: Vec<String> = (0..3).map(|n| widget(&t.d, &search::result_id(n)).label).collect();
    assert_eq!(rows[0], "Lisboa, Portugal");
    assert!(rows[1].starts_with("Lisbon, Grafton County"), "{rows:?}");
    shot(&t.d, "places-search-results-1400x900");
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    if t.d.state().pane.is_none() {
        run(&mut t, "pane.places", json!({}));
    }
    shot(&t.d, "places-search-results-900x600");
    crate::common::check_fits(&t.d, "the results at 900 × 600");
    t.d.resize(LARGE.0, LARGE.1);
    t.d.settle(FRAMES).unwrap();

    // A click on the second place: the pin there, named after it, the map moved, the list closed.
    click(&mut t, &search::result_id(1));
    let p = &t.d.state().places;
    let pin = p.pin.as_ref().expect("the pin");
    assert_eq!(pin.at, [44.2134, -71.9112]);
    assert!(pin.place.as_deref().is_some_and(|n| n.starts_with("Lisbon, Grafton County")));
    assert_eq!(p.centre, [44.2134, -71.9112]);
    assert!(p.results.is_empty() && !has(&t, search::LIST_ID));
    assert!(has(&t, "map:places/pin") && has(&t, "map:places/bubble"));
    // The same name again is answered from memory; × closes a list.
    click(&mut t, "field:places.search");
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.server.searches(), 1, "answered from memory");
    assert!(has(&t, search::LIST_ID));
    click(&mut t, search::CLOSE_ID);
    assert!(!has(&t, search::LIST_ID));
    tiles_arrive(&mut t);
    assert!(t.server.tiles() >= tiles_before);
}

/// Types `text` into the field `id`, replacing what it held.
fn retype(t: &mut T, id: &str, text: &str) {
    click(t, id);
    t.d.key("Ctrl+A").unwrap();
    t.d.type_text(text).unwrap();
    t.d.settle(FRAMES).unwrap();
}

#[test]
fn the_tabs_values_survive_a_restart_and_it_fits_the_small_window() {
    let mut t = setup(LARGE);
    run(&mut t, "app.preferences", json!({ "tab": "network" }));
    for id in [
        "check:option.network.allowMapTiles",
        "field:option.network.tileServer",
        "field:option.network.tileAttribution",
        "field:option.network.geocoder",
        "choice:option.network.proxy/system",
        "choice:option.network.proxy/manual",
    ] {
        assert!(widget(&t.d, id).enabled, "{id}");
    }
    assert!(!widget(&t.d, "field:option.network.proxyHost").enabled, "the host waits for Manual");
    assert!(!widget(&t.d, "choice:option.network.logLevel").enabled, "the log level waits for the network log");
    check_sizes(&mut t.d, "dialog:options", "dialog-options-network");
    shot(&t.d, "dialog-options-network-1400x900");

    click(&mut t, "check:option.network.allowMapTiles");
    retype(&mut t, "field:option.network.tileServer", "https://maps.example/{z}/{x}");
    retype(&mut t, "field:option.network.tileServer", "https://maps.example/{z}/{x}/{y}.png");
    retype(&mut t, "field:option.network.tileAttribution", "© Example Maps");
    retype(&mut t, "field:option.network.geocoder", "https://geo.example/search?q={query}");
    click(&mut t, "choice:option.network.proxy/manual");
    assert!(widget(&t.d, "field:option.network.proxyHost").enabled);
    retype(&mut t, "field:option.network.proxyHost", "proxy.example");
    retype(&mut t, "field:option.network.proxyPort", "3128x");
    shot(&t.d, "dialog-options-network-edited-1400x900");
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "dialog-options-network-900x600");
    t.d.resize(LARGE.0, LARGE.1);
    t.d.settle(FRAMES).unwrap();
    click(&mut t, "button:dialog.ok");
    assert!(!has(&t, "dialog:options"));
    assert_eq!(t.server.urls(), Vec::<String>::new(), "the tab connects to nothing");

    let mut t = restart(t);
    let saved: Vec<(String, Value)> = network::rows(t.d.app()).into_iter().map(|o| (o.row.key, o.row.saved)).collect();
    assert_eq!(
        saved,
        [
            (network::ALLOW.to_string(), json!(true)),
            (network::TILE_SERVER.to_string(), json!("https://maps.example/{z}/{x}/{y}.png")),
            (network::ATTRIBUTION.to_string(), json!("© Example Maps")),
            (network::GEOCODER.to_string(), json!("https://geo.example/search?q={query}")),
            (network::PROXY.to_string(), json!("manual")),
            (network::PROXY_HOST.to_string(), json!("proxy.example")),
            (network::PROXY_PORT.to_string(), json!(3128)),
            (network::LOG_LEVEL.to_string(), json!("errors")),
        ]
    );
    run(&mut t, "app.preferences", json!({ "tab": "network" }));
    assert!(widget(&t.d, "check:option.network.allowMapTiles").active, "shown ticked after the restart");
    assert!(widget(&t.d, "field:option.network.proxyHost").enabled, "Manual, after the restart");
    shot(&t.d, "dialog-options-network-restarted-1400x900");
}
