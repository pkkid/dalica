//! The window for screen readers (`SYS-A11Y`, `ui-layout.md` → Keyboard and screen readers): the
//! AccessKit tree egui builds from the widgets (each `ui-kit` widget gives its role, name and state,
//! M15.14) reshaped into the window's, and keyboard-only use ([`keyboard`]).
//!
//! - **Landmarks**: each region of the window is a named landmark around its nodes: the menu bar,
//!   the toolbar, the room tabs, the library tree (navigation), the light table (main), the side
//!   pane (complementary, named after the pane), the tray and its button rows; in the edit view its
//!   tool bar, filmstrip, edit panel, the photo (main: "Edit view") and caption bar; a room in front
//!   is the main landmark named after the room; a slideshow and the photo viewer are one main
//!   landmark each.
//! - **The light table**: its visible cells are list items (the file name; starred, edited, a
//!   video's duration and the whole caption as the description; selected), inside a named group per
//!   section (the section's title). Cells off screen are never drawn, so never in the tree: the tree
//!   grows with the window, never with the library.
//! - **The trees**: the library tree's visible rows are tree items inside a tree per group (named
//!   after the group), the Folder Manager's inside one tree; each row says its level, whether it is
//!   expanded (rows with rows below), its place among its siblings (position and set size: the
//!   rows out of view are not in the tree) and, in the library tree, its photo count. The tree is
//!   one Tab stop: the focused row is the item a screen reader announces
//!   ([`keyboard`]).
//! - **Canvases**: the edit view's photo, the other photo of a comparison, the collage, the
//!   slideshow's and the viewer's photo are images named after what they show, with their state
//!   (zoom, the open tool, the photo's place in the show). The map keeps `ui-kit`'s group with its
//!   zoom: its markers are controls inside it, which an image could not hold.
//! - **Dialogs** are `ui-kit`'s modal dialogs with their titles.
//!
//! [`describe`] gathers what the frame drew (only when AccessKit is on: a screen reader asked for
//! the tree) into a [`reshape::Shape`]; the [`Plugin`] applies it to egui's tree update at the end
//! of the pass ([`reshape::reshape`]). With AccessKit off neither does any work.

pub mod keyboard;
pub mod reshape;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_ui_kit::registry::{self, Role as Kind, WidgetInfo};
use egui::accesskit::{NodeId, Role};
use egui::{Context, Id, Rect};

use self::reshape::{Patch, Shape, Wrap};
use crate::app::App;
use crate::state::{Pane, Room, View, Zoom};
use crate::{tl, tln};

fn shape_key() -> Id {
    Id::new("dalica.a11y.shape")
}

/// Whether this pass builds an AccessKit tree (a screen reader, or a test, turned it on).
pub fn on(ctx: &Context) -> bool {
    ctx.viewport(|v| v.this_pass.accesskit_state.is_some())
}

/// The egui plugin that reshapes each pass's tree update with what [`describe`] gathered.
pub struct Plugin;

impl egui::Plugin for Plugin {
    fn debug_name(&self) -> &'static str {
        "dalica.a11y"
    }

    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        let shape = ctx.data_mut(|d| d.remove_temp::<Arc<Shape>>(shape_key()));
        if let (Some(update), Some(shape)) = (output.platform_output.accesskit_update.as_mut(), shape) {
            reshape::reshape(update, &shape);
        }
    }
}

/// Adds the [`Plugin`] to `ctx` (once; [`App::install`](crate::App::install) does).
pub fn install(ctx: &Context) {
    ctx.add_plugin(Plugin);
}

/// Gathers what this frame drew for the tree's reshape; call at the end of the frame, once every
/// panel and dialog is drawn. Nothing while AccessKit is off.
pub fn describe(app: &App, ctx: &Context) {
    if !on(ctx) {
        return;
    }
    let widgets = registry::widgets(ctx);
    let ids: Vec<Id> = widgets.iter().filter_map(|w| w.egui_id).collect();
    // The widgets on the window's own layer (not a menu, tooltip or dialog over it).
    let background: HashSet<Id> = ctx.viewport(|v| {
        ids.into_iter().filter(|id| v.this_pass.widgets.get(*id).is_some_and(|r| r.layer_id.order == egui::Order::Background)).collect()
    });
    let mut shape = Shape::default();
    cells(app, &widgets, &mut shape);
    let trees = tree_rows(ctx, &widgets, &mut shape);
    canvases(app, &widgets, &mut shape);
    let mut sections = sections(app, &widgets);
    sections.extend(trees);
    let landmarks = landmarks(app, ctx, &widgets, &background);
    shape.batches = vec![sections, landmarks];
    ctx.data_mut(|d| d.insert_temp(shape_key(), Arc::new(shape)));
}

fn rect_of(w: &WidgetInfo) -> Rect {
    let [x, y, width, height] = w.rect;
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
}

fn bounds(r: Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect { x0: f64::from(r.min.x), y0: f64::from(r.min.y), x1: f64::from(r.max.x), y1: f64::from(r.max.y) }
}

fn node(id: Id) -> NodeId {
    id.accesskit_id()
}

/// The photo of a cell's widget id: `thumb:12` and `thumb:12/in/<album>` are photo 12.
fn cell_photo(widget: &str) -> Option<PhotoId> {
    let rest = widget.strip_prefix("thumb:")?;
    let n = rest.split('/').next()?.parse::<u64>().ok()?;
    Some(PhotoId::new(n))
}

/// A duration as a cell's pill shows it: `1:23`, `1:02:03`.
fn duration(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The light table's visible cells: selected, and the badges and the whole caption as the
/// description (the cell elides a long caption; the description keeps all of it).
fn cells(app: &App, widgets: &[WidgetInfo], shape: &mut Shape) {
    let Some(panels) = app.panels() else { return };
    let shown: HashMap<PhotoId, &crate::panels::light_table::cell::Shown> = panels.light_table.cells.shown().iter().map(|s| (s.photo, s)).collect();
    let library = app.session().map(|s| s.library());
    for w in widgets.iter().filter(|w| w.role == Kind::Thumbnail) {
        let (Some(egui_id), Some(photo)) = (w.egui_id, cell_photo(&w.id)) else { continue };
        let Some(s) = shown.get(&photo) else { continue };
        let mut parts = Vec::new();
        if s.badges.starred {
            parts.push(tl!("Starred"));
        }
        if s.badges.edited {
            parts.push(tl!("Edited"));
        }
        if let Some(video) = s.badges.video {
            parts.push(match video.seconds {
                Some(secs) => tl!("Video, {duration}", duration = duration(secs)),
                None => tl!("Video"),
            });
        }
        let caption = library.map(|l| l.caption(photo).to_owned()).filter(|c| !c.trim().is_empty()).or_else(|| s.caption.clone());
        if let Some(caption) = caption.filter(|c| !c.trim().is_empty()) {
            parts.push(tl!("Caption: {caption}", caption = caption));
        }
        let mut p = Patch::new(node(egui_id));
        p.selected = Some(s.selected);
        if !parts.is_empty() {
            p.description = Some(parts.join(", "));
        }
        shape.patches.push(p);
    }
}

/// The tree pane's visible rows: level, expanded, their place among their siblings and the photo
/// count; and the trees round them: one per group of the library tree, one for the Folder
/// Manager's (whose rows set their own state as they are drawn).
fn tree_rows(ctx: &Context, widgets: &[WidgetInfo], shape: &mut Shape) -> Vec<Wrap> {
    use crate::panels::folders::{self, model::Kind as Item};
    let rows: HashMap<&str, (Id, Rect)> =
        widgets.iter().filter(|w| w.role == Kind::TreeRow).filter_map(|w| Some((w.id.as_str(), (w.egui_id?, rect_of(w))))).collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let model = folders::current_model(ctx);
    let info = folders::info(ctx);
    let mut groups: Vec<(dalica_library::Group, Vec<NodeId>, Rect)> = Vec::new();
    for i in model.visible(info.offset, info.view_h) {
        let Some(Item::Row(r)) = model.items.get(i).map(|it| &it.kind) else { continue };
        let Some((id, rect)) = rows.get(r.id.as_str()) else { continue };
        let mut p = Patch::new(node(*id));
        p.level = Some(r.level());
        p.expanded = r.expanded_state();
        p.set = model.set_of(i);
        p.description = Some(tln!(r.count, "{n} photo", "{n} photos"));
        shape.patches.push(p);
        let Some(group) = model.group_of(i) else { continue };
        match groups.iter_mut().find(|g| g.0 == group) {
            Some(g) => {
                g.1.push(node(*id));
                g.2 = g.2.union(*rect);
            }
            None => groups.push((group, vec![node(*id)], *rect)),
        }
    }
    let mut out: Vec<Wrap> = groups
        .into_iter()
        .map(|(group, members, rect)| Wrap {
            id: node(Id::new(("dalica.a11y.tree", group))),
            role: Role::Tree,
            label: folders::model::group_name(group),
            bounds: Some(bounds(rect)),
            members,
        })
        .collect();
    let (members, rects): (Vec<NodeId>, Vec<Rect>) =
        rows.iter().filter(|(id, _)| id.starts_with(crate::dialogs::folder_manager::ROW_PREFIX)).map(|(_, (id, r))| (node(*id), *r)).unzip();
    if let Some(rect) = rects.into_iter().reduce(Rect::union) {
        out.push(Wrap {
            id: node(Id::new("dalica.a11y.tree.folderManager")),
            role: Role::Tree,
            label: tl!("Folders"),
            bounds: Some(bounds(rect)),
            members,
        });
    }
    out
}

fn zoom_text(zoom: Zoom) -> String {
    match zoom {
        Zoom::Fit => tl!("Fit to the window"),
        Zoom::Scale(s) => tl!("Zoom {percent} %", percent = (s * 100.0).round()),
    }
}

/// The canvases as named images with their state.
fn canvases(app: &App, widgets: &[WidgetInfo], shape: &mut Shape) {
    let state = app.state();
    let name = |photo: Option<PhotoId>| -> Option<String> { app.session()?.library().photo(photo?).map(|p| p.name.clone()) };
    for w in widgets {
        let Some(egui_id) = w.egui_id else { continue };
        let (label, value, description) = match w.id.as_str() {
            "canvas:edit" => {
                let tool = state.edit.tool.map(|t| tl!("{tool} tool open", tool = t.name()));
                (name(state.edit.photo).unwrap_or_else(|| tl!("Photo")), Some(zoom_text(state.edit.zoom)), tool)
            }
            "canvas:edit.other" => (name(state.edit.second).unwrap_or_else(|| tl!("Other photo")), None, Some(tl!("Compared photo"))),
            "canvas:collage" => (tl!("Collage"), Some(tln!(state.collage.clips.len(), "{n} photo", "{n} photos")), None),
            "canvas:slideshow" => {
                let show = state.slideshow.as_ref();
                let place = show.map(|s| tl!("Photo {index} of {count}", index = s.index.saturating_add(1), count = s.photos.len()));
                (name(show.and_then(|s| s.photo())).unwrap_or_else(|| tl!("Slideshow")), place, None)
            }
            "canvas:viewer" => {
                let file = app.session().and_then(|s| s.viewer().file()).and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned());
                (file.unwrap_or_else(|| tl!("Photo viewer")), state.viewer.as_ref().map(|v| zoom_text(v.zoom)), None)
            }
            _ => continue,
        };
        let mut p = Patch::new(node(egui_id));
        p.role = Some(Role::Image);
        p.label = Some(label);
        p.value = value;
        p.description = description;
        shape.patches.push(p);
    }
}

/// The light table's sections drawn this frame, each a named group round its header and its
/// visible cells.
fn sections(app: &App, widgets: &[WidgetInfo]) -> Vec<Wrap> {
    let state = app.state();
    if state.view != View::Library || state.room != Room::Library || state.unnamed.open {
        return Vec::new();
    }
    let Some(panels) = app.panels() else { return Vec::new() };
    let info = panels.light_table.frame_info();
    let Some(area) = info.area else { return Vec::new() };
    let mut headers: Vec<(crate::state::Section, f32)> = info.headers.iter().map(|(s, r)| (*s, r.top())).collect();
    headers.sort_by(|a, b| a.1.total_cmp(&b.1));
    // Above the first header drawn: the section whose header is pinned at the top.
    let mut starts: Vec<(crate::state::Section, f32)> = info.sticky.map(|(s, _)| (s, f32::NEG_INFINITY)).into_iter().collect();
    for (s, top) in headers {
        // The pinned section's own header, scrolling away under the pin, starts no new span.
        if starts.last().is_none_or(|(last, _)| *last != s) {
            starts.push((s, top));
        }
    }
    let spans: Vec<(crate::state::Section, f32, f32)> =
        starts.iter().enumerate().map(|(i, (s, top))| (*s, *top, starts.get(i + 1).map_or(f32::INFINITY, |n| n.1))).collect();
    let labels: HashMap<&str, &str> =
        widgets.iter().filter(|w| w.id.starts_with("section:") && !w.id.contains('/')).map(|w| (w.id.as_str(), w.label.as_str())).collect();
    let mut out = Vec::new();
    for (section, top, bottom) in spans {
        let prefix = format!("section:{section}");
        let members: Vec<NodeId> = widgets
            .iter()
            .filter(|w| {
                let r = rect_of(w);
                let in_span = r.center().y >= top && r.center().y < bottom && area.contains(r.center());
                (w.role == Kind::Thumbnail && in_span) || ((w.id == prefix || w.id.starts_with(&format!("{prefix}/"))) && in_span)
            })
            .filter_map(|w| w.egui_id.map(node))
            .collect();
        if members.is_empty() {
            continue;
        }
        let span = Rect::from_x_y_ranges(area.x_range(), top.max(area.top())..=bottom.min(area.bottom()));
        out.push(Wrap {
            id: node(Id::new(("dalica.a11y.section", prefix.as_str()))),
            role: Role::Group,
            label: labels.get(prefix.as_str()).map_or_else(|| prefix.clone(), |l| (*l).to_owned()),
            bounds: Some(bounds(span)),
            members,
        });
    }
    out
}

/// What a region of the layout is to a screen reader: its landmark (regions drawn as one, the
/// light table and its scroll bar, share one), role and name. `None` for a region that is no
/// landmark (the splitter).
fn landmark(app: &App, region: &str) -> Option<(&'static str, Role, String)> {
    let state = app.state();
    Some(match region {
        "menuBar" => ("menuBar", Role::MenuBar, tl!("Menu bar")),
        "tabStrip" => ("tabStrip", Role::TabList, tl!("Rooms")),
        "toolbar" => ("toolbar", Role::Toolbar, tl!("Toolbar")),
        "tree" => ("tree", Role::Navigation, tl!("Library tree")),
        "banner" => ("banner", Role::Region, tl!("Search results")),
        "lightTable" | "scrollbar" => ("lightTable", Role::Main, tl!("Light table")),
        "sidePane" => {
            let name = match state.pane {
                Some(Pane::People) => tl!("People"),
                Some(Pane::Places) => tl!("Places"),
                Some(Pane::Tags) => tl!("Tags"),
                Some(Pane::Properties) => tl!("Properties"),
                None => tl!("Side pane"),
            };
            ("sidePane", Role::Complementary, name)
        }
        "statusLine" => ("statusLine", Role::Status, tl!("Status line")),
        "tray" => ("tray", Role::Region, tl!("Photo tray")),
        "trayButtons" => ("trayButtons", Role::Toolbar, tl!("Tray buttons")),
        "bottomControls" => ("bottomControls", Role::Toolbar, tl!("View controls")),
        "actionButtons" => ("actionButtons", Role::Toolbar, tl!("Actions")),
        "editTopBar" => ("editTopBar", Role::Toolbar, tl!("Edit toolbar")),
        "filmstrip" => ("filmstrip", Role::Region, tl!("Filmstrip")),
        "editPanel" => ("editPanel", Role::Region, tl!("Edit panel")),
        "canvas" => ("canvas", Role::Main, tl!("Edit view")),
        "captionBar" => ("captionBar", Role::Region, tl!("Caption")),
        "room" => {
            let name = match state.room {
                Room::Import => tl!("Import"),
                Room::Collage => tl!("Collage"),
                Room::Library => tl!("Library"),
            };
            ("room", Role::Main, name)
        }
        _ => return None,
    })
}

/// The window's landmarks: one per region with something drawn in it; a slideshow or the viewer,
/// which fill the window, one for the whole window.
fn landmarks(app: &App, ctx: &Context, widgets: &[WidgetInfo], background: &HashSet<Id>) -> Vec<Wrap> {
    let mut areas: Vec<(&'static str, Role, String, Vec<Rect>)> = Vec::new();
    match app.state().view {
        View::Slideshow => areas.push(("view", Role::Main, tl!("Slideshow"), vec![ctx.content_rect()])),
        View::Viewer => areas.push(("view", Role::Main, tl!("Photo viewer"), vec![ctx.content_rect()])),
        View::Library | View::Edit => {
            for w in widgets.iter().filter(|w| w.id.starts_with("region:")) {
                let Some((key, role, label)) = landmark(app, w.id.trim_start_matches("region:")) else { continue };
                match areas.iter_mut().find(|a| a.0 == key) {
                    Some(a) => a.3.push(rect_of(w)),
                    None => areas.push((key, role, label, vec![rect_of(w)])),
                }
            }
        }
    }
    let mut out = Vec::new();
    for (key, role, label, rects) in areas {
        let members: Vec<NodeId> = widgets
            .iter()
            .filter(|w| !w.id.starts_with("region:") && w.egui_id.is_some_and(|id| background.contains(&id)))
            .filter(|w| rects.iter().any(|r| r.contains(rect_of(w).center())))
            .filter_map(|w| w.egui_id.map(node))
            .collect();
        if members.is_empty() {
            continue;
        }
        let union = rects.iter().copied().reduce(Rect::union).unwrap_or(Rect::NOTHING);
        out.push(Wrap { id: node(Id::new(("dalica.a11y.landmark", key))), role, label, bounds: Some(bounds(union)), members });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cells_photo_and_a_videos_duration_read_as_drawn() {
        assert_eq!(cell_photo("thumb:12"), Some(PhotoId::new(12)));
        assert_eq!(cell_photo("thumb:12/in/starred"), Some(PhotoId::new(12)));
        assert_eq!(cell_photo("thumb:x"), None);
        assert_eq!(cell_photo("film:12"), None);
        assert_eq!((duration(83), duration(5), duration(3723)), ("1:23".into(), "0:05".into(), "1:02:03".into()));
    }
}
