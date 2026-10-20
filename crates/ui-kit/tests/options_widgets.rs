//! The Options dialog's widgets (`design-system.md` §6 and §11.3, `menus-and-shortcuts.md` §2.0 and
//! §5):
//!
//! - text tabs: the dialog's four tabs fit its strip whole at both window sizes and both font
//!   sizes; with more tabs than fit, the widest shrink and their labels are cut from their end with
//!   "…"; narrower still, tabs go to the overflow menu and the open one stays; a click opens a tab;
//! - checkbox rows: a click flips an enabled row, its hint sits under its label; a disabled row
//!   ignores clicks, registers as disabled and shows its reason as its tooltip;
//! - key cells: keys read "Ctrl+Shift+S" on Linux and "⇧⌘S" on macOS (every glyph in Inter), one
//!   or two caps, "—" without a key, "Press the new shortcut…" while recording, a `danger` outline
//!   for a conflict, a disabled cell ignores clicks;
//! - list rows: the columns of every row align at both font sizes, a long text is cut short inside
//!   its column, hover and selection fill the row, the modified mark, and a key cell in a row takes
//!   its own click;
//! - the sheet (`fingerprints/options-widgets.json`: every widget in its states) and the gallery: the
//!   Options dialog on its Keyboard Shortcuts tab in a 1400 × 900 window
//!   (`fingerprints/gallery-options-1400x900.json`) and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check, out_dir};
use dalica_ui_kit::icons::{OptionsIcon, paint_options};
use dalica_ui_kit::theme::FontSize;
use dalica_ui_kit::widgets::{
    ButtonKind, CHOICE_LABEL_X, CheckboxRowOut, DialogButton, Form, KEY_CELL_W, KeyCell, LIST_COLUMN_GAP, LIST_ROW_GUTTER, NO_KEY_TEXT,
    RECORDING_TEXT, TextTab, TextTabsOut, checkbox_row, dialog, key_cell, key_cell_label, key_text, list_row, list_row_height, list_text, tab_page,
    tabs_overflow_id, text_button, text_field, text_tabs, text_tabs_height,
};
use dalica_ui_kit::{Image, Role, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Context, Event, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const STRIP: &str = "tab:options";
/// The Options dialog's size at 100 % (never larger than the window).
const DIALOG: (f32, f32) = (720.0, 540.0);
const FOUR: [TextTab<'static>; 4] = [
    TextTab { name: "general", label: "General" },
    TextTab { name: "fileTypes", label: "File Types" },
    TextTab { name: "export", label: "Export" },
    TextTab { name: "shortcuts", label: "Keyboard Shortcuts" },
];
const EIGHT: [TextTab<'static>; 8] = [
    TextTab { name: "general", label: "General" },
    TextTab { name: "fileTypes", label: "File Types" },
    TextTab { name: "slideshow", label: "Slideshow" },
    TextTab { name: "network", label: "Network" },
    TextTab { name: "people", label: "People" },
    TextTab { name: "viewer", label: "Photo Viewer" },
    TextTab { name: "export", label: "Export" },
    TextTab { name: "shortcuts", label: "Keyboard Shortcuts and Menus" },
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

/// The texts of a pass's shapes, as painted (a cut label with its "…"), with their rects.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                let s: String = text.galley.rows.iter().map(|r| r.text()).collect();
                out.push((s, text.galley.rect.translate(text.pos.to_vec2())));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, &mut out);
    }
    out
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
    texts: Vec<(String, Rect)>,
    canvas: Canvas,
}

impl<S> Rig<S> {
    fn new(size: (u32, u32), t: Tokens, state: S, scene: fn(&mut Ui, &Tokens, &mut S)) -> Rig<S> {
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig { ctx, t, size, clock: 0.0, state, scene, widgets: vec![], texts: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame_dt(&mut self, events: Vec<Event>, dt: f64) {
        self.clock += dt;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let (t, scene) = (self.t, self.scene);
        let state = &mut self.state;
        let out = self.ctx.run_ui(raw, |ui| scene(ui, &t, state));
        self.widgets = registry::widgets(&self.ctx);
        self.texts = texts(&out.shapes);
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

    fn painted(&self, text: &str) -> bool {
        self.texts.iter().any(|(s, _)| s == text)
    }

    fn text_rect(&self, text: &str) -> Rect {
        self.texts.iter().find(|(s, _)| s == text).unwrap_or_else(|| panic!("{text:?} not painted: {:?}", self.texts)).1
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
        self.settle();
    }

    fn click(&mut self, id: &str) {
        let at = self.rect(id).center();
        self.click_at(at);
    }

    /// The pointer moved onto `at` and resting there long enough for a tooltip.
    fn rest_at(&mut self, at: Pos2) {
        self.frame_dt(vec![Event::PointerMoved(at + vec2(3.0, 0.0))], 1.0);
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame_dt(vec![], 1.0);
        self.settle();
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

// ---- Text tabs ----

struct TabsScene {
    items: &'static [TextTab<'static>],
    active: usize,
    /// The strip's width when not in the dialog.
    strip_w: Option<f32>,
    out: Option<TextTabsOut>,
    dialog: Rect,
}

/// The Options dialog's width in a window `w` wide.
fn dialog_width(w: f32) -> f32 {
    DIALOG.0.min(w - 32.0)
}

fn tabs_body(ui: &mut Ui, t: &Tokens, s: &mut TabsScene) {
    let out = text_tabs(ui, t, STRIP, s.items, s.active);
    if let Some(i) = out.clicked {
        s.active = i;
    }
    let page = Rect::from_min_size(out.rect.left_bottom(), vec2(out.rect.width(), 120.0));
    tab_page(ui.painter(), t, page);
    ui.allocate_space(page.size());
    s.out = Some(out);
}

fn tabs_scene(ui: &mut Ui, t: &Tokens, s: &mut TabsScene) {
    match s.strip_w {
        Some(w) => area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(w, 200.0)), |ui| tabs_body(ui, t, s)),
        None => {
            let w = dialog_width(ui.ctx().content_rect().width());
            let buttons = [
                DialogButton { id: "button:options.ok", label: "OK", kind: ButtonKind::Default, enabled: true },
                DialogButton { id: "button:options.cancel", label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
            ];
            let r = dialog(ui.ctx(), t, "dialog:options", "Options", w, &buttons, |ui| tabs_body(ui, t, s));
            s.dialog = r.rect;
        }
    }
}

fn tabs_rig(size: (u32, u32), t: Tokens, items: &'static [TextTab<'static>], strip_w: Option<f32>) -> Rig<TabsScene> {
    Rig::new(size, t, TabsScene { items, active: 0, strip_w, out: None, dialog: Rect::NOTHING }, tabs_scene)
}

#[test]
fn a_strip_of_four_tabs_fits_at_both_window_sizes_and_elides_from_its_end() {
    for font in [FontSize::Small, FontSize::Large] {
        for size in WINDOWS {
            let what = format!("{font:?} {size:?}");
            let t = Tokens::default().with_font_size(font);
            // The dialog's four tabs: every one whole, in order, inside the strip and the dialog.
            let rig = tabs_rig(size, t, &FOUR, None);
            check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
            let out = rig.state.out.clone().unwrap();
            let dw = rig.state.dialog.width();
            assert!(dw <= size.0 as f32 && (dw - dialog_width(size.0 as f32)).abs() <= 2.0, "{what}: the dialog is {dw} wide");
            assert!(rig.state.dialog.contains_rect(out.rect), "{what}: the strip {:?} leaves the dialog {:?}", out.rect, rig.state.dialog);
            assert!(out.fit.overflow.is_empty(), "{what}: {:?}", out.fit);
            assert_eq!(out.shown.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 1, 2, 3], "{what}");
            for ((_, rect, label, elided), item) in out.shown.iter().zip(FOUR) {
                assert_eq!((label.as_str(), *elided), (item.label, false), "{what}");
                assert!(out.rect.expand(0.01).contains_rect(*rect), "{what}: {item:?} at {rect:?}");
                let w = rig.info(&format!("{STRIP}/{}", item.name));
                assert_eq!((w.role, w.label.as_str(), w.active), (Role::Tab, item.label, item.name == "general"), "{what}");
                assert!(out.rect.expand(0.01).contains_rect(rig.text_rect(item.label)), "{what}: {item:?}'s label leaves the strip");
            }
            // Eight tabs do not fit whole: the widest shrinks, its label cut from its end, and the
            // others stay whole; nothing goes to the menu.
            let rig = tabs_rig(size, t, &EIGHT, None);
            let out = rig.state.out.clone().unwrap();
            assert!(out.fit.overflow.is_empty(), "{what}: {:?}", out.fit);
            assert_eq!(out.shown.len(), 8, "{what}");
            let cut: Vec<_> = out.shown.iter().filter(|s| s.3).collect();
            assert!(!cut.is_empty(), "{what}: eight tabs fit whole in {}", out.rect.width());
            for (i, _, label, _) in &cut {
                let whole = EIGHT[*i].label;
                let start = label.strip_suffix('…').unwrap_or_else(|| panic!("{what}: {label:?} is not cut at its end"));
                assert!(!start.is_empty() && whole.starts_with(start), "{what}: {label:?} is not the start of {whole:?}");
                assert_eq!(rig.info(&format!("{STRIP}/{}", EIGHT[*i].name)).label, whole, "{what}: the accessible name stays whole");
            }
            assert!(cut.iter().any(|c| c.0 == 7), "{what}: the longest label is cut");
            let total: f32 = out.fit.shown.iter().map(|s| s.1).sum();
            assert!(total <= out.rect.width() + 0.01, "{what}: {total} in {}", out.rect.width());
        }
    }
}

#[test]
fn a_narrow_strip_keeps_the_open_tab_and_a_click_opens_another() {
    let t = Tokens::default();
    let mut rig = tabs_rig((600, 300), t, &FOUR, Some(150.0));
    check_ids("narrow", &rig.widgets, &registry::unregistered(&rig.ctx));
    let out = rig.state.out.clone().unwrap();
    assert!(!out.fit.overflow.is_empty(), "{:?}", out.fit);
    assert!(out.shown.iter().any(|s| s.0 == 0), "the open tab stays: {:?}", out.fit);
    assert!(rig.find(&tabs_overflow_id(STRIP)).is_some());
    // A click on the overflow button lists the others; a click there opens one, which then shows.
    rig.click(&tabs_overflow_id(STRIP));
    let hidden = out.fit.overflow[0];
    rig.click(&format!("{STRIP}/{}", FOUR[hidden].name));
    assert_eq!(rig.state.active, hidden);
    assert!(rig.state.out.as_ref().unwrap().shown.iter().any(|s| s.0 == hidden), "the opened tab is on the strip");
    assert!(rig.info(&format!("{STRIP}/{}", FOUR[hidden].name)).active);
    // On the dialog's strip a click on a tab opens it; the open tab is filled with the page's
    // colour, the others are not.
    let mut rig = tabs_rig((1400, 900), t, &FOUR, None);
    rig.click(&format!("{STRIP}/export"));
    assert_eq!(rig.state.active, 2);
    let open = rig.rect(&format!("{STRIP}/export"));
    let other = rig.rect(&format!("{STRIP}/general"));
    assert_eq!(rig.pixel(open.left_bottom() + vec2(4.0, -3.0)), t.inset.to_array(), "the open tab joins the page");
    assert_eq!(rig.pixel(other.left_bottom() + vec2(4.0, -3.0)), t.chrome.to_array());
}

// ---- Checkbox rows ----

#[derive(Default)]
struct CheckScene {
    title_bar: bool,
    duplicates: bool,
    outs: Vec<CheckboxRowOut>,
}

const REASON: &str = "Arrives with Import";
const RESTART: &str = "Applies at next launch.";

fn check_scene(ui: &mut Ui, t: &Tokens, s: &mut CheckScene) {
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(560.0, 300.0)), |ui| {
        let form = Form::new(ui, t, &["Interface", "Import"]);
        let a = form.row(ui, t, "Interface", |ui| {
            checkbox_row(ui, t, "check:options.systemTitleBar", "Use system title bar", &mut s.title_bar, Some(RESTART), None)
        });
        let b = form.row(ui, t, "Import", |ui| {
            checkbox_row(ui, t, "check:options.detectDuplicates", "Detect duplicates while importing", &mut s.duplicates, None, Some(REASON))
        });
        s.outs = vec![a, b];
    });
}

#[test]
fn a_disabled_checkbox_ignores_clicks_and_shows_its_reason() {
    let mut rig = Rig::new((600, 340), Tokens::default(), CheckScene { duplicates: true, ..Default::default() }, check_scene);
    check_ids("checks", &rig.widgets, &registry::unregistered(&rig.ctx));
    // The enabled row flips on a click; its hint is under its line, starting under its label.
    rig.click("check:options.systemTitleBar");
    assert!(rig.state.title_bar);
    assert!(rig.info("check:options.systemTitleBar").active);
    let line = rig.rect("check:options.systemTitleBar");
    let hint = rig.state.outs[0].hint.unwrap();
    assert!(hint.min.y >= line.max.y - 0.5, "the hint {hint:?} is under the line {line:?}");
    assert!(
        (hint.min.x - (line.min.x + CHOICE_LABEL_X)).abs() <= 1.0,
        "the hint starts at {}, the label at {}",
        hint.min.x,
        line.min.x + CHOICE_LABEL_X
    );
    assert!(rig.painted(RESTART));
    // Resting on it shows no tooltip.
    rig.rest_at(line.center());
    assert!(!rig.painted(REASON), "an enabled row has no tooltip");
    // The disabled row: registered disabled, a click changes nothing, resting shows the reason.
    let w = rig.info("check:options.detectDuplicates");
    assert!(!w.enabled && w.active, "{w:?}");
    rig.click("check:options.detectDuplicates");
    assert!(rig.state.duplicates, "a disabled checkbox ignores the click");
    assert!(!rig.state.outs[1].response.changed());
    rig.rest_at(rig.rect("check:options.detectDuplicates").center());
    assert!(rig.painted(REASON), "the reason as the tooltip: {:?}", rig.texts);
    assert!(rig.state.duplicates);
}

// ---- Key cells ----

struct KeyScene {
    cells: Vec<(String, Vec<&'static str>, KeyCell<'static>, bool)>,
    clicks: Vec<String>,
}

fn key_scene(ui: &mut Ui, t: &Tokens, s: &mut KeyScene) {
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(400.0, 400.0)), |ui| {
        for (id, keys, cell, enabled) in &s.cells {
            let keys: Vec<&str> = keys.clone();
            let cell = KeyCell { keys: &keys, ..*cell };
            let r = ui.add_enabled_ui(*enabled, |ui| key_cell(ui, t, id, &cell)).inner;
            if r.clicked() {
                s.clicks.push(id.clone());
            }
        }
    });
}

#[test]
fn key_caps_read_ctrl_shift_s_on_linux_and_the_glyphs_on_macos() {
    // The platform's text, for every kind of key.
    for (key, linux, mac) in [
        ("Ctrl+Shift+S", "Ctrl+Shift+S", "⇧⌘S"),
        ("Shift+Ctrl+S", "Ctrl+Shift+S", "⇧⌘S"),
        ("Cmd+Alt+Left", "Ctrl+Alt+Left", "⌥⌘←"),
        ("F4", "F4", "F4"),
        ("Ctrl+,", "Ctrl+,", "⌘,"),
        ("Ctrl++", "Ctrl++", "⌘+"),
        ("Ctrl+Plus", "Ctrl+Plus", "⌘+"),
        ("Esc", "Esc", "⎋"),
        ("Alt+Enter", "Alt+Enter", "⌥↩"),
        ("Tab", "Tab", "⇥"),
        ("Backspace", "Backspace", "⌫"),
        ("Delete", "Delete", "⌦"),
        ("Option+Shift+PageDown", "Alt+Shift+PageDown", "⌥⇧⇟"),
        ("Home", "Home", "↖"),
    ] {
        assert_eq!(key_text(key, false), linux, "{key}");
        assert_eq!(key_text(key, true), mac, "{key}");
    }
    // Text that names no key comes back as it was.
    for odd in ["", "+", "Ctrl+", "Hyper+X", "Ctrl+Ctrl+"] {
        assert_eq!(key_text(odd, true), odd.trim(), "{odd:?}");
    }
    let base = KeyCell::default();
    let cells = vec![
        ("button:key.linux".to_string(), vec!["Ctrl+Shift+S"], base, true),
        ("button:key.mac".to_string(), vec!["Ctrl+Shift+S"], KeyCell { mac: true, ..base }, true),
        ("button:key.two".to_string(), vec!["Ctrl+8", "F4", "F9"], base, true),
        ("button:key.none".to_string(), vec![], base, true),
        ("button:key.recording".to_string(), vec!["Ctrl+8"], KeyCell { recording: true, ..base }, true),
        ("button:key.conflict".to_string(), vec!["Ctrl+1"], KeyCell { conflict: true, ..base }, true),
        ("button:key.hold".to_string(), vec!["Ctrl+Alt"], base, false),
        ("button:key.roving".to_string(), vec!["F2"], KeyCell { stop: false, ..base }, true),
    ];
    let mut rig = Rig::new((440, 440), Tokens::default(), KeyScene { cells, clicks: vec![] }, key_scene);
    check_ids("keys", &rig.widgets, &registry::unregistered(&rig.ctx));
    // Every glyph a macOS key can show is in Inter, so no cap shows a missing-glyph box.
    let font = fonts::regular(rig.t.count);
    let mut missing = String::new();
    let ctx = Context::default();
    rig.t.apply(&ctx);
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |_| {}).textures_delta.clear();
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |ui| {
        // A character Inter lacks is drawn with the replacement glyph, as a surely missing one is.
        let uv = |c: char| {
            let g = ui.painter().layout_no_wrap(c.to_string(), font.clone(), egui::Color32::WHITE);
            g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.uv_rect)
        };
        let none = uv('\u{10FFFD}');
        missing = "⌘⌥⇧↩⎋⇥⌫⌦←→↑↓⇞⇟↖↘+…—".chars().filter(|&c| uv(c) == none).collect();
        assert_ne!(uv('S'), none);
    })
    .textures_delta
    .clear();
    assert_eq!(missing, "", "key glyphs missing from Inter");
    // The caps say the keys, and so does the cell's label.
    for (id, label) in [("button:key.linux", "Ctrl+Shift+S"), ("button:key.mac", "⇧⌘S"), ("button:key.two", "Ctrl+8, F4")] {
        let w = rig.info(id);
        assert_eq!((w.role, w.label.as_str(), w.active), (Role::Button, label, false), "{id}");
        assert_eq!(w.rect[2], KEY_CELL_W);
    }
    for text in ["Ctrl+Shift+S", "⇧⌘S", "Ctrl+8", "F4"] {
        assert!(rig.painted(text), "{text:?} not on a cap: {:?}", rig.texts);
        assert!(rig.text_rect(text).width() > 4.0);
    }
    assert!(!rig.painted("F9"), "two keys at most");
    // Two caps side by side inside their cell.
    let two = rig.rect("button:key.two");
    let (a, b) = (rig.text_rect("Ctrl+8"), rig.text_rect("F4"));
    assert!(a.max.x < b.min.x && two.contains_rect(a) && two.contains_rect(b), "{a:?} {b:?} in {two:?}");
    // No key, recording, a conflict.
    assert_eq!(rig.info("button:key.none").label, NO_KEY_TEXT);
    assert!(rig.painted("—"));
    let rec = rig.info("button:key.recording");
    assert_eq!((rec.label.as_str(), rec.active), (RECORDING_TEXT, true));
    assert!(rig.painted(RECORDING_TEXT));
    assert_eq!(rig.texts.iter().filter(|(s, _)| s == "Ctrl+8").count(), 1, "a recording cell shows no caps");
    let t = rig.t;
    let conflict = rig.rect("button:key.conflict");
    assert_eq!(rig.pixel(pos2(conflict.center().x, conflict.min.y + 0.5)), t.danger.to_array(), "the conflict outline");
    let recording = rig.rect("button:key.recording");
    assert_eq!(rig.pixel(pos2(recording.center().x, recording.min.y + 0.5)), t.accent.to_array(), "the recording outline");
    assert_eq!(key_cell_label(&KeyCell { keys: &["Ctrl+1"], conflict: true, ..base }), "Ctrl+1");
    // A click is reported; a disabled cell (a hold gesture) ignores it.
    rig.click("button:key.linux");
    rig.click("button:key.hold");
    assert_eq!(rig.state.clicks, ["button:key.linux"]);
    assert!(!rig.info("button:key.hold").enabled);
    // A cell that is not its list's current one still clicks, but takes no Tab stop.
    let focusable = |rig: &Rig<KeyScene>, id: &str| rig.ctx.viewport(|v| v.prev_pass.widgets.get(egui::Id::new(id)).map(|w| w.sense.is_focusable()));
    assert_eq!((focusable(&rig, "button:key.linux"), focusable(&rig, "button:key.roving")), (Some(true), Some(false)));
    rig.click("button:key.roving");
    assert_eq!(rig.state.clicks, ["button:key.linux", "button:key.roving"]);
}

// ---- List rows ----

/// The shortcut editor's columns: the command, its context, two key cells, the ↺ button.
fn columns() -> [f32; 5] {
    [260.0, 96.0, KEY_CELL_W + 8.0, KEY_CELL_W + 8.0, f32::INFINITY]
}

const ROWS: [(&str, &str, &str, &[&str], bool); 4] = [
    ("star", "Picture › Star / Unstar", "Library", &["Ctrl+8", "F4"], true),
    ("thumbsSmall", "View › Small Thumbnails", "Library", &["Ctrl+1"], false),
    ("save", "File › Save a Copy of the Edited Picture in Its Folder", "Edit view", &["Ctrl+Shift+S"], false),
    ("about", "Help › About Dalica", "Global", &[], false),
];

#[derive(Default)]
struct ListScene {
    selected: usize,
    row_clicks: Vec<usize>,
    key_clicks: Vec<String>,
    /// Per row, the x where each column's content starts.
    starts: Vec<Vec<f32>>,
    elided: Vec<bool>,
}

fn list_scene(ui: &mut Ui, t: &Tokens, s: &mut ListScene) {
    let widths = columns();
    s.starts.clear();
    s.elided.clear();
    area(ui, Rect::from_min_size(pos2(20.0, 20.0), vec2(860.0, 400.0)), |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, (name, command, context, keys, modified)) in ROWS.into_iter().enumerate() {
            let mut starts = vec![f32::NAN; widths.len()];
            let mut elided = false;
            let id = format!("row:shortcuts/{name}");
            let r = list_row(ui, t, &id, command, &widths, s.selected == i, modified, |ui, cell| {
                starts[cell.column] = ui.cursor().min.x;
                match cell.column {
                    0 => elided = list_text(ui, t, cell, command, false).1,
                    1 => {
                        list_text(ui, t, cell, context, true);
                    }
                    2 | 3 => {
                        let key: Vec<&str> = keys.get(cell.column - 2).copied().into_iter().collect();
                        let kid = format!("button:shortcuts/{name}/key{}", cell.column - 2);
                        if key_cell(ui, t, &kid, &KeyCell { keys: &key, ..KeyCell::default() }).clicked() {
                            s.key_clicks.push(kid);
                        }
                    }
                    _ => {
                        if modified {
                            let (_, r) = ui.allocate_space(vec2(20.0, 20.0));
                            paint_options(ui.painter(), r, OptionsIcon::Restore, t.icon);
                        }
                    }
                }
            });
            if r.clicked() {
                s.row_clicks.push(i);
                s.selected = i;
            }
            s.starts.push(starts);
            s.elided.push(elided);
        }
    });
}

#[test]
fn a_rows_columns_align_at_both_font_sizes() {
    for font in [FontSize::Small, FontSize::Large] {
        let t = Tokens::default().with_font_size(font);
        let mut rig = Rig::new((900, 260), t, ListScene::default(), list_scene);
        let what = format!("{font:?}");
        check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
        // Every row is as high as the font needs, never less than 24; rows follow each other.
        let h = list_row_height(&t);
        assert!(h >= 24.0 && h >= t.body * 1.25, "{what}: {h}");
        let rows: Vec<Rect> = ROWS.iter().map(|r| rig.rect(&format!("row:shortcuts/{}", r.0))).collect();
        for (a, b) in rows.iter().zip(&rows[1..]) {
            assert!((a.height() - h).abs() < 0.01 && (b.min.y - a.max.y).abs() < 0.01, "{what}: {a:?} {b:?}");
        }
        // Each column starts at one x in every row, the gutter and the widths before it in.
        let widths = columns();
        for (row, starts) in rig.state.starts.iter().enumerate() {
            let mut x = rows[row].min.x + LIST_ROW_GUTTER;
            for (c, &start) in starts.iter().enumerate() {
                assert!((start - x).abs() < 0.01, "{what}: row {row} column {c} starts at {start}, not {x}");
                x += widths[c];
            }
        }
        // The key cells line up too, and stay inside their rows.
        for c in 0..2 {
            let xs: Vec<f32> = ROWS.iter().map(|r| rig.rect(&format!("button:shortcuts/{}/key{c}", r.0)).min.x).collect();
            assert!(xs.iter().all(|&x| (x - xs[0]).abs() < 0.01), "{what}: key column {c} at {xs:?}");
            for (row, r) in rows.iter().zip(ROWS) {
                assert!(row.contains_rect(rig.rect(&format!("button:shortcuts/{}/key{c}", r.0))), "{what}");
            }
        }
        // A command too long for its column is cut short inside it.
        assert_eq!(rig.state.elided, [false, false, true, false], "{what}");
        let save_text = rig.texts.iter().find(|(s, _)| s.starts_with("File › Save") && s.ends_with('…')).unwrap().1;
        assert!(save_text.max.x <= rows[2].min.x + LIST_ROW_GUTTER + widths[0], "{what}: {save_text:?}");
        // The modified mark, the selected fill, then the hovered fill.
        let star = rows[0];
        assert_eq!(rig.pixel(pos2(star.min.x + LIST_ROW_GUTTER / 2.0, star.center().y)), t.text.to_array(), "{what}: the modified mark");
        assert_eq!(rig.pixel(pos2(star.max.x - 4.0, star.center().y)), t.pressed.to_array(), "{what}: the selected row");
        let about = rows[3];
        assert_eq!(rig.pixel(pos2(about.max.x - 4.0, about.center().y)), t.chrome.to_array(), "{what}");
        rig.rest_at(pos2(about.max.x - 40.0, about.center().y));
        assert_eq!(rig.pixel(pos2(about.max.x - 4.0, about.center().y)), t.hover.to_array(), "{what}: the hovered row");
    }
}

#[test]
fn a_click_selects_a_row_and_a_key_cell_takes_its_own() {
    let mut rig = Rig::new((900, 260), Tokens::default(), ListScene::default(), list_scene);
    // On its command's text (the middle of the row is a key cell).
    let row = rig.rect("row:shortcuts/thumbsSmall");
    rig.click_at(pos2(row.min.x + 60.0, row.center().y));
    assert_eq!((rig.state.selected, rig.state.row_clicks.clone()), (1, vec![1]));
    assert!(rig.info("row:shortcuts/thumbsSmall").active && !rig.info("row:shortcuts/star").active);
    rig.click("button:shortcuts/save/key0");
    assert_eq!(rig.state.key_clicks, ["button:shortcuts/save/key0"]);
    assert_eq!(rig.state.row_clicks, [1], "the row under the key cell is not clicked");
}

// ---- The sheet ----

fn sheet(ui: &mut Ui, t: &Tokens, _: &mut ()) {
    let p = ui.painter().clone();
    let label = |y: f32, s: &str| {
        p.text(pos2(12.0, y), Align2::LEFT_TOP, s, fonts::semibold(t.small), t.text_label);
    };
    label(8.0, "text_tabs: 4 tabs · 8 tabs cut · 150 wide");
    for (i, (items, w, active)) in [(&FOUR[..], 560.0, 0), (&EIGHT[..], 560.0, 7), (&FOUR[..], 150.0, 1)].into_iter().enumerate() {
        let y = 26.0 + 64.0 * i as f32;
        area(ui, Rect::from_min_size(pos2(12.0, y), vec2(w, 60.0)), |ui| {
            let out = text_tabs(ui, t, &format!("tab:sheet{i}"), items, active);
            tab_page(ui.painter(), t, Rect::from_min_size(out.rect.left_bottom(), vec2(out.rect.width(), 22.0)));
        });
    }
    label(222.0, "checkbox_row: on + hint · off · disabled with a reason");
    area(ui, Rect::from_min_size(pos2(12.0, 240.0), vec2(560.0, 120.0)), |ui| {
        let (mut on, mut off, mut dis) = (true, false, true);
        checkbox_row(ui, t, "check:sheet.on", "Use system title bar", &mut on, Some(RESTART), None);
        checkbox_row(ui, t, "check:sheet.off", "Show tooltips", &mut off, None, None);
        checkbox_row(ui, t, "check:sheet.disabled", "Detect duplicates while importing", &mut dis, Some(REASON), Some(REASON));
    });
    label(380.0, "key_cell: Linux · macOS · two keys · none · recording · conflict · disabled");
    let base = KeyCell::default();
    let cells: [(&str, &[&str], KeyCell<'_>, bool); 7] = [
        ("linux", &["Ctrl+Shift+S"], base, true),
        ("mac", &["Ctrl+Shift+S"], KeyCell { mac: true, ..base }, true),
        ("two", &["Ctrl+8", "F4"], base, true),
        ("none", &[], base, true),
        ("recording", &[], KeyCell { recording: true, ..base }, true),
        ("conflict", &["Ctrl+1"], KeyCell { conflict: true, ..base }, true),
        ("disabled", &["Ctrl+Alt"], base, false),
    ];
    for (i, (name, keys, cell, enabled)) in cells.into_iter().enumerate() {
        let at = pos2(12.0 + (KEY_CELL_W + 12.0) * (i % 4) as f32, 398.0 + 30.0 * (i / 4) as f32);
        area(ui, Rect::from_min_size(at, vec2(KEY_CELL_W, 24.0)), |ui| {
            ui.add_enabled_ui(enabled, |ui| key_cell(ui, t, &format!("button:sheet.key.{name}"), &KeyCell { keys, ..cell }));
        });
    }
    label(464.0, "list_row: modified + selected · rest · cut · icons ↺ ×");
    area(ui, Rect::from_min_size(pos2(12.0, 482.0), vec2(700.0, 100.0)), |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, (name, command, context, keys, modified)) in ROWS.into_iter().take(3).enumerate() {
            list_row(ui, t, &format!("row:sheet/{name}"), command, &[240.0, 90.0, KEY_CELL_W + 8.0, f32::INFINITY], i == 0, modified, |ui, cell| {
                match cell.column {
                    0 => {
                        list_text(ui, t, cell, command, false);
                    }
                    1 => {
                        list_text(ui, t, cell, context, true);
                    }
                    2 => {
                        key_cell(ui, t, &format!("button:sheet/{name}/key"), &KeyCell { keys, ..KeyCell::default() });
                    }
                    _ => {
                        let (_, r) = ui.allocate_space(vec2(40.0, 20.0));
                        if modified {
                            paint_options(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), OptionsIcon::Restore, t.icon);
                        }
                    }
                }
            });
        }
    });
    for (i, icon) in OptionsIcon::ALL.into_iter().enumerate() {
        paint_options(&p, Rect::from_min_size(pos2(740.0 + 28.0 * i as f32, 488.0), vec2(20.0, 20.0)), icon, t.icon);
    }
}

#[test]
fn the_options_widget_sheet_matches_its_fingerprint() {
    let mut failures = vec![];
    let mut a = Rig::new((820, 570), Tokens::default(), (), sheet);
    let mut b = Rig::new((820, 570), Tokens::default(), (), sheet);
    let (one, two) = (a.image(), b.image());
    assert!(one.rgba == two.rgba, "two renders of the sheet differ");
    check_ids("sheet", &a.widgets, &registry::unregistered(&a.ctx));
    check("options-widgets", &one, &mut failures);
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}

// ---- The gallery: the Options dialog on its Keyboard Shortcuts tab ----

struct Gallery {
    search: String,
    dialog: Rect,
    page: Rect,
}

impl Default for Gallery {
    fn default() -> Gallery {
        Gallery { search: String::new(), dialog: Rect::NOTHING, page: Rect::NOTHING }
    }
}

fn gallery(ui: &mut Ui, t: &Tokens, s: &mut Gallery) {
    let screen = ui.ctx().content_rect();
    let p = ui.painter().clone();
    // The app behind the dialog: the toolbar's divider, the tree and the light table.
    p.rect_filled(Rect::from_min_size(pos2(0.0, t.metrics.toolbar_h - 1.0), vec2(screen.width(), 1.0)), 0, t.divider);
    p.rect_filled(Rect::from_min_max(pos2(t.metrics.tree_w + t.metrics.splitter_w, t.metrics.toolbar_h), screen.max), 0, t.grid_bg);
    let (w, h) = (dialog_width(screen.width()), DIALOG.1.min(screen.height() - 32.0));
    let buttons = [
        DialogButton { id: "button:options.ok", label: "OK", kind: ButtonKind::Default, enabled: true },
        DialogButton { id: "button:options.cancel", label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
        DialogButton { id: "button:options.apply", label: "Apply", kind: ButtonKind::Other, enabled: true },
    ];
    // Title 15 pt and its gap, the buttons and their gap, the margins.
    let page_h = (h - 24.0 - 38.0 - 12.0 - t.metrics.button_h - text_tabs_height(t)).max(80.0);
    let r = dialog(ui.ctx(), t, "dialog:options", "Options", w, &buttons, |ui| {
        let out = text_tabs(ui, t, STRIP, &FOUR, 3);
        let page = Rect::from_min_size(out.rect.left_bottom(), vec2(out.rect.width(), page_h));
        tab_page(ui.painter(), t, page);
        let mut content = ui.new_child(UiBuilder::new().max_rect(page.shrink2(vec2(12.0, 10.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        {
            let ui = &mut content;
            ui.horizontal(|ui| {
                text_field(ui, t, "field:shortcuts.search", &mut s.search, "Search commands and keys", 260.0);
                text_button(ui, t, "button:shortcuts.resetAll", "Reset All", false);
            });
            ui.add_space(8.0);
            let mut unbound = false;
            checkbox_row(ui, t, "check:shortcuts.unbound", "Show commands without a shortcut", &mut unbound, None, None);
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let widths = [200.0, 72.0, KEY_CELL_W + LIST_COLUMN_GAP, KEY_CELL_W + LIST_COLUMN_GAP, f32::INFINITY];
            for (i, (name, command, context, keys, modified)) in ROWS.into_iter().enumerate() {
                list_row(ui, t, &format!("row:shortcuts/{name}"), command, &widths, i == 1, modified, |ui, cell| match cell.column {
                    0 => {
                        list_text(ui, t, cell, command, false);
                    }
                    1 => {
                        list_text(ui, t, cell, context, true);
                    }
                    2 | 3 => {
                        let slot = cell.column - 2;
                        let key: Vec<&str> = keys.get(slot).copied().into_iter().collect();
                        let recording = name == "thumbsSmall" && slot == 1;
                        let conflict = name == "save" && slot == 0;
                        let cell = KeyCell { keys: &key, recording, conflict, ..KeyCell::default() };
                        key_cell(ui, t, &format!("button:shortcuts/{name}/key{slot}"), &cell);
                    }
                    _ => {
                        if modified {
                            let (_, r) = ui.allocate_space(vec2(20.0, 20.0));
                            paint_options(ui.painter(), r, OptionsIcon::Restore, t.icon);
                        }
                    }
                });
            }
        }
        ui.allocate_space(vec2(page.width(), page.height()));
        page
    });
    s.dialog = r.rect;
    s.page = r.inner;
}

#[test]
fn the_options_gallery_at_both_window_sizes() {
    let mut failures = vec![];
    for size in WINDOWS {
        let mut one = Rig::new(size, Tokens::default(), Gallery::default(), gallery);
        let what = format!("gallery {size:?}");
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let window = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        assert!(window.contains_rect(one.state.dialog), "{what}: the dialog {:?} leaves the window", one.state.dialog);
        assert!(one.state.dialog.contains_rect(one.state.page), "{what}");
        let d = one.state.dialog.size();
        assert!(d.x <= DIALOG.0 + 2.0 && d.y <= DIALOG.1 + 2.0, "{what}: the dialog is {d:?}");
        assert!(one.painted(RECORDING_TEXT), "{what}: a recording cell says so whole");
        for r in ROWS {
            let row = one.rect(&format!("row:shortcuts/{}", r.0));
            assert!(one.state.page.contains_rect(row), "{what}: {} leaves the page", r.0);
            for slot in 0..2 {
                let cell = one.rect(&format!("button:shortcuts/{}/key{slot}", r.0));
                assert!(row.contains_rect(cell) && (cell.width() - KEY_CELL_W).abs() < 0.01, "{what}: {} key {slot} at {cell:?}", r.0);
            }
        }
        // One fingerprint: the large window's (the small one has to fit, above).
        if size == (1400, 900) {
            let mut two = Rig::new(size, Tokens::default(), Gallery::default(), gallery);
            assert!(one.image().rgba == two.image().rgba, "two renders of the {size:?} window differ");
            check(&format!("gallery-options-{}x{}", size.0, size.1), &one.image(), &mut failures);
        }
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
