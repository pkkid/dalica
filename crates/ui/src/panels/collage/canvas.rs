//! The collage canvas (`creating.md` §3): the page at its ratio (`canvas:collage`) with each
//! picture where the engine's placements put it (`item:<n>`, its number among them, bottom first;
//! the selected ones outlined in `accent`), the buttons above it (Select All, Select None, Remove,
//! Set as Background, or Set as Frame Center in a Frame Mosaic) and below it (Scramble Collage,
//! Shuffle Pictures, View and Edit), and the gestures:
//!
//! - **a click** selects the picture under the pointer (the topmost); Ctrl or Shift adds it to the
//!   selection; a click on the page beside every picture selects none;
//! - **a drag** of a pile's picture moves it, and in the other styles a drag onto another picture
//!   swaps them; while it runs it is the room's `collage.gesture` (`move`) and the picture follows
//!   the pointer, and nothing changes in the collage before the drop (one step of its history);
//! - **the wheel handle** (`canvas:collage/handle`, ui-kit's `transform_handle`) on the one picture
//!   selected in a pile turns and scales it round its centre (`turn` while it runs), one step when
//!   it is let go;
//! - **a right-click** selects the picture under the pointer (unless it is selected) and opens its
//!   menu: Bring to Top, Send to Bottom, View and Edit, which act on that picture.
//!
//! The page is drawn from the session in every frame ([`pictures`]: the thumbnail ladder, never a
//! decode, never the disk; a grey placeholder until a picture's thumbnail arrives), as the render
//! draws it: the background, the borders and the shadows. The commands are
//! `crate::commands::collage_canvas`. Until the room shows a collage of the engine's (the stub room
//! `collage.open` opens) the page is empty, at the default format.

pub mod geometry;
pub mod pictures;

use dalica_create::collage::{Collage, Placement, Style};
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind, TransformHandle};
use egui::{Id, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use self::geometry::Card;
use self::pictures::Pictures;
use crate::app::App;
use crate::menus::{self, Node, Platform};
use crate::state::{CollageGesture, GESTURE_MAX, GESTURE_MIN, GestureKind};

/// The canvas's automation id.
pub const CANVAS_ID: &str = "canvas:collage";
/// The wheel handle on the one picture selected in a pile.
pub const HANDLE_ID: &str = "canvas:collage/handle";
/// The space round the page (points).
pub const MARGIN: f32 = 24.0;
/// The space above the row of buttons over the page and below the row under it (points).
pub const ROW_PAD: f32 = 12.0;
/// The space between two buttons of a row.
pub const BUTTON_GAP: f32 = 8.0;
/// The default page's width over its height (16:9, landscape).
pub const DEFAULT_RATIO: f32 = 16.0 / 9.0;
/// How a picture being dragged onto another to swap them is shown (its opacity).
pub const GHOST: f32 = 0.6;

/// The buttons above the page: command and label (Set as Frame Center stands in for Set as
/// Background in a Frame Mosaic).
pub const ABOVE: [(&str, &str); 4] = [
    ("collageCanvas.selectAll", "Select All"),
    ("collageCanvas.selectNone", "Select None"),
    ("collageCanvas.remove", "Remove"),
    ("collageCanvas.setBackground", "Set as Background"),
];
pub const FRAME_CENTER: (&str, &str) = ("collageCanvas.setFrameCenter", "Set as Frame Center");
/// The buttons below the page.
pub const BELOW: [(&str, &str); 3] =
    [("collageCanvas.scramble", "Scramble Collage"), ("collageCanvas.shuffle", "Shuffle Pictures"), ("collageCanvas.viewAndEdit", "View and Edit")];

/// A picture's automation id: `item:<n>`, its number in the engine's placements.
pub fn item_id(n: u32) -> String {
    format!("item:{n}")
}

/// A button's automation id: `button:<command id>`.
pub fn button_id(command: &str) -> String {
    format!("button:{command}")
}

/// The page of width over height `ratio`, as large as fits in `area` less [`MARGIN`], centred;
/// `None` when nothing fits.
pub fn page_rect(area: Rect, ratio: f32) -> Option<Rect> {
    let room = area.shrink(MARGIN);
    if !(room.is_finite() && room.is_positive() && ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let w = room.width().min(room.height() * ratio);
    Some(Rect::from_center_size(room.center(), vec2(w, w / ratio)))
}

/// The canvas's parts in `area` with buttons `button_h` high: the row above, the room the page
/// fits in, the row below.
pub fn parts(area: Rect, button_h: f32) -> (Rect, Rect, Rect) {
    let above = Rect::from_min_max(pos2(area.min.x, area.min.y + ROW_PAD), pos2(area.max.x, area.min.y + ROW_PAD + button_h));
    let below = Rect::from_min_max(pos2(area.min.x, area.max.y - ROW_PAD - button_h), pos2(area.max.x, area.max.y - ROW_PAD));
    let room = Rect::from_min_max(pos2(area.min.x, above.max.y), pos2(area.max.x, below.min.y.max(above.max.y)));
    (above, room, below)
}

/// How many pictures the last frame drew without the thumbnail that covers them (a smaller one, or
/// the placeholder): the canvas is still loading while this is above 0.
pub fn waiting(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<usize>(Id::new(WAITING)).unwrap_or(0))
}

/// How many textures the canvas keeps.
pub fn textures(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<Pictures>(Id::new(PICTURES)).map_or(0, |p| p.textures()))
}

const PICTURES: &str = "collage.canvas.pictures";
const WAITING: &str = "collage.canvas.waiting";
/// The picture the open menu acts on.
const MENU_ITEM: &str = "collage.canvas.menuItem";

/// What the frame has of the engine's collage.
struct Shown {
    doc: Collage,
    placements: Vec<Placement>,
}

/// Draws the canvas into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let area = ui.max_rect();
    let shown = app.session_mut().and_then(|s| {
        let doc = s.collage().document().cloned()?;
        Some(Shown { doc, placements: s.collage_placements() })
    });
    let ratio = shown.as_ref().map_or(DEFAULT_RATIO, |s| s.doc.page.ratio() as f32);
    let (above, room, below) = parts(area, t.metrics.button_h);
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    let frame = shown.as_ref().is_some_and(|s| s.doc.style == Style::FrameMosaic);
    let top: Vec<(&str, &str)> = ABOVE.iter().map(|b| if frame && b.0 == ABOVE[3].0 { FRAME_CENTER } else { *b }).collect();
    buttons(app, ui, above, &top, &mut actions);
    buttons(app, ui, below, &BELOW, &mut actions);

    if let Some(page) = page_rect(room, ratio) {
        match &shown {
            Some(shown) => canvas(app, ui, page, area, shown, &mut actions),
            None => {
                ui.painter().rect_filled(page, 0, t.canvas);
                widgets::register(ui.ctx(), CANVAS_ID, Role::Other, "Collage page", Target::Painted { rect: page, clip: area }, false);
            }
        }
    }
    for (id, params) in actions {
        if let Err(e) = app.run(id, &params) {
            app.toast(&e.to_string(), ToastKind::Caution);
        }
    }
}

/// A row of buttons centred in `row`, each greyed while its command cannot run.
fn buttons(app: &App, ui: &mut Ui, row: Rect, list: &[(&'static str, &str)], actions: &mut Vec<(&'static str, Value)>) {
    let t = *app.tokens();
    let font = fonts::semibold(t.button_label);
    let widths: Vec<f32> = list
        .iter()
        .map(|(_, label)| (ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), t.text).size().x + 18.0).max(37.0).round())
        .collect();
    let total = widths.iter().sum::<f32>() + BUTTON_GAP * widths.len().saturating_sub(1) as f32;
    let x = (row.center().x - total / 2.0).max(row.min.x).round();
    let rect = Rect::from_min_max(pos2(x, row.min.y), row.max);
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = BUTTON_GAP;
        for (command, label) in list {
            let enabled = crate::commands::enabled(app, command, &json!({})).is_ok();
            if ui.add_enabled_ui(enabled, |ui| widgets::text_button(ui, &t, &button_id(command), label, false)).inner.clicked() {
                actions.push((command, json!({})));
            }
        }
    });
}

/// The page with the collage on it, and its gestures.
fn canvas(app: &mut App, ui: &mut Ui, page: Rect, area: Rect, shown: &Shown, actions: &mut Vec<(&'static str, Value)>) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let Shown { doc, placements } = shown;
    let r = ui.interact(page, Id::new(CANVAS_ID), Sense::click_and_drag());
    widgets::register(&ctx, CANVAS_ID, Role::Other, "Collage page", Target::Widget(&r), false);

    // Where every picture is now, and the gesture's picture where the gesture puts it.
    let now: Vec<Card> = placements.iter().enumerate().filter_map(|(i, p)| geometry::card(doc, page, u32::try_from(i).ok()?, p)).collect();
    let gesture = app.state().collage.gesture.clone().filter(|g| (g.item as usize) < placements.len());
    let pile = doc.style == Style::Pile;
    let shift = ui.input(|i| i.modifiers.shift);
    let preview = |card: &Card, p: &Placement| -> Card {
        match &gesture {
            Some(g) if g.item == card.item && g.kind == GestureKind::Move && pile => {
                let d = geometry::to_screen(page, [f64::from(g.to[0]), f64::from(g.to[1])])
                    - geometry::to_screen(page, [f64::from(g.from[0]), f64::from(g.from[1])]);
                geometry::card_at(doc, page, card.item, p, card.centre() + d, 1.0, card.angle).unwrap_or(*card)
            }
            Some(g) if g.item == card.item && g.kind == GestureKind::Turn => {
                let s0 = scale_of(doc, p);
                let (angle, scale) = turned(page, card, g, s0, shift);
                geometry::card_at(doc, page, card.item, p, card.centre(), scale / s0 as f32, angle).unwrap_or(*card)
            }
            _ => *card,
        }
    };
    let cards: Vec<Card> = now.iter().zip(placements).map(|(c, p)| preview(c, p)).collect();

    // The page as the render draws it.
    let mut pics: Pictures = ctx.data_mut(|d| d.remove_temp(Id::new(PICTURES))).unwrap_or_default();
    let painter = ui.painter_at(page);
    painter.rect_filled(page, 0, pictures::ground(doc));
    let ppp = ctx.pixels_per_point();
    if let Some(session) = app.session_mut() {
        pics.begin(session);
        if doc.style != Style::MultipleExposure
            && let Some(id) = doc.background.picture
            && let Some(src) = pics.source(&ctx, session, id, page.width().max(page.height()) * ppp)
        {
            pictures::background(&painter, page, &src);
        }
        let swapping = gesture.as_ref().filter(|g| g.kind == GestureKind::Move && !pile).map(|g| g.item);
        for (k, (card, p)) in cards.iter().zip(placements).enumerate() {
            let src = pics.source(&ctx, session, p.id, pictures::need(card, p, ppp));
            let tint =
                if doc.style == Style::MultipleExposure { egui::Color32::WHITE.gamma_multiply(1.0 / (k as f32 + 1.0)) } else { egui::Color32::WHITE };
            let tint = if swapping == Some(card.item) { tint.gamma_multiply(GHOST) } else { tint };
            pictures::picture(&painter, doc, card, p, src.as_ref(), t.placeholder, tint);
        }
        pics.end();
    }
    let waiting = pics.waiting;
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(PICTURES), pics);
        d.insert_temp(Id::new(WAITING), waiting);
    });
    if waiting > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }

    // Each picture's id, its box round the picture as shown (its centre is the placement's).
    let selection = app.state().collage.selection.clone();
    let names: Vec<String> = app
        .session()
        .map(|s| placements.iter().map(|p| s.library().photo(p.id).map_or_else(|| format!("Picture {}", p.id), |ph| ph.name.clone())).collect())
        .unwrap_or_default();
    for card in &cards {
        let label = names.get(card.item as usize).map_or("Picture", String::as_str);
        let target = Target::Painted { rect: card.picture_bounds(), clip: area };
        widgets::register(&ctx, &item_id(card.item), Role::Other, label, target, selection.contains(&card.item));
    }

    // The selection's outlines; the wheel handle on a pile's one selected picture.
    let outline = ui.painter_at(area);
    let handled = (pile && selection.len() == 1 && gesture.as_ref().is_none_or(|g| g.kind == GestureKind::Turn)).then(|| selection[0]);
    for card in cards.iter().filter(|c| selection.contains(&c.item) && Some(c.item) != handled) {
        outline.add(Shape::closed_line(card.corners(card.card).to_vec(), Stroke::new(widgets::TRANSFORM_OUTLINE, t.accent)));
    }
    if let Some(g) = gesture.as_ref().filter(|g| g.kind == GestureKind::Move && !pile)
        && let Some(to) = target_of(&now, page, g)
    {
        outline.add(Shape::closed_line(to.corners(to.card).to_vec(), Stroke::new(widgets::TRANSFORM_OUTLINE, t.accent)));
    }
    if let Some(n) = handled
        && let (Some(card), Some(p)) = (now.get(n as usize), placements.get(n as usize))
    {
        let h = TransformHandle { rect: card.picture, angle: card.angle, scale: scale_of(doc, p) as f32 };
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        let out = widgets::transform_handle(&mut child, &t, HANDLE_ID, &h);
        let pointer = out.response.interact_pointer_pos().or_else(|| ctx.pointer_latest_pos());
        if out.drag_started
            && let Some(at) = pointer
        {
            let at = clamp(geometry::to_page(page, at));
            app.state_mut().collage.gesture = Some(CollageGesture { item: n, kind: GestureKind::Turn, from: at, to: at });
        } else if out.response.dragged()
            && let Some(at) = pointer
            && let Some(g) = app.state_mut().collage.gesture.as_mut().filter(|g| g.kind == GestureKind::Turn)
        {
            g.to = clamp(geometry::to_page(page, at));
        }
        if out.drag_stopped
            && let Some(g) = app.state_mut().collage.gesture.take().filter(|g| g.kind == GestureKind::Turn)
        {
            let s0 = scale_of(doc, p);
            let (angle, scale) = turned(page, card, &g, s0, shift);
            actions.push(("collageCanvas.turn", json!({ "item": n, "angle": angle, "scale": scale })));
        }
    }

    gestures(app, ui, &r, page, doc, placements, &now, actions);
}

/// The scale the user gave picture `p` (1: the layout's size).
fn scale_of(doc: &Collage, p: &Placement) -> f64 {
    doc.move_of(p.id).and_then(|m| m.scale).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0)
}

/// The angle and scale turn gesture `g` gives `card` (at scale `s0`), as the wheel handle works
/// them out: round the picture's centre, from where the pointer took the wheel to where it is.
fn turned(page: Rect, card: &Card, g: &CollageGesture, s0: f64, keep_scale: bool) -> (f32, f32) {
    let at = |v: [f32; 2]| geometry::to_screen(page, [f64::from(v[0]), f64::from(v[1])]);
    widgets::turn_and_scale(card.centre(), at(g.from), at(g.to), card.angle, s0 as f32, keep_scale)
}

/// A point normalised to the page, kept where a gesture's points may lie.
fn clamp(p: [f32; 2]) -> [f32; 2] {
    p.map(|v| if v.is_finite() { v.clamp(GESTURE_MIN, GESTURE_MAX) } else { 0.5 })
}

/// The picture a swap drag `g` would drop on: the topmost other picture under its pointer.
fn target_of<'a>(cards: &'a [Card], page: Rect, g: &CollageGesture) -> Option<&'a Card> {
    let at = geometry::to_screen(page, [f64::from(g.to[0]), f64::from(g.to[1])]);
    cards.iter().rev().find(|c| c.item != g.item && c.contains(at))
}

/// Clicks, drags and the menu on the page.
#[allow(clippy::too_many_arguments)] // one frame's parts of the canvas, passed down once
fn gestures(
    app: &mut App,
    ui: &Ui,
    r: &egui::Response,
    page: Rect,
    doc: &Collage,
    placements: &[Placement],
    cards: &[Card],
    actions: &mut Vec<(&'static str, Value)>,
) {
    let ctx = ui.ctx();
    let (adding, origin) = ui.input(|i| (i.modifiers.command || i.modifiers.shift, i.pointer.press_origin()));
    let pointer = r.interact_pointer_pos().or_else(|| ctx.pointer_latest_pos());
    let under = |p: Option<Pos2>| p.and_then(|p| geometry::hit(cards, p)).map(|c| c.item);

    if r.clicked() {
        match under(pointer) {
            Some(n) => actions.push(("collageCanvas.select", json!({ "items": [n], "add": adding }))),
            None => actions.push(("collageCanvas.selectNone", json!({}))),
        }
    }
    if r.secondary_clicked() {
        let n = under(pointer);
        if let Some(n) = n
            && !app.state().collage.selection.contains(&n)
        {
            // Selected now, so the menu's rows see it this frame.
            app.state_mut().collage.selection = vec![n];
        }
        ctx.data_mut(|d| d.insert_temp(Id::new(MENU_ITEM), n));
    }
    if r.drag_started()
        && let Some(n) = under(origin.or(pointer))
    {
        let at = clamp(geometry::to_page(page, origin.or(pointer).unwrap_or(page.center())));
        if !app.state().collage.selection.contains(&n) {
            app.state_mut().collage.selection = if adding { [app.state().collage.selection.clone(), vec![n]].concat() } else { vec![n] };
        }
        app.state_mut().collage.gesture = Some(CollageGesture { item: n, kind: GestureKind::Move, from: at, to: at });
    }
    if r.dragged()
        && let Some(at) = pointer
        && let Some(g) = app.state_mut().collage.gesture.as_mut().filter(|g| g.kind == GestureKind::Move)
    {
        g.to = clamp(geometry::to_page(page, at));
    }
    if r.drag_stopped()
        && let Some(mut g) = app.state_mut().collage.gesture.take().filter(|g| g.kind == GestureKind::Move)
    {
        if let Some(at) = pointer {
            g.to = clamp(geometry::to_page(page, at));
        }
        if doc.style == Style::Pile {
            if let Some(p) = placements.get(g.item as usize)
                && g.to != g.from
            {
                let centre = [p.centre[0] + f64::from(g.to[0] - g.from[0]), p.centre[1] + f64::from(g.to[1] - g.from[1])];
                let centre = centre.map(|v| v.clamp(f64::from(GESTURE_MIN), f64::from(GESTURE_MAX)));
                actions.push(("collageCanvas.move", json!({ "item": g.item, "centre": centre })));
            }
        } else if let Some(to) = target_of(cards, page, &g) {
            actions.push(("collageCanvas.swap", json!({ "a": g.item, "b": to.item })));
        }
    }

    // The picture menu, its rows acting on the picture right-clicked.
    let item: Option<u32> = ctx.data(|d| d.get_temp::<Option<u32>>(Id::new(MENU_ITEM))).flatten();
    let mut rows = menus::collage_menu(app, Platform::current());
    if let Some(n) = item {
        for row in &mut rows {
            if let Node::Item(i) = row {
                if let Some(o) = i.params.as_object_mut() {
                    o.insert("item".into(), json!(n));
                } else {
                    i.params = json!({ "item": n });
                }
            }
        }
    }
    menus::context_menu(r, app, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_fits_its_area_at_its_ratio() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1048.0, 548.0));
        let page = page_rect(area, DEFAULT_RATIO).unwrap();
        assert!((page.width() / page.height() - DEFAULT_RATIO).abs() < 0.01);
        assert!(area.shrink(MARGIN).contains_rect(page));
        assert_eq!(page_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 40.0)), DEFAULT_RATIO), None);
        assert_eq!(page_rect(area, f32::NAN), None);
        let (above, room, below) = parts(area, 24.0);
        assert!(above.max.y <= room.min.y && room.max.y <= below.min.y && below.max.y < area.max.y);
    }
}
