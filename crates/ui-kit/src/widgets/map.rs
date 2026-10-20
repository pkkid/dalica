//! The map (`organizing.md` §7, the Places pane): Web Mercator on `core::geo`, zoom 0 …
//! [`MAX_ZOOM`] in whole levels, 256-pixel tiles.
//!
//! - **Layers**, bottom up: the sea and a graticule, the offline land (Natural Earth 1:110m,
//!   compiled in: [`offline_land`]) in the `gray` ramp, then the tile textures the caller hands in
//!   (the widget never fetches anything), the markers, the pin, the controls, the attribution line
//!   and the bubble. The world repeats east and west, so the map pans round the globe.
//! - **Input:** a drag pans; the wheel zooms one level per notch around the pointer (a trackpad's
//!   scroll pans, a pinch zooms); + and − zoom about the middle; with the pin tool armed a click
//!   places the pin. While the caller drags a payload (photos) over it, the map reports the point
//!   under the pointer and, on release, the drop, as a [`LatLon`].
//! - **Markers** closer than [`MARKER_GROUP`] points on screen are drawn as one with the sum of
//!   their counts; they split again as the map zooms in.
//! - **The bubble** is anchored to a marker or the pin, above it when there is room, else below,
//!   and always kept inside the widget: a thumbnail stack, a text and the caller's buttons.
//!
//! Registers `map:<id>` (the map), `marker:<n>` (a group of markers, by the index of its first),
//! `button:map.zoomIn`, `button:map.zoomOut`, `button:map.pin` (the pin tool, active while armed),
//! `map:<id>/pin` and `map:<id>/bubble`; the bubble's buttons by the ids the caller gives.

mod land;

use std::collections::HashMap;

use dalica_core::geo::{GeoBox, LatLon, MAX_LATITUDE, MAX_ZOOM, TILE_SIZE, Tile, WorldPx, from_world, to_world, world_size};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align, Color32, CornerRadius, CursorIcon, Id, Layout, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use serde::{Deserialize, Serialize};

pub use land::{LAND_FILE, LAND_MAGIC, LAND_MAX_BYTES, Land, LandError, Ring as LandRing, offline_land, triangulate};

use super::{Target, access, access_range, focus_ring, one_line, primary_button, register, text_button};
use crate::Tokens;
use crate::fonts;
use crate::icons::{PeopleIcon, paint_people};
use crate::input::wheel::{self, Wheel};
use crate::registry::Role;
use crate::text::{self, Text};

/// Markers closer than this on screen (points) are drawn as one.
pub const MARKER_GROUP: f32 = 32.0;
/// The side of a marker's (and the pin's) glyph square (points); its tip is the point.
pub const MARKER_SIZE: f32 = 28.0;
/// The side of the zoom and pin buttons (points), and their distance from the map's corner.
pub const MAP_BUTTON: f32 = 24.0;
pub const MAP_PAD: f32 = 8.0;
/// The bubble's padding, the room it keeps from the map's edges, its tail and its widths (points).
pub const BUBBLE_PAD: f32 = 10.0;
pub const BUBBLE_MARGIN: f32 = 6.0;
pub const BUBBLE_TAIL: f32 = 8.0;
pub const BUBBLE_MIN_W: f32 = 180.0;
pub const BUBBLE_MAX_W: f32 = 300.0;
/// The thumbnail stack's box (points).
pub const BUBBLE_STACK: f32 = 64.0;
/// The most pictures the stack shows, and the step between them (points).
pub const BUBBLE_STACK_MAX: usize = 3;
const STACK_STEP: f32 = 5.0;
/// The gap between the bubble's text, stack and buttons, and between two buttons (points).
const BUBBLE_GAP: f32 = 8.0;
/// The attribution shown when the caller gives none (the offline layer's source).
pub const OFFLINE_ATTRIBUTION: &str = "Natural Earth";
/// Graticule spacings (degrees), and the least room between two lines (points).
const GRATICULE: [f64; 13] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 45.0];
const GRATICULE_MIN: f64 = 56.0;
/// The most graticule lines (each way) and world copies drawn.
const MAX_LINES: usize = 256;
const MAX_COPIES: i64 = 16;
/// Where the tip of a marker glyph lies down its square (the glyph's 17.8 on its 20 grid).
const TIP: f32 = 0.89;

/// Where the map looks: the point at its middle, in world fractions (`x` east from the
/// antimeridian, `y` south from [`MAX_LATITUDE`] north, both 0 … 1, so any value is a place), and
/// the zoom (0 … [`MAX_ZOOM`]). Kept by the caller (view state), changed by [`map`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapView {
    x: f64,
    y: f64,
    zoom: u8,
}

impl Default for MapView {
    /// The whole world, its middle at 20° N so the continents fill it.
    fn default() -> MapView {
        MapView { x: 0.5, y: 0.443_27, zoom: 1 }
    }
}

/// A world fraction made whole: a number, 0 … 1 (`x` wraps).
fn fraction(v: f64, wrap: bool) -> f64 {
    if !v.is_finite() {
        0.5
    } else if wrap {
        v.rem_euclid(1.0)
    } else {
        v.clamp(0.0, 1.0)
    }
}

impl MapView {
    /// The view of `center` at `zoom` (past [`MAX_ZOOM`]: [`MAX_ZOOM`]).
    pub fn new(center: LatLon, zoom: u8) -> MapView {
        let side = f64::from(TILE_SIZE);
        let w = to_world(center, 0).unwrap_or(WorldPx { x: side / 2.0, y: side / 2.0 });
        MapView { x: fraction(w.x / side, true), y: fraction(w.y / side, false), zoom: zoom.min(MAX_ZOOM) }
    }

    pub fn zoom(&self) -> u8 {
        self.zoom.min(MAX_ZOOM)
    }

    /// The point at the middle.
    pub fn center(&self) -> Option<LatLon> {
        let side = f64::from(TILE_SIZE);
        from_world(WorldPx { x: fraction(self.x, true) * side, y: fraction(self.y, false) * side }, 0).ok()
    }

    /// The world's side at this zoom (pixels).
    fn size(&self) -> f64 {
        world_size(self.zoom()).unwrap_or(f64::from(TILE_SIZE))
    }

    /// The middle in world pixels at this zoom.
    fn middle(&self) -> WorldPx {
        let size = self.size();
        WorldPx { x: fraction(self.x, true) * size, y: fraction(self.y, false) * size }
    }

    /// Where world pixel (0, 0) of the copy of the world nearest the middle lands on screen.
    fn origin(&self, rect: Rect) -> (f64, f64) {
        let m = self.middle();
        (f64::from(rect.center().x) - m.x, f64::from(rect.center().y) - m.y)
    }

    /// Where `p` shows on a map drawn in `rect` (the copy of the world nearest the middle).
    pub fn to_screen(&self, rect: Rect, p: LatLon) -> Pos2 {
        let (size, m) = (self.size(), self.middle());
        let w = to_world(p, self.zoom()).unwrap_or(m);
        let mut dx = w.x - m.x;
        dx -= size * (dx / size).round();
        pos2((f64::from(rect.center().x) + dx) as f32, (f64::from(rect.center().y) + w.y - m.y) as f32)
    }

    /// The point under screen position `at` of a map drawn in `rect`; `None` above or below the
    /// world (at low zooms the world is shorter than the map).
    pub fn to_latlon(&self, rect: Rect, at: Pos2) -> Option<LatLon> {
        let (size, m) = (self.size(), self.middle());
        let x = m.x + f64::from(at.x) - f64::from(rect.center().x);
        let y = m.y + f64::from(at.y) - f64::from(rect.center().y);
        if !(x.is_finite() && (0.0..=size).contains(&y)) {
            return None;
        }
        from_world(WorldPx { x, y }, self.zoom()).ok()
    }

    /// The view moved so the map's content follows `delta` (screen points, a drag's direction).
    pub fn panned(&self, delta: Vec2) -> MapView {
        let (size, m) = (self.size(), self.middle());
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        if !(dx.is_finite() && dy.is_finite()) {
            return *self;
        }
        MapView { x: fraction((m.x - dx) / size, true), y: fraction((m.y - dy) / size, false), zoom: self.zoom() }
    }

    /// The view at `zoom` that keeps the point under `at` where it is (`at` off the world: the
    /// middle stays).
    pub fn zoomed(&self, rect: Rect, at: Pos2, zoom: u8) -> MapView {
        let zoom = zoom.min(MAX_ZOOM);
        let Some(p) = self.to_latlon(rect, at) else {
            return MapView { zoom, ..*self };
        };
        let size = world_size(zoom).unwrap_or(f64::from(TILE_SIZE));
        let Ok(w) = to_world(p, zoom) else { return *self };
        let x = w.x - (f64::from(at.x) - f64::from(rect.center().x));
        let y = w.y - (f64::from(at.y) - f64::from(rect.center().y));
        MapView { x: fraction(x / size, true), y: fraction(y / size, false), zoom }
    }

    /// The part of the globe a map drawn in `rect` shows (the whole width of the world when it is
    /// narrower than the map).
    pub fn visible(&self, rect: Rect) -> Option<GeoBox> {
        let (size, m) = (self.size(), self.middle());
        let (hw, hh) = (f64::from(rect.width()) / 2.0, f64::from(rect.height()) / 2.0);
        let at = |x: f64, y: f64| from_world(WorldPx { x, y: y.clamp(0.0, size) }, self.zoom()).ok();
        let (west, east) = if 2.0 * hw >= size {
            (-180.0, 180.0)
        } else {
            let (w, e) = (at(m.x - hw, m.y)?, at(m.x + hw, m.y)?);
            // A box ending on the antimeridian ends at 180, not −180.
            (w.lon(), if e.lon() <= w.lon() && (m.x + hw).rem_euclid(size) == 0.0 { 180.0 } else { e.lon() })
        };
        let (north, south) = (at(m.x, m.y - hh)?.lat(), at(m.x, m.y + hh)?.lat());
        GeoBox::new(LatLon::new(south, west).ok()?, LatLon::new(north, east).ok()?).ok()
    }
}

/// A tile texture the caller has: drawn over the offline layer where it lies, scaled when its zoom
/// is not the map's (a coarser tile fills in while finer ones load).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTile {
    pub tile: Tile,
    pub texture: TextureId,
}

/// A place with photos: drawn as a marker, grouped with its neighbours on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapMarker {
    pub at: LatLon,
    /// How many photos are there (the group shows the sum when more than 1).
    pub count: usize,
}

/// What the bubble points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleAnchor {
    /// The marker of this index in [`Map::markers`] (or the group it is drawn in).
    Marker(usize),
    /// The pin.
    Pin,
}

/// A picture of the bubble's stack: a texture and its size (for its shape).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BubbleThumb {
    pub texture: TextureId,
    pub size: Vec2,
}

/// A button of the bubble: its automation id, its label, and whether it is the default (a
/// `primary_button`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BubbleButton<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub primary: bool,
}

/// The bubble: "Put 3 photos here?" with a stack and [OK] [Cancel], or a marker's "3 photos
/// here" with its buttons. Everything in it is the caller's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapBubble<'a> {
    pub anchor: BubbleAnchor,
    pub thumbs: &'a [BubbleThumb],
    pub text: &'a str,
    pub buttons: &'a [BubbleButton<'a>],
}

/// A map: what [`map`] draws this pass.
#[derive(Debug, Clone, Copy)]
pub struct Map<'a> {
    /// The automation id's name: the map registers `map:<id>`.
    pub id: &'a str,
    pub view: MapView,
    pub tiles: &'a [MapTile],
    /// The tile provider's attribution ([`OFFLINE_ATTRIBUTION`] when `None`).
    pub attribution: Option<&'a str>,
    pub markers: &'a [MapMarker],
    pub pin: Option<LatLon>,
    /// The pin tool is armed: a click on the map places the pin.
    pub pin_tool: bool,
    pub bubble: Option<MapBubble<'a>>,
    /// The caller is dragging a payload (photos) that may be dropped on the map.
    pub payload: bool,
}

impl<'a> Map<'a> {
    /// A map `map:<id>` at `view` with nothing on it.
    pub fn new(id: &'a str, view: MapView) -> Map<'a> {
        Map { id, view, tiles: &[], attribution: None, markers: &[], pin: None, pin_tool: false, bubble: None, payload: false }
    }
}

/// Markers drawn as one: the index of the first (its id: `marker:<id>`), every member, the sum
/// of their counts and where the group's tip is on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerGroup {
    pub id: usize,
    pub members: Vec<usize>,
    pub count: usize,
    pub at: Pos2,
}

/// What a [`map`] did this pass.
#[derive(Debug, Clone)]
pub struct MapOut {
    /// The view after this pass's input; the caller keeps it.
    pub view: MapView,
    /// The view changed (pan or zoom).
    pub changed: bool,
    /// The part of the globe shown (before this pass's change).
    pub visible: Option<GeoBox>,
    /// A click on the map itself (not on a marker, the pin or a control), pin tool not armed.
    pub clicked: Option<LatLon>,
    /// A marker group was clicked: its members.
    pub marker_clicked: Option<Vec<usize>>,
    /// The pin tool's button was clicked (the caller arms or disarms it).
    pub pin_tool_clicked: bool,
    /// With the pin tool armed, a click placed the pin here.
    pub pin_placed: Option<LatLon>,
    /// The pin was clicked.
    pub pin_clicked: bool,
    /// The point under the pointer while a payload is dragged over the map.
    pub drop_hover: Option<LatLon>,
    /// The payload was let go over the map, here.
    pub dropped: Option<LatLon>,
    /// The bubble's button of this index was clicked.
    pub bubble_button: Option<usize>,
    /// The marker groups drawn, in the order of their first markers.
    pub groups: Vec<MarkerGroup>,
    /// Where the bubble was drawn.
    pub bubble: Option<Rect>,
    /// The map's own response.
    pub response: Response,
}

/// Groups `markers` as a map drawn in `rect` at `view` shows them: each marker joins the first
/// group whose first marker is closer than `distance` points on screen, else starts one; markers
/// beyond the map's edges are left out. Linear in the markers (a grid of `distance` cells). Pure,
/// shared with the tests.
pub fn group_markers(view: &MapView, rect: Rect, markers: &[MapMarker], distance: f32) -> Vec<MarkerGroup> {
    let distance = if distance.is_finite() && distance > 0.0 { distance } else { MARKER_GROUP };
    let shown = rect.expand(MARKER_SIZE);
    let cell = |p: Pos2| ((p.x / distance).floor() as i64, (p.y / distance).floor() as i64);
    let mut groups: Vec<(Pos2, MarkerGroup, Vec2)> = Vec::new();
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, m) in markers.iter().enumerate() {
        let p = view.to_screen(rect, m.at);
        if !shown.contains(p) {
            continue;
        }
        let (cx, cy) = cell(p);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .filter_map(|c| grid.get(&c))
            .flatten()
            .copied()
            .filter(|&g| groups.get(g).is_some_and(|(first, ..)| first.distance(p) < distance))
            .min();
        match near.and_then(|g| groups.get_mut(g)) {
            Some((_, group, sum)) => {
                group.members.push(i);
                group.count = group.count.saturating_add(m.count.max(1));
                *sum += p.to_vec2();
            }
            None => {
                grid.entry((cx, cy)).or_default().push(groups.len());
                groups.push((p, MarkerGroup { id: i, members: vec![i], count: m.count.max(1), at: p }, p.to_vec2()));
            }
        }
    }
    groups
        .into_iter()
        .map(|(_, mut g, sum)| {
            // A group stands at the middle of its members.
            g.at = (sum / g.members.len().max(1) as f32).to_pos2();
            g
        })
        .collect()
}

/// Which side of its anchor a bubble went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleSide {
    Above,
    Below,
}

/// Where a bubble of `size` goes in a map drawn in `area`, pointing at `anchor` (the marker's or
/// the pin's glyph): above it when there is room, else below when there is, else on the side with
/// more room; centred on it across, then moved (and if need be shrunk) to stay [`BUBBLE_MARGIN`]
/// inside the area. Pure, shared with the tests.
pub fn bubble_rect(area: Rect, anchor: Rect, size: Vec2) -> (Rect, BubbleSide) {
    let inner = area.shrink(BUBBLE_MARGIN);
    let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let size = vec2(finite(size.x).min(inner.width().max(0.0)), finite(size.y).min(inner.height().max(0.0)));
    let above = anchor.min.y - BUBBLE_TAIL - size.y;
    let below = anchor.max.y + BUBBLE_TAIL;
    let (top, side) = if above >= inner.min.y {
        (above, BubbleSide::Above)
    } else if below + size.y <= inner.max.y {
        (below, BubbleSide::Below)
    } else if anchor.min.y - inner.min.y >= inner.max.y - anchor.max.y {
        (above, BubbleSide::Above)
    } else {
        (below, BubbleSide::Below)
    };
    let clamp = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.max(lo).min(hi.max(lo)) } else { lo };
    let left = clamp(anchor.center().x - size.x / 2.0, inner.min.x, inner.max.x - size.x);
    let top = clamp(top, inner.min.y, inner.max.y - size.y);
    (Rect::from_min_size(pos2(left, top), size), side)
}

/// `n` grouped by thousands: "1,204".
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The glyph square of a marker or pin whose tip is at `tip`.
fn glyph_rect(tip: Pos2) -> Rect {
    Rect::from_min_size(pos2(tip.x - MARKER_SIZE / 2.0, tip.y - TIP * MARKER_SIZE), Vec2::splat(MARKER_SIZE))
}

/// A pin glyph at `tip` in `color`, over a dark rim so it reads on land, sea and tiles.
fn paint_marker(p: &Painter, t: &Tokens, tip: Pos2, color: Color32) {
    let r = glyph_rect(tip);
    paint_people(p, r.expand(2.0).translate(vec2(0.0, 1.0)), PeopleIcon::Marker, t.canvas);
    paint_people(p, r, PeopleIcon::Marker, color);
}

/// The copies of the world (by index east of the one nearest the middle) that `rect` shows.
fn copies(origin_x: f64, size: f64, rect: Rect) -> std::ops::RangeInclusive<i64> {
    let first = ((f64::from(rect.min.x) - origin_x) / size).floor();
    let last = ((f64::from(rect.max.x) - origin_x) / size).floor();
    if !(first.is_finite() && last.is_finite()) {
        return 0..=0;
    }
    let first = first as i64;
    first..=(last as i64).min(first.saturating_add(MAX_COPIES - 1))
}

/// The sea, the graticule and the offline land.
fn paint_offline(p: &Painter, t: &Tokens, view: &MapView, rect: Rect) {
    let size = view.size();
    let (ox, oy) = view.origin(rect);
    // Outside the world (above and below it at low zooms) is the light table's black.
    p.rect_filled(rect, CornerRadius::ZERO, t.grid_bg);
    let world = Rect::from_min_max(pos2(rect.min.x, oy as f32), pos2(rect.max.x, (oy + size) as f32)).intersect(rect);
    p.rect_filled(world, CornerRadius::ZERO, t.canvas);
    paint_graticule(p, t, view, rect, world);
    let Some(land) = offline_land() else { return };
    let scale = size / f64::from(TILE_SIZE);
    let view_box = rect.expand(2.0);
    let mut mesh = Mesh::default();
    let mut coast: Vec<Vec<Pos2>> = Vec::new();
    for k in copies(ox, size, rect) {
        let x0 = ox + k as f64 * size;
        let to = |w: &WorldPx| pos2((x0 + w.x * scale) as f32, (oy + w.y * scale) as f32);
        for ring in &land.rings {
            let [bx0, by0, bx1, by1] = ring.bounds;
            let bounds = Rect::from_min_max(to(&WorldPx { x: bx0, y: by0 }), to(&WorldPx { x: bx1, y: by1 }));
            if !bounds.intersects(view_box) {
                continue;
            }
            let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
            let color = if ring.lake { t.canvas } else { t.hover };
            let pts: Vec<Pos2> = ring.points.iter().map(to).collect();
            mesh.vertices.extend(pts.iter().map(|&pos| Vertex { pos, uv: WHITE_UV, color }));
            for tri in &ring.triangles {
                let corners = tri.map(|i| pts.get(i as usize).copied().unwrap_or(Pos2::ZERO));
                let tb = Rect::from_points(&corners);
                if tb.intersects(view_box) {
                    mesh.indices.extend(tri.map(|i| base.saturating_add(i)));
                }
            }
            // The coast: the runs of the ring's edges that cross the view.
            let mut run: Vec<Pos2> = Vec::new();
            for (i, &a) in pts.iter().enumerate() {
                let b = pts.get((i + 1) % pts.len()).copied().unwrap_or(a);
                if Rect::from_two_pos(a, b).intersects(view_box) {
                    if run.is_empty() {
                        run.push(a);
                    }
                    run.push(b);
                } else if !run.is_empty() {
                    coast.push(std::mem::take(&mut run));
                }
            }
            if run.len() > 1 {
                coast.push(run);
            }
        }
    }
    p.add(Shape::mesh(mesh));
    for run in coast {
        p.add(Shape::line(run, Stroke::new(1.0, t.button_border)));
    }
}

/// Lines of longitude and latitude, as far apart as the zoom allows ([`GRATICULE_MIN`] points at
/// least), over the world's part of `rect`.
fn paint_graticule(p: &Painter, t: &Tokens, view: &MapView, rect: Rect, world: Rect) {
    let size = view.size();
    let (ox, oy) = view.origin(rect);
    let per_degree = size / 360.0;
    let step = GRATICULE.iter().copied().find(|s| s * per_degree >= GRATICULE_MIN).unwrap_or(90.0);
    let stroke = Stroke::new(1.0, t.inset);
    // Meridians: every `step` degrees from the antimeridian, across the copies shown.
    let px = step * per_degree;
    let first = ((f64::from(rect.min.x) - ox) / px).ceil();
    if first.is_finite() {
        for i in 0..MAX_LINES {
            let x = ox + (first + i as f64) * px;
            if x > f64::from(rect.max.x) {
                break;
            }
            let x = x.round() as f32 + 0.5;
            p.line_segment([pos2(x, world.min.y), pos2(x, world.max.y)], stroke);
        }
    }
    // Parallels: every `step` degrees from the equator, within the projection's edge.
    let lat_at = |y: f64| from_world(WorldPx { x: 0.0, y: (y - oy).clamp(0.0, size) }, view.zoom()).map(|q| q.lat()).unwrap_or(0.0);
    let (north, south) = (lat_at(f64::from(world.min.y)), lat_at(f64::from(world.max.y)));
    let first = (south / step).ceil();
    for i in 0..MAX_LINES {
        let lat = (first + i as f64) * step;
        if lat > north || lat.abs() > MAX_LATITUDE {
            break;
        }
        let Ok(q) = LatLon::new(lat, 0.0) else { continue };
        let Ok(w) = to_world(q, view.zoom()) else { continue };
        let y = (oy + w.y).round() as f32 + 0.5;
        p.line_segment([pos2(world.min.x, y), pos2(world.max.x, y)], stroke);
    }
}

/// The caller's tiles over the offline layer, coarser zooms first.
fn paint_tiles(p: &Painter, view: &MapView, rect: Rect, tiles: &[MapTile]) {
    let size = view.size();
    let (ox, oy) = view.origin(rect);
    let mut order: Vec<&MapTile> = tiles.iter().filter(|m| m.tile.zoom <= MAX_ZOOM).collect();
    order.sort_by_key(|m| m.tile.zoom);
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    for m in order {
        let scale = 2f64.powi(i32::from(view.zoom()) - i32::from(m.tile.zoom));
        let side = f64::from(TILE_SIZE) * scale;
        let o = m.tile.origin();
        for k in copies(ox, size, rect) {
            let x = ox + k as f64 * size + o.x * scale;
            let y = oy + o.y * scale;
            let r = Rect::from_min_max(pos2(x as f32, y as f32), pos2((x + side) as f32, (y + side) as f32));
            if r.intersects(rect) {
                p.image(m.texture, r, uv, Color32::WHITE);
            }
        }
    }
}

/// One of the map's buttons: a `button` plate (`hover`, `pressed` held, `tool_active` active)
/// with a 1 pt `button_border`, its glyph `icon` (`text` hovered, `pick` active, `text_disabled`
/// disabled); `corners` rounds the plate's corners.
#[allow(clippy::too_many_arguments)] // a private painter of three buttons
fn map_button(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, glyph: PeopleIcon, enabled: bool, active: bool, corners: CornerRadius) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if active {
        t.tool_active
    } else if enabled && r.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    let p = ui.painter();
    p.rect(rect, corners, fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let color = if !enabled {
        t.text_disabled
    } else if active {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_people(p, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), glyph, color);
    focus_ring(p, t, &r, rect, t.radius);
    r
}

/// The bubble's size for `b`: its text wrapped to fit, the stack, the buttons in a row.
fn bubble_layout(ui: &Ui, t: &Tokens, b: &MapBubble<'_>, max_w: f32) -> (Vec2, std::sync::Arc<egui::Galley>, Vec<f32>) {
    let inner_max = (max_w.min(BUBBLE_MAX_W) - 2.0 * BUBBLE_PAD).max(1.0);
    let galley = ui.painter().layout(b.text.to_owned(), fonts::semibold(t.body), t.text, inner_max);
    let widths: Vec<f32> = b
        .buttons
        .iter()
        .map(|btn| (ui.painter().layout_no_wrap(btn.label.to_owned(), fonts::semibold(t.button_label), t.text).size().x + 18.0).max(37.0).round())
        .collect();
    let row = widths.iter().sum::<f32>() + BUBBLE_GAP * widths.len().saturating_sub(1) as f32;
    let stack = if b.thumbs.is_empty() { 0.0 } else { BUBBLE_STACK + STACK_STEP * (b.thumbs.len().min(BUBBLE_STACK_MAX) as f32 - 1.0) };
    let w = (galley.size().x.max(row).max(stack) + 2.0 * BUBBLE_PAD).clamp(BUBBLE_MIN_W, BUBBLE_MAX_W);
    let mut h = 2.0 * BUBBLE_PAD + galley.size().y;
    if stack > 0.0 {
        h += BUBBLE_GAP + stack;
    }
    if !widths.is_empty() {
        h += BUBBLE_GAP + t.metrics.button_h;
    }
    (vec2(w, h.ceil()), galley, widths)
}

/// The thumbnail stack in `area`: up to [`BUBBLE_STACK_MAX`] pictures, each fitted in a
/// [`BUBBLE_STACK`] square keeping its shape, the first on top, the others stepped up and right
/// behind it, each over the photo shadow with a 2 pt `photo_focus` frame.
fn paint_stack(p: &Painter, t: &Tokens, area: Rect, thumbs: &[BubbleThumb]) {
    let shown = thumbs.len().min(BUBBLE_STACK_MAX);
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    for i in (0..shown).rev() {
        let Some(th) = thumbs.get(i) else { continue };
        let step = STACK_STEP * i as f32;
        let cell = Rect::from_min_size(pos2(area.min.x + step, area.max.y - BUBBLE_STACK - step), Vec2::splat(BUBBLE_STACK));
        let (w, h) = (th.size.x, th.size.y);
        let fit = if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
            let s = (BUBBLE_STACK / w).min(BUBBLE_STACK / h);
            Rect::from_center_size(cell.center(), vec2(w * s, h * s))
        } else {
            cell
        };
        super::photo_shadow(p, fit);
        p.rect_filled(fit.expand(2.0), CornerRadius::ZERO, t.photo_focus);
        p.image(th.texture, fit, uv, Color32::WHITE);
    }
}

/// The map in `rect` (`design-system.md` §6): see the module's notes. Returns the view after this
/// pass's input, which the caller keeps, and what was clicked or dropped.
pub fn map(ui: &mut Ui, t: &Tokens, rect: Rect, m: &Map<'_>) -> MapOut {
    let enabled = ui.is_enabled();
    let map_id = format!("map:{}", m.id);
    let mut view = m.view;
    let visible = view.visible(rect);
    let response = ui.interact(rect, Id::new(&map_id), if enabled { Sense::click_and_drag() } else { Sense::hover() });

    // Pan and zoom from this pass's input, before anything is drawn.
    if enabled && response.dragged() {
        view = view.panned(response.drag_delta());
    }
    let wheel = wheel::read(ui.ctx());
    if enabled && response.contains_pointer() {
        let pointer = ui.ctx().pointer_latest_pos().unwrap_or(rect.center());
        match wheel {
            Some(Wheel::Zoom(f)) if f.is_finite() && f > 0.0 => {
                // A notch is one level; a pinch zooms a level per doubling.
                let pinch = ui.input(|i| i.zoom_delta() != 1.0);
                let unit = if pinch { std::f32::consts::LN_2 } else { wheel::NOTCH.ln() };
                let acc_id = Id::new((&map_id, "wheel"));
                let mut acc = ui.data(|d| d.get_temp::<f32>(acc_id)).unwrap_or(0.0) + f.ln() / unit;
                let mut zoom = i32::from(view.zoom());
                while acc >= 0.5 {
                    zoom += 1;
                    acc -= 1.0;
                }
                while acc <= -0.5 {
                    zoom -= 1;
                    acc += 1.0;
                }
                ui.data_mut(|d| d.insert_temp(acc_id, if pinch { acc } else { 0.0 }));
                let zoom = u8::try_from(zoom.clamp(0, i32::from(MAX_ZOOM))).unwrap_or(0);
                if zoom != view.zoom() {
                    view = view.zoomed(rect, pointer, zoom);
                }
            }
            Some(Wheel::Pan(d)) => view = view.panned(d),
            _ => {}
        }
    }

    let p = ui.painter_at(rect);
    paint_offline(&p, t, &view, rect);
    paint_tiles(&p, &view, rect, m.tiles);

    let mut out = MapOut {
        view,
        changed: false,
        visible,
        clicked: None,
        marker_clicked: None,
        pin_tool_clicked: false,
        pin_placed: None,
        pin_clicked: false,
        drop_hover: None,
        dropped: None,
        bubble_button: None,
        groups: Vec::new(),
        bubble: None,
        response: response.clone(),
    };

    // A click on the map: the pin with the tool armed, else a plain click.
    if enabled
        && response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let point = view.to_latlon(rect, at);
        if m.pin_tool { out.pin_placed = point } else { out.clicked = point }
    }

    // A payload dragged over the map: the point under the pointer, and the drop.
    let pointer = ui.ctx().pointer_latest_pos();
    if m.payload
        && enabled
        && let Some(at) = pointer.filter(|&at| rect.contains(at) && ui.clip_rect().contains(at))
    {
        out.drop_hover = view.to_latlon(rect, at);
        if ui.input(|i| i.pointer.any_released()) {
            out.dropped = out.drop_hover;
        }
        if let Some(q) = out.drop_hover {
            let tip = view.to_screen(rect, q);
            paint_marker(&p, t, tip, t.accent.gamma_multiply(0.6));
        }
    }

    // The markers.
    out.groups = group_markers(&view, rect, m.markers, MARKER_GROUP);
    for g in &out.groups {
        let gid = format!("marker:{}", g.id);
        let glyph = glyph_rect(g.at);
        // A marker at the edge takes clicks only on the part of it inside the map.
        let r = ui.interact(glyph.intersect(rect), Id::new(&gid), if enabled { Sense::click() } else { Sense::hover() });
        let hot = enabled && r.hovered();
        paint_marker(&p, t, g.at, if hot { t.accent_hover } else { t.accent });
        if g.count > 1 {
            let text = grouped(g.count);
            let galley = p.layout_no_wrap(text.clone(), fonts::semibold(t.small), t.on_accent);
            let h = galley.size().y + 2.0;
            let pill = Rect::from_min_size(pos2(glyph.center().x + 2.0, glyph.min.y - 4.0), vec2((galley.size().x + 8.0).max(h), h));
            p.rect(
                pill,
                CornerRadius::same(u8::try_from((h / 2.0).round() as i64).unwrap_or(8)),
                t.accent,
                Stroke::new(1.5, t.canvas),
                StrokeKind::Outside,
            );
            p.galley(pill.center() - galley.size() / 2.0, galley, t.on_accent);
        }
        if r.clicked() {
            out.marker_clicked = Some(g.members.clone());
        }
        focus_ring(&p, t, &r, glyph.shrink(4.0), 4);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, grouped(g.count)));
        register(ui.ctx(), &gid, Role::Button, &grouped(g.count), Target::Widget(&r), false);
    }

    // The pin.
    let texts = text::texts(ui.ctx());
    let pin_id = format!("{map_id}/pin");
    let pin_rect = m.pin.map(|q| glyph_rect(view.to_screen(rect, q)));
    if let (Some(q), Some(glyph)) = (m.pin, pin_rect) {
        let tip = view.to_screen(rect, q);
        let r = ui.interact(glyph.intersect(rect), Id::new(&pin_id), if enabled { Sense::click() } else { Sense::hover() });
        paint_marker(&p, t, tip, t.caution);
        p.circle_filled(pos2(glyph.center().x, glyph.min.y + 0.38 * MARKER_SIZE), 2.5, t.canvas);
        out.pin_clicked = r.clicked();
        focus_ring(&p, t, &r, glyph.shrink(4.0), 4);
        let name = texts.get(Text::Pin);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
        register(ui.ctx(), &pin_id, Role::Button, name, Target::Widget(&r), false);
    }

    // The controls: + over − at the top left, the pin tool beside them.
    let zoom = view.zoom();
    let plus = Rect::from_min_size(rect.min + Vec2::splat(MAP_PAD), Vec2::splat(MAP_BUTTON));
    let minus = plus.translate(vec2(0.0, MAP_BUTTON - 1.0));
    let top = CornerRadius { nw: t.radius, ne: t.radius, sw: 0, se: 0 };
    let bottom = CornerRadius { nw: 0, ne: 0, sw: t.radius, se: t.radius };
    let zr = map_button(ui, t, "button:map.zoomIn", plus, PeopleIcon::MapZoomIn, zoom < MAX_ZOOM, false, top);
    let zo = map_button(ui, t, "button:map.zoomOut", minus, PeopleIcon::MapZoomOut, zoom > 0, false, bottom);
    let pin_tool = plus.translate(vec2(MAP_BUTTON + 6.0, 0.0));
    let pt = map_button(ui, t, "button:map.pin", pin_tool, PeopleIcon::PinTool, true, m.pin_tool, CornerRadius::same(t.radius));
    for (r, id, label, active) in [
        (&zr, "button:map.zoomIn", texts.get(Text::ZoomIn), false),
        (&zo, "button:map.zoomOut", texts.get(Text::ZoomOut), false),
        (&pt, "button:map.pin", texts.get(Text::Pin), m.pin_tool),
    ] {
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, r.enabled(), active, label));
        register(ui.ctx(), id, if id == "button:map.pin" { Role::Toggle } else { Role::IconButton }, label, Target::Widget(r), active);
    }
    if zr.clicked() {
        out.view = out.view.zoomed(rect, rect.center(), zoom.saturating_add(1).min(MAX_ZOOM));
    }
    if zo.clicked() {
        out.view = out.view.zoomed(rect, rect.center(), zoom.saturating_sub(1));
    }
    out.pin_tool_clicked = pt.clicked();
    let _ = zr.on_hover_text(texts.get(Text::ZoomIn));
    let _ = zo.on_hover_text(texts.get(Text::ZoomOut));
    let _ = pt.on_hover_text(texts.get(Text::DropPin));

    // The attribution, bottom right on a translucent `chrome` plate.
    let text = m.attribution.filter(|s| !s.is_empty()).unwrap_or(OFFLINE_ATTRIBUTION);
    let galley = one_line(&p, text, fonts::regular(t.small), t.text_label, (rect.width() - 2.0 * MAP_PAD).max(1.0));
    let plate = Rect::from_min_size(rect.max - galley.size() - vec2(8.0, 4.0), galley.size() + vec2(8.0, 4.0));
    p.rect_filled(plate, CornerRadius { nw: t.radius, ..CornerRadius::ZERO }, t.chrome.gamma_multiply(0.8));
    p.galley(plate.min + vec2(4.0, 2.0), galley, t.text_label);

    // The bubble.
    if let Some(b) = &m.bubble {
        let anchor = match b.anchor {
            BubbleAnchor::Pin => pin_rect,
            BubbleAnchor::Marker(i) => out
                .groups
                .iter()
                .find(|g| g.members.contains(&i))
                .map(|g| glyph_rect(g.at))
                .or_else(|| m.markers.get(i).map(|mk| glyph_rect(view.to_screen(rect, mk.at)))),
        };
        if let Some(anchor) = anchor {
            out.bubble = Some(paint_bubble(ui, t, &map_id, rect, anchor, b, &mut out.bubble_button));
        }
    }

    if enabled {
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(if m.pin_tool { CursorIcon::Crosshair } else { CursorIcon::Grab });
        }
    }
    out.changed = out.view != m.view;
    if out.changed {
        ui.ctx().request_repaint();
    }
    // A group with its zoom: a screen reader hears "Map, zoom level 5".
    focus_ring(&p, t, &response, rect.shrink(2.0), 0);
    let zoom = out.view.zoom();
    let name = texts.get(Text::Map);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, enabled, name));
    access(&response, |node| node.set_role(egui::accesskit::Role::Group));
    access_range(&response, f64::from(zoom), 0.0, f64::from(MAX_ZOOM), 1.0, &text::fill(texts.get(Text::MapZoom), &[("zoom", &zoom.to_string())]));
    register(ui.ctx(), &map_id, Role::Other, name, Target::Widget(&response), false);
    out
}

/// The bubble pointing at `anchor`: the popup frame (`chrome`, 1 pt `button_border`, radius 6,
/// popup shadow) with a tail to the anchor, its text centred at the top, the stack, the buttons
/// centred in a row. Returns where it was drawn.
#[allow(clippy::too_many_arguments)] // the map's parts it draws among
fn paint_bubble(ui: &mut Ui, t: &Tokens, map_id: &str, area: Rect, anchor: Rect, b: &MapBubble<'_>, clicked: &mut Option<usize>) -> Rect {
    let (size, galley, widths) = bubble_layout(ui, t, b, area.width() - 2.0 * BUBBLE_MARGIN);
    let (rect, side) = bubble_rect(area, anchor, size);
    let bubble_id = format!("{map_id}/bubble");
    // The bubble takes the clicks on it, so none reaches the map under it.
    let r = ui.interact(rect, Id::new(&bubble_id), Sense::CLICK);
    let p = ui.painter_at(area);
    let radius = CornerRadius::same(6);
    p.add(t.popup_shadow.as_shape(rect, radius));
    p.rect(rect, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    // The tail, where the anchor lies beyond the edge it leaves from.
    let tx = anchor.center().x.clamp(rect.min.x + 6.0 + BUBBLE_TAIL, rect.max.x - 6.0 - BUBBLE_TAIL);
    let tail = match side {
        BubbleSide::Above if anchor.min.y >= rect.max.y => Some([
            pos2(tx - BUBBLE_TAIL, rect.max.y - 1.0),
            pos2(tx, (rect.max.y + BUBBLE_TAIL).min(anchor.min.y)),
            pos2(tx + BUBBLE_TAIL, rect.max.y - 1.0),
        ]),
        BubbleSide::Below if anchor.max.y <= rect.min.y => Some([
            pos2(tx - BUBBLE_TAIL, rect.min.y + 1.0),
            pos2(tx, (rect.min.y - BUBBLE_TAIL).max(anchor.max.y)),
            pos2(tx + BUBBLE_TAIL, rect.min.y + 1.0),
        ]),
        _ => None,
    };
    if let Some([a, tip, c]) = tail
        && tx.is_finite()
    {
        p.add(Shape::convex_polygon(vec![a, tip, c], t.chrome, Stroke::NONE));
        p.line_segment([a, tip], Stroke::new(1.0, t.button_border));
        p.line_segment([tip, c], Stroke::new(1.0, t.button_border));
    }
    let inner = rect.shrink(BUBBLE_PAD);
    let mut y = inner.min.y;
    let text_size = galley.size();
    p.galley(pos2(inner.center().x - text_size.x / 2.0, y), galley, t.text);
    y += text_size.y + BUBBLE_GAP;
    if !b.thumbs.is_empty() {
        let n = b.thumbs.len().min(BUBBLE_STACK_MAX) as f32;
        let w = BUBBLE_STACK + STACK_STEP * (n - 1.0);
        let stack = Rect::from_min_size(pos2(inner.center().x - w / 2.0, y), vec2(w, w));
        paint_stack(&p, t, stack, b.thumbs);
        y += w + BUBBLE_GAP;
    }
    if !b.buttons.is_empty() {
        let row_w = widths.iter().sum::<f32>() + BUBBLE_GAP * widths.len().saturating_sub(1) as f32;
        let row = Rect::from_min_size(pos2((inner.center().x - row_w / 2.0).max(inner.min.x), y), vec2(row_w.min(inner.width()), t.metrics.button_h));
        let mut child = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
        child.set_clip_rect(area.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing = vec2(BUBBLE_GAP, 0.0);
        for (i, btn) in b.buttons.iter().enumerate() {
            let r = if btn.primary { primary_button(&mut child, t, btn.id, btn.label) } else { text_button(&mut child, t, btn.id, btn.label, false) };
            if r.clicked() {
                *clicked = Some(i);
            }
        }
    }
    register(ui.ctx(), &bubble_id, Role::Other, b.text, Target::Widget(&r), false);
    rect
}
