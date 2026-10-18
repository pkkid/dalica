//! The map's offline layer (`organizing.md` §7): Natural Earth's 1:110m land, compiled in, so the
//! map shows where the continents are and geotagging by drag works without the network.
//!
//! The file (`assets/maps/land-110m.bin`, made by `assets/maps/convert.py`; format in
//! `assets/maps/README.md`) is a list of rings, each land or a lake in the land before it, of points
//! in hundredths of a degree. [`Land::parse`] reads it as hostile input: every count is bounded,
//! every coordinate checked, and a truncated or damaged file is refused, never a panic. Each ring is
//! projected once to Web Mercator world pixels at zoom 0 and cut into triangles by ear clipping, so
//! a frame only scales, offsets and culls them.

use std::sync::OnceLock;

use dalica_core::geo::{LatLon, WorldPx, to_world};

/// The compiled-in offline layer.
pub const LAND_FILE: &[u8] = include_bytes!("../../../../../assets/maps/land-110m.bin");
/// The largest offline layer read (bytes): the file stays well under it.
pub const LAND_MAX_BYTES: usize = 256 * 1024;
/// The file's first eight bytes.
pub const LAND_MAGIC: &[u8; 8] = b"DLLAND01";
/// The most rings, the most points in one ring and the most points in all.
const MAX_RINGS: usize = 4096;
const MAX_RING_POINTS: usize = 16_384;
const MAX_POINTS: usize = 65_536;
/// Below this (world pixels² at zoom 0, twice the area) three points count as one line.
const DEGENERATE: f64 = 1e-12;

/// Why an offline layer was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LandError {
    #[error("not an offline map layer")]
    Magic,
    #[error("the offline map layer ends early")]
    Truncated,
    #[error("the offline map layer is too large")]
    TooLarge,
    #[error("a ring of the offline map layer is malformed")]
    Ring,
    #[error("a point of the offline map layer is off the globe")]
    Point,
}

/// One ring: its points in world pixels at zoom 0 and its triangles.
#[derive(Debug, Clone)]
pub struct Ring {
    /// A lake (a hole in the land ring before it), painted in the sea's colour.
    pub lake: bool,
    /// The points, `0 ..= 256` both ways.
    pub points: Vec<WorldPx>,
    /// Triangles as indices into `points`.
    pub triangles: Vec<[u32; 3]>,
    /// The ring's bounds: min x, min y, max x, max y.
    pub bounds: [f64; 4],
}

/// The offline layer, ready to paint.
#[derive(Debug, Clone, Default)]
pub struct Land {
    pub rings: Vec<Ring>,
}

/// A little-endian reader that refuses to read past the end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], LandError> {
        let end = self.at.checked_add(N).ok_or(LandError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(LandError::Truncated)?;
        self.at = end;
        <[u8; N]>::try_from(slice).map_err(|_| LandError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, LandError> {
        Ok(u8::from_le_bytes(self.take::<1>()?))
    }

    fn u16(&mut self) -> Result<u16, LandError> {
        Ok(u16::from_le_bytes(self.take::<2>()?))
    }

    fn i16(&mut self) -> Result<i16, LandError> {
        Ok(i16::from_le_bytes(self.take::<2>()?))
    }
}

impl Land {
    /// Reads an offline layer: [`LAND_MAGIC`], a ring count (u16), then per ring its kind (u8: 0
    /// land, 1 lake), its point count (u16, at least 3) and its points (i16 longitude, i16
    /// latitude, hundredths of a degree). Anything else is refused.
    pub fn parse(bytes: &[u8]) -> Result<Land, LandError> {
        if bytes.len() > LAND_MAX_BYTES {
            return Err(LandError::TooLarge);
        }
        let mut r = Reader { bytes, at: 0 };
        if &r.take::<8>()? != LAND_MAGIC {
            return Err(LandError::Magic);
        }
        let count = usize::from(r.u16()?);
        if count > MAX_RINGS {
            return Err(LandError::TooLarge);
        }
        // Every ring is read and checked before any is cut into triangles, so a damaged file is
        // refused at the cost of reading it.
        let mut read = Vec::with_capacity(count);
        let mut total = 0usize;
        for _ in 0..count {
            let lake = match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(LandError::Ring),
            };
            let n = usize::from(r.u16()?);
            total = total.saturating_add(n);
            if !(3..=MAX_RING_POINTS).contains(&n) || total > MAX_POINTS {
                return Err(LandError::Ring);
            }
            // The whole ring must be there before anything is made for it.
            if bytes.len().saturating_sub(r.at) < n.saturating_mul(4) {
                return Err(LandError::Truncated);
            }
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                let (lon, lat) = (r.i16()?, r.i16()?);
                let p = LatLon::new(f64::from(lat) / 100.0, f64::from(lon) / 100.0).map_err(|_| LandError::Point)?;
                points.push(to_world(p, 0).map_err(|_| LandError::Point)?);
            }
            read.push((lake, points));
        }
        if r.at != bytes.len() {
            return Err(LandError::Ring);
        }
        Ok(Land { rings: read.into_iter().map(|(lake, points)| Ring::new(lake, points)).collect() })
    }

    /// How many points and triangles the layer has.
    pub fn size(&self) -> (usize, usize) {
        self.rings.iter().fold((0, 0), |(p, t), r| (p + r.points.len(), t + r.triangles.len()))
    }
}

impl Ring {
    fn new(lake: bool, points: Vec<WorldPx>) -> Ring {
        let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in &points {
            bounds = [bounds[0].min(p.x), bounds[1].min(p.y), bounds[2].max(p.x), bounds[3].max(p.y)];
        }
        let triangles = triangulate(&points);
        Ring { lake, points, triangles, bounds }
    }
}

/// The offline layer compiled in, read and cut into triangles on first use; `None` if it does not
/// read (the map then shows the sea and the graticule only).
pub fn offline_land() -> Option<&'static Land> {
    static LAND: OnceLock<Option<Land>> = OnceLock::new();
    LAND.get_or_init(|| Land::parse(LAND_FILE).ok()).as_ref()
}

/// Twice the signed area of the triangle `a b c` (positive when it turns the ring's way).
fn cross(a: WorldPx, b: WorldPx, c: WorldPx) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether `p` lies inside the positive triangle `a b c` or on its edges.
fn inside(a: WorldPx, b: WorldPx, c: WorldPx, p: WorldPx) -> bool {
    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
}

/// Cuts a simple polygon into triangles by ear clipping (indices into `points`). Points in a line
/// are dropped; a ring with no ear left (it touches itself) has its next corner clipped anyway, so
/// the loop always ends after at most `n²` steps.
pub fn triangulate(points: &[WorldPx]) -> Vec<[u32; 3]> {
    let n = points.len();
    if n < 3 || u32::try_from(n).is_err() {
        return Vec::new();
    }
    let at = |i: usize| points.get(i).copied().unwrap_or(WorldPx { x: 0.0, y: 0.0 });
    let twice_area: f64 = (0..n).map(|i| cross(WorldPx { x: 0.0, y: 0.0 }, at(i), at((i + 1) % n))).sum();
    // Walk the ring so that its inside is on the positive side.
    let mut ring: Vec<usize> = if twice_area >= 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let mut i = 0usize;
    let mut misses = 0usize;
    let mut steps = 0usize;
    let limit = n.saturating_mul(n).saturating_add(n);
    while ring.len() > 3 && steps < limit {
        steps += 1;
        let m = ring.len();
        i %= m;
        let (ia, ib, ic) = (ring.get((i + m - 1) % m), ring.get(i), ring.get((i + 1) % m));
        let (Some(&ia), Some(&ib), Some(&ic)) = (ia, ib, ic) else { break };
        let (a, b, c) = (at(ia), at(ib), at(ic));
        let turn = cross(a, b, c);
        let tri = [ia, ib, ic].map(|v| u32::try_from(v).unwrap_or(0));
        if turn.abs() <= DEGENERATE {
            // A point in a line (or a spike back on itself): it adds no area.
            ring.remove(i);
            misses = 0;
            continue;
        }
        let ear = turn > 0.0
            && ring.iter().all(|&v| {
                if v == ia || v == ib || v == ic {
                    return true;
                }
                let p = at(v);
                let same = |q: WorldPx| q.x == p.x && q.y == p.y;
                same(a) || same(b) || same(c) || !inside(a, b, c, p)
            });
        if ear || misses > m {
            out.push(tri);
            ring.remove(i);
            misses = 0;
        } else {
            i += 1;
            misses += 1;
        }
    }
    if let [a, b, c] = ring.as_slice()
        && cross(at(*a), at(*b), at(*c)).abs() > DEGENERATE
    {
        out.push([*a, *b, *c].map(|v| u32::try_from(v).unwrap_or(0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[WorldPx], tris: &[[u32; 3]]) -> f64 {
        tris.iter().map(|t| cross(points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]).abs() / 2.0).sum()
    }

    #[test]
    fn a_concave_ring_is_cut_into_triangles_covering_its_area() {
        // An L, clockwise and anticlockwise, with a point in a line on its long edge.
        let l = [(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (4.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)].map(|(x, y)| WorldPx { x, y });
        for pts in [l.to_vec(), l.iter().rev().copied().collect()] {
            let tris = triangulate(&pts);
            assert_eq!(tris.len(), 4, "{tris:?}");
            assert!((area(&pts, &tris) - 6.0).abs() < 1e-9);
        }
    }

    #[test]
    fn the_compiled_in_layer_reads_and_its_triangles_cover_each_ring() {
        let land = offline_land().unwrap();
        let (points, triangles) = land.size();
        assert!(points > 4000 && triangles > points - 2 * land.rings.len() - 200, "{points} points, {triangles} triangles");
        for ring in &land.rings {
            let polygon: f64 = (0..ring.points.len())
                .map(|i| cross(WorldPx { x: 0.0, y: 0.0 }, ring.points[i], ring.points[(i + 1) % ring.points.len()]))
                .sum::<f64>()
                .abs()
                / 2.0;
            let cut = area(&ring.points, &ring.triangles);
            assert!((cut - polygon).abs() <= polygon * 0.01 + 1e-6, "a ring of {} points: {cut} of {polygon}", ring.points.len());
        }
    }
}
