//! Tab 2, Lighting and color tuning (`editing.md` §7): Fill Light, Highlights and Shadows with the
//! Auto lighting wand, Color Temperature, the Neutral Color Picker (swatch and eyedropper) with the
//! Auto color wand, and Reset.
//!
//! ```text
//! Fill Light        ─────○──────────  ┐
//! Highlights        ──○─────────────  ├ [wand]
//! Shadows           ○───────────────  ┘
//! Color Temperature ────────○───────
//! Neutral Color Picker
//! (●) [eyedropper]                       [wand]
//! [Reset]
//! ```
//!
//! **One "Tuning" step per tab session.** Every change goes to the engine as one change of a
//! Tuning session (`photo.history.tuning`), and every change made while the tab stays shown for
//! the same photo carries the same session key, so the engine folds them into one step of the
//! photo's history ("Undo Tuning"). Leaving the tab (or the photo) ends the session; the next
//! change starts a new one. The Basic Fixes Fill Light row ([`super::fill_light`]) edits the same
//! `fill`, each of its drags its own session.
//!
//! **The photo's Tuning after later edits** (`editing.md` §7). The sliders show the photo's last
//! `tuning` entry wherever it sits (`cmd::tuning::shown_tuning`). A session edits that entry in
//! place while only rotations and flips follow it; after anything else (a retouch, a crop, an
//! effect, an unknown entry) it appends a new entry from the defaults holding only the controls
//! the session moved ([`stored`]), so the earlier Tuning is not applied twice, and the sliders then
//! show the new entry.
//!
//! **Live while dragging.** A drag opens the Tuning tool (`edit.tool.open {tool: "tuning"}`): its
//! draft (`edit.tuning`) holds the values being dragged, and the photo shows them through the
//! preview's candidate recipe, rendered as drafts (`Session::set_preview_candidate`). Letting go
//! applies the draft as the session's change and closes the tool (`edit.tool.apply`). A click, a
//! nudge (+ or −), a double-click reset, a wand's or the picker's answer and Reset are one change at
//! once, without opening the tool. Hovering a slider shows its value (`tuning_slider`).
//!
//! **The wands and the picker** start `analysis` jobs (`photo.tuning.auto`, `photo.tuning.pick`);
//! [`poll`] applies each answer as one session change when it arrives (Auto lighting: the three
//! lighting sliders at once). The eyedropper arms the picker: the Tuning tool opens so the photo
//! area leaves its clicks to it, the pointer turns into a crosshair over the photo, and a click
//! there picks the colour under it (normalised to the photo as shown). A second click on the
//! eyedropper, or Esc, disarms it.
//!
//! **A│A's right side** (`editing.md` §4): the tab, the Fill Light row and the wands work on the
//! photo's variant: the values shown and the candidates come from the variant's recipe
//! (`tools::recipe`), each change is `photo.history.tuning {variant: true}` (a session there is one
//! step of the variant's history) and the wands and the picker read the variant's picture
//! (`variant: true`). A session belongs to its side: moving the focus starts a new one, and an
//! answer that arrives after the focus moved is dropped.
//!
//! **Scripts** drive the same paths through the tool (`edit.tool.open {tool: "tuning"}`, then
//! `edit.tool.action {name, params}`): `set {fill?, highlights?, shadows?, temperature?, neutral?,
//! preview?}` (a change; with `preview: true` only the draft and the photo shown), `auto {what:
//! "lighting" | "color"}`, `pick {x, y}`, `picker {on}`, `reset`; `edit.tool.apply` applies the
//! draft. Ids per `control-protocol.md` §7.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use dalica_core::PhotoId;
use dalica_edit::ops::tone;
use dalica_engine::cmd::tuning::{session_entry, shown_tuning, with_tuning};
use dalica_engine::{CmdError, JobId, JobState};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, SliderOut, SliderSpec, Target, ToastKind};
use egui::{Color32, CornerRadius, CursorIcon, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use super::canvas;
use crate::app::App;
use crate::commands::edit_view;
use crate::commands::tools::{self, Tool};
use crate::commands::{no_params, parse};
use crate::state::{EditState, EditTab, ToolKind, TuningDraft};

/// The four sliders' controls (`control-protocol.md` §7).
pub const FILL: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.fill_light", label: "Fill Light", min: 0.0, max: 1.0, default: 0.0, step: 0.001, decimals: 2, unit: "" };
pub const HIGHLIGHTS: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.highlights", label: "Highlights", min: 0.0, max: 0.5, default: 0.0, step: 0.001, decimals: 2, unit: "" };
pub const SHADOWS: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.shadows", label: "Shadows", min: 0.0, max: 0.5, default: 0.0, step: 0.001, decimals: 2, unit: "" };
pub const TEMPERATURE: SliderSpec<'static> =
    SliderSpec { id: "slider:tuning.temperature", label: "Color Temperature", min: -1.0, max: 1.0, default: 0.0, step: 0.001, decimals: 2, unit: "" };

pub const PICKER_ID: &str = "button:tuning.neutralPicker";
pub const PICKER_LABEL: &str = "Neutral Color Picker";
pub const PICKER_TIP: &str = "Pick a grey or white area of the photo to remove a color cast";
pub const AUTO_LIGHTING_ID: &str = "button:tuning.autoLighting";
pub const AUTO_LIGHTING_TIP: &str = "Auto lighting";
pub const AUTO_COLOR_ID: &str = "button:tuning.autoColor";
pub const AUTO_COLOR_TIP: &str = "Auto color";
pub const RESET_ID: &str = "button:tuning.reset";
pub const RESET_LABEL: &str = "Reset";
pub const RESET_TIP: &str = "Put every Tuning control back to its default";

/// The wand column's width (and the wands' edge), the gap left of it, the bracket's room.
pub const WAND: f32 = 28.0;
pub const WAND_GAP: f32 = 6.0;
pub const BRACKET: f32 = 8.0;
/// The gap above Color Temperature and above the picker's label; the picker row's height; the
/// swatch's diameter; the gap above Reset and Reset's size.
pub const GAP: f32 = 6.0;
pub const PICKER_ROW: f32 = 28.0;
pub const SWATCH: f32 = 22.0;
pub const RESET_GAP: f32 = 10.0;
pub const RESET_SIZE: Vec2 = vec2(72.0, 24.0);
/// The smallest a slider row gets in a short tab.
pub const MIN_ROW: f32 = 30.0;

/// A Tuning session's values at their defaults (`editing.md` §7).
pub const DEFAULTS: TuningDraft = TuningDraft { fill: 0.0, highlights: 0.0, shadows: 0.0, temperature: 0.0, neutral: None };

pub struct Tuning;

pub static TUNING: Tuning = Tuning;

// --- what the tab keeps between frames ---------------------------------------------------------

/// The session the tab's changes join: its photo, its key, whether it started while the Tuning tab
/// was shown, and the frame the tab last drew it (`None` until the tab draws it). Kept per thread,
/// as the UI runs on one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Visit {
    photo: PhotoId,
    /// Whether the session is on A│A's variant of the photo.
    variant: bool,
    key: u64,
    tab: bool,
    pass: Option<u64>,
}

/// A wand or picker job on its way.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    job: JobId,
    photo: PhotoId,
    variant: bool,
    what: Wand,
}

/// What a job works out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wand {
    Lighting,
    Color,
    Pick,
}

#[derive(Debug, Default)]
struct Local {
    visit: Option<Visit>,
    pending: Vec<Pending>,
    /// The photo whose preview shows a Tuning candidate this module set, and that candidate.
    candidate: Option<(PhotoId, Vec<String>)>,
    /// The photo the eyedropper is armed on.
    picking: Option<PhotoId>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

fn local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with(|l| f(&mut l.borrow_mut()))
}

/// The most wand and picker answers waited for at once (the engine runs at most 8).
const MAX_PENDING: usize = 8;

/// A session key no earlier session of this process (nor, by the clock it starts from, of an
/// earlier run) used.
fn fresh_key() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_millis());
    // At most 2^52, so a script reading it as a double keeps it exact.
    let seed = u64::try_from(millis).unwrap_or(1).min(1 << 42).saturating_mul(1024).max(1);
    let _ = NEXT.compare_exchange(0, seed, Ordering::SeqCst, Ordering::SeqCst);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

/// The key of the session a change to `photo` joins now: the tab's while it is shown (or the tool
/// is open on it), else a new one.
fn session_key(app: &App, photo: PhotoId) -> u64 {
    let tab = app.state().edit.tab == EditTab::Tuning;
    let open = app.state().edit.tool == Some(ToolKind::Tuning);
    let variant = tools::on_variant(app, photo);
    local(|l| match l.visit {
        Some(v) if v.photo == photo && v.variant == variant && (open || tab && v.tab) => v.key,
        _ => {
            let key = fresh_key();
            l.visit = Some(Visit { photo, variant, key, tab, pass: None });
            key
        }
    })
}

// --- values ------------------------------------------------------------------------------------

/// The recipe the tab works on for photo `id`: its own, or its variant's on A│A's right side
/// (empty without a session).
fn edits_of(app: &App, id: PhotoId) -> Vec<String> {
    tools::recipe(app, id)
}

/// Photo `id`'s Tuning as a draft: its last `tuning` entry (or an older `fill_light`) wherever it
/// sits in the recipe, a rotation or a retouch after it included; the defaults when it has none.
pub fn current(app: &App, id: PhotoId) -> TuningDraft {
    let last = shown_tuning(&edits_of(app, id));
    last.map_or(DEFAULTS, |t| draft_of(&t))
}

/// `v` of the range `(lo, hi)` as the draft keeps it: inside the range, 0 when not a number.
fn within(v: f64, (lo, hi): (f64, f64)) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) as f32 } else { 0.0 }
}

fn draft_of(t: &tone::Tuning) -> TuningDraft {
    TuningDraft {
        fill: within(t.fill, tone::FILL),
        highlights: within(t.highlights, tone::HIGHLIGHTS),
        shadows: within(t.shadows, tone::SHADOWS),
        temperature: within(t.temp, tone::TEMP),
        neutral: t.neutral.map(|c| c.map(|v| f32::from(v) / 255.0)),
    }
}

/// A value of the draft as the engine takes it: in `range`, to 3 decimals (0 when not a number).
fn param(v: f32, (lo, hi): (f64, f64)) -> f64 {
    let v = if v.is_finite() { f64::from(v) } else { 0.0 };
    ((v * 1000.0).round() / 1000.0).clamp(lo, hi) + 0.0
}

/// A colour of the draft as one byte per channel (a channel that is not a number is mid grey),
/// rounded in `f64` as the engine rounds an `[r, g, b]` it is given.
fn bytes(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| if v.is_finite() { (f64::from(v.clamp(0.0, 1.0)) * 255.0).round() as u8 } else { 128 })
}

fn tuning_of(d: &TuningDraft) -> tone::Tuning {
    tone::Tuning {
        fill: param(d.fill, tone::FILL),
        highlights: param(d.highlights, tone::HIGHLIGHTS),
        shadows: param(d.shadows, tone::SHADOWS),
        temp: param(d.temperature, tone::TEMP),
        neutral: d.neutral.map(bytes),
    }
}

/// The values shown for `photo`: the open Tuning tool's draft, else the photo's own.
pub fn shown(app: &App, photo: PhotoId) -> TuningDraft {
    let e = &app.state().edit;
    match (e.tool, e.tuning) {
        (Some(ToolKind::Tuning), Some(d)) if e.photo == Some(photo) => d,
        _ => current(app, photo),
    }
}

/// Whether two drafts would write the same `tuning` entry.
fn same(a: &TuningDraft, b: &TuningDraft) -> bool {
    tuning_of(a) == tuning_of(b)
}

/// `new` where it differs from `shown`, else `base`.
fn moved<T: PartialEq>(new: T, shown: T, base: T) -> T {
    if new == shown { base } else { new }
}

/// The Tuning a session's change to `d` stores in `edits`: `d` when the session edits the shown
/// entry in place; when it appends a new entry (a step that depends on the shown entry's pixels
/// follows it), only the controls `d` moved from the shown values, over the defaults.
fn stored_in(edits: &[String], d: &TuningDraft) -> tone::Tuning {
    let t = tuning_of(d);
    if session_entry(edits).is_some() {
        return t;
    }
    let shown = tuning_of(&shown_tuning(edits).map_or(DEFAULTS, |s| draft_of(&s)));
    let base = tone::Tuning::default();
    tone::Tuning {
        fill: moved(t.fill, shown.fill, base.fill),
        highlights: moved(t.highlights, shown.highlights, base.highlights),
        shadows: moved(t.shadows, shown.shadows, base.shadows),
        temp: moved(t.temp, shown.temp, base.temp),
        neutral: moved(t.neutral, shown.neutral, base.neutral),
    }
}

/// [`stored_in`] for `photo`'s recipe now.
fn stored(app: &App, photo: PhotoId, d: &TuningDraft) -> tone::Tuning {
    stored_in(&edits_of(app, photo), d)
}

/// `photo`'s recipe with the session's change to `d` made: what the change would store.
fn candidate(app: &App, photo: PhotoId, d: &TuningDraft) -> Vec<String> {
    let edits = edits_of(app, photo);
    with_tuning(&edits, &stored_in(&edits, d))
}

/// Shows `d` on `photo` through the preview's candidate (rendered as drafts, then a final), when
/// that changed: a slider held still during a drag reports its value every frame, and handing the
/// same candidate again would cancel the draft rendering it before it could arrive.
fn preview(app: &mut App, photo: PhotoId, d: &TuningDraft) {
    let recipe = candidate(app, photo, d);
    if local(|l| l.candidate.as_ref().is_some_and(|(p, r)| *p == photo && *r == recipe)) {
        return;
    }
    if let Some(s) = app.session_mut()
        && s.preview_photo() == Some(photo)
        && s.set_preview_candidate(Some(&recipe)).is_ok()
    {
        local(|l| l.candidate = Some((photo, recipe)));
    }
}

/// The photo's own recipe shown again, if this module set a candidate.
fn drop_candidate(app: &mut App) {
    if local(|l| l.candidate.take()).is_some()
        && let Some(s) = app.session_mut()
    {
        let _ = s.set_preview_candidate(None);
    }
}

/// One change of the session: `d` becomes `photo`'s Tuning (`photo.history.tuning`; [`stored`]),
/// and the open tool's draft follows. Answers the engine's answer.
fn commit(app: &mut App, photo: PhotoId, d: &TuningDraft) -> Result<Value, CmdError> {
    let t = stored(app, photo, d);
    commit_tuning(app, photo, &t)
}

/// One change of the session storing `t` as the session's entry.
fn commit_tuning(app: &mut App, photo: PhotoId, t: &tone::Tuning) -> Result<Value, CmdError> {
    let key = session_key(app, photo);
    let neutral = t.neutral.map_or(Value::Null, |c| json!(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])));
    let variant = tools::on_variant(app, photo);
    let params = json!({
        "id": photo, "session": key, "fill": t.fill, "highlights": t.highlights, "shadows": t.shadows, "temp": t.temp, "neutral": neutral,
        "variant": variant,
    });
    let out = app.run("photo.history.tuning", &params);
    drop_candidate(app);
    let now = current(app, photo);
    let e = &mut app.state_mut().edit;
    if e.tool == Some(ToolKind::Tuning) && e.photo == Some(photo) {
        e.tuning = Some(now);
    }
    out
}

/// A change made at once (a click, a nudge, a reset, an answer): applied, a refusal shown as a
/// toast.
fn change(app: &mut App, photo: PhotoId, d: &TuningDraft) {
    if let Err(e) = commit(app, photo, d) {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

// --- the wands and the picker ------------------------------------------------------------------

fn room_for_another() -> Result<(), CmdError> {
    if local(|l| l.pending.len()) >= MAX_PENDING {
        return Err(CmdError::Limit(format!("at most {MAX_PENDING} Tuning analyses at once")));
    }
    Ok(())
}

/// Starts the Auto lighting or Auto color wand on `photo`; [`poll`] applies its answer.
fn start_wand(app: &mut App, photo: PhotoId, lighting: bool) -> Result<Value, CmdError> {
    room_for_another()?;
    let (what, wand) = if lighting { ("lighting", Wand::Lighting) } else { ("color", Wand::Color) };
    let variant = tools::on_variant(app, photo);
    let out = app.run("photo.tuning.auto", &json!({ "id": photo, "what": what, "variant": variant }))?;
    track(&out, photo, variant, wand);
    Ok(out)
}

/// Starts the picker at `(x, y)` of `photo` as shown (0 … 1 each); [`poll`] applies its answer.
fn start_pick(app: &mut App, photo: PhotoId, x: f64, y: f64) -> Result<Value, CmdError> {
    room_for_another()?;
    let variant = tools::on_variant(app, photo);
    let out = app.run("photo.tuning.pick", &json!({ "id": photo, "x": x, "y": y, "variant": variant }))?;
    track(&out, photo, variant, Wand::Pick);
    Ok(out)
}

/// Remembers the job a wand or the picker started; a later ask of the same kind replaces it (the
/// engine cancels the earlier job).
fn track(out: &Value, photo: PhotoId, variant: bool, what: Wand) {
    if let Some(job) = out.get("job").and_then(Value::as_u64) {
        local(|l| {
            l.pending.retain(|p| !(p.photo == photo && p.what == what));
            l.pending.push(Pending { job, photo, variant, what });
        });
    }
}

/// Whether a job of `what` is on its way for `photo`.
fn waiting(photo: PhotoId, what: Wand) -> bool {
    local(|l| l.pending.iter().any(|p| p.photo == photo && p.what == what))
}

/// Arms or disarms the eyedropper on the photo shown. Arming opens the Tuning tool, so the photo
/// area leaves its clicks to the picker; disarming applies (and closes) it.
fn arm(app: &mut App, on: bool) -> Result<Value, CmdError> {
    let photo = edit_view::open_photo(app).ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
    if on {
        app.run("edit.tool.open", &json!({ "tool": "tuning" }))?;
        local(|l| l.picking = Some(photo));
    } else {
        local(|l| l.picking = None);
        if app.state().edit.tool == Some(ToolKind::Tuning) {
            app.run("edit.tool.apply", &json!({}))?;
        }
    }
    Ok(json!({ "picking": on }))
}

/// Whether the eyedropper is armed on `photo`.
pub fn picking(photo: PhotoId) -> bool {
    local(|l| l.picking == Some(photo))
}

/// Applies the wand and picker answers that arrived, each as one change of the session, and lets
/// go of what an outside close left (Esc or `edit.tool.cancel`: the candidate shown, the armed
/// eyedropper). Runs every frame the Basic Fixes or the Tuning tab draws; asks for another frame
/// while an answer is on its way.
pub fn poll(app: &mut App, ctx: &egui::Context) {
    let photo = edit_view::open_photo(app);
    if app.state().edit.tool != Some(ToolKind::Tuning) {
        local(|l| l.picking = None);
        drop_candidate(app);
    }
    if local(|l| l.candidate.as_ref().is_some_and(|(c, _)| Some(*c) != photo)) {
        drop_candidate(app);
    }
    let pending = local(|l| l.pending.clone());
    if pending.is_empty() {
        return;
    }
    let mut finished = Vec::new();
    for p in &pending {
        let Some(entry) = app.session().and_then(|s| s.job(p.job)) else {
            finished.push(p.job);
            continue;
        };
        if !entry.state.is_finished() {
            continue;
        }
        finished.push(p.job);
        // An answer for a photo, or a side, no longer focused is dropped.
        if Some(p.photo) != photo || photo.is_some_and(|ph| tools::on_variant(app, ph) != p.variant) {
            continue;
        }
        match (entry.state, entry.result) {
            (JobState::Done, Some(result)) => answer(app, p, &result),
            (JobState::Failed, _) => {
                let why = entry.error.unwrap_or_else(|| "Could not analyse the photo.".into());
                app.toast(&why, ToastKind::Caution);
            }
            _ => {}
        }
    }
    local(|l| l.pending.retain(|p| !finished.contains(&p.job)));
    if local(|l| !l.pending.is_empty()) {
        ctx.request_repaint();
    }
}

/// Applies a finished job's `result` to the photo's Tuning as one change: its values set on the
/// entry the session edits (the wands answer for the picture that entry receives).
fn answer(app: &mut App, p: &Pending, result: &Value) {
    let mut t = stored(app, p.photo, &shown(app, p.photo));
    let num = |k: &str| result.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    match p.what {
        Wand::Lighting => {
            t.fill = param(within(num("fill"), tone::FILL), tone::FILL);
            t.highlights = param(within(num("highlights"), tone::HIGHLIGHTS), tone::HIGHLIGHTS);
            t.shadows = param(within(num("shadows"), tone::SHADOWS), tone::SHADOWS);
        }
        Wand::Color | Wand::Pick => t.neutral = rgb(result.get("rgb")).map(bytes),
    }
    if let Err(e) = commit_tuning(app, p.photo, &t) {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// A colour `[r, g, b]` (each 0 … 1) of an answer.
fn rgb(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0) as f32);
    if a.len() != 3 {
        return None;
    }
    Some([c(0)?, c(1)?, c(2)?])
}

// --- the tool ----------------------------------------------------------------------------------

impl Tool for Tuning {
    fn kind(&self) -> ToolKind {
        ToolKind::Tuning
    }

    fn label(&self) -> &'static str {
        "Tuning"
    }

    fn modal(&self) -> bool {
        false
    }

    /// The draft starts at the photo's own Tuning; the session is the tab's while it is shown.
    fn open(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let photo = edit.photo.ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
        let _ = session_key(app, photo);
        edit.tuning = Some(current(app, photo));
        Ok(())
    }

    /// The draft as one change of the session (nothing when it changes nothing).
    fn apply(&self, app: &mut App) -> Result<Value, CmdError> {
        let e = &app.state().edit;
        let photo = e.photo.ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
        let draft = e.tuning.unwrap_or_else(|| current(app, photo));
        local(|l| l.picking = None);
        if same(&draft, &current(app, photo)) {
            drop_candidate(app);
            return Ok(json!({ "changed": false, "tuning": draft }));
        }
        let out = commit(app, photo, &draft)?;
        Ok(json!({ "changed": out.get("changed").cloned().unwrap_or(json!(false)), "tuning": current(app, photo) }))
    }

    fn action(&self, app: &mut App, name: &str, params: &Value) -> Result<Value, CmdError> {
        let photo = app.state().edit.photo.ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
        match name {
            "set" => {
                let s: Set = parse(params)?;
                let mut d = shown(app, photo);
                s.merge(&mut d)?;
                app.state_mut().edit.tuning = Some(d);
                if s.preview {
                    preview(app, photo, &d);
                    return Ok(json!({ "tuning": d, "preview": true }));
                }
                let out = commit(app, photo, &d)?;
                Ok(json!({ "tuning": shown(app, photo), "changed": out.get("changed").cloned().unwrap_or(json!(false)) }))
            }
            "auto" => {
                let a: Auto = parse(params)?;
                match a.what.as_str() {
                    "lighting" => start_wand(app, photo, true),
                    "color" => start_wand(app, photo, false),
                    _ => Err(CmdError::BadParams("what: \"lighting\" or \"color\"".into())),
                }
            }
            "pick" => {
                let p: Pick = parse(params)?;
                let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
                if !(inside(p.x) && inside(p.y)) {
                    return Err(CmdError::BadParams("x and y: numbers from 0 to 1 (the photo as shown)".into()));
                }
                local(|l| l.picking = None);
                start_pick(app, photo, p.x, p.y)
            }
            "picker" => {
                let p: Picker = parse(params)?;
                local(|l| l.picking = p.on.then_some(photo));
                Ok(json!({ "picking": p.on }))
            }
            "reset" => {
                no_params(params)?;
                app.state_mut().edit.tuning = Some(DEFAULTS);
                let out = commit(app, photo, &DEFAULTS)?;
                Ok(json!({ "tuning": shown(app, photo), "changed": out.get("changed").cloned().unwrap_or(json!(false)) }))
            }
            _ => Err(CmdError::BadParams(format!(
                "Tuning has no action `{}` (set, auto, pick, picker, reset)",
                name.chars().take(40).collect::<String>()
            ))),
        }
    }
}

/// `Some(None)` for an explicit `null`, so it is told apart from a missing field.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<[f64; 3]>>, D::Error> {
    Option::<[f64; 3]>::deserialize(d).map(Some)
}

/// `set`'s parameters.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    fill: Option<f64>,
    highlights: Option<f64>,
    shadows: Option<f64>,
    temperature: Option<f64>,
    #[serde(default, deserialize_with = "present")]
    neutral: Option<Option<[f64; 3]>>,
    #[serde(default)]
    preview: bool,
}

impl Set {
    /// The values given, checked against their ranges, written into `d`.
    fn merge(&self, d: &mut TuningDraft) -> Result<(), CmdError> {
        let check = |v: Option<f64>, name: &str, (lo, hi): (f64, f64)| match v {
            None => Ok(None),
            Some(v) if v.is_finite() && (lo..=hi).contains(&v) => Ok(Some(v as f32)),
            Some(_) => Err(CmdError::BadParams(format!("{name}: a number from {lo} to {hi}"))),
        };
        let fill = check(self.fill, "fill", tone::FILL)?;
        let highlights = check(self.highlights, "highlights", tone::HIGHLIGHTS)?;
        let shadows = check(self.shadows, "shadows", tone::SHADOWS)?;
        let temperature = check(self.temperature, "temperature", tone::TEMP)?;
        let neutral = match self.neutral {
            None => None,
            Some(None) => Some(None),
            Some(Some(c)) if c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) => Some(Some(c.map(|v| v as f32))),
            Some(Some(_)) => return Err(CmdError::BadParams("neutral: [r, g, b] (each 0 … 1) or null".into())),
        };
        d.fill = fill.unwrap_or(d.fill);
        d.highlights = highlights.unwrap_or(d.highlights);
        d.shadows = shadows.unwrap_or(d.shadows);
        d.temperature = temperature.unwrap_or(d.temperature);
        if let Some(n) = neutral {
            d.neutral = n;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Auto {
    what: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Picker {
    on: bool,
}

// --- drawing -----------------------------------------------------------------------------------

/// Where the parts of the tab are inside its rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    /// The slider rows' height and their label's (smaller when the tab is short).
    pub row: f32,
    pub label: f32,
    /// Fill Light, Highlights, Shadows, Color Temperature.
    pub sliders: [Rect; 4],
    /// The bracket joining the three lighting sliders, and the Auto lighting wand beside it.
    pub bracket: Rect,
    pub auto_lighting: Rect,
    /// The picker's row: the swatch, the eyedropper, "Neutral Color Picker", the Auto color wand.
    pub swatch: Rect,
    pub eyedropper: Rect,
    pub picker_label: Rect,
    pub auto_color: Rect,
    /// Reset, when the tab has room for it.
    pub reset: Option<Rect>,
}

/// The height the tab needs with full rows `row` high, and Reset.
pub fn full_height(row: f32) -> f32 {
    4.0 * row + 2.0 * GAP + PICKER_ROW + RESET_GAP + RESET_SIZE.y
}

/// The parts of the tab inside `rect`, with slider rows `row` high whose top `label` is the label.
/// A tab too short for them all leaves Reset out, then the gaps, then makes the rows smaller
/// (down to [`MIN_ROW`]), the label keeping its share of the row.
pub fn parts(rect: Rect, row: f32, label: f32) -> Parts {
    let h = if rect.height().is_finite() { rect.height() } else { 0.0 };
    let with_reset = h >= full_height(row);
    let gap = if h >= 4.0 * row + 2.0 * GAP + PICKER_ROW { GAP } else { 0.0 };
    let (row, label) = if gap > 0.0 {
        (row, label)
    } else {
        let fit = ((h - PICKER_ROW) / 4.0).floor().clamp(MIN_ROW, row.max(MIN_ROW));
        (fit, (label * fit / row.max(1.0)).round())
    };
    let l = rect.min.x;
    let r = if rect.max.x >= l { rect.max.x } else { l };
    let wand_x = (r - WAND).max(l);
    let slider_r = (wand_x - WAND_GAP - BRACKET).max(l);
    let row_at = |y: f32| Rect::from_min_max(pos2(l, y), pos2(slider_r, y + row));
    let mut y = rect.min.y;
    let fill = row_at(y);
    y += row;
    let highlights = row_at(y);
    y += row;
    let shadows = row_at(y);
    y += row + gap;
    let temperature = row_at(y);
    y += row + gap;
    // The middle of a slider row's track.
    let track = |r: Rect| r.min.y + label + (row - label) / 2.0;
    let bracket = Rect::from_min_max(pos2(slider_r + 2.0, track(fill)), pos2(slider_r + BRACKET, track(shadows)));
    let auto_lighting = Rect::from_center_size(pos2(wand_x + WAND / 2.0, track(highlights)), Vec2::splat(WAND));
    let swatch = Rect::from_center_size(pos2(l + 4.0 + SWATCH / 2.0, y + PICKER_ROW / 2.0), Vec2::splat(SWATCH));
    let eyedropper = Rect::from_min_size(pos2(swatch.max.x + 8.0, y), Vec2::splat(PICKER_ROW));
    let auto_color = Rect::from_center_size(pos2(wand_x + WAND / 2.0, y + PICKER_ROW / 2.0), Vec2::splat(WAND));
    let picker_label =
        Rect::from_min_max(pos2(eyedropper.max.x + 8.0, y), pos2((auto_color.min.x - 8.0).max(eyedropper.max.x + 8.0), y + PICKER_ROW));
    y += PICKER_ROW + RESET_GAP;
    let reset = with_reset.then(|| Rect::from_min_size(pos2(l, y), vec2(RESET_SIZE.x.min(r - l), RESET_SIZE.y)));
    Parts {
        row,
        label,
        sliders: [fill, highlights, shadows, temperature],
        bracket,
        auto_lighting,
        swatch,
        eyedropper,
        picker_label,
        auto_color,
        reset,
    }
}

/// Keeps the tab's session for `photo` while the tab is drawn frame after frame (`now` is the
/// frame's number); a gap (the tab left and shown again) or another photo ends it.
fn keep_visit(photo: PhotoId, now: u64) {
    local(|l| {
        if let Some(v) = &mut l.visit {
            if v.photo == photo && v.tab && v.pass.is_none_or(|p| p.saturating_add(1) >= now) {
                v.pass = Some(now);
            } else {
                l.visit = None;
            }
        }
    });
}

/// Writes a slider's new value into a draft.
pub(crate) type Setter = fn(&mut TuningDraft, f32);

/// One slider of the tab (or the Basic Fixes Fill Light) for `photo`, showing `value`; `set`
/// writes a new value into a draft. A drag opens the Tuning tool and previews its draft, and
/// applies it when let go; a one-shot change (a click, a nudge, a double-click reset) is applied
/// at once.
pub(crate) fn slider(app: &mut App, ui: &mut Ui, t: &Tokens, photo: PhotoId, spec: &SliderSpec<'_>, value: f32, set: Setter) -> SliderOut {
    let out = widgets::tuning_slider(ui, t, spec, f64::from(value));
    let one_shot = out.drag_started && out.drag_stopped;
    let open = |app: &App| app.state().edit.tool == Some(ToolKind::Tuning);
    if out.drag_started
        && !one_shot
        && !open(app)
        && let Err(e) = app.run("edit.tool.open", &json!({ "tool": "tuning" }))
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
    if let Some(v) = out.value {
        let mut d = shown(app, photo);
        set(&mut d, v as f32);
        if one_shot {
            change(app, photo, &d);
        } else if open(app) {
            app.state_mut().edit.tuning = Some(d);
            preview(app, photo, &d);
        }
    }
    if out.drag_stopped
        && !one_shot
        && open(app)
        && let Err(e) = app.run("edit.tool.apply", &json!({}))
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
    out
}

/// Draws the Tuning tab's content.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    poll(app, &ctx);
    let Some(photo) = edit_view::open_photo(app) else { return };
    keep_visit(photo, ctx.cumulative_pass_nr());
    let t = *app.tokens();
    let p = parts(ui.max_rect(), t.metrics.slider_row_h, t.metrics.slider_label_h);
    let values = shown(app, photo);
    // The sliders at the rows' height (smaller in a short tab).
    let mut rows_t = t;
    rows_t.metrics.slider_row_h = p.row;
    rows_t.metrics.slider_label_h = p.label;

    let rows: [(SliderSpec<'static>, f32, Setter); 4] = [
        (FILL, values.fill, |d, v| d.fill = v),
        (HIGHLIGHTS, values.highlights, |d, v| d.highlights = v),
        (SHADOWS, values.shadows, |d, v| d.shadows = v),
        (TEMPERATURE, values.temperature, |d, v| d.temperature = v),
    ];
    for ((spec, value, set), rect) in rows.into_iter().zip(p.sliders) {
        if rect.is_positive() {
            let mut row = ui.new_child(UiBuilder::new().id_salt(spec.id).max_rect(rect));
            slider(app, &mut row, &rows_t, photo, &spec, value, set);
        }
    }

    // The bracket joining the lighting sliders, the wand beside it.
    let line = Stroke::new(1.0, if ui.is_enabled() { t.divider } else { t.text_disabled });
    let b = p.bracket;
    let painter = ui.painter();
    painter.line_segment([b.left_top(), b.right_top()], line);
    painter.line_segment([b.right_top(), b.right_bottom()], line);
    painter.line_segment([b.left_bottom(), b.right_bottom()], line);
    painter.line_segment([pos2(b.max.x, p.auto_lighting.center().y), pos2(p.auto_lighting.min.x, p.auto_lighting.center().y)], line);
    if glyph(ui, &t, AUTO_LIGHTING_ID, EditIcon::MagicWand, p.auto_lighting, waiting(photo, Wand::Lighting), AUTO_LIGHTING_TIP).clicked()
        && let Err(e) = start_wand(app, photo, true)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }

    // The Neutral Color Picker: the swatch, the eyedropper, its label, the wand.
    let text = if ui.is_enabled() { t.text_label } else { t.text_disabled };
    let mut job = egui::text::LayoutJob::simple_singleline(PICKER_LABEL.to_owned(), fonts::regular(t.detail), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(p.picker_label.width().max(1.0));
    let galley = ui.painter().layout_job(job);
    let at = pos2(p.picker_label.min.x, p.picker_label.center().y - galley.size().y / 2.0);
    ui.painter().galley(at.round(), galley, text);
    swatch(ui, &t, p.swatch, values.neutral);
    let armed = picking(photo);
    if glyph(ui, &t, PICKER_ID, EditIcon::Eyedropper, p.eyedropper, armed || waiting(photo, Wand::Pick), PICKER_TIP).clicked()
        && let Err(e) = arm(app, !armed)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
    if glyph(ui, &t, AUTO_COLOR_ID, EditIcon::MagicWand, p.auto_color, waiting(photo, Wand::Color), AUTO_COLOR_TIP).clicked()
        && let Err(e) = start_wand(app, photo, false)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }

    // Reset: every control back to its default, as one change; drawn only where it fits.
    if let Some(reset) = p.reset.filter(Rect::is_positive) {
        let mut child = ui.new_child(UiBuilder::new().id_salt(RESET_ID).max_rect(reset));
        if same(&values, &DEFAULTS) {
            child.disable();
        }
        let r = widgets::text_button(&mut child, &t, RESET_ID, RESET_LABEL, false);
        if widgets::tooltip(r, &t, RESET_TIP, None).clicked() {
            change(app, photo, &DEFAULTS);
        }
    }

    pick_on_photo(app, &ctx, photo);
}

/// While the eyedropper is armed: a crosshair over the photo, and a click there picks the colour
/// under it (the photo as the canvas laid it out last frame).
fn pick_on_photo(app: &mut App, ctx: &egui::Context, photo: PhotoId) {
    if !picking(photo) {
        return;
    }
    let Some(v) = canvas::view(ctx).filter(|v| v.photo == photo) else { return };
    let visible = v.image.intersect(v.area);
    let (hover, clicked, at) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked(), i.pointer.interact_pos()));
    if hover.is_some_and(|h| visible.contains(h)) {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
    }
    let Some(at) = at.filter(|a| clicked && visible.contains(*a)) else { return };
    let (w, h) = (v.image.width(), v.image.height());
    if !(w > 0.0 && h > 0.0) {
        return;
    }
    let x = f64::from(((at.x - v.image.min.x) / w).clamp(0.0, 1.0));
    let y = f64::from(((at.y - v.image.min.y) / h).clamp(0.0, 1.0));
    let started = start_pick(app, photo, x, y);
    let disarmed = arm(app, false);
    if let Err(e) = started.and(disarmed) {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// The swatch of the neutral colour picked: a circle filled with it, or an empty one crossed out
/// when none is.
fn swatch(ui: &Ui, t: &Tokens, rect: Rect, colour: Option<[f32; 3]>) {
    let p = ui.painter();
    let (c, r) = (rect.center(), rect.width().min(rect.height()) / 2.0);
    let border = Stroke::new(1.0, if ui.is_enabled() { t.field_border } else { t.text_disabled });
    match colour {
        Some(rgb) => {
            let [r8, g8, b8] = bytes(rgb);
            p.circle_filled(c, r, Color32::from_rgb(r8, g8, b8));
        }
        None => {
            p.circle_filled(c, r, t.inset);
            let d = r * std::f32::consts::FRAC_1_SQRT_2 - 2.0;
            p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], border);
        }
    }
    p.circle_stroke(c, r - 0.5, border);
}

/// A square button over `rect` showing an edit glyph (70 % of its edge) in a 1 pt `button_border`
/// frame: filled `tool_active` with a `pick` glyph while `active`, `pressed` held, `hover`
/// hovered; `text_disabled` when disabled. Registers `id` (role `iconButton`, active while
/// `active`) with `tip` as its label and tooltip.
fn glyph(ui: &mut Ui, t: &Tokens, id: &str, icon: EditIcon, rect: Rect, active: bool, tip: &str) -> Response {
    let enabled = ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if enabled && active {
        p.rect_filled(rect, radius, t.tool_active);
    } else if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, radius, t.hover);
    }
    p.rect_stroke(rect, radius, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let colour = if !enabled {
        t.text_disabled
    } else if active {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    let side = (rect.width().min(rect.height()) * 0.7).round();
    paint_edit(p, Rect::from_center_size(rect.center(), Vec2::splat(side)), icon, colour);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    widgets::register(ui.ctx(), id, Role::IconButton, tip, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, tip));
    widgets::tooltip(r, t, tip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_stack_inside_the_content_and_never_turn_inside_out() {
        let p = parts(Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 478.0)), 45.0, 18.0);
        assert_eq!((p.row, p.label), (45.0, 18.0));
        assert_eq!(p.sliders[0].min, pos2(8.0, 104.0));
        assert_eq!(p.sliders[1].min.y, 149.0);
        assert_eq!(p.sliders[3].min.y, 104.0 + 3.0 * 45.0 + GAP);
        assert!(p.sliders.iter().all(|s| s.max.x < p.auto_lighting.min.x && s.width() > 200.0));
        assert_eq!(p.auto_lighting.max.x, 272.0);
        assert_eq!(p.auto_lighting.center().y, p.sliders[1].min.y + 18.0 + 13.5);
        assert!(p.bracket.min.y < p.auto_lighting.center().y && p.auto_lighting.center().y < p.bracket.max.y);
        assert!(p.eyedropper.min.y >= p.sliders[3].max.y && p.eyedropper.min.x > p.swatch.max.x);
        assert!(p.picker_label.min.x > p.eyedropper.max.x && p.picker_label.max.x < p.auto_color.min.x);
        assert!(p.picker_label.width() >= 120.0, "\"Neutral Color Picker\" fits: {:?}", p.picker_label);
        assert_eq!(p.auto_color.max.x, 272.0);
        assert!(p.reset.unwrap().min.y > p.eyedropper.max.y);
        assert!(p.reset.unwrap().max.y <= 104.0 + full_height(45.0));
        // 900 × 600 leaves 178 points: smaller rows, no gaps, no Reset; everything inside.
        let short = Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 178.0));
        let p = parts(short, 45.0, 18.0);
        assert_eq!((p.row, p.label, p.reset), (37.0, 15.0, None));
        assert!(p.auto_color.max.y <= short.max.y && p.eyedropper.max.y <= short.max.y, "{p:?}");
        for (w, h) in [(0.0, 300.0), (10.0, 0.0), (f32::NAN, f32::NAN), (264.0, -5.0)] {
            let p = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), 45.0, 18.0);
            assert!(p.row >= MIN_ROW && p.label > 0.0, "{w}×{h}: {p:?}");
            for r in p.sliders.iter().chain([&p.bracket, &p.picker_label]) {
                assert!(r.width() >= 0.0 || r.width().is_nan(), "{w}×{h}: {r:?}");
            }
        }
    }

    #[test]
    fn drafts_and_the_recipe_agree() {
        let t = tone::Tuning { fill: 0.25, highlights: 0.125, shadows: 0.5, temp: -0.75, neutral: Some([255, 0, 128]) };
        assert_eq!(tuning_of(&draft_of(&t)), t);
        assert_eq!(format!("tuning({})", tuning_of(&DEFAULTS)), "tuning(0,0,0,0,none)");
        // Hostile drafts write a valid entry.
        let wild = TuningDraft { fill: f32::NAN, highlights: 9.0, shadows: -1.0, temperature: f32::INFINITY, neutral: Some([f32::NAN, 2.0, -1.0]) };
        let w = tuning_of(&wild);
        assert_eq!((w.fill, w.highlights, w.shadows, w.temp), (0.0, 0.5, 0.0, 0.0));
        assert_eq!(w.neutral, Some([128, 255, 0]));
        assert!(shown_tuning(&[format!("tuning({w})")]).is_some());
        assert_eq!(shown_tuning(&["fill_light(0.3)".to_string()]).map(|t| t.fill), Some(0.3));
        assert_eq!(shown_tuning(&["autofix()".to_string()]), None);
        assert!(same(&TuningDraft { fill: 0.1, ..DEFAULTS }, &TuningDraft { fill: 0.100_000_1, ..DEFAULTS }));
    }

    /// After a step that depends on the shown Tuning's pixels, a session stores only the controls
    /// it moved, over the defaults; in place it stores the draft whole.
    #[test]
    fn an_appended_entry_holds_only_the_moved_controls() {
        let text = |items: &[&str]| items.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let d = TuningDraft { fill: 0.3, highlights: 0.2, neutral: Some([0.5, 0.5, 0.5]), ..DEFAULTS };
        let rotated = text(&["tuning(0.3,0,0,0,#808080)", "rotate(90)"]);
        assert_eq!(stored_in(&rotated, &d), tone::Tuning { fill: 0.3, highlights: 0.2, shadows: 0.0, temp: 0.0, neutral: Some([128; 3]) });
        let retouched = text(&["tuning(0.3,0,0,0,#808080)", "retouch(0.2,0.2,0.6,0.6,0.05)"]);
        assert_eq!(stored_in(&retouched, &d), tone::Tuning { highlights: 0.2, ..tone::Tuning::default() });
        assert_eq!(stored_in(&retouched, &DEFAULTS), tone::Tuning { neutral: None, ..tone::Tuning::default() });
        assert_eq!(stored_in(&[], &d), tuning_of(&d));
        assert_eq!(moved(1, 1, 0), 0);
        assert_eq!(moved(2, 1, 0), 2);
    }

    #[test]
    fn answers_colours_are_read_with_care() {
        assert_eq!(rgb(Some(&json!([0.5, 1.5, -1]))), Some([0.5, 1.0, 0.0]));
        assert_eq!(rgb(Some(&json!([0.5, 1.0]))), None);
        assert_eq!(rgb(Some(&json!([0.5, 1.0, 0.5, 0.5]))), None);
        assert_eq!(rgb(Some(&json!(null))), None);
        assert_eq!(rgb(Some(&json!(["a", 1, 1]))), None);
        assert_eq!(rgb(None), None);
    }

    #[test]
    fn session_keys_are_new_each_time() {
        let (a, b) = (fresh_key(), fresh_key());
        assert!(b > a && a > 0 && b < 1 << 53);
    }

    #[test]
    fn set_checks_its_values() {
        let mut d = DEFAULTS;
        let s: Set = serde_json::from_value(json!({ "fill": 0.4, "neutral": [0.5, 0.5, 0.25] })).unwrap();
        s.merge(&mut d).unwrap();
        assert_eq!((d.fill, d.neutral), (0.4, Some([0.5, 0.5, 0.25])));
        let s: Set = serde_json::from_value(json!({ "neutral": null })).unwrap();
        s.merge(&mut d).unwrap();
        assert_eq!(d.neutral, None);
        for bad in [json!({ "fill": 1.5 }), json!({ "highlights": 0.6 }), json!({ "temperature": -2 }), json!({ "neutral": [2, 0, 0] })] {
            let s: Set = serde_json::from_value(bad.clone()).unwrap();
            assert!(matches!(s.merge(&mut d), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(serde_json::from_value::<Set>(json!({ "temp": 0.1 })).is_err(), "unknown field");
    }
}
