//! Native dialogs behind a trait (`architecture.md` §10): the app plugs in `rfd`, tests and agents a
//! [`ScriptedPicker`]. A dialog never runs on the UI thread: [`Pending`] runs it on its own thread
//! and the frame loop polls for the answer.
//!
//! Four dialogs: a folder to add, a folder for something else ([`FolderRequest`]: the Export
//! dialog's Browse…, with a title and a starting folder), a file to save to ([`SaveRequest`]:
//! File ▸ Save As…, with a suggested name, a starting folder and the JPEG and PNG filters), and a
//! file to open ([`OpenRequest`]: Options ▸ People's Add a Model File…, with the ONNX filter). A
//! folder, save or open request also names the command its answer runs and that command's other
//! parameters, so the in-app stub that stands in for the picker over automation
//! (`control-protocol.md` §5.6) can show and run it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

/// A file type a save dialog offers: its name and extensions (without the dot; the first is the
/// one a typed name without an extension gets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

/// The formats Save As writes (`data-model.md` §6): JPEG first, then PNG.
pub const IMAGE_FILTERS: &[Filter] = &[Filter { name: "JPEG", extensions: &["jpg", "jpeg"] }, Filter { name: "PNG", extensions: &["png"] }];

/// A save dialog: what it shows and what its answer runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveRequest {
    /// The dialog's title ("Save As").
    pub title: &'static str,
    /// The file name suggested ("IMG_0041.jpg").
    pub name: String,
    /// The folder it opens in, when known.
    pub folder: Option<PathBuf>,
    pub filters: &'static [Filter],
    /// The command the chosen path runs (`photo.saveTo`), with `params` plus `path`.
    pub command: &'static str,
    pub params: Value,
}

impl SaveRequest {
    /// The parameters the answer `path` runs [`SaveRequest::command`] with: `params` plus `path`.
    pub fn params_with(&self, path: &std::path::Path) -> Value {
        with_path(&self.params, path)
    }
}

/// The face model files Add a Model File… offers (`faces.md` §3.1).
pub const MODEL_FILTERS: &[Filter] = &[Filter { name: "ONNX model", extensions: &["onnx"] }];

/// An open dialog (Add a Model File…): what it shows and what its answer runs.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    /// The dialog's title ("Add a Model File").
    pub title: &'static str,
    pub filters: &'static [Filter],
    /// The command the chosen file runs (`faceModel.addFile`), with `params` plus `path`.
    pub command: &'static str,
    pub params: Value,
}

impl OpenRequest {
    /// The parameters the answer `path` runs [`OpenRequest::command`] with: `params` plus `path`.
    pub fn params_with(&self, path: &std::path::Path) -> Value {
        with_path(&self.params, path)
    }
}

/// A folder dialog that is not Add Folder (the Export dialog's Browse…): what it shows and what
/// its answer runs.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderRequest {
    /// The dialog's title ("Export Location").
    pub title: &'static str,
    /// The folder it opens in, when known.
    pub folder: Option<PathBuf>,
    /// The command the chosen folder runs (`output.export.location`), with `params` plus `path`.
    pub command: &'static str,
    pub params: Value,
    /// The folder is where new files will be made (an export location), not one to read: over
    /// automation its answer is checked as a place to write (`control-protocol.md` §5.7).
    pub output: bool,
}

impl FolderRequest {
    /// The parameters the answer `path` runs [`FolderRequest::command`] with: `params` plus `path`.
    pub fn params_with(&self, path: &std::path::Path) -> Value {
        with_path(&self.params, path)
    }
}

/// `params` (an object, else an empty one) plus `path`.
fn with_path(params: &Value, path: &std::path::Path) -> Value {
    let mut obj = match params {
        Value::Object(o) => o.clone(),
        _ => serde_json::Map::new(),
    };
    obj.insert("path".into(), Value::String(path.to_string_lossy().into_owned()));
    Value::Object(obj)
}

/// Asks the user for things the OS has dialogs for. Called off the UI thread; may block.
pub trait Picker: Send + Sync {
    /// A folder to add, or `None` if the user cancelled.
    fn pick_folder(&self) -> Option<PathBuf>;

    /// A folder for `request` (not one to add), or `None` if the user cancelled. By default the
    /// folder dialog of [`Picker::pick_folder`].
    fn pick_folder_for(&self, request: &FolderRequest) -> Option<PathBuf> {
        let _ = request;
        self.pick_folder()
    }

    /// A file to save to, or `None` if the user cancelled. A picker with no save dialog cancels.
    fn pick_save(&self, request: &SaveRequest) -> Option<PathBuf> {
        let _ = request;
        None
    }

    /// A file to open, or `None` if the user cancelled. A picker with no open dialog cancels.
    fn pick_file(&self, request: &OpenRequest) -> Option<PathBuf> {
        let _ = request;
        None
    }
}

/// Answers from a queue: tests push the folder or file the "user" picks. An empty queue is a
/// cancel. It keeps the last save request it was asked, for tests to check.
#[derive(Clone, Default)]
pub struct ScriptedPicker {
    folders: Arc<Mutex<VecDeque<PathBuf>>>,
    saves: Arc<Mutex<VecDeque<PathBuf>>>,
    files: Arc<Mutex<VecDeque<PathBuf>>>,
    asked: Arc<Mutex<Option<SaveRequest>>>,
}

impl ScriptedPicker {
    pub fn new() -> ScriptedPicker {
        ScriptedPicker::default()
    }

    /// The next `pick_folder` answers `path`.
    pub fn push_folder(&self, path: impl Into<PathBuf>) {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner).push_back(path.into());
    }

    /// The next `pick_save` answers `path`.
    pub fn push_save(&self, path: impl Into<PathBuf>) {
        self.saves.lock().unwrap_or_else(PoisonError::into_inner).push_back(path.into());
    }

    /// The next `pick_file` answers `path`.
    pub fn push_file(&self, path: impl Into<PathBuf>) {
        self.files.lock().unwrap_or_else(PoisonError::into_inner).push_back(path.into());
    }

    /// The last save dialog asked for, if any.
    pub fn last_save(&self) -> Option<SaveRequest> {
        self.asked.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Picker for ScriptedPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }

    fn pick_save(&self, request: &SaveRequest) -> Option<PathBuf> {
        *self.asked.lock().unwrap_or_else(PoisonError::into_inner) = Some(request.clone());
        self.saves.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }

    fn pick_file(&self, _request: &OpenRequest) -> Option<PathBuf> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }
}

/// A dialog running on its own thread.
pub struct Pending {
    rx: Receiver<Option<PathBuf>>,
}

impl Pending {
    /// Starts `pick_folder` on a new thread. `None` if the thread could not start.
    pub fn pick_folder(picker: &Arc<dyn Picker>) -> Option<Pending> {
        let picker = Arc::clone(picker);
        Pending::spawn(move || picker.pick_folder())
    }

    /// Starts `pick_folder_for` on a new thread. `None` if the thread could not start.
    pub fn pick_folder_for(picker: &Arc<dyn Picker>, request: FolderRequest) -> Option<Pending> {
        let picker = Arc::clone(picker);
        Pending::spawn(move || picker.pick_folder_for(&request))
    }

    /// Starts `pick_save` on a new thread. `None` if the thread could not start.
    pub fn pick_save(picker: &Arc<dyn Picker>, request: SaveRequest) -> Option<Pending> {
        let picker = Arc::clone(picker);
        Pending::spawn(move || picker.pick_save(&request))
    }

    /// Starts `pick_file` on a new thread. `None` if the thread could not start.
    pub fn pick_file(picker: &Arc<dyn Picker>, request: OpenRequest) -> Option<Pending> {
        let picker = Arc::clone(picker);
        Pending::spawn(move || picker.pick_file(&request))
    }

    fn spawn(ask: impl FnOnce() -> Option<PathBuf> + Send + 'static) -> Option<Pending> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dalica-dialog".into())
            .spawn(move || {
                let _ = tx.send(ask());
            })
            .ok()?;
        Some(Pending { rx })
    }

    /// `Some(answer)` once the dialog has closed (`Some(None)` = cancelled), `None` while open.
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        match self.rx.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(pending: &Pending) -> Option<PathBuf> {
        loop {
            if let Some(a) = pending.poll() {
                break a;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_scripted_answer_arrives_through_a_dialog_thread() {
        let scripted = ScriptedPicker::new();
        scripted.push_folder("/photos/a");
        let picker: Arc<dyn Picker> = Arc::new(scripted);
        let pending = Pending::pick_folder(&picker).unwrap();
        assert_eq!(wait(&pending), Some(PathBuf::from("/photos/a")));
        // The queue is empty now: the next dialog is a cancel.
        let pending = Pending::pick_folder(&picker).unwrap();
        assert_eq!(wait(&pending), None);
    }

    fn request() -> SaveRequest {
        SaveRequest {
            title: "Save As",
            name: "IMG_0041.jpg".into(),
            folder: Some(PathBuf::from("/photos")),
            filters: IMAGE_FILTERS,
            command: "photo.saveTo",
            params: serde_json::json!({ "id": 7 }),
        }
    }

    #[test]
    fn a_save_dialog_answers_from_its_own_queue_and_keeps_the_request() {
        let scripted = ScriptedPicker::new();
        scripted.push_save("/photos/copy.png");
        let picker: Arc<dyn Picker> = Arc::new(scripted.clone());
        let pending = Pending::pick_save(&picker, request()).unwrap();
        assert_eq!(wait(&pending), Some(PathBuf::from("/photos/copy.png")));
        assert_eq!(scripted.last_save(), Some(request()));
        // The folder queue is separate; an empty save queue is a cancel.
        scripted.push_folder("/photos/a");
        assert_eq!(wait(&Pending::pick_save(&picker, request()).unwrap()), None);
        assert_eq!(wait(&Pending::pick_folder(&picker).unwrap()), Some(PathBuf::from("/photos/a")));
        // The answer runs the command with its parameters and the path.
        let p = request().params_with(std::path::Path::new("/photos/copy.png"));
        assert_eq!(p, serde_json::json!({ "id": 7, "path": "/photos/copy.png" }));
        assert_eq!(IMAGE_FILTERS.iter().map(|f| f.name).collect::<Vec<_>>(), ["JPEG", "PNG"]);
    }

    /// A picker that has no save dialog (one written before Save As) cancels it.
    #[test]
    fn a_picker_without_a_save_dialog_cancels() {
        struct FolderOnly;
        impl Picker for FolderOnly {
            fn pick_folder(&self) -> Option<PathBuf> {
                None
            }
        }
        assert_eq!(FolderOnly.pick_save(&request()), None);
    }
}
