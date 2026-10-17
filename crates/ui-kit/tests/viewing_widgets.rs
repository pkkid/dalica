//! The widgets of the rooms, the slideshow, the photo viewer and the import tray
//! (`design-system.md` §6, `ui-layout.md` §8, `viewing.md` §1.2–§1.3 and §3.1, `organizing.md` §3):
//!
//! - room tabs: Library, Import and Collage fit their strip whole at both window sizes, 190 wide,
//!   and Library alone has no ×; more tabs shrink with their labels cut from their end, then go to
//!   the overflow menu with the active one kept; a click opens a tab and a click on its × closes it
//!   (and does not open it); the active tab joins the content below;
//! - the stepper stops at its ends (its button there disabled and deaf) and steps in between;
//! - the caption plate keeps a short caption on one line, wraps a long one at two lines and cuts it
//!   short, never wider than 70 % of its area, and shows nothing for a blank caption;
//! - the overlay bar's opacity follows a virtual clock (painted as it fades, gone at 0);
//! - the import tray's group header selects its group from its clock, and the excluded ⊘ dims its
//!   cell;
//! - the overlay colours read on the brightest photo (WCAG, as `tests/contrast.rs` measures);
//! - the gallery: the Import room, a slideshow frame with its caption and bar, and the viewer's bar
//!   in a 1400 × 900 window (`fingerprints/gallery-viewing-1400x900.json`) and, checked for fit,
//!   900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check};
use dalica_ui_kit::icons::{EditIcon, Glyph, ViewingIcon};
use dalica_ui_kit::theme::{contrast, over};
use dalica_ui_kit::widgets::{
    CAPTION_PLATE_WIDTH, CaptionPlateOut, GroupHeaderOut, OVERLAY_PAD, OverlayFade, ROOM_TAB_MIN_WIDTH, RoomTab, RoomTabsOut, StepperOut,
    caption_plate, excluded_mark, group_header, group_select_id, overlay_bar, overlay_bar_rect, overlay_button, room_tab_close_id, room_tabs,
    stepper, stepper_ids, tabs_overflow_id,
};
use dalica_ui_kit::{Icon, Image, Role, Tokens, WidgetInfo, registry};
use egui::{Color32, Context, Event, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const STRIP: &str = "tab:rooms";

const THREE: [RoomTab<'static>; 3] = [
    RoomTab { name: "library", icon: Glyph::Viewing(ViewingIcon::Library), label: "Library", closable: false },
    RoomTab { name: "import", icon: Glyph::Shell(Icon::Import), label: "Import", closable: true },
    RoomTab { name: "collage", icon: Glyph::Shell(Icon::Collage), label: "Collage", closable: true },
];

/// Library and fifteen rooms with long names: more than any window holds whole.
const MANY: [RoomTab<'static>; 16] = {
    const fn room(name: &'static str, label: &'static str) -> RoomTab<'static> {
        RoomTab { name, icon: Glyph::Shell(Icon::Collage), label, closable: true }
    }
    [
        RoomTab { name: "library", icon: Glyph::Viewing(ViewingIcon::Library), label: "Library", closable: false },
        room("import", "Import from Removable drive (E:)"),
        room("c1", "Collage of the summer holidays"),
        room("c2", "Collage of the garden in spring"),
        room("c3", "Collage of the wedding reception"),
        room("c4", "Collage of the school concert"),
        room("c5", "Collage of the mountain walk"),
        room("c6", "Collage of the birthday party"),
        room("c7", "Collage of the city by night"),
        room("c8", "Collage of the harbour at dawn"),
        room("c9", "Collage of the winter market"),
        room("c10", "Collage of the family picnic"),
        room("c11", "Collage of the river in flood"),
        room("c12", "Collage of the old town walls"),
        room("c13", "Collage of the first snow"),
        room("c14", "Collage of the long drive north"),
    ]
};

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

fn row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), add);
}

fn rect_of(w: &WidgetInfo) -> Rect {
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
}

/// A scene drawn each frame with real input, on a virtual clock.
struct Rig<S> {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    state: S,
    scene: fn(&mut Ui, &Tokens, f64, &mut S),
    widgets: Vec<WidgetInfo>,
    canvas: Canvas,
}

impl<S> Rig<S> {
    fn new(size: (u32, u32), state: S, scene: fn(&mut Ui, &Tokens, f64, &mut S)) -> Rig<S> {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig { ctx, t, size, clock: 0.0, state, scene, widgets: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame_dt(&mut self, events: Vec<Event>, dt: f64) {
        self.clock += dt;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let (t, scene, now) = (self.t, self.scene, self.clock);
        let state = &mut self.state;
        let out = self.ctx.run_ui(raw, |ui| scene(ui, &t, now, state));
        self.widgets = registry::widgets(&self.ctx);
        self.canvas.keep(out);
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.frame_dt(events, 1.0 / 60.0);
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame(vec![]);
        }
    }

    fn find(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn info(&self, id: &str) -> WidgetInfo {
        self.find(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>())).clone()
    }

    fn rect(&self, id: &str) -> Rect {
        rect_of(&self.info(id))
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// A click at `at`, without settling (each frame after it still runs the scene).
    fn click_at(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
    }

    fn click(&mut self, id: &str) {
        let at = self.rect(id).center();
        self.click_at(at);
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

// ---- Room tabs ----

struct RoomsScene {
    items: &'static [RoomTab<'static>],
    active: usize,
    out: Option<RoomTabsOut>,
    clicked: Vec<usize>,
    closed: Vec<usize>,
}

fn rooms_scene(ui: &mut Ui, t: &Tokens, _now: f64, s: &mut RoomsScene) {
    let screen = ui.max_rect();
    area(ui, screen, |ui| {
        let out = room_tabs(ui, t, STRIP, s.items, s.active);
        if let Some(i) = out.clicked {
            s.clicked.push(i);
            s.active = i;
        }
        if let Some(i) = out.closed {
            s.closed.push(i);
        }
        // The content below: the toolbar's `chrome`.
        ui.painter().rect_filled(Rect::from_min_max(out.rect.left_bottom(), screen.max), 0, t.chrome);
        s.out = Some(out);
    });
}

fn rooms_rig(size: (u32, u32), items: &'static [RoomTab<'static>], active: usize) -> Rig<RoomsScene> {
    Rig::new(size, RoomsScene { items, active, out: None, clicked: vec![], closed: vec![] }, rooms_scene)
}

#[test]
fn three_room_tabs_fit_at_both_window_sizes_and_library_has_no_close() {
    for size in WINDOWS {
        let what = format!("{size:?}");
        let mut rig = rooms_rig(size, &THREE, 1);
        check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
        let out = rig.state.out.clone().unwrap();
        assert_eq!(out.rect.height(), 28.0, "{what}");
        assert!(out.fit.overflow.is_empty(), "{what}: {:?}", out.fit);
        assert_eq!(out.shown.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1, 2], "{what}");
        for (shown, item) in out.shown.iter().zip(THREE) {
            assert_eq!(shown.rect.size(), vec2(190.0, 28.0), "{what}: {item:?}");
            assert_eq!((shown.label.as_str(), shown.elided), (item.label, false), "{what}");
            let w = rig.info(&format!("{STRIP}/{}", item.name));
            assert_eq!((w.role, w.label.as_str(), w.active), (Role::Tab, item.label, item.name == "import"), "{what}");
            let close = rig.find(&room_tab_close_id(STRIP, item.name));
            assert_eq!(close.is_some(), item.closable, "{what}: {}'s close ×", item.name);
            if let Some(c) = close {
                assert_eq!((c.role, c.label.as_str()), (Role::Button, format!("Close {}", item.label).as_str()));
                assert!(shown.rect.contains_rect(rect_of(c)), "{what}: the × leaves its tab");
            }
        }
        assert!(rig.find(&room_tab_close_id(STRIP, "library")).is_none(), "{what}: Library has no ×");
        // The active tab is the content's `chrome`, joined to it; the others are on the strip's `canvas`.
        let active = out.shown[1].rect;
        assert_eq!(rig.pixel(active.center_bottom() + vec2(0.0, -2.0)), rig.t.chrome.to_array(), "{what}");
        assert_eq!(rig.pixel(out.shown[2].rect.center_bottom() + vec2(0.0, -2.0)), rig.t.canvas.to_array(), "{what}");
    }
}

#[test]
fn more_room_tabs_elide_then_overflow_keeping_the_active_one() {
    // Seven tabs in 900: each shrinks (still at least the minimum), the long labels cut from their end.
    let what = "7 in 900";
    let items: &'static [RoomTab<'static>] = &MANY[..7];
    let rig = rooms_rig((900, 600), items, 3);
    check_ids(what, &rig.widgets, &registry::unregistered(&rig.ctx));
    let out = rig.state.out.clone().unwrap();
    assert!(out.fit.overflow.is_empty(), "{what}: {:?}", out.fit);
    assert_eq!(out.shown.len(), 7);
    for shown in &out.shown {
        let whole = items[shown.index].label;
        assert!(shown.rect.width() < 190.0 && shown.rect.width() >= ROOM_TAB_MIN_WIDTH, "{what}: {:?}", shown.rect);
        if shown.index == 0 {
            assert_eq!((shown.label.as_str(), shown.elided), ("Library", false));
        } else {
            assert!(shown.elided, "{what}: {whole:?} fits whole in {}", shown.rect.width());
            let start = shown.label.strip_suffix('…').unwrap_or_else(|| panic!("{what}: {:?} is not cut at its end", shown.label));
            assert!(!start.is_empty() && whole.starts_with(start), "{what}: {:?} is not the start of {whole:?}", shown.label);
            assert_eq!(rig.info(&format!("{STRIP}/{}", items[shown.index].name)).label, whole, "the accessible name stays whole");
        }
    }
    // Sixteen in either window: the tabs that do not fit go to the menu, the active one (the last)
    // stays on the strip, and the strip never runs past its end.
    for size in WINDOWS {
        let what = format!("16 in {size:?}");
        let mut rig = rooms_rig(size, &MANY, 15);
        check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
        let out = rig.state.out.clone().unwrap();
        assert!(!out.fit.overflow.is_empty(), "{what}: {:?}", out.fit);
        assert!(out.shown.iter().any(|s| s.index == 15), "{what}: the active tab left the strip: {:?}", out.fit);
        assert!(!out.fit.overflow.contains(&15));
        let more = rig.rect(&tabs_overflow_id(STRIP));
        assert!(out.shown.iter().all(|s| s.rect.max.x <= more.min.x + 0.01), "{what}: a tab under the overflow button");
        assert!(more.max.x <= size.0 as f32 + 0.01, "{what}");
        // A tab picked from the menu opens and comes onto the strip.
        let hidden = out.fit.overflow[0];
        rig.click(&tabs_overflow_id(STRIP));
        rig.settle();
        rig.click(&format!("{STRIP}/{}", MANY[hidden].name));
        rig.settle();
        assert_eq!(rig.state.active, hidden, "{what}");
        assert!(rig.state.out.as_ref().unwrap().shown.iter().any(|s| s.index == hidden), "{what}: the picked tab is on the strip");
    }
}

#[test]
fn a_click_opens_a_room_tab_and_its_close_closes_without_opening() {
    let mut rig = rooms_rig((1400, 900), &THREE, 0);
    rig.click(&format!("{STRIP}/collage"));
    rig.settle();
    assert_eq!((rig.state.clicked.as_slice(), rig.state.active), (&[2][..], 2));
    assert!(rig.info(&format!("{STRIP}/collage")).active);
    rig.click(&room_tab_close_id(STRIP, "import"));
    rig.settle();
    assert_eq!(rig.state.closed, [1]);
    assert_eq!(rig.state.clicked, [2], "a click on the × does not open the tab");
    assert_eq!(rig.state.active, 2);
}

// ---- Stepper ----

struct StepScene {
    value: i32,
    enabled: bool,
    outs: Vec<StepperOut>,
}

const STEP: &str = "stepper:slideshow.displayTime";

fn seconds(v: i32) -> String {
    if v == 1 { "1 second".to_owned() } else { format!("{v} seconds") }
}

fn step_scene(ui: &mut Ui, t: &Tokens, _now: f64, s: &mut StepScene) {
    ui.painter().rect_filled(ui.max_rect(), 0, t.overlay_bar);
    row(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(300.0, 40.0)), |ui| {
        ui.add_enabled_ui(s.enabled, |ui| {
            let out = stepper(ui, t, STEP, s.value, 1..=60, seconds);
            s.value = out.value;
            s.outs.push(out);
        });
    });
}

#[test]
fn the_stepper_stops_at_its_ends() {
    let (minus, plus) = stepper_ids(STEP);
    let mut rig = Rig::new((360, 80), StepScene { value: 3, enabled: true, outs: vec![] }, step_scene);
    check_ids("stepper", &rig.widgets, &registry::unregistered(&rig.ctx));
    assert_eq!(rig.info(STEP).label, "3 seconds");
    rig.click(&plus);
    rig.settle();
    assert_eq!(rig.state.value, 4);
    assert!(rig.state.outs.iter().filter(|o| o.changed).count() == 1, "one click, one change");
    rig.click(&minus);
    rig.click(&minus);
    rig.click(&minus);
    rig.settle();
    // At the bottom end: − is disabled and a click there changes nothing; + still works.
    assert_eq!(rig.state.value, 1);
    assert_eq!(rig.info(STEP).label, "1 second");
    assert!(!rig.info(&minus).enabled && rig.info(&plus).enabled);
    rig.click(&minus);
    rig.settle();
    assert_eq!(rig.state.value, 1);
    // At the top end the same for +; a value outside the range comes in at its end.
    rig.state.value = 99;
    rig.settle();
    assert_eq!(rig.state.value, 60);
    assert!(rig.info(&minus).enabled && !rig.info(&plus).enabled);
    rig.click(&plus);
    rig.settle();
    assert_eq!(rig.state.value, 60);
    // The value's width is the widest value's, so the + button never moves.
    let at_60 = rig.rect(&plus);
    rig.state.value = 1;
    rig.settle();
    assert_eq!(rig.rect(&plus), at_60);
    // Disabled, neither button takes a click.
    rig.state.enabled = false;
    rig.state.value = 5;
    rig.settle();
    assert!(!rig.info(&minus).enabled && !rig.info(&plus).enabled);
    rig.click(&plus);
    rig.settle();
    assert_eq!(rig.state.value, 5);
}

// ---- Caption plate ----

struct CaptionScene {
    text: &'static str,
    area: Rect,
    out: Option<CaptionPlateOut>,
}

fn caption_scene(ui: &mut Ui, t: &Tokens, _now: f64, s: &mut CaptionScene) {
    ui.painter().rect_filled(s.area, 0, t.viewer_bg);
    s.out = caption_plate(ui.painter(), t, "caption:slideshow", s.area, 96.0, s.text);
}

const SHORT: &str = "Grandmother's garden in June";
const LONG: &str = "The whole family on the terrace after lunch, with the old cherry tree behind us and the dog asleep under the table, \
     the evening the cousins arrived from the coast and nobody wanted to go inside even after the sun went down";

#[test]
fn the_caption_plate_wraps_at_two_lines_then_cuts_short() {
    for size in WINDOWS {
        let what = format!("{size:?}");
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        let mut rig = Rig::new(size, CaptionScene { text: SHORT, area: screen, out: None }, caption_scene);
        let short = rig.state.out.clone().unwrap();
        assert_eq!((short.lines.as_slice(), short.elided), (&[SHORT.to_owned()][..], false), "{what}");
        assert!((short.rect.center().x - screen.center().x).abs() <= 1.0, "{what}: centred");
        assert!((short.rect.max.y - (screen.max.y - 96.0)).abs() <= 1.0, "{what}: {:?}", short.rect);
        assert_eq!(rig.info("caption:slideshow").label, SHORT);
        rig.state.text = LONG;
        rig.settle();
        let long = rig.state.out.clone().unwrap();
        assert_eq!(long.lines.len(), 2, "{what}: {:?}", long.lines);
        assert!(long.elided && long.lines[1].ends_with('…'), "{what}: {:?}", long.lines);
        assert!(long.rect.width() <= CAPTION_PLATE_WIDTH * screen.width() + 0.5, "{what}: {} wide", long.rect.width());
        assert!(long.rect.height() > short.rect.height() * 1.5, "{what}: two lines are taller than one");
        assert_eq!(rig.info("caption:slideshow").label, LONG, "the accessible label is the whole caption");
        // The plate is the caption plate's colour over the black screen, the text white.
        let corner = long.rect.min + vec2(8.0, 3.0);
        assert_eq!(rig.pixel(corner), over(rig.t.caption_plate, rig.t.viewer_bg).to_array(), "{what}");
        let lit = {
            let img = rig.image();
            let r = long.rect;
            (r.min.y as u32..r.max.y as u32)
                .flat_map(|y| (r.min.x as u32..r.max.x as u32).map(move |x| (x, y)))
                .filter(|&(x, y)| img.pixel(x, y).unwrap()[0] > 200)
                .count()
        };
        assert!(lit > 400, "{what}: {lit} text pixels");
        // A blank caption shows nothing.
        rig.state.text = "  \n ";
        rig.settle();
        assert!(rig.state.out.is_none() && rig.find("caption:slideshow").is_none(), "{what}");
    }
}

// ---- Overlay bar ----

struct BarScene {
    fade: OverlayFade,
    opacity: f32,
    bar: Option<Rect>,
}

const BAR: &str = "overlay:slideshow";

fn bar_scene(ui: &mut Ui, t: &Tokens, now: f64, s: &mut BarScene) {
    let screen = ui.max_rect();
    // The brightest photo there could be: the worst case for the bar's contrast.
    ui.painter().rect_filled(screen, 0, Color32::WHITE);
    s.opacity = s.fade.opacity(now);
    let rect = overlay_bar_rect(screen, vec2(240.0, 48.0));
    s.bar = overlay_bar(ui, t, BAR, rect, s.opacity, |ui| {
        overlay_button(ui, t, "button:slideshow.exit", ViewingIcon::Exit.into(), Some("Exit"), false, true, "Exit (Esc)");
        overlay_button(ui, t, "button:slideshow.play", ViewingIcon::Pause.into(), None, false, true, "Pause (Space)");
    })
    .map(|r| r.response.rect);
}

#[test]
fn the_overlay_bars_opacity_follows_a_virtual_clock() {
    let mut rig = Rig::new((400, 160), BarScene { fade: OverlayFade::new(false), opacity: 0.0, bar: None }, bar_scene);
    // Hidden: nothing drawn, registered or clickable.
    assert_eq!(rig.state.opacity, 0.0);
    assert!(rig.state.bar.is_none() && rig.find(BAR).is_none() && rig.find("button:slideshow.exit").is_none());
    let probe = pos2(200.0, 160.0 - 24.0 - 4.0);
    assert_eq!(rig.pixel(probe), [255, 255, 255, 255]);
    // Shown at t: half-way after 0.1 s, whole after 0.2 s, on the clock the scene is given.
    let t0 = rig.clock;
    rig.state.fade.set(true, t0);
    rig.frame_dt(vec![], 0.1);
    assert!((rig.state.opacity - 0.5).abs() < 1e-3, "{}", rig.state.opacity);
    let half = rig.pixel(probe)[0];
    let expected_half = over(rig.t.overlay_bar.gamma_multiply(0.5), Color32::WHITE).r();
    assert!(half.abs_diff(expected_half) <= 2, "half-way the plate is {half}, not {expected_half}");
    check_ids("bar", &rig.widgets, &registry::unregistered(&rig.ctx));
    assert!(rig.find("button:slideshow.exit").is_some());
    rig.frame_dt(vec![], 0.1);
    assert_eq!(rig.state.opacity, 1.0);
    assert_eq!(rig.pixel(probe)[0], over(rig.t.overlay_bar, Color32::WHITE).r());
    // Its controls sit inside it, its padding in.
    let bar = rig.state.bar.unwrap();
    for id in ["button:slideshow.exit", "button:slideshow.play"] {
        assert!(bar.shrink2(OVERLAY_PAD).expand(0.01).contains_rect(rig.rect(id)), "{id} leaves the bar");
    }
    // Hidden again: it fades out over 0.2 s and is gone.
    let t1 = rig.clock;
    rig.state.fade.set(false, t1);
    rig.frame_dt(vec![], 0.05);
    assert!((rig.state.opacity - 0.75).abs() < 1e-3, "{}", rig.state.opacity);
    assert!(rig.state.fade.fading(rig.clock));
    rig.frame_dt(vec![], 0.2);
    assert_eq!(rig.state.opacity, 0.0);
    assert!(rig.find(BAR).is_none());
    assert_eq!(rig.pixel(probe), [255, 255, 255, 255]);
}

/// The overlay colours read on the brightest photo there is (white), measured as painted
/// (`theme::over`): text 4.5:1, glyphs 3:1.
#[test]
fn the_overlay_colours_read_on_a_white_photo() {
    let t = Tokens::default();
    let bar = over(t.overlay_bar, Color32::WHITE);
    let hover = over(t.overlay_hover, bar);
    let pressed = over(t.overlay_pressed, bar);
    let plate = over(t.caption_plate, Color32::WHITE);
    for (what, fg, bg, min) in [
        ("overlay_text on overlay_bar", t.overlay_text, bar, 4.5),
        ("overlay_text on overlay_hover", t.overlay_text, hover, 4.5),
        ("overlay_text on overlay_pressed", t.overlay_text, pressed, 4.5),
        ("overlay_text on caption_plate", t.overlay_text, plate, 4.5),
        ("overlay_icon on overlay_bar", t.overlay_icon, bar, 3.0),
        ("danger ⊘ on excluded_dim over black", t.danger, over(t.excluded_dim, t.cell), 3.0),
    ] {
        let c = contrast(fg, bg);
        assert!(c >= min, "{what}: {c:.2}:1 < {min}:1");
    }
    // Disabled reads dimmer than rest.
    assert!(contrast(t.overlay_disabled, bar) < contrast(t.overlay_icon, bar));
}

// ---- Group header and the excluded mark ----

#[derive(Default)]
struct TrayScene {
    all_selected: bool,
    outs: Vec<GroupHeaderOut>,
}

const GROUP: &str = "group:import.0";

fn tray_scene(ui: &mut Ui, t: &Tokens, _now: f64, s: &mut TrayScene) {
    let screen = ui.max_rect();
    ui.painter().rect_filled(screen, 0, t.grid_bg);
    area(ui, Rect::from_min_size(pos2(10.0, 10.0), vec2(380.0, 40.0)), |ui| {
        let out = group_header(ui, t, GROUP, "June 4, 2026 · 10:02 – 12:40", "24 pictures", s.all_selected);
        if out.select {
            s.all_selected = true;
        }
        s.outs.push(out);
    });
    let cell = Rect::from_min_size(pos2(20.0, 60.0), vec2(120.0, 120.0));
    let image = Rect::from_min_size(pos2(20.0, 80.0), vec2(120.0, 80.0));
    ui.painter().rect_filled(cell, 0, t.cell);
    ui.painter().rect_filled(image, 0, Color32::from_rgb(200, 210, 220));
    excluded_mark(ui.painter(), t, cell, image);
}

#[test]
fn the_group_header_selects_its_group_and_the_excluded_mark_dims_its_cell() {
    let mut rig = Rig::new((400, 200), TrayScene::default(), tray_scene);
    check_ids("tray", &rig.widgets, &registry::unregistered(&rig.ctx));
    let select = group_select_id(GROUP);
    let header = rig.info(GROUP);
    assert_eq!((header.role, header.label.as_str()), (Role::Other, "June 4, 2026 · 10:02 – 12:40"));
    assert_eq!(header.rect[3], 30.0);
    assert!(!rig.info(&select).active);
    rig.click(&select);
    rig.settle();
    assert!(rig.state.all_selected);
    assert_eq!(rig.state.outs.iter().filter(|o| o.select).count(), 1);
    let w = rig.info(&select);
    assert!(w.active && w.role == Role::Button, "{w:?}");
    // The selected clock is on `tool_active`.
    let clock = rect_of(&w);
    assert_eq!(rig.pixel(clock.min + vec2(2.0, 2.0)), rig.t.tool_active.to_array());
    // The excluded cell: the photo dimmed, the ⊘ in `danger` on its ring, the cell surround dimmed too.
    let photo = rig.pixel(pos2(30.0, 90.0));
    assert_eq!(photo, over(rig.t.excluded_dim, Color32::from_rgb(200, 210, 220)).to_array());
    let surround = rig.pixel(pos2(30.0, 65.0));
    assert_eq!(surround, over(rig.t.excluded_dim, rig.t.cell).to_array());
    // The ring's left edge: 32 across (40 % of 80), its stroke 3.2 wide, round (80, 120).
    let ring = rig.pixel(pos2(80.0 - 16.0 + 1.6, 120.0));
    assert!(ring[0] > 150 && ring[0] > ring[1] + 60, "{ring:?} is not danger red");
}

// ---- The gallery ----

#[derive(Default)]
struct Gallery {
    bars: Vec<Rect>,
}

/// A procedural photo: sky, hills and a sun, letterboxed in `rect`.
fn photo(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, 0, Color32::from_rgb(120, 160, 205));
    let ground = Rect::from_min_max(pos2(rect.min.x, rect.center().y + rect.height() * 0.1), rect.max);
    p.rect_filled(ground, 0, Color32::from_rgb(70, 112, 64));
    p.circle_filled(
        pos2(rect.min.x + rect.width() * 0.75, rect.min.y + rect.height() * 0.25),
        rect.height() * 0.09,
        Color32::from_rgb(250, 228, 150),
    );
}

fn gallery(ui: &mut Ui, t: &Tokens, _now: f64, s: &mut Gallery) {
    let screen = ui.max_rect();
    s.bars.clear();
    area(ui, screen, |ui| {
        let tabs = room_tabs(ui, t, STRIP, &THREE, 1);
        let content = Rect::from_min_max(tabs.rect.left_bottom(), screen.max);
        ui.painter().rect_filled(content, 0, t.chrome);
        let split = content.min.y + (content.height() * 0.58).round();
        // A slideshow frame: the photo on black, its caption and the bar.
        let show = Rect::from_min_max(content.min, pos2(content.max.x, split));
        ui.painter().rect_filled(show, 0, t.viewer_bg);
        let h = show.height() - 40.0;
        photo(ui.painter(), Rect::from_center_size(show.center(), vec2(h * 1.5, h)));
        caption_plate(ui.painter(), t, "caption:slideshow", show, 92.0, SHORT);
        let bar = overlay_bar_rect(show, vec2(640.0, 48.0));
        overlay_bar(ui, t, "overlay:slideshow", bar, 1.0, |ui| {
            overlay_button(ui, t, "button:slideshow.exit", ViewingIcon::Exit.into(), Some("Exit"), false, true, "Exit (Esc)");
            overlay_button(ui, t, "button:slideshow.rotateLeft", Icon::RotateCcw.into(), None, false, true, "Rotate left");
            overlay_button(ui, t, "button:slideshow.rotateRight", Icon::RotateCw.into(), None, false, true, "Rotate right");
            overlay_button(ui, t, "button:slideshow.previous", ViewingIcon::Previous.into(), None, false, false, "Previous");
            overlay_button(ui, t, "button:slideshow.play", ViewingIcon::Pause.into(), None, false, true, "Pause (Space)");
            overlay_button(ui, t, "button:slideshow.next", ViewingIcon::Next.into(), None, false, true, "Next");
            overlay_button(ui, t, "button:slideshow.star", Icon::StarOutline.into(), None, false, true, "Star");
            overlay_button(ui, t, "button:slideshow.transition", ViewingIcon::Transition.into(), Some("Dissolve"), false, true, "Transition");
            overlay_button(ui, t, "button:slideshow.captions", ViewingIcon::Captions.into(), None, true, true, "Captions");
            stepper(ui, t, "stepper:slideshow.displayTime", 3, 1..=60, seconds);
        });
        s.bars.push(bar);
        // The import tray: a group header and four cells, the second excluded.
        let lower = Rect::from_min_max(pos2(content.min.x, split), content.max);
        let tray = Rect::from_min_max(lower.min, pos2(lower.min.x + (lower.width() * 0.5).round(), lower.max.y));
        ui.painter().rect_filled(tray, 0, t.grid_bg);
        area(ui, Rect::from_min_max(tray.min + vec2(12.0, 8.0), pos2(tray.max.x - 12.0, tray.min.y + 40.0)), |ui| {
            group_header(ui, t, "group:import.0", "June 4, 2026 · 10:02 – 12:40", "4 pictures", false);
        });
        for i in 0..4 {
            let cell = Rect::from_min_size(pos2(tray.min.x + 24.0 + i as f32 * 104.0, tray.min.y + 52.0), vec2(96.0, 96.0));
            if cell.max.x > tray.max.x {
                break;
            }
            let image = Rect::from_center_size(cell.center(), vec2(88.0, 59.0));
            ui.painter().rect_filled(cell, 0, t.cell);
            photo(ui.painter(), image);
            if i == 1 {
                excluded_mark(ui.painter(), t, cell, image);
            }
        }
        // The viewer's bar over its filmstrip, and compare's buttons.
        let strip = Rect::from_min_max(pos2(tray.max.x, lower.min.y), lower.max);
        ui.painter().rect_filled(strip, 0, t.viewer_overlay);
        let bar = overlay_bar_rect(strip, vec2(330.0, 48.0));
        overlay_bar(ui, t, "overlay:viewer", bar, 1.0, |ui| {
            overlay_button(ui, t, "button:viewer.actualSize", ViewingIcon::ActualSize.into(), None, false, true, "Actual size (1)");
            overlay_button(ui, t, "button:viewer.open", ViewingIcon::OpenInDalica.into(), Some("Open in Dalica"), false, true, "Open in Dalica (E)");
            overlay_button(ui, t, "button:viewer.play", EditIcon::Play.into(), None, false, true, "Play");
            overlay_button(ui, t, "button:viewer.more", Icon::More.into(), None, false, true, "More");
            overlay_button(ui, t, "button:viewer.close", Icon::Close.into(), None, false, true, "Close (Esc)");
        });
        s.bars.push(bar);
        let compare = Rect::from_min_size(pos2(strip.min.x + 8.0, strip.min.y + 8.0), vec2(strip.width() - 16.0, 48.0));
        overlay_bar(ui, t, "overlay:compare", Rect::from_min_size(compare.min, vec2(3.0 * 32.0 + 12.0 + 24.0, 48.0)), 1.0, |ui| {
            overlay_button(ui, t, "button:compare.switchFocus", ViewingIcon::SwitchFocus.into(), None, false, true, "Switch focus");
            overlay_button(ui, t, "button:compare.switchLayout", ViewingIcon::SwitchLayout.into(), None, false, true, "Switch layout");
            overlay_button(ui, t, "button:compare.original", ViewingIcon::ShowOriginal.into(), None, false, true, "Show original");
        });
        s.bars.push(Rect::from_min_size(compare.min, vec2(3.0 * 32.0 + 12.0 + 24.0, 48.0)));
    });
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in WINDOWS {
        let what = format!("gallery {size:?}");
        let mut one = Rig::new(size, Gallery::default(), gallery);
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        // Every control of a bar inside that bar, whole.
        for (prefix, bar) in [("button:slideshow.", 0), ("stepper:slideshow.", 0), ("button:viewer.", 1), ("button:compare.", 2)] {
            let inside = one.state.bars[bar].shrink2(OVERLAY_PAD).expand(0.5);
            for w in one.widgets.iter().filter(|w| w.id.starts_with(prefix)) {
                assert!(inside.contains_rect(rect_of(w)), "{what}: {} at {:?} leaves its bar {inside:?}", w.id, w.rect);
            }
        }
        for id in ["tab:rooms/library", "tab:rooms/import", "tab:rooms/collage", "caption:slideshow", "group:import.0/select"] {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            let a = one.image();
            let b = Rig::new(size, Gallery::default(), gallery).image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-viewing-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
