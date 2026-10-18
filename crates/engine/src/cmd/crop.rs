//! Crop and straighten (`editing.md` §5.1, §6.1–§6.2): applying a crop or a straighten as one step
//! of the photo's own history, the Crop panel's ratios, and the suggested crops.
//!
//! - `photo.crop.apply {id, rect, ratio?}` and `photo.straighten.apply {id, degrees}` check their
//!   parameters and append `crop(…)` / `straighten(…)` to the photo's recipe as one "Crop" /
//!   "Straighten" step ([`Session::photo_step`]). Apply on a re-opened crop appends the new
//!   rectangle too: only the last crop counts (`edit::ops::crop::combine`), so the edits made
//!   after the first crop stay, and undoing the step brings the earlier crop back. A rectangle or
//!   angle equal to the one that counts already takes no step.
//! - `crop.ratios.list` (query): the locale's photo sizes, the fixed ratios, the custom ratios in
//!   the settings and the ratio last used; `crop.ratios.add {name, width, height}` and
//!   `crop.ratios.remove {name}` change the custom ones (settings, not undo steps).
//! - `photo.crop.suggest {id, ratio}` starts an `analysis` job: a worker renders a proxy of the
//!   photo as the Crop tool shows it (its recipe without the crops, straightened, turned as
//!   displayed), maps the photo's faces (all but the ignored ones) from the stored frame onto that
//!   picture ([`shown_rect`]), asks `edit::suggest` for three rectangles of the ratio (a rectangle
//!   holding every face first, `reason: "faces"`), and answers them in the stored frame
//!   (`data-model.md` §3.3). The owner thread only reads the library, starts the worker and, in
//!   [`Session::poll`], finishes the job with what came back.
//!
//! Rectangles are `[x, y, w, h]` normalised to the straightened photo in the stored frame, as the
//! edit view's Crop draft holds them (`control-protocol.md` §6.5–§6.7).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use dalica_core::{PhotoId, Rect01};
use dalica_edit::crop_geom::MAX_RATIO;
use dalica_edit::ops::crop::Crop;
use dalica_edit::ops::straighten::{MAX_DEGREES, Straighten};
use dalica_edit::suggest::{Reason, Suggestion};
use dalica_edit::{Geometry, Op as EditOp, Point, Recipe, Rect, Typed};
use dalica_image::Orientation;
use dalica_thumbs::ladder::{self, Edits, Render};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::history::{history_of, state_json};
use crate::error::CmdError;
use crate::history::Change;
use crate::jobs::{JobId, JobKind, Outcome};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;
use crate::settings::{CropRatio, MAX_CROP_RATIOS, MAX_RATIO_NAME, PhotoSizes};

/// The largest side of a custom ratio this command takes: the Crop draft's `"W:H"` text allows
/// no more (`ui::state::is_ratio`).
pub const MAX_SIDE: f64 = 10_000.0;
/// The longest ratio text (`"W:H"`, `manual`, `current`) remembered as the last one, in
/// characters: the Crop draft's limit.
pub const MAX_RATIO_TEXT: usize = 64;
/// Rectangle corners are kept to this many decimals (a millionth of the picture).
const RECT_DECIMALS: i32 = 6;
/// Straighten angles are kept to this many decimals of a degree.
const DEGREE_DECIMALS: i32 = 3;
/// The most suggestion jobs running at once.
pub const MAX_RUNNING: usize = 8;
/// The longer side of the picture the suggestions are worked out on.
pub const PROXY_EDGE: u32 = dalica_edit::analysis::PROXY_EDGE;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.crop.apply",
            label: "Crop",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "rect":[x,y,w,h] (normalised to the straightened photo in the stored frame, inside 0…1, w and h above 0), "ratio"?:"manual"|"current"|"W:H" (remembered as the last ratio)}: one "Crop" step of the photo's history; a re-opened crop appends the new rectangle (only the last counts); the same rectangle again takes no step; answers the history as photo.history.state does, with "changed""#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: always,
            run: crop_apply,
        },
        CommandSpec {
            id: "photo.straighten.apply",
            label: "Straighten",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "degrees":number (−10…10, clockwise positive)}: one "Straighten" step of the photo's history (only the last counts; the same angle again takes no step); answers the history as photo.history.state does, with "changed""#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: always,
            run: straighten_apply,
        },
        CommandSpec {
            id: "crop.ratios.list",
            label: "Crop Ratios",
            menu: "",
            keys: &[],
            params: r#"{}: the Crop panel's sizes: {photoSizes:"us"|"metric", sizes, fixed, custom: [{name, ratio:"W:H", width, height}], last: string|null}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: ratios_list,
        },
        CommandSpec {
            id: "crop.ratios.add",
            label: "Add Custom Ratio",
            menu: "",
            keys: &[],
            params: r#"{"name":string (trimmed, unique, at most 256 bytes), "width":number, "height":number (each above 0, at most 10000; width / height within 1/20…20)}: adds a custom crop ratio (at most 100); answers as crop.ratios.list"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: ratios_add,
        },
        CommandSpec {
            id: "crop.ratios.remove",
            label: "Delete Custom Ratio",
            menu: "",
            keys: &[],
            params: r#"{"name":string}: removes the custom crop ratio of that name; answers as crop.ratios.list"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: ratios_remove,
        },
        CommandSpec {
            id: "photo.crop.suggest",
            label: "Suggest Crops",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "ratio":number (width / height as displayed) | "W:H"}: starts an analysis job (answers {job, pending:true}); its result is {id, ratio, suggestions: [{rect:[x,y,w,h], reason:"faces"|"subject"|"horizon"|"centred"}] × 3}, rectangles in the stored frame, best first (every face of the photo inside the first, when it has faces). A new ask for the same photo cancels the one before"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: suggest,
        },
    ]
}

// --- crop and straighten ---------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CropApply {
    id: PhotoId,
    rect: [f64; 4],
    ratio: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StraightenApply {
    id: PhotoId,
    degrees: f64,
}

fn round(v: f64, decimals: i32) -> f64 {
    let k = 10f64.powi(decimals);
    // `+ 0.0` turns `-0` into `0`.
    (v * k).round() / k + 0.0
}

/// The crop of the Crop draft's `[x, y, w, h]`, checked: finite, inside the picture (a millionth
/// of slack), not empty.
fn crop_of(rect: [f64; 4]) -> Result<Crop, CmdError> {
    const SLACK: f64 = 1e-6;
    let [x, y, w, h] = rect;
    let inside = |v: f64| v.is_finite() && (-SLACK..=1.0 + SLACK).contains(&v);
    let ok = [x, y, w, h].into_iter().all(inside) && w > 0.0 && h > 0.0 && x + w <= 1.0 + SLACK && y + h <= 1.0 + SLACK;
    if !ok {
        return Err(CmdError::BadParams("rect: [x, y, w, h] inside the photo (0 … 1, w and h above 0)".into()));
    }
    let r = |v: f64| round(v.clamp(0.0, 1.0), RECT_DECIMALS);
    Crop::new(Rect::new(r(x), r(y), r(x + w), r(y + h))).ok_or_else(|| CmdError::BadParams("rect: too small to keep anything".into()))
}

/// Whether `ratio` is a Crop draft's ratio: `manual`, `current` or `"W:H"` with both sides above 0
/// and at most [`MAX_SIDE`], at most [`MAX_RATIO_TEXT`] characters.
fn is_ratio_text(ratio: &str) -> bool {
    if ratio == "manual" || ratio == "current" {
        return true;
    }
    ratio.chars().count() <= MAX_RATIO_TEXT && parse_ratio(ratio).is_some()
}

/// The sides of a `"W:H"` ratio text.
fn parse_ratio(ratio: &str) -> Option<(f64, f64)> {
    let side = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0 && *v <= MAX_SIDE);
    let (w, h) = ratio.split_once(':')?;
    Some((side(w)?, side(h)?))
}

/// The `"W:H"` text of a ratio.
fn ratio_text(width: f64, height: f64) -> String {
    format!("{width}:{height}")
}

/// The typed operations of a recipe Dalica can read, in recipe order (refused entries skipped, as
/// rendering skips them).
fn typed(edits: &[String]) -> impl DoubleEndedIterator<Item = Typed> + '_ {
    edits.iter().filter_map(|t| EditOp::parse(t).typed()?.ok())
}

/// The crop that counts in `edits` (the last one), the whole picture when there is none.
fn current_crop(edits: &[String]) -> Crop {
    typed(edits).rev().find_map(|t| if let Typed::Crop(c) = t { Some(c) } else { None }).unwrap_or_default()
}

/// The straighten that counts in `edits` (the last one), 0° when there is none.
fn current_straighten(edits: &[String]) -> Straighten {
    typed(edits).rev().find_map(|t| if let Typed::Straighten(s) = t { Some(s) } else { None }).unwrap_or_default()
}

/// Takes the step (none when `op` is `None`: what counts already is what was asked) and answers
/// the history with `changed`.
fn step(s: &mut Session, id: PhotoId, op: Option<EditOp>) -> Result<Value, CmdError> {
    let changed = match op {
        Some(op) => s.photo_step(id, Change::Append(op), None)?.is_some(),
        None => false,
    };
    let mut out = state_json(id, &history_of(s, id)?);
    if let Some(o) = out.as_object_mut() {
        o.insert("changed".into(), json!(changed));
    }
    Ok(out)
}

fn crop_apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let CropApply { id, rect, ratio } = params(p)?;
    history_of(s, id)?;
    let crop = crop_of(rect)?;
    if let Some(r) = &ratio
        && !is_ratio_text(r)
    {
        return Err(CmdError::BadParams(format!(
            "ratio: \"manual\", \"current\" or \"W:H\" (sides above 0, at most {MAX_SIDE}; at most {MAX_RATIO_TEXT} characters)"
        )));
    }
    let same = current_crop(&s.library().info(id).edits) == crop;
    let op = (!same).then(|| EditOp::parse(&format!("crop({crop})")));
    let out = step(s, id, op)?;
    if let Some(ratio) = ratio
        && s.settings().edit().last_crop_ratio.as_deref() != Some(ratio.as_str())
    {
        s.change_settings(|st| {
            let mut e = st.edit();
            e.last_crop_ratio = Some(ratio);
            st.set_edit(&e);
        });
    }
    Ok(out)
}

fn straighten_apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let StraightenApply { id, degrees } = params(p)?;
    history_of(s, id)?;
    if !(degrees.is_finite() && (-MAX_DEGREES..=MAX_DEGREES).contains(&degrees)) {
        return Err(CmdError::BadParams(format!("degrees: a number from -{MAX_DEGREES} to {MAX_DEGREES}")));
    }
    let wanted = Straighten::new(round(degrees, DEGREE_DECIMALS)).ok_or_else(|| CmdError::BadParams("degrees: a finite number".into()))?;
    let same = current_straighten(&s.library().info(id).edits) == wanted;
    let op = (!same).then(|| EditOp::parse(&format!("straighten({wanted})")));
    step(s, id, op)
}

// --- ratios ----------------------------------------------------------------------------------

/// The photo sizes of each locale: name, width, height (orientation-free: the Crop tool turns them
/// to the photo, and Rotate swaps them).
const US_SIZES: [(&str, f64, f64); 4] = [("4 × 6", 4.0, 6.0), ("5 × 7", 5.0, 7.0), ("8 × 10", 8.0, 10.0), ("8.5 × 11 (Letter)", 8.5, 11.0)];
const METRIC_SIZES: [(&str, f64, f64); 4] = [("10 × 15", 10.0, 15.0), ("13 × 18", 13.0, 18.0), ("20 × 25", 20.0, 25.0), ("A4", 210.0, 297.0)];
/// The fixed ratios after the photo sizes.
const FIXED: [(&str, f64, f64); 5] = [
    ("Square (1:1)", 1.0, 1.0),
    ("4:3 (Standard screen)", 4.0, 3.0),
    ("16:10 (Widescreen)", 16.0, 10.0),
    ("16:9 (HD video)", 16.0, 9.0),
    ("5:3 (Photo frame)", 5.0, 3.0),
];

fn ratio_json(name: &str, width: f64, height: f64) -> Value {
    json!({ "name": name, "ratio": ratio_text(width, height), "width": width, "height": height })
}

/// What `crop.ratios.list` answers.
fn ratios_json(s: &Session) -> Value {
    let e = s.settings().edit();
    let sizes = e.photo_sizes();
    let table: &[(&str, f64, f64)] = match sizes {
        PhotoSizes::Us => &US_SIZES,
        PhotoSizes::Metric => &METRIC_SIZES,
    };
    json!({
        "photoSizes": sizes.id(),
        "sizes": table.iter().map(|(n, w, h)| ratio_json(n, *w, *h)).collect::<Vec<_>>(),
        "fixed": FIXED.iter().map(|(n, w, h)| ratio_json(n, *w, *h)).collect::<Vec<_>>(),
        "custom": e.crop_ratios.iter().map(|r| ratio_json(&r.name, r.width, r.height)).collect::<Vec<_>>(),
        "last": e.last_crop_ratio,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRatio {
    name: String,
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveRatio {
    name: String,
}

fn ratios_list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    Ok(ratios_json(s))
}

fn ratios_add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let AddRatio { name, width, height } = params(p)?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CmdError::BadParams("name: not empty".into()));
    }
    if name.len() > MAX_RATIO_NAME {
        return Err(CmdError::Limit(format!("a ratio's name is at most {MAX_RATIO_NAME} bytes")));
    }
    let side = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_SIDE;
    if !(side(width) && side(height)) {
        return Err(CmdError::BadParams(format!("width and height: numbers above 0, at most {MAX_SIDE}")));
    }
    if !(1.0 / MAX_RATIO..=MAX_RATIO).contains(&(width / height)) {
        return Err(CmdError::BadParams(format!("width / height: from 1/{MAX_RATIO} to {MAX_RATIO}")));
    }
    let mut e = s.settings().edit();
    if e.crop_ratios.iter().any(|r| r.name == name) {
        let shown: String = name.chars().take(64).collect();
        return Err(CmdError::BadParams(format!("name: there is already a ratio named {shown:?}")));
    }
    if e.crop_ratios.len() >= MAX_CROP_RATIOS {
        return Err(CmdError::Limit(format!("at most {MAX_CROP_RATIOS} custom ratios")));
    }
    e.crop_ratios.push(CropRatio { name, width, height });
    s.change_settings(|st| st.set_edit(&e));
    Ok(ratios_json(s))
}

fn ratios_remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let RemoveRatio { name } = params(p)?;
    let name = name.trim();
    let mut e = s.settings().edit();
    let before = e.crop_ratios.len();
    e.crop_ratios.retain(|r| r.name != name);
    if e.crop_ratios.len() == before {
        let shown: String = name.chars().take(64).collect();
        return Err(CmdError::NotFound(format!("no custom ratio named {shown:?}")));
    }
    s.change_settings(|st| st.set_edit(&e));
    Ok(ratios_json(s))
}

// --- suggestions -----------------------------------------------------------------------------

/// The suggestion jobs running (`Session::crop_suggestions`).
#[derive(Debug, Default)]
pub(crate) struct Suggestions {
    running: Vec<Running>,
}

#[derive(Debug)]
struct Running {
    job: JobId,
    photo: PhotoId,
    cancel: Arc<AtomicBool>,
    answer: Receiver<Outcome>,
}

/// A ratio as asked: width / height, or `"W:H"`.
#[derive(Deserialize)]
#[serde(untagged)]
enum AskedRatio {
    Number(f64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suggest {
    id: PhotoId,
    ratio: AskedRatio,
}

/// What a worker needs: everything read from the library on the owner thread.
struct Request {
    id: PhotoId,
    path: PathBuf,
    file_size: u64,
    mtime_ms: i64,
    /// The photo's size as displayed after its EXIF orientation (0 × 0 when unknown).
    size: (u32, u32),
    edits: Vec<String>,
    /// The photo's faces the crop holds (all but the ignored ones), in the stored frame.
    faces: Vec<Rect01>,
    aspect: f64,
    render: Render,
}

/// Where the stored-frame rectangle `r` shows on the picture `geometry` draws (its crop,
/// straighten, frames and turns), normalised: the bounds of its four corners there, cut to the
/// picture. `None` when nothing of it shows (cropped away) or a corner does not map.
pub(crate) fn shown_rect(geometry: &Geometry, r: Rect01) -> Option<Rect> {
    let corners = [(r.x0(), r.y0()), (r.x1(), r.y0()), (r.x0(), r.y1()), (r.x1(), r.y1())];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in corners {
        let p = geometry.to_display(Point::new(x, y))?;
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let c = |v: f64| v.clamp(0.0, 1.0);
    let shown = Rect::new(c(x0), c(y0), c(x1), c(y1));
    let finite = [x0, y0, x1, y1].iter().all(|v| v.is_finite());
    (finite && shown.x1 > shown.x0 && shown.y1 > shown.y0).then_some(shown)
}

fn suggest(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Suggest { id, ratio } = params(p)?;
    let aspect = match ratio {
        AskedRatio::Number(v) => v,
        AskedRatio::Text(t) => parse_ratio(&t).map_or(f64::NAN, |(w, h)| w / h),
    };
    if !(aspect.is_finite() && (1.0 / MAX_RATIO..=MAX_RATIO).contains(&aspect)) {
        return Err(CmdError::BadParams(format!("ratio: width / height from 1/{MAX_RATIO} to {MAX_RATIO}, as a number or \"W:H\"")));
    }
    let photo = s.library().photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let folder = s.library().folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))?;
    let request = Request {
        id,
        path: folder.path.join(&photo.name),
        file_size: photo.size,
        mtime_ms: photo.mtime.as_millis(),
        size: (photo.width, photo.height),
        edits: s.library().info(id).edits.clone(),
        faces: s.library().faces(id).iter().filter(|f| f.counts()).map(|f| f.rect).collect(),
        aspect,
        render: s.thumbs.render(),
    };
    // A new ask for the photo replaces the one before.
    for r in s.crop_suggestions.running.iter().filter(|r| r.photo == id) {
        r.cancel.store(true, Ordering::SeqCst);
    }
    if s.crop_suggestions.running.iter().filter(|r| !r.cancel.load(Ordering::SeqCst)).count() >= MAX_RUNNING {
        return Err(CmdError::Limit(format!("at most {MAX_RUNNING} crop suggestions at once")));
    }
    let now = s.clock.now();
    let (job, cancel) = s.jobs.start(JobKind::Analysis, "Suggesting crops", true, now);
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&cancel);
    let spawned = std::thread::Builder::new().name("dalica-crop-suggest".into()).spawn(move || work(&request, &flag, &tx));
    if let Err(e) = spawned {
        s.jobs.finish(job, Outcome::Failed(e.to_string()), "Could not suggest crops", now);
        return Err(CmdError::Limit(format!("cannot start a worker: {e}")));
    }
    s.crop_suggestions.running.push(Running { job, photo: id, cancel, answer: rx });
    Ok(json!({ "job": job, "pending": true }))
}

/// The worker: the answer, or why there is none, sent back once.
fn work(request: &Request, cancel: &AtomicBool, answer: &Sender<Outcome>) {
    let outcome = match suggestions(request, cancel) {
        Ok(Some(v)) => Outcome::Done(v),
        Ok(None) => Outcome::Cancelled(json!({})),
        Err(e) => Outcome::Failed(e),
    };
    // The session may have closed meanwhile; nobody is waiting then.
    let _ = answer.send(outcome);
}

/// The three suggestions of `request` (`None` when cancelled on the way).
fn suggestions(request: &Request, cancel: &AtomicBool) -> Result<Option<Value>, String> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let recipe = Recipe::parse(&request.edits).map_err(|e| e.to_string())?;
    // What the Crop tool shows: the recipe without its crops.
    let mut uncropped = Recipe::new();
    for op in recipe.ops().iter().filter(|op| op.name() != Some("crop")) {
        uncropped.push(op.clone()).map_err(|e| e.to_string())?;
    }
    let source = ladder::Source { path: request.path.clone(), file_size: request.file_size, mtime_ms: request.mtime_ms };
    let limits = dalica_image::Limits::default();
    // Upright, before the recipe's rotations and flips: the stored frame.
    let stored = match Edits::from_recipe(&uncropped, dalica_edit::ops::version) {
        Some(edits) => ladder::render_edited(&source, &request.render, &edits, PROXY_EDGE, &limits).map_err(|e| e.to_string())?,
        None => dalica_thumbs::thumbnail_with(&request.path, PROXY_EDGE, &request.render.decode_options(), &limits).map_err(|e| e.to_string())?,
    };
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let turn = recipe.orientation();
    // The faces on the picture shown: the photo's size in the stored frame, else the proxy's.
    let upright = match request.size {
        (w, h) if w > 0 && h > 0 => (w, h),
        _ => (stored.width, stored.height),
    };
    let shown = turn.apply(stored).map_err(|e| e.to_string())?;
    let (width, height) = turn.display_size(upright.0, upright.1);
    let geometry = Geometry::new(&uncropped, Orientation::default(), upright.0, upright.1);
    let faces: Vec<Rect> = request.faces.iter().filter_map(|r| shown_rect(&geometry, *r)).collect();
    let found = dalica_edit::suggest::suggest(&shown, width, height, request.aspect, &faces).map_err(|e| e.to_string())?;
    let list: Vec<Value> = found.iter().map(|s| suggestion_json(s, turn)).collect();
    Ok(Some(json!({ "id": request.id, "ratio": request.aspect, "suggestions": list })))
}

fn suggestion_json(s: &Suggestion, turn: dalica_edit::Dihedral) -> Value {
    let r = s.rect.to_stored(turn);
    let n = |v: f64| round(v, RECT_DECIMALS);
    let reason = match s.reason {
        Reason::Faces => "faces",
        Reason::Subject => "subject",
        Reason::Horizon => "horizon",
        Reason::Centred => "centred",
    };
    json!({ "rect": [n(r.x0), n(r.y0), n(r.x1 - r.x0), n(r.y1 - r.y0)], "reason": reason })
}

impl Session {
    /// Finishes the suggestion jobs whose workers answered; returns how many. Never blocks and
    /// never touches the disk.
    pub(crate) fn poll_crop_suggestions(&mut self) -> usize {
        if self.crop_suggestions.running.is_empty() {
            return 0;
        }
        let now = self.clock.now();
        let mut finished = 0;
        let mut still = Vec::new();
        for r in std::mem::take(&mut self.crop_suggestions.running) {
            let outcome = match r.answer.try_recv() {
                Ok(o) => o,
                Err(TryRecvError::Empty) => {
                    still.push(r);
                    continue;
                }
                Err(TryRecvError::Disconnected) => Outcome::Failed("the suggestion worker stopped".into()),
            };
            let label = match &outcome {
                Outcome::Done(_) => "Suggested crops",
                Outcome::Cancelled(_) => "Crop suggestions cancelled",
                Outcome::Failed(_) => "Could not suggest crops",
            };
            self.jobs.finish(r.job, outcome, label, now);
            finished += 1;
        }
        self.crop_suggestions.running = still;
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_are_checked_rounded_and_clamped() {
        let c = crop_of([0.1, 0.2, 0.30000000149, 0.5]).unwrap();
        assert_eq!(c.to_string(), "0.1,0.2,0.4,0.7");
        assert_eq!(crop_of([0.0, 0.0, 1.0000001, 1.0]).unwrap().to_string(), "0,0,1,1");
        for bad in [
            [0.5, 0.5, 0.6, 0.1],
            [0.0, 0.0, 0.0, 1.0],
            [-0.1, 0.0, 0.5, 0.5],
            [0.0, 0.0, 1.0, -1.0],
            [f64::NAN, 0.0, 1.0, 1.0],
            [0.0, 0.0, 1e-9, 1.0],
        ] {
            assert!(matches!(crop_of(bad), Err(CmdError::BadParams(_))), "{bad:?}");
        }
    }

    #[test]
    fn ratio_texts() {
        assert_eq!(parse_ratio("8.5:11"), Some((8.5, 11.0)));
        assert_eq!(ratio_text(8.5, 11.0), "8.5:11");
        assert_eq!(ratio_text(210.0, 297.0), "210:297");
        for good in ["manual", "current", "4:6", "1:1", "10000:1"] {
            assert!(is_ratio_text(good), "{good}");
        }
        let long = format!("1:{}", "1".repeat(70));
        for bad in ["", "4", "4:", ":6", "0:1", "-1:1", "inf:1", "NaN:1", "10001:1", "4:6:8", "Manual", " 4:6", long.as_str()] {
            assert!(!is_ratio_text(bad), "{bad}");
        }
    }

    #[test]
    fn the_operation_that_counts_is_the_last_readable_one() {
        let edits: Vec<String> =
            ["crop(0,0,0.5,0.5)", "straighten(2)", "crop(nonsense)", "tuning(0.2,0,0,0,none)", "straighten(x)"].map(String::from).to_vec();
        assert_eq!(current_crop(&edits).to_string(), "0,0,0.5,0.5");
        assert_eq!(current_straighten(&edits).degrees, 2.0);
        assert_eq!(current_crop(&[]), Crop::default());
        assert_eq!(current_straighten(&[]).degrees, 0.0);
    }
}
