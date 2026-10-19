//! `jobs.list`, `jobs.wait` and `jobs.cancel` (`control-protocol.md` §6.13) over the session's job
//! registry (`dalica_engine::jobs`). `jobs.wait` never blocks a frame: it looks at the job between
//! frames and lets one more frame run (which polls the session) until the job has finished or the
//! time is up.
//!
//! **A person's export folder** (§6.4, §6.13): an export of a person's section (`export.run` with
//! `section: "person:<id>"`, from the Export dialog, a click or automation alike) makes its folder
//! under the name suggested from theirs. The session marks the job and the folder it made
//! (`dalica_engine::automation::PersonExports`); without the people permission those jobs reach the
//! client with `folder`, `name` and `written` `null`, and every job's paths into that folder (the
//! scan that brings it into the library) `null` too, in `jobs.list`, `jobs.wait` and `ui.inspect`'s
//! `jobs`. Known by the job and the folder, never by matching a folder's name against people's
//! names.

use std::time::{Duration, Instant};

use dalica_engine::{JobEntry, JobId};
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

/// Job `e` as this client may see it: without the people permission a person's export has its
/// folder withheld, and so has any job's path into a folder such an export made (the session's
/// marks: `Session::job_for_automation`).
pub(super) fn reply(host: &dyn Host, config: &Config, e: &JobEntry) -> Value {
    match host.app().session() {
        Some(s) => s.job_for_automation(e, !super::people::withheld(config)),
        None => entry(e),
    }
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
