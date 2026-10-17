//! The Tuning tab and the Basic Fixes Fill Light (`editing.md` §6.5, §7): Tuning sessions as one
//! step of the photo's history, the auto wands and the neutral colour picker.
//!
//! - **The photo's Tuning** ([`shown_tuning`]) is its last `tuning` entry (or an older
//!   `fill_light(a)`, read as Tuning with only `fill`) wherever it sits in the recipe: what the
//!   sliders show, after a rotation or a retouch too.
//! - `photo.history.tuning {id, session, fill?, highlights?, shadows?, temp?, neutral?}` is one
//!   change of a Tuning session (a slider moved, a colour picked, a wand's values applied). The
//!   values given replace those of the `tuning` entry the session edits; the others are kept. The
//!   session edits the photo's Tuning entry in place ([`session_entry`], [`Change::ReplaceAt`])
//!   when every entry after it is a rotation or a flip, which do not depend on its pixels;
//!   otherwise (a retouch, a crop, an Auto Fix, an effect, an unknown or legacy entry after it, or
//!   no Tuning at all) it appends a new `tuning` entry, which starts from the defaults so the
//!   earlier Tuning is not applied twice. All the changes of one session (the same `session` key)
//!   are one "Tuning" step of the photo's history (the coalesce key of `architecture.md` §3), so a
//!   drag of sixty changes is undone at once. The Basic Fixes Fill Light slider is the same
//!   command with only `fill` (`editing.md` §6.5): both edit the same value. With
//!   `"variant": true` the session edits A│A's variant of the photo instead (`variant.rs`, as
//!   `photo.history.apply` does): the same rules over the variant's recipe, its steps in the
//!   variant's own history, nothing in the photo's recipe or sidecar until the right side is kept.
//! - `photo.tuning.auto {id, what: lighting | color}` and `photo.tuning.pick {id, x, y}` start
//!   `analysis` jobs: a worker renders the picture the session's `tuning` entry receives (the
//!   photo's recipe without the entry the session edits in place, cropped, straightened and turned
//!   as the edit view shows it) and answers `edit::analysis`'s values (`auto_lighting`, `auto_neutral`,
//!   `color_at`), which the UI then applies as a session change; with `"variant": true` the picture
//!   comes from A│A's variant's recipe. The owner thread only starts the worker and, in
//!   [`Session::poll`], finishes the job with what came back.
//!
//! Neutral colours are `"#rrggbb"` (as the recipe writes them); the answers carry the same colour
//! as `rgb: [r, g, b]` (each 0…1) too, as the edit view's Tuning draft holds it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use dalica_core::PhotoId;
use dalica_edit::ops::tone::{self, Tuning};
use dalica_edit::{Op as EditOp, Point, Recipe, Typed, analysis};
use dalica_image::Rgba8;
use dalica_thumbs::ladder::{self, Edits, Render};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::cmd::history::{history_of, state_json, variant_json};
use crate::error::CmdError;
use crate::history::{Change, Side};
use crate::jobs::{JobId, JobKind, Outcome};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// Slider values are kept to this many decimals (the UI's `f32` leaves no noise in the recipe).
const DECIMALS: i32 = 3;
/// The longest session key, in characters.
pub const MAX_SESSION: usize = 64;
/// The most wand and picker jobs running at once.
pub const MAX_RUNNING: usize = 8;
/// The longer side of the picture the wands and the picker work on (`edit::analysis` reduces any
/// render to this size, and the picker's square is relative to the picture).
pub const PROXY_EDGE: u32 = analysis::PROXY_EDGE;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.history.tuning",
            label: "Tuning",
            menu: "",
            keys: &[],
            params: r##"{"id":PhotoId, "session":string (1…64 characters) | integer (a new key for each Tuning tab session or Fill Light drag), "fill"?:0…1, "highlights"?:0…0.5, "shadows"?:0…0.5, "temp"?:−1…1, "neutral"?:"#rrggbb" | [r,g,b] (each 0…1) | "none" | null, "variant"?:bool (false)}: one change of a Tuning session: the values given replace those of the photo's last tuning (or fill_light) entry, in place, when only rotations and flips follow it; else a new tuning entry (from the defaults) is appended; a session's changes are one "Tuning" step; values equal to the current ones take no step; with variant, a change of the photo's open A│A variant instead (edit.variant.open), which reaches no sidecar; answers the history as photo.history.state does, with "changed" and "tuning": {fill, highlights, shadows, temp, neutral, rgb}"##,
            effect: Effect::Library,
            history: History::Photo,
            enabled: always,
            run: tuning_change,
        },
        CommandSpec {
            id: "photo.tuning.auto",
            label: "Auto Tuning",
            menu: "",
            keys: &[],
            params: r##"{"id":PhotoId, "what":"lighting"|"color", "variant"?:bool (false)}: starts an analysis job (answers {job, pending:true}) on the picture the photo's Tuning entry receives (with variant, its open A│A variant's); its result is {id, what:"lighting", fill, highlights, shadows} or {id, what:"color", neutral:"#rrggbb"|null, rgb:[r,g,b]|null}, to apply as a session change. A new ask for the same photo and wand cancels the one before"##,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: auto,
        },
        CommandSpec {
            id: "photo.tuning.pick",
            label: "Neutral Color Picker",
            menu: "",
            keys: &[],
            params: r##"{"id":PhotoId, "x":0…1, "y":0…1 (normalised to the photo as the edit view shows it: cropped, straightened, turned), "variant"?:bool (false)}: starts an analysis job (answers {job, pending:true}); its result is {id, x, y, neutral:"#rrggbb"|null, rgb:[r,g,b]|null}, the mean colour around the point before the photo's Tuning (with variant, its open A│A variant's), to apply as a session change. A new pick on the same photo cancels the one before"##,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: pick,
        },
    ]
}

// --- sessions --------------------------------------------------------------------------------

/// A session key: text or a whole number.
#[derive(Deserialize)]
#[serde(untagged)]
enum SessionKey {
    Number(u64),
    Text(String),
}

/// A neutral colour as asked: `"#rrggbb"` or `"none"`, `[r, g, b]` each 0…1, or `null` (none).
#[derive(Deserialize)]
#[serde(untagged)]
enum Neutral {
    Text(String),
    Rgb([f64; 3]),
    Null(()),
}

/// `Some(Null)` for an explicit `null`, so it is told apart from a missing field.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Neutral>, D::Error> {
    Option::<Neutral>::deserialize(d).map(|n| Some(n.unwrap_or(Neutral::Null(()))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TuningChange {
    id: PhotoId,
    session: SessionKey,
    fill: Option<f64>,
    highlights: Option<f64>,
    shadows: Option<f64>,
    temp: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    neutral: Option<Neutral>,
    #[serde(default)]
    variant: bool,
}

fn round(v: f64) -> f64 {
    let k = 10f64.powi(DECIMALS);
    // `+ 0.0` turns `-0` into `0`.
    (v * k).round() / k + 0.0
}

/// `v` checked against `range` and rounded; `None` when not given.
fn slider(v: Option<f64>, name: &str, range: (f64, f64)) -> Result<Option<f64>, CmdError> {
    match v {
        None => Ok(None),
        Some(v) if v.is_finite() && (range.0..=range.1).contains(&v) => Ok(Some(round(v))),
        Some(_) => Err(CmdError::BadParams(format!("{name}: a number from {} to {}", range.0, range.1))),
    }
}

/// The colour of `#rrggbb`.
fn hex(text: &str) -> Option<[u8; 3]> {
    let h = text.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii())?;
    let byte = |i: usize| h.get(i..i + 2).filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|s| u8::from_str_radix(s, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The neutral colour asked for; `None` clears it.
fn neutral_of(n: Neutral) -> Result<Option<[u8; 3]>, CmdError> {
    let bad = || CmdError::BadParams("neutral: \"#rrggbb\", [r, g, b] (each 0 … 1), \"none\" or null".into());
    match n {
        Neutral::Null(()) => Ok(None),
        Neutral::Text(t) if t == "none" => Ok(None),
        Neutral::Text(t) => hex(&t).map(Some).ok_or_else(bad),
        Neutral::Rgb(rgb) => {
            if !rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                return Err(bad());
            }
            Ok(Some(rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)))
        }
    }
}

fn hex_text(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A colour as `"#rrggbb"` and as `[r, g, b]` (each 0…1, to 4 decimals); both `null` for none.
fn colour_json(c: Option<[u8; 3]>) -> (Value, Value) {
    match c {
        Some(c) => (json!(hex_text(c)), json!(c.map(|v| (f64::from(v) / 255.0 * 1e4).round() / 1e4))),
        None => (Value::Null, Value::Null),
    }
}

fn tuning_json(t: &Tuning) -> Value {
    let (neutral, rgb) = colour_json(t.neutral);
    json!({ "fill": t.fill, "highlights": t.highlights, "shadows": t.shadows, "temp": t.temp, "neutral": neutral, "rgb": rgb })
}

/// The Tuning an entry holds, when it is a readable `tuning` or `fill_light` entry.
pub fn tuning_in(entry: &str) -> Option<Tuning> {
    match EditOp::parse(entry).typed()?.ok()? {
        Typed::Tuning(t) => Some(t),
        Typed::FillLight(f) => Some(f.as_tuning()),
        _ => None,
    }
}

/// Whether an entry leaves the pixels' values alone, so a Tuning before it can change without
/// changing what it did: a readable rotation or flip. Anything else (an unknown or legacy entry
/// included) may depend on them.
fn position_free(entry: &str) -> bool {
    matches!(EditOp::parse(entry).typed(), Some(Ok(Typed::Rotate(_) | Typed::FlipH(_) | Typed::FlipV(_))))
}

/// The photo's Tuning as the sliders show it: its last `tuning` (or `fill_light`) entry, wherever
/// it sits; `None` when it has none.
pub fn shown_tuning(edits: &[String]) -> Option<Tuning> {
    edits.iter().rev().find_map(|e| tuning_in(e))
}

/// The entry a new Tuning session edits in place, by index, with its Tuning: the photo's last
/// `tuning` entry when only rotations and flips follow it. `None` when the session appends a new
/// entry (starting from the defaults).
pub fn session_entry(edits: &[String]) -> Option<(usize, Tuning)> {
    for (i, e) in edits.iter().enumerate().rev() {
        if let Some(t) = tuning_in(e) {
            return Some((i, t));
        }
        if !position_free(e) {
            return None;
        }
    }
    None
}

/// `edits` as a Tuning session holding `t` leaves them: the session's entry set to `t` in place,
/// or `t` appended.
pub fn with_tuning(edits: &[String], t: &Tuning) -> Vec<String> {
    let entry = format!("tuning({t})");
    let mut out = edits.to_vec();
    match session_entry(edits).and_then(|(i, _)| out.get_mut(i)) {
        Some(slot) => *slot = entry,
        None => out.push(entry),
    }
    out
}

/// The side a `variant` flag names.
fn side_of(variant: bool) -> Side {
    if variant { Side::Variant } else { Side::Photo }
}

/// The recipe `side` of photo `id` holds: the photo's own (`NotFound` for an unknown photo), or
/// its open A│A variant's (`disabled` when none is open).
fn recipe_on(s: &Session, id: PhotoId, side: Side) -> Result<Vec<String>, CmdError> {
    match side {
        Side::Photo => Ok(history_of(s, id)?.edits),
        Side::Variant => Ok(s.history_on(id, side)?.edits),
    }
}

fn tuning_change(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let TuningChange { id, session, fill, highlights, shadows, temp, neutral, variant } = params(p)?;
    let key = match session {
        SessionKey::Number(n) => n.to_string(),
        SessionKey::Text(t) if !t.is_empty() && t.chars().count() <= MAX_SESSION => t,
        SessionKey::Text(_) => return Err(CmdError::BadParams(format!("session: 1 to {MAX_SESSION} characters, or a whole number"))),
    };
    let fill = slider(fill, "fill", tone::FILL)?;
    let highlights = slider(highlights, "highlights", tone::HIGHLIGHTS)?;
    let shadows = slider(shadows, "shadows", tone::SHADOWS)?;
    let temp = slider(temp, "temp", tone::TEMP)?;
    let neutral = neutral.map(neutral_of).transpose()?;
    let side = side_of(variant);
    let target = session_entry(&recipe_on(s, id, side)?);
    let before = target.map(|(_, t)| t).unwrap_or_default();
    let mut after = before;
    after.fill = fill.unwrap_or(after.fill);
    after.highlights = highlights.unwrap_or(after.highlights);
    after.shadows = shadows.unwrap_or(after.shadows);
    after.temp = temp.unwrap_or(after.temp);
    if let Some(n) = neutral {
        after.neutral = n;
    }
    // The same values (a new entry that would change nothing included) take no step.
    let changed = if after == before {
        false
    } else {
        let op = EditOp::parse(&format!("tuning({after})"));
        let change = match target {
            Some((i, _)) => Change::ReplaceAt(i, op),
            None => Change::Append(op),
        };
        s.step_on(id, side, change, Some(&format!("tuning:{key}")))?.is_some()
    };
    let shown = shown_tuning(&recipe_on(s, id, side)?).unwrap_or_default();
    let mut out = match side {
        Side::Photo => state_json(id, &history_of(s, id)?),
        Side::Variant => variant_json(id, &s.history_on(id, side)?),
    };
    if let Some(o) = out.as_object_mut() {
        o.insert("changed".into(), json!(changed));
        o.insert("tuning".into(), tuning_json(&shown));
    }
    Ok(out)
}

// --- the wands and the picker ----------------------------------------------------------------

/// The wand and picker jobs running (`Session::tuning_wands`).
#[derive(Debug, Default)]
pub(crate) struct Wands {
    running: Vec<Running>,
}

#[derive(Debug)]
struct Running {
    job: JobId,
    photo: PhotoId,
    task: Task,
    cancel: Arc<AtomicBool>,
    answer: Receiver<Outcome>,
}

/// What a worker works out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Task {
    Lighting,
    Color,
    Pick(Point),
}

impl Task {
    /// Two asks of the same kind on the same photo: the later replaces the earlier.
    fn same_kind(&self, other: &Task) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn running(&self) -> &'static str {
        match self {
            Task::Lighting => "Finding the lighting",
            Task::Color => "Finding a neutral colour",
            Task::Pick(_) => "Picking a colour",
        }
    }

    fn finished(&self, outcome: &Outcome) -> &'static str {
        match (outcome, self) {
            (Outcome::Done(_), Task::Lighting) => "Found the lighting",
            (Outcome::Done(_), Task::Color) => "Found a neutral colour",
            (Outcome::Done(_), Task::Pick(_)) => "Picked a colour",
            (Outcome::Cancelled(_), _) => "Tuning analysis cancelled",
            (Outcome::Failed(_), _) => "Could not analyse the photo",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum What {
    Lighting,
    Color,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Auto {
    id: PhotoId,
    what: What,
    #[serde(default)]
    variant: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    id: PhotoId,
    x: f64,
    y: f64,
    #[serde(default)]
    variant: bool,
}

/// What a worker needs: everything read from the library on the owner thread.
struct Request {
    id: PhotoId,
    path: PathBuf,
    file_size: u64,
    mtime_ms: i64,
    edits: Vec<String>,
    render: Render,
    task: Task,
}

fn auto(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Auto { id, what, variant } = params(p)?;
    let task = match what {
        What::Lighting => Task::Lighting,
        What::Color => Task::Color,
    };
    start(s, id, side_of(variant), task)
}

fn pick(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Pick { id, x, y, variant } = params(p)?;
    let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
    if !(inside(x) && inside(y)) {
        return Err(CmdError::BadParams("x and y: numbers from 0 to 1 (the photo as shown)".into()));
    }
    start(s, id, side_of(variant), Task::Pick(Point { x, y }))
}

/// Starts the worker of `task` on `side` of photo `id` as an `analysis` job.
fn start(s: &mut Session, id: PhotoId, side: Side, task: Task) -> Result<Value, CmdError> {
    let photo = s.library().photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let folder = s.library().folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))?;
    let edits = recipe_on(s, id, side)?;
    let request = Request {
        id,
        path: folder.path.join(&photo.name),
        file_size: photo.size,
        mtime_ms: photo.mtime.as_millis(),
        edits,
        render: s.thumbs.render(),
        task,
    };
    // A new ask of the same kind for the photo replaces the one before.
    for r in s.tuning_wands.running.iter().filter(|r| r.photo == id && r.task.same_kind(&task)) {
        r.cancel.store(true, Ordering::SeqCst);
    }
    if s.tuning_wands.running.iter().filter(|r| !r.cancel.load(Ordering::SeqCst)).count() >= MAX_RUNNING {
        return Err(CmdError::Limit(format!("at most {MAX_RUNNING} Tuning analyses at once")));
    }
    let now = s.clock.now();
    let (job, cancel) = s.jobs.start(JobKind::Analysis, task.running(), true, now);
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&cancel);
    let spawned = std::thread::Builder::new().name("dalica-tuning-wand".into()).spawn(move || work(&request, &flag, &tx));
    if let Err(e) = spawned {
        s.jobs.finish(job, Outcome::Failed(e.to_string()), "Could not analyse the photo", now);
        return Err(CmdError::Limit(format!("cannot start a worker: {e}")));
    }
    s.tuning_wands.running.push(Running { job, photo: id, task, cancel, answer: rx });
    Ok(json!({ "job": job, "pending": true }))
}

/// The worker: the answer, or why there is none, sent back once.
fn work(request: &Request, cancel: &AtomicBool, answer: &Sender<Outcome>) {
    let outcome = match analyse(request, cancel) {
        Ok(Some(v)) => Outcome::Done(v),
        Ok(None) => Outcome::Cancelled(json!({})),
        Err(e) => Outcome::Failed(e),
    };
    // The session may have closed meanwhile; nobody is waiting then.
    let _ = answer.send(outcome);
}

/// The recipe the session's `tuning` entry receives, as the edit view shows it: `edits` without
/// the entry the session edits in place (the rotations and flips after it kept); all of `edits`
/// when the session appends.
fn before_tuning(edits: &[String]) -> Vec<String> {
    match session_entry(edits) {
        Some((i, _)) => edits.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, e)| e.clone()).collect(),
        None => edits.to_vec(),
    }
}

/// The picture the session's `tuning` entry receives, as the edit view shows it (cropped,
/// straightened, turned), its longer side at most [`PROXY_EDGE`].
fn shown(request: &Request) -> Result<Rgba8, String> {
    let recipe = Recipe::parse(before_tuning(&request.edits)).map_err(|e| e.to_string())?;
    let source = ladder::Source { path: request.path.clone(), file_size: request.file_size, mtime_ms: request.mtime_ms };
    let limits = dalica_image::Limits::default();
    // Upright, before the recipe's rotations and flips.
    let stored = match Edits::from_recipe(&recipe, dalica_edit::ops::version) {
        Some(edits) => ladder::render_edited(&source, &request.render, &edits, PROXY_EDGE, &limits).map_err(|e| e.to_string())?,
        None => dalica_thumbs::thumbnail_with(&request.path, PROXY_EDGE, &request.render.decode_options(), &limits).map_err(|e| e.to_string())?,
    };
    recipe.orientation().apply(stored).map_err(|e| e.to_string())
}

/// The answer of `request` (`None` when cancelled on the way).
fn analyse(request: &Request, cancel: &AtomicBool) -> Result<Option<Value>, String> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let img = shown(request)?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let id = request.id;
    Ok(Some(match request.task {
        Task::Lighting => {
            let l = analysis::auto_lighting(&img);
            json!({ "id": id, "what": "lighting", "fill": l.fill, "highlights": l.highlights, "shadows": l.shadows })
        }
        Task::Color => {
            let (neutral, rgb) = colour_json(analysis::auto_neutral(&img));
            json!({ "id": id, "what": "color", "neutral": neutral, "rgb": rgb })
        }
        Task::Pick(at) => {
            let (neutral, rgb) = colour_json(analysis::color_at(&img, at));
            json!({ "id": id, "x": at.x, "y": at.y, "neutral": neutral, "rgb": rgb })
        }
    }))
}

impl Session {
    /// Finishes the wand and picker jobs whose workers answered; returns how many. Never blocks
    /// and never touches the disk.
    pub(crate) fn poll_tuning_wands(&mut self) -> usize {
        if self.tuning_wands.running.is_empty() {
            return 0;
        }
        let now = self.clock.now();
        let mut finished = 0;
        let mut still = Vec::new();
        for r in std::mem::take(&mut self.tuning_wands.running) {
            let outcome = match r.answer.try_recv() {
                Ok(o) => o,
                Err(TryRecvError::Empty) => {
                    still.push(r);
                    continue;
                }
                Err(TryRecvError::Disconnected) => Outcome::Failed("the Tuning analysis worker stopped".into()),
            };
            let label = r.task.finished(&outcome);
            self.jobs.finish(r.job, outcome, label, now);
            finished += 1;
        }
        self.tuning_wands.running = still;
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(items: &[&str]) -> Vec<String> {
        items.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn the_last_tuning_entry_is_shown_wherever_it_sits() {
        assert_eq!(shown_tuning(&text(&["crop(0,0,1,1)", "tuning(0.2,0.1,0,0.5,#808080)"])).map(|t| t.neutral), Some(Some([128; 3])));
        assert_eq!(shown_tuning(&text(&["fill_light(0.4)"])).map(|t| t.fill), Some(0.4));
        assert_eq!(shown_tuning(&text(&["tuning(0.2,0,0,0,none)", "crop(0,0,1,1)"])).map(|t| t.fill), Some(0.2));
        assert_eq!(shown_tuning(&text(&["tuning(0.2,0,0,0,none)", "tuning(0.5,0,0,0,none)", "sepia()"])).map(|t| t.fill), Some(0.5));
        assert_eq!(shown_tuning(&text(&["tuning(nonsense)"])), None);
        assert_eq!(shown_tuning(&[]), None);
    }

    #[test]
    fn a_session_edits_in_place_only_past_rotations_and_flips() {
        let at = |items: &[&str]| session_entry(&text(items)).map(|(i, t)| (i, t.fill));
        assert_eq!(at(&["crop(0,0,1,1)", "fill_light(0.4)"]), Some((1, 0.4)));
        assert_eq!(at(&["tuning(0.3,0,0,0,none)", "rotate(90)", "flip_h()", "flip_v()"]), Some((0, 0.3)));
        assert_eq!(at(&["tuning(0.3,0,0,0,none)", "rotate(90)", "tuning(0.1,0,0,0,none)", "flip_h()"]), Some((2, 0.1)));
        for after in
            ["sepia()", "crop(0,0,1,1)", "retouch(0.5,0.5,0.4,0.4,0.05)", "autofix()", "future_op(1)", "legacy:redeye(1)", "rotate(nonsense)"]
        {
            assert_eq!(at(&["tuning(0.3,0,0,0,none)", "rotate(90)", after]), None, "{after}");
        }
        assert_eq!(at(&["rotate(90)"]), None);
        assert_eq!(at(&[]), None);
        let t = Tuning { fill: 0.5, ..Tuning::default() };
        assert_eq!(with_tuning(&text(&["tuning(0.3,0,0,0,none)", "rotate(90)"]), &t), text(&["tuning(0.5,0,0,0,none)", "rotate(90)"]));
        assert_eq!(
            with_tuning(&text(&["tuning(0.3,0,0,0,none)", "sepia()"]), &t),
            text(&["tuning(0.3,0,0,0,none)", "sepia()", "tuning(0.5,0,0,0,none)"])
        );
        assert_eq!(before_tuning(&text(&["crop(0,0,1,1)", "fill_light(0.4)"])), text(&["crop(0,0,1,1)"]));
        assert_eq!(before_tuning(&text(&["crop(0,0,1,1)", "tuning(0.2,0,0,0,none)", "rotate(90)"])), text(&["crop(0,0,1,1)", "rotate(90)"]));
        assert_eq!(before_tuning(&text(&["tuning(0.2,0,0,0,none)", "sepia()"])).len(), 2);
    }

    #[test]
    fn values_are_checked_and_rounded() {
        assert_eq!(slider(Some(f64::from(0.3f32)), "fill", tone::FILL).unwrap(), Some(0.3));
        assert_eq!(slider(Some(-0.0), "temp", tone::TEMP).unwrap(), Some(0.0));
        assert_eq!(slider(None, "fill", tone::FILL).unwrap(), None);
        for bad in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
            assert!(slider(Some(bad), "fill", tone::FILL).is_err(), "{bad}");
        }
        assert_eq!(neutral_of(Neutral::Text("#A0b0C0".into())).unwrap(), Some([0xa0, 0xb0, 0xc0]));
        assert_eq!(neutral_of(Neutral::Text("none".into())).unwrap(), None);
        assert_eq!(neutral_of(Neutral::Null(())).unwrap(), None);
        assert_eq!(neutral_of(Neutral::Rgb([1.0, 0.5, 0.0])).unwrap(), Some([255, 128, 0]));
        for bad in ["", "#12345", "#1234567", "#gggggg", "808080", "#ééé"] {
            assert!(neutral_of(Neutral::Text(bad.into())).is_err(), "{bad}");
        }
        assert!(neutral_of(Neutral::Rgb([1.5, 0.0, 0.0])).is_err());
        assert!(neutral_of(Neutral::Rgb([f64::NAN, 0.0, 0.0])).is_err());
        assert_eq!(colour_json(Some([255, 0, 128])), (json!("#ff0080"), json!([1.0, 0.0, 0.502])));
    }
}
