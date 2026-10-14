//! The library tree pane (`ui-layout.md` §4), left of the light table and as wide as the splitter
//! leaves it (200 … W − 480).
//!
//! - **What it shows** is [`dalica_library::Tree`] for the view state (Flat | Tree, the folder
//!   sort, the collapsed groups and tree folders, Small and Hidden Pictures), laid out by
//!   [`model::Model`]: group headers (30, disclosure, title with the count), folder rows (24: the
//!   disclosure triangle in tree mode, indented 16 per level, the folder icon, the name elided in
//!   the middle, the count in `text_dim`) and year dividers (18). The model is rebuilt only when the
//!   library's revision, the watched roots or those options change; a frame lays out and paints
//!   the rows on screen only, so its work does not grow with the library.
//! - **Clicks:** a row with photos selects its folder as a unit (what `select.section` does) and
//!   scrolls the light table to its section (`section.scrollTo`, through [`Panels::scroll_to`]);
//!   a row without photos folds or unfolds its children. A double-click on a library folder's row
//!   opens its Folder Properties (`section.properties`). A header runs `view.groupCollapse` (its
//!   sections leave the light table); a disclosure triangle `view.folderCollapse`.
//! - **Hidden folders** (Hide Folder): with View ▸ Hidden Pictures off they leave the tree and
//!   their sections leave the light table; with it on, flat mode lists them in the Hidden Folders
//!   group (after Other), tree mode keeps them in place. The tree decides
//!   ([`dalica_library::Tree`]); the pane rebuilds when the option or the library changes. Directories
//!   that are no library folder have no `FolderId` for the view state: they fold here, in the
//!   pane's own memory, for this session.
//! - **Focus and keys:** a press in the pane gives it the keyboard focus (the selected row gets
//!   the 2 pt `accent` bar), a press anywhere else takes it away. While it has it, the plain arrows
//!   are the pane's ([`has_focus`], [`is_tree_key`]: the shortcut dispatch leaves them alone):
//!   ↑/↓ move the selection to the previous or next row with photos (scrolling the light table, and
//!   the pane to keep the row in view); in tree mode ← folds the selected row or goes to its
//!   parent, → unfolds it.
//! - **Scrolling:** the wheel over the pane scrolls it, and while the list is taller than the pane
//!   an ordinary position scroll bar ([`scrollbar`]) at its right edge drags and pages it.
//!   Collapsed groups after the last open one dock at the bottom of the pane while the list above
//!   them is short.
//! - **Offline folders:** rows under a watched root whose volume is offline
//!   ([`Session::offline_roots`](dalica_engine::Session::offline_roots), `data-model.md` §7) are
//!   dimmed with a `caution` "offline" marker; the roots are part of what the model is built from,
//!   so the rows are marked once per change, never tested per frame.
//!
//! The pane's state between frames (the model and what it was built from, the offset, the folded
//! directories, the focus) lives in the egui context's memory; [`info`] reports what the last frame
//! did, for tests and inspection.

pub mod model;
mod paint;
pub mod scrollbar;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::FolderId;
use dalica_library::{Group, Tree, TreeMode, TreeOptions};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Context, Id, Modifiers, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use self::model::{Heights, Kind, Model};
use super::{Panels, light_table};
use crate::app::App;
use crate::perf::{self, Counter};
use crate::selection::{self, Scope};
use crate::state::Section;

/// What the pane's tree was built from: the library's revision, the watched roots, the view
/// state's tree options and folded folders, the pane's folded directories, the row heights.
#[derive(Debug, Clone, PartialEq)]
struct ModelKey {
    revision: u64,
    roots: Vec<PathBuf>,
    options: TreeOptions,
    folded: Vec<FolderId>,
    dirs: BTreeSet<PathBuf>,
    heights: Heights,
}

/// The pane's state between frames.
#[derive(Debug, Clone, Default)]
struct Pane {
    key: Option<ModelKey>,
    model: Arc<Model>,
    /// Counts the model's rebuilds, and how long the last one took.
    generation: u64,
    build: Duration,
    /// The content offset at the pane's top.
    offset: f32,
    /// Tree mode: folded directories that are no library folder.
    dirs: BTreeSet<PathBuf>,
    /// The scroll bar's drag.
    bar: scrollbar::Bar,
}

/// What the last frame of the pane did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneInfo {
    /// The content offset at the pane's top.
    pub offset: f32,
    /// The content's height and the pane's.
    pub height: f32,
    pub view_h: f32,
    /// Rows and year dividers laid out (headers aside).
    pub rows_visited: usize,
    /// The model's rebuilds so far.
    pub generation: u64,
    /// The time the pane took, rebuilding the model included.
    pub time: Duration,
    /// How long the last rebuild of the model took (the tree and its items).
    pub build: Duration,
    /// Whether the model was rebuilt this frame.
    pub rebuilt: bool,
    /// The scroll bar's strip, while the list is taller than the pane.
    pub scrollbar: Option<Rect>,
}

/// What a click or key asks for, applied once the frame is drawn.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    /// Select item `i`'s folder as a unit and scroll the light table to it.
    Select(usize),
    /// Fold (`Some(true)`), unfold or toggle item `i`'s children.
    Fold(usize, Option<bool>),
    /// Fold or unfold a group.
    Group(Group),
    /// Open item `i`'s Folder Properties.
    Properties(usize),
}

fn pane_id() -> Id {
    Id::new("dalica-tree-pane")
}

fn focus_id() -> Id {
    Id::new("dalica-tree-pane/focus")
}

fn info_id() -> Id {
    Id::new("dalica-tree-pane/info")
}

/// Whether the library tree has the keyboard focus.
pub fn has_focus(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(focus_id())).unwrap_or(false)
}

/// Whether the focused tree takes this key press for itself: a plain arrow.
pub fn is_tree_key(key: egui::Key, modifiers: Modifiers) -> bool {
    modifiers.is_none() && matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight)
}

/// What the pane's last frame did.
pub fn info(ctx: &Context) -> PaneInfo {
    ctx.data(|d| d.get_temp::<PaneInfo>(info_id())).unwrap_or_default()
}

/// The pane's items as last laid out.
pub fn current_model(ctx: &Context) -> Arc<Model> {
    ctx.data(|d| d.get_temp::<Pane>(pane_id())).map(|p| p.model).unwrap_or_default()
}

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let started = Instant::now();
    let t = *app.tokens();
    let rect = ui.max_rect();
    let ctx = ui.ctx().clone();
    let Some(session) = app.session() else { return };
    let mut pane = ctx.data_mut(|d| d.remove_temp::<Pane>(pane_id())).unwrap_or_default();

    // The model, rebuilt when what it shows changed.
    let library = session.library();
    let state = app.state();
    let heights = Heights::from_metrics(&t.metrics);
    let mut options = light_table::tree_options(app);
    options.offline_roots = session.offline_roots();
    let key = ModelKey {
        revision: library.revision(),
        roots: session.roots().to_vec(),
        options,
        folded: state.collapsed_folders.clone(),
        dirs: pane.dirs.clone(),
        heights,
    };
    let rebuilt = pane.key.as_ref() != Some(&key);
    if rebuilt {
        // Folded folders by path, as the tree takes them.
        let mut options = key.options.clone();
        options.collapsed_folders =
            key.folded.iter().filter_map(|id| library.folder(*id)).map(|f| f.path.clone()).chain(key.dirs.iter().cloned()).collect();
        let tree = Tree::build(library, &key.roots, &options);
        let mut model = Model::build(&tree, heights);
        if !options.offline_roots.is_empty() {
            model.mark_offline(|path| options.is_offline(path));
        }
        pane.model = Arc::new(model);
        pane.generation = pane.generation.wrapping_add(1);
        pane.build = started.elapsed();
        pane.key = Some(key);
    }
    let model = Arc::clone(&pane.model);
    let tree_mode = model.mode == TreeMode::Tree;
    let view_h = rect.height();

    // Focus: a press in the pane takes it, a press elsewhere gives it up.
    let mut focused = has_focus(&ctx);
    if let Some(pos) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None }) {
        focused = rect.contains(pos);
    }

    // The wheel, then the keys, before anything is drawn.
    if ui.rect_contains_pointer(rect) {
        let dy = ctx.input(|i| i.smooth_scroll_delta.y);
        if dy.is_finite() && dy != 0.0 {
            pane.offset -= dy;
        }
    }
    let mut actions = Vec::new();
    if focused {
        let keys: Vec<egui::Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if is_tree_key(*key, *modifiers) => Some(*key),
                    _ => None,
                })
                .collect()
        });
        let mut cursor = state.folder.and_then(|f| model.row_of(f));
        for k in keys {
            match k {
                egui::Key::ArrowDown | egui::Key::ArrowUp => {
                    if let Some(i) = model.step(cursor, k == egui::Key::ArrowDown) {
                        cursor = Some(i);
                        actions.push(Action::Select(i));
                        pane.offset = model.reveal(i, pane.offset, view_h);
                    }
                }
                egui::Key::ArrowLeft if tree_mode => {
                    let Some(i) = cursor else { continue };
                    match model.row(i) {
                        Some(r) if r.has_children && r.expanded => actions.push(Action::Fold(i, Some(true))),
                        Some(_) => {
                            if let Some(up) = model.parent_with_section(i) {
                                cursor = Some(up);
                                actions.push(Action::Select(up));
                                pane.offset = model.reveal(up, pane.offset, view_h);
                            }
                        }
                        None => {}
                    }
                }
                egui::Key::ArrowRight if tree_mode => {
                    if let Some(i) = cursor
                        && model.row(i).is_some_and(|r| r.has_children && !r.expanded)
                    {
                        actions.push(Action::Fold(i, Some(false)));
                    }
                }
                _ => {}
            }
        }
    }
    pane.offset = if pane.offset.is_finite() { pane.offset.clamp(0.0, model.max_offset(view_h)) } else { 0.0 };

    // The scroll bar, while the list is taller than the pane: the rows end left of it.
    let strip = scrollbar::strip(rect, model.height);
    let bar = strip.map(|s| scrollbar::input(ui, &mut pane.bar, s, view_h, model.height, &mut pane.offset));
    let rows = strip.map_or(rect, |s| Rect::from_min_max(rect.min, pos2(s.min.x, rect.max.y)));

    // The items on screen.
    let painter = ui.painter().clone();
    let selected_folder = state.folder;
    let mut visited = 0usize;
    for i in model.visible(pane.offset, view_h) {
        let (Some(item), Some(top)) = (model.items.get(i), model.screen_top(i, pane.offset, view_h)) else { continue };
        let r = Rect::from_min_size(pos2(rows.min.x, rows.min.y + top), vec2(rows.width(), item.height));
        match &item.kind {
            Kind::Header { group, title, collapsed } => {
                let mut child = ui.new_child(UiBuilder::new().id_salt(("tree-header", group.id())).max_rect(r));
                if widgets::section_header(&mut child, &t, &format!("tree:group:{}", group.id()), title, !collapsed).clicked() {
                    actions.push(Action::Group(*group));
                }
            }
            Kind::Year(year) => {
                visited += 1;
                paint::year(&painter, &t, r, *year);
            }
            Kind::Row(row) => {
                visited += 1;
                let response = ui.interact(r, Id::new(("tree-row", &row.id)), Sense::click());
                let selected = row.folder.is_some() && row.folder == selected_folder;
                let look = paint::Look { selected, hovered: response.hovered(), focused, indent: heights.indent };
                paint::row(&painter, &t, r, row, look, tree_mode);
                widgets::register(&ctx, &row.id, Role::TreeRow, &row.label(), Target::Widget(&response), selected);
                // The triangle goes over the row, so a click on it folds rather than selects.
                let mut folded = false;
                if let (true, Some(id)) = (tree_mode, &row.disclosure) {
                    let d = ui.interact(paint::disclosure_rect(r, row.depth, heights.indent), Id::new(("tree-disclosure", id)), Sense::click());
                    let label = if row.expanded { format!("Collapse {}", row.name) } else { format!("Expand {}", row.name) };
                    widgets::register(&ctx, id, Role::Toggle, &label, Target::Widget(&d), row.expanded);
                    if d.clicked() {
                        folded = true;
                        actions.push(Action::Fold(i, None));
                    }
                }
                if response.clicked() && !folded {
                    if row.section.is_some() {
                        actions.push(Action::Select(i));
                    } else if row.has_children && tree_mode {
                        actions.push(Action::Fold(i, None));
                    }
                }
                // The second click of a double-click also selects (as the first did), then opens.
                if response.double_clicked() && !folded && row.folder.is_some() {
                    actions.push(Action::Properties(i));
                }
            }
        }
    }
    perf::count(&ctx, Counter::TreeRowsVisited, visited as u64);
    if let (Some(s), Some(f)) = (strip, &bar) {
        scrollbar::paint(ui, &t, f, s, view_h, model.height, pane.offset);
    }

    // With the frame drawn, apply what was asked for.
    for action in actions {
        apply(p, app, &mut pane, &model, action);
    }
    let info = PaneInfo {
        offset: pane.offset,
        height: model.height,
        view_h,
        rows_visited: visited,
        generation: pane.generation,
        build: pane.build,
        time: started.elapsed(),
        rebuilt,
        scrollbar: strip,
    };
    ctx.data_mut(|d| {
        d.insert_temp(pane_id(), pane);
        d.insert_temp(focus_id(), focused);
        d.insert_temp(info_id(), info);
    });
}

fn apply(p: &mut Panels, app: &mut App, pane: &mut Pane, model: &Model, action: Action) {
    match action {
        Action::Group(g) => {
            let _ = app.run("view.groupCollapse", &json!({ "group": g.id() }));
        }
        Action::Fold(i, want) => {
            let Some(row) = model.row(i) else { return };
            match row.folder {
                Some(folder) => {
                    let params = match want {
                        Some(c) => json!({ "folder": folder, "collapsed": c }),
                        None => json!({ "folder": folder }),
                    };
                    let _ = app.run("view.folderCollapse", &params);
                }
                None => {
                    let fold = want.unwrap_or(!pane.dirs.contains(&row.path));
                    if fold {
                        pane.dirs.insert(row.path.clone());
                    } else {
                        pane.dirs.remove(&row.path);
                    }
                }
            }
        }
        Action::Select(i) => {
            let Some(Section::Folder(folder)) = model.row(i).and_then(|r| r.section) else { return };
            select_folder(p, app, folder);
        }
        Action::Properties(i) => {
            let Some(folder) = model.row(i).and_then(|r| r.folder) else { return };
            // A refusal (no library) lands on the status line.
            let _ = app.run("section.properties", &json!({ "folder": folder }));
        }
    }
}

/// Selects `folder`'s section as a unit, as `select.section` does, and asks the light table to
/// scroll to it. The panels' sections are this frame's, so nothing is rebuilt for it.
fn select_folder(p: &mut Panels, app: &mut App, folder: FolderId) {
    let section = Section::Folder(folder);
    match p.sections.get(section) {
        Some(s) => {
            let scope = Scope { section, folder: s.folder, photos: &s.photos, cols: 1 };
            selection::commit(app.state_mut(), selection::whole(&scope));
            app.state_mut().status.clear();
        }
        None => {
            if app.run("select.section", &json!({ "section": section })).is_err() {
                return;
            }
        }
    }
    p.scroll_to = Some(0.0);
}
