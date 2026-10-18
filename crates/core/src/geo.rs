//! Places on the globe (`organizing.md` §7, the Places pane): a validated latitude and longitude
//! ([`LatLon`]), typed coordinates read and written in decimal degrees or in degrees, minutes and
//! seconds, the great-circle distance, and the Web Mercator maths of a tiled map (a point ↔ world
//! pixels at a zoom, the tile under a point, the tiles covering a box, across the antimeridian
//! too).
//!
//! - A [`LatLon`] is finite and on the globe: latitude −90…90, longitude −180…180. It keeps the
//!   precision it was given; [`LatLon::round6`] gives the six decimals a geotag is stored with
//!   (`data-model.md` §3).
//! - The map is Web Mercator: zoom 0…[`MAX_ZOOM`], [`TILE_SIZE`]-pixel tiles, the world
//!   `TILE_SIZE × 2^zoom` pixels wide and tall with `(0, 0)` at its north-west corner. Latitudes past
//!   ±[`MAX_LATITUDE`] are clamped to it (the projection's edge), longitudes wrap.
//! - Typed text is bounded ([`MAX_TEXT`] bytes) and anything that is not two coordinates is
//!   refused, never guessed at. Errors are this module's own ([`GeoError`]): nothing in them is
//!   the text typed.

use std::f64::consts::PI;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::geom::round6;

/// The highest map zoom.
pub const MAX_ZOOM: u8 = 19;
/// The side of a map tile in pixels.
pub const TILE_SIZE: u32 = 256;
/// The latitude where Web Mercator's square world ends (`atan(sinh(π))`, in degrees).
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;
/// The mean radius of the Earth in metres (IUGG), for [`LatLon::distance_m`].
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// The most bytes of typed coordinates [`LatLon::parse`] reads.
pub const MAX_TEXT: usize = 256;
/// The most tiles [`tiles_covering`] gives (a 2560 × 1600 screen needs about a hundred).
pub const MAX_TILES: usize = 4096;

/// Why a place, a box, a zoom or typed coordinates were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GeoError {
    /// A latitude outside −90…90 or not finite.
    #[error("a latitude outside -90..90 or not finite")]
    Latitude,
    /// A longitude outside −180…180 or not finite.
    #[error("a longitude outside -180..180 or not finite")]
    Longitude,
    /// A zoom past [`MAX_ZOOM`].
    #[error("a zoom past {MAX_ZOOM}")]
    Zoom,
    /// A tile outside its zoom's grid.
    #[error("a tile outside its zoom's grid")]
    Tile,
    /// A box whose south is north of its north.
    #[error("a box whose south lies north of its north")]
    Box,
    /// More tiles than [`MAX_TILES`].
    #[error("more than {MAX_TILES} tiles")]
    TooManyTiles,
    /// Typed text that is not two coordinates (or longer than [`MAX_TEXT`] bytes).
    #[error("not coordinates")]
    Text,
}

/// A point on the globe: latitude and longitude in degrees, finite, latitude −90…90 (north
/// positive), longitude −180…180 (east positive). Stored as `[lat, lon]` in JSON.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct LatLon {
    lat: f64,
    lon: f64,
}

// Both values are always finite (never NaN), so equality is reflexive.
impl Eq for LatLon {}

impl LatLon {
    /// The point at `lat`, `lon` (degrees), kept at the precision given.
    pub fn new(lat: f64, lon: f64) -> Result<LatLon, GeoError> {
        if !(lat.is_finite() && (-90.0..=90.0).contains(&lat)) {
            return Err(GeoError::Latitude);
        }
        if !(lon.is_finite() && (-180.0..=180.0).contains(&lon)) {
            return Err(GeoError::Longitude);
        }
        // `+ 0.0` turns −0 into 0, so the two zeros are one point.
        Ok(LatLon { lat: lat + 0.0, lon: lon + 0.0 })
    }

    pub fn lat(&self) -> f64 {
        self.lat
    }

    pub fn lon(&self) -> f64 {
        self.lon
    }

    /// The point with both values rounded to six decimals (about 11 cm), as a geotag is stored.
    /// Rounding never leaves the globe, so this is always a valid point.
    pub fn round6(&self) -> LatLon {
        LatLon { lat: round6(self.lat), lon: round6(self.lon) }
    }

    /// Reads typed coordinates: two numbers in decimal degrees (`38.7116, -9.1389`) or in degrees,
    /// minutes and seconds (`38°42'41.8"N 9°8'20"W`), latitude first unless the hemisphere letters
    /// say otherwise. Signs or the letters N, S, E and W (before or after) give the hemisphere;
    /// `°`, `'` or `′`, and `"` or `″` mark degrees, minutes and seconds, and without marks the
    /// numbers count as degrees, minutes, seconds in turn. A comma or semicolon may separate the
    /// two. Anything else, or more than [`MAX_TEXT`] bytes, is refused.
    pub fn parse(text: &str) -> Result<LatLon, GeoError> {
        if text.len() > MAX_TEXT {
            return Err(GeoError::Text);
        }
        let toks = lex(text)?;
        let (a, b) = split_halves(&toks)?;
        let (a, b) = (half(a)?, half(b)?);
        let (lat, lon) = match (a.axis, b.axis) {
            (Some(Axis::Lon), Some(Axis::Lat)) | (Some(Axis::Lon), None) | (None, Some(Axis::Lat)) => (b.value, a.value),
            (Some(Axis::Lat), Some(Axis::Lat)) | (Some(Axis::Lon), Some(Axis::Lon)) => return Err(GeoError::Text),
            _ => (a.value, b.value),
        };
        LatLon::new(lat, lon)
    }

    /// The point in decimal degrees with six decimals, latitude first: `38.711600, -9.138900`.
    /// [`LatLon::parse`] reads it back as [`LatLon::round6`].
    pub fn to_decimal(&self) -> String {
        let p = self.round6();
        format!("{:.6}, {:.6}", p.lat, p.lon)
    }

    /// The point in degrees, minutes and seconds with three decimals of a second (under 3 cm),
    /// latitude first: `38°42'41.760"N 9°8'20.040"W`. [`LatLon::parse`] reads it back to within
    /// half a thousandth of a second, so to [`LatLon::round6`] once rounded.
    pub fn to_dms(&self) -> String {
        format!("{} {}", dms(self.lat, ['N', 'S']), dms(self.lon, ['E', 'W']))
    }

    /// The great-circle distance to `other` in metres (haversine on a sphere of
    /// [`EARTH_RADIUS_M`]).
    pub fn distance_m(&self, other: &LatLon) -> f64 {
        let (p1, p2) = (self.lat.to_radians(), other.lat.to_radians());
        let dp = p2 - p1;
        let dl = (other.lon - self.lon).to_radians();
        let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_M * h.clamp(0.0, 1.0).sqrt().asin()
    }
}

impl TryFrom<[f64; 2]> for LatLon {
    type Error = GeoError;

    fn try_from([lat, lon]: [f64; 2]) -> Result<LatLon, GeoError> {
        LatLon::new(lat, lon)
    }
}

impl From<LatLon> for [f64; 2] {
    fn from(p: LatLon) -> [f64; 2] {
        [p.lat, p.lon]
    }
}

impl fmt::Display for LatLon {
    /// [`LatLon::to_decimal`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal())
    }
}

impl FromStr for LatLon {
    type Err = GeoError;

    fn from_str(s: &str) -> Result<LatLon, GeoError> {
        LatLon::parse(s)
    }
}

/// `v` in degrees, minutes and seconds with its hemisphere letter (`[positive, negative]`).
fn dms(v: f64, letters: [char; 2]) -> String {
    let [pos, neg] = letters;
    let letter = if v < 0.0 { neg } else { pos };
    // Thousandths of a second of arc, as a whole number, so a second never reads 60.
    let total = (v.abs() * 3_600_000.0).round() as u64;
    let (deg, rest) = (total / 3_600_000, total % 3_600_000);
    let (min, ms) = (rest / 60_000, rest % 60_000);
    format!("{deg}°{min}'{}.{:03}\"{letter}", ms / 1000, ms % 1000)
}

/// One piece of typed coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    /// A number and whether it carried a sign.
    Num(f64, bool),
    /// A unit mark: 0 degrees, 1 minutes, 2 seconds.
    Unit(u8),
    /// A hemisphere letter, upper case.
    Hemi(char),
    /// A comma or semicolon between the two coordinates.
    Sep,
}

fn lex(text: &str) -> Result<Vec<Tok>, GeoError> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ',' | ';' => out.push(Tok::Sep),
            '°' | 'º' | '˚' => out.push(Tok::Unit(0)),
            '\'' | '′' | '’' => {
                // Two apostrophes are a seconds mark.
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    out.push(Tok::Unit(2));
                } else {
                    out.push(Tok::Unit(1));
                }
            }
            '"' | '″' | '”' => out.push(Tok::Unit(2)),
            'N' | 'S' | 'E' | 'W' | 'n' | 's' | 'e' | 'w' => out.push(Tok::Hemi(c.to_ascii_uppercase())),
            '+' | '-' | '−' | '.' | '0'..='9' => {
                let mut number = String::new();
                let signed = matches!(c, '+' | '-' | '−');
                if c == '−' {
                    number.push('-');
                } else {
                    number.push(c);
                }
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        number.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let v: f64 = number.parse().map_err(|_| GeoError::Text)?;
                if !v.is_finite() {
                    return Err(GeoError::Text);
                }
                out.push(Tok::Num(v, signed));
            }
            _ => return Err(GeoError::Text),
        }
    }
    Ok(out)
}

/// The two coordinates' pieces.
fn split_halves(toks: &[Tok]) -> Result<(&[Tok], &[Tok]), GeoError> {
    let at = |pred: &dyn Fn(&Tok) -> bool| toks.iter().enumerate().filter(|(_, t)| pred(t)).map(|(i, _)| i).collect::<Vec<usize>>();
    let seps = at(&|t| *t == Tok::Sep);
    let split = match seps.as_slice() {
        [i] => return Ok((toks.get(..*i).unwrap_or_default(), toks.get(i.saturating_add(1)..).unwrap_or_default())),
        [] => {
            let hemis = at(&|t| matches!(t, Tok::Hemi(_)));
            let degrees = at(&|t| *t == Tok::Unit(0));
            let numbers = at(&|t| matches!(t, Tok::Num(..)));
            if let [first, second] = hemis.as_slice() {
                // Letters before the numbers (`N 38 W 9`) or after them (`38 N 9 W`).
                if *first == 0 { *second } else { first.saturating_add(1) }
            } else if !hemis.is_empty() {
                return Err(GeoError::Text);
            } else if let [_, second] = degrees.as_slice() {
                // The second coordinate starts at the number its degrees mark follows.
                second.checked_sub(1).ok_or(GeoError::Text)?
            } else if !degrees.is_empty() {
                return Err(GeoError::Text);
            } else {
                match numbers.len() {
                    2 | 4 | 6 => *numbers.get(numbers.len() / 2).ok_or(GeoError::Text)?,
                    _ => return Err(GeoError::Text),
                }
            }
        }
        _ => return Err(GeoError::Text),
    };
    let (a, b) = toks.split_at_checked(split).ok_or(GeoError::Text)?;
    Ok((a, b))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Lat,
    Lon,
}

/// One coordinate as read: its value (signed) and the axis its letter names, if it has one.
struct Half {
    value: f64,
    axis: Option<Axis>,
}

/// Reads one coordinate: an optional letter before or after, then one to three numbers, each with
/// an optional unit mark (marks in order: degrees, minutes, seconds).
fn half(toks: &[Tok]) -> Result<Half, GeoError> {
    let (mut letter, mut body) = (None, toks);
    if let [Tok::Hemi(h), rest @ ..] = body {
        letter = Some(*h);
        body = rest;
    }
    if let [rest @ .., Tok::Hemi(h)] = body {
        if letter.is_some() {
            return Err(GeoError::Text);
        }
        letter = Some(*h);
        body = rest;
    }
    // (value, unit) pairs, a number without a mark taking the place after the one before.
    let mut parts: [Option<f64>; 3] = [None; 3];
    let mut negative = false;
    let mut next: u8 = 0;
    let mut i = 0usize;
    let mut count = 0usize;
    while let Some(tok) = body.get(i) {
        let Tok::Num(v, signed) = *tok else { return Err(GeoError::Text) };
        if signed && count > 0 {
            return Err(GeoError::Text);
        }
        let unit = match body.get(i.saturating_add(1)) {
            Some(Tok::Unit(u)) => {
                i = i.saturating_add(2);
                *u
            }
            Some(Tok::Num(..)) | None => {
                i = i.saturating_add(1);
                next
            }
            Some(_) => return Err(GeoError::Text),
        };
        if unit < next || unit > 2 {
            return Err(GeoError::Text);
        }
        let slot = parts.get_mut(usize::from(unit)).ok_or(GeoError::Text)?;
        *slot = Some(v.abs());
        if count == 0 {
            negative = v.is_sign_negative();
        }
        next = unit.saturating_add(1);
        count = count.saturating_add(1);
    }
    let [d, m, s] = parts;
    let d = d.ok_or(GeoError::Text)?;
    // A finer part needs whole coarser parts, and minutes and seconds stay under 60.
    if (m.is_some() || s.is_some()) && d.fract() != 0.0 {
        return Err(GeoError::Text);
    }
    if let Some(m) = m
        && (m >= 60.0 || (s.is_some() && m.fract() != 0.0))
    {
        return Err(GeoError::Text);
    }
    if s.is_some_and(|s| s >= 60.0) {
        return Err(GeoError::Text);
    }
    let magnitude = d + m.unwrap_or(0.0) / 60.0 + s.unwrap_or(0.0) / 3600.0;
    let (axis, south_or_west) = match letter {
        None => (None, false),
        Some('N') => (Some(Axis::Lat), false),
        Some('S') => (Some(Axis::Lat), true),
        Some('E') => (Some(Axis::Lon), false),
        Some(_) => (Some(Axis::Lon), true),
    };
    if letter.is_some() && negative {
        // `-9 W`: two hemispheres at once.
        return Err(GeoError::Text);
    }
    let value = if negative || south_or_west { -magnitude } else { magnitude };
    Ok(Half { value, axis })
}

/// A position in world pixels at some zoom: `x` east from the antimeridian, `y` south from
/// [`MAX_LATITUDE`] north, both `0 ..= TILE_SIZE × 2^zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPx {
    pub x: f64,
    pub y: f64,
}

/// The world's side in pixels at `zoom`.
pub fn world_size(zoom: u8) -> Result<f64, GeoError> {
    if zoom > MAX_ZOOM {
        return Err(GeoError::Zoom);
    }
    Ok(f64::from(TILE_SIZE) * f64::from(1u32 << zoom))
}

/// How many tiles a row (and a column) of the world has at `zoom`.
fn tiles_across(zoom: u8) -> Result<u32, GeoError> {
    if zoom > MAX_ZOOM {
        return Err(GeoError::Zoom);
    }
    Ok(1u32 << zoom)
}

/// Where `p` lies on the world at `zoom`, its latitude clamped to ±[`MAX_LATITUDE`].
pub fn to_world(p: LatLon, zoom: u8) -> Result<WorldPx, GeoError> {
    let size = world_size(zoom)?;
    let lat = p.lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
    let x = (p.lon + 180.0) / 360.0 * size;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * size;
    Ok(WorldPx { x, y: y.clamp(0.0, size) })
}

/// The point at world pixel `px` at `zoom`: `x` wraps around the globe (the antimeridian is
/// −180), `y` is clamped to the world. A coordinate that is not finite is refused.
pub fn from_world(px: WorldPx, zoom: u8) -> Result<LatLon, GeoError> {
    let size = world_size(zoom)?;
    if !(px.x.is_finite() && px.y.is_finite()) {
        return Err(GeoError::Longitude);
    }
    // `x == size` is the east edge (180), any other x wraps into the world.
    let x = if px.x == size { size } else { px.x.rem_euclid(size) };
    let y = px.y.clamp(0.0, size);
    let lon = x / size * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * y / size)).sinh().atan().to_degrees();
    LatLon::new(lat.clamp(-90.0, 90.0), lon.clamp(-180.0, 180.0))
}

/// One map tile: its zoom and its column and row (`0 .. 2^zoom`, from the north-west).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tile {
    pub zoom: u8,
    pub x: u32,
    pub y: u32,
}

impl Tile {
    /// The tile at column `x` and row `y` of `zoom`, if the zoom has one.
    pub fn new(zoom: u8, x: u32, y: u32) -> Result<Tile, GeoError> {
        let n = tiles_across(zoom)?;
        if x >= n || y >= n {
            return Err(GeoError::Tile);
        }
        Ok(Tile { zoom, x, y })
    }

    /// The tile under `p` at `zoom` (a point on a tile edge belongs to the tile east and south of
    /// it, the world's east and south edges to the last tiles).
    pub fn at(p: LatLon, zoom: u8) -> Result<Tile, GeoError> {
        let n = tiles_across(zoom)?;
        let w = to_world(p, zoom)?;
        Ok(Tile { zoom, x: tile_index(w.x, n), y: tile_index(w.y, n) })
    }

    /// Its north-west corner in world pixels.
    pub fn origin(&self) -> WorldPx {
        WorldPx { x: f64::from(self.x) * f64::from(TILE_SIZE), y: f64::from(self.y) * f64::from(TILE_SIZE) }
    }

    /// Its north-west and south-east corners on the globe.
    pub fn corners(&self) -> Result<(LatLon, LatLon), GeoError> {
        let o = self.origin();
        let size = f64::from(TILE_SIZE);
        Ok((from_world(o, self.zoom)?, from_world(WorldPx { x: o.x + size, y: o.y + size }, self.zoom)?))
    }
}

/// The tile index of world pixel coordinate `v` on a world of `n` tiles.
fn tile_index(v: f64, n: u32) -> u32 {
    let i = (v / f64::from(TILE_SIZE)).floor();
    // `v` is within the world, so `i` is in `0..=n` and the cast cannot truncate.
    (i.max(0.0) as u32).min(n.saturating_sub(1))
}

/// A box on the globe: its south and north latitudes and its west and east longitudes. A west
/// east of its east crosses the antimeridian (`west 170, east −170` is 20° wide).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoBox {
    south: f64,
    west: f64,
    north: f64,
    east: f64,
}

impl GeoBox {
    /// The box between `south_west` and `north_east`; the south must not lie north of the north.
    pub fn new(south_west: LatLon, north_east: LatLon) -> Result<GeoBox, GeoError> {
        if south_west.lat > north_east.lat {
            return Err(GeoError::Box);
        }
        Ok(GeoBox { south: south_west.lat, west: south_west.lon, north: north_east.lat, east: north_east.lon })
    }

    pub fn south(&self) -> f64 {
        self.south
    }

    pub fn west(&self) -> f64 {
        self.west
    }

    pub fn north(&self) -> f64 {
        self.north
    }

    pub fn east(&self) -> f64 {
        self.east
    }

    /// Whether the box crosses the antimeridian.
    pub fn crosses_antimeridian(&self) -> bool {
        self.west > self.east
    }

    /// Whether `p` lies in the box, edges included.
    pub fn contains(&self, p: LatLon) -> bool {
        let lat = (self.south..=self.north).contains(&p.lat);
        let lon = if self.crosses_antimeridian() { p.lon >= self.west || p.lon <= self.east } else { (self.west..=self.east).contains(&p.lon) };
        lat && lon
    }
}

/// The tiles covering `area` at `zoom`, row by row from the north, each row from the west (across
/// the antimeridian: the columns up to the world's east edge, then from its west edge). Latitudes
/// past ±[`MAX_LATITUDE`] are clamped. Refused past [`MAX_TILES`], before anything is made.
pub fn tiles_covering(area: GeoBox, zoom: u8) -> Result<Vec<Tile>, GeoError> {
    let n = tiles_across(zoom)?;
    let nw = Tile::at(LatLon { lat: area.north, lon: area.west }, zoom)?;
    let se = Tile::at(LatLon { lat: area.south, lon: area.east }, zoom)?;
    let columns = if area.crosses_antimeridian() {
        n.saturating_sub(nw.x).saturating_add(se.x).saturating_add(1)
    } else {
        se.x.saturating_sub(nw.x).saturating_add(1)
    };
    let rows = se.y.saturating_sub(nw.y).saturating_add(1);
    let count = u64::from(columns).saturating_mul(u64::from(rows));
    if count > MAX_TILES as u64 {
        return Err(GeoError::TooManyTiles);
    }
    let xs: Vec<u32> = if area.crosses_antimeridian() { (nw.x..n).chain(0..=se.x).collect() } else { (nw.x..=se.x).collect() };
    let mut out = Vec::with_capacity(usize::try_from(count).unwrap_or(MAX_TILES));
    for y in nw.y..=se.y {
        out.extend(xs.iter().map(|&x| Tile { zoom, x, y }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minus_zero_is_zero() {
        let p = LatLon::new(-0.0, -0.0).unwrap();
        assert!(p.lat().is_sign_positive() && p.lon().is_sign_positive());
        assert_eq!(p, LatLon::new(0.0, 0.0).unwrap());
    }

    #[test]
    fn dms_never_reads_sixty_seconds() {
        assert_eq!(dms(10.999_999_99, ['N', 'S']), "11°0'0.000\"N");
        assert_eq!(dms(-0.5, ['E', 'W']), "0°30'0.000\"W");
    }
}
