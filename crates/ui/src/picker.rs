//! Native dialogs behind a trait (`architecture.md` §10): the app plugs in `rfd`, tests and agents a
//! [`ScriptedPicker`]. A dialog never runs on the UI thread: [`Pending`] runs it on its own thread
//! and the frame loop polls for the answer.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

/// Asks the user for things the OS has dialogs for. Called off the UI thread; may block.
pub trait Picker: Send + Sync {
    /// A folder to add, or `None` if the user cancelled.
    fn pick_folder(&self) -> Option<PathBuf>;
}

/// Answers from a queue: tests push the folder the "user" picks. An empty queue is a cancel.
#[derive(Clone, Default)]
pub struct ScriptedPicker {
    folders: Arc<Mutex<VecDeque<PathBuf>>>,
}

impl ScriptedPicker {
    pub fn new() -> ScriptedPicker {
        ScriptedPicker::default()
    }

    /// The next `pick_folder` answers `path`.
    pub fn push_folder(&self, path: impl Into<PathBuf>) {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner).push_back(path.into());
    }
}

impl Picker for ScriptedPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }
}

/// A dialog running on its own thread.
pub struct Pending {
    rx: Receiver<Option<PathBuf>>,
}

impl Pending {
    /// Starts `pick_folder` on a new thread. `None` if the thread could not start.
    pub fn pick_folder(picker: &Arc<dyn Picker>) -> Option<Pending> {
        let (tx, rx) = mpsc::channel();
        let picker = Arc::clone(picker);
        std::thread::Builder::new()
            .name("dalica-dialog".into())
            .spawn(move || {
                let _ = tx.send(picker.pick_folder());
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

    #[test]
    fn a_scripted_answer_arrives_through_a_dialog_thread() {
        let scripted = ScriptedPicker::new();
        scripted.push_folder("/photos/a");
        let picker: Arc<dyn Picker> = Arc::new(scripted);
        let pending = Pending::pick_folder(&picker).unwrap();
        let answer = loop {
            if let Some(a) = pending.poll() {
                break a;
            }
            std::thread::yield_now();
        };
        assert_eq!(answer, Some(PathBuf::from("/photos/a")));
        // The queue is empty now: the next dialog is a cancel.
        let pending = Pending::pick_folder(&picker).unwrap();
        let answer = loop {
            if let Some(a) = pending.poll() {
                break a;
            }
            std::thread::yield_now();
        };
        assert_eq!(answer, None);
    }
}
