//! `ui.state`, `ui.set`, `ui.inspect` and `ui.widgets` (`control-protocol.md` §6.5–§6.7).

use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::{self, Role};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, Error, Outcome, object};
use crate::state::{THUMB_MAX, THUMB_MIN, UiState};

/// The regions of `ui-layout.md` §2 that `ui.inspect` → `regions` and `ui.widgets {region}` name.
pub const REGIONS: &[&str] = &[
    "menuBar",
    "toolbar",
    "tabStrip",
    "tree",
    "splitter",
    "lightTable",
    "scrollbar",
    "sidePane",
    "statusLine",
    "tray",
    "trayButtons",
    "bottomControls",
    "actionButtons",
    "editPanel",
    "canvas",
    "filmstrip",
    "captionBar",
];

/// A region name: `unsupported` until the regions register their rects (M3), `bad_params` if it is
/// not one of [`REGIONS`].
pub(super) fn region(call: &Call<'_>, name: &str) -> Error {
    if REGIONS.contains(&name) {
        Error::new(Code::Unsupported, format!("region `{name}` is not reported yet: regions register their rects with the window shell (M3)"))
    } else {
        call.bad(format!("`{name}` is not a region; regions are {}", REGIONS.join(", ")))
    }
}

/// Fields of the view state that `ui.set` does not change, and what does.
const READ_ONLY: &[(&str, &str)] =
    [("selection", "the select.set and select.none commands"), ("status", "nothing: it is the result of the last action")].as_slice();

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

fn state_json(call: &Call<'_>) -> Result<Value, Error> {
    serde_json::to_value(call.host.app().state()).map_err(|e| Error::new(Code::Internal, format!("the view state does not serialise: {e}")))
}

/// `ui.state`: the view state (§6.5).
pub(super) fn state(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let _: NoParams = call.params(params)?;
    Ok(Outcome::Reply(Ok(state_json(call)?)))
}

/// `ui.set`: any subset of the view state; every field is checked before any changes.
pub(super) fn set(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let changes = object(call.method, params)?;
    if let Some((field, use_instead)) = READ_ONLY.iter().find(|(f, _)| changes.contains_key(*f)) {
        return Err(call.bad(format!("`{field}` cannot be set here; use {use_instead}")));
    }
    let mut merged = state_json(call)?;
    if let Some(obj) = merged.as_object_mut() {
        obj.extend(changes);
    }
    let new: UiState = serde_json::from_value(merged).map_err(|e| call.bad(e))?;
    if !(new.thumb_size.is_finite() && (THUMB_MIN..=THUMB_MAX).contains(&new.thumb_size)) {
        return Err(call.bad(format!("`thumbSize` is {THUMB_MIN} … {THUMB_MAX}, not {}", new.thumb_size)));
    }
    if let Some(f) = new.folder
        && !call.host.app().session().is_some_and(|s| s.library().folder(f).is_some())
    {
        return Err(Error::new(Code::NotFound, format!("no folder {f}")));
    }
    *call.host.app_mut().state_mut() = new;
    let reply = state_json(call)?;
    Ok(Outcome::play_then_reply(Input::Frames(1), reply))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Inspect {
    #[serde(default)]
    reset_perf: bool,
}

/// The widget under the pointer (topmost) and the one with keyboard focus.
fn hovered_and_focused(call: &Call<'_>, widgets: &[WidgetInfo]) -> (Option<String>, Option<String>) {
    let ctx = call.host.ctx();
    let hovered = ctx.pointer_hover_pos().and_then(|p| {
        widgets.iter().rev().find(|w| p.x >= w.rect[0] && p.y >= w.rect[1] && p.x < w.rect[0] + w.rect[2] && p.y < w.rect[1] + w.rect[3])
    });
    let focus = ctx.memory(|m| m.focused());
    let focused = focus.and_then(|f| widgets.iter().find(|w| w.egui_id == Some(f)));
    (hovered.map(|w| w.id.clone()), focused.map(|w| w.id.clone()))
}

/// `ui.inspect {resetPerf?}`: the view state plus the window, hover and focus, the stubbed file
/// picker, notices and the perf counters (§6.6). `resetPerf` clears the maxima after this report.
/// Regions, scroll, popups and dialogs join as their parts of the window are built (M3, M4).
pub(super) fn inspect(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Inspect = call.params(params)?;
    let mut v = state_json(call)?;
    let widgets = registry::widgets(call.host.ctx());
    let (hovered, focused) = hovered_and_focused(call, &widgets);
    let notices: Vec<String> = call.host.app().session().map(|s| [s.notices(), s.scan_warnings()].concat()).unwrap_or_default();
    let extra = json!({
        "window": call.host.window(),
        "hovered": hovered,
        "focused": focused,
        "filePicker": call.control.stub().waiting(),
        "toasts": [],
        "jobs": [],
        "notices": notices,
        "libraryOpen": call.host.app().session().is_some(),
        "perf": call.host.perf().report(),
    });
    if let (Some(obj), Value::Object(extra)) = (v.as_object_mut(), extra) {
        obj.extend(extra);
    }
    if p.reset_perf {
        call.host.perf_mut().reset();
    }
    Ok(Outcome::Reply(Ok(v)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Widgets {
    filter: Option<String>,
    region: Option<String>,
}

/// Roles whose `active` flag is a selected or toggled state.
fn checkable(role: Role) -> bool {
    matches!(role, Role::Toggle | Role::Thumbnail | Role::TreeRow | Role::MenuItem)
}

/// `ui.widgets {filter?, region?}`: the widgets of the last frame, in paint order (§6.7).
pub(super) fn widgets(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Widgets = call.params(params)?;
    if let Some(r) = &p.region {
        return Err(region(call, r));
    }
    let list: Vec<Value> = registry::widgets(call.host.ctx())
        .into_iter()
        .filter(|w| p.filter.as_deref().is_none_or(|f| w.id.contains(f)))
        .map(|w| {
            let mut v = json!({ "id": w.id, "rect": w.rect, "enabled": w.enabled, "role": w.role });
            if let Some(o) = v.as_object_mut() {
                if checkable(w.role) {
                    o.insert("checked".into(), w.active.into());
                }
                if !w.label.is_empty() {
                    o.insert("label".into(), w.label.into());
                }
            }
            v
        })
        .collect();
    Ok(Outcome::Reply(Ok(Value::Array(list))))
}
