//! The Histogram & Camera Info box at the bottom of the edit panel (`editing.md` §5.2,
//! `box:histogram`): the title, the RGB histogram (`histogram:edit`) of the preview the photo area
//! shows, updated whenever a new preview arrives (an edit, a tool's draft), and under it two
//! columns: camera make and model and the focal length (with its 35 mm equivalent) | shutter,
//! aperture and ISO, from `photo.metadata` (`box:histogram/info` carries the lines as its label).
//! A photo without them says "No camera information"; while the file is being read, "Reading
//! camera information…". Until a preview of the photo is shown the plot is an empty plate.
//!
//! The plot's bins are kept per preview (photo, generation and quality: a tool's final replaces its
//! smaller draft of the same generation) and the camera lines per photo and
//! library revision, asked again only while the engine is still reading the file: no frame reads
//! the file or recounts a histogram.

use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Quality;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Color32, CornerRadius, Id, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::panels::properties::answer::{self, Answer};

pub const BOX_ID: &str = "box:histogram";
pub const PLOT_ID: &str = "histogram:edit";
pub const INFO_ID: &str = "box:histogram/info";
pub const TITLE: &str = "Histogram & Camera Info";
pub const NO_CAMERA: &str = "No camera information";
pub const READING: &str = "Reading camera information…";

/// The padding inside the box, the title row's height, the gap between the rows, one camera line's
/// height and the lines per column.
pub const PAD: f32 = 8.0;
pub const TITLE_H: f32 = 16.0;
pub const ROW_GAP: f32 = 4.0;
pub const LINE_H: f32 = 14.0;
pub const LINES: usize = 3;

/// The parts of the box at `rect`: the title row, the plot, the camera columns' area.
pub fn layout(rect: Rect) -> (Rect, Rect, Rect) {
    let inner = rect.shrink(PAD);
    let title = Rect::from_min_size(inner.min, vec2(inner.width().max(0.0), TITLE_H));
    let info_h = LINE_H * LINES as f32;
    let info = Rect::from_min_max(
        pos2(inner.min.x, (inner.max.y - info_h).max(title.max.y)),
        pos2(inner.max.x.max(inner.min.x), inner.max.y.max(title.max.y)),
    );
    let plot = Rect::from_min_max(
        pos2(inner.min.x, title.max.y + ROW_GAP),
        pos2(inner.max.x.max(inner.min.x), (info.min.y - ROW_GAP).max(title.max.y + ROW_GAP)),
    );
    (title, plot, info)
}

/// What the camera columns say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Camera {
    /// The file is being read.
    Reading,
    /// No EXIF camera fields (or the file could not be read).
    None,
    /// The left column (camera, focal length) and the right one (shutter, aperture, ISO); each
    /// line present only when the file has it.
    Columns(Vec<String>, Vec<String>),
}

impl Camera {
    /// The lines as one text: the columns joined with " · ", or the one-line message.
    pub fn text(&self) -> String {
        match self {
            Camera::Reading => READING.into(),
            Camera::None => NO_CAMERA.into(),
            Camera::Columns(l, r) => l.iter().chain(r).cloned().collect::<Vec<_>>().join(" · "),
        }
    }
}

/// `35 mm (52 mm equiv.)`: the engine's focal length row ("35 mm (52 mm in 35 mm film)") made
/// short enough for its column.
pub fn short_focal(value: &str) -> String {
    value.replace(" in 35 mm film", " equiv.")
}

/// The camera columns of a `photo.metadata` answer.
pub fn camera(a: &Answer) -> Camera {
    let value = |key: &str| a.rows.iter().find(|r| r.key == key).map(|r| r.value.clone()).filter(|v| !v.is_empty());
    let left: Vec<String> = [value("camera"), value("focalLength").map(|f| short_focal(&f))].into_iter().flatten().collect();
    let right: Vec<String> = [value("exposure"), value("aperture"), value("iso").map(|i| format!("ISO {i}"))].into_iter().flatten().collect();
    if left.is_empty() && right.is_empty() { if a.pending { Camera::Reading } else { Camera::None } } else { Camera::Columns(left, right) }
}

/// The camera answer last asked, and what for.
#[derive(Clone)]
struct Asked {
    photo: PhotoId,
    revision: u64,
    camera: Arc<Camera>,
}

/// Photo `id`'s camera columns: the kept ones, or asked of the engine (a cache lookup, the file read
/// on its workers) when the photo or the library changed or the file is still being read.
fn camera_of(app: &mut App, ctx: &egui::Context, id: PhotoId) -> Arc<Camera> {
    let key = Id::new("editPanel.camera");
    let Some(session) = app.session_mut() else { return Arc::new(Camera::None) };
    let revision = session.library().revision();
    let kept = ctx.data(|d| d.get_temp::<Asked>(key));
    if let Some(k) = &kept
        && k.photo == id
        && k.revision == revision
        && *k.camera != Camera::Reading
    {
        return Arc::clone(&k.camera);
    }
    let a = match session.execute("photo.metadata", &json!({ "id": id })) {
        Ok(reply) => answer::read(&reply, None),
        Err(_) => Answer::default(),
    };
    if a.pending {
        ctx.request_repaint_after(answer::POLL);
    }
    let camera = Arc::new(camera(&a));
    ctx.data_mut(|d| d.insert_temp(key, Asked { photo: id, revision, camera: Arc::clone(&camera) }));
    camera
}

/// The bins kept for one preview.
#[derive(Clone)]
struct Bins {
    photo: PhotoId,
    generation: u64,
    quality: Quality,
    bins: Arc<[Vec<u64>; 3]>,
}

/// The bins the plot drew last (red, green, blue), for tests and agents.
pub fn shown_bins(ctx: &egui::Context) -> Option<Arc<[Vec<u64>; 3]>> {
    ctx.data(|d| d.get_temp::<Bins>(bins_id())).map(|b| b.bins)
}

fn bins_id() -> Id {
    Id::new("editPanel.histogram")
}

/// The red, green and blue bins of the preview shown of `id`, if one is shown; counted again only
/// when another preview arrives.
fn bins_of(app: &App, ctx: &egui::Context, id: PhotoId) -> Option<Arc<[Vec<u64>; 3]>> {
    let key = bins_id();
    let preview = app.session()?.preview().filter(|p| p.photo == id)?;
    if let Some(k) = ctx.data(|d| d.get_temp::<Bins>(key))
        && k.photo == id
        && k.generation == preview.generation
        && k.quality == preview.quality
    {
        return Some(k.bins);
    }
    let h = &preview.histogram;
    let widen = |c: &[u32]| c.iter().map(|&n| u64::from(n)).collect::<Vec<u64>>();
    let bins = Arc::new([widen(&h.red), widen(&h.green), widen(&h.blue)]);
    ctx.data_mut(|d| d.insert_temp(key, Bins { photo: id, generation: preview.generation, quality: preview.quality, bins: Arc::clone(&bins) }));
    Some(bins)
}

/// Draws the box into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(t.radius), t.inset, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let (title, plot, info) = layout(rect);
    widgets::label_in(p, title, Align2::LEFT_CENTER, TITLE, fonts::semibold(t.count), t.text_label);
    widgets::register(ui.ctx(), BOX_ID, Role::Other, TITLE, Target::Painted { rect, clip: ui.clip_rect() }, false);
    let Some(id) = crate::commands::edit_view::open_photo(app) else { return };
    let ctx = ui.ctx().clone();
    let empty: [Vec<u64>; 3] = Default::default();
    let bins = bins_of(app, &ctx, id);
    let b = bins.as_deref().unwrap_or(&empty);
    widgets::histogram(ui, &t, PLOT_ID, plot, [&b[0], &b[1], &b[2]]);
    let camera = camera_of(app, &ctx, id);
    paint_camera(ui, &t, info, &camera);
    widgets::register(ui.ctx(), INFO_ID, Role::Other, &camera.text(), Target::Painted { rect: info, clip: ui.clip_rect() }, false);
}

/// Paints the camera columns (or the one-line message) into `info`.
fn paint_camera(ui: &Ui, t: &Tokens, info: Rect, camera: &Camera) {
    let p = ui.painter();
    let font = fonts::regular(t.count);
    match camera {
        Camera::Reading | Camera::None => {
            widgets::label_in(p, info, Align2::CENTER_CENTER, &camera.text(), font, t.text_dim);
        }
        Camera::Columns(left, right) => {
            let w = (info.width() / 2.0).floor();
            for (col, lines) in [left, right].into_iter().enumerate() {
                let x = info.min.x + w * col as f32;
                for (i, line) in lines.iter().take(LINES).enumerate() {
                    let row = Rect::from_min_size(pos2(x, info.min.y + LINE_H * i as f32), vec2(w - 4.0, LINE_H));
                    paint_line(p, row, line, font.clone(), t.text_label);
                }
            }
        }
    }
}

/// `text` left-aligned in `row`, elided to its width.
fn paint_line(p: &egui::Painter, row: Rect, text: &str, font: egui::FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(row.width().max(1.0));
    let galley = p.layout_job(job);
    p.galley(Align2::LEFT_CENTER.align_size_within_rect(galley.size(), row).min.round(), galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::properties::answer::Row;

    fn row(key: &str, value: &str) -> Row {
        Row { key: key.into(), label: key.into(), value: value.into(), note: None }
    }

    #[test]
    fn the_box_holds_a_plot_248_wide_and_three_lines_of_camera() {
        let (title, plot, info) = layout(Rect::from_min_size(pos2(8.0, 626.0), vec2(264.0, 150.0)));
        assert_eq!(plot.width(), 248.0);
        assert_eq!(title.min, pos2(16.0, 634.0));
        assert_eq!(info.height(), LINE_H * 3.0);
        assert_eq!(info.max.y, 768.0);
        assert!(plot.min.y > title.max.y && plot.max.y < info.min.y && plot.height() > 60.0, "{plot:?}");
        for h in [0.0, 10.0, f32::NAN] {
            let (a, b, c) = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(h, h)));
            for r in [a, b, c] {
                assert!(!(r.width() < 0.0 || r.height() < 0.0), "{h}: {r:?}");
            }
        }
    }

    #[test]
    fn camera_columns_from_the_metadata_rows() {
        let a = Answer {
            rows: vec![
                row("path", "/x.jpg"),
                row("camera", "Canon EOS 5D"),
                row("exposure", "1/250 s"),
                row("aperture", "f/2.8"),
                row("iso", "200"),
                row("focalLength", "35 mm (52 mm in 35 mm film)"),
            ],
            ..Answer::default()
        };
        let c = camera(&a);
        assert_eq!(
            c,
            Camera::Columns(vec!["Canon EOS 5D".into(), "35 mm (52 mm equiv.)".into()], vec!["1/250 s".into(), "f/2.8".into(), "ISO 200".into()])
        );
        assert_eq!(c.text(), "Canon EOS 5D · 35 mm (52 mm equiv.) · 1/250 s · f/2.8 · ISO 200");
        assert_eq!(short_focal("52 mm in 35 mm film"), "52 mm equiv.");
        assert_eq!(short_focal("50 mm"), "50 mm");
        let none = Answer { rows: vec![row("path", "/x.jpg"), row("camera", "")], ..Answer::default() };
        assert_eq!(camera(&none), Camera::None);
        assert_eq!(camera(&Answer { pending: true, ..none.clone() }), Camera::Reading);
        assert_eq!(camera(&Answer { error: Some("unreadable".into()), ..none }), Camera::None);
    }
}
