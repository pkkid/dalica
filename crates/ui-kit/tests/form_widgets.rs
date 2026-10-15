//! The Export dialog's widgets (M8.5; `design-system.md` §6, `creating.md` §2): form rows, the
//! checkbox and the radio button, the slider with stops and dropdown items with a description.
//!
//! - every state: the checkbox, the radio button, the stops slider and a dropdown item in rest,
//!   hover, pressed, active, disabled and focused, each a tile rendered with real input, one sheet
//!   per widget (`fingerprints/form-widgets-<name>.json`);
//! - the form: in an Export to Folder dialog at 1400 × 900 and 900 × 600 (and at the Large font
//!   size), every label ends on one right edge and every row's controls start on one left edge;
//! - the stops slider: a drag near 1600 snaps to it, a drag between stops does not, typing 1700
//!   holds 1700, a bad value or Esc keeps the old one, ↑ goes to the next stop;
//! - a disabled group ignores clicks, and a radio row's controls follow its radio;
//! - the quality dropdown: its items with their descriptions, the checked one active, a pick;
//! - the gallery: the dialog over a window showing the Save and project glyphs, at 1400 × 900 (the
//!   quality menu open; `fingerprints/gallery-export-1400x900.json`) and 900 × 600.
//!
//! Every frame registers its ids with their roles, in the grammar of `control-protocol.md` §7, and
//! leaves no interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check, out_dir};
use dalica_ui_kit::icons::{self, OutputIcon, paint_output};
use dalica_ui_kit::theme::FontSize;
use dalica_ui_kit::widgets::{
    ButtonKind, DialogButton, FORM_GAP, Form, STOP_SNAP, SliderOut, SliderSpec, StopsSlider, checkbox, dialog, dropdown, dropdown_item,
    dropdown_popup_id, form_group, hint, radio, radio_row, stops_slider, text_button, text_field,
};
use dalica_ui_kit::{Icon, Image, Role, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const SIZE: SliderSpec<'static> =
    SliderSpec { id: "slider:dialog.size", label: "Resize to", min: 100.0, max: 4000.0, default: 1600.0, step: 1.0, decimals: 0, unit: "pixels" };
const STOPS: [f64; 8] = [480.0, 640.0, 800.0, 1024.0, 1280.0, 1600.0, 2048.0, 3840.0];
const QUALITY: SliderSpec<'static> =
    SliderSpec { id: "slider:dialog.quality", label: "Custom quality", min: 1.0, max: 100.0, default: 75.0, step: 1.0, decimals: 0, unit: "" };

/// The quality presets: id part, title, description.
const PRESETS: [(&str, &str, &str); 5] = [
    ("automatic", "Automatic", "Keeps the original's quality"),
    ("normal", "Normal", "Balances quality and size"),
    ("maximum", "Maximum", "Keeps fine detail; large files"),
    ("minimum", "Minimum", "Small files with some quality loss"),
    ("custom", "Custom", "Choose the quality yourself"),
];

const LABELS: [&str; 6] = ["Export location:", "Name of exported folder:", "Image size:", "Image quality:", "Export videos as:", "Watermark:"];

fn ignore<T>(_: T) {}

fn column(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
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
            name: "checkbox",
            tile: (240, 48),
            target: "check:dialog.numbers",
            ids: &[("check:dialog.numbers", Role::Toggle)],
            draw: |ui, t, state, rect| {
                let mut on = state == State::Active;
                column(ui, rect, |ui| ignore(checkbox(ui, t, "check:dialog.numbers", "Add numbers to file names", &mut on)));
            },
        },
        Sheet {
            name: "radio",
            tile: (240, 48),
            target: "choice:dialog.size/original",
            ids: &[("choice:dialog.size/original", Role::Toggle)],
            draw: |ui, t, state, rect| {
                column(ui, rect, |ui| ignore(radio(ui, t, "choice:dialog.size/original", "Use original size", state == State::Active)));
            },
        },
        Sheet {
            name: "stops_slider",
            tile: (300, 48),
            target: "slider:dialog.size",
            ids: &[("slider:dialog.size", Role::Other), ("slider:dialog.size/value", Role::Other)],
            draw: |ui, t, state, rect| {
                let value = if state == State::Active { 2048.0 } else { 800.0 };
                let s = StopsSlider { spec: SIZE, stops: &STOPS, field: true, track_w: f32::INFINITY };
                column(ui, rect, |ui| ignore(stops_slider(ui, t, &s, value)));
            },
        },
        Sheet {
            name: "dropdown_item",
            tile: (260, 62),
            target: "choice:dialog.quality/automatic",
            ids: &[("choice:dialog.quality/automatic", Role::MenuItem)],
            draw: |ui, t, state, rect| {
                let (_, title, description) = PRESETS[0];
                column(ui, rect, |ui| ignore(dropdown_item(ui, t, "choice:dialog.quality/automatic", title, description, state == State::Active)));
            },
        },
    ]
}

struct Tile {
    image: Image,
    widgets: Vec<WidgetInfo>,
    unregistered: Vec<registry::Unregistered>,
}

/// Two passes to settle, the state's input, then the frame that is kept.
fn render_tile(sheet: &Sheet, state: State) -> Tile {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let (w, h) = (sheet.tile.0 as f32, sheet.tile.1 as f32);
    let content = Rect::from_min_max(pos2(8.0, CAPTION + 4.0), pos2(w - 8.0, h - 4.0));
    let draw = |ui: &mut Ui| {
        ui.painter().text(pos2(6.0, 3.0), Align2::LEFT_TOP, state.name(), fonts::regular(t.small), t.text_dim);
        if state == State::Disabled {
            ui.disable();
        }
        (sheet.draw)(ui, &t, state, content);
    };
    let mut target: Option<(Pos2, Option<egui::Id>)> = None;
    let mut canvas = Canvas::new(sheet.tile, t.chrome);
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
        input.time = Some(0.0);
        input.events = events;
        let out = ctx.run_ui(input, &draw);
        canvas.keep(out);
        if pass == 2 {
            let info = registry::find(&ctx, sheet.target).unwrap_or_else(|| panic!("{}: {:?} registers {}", sheet.name, state, sheet.target));
            target = Some((info.center(), info.egui_id));
        }
    }
    Tile { image: canvas.image(&ctx).clone(), widgets: registry::widgets(&ctx), unregistered: registry::unregistered(&ctx) }
}

/// Tiles in `columns` columns, 1 px `divider` lines between them.
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
fn checkbox_radio_stops_slider_and_dropdown_item_in_every_state() {
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
        check(&format!("form-widgets-{}", sheet.name), &stitch(&images, 3), &mut failures);
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}

// ---- The Export dialog, driven ----

/// The dialog's draft, as the app would keep it.
#[derive(Debug, Clone, PartialEq)]
struct Draft {
    location: String,
    name: String,
    numbers: bool,
    resize: bool,
    size: f64,
    quality: usize,
    custom: f64,
    has_videos: bool,
    first_frame: bool,
    watermark: bool,
    watermark_text: String,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            location: "~/Pictures/Dalica/Exports".into(),
            name: "Lisbon Weekend".into(),
            numbers: false,
            resize: true,
            size: 1600.0,
            quality: 0,
            custom: 75.0,
            has_videos: false,
            first_frame: true,
            watermark: false,
            watermark_text: "© Ana Costa 2026".into(),
        }
    }
}

/// What the Export dialog's sliders said in the last pass.
#[derive(Default)]
struct Outs {
    size: Option<SliderOut>,
    clicked: Option<usize>,
}

/// The Export to Folder dialog of `creating.md` §2, built from the form widgets.
fn export_dialog(ctx: &Context, t: &Tokens, d: &mut Draft, outs: &mut Outs) -> Form {
    let buttons = [
        DialogButton { id: "button:dialog.export", label: "Export", kind: ButtonKind::Default, enabled: true },
        DialogButton { id: "button:dialog.cancel", label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
    ];
    let width = (560.0 * t.font_scale).min(ctx.content_rect().width() - 24.0);
    let out = dialog(ctx, t, "dialog:export", "Export to Folder", width, &buttons, |ui| {
        let form = Form::new(ui, t, &LABELS);
        let controls_w = ui.available_width() - form.label_w - FORM_GAP;
        form.row(ui, t, LABELS[0], |ui| {
            ui.horizontal(|ui| {
                ignore(text_field(ui, t, "field:dialog.location", &mut d.location, "", controls_w - 88.0));
                ignore(text_button(ui, t, "button:dialog.browse", "Browse…", false));
            });
        });
        form.row(ui, t, LABELS[1], |ui| {
            ignore(text_field(ui, t, "field:dialog.folderName", &mut d.name, "", controls_w));
            ignore(checkbox(ui, t, "check:dialog.numbers", "Add numbers to file names to keep the order", &mut d.numbers));
        });
        form.row(ui, t, LABELS[2], |ui| {
            if radio(ui, t, "choice:dialog.size/original", "Use original size", !d.resize).clicked() {
                d.resize = false;
            }
            let s = StopsSlider { spec: SIZE, stops: &STOPS, field: true, track_w: f32::INFINITY };
            let (r, out) = radio_row(ui, t, "choice:dialog.size/resize", "Resize to:", d.resize, |ui| stops_slider(ui, t, &s, d.size));
            if r.clicked() {
                d.resize = true;
            }
            if let Some(v) = out.value {
                d.size = v;
            }
            outs.size = Some(out);
        });
        form.row(ui, t, LABELS[3], |ui| {
            ui.horizontal(|ui| {
                let custom = PRESETS.len() - 1;
                let text = if d.quality == custom { format!("Custom ({})", d.custom) } else { PRESETS[d.quality].1.to_string() };
                let mut picked = None;
                dropdown(ui, t, "choice:dialog.quality", &text, |ui| {
                    for (i, (part, title, description)) in PRESETS.iter().enumerate() {
                        if dropdown_item(ui, t, &format!("choice:dialog.quality/{part}"), title, description, i == d.quality).clicked() {
                            picked = Some(i);
                        }
                    }
                });
                if let Some(i) = picked {
                    d.quality = i;
                }
                let s = StopsSlider { spec: QUALITY, stops: &[], field: false, track_w: 160.0 };
                if let Some(v) = form_group(ui, d.quality == custom, |ui| stops_slider(ui, t, &s, d.custom)).value {
                    d.custom = v;
                }
            });
        });
        form_group(ui, d.has_videos, |ui| {
            form.row(ui, t, LABELS[4], |ui| {
                ui.horizontal(|ui| {
                    if radio(ui, t, "choice:dialog.videos/firstFrame", "First frame", d.first_frame).clicked() {
                        d.first_frame = true;
                    }
                    if radio(ui, t, "choice:dialog.videos/fullVideo", "Full video (no resizing)", !d.first_frame).clicked() {
                        d.first_frame = false;
                    }
                });
            });
        });
        form.row(ui, t, LABELS[5], |ui| {
            ui.horizontal(|ui| {
                ignore(checkbox(ui, t, "check:dialog.watermark", "Add watermark", &mut d.watermark));
                let w = ui.available_width();
                form_group(ui, d.watermark, |ui| ignore(text_field(ui, t, "field:dialog.watermarkText", &mut d.watermark_text, "", w)));
            });
            ignore(hint(ui, t, "Stamp photos with your name, a web address or a copyright notice."));
        });
        form
    });
    outs.clicked = out.clicked;
    out.inner
}

/// The window behind the dialog: a library tree with two projects and a folder header with Save.
fn backdrop_window(ui: &mut Ui, t: &Tokens, size: (u32, u32)) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let p = ui.painter().clone();
    let tree = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, h));
    p.rect_filled(Rect::from_min_max(pos2(241.0, 0.0), pos2(w, h)), 0, t.grid_bg);
    p.text(pos2(12.0, 15.0), Align2::LEFT_CENTER, "Projects (2)", fonts::semibold(t.body), t.text_label);
    for (i, (name, count)) in [("Lisbon Weekend", "48"), ("Collages", "3")].into_iter().enumerate() {
        let y = 30.0 + 24.0 * i as f32 + 12.0;
        paint_output(&p, Rect::from_center_size(pos2(tree.min.x + 26.0, y), vec2(16.0, 16.0)), OutputIcon::Project, t.icon);
        p.text(pos2(42.0, y), Align2::LEFT_CENTER, name, fonts::regular(t.body), t.text_label);
        p.text(pos2(228.0, y), Align2::RIGHT_CENTER, count, fonts::regular(t.count), t.text_dim);
    }
    p.text(pos2(12.0, 95.0), Align2::LEFT_CENTER, "Folders (1)", fonts::semibold(t.body), t.text_label);
    icons::paint(&p, Rect::from_center_size(pos2(26.0, 122.0), vec2(16.0, 16.0)), Icon::Folder, t.icon);
    p.text(pos2(42.0, 122.0), Align2::LEFT_CENTER, "Lisbon", fonts::regular(t.body), t.text_label);
    let header = Rect::from_min_max(pos2(241.0, 0.0), pos2(w, 96.0));
    p.rect_filled(header, 0, t.folder_header);
    p.text(pos2(header.min.x + 56.0, 22.0), Align2::LEFT_CENTER, "Lisbon", fonts::semibold(t.title), t.text);
    p.text(pos2(header.max.x - 48.0, 22.0), Align2::RIGHT_CENTER, "3 edited", fonts::regular(t.detail), t.text_dim);
    for (i, (icon, color)) in [(OutputIcon::Save, t.icon), (OutputIcon::Save, t.text)].into_iter().enumerate() {
        let c = pos2(header.min.x + 25.0 + 30.0 * i as f32, 70.0);
        paint_output(&p, Rect::from_center_size(c, vec2(16.0, 16.0)), icon, color);
    }
    icons::paint(&p, Rect::from_center_size(pos2(header.min.x + 85.0, 70.0), vec2(16.0, 16.0)), Icon::Export, t.icon);
}

/// One scene in a window, driven by real input.
struct Rig {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    widgets: Vec<WidgetInfo>,
    /// The texts painted in the last pass with their rects (popups and tooltips included).
    texts: Vec<(String, Rect)>,
    draft: Draft,
    outs: Outs,
    form: Option<Form>,
    /// The last frame, painted only when a test asks for its pixels.
    canvas: Canvas,
}

impl Rig {
    fn new(size: (u32, u32), t: Tokens) -> Rig {
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig {
            ctx,
            t,
            size,
            clock: 0.0,
            widgets: vec![],
            texts: vec![],
            draft: Draft::default(),
            outs: Outs::default(),
            form: None,
            canvas: Canvas::new(size, t.chrome),
        };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let ctx = self.ctx.clone();
        let (t, size) = (self.t, self.size);
        let mut draft = std::mem::take(&mut self.draft);
        let mut outs = Outs::default();
        let mut form = None;
        let out = ctx.run_ui(raw, |ui| {
            backdrop_window(ui, &t, size);
            form = Some(export_dialog(ui.ctx(), &t, &mut draft, &mut outs));
        });
        self.draft = draft;
        self.outs = outs;
        self.form = form;
        self.widgets = registry::widgets(&self.ctx);
        self.texts = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.galley.rect.translate(text.pos.to_vec2()))),
                _ => None,
            })
            .collect();
        self.canvas.keep(out);
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    fn find(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn info(&self, id: &str) -> WidgetInfo {
        self.find(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>())).clone()
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
        let at = self.info(id).center();
        self.click_at(at);
    }

    /// Presses at `from`, moves to `to` in a few steps and lets go there.
    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(from), Rig::press(from, true)]);
        for i in 1..=4 {
            let at = from + (to - from) * (i as f32 / 4.0);
            self.frame(vec![Event::PointerMoved(at)]);
        }
        self.frame(vec![Rig::press(to, false)]);
        self.settle();
    }

    fn hover(&mut self, id: &str) {
        let at = self.info(id).center();
        self.frame(vec![Event::PointerMoved(at)]);
        self.settle();
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true), ev(false)]);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_owned())]);
    }

    fn text_rect(&self, text: &str) -> Rect {
        self.texts.iter().find(|(s, _)| s == text).unwrap_or_else(|| panic!("{text:?} not painted: {:?}", self.texts)).1
    }

    /// The x of `value` on the size slider's track.
    fn size_x(&self, value: f64) -> f32 {
        let r = self.info(SIZE.id).rect;
        let ring = self.t.metrics.slider_ring;
        let (x0, x1) = (r[0] + ring + 1.0, r[0] + r[2] - ring - 1.0);
        x0 + SIZE.fraction(value) * (x1 - x0)
    }

    fn size_y(&self) -> f32 {
        self.info(SIZE.id).center().y
    }
}

#[test]
fn label_columns_align_at_both_window_sizes_and_font_sizes() {
    for font in [FontSize::Small, FontSize::Large] {
        for size in [(1400, 900), (900, 600)] {
            let t = Tokens::default().with_font_size(font);
            let rig = Rig::new(size, t);
            let what = format!("{font:?} {size:?}");
            check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
            let form = rig.form.unwrap();
            // Every label ends on one right edge.
            let rights: Vec<f32> = LABELS.iter().map(|l| rig.text_rect(l).max.x).collect();
            let edge = rights[0];
            for (label, right) in LABELS.iter().zip(&rights) {
                assert!((right - edge).abs() <= 1.0, "{what}: {label:?} ends at {right}, not {edge}");
            }
            // The widest label fills the column; none runs into the controls.
            let widest = LABELS.iter().map(|l| rig.text_rect(l).width()).fold(0.0, f32::max);
            assert!((widest - form.label_w).abs() <= 1.0, "{what}: column {} for a widest label of {widest}", form.label_w);
            // Every row's controls start on one left edge, the gap after the labels.
            let fields = ["field:dialog.location", "field:dialog.folderName"];
            let firsts = [
                "field:dialog.location",
                "field:dialog.folderName",
                "check:dialog.numbers",
                "choice:dialog.size/original",
                "choice:dialog.size/resize",
                "choice:dialog.quality",
                "choice:dialog.videos/firstFrame",
                "check:dialog.watermark",
            ];
            for id in firsts {
                // A text field registers its text area, 8 pt inside its frame.
                let x = rig.info(id).rect[0] - if fields.contains(&id) { 8.0 } else { 0.0 };
                assert!((x - (edge + FORM_GAP)).abs() <= 1.0, "{what}: {id} starts at {x}, labels end at {edge}");
            }
            // A label sits on the middle of its row's first line.
            let label = rig.text_rect(LABELS[2]);
            let radio = rig.info("choice:dialog.size/original");
            assert!((label.center().y - radio.center().y).abs() <= 1.5, "{what}: {label:?} vs {:?}", radio.rect);
            // The hint is under the watermark's line, left-aligned with the controls.
            let hint = rig.text_rect("Stamp photos with your name, a web address or a copyright notice.");
            let watermark = rig.info("check:dialog.watermark");
            assert!(hint.min.y >= watermark.rect[1] + watermark.rect[3] - 0.5, "{what}: the hint is under the line");
            assert!((hint.min.x - (edge + FORM_GAP)).abs() <= 1.0, "{what}: the hint starts at {}", hint.min.x);
        }
    }
}

#[test]
fn a_drag_near_a_stop_snaps_to_it_and_a_typed_value_holds() {
    let mut rig = Rig::new((900, 600), Tokens::default());
    rig.draft.size = 800.0;
    rig.settle();
    let y = rig.size_y();
    // A drag ending 3 pt past 1600's tick lands on 1600 exactly.
    let (from, to) = (pos2(rig.size_x(800.0), y), pos2(rig.size_x(1600.0) + 3.0, y));
    rig.drag(from, to);
    assert_eq!(rig.draft.size, 1600.0);
    assert!(rig.texts.iter().any(|(s, _)| s == "1600"), "the field shows the value: {:?}", rig.texts);
    // Between two stops, more than the snap distance from either, it stays where it was let go.
    let x = (rig.size_x(2048.0) + rig.size_x(3840.0)) / 2.0;
    assert!(x - rig.size_x(2048.0) > STOP_SNAP && rig.size_x(3840.0) - x > STOP_SNAP);
    rig.drag(pos2(rig.size_x(1600.0), y), pos2(x, y));
    assert!(!STOPS.contains(&rig.draft.size), "{} is a stop", rig.draft.size);
    assert!(rig.draft.size > 2048.0 && rig.draft.size < 3840.0, "{}", rig.draft.size);
    assert_eq!(rig.draft.size, rig.draft.size.round(), "on the spec's steps");
    // Typing 1700 holds 1700: typed values are not snapped to stops.
    rig.click("slider:dialog.size/value");
    rig.type_text("1700");
    rig.key(Key::Enter);
    assert_eq!(rig.draft.size, 1700.0);
    assert_eq!(rig.outs.size.as_ref().map(|o| o.value), Some(None), "set once");
    // A value that is not a number, or Esc, keeps 1700.
    rig.click("slider:dialog.size/value");
    rig.type_text("big");
    rig.key(Key::Enter);
    assert_eq!(rig.draft.size, 1700.0);
    rig.click("slider:dialog.size/value");
    rig.type_text("900");
    rig.key(Key::Escape);
    assert_eq!(rig.draft.size, 1700.0);
    // Past the end it is clamped.
    rig.click("slider:dialog.size/value");
    rig.type_text("99999");
    rig.key(Key::Enter);
    assert_eq!(rig.draft.size, 4000.0);
    // ↑ and ↓ over the track go from stop to stop.
    rig.draft.size = 1700.0;
    rig.hover(SIZE.id);
    rig.key(Key::ArrowUp);
    assert_eq!(rig.draft.size, 2048.0);
    rig.key(Key::ArrowDown);
    rig.key(Key::ArrowDown);
    assert_eq!(rig.draft.size, 1280.0);
    // A double-click resets to the default.
    let at = rig.info(SIZE.id).center();
    rig.clock += 1.0;
    rig.frame(vec![Event::PointerMoved(at), Rig::press(at, true)]);
    rig.frame(vec![Rig::press(at, false)]);
    rig.frame(vec![Rig::press(at, true)]);
    rig.frame(vec![Rig::press(at, false)]);
    rig.settle();
    assert_eq!(rig.draft.size, 1600.0);
    check_ids("stops", &rig.widgets, &registry::unregistered(&rig.ctx));
}

#[test]
fn a_disabled_group_ignores_clicks_and_a_radio_row_governs_its_controls() {
    let mut rig = Rig::new((900, 600), Tokens::default());
    // The videos row is disabled as a group (no videos in the tray): its radios ignore clicks.
    for id in ["choice:dialog.videos/firstFrame", "choice:dialog.videos/fullVideo"] {
        assert!(!rig.info(id).enabled, "{id} is disabled");
    }
    rig.click("choice:dialog.videos/fullVideo");
    assert!(rig.draft.first_frame, "a disabled radio ignores the click");
    // So does the watermark's text while Add watermark is off; ticking it enables the field.
    assert!(!rig.info("field:dialog.watermarkText").enabled);
    rig.click("field:dialog.watermarkText");
    assert!(!rig.ctx.memory(|m| m.focused().is_some()), "a disabled field takes no focus");
    rig.click("check:dialog.watermark");
    assert!(rig.draft.watermark);
    assert!(rig.info("field:dialog.watermarkText").enabled);
    assert!(rig.info("check:dialog.watermark").active, "the checkbox is registered active while on");
    // With videos the group is enabled and the radios work.
    rig.draft.has_videos = true;
    rig.settle();
    rig.click("choice:dialog.videos/fullVideo");
    assert!(!rig.draft.first_frame);
    assert!(rig.info("choice:dialog.videos/fullVideo").active);
    // The size slider and its field follow the Resize radio.
    assert!(rig.info(SIZE.id).enabled && rig.info("slider:dialog.size/value").enabled);
    rig.click("choice:dialog.size/original");
    assert!(!rig.draft.resize);
    assert!(!rig.info(SIZE.id).enabled && !rig.info("slider:dialog.size/value").enabled);
    let (y, x) = (rig.size_y(), rig.size_x(3840.0));
    rig.drag(pos2(rig.size_x(1600.0), y), pos2(x, y));
    assert_eq!(rig.draft.size, 1600.0, "a disabled slider ignores a drag");
    rig.click("choice:dialog.size/resize");
    assert!(rig.draft.resize && rig.info(SIZE.id).enabled);
    // The quality slider is enabled for Custom only.
    assert!(!rig.info(QUALITY.id).enabled);
    check_ids("groups", &rig.widgets, &registry::unregistered(&rig.ctx));
}

#[test]
fn dropdown_items_show_their_descriptions_and_the_checked_one() {
    let mut rig = Rig::new((900, 600), Tokens::default());
    assert!(rig.find("choice:dialog.quality/automatic").is_none(), "closed, no items");
    rig.click("choice:dialog.quality");
    assert!(Rig::popup_open(&rig));
    for (i, (part, title, description)) in PRESETS.iter().enumerate() {
        let id = format!("choice:dialog.quality/{part}");
        let item = rig.info(&id);
        assert_eq!(item.role, Role::MenuItem, "{id}");
        assert_eq!(item.active, i == 0, "{id}: only Automatic is checked");
        assert_eq!(item.label, format!("{title}: {description}"));
        assert_eq!(item.rect[3], 38.0, "{id}: a title line and a description line");
        let r = Rect::from_min_size(pos2(item.rect[0], item.rect[1]), vec2(item.rect[2], item.rect[3]));
        let inside = |text: &str| rig.texts.iter().find(|(s, at)| s == text && r.contains_rect(*at)).unwrap_or_else(|| panic!("{id}: no {text:?}")).1;
        let (t, d) = (inside(title), inside(description));
        assert!(d.min.y >= t.max.y - 0.5 && (d.min.x - t.min.x).abs() <= 0.5, "{id}: the description is under the title");
    }
    // The items share one width, and the check mark's column keeps the titles in line.
    let widths: Vec<f32> = PRESETS.iter().map(|(p, ..)| rig.info(&format!("choice:dialog.quality/{p}")).rect[2]).collect();
    assert!(widths.iter().all(|w| (w - widths[0]).abs() < 0.5), "{widths:?}");
    // A pick checks the item, closes the menu and shows in the dropdown.
    rig.click("choice:dialog.quality/maximum");
    assert_eq!(rig.draft.quality, 2);
    assert!(!Rig::popup_open(&rig), "the menu closes");
    assert_eq!(rig.info("choice:dialog.quality").label, "Maximum");
    rig.click("choice:dialog.quality");
    assert!(rig.info("choice:dialog.quality/maximum").active && !rig.info("choice:dialog.quality/automatic").active);
    // Custom enables its slider and names its value.
    rig.click("choice:dialog.quality/custom");
    assert_eq!(rig.info("choice:dialog.quality").label, "Custom (75)");
    assert!(rig.info(QUALITY.id).enabled);
    check_ids("dropdown", &rig.widgets, &registry::unregistered(&rig.ctx));
}

impl Rig {
    fn popup_open(rig: &Rig) -> bool {
        egui::Popup::is_id_open(&rig.ctx, dropdown_popup_id("choice:dialog.quality"))
    }
}

// ---- The gallery: the Export dialog in a window ----

fn window(size: (u32, u32)) -> (Image, Vec<WidgetInfo>, Vec<registry::Unregistered>) {
    let mut rig = Rig::new(size, Tokens::default());
    rig.draft.numbers = true;
    rig.draft.watermark = true;
    rig.settle();
    if size.0 >= 1400 {
        // The quality menu open, the pointer resting on Normal.
        rig.click("choice:dialog.quality");
        rig.hover("choice:dialog.quality/normal");
    } else {
        rig.hover("choice:dialog.size/original");
    }
    (rig.canvas.image(&rig.ctx).clone(), rig.widgets, registry::unregistered(&rig.ctx))
}

#[test]
fn the_export_dialog_gallery_at_both_window_sizes() {
    let mut failures = vec![];
    for size in [(1400, 900), (900, 600)] {
        let (one, widgets, unregistered) = window(size);
        check_ids(&format!("gallery {size:?}"), &widgets, &unregistered);
        let dialog = widgets.iter().find(|w| w.id == "dialog:export").unwrap().rect;
        for w in widgets.iter().filter(|w| w.id.starts_with("field:") || w.id.starts_with("check:") || w.id.starts_with("slider:")) {
            assert!(w.rect[0] + w.rect[2] <= dialog[0] + dialog[2] + 0.01, "{size:?}: {} leaves the dialog: {:?}", w.id, w.rect);
        }
        // One fingerprint per state: the large window's (the small one has to fit, above).
        if size == (1400, 900) {
            let (two, _, _) = window(size);
            assert!(one.rgba == two.rgba, "two renders of the {size:?} window differ");
            check(&format!("gallery-export-{}x{}", size.0, size.1), &one, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
