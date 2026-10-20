//! The bottom area (`ui-layout.md` §6.2–§6.5): 96 high under the status line, split at 0.365·W.
//!
//! Left of the split, the Photo Tray ([`tray`]): a sunken box showing the selection and the held
//! photos (empty: the "Selection" watermark) and its three stacked buttons Hold, Clear and Add to
//! Album ▾. Right of it, the controls row (star, rotate counterclockwise and
//! clockwise; the loupe, the thumbnail-size slider and the four side-pane toggles at the right) and,
//! under a 1 pt separator, the action buttons Export ┆ Collage, Slideshow, which go behind a
//! **More…** button that opens a vertical menu when the window is narrower than [`OVERFLOW_BELOW`].
//!
//! The star and rotate buttons act on the tray's photos (`ui-layout.md` §6.3), the same set their
//! shortcuts and menu items act on ([`crate::keys::targets`]): star in one undo step
//! (`photo.star.toggle` stars them all unless all are starred). A button whose command is not
//! registered yet is drawn disabled; it comes alive by itself when the command lands and is enabled
//! for what it acts on (`CommandSpec::enabled`, or a UI command's own check: Export, the UI command
//! `output.export`, is live while the tray holds photos and opens the Export dialog for them).
//! Collage (the engine's `create.collage`, `creating.md` §1) opens the Collage room on the tray's
//! photos, the held ones with the selection, in the main row and in More…'s menu alike (M14.20).
//!
//! The size slider drives the same view state as Ctrl+1 / Ctrl+2 (`UiState::thumb_size`), so the
//! light table's cells follow it; the pane toggles open, switch and close `UiState::pane`, the
//! state `pane.toggle` (Ctrl+0) flips.
//!
//! **In the edit view** (`editing.md` §2) the loupe and the size slider give way to the **Fit** │
//! **1:1** buttons (`button:zoom.fit`, `button:zoom.1to1`: `edit.zoom.fit`, `edit.zoom.actual`)
//! and the zoom slider (`slider:zoom`, 0 fit, ½ 100 %, 1 400 %: `edit.zoom.set`), and the star and
//! rotate buttons act on the photo shown (the rotations a step of its history). The library view's
//! frames also close the edit view's preview once it is left (`canvas::release`).

use dalica_ui_kit::Tokens;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Segment, SegmentLabel, Target};
use egui::{Id, Popup, Rect, RectAlign, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use super::Panels;
use super::edit::canvas;
use super::layout::{self, Layout};
use crate::app::App;
use crate::commands::edit_view::open_photo;
use crate::commands::zoom as zoom_maths;
use crate::state::{Pane, THUMB_MAX, THUMB_MIN, UiState};
use crate::tl;

pub mod tray;

/// Below this window width the action buttons go behind More… (`ui-layout.md` §11).
pub const OVERFLOW_BELOW: f32 = 1100.0;
/// The thumbnail-size slider's width (`ui-layout.md` §6.4); the shared zoom slider is narrower.
pub const SIZE_SLIDER_W: f32 = 128.0;
/// One side-pane toggle segment.
pub const PANE_TOGGLE: Vec2 = vec2(32.0, 24.0);
/// The loupe icon button's edge.
pub const LOUPE: f32 = 24.0;
/// The controls' inset from the split and from the window's right edge.
const PAD: f32 = 12.0;
/// Between the star and rotate plates, and between the tray's stacked buttons.
const GAP: f32 = 4.0;
/// Between the loupe, the slider and the pane toggles.
const TOOL_GAP: f32 = 8.0;
/// Each side of the dotted rule between Export and Collage.
const RULE_GAP: f32 = 8.0;
/// The dashes of that rule, and the gaps between them.
const DASH: f32 = 2.0;
/// The More… menu's narrowest width, and the gutter its icons sit in.
const MENU_MIN_W: f32 = 160.0;
const MENU_GUTTER: f32 = 28.0;

/// The tray's buttons: id, icon, tooltip.
pub const TRAY_BUTTONS: [(&str, Icon, &str); 3] = [
    ("button:tray.hold", Icon::Hold, "Keep these photos in the tray"),
    ("button:tray.clear", Icon::ClearTray, "Remove photos from the tray"),
    ("button:tray.addToAlbum", Icon::AddToAlbum, "Add these photos to an album"),
];

/// The rotate buttons: id, icon, tooltip, command.
pub const ROTATE: [(&str, Icon, &str, &str); 2] = [
    ("button:rotate.ccw", Icon::RotateCcw, "Rotate counterclockwise", "photo.rotate.ccw"),
    ("button:rotate.cw", Icon::RotateCw, "Rotate clockwise", "photo.rotate.cw"),
];

/// The side-pane toggles, left to right: id, icon, tooltip, pane.
pub const PANE_TOGGLES: [(&str, Icon, &str, Pane); 4] = [
    ("toggle:pane.people", Icon::People, "Show/hide the People pane", Pane::People),
    ("toggle:pane.places", Icon::Places, "Show/hide the Places pane", Pane::Places),
    ("toggle:pane.tags", Icon::Tags, "Show/hide the Tags pane", Pane::Tags),
    ("toggle:pane.properties", Icon::Properties, "Show/hide the Properties pane", Pane::Properties),
];

/// The action buttons, in order (a dotted rule after the first): id, icon, label, command.
pub const ACTIONS: [(&str, Icon, &str, &str); 3] = [
    ("action:export", Icon::Export, "Export", "output.export"),
    ("action:collage", Icon::Collage, "Collage", COLLAGE),
    ("action:slideshow", Icon::Slideshow, "Slideshow", "view.slideshow"),
];

/// A label or tooltip of the tables above, in the language in effect (the tables keep the English,
/// which the catalogs key on).
fn text(english: &str) -> String {
    match english {
        "Keep these photos in the tray" => tl!("Keep these photos in the tray"),
        "Remove photos from the tray" => tl!("Remove photos from the tray"),
        "Add these photos to an album" => tl!("Add these photos to an album"),
        "Rotate counterclockwise" => tl!("Rotate counterclockwise"),
        "Rotate clockwise" => tl!("Rotate clockwise"),
        "Show/hide the People pane" => tl!("Show/hide the People pane"),
        "Show/hide the Places pane" => tl!("Show/hide the Places pane"),
        "Show/hide the Tags pane" => tl!("Show/hide the Tags pane"),
        "Show/hide the Properties pane" => tl!("Show/hide the Properties pane"),
        "Export" => tl!(ctx = "action", "Export"),
        "Collage" => tl!(ctx = "action", "Collage"),
        "Slideshow" => tl!(ctx = "action", "Slideshow"),
        "Fit" => tl!(ctx = "zoom", "Fit"),
        "Fit the photo to the window" => tl!("Fit the photo to the window"),
        "Show the photo at actual size" => tl!("Show the photo at actual size"),
        other => crate::i18n::tr(other),
    }
}

/// The command of the action button Collage: the engine's Create ▸ Picture Collage… on the tray.
pub const COLLAGE: &str = "create.collage";

/// The More… button that holds the action buttons in a narrow window.
pub const MORE: &str = "action:more";

/// The edit view's Fit │ 1:1 buttons (id, label, tooltip, command) and its zoom slider.
pub const ZOOM_BUTTONS: [(&str, &str, &str, &str); 2] = [
    ("button:zoom.fit", "Fit", "Fit the photo to the window", "edit.zoom.fit"),
    ("button:zoom.1to1", "1:1", "Show the photo at actual size", "edit.zoom.actual"),
];
pub const ZOOM_SLIDER: &str = "slider:zoom";
/// One Fit │ 1:1 segment.
pub const ZOOM_BUTTON: Vec2 = vec2(36.0, 24.0);

/// The parts of the bottom area, as [`layout::compute`] places them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub tray: Rect,
    pub tray_buttons: Rect,
    pub controls: Rect,
    pub actions: Rect,
}

/// The parts of the bottom area whose rect is `area`. The layout places them from the bottom area
/// alone (its left, right and top edges), so a window rect that gives exactly this bottom area gives
/// the same parts as the whole window does.
pub fn parts(area: Rect) -> Parts {
    let above = layout::MENU_BAR + layout::TOOLBAR + layout::STATUS_LINE;
    let window = Rect::from_min_max(pos2(area.min.x, area.min.y - above), area.max);
    let l: Layout = layout::compute(window, &Default::default());
    Parts { tray: l.tray, tray_buttons: l.tray_buttons, controls: l.bottom_controls, actions: l.action_buttons }
}

/// Whether the action buttons are behind More… in a window `width` wide.
pub fn overflows(width: f32) -> bool {
    width.partial_cmp(&OVERFLOW_BELOW).is_none_or(|o| o == std::cmp::Ordering::Less)
}

/// Runs `add` in a child of `ui` whose cursor starts at `rect`'s top-left.
fn at<R>(ui: &mut Ui, salt: &str, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().id_salt(("bottom", salt)).max_rect(rect));
    add(&mut child)
}

/// Whether command `id` is registered and enabled for what a click would pass it (the photos it
/// acts on, [`crate::keys::targets`]: the tray's for star and rotate, else the selection).
fn ready(app: &App, id: &str) -> bool {
    let Some(session) = app.session() else { return false };
    // A UI command (Export opens its dialog for the tray) asks its own check.
    if crate::commands::UI_COMMANDS.iter().any(|c| c.id == id) {
        return crate::commands::enabled(app, id, &params_for(app, id)).is_ok();
    }
    let Some(spec) = session.spec(id) else { return false };
    (spec.enabled)(session, &params_for(app, id)).is_ok()
}

/// What a click passes command `id`: Collage the tray's photos (the held ones and the selection,
/// `creating.md` §1; with the tray empty the engine's refusal lands on the status line); in the
/// edit view the photo shown (`App::run` turns a rotation of it into a step of its history), else
/// the photos the command acts on.
fn params_for(app: &App, id: &str) -> serde_json::Value {
    if id == COLLAGE {
        return json!({ "ids": crate::tray::photos(app.state()) });
    }
    match open_photo(app) {
        Some(photo) if crate::keys::acts_on_tray(id) => json!({ "ids": [photo] }),
        _ => crate::keys::params_for(app, id),
    }
}

/// Runs command `id` on the photos it acts on; a refusal lands on the status line.
fn run(app: &mut App, id: &str) {
    let params = params_for(app, id);
    let _ = app.run(id, &params);
}

/// A plate button with an icon (`size`, a button plate with a 1 pt border, like the star toggle):
/// one segment of the shared segmented control with none selected. Returns whether it was clicked.
fn plate(ui: &mut Ui, t: &Tokens, id: &str, icon: Icon, tooltip: &str, size: Vec2, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| widgets::segmented(ui, t, &[Segment { id, label: SegmentLabel::Icon(icon), tooltip }], None, size).is_some())
        .inner
}

/// Paints and runs the bottom area in `ui`'s rect (the bottom region of [`super::layout`]).
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    canvas::release(app);
    let area = ui.max_rect();
    let parts = parts(area);
    tray::show(p, app, ui, parts.tray);
    tray_buttons(app, ui, &parts);
    controls(app, ui, parts.controls);
    actions(app, ui, parts.actions, area.width());
}

/// The tray's three stacked buttons: Hold while the tray has photos that are not held, Clear while
/// it has any, Add to Album ▾ while it has any (its popup: [`tray::album_popup`]).
fn tray_buttons(app: &mut App, ui: &mut Ui, p: &Parts) {
    let t = *app.tokens();
    let state = app.state();
    let count = crate::tray::count(state);
    let enabled = [count > state.tray.held.len(), count > 0, tray::add_to_album_enabled(app)];
    let size = t.metrics.tray_button;
    let mut y = p.tray_buttons.min.y;
    let mut clicked = None;
    let mut album_button = Rect::NOTHING;
    for (i, (id, icon, tooltip)) in TRAY_BUTTONS.into_iter().enumerate() {
        let rect = Rect::from_min_size(pos2(p.tray_buttons.min.x, y), size);
        let on = enabled.get(i).copied().unwrap_or(false);
        if at(ui, id, rect, |ui| plate(ui, &t, id, icon, &text(tooltip), size, on)) {
            clicked = Some(id);
        }
        if id == "button:tray.addToAlbum" {
            album_button = rect;
        }
        y += size.y + GAP;
    }
    match clicked {
        Some("button:tray.hold") => {
            let _ = app.run("tray.hold", &json!({}));
        }
        Some("button:tray.clear") => tray::clear_clicked(app),
        _ => {}
    }
    tray::album_popup(app, ui, album_button, clicked == Some("button:tray.addToAlbum"));
}

/// Whether every photo in the tray is starred (the star button's state). Cached per library
/// revision and tray contents, so a tray of 100 000 photos is not looked up each frame.
fn all_starred(app: &App, ctx: &egui::Context) -> bool {
    let (Some(session), state) = (app.session(), app.state()) else { return false };
    if let Some(photo) = open_photo(app) {
        return session.library().photo(photo).is_some_and(|p| p.star);
    }
    if crate::tray::is_empty(state) {
        return false;
    }
    let key = (session.library().revision(), fingerprint(state));
    let id = Id::new("bottom.allStarred");
    if let Some((k, on)) = ctx.data(|d| d.get_temp::<((u64, u64), bool)>(id))
        && k == key
    {
        return on;
    }
    let library = session.library();
    let starred = |id: &dalica_core::PhotoId| library.photo(*id).is_some_and(|p| p.star);
    let on = state.tray.held.iter().all(starred) && state.selection.ids.iter().all(starred);
    ctx.data_mut(|d| d.insert_temp(id, (key, on)));
    on
}

/// A cheap hash of what the tray holds (the held photos and the selection, in order).
fn fingerprint(state: &UiState) -> u64 {
    let mix = |h: u64, x: u64| (h.rotate_left(5) ^ x).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let held = state.tray.held.iter().fold(0x51_7cc1_b727_220a, |h, id| mix(h, id.get()));
    let all = state.selection.ids.iter().fold(mix(held, u64::MAX), |h, id| mix(h, id.get()));
    mix(all, state.tray.held.len() as u64 ^ (state.selection.ids.len() as u64).rotate_left(32))
}

/// The controls row: star and rotate at the left; loupe, size slider and pane toggles at the right.
fn controls(app: &mut App, ui: &mut Ui, row: Rect) {
    let t = *app.tokens();
    let star = t.metrics.star_button;
    let y = (row.center().y - star.y / 2.0).round();

    // Star and the two rotations, on the tray's photos (the photo shown in the edit view).
    let any = open_photo(app).is_some() || !crate::tray::is_empty(app.state());
    let on = all_starred(app, ui.ctx());
    let mut x = row.min.x + PAD;
    let star_clicked = at(ui, "star", Rect::from_min_size(pos2(x, y), star), |ui| {
        ui.add_enabled_ui(any, |ui| widgets::star_toggle(ui, &t, "button:star", on).clicked()).inner
    });
    if star_clicked {
        run(app, "photo.star.toggle");
    }
    for (id, icon, tooltip, command) in ROTATE {
        x += star.x + GAP;
        let enabled = any && ready(app, command);
        if at(ui, id, Rect::from_min_size(pos2(x, y), star), |ui| plate(ui, &t, id, icon, &text(tooltip), star, enabled)) {
            run(app, command);
        }
    }

    // From the right edge: the pane toggles, the size slider, the loupe.
    let toggles_w = PANE_TOGGLE.x * PANE_TOGGLES.len() as f32;
    let toggles_x = row.max.x - PAD - toggles_w;
    let open = app.state().pane;
    let active = PANE_TOGGLES.iter().position(|(_, _, _, pane)| Some(*pane) == open);
    let tips = PANE_TOGGLES.map(|(_, _, tooltip, _)| text(tooltip));
    let segments: Vec<Segment<'_>> =
        PANE_TOGGLES.iter().zip(&tips).map(|(&(id, icon, _, _), tooltip)| Segment { id, label: SegmentLabel::Icon(icon), tooltip }).collect();
    let rect = Rect::from_min_size(pos2(toggles_x, (row.center().y - PANE_TOGGLE.y / 2.0).round()), vec2(toggles_w, PANE_TOGGLE.y));
    if let Some(i) = at(ui, "panes", rect, |ui| widgets::segmented(ui, &t, &segments, active, PANE_TOGGLE))
        && let Some(&(_, _, _, pane)) = PANE_TOGGLES.get(i)
    {
        let state = app.state_mut();
        state.pane = if state.pane == Some(pane) { None } else { Some(pane) };
    }

    if open_photo(app).is_some() {
        zoom_controls(app, ui, &t, row, toggles_x);
        return;
    }

    // The shared zoom slider at this control's width.
    let mut st = t;
    st.metrics.zoom_slider.x = SIZE_SLIDER_W;
    let slider = st.metrics.zoom_slider;
    let slider_x = toggles_x - TOOL_GAP - slider.x;
    let rect = Rect::from_min_size(pos2(slider_x, (row.center().y - slider.y / 2.0).round()), slider);
    let mut size = app.state().thumb_size;
    let changed = at(ui, "thumbSize", rect, |ui| {
        widgets::zoom_slider(ui, &st, "slider:thumbSize", &mut size, THUMB_MIN..=THUMB_MAX, &tl!("Thumbnail size")).changed()
    });
    if changed && size.is_finite() {
        app.state_mut().thumb_size = size.round().clamp(THUMB_MIN, THUMB_MAX);
    }

    // The loupe: drag it onto a photo to magnify (the light table's loupe, M4).
    let rect = Rect::from_min_size(pos2(slider_x - TOOL_GAP - LOUPE, (row.center().y - LOUPE / 2.0).round()), Vec2::splat(LOUPE));
    at(ui, "loupe", rect, |ui| {
        widgets::icon_button(ui, &t, "button:loupe", Icon::Loupe, LOUPE, false, false, &tl!("Drag onto a photo to magnify it"))
    });
}

/// The edit view's zoom controls left of the pane toggles (whose left edge is `right`): Fit │ 1:1
/// and the zoom slider. The slider shows the zoom at the fitted scale the canvas last laid out.
fn zoom_controls(app: &mut App, ui: &mut Ui, t: &Tokens, row: Rect, right: f32) {
    let slider = t.metrics.zoom_slider;
    let slider_x = right - TOOL_GAP - slider.x;
    let fit = canvas::view(ui.ctx()).map_or(1.0, |v| v.fit);
    let zoom = app.state().edit.zoom;
    let mut v = zoom_maths::slider_at(zoom_maths::effective(zoom, fit), fit);
    let rect = Rect::from_min_size(pos2(slider_x, (row.center().y - slider.y / 2.0).round()), slider);
    let changed = at(ui, ZOOM_SLIDER, rect, |ui| widgets::zoom_slider(ui, t, ZOOM_SLIDER, &mut v, 0.0..=1.0, &tl!("Zoom")).changed());
    if changed && v.is_finite() {
        let _ = app.run("edit.zoom.set", &json!({ "slider": v.clamp(0.0, 1.0) }));
    }

    let texts = ZOOM_BUTTONS.map(|(_, label, tooltip, _)| (text(label), text(tooltip)));
    let segments: Vec<Segment<'_>> =
        ZOOM_BUTTONS.iter().zip(&texts).map(|(&(id, ..), (label, tooltip))| Segment { id, label: SegmentLabel::Text(label), tooltip }).collect();
    let width = ZOOM_BUTTON.x * ZOOM_BUTTONS.len() as f32;
    let rect = Rect::from_min_size(pos2(slider_x - TOOL_GAP - width, (row.center().y - ZOOM_BUTTON.y / 2.0).round()), vec2(width, ZOOM_BUTTON.y));
    let active = if !zoom_maths::is_zoomed(zoom, fit) {
        Some(0)
    } else if (zoom_maths::effective(zoom, fit) - 1.0).abs() < 1e-4 {
        Some(1)
    } else {
        None
    };
    if let Some(i) = at(ui, "zoomButtons", rect, |ui| widgets::segmented(ui, t, &segments, active, ZOOM_BUTTON))
        && let Some(&(_, _, _, command)) = ZOOM_BUTTONS.get(i)
    {
        let _ = app.run(command, &json!({}));
    }
}

/// The action-button row under its separator; behind More… when the window is narrow.
fn actions(app: &mut App, ui: &mut Ui, row: Rect, width: f32) {
    let t = *app.tokens();
    if !row.is_positive() {
        return;
    }
    let y = row.min.y + 0.5;
    ui.painter().hline((row.min.x + PAD / 2.0)..=(row.max.x - PAD / 2.0), y, Stroke::new(1.0, t.divider));
    let size = t.metrics.action_button;
    let top = (row.min.y + 1.0 + (row.height() - 1.0 - size.y) / 2.0).round();
    let enabled = ACTIONS.map(|(_, _, _, command)| ready(app, command));
    let mut x = row.min.x + PAD;
    let mut clicked = None;
    if overflows(width) {
        let open = Popup::is_id_open(ui.ctx(), Id::new(MORE).with("popup"));
        at(ui, MORE, Rect::from_min_size(pos2(x, top), size), |ui| {
            let r = widgets::action_button(ui, &t, MORE, Icon::More, &tl!("More…"), open, true).on_hover_text(tl!("More actions"));
            Popup::menu(&r).align(RectAlign::TOP_START).show(|ui| clicked = more_menu(ui, &t, &enabled));
        });
    } else {
        for (i, (id, icon, label, command)) in ACTIONS.into_iter().enumerate() {
            let on = enabled.get(i).copied().unwrap_or(false);
            if at(ui, id, Rect::from_min_size(pos2(x, top), size), |ui| widgets::action_button(ui, &t, id, icon, &text(label), false, on).clicked()) {
                clicked = Some(command);
            }
            x += size.x;
            if i == 0 {
                dotted_rule(ui, &t, x + RULE_GAP, top, size.y);
                x += 2.0 * RULE_GAP + 1.0;
            } else {
                x += GAP;
            }
        }
    }
    if let Some(command) = clicked {
        run(app, command);
    }
}

/// The ┆ between Export and Collage: a 1 pt rule of 2 pt dashes, 2 pt apart, over the middle of
/// the buttons' height.
fn dotted_rule(ui: &Ui, t: &Tokens, x: f32, top: f32, height: f32) {
    let painter = ui.painter();
    // On a pixel centre, so the rule covers one column of whole pixels.
    let x = x.round() + 0.5;
    let (from, to) = ((top + height * 0.2).round(), (top + height * 0.8).round());
    let mut y = from;
    while y + DASH <= to {
        painter.line_segment([pos2(x, y), pos2(x, y + DASH)], Stroke::new(1.0, t.text_dim));
        y += 2.0 * DASH;
    }
}

/// The rows of the More… menu: each action with its icon in the left gutter, registered under the
/// action's own id. Returns the command of the row clicked.
fn more_menu(ui: &mut Ui, t: &Tokens, enabled: &[bool; 3]) -> Option<&'static str> {
    ui.set_min_width(MENU_MIN_W);
    ui.spacing_mut().button_padding.x = MENU_GUTTER;
    let mut clicked = None;
    for (i, (id, icon, label, command)) in ACTIONS.into_iter().enumerate() {
        let on = enabled.get(i).copied().unwrap_or(false);
        let label = text(label);
        let r = ui.add_enabled(on, egui::Button::new(&label));
        let glyph = Rect::from_center_size(pos2(r.rect.min.x + MENU_GUTTER / 2.0, r.rect.center().y), Vec2::splat(t.metrics.icon_menu));
        icons::paint(ui.painter(), glyph, icon, if on { t.icon } else { t.text_disabled });
        widgets::register(ui.ctx(), id, Role::MenuItem, &label, Target::Widget(&r), false);
        if r.clicked() {
            clicked = Some(command);
            ui.close();
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;

    #[test]
    fn the_parts_are_the_layouts_whatever_the_window() {
        for (w, h) in [(1400.0, 900.0), (900.0, 600.0), (1100.0, 700.0), (3000.0, 2000.0)] {
            for state in [UiState::default(), UiState { pane: Some(Pane::Tags), tree_width: 400.0, ..UiState::default() }] {
                let l = layout::compute(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), &state);
                let p = parts(l.bottom);
                assert_eq!(
                    p,
                    Parts { tray: l.tray, tray_buttons: l.tray_buttons, controls: l.bottom_controls, actions: l.action_buttons },
                    "{w}×{h}"
                );
            }
        }
    }

    #[test]
    fn the_action_buttons_overflow_below_1100_wide() {
        assert!(overflows(900.0) && overflows(1099.5));
        assert!(!overflows(1100.0) && !overflows(1400.0));
        assert!(overflows(f32::NAN), "a width that is not a number is narrow");
    }

    #[test]
    fn hostile_areas_give_parts_that_are_never_inside_out() {
        for area in [Rect::ZERO, Rect::from_min_size(pos2(0.0, 0.0), vec2(-10.0, 96.0)), Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(10.0, 96.0))] {
            let p = parts(area);
            for r in [p.tray, p.tray_buttons, p.controls, p.actions] {
                assert!(r.width() >= 0.0 && r.height() >= 0.0 && !r.any_nan(), "{area:?}: {r:?}");
            }
        }
    }
}
