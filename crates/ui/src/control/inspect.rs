//! `library.inspect {photos?, section?, offset?, limit?}` (`control-protocol.md` §6.4): the library
//! as the user sees it. The engine's inspection ([`dalica_engine::cmd::inspect::inspect`]) runs
//! over the window's own tree options (its Flat or Tree mode, folder sort, collapsed groups, Small
//! and Hidden Pictures), so its tree and sections are the ones drawn; this method adds the
//! window's fields:
//!
//! - `current`: the section the top of the light table is in (the pinned header's, else the first
//!   header drawn, else the scroll anchor's), `null` for none;
//! - `visibleSections`: the sections with a header or photos on screen, top first, as the last
//!   frame drew them;
//! - `selection`: `{section, wholeSection, ids, anchor, focus}` (`focus` is the anchor: the
//!   outlined photo a Shift range starts from);
//! - `tray`: `{token, items, held, traySelected}`: the section shown as one token with its count,
//!   every photo the tray holds (held first), the held ones and the tray's own selection;
//! - `scroll`: `ui.state`'s anchor `{section, offset}`;
//! - `view` as the window shows it, and on each listed photo `held` and `selected`.

use dalica_core::PhotoId;
use serde_json::{Map, Value, json};

use super::{Call, Code, Error, Method, Outcome, method, object};
use crate::state::UiState;

/// The method's entry in [`METHODS`](super::METHODS).
pub(super) const METHOD: Method = method(
    "library.inspect",
    "{photos?, section?, offset?, limit?}",
    "the library as the user sees it, with the window's current, visibleSections, selection, tray and scroll (§6.4)",
    inspect,
);

fn inspect(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let params = Value::Object(object(call.method, params)?);
    let app = call.host.app();
    let Some(session) = app.session() else {
        return Err(Error::new(Code::Disabled, "the library is not open yet"));
    };
    let options = crate::panels::light_table::tree_options(app);
    let mut v = dalica_engine::cmd::inspect::inspect(session, &params, Some(options)).map_err(|e| match e {
        // Named with the method and what it takes, as every method's own refusals are.
        dalica_engine::CmdError::BadParams(why) => call.bad(why),
        other => Error::from(other),
    })?;
    let state = app.state();
    let window = window_fields(call, state)?;
    if let Some(photos) = v.get_mut("photos").and_then(Value::as_array_mut) {
        for p in photos.iter_mut() {
            let id = p.get("id").and_then(Value::as_u64).map(PhotoId::new);
            if let (Some(id), Some(o)) = (id, p.as_object_mut()) {
                o.insert("held".into(), json!(state.tray.holds(id)));
                o.insert("selected".into(), json!(state.selection.contains(id)));
            }
        }
    }
    if let Some(o) = v.as_object_mut() {
        o.extend(window);
    }
    Ok(Outcome::Reply(Ok(v)))
}

/// The window's fields, from the view state and the light table's last frame.
fn window_fields(call: &Call<'_>, state: &UiState) -> Result<Map<String, Value>, Error> {
    let app = call.host.app();
    let info = app.panels().map(|p| p.light_table.frame_info().clone()).unwrap_or_default();
    let mut visible: Vec<String> = Vec::new();
    if let Some((s, _)) = info.sticky {
        visible.push(s.to_string());
    }
    if let Some(area) = info.area {
        for (s, rect) in &info.headers {
            let id = s.to_string();
            if rect.intersects(area) && !visible.contains(&id) {
                visible.push(id);
            }
        }
    }
    let current =
        info.sticky.map(|(s, _)| s.to_string()).or_else(|| visible.first().cloned()).or_else(|| state.scroll.section.map(|s| s.to_string()));
    let sel = &state.selection;
    let selection = json!({
        "section": sel.section.map(|s| s.to_string()),
        "wholeSection": sel.whole,
        "ids": sel.ids,
        "anchor": sel.anchor,
        "focus": sel.anchor,
    });
    let token = crate::tray::token(state).map(|s| json!({ "section": s.to_string(), "count": sel.ids.len() }));
    let tray = json!({
        "token": token,
        "items": crate::tray::photos(state),
        "held": state.tray.held,
        "traySelected": state.tray.selected,
    });
    let st = serde_json::to_value(state).map_err(|e| Error::new(Code::Internal, format!("the view state does not serialise: {e}")))?;
    let view = json!({
        "hiddenPictures": st.get("hiddenPictures"),
        "smallPictures": st.get("smallPictures"),
        "caption": st.get("caption"),
        "thumbSize": st.get("thumbSize"),
    });
    let mut out = Map::new();
    out.insert("current".into(), json!(current));
    out.insert("visibleSections".into(), json!(visible));
    out.insert("selection".into(), selection);
    out.insert("tray".into(), tray);
    out.insert("scroll".into(), st.get("scroll").cloned().unwrap_or(Value::Null));
    out.insert("view".into(), view);
    Ok(out)
}
