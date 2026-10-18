//! The Unnamed grid (`organizing.md` §6.2): while Unnamed is selected in the People group
//! (`unnamed.open`) it fills the light table's region in its place.
//!
//! - **The header**: "Unnamed people" with [Expand Groups] (`button:faces.expandGroups`) and [Show
//!   Ignored Faces] (`button:faces.showIgnored`), active while on.
//! - **The groups** of similar faces ([`model`]), largest first, flow left to right in cards
//!   (`group:<n>`, from 0): a line saying how many faces the group holds, its × at the top right
//!   (`group:<n>/ignore`, "Ignore these faces?" first), its face tiles ([`FACE_SIDE`] points,
//!   `face:<PhotoId>.<n>`, at most [`COLUMNS`] a row; a group shows its first row only until
//!   Expand Groups), and its "Add a name" field (`field:faces.name/<n>`) with the people's names
//!   offered as it is typed (`field:faces.name/<n>/option/<k>`): Enter names the whole group
//!   (`faces.nameGroup`). With Show Ignored Faces the ignored faces follow in a card of their own
//!   (`group:ignored`), dimmed, with no name field.
//! - **Selecting**: a click selects a face, Ctrl-click adds or takes it away, Shift-click selects
//!   the faces from the selection's anchor (its first face, outlined white) in the grid's order; a
//!   rubber band from the empty space (`lightTable`) selects the faces it touches, across groups
//!   (with Ctrl, added to the selection); a click on the empty space clears it. Delete ignores the
//!   faces selected (`crate::keys::delete_route`).
//! - **Dragging** a face carries the faces selected (or that face alone when it is not selected)
//!   onto a person in the tree (`crate::drag::start_faces`); the drag pill says "3 faces", or
//!   "Name 3 faces Ana" over a person ([`paint_drag`]).
//! - **The face menu** ([`menu`]) opens on a right-click, which selects the face first when it is
//!   not selected.
//!
//! The grid is made again only when the library, the face index or Show Ignored Faces changes
//! (kept in the egui context with what it was made from); the face pictures come from the
//! session's face tiles, cut on a worker, never on this thread, through the one store every view
//! showing faces shares (`crate::panels::people_pane::pictures`: the session hands each tile to one
//! taker only).

pub mod menu;
pub mod model;
pub mod suggestions;

use std::collections::HashSet;
use std::sync::Arc;

use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, DragPill, FaceLabel, FaceTile, PillThumb, Target, face_tile_size};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Context, CornerRadius, Id, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::panels::people_pane::pictures;
use crate::state::{DragOp, DropAt, FaceRef};
use model::{Grid, Key};

/// The header's title.
pub const TITLE: &str = "Unnamed people";
/// What the grid says with no face to show.
pub const STUB: &str = "Faces appear here once face recognition finds them.";
/// The header's toggles: their widget ids, labels and commands.
pub const TOGGLES: [(&str, &str, &str); 2] =
    [("button:faces.expandGroups", "Expand Groups", "faces.expandGroups"), ("button:faces.showIgnored", "Show Ignored Faces", "faces.showIgnored")];
/// The title of the ignored faces' card.
pub const IGNORED_TITLE: &str = "Ignored faces";
/// The hint of a group's name field.
pub const NAME_HINT: &str = "Add a name";
/// The automation id of the empty space (as the light table's).
pub const EMPTY_SPACE_ID: &str = "lightTable";
/// The id of the ignored faces' card.
pub const IGNORED_ID: &str = "group:ignored";

/// A face picture's side (points).
pub const FACE_SIDE: f32 = 72.0;
/// The most faces in a row of a card, and the faces a group shows until Expand Groups.
pub const COLUMNS: usize = 4;
/// The most faces in a row of the ignored card.
pub const IGNORED_COLUMNS: usize = 8;
/// A card's padding, the gap between cards, and the room round the grid (points).
pub const CARD_PAD: f32 = 8.0;
pub const CARD_GAP: f32 = 12.0;
pub const MARGIN: f32 = 16.0;
/// The narrowest a card's inside is (room for "Add a name").
pub const CARD_MIN_INNER: f32 = 160.0;
/// A card's top line (its count and ×) and its name field (points).
pub const CARD_TOP_H: f32 = 22.0;
pub const FIELD_GAP: f32 = 6.0;
/// The ×'s edge (points).
pub const IGNORE_BUTTON: f32 = 20.0;
/// The header's height (points).
pub const HEADER_H: f32 = 48.0;

/// What a card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Group(u32),
    Ignored,
}

/// One card placed: where, which faces it shows, and how many it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub kind: CardKind,
    /// In content coordinates (0, 0 at the top left of the scrolled content).
    pub rect: Rect,
    pub faces: Vec<FaceRef>,
    pub total: usize,
    pub cols: usize,
}

impl Card {
    /// The automation id of the card.
    pub fn id(&self) -> String {
        match self.kind {
            CardKind::Group(n) => format!("group:{n}"),
            CardKind::Ignored => IGNORED_ID.into(),
        }
    }

    /// The rect of its `i`-th face tile (content coordinates).
    pub fn tile(&self, i: usize) -> Rect {
        let size = face_tile_size(FACE_SIDE, false, false);
        let cols = self.cols.max(1);
        let (row, col) = (i / cols, i % cols);
        let min = self.rect.min + vec2(CARD_PAD + col as f32 * size.x, CARD_PAD + CARD_TOP_H + row as f32 * size.y);
        Rect::from_min_size(min, size)
    }

    /// The rect of its name field (content coordinates), for a group.
    pub fn field(&self, t: &Tokens) -> Rect {
        let h = t.metrics.text_field_h;
        Rect::from_min_max(
            pos2(self.rect.min.x + CARD_PAD, self.rect.max.y - CARD_PAD - h),
            pos2(self.rect.max.x - CARD_PAD, self.rect.max.y - CARD_PAD),
        )
    }
}

/// "4 faces", "1 face", or while a group shows only some "4 of 9 faces".
pub fn count_text(shown: usize, total: usize) -> String {
    let faces = if total == 1 { "1 face".to_string() } else { format!("{total} faces") };
    if shown < total { format!("{shown} of {faces}") } else { faces }
}

/// The cards of `grid` laid out in `width` points: each group's (its first row only unless
/// `expanded`), then the ignored faces' on a row of its own; and the content's height.
pub fn layout(grid: &Grid, expanded: bool, width: f32, field_h: f32) -> (Vec<Card>, f32) {
    let tile = face_tile_size(FACE_SIDE, false, false);
    let width = if width.is_finite() { width.max(1.0) } else { 1.0 };
    let mut cards = Vec::with_capacity(grid.groups.len() + 1);
    let (mut x, mut y, mut row_h) = (0.0f32, 0.0f32, 0.0f32);
    let mut place = |kind: CardKind, faces: Vec<FaceRef>, total: usize, max_cols: usize, field: bool, new_row: bool| {
        let cols = faces.len().clamp(1, max_cols);
        let rows = faces.len().div_ceil(cols).max(1);
        let inner_w = (cols as f32 * tile.x).max(CARD_MIN_INNER);
        let field_h = if field { FIELD_GAP + field_h } else { 0.0 };
        let size = vec2(inner_w + 2.0 * CARD_PAD, CARD_PAD + CARD_TOP_H + rows as f32 * tile.y + field_h + CARD_PAD);
        if x > 0.0 && (new_row || x + size.x > width) {
            x = 0.0;
            y += row_h + CARD_GAP;
            row_h = 0.0;
        }
        cards.push(Card { kind, rect: Rect::from_min_size(pos2(x, y), size), faces, total, cols });
        x += size.x + CARD_GAP;
        row_h = row_h.max(size.y);
    };
    for (n, g) in grid.groups.iter().enumerate() {
        let shown = if expanded { g.faces.len() } else { g.faces.len().min(COLUMNS) };
        let faces = g.faces.get(..shown).unwrap_or_default().to_vec();
        place(CardKind::Group(u32::try_from(n).unwrap_or(u32::MAX)), faces, g.faces.len(), COLUMNS, true, false);
    }
    if !grid.ignored.is_empty() {
        place(CardKind::Ignored, grid.ignored.clone(), grid.ignored.len(), IGNORED_COLUMNS, false, true);
    }
    (cards, y + row_h)
}

/// The grid kept in the egui context, with what it was made from.
#[derive(Clone)]
struct Cached {
    key: Key,
    grid: Arc<Grid>,
}

/// The grid now: the one kept while the library, the face index and Show Ignored Faces are as
/// they were, else made again (the names typed and the selection following it).
pub fn current(app: &mut App, ctx: &Context) -> Arc<Grid> {
    current_with_key(app, ctx).0
}

fn current_with_key(app: &mut App, ctx: &Context) -> (Arc<Grid>, Option<Key>) {
    let ignored = app.state().unnamed.show_ignored;
    let Some(session) = app.session_mut() else { return (Arc::default(), None) };
    let key = model::key(session, ignored);
    let id = Id::new("faces.grid");
    let kept: Option<Cached> = ctx.data(|d| d.get_temp(id));
    if let Some(c) = &kept
        && c.key == key
    {
        return (c.grid.clone(), Some(key));
    }
    let grid = Arc::new(model::grid(session, ignored));
    if let Some(old) = kept {
        let u = &mut app.state_mut().unnamed;
        u.names = model::remap_names(&u.names, &old.grid, &grid);
        u.selection = model::still_shown(&u.selection, &grid);
    }
    ctx.data_mut(|d| d.insert_temp(id, Cached { key, grid: grid.clone() }));
    (grid, Some(key))
}

/// The cards laid out, the content's height and the faces shown in the grid's order, kept while
/// the grid, Expand Groups and the width are as they were.
#[derive(Clone, Default)]
struct Laid {
    key: Option<(Key, bool, u32)>,
    cards: Arc<Vec<Card>>,
    height: f32,
    order: Arc<Vec<FaceRef>>,
}

fn laid_out(ctx: &Context, key: Option<Key>, grid: &Grid, expanded: bool, width: f32, field_h: f32) -> Laid {
    let id = Id::new("faces.layout");
    let key = key.map(|k| (k, expanded, width.to_bits()));
    if let Some(l) = ctx.data(|d| d.get_temp::<Laid>(id)).filter(|l| key.is_some() && l.key == key) {
        return l;
    }
    let (cards, height) = layout(grid, expanded, width, field_h);
    let order = cards.iter().flat_map(|c| c.faces.iter().copied()).collect();
    let laid = Laid { key, cards: Arc::new(cards), height, order: Arc::new(order) };
    ctx.data_mut(|d| d.insert_temp(id, laid.clone()));
    laid
}

/// The people's names for the name fields, kept while the library is as it was.
fn names_offered(app: &App, ctx: &Context) -> Arc<Vec<String>> {
    let Some(revision) = app.session().map(|s| s.library().revision()) else { return Arc::default() };
    let id = Id::new("faces.people");
    if let Some((r, names)) = ctx.data(|d| d.get_temp::<(u64, Arc<Vec<String>>)>(id))
        && r == revision
    {
        return names;
    }
    let names: Arc<Vec<String>> = Arc::new(menu::people_by_name(app).into_iter().map(|(_, name)| name).collect());
    ctx.data_mut(|d| d.insert_temp(id, (revision, names.clone())));
    names
}

/// A rubber band being dragged: where it started (content coordinates) and the selection it adds
/// to.
#[derive(Clone, Default)]
struct Band {
    start: Pos2,
    base: Vec<FaceRef>,
}

/// What the pass asks for once the grid is drawn.
#[derive(Default)]
struct Asks {
    command: Option<(&'static str, serde_json::Value)>,
    selection: Option<Vec<FaceRef>>,
    drag: Option<(FaceRef, Vec<FaceRef>)>,
    menu: Option<(egui::Response, FaceRef)>,
}

/// Draws the grid into `ui`'s rect (the light table's region, its scroll bar's included).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let area = ui.max_rect();
    let header = Rect::from_min_max(area.min + vec2(MARGIN, 0.0), pos2(area.max.x - MARGIN, area.min.y + HEADER_H));
    let unnamed = app.state().unnamed.clone();
    let mut asks = Asks::default();
    let mut top = ui.new_child(UiBuilder::new().max_rect(header).layout(Layout::left_to_right(Align::Center)));
    top.label(RichText::new(TITLE).font(fonts::semibold(t.title)).color(t.text));
    top.add_space(16.0);
    for ((id, label, command), on) in TOGGLES.iter().zip([unnamed.expanded, unnamed.show_ignored]) {
        if widgets::text_button(&mut top, &t, id, label, on).clicked() {
            asks.command = Some((*command, json!({})));
        }
    }

    let (grid, key) = current_with_key(app, &ctx);
    pictures::begin(app, &ctx);
    let body = Rect::from_min_max(pos2(area.min.x, header.max.y), area.max);
    let mut col = ui.new_child(UiBuilder::new().max_rect(body.shrink2(vec2(MARGIN, 0.0))).layout(Layout::top_down(Align::Min)));
    if grid.is_empty() {
        col.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
    } else {
        grid_body(app, &mut col, &t, (&grid, key), &mut asks);
    }
    suggestions::show(app, &mut col);
    pictures::finish(app, &ctx);
    finish(app, asks);
    paint_drag(app, &ctx);
}

/// The scrolled cards, their tiles and fields, and the rubber band.
fn grid_body(app: &mut App, ui: &mut Ui, t: &Tokens, (grid, key): (&Grid, Option<Key>), asks: &mut Asks) {
    let ctx = ui.ctx().clone();
    let unnamed = app.state().unnamed.clone();
    let width = ui.available_width();
    let laid = laid_out(&ctx, key, grid, unnamed.expanded, width, t.metrics.text_field_h);
    let (cards, height, order) = (&laid.cards, laid.height, &laid.order);
    let selected: HashSet<FaceRef> = unnamed.selection.iter().copied().collect();
    let anchor = unnamed.selection.first().copied();
    let dragged: HashSet<FaceRef> = app.state().drag.as_ref().map(|d| d.faces.iter().copied().collect()).unwrap_or_default();
    let side_px = pictures::side_px(&ctx, FACE_SIDE);
    let people = names_offered(app, &ctx);
    // The wheel scrolls the cards; the bar only shows where they are (a bar that takes drags would
    // be a control without an automation id).
    egui::ScrollArea::vertical().id_salt("faces.grid").auto_shrink([false, false]).scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL).show(
        ui,
        |ui| {
            let (content, bg) = ui.allocate_exact_size(vec2(width, height.max(ui.available_height())), Sense::click_and_drag());
            widgets::register(&ctx, EMPTY_SPACE_ID, Role::Other, "Unnamed faces", Target::Widget(&bg), false);
            let origin = content.min.to_vec2();
            let clip = ui.clip_rect();
            for card in cards.iter() {
                let rect = card.rect.translate(origin);
                if !rect.intersects(clip) {
                    continue;
                }
                paint_card(ui, t, card, rect, clip, &selected);
                if let CardKind::Group(n) = card.kind
                    && let Some(r) = put(
                        ui,
                        Rect::from_min_size(pos2(rect.max.x - CARD_PAD + 4.0 - IGNORE_BUTTON, rect.min.y + 4.0), Vec2::splat(IGNORE_BUTTON)),
                        |ui| widgets::icon_button(ui, t, &format!("group:{n}/ignore"), Icon::Close, IGNORE_BUTTON, false, true, "Ignore these faces"),
                    )
                    && r.clicked()
                {
                    asks.command = Some(("faces.ignoreGroup", json!({ "group": n })));
                }
                let dim = card.kind == CardKind::Ignored;
                for (i, face) in card.faces.iter().enumerate() {
                    let at = card.tile(i).translate(origin);
                    if !at.intersects(clip) {
                        continue;
                    }
                    let id = face.to_string();
                    let picture = picture_of(app, &ctx, *face, side_px);
                    let tile = FaceTile {
                        side: FACE_SIDE,
                        picture,
                        label: FaceLabel::None,
                        selected: selected.contains(face),
                        focused: anchor == Some(*face),
                        dragged: dragged.contains(face),
                        ..FaceTile::new(&id)
                    };
                    let Some(out) = put(ui, at, |ui| {
                        let out = widgets::face_tile(ui, t, &tile);
                        if dim {
                            ui.painter().rect_filled(out.picture, 0, t.excluded_dim);
                        }
                        out
                    }) else {
                        continue;
                    };
                    tile_input(ui, &out.response, *face, &unnamed.selection, order, asks);
                }
                if let CardKind::Group(n) = card.kind {
                    let field = card.field(t).translate(origin);
                    if field.intersects(clip) {
                        name_field(app, ui, t, n, field, &people, asks);
                    }
                }
            }
            band(ui, t, &bg, origin, cards, &unnamed.selection, asks);
        },
    );
}

/// The picture of `face` at `side` device pixels from the shared store, if it has arrived.
pub fn picture_of(app: &App, ctx: &Context, face: FaceRef, side: u32) -> Option<dalica_ui_kit::widgets::FacePicture> {
    let r = dalica_library::FaceRef { photo: face.photo, index: usize::from(face.index) };
    let rect = app.session()?.library().face(r)?.rect;
    pictures::picture(ctx, face, rect, side)
}

/// Runs `add` in a child ui filling `rect`; `None` when `rect` has no area.
fn put<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    if !rect.is_positive() {
        return None;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Min)));
    Some(add(&mut child))
}

/// A card's plate, its count line, and its registration.
fn paint_card(ui: &Ui, t: &Tokens, card: &Card, rect: Rect, clip: Rect, selected: &HashSet<FaceRef>) {
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(t.radius), t.cell, Stroke::new(1.0, t.divider), StrokeKind::Inside);
    let count = count_text(card.faces.len(), card.total);
    let label = match card.kind {
        CardKind::Group(_) => count,
        CardKind::Ignored => format!("{IGNORED_TITLE} · {count}"),
    };
    let line = Rect::from_min_size(rect.min + vec2(CARD_PAD, CARD_PAD), vec2(rect.width() - 2.0 * CARD_PAD - IGNORE_BUTTON, CARD_TOP_H - 4.0));
    let mut job = egui::text::LayoutJob::simple_singleline(label.clone(), fonts::regular(t.count), t.text_dim);
    job.wrap = egui::text::TextWrapping::truncate_at_width(line.width().max(1.0));
    let galley = p.layout_job(job);
    p.galley(pos2(line.min.x, line.center().y - galley.size().y / 2.0).round(), galley, t.text_dim);
    let active = card.faces.iter().any(|f| selected.contains(f));
    widgets::register(ui.ctx(), &card.id(), Role::Other, &label, Target::Painted { rect, clip }, active);
}

/// A tile's clicks, drag and menu.
fn tile_input(ui: &Ui, r: &egui::Response, face: FaceRef, selection: &[FaceRef], order: &[FaceRef], asks: &mut Asks) {
    let mods = ui.input(|i| i.modifiers);
    if r.clicked() {
        asks.selection = Some(clicked(selection, order, face, mods.command, mods.shift));
    }
    if r.secondary_clicked() && !selection.contains(&face) {
        asks.selection = Some(vec![face]);
    }
    if r.drag_started() {
        let faces = if selection.contains(&face) { selection.to_vec() } else { vec![face] };
        asks.drag = Some((face, faces));
    }
    if r.secondary_clicked() || r.context_menu_opened() {
        asks.menu = Some((r.clone(), face));
    }
}

/// The selection after a click on `face`: alone; with Ctrl added or taken away (the face added
/// becomes the anchor); with Shift the faces from the anchor to it in the grid's `order`.
pub fn clicked(selection: &[FaceRef], order: &[FaceRef], face: FaceRef, ctrl: bool, shift: bool) -> Vec<FaceRef> {
    if shift && let Some(anchor) = selection.first().copied() {
        let (a, b) = (order.iter().position(|f| *f == anchor), order.iter().position(|f| *f == face));
        if let (Some(a), Some(b)) = (a, b) {
            let range: Vec<FaceRef> = if a <= b {
                order.get(a..=b).unwrap_or_default().to_vec()
            } else {
                order.get(b..=a).unwrap_or_default().iter().rev().copied().collect()
            };
            return range;
        }
    }
    if ctrl {
        if selection.contains(&face) {
            return selection.iter().copied().filter(|f| *f != face).collect();
        }
        let mut out = vec![face];
        out.extend(selection.iter().copied());
        return out;
    }
    vec![face]
}

/// A group's "Add a name" field with the people offered; Enter names the group.
fn name_field(app: &mut App, ui: &mut Ui, t: &Tokens, n: u32, rect: Rect, people: &[String], asks: &mut Asks) {
    let id = format!("field:faces.name/{n}");
    let before = app.state().unnamed.names.get(&n).cloned().unwrap_or_default();
    let mut text = before.clone();
    let Some(out) = put(ui, rect, |ui| {
        widgets::autocomplete(ui, t, &id, &mut text, people, |ui, text| widgets::text_field(ui, t, &id, text, NAME_HINT, rect.width()))
    }) else {
        return;
    };
    if text != before {
        if let Some((at, _)) = text.char_indices().nth(crate::state::MAX_DRAFT) {
            text.truncate(at);
        }
        let names = &mut app.state_mut().unnamed.names;
        if text.is_empty() {
            names.remove(&n);
        } else if names.len() < crate::state::MAX_GROUP_NAMES || names.contains_key(&n) {
            names.insert(n, text.clone());
        }
    }
    let enter = out.field.ending == Some(widgets::Ending::Return) || out.picked.is_some();
    if enter && !text.trim().is_empty() {
        asks.command = Some(("faces.nameGroup", json!({ "group": n, "name": text })));
    }
}

/// The rubber band: from the empty space, selecting the faces it touches.
fn band(ui: &Ui, t: &Tokens, bg: &egui::Response, origin: Vec2, cards: &[Card], selection: &[FaceRef], asks: &mut Asks) {
    let ctx = ui.ctx();
    let key = Id::new("faces.band");
    let ctrl = ui.input(|i| i.modifiers.command);
    if bg.clicked() && !ctrl {
        asks.selection = Some(Vec::new());
    }
    if bg.drag_started()
        && let Some(at) = ctx.input(|i| i.pointer.press_origin())
    {
        let base = if ctrl { selection.to_vec() } else { Vec::new() };
        ctx.data_mut(|d| d.insert_temp(key, Band { start: at - origin, base }));
    }
    let Some(b) = ctx.data(|d| d.get_temp::<Band>(key)) else { return };
    if !bg.dragged() {
        ctx.data_mut(|d| d.remove::<Band>(key));
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
    let rect = Rect::from_two_pos(b.start + origin, pos);
    let mut picked = b.base.clone();
    let mut seen: HashSet<FaceRef> = picked.iter().copied().collect();
    let content = rect.translate(-origin);
    for card in cards.iter().filter(|c| c.rect.intersects(content)) {
        for (i, face) in card.faces.iter().enumerate() {
            if card.tile(i).intersects(content) && seen.insert(*face) {
                picked.push(*face);
            }
        }
    }
    asks.selection = Some(picked);
    let p = ui.painter();
    p.rect(rect, CornerRadius::ZERO, t.accent.gamma_multiply(0.15), Stroke::new(1.0, t.accent), StrokeKind::Inside);
}

/// Runs what the pass asked for, once the library is no longer borrowed.
fn finish(app: &mut App, asks: Asks) {
    if let Some(selection) = asks.selection {
        app.state_mut().unnamed.selection = selection;
    }
    if let Some((face, faces)) = asks.drag {
        crate::drag::start_faces(app, face.widget(), faces);
    }
    if let Some((response, _)) = &asks.menu {
        menu::show(response, app);
    }
    if let Some((command, params)) = asks.command {
        // A refusal's reason is on the status line.
        let _ = app.run(command, &params);
    }
}

/// The drag pill of a drag of faces: "3 faces", or over a person "Name 3 faces Ana", with the first
/// face's picture.
pub fn paint_drag(app: &App, ctx: &Context) {
    let Some(drag) = app.state().drag.as_ref().filter(|d| !d.faces.is_empty()) else { return };
    if !ctx.input(|i| i.pointer.primary_down()) {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()).filter(|p| p.is_finite()) else { return };
    let name = match drag.at {
        Some(DropAt::Person(key)) => key.id().and_then(|id| app.session()?.library().person(&id).map(|p| p.name.clone())),
        _ => None,
    };
    let op = if drag.op == Some(DragOp::Name) { widgets::DragOp::NameFaces } else { widgets::DragOp::Faces };
    let side = pictures::side_px(ctx, FACE_SIDE);
    let thumb = drag.faces.first().and_then(|f| picture_of(app, ctx, *f, side)).map(|p| PillThumb { texture: p.texture, size: vec2(1.0, 1.0) });
    let pill = DragPill { op, count: drag.faces.len(), target: name.as_deref(), thumb };
    widgets::drag_pill(ctx, app.tokens(), crate::drag::PILL_ID, pos, &pill);
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;
    use model::Group;

    fn f(p: u64, i: u16) -> FaceRef {
        FaceRef { photo: PhotoId::new(p), index: i }
    }

    #[test]
    fn clicks_select_alone_toggle_with_ctrl_and_range_with_shift() {
        let order = [f(1, 0), f(2, 0), f(3, 0), f(4, 0)];
        assert_eq!(clicked(&[f(1, 0)], &order, f(3, 0), false, false), [f(3, 0)]);
        assert_eq!(clicked(&[f(1, 0)], &order, f(3, 0), true, false), [f(3, 0), f(1, 0)]);
        assert_eq!(clicked(&[f(3, 0), f(1, 0)], &order, f(3, 0), true, false), [f(1, 0)]);
        assert_eq!(clicked(&[f(2, 0)], &order, f(4, 0), false, true), [f(2, 0), f(3, 0), f(4, 0)]);
        assert_eq!(clicked(&[f(3, 0)], &order, f(1, 0), false, true), [f(3, 0), f(2, 0), f(1, 0)]);
        assert_eq!(clicked(&[], &order, f(1, 0), false, true), [f(1, 0)], "no anchor: alone");
    }

    #[test]
    fn cards_flow_in_rows_and_groups_show_one_row_until_expanded() {
        let group = |n: u64, len: u16| Group { faces: (0..len).map(|i| f(n, i)).collect() };
        let grid = Grid { groups: vec![group(1, 6), group(2, 2), group(3, 1)], ignored: vec![f(9, 0)] };
        let (cards, height) = layout(&grid, false, 700.0, 24.0);
        assert_eq!(cards.iter().map(|c| (c.faces.len(), c.total)).collect::<Vec<_>>(), [(4, 6), (2, 2), (1, 1), (1, 1)]);
        assert_eq!(cards[0].rect.min, Pos2::ZERO);
        assert_eq!(cards[1].rect.min.y, 0.0, "the second fits beside the first");
        assert!(cards[2].rect.min.y > 0.0 && cards[2].rect.min.x == 0.0, "the third wraps");
        assert!(cards[3].rect.min.x == 0.0 && cards[3].rect.min.y > cards[2].rect.min.y, "the ignored card on its own row");
        assert_eq!(cards[3].kind, CardKind::Ignored);
        assert!(height >= cards[3].rect.max.y);
        let (open, _) = layout(&grid, true, 700.0, 24.0);
        assert_eq!(open[0].faces.len(), 6);
        assert!(open[0].rect.height() > cards[0].rect.height(), "two rows");
        assert_eq!(count_text(4, 6), "4 of 6 faces");
        assert_eq!(count_text(1, 1), "1 face");
        // No room: one card a row, never a panic.
        let (narrow, _) = layout(&grid, false, f32::NAN, 24.0);
        assert!(narrow.iter().all(|c| c.rect.min.x == 0.0));
    }
}
