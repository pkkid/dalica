//! `methods`, `engine.execute` and `engine.commands` (`control-protocol.md` §6, §6.1, §6.2).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use dalica_engine::automation::Policy;
use dalica_engine::{CommandSpec, Context, Effect, JobEntry, JobId, JobKind};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::host::{Host, Input};
use super::jobs::{WAIT_DEFAULT_MS, list};
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

/// Commands that read a path the user would pick (§5.7 reading), and the parameter: a folder to
/// add, a card to import from, a face model file (the face model dialog inspects the file it opens
/// on at once; the engine's inspect and install read theirs).
const READ_PATHS: &[(&str, &str)] = &[
    ("folders.add", "path"),
    ("import.openSource", "path"),
    ("faceModel.addFile", "path"),
    ("faceModel.open", "file"),
    ("faces.models.inspect", "path"),
    ("faces.models.install", "path"),
];

/// Commands that make new files at a path (§5.7 writing), the parameter, and what it names: the
/// folder an export folder is made in, the file Save As writes, the folder an import makes its
/// folders in.
const WRITE_PATHS: &[(&str, &str, paths::Output)] = &[
    ("export.run", "dir", paths::Output::Folder),
    ("photo.saveTo", "path", paths::Output::File),
    ("import.run", "destination", paths::Output::Folder),
    ("video.exportClip", "path", paths::Output::File),
    ("player.exportClip", "path", paths::Output::File),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execute {
    command: String,
    #[serde(default)]
    params: Value,
    /// Whether the reply waits for a job the command starts (§5.3); `true` when left out.
    wait: Option<bool>,
}

/// The commands whose scans are their jobs (§5.3): their reply names the scan job they start.
const SCANS: &[&str] = &["folders.add", "folders.set", "library.rebuild"];

/// How the reply treats a job the command starts: waits for it until `deadline`, or names it at
/// once (`{job, pending: true}`).
#[derive(Clone, Copy)]
pub(super) struct Wait {
    on: bool,
    deadline: Instant,
}

impl Wait {
    /// A wait that ends [`WAIT_DEFAULT_MS`] from now: under the 60 s reply deadline (§4).
    pub(super) fn deadline_from_now() -> Instant {
        Instant::now() + Duration::from_millis(WAIT_DEFAULT_MS)
    }
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

/// Commands that reach outside Dalica need `--automation-allow-external` (§5.6), and a command
/// automation never runs (`automation::NEVER_AUTOMATED`: removing imported files from a card) is
/// refused whatever was granted (`security.md` §5.7).
pub(super) fn external_allowed(effect: Option<Effect>, allow_external: bool, id: &str) -> Result<(), Error> {
    dalica_engine::automation::never_automated(id)?;
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
    execute_until(call, params, Wait::deadline_from_now())
}

/// `engine.execute` whose wait for a job ends at `deadline` at the latest: a `batch` gives all its
/// steps one deadline, so the batch as a whole replies within the reply deadline.
pub(super) fn execute_until(call: &mut Call<'_>, params: &Value, deadline: Instant) -> Result<Outcome, Error> {
    let e: Execute = call.params(params)?;
    let wait = Wait { on: e.wait.unwrap_or(true), deadline };
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
            Ok(_) => Next::Resume(Box::new(move |host| checked(host, &config, method, id, cmd_params, wait).unwrap_or_else(Outcome::from))),
        })));
    }
    // A collage is written into `path`, else beside the collage the room reopened, else
    // `<Pictures>/Dalica/Collages`: the folder the engine names is checked as an export's `dir`
    // is, the default too. A path the engine itself refuses is the command's own refusal.
    if COLLAGE_WRITES.contains(&id.as_str())
        && let Some(session) = call.host.app().session()
    {
        let given = match cmd_params.get("path") {
            None | Some(Value::Null) => None,
            Some(Value::String(p)) => Some(PathBuf::from(p)),
            Some(other) => return Err(call.bad(format!("`path` must be a path, not {other}"))),
        };
        if let Some(p) = &given {
            paths::lexical(p, "path")?;
        }
        if let Ok(folder) = session.collage_destination(given.as_deref()) {
            let watched = session.roots().to_vec();
            return Ok(Outcome::Work(Box::new(move || {
                match paths::check_output(&folder, &config.effective_write_roots(), &watched, paths::Output::Folder) {
                    Err(e) => Next::Reply(Err(e)),
                    Ok(_) => Next::Resume(Box::new(move |host| checked(host, &config, method, id, cmd_params, wait).unwrap_or_else(Outcome::from))),
                }
            })));
        }
    }
    // Save Frame as Picture writes into `<Pictures>/Dalica/Video Frames` (`Session::video_frames_dir`):
    // that folder is checked as an export's `dir` is, and the verdict kept for the window's own
    // command (`player.saveFrame`, which asks `super::refuse_folder`).
    if VIDEO_FRAME_WRITES.contains(&id.as_str())
        && let Some(session) = call.host.app().session()
        && let Some(folder) = session.video_frames_dir()
    {
        paths::lexical(&folder, "path")?;
        let watched = session.roots().to_vec();
        return Ok(Outcome::Work(Box::new(move || {
            match paths::check_output(&folder, &config.effective_write_roots(), &watched, paths::Output::Folder) {
                Err(e) => Next::Reply(Err(e)),
                Ok(_) => Next::Resume(Box::new(move |host| {
                    super::vet::record(folder, watched, Ok(()));
                    checked(host, &config, method, id, cmd_params, wait).unwrap_or_else(Outcome::from)
                })),
            }
        })));
    }
    // The Collage room's Create Collage and Save Draft write where `collage.create` would without
    // a `path`: that folder is checked first, and the verdict kept for the room's command.
    if COLLAGE_UI_WRITES.contains(&id.as_str()) {
        let then: super::Resume = Box::new(move |host| checked(host, &config, method, id, cmd_params, wait).unwrap_or_else(Outcome::from));
        let config = call.control.config().clone();
        return Ok(super::vet::collage_first(call.host, &config, then));
    }
    // The Import room's buttons write where the destination strip's draft says: that folder is
    // checked as `import.run`'s destination would be.
    if DRAFT_WRITES.contains(&id.as_str()) {
        let app = call.host.app();
        if let Some(path) = crate::commands::import::place(app, &app.state().import.destination) {
            paths::lexical(&path, "destination")?;
            let watched = app.session().map(|s| s.roots().to_vec()).unwrap_or_default();
            return Ok(Outcome::Work(Box::new(move || {
                match paths::check_output(&path, &config.effective_write_roots(), &watched, paths::Output::Folder) {
                    Err(e) => Next::Reply(Err(e)),
                    Ok(_) => Next::Resume(Box::new(move |host| {
                        // What the room's command asks before it imports (`super::refuse`).
                        super::vet::record(path, watched, Ok(()));
                        checked(host, &config, method, id, cmd_params, wait).unwrap_or_else(Outcome::from)
                    })),
                }
            })));
        }
    }
    checked(call.host, &config, method, id, cmd_params, wait)
}

/// The commands that write a collage (§5.7): their folder is `Session::collage_destination`.
const COLLAGE_WRITES: &[&str] = &["collage.create", "collage.saveDraft"];

/// The commands that save a video's frame (§5.7): their folder is `Session::video_frames_dir`.
const VIDEO_FRAME_WRITES: &[&str] = &["video.saveFrame", "player.saveFrame"];

/// The Collage room's commands that run them (`crate::commands::collage`).
const COLLAGE_UI_WRITES: &[&str] = &["collage.createCollage", "collage.saveAsDraft"];

/// UI commands that run `import.run` into the destination drafted in the view state.
const DRAFT_WRITES: &[&str] = &["import.importAll", "import.importSelected"];

/// The rest of `engine.execute` once its paths to write are checked: the command must exist, be
/// allowed and enabled, and not ask first; a path it reads is checked; then it runs.
fn checked(
    host: &mut dyn super::Host,
    config: &Config,
    method: &'static Method,
    id: String,
    mut cmd_params: Map<String, Value>,
    wait: Wait,
) -> Result<Outcome, Error> {
    let Some(k) = known(host, &id) else {
        return Err(Error::new(Code::UnknownCommand, format!("no command `{id}` (engine.commands lists them)")));
    };
    external_allowed(k.effect, config.allow_external, &id)?;
    if !config.allow_external
        && let Some(key) = dalica_engine::automation::external_option(&id, &cmd_params)
    {
        return Err(dalica_engine::automation::external_option_refused(key).into());
    }
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
    // Without the people permission a search runs with names left out of its match (§6.4).
    let policy = people_policy(config);
    policy.people_params(&id, &mut cmd_params);
    let cmd_params = Value::Object(cmd_params);
    if let Some((_, param)) = READ_PATHS.iter().find(|(c, _)| *c == id)
        && let Some(path) = cmd_params.get(*param).and_then(Value::as_str).map(PathBuf::from)
    {
        paths::lexical(&path, param)?;
        let config = config.clone();
        let watched = host.app().session().map(|s| s.roots().to_vec()).unwrap_or_default();
        return Ok(Outcome::Work(Box::new(move || match paths::check_read(&path, &config.read_roots, &watched, config.demo) {
            Err(e) => Next::Reply(Err(e)),
            Ok(_) => Next::Resume(Box::new(move |host| run_command(host, &policy, &id, &cmd_params, wait))),
        })));
    }
    Ok(run_command(host, &policy, &id, &cmd_params, wait))
}

/// What the app's `--automation-allow-people` grants its control clients (§6.4), as the engine's
/// policy says it: their replies and their searches.
fn people_policy(config: &Config) -> Policy {
    let mut policy = Policy::default();
    policy.set_allow_people(config.allow_people);
    policy
}

/// The newest job listed now (0 when none).
fn newest_job(host: &dyn Host) -> JobId {
    list(host).iter().map(|e| e.job).max().unwrap_or(0)
}

/// Runs the command, then lets one frame run so widgets show its effect (and the session has
/// registered the jobs it started). A command that started a job then waits for it (§5.3). Its
/// reply goes through `policy` (§6.4): without `--automation-allow-people` people's names and face
/// rectangles are withheld, and a person's export folder is suggested without their name
/// (`control/people.rs`).
fn run_command(host: &mut dyn Host, policy: &Policy, id: &str, params: &Value, wait: Wait) -> Outcome {
    let before = newest_job(host);
    let scan = SCANS.contains(&id);
    let result = host
        .app_mut()
        .run(id, params)
        .map(|value| policy.people_reply(id, value))
        .map(|value| if policy.allow_people() { value } else { super::people::command(id, params, value) });
    Outcome::Play(
        Input::Frames(1),
        Box::new(move |host, _| match result {
            Err(e) => Outcome::Reply(Err(Error::from(e))),
            Ok(value) => {
                let jobs = started(host, before, scan, &value);
                match jobs.first() {
                    None => Outcome::Reply(Ok(value)),
                    Some(&job) if !wait.on => Outcome::Reply(Ok(pending(value, job))),
                    Some(_) => wait_for(host, value, jobs, wait.deadline),
                }
            }
        }),
    )
}

/// The jobs a command started, the one its reply names first: the job it names itself
/// (`{job, pending: true}`), and for the scans every scan or rebuild listed since `before`.
fn started(host: &dyn Host, before: JobId, scan: bool, value: &Value) -> Vec<JobId> {
    let named = value.get("job").and_then(Value::as_u64).filter(|_| value.get("pending") == Some(&Value::Bool(true)));
    let mut jobs: Vec<JobId> = named.into_iter().collect();
    if scan {
        for e in list(host) {
            if e.job > before && matches!(e.kind, JobKind::Scan | JobKind::Rebuild) && !jobs.contains(&e.job) {
                jobs.push(e.job);
            }
        }
    }
    jobs
}

/// The command's reply naming a job still running: `{…, job, pending: true}`.
fn pending(value: Value, job: JobId) -> Value {
    let mut out = match value {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    out.insert("job".into(), json!(job));
    out.insert("pending".into(), Value::Bool(true));
    Value::Object(out)
}

/// The command's reply once its jobs have ended: its own fields, then the first job's final entry
/// (`job`, `kind`, `state`, `result` or `error`, … as `jobs.wait` gives it); `scanning` is then
/// false.
fn finished(value: Value, entry: Option<JobEntry>) -> Value {
    let mut out = match value {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    out.remove("pending");
    if out.get("scanning").is_some_and(Value::is_boolean) {
        out.insert("scanning".into(), Value::Bool(false));
    }
    if let Some(Value::Object(e)) = entry.and_then(|e| serde_json::to_value(e).ok()) {
        out.extend(e);
    }
    Value::Object(out)
}

/// Lets frames run (never blocking one) until every job of `jobs` has ended, then replies
/// [`finished`]; at `deadline` with one still running, replies [`pending`] instead.
fn wait_for(host: &mut dyn Host, value: Value, jobs: Vec<JobId>, deadline: Instant) -> Outcome {
    let entries: Vec<Option<JobEntry>> = jobs.iter().map(|&j| host.app().session().and_then(|s| s.job(j))).collect();
    let running = entries.iter().flatten().any(|e| !e.state.is_finished());
    let first = jobs.first().copied().unwrap_or(0);
    if !running {
        return Outcome::Reply(Ok(finished(value, entries.into_iter().next().flatten())));
    }
    if Instant::now() >= deadline {
        return Outcome::Reply(Ok(pending(value, first)));
    }
    Outcome::Play(
        Input::Frames(1),
        Box::new(move |host, played| match played {
            Ok(_) => wait_for(host, value, jobs, deadline),
            Err(e) => Error::from(e).into(),
        }),
    )
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
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::wait_for;
    use crate::app::App;
    use crate::control::{Config, Connection, Control, Outcome, Request, run, serve};
    use crate::headless::Driver;

    /// A job still running at the deadline (55 s into the request's 60) is named, as with `wait:
    /// false`, rather than left to time out; once it has ended the wait gives its final entry.
    #[test]
    fn a_job_still_running_at_the_deadline_is_named_pending() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        dalica_fs::create_dir_all(&photos).unwrap();
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(4, 64, 48), 80, None).unwrap();
        dalica_fs::replace(&photos.join("IMG_0001.jpg"), &jpeg, dalica_fs::Expect::Absent).unwrap();
        let mut s = dalica_engine::Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.set_watching(dalica_engine::watching::WatchMode::Off);
        // The scan holds its file until the gate opens.
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (g, inner) = (Arc::clone(&gate), dalica_scan::default_reader());
        s.set_scan_reader(Some(Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
            let _ = g.1.wait_timeout_while(g.0.lock().unwrap(), Duration::from_secs(20), |open| !*open).unwrap();
            inner(path, limits)
        })));
        s.execute("folders.add", &json!({ "path": photos })).unwrap();
        let job = s.jobs().first().map(|e| e.job).unwrap();
        let mut d = Driver::new(App::new(Some(s)), 320, 200, 1.0);
        match wait_for(&mut d, json!({ "new": true, "scanning": true }), vec![job], Instant::now()) {
            Outcome::Reply(Ok(v)) => assert_eq!(v, json!({ "new": true, "scanning": true, "job": job, "pending": true })),
            _ => panic!("the deadline has passed: the reply names the job"),
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let later = Instant::now() + Duration::from_secs(50);
        let waiting = wait_for(&mut d, json!({ "new": true, "scanning": true }), vec![job], later);
        let v = run(&mut d, waiting).unwrap();
        assert_eq!((v["state"].as_str(), v["scanning"].as_bool(), v.get("pending")), (Some("done"), Some(false), None), "{v}");
    }

    /// Over automation a collage is written only inside a write root or a watched folder
    /// (`control-protocol.md` §5.7): the default `<Pictures>/Dalica/Collages` and a `path`
    /// elsewhere are refused before the command runs, nothing made; a `path` in a write root is
    /// written.
    #[test]
    fn a_collage_is_written_over_automation_only_inside_the_write_roots() {
        let (dir, mut session, ids) = crate::app::tests::session_with_photos(2);
        session.set_pictures_dir(Some(dir.path().join("Pictures")));
        session.execute("create.collage", &json!({ "ids": ids })).unwrap();
        let allowed = dir.path().join("allowed");
        dalica_fs::create_dir_all(&allowed).unwrap();
        let control = Control::new(Config { write_roots: vec![allowed.clone()], ..Config::default() });
        let mut d = Driver::new(App::new(Some(session)), 320, 200, 1.0);
        let mut conn = Connection::new();
        let mut execute = |command: &str, params: Value| {
            serve(&control, &mut conn, &mut d, &Request::new("engine.execute", json!({ "command": command, "params": params })))
        };
        let code = |r: Result<Value, crate::control::Error>| r.map_or_else(|e| e.code.as_str().to_string(), |_| "ok".into());
        assert_eq!(code(execute("collage.create", json!({}))), "path_denied", "Pictures is no write root");
        assert_eq!(code(execute("collage.saveDraft", json!({ "path": dir.path().join("elsewhere") }))), "path_denied");
        assert_eq!(code(execute("collage.create", json!({ "path": "relative" }))), "bad_params");
        assert!(!dir.path().join("Pictures").exists() && !dir.path().join("elsewhere").exists(), "nothing was made");
        let done = execute("collage.create", json!({ "path": allowed.join("Collages") })).unwrap();
        assert_eq!(done["state"], "done", "{done}");
        let written = std::fs::read_dir(allowed.join("Collages")).unwrap().count();
        assert_eq!(written, 2, "the JPEG and its recipe: {done}");
    }

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

    /// A control client has no people permission (`control-protocol.md` §6.4): `faces.list` and
    /// `people.list` over `engine.execute` answer with names `null` and no rectangles, ids and
    /// counts kept, while the engine's own answer holds them.
    #[test]
    fn people_replies_reach_a_control_client_without_names_or_rectangles() {
        let control = Control::new(Config::default());
        let (_dir, mut session, ids) = crate::app::tests::session_with_photos(1);
        session.execute("faces.add", &json!({ "id": ids[0], "rect": [0.2, 0.2, 0.4, 0.5] })).unwrap();
        session.execute("faces.name", &json!({ "faces": [format!("{}.0", ids[0])], "name": "Ana", "create": true, "nickname": "Ninha" })).unwrap();
        let own = session.execute("faces.list", &json!({})).unwrap();
        assert_eq!((own["faces"][0]["name"].clone(), own["faces"][0]["rect"].clone()), (json!("Ana"), json!([0.2, 0.2, 0.4, 0.5])));
        let mut d = Driver::new(App::new(Some(session)), 320, 200, 1.0);
        let mut conn = Connection::new();
        let ask = |d: &mut Driver, conn: &mut Connection, command: &str| {
            serve(&control, conn, d, &Request::new("engine.execute", json!({ "command": command, "params": {} }))).unwrap()
        };
        let faces = ask(&mut d, &mut conn, "faces.list");
        let face = &faces["faces"][0];
        assert_eq!((face["id"].clone(), face["name"].clone()), (json!(format!("{}.0", ids[0])), Value::Null));
        assert!(face.get("rect").is_none() && face.get("displayed").is_none(), "{face}");
        assert!(face["person"].is_string(), "the person's id stays");
        let people = ask(&mut d, &mut conn, "people.list");
        assert_eq!((people["people"][0]["name"].clone(), people["people"][0]["nickname"].clone()), (Value::Null, Value::Null));
        assert_eq!(people["people"][0]["count"], json!(1), "counts stay");
        assert!(!people.to_string().contains("Ana") && !faces.to_string().contains("Ana"));
    }
}
