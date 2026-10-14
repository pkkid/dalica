//! `methods`, `engine.execute` and `engine.commands` (`control-protocol.md` §6, §6.1, §6.2).

use std::path::PathBuf;

use dalica_engine::{CommandSpec, Context, Effect};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::host::Input;
use super::{Call, Code, Config, Error, METHODS, Method, Next, Outcome, bad_params, paths};
use crate::app::UI_COMMANDS;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// `methods`: every method of §6 (but `auth`) with its params doc and whether it is built.
pub(super) fn methods(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let _: NoParams = call.params(params)?;
    let list: Vec<Value> = METHODS
        .iter()
        .map(|m| {
            let mut v = json!({ "method": m.name, "params": m.params, "result": m.result, "built": m.run.is_some() });
            if m.run.is_none()
                && let Some(o) = v.as_object_mut()
            {
                o.insert("planned".into(), m.planned.into());
            }
            v
        })
        .collect();
    Ok(Outcome::Reply(Ok(Value::Array(list))))
}

/// Commands whose UI asks first (§5.5) and that have no dialog on the host yet, with the question
/// their dialog will ask. Commands that ask through the host are in `crate::dialogs::confirm::ASKS`
/// instead (their question applies only sometimes, and the same row opens the dialog in the app).
const CONFIRM: &[(&str, &str)] = &[(
    "library.rebuild",
    "Rebuild the library? Dalica forgets its index and reads every watched folder again; stars and captions come back from the folders.",
)];

/// Commands that read a path the user would pick (§5.7 reading), and the parameter.
const READ_PATHS: &[(&str, &str)] = &[("folders.add", "path")];

/// Commands that make new files at a path (§5.7 writing), the parameter, and what it names: the
/// folder an export folder is made in, the file Save As writes.
const WRITE_PATHS: &[(&str, &str, paths::Output)] = &[("export.run", "dir", paths::Output::Folder), ("photo.saveTo", "path", paths::Output::File)];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execute {
    command: String,
    #[serde(default)]
    params: Value,
    /// Accepted for scripts written for jobs (§5.3); there are no long jobs yet, so every reply
    /// carries its result.
    #[serde(default, rename = "wait")]
    _wait: Option<bool>,
}

/// What `engine.execute` needs to know about a command before running it.
struct Known {
    label: &'static str,
    effect: Option<Effect>,
    spec: Option<CommandSpec>,
}

fn known(host: &dyn super::Host, id: &str) -> Option<Known> {
    if let Some(c) = UI_COMMANDS.iter().find(|c| c.id == id) {
        return Some(Known { label: c.label, effect: None, spec: None });
    }
    let spec = match host.app().session() {
        Some(s) => s.spec(id).copied(),
        None => dalica_engine::cmd::specs().into_iter().find(|s| s.id == id),
    }?;
    Some(Known { label: spec.label, effect: Some(spec.effect), spec: Some(spec) })
}

/// Commands that reach outside Dalica need `--automation-allow-external` (§5.6).
pub(super) fn external_allowed(effect: Option<Effect>, allow_external: bool, id: &str) -> Result<(), Error> {
    if effect == Some(Effect::External) && !allow_external {
        return Err(Error::new(
            Code::ExternalDenied,
            format!("`{id}` reaches outside Dalica; start the app with --automation-allow-external to allow it"),
        ));
    }
    Ok(())
}

/// `engine.execute {command, params?, wait?}`: one command, one undo step, never a dialog (§6.1).
/// A path where a command makes new files ([`WRITE_PATHS`]) is checked against the automation
/// roots first, before anything else about the command (§5.7).
pub(super) fn execute(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let e: Execute = call.params(params)?;
    let id = e.command;
    let cmd_params: Map<String, Value> = match &e.params {
        Value::Null => Map::new(),
        Value::Object(o) => o.clone(),
        other => return Err(call.bad(format!("`params` must be an object, not {other}"))),
    };
    let config = call.control.config().clone();
    let method = call.method;
    if let Some((_, param, kind)) = WRITE_PATHS.iter().find(|(c, _, _)| *c == id)
        && let Some(path) = cmd_params.get(*param)
    {
        let Some(path) = path.as_str().map(PathBuf::from) else {
            return Err(call.bad(format!("`{param}` must be a path, not {path}")));
        };
        paths::lexical(&path, param)?;
        let watched = call.host.app().session().map(|s| s.roots().to_vec()).unwrap_or_default();
        let kind = *kind;
        return Ok(Outcome::Work(Box::new(move || match paths::check_output(&path, &config.effective_write_roots(), &watched, kind) {
            Err(e) => Next::Reply(Err(e)),
            Ok(_) => Next::Resume(Box::new(move |host| checked(host, &config, method, id, cmd_params).unwrap_or_else(Outcome::from))),
        })));
    }
    checked(call.host, &config, method, id, cmd_params)
}

/// The rest of `engine.execute` once its paths to write are checked: the command must exist, be
/// allowed and enabled, and not ask first; a path it reads is checked; then it runs.
fn checked(
    host: &mut dyn super::Host,
    config: &Config,
    method: &'static Method,
    id: String,
    mut cmd_params: Map<String, Value>,
) -> Result<Outcome, Error> {
    let Some(k) = known(host, &id) else {
        return Err(Error::new(Code::UnknownCommand, format!("no command `{id}` (engine.commands lists them)")));
    };
    external_allowed(k.effect, config.allow_external, &id)?;
    if let (Some(spec), Some(session)) = (&k.spec, host.app().session()) {
        (spec.enabled)(session, &Value::Object(cmd_params.clone())).map_err(|d| Error::new(Code::Disabled, d.0))?;
    }
    // A command that asks through the dialog host, when its question applies (§5.5).
    if let Some(ask) = crate::dialogs::asking(host.app(), &id, &Value::Object(cmd_params.clone())) {
        let mut err = Error::new(
            Code::NeedsConfirm,
            format!("`{id}` asks first in the app; send \"confirm\": true in its params to go ahead (prompt holds the question)"),
        );
        err.prompt = Some(crate::dialogs::confirm::prompt(ask));
        return Err(err);
    }
    if let Some((_, question)) = CONFIRM.iter().find(|(c, _)| *c == id) {
        match cmd_params.remove("confirm") {
            Some(Value::Bool(true)) => {}
            None | Some(Value::Bool(false)) => {
                let mut err = Error::new(
                    Code::NeedsConfirm,
                    format!("`{id}` asks first in the app; send \"confirm\": true in its params to go ahead (prompt holds the question)"),
                );
                err.prompt =
                    Some(json!({ "title": k.label.trim_end_matches('…'), "text": question, "buttons": [k.label.trim_end_matches('…'), "Cancel"] }));
                return Err(err);
            }
            Some(other) => return Err(bad_params(method, format!("`confirm` must be true or false, not {other}"))),
        }
    }
    let cmd_params = Value::Object(cmd_params);
    if let Some((_, param)) = READ_PATHS.iter().find(|(c, _)| *c == id)
        && let Some(path) = cmd_params.get(*param).and_then(Value::as_str).map(PathBuf::from)
    {
        paths::lexical(&path, param)?;
        let config = config.clone();
        let watched = host.app().session().map(|s| s.roots().to_vec()).unwrap_or_default();
        return Ok(Outcome::Work(Box::new(move || match paths::check_read(&path, &config.read_roots, &watched, config.demo) {
            Err(e) => Next::Reply(Err(e)),
            Ok(_) => Next::Resume(Box::new(move |host| run_command(host, &id, &cmd_params))),
        })));
    }
    Ok(run_command(host, &id, &cmd_params))
}

/// Runs the command, then lets one frame run so widgets show its effect.
fn run_command(host: &mut dyn super::Host, id: &str, params: &Value) -> Outcome {
    let result = host.app_mut().run(id, params);
    Outcome::Play(Input::Frames(1), Box::new(move |_, _| Outcome::Reply(result.map_err(Error::from))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Commands {
    filter: Option<String>,
    #[serde(default)]
    enabled_only: bool,
}

/// The effective shortcut of `id` (the keymap's, with overrides), written as the docs write it
/// (`Cmd` on macOS).
fn shortcut(call: &Call<'_>, id: &str, default: Option<&str>) -> Option<String> {
    let text = match call.host.app().session() {
        Some(s) => s
            .keymap()
            .bindings()
            .into_iter()
            .find(|b| b.command == id && matches!(b.context, Context::Library | Context::Global))
            .map(|b| b.key.to_string()),
        None => default.map(str::to_string),
    }?;
    Some(if cfg!(target_os = "macos") { text.replace("Ctrl+", "Cmd+") } else { text })
}

/// `engine.commands {filter?, enabledOnly?}` (§6.2): the engine's commands and `UI_COMMANDS`.
pub(super) fn commands(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let c: Commands = call.params(params)?;
    let filter = c.filter.map(|f| f.to_lowercase());
    let matches =
        |id: &str, label: &str, menu: &str| filter.as_deref().is_none_or(|f| [id, label, menu].iter().any(|s| s.to_lowercase().contains(f)));
    let mut out = Vec::new();
    let engine: Vec<_> = match call.host.app().session() {
        Some(s) => s.commands(),
        None => dalica_engine::cmd::specs()
            .iter()
            .map(|s| dalica_engine::CommandInfo {
                id: s.id,
                label: s.label,
                menu: s.menu,
                shortcut: dalica_engine::keymap::first_default(s.keys, dalica_engine::keymap::Platform::current()),
                params: s.params,
                enabled: false,
                ui: false,
                checked: None,
                live: true,
                effect: s.effect,
                history: s.history,
            })
            .collect(),
    };
    for info in engine {
        if !matches(info.id, info.label, info.menu) || (c.enabled_only && !info.enabled) {
            continue;
        }
        out.push(json!({
            "id": info.id, "label": info.label, "menu": info.menu, "shortcut": shortcut(call, info.id, info.shortcut),
            "params": info.params, "enabled": info.enabled, "ui": false, "checked": info.checked, "live": info.live,
            "effect": info.effect, "history": info.history,
        }));
    }
    for u in UI_COMMANDS {
        let enabled = crate::commands::enabled(call.host.app(), u.id, &json!({})).is_ok();
        if !matches(u.id, u.label, u.menu) || (c.enabled_only && !enabled) {
            continue;
        }
        out.push(json!({
            "id": u.id, "label": u.label, "menu": u.menu, "shortcut": shortcut(call, u.id, dalica_engine::keymap::first_default(u.keys, dalica_engine::keymap::Platform::current())),
            "params": u.params, "enabled": enabled, "ui": true, "checked": null, "live": true, "effect": "view", "history": "none",
        }));
    }
    Ok(Outcome::Reply(Ok(Value::Array(out))))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::app::App;
    use crate::control::{Config, Connection, Control, Request, serve};
    use crate::headless::Driver;

    /// The shortcuts `engine.commands` shows, before and after a library is open: none carries a
    /// platform qualifier (`win,linux:Ctrl+Y`), and Redo shows this platform's key.
    #[test]
    fn engine_commands_show_no_platform_qualifier() {
        let control = Control::new(Config::default());
        let (_dir, session, _) = crate::app::tests::session_with_photos(1);
        let ctrl = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
        let redo = if cfg!(target_os = "macos") { "Cmd+Shift+Z" } else { "Ctrl+Y" };
        for app in [App::new(None), App::new(Some(session))] {
            let mut d = Driver::new(app, 320, 200, 1.0);
            let mut conn = Connection::new();
            let all = serve(&control, &mut conn, &mut d, &Request::new("engine.commands", json!({}))).unwrap();
            let all = all.as_array().unwrap();
            let shown: Vec<&str> = all.iter().filter_map(|c| c["shortcut"].as_str()).collect();
            assert!(shown.len() > 5, "{shown:?}");
            assert!(shown.iter().all(|k| !k.contains(':')), "{shown:?}");
            let shortcut = |id: &str| all.iter().find(|c| c["id"] == id).map(|c| c["shortcut"].clone()).unwrap_or(Value::Null);
            assert_eq!(shortcut("edit.redo"), json!(redo));
            assert_eq!(shortcut("view.thumbsSmall"), json!(format!("{ctrl}+1")));
            assert_eq!(shortcut("window.fullscreen"), json!("F11"));
        }
    }
}
