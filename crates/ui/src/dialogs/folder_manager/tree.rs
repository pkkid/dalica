//! The Folder Manager's folder tree: its rows, flattened from the roots and the expanded folders'
//! listings, and drawn in a well that scrolls with the wheel (only the rows in view are laid out).
//! Each row: disclosure triangle, state icon, folder icon (a root's own icon), name.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_ui_kit::icons::{self, FolderIcon, paint_folder, state_color};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Icon, Tokens, fonts};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::draft::Draft;
use super::listing::{self, Listing, Root};
use super::{State, expand_id, row_id};
use crate::app::App;

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
    if let Some(i) = scroll_to {
        let y = (i as f32 * row_h - (inner.height() - row_h) / 2.0).max(0.0);
        area = area.vertical_scroll_offset(y);
    }
    let time = ui.input(|i| i.time) as f32;
    area.show_rows(&mut child, row_h, rows.len(), |ui, range| {
        let w = ui.available_width();
        for i in range {
            let Some(row) = rows.get(i) else { break };
            let (rect, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
            let x = rect.min.x + LEFT + row.depth as f32 * t.metrics.tree_indent;
            let p = ui.painter();
            match &row.kind {
                Kind::Folder { open, leaf } => {
                    let id = row.id();
                    let r = ui.interact(rect, Id::new(&id), Sense::click());
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
                        let d = ui.interact(disc, Id::new(expand_id(&row.root, &row.path)), Sense::click());
                        let label = if *open { "Collapse" } else { "Expand" };
                        widgets::register(ui.ctx(), &expand_id(&row.root, &row.path), Role::Button, label, Target::Widget(&d), *open);
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
                    widgets::register(ui.ctx(), &id, Role::TreeRow, &row.name, Target::Widget(&r), is_selected);
                    if r.double_clicked() && !leaf {
                        out.toggled = Some(i);
                    } else if r.clicked() {
                        out.clicked = Some(i);
                    }
                }
                Kind::Loading => {
                    let side = t.metrics.icon_menu;
                    let spin = Rect::from_min_size(pos2(x + DISCLOSURE + GAP, rect.center().y - side / 2.0), vec2(side, side));
                    icons::paint_activity(p, spin, t.icon, time);
                    p.text(pos2(spin.max.x + GAP + 2.0, rect.center().y), Align2::LEFT_CENTER, "Loading…", fonts::regular(t.body), t.text_dim);
                    widgets::register(ui.ctx(), &row.id(), Role::Other, "Loading…", Target::Painted { rect, clip: ui.clip_rect() }, false);
                    ui.ctx().request_repaint();
                }
                Kind::Failed | Kind::More => {
                    let text = if row.kind == Kind::Failed { "This folder cannot be read." } else { "More folders are not shown." };
                    let tx = x + DISCLOSURE + GAP;
                    let g = one_line(ui, text, fonts::regular(t.body), t.text_dim, rect.max.x - tx - LEFT);
                    p.galley(pos2(tx, rect.center().y - g.size().y / 2.0).round(), g, t.text_dim);
                }
            }
        }
    });
    out
}
