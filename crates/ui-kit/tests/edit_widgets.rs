//! The edit panel's widgets (M7.5; `design-system.md` §6, `editing.md` §2, §5–§7): the tab strip,
//! the tool button, the Tuning slider and the histogram.
//!
//! - every state: the tabs, the tool button and the Tuning slider in rest, hover, pressed, active,
//!   disabled and focused, each a tile rendered with real input (pointer moved, button held, focus
//!   requested), one sheet per widget (`fingerprints/edit-widgets-<name>.json`);
//! - tab fitting: at narrow widths the tabs shrink, then move to the overflow menu, and the active
//!   tab always stays on the strip; a hidden tab is picked from the menu;
//! - the histogram of a known set of bins: the overlap colours where they belong, the outline, the
//!   fingerprint (`fingerprints/edit-widgets-histogram.json`);
//! - the Tuning slider: the value as a tooltip while hovered, + / − nudge 2 %, double-click resets;
//! - the gallery: an edit panel built from these widgets in a window at 1400 × 900 and 900 × 600
//!   (`fingerprints/gallery-edit-<w>x<h>.json`).
//!
//! Every frame registers its ids with their roles, in the grammar of `control-protocol.md` §7, and
//! leaves no interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use std::path::PathBuf;

use dalica_scenes::fingerprint;
use dalica_ui_kit::theme::{HISTOGRAM, histogram_fill};
use dalica_ui_kit::widgets::{
    SliderOut, SliderSpec, TAB_MIN_WIDTH, TOOL_BUTTON_H, TabItem, TabsOut, histogram, histogram_curves, tabs, tabs_overflow_id, tool_button,
    tool_button_width, tuning_slider,
};
use dalica_ui_kit::{Icon, Image, Rasterizer, Role, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Color32, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

// Stand-ins until the edit view's own icons land (M7.6); the strip draws whatever icon it is given.
const TABS: [TabItem<'static>; 5] = [
    TabItem { name: "basic", icon: Icon::Properties, tooltip: "Basic fixes" },
    TabItem { name: "tuning", icon: Icon::Loupe, tooltip: "Lighting and color tuning" },
    TabItem { name: "effects", icon: Icon::Star, tooltip: "Effects" },
    TabItem { name: "moreEffects", icon: Icon::Places, tooltip: "More effects" },
    TabItem { name: "evenMoreEffects", icon: Icon::People, tooltip: "Even more effects" },
];
const STRIP: &str = "tab:edit";

const TOOLS: [(&str, Icon, &str); 6] = [
    ("tool:crop", Icon::Export, "Crop"),
    ("tool:straighten", Icon::RotateCw, "Straighten"),
    ("tool:retouch", Icon::Hold, "Retouch"),
    ("tool:autoFix", Icon::Star, "Auto Fix"),
    ("tool:autoContrast", Icon::Properties, "Auto Contrast"),
    ("tool:autoColor", Icon::Faces, "Auto Color"),
];

const FILL: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.fill", label: "Fill Light", min: 0.0, max: 1.0, default: 0.0, step: 0.001, decimals: 2, unit: "" };

fn ignore<T>(_: T) {}

fn column(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
}

fn row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Min)), add);
}

fn out_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    out
}

/// Writes `img` as `<name>.png`, stores its fingerprint when blessing, and compares it with the
/// stored one.
fn check(name: &str, img: &Image, failures: &mut Vec<String>) {
    let out = out_dir();
    std::fs::write(out.join(format!("{name}.png")), fingerprint::png_rgba(img.width, img.height, &img.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.rgba)) + "\n").unwrap();
    }
    match std::fs::read_to_string(&stored).map_err(|e| e.to_string()).and_then(|s| fingerprint::from_json(&s)) {
        Ok(expected) => {
            if let Err(m) = fingerprint::check(img.width, img.height, &img.rgba, &expected, 2.0) {
                std::fs::write(out.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
                failures.push(format!("{name}: {}", m.message));
            }
        }
        Err(e) => failures.push(format!("{name}: no fingerprint ({e}); run with DALICA_BLESS=1 and look at the PNG")),
    }
}

fn check_ids(what: &str, widgets: &[WidgetInfo], unregistered: &[registry::Unregistered]) {
    assert_eq!(unregistered, [], "{what}: interactive widgets without an id");
    let mut seen = std::collections::HashSet::new();
    for w in widgets {
        assert!(registry::is_valid_id(&w.id), "{what}: {:?} breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{what}: {} registered twice", w.id);
    }
}

// ---- Every state ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Rest,
    Hover,
    Pressed,
    Active,
    Disabled,
    Focused,
}

const STATES: [State; 6] = [State::Rest, State::Hover, State::Pressed, State::Active, State::Disabled, State::Focused];

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Rest => "rest",
            State::Hover => "hover",
            State::Pressed => "pressed",
            State::Active => "active",
            State::Disabled => "disabled",
            State::Focused => "focused",
        }
    }
}

const CAPTION: f32 = 14.0;

struct Sheet {
    name: &'static str,
    tile: (u32, u32),
    /// The id the pointer and the focus go to.
    target: &'static str,
    ids: &'static [(&'static str, Role)],
    draw: fn(&mut Ui, &Tokens, State, Rect),
}

fn sheets() -> Vec<Sheet> {
    vec![
        Sheet {
            name: "tabs",
            tile: (240, 64),
            target: "tab:edit/effects",
            ids: &[("tab:edit/basic", Role::Tab), ("tab:edit/effects", Role::Tab), ("tab:edit/evenMoreEffects", Role::Tab)],
            draw: |ui, t, state, rect| {
                // Active: the target is the shown tab; otherwise the first one is.
                let active = if state == State::Active { 2 } else { 0 };
                column(ui, rect, |ui| ignore(tabs(ui, t, STRIP, &TABS, active)));
                // The panel the active tab joins.
                let below = Rect::from_min_max(pos2(rect.min.x, rect.min.y + t.metrics.edit_tab.y), rect.max);
                ui.painter().rect_filled(below, 0, t.chrome);
            },
        },
        Sheet {
            name: "tool_button",
            tile: (300, 84),
            target: "tool:crop",
            ids: &[("tool:crop", Role::Tool), ("tool:straighten", Role::Tool), ("tool:retouch", Role::Tool)],
            draw: |ui, t, state, rect| {
                let w = tool_button_width(t, rect.width(), 3, 6.0);
                row(ui, rect, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (i, (id, icon, label)) in TOOLS.iter().take(3).enumerate() {
                        ignore(tool_button(ui, t, id, *icon, label, i == 0 && state == State::Active, true, w, ""));
                    }
                });
            },
        },
        Sheet {
            name: "tuning_slider",
            tile: (260, 72),
            target: "slider:tuning.fill",
            ids: &[("slider:tuning.fill", Role::Other)],
            draw: |ui, t, state, rect| {
                let value = if state == State::Active { 0.75 } else { 0.25 };
                column(ui, rect, |ui| ignore(tuning_slider(ui, t, &FILL, value)));
            },
        },
    ]
}

struct Tile {
    image: Image,
    widgets: Vec<WidgetInfo>,
    unregistered: Vec<registry::Unregistered>,
}

/// Two passes to settle, the state's input, then the frame that is kept; the clock runs 0.6 s a
/// pass so tooltips have appeared by then.
fn render_tile(sheet: &Sheet, state: State) -> Tile {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut raster = Rasterizer::new();
    let (w, h) = (sheet.tile.0 as f32, sheet.tile.1 as f32);
    let content = Rect::from_min_max(pos2(8.0, CAPTION + 4.0), pos2(w - 8.0, h - 6.0));
    let draw = |ui: &mut Ui| {
        ui.painter().text(pos2(6.0, 3.0), Align2::LEFT_TOP, state.name(), fonts::regular(t.small), t.text_dim);
        if state == State::Disabled {
            ui.disable();
        }
        (sheet.draw)(ui, &t, state, content);
    };
    let mut target: Option<(Pos2, Option<egui::Id>)> = None;
    let mut last = None;
    for pass in 0..6 {
        let mut events = Vec::new();
        if let Some((at, egui_id)) = target {
            match (state, pass) {
                (State::Hover | State::Pressed, 3) => events.push(Event::PointerMoved(at)),
                (State::Pressed, 4) => {
                    events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE })
                }
                (State::Focused, 3) => {
                    if let Some(id) = egui_id {
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                }
                _ => {}
            }
        }
        let mut input = dalica_ui_kit::raw_input(sheet.tile.0, sheet.tile.1, 1.0);
        input.time = Some(0.6 * pass as f64);
        input.events = events;
        let out = ctx.run_ui(input, &draw);
        last = Some(dalica_ui_kit::paint(&ctx, &mut raster, out, sheet.tile.0, sheet.tile.1, t.chrome).unwrap());
        if pass == 2 {
            let info = registry::find(&ctx, sheet.target).unwrap_or_else(|| panic!("{}: {:?} registers {}", sheet.name, state, sheet.target));
            target = Some((info.center(), info.egui_id));
        }
    }
    Tile { image: last.unwrap(), widgets: registry::widgets(&ctx), unregistered: registry::unregistered(&ctx) }
}

/// Tiles in 3 columns, 1 px `divider` lines between them.
fn stitch(tiles: &[&Image], columns: u32) -> Image {
    let (tw, th) = (tiles[0].width, tiles[0].height);
    let rows = (tiles.len() as u32).div_ceil(columns);
    let (w, h) = (columns * tw + columns - 1, rows * th + rows - 1);
    let mut rgba: Vec<u8> = Tokens::default().divider.to_array().repeat((w * h) as usize);
    for (i, img) in tiles.iter().enumerate() {
        let (cx, cy) = ((i as u32 % columns) * (tw + 1), (i as u32 / columns) * (th + 1));
        for y in 0..th {
            let src = (y * tw * 4) as usize..((y + 1) * tw * 4) as usize;
            let dst = (((cy + y) * w + cx) * 4) as usize;
            rgba[dst..dst + src.len()].copy_from_slice(&img.rgba[src]);
        }
    }
    Image { width: w, height: h, rgba }
}

fn body(tile: &Tile) -> &[u8] {
    let start = (CAPTION as u32 * tile.image.width * 4) as usize;
    &tile.image.rgba[start..]
}

#[test]
fn tabs_tool_buttons_and_the_tuning_slider_in_every_state() {
    let mut failures = Vec::new();
    for sheet in sheets() {
        let tiles: Vec<(State, Tile)> = STATES.iter().map(|&s| (s, render_tile(&sheet, s))).collect();
        let again: Vec<Tile> = STATES.iter().map(|&s| render_tile(&sheet, s)).collect();
        for ((state, a), b) in tiles.iter().zip(&again) {
            assert!(a.image.rgba == b.image.rgba, "{}: two renders of {state:?} differ", sheet.name);
            let what = format!("{} {state:?}", sheet.name);
            check_ids(&what, &a.widgets, &a.unregistered);
            for (id, role) in sheet.ids {
                let w = a.widgets.iter().find(|w| w.id == *id).unwrap_or_else(|| panic!("{what}: {id} registers"));
                assert_eq!(w.role, *role, "{what}: {id}");
                assert_eq!(w.enabled, *state != State::Disabled, "{what}: {id} enabled");
            }
        }
        // Each state really draws differently from rest.
        let rest = &tiles[0].1;
        for (state, tile) in &tiles[1..] {
            assert!(body(rest) != body(tile), "{}: {state:?} looks like rest", sheet.name);
        }
        let images: Vec<&Image> = tiles.iter().map(|(_, t)| &t.image).collect();
        check(&format!("edit-widgets-{}", sheet.name), &stitch(&images, 3), &mut failures);
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}

// ---- Driven: tabs, tool buttons, the Tuning slider ----

/// One scene in a window, driven by real input.
struct Rig {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    raster: Rasterizer,
    widgets: Vec<WidgetInfo>,
    /// The texts painted in the last pass (tooltips included).
    texts: Vec<String>,
    // The scene's state.
    strip_w: f32,
    active: usize,
    tabs_out: TabsOut,
    value: f64,
    slider: Option<SliderOut>,
    tool_clicks: Vec<&'static str>,
    scene: fn(&mut Rig, &mut Ui),
}

impl Rig {
    fn new(size: (u32, u32), scene: fn(&mut Rig, &mut Ui)) -> Rig {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig {
            ctx,
            t,
            size,
            clock: 0.0,
            raster: Rasterizer::new(),
            widgets: vec![],
            texts: vec![],
            strip_w: 280.0,
            active: 0,
            tabs_out: TabsOut::default(),
            value: 0.5,
            slider: None,
            tool_clicks: vec![],
            scene,
        };
        rig.settle();
        rig
    }

    fn pass(&mut self, events: Vec<Event>, dt: f64) -> egui::FullOutput {
        self.clock += dt;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let ctx = self.ctx.clone();
        let scene = self.scene;
        let out = ctx.run_ui(raw, |ui| scene(self, ui));
        self.widgets = registry::widgets(&self.ctx);
        self.texts = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        out
    }

    fn frame_dt(&mut self, events: Vec<Event>, dt: f64) {
        let mut delta = self.pass(events, dt).textures_delta;
        self.raster.set_textures(&mut delta);
        self.raster.free_textures(&mut delta);
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.frame_dt(events, 1.0 / 60.0);
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    fn find(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn center(&self, id: &str) -> Pos2 {
        self.find(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>())).center()
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at), Rig::press(at, true)]);
        self.frame(vec![Rig::press(at, false)]);
        self.settle();
    }

    fn click(&mut self, id: &str) {
        let at = self.center(id);
        self.click_at(at);
    }

    fn hover(&mut self, id: &str) {
        let at = self.center(id);
        self.frame(vec![Event::PointerMoved(at)]);
        self.settle();
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true), ev(false)]);
        self.settle();
    }
}

fn tabs_scene(rig: &mut Rig, ui: &mut Ui) {
    let rect = Rect::from_min_size(pos2(10.0, 30.0), vec2(rig.strip_w, 32.0));
    let (t, active) = (rig.t, rig.active);
    let mut out = TabsOut::default();
    column(ui, rect, |ui| out = tabs(ui, &t, STRIP, &TABS, active));
    if let Some(i) = out.clicked {
        rig.active = i;
    }
    rig.tabs_out = out;
}

fn shown(rig: &Rig) -> Vec<usize> {
    rig.tabs_out.fit.shown.iter().map(|&(i, _)| i).collect()
}

#[test]
fn tabs_fit_narrow_strips_and_keep_the_active_tab() {
    let mut rig = Rig::new((420, 200), tabs_scene);
    // 280 wide: all five at 40 × 32, nothing in a menu.
    assert_eq!(rig.tabs_out.fit.shown, [(0, 40.0), (1, 40.0), (2, 40.0), (3, 40.0), (4, 40.0)]);
    assert!(rig.find(&tabs_overflow_id(STRIP)).is_none());
    for (i, item) in TABS.iter().enumerate() {
        let w = rig.find(&format!("{STRIP}/{}", item.name)).unwrap();
        assert_eq!((w.role, w.active, w.rect), (Role::Tab, i == 0, [10.0 + 40.0 * i as f32, 30.0, 40.0, 32.0]));
        assert_eq!(w.label, item.tooltip);
    }
    // A click selects; the tooltip shows once the pointer rests.
    rig.click("tab:edit/tuning");
    assert_eq!(rig.active, 1);
    assert!(rig.find("tab:edit/tuning").unwrap().active);
    rig.hover("tab:edit/effects");
    rig.frame_dt(vec![], 1.0);
    rig.frame(vec![]);
    assert!(rig.texts.iter().any(|t| t == "Effects"), "the tooltip: {:?}", rig.texts);

    // 150 wide: the five shrink to 30 each, still no menu.
    rig.strip_w = 150.0;
    rig.settle();
    assert_eq!(shown(&rig), [0, 1, 2, 3, 4]);
    assert!(rig.tabs_out.fit.shown.iter().all(|&(_, w)| (w - 30.0).abs() < 0.01), "{:?}", rig.tabs_out.fit);

    // Narrower than five minimum tabs: some move to the menu, the active one stays.
    for (width, active) in [(100.0, 4), (70.0, 3), (40.0, 4), (20.0, 2), (0.0, 1)] {
        rig.strip_w = width;
        rig.active = active;
        rig.settle();
        let fit = &rig.tabs_out.fit;
        assert!(shown(&rig).contains(&active), "{width}: the active tab {active} stays: {fit:?}");
        assert!(!fit.overflow.is_empty(), "{width}: {fit:?}");
        assert!(fit.shown.iter().map(|&(_, w)| w).sum::<f32>() + fit.overflow_width <= width + 0.01);
        if width >= TAB_MIN_WIDTH + 18.0 {
            let w = rig.find(&format!("{STRIP}/{}", TABS[active].name)).unwrap();
            assert!(w.active && w.rect[2] > 0.0, "{width}: {w:?}");
            assert!(rig.find(&tabs_overflow_id(STRIP)).is_some(), "{width}: the overflow button");
        }
        for hidden in &fit.overflow {
            assert!(rig.find(&format!("{STRIP}/{}", TABS[*hidden].name)).is_none(), "{width}: a hidden tab is not on the strip");
        }
    }

    // Pick a hidden tab from the menu.
    rig.strip_w = 70.0;
    rig.active = 0;
    rig.settle();
    let hidden = *rig.tabs_out.fit.overflow.last().unwrap();
    rig.click(&tabs_overflow_id(STRIP));
    let item = format!("{STRIP}/{}", TABS[hidden].name);
    assert!(rig.find(&item).is_some(), "the menu lists {item}: {:?}", rig.widgets.iter().map(|w| &w.id).collect::<Vec<_>>());
    check_ids("overflow menu", &rig.widgets, &registry::unregistered(&rig.ctx));
    rig.click(&item);
    assert_eq!(rig.active, hidden);
    assert!(shown(&rig).contains(&hidden), "now on the strip: {:?}", rig.tabs_out.fit);
}

#[test]
fn tool_buttons_click_and_a_disabled_one_does_not() {
    fn scene(rig: &mut Rig, ui: &mut Ui) {
        let t = rig.t;
        let rect = Rect::from_min_size(pos2(8.0, 8.0), vec2(264.0, 2.0 * TOOL_BUTTON_H + 6.0));
        let w = tool_button_width(&t, rect.width(), t.metrics.tool_columns, 6.0);
        let mut clicked = vec![];
        column(ui, rect, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for chunk in TOOLS.chunks(3) {
                ui.horizontal(|ui| {
                    for (id, icon, label) in chunk {
                        // Auto Color is disabled, Crop open.
                        let enabled = *id != "tool:autoColor";
                        if tool_button(ui, &t, id, *icon, label, *id == "tool:crop", enabled, w, label).clicked() {
                            clicked.push(*id);
                        }
                    }
                });
            }
        });
        rig.tool_clicks.extend(clicked);
    }
    let mut rig = Rig::new((300, 140), scene);
    let crop = rig.find("tool:crop").unwrap().clone();
    assert_eq!((crop.role, crop.active, crop.rect[2], crop.rect[3]), (Role::Tool, true, 84.0, TOOL_BUTTON_H));
    let straighten = rig.find("tool:straighten").unwrap();
    assert_eq!(straighten.rect[0], crop.rect[0] + 84.0 + 6.0, "three to a row");
    assert_eq!(rig.find("tool:autoFix").unwrap().rect[1], crop.rect[1] + TOOL_BUTTON_H + 6.0, "the second row");
    assert!(!rig.find("tool:autoColor").unwrap().enabled);
    rig.click("tool:autoFix");
    rig.click("tool:autoColor");
    assert_eq!(rig.tool_clicks, ["tool:autoFix"]);
    check_ids("tools", &rig.widgets, &registry::unregistered(&rig.ctx));
}

fn slider_scene(rig: &mut Rig, ui: &mut Ui) {
    let t = rig.t;
    let value = rig.value;
    let mut out = None;
    column(ui, Rect::from_min_size(pos2(10.0, 30.0), vec2(240.0, 45.0)), |ui| out = Some(tuning_slider(ui, &t, &FILL, value)));
    if let Some(out) = out {
        if let Some(v) = out.value {
            rig.value = v;
        }
        if out.value.is_some() || out.reset {
            rig.slider = Some(out);
        }
    }
}

#[test]
fn the_tuning_slider_shows_its_value_on_hover_nudges_two_percent_and_resets() {
    let mut rig = Rig::new((300, 140), slider_scene);
    assert!(rig.find("slider:tuning.fill/value").is_none(), "no value field");
    assert!(!rig.texts.iter().any(|t| t == "0.50"), "no number shown at rest: {:?}", rig.texts);
    // Hovered, the value is a tooltip at once.
    rig.hover("slider:tuning.fill");
    assert!(rig.texts.iter().any(|t| t == "0.50"), "the value tooltip: {:?}", rig.texts);
    // + and − nudge 2 % of the range while the pointer is on the row.
    rig.key(Key::Plus);
    assert!((rig.value - 0.52).abs() < 1e-9, "{}", rig.value);
    rig.key(Key::Equals);
    assert!((rig.value - 0.54).abs() < 1e-9, "{}", rig.value);
    rig.key(Key::Minus);
    rig.key(Key::Minus);
    rig.key(Key::Minus);
    assert!((rig.value - 0.48).abs() < 1e-9, "{}", rig.value);
    let out = rig.slider.take().unwrap();
    assert!(out.drag_started && out.drag_stopped, "a nudge is one undo step");
    assert!(rig.texts.iter().any(|t| t == "0.48"), "the tooltip follows: {:?}", rig.texts);
    // Double-click resets to the default.
    let at = rig.center("slider:tuning.fill") + vec2(30.0, 0.0);
    rig.clock += 1.0;
    rig.frame(vec![Event::PointerMoved(at), Rig::press(at, true)]);
    rig.frame(vec![Rig::press(at, false)]);
    rig.frame(vec![Rig::press(at, true)]);
    rig.frame(vec![Rig::press(at, false)]);
    rig.settle();
    assert_eq!(rig.value, 0.0);
    assert!(rig.slider.take().unwrap().reset);
    // The pointer away: no tooltip, and + does nothing.
    rig.frame(vec![Event::PointerMoved(pos2(280.0, 130.0))]);
    rig.settle();
    rig.key(Key::Plus);
    assert_eq!(rig.value, 0.0);
    assert!(!rig.texts.iter().any(|t| t == "0.00"), "{:?}", rig.texts);
    check_ids("slider", &rig.widgets, &registry::unregistered(&rig.ctx));
}

// ---- The histogram ----

const PLOT: Rect = Rect { min: Pos2 { x: 10.0, y: 10.0 }, max: Pos2 { x: 266.0, y: 120.0 } };

fn plot(ui: &mut Ui, t: &Tokens, bins: &[Vec<u64>; 3]) {
    histogram(ui, t, "histogram:edit", PLOT, [&bins[0], &bins[1], &bins[2]]);
}

/// Red, green and blue as three flat bands (3 : 2 : 1), then one tall spike, then each channel
/// alone and in pairs.
fn known_bins() -> [Vec<u64>; 3] {
    let mut bins = [vec![0u64; 256], vec![0; 256], vec![0; 256]];
    for (c, h) in [(0, 30u64), (1, 20), (2, 10)] {
        bins[c][16..96].fill(h);
    }
    for c in &mut bins {
        c[128] = 2000;
    }
    for (mask, start) in [(1usize, 150usize), (2, 168), (4, 186), (3, 204), (6, 222)] {
        for (c, bins) in bins.iter_mut().enumerate() {
            if mask & (1 << c) != 0 {
                bins[start..start + 12].fill(15);
            }
        }
    }
    bins
}

fn over(fill: Color32, back: Color32) -> [u8; 3] {
    // Premultiplied `fill` over opaque `back`.
    let a = f32::from(fill.a()) / 255.0;
    let mix = |f: u8, b: u8| (f32::from(f) + f32::from(b) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    [mix(fill.r(), back.r()), mix(fill.g(), back.g()), mix(fill.b(), back.b())]
}

fn close(a: [u8; 4], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

#[test]
fn the_histogram_of_a_known_bin_set() {
    let t = Tokens::default();
    let bins = known_bins();
    let mut roles = vec![];
    let render = |roles: &mut Vec<(String, Role)>| {
        dalica_ui_kit::snapshot(276, 130, 1.0, &t, |ui| {
            plot(ui, &t, &bins);
            *roles = registry::widgets(ui.ctx()).into_iter().map(|w| (w.id, w.role)).collect();
        })
        .unwrap()
    };
    let img = render(&mut roles);
    assert!(img.rgba == render(&mut vec![]).rgba, "two renders differ");
    assert_eq!(roles, [("histogram:edit".to_string(), Role::Histogram)]);
    let px = |x: f32, y: f32| img.pixel(x as u32, y as u32).unwrap();
    let x_of = |bin: usize| PLOT.min.x + (bin as f32 + 0.5) / 256.0 * PLOT.width();
    // The plate is `canvas`, outside the plot `chrome`.
    assert_eq!(px(PLOT.max.x - 3.0, PLOT.min.y + 3.0)[..3], t.canvas.to_array()[..3]);
    assert_eq!(px(3.0, 3.0)[..3], t.chrome.to_array()[..3]);
    // The flat bands, on one scale (3 : 2 : 1): all three at the bottom, red and green above blue,
    // red alone above green.
    let curves = histogram_curves([&bins[0], &bins[1], &bins[2]]);
    let (x, h) = (x_of(56), PLOT.height());
    let [r, g, b] = [curves[0][56], curves[1][56], curves[2][56]];
    assert!((r / b - 3.0).abs() < 0.01 && (g / b - 2.0).abs() < 0.01, "{r} {g} {b}");
    assert!(r < 0.5, "the spike does not flatten the bands: {r}");
    for (y, mask) in [(PLOT.max.y - b * h / 2.0, 7u8), (PLOT.max.y - (b + g) / 2.0 * h, 3), (PLOT.max.y - (g + r) / 2.0 * h, 1)] {
        assert!(
            close(px(x, y), over(histogram_fill(mask), t.canvas)),
            "mask {mask} at {y}: {:?} vs {:?}",
            px(x, y),
            over(histogram_fill(mask), t.canvas)
        );
    }
    // The spike clips at the top.
    assert!(px(x_of(128), PLOT.min.y + 1.0)[..3] != t.canvas.to_array()[..3], "the spike reaches the top");
    // Each channel alone and the pairs, each its exact additive colour, near the bottom.
    for (mask, start) in [(1u8, 150usize), (2, 168), (4, 186), (3, 204), (6, 222)] {
        let p = px(x_of(start + 6), PLOT.max.y - 2.0);
        assert!(close(p, over(histogram_fill(mask), t.canvas)), "mask {mask}: {p:?}");
    }
    // An empty range shows the plate.
    assert_eq!(px(x_of(110), PLOT.max.y - 2.0)[..3], t.canvas.to_array()[..3]);
    // The outlines are drawn in the channel colours: the red band's top edge is reddish.
    let edge = (0..h as u32).map(|dy| px(x, PLOT.min.y + dy as f32)).find(|p| p[..3] != t.canvas.to_array()[..3]).unwrap();
    assert!(edge[0] > edge[1] && edge[0] > edge[2], "the top edge is red: {edge:?} ({:?})", HISTOGRAM[0]);
    let mut failures = vec![];
    check("edit-widgets-histogram", &img, &mut failures);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- The gallery: an edit panel in a window ----

fn gallery(ui: &mut Ui, t: &Tokens, size: (u32, u32), active: &str) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let m = &t.metrics;
    let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(m.edit_panel_w, h));
    let canvas = Rect::from_min_max(pos2(m.edit_panel_w + 1.0, 0.0), pos2(w, h));
    let p = ui.painter().clone();
    p.rect_filled(canvas, 0, t.canvas);
    // A photo on the canvas.
    let photo = Rect::from_center_size(canvas.center(), vec2((canvas.width() - 48.0).min(800.0), (canvas.height() - 48.0).min(533.0)));
    p.rect_filled(photo, 0, Color32::from_rgb(70, 110, 150));
    p.rect_filled(Rect::from_min_max(photo.min, pos2(photo.max.x, photo.center().y)), 0, Color32::from_rgb(140, 170, 200));
    column(ui, panel, |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        ignore(tabs(ui, t, STRIP, &TABS, 0));
        let inner = panel.shrink2(vec2(8.0, 0.0));
        let tool_w = tool_button_width(t, inner.width(), m.tool_columns, 6.0);
        column(ui, Rect::from_min_max(pos2(inner.min.x, m.edit_tab.y + 8.0), inner.max), |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for chunk in TOOLS.chunks(3) {
                ui.horizontal(|ui| {
                    for (id, icon, label) in chunk {
                        ignore(tool_button(ui, t, id, *icon, label, *id == active, true, tool_w, label));
                    }
                });
            }
            ignore(tuning_slider(ui, t, &FILL, 0.3));
            let spec = SliderSpec { id: "slider:tuning.temperature", label: "Color Temperature", min: -1.0, max: 1.0, ..FILL };
            ignore(tuning_slider(ui, t, &spec, 0.2));
        });
        let bins = known_bins();
        let box_rect = Rect::from_min_size(pos2(16.0, h - 150.0 - 8.0), vec2(248.0, 150.0));
        ui.painter().text(box_rect.min, Align2::LEFT_TOP, "Histogram & Camera Info", fonts::semibold(t.count), t.text_label);
        histogram(
            ui,
            t,
            "histogram:edit",
            Rect::from_min_max(box_rect.min + vec2(0.0, 20.0), box_rect.min + vec2(248.0, 110.0)),
            [&bins[0], &bins[1], &bins[2]],
        );
        ui.painter().text(box_rect.min + vec2(0.0, 118.0), Align2::LEFT_TOP, "No camera information", fonts::regular(t.small), t.text_dim);
    });
}

fn window(size: (u32, u32)) -> (Image, Vec<WidgetInfo>, Vec<registry::Unregistered>) {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut raster = Rasterizer::new();
    let mut last = None;
    // Settle, then rest the pointer on the Crop tool.
    let mut hover = None;
    for pass in 0..5 {
        let mut input = dalica_ui_kit::raw_input(size.0, size.1, 1.0);
        input.time = Some(0.0);
        if let Some(at) = hover.filter(|_| pass >= 3) {
            input.events = vec![Event::PointerMoved(at)];
        }
        let out = ctx.run_ui(input, |ui| gallery(ui, &t, size, "tool:straighten"));
        last = Some(dalica_ui_kit::paint(&ctx, &mut raster, out, size.0, size.1, t.chrome).unwrap());
        if pass == 2 {
            hover = registry::find(&ctx, "tool:crop").map(|w| w.center());
        }
    }
    (last.unwrap(), registry::widgets(&ctx), registry::unregistered(&ctx))
}

#[test]
fn the_edit_panel_gallery_at_both_window_sizes() {
    let mut failures = vec![];
    for size in [(1400, 900), (900, 600)] {
        let (one, widgets, unregistered) = window(size);
        let (two, _, _) = window(size);
        assert!(one.rgba == two.rgba, "two renders of the {size:?} window differ");
        check_ids(&format!("gallery {size:?}"), &widgets, &unregistered);
        for id in ["tab:edit/basic", "tool:crop", "tool:autoColor", "slider:tuning.fill", "slider:tuning.temperature", "histogram:edit"] {
            assert!(widgets.iter().any(|w| w.id == id), "{size:?}: {id}");
        }
        // Everything of the panel stays inside its 280 pt.
        for w in widgets.iter().filter(|w| !w.id.starts_with("histogram")) {
            assert!(w.rect[0] + w.rect[2] <= 280.0 + 0.01, "{size:?}: {} leaves the panel: {:?}", w.id, w.rect);
        }
        check(&format!("gallery-edit-{}x{}", size.0, size.1), &one, &mut failures);
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
