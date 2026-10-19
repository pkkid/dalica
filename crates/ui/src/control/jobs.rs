//! `jobs.list`, `jobs.wait` and `jobs.cancel` (`control-protocol.md` §6.13) over the session's job
//! registry (`dalica_engine::jobs`). `jobs.wait` never blocks a frame: it looks at the job between
//! frames and lets one more frame run (which polls the session) until the job has finished or the
//! time is up.
//!
//! **A person's export folder** (§6.4, §6.13): an export of a person's section from the Export
//! dialog makes its folder under the name suggested from theirs. Without the people permission the
//! entries of those jobs reach the client with `folder`, `name` and `written` `null`
//! (`dalica_engine::automation::withhold_export_folder`), in `jobs.list`, `jobs.wait` and
//! `ui.inspect`'s `jobs`. The jobs are known by their id ([`observe`]): every export job that
//! appears while the window's Export draft names a person's section, as each control request (and
//! each step of one) begins, is marked; never by matching a folder's name against people's names.

use std::time::{Duration, Instant};

use dalica_engine::{JobEntry, JobId, JobKind};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, Config, Error, Host, MAX_WAIT_MS, Outcome};

/// How long `jobs.wait` waits without `timeoutMs`, and at most: under the 60 s request deadline,
/// so a long job ends in `{job, pending: true}` rather than a blind `timeout` (§5.3).
pub const WAIT_DEFAULT_MS: u64 = 55_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Wait {
    job: JobId,
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    job: Option<JobId>,
}

/// Every job listed now, oldest first; none before the library is open.
pub fn list(host: &dyn Host) -> Vec<JobEntry> {
    host.app().session().map(dalica_engine::Session::jobs).unwrap_or_default()
}

/// The export jobs marked as a person's ([`observe`]), kept in the window's egui memory.
#[derive(Clone, Debug, Default)]
struct PersonExports {
    /// While the Export draft names a person's section: the newest job listed when it was last
    /// seen open, so an export job after it is that draft's.
    armed: Option<JobId>,
    /// The marked jobs, by id and start (ids start again with a new session), while listed.
    jobs: Vec<(JobId, u64)>,
}

fn person_exports_id() -> egui::Id {
    egui::Id::new("dalica.control.personExports")
}

/// Marks the export jobs started from a person's section (module docs): an export job listed
/// now that is newer than the newest job listed when the Export draft last named a person's
/// section. Called as each control request and each of its steps begins, on the UI thread; looks
/// at memory only.
pub(super) fn observe(host: &dyn Host) {
    let app = host.app();
    let Some(session) = app.session() else { return };
    let jobs = session.jobs();
    let newest = jobs.iter().map(|e| e.job).max().unwrap_or(0);
    let person = app.state().export.as_ref().and_then(|d| d.section).and_then(|s| s.person()).is_some();
    host.ctx().data_mut(|d| {
        let p = d.get_temp_mut_or_default::<PersonExports>(person_exports_id());
        if let Some(after) = p.armed {
            for e in jobs.iter().filter(|e| e.kind == JobKind::Export && e.job > after) {
                if !p.jobs.contains(&(e.job, e.started_ms)) {
                    p.jobs.push((e.job, e.started_ms));
                }
            }
        }
        // Finished jobs leave the list after 10 minutes; so do their marks.
        p.jobs.retain(|k| jobs.iter().any(|e| (e.job, e.started_ms) == *k));
        p.armed = person.then_some(newest);
    });
}

/// Whether job `e` was marked as a person's export ([`observe`]).
fn person_export(host: &dyn Host, e: &JobEntry) -> bool {
    host.ctx().data(|d| d.get_temp::<PersonExports>(person_exports_id())).is_some_and(|p| p.jobs.contains(&(e.job, e.started_ms)))
}

/// Job `e` as this client may see it: a person's export without the people permission has its
/// folder withheld.
pub(super) fn reply(host: &dyn Host, config: &Config, e: &JobEntry) -> Value {
    let v = entry(e);
    if super::people::withheld(config) && person_export(host, e) { dalica_engine::automation::withhold_export_folder(v) } else { v }
}

/// Every job listed now as this client may see it ([`reply`]).
pub(super) fn list_reply(host: &dyn Host, config: &Config) -> Value {
    Value::Array(list(host).iter().map(|e| reply(host, config, e)).collect())
}

fn entry(e: &JobEntry) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Null)
}

fn no_job(job: JobId) -> Error {
    Error::new(Code::NotFound, format!("no job {job} (jobs.list lists them; finished jobs stay 10 minutes)"))
}

/// `jobs.list`: `[{job, kind, label, state, progress, cancellable, startedMs, …}]`.
pub(super) fn jobs_list(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let _: NoParams = call.params(params)?;
    Ok(Outcome::Reply(Ok(list_reply(call.host, call.control.config()))))
}

/// `jobs.wait {job, timeoutMs?}`: the job's final entry, or its running entry with `pending: true`
/// when it is still running after `timeoutMs` (default and at most 55 s).
pub(super) fn jobs_wait(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Wait = call.params(params)?;
    let ms = p.timeout_ms.unwrap_or(WAIT_DEFAULT_MS);
    if ms > MAX_WAIT_MS {
        return Err(Error::new(Code::Limit, format!("`timeoutMs` is at most {MAX_WAIT_MS}, not {ms}")));
    }
    let job = p.job;
    if call.host.app().session().and_then(|s| s.job(job)).is_none() {
        return Err(no_job(job));
    }
    let deadline = Instant::now() + Duration::from_millis(ms.min(WAIT_DEFAULT_MS));
    let config = call.control.config().clone();
    Ok(step(call.host, config, job, deadline))
}

/// The job's entry if it has finished (or the time is up); otherwise one more frame, then look
/// again.
fn step(host: &mut dyn Host, config: Config, job: JobId, deadline: Instant) -> Outcome {
    let Some(e) = host.app().session().and_then(|s| s.job(job)) else { return no_job(job).into() };
    if e.state.is_finished() {
        return Outcome::Reply(Ok(reply(host, &config, &e)));
    }
    if Instant::now() >= deadline {
        let mut v = reply(host, &config, &e);
        if let Some(o) = v.as_object_mut() {
            o.insert("pending".into(), Value::Bool(true));
        }
        return Outcome::Reply(Ok(v));
    }
    Outcome::Play(
        Input::Frames(1),
        Box::new(move |host, played| match played {
            Ok(_) => step(host, config, job, deadline),
            Err(e) => Error::from(e).into(),
        }),
    )
}

/// `jobs.cancel {job?}`: stops `job`, or every cancellable job, at its next boundary; replies
/// `{cancelled: [job…]}` once a frame has polled the session.
pub(super) fn jobs_cancel(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Cancel = call.params(params)?;
    let cancelled = match call.host.app_mut().session_mut() {
        Some(s) => s.cancel_jobs(p.job)?,
        None => match p.job {
            Some(job) => return Err(no_job(job)),
            None => Vec::new(),
        },
    };
    Ok(Outcome::play_then_reply(Input::Frames(1), json!({ "cancelled": cancelled })))
}
