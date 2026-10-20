//! The Collage room's Settings tab (`creating.md` §3): the style dropdown with its rich rows
//! (`choice:collage.style`, a row each `choice:collage.style/<style>`), Picture borders
//! (`toggle:collage.border.<none|white|instant>`), Grid spacing (`slider:collage.spacing`, one
//! room step per drag), Background (`choice:collage.background/solid` with its swatch
//! `color:collage.background`, picker and eyedropper, or `choice:collage.background/picture`),
//! Page format (`choice:collage.format`, a row each `choice:collage.format/<n>`) with its delete
//! button (`button:collage.deleteFormat`, custom ratios only) and the orientation toggles
//! (`toggle:collage.orientation.<landscape|portrait>`), Draw shadows and Show captions
//! (`check:collage.shadows`, `check:collage.captions`), each greyed for the styles that do not
//! offer it; then, along the bottom, Create Collage (`button:collage.create`; while a collage is
//! written its progress `progress:collage` and Cancel `button:collage.cancel`), Reset
//! (`button:collage.reset`) and Close (`button:collage.close`).
//!
//! What it shows is the engine's open collage (`Session::collage`), read each frame without a
//! copy; the page formats come from `collage.inspect` and are kept until the room was not drawn
//! for a frame, the display's size changes or a format is deleted. Every change is a command
//! (`crate::commands::collage`): the style and settings through `collage.style` and
//! `collage.settings`, the buttons through `collage.createCollage`, `collage.resetSettings` and
//! `collage.closeRoom`, the delete button through the engine's `collage.formats.remove`.
//!
//! The eyedropper, armed from the swatch's popup, picks the colour under a click on the canvas's
//! page ([`pick`]): the top picture under it, sampled from the thumbnail in memory (else the
//! photo's colour sketch), never from the disk.

use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_core::color::hex_u8;
use dalica_create::collage::{Border, Orientation, Style};
use dalica_engine::{Session, Thumb};
use dalica_ui_kit::icons::{CollageIcon, EditIcon, Glyph, paint_glyph};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ImageToggle, RichRow, SliderSpec, Target, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, Context, CornerRadius, Id, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::panels::thumbnails::{shape_of, turn_point};
use crate::tl;

pub const STYLE_ID: &str = "choice:collage.style";
pub const BORDER_IDS: [&str; 3] = ["toggle:collage.border.none", "toggle:collage.border.white", "toggle:collage.border.instant"];
pub const SPACING_ID: &str = "slider:collage.spacing";
pub const SOLID_ID: &str = "choice:collage.background/solid";
pub const PICTURE_ID: &str = "choice:collage.background/picture";
pub const SWATCH_ID: &str = "color:collage.background";
pub const FORMAT_ID: &str = "choice:collage.format";
pub const DELETE_FORMAT_ID: &str = "button:collage.deleteFormat";
pub const ORIENTATION_IDS: [&str; 2] = ["toggle:collage.orientation.landscape", "toggle:collage.orientation.portrait"];
pub const SHADOWS_ID: &str = "check:collage.shadows";
pub const CAPTIONS_ID: &str = "check:collage.captions";
pub const CREATE_ID: &str = "button:collage.create";
pub const PROGRESS_ID: &str = "progress:collage";
pub const CANCEL_ID: &str = "button:collage.cancel";
pub const RESET_ID: &str = "button:collage.reset";
pub const CLOSE_ID: &str = "button:collage.close";

/// What the tab says while the room shows no collage.
pub const NO_COLLAGE: &str = "No collage is open. Choose photos and click Collage to make one.";
/// The labels' column width.
pub const LABEL_W: f32 = 96.0;
/// The gap between rows.
pub const ROW_GAP: f32 = 10.0;
/// A toggle of the borders and the orientation.
pub const TOGGLE: egui::Vec2 = vec2(36.0, 28.0);
/// The delete button's side.
pub const DELETE_W: f32 = 24.0;
/// The height the buttons along the bottom take: Create Collage, then Reset and Close.
pub const FOOT_H: f32 = 24.0 + 8.0 + 24.0;
/// The grid spacing slider's range, in percent of the page's short side.
pub const SPACING_MAX: f64 = 5.0;
/// The long side of the thumbnail the eyedropper samples (device pixels).
const SAMPLE_NEED: u32 = 256;
/// The long side a colour sketch is drawn at for the eyedropper.
const SKETCH_SIDE: u32 = 48;

/// The style's glyph.
pub fn style_icon(s: Style) -> CollageIcon {
    match s {
        Style::Pile => CollageIcon::PicturePile,
        Style::Mosaic => CollageIcon::Mosaic,
        Style::FrameMosaic => CollageIcon::FrameMosaic,
        Style::Grid => CollageIcon::Grid,
        Style::ContactSheet => CollageIcon::ContactSheet,
        Style::MultipleExposure => CollageIcon::MultipleExposure,
    }
}

fn row_of(s: Style, (name, description): &(String, String)) -> RichRow<'_> {
    RichRow { glyph: Glyph::Collage(style_icon(s)), name, description }
}

/// A style's name and the line under it in the language in effect.
pub fn style_texts(s: Style) -> (String, String) {
    match s {
        Style::Pile => (tl!("Picture Pile"), tl!("Looks like a pile of scattered pictures")),
        Style::Mosaic => (tl!("Mosaic"), tl!("Fits pictures into the page automatically")),
        Style::FrameMosaic => (tl!("Frame Mosaic"), tl!("A mosaic around a large center picture")),
        Style::Grid => (tl!(ctx = "collage style", "Grid"), tl!("Pictures in regular rows and columns")),
        Style::ContactSheet => (tl!("Contact Sheet"), tl!("Small pictures with their names")),
        Style::MultipleExposure => (tl!("Multiple Exposure"), tl!("Pictures blended on top of one another")),
    }
}

/// A label or tooltip of the tab in the language in effect.
pub fn text(english: &str) -> String {
    match english {
        "No collage is open. Choose photos and click Collage to make one." => tl!("No collage is open. Choose photos and click Collage to make one."),
        "Picture borders" => tl!("Picture borders"),
        "No border" => tl!("No border"),
        "White border" => tl!("White border"),
        "Instant photo border" => tl!("Instant photo border"),
        "Grid spacing" => tl!("Grid spacing"),
        "Background" => tl!(ctx = "collage", "Background"),
        "Solid color" => tl!("Solid color"),
        "Background color" => tl!("Background color"),
        "Use picture" => tl!("Use picture"),
        "Page format" => tl!("Page format"),
        "Delete this page format" => tl!("Delete this page format"),
        "Landscape" => tl!("Landscape"),
        "Portrait" => tl!("Portrait"),
        "Draw shadows" => tl!("Draw shadows"),
        "Show captions" => tl!("Show captions"),
        "Create Collage" => tl!("Create Collage"),
        "Reset" => tl!("Reset"),
        "Put the settings back to their defaults" => tl!("Put the settings back to their defaults"),
        "Close" => tl!("Close"),
        other => crate::i18n::tr(other),
    }
}

/// What the tab shows of the open collage, read without copying its clips.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub style: Style,
    pub border: Border,
    /// Percent of the page's short side.
    pub spacing: f64,
    pub shadows: bool,
    pub captions: bool,
    pub format: String,
    pub orientation: Orientation,
    pub colour: [u8; 3],
    pub picture: Option<PhotoId>,
}

impl Shown {
    pub fn of(session: &Session) -> Option<Shown> {
        let d = session.collage().document()?;
        Some(Shown {
            style: d.style,
            border: d.settings.border,
            spacing: d.settings.spacing * 100.0,
            shadows: d.settings.shadows,
            captions: d.settings.captions,
            format: d.page.format.name().to_string(),
            orientation: d.page.orientation,
            colour: d.background.colour,
            picture: d.background.picture,
        })
    }
}

/// A page format as the dropdown lists it: its name and whether it is a custom ratio (deletable).
#[derive(Debug, Clone, PartialEq)]
pub struct FormatRow {
    pub name: String,
    pub custom: bool,
}

/// The page formats as last asked, and when.
#[derive(Debug, Clone, Default)]
struct Formats {
    pass: u64,
    display: Option<[u32; 2]>,
    rows: Arc<Vec<FormatRow>>,
}

fn formats_id() -> Id {
    Id::new("collage.formats")
}

/// The page formats: asked again when the room was not drawn on the last pass, the display's size
/// changed, the format in use is not among them, or [`forget_formats`] dropped them.
fn formats(app: &mut App, ctx: &Context, current: &str) -> Arc<Vec<FormatRow>> {
    let pass = ctx.cumulative_pass_nr();
    let display = app.collage().display();
    let kept: Option<Formats> = ctx.data(|d| d.get_temp(formats_id()));
    let fresh = kept.filter(|f| f.pass + 1 >= pass && f.display == display && f.rows.iter().any(|r| r.name == current));
    let rows = match fresh {
        Some(f) => f.rows,
        None => {
            let rows: Vec<FormatRow> = crate::commands::collage::formats(app)
                .iter()
                .filter_map(|f| Some(FormatRow { name: f.get("name")?.as_str()?.to_string(), custom: f.get("custom")?.as_bool()? }))
                .collect();
            Arc::new(rows)
        }
    };
    ctx.data_mut(|d| d.insert_temp(formats_id(), Formats { pass, display, rows: Arc::clone(&rows) }));
    rows
}

/// Drops the page formats kept, so the next frame asks again.
pub fn forget_formats(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Formats>(formats_id()));
}

fn armed_id() -> Id {
    Id::new("collage.eyedropper")
}

/// Whether the background's eyedropper is armed: the next click on the canvas's page picks.
pub fn eyedropper_armed(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(armed_id())).unwrap_or(false)
}

/// Arms or disarms the eyedropper.
pub fn set_eyedropper(ctx: &Context, on: bool) {
    ctx.data_mut(|d| {
        if on {
            d.insert_temp(armed_id(), true);
        } else {
            d.remove::<bool>(armed_id());
        }
    });
}

fn spacing_id() -> Id {
    Id::new("collage.spacing.dragged")
}

/// A label in the label column, `text_disabled` while greyed.
fn label(ui: &mut Ui, t: &Tokens, text: &str, enabled: bool) {
    let (_, rect) = ui.allocate_space(vec2(LABEL_W, TOGGLE.y));
    let color = if enabled { t.text_label } else { t.text_disabled };
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, text, fonts::regular(t.body), color);
}

/// A row of the tab: `h` high across it, laid out left to right.
fn row<R>(ui: &mut Ui, h: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width().max(1.0), h));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    add(&mut child)
}

/// A square glyph button (the page format's delete): `hover × 0.7` hovered, the glyph `icon`
/// (`text` hovered, `text_disabled` greyed); registered as `id` with `tooltip`.
fn glyph_button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, tooltip: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, r) = ui.allocate_exact_size(vec2(DELETE_W, DELETE_W), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    let color = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_glyph(p, rect.shrink(4.0), glyph, color);
    widgets::register(ui.ctx(), id, Role::IconButton, tooltip, Target::Widget(&r), false);
    r.on_hover_text(tooltip)
}

/// A command a click asks for, run once the frame's borrows end.
type Action = (&'static str, Value);

/// Draws the tab's page into `ui`.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let area = ui.max_rect();
    let shown = app.session().and_then(Shown::of);
    let body = Rect::from_min_max(area.min, pos2(area.max.x, (area.max.y - FOOT_H - ROW_GAP).max(area.min.y)));
    let mut col = ui.new_child(UiBuilder::new().id_salt("collageSettings").max_rect(body).layout(Layout::top_down(Align::Min)));
    col.set_clip_rect(body.intersect(ui.clip_rect()));
    let mut actions: Vec<Action> = Vec::new();
    match &shown {
        Some(v) => options(app, &mut col, &t, v, &mut actions),
        None => {
            let galley = col.painter().layout(text(NO_COLLAGE), fonts::regular(t.body), t.text_dim, body.width().max(1.0));
            col.painter().galley(body.min, galley, t.text_dim);
        }
    }
    let foot = Rect::from_min_max(pos2(area.min.x, (area.max.y - FOOT_H).max(area.min.y)), area.max);
    footer(app, ui, &t, foot, &mut actions);
    for (id, params) in actions {
        if id == "collage.formats.remove" {
            forget_formats(&ctx);
        }
        if let Err(e) = app.run(id, &params) {
            app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
        }
    }
}

/// The options of the open collage `v`.
fn options(app: &mut App, ui: &mut Ui, t: &Tokens, v: &Shown, actions: &mut Vec<Action>) {
    let ctx = ui.ctx().clone();
    // The style.
    let mut picked = None;
    let shown = style_texts(v.style);
    widgets::rich_dropdown(ui, t, STYLE_ID, &row_of(v.style, &shown), ui.available_width(), |ui| {
        for s in Style::ALL {
            let texts = style_texts(s);
            if widgets::rich_item(ui, t, &format!("{STYLE_ID}/{}", s.id()), &row_of(s, &texts), s == v.style).clicked() {
                picked = Some(s);
            }
        }
    });
    if let Some(s) = picked.filter(|s| *s != v.style) {
        actions.push(("collage.style", json!({ "style": s.id() })));
    }
    ui.add_space(ROW_GAP);

    // Picture borders.
    let on = v.style.has_borders();
    row(ui, TOGGLE.y, |ui| {
        label(ui, t, &text("Picture borders"), on);
        ui.add_enabled_ui(on, |ui| {
            let tips = [text("No border"), text("White border"), text("Instant photo border")];
            let toggles = [
                ImageToggle { id: BORDER_IDS[0], glyph: Glyph::Collage(CollageIcon::NoBorder), tooltip: &tips[0] },
                ImageToggle { id: BORDER_IDS[1], glyph: Glyph::Collage(CollageIcon::WhiteBorder), tooltip: &tips[1] },
                ImageToggle { id: BORDER_IDS[2], glyph: Glyph::Collage(CollageIcon::InstantBorder), tooltip: &tips[2] },
            ];
            let chosen = match v.border {
                Border::None => 0,
                Border::White => 1,
                Border::Instant => 2,
            };
            let out = widgets::toggle_group(ui, t, &toggles, chosen, TOGGLE);
            if let Some(i) = out.chosen.filter(|i| *i != chosen) {
                let id = ["none", "white", "instant"].get(i).copied().unwrap_or("none");
                actions.push(("collage.settings", json!({ "borders": id })));
            }
        });
    });
    ui.add_space(ROW_GAP);

    // Grid spacing: the value follows a drag, and the collage takes it when the drag ends.
    let on = v.style.has_spacing();
    let spacing_label = text("Grid spacing");
    let spec = SliderSpec { id: SPACING_ID, label: &spacing_label, min: 0.0, max: SPACING_MAX, default: 1.0, step: 0.1, decimals: 1, unit: "%" };
    let dragged: Option<f64> = ctx.data(|d| d.get_temp(spacing_id()));
    ui.add_enabled_ui(on, |ui| {
        let out = widgets::slider(ui, t, &spec, dragged.unwrap_or(v.spacing));
        let value = out.value.or(dragged);
        if out.response.dragged() {
            if let Some(x) = value {
                ctx.data_mut(|d| d.insert_temp(spacing_id(), x));
            }
        } else {
            ctx.data_mut(|d| d.remove::<f64>(spacing_id()));
            if let Some(x) = value.filter(|x| (x - v.spacing).abs() > 1e-9) {
                actions.push(("collage.settings", json!({ "spacing": (x / SPACING_MAX).clamp(0.0, 1.0) })));
            }
        }
    });
    ui.add_space(ROW_GAP);

    // Background.
    let on = v.style.has_background();
    ui.add_enabled_ui(on, |ui| {
        row(ui, TOGGLE.y, |ui| {
            label(ui, t, &text("Background"), on);
            let armed = eyedropper_armed(&ctx);
            let (r, out) = widgets::radio_row(ui, t, SOLID_ID, &text("Solid color"), v.picture.is_none(), |ui| {
                ui.add_space(8.0);
                widgets::swatch(ui, t, SWATCH_ID, &text("Background color"), v.colour, armed)
            });
            if r.clicked() && v.picture.is_some() {
                actions.push(("collage.settings", json!({ "usePicture": false })));
            }
            if let Some(c) = out.color {
                actions.push(("collage.settings", json!({ "background": hex_u8(c) })));
            }
            if out.eyedropper {
                set_eyedropper(&ctx, !armed);
            }
        });
        row(ui, TOGGLE.y, |ui| {
            label(ui, t, "", true);
            if widgets::radio(ui, t, PICTURE_ID, &text("Use picture"), v.picture.is_some()).clicked() && v.picture.is_none() {
                actions.push(("collage.settings", json!({ "usePicture": true })));
            }
        });
    });
    if !on && eyedropper_armed(&ctx) {
        set_eyedropper(&ctx, false);
    }
    ui.add_space(ROW_GAP);

    // Page format and orientation.
    let rows = formats(app, &ctx, &v.format);
    let custom = rows.iter().any(|r| r.name == v.format && r.custom);
    row(ui, TOGGLE.y, |ui| {
        label(ui, t, &text("Page format"), true);
        let mut picked = None;
        let name_of = |f: &FormatRow| if f.custom { f.name.clone() } else { crate::panels::edit::crop::ratio_name(&f.name) };
        let current = rows.iter().find(|f| f.name == v.format).map_or_else(|| crate::panels::edit::crop::ratio_name(&v.format), name_of);
        widgets::dropdown(ui, t, FORMAT_ID, &current, |ui| {
            for (i, f) in rows.iter().enumerate() {
                if widgets::dropdown_item(ui, t, &format!("{FORMAT_ID}/{i}"), &name_of(f), "", f.name == v.format).clicked() {
                    picked = Some(f.name.clone());
                }
            }
        });
        if let Some(name) = picked.filter(|n| *n != v.format) {
            actions.push(("collage.settings", json!({ "format": name })));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(custom, |ui| {
                let r = glyph_button(ui, t, DELETE_FORMAT_ID, Glyph::Edit(EditIcon::Trash), &text("Delete this page format"));
                if r.clicked() {
                    actions.push(("collage.formats.remove", json!({ "name": v.format })));
                }
            });
        });
    });
    ui.add_space(4.0);
    row(ui, TOGGLE.y, |ui| {
        label(ui, t, "", true);
        let tips = [text("Landscape"), text("Portrait")];
        let toggles = [
            ImageToggle { id: ORIENTATION_IDS[0], glyph: Glyph::Collage(CollageIcon::Landscape), tooltip: &tips[0] },
            ImageToggle { id: ORIENTATION_IDS[1], glyph: Glyph::Collage(CollageIcon::Portrait), tooltip: &tips[1] },
        ];
        let chosen = usize::from(v.orientation == Orientation::Portrait);
        let out = widgets::toggle_group(ui, t, &toggles, chosen, TOGGLE);
        if let Some(i) = out.chosen.filter(|i| *i != chosen) {
            actions.push(("collage.settings", json!({ "orientation": if i == 1 { "portrait" } else { "landscape" } })));
        }
    });
    ui.add_space(ROW_GAP);

    // Draw shadows, Show captions: on one row, or Show captions on the next where a longer
    // language leaves no room for both.
    let (shadows_label, captions_label) = (text("Draw shadows"), text("Show captions"));
    let captions_w = ui.painter().layout_no_wrap(captions_label.clone(), fonts::regular(t.body), t.text).size().x;
    let mut room = 0.0;
    row(ui, TOGGLE.y, |ui| {
        let mut shadows = v.shadows;
        let r = widgets::checkbox(ui, t, SHADOWS_ID, &shadows_label, &mut shadows);
        if r.changed() {
            actions.push(("collage.settings", json!({ "shadows": shadows })));
        }
        // The box and its gap: what the first checkbox takes beyond its label.
        let label_w = ui.painter().layout_no_wrap(shadows_label.clone(), fonts::regular(t.body), t.text).size().x;
        room = ui.available_width() - 16.0 - (captions_w + (r.rect.width() - label_w).max(0.0));
        if room >= 0.0 {
            ui.add_space(16.0);
            captions_check(ui, t, v, &captions_label, actions);
        }
    });
    if room < 0.0 {
        ui.add_space(4.0);
        row(ui, TOGGLE.y, |ui| captions_check(ui, t, v, &captions_label, actions));
    }
}

/// Show captions, live for a style that draws captions.
fn captions_check(ui: &mut Ui, t: &Tokens, v: &Shown, label: &str, actions: &mut Vec<Action>) {
    ui.add_enabled_ui(v.style.has_captions(), |ui| {
        let mut captions = v.captions;
        if widgets::checkbox(ui, t, CAPTIONS_ID, label, &mut captions).changed() {
            actions.push(("collage.settings", json!({ "captions": captions })));
        }
    });
}

/// Create Collage (or the collage's progress and Cancel while it is written), then Reset and
/// Close, in `rect` along the bottom of the tab.
fn footer(app: &mut App, ui: &mut Ui, t: &Tokens, rect: Rect, actions: &mut Vec<Action>) {
    let enabled = |app: &App, id: &str| crate::commands::enabled(app, id, &json!({}));
    let mut foot = ui.new_child(UiBuilder::new().id_salt("collageFoot").max_rect(rect).layout(Layout::top_down(Align::Min)));
    let running = crate::commands::collage::running(app);
    row(&mut foot, 24.0, |ui| match &running {
        Some(job) => {
            let fraction = (job.progress.total > 0).then(|| (job.progress.done as f64 / job.progress.total as f64) as f32);
            let out = widgets::progress_row(
                ui,
                t,
                PROGRESS_ID,
                &crate::i18n::tr(&job.label),
                fraction,
                job.cancellable.then_some(CANCEL_ID),
                ui.available_width(),
            );
            if out.cancel
                && let Some(s) = app.session_mut()
                && let Err(e) = s.cancel_jobs(Some(job.job))
            {
                app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
            }
        }
        None => {
            let ready = enabled(app, "collage.createCollage");
            ui.add_enabled_ui(ready.is_ok(), |ui| {
                let r = widgets::primary_button(ui, t, CREATE_ID, &text("Create Collage"));
                let r = match &ready {
                    Err(why) => r.on_disabled_hover_text(crate::i18n::tr(why)),
                    Ok(()) => r,
                };
                if r.clicked() {
                    actions.push(("collage.createCollage", json!({})));
                }
            });
        }
    });
    foot.add_space(8.0);
    row(&mut foot, 24.0, |ui| {
        let ready = enabled(app, "collage.resetSettings");
        ui.add_enabled_ui(ready.is_ok(), |ui| {
            if widgets::text_button(ui, t, RESET_ID, &text("Reset"), false).on_hover_text(text("Put the settings back to their defaults")).clicked() {
                actions.push(("collage.resetSettings", json!({})));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::text_button(ui, t, CLOSE_ID, &text("Close"), false).clicked() {
                actions.push(("collage.closeRoom", json!({})));
            }
        });
    });
}

/// The eyedropper's click at `at` on the canvas's page (each 0–1 across it): the colour under it
/// becomes the background's (`collage.settings {background}`), and the eyedropper disarms. Where no
/// picture is, nothing changes.
pub fn pick(app: &mut App, ctx: &Context, at: [f64; 2]) {
    set_eyedropper(ctx, false);
    let Some(colour) = app.session_mut().and_then(|s| colour_at(s, at)) else { return };
    if let Err(e) = app.run("collage.settings", &json!({ "background": hex_u8(colour) })) {
        app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
    }
}

/// The colour of the open collage at `at` on its page (each 0–1 across it): the topmost picture
/// whose box holds the point, at the part of it shown there; `None` where no picture is or its
/// pixels are not in memory. Never touches the disk.
pub fn colour_at(session: &mut Session, at: [f64; 2]) -> Option<[u8; 3]> {
    if !at.iter().all(|v| v.is_finite()) {
        return None;
    }
    let ratio = session.collage().document()?.page.ratio();
    if !(ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let mut placements = session.collage_placements();
    placements.sort_by_key(|p| std::cmp::Reverse(p.z));
    let p = [at[0] * ratio, at[1]];
    for pl in &placements {
        let (cx, cy) = (pl.centre[0] * ratio, pl.centre[1]);
        let (w, h) = (pl.size[0] * ratio, pl.size[1]);
        if !(w > 0.0 && h > 0.0) {
            continue;
        }
        let a = pl.angle.to_radians();
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        let (lx, ly) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
        let (u, v) = (lx / w + 0.5, ly / h + 0.5);
        if !((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)) {
            continue;
        }
        let shown: [f64; 4] = pl.shown.into();
        let x = shown[0] + u * (shown[2] - shown[0]);
        let y = shown[1] + v * (shown[3] - shown[1]);
        return sample(session, pl.id, x, y);
    }
    None
}

/// Photo `id`'s colour at `(x, y)` of the picture as it shows (each 0–1 across it): from its
/// thumbnail in memory, else its colour sketch.
fn sample(session: &mut Session, id: PhotoId, x: f64, y: f64) -> Option<[u8; 3]> {
    let photo = session.library().photo(id)?;
    let turn = shape_of(&session.library().info(id).edits, photo.width, photo.height).turn;
    let (ux, uy) = turn_point(turn.inverse(), x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32);
    let image = match session.thumbnail_now(id, SAMPLE_NEED) {
        Thumb::Image { image, .. } => image,
        _ => {
            let sketch = session.library().sketch(id)?;
            Arc::new(dalica_thumbs::sketch::decode(sketch.as_bytes()).ok()?.to_rgba(SKETCH_SIDE))
        }
    };
    let (w, h) = (image.width, image.height);
    if w == 0 || h == 0 {
        return None;
    }
    let px = ((ux * (w - 1) as f32).round() as u32).min(w - 1);
    let py = ((uy * (h - 1) as f32).round() as u32).min(h - 1);
    let at = (py as usize).checked_mul(w as usize)?.checked_add(px as usize)?.checked_mul(4)?;
    let rgba = image.data.get(at..at.checked_add(3)?)?;
    Some([*rgba.first()?, *rgba.get(1)?, *rgba.get(2)?])
}
