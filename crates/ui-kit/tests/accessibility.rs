//! Accessibility (M15.14, `design-system.md` §9): one of every `ui-kit` widget, headless, with
//! AccessKit on, read back from the tree egui hands a screen reader (VoiceOver, Narrator, AT-SPI).
//! No interactive node goes without a name; every slider has its value and range; a checkbox's
//! state follows a click; Tab walks the widgets in the order they are drawn and Space activates
//! the focused one; the focused widget shows the focus ring, whose colour stands 3:1 on the
//! surfaces controls sit on; the app's text table renames what the widgets say themselves.
//!
//! The audit (what counts as interactive, the name and slider checks) is adapted from LightCraft
//! `crates/ui-egui/tests/accessibility.rs` at revision d999e382
//! (<https://github.com/storytold/lightcraft>, MIT OR Apache-2.0; `NOTICE`). Changes for Dalica:
//! the tree is read straight from egui's `TreeUpdate` instead of through egui_kittest, over the
//! widget kit's own gallery rather than the app's views; the focus ring and the text table are
//! checked too.

use dalica_core::geo::LatLon;
use dalica_ui_kit::icons::{CollageIcon, Glyph, ViewingIcon};
use dalica_ui_kit::text::{self, Text, Texts};
use dalica_ui_kit::theme::contrast;
use dalica_ui_kit::widgets::*;
use dalica_ui_kit::{Icon, Tokens, registry};
use egui::accesskit::{self, Action, NodeId, Role as A, Toggled};
use egui::epaint::{ClippedShape, ColorMode, Shape};
use egui::{Color32, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const SIZE: (u32, u32) = (1400, 900);

/// What the gallery's widgets edit and report.
struct St {
    checked: bool,
    text: String,
    search: String,
    date: PickedDate,
    time: PickedTime,
    zoom: f32,
    age: Option<u32>,
    clicked: Vec<&'static str>,
}

impl Default for St {
    fn default() -> St {
        St {
            checked: false,
            text: String::new(),
            search: "beach".into(),
            date: PickedDate::new(2026, 10, 19).unwrap(),
            time: PickedTime::new(14, 2, 0).unwrap(),
            zoom: 160.0,
            age: Some(30),
            clicked: Vec::new(),
        }
    }
}

const SIZE_SPEC: SliderSpec<'static> =
    SliderSpec { id: "slider:a11y.size", label: "Resize to", min: 100.0, max: 4000.0, default: 1600.0, step: 1.0, decimals: 0, unit: " pixels" };
const FILL: SliderSpec<'static> =
    SliderSpec { id: "slider:a11y.fill", label: "Fill light", min: 0.0, max: 100.0, default: 0.0, step: 1.0, decimals: 0, unit: "" };
const TUNE: SliderSpec<'static> =
    SliderSpec { id: "slider:a11y.highlights", label: "Highlights", min: 0.0, max: 100.0, default: 0.0, step: 1.0, decimals: 0, unit: "" };
const EFFECT: SliderSpec<'static> =
    SliderSpec { id: "slider:a11y.radius", label: "Radius", min: 1.0, max: 100.0, default: 10.0, step: 1.0, decimals: 0, unit: " %" };
const STOPS: [f64; 3] = [800.0, 1600.0, 2400.0];

/// The first row: plain buttons, the Tab order test walks it.
const FIRST_ROW: [&str; 5] = ["button:a11y.text", "button:a11y.icon", "button:a11y.star", "button:a11y.primary", "button:a11y.action"];

/// Lays `add` out left to right in `rect`.
fn at<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        add(ui)
    })
    .inner
}

fn row(y: f32) -> Rect {
    Rect::from_min_size(pos2(16.0, y), vec2(1368.0, 60.0))
}

/// One of every interactive widget the kit has.
fn gallery(ui: &mut Ui, t: &Tokens, st: &mut St) {
    at(ui, row(10.0), |ui| {
        if text_button(ui, t, FIRST_ROW[0], "Add Folder…", false).clicked() {
            st.clicked.push(FIRST_ROW[0]);
        }
        if icon_button(ui, t, FIRST_ROW[1], Icon::Plus, 28.0, false, true, "Create a new album").clicked() {
            st.clicked.push(FIRST_ROW[1]);
        }
        star_toggle(ui, t, FIRST_ROW[2], true);
        primary_button(ui, t, FIRST_ROW[3], "Export");
        action_button(ui, t, FIRST_ROW[4], Icon::Star, "Star", false, true);
        toggle_filter(ui, t, "button:a11y.filter", Icon::Star, true, "Show starred photos only");
        let segs = [
            Segment { id: "button:a11y.flat", label: SegmentLabel::Text("Flat"), tooltip: "" },
            Segment { id: "button:a11y.tree", label: SegmentLabel::Text("Tree"), tooltip: "" },
        ];
        segmented(ui, t, &segs, Some(0), vec2(40.0, 24.0));
        tool_button(ui, t, "tool:a11y.crop", Icon::Loupe, "Crop", true, true, 70.0, "Crop the photo");
        dropdown(ui, t, "dropdown:a11y.sort", "By date", |_| {});
        let row = RichRow { glyph: Glyph::Collage(CollageIcon::NoBorder), name: "Picture pile", description: "Photos scattered" };
        rich_dropdown(ui, t, "dropdown:a11y.style", &row, 200.0, |_| {});
        key_cell(ui, t, "key:a11y.open", &KeyCell { keys: &["Ctrl+O"], mac: false, recording: false, conflict: false, width: 140.0, stop: true });
        key_cell(ui, t, "key:a11y.none", &KeyCell { keys: &[], mac: false, recording: false, conflict: false, width: 140.0, stop: true });
    });
    at(ui, row(80.0), |ui| {
        ui.allocate_ui(vec2(240.0, 50.0), |ui| slider(ui, t, &FILL, 25.0));
        ui.allocate_ui(vec2(240.0, 50.0), |ui| tuning_slider(ui, t, &TUNE, 40.0));
        ui.allocate_ui(vec2(240.0, 60.0), |ui| effect_slider(ui, t, &EFFECT, 10.0, SliderScale::Log));
        stops_slider(ui, t, &StopsSlider { spec: SIZE_SPEC, stops: &STOPS, field: false, track_w: 160.0 }, 1600.0);
        zoom_slider(ui, t, "slider:a11y.zoom", &mut st.zoom, 64.0..=320.0, "Thumbnail size");
        age_slider(ui, t, "slider:age", &mut st.age, 100.0, "Show only recent photos");
    });
    at(ui, row(150.0), |ui| {
        if checkbox(ui, t, "check:a11y.raw", "Show RAW files", &mut st.checked).clicked() {
            st.clicked.push("check:a11y.raw");
        }
        radio(ui, t, "radio:a11y.original", "Original size", true);
        let mut on = true;
        checkbox_row(ui, t, "check:a11y.later", "Check for updates", &mut on, Some("Applies at next launch."), None);
        text_field(ui, t, "field:a11y.caption", &mut st.text, "Make a caption!", 140.0);
        search_field(ui, t, "search:a11y", "button:a11y.clearSearch", &mut st.search, "Search", 160.0);
        date_field(ui, t, "field:a11y.date", &mut st.date, DATE_WIDTH);
        time_field(ui, t, "field:a11y.time", &mut st.time, TIME_WIDTH);
    });
    at(ui, Rect::from_min_size(pos2(16.0, 220.0), vec2(280.0, 40.0)), |ui| {
        let items = [
            TabItem { name: "basic", icon: Icon::Properties, tooltip: "Basic fixes" },
            TabItem { name: "tuning", icon: Icon::Loupe, tooltip: "Lighting and color tuning" },
        ];
        tabs(ui, t, "tab:a11y.edit", &items, 0);
    });
    at(ui, Rect::from_min_size(pos2(320.0, 220.0), vec2(360.0, 40.0)), |ui| {
        text_tabs(ui, t, "tab:a11y.options", &[TextTab { name: "general", label: "General" }, TextTab { name: "export", label: "Export" }], 1);
    });
    at(ui, Rect::from_min_size(pos2(700.0, 220.0), vec2(500.0, 40.0)), |ui| {
        let rooms = [
            RoomTab { name: "library", icon: Glyph::Viewing(ViewingIcon::Library), label: "Library", closable: false },
            RoomTab { name: "import", icon: Glyph::Shell(Icon::Import), label: "Import", closable: true },
        ];
        room_tabs(ui, t, "tab:a11y.rooms", &rooms, 0);
    });
    ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(16.0, 280.0), vec2(400.0, 120.0))), |ui| {
        section_header(ui, t, "tree:group:a11y", "Folders", true);
        group_header(ui, t, "group:a11y", "October 2026", "12 photos", false);
        list_row(ui, t, "row:a11y.open", "Open", &[120.0, 120.0], true, false, |ui, cell| {
            list_text(ui, t, cell, if cell.column == 0 { "Open" } else { "Ctrl+O" }, false);
        });
    });
    at(ui, row(420.0), |ui| {
        swatch(ui, t, "swatch:a11y.tint", "Tint", [200, 120, 40], false);
        ui.allocate_ui(vec2(90.0, 80.0), |ui| {
            effect_tile(
                ui,
                t,
                &EffectTile { name: "sepia", label: "Sepia", tooltip: "Warm brown tones", preview: None, one_click: true, enabled: true },
            )
        });
        face_tile(ui, t, &FaceTile { side: 48.0, label: FaceLabel::None, ignore: true, marks: true, ..FaceTile::new("a11y") });
        suggest_marks(ui, t, "suggest:a11y");
        stepper(ui, t, "stepper:a11y.seconds", 3, 1..=60, |v| format!("{v} seconds"));
        scrubber(ui, t, "scrub:a11y", &Scrubber { position_ms: 9_000, start_ms: 2_000, end_ms: 23_000, ..Scrubber::new(41_000) }, 220.0);
        volume(ui, t, "volume:a11y", 0.5, false);
        let toggles = [
            ImageToggle { id: "toggle:a11y.none", glyph: Glyph::Collage(CollageIcon::NoBorder), tooltip: "No border" },
            ImageToggle { id: "toggle:a11y.white", glyph: Glyph::Collage(CollageIcon::WhiteBorder), tooltip: "" },
        ];
        toggle_group(ui, t, &toggles, 0, vec2(32.0, 32.0));
    });
    at(ui, row(520.0), |ui| {
        progress_row(ui, t, "progress:a11y", "Exporting 3 of 12", Some(0.25), Some("button:a11y.cancel"), 300.0);
        filter_banner(ui, t, "banner:a11y", "button:a11y.showAll", "Showing photos that match “beach”");
    });
    splitter(ui, t, "splitter:a11y", Rect::from_min_size(pos2(16.0, 600.0), vec2(8.0, 120.0)));
    let photo = Rect::from_min_size(pos2(40.0, 600.0), vec2(360.0, 240.0));
    ui.painter().rect_filled(photo, 0, Color32::from_rgb(70, 110, 150));
    let face = Rect::from_min_max(pos2(0.1, 0.1), pos2(0.3, 0.4));
    face_box(ui, t, "facebox:a11y", photo, &FaceBox { rect: face, label: Some("Ana Costa"), highlighted: false, edit: true });
    crosshair(ui, t, "crosshair:a11y", photo, &Crosshair { centre: [0.6, 0.5], radius: Some(0.2), handle: Some([0.8, 0.5]) });
    transform_handle(
        ui,
        t,
        "transform:a11y",
        &TransformHandle { rect: Rect::from_min_size(pos2(440.0, 640.0), vec2(160.0, 120.0)), angle: 15.0, scale: 1.2 },
    );
    let lisbon = LatLon::new(38.72, -9.14).unwrap();
    let markers = [MapMarker { at: lisbon, count: 3 }];
    let m = Map { markers: &markers, ..Map::new("a11y", MapView::new(lisbon, 5)) };
    map(ui, t, Rect::from_min_size(pos2(700.0, 600.0), vec2(400.0, 260.0)), &m);
    let bar = Rect::from_min_size(pos2(1120.0, 600.0), vec2(220.0, 56.0));
    overlay_bar(ui, t, "overlay:a11y", bar, 1.0, |ui| {
        overlay_button(ui, t, "button:a11y.play", Glyph::Shell(Icon::Star), None, false, true, "Play");
    });
}

/// The gallery in a headless context with AccessKit on.
struct Rig {
    ctx: Context,
    t: Tokens,
    st: St,
    tree: accesskit::TreeUpdate,
    shapes: Vec<ClippedShape>,
}

impl Rig {
    fn new(texts: Option<Texts>) -> Rig {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        ctx.enable_accesskit();
        if let Some(texts) = texts {
            text::set(&ctx, texts);
        }
        let tree = accesskit::TreeUpdate { nodes: Vec::new(), tree: None, tree_id: accesskit::TreeId::ROOT, focus: NodeId(0) };
        let mut rig = Rig { ctx, t, st: St::default(), tree, shapes: Vec::new() };
        rig.pass(Vec::new());
        rig.pass(Vec::new());
        rig
    }

    fn pass(&mut self, events: Vec<Event>) {
        let mut input = dalica_ui_kit::raw_input(SIZE.0, SIZE.1, 1.0);
        input.events = events;
        let (t, st) = (&self.t, &mut self.st);
        let mut out = self.ctx.run_ui(input, |ui| gallery(ui, t, st));
        out.textures_delta.clear();
        self.tree = out.platform_output.accesskit_update.expect("AccessKit is on");
        self.shapes = out.shapes;
    }

    /// The node of the widget registered as `id`.
    fn node(&self, id: &str) -> &accesskit::Node {
        let egui_id = registry::find(&self.ctx, id).and_then(|w| w.egui_id).unwrap_or_else(|| panic!("{id} is registered"));
        let key = egui_id.accesskit_id();
        self.tree.nodes.iter().find(|(n, _)| *n == key).map(|(_, n)| n).unwrap_or_else(|| panic!("{id} is in the tree"))
    }

    /// The focused node, if a widget has the focus.
    fn focused(&self) -> Option<(NodeId, &accesskit::Node)> {
        let id = self.tree.focus;
        self.tree.nodes.iter().find(|(n, node)| *n == id && node.role() != A::Window).map(|(n, node)| (*n, node))
    }

    /// Every node in the order it was drawn: depth first from the root, children in order.
    fn drawn(&self) -> Vec<NodeId> {
        let by_id: std::collections::HashMap<NodeId, &accesskit::Node> = self.tree.nodes.iter().map(|(id, n)| (*id, n)).collect();
        let mut out = Vec::new();
        let mut stack = vec![self.tree.tree.as_ref().map_or(NodeId(0), |t| t.root)];
        while let Some(id) = stack.pop() {
            out.push(id);
            if let Some(n) = by_id.get(&id) {
                stack.extend(n.children().iter().rev().copied());
            }
        }
        out
    }

    /// The automation id of the widget whose node is `node`.
    fn automation_id(&self, node: NodeId) -> Option<String> {
        registry::widgets(&self.ctx).into_iter().find(|w| w.egui_id.is_some_and(|e| e.accesskit_id() == node)).map(|w| w.id)
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.pass(vec![ev(true)]);
        self.pass(vec![ev(false)]);
    }

    fn click(&mut self, at: Pos2) {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.pass(vec![Event::PointerMoved(at)]);
        self.pass(vec![button(true)]);
        self.pass(vec![button(false)]);
        self.pass(Vec::new());
    }
}

/// A role a screen reader treats as a control.
fn interactive(role: A) -> bool {
    matches!(
        role,
        A::Button
            | A::CheckBox
            | A::Switch
            | A::RadioButton
            | A::Slider
            | A::SpinButton
            | A::TextInput
            | A::MultilineTextInput
            | A::SearchInput
            | A::ComboBox
            | A::EditableComboBox
            | A::Tab
            | A::Link
            | A::MenuItem
            | A::MenuItemCheckBox
            | A::MenuItemRadio
            | A::ListBoxOption
            | A::TreeItem
    )
}

#[test]
fn no_interactive_node_is_without_a_name_and_every_slider_has_its_value_and_range() {
    let rig = Rig::new(None);
    let mut problems = Vec::new();
    let mut count = 0;
    let mut sliders = 0;
    for (id, n) in &rig.tree.nodes {
        let role = n.role();
        // egui's own scroll bars are announced by their role, as native ones are.
        if role == A::ScrollBar {
            continue;
        }
        // A widget that takes the focus or a click is interactive whatever its role.
        let acts = |a| n.supports_action(a);
        if !interactive(role) && !acts(Action::Focus) && !acts(Action::Click) {
            continue;
        }
        count += 1;
        let name = n.label().filter(|l| !l.trim().is_empty()).or(n.value()).unwrap_or_default();
        let who = rig.automation_id(*id).unwrap_or_else(|| format!("{id:?}"));
        if name.trim().is_empty() {
            problems.push(format!("{who}: unnamed {role:?} at {:?}", n.bounds()));
        }
        if role == A::Slider {
            sliders += 1;
            let (v, lo, hi) = (n.numeric_value(), n.min_numeric_value(), n.max_numeric_value());
            match (v, lo, hi) {
                (Some(v), Some(lo), Some(hi)) if lo <= v && v <= hi => {}
                _ => problems.push(format!("{who}: slider {name:?} lacks its value and range ({v:?} in {lo:?} … {hi:?})")),
            }
            if n.value().is_none_or(str::is_empty) {
                problems.push(format!("{who}: slider {name:?} does not say its value as text"));
            }
        }
    }
    assert!(count > 60, "the gallery's tree has its interactive nodes ({count})");
    // slider, tuning, effect, stops, zoom, age, the scrubber's three, volume, the hue strip (closed: none).
    assert!(sliders >= 10, "{sliders} sliders");
    assert!(registry::unregistered(&rig.ctx).is_empty(), "{:?}", registry::unregistered(&rig.ctx));
    assert!(problems.is_empty(), "{} accessibility problems:\n{}", problems.len(), problems.join("\n"));
}

/// A widget drawn just outside its clip (the row a scrolled list lays out past its edge) still
/// has a node, and the node has its name, though the widget is not registered (not on screen).
#[test]
fn a_widget_scrolled_out_of_sight_keeps_its_name() {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    ctx.enable_accesskit();
    let mut tree = None;
    for _ in 0..2 {
        let mut out = ctx.run_ui(dalica_ui_kit::raw_input(400, 300, 1.0), |ui| {
            ui.set_clip_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 100.0)));
            let rect = Rect::from_min_size(pos2(10.0, 150.0), vec2(200.0, 24.0));
            let r = ui.interact(rect, egui::Id::new("row:a11y.hidden"), egui::Sense::click());
            register(ui.ctx(), "row:a11y.hidden", dalica_ui_kit::Role::TreeRow, "Hidden folder", Target::Widget(&r), false);
        });
        out.textures_delta.clear();
        tree = out.platform_output.accesskit_update;
    }
    let tree = tree.expect("AccessKit is on");
    let key = egui::Id::new("row:a11y.hidden").accesskit_id();
    let node = tree.nodes.iter().find(|(n, _)| *n == key).map(|(_, n)| n).expect("the clipped row has a node");
    assert_eq!(node.label(), Some("Hidden folder"));
    assert!(registry::find(&ctx, "row:a11y.hidden").is_none(), "not registered: not on screen");
}

#[test]
fn widgets_say_their_state() {
    let rig = Rig::new(None);
    let n = |id| rig.node(id);
    // Toggles and checkboxes are checked or not; tabs and rows selected.
    assert_eq!(n("button:a11y.star").toggled(), Some(Toggled::True));
    assert_eq!(n("check:a11y.raw").toggled(), Some(Toggled::False));
    assert_eq!(n("check:a11y.raw").role(), A::CheckBox);
    assert_eq!(n("check:a11y.later").description(), Some("Applies at next launch."));
    assert_eq!(n("radio:a11y.original").role(), A::RadioButton);
    assert_eq!(n("tab:a11y.edit/basic").is_selected(), Some(true));
    assert_eq!(n("tab:a11y.options/general").is_selected(), Some(false));
    assert_eq!((n("row:a11y.open").role(), n("row:a11y.open").is_selected()), (A::ListBoxOption, Some(true)));
    assert_eq!(n("tree:group:a11y").is_expanded(), Some(true));
    // Sliders: the value as shown.
    assert_eq!(n("slider:a11y.fill").value(), Some("25"));
    assert_eq!(n("slider:a11y.size").value(), Some("1600 pixels"));
    assert_eq!(n("scrub:a11y").value(), Some("00:00:09"));
    assert_eq!((n("scrub:a11y").max_numeric_value(), n("scrub:a11y").numeric_value_step()), (Some(41_000.0), Some(33.0)));
    assert_eq!(n("volume:a11y").value(), Some("50 %"));
    assert_eq!(n("stepper:a11y.seconds/plus").value(), Some("3 seconds"));
    // Dropdowns: the choice, closed; the swatch its colour as text.
    assert_eq!((n("dropdown:a11y.sort").value(), n("dropdown:a11y.sort").is_expanded()), (Some("By date"), Some(false)));
    assert_eq!(n("dropdown:a11y.style").value(), Some("Picture pile"));
    assert_eq!((n("swatch:a11y.tint").label(), n("swatch:a11y.tint").value()), (Some("Tint"), Some("#c87828")));
    // Text fields: the value and the placeholder.
    let search = n("search:a11y");
    assert_eq!((search.value(), search.placeholder()), (Some("beach"), Some("Search")));
    // Key cells, faces, the map, the turn-and-scale handle.
    assert_eq!(n("key:a11y.none").label(), Some("No shortcut"));
    assert_eq!(n("face:a11y").label(), Some("Unnamed face"));
    assert_eq!(n("facebox:a11y/nw").label(), Some("Top-left corner"));
    let map = n("map:a11y");
    assert_eq!((map.role(), map.label(), map.value()), (A::Group, Some("Map"), Some("Zoom level 5")));
    assert_eq!(n("transform:a11y").value(), Some("Turned 15°, scaled 120 %"));
    // A toggle with no tooltip is named after its id.
    assert_eq!(n("toggle:a11y.white").label(), Some("white"));
}

#[test]
fn a_checkbox_follows_a_click() {
    let mut rig = Rig::new(None);
    let at = registry::find(&rig.ctx, "check:a11y.raw").unwrap().center();
    assert_eq!(rig.node("check:a11y.raw").toggled(), Some(Toggled::False));
    rig.click(at);
    assert!(rig.st.checked);
    assert_eq!(rig.node("check:a11y.raw").toggled(), Some(Toggled::True));
    // A screen reader's Click does the same.
    let target = registry::find(&rig.ctx, "check:a11y.raw").and_then(|w| w.egui_id).unwrap().accesskit_id();
    rig.pass(vec![Event::AccessKitActionRequest(accesskit::ActionRequest {
        action: Action::Click,
        target_node: target,
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    })]);
    rig.pass(Vec::new());
    assert_eq!(rig.node("check:a11y.raw").toggled(), Some(Toggled::False));
}

/// Whether a stroke in the focus colour is painted round `bounds`.
fn ring_round(shapes: &[ClippedShape], color: Color32, bounds: Rect) -> bool {
    fn has(shape: &Shape, color: Color32, near: Rect) -> bool {
        let stroked = match shape {
            Shape::Vec(v) => return v.iter().any(|s| has(s, color, near)),
            Shape::Rect(r) => r.stroke.color == color && r.stroke.width > 0.0,
            Shape::Circle(c) => c.stroke.color == color && c.stroke.width > 0.0,
            Shape::Path(p) => matches!(p.stroke.color, ColorMode::Solid(c) if c == color) && p.stroke.width > 0.0,
            _ => false,
        };
        stroked && near.intersects(shape.visual_bounding_rect())
    }
    shapes.iter().any(|s| has(&s.shape, color, bounds.expand(4.0)))
}

#[test]
fn tab_walks_the_gallery_in_order_with_the_focus_ring_and_space_activates() {
    let mut rig = Rig::new(None);
    let color = focus_color(&rig.t);
    assert!(rig.focused().is_none(), "nothing has the focus at first");
    let mut seen: Vec<String> = Vec::new();
    let mut no_ring = Vec::new();
    for _ in 0..120 {
        rig.key(Key::Tab);
        let Some((node, n)) = rig.focused() else { continue };
        let id = rig.automation_id(node).unwrap_or_else(|| format!("{node:?}"));
        if seen.first() == Some(&id) {
            break; // round the gallery once
        }
        let b = n.bounds().unwrap();
        let bounds = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
        if !ring_round(&rig.shapes, color, bounds) {
            no_ring.push(id.clone());
        }
        seen.push(id);
    }
    assert_eq!(seen.get(..FIRST_ROW.len()), Some(&FIRST_ROW.map(String::from)[..]), "Tab starts with the first row, left to right: {seen:?}");
    assert!(seen.len() > 40, "Tab reaches the gallery's widgets: {seen:?}");
    // The order the widgets are drawn in: the tree's children in order, depth first.
    let drawn: Vec<String> = rig.drawn().into_iter().filter_map(|n| rig.automation_id(n)).collect();
    let order: Vec<usize> = seen.iter().map(|s| drawn.iter().position(|w| w == s).unwrap_or(usize::MAX)).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "Tab follows the order the widgets are drawn in: {seen:?}\n{drawn:?}");
    // And reaches every widget that takes the focus.
    let focusable: Vec<String> = rig
        .tree
        .nodes
        .iter()
        .filter(|(_, n)| n.supports_action(Action::Focus) && !n.is_disabled())
        .filter_map(|(id, _)| rig.automation_id(*id))
        .collect();
    let missed: Vec<&String> = focusable.iter().filter(|f| !seen.contains(f)).collect();
    assert!(missed.is_empty(), "Tab never reaches {missed:?}");
    assert!(no_ring.is_empty(), "focused without a ring: {no_ring:?}");

    // Back to the first button; Space and Enter activate it.
    let mut rig = Rig::new(None);
    rig.key(Key::Tab);
    assert_eq!(rig.focused().and_then(|(n, _)| rig.automation_id(n)).as_deref(), Some(FIRST_ROW[0]));
    rig.key(Key::Space);
    assert_eq!(rig.st.clicked, [FIRST_ROW[0]]);
    rig.key(Key::Tab);
    rig.key(Key::Enter);
    assert_eq!(rig.st.clicked, [FIRST_ROW[0], FIRST_ROW[1]]);
}

#[test]
fn the_focus_ring_stands_3_to_1_on_the_surfaces_controls_sit_on() {
    let t = Tokens::default();
    let ring = focus_color(&t);
    let surfaces = [
        ("chrome", t.chrome),
        ("canvas", t.canvas),
        ("grid_bg", t.grid_bg),
        ("field", t.field),
        ("button", t.button),
        ("inset", t.inset),
        ("tray_bg", t.tray_bg),
        ("folder_header", t.folder_header),
    ];
    let low: Vec<String> =
        surfaces.iter().map(|(n, bg)| (n, contrast(ring, *bg))).filter(|(_, c)| *c < 3.0).map(|(n, c)| format!("{n}: {c:.2}:1")).collect();
    assert!(low.is_empty(), "the focus ring is below 3:1 on {low:?}");
    assert_eq!(focus_stroke(&t).width, t.focus_width);
}

#[test]
fn the_text_table_renames_what_the_widgets_say() {
    let fr = Texts::translated(|t| {
        Some(
            match t {
                Text::October => "octobre",
                Text::Monday => "lundi",
                Text::LongDate => "{weekday} {day} {month} {year}",
                Text::Mute => "Couper le son",
                Text::NoShortcut => "Aucun raccourci",
                _ => return None,
            }
            .to_owned(),
        )
    });
    let english = Rig::new(None);
    assert_eq!(english.node("field:a11y.date").description(), Some("Monday, October 19, 2026"));
    let rig = Rig::new(Some(fr));
    assert_eq!(rig.node("field:a11y.date").description(), Some("lundi 19 octobre 2026"));
    assert_eq!(rig.node("volume:a11y/mute").label(), Some("Couper le son"));
    assert_eq!(registry::find(&rig.ctx, "volume:a11y/mute").map(|w| w.label), Some("Couper le son".to_owned()));
    assert_eq!(rig.node("key:a11y.none").label(), Some("Aucun raccourci"));
    // What was not translated stays English.
    assert_eq!(rig.node("button:a11y.clearSearch").label(), Some("Clear search"));
}
