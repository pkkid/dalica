//! Repairing a file operation that was interrupted (`data-model.md` §8 and §10.5): every move, copy,
//! rename and delete is entered in the file-operation journal before anything moves and leaves it
//! once its results are in the library (`fileops.rs`), so an entry found when the library opens
//! means an operation stopped half-way (a crash, a power cut, the session closed while it ran).
//!
//! A stub until M9.21 fills it: then the journal is read on a worker at open and each item is
//! rolled forward when its destination is complete and verifies (finishing the trash step of a
//! cross-volume move, carrying its sidecar entry), else back, through `store`'s `intents::plan`
//! (M9.9), and the repair is reported in the log and in `library.inspect`.

use crate::session::Session;

impl Session {
    /// At session open: the interrupted file operations are repaired (M9.21). Does nothing yet.
    pub(crate) fn recover_file_ops_at_open(&mut self) {}
}
