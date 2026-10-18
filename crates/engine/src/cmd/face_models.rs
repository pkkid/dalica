//! Face models (`faces.md` §3 and §9; the state and the workers are `crate::face_models`):
//! `faces.models.list`, `faces.models.inspect {path}`, `faces.models.install {path, acknowledged}`,
//! `faces.models.download {id, acknowledged}`, `faces.models.cancel {id}`, `faces.models.remove
//! {id}` and `faces.models.select {id}`.
//!
//! - **Consent.** `acknowledged` must be `true` (the user ticked "I have read these terms and accept
//!   them for my own use"); otherwise nothing is read, requested or written and the command is
//!   `disabled`. Agents never set it on their own initiative.
//! - **Jobs.** Inspecting, installing and removing are `faces` jobs, a download a cancellable
//!   `network` job with its progress; each replies `{job, pending: true}` and its result is the
//!   job's (`jobs.wait`).
//! - **Automation.** Inspecting, installing and downloading reach outside Dalica (a file anywhere,
//!   the network): `Effect::External`, so automation runs them only with
//!   `--automation-allow-external`.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "faces.models.list",
            label: "Face Models",
            menu: "",
            keys: &[],
            params: r#"{"refresh"?:bool (read the models folder again first, e.g. after catalog.json was edited)}: the face models: {folder:path|null, problem:string|null (why nothing can be added: no folder, inside a watched folder, read-only), selected:id|null (people.model), inUse:id|null (the recogniser the scan uses: selected, else the newest installed), loaded:id|null, download:{id, job, done, total}|null, catalogErrors:[string], commercialNote, models:[{id, name, version, role:"detector"|"recogniser", origin:"builtin"|"catalog"|"local", licence:{name, commercial:"yes"|"no"|"unknown", url, notice}, commercialUse, trainedOn, sizeBytes, site, page, state:"available"|"partial"|"downloading"|"installed", partialBytes, inUse, default, caution}]}: the built-in list first, then catalog.json's, then other installed models"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "faces.models.inspect",
            label: "Inspect Face Model File",
            menu: "",
            keys: &[],
            params: r#"{"path":path (absolute, a .onnx file)}: what a local model file is, before Add a model file… installs it: a job (replies {job, pending}) whose result is {path, fileName, sizeBytes, sha256, known (by its SHA-256), refused:string|null (why it cannot be used), assumptions:[string] (stated for an unknown recogniser), model:{id, name, role, licence, trainedOn, …}|null, installed}"#,
            effect: Effect::External,
            history: History::None,
            enabled: always,
            run: inspect,
        },
        CommandSpec {
            id: "faces.models.install",
            label: "Add a Model File",
            menu: "",
            keys: &[],
            params: r#"{"path":path (absolute, a .onnx file), "acknowledged":bool (must be true: the user read the model's terms and accepted them; agents never set it on their own)}: installs a local model file into the models folder after its self-test: known by its SHA-256, else a recogniser with its assumptions stated; a detector only when known. A job (replies {job, pending}) whose result is {id, name, role, installed, already, known, assumptions, sha256, selfTest}"#,
            effect: Effect::External,
            history: History::None,
            enabled: always,
            run: install,
        },
        CommandSpec {
            id: "faces.models.download",
            label: "Download Face Model",
            menu: "",
            keys: &[],
            params: r#"{"id":string (a model of faces.models.list, built in or from catalog.json), "acknowledged":bool (must be true: the user read the model's terms and accepted them; agents never set it on their own)}: downloads the model from its pinned https address (resuming a partial download), checks its size and SHA-256 (a wrong file is deleted), self-tests it and installs it. A cancellable network job (replies {id, job, pending}; {installed, already} when it is installed already) whose result is {id, installed, already, sizeBytes, sha256, resumedFrom, fetched, selfTest}"#,
            effect: Effect::External,
            history: History::None,
            enabled: always,
            run: download,
        },
        CommandSpec {
            id: "faces.models.cancel",
            label: "Cancel Face Model Download",
            menu: "",
            keys: &[],
            params: r#"{"id":string}: cancels the download of model id; what it fetched is kept and the next download resumes from it. Replies {id, job, cancelled}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: cancel,
        },
        CommandSpec {
            id: "faces.models.remove",
            label: "Remove Face Model",
            menu: "",
            keys: &[],
            params: r#"{"id":string (an installed model)}: deletes the model's file and manifest, a partial download of it and its cached embeddings in this library; face rectangles and names stay. A job (replies {id, job, pending}) whose result is {id, removed, embeddingsRemoved}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: remove,
        },
        CommandSpec {
            id: "faces.models.select",
            label: "Use Face Model",
            menu: "",
            keys: &[],
            params: r#"{"id"?:string|null (an installed recogniser; null or left out: the newest installed)}: Use: the recogniser the face scan works with (people.model); each model keeps its own cached embeddings, never mixed. Replies {selected}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: select,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    #[serde(default)]
    refresh: bool,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: ListParams = params(p)?;
    s.list_face_models(p.refresh)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathParams {
    path: PathBuf,
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: PathParams = params(p)?;
    s.inspect_face_model(p.path)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallParams {
    path: PathBuf,
    acknowledged: bool,
}

fn install(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: InstallParams = params(p)?;
    s.install_face_model(p.path, p.acknowledged)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadParams {
    id: String,
    acknowledged: bool,
}

fn download(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: DownloadParams = params(p)?;
    s.download_face_model(&p.id, p.acknowledged)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: String,
}

fn cancel(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: IdParams = params(p)?;
    s.cancel_face_model(&p.id)
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: IdParams = params(p)?;
    s.remove_face_model(&p.id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectParams {
    id: Option<String>,
}

fn select(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: SelectParams = params(p)?;
    s.select_face_model(p.id)
}
