//! The Crop tool (`editing.md` §6.1): the "Crop Photo" panel in place of the tab's content, and the
//! rectangle drawn on the photo.
//!
//! **The panel** ([`show`]): the title with its glyph and the instruction; the size dropdown
//! (`choice:crop.ratio`: the live "Manual: 1210 × 1719" row, "Current ratio", the locale's photo
//! sizes, the fixed ratios, "Custom ratios" with the user's own and "Add Custom Ratio…", rows
//! `choice:crop.ratio/<row>`) and the trash beside it (`button:crop.removeRatio`, for a custom
//! ratio); the three 80 × 50 suggestions (`button:crop.suggestion/1` … `/3`, from
//! `photo.crop.suggest`, a job); Rotate (`button:crop.rotate`, greyed until a rectangle exists) |
//! Preview (`button:crop.preview`: a click toggles it, a press held shows the crop until let go);
//! Reset (`button:panel.reset`); Apply (`button:panel.apply`) | Cancel (`button:panel.cancel`).
//!
//! **On the photo** (`overlay`): while the tool is open the photo area shows the whole photo (the
//! recipe without its crops, handed to the preview as a candidate), with the rectangle drawn so far
//! (`crop:edit`) and the outside dimmed `backdrop` (black α140). A press outside the rectangle
//! draws a new one, an edge or a corner resizes it, the inside moves it; the ratio is locked unless
//! Manual; the thirds show while dragging. Preview hands the preview the recipe with the new crop
//! and hides the rectangle. Enter applies, Esc cancels; the wheel still zooms.
//!
//! **The draft** is the edit state's `crop` (`{ratio, rect}`, the rectangle in the stored frame);
//! what is not view state (the preview toggle, the suggestions, the size rows, a drag on its way,
//! the candidate handed to the preview) is kept per thread here, as the canvas keeps its fitted
//! scale, and starts afresh each time the tool opens. A re-opened crop starts from the rectangle in
//! effect and the last ratio used; Apply runs `photo.crop.apply` (one "Crop" step) with the draft's
//! ratio, which the engine remembers as the last one.
//!
//! **A│A's right side** (`editing.md` §4): the tool works on the variant's recipe
//! (`tools::recipe`): the rectangle in effect, the whole photo shown and the Preview come from it,
//! and Apply appends the `crop(…)` `photo.crop.apply` would ([`variant_op`]) as one step of the
//! variant's history (`tools::variant_step`), remembering the ratio through `options.set`; the
//! photo's own recipe is not touched.
//!
//! **Scripts** drive the same draft through `edit.tool.action {name, params}`: `ratio {ratio,
//! name?}`, `draw {from, to}` and `drag {handle, from, to}` (points `[x, y]` normalised to the
//! photo as shown), `rotate`, `preview {on?}`, `suggestion {index}` (0 … 2), `addRatio` and
//! `removeRatio`. Each answers the draft.

pub mod geom;

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_edit::crop_geom::Handle;
use dalica_edit::ops::Typed;
use dalica_edit::{Op, Point, Recipe, Rect as NRect};
use dalica_engine::jobs::{JobId, JobState};
use dalica_engine::{CmdError, Key};
use dalica_image::{Orientation, Rgba8};
use dalica_ui_kit::icons::{self, EditIcon, Icon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align2, Color32, CornerRadius, Id, LayerId, Order, Popup, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, Vec2,
    WidgetType, pos2, vec2,
};
use serde::Deserialize;
use serde_json::{Value, json};

use self::geom::Shown;
use super::canvas;
use crate::app::App;
use crate::commands::tools::{self, Tool};
use crate::dialogs::{self, custom_ratio};
use crate::state::{CropDraft, EditState, ToolKind, is_ratio};

pub struct Crop;

pub static CROP: Crop = Crop;

/// The panel's title and instruction (`editing.md` §6.1).
pub const TITLE: &str = "Crop Photo";
pub const INSTRUCTION: &str = "Choose a size below, then drag on the photo to select the part you want to keep.";
/// The automation ids.
pub const RATIO_ID: &str = "choice:crop.ratio";
pub const REMOVE_ID: &str = "button:crop.removeRatio";
pub const ROTATE_ID: &str = "button:crop.rotate";
pub const PREVIEW_ID: &str = "button:crop.preview";
pub const RESET_ID: &str = "button:panel.reset";
pub const APPLY_ID: &str = "button:panel.apply";
pub const CANCEL_ID: &str = "button:panel.cancel";
pub const RECT_ID: &str = "crop:edit";
/// The size of a suggestion's thumbnail, and how many there are.
pub const SUGGESTION: Vec2 = vec2(80.0, 50.0);
pub const SUGGESTIONS: usize = 3;
/// How long a press on Preview is held before it shows the crop only while held, in seconds.
pub const HOLD: f64 = 0.3;
/// How far (points) a press moves before it draws a new rectangle.
pub const DRAW_SLOP: f32 = 3.0;
/// The side of the squares on the rectangle's corners, in points.
pub const HANDLE: f32 = 7.0;
/// The widths of the panel's buttons and the gap between two.
pub const BUTTON_W: f32 = 110.0;
pub const BUTTON_GAP: f32 = 8.0;

/// The suggestion button `n` (1 … 3).
pub fn suggestion_id(n: usize) -> String {
    format!("button:crop.suggestion/{n}")
}

/// One suggested rectangle: the stored `[x, y, w, h]` and why it was suggested.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub rect: [f32; 4],
    pub reason: String,
}

/// One row of the size dropdown after Manual and Current ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    /// `"W:H"`, as the draft and `photo.crop.apply` write it.
    pub ratio: String,
    pub custom: bool,
    /// The label in the list ("4 × 6", "Print (3 × 2)").
    pub label: String,
}

/// The rows of `crop.ratios.list`: the photo sizes, the fixed ratios, the custom ones.
pub fn rows_of(list: &Value) -> Vec<Row> {
    let mut rows = Vec::new();
    for (key, custom) in [("sizes", false), ("fixed", false), ("custom", true)] {
        for r in list.get(key).and_then(Value::as_array).into_iter().flatten() {
            let (Some(name), Some(ratio)) = (r.get("name").and_then(Value::as_str), r.get("ratio").and_then(Value::as_str)) else { continue };
            let label = match (custom, r.get("width").and_then(Value::as_f64), r.get("height").and_then(Value::as_f64)) {
                (true, Some(w), Some(h)) => format!("{name} ({w} × {h})"),
                _ => name.to_string(),
            };
            rows.push(Row { name: name.to_string(), ratio: ratio.to_string(), custom, label });
        }
    }
    rows
}

/// A drag on the photo on its way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    /// The handle held; `None` draws a new rectangle from `grab`.
    handle: Option<Handle>,
    /// The rectangle when the press began (displayed).
    start: Option<NRect>,
    /// Where the press began, normalised to the photo shown, and in screen points.
    grab: Point,
    at: Pos2,
    aspect: Option<f64>,
    /// It moved far enough to draw (a press that does not move draws nothing).
    moved: bool,
}

/// A suggestion job asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Asked {
    photo: PhotoId,
    aspect: f64,
    job: Option<JobId>,
    done: bool,
}

/// The texture of the whole photo the suggestions are drawn from.
#[derive(Clone)]
struct Whole {
    source: Weak<Rgba8>,
    handle: TextureHandle,
}

/// What the tool keeps besides its draft, per thread (the UI thread), from one open to the next.
#[derive(Default)]
struct Extra {
    photo: Option<PhotoId>,
    /// The size row chosen by name (a custom ratio and a fixed one may share a ratio).
    row: Option<String>,
    /// Preview toggled on; a press on Preview held since this time, and whether it is held long.
    preview: bool,
    press: Option<f64>,
    holding: bool,
    asked: Option<Asked>,
    suggestions: Vec<Suggestion>,
    /// What was handed to the preview as the candidate (`Some(None)`: the photo's own recipe).
    candidate: Option<Option<Vec<String>>>,
    rows: Option<Vec<Row>>,
    drag: Option<Drag>,
    whole: Option<Whole>,
}

thread_local! {
    static EXTRA: RefCell<Extra> = RefCell::new(Extra::default());
}

fn with<R>(f: impl FnOnce(&mut Extra) -> R) -> R {
    EXTRA.with(|e| f(&mut e.borrow_mut()))
}

/// A fresh start for photo `id`, keeping only the candidate handed to the preview (the next frame
/// replaces it when it is no longer the one wanted).
fn restart(id: PhotoId) {
    with(|x| {
        let candidate = x.candidate.take();
        *x = Extra { photo: Some(id), candidate, ..Extra::default() };
    });
}

/// The suggestions answered for the open crop (empty until the job answers).
pub fn suggestions() -> Vec<Suggestion> {
    with(|x| x.suggestions.clone())
}

/// Whether Preview shows the cropped photo now (toggled or held).
pub fn previewing() -> bool {
    with(|x| x.preview || x.holding)
}

/// The recipe the tool works on: the photo's, or its variant's on A│A's right side.
fn edits(app: &App, id: PhotoId) -> Vec<String> {
    tools::recipe(app, id)
}

/// `edits` without its crops: what the tool shows.
pub fn without_crops(edits: &[String]) -> Vec<String> {
    edits.iter().filter(|t| Op::parse(t).name() != Some("crop")).cloned().collect()
}

/// The crop in effect in `edits` (the last one Dalica reads), as a draft rectangle; `None` for the
/// whole photo.
pub fn crop_in(edits: &[String]) -> Option<[f32; 4]> {
    let c = edits.iter().rev().find_map(|t| match Op::parse(t).typed() {
        Some(Ok(Typed::Crop(c))) => Some(c),
        _ => None,
    })?;
    let r = geom::unit(c.rect);
    let whole = r.x0 <= 0.0 && r.y0 <= 0.0 && r.x1 >= 1.0 && r.y1 >= 1.0;
    (!whole).then(|| Shown::new(1.0, 1.0, dalica_edit::Dihedral::IDENTITY).to_stored(r))
}

/// The `crop(…)` step Apply takes on A│A's variant, as `photo.crop.apply` takes it on the photo:
/// the rectangle's corners clamped into the picture and kept to a millionth; `None` when it is the
/// crop in effect in `edits` already (the whole photo when there is none).
pub fn variant_op(edits: &[String], rect: [f32; 4]) -> Result<Option<String>, CmdError> {
    let r = |v: f64| (v.clamp(0.0, 1.0) * 1e6).round() / 1e6 + 0.0;
    let [x, y, w, h] = rect.map(f64::from);
    let crop = dalica_edit::ops::crop::Crop::new(NRect::new(r(x), r(y), r(x + w), r(y + h)))
        .ok_or_else(|| CmdError::BadParams("rect: too small to keep anything".into()))?;
    let current = edits
        .iter()
        .rev()
        .find_map(|t| match Op::parse(t).typed() {
            Some(Ok(Typed::Crop(c))) => Some(c),
            _ => None,
        })
        .unwrap_or_default();
    Ok((current != crop).then(|| format!("crop({crop})")))
}

/// The `crop(…)` text of a draft rectangle.
fn crop_text(rect: [f32; 4]) -> Option<String> {
    let [x, y, w, h] = rect.map(f64::from);
    dalica_edit::ops::crop::Crop::new(NRect::new(x, y, x + w, y + h)).map(|c| format!("crop({c})"))
}

/// The photo as the tool shows it: its whole picture, turned by its recipe.
pub fn shown(app: &App, id: PhotoId) -> Shown {
    let whole = without_crops(&edits(app, id));
    let recipe = Recipe::parse(&whole).unwrap_or_default();
    let turn = recipe.orientation();
    let dims = app.session().and_then(|s| s.library().photo(id)).map_or((0, 0), |p| (p.width, p.height));
    if dims.0 > 0 && dims.1 > 0 {
        let (w, h) = recipe.display_size(Orientation::default(), dims.0, dims.1);
        return Shown::new(f64::from(w), f64::from(h), turn);
    }
    match app.session().and_then(|s| s.preview()).filter(|p| p.photo == id && p.recipe == whole) {
        Some(p) => Shown::new(f64::from(p.image.width), f64::from(p.image.height), turn),
        None => Shown::new(1.0, 1.0, turn),
    }
}

/// The open photo and its draft, while the Crop tool is open.
fn open_draft(app: &App) -> Result<(PhotoId, CropDraft), CmdError> {
    let e = &app.state().edit;
    match (e.photo, e.tool, &e.crop) {
        (Some(id), Some(ToolKind::Crop), Some(d)) => Ok((id, d.clone())),
        _ => Err(CmdError::Disabled("Open the Crop tool first.".into())),
    }
}

/// The draft as the actions answer it.
fn draft_json(app: &App) -> Value {
    json!({ "crop": app.state().edit.crop, "preview": previewing() })
}

/// Sets the draft's rectangle from a displayed one.
fn set_rect(app: &mut App, shown: &Shown, rect: Option<NRect>) {
    let stored = rect.map(|r| shown.to_stored(r));
    if let Some(c) = &mut app.state_mut().edit.crop {
        c.rect = stored;
    }
}

/// The draft's rectangle, displayed.
fn display_rect(app: &App, shown: &Shown) -> Option<NRect> {
    app.state().edit.crop.as_ref().and_then(|c| c.rect).map(|r| shown.to_display(r))
}

/// The size rows (asked of the engine once per open, and again after a ratio is added or removed).
fn rows(app: &mut App) -> Vec<Row> {
    if let Some(r) = with(|x| x.rows.clone()) {
        return r;
    }
    let list = app.session_mut().and_then(|s| s.execute("crop.ratios.list", &json!({})).ok()).unwrap_or(Value::Null);
    let rows = rows_of(&list);
    with(|x| x.rows = Some(rows.clone()));
    rows
}

/// Forgets the size rows (a custom ratio was added or removed).
pub fn ratios_changed() {
    with(|x| x.rows = None);
}

/// The size row shown as chosen: the one chosen by name when it still has the draft's ratio, else
/// the first with it.
fn chosen_row(rows: &[Row], ratio: &str) -> Option<Row> {
    let named = with(|x| x.row.clone());
    rows.iter().find(|r| Some(&r.name) == named.as_ref() && r.ratio == ratio).or_else(|| rows.iter().find(|r| r.ratio == ratio)).cloned()
}

/// Chooses size row `ratio` (named `name`): a ratio draws its largest rectangle centred on the
/// photo, turned to the rectangle drawn (else to the photo); Manual keeps the rectangle.
pub fn choose(app: &mut App, ratio: &str, name: Option<&str>) -> Result<Value, CmdError> {
    let (id, _) = open_draft(app)?;
    if !is_ratio(ratio) {
        return Err(CmdError::BadParams("`ratio` is \"manual\", \"current\" or \"W:H\" (sides above 0)".into()));
    }
    let shown = shown(app, id);
    let before = display_rect(app, &shown);
    if let Some(c) = &mut app.state_mut().edit.crop {
        c.ratio = ratio.to_string();
    }
    if ratio != "manual" {
        set_rect(app, &shown, Some(geom::chosen(ratio, &shown, before)));
    }
    with(|x| x.row = name.map(str::to_string));
    Ok(draft_json(app))
}

/// A new rectangle drawn from `from` to `to` (displayed, normalised).
fn draw(app: &mut App, from: Point, to: Point) -> Result<Value, CmdError> {
    let (id, d) = open_draft(app)?;
    let shown = shown(app, id);
    let aspect = geom::locked(&d.ratio, &shown, display_rect(app, &shown));
    set_rect(app, &shown, Some(geom::draw(from, to, &shown, aspect)));
    Ok(draft_json(app))
}

/// `handle` of the rectangle dragged from `from` to `to`.
fn drag(app: &mut App, handle: Handle, from: Point, to: Point) -> Result<Value, CmdError> {
    let (id, d) = open_draft(app)?;
    let shown = shown(app, id);
    let Some(start) = display_rect(app, &shown) else { return Err(CmdError::Disabled("Draw a rectangle on the photo first.".into())) };
    let aspect = geom::locked(&d.ratio, &shown, Some(start));
    set_rect(app, &shown, Some(geom::drag(start, handle, from, to, &shown, aspect)));
    Ok(draft_json(app))
}

/// Rotate: the rectangle between portrait and landscape.
fn rotate(app: &mut App) -> Result<Value, CmdError> {
    let (id, _) = open_draft(app)?;
    let shown = shown(app, id);
    let Some(r) = display_rect(app, &shown) else { return Err(CmdError::Disabled("Draw a rectangle on the photo first.".into())) };
    set_rect(app, &shown, Some(geom::rotate(r, &shown)));
    Ok(draft_json(app))
}

/// Suggestion `index` (0 … 2) drawn as the rectangle.
fn take_suggestion(app: &mut App, index: usize) -> Result<Value, CmdError> {
    open_draft(app)?;
    if index >= SUGGESTIONS {
        return Err(CmdError::BadParams("`index` is 0, 1 or 2".into()));
    }
    let Some(s) = with(|x| x.suggestions.get(index).cloned()) else { return Err(CmdError::Disabled("The suggestions are not ready yet.".into())) };
    if let Some(c) = &mut app.state_mut().edit.crop {
        c.rect = Some(s.rect);
    }
    Ok(draft_json(app))
}

/// Deletes the custom ratio chosen; the draft goes back to Manual, keeping its rectangle.
fn remove_ratio(app: &mut App) -> Result<Value, CmdError> {
    let (_, d) = open_draft(app)?;
    let rows = rows(app);
    let row = chosen_row(&rows, &d.ratio).filter(|r| r.custom).ok_or_else(|| CmdError::Disabled("Choose a custom ratio to delete.".into()))?;
    app.run("crop.ratios.remove", &json!({ "name": row.name }))?;
    ratios_changed();
    choose(app, "manual", None)
}

/// Remembers `ratio` as the last one used (what `photo.crop.apply` does on the photo), when it is
/// not already; a refusal is only a status line, as the step is taken.
fn remember_ratio(app: &mut App, ratio: &str) {
    let last = app.session().and_then(|s| s.settings().edit().last_crop_ratio);
    if last.as_deref() != Some(ratio)
        && let Err(e) = app.run("options.set", &json!({ "values": { "edit.lastCropRatio": ratio } }))
    {
        app.state_mut().status = e.to_string();
    }
}

/// Drops the candidate handed to the preview (the photo's own recipe shows again) and what the
/// tool kept.
fn release(app: &mut App) {
    let handed = with(|x| std::mem::take(x).candidate);
    if matches!(handed, Some(Some(_)))
        && let Some(s) = app.session_mut()
    {
        let _ = s.set_preview_candidate(None);
    }
}

/// Cancel: the photo's own recipe shows again and the tool closes.
fn cancel(app: &mut App) -> Result<Value, CmdError> {
    release(app);
    app.run("edit.tool.cancel", &json!({}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RatioParams {
    ratio: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DrawParams {
    from: [f64; 2],
    to: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DragParams {
    handle: String,
    from: [f64; 2],
    to: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewParams {
    #[serde(default)]
    on: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestionParams {
    index: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

fn parse<T: serde::de::DeserializeOwned>(name: &str, params: &Value) -> Result<T, CmdError> {
    serde_json::from_value(params.clone()).map_err(|e| CmdError::BadParams(format!("{name}: {e}")))
}

/// A point `[x, y]` normalised to the photo shown; finite, within a photo's width of it.
fn point(p: [f64; 2]) -> Result<Point, CmdError> {
    if p.iter().all(|v| v.is_finite() && (-1.0..=2.0).contains(v)) {
        Ok(Point::new(p[0], p[1]))
    } else {
        Err(CmdError::BadParams("a point is [x, y], normalised to the photo shown (−1 … 2)".into()))
    }
}

impl Tool for Crop {
    fn kind(&self) -> ToolKind {
        ToolKind::Crop
    }

    fn label(&self) -> &'static str {
        "Crop"
    }

    /// The rectangle in effect (a re-opened crop) and the last ratio used.
    fn open(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let id = edit.photo.ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
        let last = app.session().and_then(|s| s.settings().edit().last_crop_ratio).filter(|r| is_ratio(r));
        edit.crop = Some(CropDraft { ratio: last.unwrap_or_else(|| "manual".into()), rect: crop_in(&edits(app, id)) });
        restart(id);
        Ok(())
    }

    /// One "Crop" step: the rectangle drawn (the whole photo when none is: a crop removed).
    fn apply(&self, app: &mut App) -> Result<Value, CmdError> {
        let (id, d) = open_draft(app)?;
        let rect = d.rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let result = if tools::on_variant(app, id) {
            let op = variant_op(&edits(app, id), rect)?;
            let out = tools::variant_step(app, id, op)?;
            remember_ratio(app, &d.ratio);
            out
        } else {
            app.run("photo.crop.apply", &json!({ "id": id, "rect": rect, "ratio": d.ratio }))?
        };
        release(app);
        Ok(result)
    }

    fn action(&self, app: &mut App, name: &str, params: &Value) -> Result<Value, CmdError> {
        match name {
            "ratio" => {
                let p: RatioParams = parse(name, params)?;
                choose(app, &p.ratio, p.name.as_deref())
            }
            "draw" => {
                let p: DrawParams = parse(name, params)?;
                draw(app, point(p.from)?, point(p.to)?)
            }
            "drag" => {
                let p: DragParams = parse(name, params)?;
                let handle = Handle::from_name(&p.handle).ok_or_else(|| {
                    CmdError::BadParams("`handle` is topLeft, topRight, bottomRight, bottomLeft, top, right, bottom, left or move".into())
                })?;
                drag(app, handle, point(p.from)?, point(p.to)?)
            }
            "rotate" => {
                let NoParams {} = parse(name, params)?;
                rotate(app)
            }
            "preview" => {
                let p: PreviewParams = parse(name, params)?;
                open_draft(app)?;
                with(|x| x.preview = p.on.unwrap_or(!x.preview));
                Ok(draft_json(app))
            }
            "suggestion" => {
                let p: SuggestionParams = parse(name, params)?;
                take_suggestion(app, p.index)
            }
            "addRatio" => {
                let NoParams {} = parse(name, params)?;
                open_draft(app)?;
                dialogs::open(app, custom_ratio::dialog())
            }
            "removeRatio" => {
                let NoParams {} = parse(name, params)?;
                remove_ratio(app)
            }
            _ => Err(CmdError::BadParams(format!(
                "Crop has no action `{}` (ratio, draw, drag, rotate, preview, suggestion, addRatio, removeRatio)",
                name.chars().take(40).collect::<String>()
            ))),
        }
    }

    /// Enter applies the crop, Esc cancels it (`editing.md` §6.1).
    fn key(&self, app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
        match key.to_string().as_str() {
            "Enter" => Some(app.run("edit.tool.apply", &json!({}))),
            "Esc" => Some(cancel(app)),
            _ => None,
        }
    }
}

// --- the photo shown, the candidate and the suggestions ------------------------------------------

/// The recipe the photo area shows while the tool is open: the whole photo, or with Preview the
/// recipe the Apply would store.
fn wanted(app: &App, id: PhotoId) -> Vec<String> {
    let edits = edits(app, id);
    let rect = app.state().edit.crop.as_ref().and_then(|c| c.rect);
    match (previewing(), rect.and_then(crop_text)) {
        (true, Some(text)) => {
            let mut v = edits;
            v.push(text);
            v
        }
        _ => without_crops(&edits),
    }
}

/// Hands the preview the recipe it should show, when that changed. Returns that recipe.
fn sync_candidate(app: &mut App, id: PhotoId) -> Vec<String> {
    let want = wanted(app, id);
    let candidate = (want != edits(app, id)).then(|| want.clone());
    if with(|x| x.candidate.as_ref() != Some(&candidate))
        && let Some(s) = app.session_mut()
        && s.preview_photo() == Some(id)
        && s.set_preview_candidate(candidate.as_deref()).is_ok()
    {
        with(|x| x.candidate = Some(candidate));
    }
    want
}

/// Asks for the suggestions in the ratio chosen (the photo's own for Manual), once per ratio, and
/// takes the answer when the job has finished. Draws again while the job runs (a short one), so
/// the thumbnails show as soon as it answers and settling waits for them.
fn poll_suggestions(app: &mut App, ctx: &egui::Context, id: PhotoId, shown: &Shown) {
    take_suggestions(app, id, shown);
    if with(|x| x.asked.is_some_and(|a| !a.done)) {
        ctx.request_repaint();
    }
}

fn take_suggestions(app: &mut App, id: PhotoId, shown: &Shown) {
    let ratio = app.state().edit.crop.as_ref().map(|c| c.ratio.clone()).unwrap_or_default();
    let aspect = geom::locked(&ratio, shown, display_rect(app, shown)).unwrap_or_else(|| shown.aspect());
    let asked = with(|x| x.asked);
    let fresh = asked.is_some_and(|a| a.photo == id && (a.aspect - aspect).abs() <= 1e-9 * aspect.max(1.0));
    if !fresh {
        let job = app
            .session_mut()
            .and_then(|s| s.execute("photo.crop.suggest", &json!({ "id": id, "ratio": aspect })).ok())
            .and_then(|v| v.get("job").and_then(Value::as_u64));
        with(|x| {
            x.asked = Some(Asked { photo: id, aspect, job, done: job.is_none() });
            x.suggestions.clear();
        });
        return;
    }
    let Some(Asked { job: Some(job), done: false, .. }) = asked else { return };
    // A job no longer listed answers nothing.
    let entry = app.session().and_then(|s| s.job(job));
    if entry.as_ref().is_some_and(|e| !e.state.is_finished()) {
        return;
    }
    let list: Vec<Suggestion> = match entry {
        Some(e) if e.state == JobState::Done => suggestions_of(e.result.as_ref()),
        _ => Vec::new(),
    };
    with(|x| {
        x.suggestions = list;
        if let Some(a) = &mut x.asked {
            a.done = true;
        }
    });
}

/// The suggestions of a `photo.crop.suggest` result: at most three, each inside the photo.
pub fn suggestions_of(result: Option<&Value>) -> Vec<Suggestion> {
    result
        .and_then(|r| r.get("suggestions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let v: Vec<f32> = s.get("rect")?.as_array()?.iter().filter_map(Value::as_f64).map(|v| v as f32).collect();
            let rect: [f32; 4] = v.try_into().ok()?;
            let [x, y, w, h] = rect;
            let ok =
                rect.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) && w > 0.0 && h > 0.0 && x + w <= 1.0 + 1e-6 && y + h <= 1.0 + 1e-6;
            ok.then(|| Suggestion { rect, reason: s.get("reason").and_then(Value::as_str).unwrap_or("").chars().take(40).collect() })
        })
        .take(SUGGESTIONS)
        .collect()
}

/// The texture of the whole photo (the preview of the recipe without crops), once it shows.
fn whole_texture(app: &App, ctx: &egui::Context, id: PhotoId) -> Option<TextureHandle> {
    let whole = without_crops(&edits(app, id));
    if let Some(p) = app.session().and_then(|s| s.preview()).filter(|p| p.photo == id && p.recipe == whole) {
        let image: &Arc<Rgba8> = &p.image;
        let current = with(|x| x.whole.as_ref().is_some_and(|w| w.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, image))));
        if !current {
            let dims = [image.width as usize, image.height as usize];
            if dims[0] > 0 && dims[1] > 0 && dims[0].saturating_mul(dims[1]).saturating_mul(4) == image.data.len() {
                let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
                let handle = ctx.load_texture("crop:whole", pixels, egui::TextureOptions::LINEAR);
                with(|x| x.whole = Some(Whole { source: Arc::downgrade(image), handle }));
            }
        }
    }
    with(|x| x.whole.as_ref().map(|w| w.handle.clone()))
}

// --- the panel ----------------------------------------------------------------------------------

/// How a panel button looks.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Plain,
    Primary,
    /// Shown pressed while on.
    Toggle(bool),
}

/// A button filling `rect` with a centred SemiBold label (as `widgets::text_button` draws it, the
/// primary one filled `accent`), registered as `id`.
#[allow(clippy::too_many_arguments)]
fn button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, rect: Rect, enabled: bool, look: Look, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let down = r.is_pointer_button_down_on();
    let radius = CornerRadius::same(t.radius);
    let mut text = if enabled { t.text } else { t.text_disabled };
    match look {
        Look::Primary if enabled => {
            let fill = if down {
                t.accent_pressed
            } else if r.hovered() {
                t.accent_hover
            } else {
                t.accent
            };
            p.rect_filled(rect, radius, fill);
            text = t.on_accent;
        }
        _ => {
            let on = matches!(look, Look::Toggle(true));
            let fill = if enabled && (down || on) {
                t.pressed
            } else if enabled && r.hovered() {
                t.hover
            } else {
                t.button
            };
            p.rect(rect, radius, fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
        }
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::semibold(t.button_label), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 12.0).max(1.0));
    let galley = p.layout_job(job);
    p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), rect).min.round(), galley, text);
    widgets::register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), matches!(look, Look::Toggle(true)));
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::tooltip(r, t, tip, None)
}

/// `n` buttons centred side by side at `y`, each at most [`BUTTON_W`] wide.
pub fn row_rects(content: Rect, y: f32, h: f32, n: usize) -> Vec<Rect> {
    let count = n.max(1) as f32;
    let gaps = BUTTON_GAP * (count - 1.0);
    let w = BUTTON_W.min(((content.width() - gaps) / count).max(0.0)).floor();
    let x0 = (content.center().x - (w * count + gaps) / 2.0).round();
    (0..n.max(1)).map(|i| Rect::from_min_size(pos2(x0 + i as f32 * (w + BUTTON_GAP), y), vec2(w, h))).collect()
}

/// Where the panel's parts go in `content`: the title, the instruction (when there is room), the
/// dropdown and its trash, the three suggestions (smaller, or none, when room is short), Rotate,
/// Preview and Reset (on one row when room is short), Apply and Cancel. Apply and Cancel always
/// fit when the panel holds them at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub title: Rect,
    pub instruction: Option<Rect>,
    pub dropdown: Rect,
    pub trash: Rect,
    /// Empty when there is no room for them.
    pub suggestions: Vec<Rect>,
    pub rotate: Rect,
    pub preview: Rect,
    pub reset: Rect,
    pub apply: Rect,
    pub cancel: Rect,
}

/// The title row's height.
pub const TITLE_H: f32 = 26.0;
/// The smallest height a suggestion is drawn at.
pub const SUGGESTION_MIN_H: f32 = 24.0;

/// The panel's [`Layout`] in `content`, the instruction `instruction_h` tall, rows `row_h` (the
/// dropdown) and `button_h` (the buttons) tall.
pub fn layout(content: Rect, instruction_h: f32, row_h: f32, button_h: f32) -> Layout {
    let (x0, x1, top) = (content.min.x, content.max.x, content.min.y);
    let full = TITLE_H + 4.0 + instruction_h + 10.0 + row_h + 10.0 + SUGGESTION.y + 12.0 + button_h * 3.0 + 16.0;
    let roomy = content.height() >= full;
    let gap = if roomy { 10.0 } else { 6.0 };
    let title = Rect::from_min_max(pos2(x0, top), pos2(x1, top + TITLE_H));
    let mut y = title.max.y + if roomy { 4.0 } else { 2.0 };
    let instruction = roomy.then(|| {
        let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + instruction_h));
        y = r.max.y + gap;
        r
    });
    let trash = Rect::from_min_size(pos2(x1 - row_h, y), Vec2::splat(row_h));
    let dropdown = Rect::from_min_max(pos2(x0, y), pos2(trash.min.x - 4.0, y + row_h));
    y = dropdown.max.y + gap;
    // Room left for the suggestions once the button rows are placed.
    let rows_h = if roomy { button_h * 3.0 + 16.0 } else { button_h * 2.0 + 6.0 };
    let after = if roomy { 12.0 } else { 8.0 };
    let room = content.max.y - y - rows_h - after;
    let sh = SUGGESTION.y.min(room);
    let suggestions = if sh >= SUGGESTION_MIN_H {
        let size = vec2(SUGGESTION.x * sh / SUGGESTION.y, sh);
        let n = SUGGESTIONS as f32;
        let between = ((content.width() - size.x * n) / (n - 1.0)).clamp(0.0, 12.0);
        let left = (content.center().x - (size.x * n + between * (n - 1.0)) / 2.0).round();
        let list = (0..SUGGESTIONS).map(|i| Rect::from_min_size(pos2(left + i as f32 * (size.x + between), y), size)).collect();
        y += sh + after;
        list
    } else {
        Vec::new()
    };
    let (rotate, preview, reset);
    if roomy {
        let r = row_rects(content, y, button_h, 2);
        (rotate, preview) = (r[0], r[1]);
        y += button_h + 8.0;
        reset = row_rects(content, y, button_h, 1)[0];
        y += button_h + 8.0;
    } else {
        let r = row_rects(content, y, button_h, 3);
        (rotate, preview, reset) = (r[0], r[1], r[2]);
        y += button_h + 6.0;
    }
    let r = row_rects(content, y, button_h, 2);
    Layout { title, instruction, dropdown, trash, suggestions, rotate, preview, reset, apply: r[0], cancel: r[1] }
}

/// What the panel was asked this pass, run once it is drawn.
enum Ask {
    Ratio(String, Option<String>),
    Add,
    Remove,
    Suggestion(usize),
    Rotate,
    Preview,
    Reset,
    Apply,
    Cancel,
}

/// The photo area's part of the tool, drawn after the canvas (`panels/edit.rs`), so it runs with
/// the edit panel hidden too: keeps the photo area showing the right recipe, takes the suggestions
/// as they arrive, and paints and runs the rectangle.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let Ok((id, _)) = open_draft(app) else { return };
    let ctx = ui.ctx().clone();
    let t = *app.tokens();
    let shown = shown(app, id);
    if with(|x| x.photo != Some(id)) {
        restart(id);
    }
    let want = sync_candidate(app, id);
    poll_suggestions(app, &ctx, id, &shown);
    overlay(app, &ctx, &t, id, &shown, &want);
}

/// Draws the Crop panel in place of the tab's content.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Ok((id, draft)) = open_draft(app) else { return };
    let ctx = ui.ctx().clone();
    let t = *app.tokens();
    let shown = shown(app, id);
    if with(|x| x.photo != Some(id)) {
        restart(id);
    }
    let rows = rows(app);
    let now = ctx.input(|i| i.time);

    let content = ui.max_rect();
    let p = ui.painter().clone();
    let instruction = p.layout(INSTRUCTION.to_string(), fonts::regular(t.small), t.text_label, content.width());
    let l = layout(content, instruction.size().y, t.metrics.interact_h, t.metrics.button_h);
    // The title with its glyph, and the instruction.
    paint_edit(&p, Rect::from_min_size(pos2(l.title.min.x, l.title.center().y - 10.0), Vec2::splat(20.0)), EditIcon::Crop, t.icon);
    p.text(pos2(l.title.min.x + 28.0, l.title.center().y), Align2::LEFT_CENTER, TITLE, fonts::semibold(t.title), t.text);
    if let Some(r) = l.instruction {
        p.galley(r.min, instruction, t.text_label);
    }

    let mut asked = None;
    // The size dropdown and the trash beside it.
    let current = chosen_row(&rows, &draft.ratio);
    let label = match draft.ratio.as_str() {
        "manual" => {
            let (w, h) = shown.pixels(display_rect(app, &shown).unwrap_or(NRect::new(0.0, 0.0, 1.0, 1.0)));
            format!("Manual: {w} × {h}")
        }
        "current" => "Current ratio".to_string(),
        other => current.as_ref().map_or_else(|| other.replace(':', " × "), |r| r.label.clone()),
    };
    if let Some(a) = dropdown(ui, &t, l.dropdown, &label, &draft.ratio, current.as_ref(), &rows) {
        asked = Some(a);
    }
    if trash_button(ui, &t, l.trash, current.as_ref().is_some_and(|r| r.custom)).clicked() {
        asked = Some(Ask::Remove);
    }

    // The suggestions.
    let tex = whole_texture(app, &ctx, id);
    let list = suggestions();
    for (i, rect) in l.suggestions.iter().enumerate() {
        if suggestion(ui, &t, i, *rect, list.get(i), tex.as_ref(), &shown).clicked() {
            asked = Some(Ask::Suggestion(i));
        }
    }

    // Rotate | Preview; Reset; Apply | Cancel.
    if button(ui, &t, ROTATE_ID, "Rotate", l.rotate, draft.rect.is_some(), Look::Plain, "Turn the rectangle between portrait and landscape").clicked()
    {
        asked = Some(Ask::Rotate);
    }
    let pr =
        button(ui, &t, PREVIEW_ID, "Preview", l.preview, true, Look::Toggle(previewing()), "Show the cropped photo (hold to look while pressed)");
    preview_press(&pr, now, &mut asked);
    if button(ui, &t, RESET_ID, "Reset", l.reset, true, Look::Plain, "Start again from the crop in effect").clicked() {
        asked = Some(Ask::Reset);
    }
    if button(ui, &t, APPLY_ID, "Apply", l.apply, true, Look::Primary, "Apply the crop (Enter)").clicked() {
        asked = Some(Ask::Apply);
    }
    if button(ui, &t, CANCEL_ID, "Cancel", l.cancel, true, Look::Plain, "Close without cropping (Esc)").clicked() {
        asked = Some(Ask::Cancel);
    }

    let Some(asked) = asked else { return };
    let result = match asked {
        Ask::Ratio(ratio, name) => choose(app, &ratio, name.as_deref()),
        Ask::Add => app.run("edit.tool.action", &json!({ "name": "addRatio" })),
        Ask::Remove => remove_ratio(app),
        Ask::Suggestion(i) => take_suggestion(app, i),
        Ask::Rotate => rotate(app),
        Ask::Preview => {
            with(|x| x.preview = !x.preview);
            Ok(Value::Null)
        }
        Ask::Reset => app.run("edit.tool.reset", &json!({})),
        Ask::Apply => app.run("edit.tool.apply", &json!({})),
        Ask::Cancel => cancel(app),
    };
    if let Err(e) = result {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
    ctx.request_repaint();
}

/// Preview: a click toggles it; a press held [`HOLD`] seconds shows the crop until let go (and that
/// release is no click). A press that started on the button lasts while the primary button stays
/// down: egui stops counting a press as on a click-only widget once it is held past its longest
/// click (`max_click_duration`, 0.8 s), which would end a long look by itself.
fn preview_press(r: &Response, now: f64, asked: &mut Option<Ask>) {
    let (press, holding) = with(|x| (x.press, x.holding));
    let down = r.is_pointer_button_down_on() || (press.is_some() && r.ctx.input(|i| i.pointer.primary_down()));
    match (down, press) {
        (true, None) => {
            with(|x| x.press = Some(now));
            r.ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOLD));
        }
        (true, Some(at)) if !holding && now - at >= HOLD => with(|x| x.holding = true),
        (true, Some(at)) if !holding => r.ctx.request_repaint_after(std::time::Duration::from_secs_f64((HOLD - (now - at)).clamp(0.0, HOLD))),
        (true, Some(_)) => {}
        (false, press) => {
            if press.is_some() {
                with(|x| {
                    x.press = None;
                    x.holding = false;
                });
            }
            if !holding && r.clicked() {
                *asked = Some(Ask::Preview);
            }
        }
    }
}

/// The id of size row `row` in the dropdown.
pub fn row_id(row: &Row) -> String {
    if row.custom {
        let name: String = row.name.chars().map(|c| if c == '/' || c.is_control() { '_' } else { c }).collect();
        format!("{RATIO_ID}/custom/{}", name.trim())
    } else {
        format!("{RATIO_ID}/{}", row.ratio)
    }
}

/// The size dropdown filling `rect`: the label and a chevron on a field plate; a click opens the
/// rows below it.
fn dropdown(ui: &mut Ui, t: &Tokens, rect: Rect, label: &str, ratio: &str, current: Option<&Row>, rows: &[Row]) -> Option<Ask> {
    let enabled = ui.is_enabled();
    let r = ui.interact(rect, Id::new(RATIO_ID), if enabled { Sense::click() } else { Sense::hover() });
    let popup = Id::new(RATIO_ID).with("popup");
    let open = Popup::is_id_open(ui.ctx(), popup);
    let p = ui.painter();
    let fill = if enabled && (open || r.is_pointer_button_down_on()) {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.field
    };
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let color = if enabled { t.text } else { t.text_disabled };
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::regular(t.body), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 30.0).max(1.0));
    let galley = p.layout_job(job);
    p.galley(pos2(rect.min.x + 6.0, rect.center().y - galley.size().y / 2.0).round(), galley, color);
    icons::paint(p, Rect::from_center_size(pos2(rect.max.x - 11.0, rect.center().y), Vec2::splat(12.0)), Icon::ChevronDown, color);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    widgets::register(ui.ctx(), RATIO_ID, Role::Button, label, Target::Widget(&r), open);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ComboBox, enabled, label));
    if !enabled {
        return None;
    }
    let mut asked = None;
    Popup::menu(&r).id(popup).width(rect.width()).show(|ui| {
        ui.set_min_width(rect.width());
        let mut row = |ui: &mut Ui, id: &str, text: &str, on: bool, then: Ask| {
            let r = ui.add(egui::Button::selectable(on, text));
            widgets::register(ui.ctx(), id, Role::MenuItem, text, Target::Widget(&r), on);
            if r.clicked() {
                asked = Some(then);
                ui.close();
            }
        };
        let manual = if ratio == "manual" { label } else { "Manual" };
        row(ui, &format!("{RATIO_ID}/manual"), manual, ratio == "manual", Ask::Ratio("manual".into(), None));
        row(ui, &format!("{RATIO_ID}/current"), "Current ratio", ratio == "current", Ask::Ratio("current".into(), None));
        let mut header = false;
        for r in rows {
            if r.custom && !header {
                header = true;
                custom_header(ui, t);
            }
            let on = current.is_some_and(|c| c == r);
            row(ui, &row_id(r), &r.label, on, Ask::Ratio(r.ratio.clone(), Some(r.name.clone())));
        }
        if !header {
            custom_header(ui, t);
        }
        row(ui, &format!("{RATIO_ID}/add"), "Add Custom Ratio…", false, Ask::Add);
    });
    asked
}

/// The "Custom ratios" section header of the dropdown.
fn custom_header(ui: &mut Ui, t: &Tokens) {
    ui.separator();
    ui.label(egui::RichText::new("Custom ratios").font(fonts::semibold(t.small)).color(t.text_dim));
}

/// The trash beside the dropdown: deletes the custom ratio chosen (greyed for any other row).
fn trash_button(ui: &mut Ui, t: &Tokens, rect: Rect, enabled: bool) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(REMOVE_ID), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
    }
    let color = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_edit(p, rect.shrink(3.0), EditIcon::Trash, color);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let tip = "Delete this custom ratio";
    widgets::register(ui.ctx(), REMOVE_ID, Role::IconButton, tip, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, tip));
    widgets::tooltip(r, t, tip, None)
}

/// Suggestion `i` in `rect`: its part of the whole photo fitted on an `inset` plate (an empty plate
/// until the job answers), outlined while hovered; a click draws it.
fn suggestion(ui: &mut Ui, t: &Tokens, i: usize, rect: Rect, s: Option<&Suggestion>, tex: Option<&TextureHandle>, shown: &Shown) -> Response {
    let id = suggestion_id(i + 1);
    let enabled = s.is_some() && ui.is_enabled();
    let r = ui.interact(rect, Id::new(&id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(t.radius), t.inset);
    if let (Some(s), Some(tex)) = (s, tex) {
        let d = shown.to_display(s.rect);
        let (w, h) = ((d.x1 - d.x0) * shown.width, (d.y1 - d.y0) * shown.height);
        if w > 0.0 && h > 0.0 {
            let k = (f64::from(rect.width()) / w).min(f64::from(rect.height()) / h);
            let fitted = Rect::from_center_size(rect.center(), vec2((w * k) as f32, (h * k) as f32));
            let uv = Rect::from_min_max(pos2(d.x0 as f32, d.y0 as f32), pos2(d.x1 as f32, d.y1 as f32));
            p.image(tex.id(), fitted, uv, Color32::WHITE);
        }
    }
    if enabled && r.hovered() {
        p.rect_stroke(rect, CornerRadius::same(t.radius), Stroke::new(1.5, t.text), StrokeKind::Inside);
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let label = match s {
        Some(s) if !s.reason.is_empty() => format!("Suggested crop {} ({})", i + 1, s.reason),
        Some(_) => format!("Suggested crop {}", i + 1),
        None => format!("Suggested crop {} (on its way)", i + 1),
    };
    widgets::register(ui.ctx(), &id, Role::Button, &label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, &label));
    r
}

// --- the rectangle on the photo -----------------------------------------------------------------

/// The layer the rectangle is painted on, above the photo and under popups and dialogs. It is a
/// painter only (no area), so the photo area keeps the pointer and the wheel still zooms.
fn layer() -> LayerId {
    LayerId::new(Order::Middle, Id::new(RECT_ID))
}

/// Paints the rectangle over the photo and runs the pointer on it (`editing.md` §6.1), from the
/// canvas's layout of this frame. Nothing is drawn or taken while Preview shows, while the
/// photo area still shows another recipe than `want`; under a dialog it is drawn and takes no input.
fn overlay(app: &mut App, ctx: &egui::Context, t: &Tokens, id: PhotoId, shown: &Shown, want: &[String]) {
    let Some(view) = canvas::view(ctx).filter(|v| v.photo == id) else { return };
    let showing = app.session().and_then(|s| s.preview()).is_some_and(|p| p.photo == id && p.recipe == want);
    let image = view.image;
    if previewing() || !showing || !(image.is_finite() && image.width() > 0.0 && image.height() > 0.0) {
        with(|x| x.drag = None);
        return;
    }
    let to_norm = |p: Pos2| Point::new(f64::from((p.x - image.min.x) / image.width()), f64::from((p.y - image.min.y) / image.height()));
    let to_screen = |r: NRect| {
        Rect::from_min_max(
            pos2(image.min.x + r.x0 as f32 * image.width(), image.min.y + r.y0 as f32 * image.height()),
            pos2(image.min.x + r.x1 as f32 * image.width(), image.min.y + r.y1 as f32 * image.height()),
        )
    };

    // The pointer: a press on the photo area (not over a popup) starts a drag.
    let (pressed, down, at, origin) =
        ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos(), i.pointer.press_origin()));
    // Under a dialog the rectangle is drawn but takes no input.
    let free = app.state().dialogs.is_empty();
    if !free {
        with(|x| x.drag = None);
    }
    let over_canvas = |p: Pos2| free && view.area.contains(p) && ctx.layer_id_at(p).is_none_or(|l| l.order == Order::Background);
    let current = display_rect(app, shown);
    if pressed && let Some(o) = origin.filter(|o| over_canvas(*o)) {
        let handle = current.and_then(|r| geom::hit(to_screen(r), o));
        if handle.is_some() || image.expand(geom::EDGE_GRAB).contains(o) {
            let ratio = app.state().edit.crop.as_ref().map(|c| c.ratio.clone()).unwrap_or_default();
            let aspect = geom::locked(&ratio, shown, current);
            with(|x| x.drag = Some(Drag { handle, start: current, grab: to_norm(o), at: o, aspect, moved: false }));
        }
    }
    if let Some(mut d) = with(|x| x.drag) {
        if down {
            if let Some(p) = at {
                d.moved |= (p - d.at).length() > DRAW_SLOP;
                let next = match (d.handle, d.start) {
                    (Some(h), Some(start)) => Some(geom::drag(start, h, d.grab, to_norm(p), shown, d.aspect)),
                    _ if d.moved => Some(geom::draw(d.grab, to_norm(p), shown, d.aspect)),
                    _ => None,
                };
                if let Some(r) = next {
                    set_rect(app, shown, Some(r));
                }
            }
            with(|x| x.drag = Some(d));
        } else {
            with(|x| x.drag = None);
        }
    }
    let dragging = with(|x| x.drag.is_some_and(|d| d.moved || d.handle.is_some()));
    let rect = display_rect(app, shown).map(to_screen);
    if let Some(p) = at.filter(|p| over_canvas(*p) && image.contains(*p)) {
        let held = with(|x| x.drag.and_then(|d| d.handle));
        ctx.set_cursor_icon(geom::cursor(held.or_else(|| rect.and_then(|r| geom::hit(r, p)))));
    }

    // The paint: the outside dimmed, the thirds while dragging, the frame and its corners.
    let painter = ctx.layer_painter(layer()).with_clip_rect(view.area);
    if let Some(r) = rect {
        let parts = [
            Rect::from_min_max(image.min, pos2(image.max.x, r.min.y)),
            Rect::from_min_max(pos2(image.min.x, r.max.y), image.max),
            Rect::from_min_max(pos2(image.min.x, r.min.y), pos2(r.min.x, r.max.y)),
            Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(image.max.x, r.max.y)),
        ];
        for part in parts.iter().filter(|p| p.is_positive()) {
            painter.rect_filled(*part, 0, t.backdrop);
        }
        if dragging {
            let line = Stroke::new(1.0, Color32::from_white_alpha(150));
            for l in dalica_edit::guides::thirds() {
                let pts: Vec<Pos2> = l.iter().map(|[x, y]| pos2(r.min.x + x * r.width(), r.min.y + y * r.height())).collect();
                painter.extend(Shape::dashed_line(&pts, line, 4.0, 4.0));
            }
        }
        painter.rect_stroke(r, 0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Middle);
        for c in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()] {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(HANDLE)), 0, Color32::WHITE);
        }
        let (w, h) = display_rect(app, shown).map_or((0, 0), |d| shown.pixels(d));
        widgets::register(ctx, RECT_ID, Role::Other, &format!("Crop rectangle {w} × {h}"), Target::Painted { rect: r, clip: view.area }, dragging);
    }
    if with(|x| x.drag.is_some()) {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crop_in_effect_is_the_last_one_and_the_whole_photo_is_none() {
        assert_eq!(crop_in(&[]), None);
        assert_eq!(crop_in(&["crop(0,0,1,1)".into()]), None);
        let r = crop_in(&["crop(0.1,0.1,0.5,0.5)".into(), "autofix()".into(), "crop(0.2,0.25,0.6,0.75)".into()]).unwrap();
        for (a, b) in r.iter().zip([0.2, 0.25, 0.4, 0.5]) {
            assert!((a - b).abs() < 1e-6, "{r:?}");
        }
        assert_eq!(without_crops(&["rotate(90)".into(), "crop(0.1,0.1,0.5,0.5)".into(), "autofix()".into()]), vec!["rotate(90)", "autofix()"]);
        assert_eq!(crop_text([0.25, 0.5, 0.5, 0.25]).as_deref(), Some("crop(0.25,0.5,0.75,0.75)"));
        assert_eq!(crop_text([0.5, 0.5, 0.0, 0.25]), None);
    }

    #[test]
    fn rows_come_from_the_ratio_list_and_their_ids_are_valid() {
        let list = json!({
            "sizes": [{ "name": "4 × 6", "ratio": "4:6", "width": 4, "height": 6 }],
            "fixed": [{ "name": "Square (1:1)", "ratio": "1:1", "width": 1, "height": 1 }, { "bad": 1 }],
            "custom": [{ "name": "Print / wide", "ratio": "3:2", "width": 3.0, "height": 2.0 }],
        });
        let rows = rows_of(&list);
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), vec!["4 × 6", "Square (1:1)", "Print / wide (3 × 2)"]);
        for r in &rows {
            assert!(dalica_ui_kit::registry::is_valid_id(&row_id(r)), "{}", row_id(r));
        }
        assert!(rows_of(&Value::Null).is_empty());
    }

    #[test]
    fn suggestions_are_read_defensively() {
        let good = json!({ "suggestions": [
            { "rect": [0.1, 0.1, 0.5, 0.5], "reason": "subject" },
            { "rect": [0.5, 0.5, 0.6, 0.1], "reason": "outside" },
            { "rect": [0.0, 0.0, 1.0], "reason": "short" },
            { "rect": [0.0, 0.0, 1.0, 1.0] },
            { "rect": [0.0, 0.0, 0.5, 0.5] },
            { "rect": [0.1, 0.0, 0.5, 0.5] },
        ]});
        let s = suggestions_of(Some(&good));
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].reason.as_str(), s[1].reason.as_str()), ("subject", ""));
        assert!(suggestions_of(None).is_empty() && suggestions_of(Some(&json!({ "suggestions": 3 }))).is_empty());
    }

    #[test]
    fn the_layout_fits_tall_and_short_panels_and_keeps_apply_and_cancel() {
        let tall = Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 480.0));
        let l = layout(tall, 32.0, 22.0, 24.0);
        assert!(l.instruction.is_some() && l.suggestions.len() == 3);
        assert_eq!(l.suggestions[0].size(), SUGGESTION);
        assert!(l.rotate.max.y <= l.reset.min.y && l.reset.max.y <= l.apply.min.y, "three rows");
        // The edit panel's content at 900 × 600 is under 200 points tall.
        for h in [260.0, 200.0, 178.0, 150.0, 120.0] {
            let short = Rect::from_min_size(pos2(8.0, 40.0), vec2(264.0, h));
            let l = layout(short, 32.0, 22.0, 24.0);
            assert!(short.contains_rect(l.apply) && short.contains_rect(l.cancel), "{h}: {l:?}");
            let parts = [l.dropdown, l.trash, l.rotate, l.preview, l.reset, l.apply, l.cancel];
            for (i, a) in parts.iter().enumerate() {
                assert!(content_has(short, *a), "{h}: {a:?}");
                for b in parts.iter().skip(i + 1) {
                    assert!(!a.intersects(*b) || a.intersect(*b).area() < 1e-3, "{h}: {a:?} {b:?}");
                }
            }
            for s in &l.suggestions {
                assert!(short.contains_rect(*s) && s.height() >= SUGGESTION_MIN_H && s.max.y <= l.rotate.min.y, "{h}: {s:?}");
            }
        }
        let r = row_rects(tall, 0.0, 24.0, 3);
        assert!(r.iter().all(|b| tall.x_range().contains(b.min.x) && b.max.x <= tall.max.x));
    }

    fn content_has(content: Rect, r: Rect) -> bool {
        r.min.x >= content.min.x - 0.5 && r.max.x <= content.max.x + 0.5 && r.min.y >= content.min.y - 0.5
    }
}
