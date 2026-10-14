//! `ui.menu.tree` and `ui.menu.invoke` (`control-protocol.md` §6.10): the menu bar as shown, from
//! the same model as the in-window menus ([`crate::menus`]), and an item run as a click runs it.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, Error, Next, Outcome};
use crate::menus::{self, Lookup, Platform};

/// The context menus `ui.menu.tree {context}` names (§6.10).
const CONTEXTS: &[&str] = &["photo", "section", "tray", "treeGroup", "tag", "face"];

/// How long `ui.menu.invoke` waits for a dialog it opened to show.
const DIALOG_WAIT: Duration = Duration::from_secs(5);

/// The longest command id `ui.menu.invoke {id}` takes.
const MAX_ID: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tree {
    context: Option<String>,
}

/// `ui.menu.tree {context?}`: the bar, or a context menu's rows, for the current state.
pub(super) fn tree(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let t: Tree = call.params(params)?;
    let app = call.host.app();
    let platform = Platform::current();
    let reply = match t.context.as_deref() {
        None => menus::tree_json(&menus::bar(app, platform)),
        Some("section") => menus::nodes_json(&menus::section_menu(app, platform)),
        Some("photo") => menus::nodes_json(&menus::photo_menu(app, platform)),
        Some("tray") => menus::nodes_json(&crate::panels::bottom::tray::context_nodes(app, &tray_targets(app))),
        Some(c) if CONTEXTS.contains(&c) => {
            return Err(Error::new(Code::Unsupported, format!("the {c} context menu is not built yet; it comes with its region")));
        }
        Some(other) => return Err(call.bad(format!("no context menu `{other}`; one of {}", CONTEXTS.join(", ")))),
    };
    Ok(Outcome::Reply(Ok(reply)))
}

/// The photos the tray menu acts on when it opens on the tray's empty space: the tray-selected
/// ones, or with none every photo in the tray (`ui-layout.md` §6.2).
fn tray_targets(app: &crate::App) -> Vec<dalica_core::PhotoId> {
    let state = app.state();
    let selected: Vec<dalica_core::PhotoId> = state.tray.selected.iter().copied().filter(|id| crate::tray::shows(state, *id)).collect();
    if selected.is_empty() { crate::tray::photos(state) } else { selected }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invoke {
    path: Option<String>,
    id: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    wait: Option<bool>,
}

/// `ui.menu.invoke {path | id, params?, wait?}`: behaves like clicking the item. A dialog item opens
/// its dialog (`{dialog: kind}`); any other runs its command as `engine.execute` does (the
/// selection where it takes `ids`, the row's fixed parameters, then `params`; confirmation, path
/// and external-effect rules included). Disabled items are `disabled`, catalog items not live yet
/// `unsupported`, unknown ones `not_found`.
pub(super) fn invoke(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Invoke = call.params(params)?;
    if !matches!(p.params, Value::Null | Value::Object(_)) {
        return Err(call.bad(format!("`params` must be an object, not {}", p.params)));
    }
    let lookup = match (p.path, p.id) {
        (Some(path), None) => Lookup::Path(menus::split_path(&path).map_err(|e| call.bad(e))?),
        (None, Some(id)) if id.is_empty() || id.len() > MAX_ID => return Err(call.bad(format!("`id` must be a command id of 1–{MAX_ID} bytes"))),
        (None, Some(id)) => Lookup::Id { id, params: p.params.clone() },
        _ => return Err(call.bad("give either `path` or `id`")),
    };
    let model = menus::bar(call.host.app(), Platform::current());
    let Some(item) = menus::find(&model, &lookup).cloned() else {
        let what = match &lookup {
            Lookup::Path(parts) => format!("at `{}`", parts.join("/")),
            Lookup::Id { id, .. } => format!("running `{id}`"),
        };
        return Err(Error::new(Code::NotFound, format!("no menu item {what} (ui.menu.tree lists them)")));
    };
    if !item.live {
        return Err(Error::new(Code::Unsupported, format!("`{}` ({}) is not built yet", item.catalog_label, item.id)));
    }
    if !item.enabled {
        let why = item.reason.clone().unwrap_or_else(|| "it is disabled now".into());
        return Err(Error::new(Code::Disabled, format!("`{}` is disabled: {why}", item.label)));
    }
    if let Some(dialog) = item.dialog {
        if p.params.as_object().is_some_and(|o| !o.is_empty()) {
            return Err(call.bad(format!("`{}` opens a dialog and takes no `params`; answer the dialog instead", item.label)));
        }
        let reply = menus::activate(call.host.app_mut(), &item, &Value::Null)?;
        if dialog != menus::FILE_PICKER {
            // An in-app dialog is open now: reply once a frame has drawn it.
            return Ok(Outcome::play_then_reply(Input::Frames(1), reply));
        }
        let stub = call.control.picker.clone();
        let reply = json!({ "dialog": dialog });
        // The picker opens on its own thread (after one frame, if the panels were drawing): wait
        // for it off the UI thread.
        return Ok(Outcome::Play(
            Input::Frames(1),
            Box::new(move |_, _| {
                Outcome::Work(Box::new(move || {
                    let start = Instant::now();
                    while stub.waiting().is_none() {
                        if start.elapsed() > DIALOG_WAIT {
                            return Next::Reply(Err(Error::new(Code::Timeout, "the dialog did not open within 5 s")));
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Next::Reply(Ok(reply))
                }))
            }),
        ));
    }
    let mut command_params = menus::params_for(call.host.app(), item.id, &item.params);
    if let (Value::Object(c), Value::Object(extra)) = (&mut command_params, &p.params) {
        for (k, v) in extra {
            c.insert(k.clone(), v.clone());
        }
    }
    // A command that asks first opens its question, as a click does (§5.5).
    if let Some(ask) = crate::dialogs::asking(call.host.app(), item.id, &command_params) {
        let reply = crate::dialogs::ask(call.host.app_mut(), ask, &command_params)?;
        return Ok(Outcome::play_then_reply(Input::Frames(1), reply));
    }
    super::engine::execute(call, &json!({ "command": item.id, "params": command_params, "wait": p.wait }))
}
