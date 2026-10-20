//! The import tray (`organizing.md` §3 item 2): the left of the Import room, under "Import
//! from:".
//!
//! - **The status line** ([`STATUS_ID`]): the engine's ("Loading 112 of 340 files…", "340
//!   files", "No pictures found") and, once anything is listed, how many will be imported; at its
//!   right the **"Exclude duplicates"** checkbox ([`EXCLUDE_ID`], tooltip "Skip pictures that are
//!   already in your library"), which starts from Options ▸ General ▸ Detect duplicates while
//!   importing and runs `import.options`.
//! - **The thumbnails**, appearing as the listing finds them, in capture order and grouped where
//!   capture times are more than two hours apart: each group under ui-kit's group header
//!   ([`group_id`]), whose clock selects the whole group. A cell ([`cell_id`]) shows the thumbnail
//!   turned as the room's rotation says, the star badge when starred, and when it will not be
//!   imported (excluded, or a duplicate "Exclude duplicates" leaves out) is dimmed under a `danger`
//!   ⊘ (`<cell>/excluded`). A click selects one, Ctrl-click toggles, Shift-click runs a range from
//!   the focused item (`import.select`); a click on empty space selects nothing. The context menu
//!   offers Exclude, Include, Exclude All and Include All (the engine's `import.exclude`,
//!   `import.excludeAll`).
//! - **Scrolling**: the wheel, and a bar at the right while the thumbnails do not fit
//!   (`scroll:import`, `scroll:import/thumb`, the tree's bar's look); the item an arrow reaches is
//!   scrolled into view.
//!
//! Only the rows in view are drawn; the layout is a function of the groups' sizes and the width
//! ([`grid`]). The columns as last drawn are what Up and Down step by ([`columns`]).

use std::cell::Cell;
use std::collections::HashSet;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, CornerRadius, Id, Layout, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::{Action, Snapshot, Span, Textures, group_title, one_line, paint_turned, pictures, turned};
use crate::app::App;
use crate::menu_catalog::Mark;
use crate::menus::{ItemNode, Node};
use crate::panels::folders::scrollbar;
use crate::panels::thumbnails::fit_within;
use crate::{tl, tln};

/// The status line's height.
pub const STATUS_H: f32 = 36.0;
/// A cell's edge.
pub const CELL: f32 = 124.0;
/// The space between cells.
pub const GAP: f32 = 6.0;
/// The space left and right of the cells.
pub const PAD: f32 = 12.0;
/// The space between a cell's edge and its thumbnail.
pub const IMAGE_INSET: f32 = 8.0;
/// The space after a group's last row.
pub const GROUP_GAP: f32 = 10.0;
/// The space above the first group.
pub const TOP: f32 = 2.0;

/// The status line's automation id (its label is the text shown).
pub const STATUS_ID: &str = "text:import.status";
/// "Exclude duplicates".
pub const EXCLUDE_ID: &str = "check:import.excludeDuplicates";
pub const EXCLUDE_LABEL: &str = "Exclude duplicates";
pub const EXCLUDE_TIP: &str = "Skip pictures that are already in your library";
/// The tray's empty space.
pub const BACKGROUND_ID: &str = "importTray";
/// The scroll bar's track and thumb.
pub const TRACK_ID: &str = "scroll:import";
pub const THUMB_ID: &str = "scroll:import/thumb";
/// The ⊘'s label: a duplicate left out, or an excluded picture.
pub const DUPLICATE: &str = "Already in your library";
pub const EXCLUDED: &str = "Excluded";

/// A label, tooltip, menu row or reason of the tray in the language in effect (the constants keep
/// the English, which the catalogs key on).
pub fn text(english: &str) -> String {
    match english {
        "Exclude duplicates" => tl!("Exclude duplicates"),
        "Skip pictures that are already in your library" => tl!("Skip pictures that are already in your library"),
        "Already in your library" => tl!("Already in your library"),
        "Excluded" => tl!("Excluded"),
        "Import tray" => tl!("Import tray"),
        "Scroll the import tray" => tl!("Scroll the import tray"),
        "Import tray position" => tl!("Import tray position"),
        "Exclude" => tl!(ctx = "import", "Exclude"),
        "Include" => tl!(ctx = "import", "Include"),
        "Exclude All" => tl!("Exclude All"),
        "Include All" => tl!("Include All"),
        "Every picture chosen is excluded." => tl!("Every picture chosen is excluded."),
        "No picture chosen is excluded." => tl!("No picture chosen is excluded."),
        other => crate::i18n::tr(other),
    }
}

thread_local! {
    /// The columns of the tray as last drawn on this thread (the UI thread), for Up and Down.
    static COLUMNS: Cell<usize> = const { Cell::new(1) };
}

/// The tray's columns as last drawn (1 before it is).
pub fn columns() -> usize {
    COLUMNS.with(Cell::get).max(1)
}

/// A cell's automation id.
pub fn cell_id(id: u64) -> String {
    format!("candidate:{id}")
}

/// Group `n`'s header (its clock is `<id>/select`).
pub fn group_id(n: usize) -> String {
    format!("group:import/{n}")
}

/// What the tray keeps between frames.
#[derive(Debug, Default)]
pub struct TrayMemory {
    /// How far the thumbnails are scrolled (points).
    pub offset: f32,
    /// While the bar's thumb is held: the offset and the pointer's y when it was pressed.
    held: Option<(f32, f32)>,
    /// The item in the preview last frame: one reached since is scrolled into view.
    last_preview: Option<u64>,
}

/// A group placed in the tray's content (y from the content's top).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub header: f32,
    pub rows: f32,
    pub span: Span,
}

/// The tray's content laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub cols: usize,
    pub groups: Vec<Placed>,
    pub height: f32,
}

/// How many cells fit across `width`.
pub fn cols_for(width: f32) -> usize {
    let n = ((width - 2.0 * PAD + GAP) / (CELL + GAP)).floor();
    if n.is_finite() && n >= 1.0 { n as usize } else { 1 }
}

/// The groups `spans` laid out `width` wide with headers `header_h` high: each header, then its
/// rows, then [`GROUP_GAP`].
pub fn grid(width: f32, spans: &[Span], header_h: f32) -> Grid {
    let cols = cols_for(width);
    let mut y = TOP;
    let groups = spans
        .iter()
        .map(|&span| {
            let header = y;
            let count = span.end.saturating_sub(span.start);
            let rows = count.div_ceil(cols) as f32;
            y += header_h + rows * (CELL + GAP) + GROUP_GAP;
            Placed { header, rows: header + header_h, span }
        })
        .collect();
    Grid { cols, groups, height: y + PAD }
}

impl Grid {
    /// Candidate `index`'s cell in the content (x from the content's left).
    pub fn cell(&self, index: usize) -> Option<Rect> {
        let g = self.groups.iter().find(|g| (g.span.start..g.span.end).contains(&index))?;
        let k = index - g.span.start;
        let (row, col) = (k / self.cols, k % self.cols);
        Some(Rect::from_min_size(pos2(PAD + col as f32 * (CELL + GAP), g.rows + row as f32 * (CELL + GAP)), vec2(CELL, CELL)))
    }
}

/// The offset that brings `cell` (content coordinates) into a view `view_h` high scrolled to
/// `offset`, moving as little as it can.
pub fn reveal(offset: f32, view_h: f32, cell: Rect) -> f32 {
    if cell.min.y - GAP < offset {
        (cell.min.y - GAP).max(0.0)
    } else if cell.max.y + GAP > offset + view_h {
        cell.max.y + GAP - view_h
    } else {
        offset
    }
}

/// A context menu to show on a cell: its response and the items it acts on.
pub struct Menu {
    response: Response,
    targets: Vec<u64>,
}

/// Draws the tray into `ui`'s rect; returns the context menu to show once the actions ran.
pub fn show(
    app: &App,
    ui: &mut Ui,
    t: &Tokens,
    mem: &mut TrayMemory,
    textures: &mut Textures,
    snap: &Snapshot,
    actions: &mut Vec<Action>,
) -> Option<Menu> {
    let session = app.session()?;
    let rect = ui.max_rect();
    let status = Rect::from_min_max(rect.min, pos2(rect.max.x, (rect.min.y + STATUS_H).min(rect.max.y)));
    status_line(ui, t, status, snap, actions);

    // The thumbnails' area, and the bar when they do not fit.
    let area = Rect::from_min_max(pos2(rect.min.x, status.max.y), rect.max);
    let header_h = t.metrics.group_header_h;
    let mut g = grid(area.width(), &snap.groups, header_h);
    let strip = scrollbar::strip(area, g.height);
    if strip.is_some() {
        g = grid(area.width() - scrollbar::WIDTH, &snap.groups, header_h);
    }
    let view = strip.map_or(area, |s| Rect::from_min_max(area.min, pos2(s.min.x, area.max.y)));
    COLUMNS.with(|c| c.set(g.cols));
    let view_h = view.height().max(0.0);
    let max = (g.height - view_h).max(0.0);

    if ui.rect_contains_pointer(area) {
        mem.offset -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    let room = &app.state().import;
    if room.preview != mem.last_preview
        && let Some(cell) = room.preview.and_then(|id| snap.find(id)).and_then(|(i, _)| g.cell(i))
    {
        mem.offset = reveal(mem.offset, view_h, cell);
    }
    mem.last_preview = room.preview;
    let bar = strip.map(|s| bar_input(ui, mem, s, view_h, g.height));
    mem.offset = if mem.offset.is_finite() { mem.offset.clamp(0.0, max) } else { 0.0 };
    let offset = mem.offset;

    let mut grid_ui = ui.new_child(UiBuilder::new().max_rect(view));
    grid_ui.set_clip_rect(view.intersect(ui.clip_rect()));
    let bg = grid_ui.interact(view, Id::new(BACKGROUND_ID), Sense::click());
    widgets::register(ui.ctx(), BACKGROUND_ID, Role::Other, &text("Import tray"), Target::Widget(&bg), false);
    if bg.clicked() {
        actions.push(("import.select", json!({ "ids": [] })));
    }

    let selected: HashSet<u64> = room.selection.iter().copied().collect();
    let mods = ui.input(|i| i.modifiers);
    let ctx = ui.ctx().clone();
    let painter = grid_ui.painter().clone();
    let origin = pos2(view.min.x, view.min.y - offset);
    let mut menu = None;
    for (n, placed) in g.groups.iter().enumerate() {
        let span = placed.span;
        let rows_h = placed.rows - placed.header + (span.end - span.start).div_ceil(g.cols) as f32 * (CELL + GAP);
        if placed.header + rows_h < offset || placed.header > offset + view_h {
            continue;
        }
        let members = snap.candidates.get(span.start..span.end).unwrap_or_default();
        // The header and its clock.
        let header =
            Rect::from_min_size(pos2(view.min.x + PAD - 4.0, origin.y + placed.header), vec2((view.width() - 2.0 * PAD + 8.0).max(0.0), header_h));
        if header.intersects(view) {
            let (first, last) = (members.first(), members.last());
            let title = match (first, last) {
                (Some(a), Some(b)) => group_title(a.captured, b.captured),
                _ => String::new(),
            };
            let all = !members.is_empty() && members.iter().all(|c| selected.contains(&c.id));
            let mut hui = grid_ui.new_child(UiBuilder::new().max_rect(header));
            hui.set_clip_rect(view.intersect(ui.clip_rect()));
            let out = widgets::group_header(&mut hui, t, &group_id(n), &title, &pictures(members.len()), all);
            if out.select {
                let ids: Vec<u64> = members.iter().map(|c| c.id).collect();
                actions.push(("import.select", json!({ "ids": ids })));
            }
        }
        // The cells in view.
        for (k, c) in members.iter().enumerate() {
            let index = span.start + k;
            let Some(cell) = g.cell(index).map(|r| r.translate(origin.to_vec2())) else { continue };
            if !cell.intersects(view) {
                continue;
            }
            let r = grid_ui.interact(cell, Id::new(("import.cell", c.id)), Sense::click());
            let is_selected = selected.contains(&c.id);
            if is_selected {
                painter.rect_filled(cell, 0, t.cell_selected);
            } else if r.hovered() {
                painter.rect_filled(cell, 0, t.hover.gamma_multiply(0.5));
            }
            let area = cell.shrink(IMAGE_INSET);
            let image = match textures.get(&ctx, session, c.id) {
                Some((tex, [w, h])) => {
                    let (dw, dh) = turned(w, h, c.turns);
                    let image = fit_within(area, dw, dh);
                    widgets::photo_shadow(&painter, image);
                    paint_turned(&painter, image, tex, c.turns);
                    image
                }
                None => {
                    painter.rect_filled(area, 0, t.placeholder);
                    area
                }
            };
            if is_selected {
                let color = if room.focus == Some(c.id) { t.photo_focus } else { t.photo_selected };
                painter.rect_stroke(image, 0, Stroke::new(2.0, color), StrokeKind::Outside);
            }
            let id = cell_id(c.id);
            if snap.left_out(index)
                && let Some(mark) = widgets::excluded_mark(&painter, t, cell, image)
            {
                let why = text(if c.excluded { EXCLUDED } else { DUPLICATE });
                widgets::register(&ctx, &format!("{id}/excluded"), Role::Badge, &why, Target::Painted { rect: mark, clip: view }, true);
            }
            if c.starred {
                widgets::star_badge(&painter, t, &format!("{id}/star"), image);
            }
            widgets::register(&ctx, &id, Role::Thumbnail, &c.name, Target::Widget(&r), is_selected);
            if r.clicked() {
                let mode = if mods.command {
                    "toggle"
                } else if mods.shift {
                    "range"
                } else {
                    "replace"
                };
                actions.push(("import.select", json!({ "ids": [c.id], "mode": mode })));
            }
            if r.secondary_clicked() && !is_selected {
                actions.push(("import.select", json!({ "ids": [c.id] })));
            }
            if r.secondary_clicked() || r.context_menu_opened() {
                let targets = if is_selected { room.selection.clone() } else { vec![c.id] };
                menu = Some(Menu { response: r, targets });
            }
        }
    }
    if let (Some(s), Some(f)) = (strip, &bar) {
        bar_paint(ui, t, f, s, view_h, g.height, offset);
    }
    menu
}

/// The status line and "Exclude duplicates" in `rect`.
fn status_line(ui: &mut Ui, t: &Tokens, rect: Rect, snap: &Snapshot, actions: &mut Vec<Action>) {
    let p = ui.painter().clone();
    p.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, t.divider));
    let mut on = snap.exclude_duplicates;
    let check = ui
        .scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(PAD, 0.0))).layout(Layout::right_to_left(Align::Center)), |ui| {
            let label = text(EXCLUDE_LABEL);
            let r = widgets::checkbox(ui, t, EXCLUDE_ID, &label, &mut on);
            widgets::tooltip(r, t, &label, Some(&text(EXCLUDE_TIP)))
        })
        .inner;
    if check.changed() {
        actions.push(("import.options", json!({ "excludeDuplicates": on })));
    }
    let line = status_text(&snap.status, (!snap.candidates.is_empty()).then(|| snap.to_import()));
    let room = (check.rect.min.x - rect.min.x - PAD - 12.0).max(1.0);
    let g = one_line(&p, &line, fonts::regular(t.body), t.text_label, room);
    let at = pos2(rect.min.x + PAD, rect.center().y - g.size().y / 2.0).round();
    let shown = Rect::from_min_size(at, g.size());
    p.galley(at, g, t.text_label);
    widgets::register(ui.ctx(), STATUS_ID, Role::Other, &line, Target::Painted { rect: shown, clip: rect }, false);
}

/// The status line: what the listing says (`import.list`'s status, its counts written in the
/// language in effect), then how many of the pictures will be imported when any are listed.
pub fn status_text(status: &str, to_import: Option<usize>) -> String {
    let status = listing_status(status);
    match to_import {
        Some(n) => tln!(n, "{status} \u{b7} {n} to import", "{status} \u{b7} {n} to import", status = status),
        None => status,
    }
}

/// `import.list`'s status line (`dalica_engine::import`'s) in the language in effect: its counts
/// by the numbers in it, a reason after "cannot be read:" as the source gave it, any other text
/// looked up whole by its English.
fn listing_status(status: &str) -> String {
    // A count as the engine writes it (`1,234`).
    let count = |s: &str| s.replace(',', "").parse::<u64>().ok();
    if let Some(n) = status.strip_suffix(" files").or_else(|| status.strip_suffix(" file")).and_then(count) {
        return tln!(n, "{n} file", "{n} files");
    }
    if let Some((done, total)) = status
        .strip_prefix("Loading ")
        .and_then(|r| r.strip_suffix(" files\u{2026}"))
        .and_then(|r| r.split_once(" of "))
        .and_then(|(d, t)| Some((count(d)?, count(t)?)))
    {
        let done = crate::i18n::format::number(done);
        return tln!(total, "Loading {done} of {n} file\u{2026}", "Loading {done} of {n} files\u{2026}", done = done);
    }
    if let Some(reason) = status.strip_prefix("The source cannot be read: ") {
        return tl!("The source cannot be read: {reason}", reason = crate::i18n::tr(reason));
    }
    crate::i18n::tr(status)
}

/// This frame's interaction with the bar.
struct Bar {
    track: Response,
    thumb: Response,
}

/// Before the cells: the bar's thumb dragged and a press in its track, applied to the offset.
fn bar_input(ui: &mut Ui, mem: &mut TrayMemory, strip: Rect, view_h: f32, content_h: f32) -> Bar {
    let th = scrollbar::thumb(strip, view_h, content_h, mem.offset);
    let track = ui.interact(strip, Id::new(TRACK_ID), Sense::click());
    let thumb = ui.interact(th, Id::new(THUMB_ID), Sense::drag());
    let pointer_y = ui.input(|i| i.pointer.interact_pos()).map(|p| p.y);
    if thumb.drag_started()
        && let Some(y) = pointer_y
    {
        mem.held = Some((mem.offset, y));
    }
    match (mem.held, thumb.dragged() || thumb.drag_started(), pointer_y) {
        (Some((from, y0)), true, Some(y)) => mem.offset = scrollbar::dragged(strip, view_h, content_h, from, y - y0),
        (_, false, _) => mem.held = None,
        _ => {}
    }
    if track.clicked()
        && mem.held.is_none()
        && let Some(p) = track.interact_pointer_pos()
        && !th.contains(p)
    {
        mem.offset = scrollbar::paged(strip, view_h, content_h, mem.offset, p.y);
    }
    Bar { track, thumb }
}

/// After the cells: the bar at `offset`, and its ids.
fn bar_paint(ui: &Ui, t: &Tokens, f: &Bar, strip: Rect, view_h: f32, content_h: f32, offset: f32) {
    let painter = ui.painter().with_clip_rect(strip.intersect(ui.clip_rect()));
    painter.rect_filled(strip, 0, t.inset);
    let th = scrollbar::thumb(strip, view_h, content_h, offset);
    let active = f.thumb.hovered() || f.thumb.is_pointer_button_down_on() || f.thumb.dragged();
    let pill = th.shrink2(vec2(2.0, 2.0));
    if pill.is_positive() {
        painter.rect_filled(pill, CornerRadius::same(3), if active { t.thumb } else { t.track });
    }
    widgets::register(ui.ctx(), TRACK_ID, Role::ScrollTrack, "Scroll the import tray", Target::Widget(&f.track), f.track.is_pointer_button_down_on());
    widgets::register(ui.ctx(), THUMB_ID, Role::ScrollThumb, "Import tray position", Target::Widget(&f.thumb), f.thumb.dragged());
}

/// A row of the tray's context menu.
fn row(id: &'static str, label: &'static str, part: &str, params: serde_json::Value, enabled: bool, reason: &str) -> Node {
    Node::Item(ItemNode {
        id,
        label: text(label),
        catalog_label: label,
        params,
        widget: if part.is_empty() { format!("menuitem:{id}") } else { format!("menuitem:{id}/{part}") },
        shortcut: None,
        live: true,
        enabled,
        reason: (!enabled).then(|| text(reason)),
        mark: Mark::None,
        checked: false,
        dialog: None,
    })
}

/// The context menu's rows for `targets`: Exclude and Include (each enabled while one of them can
/// change that way), Exclude All and Include All.
pub fn menu_rows(snap: &Snapshot, targets: &[u64]) -> Vec<Node> {
    let excluded = |id: &u64| snap.find(*id).is_some_and(|(_, c)| c.excluded);
    let (some_in, some_out) = (targets.iter().any(|id| !excluded(id)), targets.iter().any(excluded));
    vec![
        row("import.exclude", "Exclude", "", json!({ "ids": targets, "on": true }), some_in, "Every picture chosen is excluded."),
        row("import.exclude", "Include", "include", json!({ "ids": targets, "on": false }), some_out, "No picture chosen is excluded."),
        Node::Separator,
        row("import.excludeAll", "Exclude All", "", json!({ "on": true }), true, ""),
        row("import.excludeAll", "Include All", "include", json!({ "on": false }), true, ""),
    ]
}

/// Shows the context menu `menu` asked for this frame; a row clicked runs its command.
pub fn context_menu(app: &mut App, snap: &Snapshot, menu: Menu) {
    let nodes = menu_rows(snap, &menu.targets);
    crate::menus::context_menu(&menu.response, app, &nodes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_stack_their_rows_under_their_headers() {
        // 1400 wide less the preview: 910 points hold 6 cells.
        assert_eq!(cols_for(910.0), 6);
        assert_eq!(cols_for(2.0 * PAD + CELL), 1);
        assert_eq!(cols_for(f32::NAN), 1);
        let spans = [Span { start: 0, end: 7 }, Span { start: 7, end: 8 }];
        let g = grid(2.0 * PAD + 3.0 * CELL + 2.0 * GAP, &spans, 30.0);
        assert_eq!(g.cols, 3);
        let row = CELL + GAP;
        assert_eq!(g.groups[0].header, TOP);
        assert_eq!(g.groups[1].header, TOP + 30.0 + 3.0 * row + GROUP_GAP);
        assert_eq!(g.height, g.groups[1].header + 30.0 + row + GROUP_GAP + PAD);
        // Candidate 4: the first group's second row, second column; 7 opens the second group.
        assert_eq!(g.cell(4).unwrap().min, pos2(PAD + row, TOP + 30.0 + row));
        assert_eq!(g.cell(7).unwrap().min, pos2(PAD, g.groups[1].rows));
        assert_eq!(g.cell(8), None);
    }

    #[test]
    fn revealing_a_cell_moves_the_view_as_little_as_it_can() {
        let cell = |y: f32| Rect::from_min_size(pos2(PAD, y), vec2(CELL, CELL));
        assert_eq!(reveal(100.0, 400.0, cell(200.0)), 100.0, "already in view");
        assert_eq!(reveal(300.0, 400.0, cell(200.0)), 200.0 - GAP, "above: to its top");
        assert_eq!(reveal(0.0, 400.0, cell(500.0)), 500.0 + CELL + GAP - 400.0, "below: to its bottom");
        assert_eq!(reveal(50.0, 400.0, cell(2.0)), 0.0);
    }
}
