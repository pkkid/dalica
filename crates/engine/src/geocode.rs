//! Searching for a place (`organizing.md` §7, `security.md` §4.2, reserved decision 4): the text
//! typed in "Search for a place:" as typed coordinates, found here without the network
//! ([`dalica_core::geo::LatLon::parse`], `cmd/map.rs`), or as a place name for the geocoding
//! service (Nominatim by default, [`crate::map_tiles::DEFAULT_GEOCODER`]).
//!
//! - **Only when asked.** A name is looked up only by [`Session::search_place`] (`places.search`
//!   on Enter), only once Allow map tiles is on and only from the open Places pane; nothing is
//!   looked up at start-up, during scans or as the user types.
//! - **Nominatim's usage policy:** at most one request a second from this program ([`MIN_INTERVAL`],
//!   for every session of the process), the Dalica User-Agent, and every reply cached (in memory
//!   for the session, and on disk in the map cache as `q-<16 hex digits>.json` for
//!   [`crate::map_tiles::MAX_AGE`]), so the same search is answered without a request.
//! - **Hostile replies** (`security.md` §4.2): at most [`MAX_REPLY_BYTES`] (more is refused as it
//!   arrives), nesting at most [`MAX_DEPTH`] deep (checked before the JSON is parsed), at most
//!   [`MAX_RESULTS`] results, each with a name and a place cut to [`MAX_NAME_BYTES`] and
//!   [`MAX_PLACE_BYTES`] and a point on the globe; an entry without one is passed over. A reply
//!   that is not that is an error, never a panic. Result text is never logged (`security.md` §4.4).
//!
//! One search at a time: a new one cancels the one on its way. [`Session::take_place_search`]
//! hands over the last one finished.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dalica_core::geo::LatLon;
use dalica_thumbs::{Pool, Priority};
use serde_json::Value;

use crate::error::CmdError;
use crate::map_tiles::{Cached, Net, cached, fetch_into, key16, lock, own_shape, read_capped};
use crate::session::Session;

/// The largest geocoder reply, in bytes.
pub const MAX_REPLY_BYTES: u64 = 1 << 20;
/// The deepest a reply's arrays and objects nest.
pub const MAX_DEPTH: usize = 32;
/// The most results kept from one reply.
pub const MAX_RESULTS: usize = 20;
/// The longest text searched for, in bytes.
pub const MAX_QUERY_BYTES: usize = 512;
/// The longest name and place of a result kept, in bytes (cut at a character boundary).
pub const MAX_NAME_BYTES: usize = 256;
pub const MAX_PLACE_BYTES: usize = 1024;
/// The shortest time between two requests to the geocoding service.
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
/// Searches remembered in memory.
const MEMORY: usize = 32;
/// Between two looks at the result while waiting, and between two looks at the cancel flag while
/// a request waits for its turn.
const WAIT_STEP: Duration = Duration::from_millis(5);

/// The process's turn at the geocoding service: whether a request is on its way, and when the
/// next one may go ([`MIN_INTERVAL`] after the last one ended).
struct Slot {
    in_flight: bool,
    next: Option<Instant>,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot { in_flight: false, next: None });

/// A turn at the geocoding service, held while its request is made. Dropping it (the request
/// ended, failed or was abandoned) lets the next request go [`MIN_INTERVAL`] later. The interval
/// is counted from the end of a request, not from when its turn was given, so the time between
/// being given the turn and the request leaving (or reaching the server) never shortens the gap
/// between two arrivals.
struct Turn;

impl Drop for Turn {
    fn drop(&mut self) {
        let mut slot = lock(&SLOT);
        slot.in_flight = false;
        slot.next = Some(Instant::now() + MIN_INTERVAL);
    }
}

/// One place found.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Its own name (`Lisboa`).
    pub name: String,
    /// Where it is, in full (`Lisboa, Portugal`).
    pub place: String,
    pub point: LatLon,
}

impl Place {
    /// `{name, place, lat, lon}`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({ "name": self.name, "place": self.place, "lat": self.point.lat(), "lon": self.point.lon() })
    }
}

/// A search finished.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaceSearch {
    /// The id `places.search` answered.
    pub id: u64,
    pub text: String,
    /// The places found (perhaps none), or why the service could not be asked or understood.
    pub result: Result<Vec<Place>, String>,
}

/// What [`Session::search_place`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Search {
    /// Answered at once, from this session's memory.
    Found(Vec<Place>),
    /// Asked on a worker; [`Session::take_place_search`] hands it over with this id.
    Pending(u64),
}

/// Counters of the searches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Searches that went to the service.
    pub requested: u64,
    /// Searches answered from memory or the disk cache.
    pub cached: u64,
    pub failed: u64,
}

/// What a search job answers.
struct Done {
    result: Result<Vec<Place>, String>,
    fetched: bool,
}

/// The place search's state.
#[derive(Default)]
pub(crate) struct Geocode {
    pool: Option<Pool<u64, Done>>,
    next: u64,
    /// The search on its way: its id, text and address.
    pending: Option<(u64, String, String)>,
    done: Option<PlaceSearch>,
    /// Replies by address, the latest last.
    memory: VecDeque<(String, Vec<Place>)>,
    stats: SearchStats,
}

/// The service's address for `text`: the template with `{query}` replaced by `text`
/// percent-encoded (UTF-8; letters, digits and `-._~` kept).
pub fn query_url(template: &str, text: &str) -> Result<String, String> {
    if !template.contains("{query}") {
        return Err("the geocoding service's address has no {query}".into());
    }
    let mut q = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            q.push(char::from(b));
        } else {
            q.push_str(&format!("%{b:02X}"));
        }
    }
    let url = template.replace("{query}", &q);
    dalica_fetch::Url::parse(&url).map_err(|e| format!("the geocoding service's address cannot be used: {e}"))?;
    Ok(url)
}

/// How deep `bytes` nest their JSON arrays and objects (strings skipped), stopping past `limit`.
fn depth_within(bytes: &[u8], limit: usize) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &b in bytes {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return false;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}

/// At most `max` bytes of `text`, cut at a character boundary, trimmed.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().trim().to_string()
}

/// A coordinate given as a number or as a string of one.
fn coordinate(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) if s.len() <= 64 => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Reads a geocoder reply: a JSON array of results with `lat` and `lon` (numbers or strings of
/// them), `name` and `display_name` (Nominatim's `jsonv2`, which other services share). Results
/// without a point on the globe or any name are passed over; at most [`MAX_RESULTS`]. Anything
/// else (too large, too deep, not JSON, not an array) is an error.
pub fn parse_reply(bytes: &[u8]) -> Result<Vec<Place>, String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REPLY_BYTES {
        return Err(format!("the geocoding service's reply is larger than {MAX_REPLY_BYTES} bytes"));
    }
    if !depth_within(bytes, MAX_DEPTH) {
        return Err(format!("the geocoding service's reply nests deeper than {MAX_DEPTH}"));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "the geocoding service's reply is not JSON".to_string())?;
    let Value::Array(items) = value else { return Err("the geocoding service's reply is not a list of places".into()) };
    let mut out = Vec::new();
    for item in &items {
        if out.len() >= MAX_RESULTS {
            break;
        }
        let Value::Object(o) = item else { continue };
        let (Some(lat), Some(lon)) = (coordinate(o.get("lat")), coordinate(o.get("lon"))) else { continue };
        let Ok(point) = LatLon::new(lat, lon) else { continue };
        let text = |k: &str, max: usize| o.get(k).and_then(Value::as_str).map(|s| cut(s, max)).unwrap_or_default();
        let place = text("display_name", MAX_PLACE_BYTES);
        let mut name = text("name", MAX_NAME_BYTES);
        if name.is_empty() {
            name = cut(place.split(',').next().unwrap_or_default(), MAX_NAME_BYTES);
        }
        if name.is_empty() {
            continue;
        }
        out.push(Place { name, place, point });
    }
    Ok(out)
}

/// Waits for this process's next turn at the geocoding service: no other request on its way and
/// [`MIN_INTERVAL`] since the last one ended; `None` when cancelled while waiting.
fn wait_turn(cancel: &AtomicBool) -> Option<Turn> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let wait = {
            let mut slot = lock(&SLOT);
            let now = Instant::now();
            match slot.next {
                _ if slot.in_flight => WAIT_STEP,
                Some(next) if now < next => WAIT_STEP.min(next.saturating_duration_since(now)),
                _ => {
                    slot.in_flight = true;
                    return Some(Turn);
                }
            }
        };
        std::thread::sleep(wait);
    }
}

/// The search job: the disk cache's reply when it is fresh, else the service's (in its turn),
/// kept in the cache. A reply that cannot be read leaves the cache.
fn search(url: &str, folder: &str, net: &Net, cancel: &AtomicBool) -> Done {
    let failed = |why: String| Done { result: Err(why), fetched: false };
    let root = match net.cache.path() {
        Ok(r) => r,
        Err(why) => return failed(format!("no cache folder: {why}")),
    };
    net.upkeep.prune_if_due(&root);
    let dir = root.join(folder);
    let name = format!("q-{}.json", key16(url));
    let path = dir.join(&name);
    let mut fetched = false;
    match cached(&path) {
        Cached::Fresh => {}
        Cached::Stale | Cached::Missing => {
            if let Err(e) = dalica_fs::remove_shaped(&path, own_shape) {
                return failed(e.to_string());
            }
            let Some(turn) = wait_turn(cancel) else {
                return failed("cancelled".into());
            };
            // One attempt: a second would come sooner than the service's interval.
            let fetched_into = fetch_into(url, &dir, &name, MAX_REPLY_BYTES, 1, net, cancel);
            drop(turn);
            match fetched_into {
                Ok(size) => {
                    fetched = true;
                    net.upkeep.after_adding(&root, size);
                }
                Err(why) => return Done { result: Err(format!("the geocoding service could not be asked: {why}")), fetched: true },
            }
        }
    }
    let result = read_capped(&path, MAX_REPLY_BYTES).map_err(|e| format!("the geocoding service's reply is {e}")).and_then(|b| parse_reply(&b));
    if result.is_err() {
        let _ = dalica_fs::remove_shaped(&path, own_shape);
    }
    Done { result, fetched }
}

impl Session {
    /// Looks `text` (a place name) up with the geocoding service of Options ▸ Network: answered at
    /// once when this session searched for it before, else on a worker
    /// ([`Session::take_place_search`]). Refused (`disabled`, nothing asked) unless map tiles are
    /// allowed and the Places pane is open. A new search cancels the one on its way. Never touches
    /// the disk.
    pub fn search_place(&mut self, text: &str) -> Result<Search, CmdError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CmdError::BadParams("text is empty".into()));
        }
        if text.len() > MAX_QUERY_BYTES {
            return Err(CmdError::Limit(format!("a place to search for is at most {MAX_QUERY_BYTES} bytes")));
        }
        if !self.map_tiles_allowed() {
            return Err(CmdError::Disabled(
                "Searching for a place by name asks the geocoding service: allow map tiles in Options ▸ Network first".into(),
            ));
        }
        if !self.map_tiles().places_open() {
            return Err(CmdError::Disabled("Open the Places pane to search for a place by name".into()));
        }
        let n = self.settings().network();
        let url = query_url(&n.geocoder, text).map_err(CmdError::Disabled)?;
        self.cancel_place_search();
        let g = &mut self.map_tiles.search;
        if let Some((_, places)) = g.memory.iter().find(|(u, _)| *u == url) {
            g.stats.cached = g.stats.cached.saturating_add(1);
            return Ok(Search::Found(places.clone()));
        }
        let net = self.network_job(&n).map_err(CmdError::Disabled)?;
        let folder = key16(&n.geocoder);
        let g = &mut self.map_tiles.search;
        g.next = g.next.wrapping_add(1);
        let id = g.next;
        g.pending = Some((id, text.to_string(), url.clone()));
        let pool = g.pool.get_or_insert_with(|| Pool::new(1));
        pool.submit(id, Priority::VISIBLE, move |cancel| {
            catch_unwind(AssertUnwindSafe(|| search(&url, &folder, &net, cancel)))
                .unwrap_or_else(|_| Done { result: Err("the search panicked".into()), fetched: false })
        });
        Ok(Search::Pending(id))
    }

    /// Cancels the search on its way, if any.
    pub fn cancel_place_search(&mut self) {
        let g = &mut self.map_tiles.search;
        if let (Some((id, _, _)), Some(pool)) = (g.pending.take(), &g.pool) {
            pool.cancel(&id);
        }
    }

    /// Takes in what the worker finished.
    fn poll_place_search(&mut self) {
        let g = &mut self.map_tiles.search;
        while let Some((id, done)) = g.pool.as_ref().and_then(Pool::try_recv) {
            let Some((pending, text, url)) = g.pending.take() else { continue };
            if pending != id {
                g.pending = Some((pending, text, url));
                continue;
            }
            match &done.result {
                Ok(places) => {
                    if done.fetched {
                        g.stats.requested = g.stats.requested.saturating_add(1);
                    } else {
                        g.stats.cached = g.stats.cached.saturating_add(1);
                    }
                    if g.memory.len() >= MEMORY {
                        g.memory.pop_front();
                    }
                    g.memory.push_back((url, places.clone()));
                }
                Err(why) => {
                    if done.fetched {
                        g.stats.requested = g.stats.requested.saturating_add(1);
                    }
                    g.stats.failed = g.stats.failed.saturating_add(1);
                    // The reason only: never the text searched for.
                    log::debug!("place search {id}: {why}");
                }
            }
            g.done = Some(PlaceSearch { id, text, result: done.result });
        }
    }

    /// The search finished since the last call, if any (the Places pane lists its places).
    pub fn take_place_search(&mut self) -> Option<PlaceSearch> {
        self.poll_place_search();
        self.map_tiles.search.done.take()
    }

    /// Whether a search is on its way.
    pub fn place_search_pending(&self) -> bool {
        self.map_tiles.search.pending.is_some()
    }

    /// The searches' counters.
    pub fn place_search_stats(&self) -> SearchStats {
        self.map_tiles.search.stats
    }

    /// Waits until the search on its way has finished or `timeout` has passed, keeping it for
    /// [`Session::take_place_search`]; returns whether it got there. Blocks the calling thread
    /// (CLI, tests).
    pub fn wait_place_search(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll_place_search();
            if self.map_tiles.search.pending.is_none() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_is_percent_encoded_into_the_template() {
        let url = query_url(crate::map_tiles::DEFAULT_GEOCODER, "São Paulo & co/2").unwrap();
        assert_eq!(url, "https://nominatim.openstreetmap.org/search?format=jsonv2&q=S%C3%A3o%20Paulo%20%26%20co%2F2");
        assert!(query_url("https://geo.example/search", "x").is_err(), "no {{query}}");
        assert!(query_url("http://geo.example/?q={query}", "x").is_err(), "https only");
    }

    #[test]
    fn a_nominatim_reply_reads_as_places() {
        let reply = br#"[
            {"lat":"38.7077507","lon":"-9.1365919","name":"Lisboa","display_name":"Lisboa, Portugal","type":"city"},
            {"lat":38.5,"lon":-9.5,"name":"","display_name":"Cascais, Lisboa, Portugal"},
            {"lat":"91","lon":"0","name":"Off the globe"},
            {"lon":"0","name":"No latitude"},
            "not an object",
            {"lat":"1","lon":"2","name":"","display_name":""}
        ]"#;
        let places = parse_reply(reply).unwrap();
        let names: Vec<&str> = places.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Lisboa", "Cascais"]);
        assert_eq!(places[0].place, "Lisboa, Portugal");
        assert!((places[0].point.lat() - 38.707_750_7).abs() < 1e-9);
        assert_eq!(parse_reply(b"[]").unwrap(), Vec::new());
    }

    #[test]
    fn hostile_replies_are_refused() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(parse_reply(deep.as_bytes()).unwrap_err().contains("deeper"));
        let ok_depth = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse_reply(ok_depth.as_bytes()).is_ok());
        // Brackets inside strings do not count.
        let quoted = format!(r#"[{{"name":"{}","lat":1,"lon":2}}]"#, "[".repeat(100));
        assert_eq!(parse_reply(quoted.as_bytes()).unwrap().len(), 1);
        for bad in [&b"{\"lat\":1}"[..], b"not json", b"", b"[1,", b"\xff\xfe"] {
            assert!(parse_reply(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        assert!(parse_reply(&vec![b' '; MAX_REPLY_BYTES as usize + 1]).unwrap_err().contains("larger"));
        // Many results are cut, long names too, on a character boundary.
        let many: Vec<Value> = (0..100).map(|i| serde_json::json!({"lat": 1, "lon": i, "name": "é".repeat(400)})).collect();
        let places = parse_reply(serde_json::to_string(&many).unwrap().as_bytes()).unwrap();
        assert_eq!(places.len(), MAX_RESULTS);
        assert!(places.iter().all(|p| p.name.len() <= MAX_NAME_BYTES && p.name.chars().all(|c| c == 'é')));
    }
}
