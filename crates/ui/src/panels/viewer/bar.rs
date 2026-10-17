//! The viewer's control bar and its close button (`viewing.md` §3.1), on `ui-kit`'s overlay bar:
//!
//! ```text
//!   [+][−][1:1] [Open in Dalica]      ╭───╮      [↺][↻] [★] [⋯]
//!                                     │ ▶ │
//!                                     ╰───╯
//! ```
//!
//! Zoom in, zoom out and 1:1 (`viewer.zoomIn`, `viewer.zoomOut`, `viewer.actualSize`), Open in
//! Dalica (`viewer.openInDalica`), the large round Play (a slideshow of the folder's photos the
//! library holds, from the one shown, through `view.slideshow`), rotate left and right
//! (`viewer.rotateLeft`, `viewer.rotateRight`), star (`viewer.toggleStar`, lit while starred) and ⋯
//! More: Open With ▸ and Show in File Manager (drawn disabled, [`OS_INTEGRATION`]) and Settings…
//! (`viewer.settings`). A control that cannot act is drawn disabled, its reason as its tooltip
//! (the read-only viewer's "Dalica is already open"). "close ✕" sits at the top right
//! (`viewer.close`).
//!
//! Automation ids: the bar `controls:viewer`, its buttons `button:viewer.<name>` (`zoomIn`,
//! `zoomOut`, `actualSize`, `openInDalica`, `play`, `rotateLeft`, `rotateRight`, `star`, `more`),
//! the More menu's rows `menuitem:viewer.openWith`, `menuitem:viewer.showInFileManager`,
//! `menuitem:viewer.settings`, and `button:viewer.close`.

use dalica_core::PhotoId;
use dalica_ui_kit::icons::{self, Glyph, Icon, ViewingIcon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, OVERLAY_BUTTON, OVERLAY_PAD, Target, overlay_bar, overlay_button};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Color32, CornerRadius, Id, Layout, Popup, Rect, Response, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};

use super::listing::Photo;
use crate::app::App;
use crate::commands::enabled;
use crate::state::Zoom;

/// The bar's automation id.
pub const BAR_ID: &str = "controls:viewer";
/// The close button's automation id.
pub const CLOSE_ID: &str = "button:viewer.close";
pub const MORE_ID: &str = "button:viewer.more";
pub const PLAY_ID: &str = "button:viewer.play";
/// The bar's size (points), never wider than its area.
pub const BAR_W: f32 = 640.0;
pub const BAR_H: f32 = 48.0;
/// The large round Play's diameter (points): it stands out of the bar.
pub const PLAY_D: f32 = 60.0;
/// The gap between two buttons of a group (points).
const GAP: f32 = 6.0;
/// Why the OS's own entries are greyed.
pub const OS_INTEGRATION: &str = "Arrives with OS integration";
/// Why Play is greyed for a photo outside the library.
pub const PLAY_OUTSIDE: &str = "Plays the photos of folders in your library.";
/// The close button's label.
pub const CLOSE: &str = "close";

/// The popup of the More menu.
pub fn more_popup() -> Id {
    Id::new(MORE_ID).with("popup")
}

/// What a control asks to run.
type Run = Option<(&'static str, Value)>;

/// Whether UI command `id` can run with `{}`; its reason when it cannot.
fn ready(app: &App, id: &str) -> Result<(), String> {
    enabled(app, id, &json!({}))
}

/// Draws the bar in `rect` at `opacity` for `photo` (`held`: the folder's photos the library
/// holds, what Play plays) and runs what was clicked.
pub fn show(app: &mut App, ui: &mut Ui, rect: Rect, opacity: f32, photo: &Photo, held: &[PhotoId]) {
    let t = *app.tokens();
    let zoom = app.state().viewer.map_or(Zoom::Fit, |v| v.zoom);
    let open = ready(app, "viewer.openInDalica");
    let change = ready(app, "viewer.toggleStar");
    let play_params = json!({ "ids": held, "start": photo.id });
    let play = match photo.id {
        Some(_) => enabled(app, "view.slideshow", &play_params),
        None => Err(PLAY_OUTSIDE.to_string()),
    };
    let mut run: Run = None;
    let left_w = 3.0 * OVERLAY_BUTTON + 3.0 * GAP + open_width(ui, &t);
    let right_w = 4.0 * OVERLAY_BUTTON + 3.0 * GAP;
    overlay_bar(ui, &t, BAR_ID, rect, opacity, |ui| {
        let inner = rect.shrink2(OVERLAY_PAD);
        let row = |x: f32, w: f32| Rect::from_min_size(pos2(x, inner.min.y), vec2(w, inner.height()));
        group(ui, row(inner.min.x, left_w), |ui| {
            if button(ui, &t, "button:viewer.zoomIn", Icon::Plus.into(), None, false, Ok(()), "Zoom in").clicked() {
                run = Some(("viewer.zoomIn", json!({})));
            }
            if minus_button(ui, &t, "button:viewer.zoomOut", "Zoom out").clicked() {
                run = Some(("viewer.zoomOut", json!({})));
            }
            let actual = zoom == Zoom::Scale(1.0);
            if button(ui, &t, "button:viewer.actualSize", ViewingIcon::ActualSize.into(), None, actual, Ok(()), "Actual size").clicked() {
                run = Some(("viewer.actualSize", json!({})));
            }
            let label = Some("Open in Dalica");
            if button(ui, &t, "button:viewer.openInDalica", ViewingIcon::OpenInDalica.into(), label, false, open.clone(), "Open in Dalica").clicked()
            {
                run = Some(("viewer.openInDalica", json!({})));
            }
        });
        group(ui, row(inner.max.x - right_w, right_w), |ui| {
            if button(ui, &t, "button:viewer.rotateLeft", Icon::RotateCcw.into(), None, false, change.clone(), "Rotate left").clicked() {
                run = Some(("viewer.rotateLeft", json!({})));
            }
            if button(ui, &t, "button:viewer.rotateRight", Icon::RotateCw.into(), None, false, change.clone(), "Rotate right").clicked() {
                run = Some(("viewer.rotateRight", json!({})));
            }
            let glyph = if photo.star { Icon::Star } else { Icon::StarOutline };
            if button(ui, &t, "button:viewer.star", glyph.into(), None, photo.star, change.clone(), "Star").clicked() {
                run = Some(("viewer.toggleStar", json!({})));
            }
            let more_open = Popup::is_id_open(ui.ctx(), more_popup());
            let r = button(ui, &t, MORE_ID, Icon::More.into(), None, more_open, Ok(()), "More");
            Popup::menu(&r).id(more_popup()).show(|ui| {
                if let Some(c) = more_menu(ui, &t) {
                    run = Some((c, json!({})));
                }
            });
        });
        if play_button(ui, &t, rect.center(), &play).clicked() {
            run = Some(("view.slideshow", play_params.clone()));
        }
    });
    if let Some((id, params)) = run {
        let _ = app.run(id, &params);
    }
}

/// The width of the Open in Dalica button (its glyph and label).
fn open_width(ui: &Ui, t: &Tokens) -> f32 {
    let g = ui.painter().layout_no_wrap("Open in Dalica".to_owned(), fonts::regular(t.body), t.overlay_text);
    (10.0 + widgets::OVERLAY_GLYPH + 6.0 + g.size().x + 12.0).ceil()
}

/// A row of buttons at `rect`, left to right.
fn group(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        add(ui);
    });
}

/// An overlay button that may be disabled: `Err(reason)` draws it disabled with the reason as its
/// tooltip.
#[allow(clippy::too_many_arguments)]
fn button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, label: Option<&str>, active: bool, ready: Result<(), String>, tip: &str) -> Response {
    match ready {
        Ok(()) => overlay_button(ui, t, id, glyph, label, active, true, tip),
        Err(why) => overlay_button(ui, t, id, glyph, label, false, false, &why),
    }
}

/// Zoom out: an overlay button whose glyph is a minus sign (the overlay button's look, `ui-kit`
/// has no minus glyph).
fn minus_button(ui: &mut Ui, t: &Tokens, id: &str, tip: &str) -> Response {
    let (_, rect) = ui.allocate_space(Vec2::splat(OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let p = ui.painter();
    if r.is_pointer_button_down_on() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_pressed);
    } else if r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_hover);
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if r.hovered() { t.overlay_text } else { t.overlay_icon };
    let half = widgets::OVERLAY_GLYPH * 0.35;
    let c = rect.center();
    p.line_segment([pos2(c.x - half, c.y), pos2(c.x + half, c.y)], Stroke::new(1.6, color));
    widgets::register(ui.ctx(), id, Role::IconButton, tip, Target::Widget(&r), false);
    widgets::tooltip(r, t, tip, None)
}

/// The large round Play centred at `centre`, standing out of the bar.
fn play_button(ui: &mut Ui, t: &Tokens, centre: egui::Pos2, ready: &Result<(), String>) -> Response {
    let rect = Rect::from_center_size(centre, Vec2::splat(PLAY_D));
    let on = ready.is_ok();
    let r = ui.interact(rect, Id::new(PLAY_ID), if on { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let ring = if !on {
        t.overlay_disabled
    } else if r.hovered() {
        t.overlay_text
    } else {
        t.overlay_icon
    };
    let fill = if on && r.is_pointer_button_down_on() { t.overlay_pressed } else { Color32::from_black_alpha(200) };
    p.circle(centre, PLAY_D / 2.0, fill, Stroke::new(2.0, ring));
    let s = PLAY_D * 0.2;
    let tri = vec![pos2(centre.x - s * 0.7, centre.y - s), pos2(centre.x + s, centre.y), pos2(centre.x - s * 0.7, centre.y + s)];
    p.add(Shape::convex_polygon(tri, ring, Stroke::NONE));
    let tip = match ready {
        Ok(()) => "Play a slideshow of the folder",
        Err(why) => why.as_str(),
    };
    widgets::register(ui.ctx(), PLAY_ID, Role::IconButton, tip, Target::Widget(&r), false);
    widgets::tooltip(r, t, tip, None)
}

/// The More menu's rows; returns the command of the row clicked.
fn more_menu(ui: &mut Ui, t: &Tokens) -> Option<&'static str> {
    ui.set_min_width(200.0);
    let rows: [(&str, &str, Option<&'static str>); 3] = [
        ("menuitem:viewer.openWith", "Open With", None),
        ("menuitem:viewer.showInFileManager", "Show in File Manager", None),
        ("menuitem:viewer.settings", "Settings…", Some("viewer.settings")),
    ];
    let mut clicked = None;
    for (id, label, command) in rows {
        if command.is_some() {
            ui.separator();
        }
        let r = ui.add_enabled(command.is_some(), egui::Button::new(label).min_size(vec2(ui.available_width(), 0.0)));
        if id == "menuitem:viewer.openWith" {
            // A submenu: its chevron at the right.
            let glyph = Rect::from_center_size(pos2(r.rect.max.x - 10.0, r.rect.center().y), Vec2::splat(12.0));
            icons::paint(ui.painter(), glyph, Icon::ChevronRight, t.text_disabled);
        }
        widgets::register(ui.ctx(), id, Role::MenuItem, label, Target::Widget(&r), false);
        let r = if command.is_none() { widgets::tooltip(r, t, OS_INTEGRATION, None) } else { r };
        if r.clicked() {
            clicked = command;
            ui.close();
        }
    }
    clicked
}

/// "close ✕" at the top right of `area`; returns whether it was clicked.
pub fn close_button(ui: &mut Ui, t: &Tokens, area: Rect, opacity: f32) -> bool {
    if opacity <= 0.0 {
        return false;
    }
    let font = fonts::regular(t.body);
    let g = ui.painter().layout_no_wrap(CLOSE.to_owned(), font, t.overlay_text);
    let w = (10.0 + g.size().x + 6.0 + widgets::OVERLAY_GLYPH + 10.0).ceil();
    let rect = Rect::from_min_size(pos2(area.max.x - w - 12.0, area.min.y + 12.0), vec2(w, OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(CLOSE_ID), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(t.radius), if r.hovered() { t.overlay_hover } else { t.overlay_bar }.gamma_multiply(opacity));
    let color = if r.hovered() { t.overlay_text } else { t.overlay_icon }.gamma_multiply(opacity);
    p.galley(pos2(rect.min.x + 10.0, rect.center().y - g.size().y / 2.0).round(), g, color);
    let glyph = Rect::from_center_size(pos2(rect.max.x - 10.0 - widgets::OVERLAY_GLYPH / 2.0, rect.center().y), Vec2::splat(widgets::OVERLAY_GLYPH));
    icons::paint_glyph(&p, glyph, Icon::Close.into(), color);
    widgets::register(ui.ctx(), CLOSE_ID, Role::Button, "Close", Target::Widget(&r), false);
    widgets::tooltip(r, t, "Close (Esc)", None).clicked()
}
