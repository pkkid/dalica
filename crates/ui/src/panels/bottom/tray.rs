//! The Photo Tray's box (`ui-layout.md` §6.2), drawn from [`crate::tray`]: the "Selection"
//! watermark when empty; a section selected as a unit as one token (its cover and the pill
//! "Folder selected — 48 photos"); every other photo as a square, centre-cropped mini thumbnail
//! from the light table's thumbnails ([`Thumbnails::paint`]), held photos first with the held
//! badge, shrinking as their count grows; what does not fit counted on the last place ("+12").
//!
//! Input: a click on a mini thumbnail tray-selects it (Ctrl toggles; a double-click will open it in
//! the edit view), a click on the box's empty space tray-selects nothing, a rubber band dragged from it tray-selects the minis it touches
//! (Ctrl adds), and a right-click opens the tray menu (a mini not tray-selected is selected
//! first). Every change goes through the `tray.*` commands. The Clear button asks
//! "Clear all photos from the tray?" before it empties a tray that holds held photos.

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_engine::Context;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, DialogButton, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, CornerRadius, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2};
use serde_json::{Value, json};

use super::super::Panels;
use super::super::thumbnails::PhotoRef;
use crate::app::App;
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node};
use crate::tray;

/// The automation ids of the clear-tray confirmation and its buttons (`control-protocol.md` §7).
pub const DIALOG_ID: &str = "dialog:clearTray";
pub const DIALOG_CLEAR: &str = "button:dialog.clearTray";
pub const DIALOG_CANCEL: &str = "button:dialog.cancel";

/// The width of the confirmation.
const DIALOG_W: f32 = 360.0;
/// The pill's padding either side of its text.
const PILL_PAD: f32 = 8.0;

/// egui memory: the confirmation is open; a rubber band started at this point (and the tray
/// selection it adds to).
fn confirm_id() -> Id {
    Id::new("tray.confirmClear")
}

fn band_id() -> Id {
    Id::new("tray.band")
}

/// A command to run once the frame's borrows end.
type Action = (&'static str, Value);

/// Paints and runs the tray box `rect`.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui, rect: Rect) {
    tray::prune(app.state_mut());
    if !rect.is_positive() {
        return;
    }
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    painter.rect(rect, CornerRadius::same(t.radius), t.tray_bg, Stroke::new(1.0, t.divider), StrokeKind::Inside);
    // The box's own interaction, under the minis so they win their clicks.
    let bg = ui.interact(rect, Id::new("tray.box"), Sense::click_and_drag());
    widgets::register(&ctx, "tray", Role::Other, "Photo tray", Target::Widget(&bg), false);
    let mut actions: Vec<Action> = Vec::new();
    let mut menu: Option<(Response, Vec<PhotoId>)> = None;

    // A double-click opens the photo in the edit view, once there is one (M7).
    let can_edit = crate::app::UI_COMMANDS.iter().any(|c| c.id == "view.edit") || app.session().is_some_and(|s| s.spec("view.edit").is_some());
    let state = app.state();
    if tray::is_empty(state) {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Selection", fonts::regular(t.body), t.text_dim);
    }

    // The token, at the left.
    let token = tray::token(state).map(|section| (section, tray::token_label(section, state.selection.ids.len())));
    let pill_w =
        token.as_ref().map(|(_, label)| painter.layout_no_wrap(label.clone(), fonts::semibold(t.small), t.on_accent).size().x + 2.0 * PILL_PAD);
    let parts = tray::parts(rect, pill_w);
    let clip = rect.shrink(1.0).intersect(ui.clip_rect());
    let thumbs_painter = painter.with_clip_rect(clip);
    let session = app.session();
    let ppp = ctx.pixels_per_point();
    if let (Some((cover, pill)), Some((_, label))) = (parts.token, &token) {
        if let (Some(session), Some(&first)) = (session, state.selection.ids.first())
            && let Some(photo) = session.library().photo(first)
        {
            let cell = cover.intersect(clip);
            let image = tray::crop(cover, photo.width, photo.height);
            p.thumbnails.paint(&thumbs_painter.with_clip_rect(cell), PhotoRef { session, photo }, image, cover.width() * ppp);
        }
        thumbs_painter.rect_filled(pill, CornerRadius::same((pill.height() / 2.0) as u8), t.accent);
        thumbs_painter.text(pill.center(), Align2::CENTER_CENTER, label, fonts::semibold(t.small), t.on_accent);
        let r = ui.interact(cover.union(pill).intersect(clip), Id::new("tray.token"), Sense::click());
        widgets::register(&ctx, tray::TOKEN_ID, Role::Other, label, Target::Widget(&r), false);
        if r.clicked() && !state.tray.selected.is_empty() {
            actions.push(("tray.select", json!({ "ids": [] })));
        }
        if r.secondary_clicked() || r.context_menu_opened() {
            menu = Some((r, tray::photos(state)));
        }
    }

    // The minis.
    let total = if token.is_some() { state.tray.held.len() } else { tray::count(state) };
    let area = parts.minis;
    let m = tray::minis(total, area.size());
    let overflow = total > m.capacity();
    let shown = if overflow { m.capacity().saturating_sub(1) } else { total };
    let (ids, _) = tray::thumbs(state, shown);
    let (command, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
    let mut placed: Vec<(PhotoId, Rect)> = Vec::with_capacity(ids.len());
    for (k, &id) in ids.iter().enumerate() {
        let Some(cell) = m.place(area, k) else { break };
        let selected = state.tray.selected.contains(&id);
        let photo = session.and_then(|s| s.library().photo(id).map(|p| (s, p)));
        let visible = cell.intersect(clip);
        match photo {
            Some((session, photo)) => {
                let image = tray::crop(cell, photo.width, photo.height);
                p.thumbnails.paint(&thumbs_painter.with_clip_rect(visible), PhotoRef { session, photo }, image, cell.width() * ppp);
            }
            None => {
                thumbs_painter.rect_filled(cell, 0, t.placeholder);
            }
        }
        if state.tray.holds(id) {
            held_badge(&thumbs_painter, &t, id, cell);
        }
        if selected {
            thumbs_painter.rect_stroke(cell, 0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
        }
        let r = ui.interact(visible, Id::new(("tray.item", id.get())), Sense::click());
        let name = photo.map_or("Photo", |(_, p)| p.name.as_str());
        widgets::register(&ctx, &tray::item_id(id), Role::Thumbnail, name, Target::Widget(&r), selected);
        if r.double_clicked() && can_edit {
            actions.push(("view.edit", json!({ "ids": [id] })));
        } else if r.clicked() {
            let mode = if command || shift { "toggle" } else { "replace" };
            actions.push(("tray.select", json!({ "ids": [id], "mode": mode })));
        } else if r.secondary_clicked() && !selected {
            actions.push(("tray.select", json!({ "ids": [id] })));
        }
        if r.secondary_clicked() || r.context_menu_opened() {
            let targets = if selected { state.tray.selected.clone() } else { vec![id] };
            menu = Some((r, targets));
        }
        placed.push((id, cell));
    }
    if overflow && let Some(cell) = m.place(area, shown) {
        let more = total.saturating_sub(shown);
        thumbs_painter.rect_filled(cell, CornerRadius::same(2), t.hover);
        let font = fonts::semibold(if m.size >= 28.0 { t.small } else { t.small * 0.8 });
        thumbs_painter.text(cell.center(), Align2::CENTER_CENTER, format!("+{more}"), font, t.text_label);
        widgets::register(&ctx, tray::MORE_ID, Role::Other, &format!("{more} more"), Target::Painted { rect: cell, clip }, false);
    }

    // The box's empty space: a click tray-selects nothing, a drag is a rubber band.
    if bg.clicked() && !state.tray.selected.is_empty() && !command {
        actions.push(("tray.select", json!({ "ids": [] })));
    }
    if let Some(a) = band(&ctx, &painter, &t, &bg, &placed, &state.tray.selected, command) {
        actions.push(a);
    }
    if bg.secondary_clicked() || bg.context_menu_opened() {
        let targets = if state.tray.selected.is_empty() { tray::photos(state) } else { state.tray.selected.clone() };
        menu = Some((bg, targets));
    }

    for (command, params) in actions {
        let _ = app.run(command, &params);
    }
    if let Some((response, targets)) = menu {
        let nodes = context_nodes(app, &targets);
        menus::context_menu(&response, app, &nodes);
    }
}

/// The held badge: a pin on a dark plate in the mini's top-right corner, registered as a part of
/// it (`tray:item:<PhotoId>/held`).
fn held_badge(painter: &egui::Painter, t: &Tokens, id: PhotoId, cell: Rect) {
    let edge = (cell.width() * 0.42).clamp(9.0, 16.0).round();
    let plate = Rect::from_min_size(pos2(cell.max.x - edge - 1.0, cell.min.y + 1.0), Vec2::splat(edge));
    painter.rect_filled(plate, CornerRadius::same(2), t.badge_plate);
    icons::paint(painter, plate.shrink(edge * 0.15), Icon::Hold, t.pick);
    let clip = painter.clip_rect();
    widgets::register(painter.ctx(), &format!("{}/held", tray::item_id(id)), Role::Badge, "Held", Target::Painted { rect: plate, clip }, true);
}

/// The rubber band: a drag from the box's empty space tray-selects the minis it touches (Ctrl:
/// added to the tray selection it started from). Returns the `tray.select` to run when its hits
/// change.
fn band(
    ctx: &egui::Context,
    painter: &egui::Painter,
    t: &Tokens,
    bg: &Response,
    placed: &[(PhotoId, Rect)],
    selected: &[PhotoId],
    command: bool,
) -> Option<Action> {
    type Band = (Pos2, Vec<PhotoId>, Vec<PhotoId>);
    if bg.drag_started() {
        let origin = ctx.input(|i| i.pointer.press_origin());
        let start = origin.filter(|o| !placed.iter().any(|(_, r)| r.contains(*o)));
        ctx.data_mut(|d| match start {
            Some(o) => {
                d.insert_temp::<Band>(band_id(), (o, if command { selected.to_vec() } else { Vec::new() }, Vec::new()));
            }
            None => d.remove::<Band>(band_id()),
        });
    }
    if !bg.dragged() || bg.drag_stopped() {
        ctx.data_mut(|d| d.remove::<Band>(band_id()));
        return None;
    }
    let (start, base, last) = ctx.data(|d| d.get_temp::<Band>(band_id()))?;
    let pos = ctx.input(|i| i.pointer.interact_pos())?;
    let rect = Rect::from_two_pos(start, pos);
    painter.rect(rect.intersect(bg.rect), 0, t.accent.gamma_multiply(0.18), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    let hits: Vec<PhotoId> = placed.iter().filter(|(_, r)| r.intersects(rect)).map(|(id, _)| *id).collect();
    if hits == last {
        return None;
    }
    ctx.data_mut(|d| d.insert_temp::<Band>(band_id(), (start, base.clone(), hits.clone())));
    let mut seen: HashSet<PhotoId> = base.iter().copied().collect();
    let mut ids = base;
    ids.extend(hits.into_iter().filter(|id| seen.insert(*id)));
    Some(("tray.select", json!({ "ids": ids })))
}

/// The Clear button: removes the tray-selected photos; with none selected empties the tray, asking
/// first when it holds held photos.
pub fn clear_clicked(app: &mut App, ctx: &egui::Context) {
    let state = app.state();
    let selected = !state.tray.selected.is_empty();
    if !selected && !state.tray.held.is_empty() {
        ctx.data_mut(|d| d.insert_temp(confirm_id(), true));
        return;
    }
    let _ = app.run("tray.clear", &json!({}));
}

/// The confirmation "Clear all photos from the tray?" [Cancel] [Clear Tray], while it is open
/// (drawn after the tray's buttons, so it shows in the frame Clear was clicked).
pub fn confirmation(app: &mut App, ctx: &egui::Context) {
    if !ctx.data(|d| d.get_temp::<bool>(confirm_id())).unwrap_or(false) {
        return;
    }
    let t = *app.tokens();
    let buttons = [
        DialogButton { id: DIALOG_CANCEL, label: "Cancel", kind: ButtonKind::Cancel, enabled: true },
        DialogButton { id: DIALOG_CLEAR, label: tray::CLEAR_TITLE, kind: ButtonKind::Default, enabled: true },
    ];
    let r = widgets::dialog(ctx, &t, DIALOG_ID, tray::CLEAR_TITLE, DIALOG_W, &buttons, |ui| {
        ui.label(egui::RichText::new(tray::CLEAR_QUESTION).font(fonts::regular(t.body)).color(t.text));
    });
    match r.clicked {
        Some(0) => ctx.data_mut(|d| d.remove::<bool>(confirm_id())),
        Some(_) => {
            ctx.data_mut(|d| d.remove::<bool>(confirm_id()));
            let _ = app.run("tray.clear", &json!({ "confirm": true }));
        }
        None => {}
    }
}

/// The tray menu (`ui-layout.md` §6.2) for `targets`: View and Edit · Hold · Remove from Tray ·
/// Rotate Clockwise · Rotate Counterclockwise · Locate on Disk · — · Properties. A row is live when
/// its command exists and enabled when it accepts `targets`; the others are greyed, never hidden.
pub fn context_nodes(app: &App, targets: &[PhotoId]) -> Vec<Node> {
    const ROWS: [Option<(&str, &str)>; 8] = [
        Some(("View and Edit", "view.edit")),
        Some(("Hold", "tray.hold")),
        Some(("Remove from Tray", "tray.clear")),
        Some(("Rotate Clockwise", "photo.rotate.cw")),
        Some(("Rotate Counterclockwise", "photo.rotate.ccw")),
        Some(("Locate on Disk", "photo.locate")),
        None,
        Some(("Properties", "pane.properties")),
    ];
    let ids = json!({ "ids": targets });
    ROWS.iter()
        .map(|row| match *row {
            None => Node::Separator,
            Some((label, id)) => {
                let ui_command = crate::app::UI_COMMANDS.iter().any(|c| c.id == id);
                let spec = app.session().and_then(|s| s.spec(id));
                let takes_ids = matches!(id, "tray.hold" | "tray.clear") || spec.is_some_and(|s| s.params.contains("\"ids\""));
                let params = if takes_ids { ids.clone() } else { Value::Null };
                let live = ui_command || spec.is_some();
                let (enabled, reason) = if !live {
                    (false, Some("Not available yet.".to_string()))
                } else if takes_ids && targets.is_empty() {
                    (false, Some(tray::EMPTY.to_string()))
                } else if let (Some(session), Some(spec)) = (app.session(), spec) {
                    match (spec.enabled)(session, &params) {
                        Ok(()) => (true, None),
                        Err(d) => (false, Some(d.0)),
                    }
                } else {
                    (true, None)
                };
                // The binding in effect, as the menus show it.
                let shortcut = app.session().filter(|_| live).and_then(|s| {
                    let keymap = s.keymap();
                    [Context::Library, Context::Global].into_iter().find_map(|c| keymap.keys_for(id, c).into_iter().next())
                });
                let shortcut = shortcut.map(|k| {
                    let k = k.to_string();
                    if cfg!(target_os = "macos") { k.replace("Ctrl+", "Cmd+") } else { k }
                });
                Node::Item(ItemNode {
                    id,
                    label: label.to_string(),
                    catalog_label: label,
                    params,
                    widget: format!("menuitem:{id}"),
                    shortcut,
                    live,
                    enabled,
                    reason,
                    mark: Mark::None,
                    checked: false,
                    dialog: None,
                })
            }
        })
        .collect()
}
