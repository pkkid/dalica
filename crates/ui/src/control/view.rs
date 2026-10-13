//! `ui.state`, `ui.set`, `ui.inspect` and `ui.widgets` (`control-protocol.md` §6.5–§6.7).

use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::{self, Role};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, Error, Outcome, object};
use crate::state::{LIGHT_TABLE_MIN, MAX_IDS, SCROLL_OFFSET_MAX, Section, THUMB_MAX, THUMB_MIN, TREE_MIN, UiState};

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
    "editTopBar",
    "editPanel",
    "canvas",
    "filmstrip",
    "captionBar",
];

/// `ui.screenshot {region: "<name>"}`: the region's widget (`region:<name>`) to crop to;
/// `bad_params` if it is not one of [`REGIONS`], `not_found` if it is not shown.
pub(super) fn region(call: &Call<'_>, name: &str) -> Result<dalica_ui_kit::WidgetInfo, Error> {
    if !REGIONS.contains(&name) {
        return Err(not_a_region(call, name));
    }
    registry::find(call.host.ctx(), &format!("region:{name}"))
        .ok_or_else(|| Error::new(Code::NotFound, format!("region `{name}` is not shown (ui.inspect → regions)")))
}

fn not_a_region(call: &Call<'_>, name: &str) -> Error {
    call.bad(format!("`{name}` is not a region; regions are {}", REGIONS.join(", ")))
}

/// Every region of [`REGIONS`] with its rect as the last frame registered it (`region:<name>`),
/// `null` for one not shown (§6.6).
fn regions(widgets: &[WidgetInfo]) -> serde_json::Map<String, Value> {
    REGIONS
        .iter()
        .map(|name| {
            let id = format!("region:{name}");
            let rect = widgets.iter().find(|w| w.id == id).map_or(Value::Null, |w| json!(w.rect));
            ((*name).to_string(), rect)
        })
        .collect()
}

/// Whether point `(x, y)` is inside `r` (`[x, y, w, h]`, the right and bottom edges excluded).
fn contains(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

/// Fields of the view state that `ui.set` does not change, and what does.
const READ_ONLY: &[(&str, &str)] = [
    ("selection", "the select.set and select.none commands"),
    ("status", "nothing: it is the status line's text as drawn"),
    ("dialogs", "the commands that open them, their buttons, ui.dialog.confirm and ui.dialog.cancel"),
]
.as_slice();

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
    let sets_tree = changes.contains_key("treeWidth");
    let mut merged = state_json(call)?;
    if let Some(obj) = merged.as_object_mut() {
        for (key, value) in changes {
            // `edit` merges field by field (`{"edit": {"zoom": 2}}` keeps the photo and the tab);
            // every other field is replaced whole.
            match (key.as_str(), value, obj.get_mut(&key)) {
                ("edit", Value::Object(fields), Some(Value::Object(current))) => current.extend(fields),
                (_, value, _) => {
                    obj.insert(key, value);
                }
            }
        }
    }
    let new: UiState = serde_json::from_value(merged).map_err(|e| call.bad(e))?;
    if !(new.thumb_size.is_finite() && (THUMB_MIN..=THUMB_MAX).contains(&new.thumb_size)) {
        return Err(call.bad(format!("`thumbSize` is {THUMB_MIN} … {THUMB_MAX}, not {}", new.thumb_size)));
    }
    // The splitter's limits for this window: 200 … W − 480 (`ui-layout.md` §2).
    let tree_max = (call.host.window().width - LIGHT_TABLE_MIN).max(TREE_MIN);
    if sets_tree && !(new.tree_width.is_finite() && (TREE_MIN..=tree_max).contains(&new.tree_width)) {
        return Err(call.bad(format!("`treeWidth` is {TREE_MIN} … {tree_max} in this window, not {}", new.tree_width)));
    }
    if !(new.scroll.offset.is_finite() && (0.0..=SCROLL_OFFSET_MAX).contains(&new.scroll.offset)) {
        return Err(call.bad(format!("`scroll.offset` is 0 … {SCROLL_OFFSET_MAX}, not {}", new.scroll.offset)));
    }
    new.check_m6().map_err(|e| call.bad(e))?;
    new.check_m7().map_err(|e| call.bad(e))?;
    let tray = &new.tray;
    if tray.held.len() > MAX_IDS || tray.selected.len() > MAX_IDS || new.collapsed_folders.len() > MAX_IDS {
        return Err(Error::new(Code::Limit, format!("`tray.held`, `tray.selected` and `collapsedFolders` take at most {MAX_IDS} ids each")));
    }
    if let Some(id) = tray.selected.iter().find(|id| !tray.held.contains(id) && !new.selection.ids.contains(id)) {
        return Err(call.bad(format!("`tray.selected` names photo {id}, which the tray does not show (it shows the selection and `tray.held`)")));
    }
    let library = call.host.app().session().map(dalica_engine::Session::library);
    let has_folder = |f| library.is_some_and(|l| l.folder(f).is_some());
    let folder_of = |s: Section| match s {
        Section::Folder(f) => f,
    };
    let section = new.scroll.section.map(folder_of);
    let described = new.description_edit.as_ref().map(|e| folder_of(e.section));
    if let Some(f) = new.folder.iter().chain(&section).chain(&described).chain(&new.collapsed_folders).find(|f| !has_folder(**f)) {
        return Err(Error::new(Code::NotFound, format!("no folder {f}")));
    }
    let captioned = new.caption_edit.as_ref().map(|e| e.photo);
    let edited = new.edit.photo;
    if let Some(id) =
        tray.held.iter().chain(&tray.selected).chain(&captioned).chain(&edited).find(|id| !library.is_some_and(|l| l.photo(**id).is_some()))
    {
        return Err(Error::new(Code::NotFound, format!("no photo {id}")));
    }
    // Repeated ids are dropped; every number is already in range.
    let mut new = new.sanitized();
    // The transient status message is not part of the protocol's state: keep it.
    new.status = call.host.app().state().status.clone();
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

/// `ui.inspect {resetPerf?}`: the view state plus the window, hover and focus, the scroll offsets,
/// the open popup, dialog and tooltip, running animations, jobs, the stubbed file picker, notices,
/// the regions' rects and the perf counters (§6.6). `resetPerf` clears the maxima after this
/// report.
pub(super) fn inspect(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Inspect = call.params(params)?;
    let mut v = state_json(call)?;
    let widgets = registry::widgets(call.host.ctx());
    let (hovered, focused) = hovered_and_focused(call, &widgets);
    let notices: Vec<String> = call.host.app().session().map(|s| [s.notices(), s.scan_warnings()].concat()).unwrap_or_default();
    let extra = json!({
        "window": call.host.window(),
        "regions": regions(&widgets),
        "hovered": hovered,
        "focused": focused,
        "filePicker": call.control.stub().waiting(),
        "toasts": call.host.app().notices().inspect(call.host.ctx().input(|i| i.time)),
        "jobs": super::jobs::list(call.host),
        "dialog": crate::dialogs::inspect(call.host.app()),
        "popup": popup(&widgets),
        "tooltip": call.host.app().tooltip(),
        "animations": animations(call),
        "notices": notices,
        "libraryOpen": call.host.app().session().is_some(),
        "perf": perf(call),
    });
    if let (Some(obj), Value::Object(extra)) = (v.as_object_mut(), extra) {
        obj.extend(extra);
    }
    // Where the views are scrolled to, beside `scroll` (the view state's anchor) (§6.6).
    let views = Value::Object(scroll_views(call));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("scrollViews".into(), views);
    }
    if p.reset_perf {
        call.host.perf_mut().reset();
    }
    Ok(Outcome::Reply(Ok(v)))
}

/// The light table's and the tree's offsets (points), the section at the top of the light table
/// and the pinned header, as the last frame drew them.
fn scroll_views(call: &Call<'_>) -> serde_json::Map<String, Value> {
    let app = call.host.app();
    let info = app.panels().map(|p| p.light_table.frame_info().clone()).unwrap_or_default();
    let tree = crate::panels::folders::info(call.host.ctx());
    let top = app.state().scroll.section.map(|s| s.to_string());
    let sticky = info.sticky.map(|(s, _)| s.to_string());
    let v = json!({ "lightTable": info.offset, "tree": tree.offset, "topSection": top, "sticky": sticky });
    match v {
        Value::Object(o) => o,
        _ => serde_json::Map::new(),
    }
}

/// The animations running now: the light table easing to a section.
fn animations(call: &Call<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    if call.host.app().panels().is_some_and(|p| p.light_table.animating()) {
        out.push(json!({ "kind": "scrollToSection", "region": "lightTable" }));
    }
    out
}

/// The open menu, context menu or popup as the last frame drew it: `{kind, menu?, items: [{widget,
/// id, label, enabled, checked}]}`, from the rows it registered (`menuitem:<command id>[/<part>]`,
/// `submenu:…`); `null` when none is open. `kind` is `menu` (with the bar menu's `menu` key) for a
/// menu of the bar, `viewOptions` for the toolbar's view-options popup, `contextMenu` otherwise.
fn popup(widgets: &[WidgetInfo]) -> Value {
    let rows: Vec<Value> = widgets
        .iter()
        .filter(|w| w.role == Role::MenuItem)
        .map(|w| {
            let id = w.id.strip_prefix("menuitem:").map(|rest| rest.split('/').next().unwrap_or(rest));
            json!({ "widget": w.id, "id": id, "label": w.label, "enabled": w.enabled, "checked": w.active })
        })
        .collect();
    if rows.is_empty() {
        return Value::Null;
    }
    let menu = widgets.iter().find(|w| w.role == Role::Menu && w.active).and_then(|w| w.id.strip_prefix("menu:"));
    let view_options = widgets.iter().any(|w| w.id == crate::panels::toolbar::VIEW_OPTIONS_ID && w.active);
    match (menu, view_options) {
        (Some(m), _) => json!({ "kind": "menu", "menu": m, "items": rows }),
        (None, true) => json!({ "kind": "viewOptions", "items": rows }),
        (None, false) => json!({ "kind": "contextMenu", "items": rows }),
    }
}

/// The frame counters plus the thumbnails' cell and texture counters (`placeholderCells`, …).
fn perf(call: &Call<'_>) -> Value {
    let mut perf = serde_json::to_value(call.host.perf().report()).unwrap_or_else(|_| json!({}));
    let thumbs = call.host.app().panels().map(|p| p.thumbnails.counts()).unwrap_or_default();
    if let (Some(obj), Ok(Value::Object(extra))) = (perf.as_object_mut(), serde_json::to_value(thumbs)) {
        obj.extend(extra);
    }
    perf
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

/// `ui.widgets {filter?, region?}`: the widgets of the last frame, in paint order (§6.7). `region`
/// keeps the widgets whose centre is inside that region's rect (the region's own `region:<name>`
/// entry included, the other regions' left out); a region not shown has none.
pub(super) fn widgets(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Widgets = call.params(params)?;
    let all = registry::widgets(call.host.ctx());
    let within = match &p.region {
        None => None,
        Some(name) if REGIONS.contains(&name.as_str()) => {
            let id = format!("region:{name}");
            Some((all.iter().find(|w| w.id == id).map(|w| w.rect), id))
        }
        Some(name) => return Err(not_a_region(call, name)),
    };
    let in_region = |w: &WidgetInfo| match &within {
        None => true,
        Some((None, _)) => false,
        Some((Some(r), id)) => {
            w.id == *id || (!w.id.starts_with("region:") && contains(*r, w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0))
        }
    };
    let list: Vec<Value> = all
        .iter()
        .filter(|w| in_region(w))
        .filter(|w| p.filter.as_deref().is_none_or(|f| w.id.contains(f)))
        .map(|w| {
            let mut v = json!({ "id": w.id, "rect": w.rect, "enabled": w.enabled, "role": w.role });
            if let Some(o) = v.as_object_mut() {
                if checkable(w.role) {
                    o.insert("checked".into(), w.active.into());
                }
                if !w.label.is_empty() {
                    o.insert("label".into(), w.label.clone().into());
                }
            }
            v
        })
        .collect();
    Ok(Outcome::Reply(Ok(Value::Array(list))))
}
