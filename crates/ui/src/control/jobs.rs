//! `jobs.list`, `jobs.wait` and `jobs.cancel` (`control-protocol.md` §6.13) over the session's job
//! registry (`dalica_engine::jobs`). `jobs.wait` never blocks a frame: it looks at the job between
//! frames and lets one more frame run (which polls the session) until the job has finished or the
//! time is up.

use std::time::{Duration, Instant};

use dalica_engine::{JobEntry, JobId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, Error, Host, MAX_WAIT_MS, Outcome};

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

fn entry(e: &JobEntry) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Null)
}

fn no_job(job: JobId) -> Error {
    Error::new(Code::NotFound, format!("no job {job} (jobs.list lists them; finished jobs stay 10 minutes)"))
}

/// `jobs.list`: `[{job, kind, label, state, progress, cancellable, startedMs, …}]`.
pub(super) fn jobs_list(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let _: NoParams = call.params(params)?;
    Ok(Outcome::Reply(Ok(Value::Array(list(call.host).iter().map(entry).collect()))))
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
    Ok(step(call.host, job, deadline))
}

/// The job's entry if it has finished (or the time is up); otherwise one more frame, then look
/// again.
fn step(host: &mut dyn Host, job: JobId, deadline: Instant) -> Outcome {
    let Some(e) = host.app().session().and_then(|s| s.job(job)) else { return no_job(job).into() };
    if e.state.is_finished() {
        return Outcome::Reply(Ok(entry(&e)));
    }
    if Instant::now() >= deadline {
        let mut v = entry(&e);
        if let Some(o) = v.as_object_mut() {
            o.insert("pending".into(), Value::Bool(true));
        }
        return Outcome::Reply(Ok(v));
    }
    Outcome::Play(
        Input::Frames(1),
        Box::new(move |host, played| match played {
            Ok(_) => step(host, job, deadline),
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
