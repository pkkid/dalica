//! The light table (`ui-layout.md` §5): every visible folder as a section, in the library tree's
//! order — a header block, then a justified grid of square cells, each photo letterboxed — in one
//! continuous view with our own scrolling.
//!
//! This module is the **parent**: it owns the sections ([`Sections`], rebuilt only when the
//! library or the tree options change), the scroll state ([`LightTable`]: the view's offset, the
//! scroll-to-section animation, the wheel and Ctrl+wheel) and the painting order. One module per
//! part does the rest, each behind a fixed boundary so the tasks that grow them never share a file:
//!
//! | Part | Called with | Returns |
//! |---|---|---|
//! | [`layout`] | the sections' photo counts, the width, the cell size, captions | the geometry, [`layout::Layout`] |
//! | [`scrollbar`] | the [`Frame`] and its rect, before the content ([`scrollbar::input`]) and after it ([`scrollbar::paint`]) | a [`scrollbar::ScrollRequest`] |
//! | [`input`] | the [`Frame`] before the cells ([`input::begin`]), the cells' [`cell::CellOut`]s after them ([`input::finish`]) | [`input::Pending`], applied by [`input::apply`] |
//! | [`header`] | the [`Frame`], the section, its rect and [`header::Form`] | [`header::HeaderOut`], applied by [`header::apply`] |
//! | [`cell`] | the [`Frame`], the [`Thumbnails`](super::thumbnails::Thumbnails), a [`cell::CellIn`] | a [`cell::CellOut`] |
//!
//! Each part keeps what it needs between frames in its own state struct ([`scrollbar::Scrollbar`],
//! [`input::Input`], [`header::Headers`], [`cell::Cells`]), held here by [`LightTable`]. A frame:
//! the background; the scroll bar's input and the wheel; the scroll resolved (anchor, animation);
//! [`input::begin`]; for each section on screen its header block, then its cells; the sticky header
//! over them; the scroll bar painted; [`input::finish`]; then, with the library no longer borrowed,
//! every part's output applied and the scroll anchor written back to [`UiState::scroll`].
//!
//! **Scrolling.** [`UiState::scroll`] is the position by content (a section and the offset below
//! its top). The light table writes it when the user moves the view (wheel, scroll bar); when
//! something else writes it (`section.scrollTo`, the scroll commands, `ui.set`), the view eases
//! there over [`SCROLL_SECONDS`] (ease-out, on the frame clock, which is virtual in headless runs)
//! and the anchor stays as written, even where the view stops short of it at the end of the
//! content. A resize, a new cell size or a caption change keeps the anchor, so the same photos stay
//! in view. A scroll part asks for either kind of move with a [`scrollbar::ScrollRequest`]; a
//! rubber band held at an edge with [`input::Pending::scroll`], applied after the cells, so the
//! next frame shows (and the band selects) the rows it brings in.

pub mod cell;
pub mod header;
pub mod input;
pub mod layout;
pub mod scrollbar;

use std::collections::HashMap;
use std::path::PathBuf;

use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_library::{Library, Photo, SectionId, Tree, TreeOptions};
use dalica_ui_kit::{Tokens, fonts, widgets};
use egui::{Align, Rect, Ui, UiBuilder, pos2, vec2};

use self::layout::{Layout, LayoutKey, Spacing};
use super::Panels;
use super::thumbnails::Motion;
use crate::app::App;
use crate::perf::{self, Counter};
use crate::state::{self, Caption, ScrollAnchor, UiState};

/// How long `section.scrollTo` (and any scroll written from outside) takes to arrive (seconds).
pub const SCROLL_SECONDS: f64 = 0.2;
/// The ids of the empty state's block and of its buttons (`control-protocol.md` §7).
pub const EMPTY_ID: &str = "empty:lightTable";
pub const ADD_FOLDER_ID: &str = "button:empty.addFolder";
pub const IMPORT_ID: &str = "button:empty.import";

/// One section of the light table: a folder's photos as the light table shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Its id (`folder:12`).
    pub id: state::Section,
    pub folder: FolderId,
    pub title: String,
    /// The date and count line under the title.
    pub subtitle: String,
    /// The photos shown, in display order (capture date, then name); small pictures only with
    /// View ▸ Small Pictures.
    pub photos: Vec<PhotoId>,
}

/// What the sections were built from.
#[derive(Debug, Clone, PartialEq)]
struct ModelKey {
    revision: u64,
    roots: Vec<PathBuf>,
    options: TreeOptions,
}

/// The sections in tree order ([`dalica_library::Tree`]), rebuilt when the library's revision,
/// the watched roots or the tree options change; [`Sections::generation`] counts the rebuilds.
#[derive(Debug, Default)]
pub struct Sections {
    key: Option<ModelKey>,
    generation: u64,
    pub list: Vec<Section>,
    index: HashMap<state::Section, usize>,
}

impl Sections {
    /// Rebuilds the sections when `library`, `roots` or `options` differ from the last build.
    /// Returns whether it rebuilt. One pass over the photos and one sort per folder.
    pub fn refresh(&mut self, library: &Library, roots: &[PathBuf], options: &TreeOptions) -> bool {
        let fresh = self.key.as_ref().is_some_and(|k| k.revision == library.revision() && k.roots == roots && k.options == *options);
        if fresh {
            return false;
        }
        self.list = build(library, roots, options);
        self.index = self.list.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        self.generation = self.generation.wrapping_add(1);
        self.key = Some(ModelKey { revision: library.revision(), roots: roots.to_vec(), options: options.clone() });
        true
    }

    /// Keeps the sections of the current library revision; on a new revision rebuilds them with
    /// the default tree options. The panels refresh them with the real options first each frame
    /// ([`refresh`]), so in the app this is a no-op.
    pub fn update(&mut self, library: &Library) {
        if self.key.as_ref().is_some_and(|k| k.revision == library.revision()) {
            return;
        }
        self.refresh(library, &[], &TreeOptions::default());
    }

    /// Counts the rebuilds: a layout built for one generation is stale on the next.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, id: state::Section) -> Option<&Section> {
        self.index.get(&id).and_then(|&i| self.list.get(i))
    }
}

/// The tree options the view state asks for (`organizing.md` §10). Folded tree-mode folders keep
/// their sections, so they are left out.
pub fn tree_options(s: &UiState) -> TreeOptions {
    use dalica_library::{FolderSort as LibSort, Group as LibGroup, TreeMode as LibMode};
    let group = |g: state::Group| match g {
        state::Group::Albums => LibGroup::Albums,
        state::Group::People => LibGroup::People,
        state::Group::Projects => LibGroup::Projects,
        state::Group::Folders => LibGroup::Folders,
        state::Group::Other => LibGroup::Other,
        state::Group::Hidden => LibGroup::Hidden,
    };
    TreeOptions {
        mode: match s.tree_mode {
            state::TreeMode::Flat => LibMode::Flat,
            state::TreeMode::Tree => LibMode::Tree,
        },
        sort: match s.folder_sort {
            state::FolderSort::Date => LibSort::Date,
            state::FolderSort::Recent => LibSort::RecentChanges,
            state::FolderSort::Size => LibSort::Size,
            state::FolderSort::Name => LibSort::Name,
        },
        reverse: s.folder_sort_reverse,
        collapsed_groups: s.collapsed_groups.iter().map(|&g| group(g)).collect(),
        small_pictures: s.small_pictures,
        hidden_pictures: s.hidden_pictures,
        ..TreeOptions::default()
    }
}

fn is_small(p: &Photo, threshold: u32) -> bool {
    p.width > 0 && p.height > 0 && p.width < threshold && p.height < threshold
}

fn build(library: &Library, roots: &[PathBuf], options: &TreeOptions) -> Vec<Section> {
    let tree = Tree::build(library, roots, options);
    let mut by_folder: HashMap<FolderId, Vec<&Photo>> = HashMap::new();
    for p in library.photos() {
        if options.small_pictures || !is_small(p, options.small_threshold) {
            by_folder.entry(p.folder).or_default().push(p);
        }
    }
    let mut out = Vec::with_capacity(tree.sections.len());
    for section in &tree.sections {
        let SectionId::Folder(folder) = *section;
        let Some(f) = library.folder(folder) else { continue };
        let mut photos = by_folder.remove(&folder).unwrap_or_default();
        if photos.is_empty() {
            continue;
        }
        photos.sort_by(|a, b| a.sort_date().cmp(&b.sort_date()).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
        let count = if photos.len() == 1 { "1 photo".to_string() } else { format!("{} photos", photos.len()) };
        let subtitle = match photos.iter().find_map(|p| p.sort_date()) {
            Some(d) => format!("{} · {count}", long_date(d.date())),
            None => count,
        };
        out.push(Section {
            id: state::Section::Folder(folder),
            folder,
            title: folder_name(&f.path),
            subtitle,
            photos: photos.iter().map(|p| p.id).collect(),
        });
    }
    out
}

/// `June 4, 2026`.
pub fn long_date(d: dalica_core::Date) -> String {
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let month = MONTHS.get(usize::from(d.month()).saturating_sub(1)).copied().unwrap_or("?");
    format!("{month} {}, {}", d.day(), d.year())
}

/// A folder's display name: its last path component.
pub fn folder_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The offset of `folder`'s section from the top of a light table `width` points wide with
/// `cell`-edged cells and no captions; `None` when it has no section. The tree's clicks use it only
/// to ask for a scroll; the light table itself scrolls with `section.scrollTo`.
pub fn offset_of(sections: &Sections, folder: FolderId, width: f32, cell: f32) -> Option<f32> {
    let layout = Layout::new(sections.list.iter().map(|s| (s.id, s.photos.len())), sections.generation, width, cell, false, Spacing::default());
    layout.offset_of(&ScrollAnchor { section: Some(state::Section::Folder(folder)), offset: 0.0 }).map(|y| y as f32)
}

/// What every part reads while one frame is painted. Built by the parent; parts never change it.
pub struct Frame<'a> {
    pub t: &'a Tokens,
    pub session: &'a Session,
    pub state: &'a UiState,
    pub sections: &'a Sections,
    pub layout: &'a Layout,
    /// The grid's area on screen (the light table without its scroll bar).
    pub area: Rect,
    /// The content offset at the area's top, snapped to device pixels.
    pub offset: f64,
    pub pixels_per_point: f32,
    /// The frame clock (seconds; virtual in headless runs).
    pub now: f64,
}

impl Frame<'_> {
    /// The screen y of content offset `y`.
    pub fn screen_y(&self, y: f64) -> f32 {
        self.area.min.y + (y - self.offset) as f32
    }

    /// `v` on the device-pixel grid, so edges stay crisp and sizes exact.
    pub fn snap(&self, v: f32) -> f32 {
        (v * self.pixels_per_point).round() / self.pixels_per_point
    }

    /// The view's height.
    pub fn view_h(&self) -> f32 {
        self.area.height()
    }
}

/// A scroll in progress towards an anchor written from outside the light table.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Animation {
    from: f64,
    to: ScrollAnchor,
    start: f64,
}

/// What the last frame showed, for tests and inspection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameInfo {
    /// The content offset at the view's top.
    pub offset: f64,
    /// The content's height.
    pub height: f64,
    /// The grid's area on screen.
    pub area: Option<Rect>,
    /// The header blocks drawn, with their on-screen rects (unclipped).
    pub headers: Vec<(state::Section, Rect)>,
    /// The pinned header, if any, with its on-screen rect.
    pub sticky: Option<(state::Section, Rect)>,
    /// Cells laid out.
    pub cells: usize,
    /// A scroll animation is running.
    pub animating: bool,
}

/// The light table's state between frames: the layout for the current key, the scroll position
/// and animation, and each part's own state.
#[derive(Default)]
pub struct LightTable {
    layout: Option<Layout>,
    /// The content offset at the view's top (points).
    offset: f64,
    /// [`UiState::scroll`] as of the end of the last frame: a different value now was written by
    /// someone else (a command, `ui.set`) and is eased to.
    written: ScrollAnchor,
    /// The view was moved by the user this frame (wheel, scroll bar): its anchor is written back.
    moved: bool,
    /// An anchor a part asked to ease to this frame, written back as the new scroll state.
    wanted: Option<ScrollAnchor>,
    animation: Option<Animation>,
    /// The previous frame's offset and clock, for the view's velocity.
    previous: Option<(f64, f64)>,
    info: FrameInfo,
    pub headers: header::Headers,
    pub cells: cell::Cells,
    pub scrollbar: scrollbar::Scrollbar,
    pub input: input::Input,
}

impl LightTable {
    /// The layout of the last frame.
    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// The content offset at the view's top.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// What the last frame showed.
    pub fn frame_info(&self) -> &FrameInfo {
        &self.info
    }

    /// Whether a scroll animation is running.
    pub fn animating(&self) -> bool {
        self.animation.is_some()
    }

    /// The layout for `key`, rebuilt when it changed. Returns whether it was rebuilt.
    fn prepare(&mut self, sections: &Sections, key: LayoutKey, width: f32, cell: f32, caption: bool, spacing: Spacing) -> bool {
        if self.layout.as_ref().is_some_and(|l| l.key() == key && l.spacing() == spacing) {
            return false;
        }
        self.layout = Some(Layout::new(sections.list.iter().map(|s| (s.id, s.photos.len())), sections.generation(), width, cell, caption, spacing));
        true
    }

    /// Resolves this frame's offset: an anchor written from outside starts an animation towards
    /// it, a running animation advances, a new layout keeps the anchor in view.
    fn resolve(&mut self, wanted: &ScrollAnchor, relaid: bool, view_h: f32, now: f64) {
        let Some(layout) = &self.layout else { return };
        if *wanted != self.written {
            self.animation = Some(Animation { from: self.offset, to: *wanted, start: now });
            self.written = *wanted;
        } else if relaid && self.animation.is_none() {
            self.offset = layout.offset_of(wanted).unwrap_or(self.offset);
        }
        if let Some(a) = self.animation {
            let to = layout.clamp(layout.offset_of(&a.to).unwrap_or(a.from), view_h);
            let p = if now.is_finite() && a.start.is_finite() { ((now - a.start) / SCROLL_SECONDS).clamp(0.0, 1.0) } else { 1.0 };
            if p >= 1.0 - 1e-6 {
                self.offset = to;
                self.animation = None;
            } else {
                let eased = 1.0 - (1.0 - p).powi(3);
                self.offset = a.from + (to - a.from) * eased;
            }
        }
        self.offset = layout.clamp(self.offset, view_h);
    }

    /// Moves the view at once (wheel, scroll-bar drag); stops an animation.
    fn jump(&mut self, offset: f64, view_h: f32) {
        self.animation = None;
        self.moved = true;
        if let Some(l) = &self.layout {
            self.offset = l.clamp(offset, view_h);
        }
    }
}

/// Rebuilds the sections for the current library and view state. The panels call it once per
/// frame, before any region reads them.
pub fn refresh(p: &mut Panels, app: &App) {
    if let Some(session) = app.session() {
        p.sections.refresh(session.library(), session.roots(), &tree_options(app.state()));
    }
}

/// Paints the light table: the grid in `area`, the scroll bar in `bar`.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui, area: Rect, bar: Rect) {
    let t = *app.tokens();
    ui.painter().rect_filled(area.union(bar), 0, t.grid_bg);
    let Some(session) = app.session() else { return };
    if p.sections.list.is_empty() {
        p.light_table.info = FrameInfo { area: Some(area), ..FrameInfo::default() };
        perf::count(ui.ctx(), Counter::CellsVisited, 0);
        if session.roots().is_empty() && session.library().folders().next().is_none() {
            empty_state(app, ui, area);
        }
        return;
    }
    let state = app.state();
    let caption = state.caption != Caption::None;
    let spacing = Spacing::from_metrics(&t.metrics);
    let key = LayoutKey::new(p.sections.generation(), area.width(), state.thumb_size, caption);
    let lt = &mut p.light_table;
    let relaid = lt.prepare(&p.sections, key, area.width(), state.thumb_size, caption, spacing);
    let (now, ppp) = ui.input(|i| (i.time, i.pixels_per_point()));
    let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
    let view_h = area.height();
    lt.resolve(&state.scroll, relaid, view_h, now);

    // Input that moves the view comes first, so this frame paints where it lands.
    let mut zoom = 1.0;
    if ui.rect_contains_pointer(area.union(bar)) {
        let (wheel, z) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if wheel != 0.0 && wheel.is_finite() {
            lt.jump(lt.offset - f64::from(wheel), view_h);
        }
        zoom = z;
    }
    let Some(layout) = lt.layout.as_ref() else { return };
    let request = {
        let f = Frame { t: &t, session, state, sections: &p.sections, layout, area, offset: lt.offset, pixels_per_point: ppp, now };
        scrollbar::input(ui, &f, &mut lt.scrollbar, bar)
    };
    match request {
        Some(scrollbar::ScrollRequest::To(y)) => lt.jump(y, view_h),
        Some(scrollbar::ScrollRequest::Animate(to)) => {
            lt.animation = Some(Animation { from: lt.offset, to, start: now });
            lt.wanted = Some(to);
        }
        None => {}
    }
    let Some(layout) = lt.layout.as_ref() else { return };
    let offset = (lt.offset * f64::from(ppp)).round() / f64::from(ppp);
    let f = Frame { t: &t, session, state, sections: &p.sections, layout, area, offset, pixels_per_point: ppp, now };
    let mut info = FrameInfo { offset, height: layout.height(), area: Some(area), animating: lt.animation.is_some(), ..FrameInfo::default() };

    // The content, clipped to the grid's area.
    let mut content = ui.new_child(UiBuilder::new().id_salt("lightTable.content").max_rect(area));
    content.set_clip_rect(area.intersect(ui.clip_rect()));
    let background = input::begin(&mut content, &f, &mut lt.input);
    let library = session.library();
    let grid = layout.grid();
    let mut cells = Vec::new();
    let mut header_outs = Vec::new();
    let visible = layout.visible(offset, view_h);
    for v in &visible {
        let Some(s) = p.sections.get(v.section) else { continue };
        let rect = Rect::from_min_size(pos2(area.min.x, f.screen_y(v.top)), vec2(area.width(), spacing.header));
        header_outs.push(header::show(&mut content, &f, &mut lt.headers, s, rect, header::Form::Block));
        info.headers.push((s.id, rect));
        for k in v.cells.clone() {
            let (Some(&id), Some((x, y))) = (s.photos.get(k), layout.cell_origin(v.index, k)) else { continue };
            let Some(photo) = library.photo(id) else { continue };
            let rect = Rect::from_min_size(pos2(f.snap(area.min.x + x), f.snap(f.screen_y(y))), vec2(grid.cell, grid.cell));
            let caption = caption.then(|| Rect::from_min_size(rect.left_bottom(), vec2(rect.width(), spacing.caption)));
            let cell = cell::CellIn { photo, section: s.id, rect, caption };
            cells.push(cell::show(&mut content, &f, &mut lt.cells, &mut p.thumbnails, &cell));
        }
    }
    info.cells = cells.len();
    // The pinned header goes over the cells scrolling under it.
    if let Some(st) = layout.sticky(offset)
        && let Some(s) = p.sections.get(st.section)
    {
        let rect = Rect::from_min_size(pos2(area.min.x, area.min.y + st.y as f32), vec2(area.width(), spacing.sticky));
        header_outs.push(header::show(&mut content, &f, &mut lt.headers, s, rect, header::Form::Sticky));
        info.sticky = Some((s.id, rect));
    }
    scrollbar::paint(ui, &f, &mut lt.scrollbar, bar);
    let pending = input::finish(&mut content, &f, &mut lt.input, background, &cells);
    // A band held at an edge moves the view as the user's own scroll; the next frame paints it.
    if let Some(a) = pending.scroll {
        let to = layout.clamp(a.apply(lt.offset), view_h);
        if to != lt.offset {
            lt.animation = None;
            lt.moved = true;
            lt.offset = to;
            ui.ctx().request_repaint();
        }
    }
    perf::count(ui.ctx(), Counter::CellsVisited, cells.len() as u64);
    perf::count(ui.ctx(), Counter::SectionsVisited, visible.len() as u64);

    // How the view moved, for prefetch.
    let velocity = match lt.previous {
        Some((y, at)) if now > at => (offset - y) / (now - at),
        _ => 0.0,
    };
    lt.previous = Some((offset, now));
    let motion = Motion { offset, view_h, velocity };
    // The anchor: where the user moved the view, else what was asked for (kept as asked, even when
    // the view stops short of it at the end of the content, so it survives a resize).
    let anchor = match (std::mem::take(&mut lt.moved), lt.wanted.take()) {
        (true, _) => Some(layout.anchor_at(lt.offset)),
        (false, wanted) => wanted,
    };
    if lt.animation.is_some() {
        ui.ctx().request_repaint();
    }
    lt.info = info;
    if let Some(a) = anchor {
        lt.written = a;
    }

    // The library is no longer borrowed: apply what the parts asked for.
    input::apply(app, pending);
    for out in header_outs {
        header::apply(app, out);
    }
    if let Some(s) = app.session_mut() {
        p.thumbnails.set_order(&p.sections);
        p.thumbnails.end_frame(s, &motion);
    }
    let state = app.state_mut();
    if let Some(a) = anchor {
        state.scroll = a;
    }
    // Ctrl+wheel: the thumbnail size; the anchor keeps the view (the next frame lays out for it).
    if zoom.is_finite() && zoom > 0.0 && zoom != 1.0 {
        state.thumb_size = state::thumb_size(state.thumb_size * zoom);
    }
}

/// "No pictures yet" with Add a Folder… and Import… (`ui-layout.md` §5.3), centred in `area`. Add a
/// Folder… opens the Folder Manager, as File ▸ Add Folder… does (`organizing.md` §2).
fn empty_state(app: &mut App, ui: &mut Ui, area: Rect) {
    let t = *app.tokens();
    let mut child = ui.new_child(UiBuilder::new().id_salt("lightTable.empty").max_rect(area));
    let body = "Add a folder of photos to see them here, or import them from a camera or a card.";
    widgets::empty_state(&mut child, &t, EMPTY_ID, "No pictures yet", body, None);
    let can_add = crate::dialogs::can_open(app, crate::dialogs::DialogKind::FolderManager).is_ok();
    let can_import = available(app, "import.open");
    let width = |label: &str| {
        let g = child.painter().layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), t.text);
        (g.size().x + 18.0).max(37.0).round()
    };
    let (add, import) = ("Add a Folder…", "Import…");
    let (wa, wi) = (width(add), width(import));
    const GAP: f32 = 8.0;
    let top = (child.cursor().top() + 16.0).round();
    let left = (area.center().x - (wa + GAP + wi) / 2.0).round();
    let slot = |x: f32, w: f32| {
        UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, top), vec2(w, t.metrics.button_h))).layout(egui::Layout::left_to_right(Align::Center))
    };
    let added = child
        .scope_builder(slot(left, wa), |ui| {
            if !can_add {
                ui.disable();
            }
            widgets::primary_button(ui, &t, ADD_FOLDER_ID, add).clicked()
        })
        .inner;
    let imported = child
        .scope_builder(slot(left + wa + GAP, wi), |ui| {
            if !can_import {
                ui.disable();
            }
            widgets::text_button(ui, &t, IMPORT_ID, import, false).clicked()
        })
        .inner;
    if added {
        let _ = app.run("folders.manager", &serde_json::json!({}));
    }
    if imported {
        let _ = app.run("import.open", &serde_json::json!({}));
    }
}

/// Whether command `id` exists here (a UI command or one the session registered).
fn available(app: &App, id: &str) -> bool {
    crate::commands::UI_COMMANDS.iter().any(|c| c.id == id) || app.session().is_some_and(|s| s.spec(id).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_words() {
        assert_eq!(long_date(dalica_core::Date::new(2026, 6, 4).unwrap()), "June 4, 2026");
    }

    #[test]
    fn folder_names_are_the_last_component() {
        assert_eq!(folder_name(std::path::Path::new("/photos/2026/Lisbon")), "Lisbon");
        assert_eq!(folder_name(std::path::Path::new("/")), "/");
    }
}
