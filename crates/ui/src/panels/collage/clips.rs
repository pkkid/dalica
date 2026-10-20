//! The Collage room's Clips tab (`creating.md` §3): every picture the collage can use, as a grid
//! of thumbnails (`clip:<PhotoId>`, active while selected; those on the page carry a check), and
//! along the bottom + (`button:collage.addClips`: the selected clips onto the page), −
//! (`button:collage.removeClips`: out of the Clips) and Get More (`button:collage.getMore`: the
//! Library tab, with the tray's Back to Collage). The tab's label counts the clips ("Clips (N)").
//!
//! A click selects a clip (Ctrl toggles it, Shift adds the run from the last one clicked) through
//! `collage.selectClips`; the wheel scrolls the grid. The thumbnails are the session's ladder images
//! in memory (`Session::thumbnail`, which asks for what is missing; never a decode or a disk read
//! here), else the photo's colour sketch, else a placeholder, uploaded as textures at most
//! [`UPLOADS_PER_FRAME`] a frame and dropped once unseen for [`KEEP_PASSES`] passes; each is
//! drawn whole in its square cell, turned as its recipe turns it. A clip that is a video (M15.28:
//! its poster frame is its thumbnail) carries the light table's sprocket edge and duration pill
//! (`clip:<PhotoId>/duration`, its text the length as `0:04`).

use std::collections::HashMap;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_edit::Dihedral;
use dalica_engine::{Session, Thumb};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align, Color32, Context, CornerRadius, Id, Layout, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, UiBuilder, pos2, vec2,
};
use serde_json::{Value, json};

use crate::app::App;
use crate::panels::thumbnails::{fit_within, shape_of, turn_point};
use crate::tl;

/// A clip's cell (points, square) and the gap between cells.
pub const CELL: f32 = 76.0;
pub const GAP: f32 = 6.0;
/// The height of the buttons' row along the bottom.
pub const FOOT_H: f32 = 24.0;
/// Most thumbnails uploaded as textures in one frame.
pub const UPLOADS_PER_FRAME: usize = 24;
/// Passes a texture is kept once its cell is no longer drawn.
pub const KEEP_PASSES: u64 = 120;
/// The long side a colour sketch is uploaded at.
const SKETCH_SIDE: u32 = 48;
pub const ADD_ID: &str = "button:collage.addClips";
pub const REMOVE_ID: &str = "button:collage.removeClips";
pub const GET_MORE_ID: &str = "button:collage.getMore";
/// What the tab says with no clips.
pub const EMPTY: &str = "No pictures yet. Click Get More to choose some.";

/// A clip's automation id: `clip:<PhotoId>`.
pub fn clip_id(photo: PhotoId) -> String {
    format!("clip:{photo}")
}

/// The automation id of a video clip's duration pill: `clip:<PhotoId>/duration`.
pub fn duration_id(photo: PhotoId) -> String {
    format!("clip:{photo}/duration")
}

/// What a clip's texture shows: a ladder image (by its address, so a better one replaces it) or
/// the colour sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Image(usize),
    Sketch,
}

#[derive(Clone)]
struct Slot {
    handle: TextureHandle,
    source: Source,
    used: u64,
}

/// The clips' textures, kept in egui's memory between frames.
#[derive(Clone, Default)]
struct Textures {
    map: HashMap<PhotoId, Slot>,
    uploads: usize,
    pass: u64,
    /// Cells drawn this pass whose ladder image is not in memory yet.
    waiting: usize,
}

fn textures_id() -> Id {
    Id::new("collage.clips.textures")
}

fn scroll_id() -> Id {
    Id::new("collage.clips.scroll")
}

fn anchor_id() -> Id {
    Id::new("collage.clips.anchor")
}

impl Textures {
    /// Photo `id`'s texture for a cell of `need` device pixels, uploading what the session has in
    /// memory (within this pass's budget); `None` while there is nothing to draw.
    fn get(&mut self, ctx: &Context, session: &mut Session, id: PhotoId, need: u32) -> Option<egui::TextureId> {
        let thumb = session.thumbnail(id, need);
        let (source, image) = match thumb {
            Thumb::Image { image, covers, .. } => {
                if !covers {
                    self.waiting += 1;
                }
                (Source::Image(Arc::as_ptr(&image) as usize), Some(image))
            }
            Thumb::Sketch(_) | Thumb::NotScanned => {
                self.waiting += 1;
                (Source::Sketch, None)
            }
        };
        if let Some(slot) = self.map.get_mut(&id)
            && (slot.source == source || self.uploads >= UPLOADS_PER_FRAME)
        {
            slot.used = self.pass;
            return Some(slot.handle.id());
        }
        if self.uploads >= UPLOADS_PER_FRAME {
            return None;
        }
        let rgba = match image {
            Some(image) => image,
            None => Arc::new(dalica_thumbs::sketch::decode(session.library().sketch(id)?.as_bytes()).ok()?.to_rgba(SKETCH_SIDE)),
        };
        let size = [usize::try_from(rgba.width).ok()?, usize::try_from(rgba.height).ok()?];
        if size[0] == 0 || size[1] == 0 || size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) != Some(rgba.data.len()) {
            return None;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &rgba.data);
        let handle = ctx.load_texture(format!("collage.clip:{id}"), pixels, egui::TextureOptions::LINEAR);
        let tex = handle.id();
        self.map.insert(id, Slot { handle, source, used: self.pass });
        self.uploads += 1;
        Some(tex)
    }
}

/// The upright picture in `texture` drawn over `rect` turned by `turn`.
fn paint_turned(painter: &egui::Painter, rect: Rect, texture: egui::TextureId, turn: Dihedral) {
    if !(rect.is_finite() && rect.is_positive()) {
        return;
    }
    let back = turn.inverse();
    let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut mesh = Mesh::with_texture(texture);
    for (x, y) in corners {
        let (u, v) = turn_point(back, x, y);
        let pos = pos2(rect.min.x + x * rect.width(), rect.min.y + y * rect.height());
        mesh.vertices.push(egui::epaint::Vertex { pos, uv: Pos2::new(u, v), color: Color32::WHITE });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(mesh));
}

/// The on-page check: a small `accent` plate with an `on_accent` tick in the cell's top-right
/// corner.
fn on_page_mark(painter: &egui::Painter, t: &Tokens, cell: Rect) {
    let plate = Rect::from_min_size(pos2(cell.max.x - 15.0, cell.min.y + 3.0), vec2(12.0, 12.0));
    painter.rect_filled(plate, CornerRadius::same(2), t.accent);
    let at = |x: f32, y: f32| pos2(plate.min.x + x * plate.width(), plate.min.y + y * plate.height());
    painter.add(Shape::line(vec![at(0.22, 0.52), at(0.42, 0.72), at(0.78, 0.3)], Stroke::new(1.5, t.on_accent)));
}

/// The clips selected after a click on `id` with Ctrl (`toggle`) or Shift (`range`, from `anchor`
/// in `order`), else `id` alone.
pub fn clicked_selection(order: &[PhotoId], selected: &[PhotoId], id: PhotoId, toggle: bool, range: bool, anchor: Option<PhotoId>) -> Vec<PhotoId> {
    if range && let (Some(a), Some(b)) = (anchor.and_then(|a| order.iter().position(|x| *x == a)), order.iter().position(|x| *x == id)) {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut out = selected.to_vec();
        out.extend(order.get(lo..=hi).unwrap_or_default().iter().filter(|x| !selected.contains(x)));
        return out;
    }
    if toggle {
        let mut out = selected.to_vec();
        match out.iter().position(|x| *x == id) {
            Some(i) => {
                out.remove(i);
            }
            None => out.push(id),
        }
        return out;
    }
    vec![id]
}

/// Draws the tab's page into `ui`.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let area = ui.max_rect();
    let grid = Rect::from_min_max(area.min, pos2(area.max.x, (area.max.y - FOOT_H - GAP * 2.0).max(area.min.y)));
    let clips: Vec<(PhotoId, bool)> =
        app.session().and_then(|s| s.collage().document()).map(|d| d.clips.iter().map(|c| (c.id, c.on_page)).collect()).unwrap_or_default();
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    if clips.is_empty() {
        let galley =
            ui.painter().layout(tl!("No pictures yet. Click Get More to choose some."), fonts::regular(t.body), t.text_dim, grid.width().max(1.0));
        ui.painter().galley(grid.min, galley, t.text_dim);
    } else {
        cells(app, ui, &t, grid, &clips, &mut actions);
    }
    let foot = Rect::from_min_max(pos2(area.min.x, (area.max.y - FOOT_H).max(area.min.y)), area.max);
    let mut row = ui.new_child(UiBuilder::new().id_salt("collageClipsFoot").max_rect(foot).layout(Layout::left_to_right(Align::Center)));
    for (id, label, command, tip) in [
        (ADD_ID, "+", "collage.addClips", tl!("Put the selected pictures on the page")),
        (REMOVE_ID, "\u{2212}", "collage.removeClips", tl!("Take the selected pictures out of the Clips")),
    ] {
        let ready = crate::commands::enabled(app, command, &json!({}));
        row.add_enabled_ui(ready.is_ok(), |ui| {
            let r = widgets::text_button(ui, &t, id, label, false);
            let r = match &ready {
                Ok(()) => r.on_hover_text(tip),
                Err(why) => r.on_disabled_hover_text(crate::i18n::tr(why)),
            };
            if r.clicked() {
                actions.push((command, json!({})));
            }
        });
        row.add_space(4.0);
    }
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let r = widgets::text_button(ui, &t, GET_MORE_ID, &tl!("Get More"), false).on_hover_text(tl!("Choose more pictures in the library"));
        if r.clicked() {
            actions.push(("collage.getMore", json!({})));
        }
    });
    for (id, params) in actions {
        if let Err(e) = app.run(id, &params) {
            app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
        }
    }
}

/// The grid of `clips` in `grid`: the visible cells drawn, clicks turned into `collage.selectClips`.
fn cells(app: &mut App, ui: &mut Ui, t: &Tokens, grid: Rect, clips: &[(PhotoId, bool)], actions: &mut Vec<(&'static str, Value)>) {
    let ctx = ui.ctx().clone();
    let cols = (((grid.width() + GAP) / (CELL + GAP)).floor() as usize).max(1);
    let rows = clips.len().div_ceil(cols);
    let content = rows as f32 * (CELL + GAP) - GAP;
    let max_scroll = (content - grid.height()).max(0.0);
    let mut scroll: f32 = ctx.data(|d| d.get_temp(scroll_id())).unwrap_or(0.0);
    if ui.rect_contains_pointer(grid) {
        scroll -= ctx.input(|i| i.smooth_scroll_delta.y);
    }
    let scroll = if scroll.is_finite() { scroll.clamp(0.0, max_scroll) } else { 0.0 };
    ctx.data_mut(|d| d.insert_temp(scroll_id(), scroll));
    let left = grid.min.x + ((grid.width() - (cols as f32 * (CELL + GAP) - GAP)) / 2.0).max(0.0).floor();
    let clip_rect = grid.intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(clip_rect);
    let selected = app.state().collage.clips.clone();
    let order: Vec<PhotoId> = clips.iter().map(|(id, _)| *id).collect();
    let (command, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
    let ppp = ctx.pixels_per_point();
    let need = (CELL * ppp).round().max(1.0) as u32;
    let mut tex = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Textures>(textures_id())));
    tex.pass = ctx.cumulative_pass_nr();
    tex.uploads = 0;
    tex.waiting = 0;
    let first_row = (scroll / (CELL + GAP)).floor() as usize;
    let last_row = ((scroll + grid.height()) / (CELL + GAP)).ceil() as usize;
    for (k, (id, on_page)) in clips.iter().enumerate().skip(first_row * cols).take((last_row.saturating_sub(first_row) + 1) * cols) {
        let (r, c) = (k / cols, k % cols);
        let cell = Rect::from_min_size(pos2(left + c as f32 * (CELL + GAP), grid.min.y + r as f32 * (CELL + GAP) - scroll), vec2(CELL, CELL));
        let visible = cell.intersect(clip_rect);
        if !visible.is_positive() {
            continue;
        }
        let is_selected = selected.contains(id);
        painter.rect_filled(cell, CornerRadius::same(2), t.inset);
        let drawn = app.session_mut().and_then(|s| {
            let photo = s.library().photo(*id)?;
            let shape = shape_of(&s.library().info(*id).edits, photo.width, photo.height);
            let (w, h) = shape.kept.unwrap_or((photo.width, photo.height));
            let (w, h) = shape.turn.display_size(w, h);
            let texture = tex.get(&ctx, s, *id, need)?;
            Some((texture, fit_within(cell.shrink(3.0), w, h), shape.turn))
        });
        let image = match drawn {
            Some((texture, rect, turn)) => {
                paint_turned(&painter, rect, texture, turn);
                rect
            }
            None => {
                painter.rect_filled(cell.shrink(3.0), 0, t.placeholder);
                cell.shrink(3.0)
            }
        };
        let video = app.session().and_then(|s| crate::panels::light_table::cell::video_of(s.library(), *id));
        if let Some(video) = video {
            let marks = widgets::badges(&painter, t, image, &widgets::Badges { video: Some(video), ..widgets::Badges::default() });
            if let (Some(pill), Some(seconds)) = (marks.duration, video.seconds) {
                let text = widgets::duration_text(seconds);
                widgets::register(&ctx, &duration_id(*id), Role::Badge, &text, Target::Painted { rect: pill, clip: clip_rect }, true);
            }
        }
        if *on_page {
            on_page_mark(&painter, t, cell);
        }
        if is_selected {
            painter.rect_stroke(cell, CornerRadius::same(2), Stroke::new(2.0, t.accent), StrokeKind::Inside);
        }
        let resp = ui.interact(visible, Id::new(("collage.clip", id.get())), Sense::click());
        let name = app.session().and_then(|s| s.library().photo(*id)).map_or_else(|| tl!("Picture"), |p| p.name.clone());
        widgets::register(&ctx, &clip_id(*id), Role::Thumbnail, &name, Target::Widget(&resp), is_selected);
        if resp.hovered() && !is_selected {
            painter.rect_stroke(cell, CornerRadius::same(2), Stroke::new(1.0, t.text_label), StrokeKind::Inside);
        }
        if resp.clicked() {
            let anchor = ctx.data(|d| d.get_temp::<PhotoId>(anchor_id()));
            let ids = clicked_selection(&order, &selected, *id, command, shift, anchor);
            if !shift {
                ctx.data_mut(|d| d.insert_temp(anchor_id(), *id));
            }
            actions.push(("collage.selectClips", json!({ "ids": ids })));
        }
    }
    let pass = tex.pass;
    tex.map.retain(|_, s| pass.saturating_sub(s.used) <= KEEP_PASSES);
    if tex.waiting > 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    ctx.data_mut(|d| *d.get_temp_mut_or_default::<Textures>(textures_id()) = tex);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_selects_ctrl_toggles_and_shift_adds_the_run() {
        let ids: Vec<PhotoId> = (1..=5).map(PhotoId::new).collect();
        let p = |n: u64| PhotoId::new(n);
        assert_eq!(clicked_selection(&ids, &[p(1)], p(3), false, false, Some(p(1))), [p(3)]);
        assert_eq!(clicked_selection(&ids, &[p(1)], p(3), true, false, None), [p(1), p(3)]);
        assert_eq!(clicked_selection(&ids, &[p(1), p(3)], p(3), true, false, None), [p(1)]);
        assert_eq!(clicked_selection(&ids, &[p(2)], p(4), false, true, Some(p(2))), [p(2), p(3), p(4)]);
        assert_eq!(clicked_selection(&ids, &[], p(2), false, true, Some(p(4))), [p(2), p(3), p(4)]);
        // An anchor no longer among the clips: a plain click.
        assert_eq!(clicked_selection(&ids, &[], p(2), false, true, Some(p(9))), [p(2)]);
    }
}
