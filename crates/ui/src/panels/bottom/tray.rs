//! The Photo Tray's box (`ui-layout.md` §6.2), drawn from [`crate::tray`]: the "Selection"
//! watermark when empty; a section selected as a unit as one token (its cover and the pill
//! "Folder selected — 48 photos"); every other photo as a square, centre-cropped mini thumbnail
//! from the light table's thumbnails ([`Thumbnails::paint`]; the crop is of the photo as it shows,
//! turned by its recipe, [`Thumbnails::displayed`]), held photos first with the held
//! badge, shrinking as their count grows; what does not fit counted on the last place ("+12"). The
//! block of minis sits centred in its area ([`centred`]): the box, or the part right of a token.
//!
//! Input: a click on a mini thumbnail tray-selects it (Ctrl toggles; a double-click will open it in
//! the edit view), a click on the box's empty space tray-selects nothing, a rubber band dragged
//! from it tray-selects the minis it touches (Ctrl adds), and a right-click opens the tray menu (a mini not tray-selected is selected
//! first). Every change goes through the `tray.*` commands. The Clear button asks
//! "Clear all photos from the tray?" (through the dialog host) before it empties a tray that holds
//! held photos.
//!
//! **Add to Album ▾** ([`album_popup`]; the button is drawn with the tray's other buttons in
//! `super::tray_buttons`) opens a menu upward: New Album… (Album Properties for the tray's photos,
//! `album.new`), then the user albums, the most recent first, each adding the tray's photos
//! (`album.add`). An album section selected as a unit is one token like a folder's ("Album
//! selected — 40 photos") with the album's cover ([`token_cover`]).
//!
//! While the Collage room's Get More is on (`creating.md` §3), **Back to Collage**
//! (`button:tray.backToCollage`, [`back_to_collage`]) sits in the box's top-right corner, over
//! the minis: it brings the room back with the tray's photos added to its Clips
//! (`collage.backToCollage`).

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_engine::Context;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align, Align2, CornerRadius, Id, Layout, Popup, PopupKind, Pos2, Rect, RectAlign, Response, Sense, SetOpenCommand, Stroke, StrokeKind, Ui, Vec2,
    pos2,
};
use serde_json::{Value, json};

use super::super::Panels;
use super::super::thumbnails::PhotoRef;
use crate::app::App;
use crate::dialogs::DialogKind;
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node};
use crate::panels::light_table::context_menu;
use crate::state::{Section, UiState};
use crate::{selection, tray};

/// The pill's padding either side of its text.
const PILL_PAD: f32 = 8.0;

/// egui memory: a rubber band started at this point (and the tray selection it adds to).
fn band_id() -> Id {
    Id::new("tray.band")
}

/// A command to run once the frame's borrows end.
type Action = (&'static str, Value);

/// Paints and runs the tray box `rect`.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui, rect: Rect) {
    drop_hidden(app, ui.ctx());
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
        if let Some(session) = session
            && let Some(photo) = token_cover(state, session).and_then(|id| session.library().photo(id))
        {
            let cell = cover.intersect(clip);
            let photo = PhotoRef { session, photo };
            let (w, h) = p.thumbnails.displayed(photo);
            p.thumbnails.paint(&thumbs_painter.with_clip_rect(cell), photo, tray::crop(cover, w, h), cover.width() * ppp);
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
    let m = tray::minis(total, parts.minis.size());
    let overflow = total > m.capacity();
    let shown = if overflow { m.capacity().saturating_sub(1) } else { total };
    let area = centred(parts.minis, &m, total);
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
                let photo = PhotoRef { session, photo };
                let (w, h) = p.thumbnails.displayed(photo);
                p.thumbnails.paint(&thumbs_painter.with_clip_rect(visible), photo, tray::crop(cell, w, h), cell.width() * ppp);
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

    if back_to_collage(app, ui, &t, rect) {
        actions.push(("collage.backToCollage", json!({})));
    }
    for (command, params) in actions {
        if let Err(e) = app.run(command, &params)
            && command == "collage.backToCollage"
        {
            app.toast(&e.to_string(), widgets::ToastKind::Caution);
        }
    }
    if let Some((response, targets)) = menu {
        let nodes = context_nodes(app, &targets);
        menus::context_menu(&response, app, &nodes);
    }
}

/// Back to Collage's automation id and label.
pub const BACK_TO_COLLAGE_ID: &str = "button:tray.backToCollage";
pub const BACK_TO_COLLAGE: &str = "Back to Collage";
/// Its distance from the box's top-right corner.
const BACK_INSET: f32 = 6.0;

/// While Get More is on: Back to Collage in the top-right corner of the tray box `rect`; whether
/// it was clicked.
pub fn back_to_collage(app: &App, ui: &mut Ui, t: &Tokens, rect: Rect) -> bool {
    let state = app.state();
    if !(state.collage.get_more && state.rooms.contains(&crate::state::Room::Collage)) {
        return false;
    }
    let w = (ui.painter().layout_no_wrap(BACK_TO_COLLAGE.to_owned(), fonts::semibold(t.button_label), t.text).size().x + 18.0).max(37.0).round();
    let h = t.metrics.button_h;
    let at = Rect::from_min_size(pos2(rect.max.x - BACK_INSET - w, rect.min.y + BACK_INSET), Vec2::new(w, h)).intersect(rect);
    if !at.is_positive() {
        return false;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt("tray.backToCollage").max_rect(at));
    widgets::text_button(&mut child, t, BACK_TO_COLLAGE_ID, BACK_TO_COLLAGE, false)
        .on_hover_text("Add the photos in the tray to the collage and go back to it")
        .clicked()
}

/// The photo the token shows: a user album's cover when it is selected, else the section's first
/// photo.
pub fn token_cover(state: &UiState, session: &dalica_engine::Session) -> Option<PhotoId> {
    let sel = &state.selection;
    let cover = sel.section.and_then(Section::user_album).and_then(|a| session.library().album_cover(&a)).filter(|c| sel.contains(*c));
    cover.or_else(|| sel.ids.first().copied())
}

/// The id of Add to Album ▾'s popup.
pub fn album_popup_id() -> Id {
    Id::new("tray.addToAlbum.popup")
}

/// Whether Add to Album ▾ can open: the tray has photos to add.
pub fn add_to_album_enabled(app: &App) -> bool {
    !tray::is_empty(app.state())
}

/// The rows of Add to Album ▾ (`ui-layout.md` §6.2): New Album…, then the user albums, the most
/// recent first ([`context_menu::albums_by_date`]); they act on the tray's photos.
pub fn album_nodes(app: &App) -> Vec<Node> {
    let empty = tray::is_empty(app.state());
    let new_ready = if empty { Err(tray::EMPTY.to_string()) } else { crate::dialogs::can_open(app, DialogKind::AlbumProperties) };
    let mut out = vec![Node::Item(ItemNode {
        id: "album.new",
        label: "New Album…".into(),
        catalog_label: "New Album…",
        params: Value::Null,
        widget: "menuitem:album.new".into(),
        shortcut: None,
        live: true,
        enabled: new_ready.is_ok(),
        reason: new_ready.err(),
        mark: Mark::None,
        checked: false,
        dialog: Some("albumProperties"),
    })];
    let albums = context_menu::albums_by_date(app);
    if !albums.is_empty() {
        let live = app.session().is_some_and(|s| s.spec("album.add").is_some());
        let ready = if empty { Err(tray::EMPTY.to_string()) } else { Ok(()) };
        out.push(Node::Separator);
        out.extend(context_menu::album_rows(&albums, live, &ready));
    }
    out
}

/// Add to Album ▾'s popup over the button `button`: a click on the button (`clicked`) opens it, or
/// closes it when open; it opens upward and is bounded by the window above the button. A row
/// clicked runs on the tray's photos: New Album… opens Album Properties for them, an album adds
/// them (one undo step); a refusal is on the status line.
pub fn album_popup(app: &mut App, ui: &Ui, button: Rect, clicked: bool) {
    let id = album_popup_id();
    let ctx = ui.ctx().clone();
    if !clicked && !Popup::is_id_open(&ctx, id) {
        return;
    }
    let nodes = album_nodes(app);
    // The rows may take the room above the button: `menus::level` measures it down from `top`.
    let screen = ctx.content_rect();
    let top = screen.bottom() - (button.top() - screen.top());
    let mut picked: Option<ItemNode> = None;
    Popup::new(id, ctx, button, ui.layer_id())
        .kind(PopupKind::Menu)
        .layout(Layout::top_down_justified(Align::Min))
        .style(egui::containers::menu::menu_style)
        .gap(2.0)
        .align(RectAlign::TOP_END)
        .open_memory(clicked.then_some(SetOpenCommand::Toggle))
        .show(|ui| picked = menus::popup_rows(ui, &nodes, top));
    let Some(item) = picked.filter(|i| i.enabled) else { return };
    let r = match item.id {
        "album.add" => {
            let album = item.params.get("album").cloned().unwrap_or(Value::Null);
            app.run("album.add", &json!({ "album": album, "ids": tray::photos(app.state()) }))
        }
        id => app.run(id, &json!({})),
    };
    if let Err(e) = r {
        app.state_mut().status = e.to_string();
    }
}

/// Photos hidden while selected leave the selection (unless View ▸ Hidden Pictures), before the
/// tray shows it ([`selection::without_hidden`]). Looked at only when the library, the option or the
/// selection changed, so a still frame costs nothing whatever the selection's size.
fn drop_hidden(app: &mut App, ctx: &egui::Context) {
    let Some(revision) = app.session().map(|s| s.library().revision()) else { return };
    let sel = &app.state().selection;
    let key = (revision, app.state().hidden_pictures, sel.ids.len(), sel.ids.first().copied(), sel.ids.last().copied());
    let id = Id::new("tray.hiddenChecked");
    if ctx.data(|d| d.get_temp::<HiddenKey>(id)) == Some(key) {
        return;
    }
    let state = app.state();
    let kept = app.session().and_then(|s| selection::without_hidden(&state.selection, state.hidden_pictures, s.library()));
    if let Some(sel) = kept {
        app.state_mut().selection = sel;
    }
    let sel = &app.state().selection;
    let key = (revision, app.state().hidden_pictures, sel.ids.len(), sel.ids.first().copied(), sel.ids.last().copied());
    ctx.data_mut(|d| d.insert_temp(id, key));
}

/// What [`drop_hidden`] last looked at: the library revision, Hidden Pictures, and the selection's
/// length and ends.
type HiddenKey = (u64, bool, usize, Option<PhotoId>, Option<PhotoId>);

/// `area` moved right so that the places `count` minis laid out as `m` take ([`tray::Minis::place`]:
/// as many columns as the first row fills) sit centred across it. Whole points, so edges stay crisp.
pub fn centred(area: Rect, m: &tray::Minis, count: usize) -> Rect {
    let used = count.min(m.capacity()).min(m.cols);
    if used == 0 {
        return area;
    }
    let block = used as f32 * (m.size + tray::GAP) - tray::GAP;
    let slack = area.width() - 2.0 * tray::PAD - block;
    if !slack.is_finite() || slack <= 0.0 {
        return area;
    }
    area.translate(Vec2::new((slack / 2.0).floor(), 0.0))
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

/// The Clear button: removes the tray-selected photos; with none selected empties the tray. When
/// that would drop held photos, `tray.clear` asks "Clear all photos from the tray?" through the
/// dialog host (`dialog:clearTray`, `crate::dialogs::confirm`).
pub fn clear_clicked(app: &mut App) {
    let _ = app.run("tray.clear", &json!({}));
}

/// The tray menu (`ui-layout.md` §6.2) for `targets`: View and Edit · Hold · Remove from Tray ·
/// Rotate Clockwise · Rotate Counterclockwise · Locate on Disk · — · Properties. A row is live when
/// its command exists and enabled when it accepts `targets` (Locate on Disk the first of them,
/// greyed with the system's reason where it cannot show files); the others are greyed, never
/// hidden.
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
                // Locate on Disk shows the first of them (`photo.locate` takes one photo).
                let one = id == "photo.locate";
                let takes_ids = one || matches!(id, "tray.hold" | "tray.clear") || spec.is_some_and(|s| s.params.contains("\"ids\""));
                let params = match targets.first() {
                    Some(first) if one => json!({ "id": first }),
                    _ if takes_ids && !one => ids.clone(),
                    _ => Value::Null,
                };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::state::{AlbumKey, AlbumSection, Selection};

    #[test]
    fn the_album_token_shows_the_album_cover_and_the_popup_lists_new_album_then_the_albums() {
        let (_dir, mut s, ids) = session_with_photos(3);
        let a = s.execute("album.create", &json!({ "name": "Trams", "ids": [ids[0], ids[1]] })).unwrap()["album"].as_str().unwrap().to_string();
        let album = dalica_core::AlbumId::parse(&a).unwrap();
        s.execute("album.setCover", &json!({ "album": a, "id": ids[1] })).unwrap();
        let section = Section::Album(AlbumSection::User(AlbumKey::of(&album)));
        let mut state = UiState { selection: Selection::new(vec![ids[0], ids[1]], None, Some(section), true), ..UiState::default() };
        assert_eq!(token_cover(&state, &s), Some(ids[1]), "the album's cover");
        state.selection = Selection::new(vec![ids[0]], None, Some(section), true);
        assert_eq!(token_cover(&state, &s), Some(ids[0]), "a cover outside the selection is not shown");
        state.selection = Selection::new(vec![ids[2], ids[0]], None, Some(Section::Album(AlbumSection::Starred)), true);
        assert_eq!(token_cover(&state, &s), Some(ids[2]), "an automatic album shows its first photo");

        let mut app = App::new(Some(s));
        assert!(!add_to_album_enabled(&app), "an empty tray");
        let labels = |app: &App| -> Vec<(String, bool)> {
            album_nodes(app).iter().map(|n| if let Node::Item(i) = n { (i.label.clone(), i.enabled) } else { ("—".into(), false) }).collect()
        };
        assert_eq!(labels(&app), [("New Album…".to_string(), false), ("—".into(), false), ("Trams".into(), false)]);
        app.run("select.set", &json!({ "ids": [ids[2]] })).unwrap();
        assert!(add_to_album_enabled(&app));
        assert_eq!(labels(&app), [("New Album…".to_string(), true), ("—".into(), false), ("Trams".into(), true)]);
        // No library: New Album… only, never a panic.
        assert_eq!(album_nodes(&App::new(None)).len(), 1);
    }
}
