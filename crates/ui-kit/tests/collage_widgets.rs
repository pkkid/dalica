//! The widgets of the Collage room (`design-system.md` §6, `creating.md` §3):
//!
//! - rich rows: a row too narrow for its description cuts the description short with "…" and keeps
//!   its name whole (a rich dropdown is never narrower than its name needs); the menu opens with a
//!   row per style, the style in use checked, and a click on another picks it and closes the menu;
//! - the toggle group: a click chooses a toggle, a click on the chosen one keeps it (one is always
//!   chosen), and with the focus the arrows move the choice one step, kept at the ends;
//! - the turn-and-scale handle: dragging the wheel a quarter turn round the picture's centre
//!   reports 90°, dragging it out to twice the distance doubles the scale, Shift keeps the scale;
//!   the outline follows the angle;
//! - the gallery: the settings column (the style dropdown open, the borders and the orientation)
//!   and a pile on a page with its selected picture turned, in a 1400 × 900 window
//!   (`fingerprints/gallery-collage-1400x900.json`) and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check};
use dalica_ui_kit::icons::{CollageIcon, Glyph};
use dalica_ui_kit::widgets::{
    ImageToggle, RICH_ROW_H, RichRow, TRANSFORM_OUTLINE, TransformHandle, dropdown_popup_id, rich_dropdown, rich_item, rich_row_min_width,
    rich_row_text, toggle_group, toggle_step, transform_corners, transform_handle, transform_handle_pos,
};
use dalica_ui_kit::{Image, Tokens, WidgetInfo, fonts, registry};
use egui::{Color32, Context, Event, Id, Key, Modifiers, PointerButton, Popup, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// The six styles with their glyphs and descriptions (`creating.md` §3).
const STYLES: [(&str, CollageIcon, &str, &str); 6] = [
    ("pile", CollageIcon::PicturePile, "Picture Pile", "Looks like a pile of scattered pictures"),
    ("mosaic", CollageIcon::Mosaic, "Mosaic", "Fits pictures into the page automatically"),
    ("frame", CollageIcon::FrameMosaic, "Frame Mosaic", "A mosaic around a large center picture"),
    ("grid", CollageIcon::Grid, "Grid", "Pictures in regular rows and columns"),
    ("sheet", CollageIcon::ContactSheet, "Contact Sheet", "Small pictures with their names"),
    ("exposure", CollageIcon::MultipleExposure, "Multiple Exposure", "Pictures blended on top of one another"),
];

const STYLE_ID: &str = "choice:collage.style";

fn row(i: usize) -> RichRow<'static> {
    let (_, glyph, name, description) = STYLES[i.min(STYLES.len() - 1)];
    RichRow { glyph: Glyph::Collage(glyph), name, description }
}

fn style_item_id(i: usize) -> String {
    format!("menuitem:collage.style.{}", STYLES[i.min(STYLES.len() - 1)].0)
}

const BORDERS: [ImageToggle<'static>; 3] = [
    ImageToggle { id: "toggle:collage.border.none", glyph: Glyph::Collage(CollageIcon::NoBorder), tooltip: "No border" },
    ImageToggle { id: "toggle:collage.border.white", glyph: Glyph::Collage(CollageIcon::WhiteBorder), tooltip: "White border" },
    ImageToggle { id: "toggle:collage.border.instant", glyph: Glyph::Collage(CollageIcon::InstantBorder), tooltip: "Instant print border" },
];

const ORIENTATION: [ImageToggle<'static>; 2] = [
    ImageToggle { id: "toggle:collage.landscape", glyph: Glyph::Collage(CollageIcon::Landscape), tooltip: "Landscape" },
    ImageToggle { id: "toggle:collage.portrait", glyph: Glyph::Collage(CollageIcon::Portrait), tooltip: "Portrait" },
];

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

/// A scene drawn each frame with real input.
struct Rig<S> {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    modifiers: Modifiers,
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
        let mut rig =
            Rig { ctx, t, size, clock: 0.0, modifiers: Modifiers::NONE, state, scene, widgets: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        // egui keeps the modifiers from event to event: say them first each frame.
        raw.events = std::iter::once(Event::ModifiersChanged(self.modifiers)).chain(events).collect();
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

    fn press(&self, at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: self.modifiers }
    }

    /// A click at `at`, a second after the last (never a double-click).
    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        let (down, up) = (self.press(at, true), self.press(at, false));
        self.frame(vec![Event::PointerMoved(at), down]);
        self.frame(vec![up]);
    }

    fn click(&mut self, id: &str) {
        let at = self.rect(id).center();
        self.click_at(at);
    }

    /// A drag through `path` (the pointer pressed at the first point, let go at the last).
    fn drag_through(&mut self, path: &[Pos2]) {
        let (Some(&from), Some(&to)) = (path.first(), path.last()) else { return };
        self.clock += 1.0;
        let down = self.press(from, true);
        self.frame(vec![Event::PointerMoved(from), down]);
        for &p in path.iter().skip(1) {
            self.frame(vec![Event::PointerMoved(p)]);
        }
        let up = self.press(to, false);
        self.frame(vec![up]);
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

// ---- Rich rows ----

#[test]
fn a_rich_row_elides_its_description_and_keeps_its_name() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut out = Vec::new();
    let mut frame = ctx.run_ui(dalica_ui_kit::raw_input(400, 300, 1.0), |ui| {
        let p = ui.painter();
        let pile = row(0);
        let min = rich_row_min_width(p, &t, &pile, true);
        out.push((min, rich_row_text(p, &t, &pile, 420.0, true), rich_row_text(p, &t, &pile, 150.0, true), rich_row_text(p, &t, &pile, 10.0, true)));
    });
    frame.textures_delta.clear();
    let [(min, wide, narrow, tiny)] = out.as_slice() else { panic!("{out:?}") };
    assert!(*min < 150.0, "the name and its chevron fit in 150 pt: {min}");
    assert_eq!(wide.name, "Picture Pile");
    assert_eq!(wide.description, "Looks like a pile of scattered pictures");
    assert!(!wide.name_elided && !wide.description_elided, "{wide:?}");
    for cut in [narrow, tiny] {
        assert_eq!(cut.name, "Picture Pile", "the name stays whole: {cut:?}");
        assert!(!cut.name_elided && cut.description_elided && cut.description.ends_with('…'), "{cut:?}");
        assert!(cut.description.starts_with("Looks"), "the description is cut from its end: {cut:?}");
    }
    assert!(tiny.description.len() <= narrow.description.len(), "{tiny:?} vs {narrow:?}");
}

struct StyleScene {
    style: usize,
    width: f32,
    face: Rect,
}

fn style_scene(ui: &mut Ui, t: &Tokens, s: &mut StyleScene) {
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(400.0, 60.0)), |ui| {
        let r = rich_dropdown(ui, t, STYLE_ID, &row(s.style), s.width, |ui| {
            for i in 0..STYLES.len() {
                if rich_item(ui, t, &style_item_id(i), &row(i), i == s.style).clicked() {
                    s.style = i;
                }
            }
        });
        s.face = r.rect;
    });
}

#[test]
fn the_style_menu_opens_with_a_row_per_style_and_a_click_picks_one() {
    let mut rig = Rig::new((900, 600), StyleScene { style: 0, width: 40.0, face: Rect::NOTHING }, style_scene);
    let t = rig.t;
    // Asked for 40 pt, the face takes what its name needs.
    assert_eq!(rig.state.face.height(), RICH_ROW_H);
    assert!(rig.state.face.width() > 40.0, "{:?}", rig.state.face);
    assert_eq!(
        rig.find(STYLE_ID).map(|w| (w.label.clone(), w.active)),
        Some(("Picture Pile: Looks like a pile of scattered pictures".into(), false))
    );
    rig.click(STYLE_ID);
    rig.clock += 1.0; // past the menu's fade-in
    rig.settle();
    check_ids("open", &rig.widgets, &registry::unregistered(&rig.ctx));
    assert!(rig.find(STYLE_ID).unwrap().active, "open");
    let rows: Vec<Rect> = (0..STYLES.len()).map(|i| rig.rect(&style_item_id(i))).collect();
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r.height(), RICH_ROW_H, "row {i}");
        assert_eq!(rig.find(&style_item_id(i)).unwrap().active, i == 0, "only the style in use is checked");
        assert!(r.min.y >= rig.state.face.max.y, "row {i} is below the face");
    }
    // The checked row is filled `tool_active`.
    let checked = rows[0];
    assert_eq!(rig.pixel(pos2(checked.max.x - 5.0, checked.center().y)), t.tool_active.to_array());
    rig.click(&style_item_id(3));
    rig.settle();
    assert_eq!(rig.state.style, 3);
    assert!(!Popup::is_id_open(&rig.ctx, dropdown_popup_id(STYLE_ID)), "a pick closes the menu");
    assert!(rig.find(STYLE_ID).unwrap().label.starts_with("Grid: "));
}

// ---- The toggle group ----

struct ToggleScene {
    chosen: usize,
    changes: Vec<usize>,
}

fn toggle_scene(ui: &mut Ui, t: &Tokens, s: &mut ToggleScene) {
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(300.0, 40.0)), |ui| {
        let out = toggle_group(ui, t, &BORDERS, s.chosen, vec2(36.0, 30.0));
        if let Some(i) = out.chosen {
            s.chosen = i;
            s.changes.push(i);
        }
    });
}

#[test]
fn a_toggle_group_keeps_one_chosen_and_moves_by_the_arrows() {
    let mut rig = Rig::new((400, 120), ToggleScene { chosen: 0, changes: vec![] }, toggle_scene);
    check_ids("toggles", &rig.widgets, &registry::unregistered(&rig.ctx));
    let active: Vec<bool> = BORDERS.iter().map(|b| rig.find(b.id).unwrap().active).collect();
    assert_eq!(active, [true, false, false]);
    let t = rig.t;
    let none = rig.rect(BORDERS[0].id);
    assert_eq!(rig.pixel(none.left_top() + vec2(3.0, 3.0)), t.tool_active.to_array(), "the chosen one is filled");
    rig.click(BORDERS[1].id);
    assert_eq!(rig.state.chosen, 1);
    // A click on the chosen one keeps it.
    rig.click(BORDERS[1].id);
    assert_eq!((rig.state.chosen, rig.state.changes.clone()), (1, vec![1]));
    // With the focus on the row the arrows move the choice, kept at the ends.
    rig.ctx.memory_mut(|m| m.request_focus(Id::new(BORDERS[1].id)));
    rig.settle();
    rig.key(Key::ArrowRight);
    assert_eq!(rig.state.chosen, 2);
    rig.settle();
    rig.key(Key::ArrowRight);
    assert_eq!(rig.state.chosen, 2, "kept at the end");
    rig.key(Key::ArrowLeft);
    rig.settle();
    rig.key(Key::ArrowUp);
    assert_eq!(rig.state.chosen, 0);
    rig.settle();
    assert_eq!(rig.find(BORDERS[0].id).map(|w| w.active), Some(true));
    assert_eq!(rig.state.changes, [1, 2, 1, 0]);
    // The step itself: none chosen steps onto the first, an empty group has none.
    assert_eq!(toggle_step(None, 3, true), Some(0));
    assert_eq!(toggle_step(Some(9), 3, false), Some(1));
    assert_eq!(toggle_step(Some(0), 0, true), None);
}

// ---- The turn-and-scale handle ----

const PICTURE: Rect = Rect { min: pos2(200.0, 200.0), max: pos2(400.0, 300.0) };
const HANDLE_ID: &str = "item:3/turn";

struct TurnScene {
    angle: f32,
    scale: f32,
    started: usize,
    stopped: usize,
}

fn turn_scene(ui: &mut Ui, t: &Tokens, s: &mut TurnScene) {
    ui.painter().rect_filled(ui.max_rect(), 0, t.canvas);
    let rect = Rect::from_center_size(PICTURE.center(), PICTURE.size() * s.scale);
    let out = transform_handle(ui, t, HANDLE_ID, &TransformHandle { rect, angle: s.angle, scale: s.scale });
    if let Some(a) = out.angle {
        s.angle = a;
    }
    if let Some(sc) = out.scale {
        s.scale = sc;
    }
    s.started += usize::from(out.drag_started);
    s.stopped += usize::from(out.drag_stopped);
}

fn turn_rig() -> Rig<TurnScene> {
    Rig::new((600, 500), TurnScene { angle: 0.0, scale: 1.0, started: 0, stopped: 0 }, turn_scene)
}

/// Points along the arc from `from` round `centre` by `deg` (clockwise on the screen), its radius
/// growing by `grow`.
fn arc(centre: Pos2, from: Pos2, deg: f32, grow: f32) -> Vec<Pos2> {
    (0..=8)
        .map(|i| {
            let f = i as f32 / 8.0;
            let (s, c) = (deg * f).to_radians().sin_cos();
            let d = (from - centre) * (1.0 + (grow - 1.0) * f);
            centre + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
        })
        .collect()
}

#[test]
fn dragging_the_handle_a_quarter_turn_reports_90_degrees_and_dragging_it_out_doubles_the_scale() {
    let mut rig = turn_rig();
    check_ids("turn", &rig.widgets, &registry::unregistered(&rig.ctx));
    let centre = PICTURE.center();
    let wheel = rig.rect(HANDLE_ID).center();
    assert_eq!(wheel, PICTURE.right_top(), "the wheel sits on the top-right corner");
    rig.drag_through(&arc(centre, wheel, 90.0, 1.0));
    assert!((rig.state.angle - 90.0).abs() < 0.05, "a quarter turn: {}", rig.state.angle);
    assert!((rig.state.scale - 1.0).abs() < 1e-3, "the same distance keeps the scale: {}", rig.state.scale);
    assert_eq!((rig.state.started, rig.state.stopped), (1, 1), "one drag, one undo step");
    // The wheel followed the corner round.
    let wheel = rig.rect(HANDLE_ID).center();
    assert!((wheel - transform_handle_pos(PICTURE, 90.0)).length() < 0.01, "{wheel:?}");
    // Back to level, then straight out to twice the distance: twice the size.
    rig.state.angle = 0.0;
    rig.settle();
    let wheel = rig.rect(HANDLE_ID).center();
    rig.drag_through(&arc(centre, wheel, 0.0, 2.0));
    assert!((rig.state.scale - 2.0).abs() < 1e-3 && rig.state.angle.abs() < 0.05, "{} at {}°", rig.state.scale, rig.state.angle);
    // Shift keeps the scale: out again and round, only the angle moves.
    rig.modifiers = Modifiers::SHIFT;
    rig.settle();
    let wheel = rig.rect(HANDLE_ID).center();
    rig.drag_through(&arc(centre, wheel, -30.0, 1.5));
    assert!((rig.state.scale - 2.0).abs() < 1e-3, "Shift kept the scale: {}", rig.state.scale);
    assert!((rig.state.angle + 30.0).abs() < 0.05, "{}", rig.state.angle);
}

#[test]
fn the_outline_follows_the_angle() {
    let mut rig = turn_rig();
    rig.state.angle = 30.0;
    rig.settle();
    let accent = rig.t.accent;
    let near = |px: [u8; 4], c: Color32| px[0].abs_diff(c.r()) < 24 && px[1].abs_diff(c.g()) < 24 && px[2].abs_diff(c.b()) < 24;
    let corners = transform_corners(PICTURE, 30.0);
    // Along each turned edge (away from the wheel) the outline is `accent`.
    for (a, b) in [(corners[0], corners[3]), (corners[2], corners[3]), (corners[0], corners[1])] {
        for f in [0.3, 0.5] {
            let at = a + (b - a) * f;
            let found = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).any(|(dx, dy)| {
                let px = rig.pixel(at + vec2(dx as f32, dy as f32));
                near(px, accent)
            });
            assert!(found, "no outline at {at:?} ({TRANSFORM_OUTLINE} pt accent)");
        }
    }
    // Where the unturned outline would be, there is none.
    let left_mid = pos2(PICTURE.min.x, PICTURE.center().y - 30.0);
    assert!(!near(rig.pixel(left_mid), accent), "the unturned edge is not drawn");
    let wheel = rig.rect(HANDLE_ID).center();
    assert!((wheel - corners[1]).length() < 0.01, "the wheel turned with the outline: {wheel:?}");
}

// ---- The gallery ----

/// The pictures of the gallery's pile: centre, size, angle and colour.
const PILE: [(Pos2, [f32; 2], f32, [u8; 3]); 4] = [
    (pos2(0.30, 0.38), [0.34, 0.36], -9.0, [120, 160, 205]),
    (pos2(0.62, 0.34), [0.30, 0.42], 7.0, [196, 150, 96]),
    (pos2(0.40, 0.68), [0.36, 0.34], 4.0, [92, 140, 92]),
    (pos2(0.70, 0.66), [0.32, 0.38], -14.0, [170, 120, 160]),
];

fn gallery(ui: &mut Ui, t: &Tokens, _: &mut ()) {
    let screen = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(screen, 0, t.canvas);
    // The settings column on the left; the page on the right.
    let column = Rect::from_min_max(screen.min + vec2(0.0, 32.0), pos2(screen.min.x + 320.0, screen.max.y));
    p.rect_filled(column, 0, t.chrome);
    area(ui, column.shrink2(vec2(12.0, 10.0)), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let label = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).font(fonts::regular(t.body)).color(t.text_label));
        };
        label(ui, "Picture borders");
        toggle_group(ui, t, &BORDERS, 1, vec2(40.0, 32.0));
        label(ui, "Page format");
        toggle_group(ui, t, &ORIENTATION, 0, vec2(40.0, 32.0));
        label(ui, "Unavailable for this style");
        ui.add_enabled_ui(false, |ui| {
            let off: Vec<ImageToggle<'_>> = BORDERS.iter().map(|b| ImageToggle { id: "toggle:collage.disabled", ..*b }).take(1).collect();
            toggle_group(ui, t, &off, 0, vec2(40.0, 32.0));
        });
        ui.add_space(8.0);
        label(ui, "Style");
        rich_dropdown(ui, t, STYLE_ID, &row(0), 296.0, |ui| {
            for i in 0..STYLES.len() {
                ui.add_enabled_ui(i != 5, |ui| rich_item(ui, t, &style_item_id(i), &row(i), i == 0));
            }
        });
    });
    let room = Rect::from_min_max(pos2(column.max.x + 24.0, screen.min.y + 56.0), screen.max - vec2(24.0, 24.0));
    let h = (room.width() / (16.0 / 9.0)).min(room.height());
    let page = Rect::from_center_size(room.center(), vec2(h * 16.0 / 9.0, h));
    p.rect_filled(page, 0, Color32::from_rgb(58, 58, 58));
    for (i, &(c, [w, hh], angle, rgb)) in PILE.iter().enumerate() {
        let centre = pos2(page.min.x + c.x * page.width(), page.min.y + c.y * page.height());
        let rect = Rect::from_center_size(centre, vec2(w * page.height(), hh * page.height()));
        let corners = transform_corners(rect.expand(6.0), angle);
        p.add(egui::Shape::convex_polygon(corners.to_vec(), Color32::from_rgb(240, 240, 236), egui::Stroke::NONE));
        p.add(egui::Shape::convex_polygon(transform_corners(rect, angle).to_vec(), Color32::from_rgb(rgb[0], rgb[1], rgb[2]), egui::Stroke::NONE));
        if i == PILE.len() - 1 {
            transform_handle(ui, t, HANDLE_ID, &TransformHandle { rect: rect.expand(6.0), angle, scale: 1.0 });
        }
    }
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in WINDOWS {
        let what = format!("gallery {size:?}");
        let open = |ctx: &Context| Popup::open_id(ctx, dropdown_popup_id(STYLE_ID));
        let mut one = Rig::new(size, (), gallery);
        open(&one.ctx);
        one.clock += 1.0; // past the menu's fade-in
        one.settle();
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        for id in [STYLE_ID, "menuitem:collage.style.exposure", "toggle:collage.border.instant", "toggle:collage.portrait", HANDLE_ID] {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            let a = one.image();
            let mut two = Rig::new(size, (), gallery);
            open(&two.ctx);
            two.clock += 1.0;
            two.settle();
            let b = two.image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-collage-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
