//! The map (`design-system.md` §6, `organizing.md` §7):
//!
//! - a payload dropped at a pixel reports the point the Web Mercator projection gives there, within
//!   a pixel, at zoom 2 and 15;
//! - the wheel zooms a level per notch and keeps the point under the pointer where it is; a drag
//!   pans with the pointer; + and − step the zoom and are disabled at its ends;
//! - two markers 20 points apart draw as one marked 2 and split when zoomed in;
//! - a bubble anchored near each edge and corner stays inside the map, buttons included;
//! - the pin tool's button is reported, and while it is armed a click places the pin;
//! - a tile the caller hands in is drawn where it lies, over the offline layer;
//! - the offline layer's file stays within 256 KB, and a truncated or damaged copy is refused
//!   without a panic;
//! - fingerprints: the offline world with markers and a bubble (`fingerprints/map-offline.json`)
//!   and the gallery, a Places pane with a tile, markers, the pin and its bubble, in a 1400 × 900
//!   window (`fingerprints/gallery-places-1400x900.json`) and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check, check_one};
use dalica_core::geo::{LatLon, MAX_ZOOM, Tile, WorldPx, from_world, to_world, world_size};
use dalica_ui_kit::widgets::{
    BUBBLE_MARGIN, BubbleAnchor, BubbleButton, BubbleSide, BubbleThumb, LAND_FILE, LAND_MAX_BYTES, Land, LandError, MARKER_GROUP, Map, MapBubble,
    MapMarker, MapOut, MapTile, MapView, bubble_rect, map,
};
use dalica_ui_kit::{Image, Tokens, WidgetInfo, registry};
use egui::{
    Color32, ColorImage, Context, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, TextureHandle, TextureOptions, TouchPhase, Ui, pos2,
    vec2,
};

/// The map's place in the rig's window.
const WINDOW: (u32, u32) = (680, 480);
const MAP: Rect = Rect { min: pos2(20.0, 20.0), max: pos2(660.0, 460.0) };
const LISBON: (f64, f64) = (38.7223, -9.1393);

fn ll(lat: f64, lon: f64) -> LatLon {
    LatLon::new(lat, lon).unwrap()
}

fn rect_of(w: &WidgetInfo) -> Rect {
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
}

fn check_ids(what: &str, widgets: &[WidgetInfo], unregistered: &[registry::Unregistered]) {
    assert_eq!(unregistered, [], "{what}: interactive widgets without an id");
    let mut seen = std::collections::HashSet::new();
    for w in widgets {
        assert!(registry::is_valid_id(&w.id), "{what}: {:?} breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{what}: {} registered twice", w.id);
    }
}

/// A procedural picture `w × h`: a diagonal blend of two colours. No photo of anyone.
fn picture(w: usize, h: usize, a: [u8; 3], b: [u8; 3]) -> ColorImage {
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    for y in 0..h {
        for x in 0..w {
            let f = (x + y) as f32 / (w + h) as f32;
            let mix = |i: usize| (a[i] as f32 * (1.0 - f) + b[i] as f32 * f) as u8;
            img.pixels[y * w + x] = Color32::from_rgb(mix(0), mix(1), mix(2));
        }
    }
    img
}

/// A procedural map tile: a flat colour with a darker road grid.
fn tile_image(color: [u8; 3]) -> ColorImage {
    let mut img = ColorImage::filled([256, 256], Color32::from_rgb(color[0], color[1], color[2]));
    for y in 0..256 {
        for x in 0..256 {
            if x % 64 < 3 || y % 48 < 3 {
                img.pixels[y * 256 + x] = Color32::from_rgb(color[0] / 2 + 60, color[1] / 2 + 60, color[2] / 2 + 40);
            }
        }
    }
    img
}

const BUTTONS: [BubbleButton<'static>; 2] = [
    BubbleButton { id: "button:places.ok", label: "OK", primary: true },
    BubbleButton { id: "button:places.cancel", label: "Cancel", primary: false },
];

/// What the map scene draws, and what the map reported.
struct Scene {
    rect: Rect,
    view: MapView,
    markers: Vec<MapMarker>,
    pin: Option<LatLon>,
    pin_tool: bool,
    payload: bool,
    bubble: Option<(BubbleAnchor, &'static str, bool)>,
    tiles: Vec<(Tile, [u8; 3])>,
    textures: Vec<TextureHandle>,
    thumbs: Vec<TextureHandle>,
    attribution: Option<&'static str>,
    out: Option<MapOut>,
    drops: Vec<LatLon>,
    hovers: Vec<LatLon>,
    pins: Vec<LatLon>,
    pin_tool_clicks: usize,
    bubble_clicks: Vec<usize>,
    clicks: Vec<LatLon>,
    marker_clicks: Vec<Vec<usize>>,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            rect: MAP,
            view: MapView::default(),
            markers: vec![],
            pin: None,
            pin_tool: false,
            payload: false,
            bubble: None,
            tiles: vec![],
            textures: vec![],
            thumbs: vec![],
            attribution: None,
            out: None,
            drops: vec![],
            hovers: vec![],
            pins: vec![],
            pin_tool_clicks: 0,
            bubble_clicks: vec![],
            clicks: vec![],
            marker_clicks: vec![],
        }
    }
}

fn map_scene(ui: &mut Ui, t: &Tokens, s: &mut Scene) {
    if s.textures.len() != s.tiles.len() {
        s.textures = s
            .tiles
            .iter()
            .enumerate()
            .map(|(i, (_, c))| ui.ctx().load_texture(format!("tile-{i}"), tile_image(*c), TextureOptions::LINEAR))
            .collect();
    }
    if s.thumbs.is_empty() {
        let pics = [([200, 120, 60], [60, 90, 160], 90, 60), ([90, 160, 110], [30, 50, 40], 60, 80), ([220, 200, 120], [150, 60, 50], 80, 80)];
        s.thumbs = pics
            .iter()
            .enumerate()
            .map(|(i, (a, b, w, h))| ui.ctx().load_texture(format!("thumb-{i}"), picture(*w, *h, *a, *b), TextureOptions::LINEAR))
            .collect();
    }
    let tiles: Vec<MapTile> = s.tiles.iter().zip(&s.textures).map(|((tile, _), tex)| MapTile { tile: *tile, texture: tex.id() }).collect();
    let thumbs: Vec<BubbleThumb> =
        s.thumbs.iter().map(|t| BubbleThumb { texture: t.id(), size: vec2(t.size()[0] as f32, t.size()[1] as f32) }).collect();
    let bubble =
        s.bubble.map(|(anchor, text, with_thumbs)| MapBubble { anchor, thumbs: if with_thumbs { &thumbs } else { &[] }, text, buttons: &BUTTONS });
    let m = Map {
        tiles: &tiles,
        attribution: s.attribution,
        markers: &s.markers,
        pin: s.pin,
        pin_tool: s.pin_tool,
        bubble,
        payload: s.payload,
        ..Map::new("places", s.view)
    };
    let out = map(ui, t, s.rect, &m);
    s.view = out.view;
    s.drops.extend(out.dropped);
    s.hovers.extend(out.drop_hover);
    s.pins.extend(out.pin_placed);
    s.pin_tool_clicks += usize::from(out.pin_tool_clicked);
    s.bubble_clicks.extend(out.bubble_button);
    s.clicks.extend(out.clicked);
    s.marker_clicks.extend(out.marker_clicked.clone());
    s.out = Some(out);
}

/// A scene drawn each frame with real input.
struct Rig<S> {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    state: S,
    scene: fn(&mut Ui, &Tokens, &mut S),
    widgets: Vec<WidgetInfo>,
    canvas: Canvas,
}

impl<S> Rig<S> {
    fn new(size: (u32, u32), state: S, scene: fn(&mut Ui, &Tokens, &mut S)) -> Rig<S> {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        dalica_ui_kit::input::wheel::configure(&ctx);
        let mut rig = Rig { ctx, t, size, clock: 0.0, state, scene, widgets: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let (t, scene) = (self.t, self.scene);
        let state = &mut self.state;
        let out = self.ctx.run_ui(raw, |ui| scene(ui, &t, state));
        self.widgets = registry::widgets(&self.ctx);
        self.canvas.keep(out);
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame(vec![]);
        }
    }

    fn find(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn rect(&self, id: &str) -> Rect {
        let w = self.find(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>()));
        rect_of(w)
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
    }

    fn click(&mut self, id: &str) {
        let at = self.rect(id).center();
        self.click_at(at);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(from), Self::press(from, true)]);
        for i in 1..=4 {
            self.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.0))]);
        }
        self.frame(vec![Self::press(to, false)]);
    }

    fn notch(&mut self, at: Pos2, up: bool) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at)]);
        let delta = vec2(0.0, if up { 1.0 } else { -1.0 });
        self.frame(vec![Event::MouseWheel { unit: MouseWheelUnit::Line, delta, phase: TouchPhase::Move, modifiers: Modifiers::NONE }]);
        self.settle();
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

fn rig(scene: Scene) -> Rig<Scene> {
    Rig::new(WINDOW, scene, |ui, t, s| {
        ui.painter().rect_filled(ui.max_rect(), 0, t.chrome);
        map_scene(ui, t, s);
    })
}

/// How far (pixels) `p` lies from world pixel `expected` at `zoom`, across the antimeridian too.
fn world_distance(p: LatLon, zoom: u8, expected: WorldPx) -> f64 {
    let size = world_size(zoom).unwrap();
    let w = to_world(p, zoom).unwrap();
    let mut dx = (w.x - expected.x).rem_euclid(size);
    if dx > size / 2.0 {
        dx -= size;
    }
    dx.hypot(w.y - expected.y)
}

#[test]
fn a_drop_reports_the_point_the_projection_gives_within_a_pixel() {
    for zoom in [2, 15] {
        let view = MapView::new(ll(LISBON.0, LISBON.1), zoom);
        let mut r = rig(Scene { view, payload: true, ..Scene::default() });
        let middle = to_world(view.center().unwrap(), zoom).unwrap();
        // At zoom 2 the world is 1024 pixels: the map shows part of it, rows of the poles too.
        for at in [MAP.center(), pos2(60.0, 50.0), pos2(640.0, 430.0), pos2(333.3, 77.7)] {
            // The payload's drag began outside the map; it moves in and is let go.
            r.frame(vec![Event::PointerMoved(pos2(5.0, 5.0)), Rig::<Scene>::press(pos2(5.0, 5.0), true)]);
            r.frame(vec![Event::PointerMoved(at)]);
            let hover = *r.state.hovers.last().expect("the point under the pointer while dragging");
            r.frame(vec![Rig::<Scene>::press(at, false)]);
            let dropped = *r.state.drops.last().unwrap_or_else(|| panic!("zoom {zoom}: no drop at {at:?}"));
            assert_eq!(hover, dropped, "zoom {zoom}");
            let expected = WorldPx { x: middle.x + f64::from(at.x - MAP.center().x), y: middle.y + f64::from(at.y - MAP.center().y) };
            let off = world_distance(dropped, zoom, expected);
            assert!(off <= 1.0, "zoom {zoom}: a drop at {at:?} reports {dropped} {off} pixels from the projection's point");
            r.state.drops.clear();
        }
        // Without a payload, letting go over the map drops nothing.
        r.state.payload = false;
        r.frame(vec![Event::PointerMoved(MAP.center()), Rig::<Scene>::press(MAP.center(), false)]);
        assert!(r.state.drops.is_empty(), "zoom {zoom}");
    }
}

#[test]
fn the_wheel_zooms_around_the_pointer_and_a_drag_pans_with_it() {
    let view = MapView::new(ll(LISBON.0, LISBON.1), 5);
    let mut r = rig(Scene { view, ..Scene::default() });
    let at = pos2(180.0, 330.0);
    let under = view.to_latlon(MAP, at).unwrap();
    r.notch(at, true);
    assert_eq!(r.state.view.zoom(), 6, "a notch up is one level in");
    let moved = r.state.view.to_screen(MAP, under).distance(at);
    assert!(moved <= 0.5, "the point under the pointer moved {moved} points");
    r.notch(at, false);
    r.notch(at, false);
    assert_eq!(r.state.view.zoom(), 4);
    let moved = r.state.view.to_screen(MAP, under).distance(at);
    assert!(moved <= 0.5, "zooming out it moved {moved} points");

    // A drag takes the point under the press along with the pointer.
    let (from, to) = (pos2(400.0, 200.0), pos2(310.0, 260.0));
    let grabbed = r.state.view.to_latlon(MAP, from).unwrap();
    r.drag(from, to);
    let moved = r.state.view.to_screen(MAP, grabbed).distance(to);
    assert!(moved <= 1.0, "the dragged point is {moved} points from the pointer");
    assert!(r.state.out.as_ref().unwrap().visible.unwrap().contains(grabbed));
}

#[test]
fn zoom_buttons_step_the_zoom_and_stop_at_its_ends() {
    let mut r = rig(Scene { view: MapView::new(ll(0.0, 0.0), 1), ..Scene::default() });
    let middle = r.state.view.center().unwrap();
    r.click("button:map.zoomIn");
    r.settle();
    assert_eq!(r.state.view.zoom(), 2);
    assert!(r.state.view.center().unwrap().distance_m(&middle) < 1.0, "+ zooms about the middle");
    r.click("button:map.zoomOut");
    r.click("button:map.zoomOut");
    r.settle();
    assert_eq!(r.state.view.zoom(), 0);
    assert_eq!(r.find("button:map.zoomOut").map(|w| w.enabled), Some(false), "− is disabled at zoom 0");
    r.click("button:map.zoomOut");
    assert_eq!(r.state.view.zoom(), 0);
    r.state.view = MapView::new(ll(0.0, 0.0), MAX_ZOOM);
    r.settle();
    assert_eq!(r.find("button:map.zoomIn").map(|w| w.enabled), Some(false), "+ is disabled at the last zoom");
    assert_eq!(r.find("button:map.zoomOut").map(|w| w.enabled), Some(true));
}

#[test]
fn markers_20_points_apart_draw_as_one_marked_2_and_split_when_zoomed_in() {
    let zoom = 9;
    let view = MapView::new(ll(LISBON.0, LISBON.1), zoom);
    let a = view.center().unwrap();
    let w = to_world(a, zoom).unwrap();
    let b = from_world(WorldPx { x: w.x + 20.0, y: w.y }, zoom).unwrap();
    let mut r = rig(Scene { view, markers: vec![MapMarker { at: a, count: 1 }, MapMarker { at: b, count: 1 }], ..Scene::default() });
    let groups = &r.state.out.as_ref().unwrap().groups;
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!((groups[0].count, groups[0].members.clone()), (2, vec![0, 1]));
    assert_eq!(r.find("marker:0").map(|w| w.label.as_str()), Some("2"), "the one marker is marked 2");
    assert!(r.find("marker:1").is_none());
    // One level in they are 40 points apart: two markers of 1.
    r.click("button:map.zoomIn");
    r.settle();
    let groups = &r.state.out.as_ref().unwrap().groups;
    assert_eq!(groups.len(), 2, "{groups:?}");
    assert!(groups[0].at.distance(groups[1].at) >= MARKER_GROUP);
    assert_eq!(r.find("marker:0").map(|w| w.label.as_str()), Some("1"));
    assert_eq!(r.find("marker:1").map(|w| w.label.as_str()), Some("1"));
    // A click on one reports its markers.
    r.click("marker:1");
    assert_eq!(r.state.marker_clicks, [vec![1]]);
    assert!(r.state.clicks.is_empty(), "a click on a marker is not a click on the map");
}

#[test]
fn a_bubble_near_each_edge_and_corner_stays_inside_the_map() {
    let view = MapView::new(ll(LISBON.0, LISBON.1), 6);
    let inset = 6.0;
    let (x0, x1, y0, y1) = (MAP.min.x + inset, MAP.max.x - inset, MAP.min.y + inset, MAP.max.y - inset);
    let (cx, cy) = (MAP.center().x, MAP.center().y);
    let spots = [(x0, y0), (cx, y0), (x1, y0), (x1, cy), (x1, y1), (cx, y1), (x0, y1), (x0, cy), (cx, cy)];
    for (x, y) in spots {
        let at = view.to_latlon(MAP, pos2(x, y)).unwrap();
        for anchor in [BubbleAnchor::Marker(0), BubbleAnchor::Pin] {
            let mut r = rig(Scene {
                view,
                markers: vec![MapMarker { at, count: 3 }],
                pin: Some(at),
                bubble: Some((anchor, "Put 3 photos here?", true)),
                ..Scene::default()
            });
            let what = format!("{anchor:?} at ({x}, {y})");
            let bubble = r.rect("map:places/bubble");
            assert!(MAP.shrink(BUBBLE_MARGIN - 0.01).contains_rect(bubble), "{what}: the bubble at {bubble:?} leaves the map");
            for id in ["button:places.ok", "button:places.cancel"] {
                assert!(bubble.contains_rect(r.rect(id)), "{what}: {id} leaves the bubble");
            }
            // Above the anchor when there is room, else below it.
            let side = bubble_rect(MAP, Rect::from_center_size(pos2(x, y), vec2(28.0, 28.0)), bubble.size()).1;
            assert_eq!(side, if y == y0 { BubbleSide::Below } else { BubbleSide::Above }, "{what}");
            check_ids(&what, &r.widgets, &registry::unregistered(&r.ctx));
            if x == cx && y == cy && anchor == BubbleAnchor::Pin {
                r.click("button:places.ok");
                assert_eq!(r.state.bubble_clicks, [0], "{what}: OK is reported");
                // A click on the bubble's body reaches nothing under it.
                r.click_at(bubble.center_top() + vec2(0.0, 12.0));
                assert!(r.state.clicks.is_empty(), "{what}: a click on the bubble reached the map");
            }
        }
    }
    // A bubble larger than the map shrinks to fit it.
    let (big, _) = bubble_rect(MAP, Rect::from_center_size(MAP.center(), vec2(28.0, 28.0)), vec2(2000.0, 2000.0));
    assert_eq!(big, MAP.shrink(BUBBLE_MARGIN));
}

#[test]
fn the_pin_tool_is_reported_and_armed_a_click_places_the_pin() {
    let view = MapView::new(ll(LISBON.0, LISBON.1), 8);
    let mut r = rig(Scene { view, ..Scene::default() });
    r.click("button:map.pin");
    assert_eq!(r.state.pin_tool_clicks, 1);
    let at = pos2(250.0, 300.0);
    r.click_at(at);
    assert!(r.state.pins.is_empty(), "not armed: a plain click");
    assert_eq!(r.state.clicks, [view.to_latlon(MAP, at).unwrap()]);
    r.state.pin_tool = true;
    r.settle();
    assert_eq!(r.find("button:map.pin").map(|w| w.active), Some(true));
    r.click_at(at);
    assert_eq!(r.state.pins, [view.to_latlon(MAP, at).unwrap()]);
    assert_eq!(r.state.clicks.len(), 1, "armed, a click places the pin and is no plain click");
    r.state.pin = r.state.pins.first().copied();
    r.settle();
    let pin = r.rect("map:places/pin");
    assert!((pin.center().x - at.x).abs() < 1.0 && pin.max.y > at.y - 4.0 && pin.min.y < at.y, "the pin's tip is on the point: {pin:?}");
}

#[test]
fn a_tile_handed_in_is_drawn_where_it_lies() {
    let zoom = 6;
    let view = MapView::new(ll(LISBON.0, LISBON.1), zoom);
    let tile = Tile::at(ll(LISBON.0, LISBON.1), zoom).unwrap();
    // The tile under Lisbon, and its parent one level coarser, drawn twice the size beneath it.
    let parent = Tile::new(zoom - 1, tile.x / 2, tile.y / 2).unwrap();
    let mut r = rig(Scene { view, tiles: vec![(tile, [200, 40, 40]), (parent, [40, 40, 200])], ..Scene::default() });
    let o = view.to_screen(MAP, from_world(tile.origin(), zoom).unwrap());
    let inside = o + vec2(30.0, 30.0);
    assert_eq!(&r.pixel(inside)[..3], &[200, 40, 40], "the tile at {o:?}");
    // The parent's other quarters show round it; past both, the offline layer.
    let p = view.to_screen(MAP, from_world(WorldPx { x: f64::from(parent.x) * 512.0, y: f64::from(parent.y) * 512.0 }, zoom).unwrap());
    let quarter = if p.x < o.x { pos2(p.x + 30.0, o.y + 30.0) } else { pos2(p.x + 286.0, o.y + 30.0) };
    if MAP.contains(quarter) {
        assert_eq!(&r.pixel(quarter)[..3], &[40, 40, 200], "the parent at {p:?}");
    }
}

#[test]
fn the_offline_layer_is_small_and_a_damaged_copy_is_refused() {
    assert!(LAND_FILE.len() <= 256 * 1024 && LAND_FILE.len() <= LAND_MAX_BYTES, "{} bytes", LAND_FILE.len());
    let land = Land::parse(LAND_FILE).unwrap();
    assert!(land.rings.len() > 100);
    // Every cut short of the whole file is refused, never a panic.
    let step = if crate::common::full() { 1 } else { 37 };
    for cut in (0..LAND_FILE.len()).step_by(step).chain(LAND_FILE.len() - 8..LAND_FILE.len()) {
        assert!(Land::parse(&LAND_FILE[..cut]).is_err(), "cut at {cut}");
    }
    assert_eq!(Land::parse(&LAND_FILE[..LAND_FILE.len() - 1]).err(), Some(LandError::Truncated));
    let mut bad = LAND_FILE.to_vec();
    bad[0] ^= 0xff;
    assert_eq!(Land::parse(&bad).err(), Some(LandError::Magic));
    // A ring claiming more points than are left, a kind that is no kind, a point off the globe.
    let mut bad = LAND_FILE.to_vec();
    bad[11] = 0xff;
    bad[12] = 0xff;
    assert!(Land::parse(&bad).is_err());
    let mut bad = LAND_FILE.to_vec();
    bad[10] = 7;
    assert_eq!(Land::parse(&bad).err(), Some(LandError::Ring));
    let mut bad = LAND_FILE.to_vec();
    bad[13..15].copy_from_slice(&30_000i16.to_le_bytes());
    assert_eq!(Land::parse(&bad).err(), Some(LandError::Point));
    let mut long = LAND_FILE.to_vec();
    long.resize(LAND_MAX_BYTES + 1, 0);
    assert_eq!(Land::parse(&long).err(), Some(LandError::TooLarge));
}

/// Markers round Lisbon and Porto for the renders.
fn markers() -> Vec<MapMarker> {
    [
        (38.7223, -9.1393, 12),
        (38.70, -9.40, 1),
        (41.1579, -8.6291, 4),
        (37.0194, -7.9304, 1),
        (40.2033, -8.4103, 2),
        (51.5072, -0.1276, 7),
        (48.8566, 2.3522, 1),
    ]
    .into_iter()
    .map(|(lat, lon, count)| MapMarker { at: ll(lat, lon), count })
    .collect()
}

#[test]
fn the_offline_map_with_markers_and_a_bubble_matches_its_fingerprint() {
    let view = MapView::new(ll(42.0, -20.0), 2);
    let mut r = rig(Scene { view, markers: markers(), bubble: Some((BubbleAnchor::Marker(5), "7 photos here", true)), ..Scene::default() });
    check_ids("map", &r.widgets, &registry::unregistered(&r.ctx));
    for id in ["map:places", "button:map.zoomIn", "button:map.zoomOut", "button:map.pin", "marker:0", "map:places/bubble"] {
        assert!(r.find(id).is_some(), "no {id}");
    }
    check_one("map-offline", &r.image());
}

// ---- The gallery ----

fn gallery(ui: &mut Ui, t: &Tokens, s: &mut Scene) {
    let screen = ui.max_rect();
    ui.painter().rect_filled(screen, 0, t.chrome);
    // The light table on the left, the Places pane on the right: header, status, map, search.
    let pane_w = (screen.width() * 0.3).round().max(300.0);
    let table = Rect::from_min_max(screen.min + vec2(0.0, 32.0), pos2(screen.max.x - pane_w, screen.max.y));
    ui.painter().rect_filled(table, 0, t.grid_bg);
    let pane = Rect::from_min_max(pos2(table.max.x, table.min.y), screen.max);
    let font = dalica_ui_kit::fonts::semibold(t.title);
    ui.painter().text(pos2(pane.center().x, pane.min.y + 16.0), egui::Align2::CENTER_CENTER, "Places", font, t.text);
    ui.painter().text(
        pos2(pane.min.x + 12.0, pane.min.y + 44.0),
        egui::Align2::LEFT_CENTER,
        "5 places in the selection",
        dalica_ui_kit::fonts::regular(t.body),
        t.text_label,
    );
    s.rect = Rect::from_min_max(pos2(pane.min.x + 8.0, pane.min.y + 60.0), pos2(pane.max.x - 8.0, pane.max.y - 64.0));
    ui.painter().text(
        pos2(pane.min.x + 12.0, s.rect.max.y + 18.0),
        egui::Align2::LEFT_CENTER,
        "Search for a place:",
        dalica_ui_kit::fonts::regular(t.body),
        t.text_label,
    );
    map_scene(ui, t, s);
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in [(1400, 900), (900, 600)] {
        let what = format!("gallery {size:?}");
        // Lisbon at zoom 6, one tile handed in under it, markers round the country and the pin
        // with its "Put 3 photos here?" bubble.
        let zoom = 5;
        let tile = Tile::at(ll(LISBON.0, LISBON.1), zoom).unwrap();
        let scene = || Scene {
            view: MapView::new(ll(40.0, -6.0), zoom),
            markers: markers(),
            pin: Some(ll(39.48, -8.0)),
            bubble: Some((BubbleAnchor::Pin, "Put 3 photos here?", true)),
            tiles: vec![(tile, [214, 205, 186])],
            attribution: Some("Map tiles: example provider"),
            ..Scene::default()
        };
        let mut one = Rig::new(size, scene(), gallery);
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        let map_rect = one.state.rect;
        assert!(map_rect.contains_rect(one.rect("map:places/bubble")), "{what}: the bubble leaves the map");
        for id in ["map:places", "map:places/pin", "button:map.zoomIn", "button:map.pin", "button:places.ok", "button:places.cancel", "marker:0"] {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            let a = one.image();
            let mut two = Rig::new(size, scene(), gallery);
            let b = two.image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-places-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
