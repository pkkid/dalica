//! The widget-state gallery (M3.1): every `ui-kit` widget of `design-system.md` §6 in every state —
//! rest, hover, pressed, active, disabled, focused — each state a tile rendered by the CPU
//! rasterizer with real input (the pointer moved onto the widget, the button held down, the keyboard
//! focus requested), so the states are the ones a user sees, not flags faked for the picture.
//!
//! One sheet per widget (3 × 2 tiles) must render identically twice and match its fingerprint
//! (`fingerprints/widgets-<name>.json`, `architecture.md` §11). The sheets are always written to
//! `target/test-output/ui-kit/widgets-<name>.png`, and all of them stacked to `widgets.png`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).
//!
//! Each tile also checks the registry: the widget's ids are registered with their roles, follow the
//! grammar of `control-protocol.md` §7, and no interactive widget is left without one.

use std::path::PathBuf;

use crate::common::out_dir;
use dalica_scenes::fingerprint;
use dalica_ui_kit::widgets::{
    ButtonKind, DialogButton, Segment, SegmentLabel, SliderSpec, ToastKind, action_button, dialog, dropdown, dropdown_popup_id, edit_slider_value,
    empty_state, icon_button, primary_button, progress_row, search_field, section_header, segmented, slider, splitter, star_toggle, text_button,
    text_field, toast, toggle_filter, tooltip, zoom_slider,
};
use dalica_ui_kit::{Icon, Image, Rasterizer, Role, Tokens, fonts, registry};
use egui::{Align2, Context, Event, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

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

/// Height of the state caption at the top of each tile.
const CAPTION: f32 = 14.0;

/// One widget's sheet.
struct Sheet {
    name: &'static str,
    /// One tile, in pixels (scale 1).
    tile: (u32, u32),
    /// The id the pointer and the focus go to.
    target: &'static str,
    /// The ids each tile must register, with their roles (in any order, others allowed).
    ids: &'static [(&'static str, Role)],
    /// Pairs of states whose tiles must differ (the widget really draws them differently).
    distinct: &'static [(State, State)],
    /// The clock: fixed at `time0`, or running 0.6 s per pass from it (tooltips wait for the
    /// pointer to rest; popups and dialogs fade in).
    time0: f64,
    running: bool,
    /// Draws the widget in the tile's content rect for `state` (disabled is applied around it).
    draw: fn(&mut Ui, &Tokens, State, Rect),
}

const ALL_DISTINCT: &[(State, State)] = &[
    (State::Rest, State::Hover),
    (State::Hover, State::Pressed),
    (State::Rest, State::Active),
    (State::Rest, State::Disabled),
    (State::Rest, State::Focused),
];
const NO_ACTIVE: &[(State, State)] =
    &[(State::Rest, State::Hover), (State::Hover, State::Pressed), (State::Rest, State::Disabled), (State::Rest, State::Focused)];

/// Discards a widget's response (the gallery only looks).
fn ignore<T>(_: T) {}

/// Ends a pass that is not painted: its texture uploads are dropped on purpose.
fn done(mut out: egui::FullOutput) {
    out.textures_delta.clear();
}

fn row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), add);
}

fn column(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
}

const SPEC: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.fill_light", label: "Fill light", min: 0.0, max: 100.0, default: 0.0, step: 1.0, decimals: 0, unit: "" };

fn sheets() -> Vec<Sheet> {
    vec![
        Sheet {
            name: "text_button",
            tile: (150, 56),
            target: "button:gallery.text",
            ids: &[("button:gallery.text", Role::Button)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| row(ui, r, |ui| ignore(text_button(ui, t, "button:gallery.text", "Add Folder…", s == State::Active))),
        },
        Sheet {
            name: "primary_button",
            tile: (150, 56),
            target: "button:dialog.ok",
            ids: &[("button:dialog.ok", Role::Button)],
            distinct: NO_ACTIVE,
            time0: 0.0,
            running: false,
            draw: |ui, t, _, r| row(ui, r, |ui| ignore(primary_button(ui, t, "button:dialog.ok", "Export"))),
        },
        Sheet {
            name: "icon_button",
            tile: (100, 60),
            target: "button:toolbar.newAlbum",
            ids: &[("button:toolbar.newAlbum", Role::IconButton)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                row(ui, r, |ui| {
                    ignore(icon_button(ui, t, "button:toolbar.newAlbum", Icon::Plus, 30.0, s == State::Active, true, "Create a new album"))
                })
            },
        },
        Sheet {
            name: "star_toggle",
            tile: (100, 56),
            target: "button:star",
            ids: &[("button:star", Role::Toggle)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| row(ui, r, |ui| ignore(star_toggle(ui, t, "button:star", s == State::Active))),
        },
        Sheet {
            name: "segmented",
            tile: (130, 56),
            target: "button:tree.tree",
            ids: &[("button:tree.flat", Role::Toggle), ("button:tree.tree", Role::Toggle)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                let segs = [
                    Segment { id: "button:tree.flat", label: SegmentLabel::Icon(Icon::Plus), tooltip: "Show folders as a flat list" },
                    Segment { id: "button:tree.tree", label: SegmentLabel::Icon(Icon::Star), tooltip: "Show the folder tree" },
                ];
                row(ui, r, |ui| ignore(segmented(ui, t, &segs, Some(if s == State::Active { 1 } else { 0 }), vec2(30.0, 28.0))));
            },
        },
        Sheet {
            name: "segmented_text",
            tile: (170, 56),
            target: "button:gallery.month",
            ids: &[("button:gallery.day", Role::Toggle), ("button:gallery.month", Role::Toggle), ("button:gallery.year", Role::Toggle)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                let segs = [
                    Segment { id: "button:gallery.day", label: SegmentLabel::Text("Day"), tooltip: "" },
                    Segment { id: "button:gallery.month", label: SegmentLabel::Text("Month"), tooltip: "" },
                    Segment { id: "button:gallery.year", label: SegmentLabel::Text("Year"), tooltip: "" },
                ];
                row(ui, r, |ui| ignore(segmented(ui, t, &segs, Some(if s == State::Active { 1 } else { 0 }), vec2(40.0, 24.0))));
            },
        },
        Sheet {
            name: "action_button",
            tile: (110, 86),
            target: "action:export",
            ids: &[("action:export", Role::Button)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| row(ui, r, |ui| ignore(action_button(ui, t, "action:export", Icon::Star, "Export", s == State::Active, true))),
        },
        Sheet {
            name: "toggle_filter",
            tile: (100, 56),
            target: "button:filter.starred",
            ids: &[("button:filter.starred", Role::Toggle)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                row(ui, r, |ui| ignore(toggle_filter(ui, t, "button:filter.starred", Icon::Star, s == State::Active, "Show starred photos only")))
            },
        },
        Sheet {
            name: "slider",
            tile: (230, 74),
            target: "slider:tuning.fill_light",
            ids: &[("slider:tuning.fill_light", Role::Other), ("slider:tuning.fill_light/value", Role::Other)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                column(ui, r, |ui| {
                    if s == State::Active {
                        edit_slider_value(ui, &SPEC, 35.0);
                    }
                    ignore(slider(ui, t, &SPEC, 35.0));
                });
            },
        },
        Sheet {
            name: "zoom_slider",
            tile: (150, 48),
            target: "slider:thumbSize",
            ids: &[("slider:thumbSize", Role::Other)],
            distinct: NO_ACTIVE,
            time0: 0.0,
            running: false,
            draw: |ui, t, _, r| {
                let mut v = 144.0;
                row(ui, r, |ui| ignore(zoom_slider(ui, t, "slider:thumbSize", &mut v, 64.0..=320.0, "Thumbnail size")));
            },
        },
        Sheet {
            name: "section_header",
            tile: (200, 52),
            target: "tree:group:folders",
            ids: &[("tree:group:folders", Role::Toggle)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| column(ui, r, |ui| ignore(section_header(ui, t, "tree:group:folders", "Folders", s == State::Active))),
        },
        Sheet {
            name: "dropdown",
            tile: (200, 130),
            target: "button:toolbar.viewOptions",
            ids: &[("button:toolbar.viewOptions", Role::Button)],
            distinct: ALL_DISTINCT,
            time0: 0.0,
            running: true,
            draw: |ui, t, s, r| {
                if s == State::Active {
                    egui::Popup::open_id(ui.ctx(), dropdown_popup_id("button:toolbar.viewOptions"));
                }
                column(ui, r, |ui| {
                    ignore(dropdown(ui, t, "button:toolbar.viewOptions", "Sort by Date", |ui| {
                        for item in ["Sort by Date", "Sort by Name", "Reverse Order"] {
                            ui.label(item);
                        }
                    }));
                });
            },
        },
        Sheet {
            name: "text_field",
            tile: (200, 52),
            target: "field:dialog.name",
            ids: &[("field:dialog.name", Role::Other)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                let mut text = if s == State::Active { "Lisbon weekend".to_owned() } else { String::new() };
                row(ui, r, |ui| ignore(text_field(ui, t, "field:dialog.name", &mut text, "Album name", 180.0)));
            },
        },
        Sheet {
            name: "search_field",
            tile: (220, 52),
            target: "field:search",
            ids: &[("field:search", Role::Other)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                let mut text = if s == State::Active { "beach".to_owned() } else { String::new() };
                row(ui, r, |ui| ignore(search_field(ui, t, "field:search", "button:search.clear", &mut text, "Search", 200.0)));
                if s == State::Active {
                    assert!(registry::find(ui.ctx(), "button:search.clear").is_some(), "the clear button shows with text");
                }
            },
        },
        Sheet {
            name: "tooltip",
            tile: (300, 110),
            target: "button:toolbar.import",
            ids: &[("button:toolbar.import", Role::Button)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: true,
            draw: |ui, t, _, r| {
                row(ui, Rect::from_min_size(r.min, vec2(r.width(), 30.0)), |ui| {
                    let b = text_button(ui, t, "button:toolbar.import", "Import", false);
                    ignore(tooltip(b, t, "Get photos from a camera, card or folder", Some("Ctrl+M")));
                });
            },
        },
        Sheet {
            name: "progress_row",
            tile: (280, 44),
            target: "button:job.1.cancel",
            ids: &[("indicator:job.1", Role::Other), ("button:job.1.cancel", Role::Button)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.6,
            running: false,
            draw: |ui, t, s, r| {
                let fraction = if s == State::Active { None } else { Some(0.42) };
                row(ui, r, |ui| {
                    ignore(progress_row(ui, t, "indicator:job.1", "Scanning Pictures/2024…", fraction, Some("button:job.1.cancel"), 260.0))
                });
            },
        },
        Sheet {
            name: "empty_state",
            tile: (320, 150),
            target: "button:folders.add",
            ids: &[("text:emptyLibrary", Role::Other), ("button:folders.add", Role::Button)],
            distinct: NO_ACTIVE,
            time0: 0.0,
            running: false,
            draw: |ui, t, _, r| {
                column(ui, r, |ui| {
                    ignore(empty_state(
                        ui,
                        t,
                        "text:emptyLibrary",
                        "No photos yet",
                        "Add a folder of photos to start. Dalica never moves or changes them.",
                        Some(("button:folders.add", "Add Folder…")),
                    ));
                });
            },
        },
        Sheet {
            name: "toast",
            tile: (340, 90),
            target: "button:toast.showFolder",
            ids: &[("toast:1", Role::Other), ("button:toast.showFolder", Role::Button)],
            distinct: &[(State::Rest, State::Hover), (State::Rest, State::Active), (State::Rest, State::Focused)],
            time0: 0.0,
            running: false,
            draw: |ui, t, s, r| {
                let kind = if s == State::Active { ToastKind::Error } else { ToastKind::Info };
                let text = if s == State::Active { "Could not move 2 photos" } else { "Moved 3 photos to Holidays" };
                ignore(toast(ui, t, "toast:1", r.expand2(vec2(0.0, 24.0 - 4.0)), text, kind, 0.0, Some(("button:toast.showFolder", "Show"))));
            },
        },
        Sheet {
            name: "splitter",
            tile: (120, 100),
            target: "splitter:tree",
            ids: &[("splitter:tree", Role::Other), ("splitter:gallery", Role::Other)],
            distinct: NO_ACTIVE,
            time0: 0.0,
            running: false,
            draw: |ui, t, _, r| {
                let v = Rect::from_center_size(pos2(r.min.x + 30.0, r.center().y), vec2(8.0, r.height() - 4.0));
                ignore(splitter(ui, t, "splitter:tree", v));
                let h = Rect::from_center_size(pos2(r.min.x + 80.0, r.center().y), vec2(60.0, 8.0));
                ignore(splitter(ui, t, "splitter:gallery", h));
            },
        },
        Sheet {
            name: "dialog",
            tile: (380, 190),
            target: "button:dialog.ok",
            ids: &[
                ("dialog:gallery", Role::Other),
                ("dialog:gallery/backdrop", Role::Other),
                ("button:dialog.ok", Role::Button),
                ("button:dialog.cancel", Role::Button),
            ],
            distinct: &[(State::Rest, State::Hover), (State::Hover, State::Pressed), (State::Rest, State::Disabled), (State::Rest, State::Focused)],
            time0: 0.0,
            running: true,
            draw: |ui, t, s, r| {
                // Something behind it, to show the backdrop dims it.
                ui.painter().text(r.min + vec2(4.0, 4.0), Align2::LEFT_TOP, "The app behind the dialog", fonts::regular(t.body), t.text);
                let buttons = [
                    DialogButton { id: "button:dialog.cancel", label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
                    DialogButton { id: "button:dialog.ok", label: "Remove", kind: ButtonKind::Default, enabled: s != State::Disabled },
                ];
                let out = dialog(ui.ctx(), t, "dialog:gallery", "Remove from album", 300.0, &buttons, |ui| {
                    ui.label("Remove 3 photos from Holidays? The photos stay in their folders.");
                });
                assert!(out.clicked.is_none(), "nothing clicks in the gallery");
            },
        },
    ]
}

/// The input of one pass of a tile.
fn input(sheet: &Sheet, pass: usize, events: Vec<Event>) -> egui::RawInput {
    let mut input = dalica_ui_kit::raw_input(sheet.tile.0, sheet.tile.1, 1.0);
    // 0.6 s a pass: the pointer has rested past the tooltip delay one pass before the kept frame
    // (so the tooltip's fade-in is over), a press is held 0.6 s (still a click, not a long press),
    // and popups and dialogs have faded in.
    input.time = Some(if sheet.running { sheet.time0 + 0.6 * pass as f64 } else { sheet.time0 });
    input.events = events;
    input
}

struct Tile {
    image: Image,
    widgets: Vec<dalica_ui_kit::WidgetInfo>,
    unregistered: Vec<registry::Unregistered>,
}

/// Renders one tile: two passes to settle (fonts, layout), then the state's input, then the frame
/// that is kept.
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
        let out = ctx.run_ui(input(sheet, pass, events), &draw);
        last = Some(dalica_ui_kit::paint(&ctx, &mut raster, out, sheet.tile.0, sheet.tile.1, t.chrome).unwrap());
        if pass == 2 {
            let info = registry::find(&ctx, sheet.target).unwrap_or_else(|| panic!("{}: {:?} registers {}", sheet.name, state, sheet.target));
            target = Some((info.center(), info.egui_id));
        }
    }
    Tile { image: last.unwrap(), widgets: registry::widgets(&ctx), unregistered: registry::unregistered(&ctx) }
}

/// A sheet: its six tiles in 3 columns × 2 rows, 1 px `divider` lines between them.
fn render_sheet(sheet: &Sheet) -> (Image, Vec<(State, Tile)>) {
    let (tw, th) = sheet.tile;
    let (w, h) = (3 * tw + 2, 2 * th + 1);
    let divider = Tokens::default().divider.to_array();
    let mut rgba: Vec<u8> = divider.repeat((w * h) as usize);
    let mut tiles = Vec::new();
    for (i, state) in STATES.into_iter().enumerate() {
        let tile = render_tile(sheet, state);
        let (cx, cy) = ((i as u32 % 3) * (tw + 1), (i as u32 / 3) * (th + 1));
        for y in 0..th {
            let src = (y * tw * 4) as usize..((y + 1) * tw * 4) as usize;
            let dst = (((cy + y) * w + cx) * 4) as usize;
            rgba[dst..dst + src.len()].copy_from_slice(&tile.image.rgba[src]);
        }
        tiles.push((state, tile));
    }
    (Image { width: w, height: h, rgba }, tiles)
}

/// The tile's pixels below the caption (the captions always differ).
fn body(tile: &Tile) -> &[u8] {
    let start = (CAPTION as u32 * tile.image.width * 4) as usize;
    &tile.image.rgba[start..]
}

#[test]
fn every_widget_in_every_state_renders_identically_twice_and_matches_its_fingerprint() {
    let sheets = sheets();
    let out = out_dir();
    let bless = std::env::var_os("DALICA_BLESS").is_some();
    let mut failures = Vec::new();
    let mut stack: Vec<Image> = Vec::new();
    for sheet in &sheets {
        let (a, tiles) = render_sheet(sheet);
        let (b, _) = render_sheet(sheet);
        assert!(a.rgba == b.rgba, "{}: two renders of the same sheet differ", sheet.name);
        std::fs::write(out.join(format!("widgets-{}.png", sheet.name)), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();

        for (state, tile) in &tiles {
            let ids: Vec<(&str, Role)> = tile.widgets.iter().map(|w| (w.id.as_str(), w.role)).collect();
            for want in sheet.ids {
                assert!(ids.contains(want), "{} {:?}: {want:?} not registered in {ids:?}", sheet.name, state);
            }
            for w in &tile.widgets {
                assert!(registry::is_valid_id(&w.id), "{}: {:?} breaks the id grammar", sheet.name, w.id);
            }
            assert_eq!(tile.unregistered, [], "{} {:?}: interactive widgets without an id", sheet.name, state);
            let enabled = tile.widgets.iter().find(|w| w.id == sheet.target).map(|w| w.enabled);
            if *state == State::Disabled && sheet.name != "toast" {
                assert_eq!(enabled, Some(false), "{}: the disabled tile's target is disabled", sheet.name);
            }
        }
        let tile = |s: State| tiles.iter().find(|(st, _)| *st == s).map(|(_, t)| t).unwrap();
        for &(x, y) in sheet.distinct {
            assert!(body(tile(x)) != body(tile(y)), "{}: {:?} and {:?} look the same", sheet.name, x, y);
        }

        let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/widgets-{}.json", sheet.name));
        if bless {
            std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        }
        match std::fs::read_to_string(&stored).map_err(|e| e.to_string()).and_then(|s| fingerprint::from_json(&s)) {
            Ok(expected) => {
                if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
                    std::fs::write(out.join(format!("widgets-{}-actual.png", sheet.name)), &m.actual_png).unwrap();
                    failures.push(format!("{}: {}", sheet.name, m.message));
                }
            }
            Err(e) => failures.push(format!("{}: no fingerprint ({e}); run with DALICA_BLESS=1 and look at the PNG", sheet.name)),
        }
        stack.push(a);
    }
    // Everything on one page, to look at.
    let width = stack.iter().map(|i| i.width).max().unwrap_or(1);
    let height: u32 = stack.iter().map(|i| i.height + 8).sum();
    let mut page = Tokens::default().grid_bg.to_array().repeat((width * height) as usize);
    let mut y0 = 0;
    for img in &stack {
        for y in 0..img.height {
            let src = &img.rgba[(y * img.width * 4) as usize..((y + 1) * img.width * 4) as usize];
            let dst = (((y0 + y) * width) * 4) as usize;
            page[dst..dst + src.len()].copy_from_slice(src);
        }
        y0 += img.height + 8;
    }
    std::fs::write(out.join("widgets.png"), fingerprint::png_rgba(width, height, &page)).unwrap();
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out.display());
}

/// The states are real: hovering, pressing and focusing move the registry's state and egui's, not
/// only the pixels. Spot checks on a few widgets that the harness drives as a user would.
#[test]
fn the_harness_drives_real_hover_press_and_focus() {
    let sheets = sheets();
    let sheet = sheets.iter().find(|s| s.name == "toggle_filter").unwrap();
    let rest = render_tile(sheet, State::Rest);
    let active = render_tile(sheet, State::Active);
    let find = |tile: &Tile| tile.widgets.iter().find(|w| w.id == sheet.target).cloned().unwrap();
    assert!(!find(&rest).active && find(&active).active, "the toggle's active state is registered");
    let t = Tokens::default();
    // The on toggle has its accent stroke on the tile's left edge, the off one does not.
    let edge = |tile: &Tile| {
        let w = find(tile);
        tile.image.pixel(w.rect[0] as u32, w.center().y as u32).unwrap()
    };
    assert_eq!(edge(&active)[..3], t.accent.to_array()[..3], "on: 1 pt accent stroke");
    assert_eq!(edge(&rest)[..3], t.chrome.to_array()[..3], "off: no fill at rest");
    let hover = render_tile(sheet, State::Hover);
    assert_ne!(edge(&hover)[..3], t.chrome.to_array()[..3], "hovered: filled");
}

/// Widgets in a gallery of their own context still answer like controls: a click on a segment
/// returns its index, a click on the dialog's default button chooses it, Esc chooses Cancel.
#[test]
fn widgets_report_clicks_and_keys() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let segs = [
        Segment { id: "button:tree.flat", label: SegmentLabel::Text("Flat"), tooltip: "" },
        Segment { id: "button:tree.tree", label: SegmentLabel::Text("Tree"), tooltip: "" },
    ];
    let mut clicked = None;
    let pass = |events: Vec<Event>, clicked: &mut Option<usize>| {
        let mut input = dalica_ui_kit::raw_input(300, 200, 1.0);
        input.events = events;
        let out = ctx.run_ui(input, |ui| {
            if let Some(i) = segmented(ui, &t, &segs, Some(0), vec2(40.0, 24.0)) {
                *clicked = Some(i);
            }
        });
        done(out);
    };
    pass(vec![], &mut clicked);
    pass(vec![], &mut clicked);
    let tree = registry::find(&ctx, "button:tree.tree").unwrap().center();
    pass(vec![Event::PointerMoved(tree)], &mut clicked);
    pass(vec![Event::PointerButton { pos: tree, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], &mut clicked);
    pass(vec![Event::PointerButton { pos: tree, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], &mut clicked);
    assert_eq!(clicked, Some(1));

    // The dialog: Esc chooses the cancel button, Enter the default one.
    let ctx = Context::default();
    t.apply(&ctx);
    let buttons = [
        DialogButton { id: "button:dialog.cancel", label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
        DialogButton { id: "button:dialog.ok", label: "OK", kind: ButtonKind::Default, enabled: true },
    ];
    let run = |key: Option<egui::Key>| {
        let mut input = dalica_ui_kit::raw_input(500, 300, 1.0);
        if let Some(key) = key {
            input.events = vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }];
        }
        let mut chosen = None;
        done(ctx.run_ui(input, |ui| chosen = dialog(ui.ctx(), &t, "dialog:test", "Test", 300.0, &buttons, |_| ()).clicked));
        chosen
    };
    assert_eq!(run(None), None);
    assert_eq!(run(None), None);
    assert_eq!(run(Some(egui::Key::Escape)), Some(0));
    assert_eq!(run(Some(egui::Key::Enter)), Some(1));
    // A disabled default is never chosen by Enter.
    let ctx2 = Context::default();
    t.apply(&ctx2);
    let off = [buttons[0], DialogButton { enabled: false, ..buttons[1] }];
    for _ in 0..2 {
        done(ctx2.run_ui(dalica_ui_kit::raw_input(500, 300, 1.0), |ui| ignore(dialog(ui.ctx(), &t, "dialog:test", "Test", 300.0, &off, |_| ()))));
    }
    let mut input = dalica_ui_kit::raw_input(500, 300, 1.0);
    input.events = vec![Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }];
    let mut chosen = Some(9);
    done(ctx2.run_ui(input, |ui| chosen = dialog(ui.ctx(), &t, "dialog:test", "Test", 300.0, &off, |_| ()).clicked));
    assert_eq!(chosen, None);
}

/// Sliders change their value by drag, by typing and by the arrow keys, and report one undo step.
#[test]
fn the_slider_drags_types_and_nudges() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut value = 35.0;
    let mut log: Vec<(Option<f64>, bool, bool)> = Vec::new();
    let pass = |events: Vec<Event>, value: &mut f64, log: &mut Vec<(Option<f64>, bool, bool)>| {
        let mut input = dalica_ui_kit::raw_input(300, 100, 1.0);
        input.events = events;
        done(ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(10.0, 10.0), vec2(220.0, 45.0))), |ui| {
                let out = slider(ui, &t, &SPEC, *value);
                if let Some(v) = out.value {
                    *value = v;
                }
                if out.value.is_some() || out.drag_started || out.drag_stopped {
                    log.push((out.value, out.drag_started, out.drag_stopped));
                }
            });
        }));
    };
    pass(vec![], &mut value, &mut log);
    pass(vec![], &mut value, &mut log);
    let track = registry::find(&ctx, "slider:tuning.fill_light").unwrap();
    let [x, y, w, _] = track.rect;
    // Press at the start of the track, drag to its middle, release: one drag, 50 at the end.
    let start = pos2(x + 8.0, y + 10.0);
    let mid = pos2(x + w / 2.0, y + 10.0);
    pass(vec![Event::PointerMoved(start)], &mut value, &mut log);
    pass(vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], &mut value, &mut log);
    pass(vec![Event::PointerMoved(pos2(start.x + 20.0, start.y))], &mut value, &mut log);
    pass(vec![Event::PointerMoved(mid)], &mut value, &mut log);
    pass(vec![Event::PointerButton { pos: mid, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], &mut value, &mut log);
    assert_eq!(value, 50.0, "{log:?}");
    assert_eq!(log.iter().filter(|e| e.1).count(), 1, "one drag start: {log:?}");
    assert_eq!(log.iter().filter(|e| e.2).count(), 1, "one drag stop: {log:?}");
    // ↑ with the pointer on the row nudges by one step.
    log.clear();
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    pass(vec![key(egui::Key::ArrowUp)], &mut value, &mut log);
    assert_eq!(value, 51.0);
    // Typing: open the value, type, press Return.
    done(ctx.run_ui(dalica_ui_kit::raw_input(300, 100, 1.0), |ui| edit_slider_value(ui, &SPEC, value)));
    pass(vec![], &mut value, &mut log);
    pass(vec![], &mut value, &mut log);
    pass(vec![Event::Text("72".into())], &mut value, &mut log);
    pass(vec![key(egui::Key::Enter)], &mut value, &mut log);
    pass(vec![], &mut value, &mut log);
    assert_eq!(value, 72.0, "{log:?}");
}

/// The thin zoom slider follows a click and the arrow keys and keeps its value in range.
#[test]
fn the_zoom_slider_clicks_and_clamps() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut v = 1000.0f32;
    let pass = |events: Vec<Event>, v: &mut f32| {
        let mut input = dalica_ui_kit::raw_input(300, 100, 1.0);
        input.events = events;
        done(ctx.run_ui(input, |ui| ignore(zoom_slider(ui, &t, "slider:thumbSize", v, 64.0..=320.0, "Thumbnail size"))));
    };
    pass(vec![], &mut v);
    assert_eq!(v, 320.0, "pulled into the range");
    pass(vec![], &mut v);
    let w = registry::find(&ctx, "slider:thumbSize").unwrap();
    let left = pos2(w.rect[0] + 1.0, w.center().y);
    pass(vec![Event::PointerMoved(left)], &mut v);
    pass(vec![Event::PointerButton { pos: left, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], &mut v);
    pass(vec![Event::PointerButton { pos: left, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], &mut v);
    assert_eq!(v, 64.0);
    let mut nan = f32::NAN;
    pass(vec![], &mut nan);
    assert_eq!(nan, 64.0, "NaN becomes the low end");
}

/// The search field's clear button empties the text and says so.
#[test]
fn the_search_field_clears() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut text = "beach".to_owned();
    let mut cleared = false;
    let pass = |events: Vec<Event>, text: &mut String, cleared: &mut bool| {
        let mut input = dalica_ui_kit::raw_input(300, 100, 1.0);
        input.events = events;
        done(ctx.run_ui(input, |ui| {
            let out = search_field(ui, &t, "field:search", "button:search.clear", text, "Search", 200.0);
            *cleared |= out.cleared && out.changed;
        }));
    };
    pass(vec![], &mut text, &mut cleared);
    pass(vec![], &mut text, &mut cleared);
    let c = registry::find(&ctx, "button:search.clear").unwrap().center();
    pass(vec![Event::PointerMoved(c)], &mut text, &mut cleared);
    pass(vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], &mut text, &mut cleared);
    pass(vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], &mut text, &mut cleared);
    assert!(text.is_empty() && cleared);
    pass(vec![], &mut text, &mut cleared);
    assert!(registry::find(&ctx, "button:search.clear").is_none(), "no clear button on an empty field");
}
