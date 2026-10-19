//! Map tiles and place search (`roadmap.md` M13.20; `organizing.md` §7, `security.md` §4.2,
//! reserved decision 4), over a counting in-memory transport and, for the HTTPS transport the app
//! uses, a loopback listener that counts connections; never the real network.
//!
//! No request with tiles off or the Places pane closed, at start-up or during a scan; once allowed
//! and open, requests for exactly the visible tiles, at most two at a time, none after the pane
//! closes; a cached tile is not fetched again (not even by the next session); an oversized,
//! undecodable or missing tile is a placeholder and is not kept; typed coordinates are found
//! without a request; a place name is looked up only when allowed and asked from the open pane,
//! a second time from memory, never sooner than a second after the last request; hostile replies
//! are refused without a panic; the cache prunes only its own files; the owner thread makes no
//! file system call.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use dalica_core::geo::{GeoBox, LatLon, Tile, tiles_covering};
use dalica_engine::geocode;
use dalica_engine::map_tiles::{self, MAX_TILE_BYTES, MapTile, SharedTransport};
use dalica_engine::{CmdError, Session};
use dalica_fetch::{Limits, MemoryTransport, Reply, Request, Served, Transport, TransportError};
use serde_json::{Value, json};

use crate::common::{jpeg, off_owner, open};

const WAIT: Duration = Duration::from_secs(30);
const SERVER: &str = "https://tiles.test/{z}/{x}/{y}.png";
const GEOCODER: &str = "https://geo.test/search?format=jsonv2&q={query}";

/// The in-memory transport, counting requests, how many run at once, and when each reached the
/// server. `lead` holds the first request back before it arrives (a slow connection's set-up).
struct Counting {
    inner: MemoryTransport,
    delay: Duration,
    lead: Mutex<Option<Duration>>,
    active: AtomicUsize,
    most: AtomicUsize,
    seen: Mutex<Vec<(String, Instant)>>,
}

impl Counting {
    fn new(delay: Duration) -> Arc<Counting> {
        Arc::new(Counting {
            inner: MemoryTransport::new(),
            delay,
            lead: Mutex::new(None),
            active: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn urls(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|(u, _)| u.clone()).collect()
    }

    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl Transport for Counting {
    fn get(&self, request: &Request, limits: &Limits<'_>) -> Result<Reply, TransportError> {
        let lead = self.lead.lock().unwrap().take();
        std::thread::sleep(lead.unwrap_or_default());
        self.seen.lock().unwrap().push((request.url().to_string(), Instant::now()));
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.inner.get(request, limits)
    }
}

fn tile_url(t: Tile) -> String {
    format!("https://tiles.test/{}/{}/{}.png", t.zoom, t.x, t.y)
}

/// A small tile picture.
fn png(shade: u8) -> Vec<u8> {
    dalica_scenes::fingerprint::png_rgba(8, 8, &[shade; 8 * 8 * 4])
}

fn area(south: f64, west: f64, north: f64, east: f64) -> GeoBox {
    GeoBox::new(LatLon::new(south, west).unwrap(), LatLon::new(north, east).unwrap()).unwrap()
}

/// Four tiles at zoom 2 (the north-west quarter of the world).
fn view() -> (GeoBox, u8) {
    (area(10.0, -170.0, 80.0, -10.0), 2)
}

/// A session on `lib` with the map cache in `cache`, talking to `t`, the test servers configured.
fn session(lib: &Path, cache: &Path, t: &Arc<Counting>) -> Session {
    let mut s = open(lib);
    s.set_network_transport(Some(t.clone() as SharedTransport));
    s.set_map_cache_dir(Some(cache.to_path_buf()));
    s.execute("options.set", &json!({ "values": { "network.tileServer": SERVER, "network.geocoder": GEOCODER } })).unwrap();
    s
}

/// Waits for the tiles asked for and takes them, by tile.
fn tiles(s: &mut Session) -> Vec<MapTile> {
    assert!(s.wait_map_tiles(WAIT), "the tiles arrived");
    let mut got = s.take_map_tiles();
    got.sort_by_key(|t| t.tile);
    got
}

fn search(s: &mut Session, text: &str) -> Result<Value, CmdError> {
    s.execute("places.search", &json!({ "text": text }))
}

/// Tiles off or the pane closed: nothing is asked. Allowed and open: exactly the visible tiles,
/// at most two at a time, each a picture. Closed again: nothing more, and what is taken is
/// nothing. The owner thread made no file system call.
#[test]
fn tiles_are_fetched_only_while_allowed_and_the_pane_is_open() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::from_millis(30));
    let mut s = session(lib.path(), cache.path(), &t);
    let (box_, zoom) = view();
    let mut visible = tiles_covering(box_, zoom).unwrap();
    visible.sort();
    assert_eq!(visible.len(), 4);
    for (i, tile) in visible.iter().enumerate() {
        t.inner.serve(&tile_url(*tile), Served::new(png(i as u8 * 40)));
    }
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();

    assert!(!s.map_tiles_allowed(), "off by default");
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0, "off, closed");
    s.set_places_open(true);
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0, "off, open");
    s.set_places_open(false);
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    assert!(s.map_tiles_allowed());
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0, "allowed, closed");
    assert!(s.take_map_tiles().is_empty());
    assert_eq!(t.count(), 0, "no request with tiles off or the pane closed");

    s.set_places_open(true);
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 4);
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0, "tiles on their way are not asked twice");
    let got = tiles(&mut s);
    assert_eq!(got.iter().map(|m| m.tile).collect::<Vec<_>>(), visible);
    assert!(got.iter().all(|m| m.image.as_ref().is_ok_and(|i| (i.width, i.height) == (8, 8))), "{got:?}");
    let urls: BTreeSet<String> = t.urls().into_iter().collect();
    assert_eq!(urls, visible.iter().map(|v| tile_url(*v)).collect(), "exactly the visible tiles");
    assert_eq!(t.count(), 4, "each once");
    assert!(t.most.load(Ordering::SeqCst) <= map_tiles::WORKERS, "at most two at a time");
    let ua = t.inner.requests()[0].headers.iter().find(|(k, _)| k == "User-Agent").map(|(_, v)| v.clone()).unwrap();
    assert!(ua.starts_with("Dalica/"), "{ua}");

    s.set_places_open(false);
    assert_eq!(s.request_map_tiles(area(-60.0, 10.0, -10.0, 170.0), zoom).unwrap(), 0);
    assert!(s.take_map_tiles().is_empty());
    assert_eq!(t.count(), 4, "none after closing");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "no fs call was refused on the owner thread");
}

/// Closing the pane cancels the tiles not yet started, and turning tiles off cancels too; the
/// tiles of a view the pane left are cancelled when the next view is asked for.
#[test]
fn closing_the_pane_cancels_the_tiles_on_their_way() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::from_millis(150));
    let mut s = session(lib.path(), cache.path(), &t);
    let world = area(-80.0, -179.0, 80.0, 179.0);
    for tile in tiles_covering(world, 3).unwrap() {
        t.inner.serve(&tile_url(tile), Served::new(png(9)));
    }
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    s.set_places_open(true);
    let asked = s.request_map_tiles(world, 3).unwrap();
    assert_eq!(asked, 64);
    std::thread::sleep(Duration::from_millis(50));
    s.set_places_open(false);
    std::thread::sleep(Duration::from_millis(400));
    assert!(t.count() <= 2 * map_tiles::WORKERS, "only the tiles already started went out: {}", t.count());
    assert!(s.take_map_tiles().is_empty());
    assert_eq!(s.map_tiles().pending(), 0);

    // A new view cancels what the last one still waits for.
    s.set_places_open(true);
    let (box_, zoom) = view();
    for tile in tiles_covering(box_, zoom).unwrap() {
        t.inner.serve(&tile_url(tile), Served::new(png(1)));
    }
    let before = t.count();
    s.request_map_tiles(world, 3).unwrap();
    s.request_map_tiles(box_, zoom).unwrap();
    let got = tiles(&mut s);
    assert!(got.iter().all(|m| m.tile.zoom == zoom), "only the new view's tiles are handed over");
    assert!(t.count() - before <= 4 + 2 * map_tiles::WORKERS, "the old view's tiles were cancelled: {}", t.count() - before);

    // Allow off cancels as well.
    s.request_map_tiles(world, 3).unwrap();
    s.execute("map.allow", &json!({ "on": false })).unwrap();
    assert_eq!(s.map_tiles().pending(), 0);
    assert!(s.take_map_tiles().is_empty());
}

/// A tile is fetched once: asked again (the pane reopened, or the next session on the same
/// cache), it comes from the cache with no request.
#[test]
fn a_cached_tile_is_not_fetched_again() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::ZERO);
    let (box_, zoom) = view();
    for tile in tiles_covering(box_, zoom).unwrap() {
        t.inner.serve(&tile_url(tile), Served::new(png(7)));
    }
    let mut s = session(lib.path(), cache.path(), &t);
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    s.set_places_open(true);
    s.request_map_tiles(box_, zoom).unwrap();
    assert!(tiles(&mut s).iter().all(|m| m.image.is_ok()));
    assert_eq!((t.count(), s.map_tiles().stats().fetched), (4, 4));

    s.set_places_open(false);
    s.set_places_open(true);
    s.request_map_tiles(box_, zoom).unwrap();
    assert!(tiles(&mut s).iter().all(|m| m.image.is_ok()));
    assert_eq!((t.count(), s.map_tiles().stats().cached), (4, 4), "from the cache");
    drop(s);

    let mut next = session(lib.path(), cache.path(), &t);
    next.set_places_open(true);
    next.request_map_tiles(box_, zoom).unwrap();
    assert_eq!(tiles(&mut next).len(), 4);
    assert_eq!(t.count(), 4, "the next session reads the same cache");
}

/// A tile past 1 MiB (announced or not), past 1 024 pixels, that is no picture, or that the
/// server does not have is a placeholder saying why; none is kept in the cache, and none is asked
/// for again until the pane reopens.
#[test]
fn an_oversized_or_undecodable_tile_is_a_placeholder() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::ZERO);
    let (box_, zoom) = view();
    let mut v = tiles_covering(box_, zoom).unwrap();
    v.sort();
    let big = vec![0u8; MAX_TILE_BYTES as usize + 1];
    t.inner.serve(&tile_url(v[0]), Served::new(big.clone()));
    t.inner.serve(&tile_url(v[1]), Served { announce: false, ..Served::new(big) });
    t.inner.serve(&tile_url(v[2]), Served::new(dalica_scenes::fingerprint::png_rgba(1025, 1, &[0; 1025 * 4])));
    t.inner.serve(&tile_url(v[3]), Served::new(b"<html>Rate limited</html>".to_vec()));
    let mut s = session(lib.path(), cache.path(), &t);
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    s.set_places_open(true);
    s.request_map_tiles(box_, zoom).unwrap();
    let got = tiles(&mut s);
    let why: Vec<String> = got.iter().map(|m| m.image.clone().unwrap_err()).collect();
    assert!(why[0].contains("bytes") && why[1].contains("bytes"), "{why:?}");
    assert!(why[2].contains("pixels"), "{why:?}");
    assert!(why[3].contains("cannot be"), "{why:?}");
    assert_eq!(s.map_tiles().stats().failed, 4);
    let kept: Vec<String> = off_owner(|| walk_names(cache.path()));
    assert!(kept.iter().all(|n| !n.ends_with(".tile") && !n.ends_with(".part")), "nothing kept: {kept:?}");

    // Asked again: the placeholders, no request. A tile the server does not have is one too.
    s.request_map_tiles(box_, zoom).unwrap();
    assert_eq!(s.take_map_tiles().len(), 4);
    assert_eq!(t.count(), 4);
    s.request_map_tiles(area(-60.0, 10.0, -10.0, 170.0), zoom).unwrap();
    assert!(tiles(&mut s).iter().all(|m| m.image.as_ref().is_err_and(|e| e.contains("404"))));
}

/// Every file name under `root`, recursively.
fn walk_names(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if e.file_type().unwrap().is_dir() {
            out.extend(walk_names(&e.path()));
        } else {
            out.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

/// Typed coordinates are found without a request, with tiles off and the pane closed; a place
/// name is refused then, with nothing asked.
#[test]
fn typed_coordinates_are_found_without_a_request() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::ZERO);
    let mut s = session(lib.path(), cache.path(), &t);
    for (text, lat, lon) in [("38.7116, -9.1389", 38.7116, -9.1389), ("38°42′42″N 9°8′20″W", 38.711_667, -9.138_889)] {
        let r = search(&mut s, text).unwrap();
        assert_eq!((r["coordinates"].as_bool(), r["pending"].as_bool()), (Some(true), Some(false)), "{text}");
        let found = &r["results"][0];
        assert!((found["lat"].as_f64().unwrap() - lat).abs() < 1e-5 && (found["lon"].as_f64().unwrap() - lon).abs() < 1e-5, "{text}: {found}");
    }
    assert!(matches!(search(&mut s, "Lisbon"), Err(CmdError::Disabled(m)) if m.contains("Options")), "not allowed");
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    assert!(matches!(search(&mut s, "Lisbon"), Err(CmdError::Disabled(m)) if m.contains("Places pane")), "the pane closed");
    assert!(matches!(search(&mut s, "   "), Err(CmdError::BadParams(_))));
    assert!(matches!(search(&mut s, &"x".repeat(600)), Err(CmdError::Limit(_))));
    assert_eq!(t.count(), 0);
}

/// A place name asked from the open pane goes to the geocoding service on a worker and lists its
/// places; asked again, it is answered from memory; two requests arrive at least a second apart,
/// also when the first is slow to arrive (the interval runs from a request's end, not from its
/// turn); hostile replies (too deep, too large, not JSON) are errors, never a panic.
#[test]
fn a_place_name_is_looked_up_once_and_hostile_replies_are_refused() {
    let (lib, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Counting::new(Duration::ZERO);
    *t.lead.lock().unwrap() = Some(Duration::from_millis(300));
    let geo = |q: &str| format!("https://geo.test/search?format=jsonv2&q={q}");
    t.inner.serve(
        &geo("Lisbon"),
        Served::new(br#"[{"lat":"38.7077507","lon":"-9.1365919","name":"Lisboa","display_name":"Lisboa, Portugal"}]"#.to_vec()),
    );
    let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
    t.inner.serve(&geo("Deep"), Served::new(deep.into_bytes()));
    t.inner.serve(&geo("Big"), Served { announce: false, ..Served::new(vec![b' '; 2 << 20]) });
    t.inner.serve(&geo("Junk"), Served::new(b"\x00\xff<html>".to_vec()));
    let mut s = session(lib.path(), cache.path(), &t);
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    s.set_places_open(true);

    let r = search(&mut s, "Lisbon").unwrap();
    assert_eq!((r["pending"].as_bool(), r["coordinates"].as_bool()), (Some(true), Some(false)));
    assert!(s.wait_place_search(WAIT));
    let done = s.take_place_search().unwrap();
    assert_eq!(done.id, r["search"].as_u64().unwrap());
    let places = done.result.unwrap();
    assert_eq!((places[0].name.as_str(), places[0].place.as_str()), ("Lisboa", "Lisboa, Portugal"));
    let again = search(&mut s, " Lisbon ").unwrap();
    assert_eq!((again["pending"].as_bool(), again["results"][0]["name"].as_str()), (Some(false), Some("Lisboa")), "from memory");
    assert_eq!(t.count(), 1);

    for name in ["Deep", "Big", "Junk"] {
        search(&mut s, name).unwrap();
        assert!(s.wait_place_search(WAIT), "{name}");
        let err = s.take_place_search().unwrap().result.unwrap_err();
        assert!(!err.is_empty(), "{name}");
    }
    let seen = t.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4);
    for pair in seen.windows(2) {
        let gap = pair[1].1.duration_since(pair[0].1);
        assert!(gap >= geocode::MIN_INTERVAL, "{gap:?} between two requests to the service");
    }
    // Nothing of a hostile reply is kept.
    let kept = off_owner(|| walk_names(cache.path()));
    assert_eq!(kept.iter().filter(|n| n.ends_with(".json")).count(), 1, "only Lisbon's reply: {kept:?}");
    // Closing the pane cancels a search on its way.
    t.inner.serve(&geo("Porto"), Served::new(b"[]".to_vec()));
    search(&mut s, "Porto").unwrap();
    s.set_places_open(false);
    assert!(!s.place_search_pending());
    assert!(s.take_place_search().is_none());
}

/// The cache is pruned oldest first down to four fifths of its budget, and only its own files go:
/// a file of another shape, a file in a folder of another shape, a folder, and a link named like
/// a tile (with the file it points at) are left alone.
#[test]
fn the_cache_prunes_only_its_own_files() {
    let root = tempfile::tempdir().unwrap();
    let server = root.path().join("0123456789abcdef");
    let other = root.path().join("not-a-server");
    std::fs::create_dir_all(&server).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(server.join("9-9-9.tile")).unwrap();
    let day = Duration::from_secs(86_400);
    let plant = |path: &Path, len: usize, age_days: u32| {
        std::fs::write(path, vec![1u8; len]).unwrap();
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - day * age_days).unwrap();
    };
    for (i, name) in ["1-0-0.tile", "2-0-0.tile", "3-0-0.tile", "q-0123456789abcdef.json", "4-0-0.tile.part"].iter().enumerate() {
        plant(&server.join(name), 1000, 10 - i as u32);
    }
    plant(&server.join("notes.txt"), 5000, 30);
    plant(&other.join("5-0-0.tile"), 5000, 30);
    let outside = tempfile::tempdir().unwrap();
    plant(&outside.path().join("kept.bin"), 5000, 30);
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("kept.bin"), server.join("6-0-0.tile")).unwrap();

    let pruned = map_tiles::prune_cache(root.path(), 3000).unwrap();
    assert_eq!((pruned.files, pruned.bytes), (5, 5000));
    assert_eq!((pruned.removed, pruned.removed_bytes), (3, 3000), "down to 2 400 bytes, oldest first");
    let mut left: Vec<String> = std::fs::read_dir(&server).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    let mut want = vec!["4-0-0.tile.part", "9-9-9.tile", "notes.txt", "q-0123456789abcdef.json"];
    if cfg!(unix) {
        want.insert(1, "6-0-0.tile");
    }
    assert_eq!(left, want);
    assert!(other.join("5-0-0.tile").exists() && outside.path().join("kept.bin").exists());
    assert_eq!(map_tiles::prune_cache(root.path(), 1 << 20).unwrap().removed, 0, "under the budget: nothing goes");
    assert_eq!(map_tiles::prune_cache(&root.path().join("missing"), 0).unwrap(), map_tiles::Pruned::default());
}

/// With the HTTPS transport the app uses (nothing injected) and a tile server, geocoder and proxy
/// on a loopback listener: start-up, a scan, the pane with tiles off, and tiles allowed with the
/// pane closed make no connection; the open pane with tiles allowed does.
#[test]
fn start_up_scans_and_the_closed_pane_make_no_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (connections, stop) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
    let counter = {
        let (connections, stop) = (connections.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((sock, _)) => {
                        connections.fetch_add(1, Ordering::SeqCst);
                        drop(sock);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        })
    };
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let settings = json!({ "network": {
        "allowMapTiles": false,
        "tileServer": format!("https://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png"),
        "geocoder": format!("https://127.0.0.1:{port}/search?q={{query}}"),
        "proxy": "manual", "proxyHost": "127.0.0.1", "proxyPort": port,
    } });
    std::fs::write(lib.path().join("settings.json"), serde_json::to_vec(&settings).unwrap()).unwrap();
    let folder = disk.path().join("Lisbon");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("IMG_0001.jpg"), jpeg(1)).unwrap();

    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 1, "scanned");
    let (box_, zoom) = view();
    s.set_places_open(true);
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0);
    s.set_places_open(false);
    s.execute("map.allow", &json!({ "on": true })).unwrap();
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 0);
    assert!(search(&mut s, "Lisbon").is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(connections.load(Ordering::SeqCst), 0, "no connection at start-up, during the scan or with the pane closed");

    s.set_places_open(true);
    assert_eq!(s.request_map_tiles(box_, zoom).unwrap(), 4);
    let got = tiles(&mut s);
    assert!(got.iter().all(|m| m.image.is_err()), "the listener is no tile server");
    assert!(connections.load(Ordering::SeqCst) >= 1, "the open pane connects");
    stop.store(true, Ordering::SeqCst);
    counter.join().unwrap();
}
