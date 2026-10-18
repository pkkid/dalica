//! Map tiles (`organizing.md` §7, `security.md` §4.2 and §8.2, reserved decision 4): the pictures
//! the Places pane draws over its offline map, fetched from the tile server only once Allow map
//! tiles is on and only while the pane is open; the place search is `geocode.rs`.
//!
//! **Reserved decision 4** (the maintainer, 2026-10-18, on #821): OpenStreetMap's standard tiles
//! and Nominatim's place search, both off until the user allows them in Options ▸ Network, the
//! OpenStreetMap attribution on the map, a Dalica User-Agent ([`dalica_fetch::USER_AGENT`]), and
//! both addresses editable for a self-hosted server. The tile usage policy is kept: every tile is
//! cached on disk and asked for again only once it is [`MAX_AGE`] old, only the tiles the pane
//! shows are asked for (no prefetching, no bulk download), at most [`WORKERS`] at a time.
//!
//! - [`Session::set_places_open`]: the window says whether the Places pane is open. Closing it
//!   cancels the tiles on their way and drops the ones not taken.
//! - [`Session::request_map_tiles`] asks for the tiles covering a box at a zoom (the pane's view);
//!   [`Session::take_map_tiles`] (each frame) hands over the tiles finished since, each a picture
//!   or a placeholder with its reason (a tile that is too large, cannot be decoded or could not be
//!   fetched). A tile on its way is not asked twice; asking for another view cancels the tiles of
//!   the last one that have not started.
//! - **Nothing is fetched** with tiles off, with the pane closed, at start-up or during scans: the
//!   only way to the network is a request from the open pane (and a place search from it).
//! - **The cache** is `<cache>/tiles/` beside the thumbnail cache (a temporary folder removed with
//!   the session when there is none): a folder per server (16 hex digits of its address template),
//!   a file per tile (`<zoom>-<x>-<y>.tile`) and per search reply (`q-<16 hex digits>.json`, from
//!   `geocode.rs`). It is kept under [`MAX_CACHE_BYTES`], oldest files first, by
//!   [`prune_cache`], which lists and deletes only files of those shapes inside folders of that
//!   shape: anything else in the folder is left alone.
//! - **Limits** (`security.md` §4.2): a tile is at most [`MAX_TILE_BYTES`] (more is refused as it
//!   arrives) and [`MAX_TILE_SIDE`] pixels on a side (read from its header before it is decoded),
//!   decoded under the raster limits; anything else is a placeholder.
//!
//! The owner thread only reads the settings, submits jobs and takes their results: the cache, the
//! transport and the decoder are the workers'. The transport is the HTTPS one (through the proxy
//! of Options ▸ Network), or the one a test or the app gives ([`Session::set_network_transport`]).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use dalica_core::geo::{GeoBox, MAX_ZOOM, Tile, tiles_covering};
use dalica_fetch::{Consent, FetchError, FileSpec, HttpsTransport, Proxy, Transport};
use dalica_image::{Limits, Rgba8};
use dalica_thumbs::{Pool, Priority};

use crate::error::CmdError;
use crate::session::Session;
use crate::settings::{NetworkSettings, ProxyMode};

/// Why the map rows of Options ▸ Network (allow map tiles, the tile server, its attribution, the
/// geocoding service) cannot be changed in this build, `None` once they can (reserved decision 4,
/// M13.20).
pub const TILES_UNAVAILABLE: Option<&str> = None;
/// Why the proxy rows cannot be changed in this build, `None` once they can (M13.20: tiles and
/// place searches go through the proxy).
pub const PROXY_UNAVAILABLE: Option<&str> = None;
/// The tile server's address template by default (OpenStreetMap's standard tiles): `{z}`, `{x}`
/// and `{y}` are the tile's zoom and column and row.
pub const DEFAULT_TILE_SERVER: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// The attribution drawn on the map under the default server's tiles.
pub const DEFAULT_ATTRIBUTION: &str = "© OpenStreetMap contributors";
/// The geocoding service's address template by default (Nominatim): `{query}` is the text searched
/// for.
pub const DEFAULT_GEOCODER: &str = "https://nominatim.openstreetmap.org/search?format=jsonv2&q={query}";

/// Tiles fetched at the same time, at most.
pub const WORKERS: usize = 2;
/// The largest tile file, in bytes.
pub const MAX_TILE_BYTES: u64 = 1 << 20;
/// The longest tile side, in pixels.
pub const MAX_TILE_SIDE: u32 = 1024;
/// The most tiles one request covers (a pane on a large screen shows a few dozen).
pub const MAX_REQUEST: usize = 256;
/// The cache's size: past it, the oldest files go until it is at [`PRUNE_TO`].
pub const MAX_CACHE_BYTES: u64 = 200_000_000;
/// What a pruning leaves, at most.
pub const PRUNE_TO: u64 = MAX_CACHE_BYTES / 5 * 4;
/// A cached tile or search reply older than this is asked for again.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// The cache is pruned again once this much has been added since the last pruning.
const PRUNE_EVERY: u64 = 16 << 20;
/// Placeholders remembered per session (a tile that failed is not asked for again until the pane
/// reopens).
const MAX_FAILED: usize = 4096;
/// The most cache files one pruning looks at, per folder and in folders.
const MAX_LISTED: usize = 200_000;
/// Between two looks at the results while waiting.
const WAIT_STEP: Duration = Duration::from_millis(2);
/// The folder of the cache, inside Dalica's cache folder.
pub const CACHE_FOLDER: &str = "tiles";

/// A transport the session's network jobs share.
pub type SharedTransport = Arc<dyn Transport + Send + Sync>;

/// One tile handed over: its picture, or why it is a placeholder.
#[derive(Clone, Debug)]
pub struct MapTile {
    pub tile: Tile,
    pub image: Result<Arc<Rgba8>, String>,
}

/// Counters of the tiles' work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapTileStats {
    /// Tiles asked for (a tile already on its way not counted again).
    pub requested: u64,
    /// Tiles that came over the network.
    pub fetched: u64,
    /// Tiles read from the cache, with no request.
    pub cached: u64,
    /// Placeholders handed over.
    pub failed: u64,
    /// Tiles cancelled, or finished after the pane closed or the server changed.
    pub dropped: u64,
}

/// Where the cache is: a folder, or a temporary one made by the first job that needs it.
#[derive(Clone, Debug)]
pub(crate) enum CacheRoot {
    At(PathBuf),
    Temp(Arc<OnceLock<Result<dalica_fs::TempDir, String>>>),
}

impl CacheRoot {
    /// The folder (made on a worker for a temporary one).
    pub(crate) fn path(&self) -> Result<PathBuf, String> {
        match self {
            CacheRoot::At(p) => Ok(p.clone()),
            CacheRoot::Temp(cell) => cell
                .get_or_init(|| dalica_fs::TempDir::new("dalica-tiles").map_err(|e| e.to_string()))
                .as_ref()
                .map(|t| t.path().to_path_buf())
                .map_err(Clone::clone),
        }
    }
}

/// The cache's upkeep, shared by the jobs: pruned by the first job, then every [`PRUNE_EVERY`]
/// bytes added.
#[derive(Debug, Default)]
pub(crate) struct Upkeep {
    pruned_once: AtomicBool,
    added: AtomicU64,
    running: AtomicBool,
}

impl Upkeep {
    /// Notes `bytes` added to the cache at `root` and prunes it when due. On a worker.
    pub(crate) fn after_adding(&self, root: &Path, bytes: u64) {
        self.added.fetch_add(bytes, Ordering::Relaxed);
        self.prune_if_due(root);
    }

    /// Prunes the cache at `root` if it has not been pruned yet or enough was added since.
    pub(crate) fn prune_if_due(&self, root: &Path) {
        let due = !self.pruned_once.load(Ordering::Relaxed) || self.added.load(Ordering::Relaxed) >= PRUNE_EVERY;
        if !due || self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        self.added.store(0, Ordering::Relaxed);
        self.pruned_once.store(true, Ordering::Relaxed);
        if let Err(why) = prune_cache(root, MAX_CACHE_BYTES) {
            log::warn!("map cache: {why}");
        }
        self.running.store(false, Ordering::Release);
    }
}

/// What a job needs for the network: owned, so the worker reads nothing of the session's.
#[derive(Clone)]
pub(crate) struct Net {
    pub(crate) transport: SharedTransport,
    pub(crate) cache: CacheRoot,
    pub(crate) upkeep: Arc<Upkeep>,
}

/// A tile asked for and not yet handed over.
#[derive(Clone, Debug)]
struct Asked {
    tile: Tile,
    /// The tile server's address template it was asked from.
    server: Arc<str>,
}

/// What a tile job answers.
struct Done {
    image: Result<Rgba8, String>,
    /// Whether it came over the network (else from the cache, or not at all).
    fetched: bool,
    cached: bool,
}

/// The map tiles' state.
#[derive(Default)]
pub struct MapTiles {
    places_open: bool,
    transport: Option<SharedTransport>,
    /// The HTTPS transport made for the proxy setting it was made for.
    https: Option<(String, SharedTransport)>,
    cache_dir: Option<PathBuf>,
    temp: Option<CacheRoot>,
    upkeep: Arc<Upkeep>,
    pool: Option<Pool<u64, Done>>,
    next: u64,
    outstanding: HashMap<u64, Asked>,
    asked: HashSet<(Tile, Arc<str>)>,
    failed: HashMap<(Tile, Arc<str>), String>,
    ready: Vec<MapTile>,
    stats: MapTileStats,
    /// The place search's state (`geocode.rs`).
    pub(crate) search: crate::geocode::Geocode,
}

impl std::fmt::Debug for MapTiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapTiles")
            .field("places_open", &self.places_open)
            .field("pending", &self.outstanding.len())
            .field("ready", &self.ready.len())
            .finish_non_exhaustive()
    }
}

impl MapTiles {
    /// Whether the window shows the Places pane.
    pub fn places_open(&self) -> bool {
        self.places_open
    }

    /// Tiles asked for and not yet handed over.
    pub fn pending(&self) -> usize {
        self.outstanding.len()
    }

    pub fn stats(&self) -> MapTileStats {
        self.stats
    }

    /// Cancels every tile on its way and drops those not taken.
    fn close(&mut self) {
        if let Some(pool) = &self.pool {
            for key in self.outstanding.keys() {
                pool.cancel(key);
            }
        }
        let gone = self.outstanding.len().saturating_add(self.ready.len());
        self.stats.dropped = self.stats.dropped.saturating_add(gone as u64);
        self.outstanding.clear();
        self.asked.clear();
        self.ready.clear();
    }
}

/// 16 lower-case hex digits naming `text` (FNV-1a, 64 bits): a cache folder or file name, stable
/// across runs and platforms.
pub(crate) fn key16(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Whether `name` is a cache folder's name: 16 lower-case hex digits.
fn folder_shape(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `name` is a tile file's name: `<zoom>-<x>-<y>.tile`.
fn tile_shape(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tile") else { return false };
    let parts: Vec<&str> = stem.split('-').collect();
    parts.len() == 3 && parts.iter().all(|p| (1..=7).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `name` is a search reply's name: `q-<16 hex digits>.json`.
fn reply_shape(name: &str) -> bool {
    name.strip_prefix("q-").and_then(|n| n.strip_suffix(".json")).is_some_and(folder_shape)
}

/// Whether `name` is one of the cache's own files: a tile, a search reply, or a partial download
/// of either.
pub fn own_shape(name: &str) -> bool {
    let whole = name.strip_suffix(dalica_fetch::spec::PART_SUFFIX).unwrap_or(name);
    tile_shape(whole) || reply_shape(whole)
}

/// The tile file's name.
fn tile_name(t: Tile) -> String {
    format!("{}-{}-{}.tile", t.zoom, t.x, t.y)
}

/// The tile's address: the server's template with `{z}`, `{x}` and `{y}` filled in.
pub fn tile_url(template: &str, t: Tile) -> Result<String, String> {
    if !(template.contains("{z}") && template.contains("{x}") && template.contains("{y}")) {
        return Err("the tile server's address has no {z}, {x} or {y}".into());
    }
    let url = template.replace("{z}", &t.zoom.to_string()).replace("{x}", &t.x.to_string()).replace("{y}", &t.y.to_string());
    dalica_fetch::Url::parse(&url).map_err(|e| format!("the tile server's address cannot be used: {e}"))?;
    Ok(url)
}

/// What a pruning did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// The cache's own files before, and their bytes.
    pub files: usize,
    pub bytes: u64,
    /// Files deleted, and their bytes.
    pub removed: usize,
    pub removed_bytes: u64,
}

/// Keeps the cache at `root` under `budget` bytes: when its own files (see [`own_shape`], inside
/// folders of 16 hex digits) come to more, the oldest go until they come to at most four fifths
/// of it. Every other file and folder is left alone, links are never followed. Runs on a worker.
pub fn prune_cache(root: &Path, budget: u64) -> Result<Pruned, String> {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Pruned::default()),
        Err(e) => return Err(format!("{}: {e}", root.display())),
    };
    for entry in entries.take(MAX_LISTED) {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        // `DirEntry::file_type` does not follow links.
        if !folder_shape(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(inner) = std::fs::read_dir(entry.path()) else { continue };
        for f in inner.take(MAX_LISTED) {
            let Ok(f) = f else { continue };
            if !f.file_name().to_str().is_some_and(own_shape) {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            if meta.file_type().is_file() {
                files.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len(), f.path()));
            }
            if files.len() >= MAX_LISTED {
                break;
            }
        }
    }
    let bytes = files.iter().fold(0u64, |a, f| a.saturating_add(f.1));
    let mut done = Pruned { files: files.len(), bytes, ..Pruned::default() };
    if bytes <= budget {
        return Ok(done);
    }
    let target = budget / 5 * 4;
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
    let mut left = bytes;
    for (_, len, path) in files {
        if left <= target {
            break;
        }
        match dalica_fs::remove_shaped(&path, own_shape) {
            Ok(_) => {
                left = left.saturating_sub(len);
                done.removed += 1;
                done.removed_bytes = done.removed_bytes.saturating_add(len);
            }
            Err(e) => log::debug!("map cache: {e}"),
        }
    }
    Ok(done)
}

/// How a cache file stands.
pub(crate) enum Cached {
    /// Younger than [`MAX_AGE`].
    Fresh,
    /// Old: asked for again.
    Stale,
    Missing,
}

/// Whether the plain file at `path` is there and fresh (a link or a folder counts as missing).
pub(crate) fn cached(path: &Path) -> Cached {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => {
            let age = m.modified().ok().and_then(|t| SystemTime::now().duration_since(t).ok()).unwrap_or_default();
            if age < MAX_AGE { Cached::Fresh } else { Cached::Stale }
        }
        _ => Cached::Missing,
    }
}

/// The bytes of the plain file at `path`, at most `limit` (more is refused).
pub(crate) fn read_capped(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("larger than {limit} bytes"));
    }
    Ok(bytes)
}

/// Downloads `url` into `dir` as `name` (at most `cap` bytes, in at most `attempts` requests), the
/// user having allowed it. On a worker. Returns the file's size.
pub(crate) fn fetch_into(url: &str, dir: &Path, name: &str, cap: u64, attempts: u32, net: &Net, cancel: &AtomicBool) -> Result<u64, String> {
    let spec = FileSpec::new(url, name, cap, None, None).map_err(|e| e.to_string())?;
    // The consent is the user's Allow (checked by the owner before the job was submitted).
    let consent = Consent::acknowledged(&spec, true).map_err(|e| e.to_string())?;
    let options = dalica_fetch::Options { attempts, ..dalica_fetch::Options::default() };
    match dalica_fetch::download(&spec, &consent, dir, &*net.transport, &options, cancel, &mut |_| {}) {
        Ok(d) => Ok(d.size),
        Err(FetchError::TooLarge { .. }) => Err(format!("larger than {cap} bytes")),
        Err(e) => Err(e.to_string()),
    }
}

/// The decode limits of a tile: the raster limits, no side past [`MAX_TILE_SIDE`].
fn tile_limits() -> Limits {
    Limits { max_side: MAX_TILE_SIDE, max_pixels: u64::from(MAX_TILE_SIDE) * u64::from(MAX_TILE_SIDE), ..Limits::default() }
}

/// Decodes a tile's bytes: its header read first, a side past [`MAX_TILE_SIDE`] refused before
/// any pixel is.
pub fn decode_tile(bytes: &[u8]) -> Result<Rgba8, String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_TILE_BYTES {
        return Err(format!("the tile is larger than {MAX_TILE_BYTES} bytes"));
    }
    let limits = tile_limits();
    let header = dalica_image::read_header(bytes, &limits).map_err(|e| format!("the tile cannot be read: {e}"))?;
    if header.width > MAX_TILE_SIDE || header.height > MAX_TILE_SIDE {
        return Err(format!("the tile is larger than {MAX_TILE_SIDE} pixels"));
    }
    dalica_image::decode(bytes, &limits).map_err(|e| format!("the tile cannot be decoded: {e}"))
}

/// The tile job: from the cache when it has the tile fresh, else over the network into the cache;
/// then decoded. A tile that cannot be decoded leaves the cache.
fn get_tile(url: &str, folder: &str, name: &str, net: &Net, cancel: &AtomicBool) -> Done {
    let failed = |why: String| Done { image: Err(why), fetched: false, cached: false };
    let root = match net.cache.path() {
        Ok(r) => r,
        Err(why) => return failed(format!("no cache folder: {why}")),
    };
    net.upkeep.prune_if_due(&root);
    let dir = root.join(folder);
    let path = dir.join(name);
    let (mut fetched, mut was_cached) = (false, false);
    match cached(&path) {
        Cached::Fresh => was_cached = true,
        Cached::Stale => {
            if let Err(e) = dalica_fs::remove_shaped(&path, own_shape) {
                return failed(e.to_string());
            }
        }
        Cached::Missing => {}
    }
    if !was_cached {
        match fetch_into(url, &dir, name, MAX_TILE_BYTES, 2, net, cancel) {
            Ok(size) => {
                fetched = true;
                net.upkeep.after_adding(&root, size);
            }
            Err(why) => return Done { image: Err(why), fetched: false, cached: false },
        }
    }
    let image = read_capped(&path, MAX_TILE_BYTES).map_err(|e| format!("the tile is {e}")).and_then(|bytes| decode_tile(&bytes));
    if image.is_err() {
        // Never kept: the next time the pane opens it is asked for again.
        let _ = dalica_fs::remove_shaped(&path, own_shape);
    }
    Done { image, fetched, cached: was_cached }
}

impl Session {
    /// The window says whether the Places pane is open: tiles are fetched only while it is (and
    /// a place search is made only from it). Closing it cancels the tiles on their way and the
    /// search; opening it again tries the tiles that failed before.
    pub fn set_places_open(&mut self, open: bool) {
        if self.map_tiles.places_open == open {
            return;
        }
        self.map_tiles.places_open = open;
        if open {
            self.map_tiles.failed.clear();
        } else {
            self.map_tiles.close();
            self.cancel_place_search();
        }
    }

    /// The map tiles' state.
    pub fn map_tiles(&self) -> &MapTiles {
        &self.map_tiles
    }

    /// Whether the user allowed map tiles and place searches (Options ▸ Network ▸ Allow map tiles).
    pub fn map_tiles_allowed(&self) -> bool {
        self.settings().network().allow_map_tiles
    }

    /// Whether tiles may be fetched now: allowed, and the Places pane open.
    fn tiles_on(&self) -> bool {
        self.map_tiles.places_open && self.map_tiles_allowed()
    }

    /// Uses `transport` for map tiles and place searches from now on instead of the HTTPS one
    /// (tests: an in-memory or counting transport; `None` goes back to HTTPS).
    pub fn set_network_transport(&mut self, transport: Option<SharedTransport>) {
        self.map_tiles.transport = transport;
    }

    /// Keeps the map cache (`tiles`) in `dir` instead of beside the thumbnail cache.
    pub fn set_map_cache_dir(&mut self, dir: Option<PathBuf>) {
        self.map_tiles.cache_dir = dir;
    }

    /// The map cache's folder: the one set, else `tiles` beside the thumbnail cache when that is
    /// Dalica's (`<cache>/thumbs`), else inside it; with no thumbnail cache, a temporary folder.
    /// Touches no disk.
    fn map_cache(&mut self) -> CacheRoot {
        if let Some(dir) = &self.map_tiles.cache_dir {
            return CacheRoot::At(dir.clone());
        }
        if let Some(thumbs) = self.thumbs.cache_dir() {
            let beside = thumbs.file_name().is_some_and(|n| n == "thumbs");
            match thumbs.parent() {
                Some(parent) if beside => return CacheRoot::At(parent.join(CACHE_FOLDER)),
                _ => return CacheRoot::At(thumbs.join(CACHE_FOLDER)),
            }
        }
        self.map_tiles.temp.get_or_insert_with(|| CacheRoot::Temp(Arc::new(OnceLock::new()))).clone()
    }

    /// The transport: the one given, else the HTTPS one for the proxy setting (made once per
    /// setting; no connection is made here).
    fn network_transport(&mut self, n: &NetworkSettings) -> Result<SharedTransport, String> {
        if let Some(t) = &self.map_tiles.transport {
            return Ok(t.clone());
        }
        let key = format!("{}|{}|{}", n.proxy.id(), n.proxy_host, n.proxy_port);
        if let Some((k, t)) = &self.map_tiles.https
            && *k == key
        {
            return Ok(t.clone());
        }
        let proxy = match n.proxy {
            ProxyMode::System => Proxy::system(),
            ProxyMode::Manual => Proxy::manual(&n.proxy_host, n.proxy_port),
        }
        .map_err(|e| e.to_string())?;
        let t: SharedTransport = Arc::new(HttpsTransport::new(proxy).map_err(|e| e.to_string())?);
        self.map_tiles.https = Some((key, t.clone()));
        Ok(t)
    }

    /// What a network job needs (`geocode.rs` too).
    pub(crate) fn network_job(&mut self, n: &NetworkSettings) -> Result<Net, String> {
        let transport = self.network_transport(n)?;
        let cache = self.map_cache();
        Ok(Net { transport, cache, upkeep: self.map_tiles.upkeep.clone() })
    }

    /// Asks for the tiles covering `area` at `zoom` (the Places pane's view; at most
    /// [`MAX_REQUEST`]), fetched on the workers; [`Session::take_map_tiles`] hands them over.
    /// Nothing is asked with tiles off or the pane closed (`Ok(0)`). Tiles of an earlier view not
    /// in this one are cancelled if they have not started; a tile on its way is not asked twice;
    /// one that failed since the pane opened is handed over as its placeholder again. Returns how
    /// many tiles were asked for. Never touches the disk.
    pub fn request_map_tiles(&mut self, area: GeoBox, zoom: u8) -> Result<usize, CmdError> {
        if zoom > MAX_ZOOM {
            return Err(CmdError::BadParams(format!("the zoom is 0 to {MAX_ZOOM}")));
        }
        if !self.tiles_on() {
            return Ok(0);
        }
        let tiles = tiles_covering(area, zoom).map_err(|e| CmdError::Limit(format!("the map view covers {e}")))?;
        if tiles.len() > MAX_REQUEST {
            return Err(CmdError::Limit(format!("at most {MAX_REQUEST} map tiles at once")));
        }
        let n = self.settings().network();
        let server: Arc<str> = Arc::from(n.tile_server.as_str());
        // The last view's tiles that this one does not show.
        let wanted: HashSet<Tile> = tiles.iter().copied().collect();
        let tiles_state = &mut self.map_tiles;
        let gone: Vec<u64> =
            tiles_state.outstanding.iter().filter(|(_, a)| !wanted.contains(&a.tile) || a.server != server).map(|(k, _)| *k).collect();
        for key in gone {
            if let Some(a) = tiles_state.outstanding.remove(&key) {
                tiles_state.asked.remove(&(a.tile, a.server));
                if let Some(pool) = &tiles_state.pool {
                    pool.cancel(&key);
                }
                tiles_state.stats.dropped = tiles_state.stats.dropped.saturating_add(1);
            }
        }
        let net = match self.network_job(&n) {
            Ok(net) => net,
            Err(why) => {
                self.placeholders(&tiles, &server, &why);
                return Ok(0);
            }
        };
        let folder = key16(&server);
        let mut asked = 0;
        for tile in tiles {
            let id = (tile, server.clone());
            if self.map_tiles.asked.contains(&id) {
                continue;
            }
            if let Some(why) = self.map_tiles.failed.get(&id).cloned() {
                self.map_tiles.ready.push(MapTile { tile, image: Err(why) });
                continue;
            }
            let url = match tile_url(&server, tile) {
                Ok(u) => u,
                Err(why) => {
                    self.placeholders(&[tile], &server, &why);
                    continue;
                }
            };
            let m = &mut self.map_tiles;
            m.next = m.next.wrapping_add(1);
            let key = m.next;
            m.asked.insert(id);
            m.outstanding.insert(key, Asked { tile, server: server.clone() });
            m.stats.requested = m.stats.requested.saturating_add(1);
            let (net, folder, name) = (net.clone(), folder.clone(), tile_name(tile));
            let pool = m.pool.get_or_insert_with(|| Pool::new(WORKERS));
            pool.submit(key, Priority::VISIBLE, move |cancel| {
                catch_unwind(AssertUnwindSafe(|| get_tile(&url, &folder, &name, &net, cancel))).unwrap_or_else(|_| Done {
                    image: Err("fetching the tile panicked".into()),
                    fetched: false,
                    cached: false,
                })
            });
            asked += 1;
        }
        Ok(asked)
    }

    /// Hands `tiles` over as placeholders for `why`, remembered until the pane reopens.
    fn placeholders(&mut self, tiles: &[Tile], server: &Arc<str>, why: &str) {
        let m = &mut self.map_tiles;
        for &tile in tiles {
            if m.failed.len() < MAX_FAILED {
                m.failed.insert((tile, server.clone()), why.to_string());
            }
            m.ready.push(MapTile { tile, image: Err(why.to_string()) });
            m.stats.failed = m.stats.failed.saturating_add(1);
        }
    }

    /// Takes in what the workers finished.
    fn poll_map_tiles(&mut self) {
        let server: Arc<str> = Arc::from(self.settings().network().tile_server.as_str());
        let m = &mut self.map_tiles;
        while let Some((key, done)) = m.pool.as_ref().and_then(Pool::try_recv) {
            let Some(asked) = m.outstanding.remove(&key) else { continue };
            m.asked.remove(&(asked.tile, asked.server.clone()));
            if asked.server != server {
                m.stats.dropped = m.stats.dropped.saturating_add(1);
                continue;
            }
            if done.fetched {
                m.stats.fetched = m.stats.fetched.saturating_add(1);
            }
            if done.cached {
                m.stats.cached = m.stats.cached.saturating_add(1);
            }
            match done.image {
                Ok(image) => m.ready.push(MapTile { tile: asked.tile, image: Ok(Arc::new(image)) }),
                Err(why) => {
                    log::debug!("map tile {}/{}/{}: {why}", asked.tile.zoom, asked.tile.x, asked.tile.y);
                    if m.failed.len() < MAX_FAILED {
                        m.failed.insert((asked.tile, asked.server), why.clone());
                    }
                    m.stats.failed = m.stats.failed.saturating_add(1);
                    m.ready.push(MapTile { tile: asked.tile, image: Err(why) });
                }
            }
        }
    }

    /// The tiles finished since the last call, in the order they finished. The open Places pane
    /// calls it each frame; with tiles turned off (or the pane closed) it cancels what is on its
    /// way and hands over nothing.
    pub fn take_map_tiles(&mut self) -> Vec<MapTile> {
        if !self.tiles_on() {
            if !self.map_tiles.outstanding.is_empty() || !self.map_tiles.ready.is_empty() {
                self.map_tiles.close();
            }
            return Vec::new();
        }
        self.poll_map_tiles();
        std::mem::take(&mut self.map_tiles.ready)
    }

    /// Waits until every tile asked for has been fetched (or could not be) or `timeout` has
    /// passed, keeping the tiles for [`Session::take_map_tiles`]; returns whether it got there.
    /// Blocks the calling thread (CLI, tests).
    pub fn wait_map_tiles(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll_map_tiles();
            if self.map_tiles.outstanding.is_empty() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }
}

/// Locks `m`, whatever a panicked holder left (the data is a time).
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_says_whether_the_places_pane_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = crate::cmd::testing::open(dir.path());
        assert!(!s.map_tiles().places_open(), "closed until the window says otherwise");
        s.set_places_open(true);
        assert!(s.map_tiles().places_open());
        s.set_places_open(false);
        assert!(!s.map_tiles().places_open());
    }

    #[test]
    fn cache_names_have_their_own_shapes() {
        assert!(tile_shape("3-4-5.tile") && tile_shape("19-524287-524287.tile"));
        assert!(own_shape("3-4-5.tile.part") && own_shape("q-0123456789abcdef.json"));
        for bad in ["3-4.tile", "3-4-5.png", "-4-5.tile", "a-4-5.tile", "3-4-12345678.tile", "q-0123.json", "q-0123456789ABCDEF.json", "notes.txt"] {
            assert!(!own_shape(bad), "{bad}");
        }
        assert!(folder_shape(&key16("https://tile.openstreetmap.org/{z}/{x}/{y}.png")));
        assert_eq!(key16(""), "cbf29ce484222325", "FNV-1a's offset basis");
        assert_ne!(key16("a"), key16("b"));
    }

    #[test]
    fn tile_addresses_fill_the_template() {
        let t = Tile::new(3, 4, 5).unwrap();
        assert_eq!(tile_url(DEFAULT_TILE_SERVER, t).unwrap(), "https://tile.openstreetmap.org/3/4/5.png");
        assert_eq!(tile_url("https://maps.example/t?z={z}&x={x}&y={y}", t).unwrap(), "https://maps.example/t?z=3&x=4&y=5");
        for bad in ["http://plain.example/{z}/{x}/{y}.png", "https://a.example/{z}/{x}.png", "", "https://a b/{z}/{x}/{y}"] {
            assert!(tile_url(bad, t).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_tile_past_its_limits_is_refused_before_it_is_decoded() {
        let small = dalica_scenes::fingerprint::png_rgba(4, 4, &[200; 64]);
        assert_eq!(decode_tile(&small).map(|i| (i.width, i.height)), Ok((4, 4)));
        let wide = dalica_scenes::fingerprint::png_rgba(MAX_TILE_SIDE + 1, 1, &vec![0; (MAX_TILE_SIDE as usize + 1) * 4]);
        assert!(decode_tile(&wide).unwrap_err().contains("pixels"));
        assert!(decode_tile(b"<html>not a picture</html>").is_err());
        assert!(decode_tile(&vec![0; MAX_TILE_BYTES as usize + 1]).unwrap_err().contains("bytes"));
    }
}
