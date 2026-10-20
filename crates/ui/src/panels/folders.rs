//! The library tree pane (`ui-layout.md` §4), left of the light table and as wide as the splitter
//! leaves it (200 … W − 480).
//!
//! - **What it shows** is the Albums group ([`dalica_library::AlbumTree`]: album rows with their
//!   icons, names and counts, the search results first and highlighted while searching), then
//!   [`dalica_library::Tree`] for the view state (Flat | Tree, the folder
//!   sort, the collapsed groups and tree folders, Small and Hidden Pictures), laid out by
//!   [`model::Model`]: group headers (30, disclosure, title with the count), folder rows (24: the
//!   disclosure triangle in tree mode, indented 16 per level, the folder icon, the name elided in
//!   the middle, the count in `text_dim`) and year dividers (18). The model is rebuilt only when the
//!   library's revision, the watched roots or those options change; a frame lays out and paints
//!   the rows on screen only, so its work does not grow with the library.
//! - **Clicks:** a row with photos selects its folder or album as a unit (what `select.section`
//!   does) and scrolls the light table to its section (`section.scrollTo`, through
//!   [`Panels::scroll_to`]); a row without photos folds or unfolds its children. A double-click on
//!   a library folder's row opens its Folder Properties, on a user album's its Album Properties
//!   (`section.properties`). The selected row is the album the selection is in, else the folder
//!   selected. In search mode the group headers show a magnifier in place of their triangle. A header runs `view.groupCollapse` (its
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
//! - **Drops** (`ui-layout.md` §4): while photos are dragged (`crate::drag`), the row under the
//!   pointer is offered as a drop target, a library folder's (move; Ctrl copies) or a user album's
//!   (add), and gets the `drop_target` highlight when a drop there would change something; the
//!   pointer held within [`DRAG_EDGE`] of the pane's top or bottom edge scrolls the rows
//!   (faster nearer the edge), so a folder off screen can be reached. A drag of faces (the Unnamed
//!   grid, a person's Zoom to Faces cells) is offered a person's row: the drop names them as that
//!   person's (one undo step); photos dragged onto a person change nothing (they hold no face).
//! - **People** ([`people`], `organizing.md` §6.2–§6.3): the People group between Albums and
//!   Folders. A click on Unnamed shows the Unnamed grid in the light table's place
//!   (`unnamed.open`, its row then the selected one; a click on any other row puts the light table
//!   back); a click on a person selects their section as a unit and scrolls to it, as an album's
//!   row does. A right-click on a person's row opens the person menu (Rename…, Merge Into ▸,
//!   Delete Person; `crate::commands::people::show_menu`). Their faces come from the shared face
//!   picture store (`crate::panels::people_pane::pictures`); while the face scan runs the group's
//!   header reads "Finding faces… n of m". The group is part of what the model is built from
//!   (the library's revision covers the people, the view state's people sort their order).
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
pub mod people;
pub mod scrollbar;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{FolderId, PersonId};
use dalica_library::{AlbumTree, Group, Tree, TreeMode, TreeOptions};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Context, Id, Modifiers, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use self::model::{Heights, Kind, Model};
use super::{Panels, light_table};
use crate::app::App;
use crate::panels::people_pane::pictures;
use crate::perf::{self, Counter};
use crate::selection::{self, Scope};
use crate::state::{AlbumSection, DropAt, PeopleSort, Section};
use crate::tl;

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
    people: PeopleSort,
    /// The suggestions' copy the "?" badges were lit from (`crate::commands::suggestions`).
    suggested: u64,
    /// The language the group names and built-in rows are written in.
    language: &'static str,
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
    /// Open item `i`'s Folder or Album Properties.
    Properties(usize),
    /// Show the Unnamed grid in the light table's place.
    Unnamed,
}

/// How far inside the pane's top and bottom edges a drag of photos scrolls the rows (points), and
/// the speed there (points per second) growing by [`DRAG_GAIN`] per point nearer the edge.
pub const DRAG_EDGE: f32 = 24.0;
pub const DRAG_SPEED: f32 = 240.0;
pub const DRAG_GAIN: f32 = 40.0;

/// How fast a drag of photos held at `y` scrolls a pane spanning `top..bottom` (points per second,
/// positive = down): none farther than [`DRAG_EDGE`] from both edges, else [`DRAG_SPEED`] plus
/// [`DRAG_GAIN`] per point past that line. 0 for numbers that are not finite.
pub fn drag_scroll_speed(y: f32, top: f32, bottom: f32) -> f32 {
    if !(y.is_finite() && top.is_finite() && bottom.is_finite()) || y < top || y > bottom {
        return 0.0;
    }
    let (below, above) = (y - (bottom - DRAG_EDGE), (top + DRAG_EDGE) - y);
    if below > 0.0 && below >= above {
        DRAG_SPEED + DRAG_GAIN * below
    } else if above > 0.0 {
        -(DRAG_SPEED + DRAG_GAIN * above)
    } else {
        0.0
    }
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
    // The people's faces: the tiles made since the last frame.
    pictures::begin(app, &ctx);
    let suggested = crate::commands::suggestions::current(app);
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
        people: state.people.sort,
        suggested: suggested.generation,
        language: crate::i18n::current().code,
    };
    let rebuilt = pane.key.as_ref() != Some(&key);
    if rebuilt {
        // Folded folders by path, as the tree takes them.
        let mut options = key.options.clone();
        options.collapsed_folders =
            key.folded.iter().filter_map(|id| library.folder(*id)).map(|f| f.path.clone()).chain(key.dirs.iter().cloned()).collect();
        let tree = Tree::build(library, &key.roots, &options);
        let albums = AlbumTree::build(library, &options);
        let mut people = people::PeopleGroup::build(library, key.people, options.collapsed_groups.contains(&Group::People));
        people.set_pending(&suggested.pending);
        let mut model = Model::with_groups(&tree, &albums, &people, options.filter.is_some(), heights);
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
        let mut cursor =
            selected_album(state).and_then(|a| model.row_of_section(Section::Album(a))).or_else(|| state.folder.and_then(|f| model.row_of(f)));
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
    // A drag of photos held near the top or bottom edge scrolls the rows.
    let dragging = crate::drag::pointer(&ctx, state).filter(|p| rect.contains(*p));
    if let Some(pos) = dragging {
        let speed = drag_scroll_speed(pos.y, rect.min.y, rect.max.y);
        if speed != 0.0 {
            let dt = ctx.input(|i| i.stable_dt).clamp(1.0 / 120.0, 0.1);
            pane.offset += speed * dt;
            ctx.request_repaint();
        }
    }
    pane.offset = if pane.offset.is_finite() { pane.offset.clamp(0.0, model.max_offset(view_h)) } else { 0.0 };

    // The scroll bar, while the list is taller than the pane: the rows end left of it.
    let strip = scrollbar::strip(rect, model.height);
    let bar = strip.map(|s| scrollbar::input(ui, &mut pane.bar, s, view_h, model.height, &mut pane.offset));
    let rows = strip.map_or(rect, |s| Rect::from_min_max(rect.min, pos2(s.min.x, rect.max.y)));

    // The items on screen.
    let painter = ui.painter().clone();
    let unnamed_open = state.unnamed.open;
    let selected_folder = state.folder.filter(|_| !unnamed_open);
    let selected_album = selected_album(state).filter(|_| !unnamed_open);
    let scan = people::scan_title(session);
    let face_side = pictures::side_px(&ctx, paint::FACE);
    let mut menu: Option<(egui::Response, PersonId)> = None;
    let mut visited = 0usize;
    for i in model.visible(pane.offset, view_h) {
        let (Some(item), Some(top)) = (model.items.get(i), model.screen_top(i, pane.offset, view_h)) else { continue };
        let r = Rect::from_min_size(pos2(rows.min.x, rows.min.y + top), vec2(rows.width(), item.height));
        match &item.kind {
            Kind::Header { group, title, collapsed } => {
                let id = format!("tree:group:{}", group.id());
                // While the face scan runs, the People group's header says how far it has come.
                let title = scan.as_ref().filter(|_| *group == Group::People).unwrap_or(title);
                let clicked = if model.search {
                    paint::search_header(ui, &t, &id, title, !collapsed, r).clicked()
                } else {
                    let mut child = ui.new_child(UiBuilder::new().id_salt(("tree-header", group.id())).max_rect(r));
                    widgets::section_header(&mut child, &t, &id, title, !collapsed).clicked()
                };
                if clicked {
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
                let selected = match row.album {
                    Some(album) => selected_album == Some(album),
                    None if row.unnamed => unnamed_open,
                    None => selected_album.is_none() && row.folder.is_some() && row.folder == selected_folder,
                };
                let look = paint::Look { selected, hovered: response.hovered() && dragging.is_none(), focused, indent: heights.indent };
                let face = row.face.and_then(|f| {
                    let rect = library.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) })?.rect;
                    pictures::picture(&ctx, f, rect, face_side)
                });
                paint::row(&painter, &t, r, row, look, tree_mode, face);
                if let Some(at) = dragging.filter(|p| r.contains(*p)).and_then(|_| drop_at(row))
                    && crate::drag::offer(&ctx, session, state, at, &row.id).is_some()
                {
                    widgets::drop_target(&painter, &t, r.shrink(1.0));
                }
                widgets::register(&ctx, &row.id, Role::TreeRow, &row.label(), Target::Widget(&response), selected);
                // The triangle goes over the row, so a click on it folds rather than selects.
                let mut folded = false;
                if let (true, Some(id)) = (tree_mode, &row.disclosure) {
                    let d = ui.interact(paint::disclosure_rect(r, row.depth, heights.indent), Id::new(("tree-disclosure", id)), Sense::click());
                    let label = if row.expanded { tl!("Collapse {name}", name = row.name) } else { tl!("Expand {name}", name = row.name) };
                    widgets::register(&ctx, id, Role::Toggle, &label, Target::Widget(&d), row.expanded);
                    if d.clicked() {
                        folded = true;
                        actions.push(Action::Fold(i, None));
                    }
                }
                if response.clicked() && !folded {
                    if row.unnamed {
                        actions.push(Action::Unnamed);
                    } else if row.section.is_some() {
                        actions.push(Action::Select(i));
                    } else if row.has_children && tree_mode {
                        actions.push(Action::Fold(i, None));
                    }
                }
                // A person's row: the person menu, after selecting their section.
                if let Some(person) = row.person().and_then(|k| k.id())
                    && (response.secondary_clicked() || response.context_menu_opened())
                {
                    if response.secondary_clicked() && row.section.is_some() && !selected {
                        actions.push(Action::Select(i));
                    }
                    menu = Some((response.clone(), person));
                }
                // The second click of a double-click also selects (as the first did), then opens.
                if response.double_clicked() && !folded && (row.folder.is_some() || matches!(row.album, Some(AlbumSection::User(_)))) {
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
    if let Some((r, person)) = menu {
        crate::commands::people::show_menu(&r, app, &person);
    }
    pictures::finish(app, &ctx);
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
            let Some(section) = model.row(i).and_then(|r| r.section) else { return };
            app.state_mut().unnamed.open = false;
            select_section(p, app, section);
        }
        Action::Unnamed => {
            app.state_mut().unnamed.open = true;
        }
        Action::Properties(i) => {
            let Some(row) = model.row(i) else { return };
            // A refusal (no library) lands on the status line.
            if let Some(folder) = row.folder {
                let _ = app.run("section.properties", &json!({ "folder": folder }));
            } else if let Some(section @ Section::Album(AlbumSection::User(_))) = row.section {
                // Album Properties is the selected album's: the double-click's first click selected
                // it, and selecting it again here keeps that true whatever ran in between.
                select_section(p, app, section);
                let _ = app.run("section.properties", &json!({}));
            }
        }
    }
}

/// What a drag dropped on `row` goes into: a library folder or a user album (photos), or a person
/// (faces).
fn drop_at(row: &model::Row) -> Option<DropAt> {
    match (row.folder, row.album) {
        (Some(f), None) => Some(DropAt::Folder(f)),
        (None, Some(AlbumSection::User(k))) => Some(DropAt::Album(k)),
        (None, Some(AlbumSection::Person(k))) => Some(DropAt::Person(k)),
        _ => None,
    }
}

/// The album section the selection is in, if any: its tree row is the selected one.
fn selected_album(state: &crate::state::UiState) -> Option<AlbumSection> {
    match state.selection.section {
        Some(Section::Album(a)) => Some(a),
        _ => None,
    }
}

/// Selects `section` (a folder's or an album's) as a unit, as `select.section` does, and asks the
/// light table to scroll to it. The panels' sections are this frame's, so nothing is rebuilt for it.
fn select_section(p: &mut Panels, app: &mut App, section: Section) {
    match p.sections.get(section) {
        Some(s) if app.session().is_some() => {
            let scope = Scope { section, folder: s.folder, photos: &s.photos, cols: 1 };
            let Some(library) = app.session().map(dalica_engine::Session::library) else { return };
            let sel = selection::whole(&scope, library);
            selection::commit(app.state_mut(), sel);
            app.state_mut().status.clear();
        }
        _ => {
            if app.run("select.section", &json!({ "section": section })).is_err() {
                return;
            }
        }
    }
    match section {
        // The panels scroll to the tree's folder once the pane is drawn.
        Section::Folder(_) => p.scroll_to = Some(0.0),
        // An album's section, which this frame's sections hold: the light table eases there, as
        // `section.scrollTo` asks (the command waits for the panels, which are drawing now).
        Section::Album(_) if p.sections.get(section).is_some() => {
            app.state_mut().scroll = crate::state::ScrollAnchor { section: Some(section), offset: 0.0 };
        }
        Section::Album(_) => {}
    }
}
