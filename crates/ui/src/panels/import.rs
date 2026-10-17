//! The Import room (`organizing.md` §3): "Import from:" along its top ([`source`]), the import
//! tray on the left ([`tray`]) and the preview on the right, about 35 % of the width ([`preview`]),
//! in the room's region under the tab strip, and the destination strip across the bottom with the
//! import's progress ([`destination`], [`progress`]).
//!
//! Everything shown comes from the engine's import session (`import.sources`, `import.list`, the
//! candidates and their in-memory thumbnails), read under a short lock each frame the room is drawn
//! and never touching the disk; what the room does goes through commands: the engine's
//! `import.openSource`, `import.exclude`, `import.excludeAll`, `import.mark` and
//! `import.options`, and the UI's `import.select`, `import.chooseFolder` and the arrows
//! (`crate::commands::import`). A new listing (its generation rises) clears the room's focus,
//! selection and preview, whose ids were the old listing's.
//!
//! What is kept between frames and is not view state (the sources as last asked, the tray's
//! scroll, the thumbnails uploaded) lives in egui's memory ([`Memory`]). Textures are uploaded
//! only for the cells drawn, at most [`UPLOADS_PER_FRAME`] a frame, and dropped once unseen for
//! [`KEEP_FRAMES`] frames.

pub mod destination;
pub mod preview;
pub mod progress;
pub mod source;
pub mod tray;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dalica_core::DateTime;
use dalica_engine::Session;
use dalica_engine::import::{Candidate, Source};
use dalica_ui_kit::widgets;
use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, Context, Galley, Id, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureId, Ui, UiBuilder, pos2};
use serde_json::{Value, json};

use crate::app::App;

/// The height of the "Import from:" bar.
pub const TOP_BAR: f32 = 44.0;
/// The preview's share of the room's width.
pub const PREVIEW_SHARE: f32 = 0.35;
/// The narrowest the preview gets (points), the room allowing.
pub const PREVIEW_MIN: f32 = 260.0;

/// The empty room's title and line (no source listed).
pub const EMPTY_TITLE: &str = "Nothing to import yet";
pub const EMPTY_BODY: &str = "Insert a camera's card or a removable drive, or choose a folder from \u{201c}Import from:\u{201d}.";
/// The empty room's button: Folder….
pub const EMPTY_ACTION: (&str, &str) = ("button:import.chooseFolder", "Choose a Folder\u{2026}");

/// Most thumbnails uploaded as textures in one frame.
pub const UPLOADS_PER_FRAME: usize = 24;
/// Frames a texture is kept once its cell is no longer drawn.
pub const KEEP_FRAMES: u64 = 120;
/// Most textures kept.
pub const MAX_TEXTURES: usize = 600;
/// How soon to draw again while thumbnails or the listing are on their way.
const POLL: Duration = Duration::from_millis(50);
/// How long after the last upload missing thumbnails are still waited for, the listing done
/// (seconds of the frame clock): one made again after it was dropped arrives within it, one that
/// cannot be made is not asked for forever.
const SETTLE_SECS: f64 = 2.0;

/// One action of the room's widgets: a command and its parameters, run once the room is drawn.
pub type Action = (&'static str, Value);

/// The room's regions in `room`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rects {
    /// "Import from:".
    pub top: Rect,
    /// The import tray: its status line and the thumbnails.
    pub tray: Rect,
    /// The preview, right of the tray.
    pub preview: Rect,
    /// The destination strip across the bottom (empty when it has no height).
    pub strip: Rect,
}

/// Where the room's parts go in `room`: the bar across the top, then the tray and the preview
/// (35 % of the width, at least [`PREVIEW_MIN`] while the tray keeps as much) side by side.
pub fn layout(room: Rect) -> Rects {
    layout_with(room, 0.0)
}

/// [`layout`] with the destination strip `strip_h` high across the bottom, under the tray and the
/// preview.
pub fn layout_with(room: Rect, strip_h: f32) -> Rects {
    let room = if room.is_finite() { room } else { Rect::ZERO };
    let strip_h = if strip_h.is_finite() { strip_h.clamp(0.0, (room.height() - TOP_BAR).max(0.0)) } else { 0.0 };
    let strip = Rect::from_min_max(pos2(room.min.x, room.max.y - strip_h), room.max);
    let room = Rect::from_min_max(room.min, pos2(room.max.x, strip.min.y));
    let top = Rect::from_min_max(room.min, pos2(room.max.x, (room.min.y + TOP_BAR).min(room.max.y)));
    let width = room.width().max(0.0);
    let share = (width * PREVIEW_SHARE).round();
    let preview_w = if width >= 2.0 * PREVIEW_MIN { share.max(PREVIEW_MIN) } else { share };
    let split = room.max.x - preview_w;
    let tray = Rect::from_min_max(pos2(room.min.x, top.max.y), pos2(split, room.max.y));
    let preview = Rect::from_min_max(pos2(split, top.max.y), room.max);
    Rects { top, tray, preview, strip }
}

/// A run of candidates taken within two hours of each other: `start..end` of
/// [`Snapshot::candidates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// What the room shows this frame, read from the engine's import session.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// The listing's generation (it rises with each source opened).
    pub generation: u64,
    pub source: Option<Source>,
    /// The listing is still running.
    pub loading: bool,
    /// The engine's status line ("Loading 112 of 340 files…", "340 files", "No pictures found").
    pub status: String,
    pub exclude_duplicates: bool,
    /// In capture order.
    pub candidates: Vec<Candidate>,
    /// Whether each candidate will be imported (neither excluded nor a duplicate left out).
    pub will_import: Vec<bool>,
    /// The groups, in order.
    pub groups: Vec<Span>,
}

impl Snapshot {
    /// The import session of `session` as it stands.
    pub fn of(session: &mut Session) -> Snapshot {
        let listing = session.execute("import.list", &json!({ "limit": 0 })).unwrap_or(Value::Null);
        let imp = session.import_session();
        let candidates = imp.candidates();
        let will_import = candidates.iter().map(|c| imp.will_import(c)).collect();
        let mut groups: Vec<Span> = Vec::new();
        for (i, c) in candidates.iter().enumerate() {
            match groups.last_mut() {
                Some(g) if candidates.get(g.start).is_some_and(|f| f.group == c.group) => g.end = i + 1,
                _ => groups.push(Span { start: i, end: i + 1 }),
            }
        }
        Snapshot {
            generation: imp.generation(),
            source: imp.source().cloned(),
            loading: listing["state"] == "loading",
            status: listing["status"].as_str().unwrap_or_default().to_string(),
            exclude_duplicates: imp.exclude_duplicates(),
            candidates,
            will_import,
            groups,
        }
    }

    /// How many candidates will be imported.
    pub fn to_import(&self) -> usize {
        self.will_import.iter().filter(|w| **w).count()
    }

    /// Candidate `id` and its index, if listed.
    pub fn find(&self, id: u64) -> Option<(usize, &Candidate)> {
        self.candidates.iter().enumerate().find(|(_, c)| c.id == id)
    }

    /// Whether candidate `i` is left out (excluded, or a duplicate "Exclude duplicates" leaves
    /// out): dimmed with the ⊘.
    pub fn left_out(&self, i: usize) -> bool {
        self.will_import.get(i).is_some_and(|w| !*w)
    }
}

/// `9:12 AM`.
pub fn clock(d: DateTime) -> String {
    let (hour, half) = match d.hour() {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h.saturating_sub(12), "PM"),
    };
    format!("{hour}:{:02} {half}", d.minute())
}

/// A group header's title: `June 4, 2026 8:00 AM – 9:12 AM`, the whole second date when the
/// group runs past midnight, one time for a group of one moment.
pub fn group_title(first: DateTime, last: DateTime) -> String {
    let start = crate::status_text::date_time(first);
    if first == last {
        start
    } else if first.date() == last.date() {
        format!("{start} \u{2013} {}", clock(last))
    } else {
        format!("{start} \u{2013} {}", crate::status_text::date_time(last))
    }
}

/// `12 pictures`.
pub fn pictures(n: usize) -> String {
    if n == 1 { "1 picture".into() } else { format!("{n} pictures") }
}

/// `text` on one line, cut with "…" to `width`.
pub fn one_line(painter: &Painter, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    painter.layout_job(job)
}

/// The size a `width × height` picture shows at once turned `turns` quarter turns.
pub fn turned(width: usize, height: usize, turns: u8) -> (u32, u32) {
    let (w, h) = (u32::try_from(width).unwrap_or(u32::MAX), u32::try_from(height).unwrap_or(u32::MAX));
    if turns % 2 == 1 { (h, w) } else { (w, h) }
}

/// The upright picture in `texture` drawn over `rect` turned `turns` quarter turns clockwise.
pub fn paint_turned(painter: &Painter, rect: Rect, texture: TextureId, turns: u8) {
    const UV: [Pos2; 4] = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    if !(rect.is_finite() && rect.is_positive()) {
        return;
    }
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    let mut mesh = Mesh::with_texture(texture);
    let q = usize::from(turns % 4);
    for (i, at) in corners.into_iter().enumerate() {
        let uv = UV.get((i + 4 - q) % 4).copied().unwrap_or(Pos2::ZERO);
        mesh.vertices.push(egui::epaint::Vertex { pos: at, uv, color: Color32::WHITE });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(mesh));
}

/// A texture of a candidate's thumbnail.
struct Slot {
    handle: TextureHandle,
    size: [usize; 2],
    used: u64,
}

/// The candidates' thumbnails uploaded as textures, for one listing.
#[derive(Default)]
pub struct Textures {
    map: HashMap<u64, Slot>,
    generation: u64,
    frame: u64,
    uploads: usize,
    /// Cells drawn this frame whose thumbnail is not in memory yet.
    pub missing: usize,
    /// The frame clock's time of the last upload.
    last_upload: f64,
}

impl Textures {
    fn begin(&mut self, generation: u64) {
        if generation != self.generation {
            self.map.clear();
            self.generation = generation;
        }
        self.frame = self.frame.saturating_add(1);
        self.uploads = 0;
        self.missing = 0;
    }

    /// Candidate `id`'s texture and the thumbnail's size, uploading it when the engine has it in
    /// memory (at most [`UPLOADS_PER_FRAME`] a frame); `None` while it has not.
    pub fn get(&mut self, ctx: &Context, session: &Session, id: u64) -> Option<(TextureId, [usize; 2])> {
        if let Some(s) = self.map.get_mut(&id) {
            s.used = self.frame;
            return Some((s.handle.id(), s.size));
        }
        if self.uploads >= UPLOADS_PER_FRAME {
            self.missing += 1;
            return None;
        }
        let Some(image) = session.import_session().thumbnail(id) else {
            self.missing += 1;
            return None;
        };
        let size = [usize::try_from(image.width).ok()?, usize::try_from(image.height).ok()?];
        if size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) != Some(image.data.len()) || size[0] == 0 || size[1] == 0 {
            return None;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &image.data);
        let handle = ctx.load_texture(format!("import:{}:{id}", self.generation), pixels, egui::TextureOptions::LINEAR);
        let tex = handle.id();
        self.map.insert(id, Slot { handle, size, used: self.frame });
        self.uploads += 1;
        self.last_upload = ctx.input(|i| i.time);
        Some((tex, size))
    }

    fn end(&mut self) {
        let frame = self.frame;
        self.map.retain(|_, s| frame.saturating_sub(s.used) <= KEEP_FRAMES);
        if self.map.len() > MAX_TEXTURES {
            let mut used: Vec<(u64, u64)> = self.map.iter().map(|(id, s)| (s.used, *id)).collect();
            used.sort_unstable();
            for (_, id) in used.iter().take(self.map.len() - MAX_TEXTURES) {
                self.map.remove(id);
            }
        }
    }

    /// How many textures are held.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether none is held.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// What the room keeps between frames that is not view state.
#[derive(Default)]
pub struct Memory {
    pub sources: source::Sources,
    pub tray: tray::TrayMemory,
    pub textures: Textures,
    /// The listing generation last drawn.
    generation: Option<u64>,
}

type Shared = Arc<Mutex<Memory>>;

fn memory(ctx: &Context) -> Shared {
    ctx.data_mut(|d| Arc::clone(d.get_temp_mut_or_default::<Shared>(Id::new("import.room"))))
}

fn lock(m: &Mutex<Memory>) -> MutexGuard<'_, Memory> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Draws the room into `ui`'s rect (the `room` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let shared = memory(&ctx);
    let mut mem = lock(&shared);
    let mut actions: Vec<Action> = Vec::new();

    // The sources (a card plugged in while the room is open is listed, and opened), then the
    // listing as it stands.
    let Some(session) = app.session_mut() else { return };
    if let Some(open) = source::poll(session, &mut mem.sources, now) {
        let _ = session.execute("import.openSource", &open);
    }
    let snap = Snapshot::of(session);
    if mem.generation != Some(snap.generation) {
        if mem.generation.is_some() {
            let room = &mut app.state_mut().import;
            (room.focus, room.selection, room.preview) = (None, Vec::new(), None);
        }
        mem.generation = Some(snap.generation);
        mem.tray = tray::TrayMemory::default();
    }
    mem.textures.begin(snap.generation);
    // The import running or ended: its progress, and what follows its end.
    let shown = progress::poll(app, &ctx);

    let strip_h = if snap.source.is_some() { destination::height(shown.lines()) } else { 0.0 };
    let rects = layout_with(ui.max_rect(), strip_h);
    let p = ui.painter().clone();
    p.rect_filled(rects.top, 0, t.chrome);
    p.hline(rects.top.x_range(), rects.top.max.y - 0.5, egui::Stroke::new(1.0, t.divider));
    p.vline(rects.preview.min.x + 0.5, rects.preview.y_range(), egui::Stroke::new(1.0, t.divider));
    in_rect(ui, rects.top, |ui| source::show(app, ui, &t, &mem.sources, &snap, &mut actions));

    if snap.source.is_none() {
        let body = rects.tray.union(rects.preview);
        in_rect(ui, body, |ui| {
            if widgets::empty_state(ui, &t, "empty:import", EMPTY_TITLE, EMPTY_BODY, Some(EMPTY_ACTION)).is_some_and(|r| r.clicked()) {
                actions.push(("import.chooseFolder", json!({})));
            }
        });
    } else {
        let m = &mut *mem;
        let menu = in_rect(ui, rects.tray, |ui| tray::show(app, ui, &t, &mut m.tray, &mut m.textures, &snap, &mut actions));
        in_rect(ui, rects.preview, |ui| preview::show(app, ui, &t, &mut m.textures, &snap, &mut actions));
        in_rect(ui, rects.strip, |ui| destination::show(app, ui, &t, &snap, &shown, &mut actions));
        for (command, params) in actions.drain(..) {
            let _ = app.run(command, &params);
        }
        if let Some(menu) = menu {
            tray::context_menu(app, &snap, menu);
        }
    }
    for (command, params) in actions {
        let _ = app.run(command, &params);
    }

    mem.textures.end();
    let waiting = mem.textures.missing > 0 && (snap.loading || now - mem.textures.last_upload < SETTLE_SECS);
    if snap.loading || waiting {
        ctx.request_repaint_after(POLL);
    }
    ctx.request_repaint_after(source::POLL);
}

/// Runs `add` in a child of `ui` filling `rect`, clipped to it.
fn in_rect<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

#[cfg(test)]
mod tests {
    use egui::vec2;

    use super::*;

    #[test]
    fn the_preview_takes_about_a_third_of_the_room() {
        let room = Rect::from_min_size(pos2(0.0, 60.0), vec2(1400.0, 816.0));
        let r = layout(room);
        assert_eq!(r.top, Rect::from_min_size(pos2(0.0, 60.0), vec2(1400.0, TOP_BAR)));
        assert_eq!((r.preview.width(), r.preview.height()), (490.0, 816.0 - TOP_BAR));
        assert_eq!(r.tray.max.x, r.preview.min.x);
        // At 900 the preview keeps its least width; in a sliver it keeps its share.
        let r = layout(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 500.0)));
        assert_eq!(r.preview.width(), 315.0);
        let r = layout(Rect::from_min_size(Pos2::ZERO, vec2(100.0, 50.0)));
        assert_eq!(r.preview.width(), 35.0);
        // Hostile rects give empty regions, never a panic.
        let r = layout(Rect::from_min_size(Pos2::ZERO, vec2(f32::NAN, 10.0)));
        assert_eq!(r.preview.width(), 0.0);
        // The destination strip takes the bottom, under both.
        let r = layout_with(room, 88.0);
        assert_eq!(r.strip, Rect::from_min_max(pos2(0.0, 60.0 + 816.0 - 88.0), pos2(1400.0, 876.0)));
        assert_eq!((r.tray.max.y, r.preview.max.y), (r.strip.min.y, r.strip.min.y));
        assert_eq!(layout_with(room, f32::INFINITY).strip.height(), 0.0);
    }

    #[test]
    fn group_titles_name_the_period() {
        let at = |h, m| DateTime::new(dalica_core::Date::new(2026, 6, 4).unwrap(), h, m, 0).unwrap();
        assert_eq!(group_title(at(8, 0), at(8, 0)), "June 4, 2026 8:00 AM");
        assert_eq!(group_title(at(8, 0), at(21, 12)), "June 4, 2026 8:00 AM \u{2013} 9:12 PM");
        let next = DateTime::new(dalica_core::Date::new(2026, 6, 5).unwrap(), 0, 30, 0).unwrap();
        assert_eq!(group_title(at(23, 0), next), "June 4, 2026 11:00 PM \u{2013} June 5, 2026 12:30 AM");
        assert_eq!((pictures(1), pictures(12)), ("1 picture".to_string(), "12 pictures".to_string()));
    }

    #[test]
    fn a_turned_picture_swaps_its_sides() {
        assert_eq!(turned(256, 170, 0), (256, 170));
        assert_eq!(turned(256, 170, 1), (170, 256));
        assert_eq!(turned(256, 170, 2), (256, 170));
        assert_eq!(turned(256, 170, 3), (170, 256));
    }
}
