//! The toolbar (`ui-layout.md` §3): 40 high under the menu bar. Left to right: **Import**, **New
//! Album**, the **Flat | Tree** pair and the **▾** view options popup over the library tree; then,
//! in the light table's column (so they move right as the tree widens), the **Filters** group with
//! its caption, the **age slider**, the **search field** and the **activity indicator**.
//!
//! [`compute`] is the geometry, one pure function of the toolbar rect and the tree width, so the
//! panel and the tests use the same numbers. Every control registers the id of
//! `control-protocol.md` §7. A control whose command is not registered yet is drawn disabled (its
//! tooltip says so) and goes live by itself once the command exists: Import (`import.open`), New
//! Album (`album.new`), Flat | Tree (`view.folderView`) and the rows of the view options popup
//! ([`VIEW_OPTIONS_ROWS`]). The filters, the age slider and the search field have no command until
//! search arrives (`organizing.md` §8), so they stay disabled until then.
//!
//! The activity indicator spins while the library is busy (scanning) and draws nothing at rest. Its
//! phase comes from egui's input time, which the headless driver's virtual clock sets, so renders
//! stay deterministic. A click toggles the activity popover ([`super::activity`]: the jobs with
//! their progress and Cancel); while it is open the indicator sits on a `pressed` plate. It takes
//! clicks while it shows something (a scan running, or the popover open); at rest it is inert as
//! well as blank, and its tooltip says "No background work".

use std::time::Duration;

use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Segment, SegmentLabel, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align, Align2, Context, CornerRadius, Id, Layout, Popup, Rect, Response, Sense, SetOpenCommand, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2,
    vec2,
};
use serde_json::{Value, json};

use super::layout;
use crate::app::App;
use crate::menus::{self, UI_COMMANDS};

/// Automation ids (`control-protocol.md` §7).
pub const IMPORT_ID: &str = "button:toolbar.import";
pub const NEW_ALBUM_ID: &str = "button:toolbar.newAlbum";
pub const FLAT_ID: &str = "button:tree.flat";
pub const TREE_ID: &str = "button:tree.tree";
pub const VIEW_OPTIONS_ID: &str = "button:toolbar.viewOptions";
pub const FILTER_IDS: [&str; 4] = ["button:filter.starred", "button:filter.faces", "button:filter.videos", "button:filter.geotagged"];
pub const AGE_ID: &str = "slider:age";
pub const SEARCH_ID: &str = "field:search";
pub const SEARCH_CLEAR_ID: &str = "button:search.clear";
pub const ACTIVITY_ID: &str = "indicator:activity";

/// Sizes of `ui-layout.md` §3 (points).
pub const IMPORT: Vec2 = vec2(110.0, 28.0);
pub const NEW_ALBUM: Vec2 = vec2(30.0, 28.0);
/// One segment of the Flat | Tree pair.
pub const SEGMENT: Vec2 = vec2(30.0, 28.0);
pub const VIEW_OPTIONS: Vec2 = vec2(22.0, 28.0);
/// A filter toggle's edge; the four are joined.
pub const FILTER: f32 = 26.0;
/// The age slider: 100 wide, as high as the thin slider.
pub const AGE: Vec2 = vec2(100.0, 20.0);
/// The search field is 28 high, starts at 0.4·W (or further right, to leave the filters room in
/// the light table's column), ends at W − 64 and never gets narrower than 160
/// (`design-system.md` §11.1).
pub const SEARCH_HEIGHT: f32 = 28.0;
pub const SEARCH_START: f32 = 0.4;
pub const SEARCH_END_INSET: f32 = 64.0;
pub const SEARCH_MIN: f32 = 160.0;
pub const ACTIVITY: f32 = 28.0;
/// Below this window width the filter caption hides (`ui-layout.md` §11).
pub const CAPTION_MIN_WINDOW: f32 = 1100.0;
/// The caption's height above the filter toggles.
pub const CAPTION_HEIGHT: f32 = 13.0;

/// Gaps: the toolbar's left and right margin, between the left-hand buttons, between the pair and
/// ▾, between the filters and the age slider, and around the filter group.
const MARGIN: f32 = 8.0;
const GAP: f32 = 6.0;
const PAIR_GAP: f32 = 4.0;
const AGE_GAP: f32 = 8.0;
const GROUP_GAP: f32 = 12.0;
/// The spinner turns once a second, in eight steps.
const SPIN_TURN_SECONDS: f64 = 1.0;
const SPIN_STEPS: f64 = 8.0;

/// Where every toolbar control is, in window points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolbarLayout {
    pub import: Rect,
    pub new_album: Rect,
    pub flat: Rect,
    pub tree: Rect,
    pub view_options: Rect,
    /// Starred, Faces, Videos, Geotagged.
    pub filters: [Rect; 4],
    pub age: Rect,
    /// The "Filters" caption above the toggles; `None` below [`CAPTION_MIN_WINDOW`].
    pub caption: Option<Rect>,
    pub search: Rect,
    pub activity: Rect,
}

/// A `size` rect with its left edge at `x`, centred on `cy`.
fn at(x: f32, cy: f32, size: Vec2) -> Rect {
    Rect::from_min_size(pos2(x, (cy - size.y / 2.0).round()), size)
}

/// Lays out the toolbar in `toolbar` (the toolbar region: as wide as the window) for a library tree
/// `tree_width` wide as the user set it (clamped as [`layout::tree_width`] does).
pub fn compute(toolbar: Rect, tree_width: f32) -> ToolbarLayout {
    let toolbar = if toolbar.is_finite() { toolbar } else { Rect::ZERO };
    let (left, right, top) = (toolbar.min.x, toolbar.max.x.max(toolbar.min.x), toolbar.min.y);
    let width = right - left;
    // Centred in the 39 pt above the bottom rule.
    let cy = top + ((toolbar.height() - 1.0).max(0.0) / 2.0).round();

    let import = at(left + MARGIN, cy, IMPORT);
    let new_album = at(import.max.x + GAP, cy, NEW_ALBUM);
    let flat = at(new_album.max.x + GAP, cy, SEGMENT);
    let tree = at(flat.max.x, cy, SEGMENT);
    let view_options = at(tree.max.x + PAIR_GAP, cy, VIEW_OPTIONS);

    // The right-hand group lives in the light table's column, right of the splitter.
    let column = left + layout::tree_width(tree_width, width) + layout::SPLITTER;
    let group = 4.0 * FILTER + AGE_GAP + AGE.x;
    let search_right = (right - SEARCH_END_INSET).max(left);
    let group_left_min = (column + MARGIN).max(view_options.max.x + GROUP_GAP);
    let search_left = (left + SEARCH_START * width).max(group_left_min + group + GROUP_GAP).min(search_right - SEARCH_MIN).max(left).round();
    let search = at(search_left, cy, vec2((search_right - search_left).max(0.0), SEARCH_HEIGHT));
    let activity = at((right - MARGIN - ACTIVITY).max(left), cy, Vec2::splat(ACTIVITY));

    let show_caption = width >= CAPTION_MIN_WINDOW;
    let group_left = search_left - GROUP_GAP - group;
    // With the caption the toggles sit under it, on the bottom rule; without it, centred.
    let filter_cy = if show_caption { top + CAPTION_HEIGHT + FILTER / 2.0 } else { cy };
    let filters = std::array::from_fn(|i| at(group_left + i as f32 * FILTER, filter_cy, Vec2::splat(FILTER)));
    let age = at(group_left + 4.0 * FILTER + AGE_GAP, filter_cy, AGE);
    let caption = show_caption.then(|| Rect::from_min_size(pos2(group_left, top), vec2(group, CAPTION_HEIGHT)));

    ToolbarLayout { import, new_album, flat, tree, view_options, filters, age, caption, search, activity }
}

/// One row of the ▾ view options popup (`ui-layout.md` §3): a label and the command it runs with
/// fixed parameters (`part` names the row's id variant, `menuitem:<command>/<part>`), or a
/// separator. Rows whose command is not registered are drawn disabled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewOption {
    Row { label: &'static str, command: &'static str, part: Option<&'static str>, params: &'static str, checked: bool },
    Separator,
}

const fn row(label: &'static str, command: &'static str, part: &'static str, params: &'static str) -> ViewOption {
    ViewOption::Row { label, command, part: Some(part), params, checked: false }
}

/// The view options popup. A row's check mark reads the view state (`view::checked`); `checked`
/// is only the fallback for a row whose command does not answer yet (the people sorts and tree
/// thumbnails get theirs later).
pub const VIEW_OPTIONS_ROWS: &[ViewOption] = &[
    ViewOption::Row { label: "Sort by Date", command: "view.folderSort", part: Some("date"), params: r#"{"by":"date"}"#, checked: true },
    row("Sort by Recent Changes", "view.folderSort", "recent", r#"{"by":"recent"}"#),
    row("Sort by Size", "view.folderSort", "size", r#"{"by":"size"}"#),
    row("Sort by Name", "view.folderSort", "name", r#"{"by":"name"}"#),
    row("Reverse Order", "view.folderSort", "reverse", r#"{"reverse":true}"#),
    ViewOption::Separator,
    row("Sort People by Name", "view.peopleSort", "name", r#"{"by":"name"}"#),
    row("Sort People by Count", "view.peopleSort", "count", r#"{"by":"count"}"#),
    ViewOption::Separator,
    ViewOption::Row { label: "Show Thumbnails in Tree", command: "view.treeThumbnails", part: None, params: "", checked: false },
    ViewOption::Row { label: "Simplified Tree", command: "view.simplifiedTree", part: None, params: "", checked: false },
];

impl ViewOption {
    /// The row's automation id while the popup is open; `None` for a separator.
    pub fn widget_id(&self) -> Option<String> {
        match self {
            ViewOption::Row { command, part: Some(part), .. } => Some(format!("menuitem:{command}/{part}")),
            ViewOption::Row { command, part: None, .. } => Some(format!("menuitem:{command}")),
            ViewOption::Separator => None,
        }
    }
}

/// Whether command `id` is registered: a UI command, or one of the open library's.
fn available(app: &App, id: &str) -> bool {
    UI_COMMANDS.iter().any(|c| c.id == id) || app.session().is_some_and(|s| s.spec(id).is_some())
}

/// What the toolbar keeps between frames that is not view state yet: the search text and the age
/// slider's position (both become view state with search, `organizing.md` §8).
#[derive(Debug, Clone, Default)]
struct Memory {
    search: String,
    age: f32,
}

fn memory_id() -> Id {
    Id::new("dalica-toolbar-memory")
}

/// The search field's text.
pub fn search_text(ctx: &Context) -> String {
    ctx.data(|d| d.get_temp::<Memory>(memory_id())).unwrap_or_default().search
}

/// Puts `text` in the search field (for scripts and tests while the field has no command).
pub fn set_search_text(ctx: &Context, text: &str) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Memory>(memory_id()).search = text.to_owned());
}

const NOT_YET: &str = "Not available yet.";

/// A tooltip that also shows while `r` is disabled, saying why.
fn disabled_tooltip(r: Response, t: &Tokens, title: &str) -> Response {
    if r.enabled() { r } else { widgets::tooltip(r, t, title, Some(NOT_YET)) }
}

/// Runs `add` in a child ui that fills `rect`, enabled or not, with no spacing between widgets.
fn place<R>(ui: &mut Ui, rect: Rect, enabled: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing = Vec2::ZERO;
    if !enabled {
        child.disable();
    }
    add(&mut child)
}

/// Paints the toolbar into `ui`'s rect (the toolbar region of [`super::layout`]).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    let rule = Rect::from_min_max(pos2(rect.min.x, rect.max.y - 1.0), rect.max);
    ui.painter().rect_filled(rule, 0, t.divider);
    // The tree as the layout narrows it (beside a side pane), so the filters follow the splitter.
    let l = compute(rect, layout::effective_tree_width(app.state(), rect.width()));
    let mut memory = ui.ctx().data(|d| d.get_temp::<Memory>(memory_id())).unwrap_or_default();

    // Import and New Album.
    let r = place(ui, l.import, available(app, "import.open"), |ui| import_button(ui, &t));
    if r.clicked() {
        let _ = app.run("import.open", &json!({}));
    }
    if single(ui, &t, l.new_album, available(app, "album.new"), NEW_ALBUM_ID, Icon::NewAlbum, "Create a new album", false) {
        let _ = app.run("album.new", &json!({}));
    }

    // Flat | Tree, the selected segment as the view state says.
    let pair = [
        Segment { id: FLAT_ID, label: SegmentLabel::Icon(Icon::FlatList), tooltip: "Show folders as a flat list" },
        Segment { id: TREE_ID, label: SegmentLabel::Icon(Icon::Tree), tooltip: "Show the folder tree" },
    ];
    let active = usize::from(menus::checked(app, "view.folderView", &json!({ "mode": "tree" })));
    let clicked = place(ui, l.flat.union(l.tree), available(app, "view.folderView"), |ui| widgets::segmented(ui, &t, &pair, Some(active), SEGMENT));
    segment_tooltips(ui, &t, &pair);
    if let Some(i) = clicked {
        let mode = if i == 0 { "flat" } else { "tree" };
        let _ = app.run("view.folderView", &json!({ "mode": mode }));
    }

    view_options(app, ui, &t, l.view_options);

    // The filters (no command until search), with the caption the hovered one's tooltip replaces.
    let filters = [
        (Icon::Star, "Show starred photos only"),
        (Icon::Faces, "Show only photos with faces"),
        (Icon::Videos, "Show videos only"),
        (Icon::Geotagged, "Show only geotagged photos"),
    ];
    let mut hovered: Option<&str> = None;
    for ((id, (icon, tip)), rect) in FILTER_IDS.iter().zip(filters).zip(l.filters) {
        let r = place(ui, rect, false, |ui| widgets::toggle_filter(ui, &t, id, icon, false, tip));
        if r.contains_pointer() {
            hovered = Some(tip);
        }
        disabled_tooltip(r, &t, tip);
    }
    let age_tip = "Show only recent photos";
    // The age slider is the thin slider at 100 wide (§3) rather than the size slider's 110.
    let mut age_tokens = t;
    age_tokens.metrics.zoom_slider = AGE;
    let r = place(ui, l.age, false, |ui| widgets::zoom_slider(ui, &age_tokens, AGE_ID, &mut memory.age, 0.0..=1.0, age_tip));
    if r.contains_pointer() {
        hovered = Some(age_tip);
    }
    disabled_tooltip(r, &t, age_tip);
    if let Some(caption) = l.caption {
        let text = hovered.unwrap_or("Filters");
        ui.painter().with_clip_rect(rect).text(caption.center(), Align2::CENTER_CENTER, text, fonts::regular(t.small), t.text_dim);
    }

    // The search field: live search comes with search; until then it is disabled.
    let out =
        place(ui, l.search, false, |ui| widgets::search_field(ui, &t, SEARCH_ID, SEARCH_CLEAR_ID, &mut memory.search, "Search", l.search.width()));
    disabled_tooltip(out.response, &t, "Search");

    activity(app, ui, &t, l.activity);
    ui.ctx().data_mut(|d| d.insert_temp(memory_id(), memory));
}

/// A one-segment button (New Album, ▾): a segment plate of `rect`'s size with an icon, `open`
/// showing it pressed. Returns whether it was clicked.
#[allow(clippy::too_many_arguments)] // a rect, its state and its labels; a struct would not read better
fn single(ui: &mut Ui, t: &Tokens, rect: Rect, enabled: bool, id: &'static str, icon: Icon, tooltip: &'static str, open: bool) -> bool {
    let seg = [Segment { id, label: SegmentLabel::Icon(icon), tooltip }];
    let clicked = place(ui, rect, enabled, |ui| widgets::segmented(ui, t, &seg, open.then_some(0), rect.size()));
    segment_tooltips(ui, t, &seg);
    clicked.is_some()
}

/// Disabled segments still say what they do, and why they are disabled (`design-system.md` §6).
fn segment_tooltips(ui: &Ui, t: &Tokens, segments: &[Segment<'_>]) {
    for s in segments {
        if let Some(r) = ui.ctx().read_response(Id::new(s.id)) {
            disabled_tooltip(r, t, s.tooltip);
        }
    }
}

/// The Import button: a camera glyph and "Import" on a button plate, 110 × 28. ui-kit has no
/// icon-and-label button, so it is drawn here from the tokens with ui-kit's focus ring and
/// registration, in the look of `text_button` and the segments.
fn import_button(ui: &mut Ui, t: &Tokens) -> Response {
    let enabled = ui.is_enabled();
    let (_, rect) = ui.allocate_space(IMPORT);
    let r = ui.interact(rect, Id::new(IMPORT_ID), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if enabled && r.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let (glyph, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if r.hovered() {
        (t.text, t.text)
    } else {
        (t.icon, t.text)
    };
    let label = "Import";
    let galley = p.layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), text);
    let icon = t.metrics.icon_toolbar;
    let content = icon + GAP + galley.size().x;
    let x = (rect.center().x - content / 2.0).round();
    icons::paint(p, Rect::from_min_size(pos2(x, (rect.center().y - icon / 2.0).round()), Vec2::splat(icon)), Icon::Import, glyph);
    p.galley(pos2(x + icon + GAP, (rect.center().y - galley.size().y / 2.0).round()), galley, text);
    widgets::register(ui.ctx(), IMPORT_ID, Role::Button, label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let tip = "Get photos from a camera, card or folder";
    if enabled { r.on_hover_text(tip) } else { disabled_tooltip(r, t, tip) }
}

/// The ▾ button and its popup of [`VIEW_OPTIONS_ROWS`], bounded by the window like every menu. The
/// button always opens the popup; rows whose command is not registered are disabled. A clicked row
/// runs its command.
fn view_options(app: &mut App, ui: &mut Ui, t: &Tokens, rect: Rect) {
    let popup_id = widgets::dropdown_popup_id(VIEW_OPTIONS_ID);
    let open = Popup::is_id_open(ui.ctx(), popup_id);
    let toggled = single(ui, t, rect, true, VIEW_OPTIONS_ID, Icon::ChevronDown, "View options", open);
    let Some(button) = ui.ctx().read_response(Id::new(VIEW_OPTIONS_ID)) else { return };
    // Whether each row's command is registered, and whether the view state checks it.
    let live: Vec<(bool, bool)> = VIEW_OPTIONS_ROWS
        .iter()
        .map(|o| match o {
            ViewOption::Row { command, params, checked, .. } => {
                let params = serde_json::from_str(params).unwrap_or(Value::Null);
                let on = crate::commands::view::checked(app.state(), command, &params).unwrap_or(*checked);
                (available(app, command), on)
            }
            ViewOption::Separator => (false, false),
        })
        .collect();
    let top = rect.max.y;
    let mut clicked: Option<(&'static str, Value)> = None;
    Popup::menu(&button).id(popup_id).open_memory(toggled.then_some(SetOpenCommand::Toggle)).show(|ui| {
        menus::level(ui, 1, top, |ui| {
            ui.set_min_width(220.0);
            ui.spacing_mut().button_padding.x = 24.0;
            for (o, &(live, checked)) in VIEW_OPTIONS_ROWS.iter().zip(&live) {
                let ViewOption::Row { label, command, params, .. } = *o else {
                    ui.separator();
                    continue;
                };
                let r = ui.add_enabled(live, egui::Button::new(label));
                if checked {
                    let color = if live { t.text } else { t.text_disabled };
                    let c = pos2(r.rect.left() + 12.0, r.rect.center().y);
                    let pts = vec![pos2(c.x - 4.0, c.y), pos2(c.x - 1.0, c.y + 3.5), pos2(c.x + 4.5, c.y - 4.0)];
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.6, color)));
                }
                if let Some(id) = o.widget_id() {
                    widgets::register(ui.ctx(), &id, Role::MenuItem, label, Target::Widget(&r), checked);
                }
                if r.clicked() {
                    clicked = Some((command, serde_json::from_str(params).unwrap_or_else(|_| json!({}))));
                    ui.close();
                }
            }
        });
    });
    if let Some((command, params)) = clicked {
        let _ = app.run(command, &params);
    }
}

/// The activity indicator: the spinner while the library is busy, nothing at rest; a click
/// toggles the activity popover under it, drawn here (`ui-layout.md` §3 item 8).
fn activity(app: &mut App, ui: &mut Ui, t: &Tokens, rect: Rect) {
    let busy = app.session().is_some_and(dalica_engine::Session::busy);
    // Clickable while it shows something: the spinner, or the popover open.
    let live = busy || app.state().activity;
    let r = ui.interact(rect, Id::new(ACTIVITY_ID), if live { Sense::click() } else { Sense::hover() });
    if r.clicked() {
        let open = !app.state().activity;
        app.state_mut().activity = open;
    }
    let open = app.state().activity;
    let label = if busy { "Working in the background" } else { "No background work" };
    if open {
        ui.painter().rect_filled(rect, CornerRadius::same(t.radius), t.pressed);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    if busy {
        let time = ui.input(|i| i.time);
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        let phase = (time.rem_euclid(SPIN_TURN_SECONDS) / SPIN_TURN_SECONDS) as f32;
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(t.metrics.icon_toolbar));
        icons::paint_activity(ui.painter(), glyph, t.icon, phase);
        // Wake up for the spinner's next step.
        let step = SPIN_TURN_SECONDS / SPIN_STEPS;
        let wait = (step - time.rem_euclid(step)).clamp(0.001, step);
        ui.ctx().request_repaint_after(Duration::from_secs_f64(wait));
    }
    widgets::register(ui.ctx(), ACTIVITY_ID, Role::Toggle, label, Target::Widget(&r), open);
    widgets::tooltip(r, t, label, None);
    super::activity::show(app, ui, rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolbar(w: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 24.0), vec2(w, 40.0))
    }

    #[test]
    fn the_controls_have_the_sizes_of_the_spec_and_do_not_overlap() {
        for w in [900.0, 1100.0, 1400.0, 2560.0] {
            for tree in [200.0, 240.0, 420.0, 5000.0] {
                let l = compute(toolbar(w), tree);
                assert_eq!(l.import.size(), IMPORT);
                assert_eq!((l.new_album.size(), l.flat.size(), l.tree.size(), l.view_options.size()), (NEW_ALBUM, SEGMENT, SEGMENT, VIEW_OPTIONS));
                assert_eq!(l.flat.max.x, l.tree.min.x, "the pair is joined");
                for pair in l.filters.windows(2) {
                    assert_eq!(pair[0].max.x, pair[1].min.x, "the filters are joined");
                }
                assert!(l.filters.iter().all(|f| f.size() == Vec2::splat(FILTER)));
                assert_eq!(l.age.size(), AGE);
                assert_eq!((l.search.height(), l.search.max.x), (SEARCH_HEIGHT, w - SEARCH_END_INSET));
                assert!(l.search.min.x >= SEARCH_START * w && l.search.width() >= SEARCH_MIN, "{w} {tree}: {:?}", l.search);
                assert_eq!(l.activity.size(), Vec2::splat(ACTIVITY));
                // Left to right, never overlapping, inside the toolbar.
                let row = [l.import, l.new_album, l.flat, l.tree, l.view_options, l.filters[0], l.filters[3], l.age, l.search, l.activity];
                for pair in row.windows(2) {
                    assert!(pair[0].max.x <= pair[1].min.x, "{w} {tree}: {:?} then {:?}", pair[0], pair[1]);
                }
                for r in row {
                    assert!(toolbar(w).contains_rect(r), "{w} {tree}: {r:?}");
                }
                // The filters are in the light table's column.
                assert!(l.filters[0].min.x >= layout::tree_width(tree, w) + layout::SPLITTER, "{w} {tree}");
            }
        }
    }

    #[test]
    fn the_caption_shows_from_1100_wide_and_the_filters_move_with_the_tree() {
        assert!(compute(toolbar(1099.0), 240.0).caption.is_none());
        let wide = compute(toolbar(1100.0), 240.0);
        let caption = wide.caption.unwrap();
        assert_eq!(caption.max.y, wide.filters[0].min.y, "the caption sits on the toggles");
        assert_eq!((caption.min.x, caption.max.x), (wide.filters[0].min.x, wide.age.max.x));
        // At 900 the filters follow the tree as it widens; at 1400 the search starts at 0.4·W.
        let (narrow, wider) = (compute(toolbar(900.0), 240.0), compute(toolbar(900.0), 400.0));
        assert!(wider.filters[0].min.x > narrow.filters[0].min.x && wider.search.min.x > narrow.search.min.x);
        assert_eq!(compute(toolbar(1400.0), 240.0).search.min.x, 560.0);
    }

    #[test]
    fn hostile_rects_give_finite_layouts() {
        for (r, tree) in
            [(Rect::ZERO, 240.0), (toolbar(f32::NAN), 240.0), (toolbar(10.0), f32::NAN), (toolbar(-50.0), f32::INFINITY), (toolbar(1e9), 1e9)]
        {
            let l = compute(r, tree);
            for rect in [l.import, l.search, l.age, l.activity, l.filters[0], l.view_options] {
                assert!(rect.is_finite() && rect.width() >= 0.0 && rect.height() >= 0.0, "{r:?}: {rect:?}");
            }
        }
    }

    #[test]
    fn every_view_option_row_has_a_menu_item_id_and_valid_params() {
        let ids: Vec<String> = VIEW_OPTIONS_ROWS.iter().filter_map(ViewOption::widget_id).collect();
        assert_eq!(ids.len(), 9);
        assert!(ids.iter().all(|id| dalica_ui_kit::registry::is_valid_id(id)), "{ids:?}");
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
        for o in VIEW_OPTIONS_ROWS {
            if let ViewOption::Row { params, .. } = o {
                assert!(params.is_empty() || serde_json::from_str::<Value>(params).is_ok(), "{params}");
            }
        }
    }
}
