//! The file-picker stub (`control-protocol.md` §5.6): automation never opens a native picker. A
//! picker requested by synthetic input waits here instead; `ui.inspect` shows it as `filePicker`
//! and `ui.filePicker` answers it.

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::host::Input;
use super::{Call, Code, DEFAULT_SETTLE_MS, Error, Next, Outcome, paths};
use crate::picker::{Ask, FolderRequest, OpenRequest, Picker, SaveRequest};

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
    /// Blocks (on the dialog's own thread) until the waiting picker is answered. The request shows
    /// as waiting from [`Picker::opening`], before this thread starts; a dialog opened without
    /// that call shows `request` here.
    fn wait_for(&self, request: PickerRequest) -> Option<PathBuf> {
        let mut s = self.lock();
        if s.waiting.is_none() && s.answer.is_none() {
            s.waiting = Some(request);
        }
        loop {
            if let Some(answer) = s.answer.take() {
                return answer;
            }
            s = self.answered.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// What `ask` shows as `filePicker`.
fn request_for(ask: Ask<'_>) -> PickerRequest {
    match ask {
        Ask::Folder => PickerRequest {
            command: "folders.add",
            kind: PickerKind::Folder,
            title: "Add Folder",
            name: None,
            folder: None,
            params: Value::Null,
            output: false,
        },
        Ask::FolderFor(request) => PickerRequest {
            command: request.command,
            kind: PickerKind::Folder,
            title: request.title,
            name: None,
            folder: request.folder.clone(),
            params: request.params.clone(),
            output: request.output,
        },
        Ask::Save(request) => PickerRequest {
            command: request.command,
            kind: PickerKind::Save,
            title: request.title,
            name: Some(request.name.clone()),
            folder: request.folder.clone(),
            params: request.params.clone(),
            output: false,
        },
        Ask::File(request) => PickerRequest {
            command: request.command,
            kind: PickerKind::File,
            title: request.title,
            name: None,
            folder: None,
            params: request.params.clone(),
            output: false,
        },
    }
}

impl Picker for StubPicker {
    /// The picker shows as waiting at once, on the thread that opens it: an answer sent as soon as
    /// the opening command has replied finds it, however late the dialog's own thread starts.
    fn opening(&self, ask: Ask<'_>) {
        let mut s = self.lock();
        s.answer = None;
        s.waiting = Some(request_for(ask));
    }

    fn not_opened(&self) {
        let mut s = self.lock();
        s.waiting = None;
        s.answer = None;
    }

    fn pick_folder(&self) -> Option<PathBuf> {
        self.wait_for(request_for(Ask::Folder))
    }

    fn pick_folder_for(&self, request: &FolderRequest) -> Option<PathBuf> {
        self.wait_for(request_for(Ask::FolderFor(request)))
    }

    fn pick_save(&self, request: &SaveRequest) -> Option<PathBuf> {
        self.wait_for(request_for(Ask::Save(request)))
    }

    fn pick_file(&self, request: &OpenRequest) -> Option<PathBuf> {
        self.wait_for(request_for(Ask::File(request)))
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::picker::{MODEL_FILTERS, Pending};

    /// The stub behind a dialog thread that starts late (a loaded machine).
    struct Late(Arc<StubPicker>);

    impl Picker for Late {
        fn opening(&self, ask: Ask<'_>) {
            self.0.opening(ask);
        }

        fn not_opened(&self) {
            self.0.not_opened();
        }

        fn pick_folder(&self) -> Option<PathBuf> {
            std::thread::sleep(Duration::from_millis(200));
            self.0.pick_folder()
        }

        fn pick_file(&self, request: &OpenRequest) -> Option<PathBuf> {
            std::thread::sleep(Duration::from_millis(200));
            self.0.pick_file(request)
        }
    }

    fn wait(pending: &Pending) -> Option<PathBuf> {
        loop {
            if let Some(a) = pending.poll() {
                break a;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A picker shows as waiting as soon as it is opened, before its thread starts, and an answer
    /// given then reaches the dialog when it does (the M13.R review's Windows run answered too soon
    /// and was told no picker was open).
    #[test]
    fn a_picker_is_waiting_from_its_opening_and_an_early_answer_reaches_it() {
        let stub = Arc::new(StubPicker::new());
        let picker: Arc<dyn Picker> = Arc::new(Late(Arc::clone(&stub)));
        let request = OpenRequest { title: "Add a Model File", filters: MODEL_FILTERS, command: "faceModel.addFile", params: Value::Null };
        let pending = Pending::pick_file(&picker, request).unwrap();
        let waiting = stub.waiting().expect("waiting from its opening");
        assert_eq!((waiting.command, waiting.kind), ("faceModel.addFile", PickerKind::File));
        assert!(stub.answer(Some(PathBuf::from("/models/a.onnx"))));
        assert_eq!(wait(&pending), Some(PathBuf::from("/models/a.onnx")));
        assert!(stub.waiting().is_none());
        // A cancel given early is a cancel, and the next picker starts afresh.
        let pending = Pending::pick_folder(&picker).unwrap();
        assert_eq!(stub.waiting().map(|w| w.command), Some("folders.add"));
        assert!(stub.answer(None));
        assert_eq!(wait(&pending), None);
        assert!(!stub.answer(None), "nothing waits now");
    }
}
