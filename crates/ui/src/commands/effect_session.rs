//! The effect panels' session (`editing.md` §8.1, M12): the [`Tool`] behind
//! `edit.tool.open {tool: "effect", name}`, so `edit.tool.action`, `.apply`, `.cancel`, `.reset`
//! and Esc reach an open parameter panel as they reach Crop or Retouch ([`super::tools`]). Its
//! draft is [`crate::state::EditState::effect`].
//!
//! - **Open** ([`Tool::open_named`]) starts the panel of any parameter effect the catalogue lists
//!   (a one-click effect has no panel: it is `edit.effect.apply`, [`super::effects`]) with every
//!   value at its catalogue default; **Reset** goes back to them.
//! - **The live preview.** Every change (a control of the panel, an action, a colour picked)
//!   makes the edit view's candidate recipe the recipe of the focused side plus the effect's entry
//!   ([`entry`]), rendered as drafts then a final off the UI thread
//!   (`Session::set_preview_candidate`; [`sync_preview`] hands it only when it changed). Nothing
//!   reaches the recipe before Apply. An effect whose renderer is still a stub previews as the
//!   photo (its entry is kept and skipped).
//! - **Apply** is one step of the photo's history named after the effect, the entry appended
//!   (`photo.history.apply`; on A│A's right side the variant's). **Cancel** and **Esc** drop the
//!   candidate and record nothing; Esc first closes an open picker popup or disarms the
//!   eyedropper.
//! - **The eyedropper** ([`start_pick`]) asks `photo.effect.pick` for the colour under a point of
//!   the photo (stored frame; the engine answers off the UI thread through an `analysis` job);
//!   [`poll`] sets the colour parameter when the answer arrives.
//! - **Scripts:** `edit.tool.action {name: "set", params: {<param>: value, …}}` sets values (all or
//!   nothing: a value its catalogue row refuses changes none), `{name: "pick", params: {x, y,
//!   param?}}` picks a colour from the photo (the armed eyedropper's parameter, else the effect's
//!   first colour), `{name: "reset"}` the defaults again. Each answers the panel's values.
//!
//! The crosshair ([`crate::panels::edit::crosshair`], M12.17) and Soften's brush
//! ([`crate::panels::edit::soften_brush`], M12.18) change the draft the same way and call
//! [`sync_preview`]; [`entry`] already writes Soften's strokes.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dalica_core::PhotoId;
use dalica_edit::catalog::{Kind, OpInfo};
use dalica_edit::{Geometry, Point, Recipe};
use dalica_engine::{CmdError, JobId, JobState, Key};
use dalica_image::Orientation;
use dalica_ui_kit::widgets::ToastKind;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::tools::{self, Tool};
use super::{edit_view, effects, parse};
use crate::app::App;
use crate::state::{EditState, EffectDraft, EffectValue, ToolKind, effect_row};

/// The effect tool.
pub struct Effect;

pub static EFFECT: Effect = Effect;

/// A colour pick on its way: the job, and the photo, effect and parameter it answers for.
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    job: JobId,
    photo: PhotoId,
    effect: String,
    param: String,
}

/// What the session keeps on the UI thread outside the edit state: the candidate last handed to
/// the preview and the pick waited for.
#[derive(Debug, Default)]
struct Local {
    sent: Option<(PhotoId, Vec<String>)>,
    pending: Option<Pending>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

fn local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with(|l| f(&mut l.borrow_mut()))
}

/// The parameter values of `row` at their catalogue defaults; a parameter whose default does not
/// read is left out and reads as its default when it does.
pub fn defaults(row: &OpInfo) -> BTreeMap<String, EffectValue> {
    row.params.iter().filter_map(|p| Some((p.name.to_string(), default_of(p)?))).collect()
}

/// The catalogue default of `p` as a panel value.
fn default_of(p: &dalica_edit::catalog::Param) -> Option<EffectValue> {
    if let Kind::Flag = p.kind {
        Some(EffectValue::Flag(matches!(p.default, "1" | "true" | "on")))
    } else if let Kind::Number { .. } | Kind::Open = p.kind {
        Some(EffectValue::Number(p.default.parse::<f64>().ok().filter(|v| v.is_finite())?))
    } else if let Kind::Colour { .. } | Kind::Choice(_) = p.kind {
        (!p.default.is_empty()).then(|| EffectValue::Text(p.default.into()))
    } else {
        None
    }
}

/// A fresh panel on effect `name`: refused (`bad_params`) when `name` is no effect or a one-click
/// one.
pub fn draft(name: &str) -> Result<EffectDraft, CmdError> {
    let shown: String = name.chars().take(40).collect();
    let row = effect_row(name).ok_or_else(|| CmdError::BadParams(format!("`name` is an effect of tabs 3–5 (\"sharpen\"), not `{shown}`")))?;
    if effects::one_click(row) {
        return Err(CmdError::BadParams(format!("{} has no panel: it applies with one click (edit.effect.apply)", row.label)));
    }
    let mut d = EffectDraft::new(name);
    d.params = defaults(row);
    Ok(d)
}

/// The value of parameter `name` in `d`: the one set, else its catalogue default.
pub fn value(d: &EffectDraft, name: &str) -> Option<EffectValue> {
    let row = effect_row(&d.name)?;
    let p = row.params.iter().find(|p| p.name == name)?;
    d.params.get(name).cloned().or_else(|| default_of(p))
}

/// A number as the entry writes it: to six decimals (finer than any control sets), the shortest
/// text that reads back, never "-0".
fn number_text(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    let r = if r.is_finite() { r } else { v };
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

/// The recipe entry of `d` (`tint(#80cfff,0)`): its values in the catalogue's order, a missing
/// one at its default, colours in lower case; Soften's strokes after a `;` each (`paint` or
/// `erase`, the radius, the points). `None` when the effect is not one or a value cannot be
/// written (a parameter with no default left unset).
pub fn entry(d: &EffectDraft) -> Option<String> {
    let row = effect_row(&d.name)?;
    let mut fields = Vec::with_capacity(row.params.len());
    let mut strokes = String::new();
    for p in row.params {
        if let Kind::Strokes { fields: brush, .. } = p.kind {
            let (lo, hi) = brush
                .iter()
                .find(|f| f.name == "r")
                .and_then(|f| if let Kind::Number { min, max } = f.kind { Some((min, max)) } else { None })
                .unwrap_or((0.0, 1.0));
            for s in d.strokes.iter().filter(|s| !s.points.is_empty()) {
                let radius = f64::from(s.radius);
                let radius = if radius.is_finite() { radius.clamp(lo, hi) } else { lo };
                strokes.push(';');
                strokes.push_str(if s.erase { "erase" } else { "paint" });
                strokes.push(',');
                strokes.push_str(&number_text(radius));
                for pt in &s.points {
                    for v in pt {
                        strokes.push(',');
                        strokes.push_str(&number_text(f64::from(*v)));
                    }
                }
            }
            continue;
        }
        let text = match value(d, p.name)? {
            EffectValue::Flag(b) => b.to_string(),
            EffectValue::Number(n) => number_text(n),
            EffectValue::Text(t) => t.to_ascii_lowercase(),
            EffectValue::Point([x, y]) => format!("{},{}", number_text(x), number_text(y)),
        };
        fields.push(text);
    }
    Some(format!("{}({}{strokes})", row.name, fields.join(",")))
}

/// The photo the panel works on and its draft, while the effect tool is open in the edit view.
fn open_draft(app: &App) -> Option<(PhotoId, EffectDraft)> {
    let photo = edit_view::open_photo(app)?;
    let e = &app.state().edit;
    match (e.tool, &e.effect) {
        (Some(ToolKind::Effect), Some(d)) => Some((photo, d.clone())),
        _ => None,
    }
}

/// [`open_draft`], or `disabled`.
fn the_draft(app: &App) -> Result<(PhotoId, EffectDraft), CmdError> {
    open_draft(app).ok_or_else(|| CmdError::Disabled("No effect is open.".into()))
}

/// The candidate recipe of the open panel: the focused side's recipe plus the entry.
pub fn candidate(app: &App) -> Option<(PhotoId, Vec<String>)> {
    let (photo, d) = open_draft(app)?;
    let mut recipe = tools::recipe(app, photo);
    recipe.push(entry(&d)?);
    Some((photo, recipe))
}

/// Hands the preview the open panel's candidate when it changed since it was last handed (a value
/// held still would otherwise cancel the draft rendering it every frame), and the photo's own
/// recipe back once the panel is gone. Never waits: the preview renders on its own workers.
pub fn sync_preview(app: &mut App) {
    let want = candidate(app);
    if local(|l| l.sent == want) {
        return;
    }
    let Some(s) = app.session_mut() else { return };
    match want {
        Some((photo, recipe)) => {
            // The canvas opens the photo's preview; a later frame hands the candidate then.
            if s.preview_photo() == Some(photo) && s.set_preview_candidate(Some(&recipe)).is_ok() {
                local(|l| l.sent = Some((photo, recipe)));
            }
        }
        None => {
            if s.preview_photo().is_some() {
                let _ = s.set_preview_candidate(None);
            }
            local(|l| l.sent = None);
        }
    }
}

/// Drops the candidate and the pick waited for: the preview shows the recipe again.
fn release(app: &mut App) {
    local(|l| l.pending = None);
    if local(|l| l.sent.take()).is_some()
        && let Some(s) = app.session_mut()
        && s.preview_photo().is_some()
    {
        let _ = s.set_preview_candidate(None);
    }
}

/// The panel's values, as the actions answer them.
fn summary(d: &EffectDraft) -> Value {
    let params: Map<String, Value> = effect_row(&d.name)
        .map(|row| row.params.iter().filter_map(|p| Some((p.name.to_string(), serde_json::to_value(value(d, p.name)?).ok()?))).collect())
        .unwrap_or_default();
    json!({ "name": d.name, "params": params, "entry": entry(d), "picker": d.picker, "eyedropper": d.eyedropper })
}

/// Sets `values` (parameter → value) on the open panel, all or none: refused (`bad_params`) when
/// one is not a value its catalogue row takes. The preview follows.
pub fn set_values(app: &mut App, values: &Map<String, Value>) -> Result<Value, CmdError> {
    let (_, mut d) = the_draft(app)?;
    for (name, v) in values {
        let shown: String = name.chars().take(40).collect();
        let value: EffectValue =
            serde_json::from_value(v.clone()).map_err(|_| CmdError::BadParams(format!("`{shown}` is a number, a checkbox, a colour or a choice")))?;
        let value = match value {
            EffectValue::Text(t) => EffectValue::Text(t.to_ascii_lowercase()),
            other => other,
        };
        d.params.insert(name.clone(), value);
    }
    d.check().map_err(CmdError::BadParams)?;
    let out = summary(&d);
    app.state_mut().edit.effect = Some(d);
    sync_preview(app);
    Ok(out)
}

/// Opens or closes the picker popup of colour parameter `param` (`None`: closes any); closing it
/// disarms its eyedropper.
pub fn set_picker(app: &mut App, param: Option<&str>) {
    if let Some(d) = app.state_mut().edit.effect.as_mut() {
        if param.is_none() && d.eyedropper == d.picker {
            d.eyedropper = None;
        }
        d.picker = param.map(str::to_owned);
    }
}

/// Arms the eyedropper for colour parameter `param` (`None`: disarms it).
pub fn set_eyedropper(app: &mut App, param: Option<&str>) {
    if let Some(d) = app.state_mut().edit.effect.as_mut() {
        d.eyedropper = param.map(str::to_owned);
    }
}

/// The colour parameter a pick sets: `param` when it is one of the effect's colours, else the armed
/// eyedropper's, else the effect's first colour.
fn colour_param(d: &EffectDraft, param: Option<&str>) -> Result<String, CmdError> {
    let row = effect_row(&d.name).ok_or_else(|| CmdError::Disabled("No effect is open.".into()))?;
    let colours: Vec<&str> = row.params.iter().filter(|p| matches!(p.kind, Kind::Colour { .. })).map(|p| p.name).collect();
    let chosen = match param {
        Some(p) => colours.iter().find(|c| **c == p).copied(),
        None => d.eyedropper.as_deref().filter(|e| colours.contains(e)).or_else(|| colours.first().copied()),
    };
    chosen.map(str::to_owned).ok_or_else(|| match param {
        Some(p) => CmdError::BadParams(format!("{} has no colour `{}`", row.label, p.chars().take(40).collect::<String>())),
        None => CmdError::BadParams(format!("{} has no colour to pick", row.label)),
    })
}

/// Starts a pick of the colour under `(x, y)` (normalised to the photo in the stored frame) for
/// colour parameter `param` (see [`colour_param`]); [`poll`] sets it when the answer arrives. The
/// eyedropper disarms. A later pick replaces one still on its way.
pub fn start_pick(app: &mut App, param: Option<&str>, x: f64, y: f64) -> Result<Value, CmdError> {
    let (photo, d) = the_draft(app)?;
    let param = colour_param(&d, param)?;
    let out = app.run("photo.effect.pick", &json!({ "id": photo, "x": x, "y": y }))?;
    let job = out.get("job").and_then(Value::as_u64).ok_or_else(|| CmdError::Internal("the pick started no job".into()))?;
    local(|l| l.pending = Some(Pending { job, photo, effect: d.name.clone(), param: param.clone() }));
    set_eyedropper(app, None);
    Ok(json!({ "job": job, "pending": true, "param": param }))
}

/// Whether a colour pick is on its way.
pub fn picking() -> bool {
    local(|l| l.pending.is_some())
}

/// Sets the colour of a finished pick on the panel it was asked for (an answer for a panel since
/// closed, another effect or another photo is dropped), and keeps the preview in step. Runs every
/// frame the panel is drawn; asks for another frame while a pick is on its way.
pub fn poll(app: &mut App, ctx: &egui::Context) {
    let Some(p) = local(|l| l.pending.clone()) else { return };
    let Some(entry) = app.session().and_then(|s| s.job(p.job)) else {
        local(|l| l.pending = None);
        return;
    };
    if !entry.state.is_finished() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return;
    }
    local(|l| l.pending = None);
    let still = open_draft(app).is_some_and(|(photo, d)| photo == p.photo && d.name == p.effect);
    if !still {
        return;
    }
    match (entry.state, entry.result) {
        (JobState::Done, Some(result)) => match result.get("color").and_then(Value::as_str) {
            Some(c) => {
                let mut values = Map::new();
                values.insert(p.param.clone(), json!(c));
                if let Err(e) = set_values(app, &values) {
                    app.toast(&e.to_string(), ToastKind::Caution);
                }
            }
            None => app.toast("Nothing of the photo shows there.", ToastKind::Caution),
        },
        (JobState::Failed, _) => app.toast(&entry.error.unwrap_or_else(|| "Could not pick a colour.".into()), ToastKind::Caution),
        _ => {}
    }
}

/// The stored-frame point (normalised) under the displayed point `at` (normalised to the photo as
/// the edit view shows it now: the candidate's picture while the panel previews), brought inside
/// the photo.
pub fn stored_point(app: &App, photo: PhotoId, at: [f32; 2]) -> Option<[f64; 2]> {
    let s = app.session()?;
    let p = s.library().photo(photo)?;
    if p.width == 0 || p.height == 0 {
        return None;
    }
    let shown = local(|l| l.sent.clone()).filter(|(ph, _)| *ph == photo).map_or_else(|| tools::recipe(app, photo), |(_, r)| r);
    let recipe = Recipe::parse(shown).or_else(|_| Recipe::parse(tools::recipe(app, photo))).ok()?;
    let g = Geometry::new(&recipe, Orientation::default(), p.width, p.height);
    let q = g.to_stored(Point::new(f64::from(at[0]), f64::from(at[1])))?;
    let c = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 };
    Some([c(q.x), c(q.y)])
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PickParams {
    x: f64,
    y: f64,
    #[serde(default)]
    param: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

impl Tool for Effect {
    fn kind(&self) -> ToolKind {
        ToolKind::Effect
    }

    fn label(&self) -> &'static str {
        "the effect"
    }

    fn open(&self, _app: &App, _edit: &mut EditState) -> Result<(), CmdError> {
        Err(CmdError::BadParams("`name` names the effect to open (\"sharpen\")".into()))
    }

    fn open_named(&self, app: &App, edit: &mut EditState, name: Option<&str>) -> Result<(), CmdError> {
        let Some(name) = name else { return self.open(app, edit) };
        edit.effect = Some(draft(name)?);
        // Whatever an earlier panel handed, the preview is handed this one's on the next change.
        local(|l| *l = Local::default());
        Ok(())
    }

    /// The entry as one step of the photo's history ("Undo Tint"); the preview shows the recipe.
    fn apply(&self, app: &mut App) -> Result<Value, CmdError> {
        let (photo, d) = the_draft(app)?;
        let op = entry(&d).ok_or_else(|| CmdError::Internal(format!("{} has no entry", d.name)))?;
        let variant = tools::on_variant(app, photo);
        let history = app.run("photo.history.apply", &json!({ "id": photo, "op": op, "variant": variant }))?;
        release(app);
        Ok(json!({ "applied": true, "op": op, "history": history }))
    }

    fn reset(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let name = edit.effect.as_ref().map(|e| e.name.clone()).ok_or_else(|| CmdError::Disabled("No effect is open.".into()))?;
        edit.effect = Some(draft(&name)?);
        Ok(())
    }

    fn action(&self, app: &mut App, name: &str, params: &Value) -> Result<Value, CmdError> {
        match name {
            "set" => {
                let values = params.as_object().ok_or_else(|| CmdError::BadParams("`params` maps parameters to values".into()))?;
                set_values(app, values)
            }
            "pick" => {
                let p: PickParams = parse(params)?;
                let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
                if !(inside(p.x) && inside(p.y)) {
                    return Err(CmdError::BadParams("`x` and `y` are 0 … 1 (the photo in the stored frame)".into()));
                }
                start_pick(app, p.param.as_deref(), p.x, p.y)
            }
            "reset" => {
                let _: Nothing = parse(params)?;
                let (_, d) = the_draft(app)?;
                let fresh = draft(&d.name)?;
                let out = summary(&fresh);
                app.state_mut().edit.effect = Some(fresh);
                sync_preview(app);
                Ok(out)
            }
            _ => {
                Err(CmdError::BadParams(format!("the effect panel has no action `{}` (set, pick, reset)", name.chars().take(40).collect::<String>())))
            }
        }
    }

    /// Esc closes an open picker popup, else disarms the eyedropper; with neither it drops the
    /// candidate and leaves the key to cancel the panel.
    fn key(&self, app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
        if key.code != "Esc" || key.ctrl || key.alt {
            return None;
        }
        let (picker, eyedropper) = app.state().edit.effect.as_ref().map_or((false, false), |d| (d.picker.is_some(), d.eyedropper.is_some()));
        if picker {
            set_picker(app, None);
            return Some(Ok(json!({ "picker": null })));
        }
        if eyedropper {
            set_eyedropper(app, None);
            return Some(Ok(json!({ "eyedropper": null })));
        }
        release(app);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SoftenStroke;

    #[test]
    fn a_panel_starts_at_the_catalogue_defaults_and_one_click_effects_have_none() {
        let d = draft("sharpen").unwrap();
        assert_eq!(d.params.get("a"), Some(&EffectValue::Number(0.6)));
        assert_eq!(d.check(), Ok(()));
        let grain = draft("grain").unwrap();
        assert_eq!(grain.params.get("lighten"), Some(&EffectValue::Flag(false)));
        assert!(matches!(draft("sepia"), Err(CmdError::BadParams(m)) if m.contains("one click")));
        for bad in ["", "crop", "autofix", "nope", &"x".repeat(5000)] {
            assert!(matches!(draft(bad), Err(CmdError::BadParams(_))), "{bad:.20}");
        }
    }

    #[test]
    fn the_entry_at_the_defaults_is_the_catalogues_and_follows_the_values() {
        for (op, _) in dalica_edit::catalog::TABS.iter().flat_map(|t| dalica_edit::catalog::effects(*t)) {
            if effects::one_click(op) {
                continue;
            }
            assert_eq!(entry(&draft(op.name).unwrap()), op.default_entry(), "{}", op.name);
            // Nothing set at all reads the defaults too.
            assert_eq!(entry(&EffectDraft::new(op.name)), op.default_entry(), "{}", op.name);
        }
        let mut d = draft("tint").unwrap();
        d.params.insert("color".into(), EffectValue::Text("#FF8800".into()));
        d.params.insert("fade".into(), EffectValue::Number(0.1 + 0.2));
        assert_eq!(entry(&d).as_deref(), Some("tint(#ff8800,0.3)"));
        d.params.insert("fade".into(), EffectValue::Number(-0.0));
        assert_eq!(entry(&d).as_deref(), Some("tint(#ff8800,0)"));
        let mut s = draft("soften").unwrap();
        s.strokes = vec![
            SoftenStroke { points: vec![[0.25, 0.5], [0.75, 0.5]], radius: 0.05, erase: false },
            SoftenStroke { points: vec![], radius: 0.05, erase: true },
            SoftenStroke { points: vec![[0.5, 0.5]], radius: 0.5, erase: true },
        ];
        assert_eq!(entry(&s).as_deref(), Some("soften(50,50;paint,0.05,0.25,0.5,0.75,0.5;erase,0.25,0.5,0.5)"));
        assert_eq!(entry(&EffectDraft::new("nope")), None);
    }

    #[test]
    fn a_pick_sets_a_colour_of_the_effect() {
        let d = draft("duotone").unwrap();
        assert_eq!(colour_param(&d, None).unwrap(), "first");
        assert_eq!(colour_param(&d, Some("second")).unwrap(), "second");
        assert!(matches!(colour_param(&d, Some("brightness")), Err(CmdError::BadParams(_))));
        let armed = EffectDraft { eyedropper: Some("second".into()), ..d };
        assert_eq!(colour_param(&armed, None).unwrap(), "second");
        assert!(matches!(colour_param(&draft("sharpen").unwrap(), None), Err(CmdError::BadParams(m)) if m.contains("no colour")));
    }
}
