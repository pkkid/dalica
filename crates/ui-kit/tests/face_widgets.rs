//! The widgets of people (`design-system.md` §6, `organizing.md` §6):
//!
//! - face tiles: a tile shows the placeholder until it is given a picture, then the picture; its
//!   ignore × and its ✓ / × marks show (and register) only when asked, and their clicks are theirs,
//!   not the tile's; a click on the tile is reported and, selected, it draws its outline; a dragged
//!   face's picture fades; sizes add the label and marks rows;
//! - the face box: in edit mode a drag of a corner, an edge or the inside reports the new rectangle
//!   in its own normalised coordinates and never leaves its area; outside edit mode it has no
//!   handles, and the pointer on it lights it `accent`; its label sits under it, or above it at the
//!   area's bottom;
//! - the ✓ / × marks on their own, and the caution "?" badge;
//! - the colours they paint read on what they are painted on;
//! - the gallery: an Unnamed grid of tiles in their states, face boxes on a photo, person rows with
//!   the "?" badge and the new icons, in a 1400 × 900 window
//!   (`fingerprints/gallery-people-1400x900.json`) and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check};
use dalica_ui_kit::icons::{PeopleIcon, paint_people};
use dalica_ui_kit::theme::{contrast, over};
use dalica_ui_kit::widgets::{
    BoxHandle, FACE_BOX_MIN, FaceBox, FaceBoxOut, FaceLabel, FacePicture, FaceTile, SUGGESTION_BADGE, face_box, face_box_handle_id, face_tile,
    face_tile_id, face_tile_size, suggest_mark_ids, suggest_marks, suggestion_badge,
};
use dalica_ui_kit::{Image, Tokens, WidgetInfo, registry};
use egui::{Color32, ColorImage, Context, Event, Modifiers, PointerButton, Pos2, Rect, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];

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

/// A procedural face, `side` square: a head and shoulders in `skin` on a `back` ground, two eyes
/// and a mouth. No photo of anyone.
fn face_image(side: usize, skin: [u8; 3], back: [u8; 3]) -> ColorImage {
    let mut img = ColorImage::filled([side, side], Color32::from_rgb(back[0], back[1], back[2]));
    let s = side as f32;
    let skin = Color32::from_rgb(skin[0], skin[1], skin[2]);
    let dark = Color32::from_rgb(40, 30, 28);
    for y in 0..side {
        for x in 0..side {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let head = ((fx - 0.5 * s) / (0.27 * s)).powi(2) + ((fy - 0.42 * s) / (0.34 * s)).powi(2) <= 1.0;
            let body = ((fx - 0.5 * s) / (0.48 * s)).powi(2) + ((fy - 1.08 * s) / (0.3 * s)).powi(2) <= 1.0;
            let eye = |cx: f32| ((fx - cx * s).powi(2) + (fy - 0.38 * s).powi(2)).sqrt() <= 0.035 * s;
            let mouth = (fy - 0.55 * s).abs() <= 0.012 * s + 0.5 && (fx - 0.5 * s).abs() <= 0.08 * s;
            let c = if head && (eye(0.4) || eye(0.6) || mouth) {
                dark
            } else if head || body {
                skin
            } else {
                continue;
            };
            img.pixels[y * side + x] = c;
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

    fn hover(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(from), Self::press(from, true)]);
        for i in 1..=4 {
            self.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.0))]);
        }
        self.frame(vec![Self::press(to, false)]);
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

// ---- Face tiles ----

const TILE_AT: Pos2 = pos2(40.0, 30.0);

struct TileScene {
    texture: Option<TextureHandle>,
    with_picture: bool,
    label: FaceLabel<'static>,
    ignore: bool,
    marks: bool,
    selected: bool,
    dragged: bool,
    clicks: usize,
    ignored: usize,
    confirmed: usize,
    rejected: usize,
    picture: Rect,
    tile: Rect,
}

impl Default for TileScene {
    fn default() -> TileScene {
        TileScene {
            texture: None,
            with_picture: false,
            label: FaceLabel::None,
            ignore: false,
            marks: false,
            selected: false,
            dragged: false,
            clicks: 0,
            ignored: 0,
            confirmed: 0,
            rejected: 0,
            picture: Rect::NOTHING,
            tile: Rect::NOTHING,
        }
    }
}

const RED: Color32 = Color32::from_rgb(200, 40, 40);

fn tile_scene(ui: &mut Ui, t: &Tokens, s: &mut TileScene) {
    ui.painter().rect_filled(ui.max_rect(), 0, t.grid_bg);
    if s.with_picture && s.texture.is_none() {
        s.texture = Some(ui.ctx().load_texture("red", ColorImage::filled([8, 8], RED), TextureOptions::LINEAR));
    }
    let tile = FaceTile {
        side: 96.0,
        picture: s
            .texture
            .as_ref()
            .filter(|_| s.with_picture)
            .map(|tex| FacePicture { texture: tex.id(), uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)) }),
        label: s.label,
        selected: s.selected,
        focused: s.selected,
        dragged: s.dragged,
        ignore: s.ignore,
        marks: s.marks,
        ..FaceTile::new("7")
    };
    area(ui, Rect::from_min_size(TILE_AT, vec2(200.0, 200.0)), |ui| {
        let out = face_tile(ui, t, &tile);
        if out.response.clicked() {
            s.clicks += 1;
            s.selected = !s.selected;
        }
        s.ignored += usize::from(out.ignore);
        s.confirmed += usize::from(out.confirm);
        s.rejected += usize::from(out.reject);
        s.picture = out.picture;
        s.tile = out.response.rect;
    });
}

#[test]
fn a_tile_shows_its_placeholder_until_given_a_picture() {
    let mut rig = Rig::new((300, 260), TileScene::default(), tile_scene);
    check_ids("placeholder", &rig.widgets, &registry::unregistered(&rig.ctx));
    let t = rig.t;
    let pic = rig.state.picture;
    assert_eq!(pic, Rect::from_min_size(TILE_AT + vec2(4.0, 4.0), vec2(96.0, 96.0)));
    assert_eq!(rig.state.tile.size(), face_tile_size(96.0, false, false));
    assert_eq!(rig.pixel(pic.min + vec2(4.0, 4.0)), t.placeholder.to_array(), "no picture: gray 38");
    // The person drawn on the placeholder, in `text_disabled`.
    let person = (0..96).filter(|&y| rig.pixel(pos2(pic.center().x, pic.min.y + y as f32))[0] > t.placeholder.r() + 20).count();
    assert!(person >= 4, "the placeholder's person: {person} light pixels down the middle");
    assert_eq!(rig.find(&face_tile_id("7")).map(|w| (w.label.as_str(), w.active)), Some(("Unnamed face", false)));

    rig.state.with_picture = true;
    rig.settle();
    for at in [pic.min + vec2(4.0, 4.0), pic.center(), pic.max - vec2(4.0, 4.0)] {
        let px = rig.pixel(at);
        assert!(px[0].abs_diff(RED.r()) <= 2 && px[1].abs_diff(RED.g()) <= 2 && px[2].abs_diff(RED.b()) <= 2, "the picture at {at:?}: {px:?}");
    }
    // Dragged away, the picture fades into the light table.
    rig.state.dragged = true;
    rig.settle();
    let px = rig.pixel(pic.center());
    assert!(px[0] < 130 && px[0] > 60, "the dragged picture at half strength: {px:?}");
}

#[test]
fn the_ignore_cross_and_the_marks_show_only_when_asked_and_take_their_own_clicks() {
    let mut rig = Rig::new((300, 260), TileScene { with_picture: true, label: FaceLabel::Suggested("Ana"), ..TileScene::default() }, tile_scene);
    let [confirm, reject] = suggest_mark_ids(&face_tile_id("7"));
    let ignore = format!("{}/ignore", face_tile_id("7"));
    for id in [&confirm, &reject, &ignore] {
        assert!(rig.find(id).is_none(), "{id} shows unasked");
    }
    assert_eq!(rig.state.tile.size(), face_tile_size(96.0, true, false));
    assert_eq!(rig.find(&face_tile_id("7")).map(|w| w.label.clone()), Some("Ana?".to_owned()), "a suggestion reads Name?");
    // The picture's top-right corner is the picture's own.
    let corner = rig.state.picture.right_top() + vec2(-8.0, 8.0);
    assert!(rig.pixel(corner)[0] > 190, "no × painted unasked");

    rig.state.ignore = true;
    rig.state.marks = true;
    rig.settle();
    check_ids("marks", &rig.widgets, &registry::unregistered(&rig.ctx));
    assert_eq!(rig.state.tile.size(), face_tile_size(96.0, true, true));
    let pic = rig.state.picture;
    let x = rig.rect(&ignore);
    assert_eq!(x, Rect::from_min_size(pos2(pic.max.x - 18.0, pic.min.y + 2.0), vec2(16.0, 16.0)), "the × at the picture's top-right");
    assert!(rig.pixel(corner)[0] < 150, "the × plate darkens the corner");
    let (c, r) = (rig.rect(&confirm), rig.rect(&reject));
    assert!(c.min.y >= pic.max.y + 18.0 && r.min.y == c.min.y && c.max.x < r.min.x, "the marks side by side under the label: {c:?} {r:?}");
    assert!(rig.state.tile.contains_rect(c) && rig.state.tile.contains_rect(r));
    assert!(((c.min.x + r.max.x) / 2.0 - rig.state.tile.center().x).abs() <= 0.5, "centred under the tile");

    rig.click(&ignore);
    rig.click(&confirm);
    rig.click(&reject);
    assert_eq!((rig.state.ignored, rig.state.confirmed, rig.state.rejected), (1, 1, 1));
    assert_eq!(rig.state.clicks, 0, "the parts' clicks are not the tile's");
    assert!(!rig.state.selected);
}

#[test]
fn a_click_selects_a_tile_and_it_draws_its_outline() {
    let mut rig = Rig::new((300, 260), TileScene { with_picture: true, label: FaceLabel::Name("Ana"), ..TileScene::default() }, tile_scene);
    let t = rig.t;
    let pic = rig.state.picture;
    let left_of = pos2(pic.min.x - 1.0, pic.center().y);
    assert_eq!(rig.pixel(left_of), t.grid_bg.to_array(), "no outline at rest");
    let id = face_tile_id("7");
    rig.click(&id);
    rig.settle();
    assert_eq!(rig.state.clicks, 1);
    assert!(rig.state.selected);
    assert_eq!(rig.find(&id).map(|w| w.active), Some(true), "registered as selected");
    assert_eq!(rig.pixel(left_of), t.photo_focus.to_array(), "the anchor's white outline");
    let below = pos2(rig.state.tile.min.x + 1.0, rig.state.tile.max.y - 1.0);
    assert_eq!(rig.pixel(below), t.cell_selected.to_array(), "the selected surround");
    rig.click(&id);
    assert!(!rig.state.selected, "a second click is the caller's to read: here it toggles");
}

// ---- The face box ----

const PHOTO: Rect = Rect { min: pos2(50.0, 40.0), max: pos2(450.0, 340.0) };
const GREY: Color32 = Color32::from_gray(128);

struct BoxScene {
    rect: Rect,
    edit: bool,
    label: Option<&'static str>,
    outs: Vec<FaceBoxOut>,
}

fn box_scene(ui: &mut Ui, t: &Tokens, s: &mut BoxScene) {
    ui.painter().rect_filled(PHOTO, 0, GREY);
    let out = face_box(ui, t, "facebox:7", PHOTO, &FaceBox { rect: s.rect, label: s.label, highlighted: false, edit: s.edit });
    if let Some(r) = out.rect {
        s.rect = r;
    }
    if out.rect.is_some() || out.drag_started || out.drag_stopped {
        s.outs.push(out);
    }
}

fn near(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 1e-4 && (a.max - b.max).length() < 1e-4
}

#[test]
fn a_face_box_dragged_by_a_corner_stays_inside_its_area_and_reports_the_new_rectangle() {
    let start = Rect::from_min_max(pos2(0.25, 0.25), pos2(0.5, 0.5));
    let mut rig = Rig::new((500, 400), BoxScene { rect: start, edit: true, label: None, outs: vec![] }, box_scene);
    check_ids("edit", &rig.widgets, &registry::unregistered(&rig.ctx));
    assert_eq!(rig.rect("facebox:7"), Rect::from_min_max(pos2(150.0, 115.0), pos2(250.0, 190.0)));
    for h in BoxHandle::ALL {
        assert!(rig.find(&face_box_handle_id("facebox:7", h)).is_some(), "{h:?}");
    }
    // The bottom-right corner, 40 right and 30 down: one drag, one undo step.
    let se = rig.rect(&face_box_handle_id("facebox:7", BoxHandle::BottomRight)).center();
    assert_eq!(se, pos2(250.0, 190.0));
    rig.drag(se, se + vec2(40.0, 30.0));
    assert!(near(rig.state.rect, Rect::from_min_max(pos2(0.25, 0.25), pos2(0.6, 0.6))), "{:?}", rig.state.rect);
    assert_eq!(rig.state.outs.iter().filter(|o| o.drag_started).count(), 1);
    assert_eq!(rig.state.outs.iter().filter(|o| o.drag_stopped).count(), 1);
    // Far past the photo's corner, it stops at the photo's edges.
    let se = rig.rect(&face_box_handle_id("facebox:7", BoxHandle::BottomRight)).center();
    rig.drag(se, pos2(900.0, 900.0));
    assert!(near(rig.state.rect, Rect::from_min_max(pos2(0.25, 0.25), pos2(1.0, 1.0))), "{:?}", rig.state.rect);
    let nw = rig.rect(&face_box_handle_id("facebox:7", BoxHandle::TopLeft)).center();
    rig.drag(nw, pos2(-300.0, -300.0));
    assert!(near(rig.state.rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))), "{:?}", rig.state.rect);
    // Dragged past the opposite corner, it keeps its minimum size.
    let nw = rig.rect(&face_box_handle_id("facebox:7", BoxHandle::TopLeft)).center();
    rig.drag(nw, pos2(480.0, 380.0));
    let r = rig.state.rect;
    assert!((r.width() * PHOTO.width() - FACE_BOX_MIN).abs() < 0.01 && (r.height() * PHOTO.height() - FACE_BOX_MIN).abs() < 0.01, "{r:?}");
    assert!((r.max.x - 1.0).abs() < 1e-5 && (r.max.y - 1.0).abs() < 1e-5);
    // The inside moves it whole, and it stays on the photo.
    rig.state.rect = start;
    rig.settle();
    rig.drag(pos2(200.0, 150.0), pos2(-500.0, 160.0));
    assert!(near(rig.state.rect, Rect::from_min_max(pos2(0.0, 0.2833333), pos2(0.25, 0.5333333))), "{:?}", rig.state.rect);
    for w in &rig.widgets {
        assert!(PHOTO.expand(8.0).contains_rect(rect_of(w)), "{} at {:?}", w.id, w.rect);
    }
}

#[test]
fn outside_edit_mode_a_box_has_no_handles_and_lights_under_the_pointer() {
    let start = Rect::from_min_max(pos2(0.25, 0.25), pos2(0.5, 0.5));
    let mut rig = Rig::new((500, 400), BoxScene { rect: start, edit: false, label: Some("Ana"), outs: vec![] }, box_scene);
    check_ids("view", &rig.widgets, &registry::unregistered(&rig.ctx));
    for h in BoxHandle::ALL {
        assert!(rig.find(&face_box_handle_id("facebox:7", h)).is_none(), "{h:?} outside edit mode");
    }
    let edge = pos2(150.0, 150.0);
    let rest = rig.pixel(edge);
    assert!(rest[2].abs_diff(rest[0]) < 10 && rest[0] > 150, "a light edge at rest: {rest:?}");
    assert_eq!(rig.find("facebox:7").map(|w| (w.label.as_str(), w.active)), Some(("Ana", false)));
    rig.hover(pos2(200.0, 150.0));
    let lit = rig.pixel(edge);
    assert!(lit[2] > lit[0] + 60, "an accent edge under the pointer: {lit:?}");
    assert_eq!(rig.find("facebox:7").map(|w| w.active), Some(true));
    // A drag moves nothing outside edit mode.
    rig.drag(pos2(200.0, 150.0), pos2(300.0, 250.0));
    assert_eq!(rig.state.rect, start);
    // The label under the box; at the photo's bottom, above it.
    let label_px = rig.pixel(pos2(186.0, 200.0));
    assert!(label_px[0] < GREY.r(), "the label's plate under the box: {label_px:?}");
    rig.state.rect = Rect::from_min_max(pos2(0.25, 0.9), pos2(0.5, 1.0));
    rig.settle();
    let above = rig.pixel(pos2(186.0, 298.0));
    assert!(above[0] < GREY.r(), "the label's plate above a box at the bottom: {above:?}");
}

// ---- Marks and the badge on their own ----

#[derive(Default)]
struct MarksScene {
    confirmed: usize,
    rejected: usize,
}

fn marks_scene(ui: &mut Ui, _t: &Tokens, s: &mut MarksScene) {
    let t = Tokens::default();
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(100.0, 40.0)), |ui| {
        let out = suggest_marks(ui, &t, "suggestion:7");
        s.confirmed += usize::from(out.confirm);
        s.rejected += usize::from(out.reject);
    });
    area(ui, Rect::from_min_size(pos2(20.0, 60.0), vec2(100.0, 40.0)), |ui| {
        ui.add_enabled_ui(false, |ui| suggest_marks(ui, &t, "suggestion:8"));
    });
}

#[test]
fn the_marks_on_their_own_report_their_clicks_and_disabled_take_none() {
    let mut rig = Rig::new((200, 120), MarksScene::default(), marks_scene);
    check_ids("marks", &rig.widgets, &registry::unregistered(&rig.ctx));
    let [c, r] = suggest_mark_ids("suggestion:7");
    assert_eq!(rig.rect(&c).size(), vec2(20.0, 20.0));
    assert_eq!(rig.rect(&r).min.x - rig.rect(&c).max.x, 6.0);
    rig.click(&c);
    rig.click(&r);
    rig.click(&r);
    assert_eq!((rig.state.confirmed, rig.state.rejected), (1, 2));
    let [c8, _] = suggest_mark_ids("suggestion:8");
    assert_eq!(rig.find(&c8).map(|w| w.enabled), Some(false));
}

#[test]
fn the_suggestion_badge_is_a_caution_disc_with_a_dark_question_mark() {
    let t = Tokens::default();
    let at = Rect::from_min_size(pos2(10.0, 10.0), vec2(SUGGESTION_BADGE, SUGGESTION_BADGE));
    let mut out = Vec::new();
    let img = dalica_ui_kit::snapshot(80, 80, 2.0, &t, |ui| {
        out.push(suggestion_badge(ui.painter(), &t, at));
        for bad in [Rect::NOTHING, Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::NAN, 4.0)), Rect::from_min_size(pos2(5.0, 5.0), vec2(0.0, 9.0))] {
            out.push(suggestion_badge(ui.painter(), &t, bad));
        }
    })
    .unwrap();
    assert_eq!(out.last(), Some(&None));
    assert_eq!(out.first(), Some(&Some(at)));
    // At 2×: the disc's left part is caution, the middle holds the dark mark.
    let caution = img.pixel(2 * 10 + 4, 2 * 17).unwrap();
    assert_eq!(&caution[..3], &[t.caution.r(), t.caution.g(), t.caution.b()], "{caution:?}");
    let dark = (2 * 12..2 * 22).flat_map(|y| (2 * 14..2 * 20).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y).unwrap()[0] < 100).count();
    assert!(dark >= 10, "the question mark: {dark} dark pixels");
    // Every bad rect paints nothing: only the one disc is on the frame.
    let lit = img.rgba.as_chunks::<4>().0.iter().filter(|px| px[0] > 150).count();
    assert!(lit < (2.0 * SUGGESTION_BADGE).powi(2) as usize, "{lit}");
}

// ---- Contrast ----

/// The pairs the new widgets paint, measured as they show (`design-system.md` §9): text 4.5:1,
/// glyphs and marks 3:1.
#[test]
fn the_new_widgets_colours_read_on_what_they_are_painted_on() {
    let t = Tokens::default();
    let tile_hover = over(t.hover.gamma_multiply(0.5), t.grid_bg);
    let label_on_white = over(t.caption_plate, Color32::WHITE);
    let edge_on_white = over(t.face_box_outer, Color32::WHITE);
    let ignore_on_white = over(t.overlay_bar, Color32::WHITE);
    let text = [
        ("a face's name on the light table", t.text_label, t.grid_bg),
        ("a face's name on the People pane", t.text_label, t.chrome),
        ("a hovered face's name", t.text, tile_hover),
        ("a selected face's name", t.text, t.cell_selected),
        ("Name? on the light table", t.text_dim, t.grid_bg),
        ("Name? selected", t.text_dim, t.cell_selected),
        ("Name? on the People pane", t.text_dim, t.chrome),
        ("a face box's label over a white photo", t.overlay_text, label_on_white),
        ("the ? on its caution disc", t.canvas, t.caution),
    ];
    let marks = [
        ("a mark on its button", t.icon, t.button),
        ("a hovered mark", t.text, t.hover),
        ("the ignore × over a white photo", t.overlay_icon, ignore_on_white),
        ("the hovered ignore × over a white photo", t.overlay_text, over(t.overlay_hover, ignore_on_white)),
        ("the caution disc on the tree", t.caution, t.chrome),
        ("a face box's edge on a white photo", edge_on_white, Color32::WHITE),
        ("a face box on a black photo", t.face_box, Color32::BLACK),
        ("a lit face box on a white photo", t.accent, Color32::WHITE),
        ("the selection outline on its surround", t.photo_selected, t.cell_selected),
    ];
    for (what, fg, bg) in text {
        let c = contrast(fg, bg);
        assert!(c >= 4.5, "{what}: {c:.2}:1");
    }
    for (what, fg, bg) in marks {
        let c = contrast(fg, bg);
        assert!(c >= 3.0, "{what}: {c:.2}:1");
    }
}

// ---- The gallery ----

#[derive(Default)]
struct Gallery {
    faces: Vec<TextureHandle>,
    photo: Option<TextureHandle>,
}

/// The tiles of the gallery's Unnamed grid: (id, label, picture, selected, focused, dragged,
/// ignore, marks, enabled).
type Shown = (&'static str, FaceLabel<'static>, bool, bool, bool, bool, bool, bool, bool);

const GRID: [Shown; 8] = [
    ("1", FaceLabel::None, true, false, false, false, true, false, true),
    ("2", FaceLabel::Name("Ana Costa"), true, true, true, false, true, false, true),
    ("3", FaceLabel::Name("Rui"), true, true, false, false, false, false, true),
    ("4", FaceLabel::Suggested("Ana Costa"), true, false, false, false, false, true, true),
    ("5", FaceLabel::Suggested("Rui"), false, false, false, false, true, true, true),
    ("6", FaceLabel::Name("A name far too long for its tile"), true, false, false, true, false, false, true),
    ("7", FaceLabel::None, false, false, false, false, false, false, true),
    ("8", FaceLabel::Name("Disabled"), true, false, false, false, true, false, false),
];

fn gallery(ui: &mut Ui, t: &Tokens, s: &mut Gallery) {
    if s.faces.is_empty() {
        let looks =
            [([224, 182, 150], [70, 92, 120]), ([150, 105, 80], [140, 150, 90]), ([238, 205, 180], [60, 60, 70]), ([196, 150, 112], [120, 80, 70])];
        s.faces = looks
            .iter()
            .enumerate()
            .map(|(i, &(skin, back))| ui.ctx().load_texture(format!("face{i}"), face_image(96, skin, back), TextureOptions::LINEAR))
            .collect();
        let mut photo = ColorImage::filled([300, 200], Color32::from_rgb(120, 140, 160));
        for &(x0, skin) in &[(30usize, [224u8, 182u8, 150u8]), (170, [150, 105, 80])] {
            let face = face_image(110, skin, [120, 140, 160]);
            for y in 0..110 {
                for x in 0..110 {
                    photo.pixels[(y + 60) * 300 + x0 + x] = face.pixels[y * 110 + x];
                }
            }
        }
        s.photo = Some(ui.ctx().load_texture("photo", photo, TextureOptions::LINEAR));
    }
    let screen = ui.max_rect();
    ui.painter().rect_filled(screen, 0, t.chrome);
    // Left: person rows with the icons and the "?" badge. Middle: the Unnamed grid on the light
    // table. Right: a photo with its face boxes.
    let tree = Rect::from_min_max(screen.min + vec2(0.0, 32.0), pos2(screen.min.x + 200.0, screen.max.y));
    let photo_w = (screen.width() * 0.36).round();
    let table = Rect::from_min_max(pos2(tree.max.x, tree.min.y), pos2(screen.max.x - photo_w, screen.max.y));
    let canvas = Rect::from_min_max(pos2(table.max.x, tree.min.y), screen.max);
    ui.painter().rect_filled(table, 0, t.grid_bg);
    ui.painter().rect_filled(canvas, 0, t.canvas);

    let rows: [(PeopleIcon, &str, bool); 6] = [
        (PeopleIcon::People, "People", false),
        (PeopleIcon::Unnamed, "Unnamed (42)", false),
        (PeopleIcon::Person, "Ana Costa (128)", true),
        (PeopleIcon::Person, "Rui (17)", false),
        (PeopleIcon::IgnoredFace, "Ignored (3)", false),
        (PeopleIcon::AddPerson, "Add a Person", false),
    ];
    for (i, (icon, label, pending)) in rows.into_iter().enumerate() {
        let row = Rect::from_min_size(tree.min + vec2(8.0, 8.0 + i as f32 * t.metrics.person_row_h), vec2(184.0, t.metrics.person_row_h));
        let glyph = Rect::from_center_size(pos2(row.min.x + 12.0, row.center().y), vec2(16.0, 16.0));
        paint_people(ui.painter(), glyph, icon, t.icon);
        let text = ui.painter().text(
            pos2(row.min.x + 28.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            dalica_ui_kit::fonts::regular(t.body),
            t.text_label,
        );
        if pending {
            suggestion_badge(
                ui.painter(),
                t,
                Rect::from_min_size(pos2(text.max.x + 6.0, row.center().y - 7.0), vec2(SUGGESTION_BADGE, SUGGESTION_BADGE)),
            );
        }
    }
    // Every glyph of the set, at 16 and 20, under the rows.
    for (i, icon) in PeopleIcon::ALL.into_iter().enumerate() {
        let x = tree.min.x + 10.0 + (i % 6) as f32 * 30.0;
        let y = tree.min.y + 200.0 + (i / 6) as f32 * 30.0;
        paint_people(ui.painter(), Rect::from_min_size(pos2(x, y), vec2(16.0, 16.0)), icon, t.icon);
    }

    area(ui, table.shrink2(vec2(24.0, 16.0)), |ui| {
        ui.label(egui::RichText::new("Unnamed people").font(dalica_ui_kit::fonts::semibold(t.title)).color(t.text));
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 16.0);
            for (i, &(id, label, pic, selected, focused, dragged, ignore, marks, enabled)) in GRID.iter().enumerate() {
                let picture = pic
                    .then(|| s.faces.get(i % s.faces.len()))
                    .flatten()
                    .map(|tex| FacePicture { texture: tex.id(), uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)) });
                let tile = FaceTile { side: 96.0, picture, label, selected, focused, dragged, ignore, marks, ..FaceTile::new(id) };
                ui.add_enabled_ui(enabled, |ui| face_tile(ui, t, &tile));
            }
        });
    });

    let photo_area = canvas.shrink(16.0);
    let h = (photo_area.width() / 1.5).min(photo_area.height());
    let photo = Rect::from_center_size(photo_area.center(), vec2((h * 1.5).round(), h.round()));
    if let Some(tex) = &s.photo {
        ui.painter().image(tex.id(), photo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    let first = Rect::from_min_max(pos2(0.13, 0.33), pos2(0.33, 0.66));
    let second = Rect::from_min_max(pos2(0.6, 0.33), pos2(0.8, 0.66));
    let third = Rect::from_min_max(pos2(0.42, 0.06), pos2(0.54, 0.26));
    face_box(ui, t, "facebox:1", photo, &FaceBox { rect: first, label: Some("Ana Costa"), highlighted: false, edit: false });
    face_box(ui, t, "facebox:2", photo, &FaceBox { rect: second, label: Some("Rui"), highlighted: true, edit: false });
    face_box(ui, t, "facebox:new", photo, &FaceBox { rect: third, label: None, highlighted: false, edit: true });
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in WINDOWS {
        let what = format!("gallery {size:?}");
        let mut one = Rig::new(size, Gallery::default(), gallery);
        // The pointer rests on the first tile: its hovered look.
        let first = one.rect(&face_tile_id("1")).center();
        one.hover(first);
        one.settle();
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        for id in ["face:1", "face:1/ignore", "face:4/confirm", "face:5/reject", "facebox:2", "facebox:new/se"] {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            assert_eq!(one.find("face:8").map(|w| w.enabled), Some(false), "{what}: the disabled tile");
            let a = one.image();
            let mut two = Rig::new(size, Gallery::default(), gallery);
            two.hover(first);
            two.settle();
            let b = two.image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-people-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
