//! Map tiles and the geocoding service (`organizing.md` §7, `security.md` §4.2, `roadmap.md` M13 →
//! *The network*): tiles are fetched only once Allow map tiles is on and only while the Places pane
//! is open ([`Session::set_places_open`]), a place search only when asked. A stub until M13.20
//! fills it.
//!
//! Which provider, its attribution and terms, and the geocoding service wait on reserved decisions
//! 4 and 9 (`roadmap.md` → *Reserved decisions*). Until they are recorded the Network tab's rows
//! are unavailable with the reasons below, and the defaults are the spec's (OpenStreetMap's tiles,
//! `menus-and-shortcuts.md` §5); the task that builds tiles (M13.20) sets them by the decision and
//! makes the rows available. The settings themselves always read through
//! [`crate::settings::Settings::network`], whatever the rows say.

use crate::session::Session;

/// Why the map rows of Options ▸ Network (allow map tiles, the tile server, its attribution, the
/// geocoding service) cannot be changed in this build, `None` once they can.
pub const TILES_UNAVAILABLE: Option<&str> = Some("Arrives once a map tile provider is chosen");
/// Why the proxy rows cannot be changed in this build, `None` once they can.
pub const PROXY_UNAVAILABLE: Option<&str> = Some("Arrives with map tiles and model downloads");
/// The tile server's address template by default: `{z}`, `{x}` and `{y}` are the tile's zoom and
/// column and row.
pub const DEFAULT_TILE_SERVER: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// The attribution drawn under the default server's tiles.
pub const DEFAULT_ATTRIBUTION: &str = "© OpenStreetMap contributors";
/// The geocoding service's address template by default: `{query}` is the text searched for.
pub const DEFAULT_GEOCODER: &str = "https://nominatim.openstreetmap.org/search?format=jsonv2&q={query}";

/// The map tiles' state.
#[derive(Debug, Default)]
pub struct MapTiles {
    places_open: bool,
}

impl MapTiles {
    /// Whether the window shows the Places pane.
    pub fn places_open(&self) -> bool {
        self.places_open
    }
}

impl Session {
    /// The window says whether the Places pane is open: tiles are fetched only while it is.
    pub fn set_places_open(&mut self, open: bool) {
        self.map_tiles.places_open = open;
    }

    /// The map tiles' state.
    pub fn map_tiles(&self) -> &MapTiles {
        &self.map_tiles
    }
}

#[cfg(test)]
mod tests {
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
}
