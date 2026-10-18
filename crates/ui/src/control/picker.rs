//! The file-picker stub (`control-protocol.md` §5.6): automation never opens a native picker. A
//! picker requested by synthetic input waits here instead; `ui.inspect` shows it as `filePicker`
//! and `ui.filePicker` answers it.

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, DEFAULT_SETTLE_MS, Error, Next, Outcome, paths};
use crate::picker::{FolderRequest, OpenRequest, Picker, SaveRequest};

/// What a picker asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PickerKind {
    Folder,
    File,
    Save,
}

/// A picker waiting for an answer: `ui.inspect` → `filePicker`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PickerRequest {
    /// The command the answer is for.
    pub command: &'static str,
    pub kind: PickerKind,
    pub title: &'static str,
    /// A save dialog's suggested file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A save dialog's starting folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The command's other parameters (the answer adds `path`).
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
    /// A folder where new files will be made (an export location): its answer is checked as a
    /// place to write (§5.7 writing), not to read.
    #[serde(skip)]
    pub output: bool,
}

#[derive(Default)]
struct State {
    waiting: Option<PickerRequest>,
    /// The answer for the waiting picker, until it takes it.
    answer: Option<Option<PathBuf>>,
}

/// A [`Picker`] whose dialogs wait for `ui.filePicker` (called on the dialog's own thread, so the
/// wait blocks nothing else).
#[derive(Default)]
pub struct StubPicker {
    state: Mutex<State>,
    answered: Condvar,
}

impl StubPicker {
    pub fn new() -> StubPicker {
        StubPicker::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The picker waiting for an answer, if any.
    pub fn waiting(&self) -> Option<PickerRequest> {
        self.lock().waiting.clone()
    }

    /// Answers the waiting picker (`None` = cancel). False if none is waiting.
    pub fn answer(&self, answer: Option<PathBuf>) -> bool {
        let mut s = self.lock();
        if s.waiting.is_none() {
            return false;
        }
        s.waiting = None;
        s.answer = Some(answer);
        self.answered.notify_all();
        true
    }
}

impl StubPicker {
    /// Shows `request` as waiting and blocks (on the dialog's own thread) until it is answered.
    fn wait_for(&self, request: PickerRequest) -> Option<PathBuf> {
        let mut s = self.lock();
        s.answer = None;
        s.waiting = Some(request);
        loop {
            if let Some(answer) = s.answer.take() {
                return answer;
            }
            s = self.answered.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Picker for StubPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        self.wait_for(PickerRequest {
            command: "folders.add",
            kind: PickerKind::Folder,
            title: "Add Folder",
            name: None,
            folder: None,
            params: Value::Null,
            output: false,
        })
    }

    fn pick_folder_for(&self, request: &FolderRequest) -> Option<PathBuf> {
        self.wait_for(PickerRequest {
            command: request.command,
            kind: PickerKind::Folder,
            title: request.title,
            name: None,
            folder: request.folder.clone(),
            params: request.params.clone(),
            output: request.output,
        })
    }

    fn pick_save(&self, request: &SaveRequest) -> Option<PathBuf> {
        self.wait_for(PickerRequest {
            command: request.command,
            kind: PickerKind::Save,
            title: request.title,
            name: Some(request.name.clone()),
            folder: request.folder.clone(),
            params: request.params.clone(),
            output: false,
        })
    }

    fn pick_file(&self, request: &OpenRequest) -> Option<PathBuf> {
        self.wait_for(PickerRequest {
            command: request.command,
            kind: PickerKind::File,
            title: request.title,
            name: None,
            folder: None,
            params: request.params.clone(),
            output: false,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    path: Option<PathBuf>,
    cancel: Option<bool>,
}

/// `ui.filePicker {path} | {cancel: true}`: closes the stubbed picker and runs its command with the
/// path (and a folder or save request's other parameters), as the real picker would; replies the
/// command's result once the dialog has closed. A folder to add is checked as a read (§5.7
/// reading); an export location as a folder new files are made in and a file to save to as a new
/// file written (§5.7 writing: [`paths::check_output`]).
pub(super) fn answer(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let a: Answer = call.params(params)?;
    let path = match (a.path, a.cancel) {
        (Some(p), None) => Some(p),
        (None, Some(true)) => None,
        _ => return Err(call.bad("give either `path` or `cancel: true`")),
    };
    let Some(waiting) = call.control.stub().waiting() else {
        return Err(Error::new(Code::NotFound, "no file picker is open (ui.inspect → filePicker)"));
    };
    let settle = Input::Settle { timeout_ms: DEFAULT_SETTLE_MS, jobs: false };
    let Some(path) = path else {
        call.control.stub().answer(None);
        return Ok(Outcome::play_then_reply(settle, json!({})));
    };
    paths::lexical(&path, "path")?;
    let config = call.control.config().clone();
    let watched = call.host.app().session().map(|s| s.roots().to_vec()).unwrap_or_default();
    let stub = call.control.picker.clone();
    Ok(Outcome::Work(Box::new(move || {
        let checked = match waiting.kind {
            PickerKind::Save => paths::check_output(&path, &config.effective_write_roots(), &watched, paths::Output::File).map(|_| ()),
            PickerKind::Folder if waiting.output => {
                paths::check_output(&path, &config.effective_write_roots(), &watched, paths::Output::Folder).map(|_| ())
            }
            PickerKind::Folder | PickerKind::File => paths::check_read(&path, &config.read_roots, &watched, config.demo).map(|_| ()),
        };
        if let Err(e) = checked {
            return Next::Reply(Err(e));
        }
        Next::Resume(Box::new(move |host| {
            if stub.waiting().as_ref() != Some(&waiting) {
                return Error::new(Code::NotFound, "the file picker closed before the answer arrived").into();
            }
            // The dialog closes as cancelled; the command runs here, so its result is the reply.
            stub.answer(None);
            let mut params = match &waiting.params {
                Value::Object(o) => o.clone(),
                _ => serde_json::Map::new(),
            };
            params.insert("path".into(), json!(path));
            let result = host.app_mut().run(waiting.command, &Value::Object(params));
            Outcome::Play(settle, Box::new(move |_, _| Outcome::Reply(result.map_err(Error::from))))
        }))
    })))
}
