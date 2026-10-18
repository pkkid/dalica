//! The widgets of the effect tabs (`design-system.md` §6, `editing.md` §8.1):
//!
//! - effect tiles: a 3 × 4 grid fits the 280-wide edit panel at both window sizes, each tile
//!   86 × 69 and registered `effect:<name>`; a tile without a preview paints `gray 38`, one with a
//!   preview shows it; the "1-click" badge shows only when asked; a click reports its tile, a
//!   disabled tile takes none;
//! - the effect slider: the logarithmic mapping round-trips its range (and the linear one too), a
//!   click halfway along a log track gives the range's geometric middle, + moves 2 % of the track,
//!   and the label is centred above;
//! - the colour swatch: a colour picked in the field and the hue strip gives that colour back, a
//!   typed hex is taken and a bad one refused, the eyedropper arms and keeps the popup open for the
//!   pick, a click outside closes it, and from a swatch at each corner of a 900 × 600 window the
//!   popup stays inside it;
//! - the crosshair reports a drag of its centre and of its angle handle in its own normalised
//!   coordinates, kept on its rect;
//! - the gallery: tab 3's grid, a parameter panel with its sliders and an open picker, and the two
//!   crosshairs on a photo, in a 1400 × 900 window (`fingerprints/gallery-effects-1400x900.json`)
//!   and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check};
use dalica_core::color::{hsv_to_rgb, rgb_to_hsv, rgb_to_u8};
use dalica_ui_kit::widgets::{
    Crosshair, CrosshairOut, EFFECT_GRID_COLUMNS, EffectTile, SliderScale, SliderSpec, SwatchOut, crosshair, crosshair_handle_id, effect_grid,
    effect_slider, effect_tile_id, field_color, field_point, handle_angle, hue_at, hue_color, hue_point, open_swatch, scale_fraction, scale_nudged,
    scale_value, swatch, swatch_open, swatch_popup_rect, swatch_popup_size,
};
use dalica_ui_kit::{Image, Tokens, WidgetInfo, registry};
use egui::{
    Color32, ColorImage, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2,
};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const PANEL_W: f32 = 280.0;

/// Tab 3's twelve effects in grid order (`editing.md` §8.2), the one-click ones marked.
const TAB3: [(&str, &str, bool); 12] = [
    ("sharpen", "Sharpen", false),
    ("sepia", "Sepia", true),
    ("bw", "B&W", true),
    ("warmify", "Warmify", true),
    ("film_grain", "Film Grain", false),
    ("tint", "Tint", false),
    ("saturation", "Saturation", false),
    ("soft_focus", "Soft Focus", false),
    ("glow", "Glow", false),
    ("filtered_bw", "Filtered B&W", false),
    ("focal_bw", "Focal B&W", false),
    ("graduated_tint", "Graduated Tint", false),
];

fn tiles(previews: &[Option<egui::TextureId>]) -> Vec<EffectTile<'static>> {
    TAB3.iter()
        .enumerate()
        .map(|(i, &(name, label, one_click))| EffectTile {
            name,
            label,
            tooltip: "What the effect does.",
            preview: previews.get(i).copied().flatten(),
            one_click,
            enabled: true,
        })
        .collect()
}

fn check_ids(what: &str, widgets: &[WidgetInfo], unregistered: &[registry::Unregistered]) {
    assert_eq!(unregistered, [], "{what}: interactive widgets without an id");
    let mut seen = std::collections::HashSet::new();
    for w in widgets {
        assert!(registry::is_valid_id(&w.id), "{what}: {:?} breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{what}: {} registered twice", w.id);
    }
}

fn area(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
}

fn rect_of(w: &WidgetInfo) -> Rect {
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
}

/// A procedural preview: a sky over a field, `w × h`, tinted by `tint`.
fn preview_image(w: usize, h: usize, tint: [u8; 3]) -> ColorImage {
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    for y in 0..h {
        for x in 0..w {
            let sky = y < h / 2;
            let base: [u16; 3] = if sky { [120, 160, 205] } else { [70, 112, 64] };
            let shade = (x * 40 / w) as u16;
            let c = |i: usize| ((base[i] + shade) * u16::from(tint[i]) / 255) as u8;
            img.pixels[y * w + x] = Color32::from_rgb(c(0), c(1), c(2));
        }
    }
    img
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

    /// A click at `at`, a second after the last (never a double-click).
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

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true), ev(false)]);
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

// ---- Effect tiles ----

struct TileScene {
    textures: Vec<TextureHandle>,
    with_previews: bool,
    disabled: Option<usize>,
    clicked: Vec<usize>,
    panel: Rect,
    grid: Rect,
    tiles: Vec<Rect>,
}

impl Default for TileScene {
    fn default() -> TileScene {
        TileScene {
            textures: vec![],
            with_previews: false,
            disabled: None,
            clicked: vec![],
            panel: Rect::NOTHING,
            grid: Rect::NOTHING,
            tiles: vec![],
        }
    }
}

fn tile_scene(ui: &mut Ui, t: &Tokens, s: &mut TileScene) {
    if s.with_previews && s.textures.is_empty() {
        let tint = |i: usize| [255, 255 - (i * 15) as u8, 200 + (i * 4) as u8];
        s.textures = (0..12).map(|i| ui.ctx().load_texture(format!("tile{i}"), preview_image(78, 48, tint(i)), TextureOptions::LINEAR)).collect();
    }
    let screen = ui.max_rect();
    // The edit panel: 280 wide on the right, its content 6 pt in.
    let panel = Rect::from_min_max(pos2(screen.max.x - PANEL_W, screen.min.y + 40.0), screen.max);
    ui.painter().rect_filled(panel, 0, t.chrome);
    s.panel = panel;
    let previews: Vec<Option<egui::TextureId>> = (0..12).map(|i| if i % 3 == 2 { None } else { s.textures.get(i).map(TextureHandle::id) }).collect();
    let mut list = tiles(&previews);
    if let Some(d) = s.disabled
        && let Some(tile) = list.get_mut(d)
    {
        tile.enabled = false;
    }
    area(ui, panel.shrink2(vec2(6.0, 8.0)), |ui| {
        let out = effect_grid(ui, t, &list);
        if let Some(i) = out.clicked {
            s.clicked.push(i);
        }
        s.grid = out.rect;
        s.tiles = out.tiles;
    });
}

#[test]
fn a_grid_of_twelve_tiles_fits_the_280_wide_panel() {
    for size in WINDOWS {
        let what = format!("{size:?}");
        let rig = Rig::new(size, TileScene::default(), tile_scene);
        check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
        let s = &rig.state;
        let inner = s.panel.shrink2(vec2(6.0, 8.0));
        assert!(inner.contains_rect(s.grid), "{what}: grid {:?} leaves the panel {inner:?}", s.grid);
        assert_eq!(s.tiles.len(), 12);
        for (i, tile) in s.tiles.iter().enumerate() {
            assert_eq!(tile.size(), vec2(86.0, 69.0), "{what}: tile {i}");
            assert!(s.grid.contains_rect(*tile), "{what}: tile {i}");
            let (col, row) = (i % EFFECT_GRID_COLUMNS, i / EFFECT_GRID_COLUMNS);
            assert_eq!(tile.min.x, s.tiles[col].min.x, "{what}: tile {i} in its column");
            assert_eq!(tile.min.y, s.tiles[row * 3].min.y, "{what}: tile {i} in its row");
            for other in s.tiles.iter().skip(i + 1) {
                assert!(!tile.intersects(*other) || tile.intersect(*other).area() == 0.0, "{what}: tiles overlap");
            }
            let id = effect_tile_id(TAB3[i].0);
            assert_eq!(rig.rect(&id), *tile, "{what}: {id}");
        }
        assert!((s.grid.center().x - inner.center().x).abs() <= 0.5, "{what}: the grid is centred");
    }
}

#[test]
fn a_tile_without_a_preview_paints_gray_38_and_the_badge_shows_only_when_asked() {
    let mut rig = Rig::new((900, 600), TileScene { with_previews: true, ..TileScene::default() }, tile_scene);
    rig.settle();
    let t = rig.t;
    let preview_of = |tile: Rect| Rect::from_min_size(pos2(tile.center().x - 39.0, tile.min.y + 3.0), vec2(78.0, 48.0));
    // Tile 2 (B&W) and 5 (Tint) have no preview: B&W is one-click, Tint is not.
    let bw = preview_of(rig.state.tiles[2]);
    let tint = preview_of(rig.state.tiles[5]);
    assert_eq!(rig.pixel(bw.left_bottom() + vec2(10.0, -10.0)), t.placeholder.to_array(), "no preview: gray 38");
    assert_eq!(rig.pixel(tint.right_top() + vec2(-10.0, 10.0)), t.placeholder.to_array(), "no badge where not asked");
    let badge = Rect::from_min_size(pos2(bw.max.x - 18.0, bw.min.y + 2.0), vec2(16.0, 16.0));
    let corner = rig.pixel(badge.min + vec2(1.0, 1.0));
    assert!(corner[0] < t.placeholder.r(), "the badge's plate darkens the corner: {corner:?}");
    let lit = (0..16).flat_map(|y| (0..16).map(move |x| (x, y))).filter(|&(x, y)| rig.pixel(badge.min + vec2(x as f32, y as f32))[0] > 150).count();
    assert!(lit >= 6, "the one-click glyph is on the badge: {lit} light pixels");
    // Tile 0 (Sharpen) has a preview: its pixels are the texture's, not the placeholder.
    let sharpen = preview_of(rig.state.tiles[0]);
    let sky = rig.pixel(sharpen.min + vec2(4.0, 4.0));
    assert!(sky[2] > sky[0] + 20 && sky[0] > 100, "the preview's sky: {sky:?}");
    let not_badged = rig.pixel(sharpen.right_top() + vec2(-10.0, 10.0));
    assert!(not_badged[2] > not_badged[0] + 20 && not_badged[0] > 100, "Sharpen has no badge: {not_badged:?}");
}

#[test]
fn a_click_picks_its_tile_and_a_disabled_tile_takes_none() {
    let mut rig = Rig::new((1400, 900), TileScene { disabled: Some(4), ..TileScene::default() }, tile_scene);
    rig.click(&effect_tile_id("soft_focus"));
    rig.click(&effect_tile_id("film_grain"));
    assert_eq!(rig.state.clicked, [7]);
    assert!(!rig.find(&effect_tile_id("film_grain")).unwrap().enabled);
}

// ---- The effect slider ----

const RADIUS: SliderSpec<'static> =
    SliderSpec { id: "slider:effect.radius", label: "Radius", min: 1.3, max: 80.0, default: 3.0, step: 0.0, decimals: 1, unit: "" };

#[test]
fn the_logarithmic_mapping_round_trips_its_range() {
    let specs = [
        RADIUS,
        SliderSpec { min: 0.0, max: 100.0, ..RADIUS },
        SliderSpec { min: -5.0, max: 5.0, ..RADIUS },
        SliderSpec { min: 80.0, max: 1.3, ..RADIUS },
    ];
    for spec in specs {
        for scale in [SliderScale::Log, SliderScale::Linear] {
            let (lo, hi) = spec.bounds();
            assert_eq!(scale_fraction(&spec, scale, lo), 0.0);
            assert_eq!(scale_fraction(&spec, scale, hi), 1.0);
            assert!((scale_value(&spec, scale, 0.0) - lo).abs() < 1e-9 && (scale_value(&spec, scale, 1.0) - hi).abs() < 1e-9);
            for i in 0..=200 {
                let f = i as f32 / 200.0;
                let back = scale_fraction(&spec, scale, scale_value(&spec, scale, f));
                assert!((back - f).abs() < 1e-5, "{spec:?} {scale:?}: {f} → {back}");
                let v = lo + (hi - lo) * f64::from(f);
                let again = scale_value(&spec, scale, scale_fraction(&spec, scale, v));
                assert!((again - v).abs() <= 1e-4 * (hi - lo), "{spec:?} {scale:?}: {v} → {again}");
            }
            // Hostile input never leaves the range.
            for v in [f64::NAN, f64::INFINITY, -1e300] {
                assert!((0.0..=1.0).contains(&scale_fraction(&spec, scale, v)));
            }
            assert_eq!(scale_value(&spec, scale, f32::NAN), lo);
        }
    }
    // Halfway along a log track is the geometric middle: equal distances multiply.
    assert!((scale_value(&RADIUS, SliderScale::Log, 0.5) - (1.3f64 * 80.0).sqrt()).abs() < 1e-9);
    assert!(scale_fraction(&RADIUS, SliderScale::Log, 3.0) > 0.15, "the low end gets the room");
    // A nudge too small for one step takes one.
    let stepped = SliderSpec { min: 0.0, max: 10.0, step: 1.0, decimals: 0, ..RADIUS };
    assert_eq!(scale_nudged(&stepped, SliderScale::Log, 5.0, 0.0001), 6.0);
    assert_eq!(scale_nudged(&stepped, SliderScale::Log, 5.0, -0.0001), 4.0);
    assert_eq!(scale_nudged(&stepped, SliderScale::Linear, 5.0, f64::NAN), 5.0);
}

struct SliderScene {
    value: f64,
    track: Rect,
}

fn slider_scene(ui: &mut Ui, t: &Tokens, s: &mut SliderScene) {
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(250.0, 60.0)), |ui| {
        let out = effect_slider(ui, t, &RADIUS, s.value, SliderScale::Log);
        if let Some(v) = out.value {
            s.value = v;
        }
        s.track = out.response.rect;
    });
}

#[test]
fn the_effect_slider_maps_its_track_on_its_scale_with_the_label_centred_above() {
    let mut rig = Rig::new((320, 120), SliderScene { value: 3.0, track: Rect::NOTHING }, slider_scene);
    let t = rig.t;
    // The label is centred over the row: as many light pixels left of the middle as right of it.
    let row = Rect::from_min_size(pos2(20.0, 20.0), vec2(250.0, t.metrics.slider_label_h));
    let (mut left, mut right) = (0i32, 0i32);
    for y in row.min.y as u32..row.max.y as u32 {
        for x in row.min.x as u32..row.max.x as u32 {
            if rig.pixel(pos2(x as f32, y as f32))[0] > 120 {
                if (x as f32) < row.center().x { left += 1 } else { right += 1 }
            }
        }
    }
    assert!(left > 10 && right > 10 && (left - right).abs() <= (left + right) / 4, "label ink {left} | {right}");
    // A click halfway along the line (the ring's radius in from each end) is the geometric middle.
    let track = rig.state.track;
    let ring = t.metrics.slider_ring;
    rig.click_at(pos2((track.min.x + ring + 1.0 + track.max.x - ring - 1.0) / 2.0, track.center().y));
    let mid = (1.3f64 * 80.0).sqrt();
    assert!((rig.state.value - mid).abs() < 0.2, "{} vs {mid}", rig.state.value);
    // + moves 2 % of the track: the value is multiplied by (80 / 1.3)^0.02.
    let before = rig.state.value;
    rig.frame(vec![Event::PointerMoved(track.center())]);
    rig.key(Key::Plus);
    let want = scale_value(&RADIUS, SliderScale::Log, scale_fraction(&RADIUS, SliderScale::Log, before) + 0.02);
    assert!((rig.state.value - want).abs() < 1e-6, "{} vs {want}", rig.state.value);
}

// ---- The colour swatch ----

const SWATCH: &str = "color:effect.tint";

struct SwatchScene {
    at: Pos2,
    color: [u8; 3],
    armed: bool,
    out: Option<SwatchOut>,
    eyedropper_clicks: usize,
}

fn swatch_scene(ui: &mut Ui, t: &Tokens, s: &mut SwatchScene) {
    area(ui, Rect::from_min_size(s.at, vec2(40.0, 24.0)), |ui| {
        let out = swatch(ui, t, SWATCH, "Tint color", s.color, s.armed);
        if let Some(c) = out.color {
            s.color = c;
        }
        if out.eyedropper {
            s.eyedropper_clicks += 1;
            s.armed = !s.armed;
        }
        s.out = Some(out);
    });
}

fn swatch_rig(size: (u32, u32), at: Pos2, color: [u8; 3]) -> Rig<SwatchScene> {
    Rig::new(size, SwatchScene { at, color, armed: false, out: None, eyedropper_clicks: 0 }, swatch_scene)
}

#[test]
fn the_field_and_the_strip_are_inverse_to_their_points() {
    for hue in [0.0f32, 30.0, 120.0, 200.0, 300.0] {
        for (x, y) in [(1.0f32, 0.0f32), (0.5, 0.25), (0.25, 0.5), (0.8, 0.1)] {
            let hsv = rgb_to_hsv(field_color(hue, x, y));
            let (px, py) = field_point(hsv);
            assert!((px - x).abs() < 1e-4 && (py - y).abs() < 1e-4, "hue {hue} at ({x}, {y}) → ({px}, {py})");
            assert!((hsv[0] - hue).abs() < 1e-2, "hue {hue} → {}", hsv[0]);
        }
        let y = hue_point(hue);
        assert!((hue_at(y) - hue).abs() < 1e-3 || (hue == 0.0 && hue_at(y) == 0.0), "hue {hue} at {y}");
        let (a, b) = (rgb_to_u8(hue_color(y)), rgb_to_u8(hsv_to_rgb(hue, 1.0, 1.0)));
        assert!(a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= 1), "hue {hue}: {a:?} vs {b:?}");
    }
    assert_eq!(hue_point(f32::NAN), 1.0);
    assert_eq!(field_point([f32::NAN, f32::NAN, 2.0]), (0.0, 0.0));
}

#[test]
fn a_colour_picked_in_the_field_and_the_hue_strip_gives_that_colour_back() {
    let mut rig = swatch_rig((900, 600), pos2(40.0, 40.0), [255, 0, 0]);
    rig.click(SWATCH);
    rig.settle();
    assert!(rig.state.out.as_ref().unwrap().open && swatch_open(&rig.ctx, SWATCH));
    let (field, strip) = (rig.rect(&format!("{SWATCH}/field")), rig.rect(&format!("{SWATCH}/hue")));
    // The strip two thirds down: hue 120, green at full saturation and value.
    rig.click_at(pos2(strip.center().x, strip.min.y + strip.height() * 2.0 / 3.0));
    rig.settle();
    let hue = hue_at(2.0 / 3.0);
    assert_eq!(rig.state.color, rgb_to_u8(hsv_to_rgb(hue, 1.0, 1.0)));
    // Then the field a half across and a quarter down: that saturation and value at that hue.
    rig.click_at(field.min + vec2(field.width() * 0.5, field.height() * 0.25));
    rig.settle();
    assert_eq!(rig.state.color, rgb_to_u8(field_color(hue, 0.5, 0.25)));
    // A drag along the field's top edge to its left end gives white, and the hue survives it.
    rig.drag(field.min + vec2(field.width() * 0.5, 0.0), field.min - vec2(20.0, 20.0));
    rig.settle();
    assert_eq!(rig.state.color, [255, 255, 255]);
    rig.click_at(field.min + vec2(field.width(), 0.0));
    rig.settle();
    assert_eq!(rig.state.color, rgb_to_u8(hsv_to_rgb(hue, 1.0, 1.0)), "the hue survived white");
}

#[test]
fn a_typed_hex_is_taken_and_a_bad_one_refused() {
    let mut rig = swatch_rig((900, 600), pos2(40.0, 40.0), [255, 0, 0]);
    rig.click(SWATCH);
    rig.settle();
    let hex = format!("{SWATCH}/hex");
    rig.click(&hex);
    rig.frame(vec![Event::Text("#336699".into())]);
    rig.key(Key::Enter);
    rig.settle();
    assert_eq!(rig.state.color, [0x33, 0x66, 0x99]);
    rig.click(&hex);
    rig.frame(vec![Event::Text("#33zz99".into())]);
    rig.key(Key::Enter);
    rig.settle();
    assert_eq!(rig.state.color, [0x33, 0x66, 0x99], "a bad hex changes nothing");
    assert!(swatch_open(&rig.ctx, SWATCH), "the popup is still open");
    rig.key(Key::Escape);
    rig.settle();
    assert!(!swatch_open(&rig.ctx, SWATCH), "Esc closes it");
}

#[test]
fn the_eyedropper_arms_and_keeps_the_popup_open_and_a_click_outside_closes_it() {
    let mut rig = swatch_rig((900, 600), pos2(40.0, 40.0), [10, 20, 30]);
    rig.click(SWATCH);
    rig.settle();
    let dropper = format!("{SWATCH}/eyedropper");
    rig.click(&dropper);
    rig.settle();
    assert_eq!(rig.state.eyedropper_clicks, 1);
    assert!(rig.state.armed && rig.find(&dropper).unwrap().active, "armed");
    rig.click_at(pos2(700.0, 500.0));
    rig.settle();
    assert!(swatch_open(&rig.ctx, SWATCH), "the pick does not close the popup");
    rig.state.armed = false;
    rig.click_at(pos2(700.0, 500.0));
    rig.settle();
    assert!(!swatch_open(&rig.ctx, SWATCH) && rig.find(&dropper).is_none(), "a click outside closes it");
    assert_eq!(rig.state.color, [10, 20, 30], "opening and closing changes nothing");
}

#[test]
fn the_popup_stays_inside_a_900_by_600_window_from_a_swatch_at_each_corner() {
    let size = (900, 600);
    let window = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0));
    for at in [pos2(2.0, 2.0), pos2(860.0, 2.0), pos2(2.0, 574.0), pos2(860.0, 574.0), pos2(430.0, 290.0)] {
        let mut rig = swatch_rig(size, at, [0, 128, 255]);
        open_swatch(&rig.ctx, SWATCH);
        rig.settle();
        check_ids(&format!("{at:?}"), &rig.widgets, &registry::unregistered(&rig.ctx));
        let popup = rig.rect(&format!("{SWATCH}/popup"));
        let swatch = rig.rect(SWATCH);
        assert!(window.contains_rect(popup), "from {at:?}: the popup {popup:?} leaves the window");
        assert!(!popup.intersects(swatch.shrink(0.5)), "from {at:?}: the popup covers its swatch");
        for part in ["field", "hue", "hex", "eyedropper"] {
            assert!(popup.contains_rect(rig.rect(&format!("{SWATCH}/{part}"))), "from {at:?}: {part} leaves the popup");
        }
    }
    // Pure placement: below when there is room, above otherwise, the left edge pulled in.
    let size = swatch_popup_size();
    let below = swatch_popup_rect(Rect::from_min_size(pos2(100.0, 100.0), vec2(36.0, 22.0)), size, window);
    assert_eq!(below.min, pos2(100.0, 126.0));
    let above = swatch_popup_rect(Rect::from_min_size(pos2(880.0, 560.0), vec2(36.0, 22.0)), size, window);
    assert_eq!((above.max.x, above.max.y), (896.0, 556.0));
    let tiny = swatch_popup_rect(Rect::from_min_size(pos2(50.0, 50.0), vec2(36.0, 22.0)), size, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)));
    assert!(tiny.min.x.is_finite() && tiny.min.y.is_finite());
}

// ---- The crosshair ----

const PHOTO: Rect = Rect { min: pos2(50.0, 40.0), max: pos2(450.0, 340.0) };

struct CrossScene {
    c: Crosshair,
    out: Vec<CrosshairOut>,
}

fn cross_scene(ui: &mut Ui, t: &Tokens, s: &mut CrossScene) {
    ui.painter().rect_filled(PHOTO, 0, Color32::from_rgb(120, 160, 205));
    let out = crosshair(ui, t, "crosshair:effect", PHOTO, &s.c);
    if let Some(c) = out.centre {
        s.c.centre = c;
    }
    if let Some(h) = out.handle {
        s.c.handle = Some(h);
    }
    if out.drag_started || out.drag_stopped || out.centre.is_some() || out.handle.is_some() {
        s.out.push(out);
    }
}

#[test]
fn the_crosshair_reports_a_drag_of_its_centre_and_of_its_angle_handle() {
    let c = Crosshair { centre: [0.5, 0.5], radius: Some(0.2), handle: Some([0.75, 0.5]) };
    let mut rig = Rig::new((500, 400), CrossScene { c, out: vec![] }, cross_scene);
    check_ids("crosshair", &rig.widgets, &registry::unregistered(&rig.ctx));
    let centre = rig.rect("crosshair:effect").center();
    assert_eq!(centre, pos2(250.0, 190.0));
    // The centre, grabbed 3 pt off its middle, moves by the drag: 40 right, 30 down.
    rig.drag(centre + vec2(3.0, -2.0), centre + vec2(43.0, 28.0));
    let [x, y] = rig.state.c.centre;
    assert!((x - 0.6).abs() < 1e-4 && (y - 0.6).abs() < 1e-4, "centre at ({x}, {y})");
    assert!(rig.state.out.iter().any(|o| o.drag_started) && rig.state.out.iter().any(|o| o.drag_stopped), "one drag, one undo step");
    assert_eq!(rig.state.c.handle, Some([0.75, 0.5]), "the handle stays");
    // The handle, dragged straight down from the centre's right: its angle is now 45° below.
    rig.state.out.clear();
    let handle = rig.rect(&crosshair_handle_id("crosshair:effect")).center();
    assert_eq!(handle, pos2(350.0, 190.0));
    rig.drag(handle, pos2(350.0, 250.0));
    let h = rig.state.c.handle.unwrap();
    assert!((h[0] - 0.75).abs() < 1e-4 && (h[1] - 0.7).abs() < 1e-4, "handle at {h:?}");
    assert!((handle_angle(PHOTO, rig.state.c.centre, h) - 26.565).abs() < 0.01, "{}", handle_angle(PHOTO, rig.state.c.centre, h));
    assert!(rig.state.out.iter().all(|o| o.centre.is_none()), "the centre did not move");
    // Dragged off the photo, the centre stops at its edge.
    let centre = rig.rect("crosshair:effect").center();
    rig.drag(centre, pos2(-200.0, 900.0));
    assert_eq!(rig.state.c.centre, [0.0, 1.0]);
    assert_eq!(handle_angle(PHOTO, [0.5, 0.5], [0.5, 0.5]), 0.0);
    assert_eq!(handle_angle(PHOTO, [0.5, 0.5], [f32::NAN, 0.5]), 0.0);
}

// ---- The gallery ----

#[derive(Default)]
struct Gallery {
    textures: Vec<TextureHandle>,
    photo: Option<TextureHandle>,
}

fn gallery(ui: &mut Ui, t: &Tokens, s: &mut Gallery) {
    if s.textures.is_empty() {
        let tints = [[255, 255, 255], [255, 220, 170], [200, 200, 200], [255, 225, 160], [235, 235, 235], [170, 220, 255]];
        s.textures =
            (0..12).map(|i| ui.ctx().load_texture(format!("g{i}"), preview_image(78, 48, tints[i % tints.len()]), TextureOptions::LINEAR)).collect();
        s.photo = Some(ui.ctx().load_texture("photo", preview_image(300, 200, [255, 255, 255]), TextureOptions::LINEAR));
    }
    let screen = ui.max_rect();
    ui.painter().rect_filled(screen, 0, t.canvas);
    // The photo with the two crosshairs on the left; tab 3's grid in the edit panel on the right,
    // and a parameter panel with an open picker left of it.
    let panel = Rect::from_min_max(pos2(screen.max.x - PANEL_W, screen.min.y + 32.0), screen.max);
    let params = Rect::from_min_max(pos2(panel.min.x - PANEL_W - 8.0, panel.min.y), pos2(panel.min.x - 8.0, screen.max.y));
    ui.painter().rect_filled(panel, 0, t.chrome);
    ui.painter().rect_filled(params, 0, t.chrome);
    let photo_area = Rect::from_min_max(screen.min + vec2(16.0, 48.0), pos2(params.min.x - 16.0, screen.max.y - 16.0));
    let h = (photo_area.width() / 1.5).min(photo_area.height());
    let photo = Rect::from_center_size(photo_area.center(), vec2(h * 1.5, h));
    if let Some(tex) = &s.photo {
        ui.painter().image(tex.id(), photo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    crosshair(ui, t, "crosshair:effect", photo, &Crosshair { centre: [0.35, 0.45], radius: Some(0.18), handle: None });
    crosshair(ui, t, "crosshair:effect.gradient", photo, &Crosshair { centre: [0.7, 0.3], radius: None, handle: Some([0.85, 0.55]) });

    let previews: Vec<Option<egui::TextureId>> = (0..12).map(|i| if i == 11 { None } else { s.textures.get(i).map(TextureHandle::id) }).collect();
    let mut list = tiles(&previews);
    if let Some(last) = list.get_mut(10) {
        last.enabled = false;
    }
    area(ui, panel.shrink2(vec2(6.0, 8.0)), |ui| {
        effect_grid(ui, t, &list);
    });

    area(ui, params.shrink2(vec2(12.0, 10.0)), |ui| {
        ui.label(egui::RichText::new("Glow").font(dalica_ui_kit::fonts::semibold(t.title)).color(t.text));
        ui.add_space(6.0);
        let intensity =
            SliderSpec { id: "slider:effect.intensity", label: "Intensity", min: 0.0, max: 1.0, default: 0.65, step: 0.01, decimals: 2, unit: "" };
        effect_slider(ui, t, &intensity, 0.65, SliderScale::Linear);
        effect_slider(ui, t, &RADIUS, 3.0, SliderScale::Log);
        let fade = SliderSpec { id: "slider:effect.fade", label: "Fade", min: 0.0, max: 100.0, default: 0.0, step: 1.0, decimals: 0, unit: "" };
        ui.add_enabled_ui(false, |ui| effect_slider(ui, t, &fade, 30.0, SliderScale::Linear));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            swatch(ui, t, "color:effect.color", "Color", [0x80, 0xcf, 0xff], true);
            swatch(ui, t, "color:effect.background", "Background", [0xff, 0xff, 0xff], false);
            ui.add_enabled_ui(false, |ui| swatch(ui, t, "color:effect.shadow", "Shadow", [0, 0, 0], false));
        });
    });
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in WINDOWS {
        let what = format!("gallery {size:?}");
        let mut one = Rig::new(size, Gallery::default(), gallery);
        open_swatch(&one.ctx, "color:effect.color");
        one.settle();
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        for id in ["effect:sharpen", "effect:graduated_tint", "slider:effect.radius", "color:effect.color/popup", "crosshair:effect.gradient/handle"]
        {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            let a = one.image();
            let mut two = Rig::new(size, Gallery::default(), gallery);
            open_swatch(&two.ctx, "color:effect.color");
            two.settle();
            let b = two.image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-effects-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
