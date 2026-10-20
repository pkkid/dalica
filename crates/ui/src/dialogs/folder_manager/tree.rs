//! The Folder Manager's folder tree: its rows, flattened from the roots and the expanded folders'
//! listings, and drawn in a well that scrolls with the wheel (only the rows in view are laid out).
//! Each row: disclosure triangle, state icon, folder icon (a root's own icon), name.
//!
//! **Keyboard** (`crate::a11y::keyboard`): the tree is one Tab stop, its *current* row (the row
//! last focused or clicked, else the selected one, else the first), brought into view as it gains
//! the focus. With it focused, ↑/↓ go to the previous or next folder, Home and End to the first and
//! last, → expands a folder (expanded: goes to its first subfolder), ← collapses it (collapsed:
//! goes to its parent), Enter and Space expand or collapse it; the selection follows the focus.
//! For a screen reader each row is a tree item with its level, whether it is expanded and its place
//! among its siblings.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_ui_kit::icons::{self, FolderIcon, paint_folder, state_color};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Icon, Tokens, fonts};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CornerRadius, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::draft::Draft;
use super::listing::{self, Listing, Root};
use super::{State, expand_id, row_id};
use crate::app::App;
use crate::tl;

/// The most rows the tree lists, and the deepest it goes.
const MAX_ROWS: usize = 100_000;
const MAX_DEPTH: usize = 64;
/// The disclosure triangle's glyph and the gaps between a row's parts (points).
const DISCLOSURE: f32 = 12.0;
const LEFT: f32 = 4.0;
const GAP: f32 = 4.0;

/// What a row is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A folder: expanded or not, and whether it is known to have no subfolders.
    Folder { open: bool, leaf: bool },
    /// Its parent's subfolders are being read: a spinner.
    Loading,
    /// Its parent could not be read.
    Failed,
    /// Its parent has more subfolders than were listed.
    More,
}

/// One row of the tree.
#[derive(Clone, Debug)]
pub struct Row {
    /// The root it is under.
    pub root: Arc<PathBuf>,
    /// The folder (for Loading, Failed and More rows: the parent's).
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    /// A root's icon; other folders show the folder icon.
    pub icon: Option<FolderIcon>,
    pub kind: Kind,
}

impl Row {
    /// Its automation id (`choice:dialog.folder/…`; a spinner is `…/loading` under its parent's).
    pub fn id(&self) -> String {
        let row = row_id(&self.root, &self.path);
        match self.kind {
            Kind::Loading => format!("{row}/loading"),
            _ => row,
        }
    }
}

/// The rows: every root, and under each expanded folder its subfolders (a spinner row while they
/// are read). Asks for the listings it needs.
pub fn rows(app: &mut App, ctx: &egui::Context, draft: &Draft, roots: &[Root]) -> Vec<Row> {
    let expanded: HashSet<&Path> = draft.expanded.iter().map(Path::new).collect();
    let mut out = Vec::new();
    let mut walk = Walk { app, ctx, expanded: &expanded, out: &mut out };
    for root in roots {
        let base = Arc::new(root.path.clone());
        walk.visit(&base, &root.path, &root.name, Some(root.icon), 0);
    }
    out
}

/// One flattening of the tree.
struct Walk<'a> {
    app: &'a mut App,
    ctx: &'a egui::Context,
    expanded: &'a HashSet<&'a Path>,
    out: &'a mut Vec<Row>,
}

impl Walk<'_> {
    fn visit(&mut self, root: &Arc<PathBuf>, path: &Path, name: &str, icon: Option<FolderIcon>, depth: usize) {
        let Walk { app, ctx, expanded, out } = self;
        if out.len() >= MAX_ROWS {
            return;
        }
        let open = depth < MAX_DEPTH && expanded.contains(path);
        let row = |kind: Kind, depth: usize| Row { root: Arc::clone(root), path: path.to_path_buf(), name: name.to_string(), depth, icon, kind };
        if !open {
            let leaf = matches!(listing::peek(ctx, Some(path)), Some(Listing::Children(c)) if c.folders.is_empty() && !c.truncated);
            out.push(row(Kind::Folder { open: false, leaf }, depth));
            return;
        }
        match listing::get(app, ctx, Some(path)) {
            Listing::Children(c) => {
                out.push(row(Kind::Folder { open: true, leaf: c.folders.is_empty() && !c.truncated }, depth));
                for child in &c.folders {
                    self.visit(root, &child.path, &child.name, None, depth + 1);
                }
                if c.truncated {
                    self.out.push(row(Kind::More, depth + 1));
                }
            }
            Listing::Pending | Listing::Roots(_) => {
                out.push(row(Kind::Folder { open: true, leaf: false }, depth));
                out.push(row(Kind::Loading, depth + 1));
            }
            Listing::Failed(_) => {
                out.push(row(Kind::Folder { open: true, leaf: false }, depth));
                out.push(row(Kind::Failed, depth + 1));
            }
        }
    }
}

/// The keyboard's place in the tree between frames: the current row (its index and id, the index
/// checked against the id each frame) and the well's scroll offset and height as last drawn.
#[derive(Clone, Debug, Default)]
struct Nav {
    current: Option<(usize, String)>,
    keyed: bool,
    offset: f32,
    view_h: f32,
}

fn nav_id() -> Id {
    Id::new("dialog:folderManager/tree/nav")
}

/// Forgets the keyboard's place: the dialog has just opened.
pub fn clear(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Nav>(nav_id()));
}

fn is_folder(rows: &[Row], i: usize) -> bool {
    rows.get(i).is_some_and(|r| matches!(r.kind, Kind::Folder { .. }))
}

/// Where `key` takes the keyboard from row `at`: the row it goes to, and whether it expands or
/// collapses `at`.
fn step(rows: &[Row], at: usize, key: Key) -> (Option<usize>, bool) {
    let n = rows.len();
    let Some(row) = rows.get(at) else { return (None, false) };
    let (open, leaf) = match row.kind {
        Kind::Folder { open, leaf } => (open, leaf),
        _ => (false, true),
    };
    match key {
        Key::ArrowDown => ((at.saturating_add(1)..n).find(|i| is_folder(rows, *i)), false),
        Key::ArrowUp => ((0..at.min(n)).rev().find(|i| is_folder(rows, *i)), false),
        Key::Home => ((0..n).find(|i| is_folder(rows, *i)), false),
        Key::End => ((0..n).rev().find(|i| is_folder(rows, *i)), false),
        Key::ArrowRight if !open && !leaf => (None, true),
        Key::ArrowRight if open => (Some(at + 1).filter(|i| is_folder(rows, *i) && rows.get(*i).is_some_and(|c| c.depth > row.depth)), false),
        Key::ArrowLeft if open => (None, true),
        Key::ArrowLeft => ((0..at.min(n)).rev().find(|i| is_folder(rows, *i) && rows.get(*i).is_some_and(|p| p.depth < row.depth)), false),
        Key::Enter | Key::Space => (None, !leaf),
        _ => (None, false),
    }
}

/// Each row's place among its siblings (1-based position, set size): the folders at its depth under
/// the same parent. Spinner and message rows are in no set.
fn sibling_sets(rows: &[Row]) -> Vec<(usize, usize)> {
    let mut out = vec![(0, 0); rows.len()];
    let mut open: Vec<Vec<usize>> = Vec::new();
    let close = |out: &mut Vec<(usize, usize)>, set: Vec<usize>| {
        let size = set.len();
        for (k, i) in set.into_iter().enumerate() {
            if let Some(p) = out.get_mut(i) {
                *p = (k + 1, size);
            }
        }
    };
    for (i, row) in rows.iter().enumerate() {
        if !matches!(row.kind, Kind::Folder { .. }) {
            continue;
        }
        let depth = row.depth.min(MAX_DEPTH);
        while open.len() > depth + 1 {
            if let Some(set) = open.pop() {
                close(&mut out, set);
            }
        }
        open.resize_with(depth + 1, Vec::new);
        if let Some(set) = open.get_mut(depth) {
            set.push(i);
        }
    }
    for set in open {
        close(&mut out, set);
    }
    out
}

/// What the user did in the tree this frame (indices into the rows).
#[derive(Debug, Default)]
pub struct Out {
    pub clicked: Option<usize>,
    pub toggled: Option<usize>,
}

/// A one-line galley of `text` cut with an ellipsis to `max_w`.
pub fn one_line(ui: &Ui, text: &str, font: egui::FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Where a row at `depth` drawn in `row` paints its state icon (after its disclosure triangle).
pub fn state_rect(t: &Tokens, row: Rect, depth: usize) -> Rect {
    let x = row.min.x + LEFT + depth as f32 * t.metrics.tree_indent + DISCLOSURE + GAP;
    let side = t.metrics.icon_menu;
    Rect::from_min_size(pos2(x, row.center().y - side / 2.0), vec2(side, side))
}

/// The well the tree and the watched list draw in.
pub fn well(ui: &Ui, t: &Tokens, rect: Rect) {
    ui.painter().rect(rect, CornerRadius::same(t.radius), t.field, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
}

/// Draws the rows in `rect` (a well), the row at `scroll_to` brought into view; `state(path)`
/// gives a folder's effective state and whether it is inherited.
pub fn show(
    ui: &mut Ui,
    t: &Tokens,
    rect: Rect,
    rows: &[Row],
    selected: Option<&str>,
    scroll_to: Option<usize>,
    state: impl Fn(&Path) -> (State, bool),
) -> Out {
    well(ui, t, rect);
    let mut out = Out::default();
    let row_h = t.metrics.tree_row_h;
    let inner = rect.shrink(1.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let mut area = egui::ScrollArea::vertical()
        .id_salt("dialog:folderManager/tree")
        .max_height(inner.height())
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL);
    let ctx = ui.ctx().clone();
    let mut nav = ctx.data(|d| d.get_temp::<Nav>(nav_id())).unwrap_or_default();
    // The current row: the one last current while it is still there, else the selected one, else
    // the first folder. (Found by a walk only when the rows changed under it.)
    let kept = nav
        .current
        .as_ref()
        .and_then(|(i, id)| if rows.get(*i).is_some_and(|r| r.id() == *id) { Some(*i) } else { rows.iter().position(|r| r.id() == *id) });
    let picked = || selected.and_then(|s| rows.iter().position(|r| matches!(r.kind, Kind::Folder { .. }) && r.path.to_string_lossy() == s));
    let mut current = scroll_to.or(kept).or_else(picked).or_else(|| (0..rows.len()).find(|i| is_folder(rows, *i)));
    let row_egui_id = |i: usize| rows.get(i).map(|r| Id::new(r.id()));
    let keyboard = current.is_some() && ctx.memory(|m| m.focused()) == current.and_then(row_egui_id);
    // Where the keyboard goes: the keys a focused row takes (`crate::a11y::keyboard` keeps them
    // from the shortcuts and the dialog's buttons).
    let mut reveal = keyboard && !nav.keyed;
    if keyboard && let Some(at) = current {
        const KEYS: [Key; 8] = [Key::ArrowDown, Key::ArrowUp, Key::Home, Key::End, Key::ArrowRight, Key::ArrowLeft, Key::Enter, Key::Space];
        for key in KEYS {
            if !ui.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                continue;
            }
            let (to, toggle) = step(rows, at, key);
            if toggle {
                out.toggled = Some(at);
            }
            if let Some(to) = to {
                current = Some(to);
                out.clicked = Some(to);
                reveal = true;
                if let Some(id) = row_egui_id(to) {
                    ctx.memory_mut(|m| m.request_focus(id));
                }
            }
            break;
        }
    }
    if let Some(i) = scroll_to {
        let y = (i as f32 * row_h - (inner.height() - row_h) / 2.0).max(0.0);
        area = area.vertical_scroll_offset(y);
    } else if reveal && let Some(i) = current {
        // The least scroll that shows the row whole.
        let (top, view_h) = (i as f32 * row_h, nav.view_h.max(row_h));
        let y = if top < nav.offset {
            top
        } else if top + row_h > nav.offset + view_h {
            top + row_h - view_h
        } else {
            nav.offset
        };
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    nav.keyed = keyboard;
    let sets = if crate::a11y::on(&ctx) { sibling_sets(rows) } else { Vec::new() };
    let mut stop_at = None;
    let time = ui.input(|i| i.time) as f32;
    let shown = area.show_rows(&mut child, row_h, rows.len(), |ui, range| {
        let w = ui.available_width();
        for i in range {
            let Some(row) = rows.get(i) else { break };
            let (rect, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
            let x = rect.min.x + LEFT + row.depth as f32 * t.metrics.tree_indent;
            let p = ui.painter();
            match &row.kind {
                Kind::Folder { open, leaf } => {
                    let id = row.id();
                    // Only the current row takes the focus: the tree is one Tab stop.
                    let is_current = current == Some(i);
                    let r = ui.interact(rect, Id::new(&id), crate::a11y::keyboard::row_sense(is_current));
                    if is_current {
                        stop_at = Some(rect);
                    }
                    let path_key = row.path.to_string_lossy();
                    let is_selected = selected == Some(path_key.as_ref());
                    if is_selected {
                        p.rect_filled(rect, 0, t.pressed);
                    } else if r.hovered() {
                        p.rect_filled(rect, 0, t.hover.gamma_multiply(0.6));
                    }
                    let disc = Rect::from_min_size(pos2(x, rect.min.y), vec2(DISCLOSURE + GAP, row_h));
                    if !leaf {
                        let glyph = Rect::from_center_size(disc.center(), vec2(DISCLOSURE, DISCLOSURE));
                        icons::paint(p, glyph, if *open { Icon::ChevronDown } else { Icon::ChevronRight }, t.icon);
                        // A click expands; the keyboard expands with → and ← on the row.
                        let d = ui.interact(disc, Id::new(expand_id(&row.root, &row.path)), Sense::CLICK);
                        let label = if *open { tl!("Collapse") } else { tl!("Expand") };
                        widgets::register(ui.ctx(), &expand_id(&row.root, &row.path), Role::Button, &label, Target::Widget(&d), *open);
                        if d.clicked() {
                            out.toggled = Some(i);
                        }
                    }
                    let side = t.metrics.icon_menu;
                    let (st, inherited) = state(&row.path);
                    let state_rect = state_rect(t, rect, row.depth);
                    paint_folder(p, state_rect, st.icon(), state_color(st.color(t), inherited));
                    let icon_rect = state_rect.translate(vec2(side + GAP, 0.0));
                    match row.icon {
                        Some(icon) => paint_folder(p, icon_rect, icon, t.icon),
                        None => icons::paint(p, icon_rect, Icon::Folder, t.icon),
                    }
                    let tx = icon_rect.max.x + GAP + 2.0;
                    let color = if is_selected { t.text } else { t.text_label };
                    let g = one_line(ui, &row.name, fonts::regular(t.body), color, rect.max.x - tx - LEFT);
                    p.galley(pos2(tx, rect.center().y - g.size().y / 2.0).round(), g, color);
                    widgets::focus_ring(p, t, &r, rect.shrink(2.0), 0);
                    let set = sets.get(i).copied().filter(|s| s.1 > 0);
                    widgets::access(&r, |node| {
                        node.set_role(egui::accesskit::Role::TreeItem);
                        node.clear_toggled();
                        node.set_selected(is_selected);
                        if !leaf {
                            node.set_expanded(*open);
                        }
                        node.set_level(row.depth.saturating_add(1));
                        if let Some((position, size)) = set {
                            node.set_position_in_set(position);
                            node.set_size_of_set(size);
                        }
                    });
                    widgets::register(ui.ctx(), &id, Role::TreeRow, &row.name, Target::Widget(&r), is_selected);
                    if r.double_clicked() && !leaf {
                        out.toggled = Some(i);
                    } else if r.clicked() {
                        out.clicked = Some(i);
                        current = Some(i);
                    }
                }
                Kind::Loading => {
                    let side = t.metrics.icon_menu;
                    let spin = Rect::from_min_size(pos2(x + DISCLOSURE + GAP, rect.center().y - side / 2.0), vec2(side, side));
                    icons::paint_activity(p, spin, t.icon, time);
                    let loading = tl!("Loading…");
                    p.text(pos2(spin.max.x + GAP + 2.0, rect.center().y), Align2::LEFT_CENTER, &loading, fonts::regular(t.body), t.text_dim);
                    widgets::register(ui.ctx(), &row.id(), Role::Other, &loading, Target::Painted { rect, clip: ui.clip_rect() }, false);
                    ui.ctx().request_repaint();
                }
                Kind::Failed | Kind::More => {
                    let text = if row.kind == Kind::Failed { tl!("This folder cannot be read.") } else { tl!("More folders are not shown.") };
                    let tx = x + DISCLOSURE + GAP;
                    let g = one_line(ui, &text, fonts::regular(t.body), t.text_dim, rect.max.x - tx - LEFT);
                    p.galley(pos2(tx, rect.center().y - g.size().y / 2.0).round(), g, t.text_dim);
                }
            }
        }
    });
    nav.offset = shown.state.offset.y;
    nav.view_h = shown.inner_rect.height();
    nav.current = current.and_then(|i| Some((i, rows.get(i)?.id())));
    // The tree's one Tab stop, where it reads: its row, or the edge of the well it is scrolled past.
    if let Some(i) = current
        && let Some(id) = row_egui_id(i)
    {
        let at = stop_at.unwrap_or_else(|| {
            let y = if (i as f32) * row_h < nav.offset { inner.min.y } else { inner.max.y - row_h };
            Rect::from_min_size(pos2(inner.min.x, y), vec2(inner.width(), row_h))
        });
        crate::a11y::keyboard::one_stop(&ctx, id, ui.layer_id(), at);
    }
    ctx.data_mut(|d| d.insert_temp(nav_id(), nav));
    out
}
