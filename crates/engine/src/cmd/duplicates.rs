//! Find Duplicates (`organizing.md` §8.3): `search.duplicates {}` (Tools ▸ Experimental ▸ Show
//! Duplicate Files, Ctrl+F6) starts a job (`duplicates.rs`) and replies `{job}` at once; no undo
//! step. The job reads and digests on workers, with progress and Cancel (`jobs.list`,
//! `jobs.cancel`); when it ends, the groups of identical files show in search mode `duplicates`
//! (`library.inspect` → `search`: `mode`, `groups`, `banner`), until `search.clear` or a search.
//! Greyed out while it runs.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "search.duplicates",
        label: "Show Duplicate Files",
        menu: "Tools/Experimental/Show Duplicate Files",
        keys: &[(Context::Library, "Ctrl+F6")],
        params: r#"{}: find the photos whose files are identical (same size, MD5 digest, then byte for byte) as a job on workers, with progress and Cancel; digests are cached in the index. Replies {job} at once; jobs.wait gives {groups, files, bytes, candidates, hashed, cached, compared, skipped, seconds}. When it ends the light table shows the groups in search mode "duplicates" (library.inspect's search: groups [{count, size, bytes, ids}], largest total first, and banner "12 duplicate groups (31 files)"); search.clear leaves it. No undo step"#,
        effect: Effect::Query,
        history: History::None,
        enabled: idle,
        run: duplicates,
    }]
}

fn idle(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.duplicates_job().is_some() { Err(Disabled(crate::duplicates::RUNNING.into())) } else { Ok(()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn duplicates(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    let job = s.start_duplicates()?;
    Ok(json!({ "job": job }))
}
